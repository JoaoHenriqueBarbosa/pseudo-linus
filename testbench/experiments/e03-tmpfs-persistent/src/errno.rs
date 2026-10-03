//! Erros com os números do Linux (x86_64), pra comparar direto com `io::Error::raw_os_error`.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Errno {
    EPERM,
    ENOENT,
    EBADF,
    EBUSY,
    EEXIST,
    ENOTDIR,
    EISDIR,
    EINVAL,
    EFBIG,
    ENAMETOOLONG,
    ENOTEMPTY,
    ESTALE,
    /// Errno que o modelo não conhece (só aparece vindo do alvo real).
    Other(i32),
}

impl Errno {
    pub fn raw(self) -> i32 {
        match self {
            Errno::EPERM => 1,
            Errno::ENOENT => 2,
            Errno::EBADF => 9,
            Errno::EBUSY => 16,
            Errno::EEXIST => 17,
            Errno::ENOTDIR => 20,
            Errno::EISDIR => 21,
            Errno::EINVAL => 22,
            Errno::EFBIG => 27,
            Errno::ENAMETOOLONG => 36,
            Errno::ENOTEMPTY => 39,
            Errno::ESTALE => 116,
            Errno::Other(n) => n,
        }
    }

    pub fn from_raw(n: i32) -> Errno {
        match n {
            1 => Errno::EPERM,
            2 => Errno::ENOENT,
            9 => Errno::EBADF,
            16 => Errno::EBUSY,
            17 => Errno::EEXIST,
            20 => Errno::ENOTDIR,
            21 => Errno::EISDIR,
            22 => Errno::EINVAL,
            27 => Errno::EFBIG,
            36 => Errno::ENAMETOOLONG,
            39 => Errno::ENOTEMPTY,
            116 => Errno::ESTALE,
            n => Errno::Other(n),
        }
    }

    pub fn from_io(e: &std::io::Error) -> Errno {
        Errno::from_raw(e.raw_os_error().unwrap_or(-1))
    }
}

impl fmt::Display for Errno {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?} ({})", self.raw())
    }
}

impl std::error::Error for Errno {}
