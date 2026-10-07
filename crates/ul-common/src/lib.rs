//! Utilitários compartilhados pelos programas do pseudo-linus.
//!
//! Cada módulo é a versão única de algo que vários crates reimplementavam: quem precisa chama daqui,
//! sem cópia local. Nada aqui toca o host: o que precisa de sistema passa pelo `sysabi`.

pub mod codec;
pub mod ctype;
pub mod fnmatch;
pub mod fsutil;
pub mod getopt;
pub mod hash;
pub mod quote;
pub mod signal;
pub mod time;
pub mod width;
mod width_table;
