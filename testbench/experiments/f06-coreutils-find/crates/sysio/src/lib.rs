//! sysio: o shim do experimento F06. API com a mesma forma de `std::fs`, `std::io` (handles),
//! `std::env`, `std::process`, `std::os::unix::fs` e `std::thread::spawn`, só que sobre um VFS em
//! memória e um contexto de pseudo-processo, em vez do kernel do host.
//!
//! Por que um shim e não "trocar o std::fs": o std não é substituível. Não existe `[patch]` pra ele,
//! `std::fs::File` é um tipo concreto sobre fd do host, `std::io::IsTerminal` é selado, `Path::exists()`
//! e `Path::is_dir()` são métodos inerentes que chamam `stat` do host e não podem ser sombreados por
//! trait, e `print!`/`eprintln!` escrevem no fd 1/2 do host. O porte tem que trocar cada caminho de
//! import e cada chamada desses métodos; o README do experimento mede quantas linhas isso custa.

pub mod env;
pub mod errno;
pub mod fs;
pub mod io;
pub mod proc;
pub mod process;
pub mod random;
pub mod thread;
pub mod time;
pub mod unix_fs;
pub mod users;
pub mod vfs;

/// Mesmos caminhos do std: `sysio::os::unix::fs::MetadataExt` etc.
pub mod os {
    pub mod unix {
        pub mod fs {
            pub use crate::unix_fs::*;
        }
    }

    /// No lugar de `std::os::fd`: `AsFd` vira "dá pra fazer fstat/lseek/fcntl" e `AsRawFd` devolve
    /// o número que o descritor teria (0, 1, 2 pros fluxos padrão; 3 pra arquivo aberto).
    pub mod fd {
        pub use crate::fs::Fstat as AsFd;

        pub type RawFd = i32;

        pub trait AsRawFd {
            fn as_raw_fd(&self) -> RawFd;
        }

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

        impl AsRawFd for crate::fs::File {
            fn as_raw_fd(&self) -> RawFd {
                3
            }
        }

        impl AsRawFd for crate::io::Stdin {
            fn as_raw_fd(&self) -> RawFd {
                0
            }
        }

        impl AsRawFd for crate::io::StdinLock<'_> {
            fn as_raw_fd(&self) -> RawFd {
                0
            }
        }

        impl AsRawFd for crate::io::Stdout {
            fn as_raw_fd(&self) -> RawFd {
                1
            }
        }

        impl AsRawFd for crate::io::Stderr {
            fn as_raw_fd(&self) -> RawFd {
                2
            }
        }
    }
}

/// `print!` do pseudo-processo.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => { $crate::io::print_fmt(format_args!($($arg)*)) };
}

/// `println!` do pseudo-processo.
#[macro_export]
macro_rules! println {
    () => { $crate::io::print_fmt(format_args!("\n")) };
    ($($arg:tt)*) => { $crate::io::print_fmt(format_args!("{}\n", format_args!($($arg)*))) };
}

/// `eprint!` do pseudo-processo.
#[macro_export]
macro_rules! eprint {
    ($($arg:tt)*) => { $crate::io::eprint_fmt(format_args!($($arg)*)) };
}

/// `eprintln!` do pseudo-processo.
#[macro_export]
macro_rules! eprintln {
    () => { $crate::io::eprint_fmt(format_args!("\n")) };
    ($($arg:tt)*) => { $crate::io::eprint_fmt(format_args!("{}\n", format_args!($($arg)*))) };
}
