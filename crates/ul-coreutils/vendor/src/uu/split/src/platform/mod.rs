// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

#[cfg(unix)]
// Porte pseudo-linus: E/S, FS, ambiente, processos e threads do pseudo-processo (sysio).
pub use self::unix::{FilterWriter, instantiate_current_writer, paths_refer_to_same_file};

#[cfg(windows)]
pub use self::windows::{instantiate_current_writer, paths_refer_to_same_file};

#[cfg(target_os = "wasi")]
pub use self::wasi::{instantiate_current_writer, paths_refer_to_same_file};

#[cfg(unix)]
mod unix;

#[cfg(windows)]
mod windows;

#[cfg(target_os = "wasi")]
mod wasi;

// todo: add .as_fd for sysio::io::copy's specialization for --bytes
pub enum Writer {
    File(sysio::fs::File),
    #[cfg(unix)]
    Filter(FilterWriter),
}

impl sysio::io::Write for Writer {
    fn write(&mut self, buf: &[u8]) -> sysio::io::Result<usize> {
        match self {
            Self::File(w) => w.write(buf),
            #[cfg(unix)]
            Self::Filter(w) => w.write(buf),
        }
    }

    fn flush(&mut self) -> sysio::io::Result<()> {
        match self {
            Self::File(w) => w.flush(),
            #[cfg(unix)]
            Self::Filter(w) => w.flush(),
        }
    }
}
