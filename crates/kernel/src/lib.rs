//! Kernel do pseudo-linus: processos, threads, fds, pipes, sinais, wait, spawn, exec e a imagem do
//! sandbox, sobre o VFS (`crates/vfs`) e, a partir do marco 2, o EEVDF (`crates/sched`).
//!
//! Modelo de execução A do design: cada thread de pseudo-processo é uma thread do SO com a implementação
//! de [`sysabi::Syscalls`] instalada (`sysabi::sys::install`). Sinais são entregues nos pontos de
//! checagem (toda syscall e todo `checkpoint`); morte por sinal e `exit` desenrolam a pilha com
//! `KillUnwind` e `ExitUnwind`, então os `Drop` rodam e os fds fecham.
//!
//! A API pública (host e conformidade) está documentada em `crates/kernel/API.md`.

mod cpu;
mod dev;
mod exec;
mod fd;
mod hostio;
mod image;
mod loadavg;
mod net;
mod park;
mod pipe;
mod proc;
mod procinfo;
mod procmem;
mod sandbox;
mod signal;
mod spawn;
mod sys;
mod tty;

pub mod config;
pub mod kernel;

pub use config::{BASE_ENV, ClockMode, CpuTopology, KernelConfig, SandboxConfig, SandboxLimits, SpawnerHook, SpawnerInfo};
pub use hostio::{HostReader, HostStdio, HostWriter, ReadOutcome, RunOutput, RunRequest, Spawned, StdioSpec};
pub use kernel::{Kernel, UserGroup, UserGroupSpec};
pub use sandbox::{CreateError, FsEntry, HostProcInfo, Sandbox, SandboxFs, Snapshot, TreeEntry, Usage, WriteMode};
