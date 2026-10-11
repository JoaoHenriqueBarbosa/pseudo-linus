//! O `Host` do zjsc sobre o `sysabi`: tudo o que o motor lê da máquina (arquivos de `/proc` e `/etc`, ambiente,
//! diretório de trabalho, pid, entropia, sinais, stderr) vem do pseudo-processo que roda o `bun`, nunca do host real.

use std::io::Write;

use sysabi::{sys, Errno, Fd, KillTarget, Signal};
use zjsc::runtime::host::{Host, HostFile, HostStat};

fn errno(error: std::io::Error) -> i32 {
    error.raw_os_error().unwrap_or(Errno::EIO.0)
}

struct SysioFile(sysio::fs::File);

impl HostFile for SysioFile {
    fn write_all(&mut self, bytes: &[u8]) -> Result<(), i32> {
        self.0.write_all(bytes).map_err(errno)
    }
}

/// O host do pseudo-processo corrente.
#[derive(Debug, Default)]
pub struct SysabiHost;

impl Host for SysabiHost {
    fn read_file(&self, path: &str) -> Result<Vec<u8>, i32> {
        sysio::fs::read(path).map_err(errno)
    }

    fn stat(&self, path: &str) -> Result<HostStat, i32> {
        let metadata = sysio::fs::metadata(path).map_err(errno)?;
        let stat = metadata.stat();
        Ok(HostStat { is_dir: metadata.is_dir(), uid: stat.uid, gid: stat.gid })
    }

    fn create_file(&self, path: &str, exclusive: bool) -> Result<Box<dyn HostFile>, i32> {
        let mut options = sysio::fs::OpenOptions::new();
        options.write(true);
        if exclusive {
            options.create_new(true);
        } else {
            options.create(true).truncate(true);
        }
        Ok(Box::new(SysioFile(options.open(path).map_err(errno)?)))
    }

    fn environ(&self) -> Vec<(String, String)> {
        sysio::env::vars_os().map(|(key, value)| (key.to_string_lossy().into_owned(), value.to_string_lossy().into_owned())).collect()
    }

    fn cwd(&self) -> String {
        sys::current().getcwd().map_or_else(|_| String::from("/"), |path| String::from_utf8_lossy(&path).into_owned())
    }

    fn pid(&self) -> u32 {
        sys::current().getpid() as u32
    }

    fn cpus(&self) -> usize {
        sys::current().sched_getaffinity().len().max(1)
    }

    fn random_fill(&self, bytes: &mut [u8]) {
        sysio::random::fill_bytes(bytes);
    }

    fn kill(&self, pid: i32, signal: i32) -> Result<(), i32> {
        let target = if pid > 0 { KillTarget::Pid(pid) } else { KillTarget::Group(pid.saturating_abs()) };
        sys::current().kill(target, Signal(signal)).map_err(|error| error.0)
    }

    fn set_priority(&self, pid: i32, nice: i32) -> Result<(), i32> {
        sys::current().setpriority(pid, nice).map_err(|error| error.0)
    }

    fn write_stderr(&self, bytes: &[u8]) {
        let _ = sys::write_all(Fd::STDERR, bytes);
    }

    fn exit(&self, code: i32) -> ! {
        sys::exit(code)
    }
}
