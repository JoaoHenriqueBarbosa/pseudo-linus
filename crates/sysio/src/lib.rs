//! sysio: fachada com a forma do std (`std::fs`, `std::io`, `std::env`, `std::process`,
//! `std::time`, `std::thread`, `std::os::unix`, `std::os::fd`) sobre as syscalls do `sysabi`.
//!
//! Serve pra portar programas Rust pro pseudo-linus trocando o caminho dos imports: nada aqui
//! toca o host, tudo vira chamada em `sysabi::sys` (o processo corrente da thread).

//!
//! Mapa dos módulos (cada um com os nomes do equivalente do std):
//!
//! | std | sysio |
//! |---|---|
//! | `std::fs` | [`fs`] (e [`path::PathExt`] pros métodos de `Path` que tocam o FS) |
//! | `std::io` (handles) | [`io`]: `stdin`, `stdout`, `stderr`, `IsTerminal`; o resto é reexportado |
//! | `std::env` | [`env`] |
//! | `std::process` | [`process`]: `exit`, `Command`, `Child`, `Stdio`, `CommandExt`, `ExitStatusExt` |
//! | `std::time` | [`time`]: `now`, `Instant`, `sleep` |
//! | `std::thread` | [`thread`]: `spawn` herdando o processo, `sleep`, `available_parallelism` |
//! | `std::os::unix::fs`, `std::os::fd` | [`os::unix::fs`], [`os::fd`] |
//! | `print!`, `eprintln!`... | as macros deste crate |
//! | `libc`/`nix` avulsos | [`unistd`], [`users`], [`random`], [`errno`] |
//!
//! Todo programa entra por [`run`], que abre o estado de userland do processo (buffer do stdout,
//! código de saída) e descarrega o stdout no fim. Detalhes no README do crate.

pub mod env;
pub mod errno;
pub mod fd;
pub mod fs;
pub mod io;
pub mod path;
pub mod proc;
pub mod process;
pub mod random;
pub mod thread;
pub mod time;
pub mod unistd;
pub mod unix_fs;
pub mod users;

pub use proc::run;
/// O contrato por baixo, pra quem precisa de uma syscall que a fachada não embrulha.
pub use sysabi;

/// Mesmos caminhos do std: `sysio::os::unix::fs::MetadataExt`, `sysio::os::fd::AsRawFd` etc.
pub mod os {
    pub mod unix {
        pub mod fs {
            pub use crate::unix_fs::*;
        }
        pub mod ffi {
            pub use std::os::unix::ffi::*;
        }
        pub mod io {
            pub use crate::fd::*;
        }
        pub mod process {
            pub use crate::process::{CommandExt, ExitStatusExt};
        }
    }
    pub mod fd {
        pub use crate::fd::*;
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
