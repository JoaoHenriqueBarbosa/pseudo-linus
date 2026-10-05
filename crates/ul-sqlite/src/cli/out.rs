//! Fluxos de saída com o buffer do stdio da glibc.
//!
//! O sqlite3 escreve resultados no stdout com `fputs`/`printf` (buffer cheio quando não é terminal,
//! por linha quando é) e erros no stderr sem buffer. A ordem em que os dois aparecem num `2>&1`
//! depende disso: um erro no meio de um comando sai antes das linhas ainda no buffer. Este módulo
//! reproduz o buffer do `_IO_new_file_xsputn` (enche, esvazia, escreve direto os blocos inteiros,
//! guarda o resto).

use sysabi::{Errno, Fd, Pid, SysResult, Syscalls, WaitOptions, WaitTarget, sys};

/// Tamanho do buffer do stdio (`st_blksize` dos pipes e do tmpfs).
pub const BUFSIZ: usize = 4096;

/// Para onde a saída vai.
#[derive(Debug)]
pub enum Sink {
    /// fd herdado (stdout ou stderr): não fecha.
    Inherited(Fd),
    /// Arquivo aberto pelo `.output`/`.once`: fecha no reset.
    File(Fd),
    /// Pipe pra um comando (`.output |cmd`): fecha e espera o filho no reset.
    Pipe { fd: Fd, child: Pid },
    /// `.output off`.
    Off,
}

/// Um `FILE*` de escrita.
#[derive(Debug)]
pub struct Stream {
    pub sink: Sink,
    buf: Vec<u8>,
    line_buffered: bool,
    unbuffered: bool,
    /// Escrita falhou (EPIPE etc.): o resto é descartado, como o stdio com erro no fluxo.
    pub error: bool,
}

fn write_all(s: &dyn Syscalls, fd: Fd, mut data: &[u8]) -> SysResult<()> {
    while !data.is_empty() {
        match s.write(fd, data) {
            Ok(0) => return Err(Errno::EIO),
            Ok(n) => data = &data[n..],
            Err(Errno::EINTR) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

impl Stream {
    pub fn new(sink: Sink) -> Stream {
        let (line_buffered, unbuffered) = match &sink {
            Sink::Inherited(fd) => {
                let tty = sys::current().isatty(*fd);
                (tty, *fd == Fd::STDERR)
            }
            _ => (false, false),
        };
        Stream { sink, buf: Vec::new(), line_buffered, unbuffered, error: false }
    }

    pub fn fd(&self) -> Option<Fd> {
        match &self.sink {
            Sink::Inherited(fd) | Sink::File(fd) => Some(*fd),
            Sink::Pipe { fd, .. } => Some(*fd),
            Sink::Off => None,
        }
    }

    fn raw_write(&mut self, data: &[u8]) {
        if self.error || data.is_empty() {
            return;
        }
        let Some(fd) = self.fd() else { return };
        if write_all(sys::current().as_ref(), fd, data).is_err() {
            self.error = true;
        }
    }

    /// `fputs`/`fwrite`.
    pub fn put(&mut self, data: &[u8]) {
        if matches!(self.sink, Sink::Off) || data.is_empty() {
            return;
        }
        if self.unbuffered {
            self.raw_write(data);
            return;
        }
        if self.line_buffered {
            self.buf.extend_from_slice(data);
            if let Some(p) = self.buf.iter().rposition(|&b| b == b'\n') {
                let chunk: Vec<u8> = self.buf.drain(..=p).collect();
                self.raw_write(&chunk);
            }
            if self.buf.len() >= BUFSIZ {
                self.flush();
            }
            return;
        }
        let space = BUFSIZ - self.buf.len();
        if data.len() <= space {
            self.buf.extend_from_slice(data);
            return;
        }
        let (head, rest) = data.split_at(space);
        self.buf.extend_from_slice(head);
        self.flush();
        let direct = rest.len() - rest.len() % BUFSIZ;
        if direct > 0 {
            self.raw_write(&rest[..direct]);
        }
        self.buf.extend_from_slice(&rest[direct..]);
    }

    pub fn flush(&mut self) {
        if self.buf.is_empty() {
            return;
        }
        let b = std::mem::take(&mut self.buf);
        self.raw_write(&b);
    }

    /// `fclose`/`pclose`: esvazia, fecha o que for nosso e espera o filho do pipe.
    pub fn close(&mut self) {
        self.flush();
        let s = sys::current();
        match std::mem::replace(&mut self.sink, Sink::Off) {
            Sink::File(fd) => {
                let _ = s.close(fd);
            }
            Sink::Pipe { fd, child } => {
                let _ = s.close(fd);
                while let Err(Errno::EINTR) = s.wait4(WaitTarget::Pid(child), WaitOptions::empty()) {}
            }
            other => self.sink = other,
        }
    }
}
