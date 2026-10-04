// Porte pseudo-linus: substitui o crate `filetime` (utimensat/futimens do host) com a mesma
// interface que o touch usa, sobre as syscalls do pseudo-kernel. As sentinelas `UTIME_NOW` e
// `UTIME_OMIT` no campo de nanossegundos viram `SetTime::Now` e `SetTime::Omit`, como o kernel
// do Linux as interpreta.

use std::fmt;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use sysabi::{SetTime, TimeSpec};
use sysio::fs::Metadata;
use sysio::os::unix::fs::MetadataExt;

/// `UTIME_NOW` do Linux.
pub const UTIME_NOW: u32 = (1 << 30) - 1;
/// `UTIME_OMIT` do Linux.
pub const UTIME_OMIT: u32 = (1 << 30) - 2;

/// Um instante como o `struct timespec` do kernel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileTime {
    seconds: i64,
    nanos: u32,
}

impl FileTime {
    pub fn from_unix_time(seconds: i64, nanos: u32) -> Self {
        Self { seconds, nanos }
    }

    pub fn from_system_time(time: SystemTime) -> Self {
        match time.duration_since(UNIX_EPOCH) {
            Ok(d) => Self { seconds: d.as_secs() as i64, nanos: d.subsec_nanos() },
            Err(e) => {
                let d = e.duration();
                if d.subsec_nanos() == 0 {
                    Self { seconds: -(d.as_secs() as i64), nanos: 0 }
                } else {
                    Self { seconds: -(d.as_secs() as i64) - 1, nanos: 1_000_000_000 - d.subsec_nanos() }
                }
            }
        }
    }

    pub fn from_last_access_time(meta: &Metadata) -> Self {
        Self { seconds: meta.atime(), nanos: meta.atime_nsec() as u32 }
    }

    pub fn from_last_modification_time(meta: &Metadata) -> Self {
        Self { seconds: meta.mtime(), nanos: meta.mtime_nsec() as u32 }
    }

    pub fn unix_seconds(&self) -> i64 {
        self.seconds
    }

    pub fn nanoseconds(&self) -> u32 {
        self.nanos
    }

    /// O valor que o `utimensat(2)` recebe.
    pub fn to_set_time(self) -> SetTime {
        match self.nanos {
            UTIME_NOW => SetTime::Now,
            UTIME_OMIT => SetTime::Omit,
            nsec => SetTime::At(TimeSpec { sec: self.seconds, nsec }),
        }
    }
}

impl fmt::Display for FileTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{:09}s", self.seconds, self.nanos)
    }
}

/// `utimensat(AT_FDCWD, path, times, 0)`.
#[allow(dead_code)]
pub fn set_file_times<P: AsRef<Path>>(path: P, atime: FileTime, mtime: FileTime) -> sysio::io::Result<()> {
    sysio::fs::utimensat(path, atime.to_set_time(), mtime.to_set_time(), true)
}

/// `utimensat(AT_FDCWD, path, times, AT_SYMLINK_NOFOLLOW)`.
pub fn set_symlink_file_times<P: AsRef<Path>>(path: P, atime: FileTime, mtime: FileTime) -> sysio::io::Result<()> {
    sysio::fs::utimensat(path, atime.to_set_time(), mtime.to_set_time(), false)
}

/// `futimens(fd, times)`.
pub fn set_fd_times(fd: i32, atime: FileTime, mtime: FileTime) -> sysio::io::Result<()> {
    sysio::errno::cvt(sysabi::sys::current().futimens(sysabi::Fd(fd), atime.to_set_time(), mtime.to_set_time()))
}
