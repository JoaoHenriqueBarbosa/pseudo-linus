//! O `Host` do zjsc sobre o `sysabi`: tudo o que o motor lê da máquina (arquivos de `/proc` e `/etc`, ambiente,
//! diretório de trabalho, pid, entropia, sinais, stderr) vem do pseudo-processo que roda o `bun`, nunca do host real.

use std::io::Write;

use sysabi::{sys, AccessMode, AtFlags, Errno, Fd, ID_UNCHANGED, KillTarget, MsgFlags, Mode, OFlags, PollEvents, PollFd, RenameFlags, SetTime, Signal, Stat, TimeSpec};
use zjsc::runtime::host::{FsDirEntry, FsStat, FsStatFs, FsTime, Host, HostFile, HostStat, NetReady, NetWatch};

fn errno(error: std::io::Error) -> i32 {
    error.raw_os_error().unwrap_or(Errno::EIO.0)
}

struct SysioFile(sysio::fs::File);

impl HostFile for SysioFile {
    fn write_all(&mut self, bytes: &[u8]) -> Result<(), i32> {
        self.0.write_all(bytes).map_err(errno)
    }
}

thread_local! {
    /// Os atributos de cada terminal antes de ligar o modo cru, para desligá-lo devolvendo o que havia.
    static SAVED_TERMIOS: std::cell::RefCell<std::collections::HashMap<i32, sysabi::Termios>> = std::cell::RefCell::new(std::collections::HashMap::new());
}

/// O modo cru do libuv (`uv_tty_set_mode(UV_TTY_MODE_RAW)`): sem eco, sem modo canônico, sem sinais de teclado.
fn set_raw_mode(fd: Fd, raw: bool) -> Result<(), Errno> {
    use sysabi::termios::{BRKINT, CS8, ECHO, ICANON, ICRNL, IEXTEN, INPCK, ISIG, ISTRIP, IXON, ONLCR, VMIN, VTIME};
    let current = sys::current();
    if raw {
        let original = current.tcgetattr(fd)?;
        let mut cooked = original;
        cooked.c_iflag &= !(BRKINT | ICRNL | INPCK | ISTRIP | IXON);
        cooked.c_oflag |= ONLCR;
        cooked.c_cflag |= CS8;
        cooked.c_lflag &= !(ECHO | ICANON | IEXTEN | ISIG);
        cooked.c_cc[VMIN] = 1;
        cooked.c_cc[VTIME] = 0;
        current.tcsetattr(fd, sysabi::SetAttrWhen::Flush, &cooked)?;
        SAVED_TERMIOS.with(|saved| {
            saved.borrow_mut().entry(fd.0).or_insert(original);
        });
    } else if let Some(original) = SAVED_TERMIOS.with(|saved| saved.borrow_mut().remove(&fd.0)) {
        current.tcsetattr(fd, sysabi::SetAttrWhen::Flush, &original)?;
    } else {
        // Sem modo cru anterior: só confere que é um terminal.
        current.tcgetattr(fd)?;
    }
    Ok(())
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

/// Um método `fs_*` do `Host` que é uma chamada de sistema do pseudo-processo: o corpo recebe o `Syscalls` e devolve
/// o `SysResult`, e o errno sai como número.
macro_rules! syscall_method {
    ($name:ident($($argument:ident: $type:ty),*) -> $result:ty, |$syscalls:ident| $body:expr) => {
        fn $name(&self, $($argument: $type),*) -> Result<$result, i32> {
            let $syscalls = sys::current();
            $body.map_err(|error| error.0)
        }
    };
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

    syscall_method!(fs_open(path: &str, flags: u32, mode: u32) -> i32, |s| {
        s.openat(Fd::CWD, path.as_bytes(), OFlags::from_bits_retain(flags | OFlags::CLOEXEC.bits()), mode as Mode).map(|fd| fd.0)
    });
    fn tty_isatty(&self, fd: i32) -> bool {
        sys::current().isatty(Fd(fd))
    }
    syscall_method!(tty_window_size(fd: i32) -> (u16, u16), |s| s.tcgetwinsize(Fd(fd)).map(|size| (size.cols, size.rows)));
    fn tty_set_raw_mode(&self, fd: i32, raw: bool) -> Result<(), i32> {
        set_raw_mode(Fd(fd), raw).map_err(|e| e.0)
    }
    syscall_method!(fs_close(fd: i32) -> (), |s| s.close(Fd(fd)));
    syscall_method!(fs_read(fd: i32, len: usize, offset: Option<u64>) -> Vec<u8>, |s| {
        let mut buffer = vec![0u8; len];
        match offset {
            Some(offset) => s.pread(Fd(fd), &mut buffer, offset),
            None => s.read(Fd(fd), &mut buffer),
        }
        .map(|read| {
            buffer.truncate(read);
            buffer
        })
    });
    syscall_method!(fs_write(fd: i32, bytes: &[u8], offset: Option<u64>) -> usize, |s| match offset {
        Some(offset) => s.pwrite(Fd(fd), bytes, offset),
        None => s.write(Fd(fd), bytes),
    });
    syscall_method!(fs_fstat(fd: i32) -> FsStat, |s| s.fstat(Fd(fd)).map(fs_stat));
    syscall_method!(fs_stat(path: &str, follow: bool) -> FsStat, |s| {
        s.fstatat(Fd::CWD, path.as_bytes(), if follow { AtFlags::empty() } else { AtFlags::SYMLINK_NOFOLLOW }).map(fs_stat)
    });
    syscall_method!(fs_readdir(path: &str) -> Vec<FsDirEntry>, |s| {
        s.openat(Fd::CWD, path.as_bytes(), OFlags::DIRECTORY | OFlags::CLOEXEC, 0).and_then(|fd| {
            let mut entries = Vec::new();
            let outcome = loop {
                match s.getdents(fd) {
                    Ok(batch) if batch.is_empty() => break Ok(()),
                    Ok(batch) => entries.extend(
                        batch.into_iter().filter(|entry| entry.name != b"." && entry.name != b"..").map(|entry| FsDirEntry { name: entry.name, mode_type: entry.kind.mode_bits() }),
                    ),
                    Err(error) => break Err(error),
                }
            };
            let _ = s.close(fd);
            outcome.map(|()| entries)
        })
    });
    syscall_method!(fs_mkdir(path: &str, mode: u32) -> (), |s| s.mkdirat(Fd::CWD, path.as_bytes(), mode as Mode));
    syscall_method!(fs_unlink(path: &str) -> (), |s| s.unlinkat(Fd::CWD, path.as_bytes(), AtFlags::empty()));
    syscall_method!(fs_rmdir(path: &str) -> (), |s| s.unlinkat(Fd::CWD, path.as_bytes(), AtFlags::REMOVEDIR));
    syscall_method!(fs_rename(from: &str, to: &str) -> (), |s| {
        s.renameat2(Fd::CWD, from.as_bytes(), Fd::CWD, to.as_bytes(), RenameFlags::empty())
    });
    syscall_method!(fs_link(from: &str, to: &str) -> (), |s| s.linkat(Fd::CWD, from.as_bytes(), Fd::CWD, to.as_bytes(), AtFlags::empty()));
    syscall_method!(fs_symlink(target: &str, path: &str) -> (), |s| s.symlinkat(target.as_bytes(), Fd::CWD, path.as_bytes()));
    syscall_method!(fs_readlink(path: &str) -> String, |s| {
        s.readlinkat(Fd::CWD, path.as_bytes()).map(|target| String::from_utf8_lossy(&target).into_owned())
    });
    syscall_method!(fs_chmod(path: &str, mode: u32) -> (), |s| s.fchmodat(Fd::CWD, path.as_bytes(), mode as Mode, AtFlags::empty()));
    syscall_method!(fs_fchmod(fd: i32, mode: u32) -> (), |s| s.fchmod(Fd(fd), mode as Mode));
    syscall_method!(fs_utimes(path: &str, atime: FsTime, mtime: FsTime, follow: bool) -> (), |s| {
        let flags = if follow { AtFlags::empty() } else { AtFlags::SYMLINK_NOFOLLOW };
        s.utimensat(Fd::CWD, path.as_bytes(), set_time(atime), set_time(mtime), flags)
    });
    syscall_method!(fs_futimes(fd: i32, atime: FsTime, mtime: FsTime) -> (), |s| s.futimens(Fd(fd), set_time(atime), set_time(mtime)));
    syscall_method!(fs_ftruncate(fd: i32, len: u64) -> (), |s| s.ftruncate(Fd(fd), len));
    syscall_method!(fs_fsync(fd: i32) -> (), |s| s.fsync(Fd(fd)));
    syscall_method!(fs_access(path: &str, mode: u32) -> (), |s| {
        s.faccessat(Fd::CWD, path.as_bytes(), AccessMode::from_bits_truncate(mode), AtFlags::empty())
    });

    syscall_method!(fs_chown(path: &str, uid: u32, gid: u32, follow: bool) -> (), |s| {
        let flags = if follow { AtFlags::empty() } else { AtFlags::SYMLINK_NOFOLLOW };
        s.fchownat(Fd::CWD, path.as_bytes(), (uid != ID_UNCHANGED).then_some(uid), (gid != ID_UNCHANGED).then_some(gid), flags)
    });
    syscall_method!(fs_fchown(fd: i32, uid: u32, gid: u32) -> (), |s| {
        s.fchownat(Fd(fd), b"", (uid != ID_UNCHANGED).then_some(uid), (gid != ID_UNCHANGED).then_some(gid), AtFlags::EMPTY_PATH)
    });
    syscall_method!(fs_statfs(path: &str) -> FsStatFs, |s| {
        s.statfs(path.as_bytes()).map(|stat| FsStatFs {
            fs_type: stat.fs_type,
            bsize: stat.bsize,
            frsize: stat.frsize,
            blocks: stat.blocks,
            bfree: stat.bfree,
            bavail: stat.bavail,
            files: stat.files,
            ffree: stat.ffree,
        })
    });

    syscall_method!(net_listen(port: u16, backlog: u32) -> (i32, u16), |s| {
        s.tcp_listen_at(std::net::Ipv4Addr::LOCALHOST.into(), port, backlog, true, true).map(|(fd, port)| (fd.0, port))
    });
    syscall_method!(net_accept(fd: i32) -> (i32, u16), |s| s.tcp_accept(Fd(fd), true, true).map(|(fd, port)| (fd.0, port)));
    syscall_method!(net_connect(port: u16) -> (i32, u16), |s| s.tcp_connect(port, true, true).map(|(fd, port)| (fd.0, port)));
    syscall_method!(net_recv(fd: i32, max: usize) -> Vec<u8>, |s| s.sock_recv(Fd(fd), max, MsgFlags::DONTWAIT));
    syscall_method!(net_send(fd: i32, bytes: &[u8]) -> usize, |s| s.sock_send(Fd(fd), bytes, MsgFlags::DONTWAIT | MsgFlags::NOSIGNAL));
    syscall_method!(net_shutdown(fd: i32, read: bool, write: bool) -> (), |s| s.tcp_shutdown(Fd(fd), read, write));

    fn net_poll(&self, watch: &[NetWatch], timeout: Option<std::time::Duration>) -> Result<Vec<NetReady>, i32> {
        let mut fds: Vec<PollFd> = watch
            .iter()
            .map(|entry| {
                let mut events = PollEvents::empty();
                events.set(PollEvents::IN, entry.read);
                events.set(PollEvents::OUT, entry.write);
                PollFd { fd: Fd(entry.fd), events, revents: PollEvents::empty() }
            })
            .collect();
        sys::current().poll(&mut fds, timeout).map_err(|error| error.0)?;
        Ok(fds
            .iter()
            .map(|entry| NetReady {
                readable: entry.revents.contains(PollEvents::IN),
                writable: entry.revents.contains(PollEvents::OUT),
                hangup: entry.revents.contains(PollEvents::HUP),
                error: entry.revents.intersects(PollEvents::ERR | PollEvents::NVAL),
            })
            .collect())
    }

    fn fs_realpath(&self, path: &str) -> Result<String, i32> {
        sysio::fs::canonicalize(path).map(|resolved| resolved.to_string_lossy().into_owned()).map_err(errno)
    }
}
