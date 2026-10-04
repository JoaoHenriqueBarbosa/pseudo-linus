//! Chamadas avulsas do `unistd.h` com cara de Rust (o que os portes faziam com `libc`, `nix` ou
//! `rustix`): identidade, nome do host, terminal, prioridade, sinais, sincronização.

use std::io;

use sysabi::{Fd, KillTarget, Pid, Resource, Rlimit, Signal, Utsname, Winsize};

use crate::errno::cvt;
use crate::proc;

pub fn getpid() -> Pid {
    proc::sys().getpid()
}

pub fn getppid() -> Pid {
    proc::sys().getppid()
}

pub fn getpgid(pid: Pid) -> io::Result<Pid> {
    cvt(proc::sys().getpgid(pid))
}

pub fn getsid(pid: Pid) -> io::Result<Pid> {
    cvt(proc::sys().getsid(pid))
}

pub fn setsid() -> io::Result<Pid> {
    cvt(proc::sys().setsid())
}

pub fn setpgid(pid: Pid, pgid: Pid) -> io::Result<()> {
    cvt(proc::sys().setpgid(pid, pgid))
}

/// `uname(2)`.
pub fn uname() -> Utsname {
    proc::sys().uname()
}

/// `gethostname(2)` (o `nodename` do `uname`).
pub fn gethostname() -> Vec<u8> {
    proc::sys().uname().nodename
}

pub fn sethostname(name: &[u8]) -> io::Result<()> {
    cvt(proc::sys().sethostname(name))
}

/// `isatty(3)`.
pub fn isatty(fd: i32) -> bool {
    proc::sys().isatty(Fd(fd))
}

/// `ttyname(3)`: o alvo do link `/proc/self/fd/N` quando o fd é terminal; ENOTTY se não é.
pub fn ttyname(fd: i32) -> io::Result<Vec<u8>> {
    if !isatty(fd) {
        return Err(crate::errno::err(crate::errno::ENOTTY));
    }
    let link = format!("/proc/self/fd/{fd}");
    crate::fs::read_link_bytes(std::ffi::OsStr::new(&link))
}

/// `ioctl(TIOCGWINSZ)`.
pub fn winsize(fd: i32) -> io::Result<Winsize> {
    cvt(proc::sys().tcgetwinsize(Fd(fd)))
}

/// `getpriority(PRIO_PROCESS, pid)`: o valor de nice (`0` = o próprio processo).
pub fn getpriority(pid: Pid) -> io::Result<i32> {
    cvt(proc::sys().getpriority(pid))
}

pub fn setpriority(pid: Pid, nice: i32) -> io::Result<()> {
    cvt(proc::sys().setpriority(pid, nice))
}

/// `kill(2)` num pid.
pub fn kill(pid: Pid, sig: Signal) -> io::Result<()> {
    cvt(proc::sys().kill(KillTarget::Pid(pid), sig))
}

/// `kill(2)` com o alvo completo (grupo, todos).
pub fn kill_target(target: KillTarget, sig: Signal) -> io::Result<()> {
    cvt(proc::sys().kill(target, sig))
}

/// `sigaction` reduzido: padrão, ignorar ou capturar.
pub fn signal(sig: Signal, disposition: sysabi::SigDisposition) -> io::Result<sysabi::SigDisposition> {
    cvt(proc::sys().sigaction(sig, disposition))
}

/// Sinais capturados desde a última consulta.
pub fn take_caught_signals() -> Vec<Signal> {
    proc::sys().take_caught_signals()
}

pub fn getrlimit(res: Resource) -> io::Result<Rlimit> {
    cvt(proc::sys().getrlimit(res))
}

pub fn setrlimit(res: Resource, lim: Rlimit) -> io::Result<()> {
    cvt(proc::sys().setrlimit(res, lim))
}

/// `sync(2)`: no pseudo-linus o tmpfs já é a memória; o hostfs (quando houver) sincroniza no
/// kernel. Sincroniza o que der pelos fds abertos e não falha, como o `sync(2)`.
pub fn sync() {
    let sys = proc::sys();
    for fd in sys.open_fds() {
        let _ = sys.fsync(fd);
    }
}

/// `fsync(2)`/`fdatasync(2)`.
pub fn fsync(fd: i32) -> io::Result<()> {
    cvt(proc::sys().fsync(Fd(fd)))
}

/// Fds abertos do processo.
pub fn open_fds() -> Vec<i32> {
    proc::sys().open_fds().into_iter().map(|f| f.0).collect()
}

/// Processos visíveis no sandbox.
pub fn processes() -> Vec<sysabi::ProcInfo> {
    proc::sys().list_processes()
}

/// `close(2)`.
pub fn close(fd: i32) -> io::Result<()> {
    cvt(proc::sys().close(Fd(fd)))
}

/// `dup2(2)`.
pub fn dup2(old: i32, new: i32) -> io::Result<i32> {
    if old == new {
        return Ok(new);
    }
    cvt(proc::sys().dup3(Fd(old), Fd(new), false)).map(|f| f.0)
}

/// Ponto de preempção pra laços longos sem syscall.
pub fn checkpoint() {
    sysabi::sys::checkpoint();
}
