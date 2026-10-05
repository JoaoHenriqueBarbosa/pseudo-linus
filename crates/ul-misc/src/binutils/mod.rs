//! Programas do GNU binutils 2.44 (Debian 13) que o pseudo-linus porta, um módulo por programa:
//! `ar` e `ranlib` (formato ar GNU), `size` e `nm` sobre ELF64 x86_64. O `strings` fica em
//! [`crate::strings`], de antes desta pasta.

pub mod ar;
pub mod elf;
pub mod nm;
pub mod size;
