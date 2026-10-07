//! E/S dos programas sobre o `sysabi`: saída com buffer no estilo do stdio do glibc, leitura de
//! arquivos e mensagens de erro no formato do `error(3)`.

use sysabi::{Errno, Fd, sys};

/// Saída com buffer como o `stdout` do glibc: em pipe e arquivo, buffer cheio (do tamanho do
/// `st_blksize`); em terminal, por linha. Erro de escrita fica guardado (o primeiro) pra quem
/// chamou decidir a mensagem (`write error`).
pub struct Out {
    fd: Fd,
    buf: Vec<u8>,
    cap: usize,
    line_buffered: bool,
    pub error: Option<Errno>,
}

impl Out {
    pub fn new(fd: Fd) -> Out {
        let sys = sys::current();
        let tty = sys.isatty(fd);
        let cap = match sys.fstat(fd) {
            Ok(st) if st.blksize > 0 => st.blksize.min(1 << 20) as usize,
            _ => 4096,
        };
        Out { fd, buf: Vec::new(), cap, line_buffered: tty, error: None }
    }

    pub fn stdout() -> Out {
        Out::new(Fd::STDOUT)
    }

    /// Força buffer por linha (`--line-buffered`, `sed -u`).
    pub fn set_line_buffered(&mut self, yes: bool) {
        self.line_buffered = yes;
    }

    pub fn write(&mut self, data: &[u8]) {
        if self.error.is_some() {
            return;
        }
        self.buf.extend_from_slice(data);
        if self.buf.len() >= self.cap || (self.line_buffered && data.contains(&b'\n')) {
            self.flush();
        }
    }

    pub fn byte(&mut self, b: u8) {
        self.write(&[b]);
    }

    pub fn flush(&mut self) {
        if self.buf.is_empty() || self.error.is_some() {
            self.buf.clear();
            return;
        }
        if let Err(e) = sys::write_all(self.fd, &self.buf) {
            self.error = Some(e);
        }
        self.buf.clear();
    }

}

impl Drop for Out {
    fn drop(&mut self) {
        self.flush();
    }
}

/// Escreve `prog: msg\n` no stderr (sem buffer, como o `error(3)`).
pub fn error(prog: &[u8], msg: &[u8]) {
    let mut line = Vec::with_capacity(prog.len() + msg.len() + 3);
    line.extend_from_slice(prog);
    line.extend_from_slice(b": ");
    line.extend_from_slice(msg);
    line.push(b'\n');
    let _ = sys::write_all(Fd::STDERR, &line);
}

/// Escreve bytes crus no stderr.
pub fn stderr(data: &[u8]) {
    let _ = sys::write_all(Fd::STDERR, data);
}

/// `prog: <arquivo>: <strerror>`.
pub fn errno_msg(name: &[u8], e: Errno) -> Vec<u8> {
    let mut v = name.to_vec();
    v.extend_from_slice(b": ");
    v.extend_from_slice(e.message().as_bytes());
    v
}

/// Lê de um fd até `buf` encher ou o fd dar EOF numa leitura (uma chamada de `read`, repetida só
/// em EINTR), como o `safe_read` do gnulib.
pub fn safe_read(fd: Fd, buf: &mut [u8]) -> Result<usize, Errno> {
    loop {
        match sys::read(fd, buf) {
            Err(Errno::EINTR) => continue,
            r => return r,
        }
    }
}

/// Lê tudo de um fd.
pub fn read_all(fd: Fd) -> Result<Vec<u8>, Errno> {
    let mut out = Vec::new();
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        let n = safe_read(fd, &mut chunk)?;
        if n == 0 {
            return Ok(out);
        }
        if out.try_reserve(n).is_err() {
            return Err(Errno::ENOMEM);
        }
        out.extend_from_slice(&chunk[..n]);
        sys::checkpoint();
    }
}

/// Junta pedaços de bytes.
pub fn cat(parts: &[&[u8]]) -> Vec<u8> {
    let mut v = Vec::with_capacity(parts.iter().map(|p| p.len()).sum());
    for p in parts {
        v.extend_from_slice(p);
    }
    v
}
