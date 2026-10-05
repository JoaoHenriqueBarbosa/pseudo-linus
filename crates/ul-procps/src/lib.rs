//! procps do pseudo-linus, fiel ao Debian 13: `ps`, `top -b`, `free`, `uptime`, `w`, `pgrep`, `pkill`,
//! `kill`, `watch`, `pwdx`, `pmap` e `sysctl` (procps-ng 4.0.4), `pidof` (sysvinit-utils 3.14),
//! `killall` e `pstree` (psmisc 23.7).
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
pub mod pmap;
pub mod procfs;
pub mod ps;
pub mod pstree;
pub mod pwdx;
pub mod sysctl;
pub mod top;
pub mod uptime;
pub mod w;
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
        Program::bin("pmap", pmap::main),
        Program::bin("ps", ps::main),
        Program::bin("pstree", pstree::main),
        Program::bin("pwdx", pwdx::main),
        Program::bin("sysctl", sysctl::main),
        Program::bin("top", top::main),
        Program::bin("uptime", uptime::main),
        Program::bin("w", w::main),
        Program::bin("watch", watch::main),
    ]
}
