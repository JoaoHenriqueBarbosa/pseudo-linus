//! Peças comuns dos CLIs de compressão: entrada com buffer sobre um fd, saída sobre fd, criação do
//! arquivo de saída sem sobrescrever, cópia de dono, modo e datas, e testes de terminal. Tudo sobre o
//! `sysabi`.

use std::io;

use sysabi::sys::{self, SysResult};
use sysabi::{AtFlags, Errno, Fd, Mode, OFlags, SetTime, Stat, TimeSpec};

pub use crate::sysutil::getenv;

/// Tamanho dos blocos de leitura e escrita.
pub const CHUNK: usize = 64 * 1024;

/// Entrada com buffer sobre um fd, com espiada (`ensure`) e contagem do que já foi consumido.
pub struct Input {
    pub fd: Fd,
    buf: Vec<u8>,
    start: usize,
    end: usize,
    eof: bool,
    /// Bytes já consumidos desde o começo.
    pub consumed: u64,
    /// Erro de leitura, se houve.
    pub error: Option<Errno>,
}

impl Input {
    pub fn new(fd: Fd) -> Input {
        Input { fd, buf: vec![0u8; CHUNK], start: 0, end: 0, eof: false, consumed: 0, error: None }
    }

    /// O que está no buffer.
    pub fn available(&self) -> &[u8] {
        &self.buf[self.start..self.end]
    }

    fn read_more(&mut self) -> bool {
        if self.eof {
            return false;
        }
        if self.start > 0 {
            self.buf.copy_within(self.start..self.end, 0);
            self.end -= self.start;
            self.start = 0;
        }
        if self.end == self.buf.len() {
            let grow = self.buf.len();
            if self.buf.try_reserve(grow).is_err() {
                self.error = Some(Errno::ENOMEM);
                self.eof = true;
                return false;
            }
            self.buf.resize(self.buf.len() + grow, 0);
        }
        loop {
            match sys::read(self.fd, &mut self.buf[self.end..]) {
                Ok(0) => {
                    self.eof = true;
                    return false;
                }
                Ok(n) => {
                    self.end += n;
                    return true;
                }
                Err(Errno::EINTR) => {}
                Err(e) => {
                    self.error = Some(e);
                    self.eof = true;
                    return false;
                }
            }
        }
    }

    /// Garante pelo menos `n` bytes no buffer (menos só no fim da entrada). Devolve o que há.
    pub fn ensure(&mut self, n: usize) -> &[u8] {
        while self.end - self.start < n && self.read_more() {}
        self.available()
    }

    /// Lê mais, se o buffer estiver vazio. Devolve o que há (vazio = fim).
    pub fn fill(&mut self) -> &[u8] {
        if self.start == self.end {
            self.read_more();
        }
        self.available()
    }

    pub fn consume(&mut self, n: usize) {
        let n = n.min(self.end - self.start);
        self.start += n;
        self.consumed += n as u64;
    }

    /// Fim da entrada e buffer vazio.
    pub fn at_end(&mut self) -> bool {
        self.fill().is_empty()
    }

    /// Lê e descarta o resto, devolvendo se era tudo zero e quantos bytes eram.
    pub fn drain_check_zeros(&mut self) -> (bool, u64) {
        let mut zeros = true;
        let mut count = 0u64;
        loop {
            let a = self.fill();
            if a.is_empty() {
                return (zeros, count);
            }
            zeros &= a.iter().all(|&b| b == 0);
            let n = a.len();
            count += n as u64;
            self.consume(n);
        }
    }
}

impl io::Read for Input {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let a = self.fill();
        if a.is_empty() {
            return match self.error.take() {
                Some(e) => Err(e.to_io()),
                None => Ok(0),
            };
        }
        let n = a.len().min(out.len());
        out[..n].copy_from_slice(&a[..n]);
        self.consume(n);
        Ok(n)
    }
}

/// Destino da saída: um fd (escrita direta, sem buffer extra) ou nada (`-t`).
pub struct Sink {
    pub fd: Option<Fd>,
    pub written: u64,
    pub error: Option<Errno>,
}

impl Sink {
    pub fn fd(fd: Fd) -> Sink {
        Sink { fd: Some(fd), written: 0, error: None }
    }

    pub fn null() -> Sink {
        Sink { fd: None, written: 0, error: None }
    }
}

impl io::Write for Sink {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if let Some(e) = self.error {
            return Err(e.to_io());
        }
        if let Some(fd) = self.fd
            && let Err(e) = sys::write_all(fd, data)
        {
            self.error = Some(e);
            return Err(e.to_io());
        }
        self.written += data.len() as u64;
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// `open` de leitura; `nofollow` recusa symlink no último componente (ELOOP), como o gzip faz sem -f.
pub fn open_input(path: &[u8], nofollow: bool) -> SysResult<Fd> {
    let mut flags = OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK;
    if nofollow {
        flags |= OFlags::NOFOLLOW;
    }
    let fd = sys::open(path, flags, 0)?;
    // O NONBLOCK só serve pra não travar no open de FIFO; a leitura é bloqueante.
    let cur = sys::current();
    if let Ok(fl) = cur.get_status_flags(fd) {
        let _ = cur.set_status_flags(fd, fl - OFlags::NONBLOCK);
    }
    Ok(fd)
}

/// Cria o arquivo de saída sem sobrescrever (`O_EXCL`), com modo provisório 0600.
pub fn create_exclusive(path: &[u8]) -> SysResult<Fd> {
    sys::open(path, OFlags::WRONLY | OFlags::CREAT | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW, 0o600)
}

/// Cria ou trunca o arquivo de saída.
pub fn create_truncate(path: &[u8], mode: Mode) -> SysResult<Fd> {
    sys::open(path, OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC | OFlags::CLOEXEC, mode)
}

pub fn fstat(fd: Fd) -> SysResult<Stat> {
    sys::current().fstat(fd)
}

pub use sysabi::sys::{lstat, stat};

pub fn unlink(path: &[u8]) -> SysResult<()> {
    sys::current().unlinkat(Fd::CWD, path, AtFlags::empty())
}

pub fn close(fd: Fd) {
    let _ = sys::close(fd);
}

/// Copia dono, grupo, modo (com os bits especiais) e datas pro arquivo de saída `path` (aberto em
/// `fd`), como os compressores fazem ao terminar. `mtime` permite usar a data do cabeçalho (gzip -N).
pub fn copy_attrs(fd: Fd, path: &[u8], st: &Stat, mtime: TimeSpec) {
    let cur = sys::current();
    let _ = cur.futimens(fd, SetTime::At(st.atime), SetTime::At(mtime));
    // Dono antes do modo: o chown limparia setuid e setgid.
    let _ = cur.fchownat(Fd::CWD, path, Some(st.uid), Some(st.gid), AtFlags::SYMLINK_NOFOLLOW);
    let _ = cur.fchmod(fd, st.mode & 0o7777);
}

/// Ajusta só as datas de um arquivo pelo caminho.
pub fn set_times(path: &[u8], atime: TimeSpec, mtime: TimeSpec) {
    let _ = sys::current().utimensat(Fd::CWD, path, SetTime::At(atime), SetTime::At(mtime), AtFlags::empty());
}

pub fn isatty(fd: Fd) -> bool {
    sys::current().isatty(fd)
}

/// O fd aceita `lseek` (arquivo regular).
pub fn seekable(fd: Fd) -> bool {
    sys::current().lseek(fd, 0, sysabi::Whence::Cur).is_ok()
}

/// Lista um diretório (sem `.` e `..`), na ordem do sistema de arquivos.
pub fn read_dir(path: &[u8]) -> SysResult<Vec<Vec<u8>>> {
    Ok(sys::read_dir(path)?.into_iter().map(|e| e.name).collect())
}

pub use crate::sysutil::{eprint, join, read_fd as read_all};

/// Último componente do caminho.
pub fn base_name(path: &[u8]) -> &[u8] {
    match path.iter().rposition(|&b| b == b'/') {
        Some(i) => &path[i + 1..],
        None => path,
    }
}

/// Escreve no stdout (as listagens são linha a linha, sem buffer próprio).
pub fn eprint_stdout(s: impl AsRef<[u8]>) {
    let _ = sys::write_all(Fd::STDOUT, s.as_ref());
}

/// Texto de um caminho em bytes (as mensagens das ferramentas imprimem os bytes como vieram).
pub fn show(path: &[u8]) -> String {
    String::from_utf8_lossy(path).into_owned()
}

/// Concatena pedaços de bytes.
pub fn cat(parts: &[&[u8]]) -> Vec<u8> {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(p);
    }
    v
}

/// `pread` completo de `len` bytes a partir de `off` (menos no fim do arquivo).
pub fn pread_exact(fd: Fd, off: u64, len: usize) -> SysResult<Vec<u8>> {
    let cur = sys::current();
    let mut out = vec![0u8; len];
    let mut got = 0;
    while got < len {
        match cur.pread(fd, &mut out[got..], off + got as u64) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(Errno::EINTR) => {}
            Err(e) => return Err(e),
        }
    }
    out.truncate(got);
    Ok(out)
}

/// Bits de modo.
pub const S_ISUID: Mode = 0o4000;
pub const S_ISGID: Mode = 0o2000;
pub const S_ISVTX: Mode = 0o1000;

/// Número decimal sem sinal.
pub fn parse_u64(s: &[u8]) -> Option<u64> {
    if s.is_empty() || !s.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(s).ok()?.parse().ok()
}
