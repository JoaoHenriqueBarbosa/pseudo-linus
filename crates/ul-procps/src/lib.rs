//! procps do pseudo-linus, fiel ao Debian 13: `ps`, `top -b`, `free`, `uptime`, `pgrep`, `pkill`,
//! `kill` e `watch` (procps-ng 4.0.4), `pidof` (sysvinit-utils 3.14) e `killall` (psmisc 23.7).
//!
//! Os dados vêm do `/proc` do sandbox, como no Linux; enquanto o procfs do kernel não existe, a lista
//! de processos vem de `Syscalls::list_processes` (ver `STATUS.md`). Nada toca o host.

pub mod common;
pub mod free;
pub mod kill;
pub mod killall;
pub mod matcher;
pub mod pgrep;
pub mod pidof;
pub mod procfs;
pub mod ps;
pub mod top;
pub mod uptime;
pub mod watch;

use sysabi::Program;

/// Tabela de programas do crate.
pub fn programs() -> Vec<Program> {
    vec![
        Program::bin("free", free::main),
        Program::bin("kill", kill::main),
        Program::bin("killall", killall::main),
        Program::bin("pgrep", pgrep::pgrep_main),
        Program::bin("pidof", pidof::main),
        Program::bin("pidwait", pgrep::pidwait_main),
        Program::bin("pkill", pgrep::pkill_main),
        Program::bin("ps", ps::main),
        Program::bin("top", top::main),
        Program::bin("uptime", uptime::main),
        Program::bin("watch", watch::main),
    ]
}
