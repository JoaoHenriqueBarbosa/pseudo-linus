//! Descritores de arquivo do pseudo-processo, com os nomes de `std::os::fd`.
//!
//! Diferenças de propósito em relação ao std: `FromRawFd::from_raw_fd` e `BorrowedFd::borrow_raw`
//! são seguros (o pior que um fd errado faz aqui é fechar ou ler o fd errado do pseudo-processo,
//! nunca memória), e [`AsFd`] traz `fstat`, `seek_position` e `is_appending`, que é o que os portes
//! faziam com `rustix` sobre um `AsFd`.

use std::io;
use std::marker::PhantomData;

use sysabi::{Fd, OFlags, Whence};

use crate::errno::cvt;
use crate::proc;

pub type RawFd = i32;

pub trait AsRawFd {
    fn as_raw_fd(&self) -> RawFd;
}

pub trait FromRawFd {
    /// Assume a posse de `fd` (que será fechado no `Drop`).
    fn from_raw_fd(fd: RawFd) -> Self;
}

pub trait IntoRawFd {
    /// Entrega a posse do fd sem fechar.
    fn into_raw_fd(self) -> RawFd;
}

/// Um fd emprestado.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BorrowedFd<'a> {
    fd: RawFd,
    _p: PhantomData<&'a ()>,
}

impl BorrowedFd<'_> {
    pub fn borrow_raw(fd: RawFd) -> BorrowedFd<'static> {
        BorrowedFd { fd, _p: PhantomData }
    }

    /// `dup(2)` com `FD_CLOEXEC`, como o std.
    pub fn try_clone_to_owned(&self) -> io::Result<OwnedFd> {
        let sys = proc::sys();
        cvt(sys.dup_min(Fd(self.fd), Fd(3), true)).map(|fd| OwnedFd { fd: fd.0 })
    }
}

impl AsRawFd for BorrowedFd<'_> {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}

/// Um fd com dono: fecha no `Drop`.
#[derive(Debug, PartialEq, Eq)]
pub struct OwnedFd {
    fd: RawFd,
}

impl OwnedFd {
    pub fn try_clone(&self) -> io::Result<OwnedFd> {
        self.as_fd().try_clone_to_owned()
    }

    pub(crate) fn raw(&self) -> Fd {
        Fd(self.fd)
    }
}

impl Drop for OwnedFd {
    fn drop(&mut self) {
        // Erro no close é ignorado, como no std.
        if let Some(sys) = sysabi::sys::try_current() {
            let _ = sys.close(Fd(self.fd));
        }
    }
}

impl AsRawFd for OwnedFd {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}

impl FromRawFd for OwnedFd {
    fn from_raw_fd(fd: RawFd) -> Self {
        OwnedFd { fd }
    }
}

impl IntoRawFd for OwnedFd {
    fn into_raw_fd(self) -> RawFd {
        let fd = self.fd;
        std::mem::forget(self);
        fd
    }
}

/// Algo que tem um fd. Os métodos com corpo são o que os portes faziam com `rustix::fs::fstat`,
/// `lseek(fd, 0, SEEK_CUR)` e `fcntl(F_GETFL)`.
pub trait AsFd {
    fn as_fd(&self) -> BorrowedFd<'_>;

    /// `fstat(2)`.
    fn fstat(&self) -> io::Result<crate::fs::Metadata> {
        let sys = proc::sys();
        cvt(sys.fstat(Fd(self.as_fd().fd))).map(crate::fs::Metadata::from_stat)
    }

    /// `lseek(fd, 0, SEEK_CUR)`: pipe dá ESPIPE.
    fn seek_position(&self) -> io::Result<u64> {
        let sys = proc::sys();
        cvt(sys.lseek(Fd(self.as_fd().fd), 0, Whence::Cur))
    }

    /// `fcntl(fd, F_GETFL) & O_APPEND`.
    fn is_appending(&self) -> bool {
        let sys = proc::sys();
        sys.get_status_flags(Fd(self.as_fd().fd)).is_ok_and(|f| f.contains(OFlags::APPEND))
    }
}

impl<T: AsFd + ?Sized> AsFd for &T {
    fn as_fd(&self) -> BorrowedFd<'_> {
        (**self).as_fd()
    }
}

impl<T: AsFd + ?Sized> AsFd for &mut T {
    fn as_fd(&self) -> BorrowedFd<'_> {
        (**self).as_fd()
    }
}

impl AsFd for BorrowedFd<'_> {
    fn as_fd(&self) -> BorrowedFd<'_> {
        *self
    }
}

impl AsFd for OwnedFd {
    fn as_fd(&self) -> BorrowedFd<'_> {
        BorrowedFd { fd: self.fd, _p: PhantomData }
    }
}

impl crate::io::IsTerminal for BorrowedFd<'_> {
    fn is_terminal(&self) -> bool {
        crate::io::isatty(self.fd)
    }
}

impl crate::io::IsTerminal for OwnedFd {
    fn is_terminal(&self) -> bool {
        crate::io::isatty(self.fd)
    }
}

macro_rules! std_stream_fd {
    ($t:ty, $n:expr) => {
        impl AsRawFd for $t {
            fn as_raw_fd(&self) -> RawFd {
                $n
            }
        }
        impl AsFd for $t {
            fn as_fd(&self) -> BorrowedFd<'_> {
                BorrowedFd { fd: $n, _p: PhantomData }
            }
        }
    };
}

std_stream_fd!(crate::io::Stdin, 0);
std_stream_fd!(crate::io::StdinLock<'_>, 0);
std_stream_fd!(crate::io::Stdout, 1);
std_stream_fd!(crate::io::StdoutLock<'_>, 1);
std_stream_fd!(crate::io::Stderr, 2);
std_stream_fd!(crate::io::StderrLock<'_>, 2);

impl<T: AsRawFd + ?Sized> AsRawFd for &T {
    fn as_raw_fd(&self) -> RawFd {
        (**self).as_raw_fd()
    }
}

impl<T: AsRawFd + ?Sized> AsRawFd for &mut T {
    fn as_raw_fd(&self) -> RawFd {
        (**self).as_raw_fd()
    }
}
