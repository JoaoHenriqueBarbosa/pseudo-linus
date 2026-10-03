//! sysabi: a "libc" do pseudo-linus.
//!
//! - [`linux`]: `Errno` e `Signal` com números do Linux e mensagens da glibc reais.
//! - [`types`]: tipos das syscalls (`Stat`, `OFlags`, `WaitStatus`, `SpawnSpec`...).
//! - [`sys`]: a trait [`Syscalls`] que o kernel implementa por processo, e as funções livres que os
//!   programas chamam sobre o processo corrente da thread.
//! - [`ctx`]: o [`Ctx`] dos builtins e adaptadores `Read`/`Write` sobre fds.
//! - [`program`]: a tabela de programas embutidos.
//! - `testkit` (feature): kernel de teste em memória pra testar programas isolados.

pub mod ctx;
pub mod linux;
pub mod program;
pub mod sys;
pub mod types;

#[cfg(feature = "testkit")]
pub mod testkit;

pub use ctx::{Ctx, FdReader, FdWriter};
pub use linux::{DefaultAction, Errno, Signal};
pub use program::{Main, Program};
pub use sys::{ExecUnwind, ExitUnwind, KillUnwind, SysResult, Syscalls};
pub use types::*;
