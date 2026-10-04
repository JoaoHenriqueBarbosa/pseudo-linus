//! E/S do jq sobre o `sysabi`, com o comportamento observável do stdio da glibc:
//!
//! - stdout com buffer de bloco (`st_blksize` limitado a `BUFSIZ`, 4096 em pipe e arquivo; linha em
//!   terminal), escrito em múltiplos exatos do tamanho do buffer, como o `_IO_new_file_xsputn`. Isso
//!   decide onde as mensagens de erro (stderr sem buffer) caem no meio da saída quando os dois vão
//!   para o mesmo lugar (`jq ... 2>&1`).
//! - stderr sem buffer: cada mensagem é um `write` só.
//! - leitura de arquivos e da entrada padrão em pedaços, como o `fgets` com buffer de 4096 do
//!   `jq_util_input`, para que entrada infinita (`yes '{}' | jq .`) funcione em fluxo.

use sysabi::{sys, Errno, Fd, OFlags};

/// `BUFSIZ` da glibc.
const BUFSIZ: usize = 8192;

/// Saída padrão com o buffer do stdio.
pub struct Stdout {
    buf: Vec<u8>,
    cap: usize,
    line_buffered: bool,
    /// Primeiro erro de escrita (o `ferror(stdout)` que o jq confere no fim).
    pub error: Option<Errno>,
}

impl Stdout {
    pub fn new() -> Stdout {
        let sys = sys::current();
        let line_buffered = sys.isatty(Fd::STDOUT);
        let cap = match sys.fstat(Fd::STDOUT) {
            Ok(st) if st.blksize > 0 && (st.blksize as usize) < BUFSIZ => st.blksize as usize,
            _ => BUFSIZ,
        };
        Stdout { buf: Vec::with_capacity(cap), cap, line_buffered, error: None }
    }

    /// `true` se a saída é um terminal (o jq liga cor e `JV_PRINT_ISATTY`).
    pub fn is_tty(&self) -> bool {
        self.line_buffered
    }

    fn write_out(&mut self, data: &[u8]) {
        if self.error.is_some() {
            return;
        }
        if let Err(e) = sys::write_all(Fd::STDOUT, data) {
            self.error = Some(e);
        }
    }

    /// `fwrite` no stdout.
    pub fn write(&mut self, mut data: &[u8]) {
        if self.line_buffered {
            self.buf.extend_from_slice(data);
            if let Some(nl) = self.buf.iter().rposition(|b| *b == b'\n') {
                let rest = self.buf.split_off(nl + 1);
                let out = std::mem::replace(&mut self.buf, rest);
                self.write_out(&out);
            }
            return;
        }
        // Enche o buffer; se ainda sobrar dado, descarrega o buffer cheio, escreve direto os
        // blocos inteiros do resto e guarda o que faltar (o `_IO_new_file_xsputn`).
        let room = self.cap - self.buf.len();
        let take = room.min(data.len());
        self.buf.extend_from_slice(&data[..take]);
        data = &data[take..];
        if data.is_empty() {
            return;
        }
        let full = std::mem::take(&mut self.buf);
        self.write_out(&full);
        let whole = data.len() - data.len() % self.cap;
        if whole > 0 {
            self.write_out(&data[..whole]);
            data = &data[whole..];
        }
        self.buf.extend_from_slice(data);
    }

    /// `fflush(stdout)`.
    pub fn flush(&mut self) {
        if !self.buf.is_empty() {
            let out = std::mem::take(&mut self.buf);
            self.write_out(&out);
        }
    }
}

impl Default for Stdout {
    fn default() -> Self {
        Stdout::new()
    }
}

/// Escreve no stderr de uma vez (stderr do C não tem buffer).
pub fn stderr(data: &[u8]) {
    let _ = sys::write_all(Fd::STDERR, data);
}

/// Texto do `strerror`.
pub fn strerror(e: Errno) -> String {
    e.message().to_string()
}

/// Lê um arquivo inteiro (`jv_load_file` e `-f`): diretório dá "It's a directory".
pub fn load_file(path: &str) -> Result<Vec<u8>, String> {
    let sys = sys::current();
    let fd = sys
        .openat(Fd::CWD, path.as_bytes(), OFlags::RDONLY | OFlags::CLOEXEC, 0)
        .map_err(strerror)?;
    let is_dir = sys.fstat(fd).map(|st| st.mode & sysabi::mode::S_IFMT == sysabi::mode::S_IFDIR);
    if is_dir.unwrap_or(false) {
        let _ = sys.close(fd);
        return Err("It's a directory".into());
    }
    let data = sys::read_to_end(fd);
    let _ = sys.close(fd);
    data.map_err(strerror)
}

/// Fonte de entrada lida em pedaços (o `FILE*` de um arquivo ou do stdin).
pub struct Source {
    fd: Fd,
    close: bool,
    pending: Vec<u8>,
    pos: usize,
    eof: bool,
    /// Erro de leitura (o `ferror`).
    pub error: Option<Errno>,
}

impl Source {
    pub fn stdin() -> Source {
        Source { fd: Fd::STDIN, close: false, pending: Vec::new(), pos: 0, eof: false, error: None }
    }

    /// `fopen(f, "r")`.
    pub fn open(path: &str) -> Result<Source, Errno> {
        let sys = sys::current();
        let fd = sys.openat(Fd::CWD, path.as_bytes(), OFlags::RDONLY | OFlags::CLOEXEC, 0)?;
        Ok(Source { fd, close: true, pending: Vec::new(), pos: 0, eof: false, error: None })
    }

    fn fill(&mut self) {
        if self.eof || self.error.is_some() {
            return;
        }
        if self.pos >= self.pending.len() {
            self.pending.clear();
            self.pos = 0;
        }
        let mut buf = vec![0u8; 65536];
        loop {
            match sys::read(self.fd, &mut buf) {
                Ok(0) => {
                    self.eof = true;
                    return;
                }
                Ok(n) => {
                    self.pending.extend_from_slice(&buf[..n]);
                    return;
                }
                Err(Errno::EINTR) => continue,
                Err(e) => {
                    self.error = Some(e);
                    return;
                }
            }
        }
    }

    /// `fgets(buf, 4096, f)`: até 4095 bytes, parando depois do primeiro `\n`. Vazio no fim.
    pub fn fgets(&mut self, out: &mut Vec<u8>) {
        out.clear();
        const LIMIT: usize = 4095;
        loop {
            let avail = &self.pending[self.pos..];
            let want = LIMIT - out.len();
            let chunk = &avail[..avail.len().min(want)];
            if let Some(i) = chunk.iter().position(|b| *b == b'\n') {
                out.extend_from_slice(&chunk[..=i]);
                self.pos += i + 1;
                return;
            }
            out.extend_from_slice(chunk);
            self.pos += chunk.len();
            if out.len() >= LIMIT {
                return;
            }
            if self.eof || self.error.is_some() {
                return;
            }
            self.fill();
            if self.pos >= self.pending.len() && (self.eof || self.error.is_some()) {
                return;
            }
        }
    }

    /// `feof`: só depois de uma leitura ter batido no fim.
    pub fn at_eof(&self) -> bool {
        self.eof && self.pos >= self.pending.len()
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        if self.close {
            let _ = sys::current().close(self.fd);
        }
    }
}
