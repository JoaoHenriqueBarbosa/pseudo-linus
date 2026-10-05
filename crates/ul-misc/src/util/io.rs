//! E/S dos programas sobre `sysabi`: arquivo com fechamento automático, saída com buffer e leitura
//! inteira de arquivo ou stdin. Nada aqui toca o host.

use std::ffi::OsString;
use std::io::{self, BufWriter, Read, Write};
use std::os::unix::ffi::OsStrExt;

use sysabi::{Errno, Fd, FdWriter, Mode, OFlags, SysResult, sys};

/// Um fd aberto que se fecha no `Drop`.
#[derive(Debug)]
pub struct File {
    fd: Fd,
    owned: bool,
}

impl File {
    pub fn open(path: &[u8]) -> SysResult<File> {
        let fd = sys::open(path, OFlags::RDONLY | OFlags::CLOEXEC, 0)?;
        Ok(File { fd, owned: true })
    }

    /// Abre com flags e modo arbitrários (`O_WRONLY | O_CREAT | O_TRUNC`...).
    pub fn open_with(path: &[u8], flags: OFlags, mode: Mode) -> SysResult<File> {
        let fd = sys::open(path, flags | OFlags::CLOEXEC, mode)?;
        Ok(File { fd, owned: true })
    }

    /// O stdin do processo, sem fechar no fim.
    pub fn stdin() -> File {
        File {
            fd: Fd::STDIN,
            owned: false,
        }
    }

    pub fn fd(&self) -> Fd {
        self.fd
    }

    /// Lê até `buf` encher ou o arquivo acabar (repete leituras curtas, como `fread`).
    pub fn read_full(&mut self, buf: &mut [u8]) -> SysResult<usize> {
        let mut n = 0;
        while n < buf.len() {
            match sys::read(self.fd, &mut buf[n..]) {
                Ok(0) => break,
                Ok(k) => n += k,
                Err(Errno::EINTR) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(n)
    }

    pub fn read_to_end_sys(&mut self) -> SysResult<Vec<u8>> {
        let mut out = Vec::new();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match sys::read(self.fd, &mut buf) {
                Ok(0) => return Ok(out),
                Ok(n) => {
                    out.try_reserve(n).map_err(|_| Errno::ENOMEM)?;
                    out.extend_from_slice(&buf[..n]);
                }
                Err(Errno::EINTR) => {}
                Err(e) => return Err(e),
            }
        }
    }
}

impl Drop for File {
    fn drop(&mut self) {
        if self.owned {
            let _ = sys::close(self.fd);
        }
    }
}

impl Read for File {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            match sys::read(self.fd, buf) {
                Err(Errno::EINTR) => continue,
                r => return r.map_err(Errno::to_io),
            }
        }
    }
}

impl Write for File {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        loop {
            match sys::write(self.fd, buf) {
                Err(Errno::EINTR) => continue,
                r => return r.map_err(Errno::to_io),
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Lê um arquivo inteiro; `-` não é tratado aqui (cada programa decide se `-` é stdin).
pub fn read_path(path: &[u8]) -> SysResult<Vec<u8>> {
    File::open(path)?.read_to_end_sys()
}

/// Lê o stdin inteiro.
pub fn read_stdin() -> SysResult<Vec<u8>> {
    File::stdin().read_to_end_sys()
}

/// Saída padrão do processo com a bufferização do stdio da glibc (por linha em terminal, bloco de
/// 4096 bytes em pipe ou arquivo), via `sysio`. O programa tem que rodar dentro de [`run`], que
/// descarrega no fim (e no `exit`) e avisa `write error` como o `close_stdout` do gnulib.
pub fn stdout() -> sysio::io::Stdout {
    sysio::io::stdout()
}

/// Descarrega o stdout agora (antes de escrever direto num fd, de dormir no `watch`...).
pub fn flush_stdout() -> io::Result<()> {
    sysio::io::flush_stdout()
}

/// Escritor direto no fd 1, sem buffer nenhum, pra programa que no original escreve com `write(2)`.
pub fn raw_stdout() -> BufWriter<FdWriter> {
    BufWriter::with_capacity(64 * 1024, FdWriter(Fd::STDOUT))
}

/// Escreve no stderr sem buffer, numa escrita só (como `fprintf(stderr, ...)` da glibc).
pub fn eprint(s: impl AsRef<[u8]>) {
    let _ = sys::write_all(Fd::STDERR, s.as_ref());
}

/// Entrada de todo `main` do crate: abre o estado de userland do processo (buffer do stdout da glibc)
/// e descarrega no fim. É o `sysio::run`.
pub fn run(main: impl FnOnce() -> i32) -> i32 {
    sysio::run(main)
}

/// O argv do `main` em bytes.
pub fn args_bytes(args: &[OsString]) -> Vec<Vec<u8>> {
    args.iter().map(|a| a.as_bytes().to_vec()).collect()
}

/// `argv[0]` como texto (pras mensagens do getopt, que usam o argv[0] inteiro).
pub fn argv0(args: &[OsString]) -> String {
    args.first()
        .map(|a| String::from_utf8_lossy(a.as_bytes()).into_owned())
        .unwrap_or_default()
}

/// Bytes como texto pra mensagens (UTF-8 inválido vira U+FFFD, como faria um terminal).
pub fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// Converte erro de escrita do `std::io` de volta pra errno.
pub fn io_errno(e: &io::Error) -> Errno {
    Errno::from_io(e)
}

/// `true` se o stdout é terminal.
pub fn stdout_is_tty() -> bool {
    sys::try_current().is_some_and(|s| s.isatty(Fd::STDOUT))
}
