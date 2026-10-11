//! O `Host` do zjsc sobre o `sysabi`: tudo o que o motor lê da máquina (arquivos de `/proc` e `/etc`, ambiente,
//! diretório de trabalho, pid, entropia, sinais, stderr) vem do pseudo-processo que roda o `bun`, nunca do host real.

use std::io::Write;

use sysabi::{sys, AccessMode, AtFlags, Errno, Fd, KillTarget, Mode, OFlags, RenameFlags, SetTime, Signal, Stat, TimeSpec};
use zjsc::runtime::host::{FsDirEntry, FsStat, FsTime, Host, HostFile, HostStat};

fn errno(error: std::io::Error) -> i32 {
    error.raw_os_error().unwrap_or(Errno::EIO.0)
}

struct SysioFile(sysio::fs::File);

impl HostFile for SysioFile {
    fn write_all(&mut self, bytes: &[u8]) -> Result<(), i32> {
        self.0.write_all(bytes).map_err(errno)
    }
}

fn fs_stat(stat: Stat) -> FsStat {
    let time = |t: TimeSpec| (t.sec, i64::from(t.nsec));
    FsStat {
        dev: stat.dev,
        ino: stat.ino,
        mode: stat.mode,
        nlink: stat.nlink,
        uid: stat.uid,
        gid: stat.gid,
        rdev: stat.rdev,
        size: stat.size,
        blksize: stat.blksize,
        blocks: stat.blocks,
        atime: time(stat.atime),
        mtime: time(stat.mtime),
        ctime: time(stat.ctime),
        birthtime: time(stat.btime.unwrap_or(stat.ctime)),
    }
}

fn set_time(time: FsTime) -> SetTime {
    match time {
        FsTime::Now => SetTime::Now,
        FsTime::Omit => SetTime::Omit,
        FsTime::At(sec, nsec) => SetTime::At(TimeSpec { sec, nsec: nsec as u32 }),
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

    fn fs_open(&self, path: &str, flags: u32, mode: u32) -> Result<i32, i32> {
        let flags = OFlags::from_bits_retain(flags | OFlags::CLOEXEC.bits());
        sys::current().openat(Fd::CWD, path.as_bytes(), flags, mode as Mode).map(|fd| fd.0).map_err(|error| error.0)
    }

    fn fs_close(&self, fd: i32) -> Result<(), i32> {
        sys::current().close(Fd(fd)).map_err(|error| error.0)
    }

    fn fs_read(&self, fd: i32, len: usize, offset: Option<u64>) -> Result<Vec<u8>, i32> {
        let mut buffer = vec![0u8; len];
        let read = match offset {
            Some(offset) => sys::current().pread(Fd(fd), &mut buffer, offset),
            None => sys::current().read(Fd(fd), &mut buffer),
        }
        .map_err(|error| error.0)?;
        buffer.truncate(read);
        Ok(buffer)
    }

    fn fs_write(&self, fd: i32, bytes: &[u8], offset: Option<u64>) -> Result<usize, i32> {
        match offset {
            Some(offset) => sys::current().pwrite(Fd(fd), bytes, offset),
            None => sys::current().write(Fd(fd), bytes),
        }
        .map_err(|error| error.0)
    }

    fn fs_fstat(&self, fd: i32) -> Result<FsStat, i32> {
        sys::current().fstat(Fd(fd)).map(fs_stat).map_err(|error| error.0)
    }

    fn fs_stat(&self, path: &str, follow: bool) -> Result<FsStat, i32> {
        let flags = if follow { AtFlags::empty() } else { AtFlags::SYMLINK_NOFOLLOW };
        sys::current().fstatat(Fd::CWD, path.as_bytes(), flags).map(fs_stat).map_err(|error| error.0)
    }

    fn fs_readdir(&self, path: &str) -> Result<Vec<FsDirEntry>, i32> {
        let syscalls = sys::current();
        let fd = syscalls.openat(Fd::CWD, path.as_bytes(), OFlags::DIRECTORY | OFlags::CLOEXEC, 0).map_err(|error| error.0)?;
        let mut entries = Vec::new();
        let outcome = loop {
            match syscalls.getdents(fd) {
                Ok(batch) if batch.is_empty() => break Ok(()),
                Ok(batch) => entries.extend(
                    batch.into_iter().filter(|entry| entry.name != b"." && entry.name != b"..").map(|entry| FsDirEntry { name: entry.name, mode_type: entry.kind.mode_bits() }),
                ),
                Err(error) => break Err(error.0),
            }
        };
        let _ = syscalls.close(fd);
        outcome.map(|()| entries)
    }

    fn fs_mkdir(&self, path: &str, mode: u32) -> Result<(), i32> {
        sys::current().mkdirat(Fd::CWD, path.as_bytes(), mode as Mode).map_err(|error| error.0)
    }

    fn fs_unlink(&self, path: &str) -> Result<(), i32> {
        sys::current().unlinkat(Fd::CWD, path.as_bytes(), AtFlags::empty()).map_err(|error| error.0)
    }

    fn fs_rmdir(&self, path: &str) -> Result<(), i32> {
        sys::current().unlinkat(Fd::CWD, path.as_bytes(), AtFlags::REMOVEDIR).map_err(|error| error.0)
    }

    fn fs_rename(&self, from: &str, to: &str) -> Result<(), i32> {
        sys::current().renameat2(Fd::CWD, from.as_bytes(), Fd::CWD, to.as_bytes(), RenameFlags::empty()).map_err(|error| error.0)
    }

    fn fs_link(&self, from: &str, to: &str) -> Result<(), i32> {
        sys::current().linkat(Fd::CWD, from.as_bytes(), Fd::CWD, to.as_bytes(), AtFlags::empty()).map_err(|error| error.0)
    }

    fn fs_symlink(&self, target: &str, path: &str) -> Result<(), i32> {
        sys::current().symlinkat(target.as_bytes(), Fd::CWD, path.as_bytes()).map_err(|error| error.0)
    }

    fn fs_readlink(&self, path: &str) -> Result<Vec<u8>, i32> {
        sys::current().readlinkat(Fd::CWD, path.as_bytes()).map_err(|error| error.0)
    }

    fn fs_chmod(&self, path: &str, mode: u32) -> Result<(), i32> {
        sys::current().fchmodat(Fd::CWD, path.as_bytes(), mode as Mode, AtFlags::empty()).map_err(|error| error.0)
    }

    fn fs_fchmod(&self, fd: i32, mode: u32) -> Result<(), i32> {
        sys::current().fchmod(Fd(fd), mode as Mode).map_err(|error| error.0)
    }

    fn fs_utimes(&self, path: &str, atime: FsTime, mtime: FsTime, follow: bool) -> Result<(), i32> {
        let flags = if follow { AtFlags::empty() } else { AtFlags::SYMLINK_NOFOLLOW };
        sys::current().utimensat(Fd::CWD, path.as_bytes(), set_time(atime), set_time(mtime), flags).map_err(|error| error.0)
    }

    fn fs_futimes(&self, fd: i32, atime: FsTime, mtime: FsTime) -> Result<(), i32> {
        sys::current().futimens(Fd(fd), set_time(atime), set_time(mtime)).map_err(|error| error.0)
    }

    fn fs_ftruncate(&self, fd: i32, len: u64) -> Result<(), i32> {
        sys::current().ftruncate(Fd(fd), len).map_err(|error| error.0)
    }

    fn fs_fsync(&self, fd: i32) -> Result<(), i32> {
        sys::current().fsync(Fd(fd)).map_err(|error| error.0)
    }

    fn fs_access(&self, path: &str, mode: u32) -> Result<(), i32> {
        sys::current().faccessat(Fd::CWD, path.as_bytes(), AccessMode::from_bits_truncate(mode), AtFlags::empty()).map_err(|error| error.0)
    }

    fn fs_realpath(&self, path: &str) -> Result<String, i32> {
        sysio::fs::canonicalize(path).map(|resolved| resolved.to_string_lossy().into_owned()).map_err(errno)
    }
}
