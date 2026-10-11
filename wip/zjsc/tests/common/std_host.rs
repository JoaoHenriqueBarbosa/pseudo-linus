//! O `Host` dos testes: lê a máquina real (o `/proc`, o ambiente e o diretório de trabalho do processo de teste).
//! SÓ os testes o usam, para conferir o que o motor devolve contra o bun rodando nesta mesma máquina. O binário final
//! não o tem: dentro do pseudo-linus o `ul-bun` instala o host do `sysabi`, e o motor nunca olha o sistema real. Este
//! é o único arquivo do zjsc que usa `std::fs`, `std::env` e `std::process::Command`, e por isso fica em `tests/` e
//! não em `src/` (o `tests/host_boundary.rs` varre o `src/` sem exceção).
//!
//! Incluído pelos testes de integração (`mod common`) e, por `#[path]`, pelos testes de unidade do `src/`.

use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt;
use std::process::Command;
use std::rc::Rc;

use zjsc::runtime::host::{self, Errno, Host, HostFile, HostStat};

fn errno(error: std::io::Error) -> Errno {
    error.raw_os_error().unwrap_or(5)
}

struct StdFile(std::fs::File);

impl HostFile for StdFile {
    fn write_all(&mut self, bytes: &[u8]) -> Result<(), Errno> {
        self.0.write_all(bytes).and_then(|()| self.0.flush()).map_err(errno)
    }
}

/// A máquina real, como o motor a via antes de existir a fronteira.
#[derive(Debug, Default)]
pub struct StdHost;

impl Host for StdHost {
    fn read_file(&self, path: &str) -> Result<Vec<u8>, Errno> {
        std::fs::read(path).map_err(errno)
    }

    fn stat(&self, path: &str) -> Result<HostStat, Errno> {
        let metadata = std::fs::metadata(path).map_err(errno)?;
        Ok(HostStat { is_dir: metadata.is_dir(), uid: metadata.uid(), gid: metadata.gid() })
    }

    fn create_file(&self, path: &str, exclusive: bool) -> Result<Box<dyn HostFile>, Errno> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true);
        if exclusive {
            options.create_new(true);
        } else {
            options.create(true).truncate(true);
        }
        Ok(Box::new(StdFile(options.open(path).map_err(errno)?)))
    }

    fn environ(&self) -> Vec<(String, String)> {
        std::env::vars_os().map(|(key, value)| (key.to_string_lossy().into_owned(), value.to_string_lossy().into_owned())).collect()
    }

    fn cwd(&self) -> String {
        std::env::current_dir().map_or_else(|_| String::from("/"), |path| path.to_string_lossy().into_owned())
    }

    fn pid(&self) -> u32 {
        std::process::id()
    }

    fn cpus(&self) -> usize {
        std::thread::available_parallelism().map_or(1, |count| count.get())
    }

    fn random_fill(&self, bytes: &mut [u8]) {
        std::fs::File::open("/dev/urandom").and_then(|mut source| source.read_exact(bytes)).expect("/dev/urandom existe no Debian");
    }

    fn kill(&self, pid: i32, signal: i32) -> Result<(), Errno> {
        let output = Command::new("kill").arg("-s").arg(signal.to_string()).arg("--").arg(pid.to_string()).output().map_err(errno)?;
        if output.status.success() { Ok(()) } else { Err(1) }
    }

    fn set_priority(&self, pid: i32, nice: i32) -> Result<(), Errno> {
        let output = Command::new("renice").arg("-n").arg(nice.to_string()).arg("-p").arg(pid.to_string()).output().map_err(errno)?;
        if output.status.success() { Ok(()) } else { Err(13) }
    }

    fn write_stderr(&self, bytes: &[u8]) {
        let _ = std::io::stderr().write_all(bytes);
    }

    fn exit(&self, code: i32) -> ! {
        std::process::exit(code)
    }
}

/// Instala o `StdHost` na thread de teste.
pub fn install() {
    host::install(Rc::new(StdHost));
}
