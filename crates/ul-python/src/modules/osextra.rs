//! `_os` (segunda parte): as chamadas de sistema que o `os`/`posix` do 3.13 expõe além das de `osnative`
//! (grupos e sessões, escalonador, prioridade, limites, tempos, terminal, `pread`/`pwrite`, `utimensat` com
//! nanossegundos). Cada função é uma chamada de sistema só: a conta que a glibc faz por cima (`nice`,
//! `sysconf`, `times`) fica no `os.py`, onde o CPython também a tem em C.

use sysabi::{sys, AtFlags, Errno, FallocFlags, Fd, Resource, RusageWho, SchedParam, SetAttrWhen, SetTime, SysResult, Syscalls, TimeSpec, Termios, Winsize};

use crate::modules::osnative::{arg, need_kernel, os_error, path_bytes, want_fd, OrOs};
use crate::modules::ModuleBuilder;
use crate::native_util::{no_kwargs, want_int};
use crate::object::{Kw, Value};
use crate::vm::{PyResult, Vm};

fn want_pid(fname: &str, args: &[Value], i: usize) -> PyResult<i32> {
    need_kernel()?;
    Ok(want_int(arg(fname, args, i)?)? as i32)
}

fn int(n: impl Into<i64>) -> Value {
    Value::Int(n.into())
}

/// `-1` (o `(uid_t) -1` do C) é "não muda".
fn opt_id(fname: &str, args: &[Value], i: usize) -> PyResult<Option<u32>> {
    Ok(match want_int(arg(fname, args, i)?)? {
        -1 => None,
        n => Some(n as u32),
    })
}

fn cpu_list(cpus: Vec<usize>) -> Value {
    Value::list(cpus.into_iter().map(|c| int(c as i64)).collect())
}

fn getpgid(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("getpgid", &kw)?;
    let pid = want_pid("getpgid", &args, 0)?;
    Ok(int(sys::current().getpgid(pid).or_os(None)?))
}

fn getsid(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("getsid", &kw)?;
    let pid = want_pid("getsid", &args, 0)?;
    Ok(int(sys::current().getsid(pid).or_os(None)?))
}

fn tcgetpgrp(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("tcgetpgrp", &kw)?;
    let fd = want_fd("tcgetpgrp", &args, 0)?;
    Ok(int(sys::current().tcgetpgrp(fd).or_os(None)?))
}

fn tcsetpgrp(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("tcsetpgrp", &kw)?;
    let fd = want_fd("tcsetpgrp", &args, 0)?;
    let pgrp = want_int(arg("tcsetpgrp", &args, 1)?)? as i32;
    sys::current().tcsetpgrp(fd, pgrp).or_os(None)?;
    Ok(Value::None)
}

fn fchdir(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("fchdir", &kw)?;
    let fd = want_fd("fchdir", &args, 0)?;
    sys::current().fchdir(fd).or_os(None)?;
    Ok(Value::None)
}

fn fchmod(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("fchmod", &kw)?;
    let fd = want_fd("fchmod", &args, 0)?;
    let mode = want_int(arg("fchmod", &args, 1)?)? as u32;
    sys::current().fchmod(fd, mode).or_os(None)?;
    Ok(Value::None)
}

/// `fchown(fd, uid, gid)`: o `fchownat(fd, "", ..., AT_EMPTY_PATH)`.
fn fchown(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("fchown", &kw)?;
    let fd = want_fd("fchown", &args, 0)?;
    let uid = opt_id("fchown", &args, 1)?;
    let gid = opt_id("fchown", &args, 2)?;
    sys::current().fchownat(fd, b"", uid, gid, AtFlags::EMPTY_PATH).or_os(None)?;
    Ok(Value::None)
}

fn pread(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("pread", &kw)?;
    let fd = want_fd("pread", &args, 0)?;
    let n = want_int(arg("pread", &args, 1)?)? as usize;
    let offset = want_int(arg("pread", &args, 2)?)? as u64;
    let mut buf = vec![0u8; n];
    let got = sys::current().pread(fd, &mut buf, offset).or_os(None)?;
    buf.truncate(got);
    Ok(Value::bytes(buf))
}

fn pwrite(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("pwrite", &kw)?;
    let fd = want_fd("pwrite", &args, 0)?;
    let data = match arg("pwrite", &args, 1)?.bytes_like() {
        Some(b) => b.to_vec(),
        None => return Err(crate::vm::type_error("a bytes-like object is required")),
    };
    let offset = want_int(arg("pwrite", &args, 2)?)? as u64;
    let wrote = sys::current().pwrite(fd, &data, offset).or_os(None)?;
    Ok(int(wrote as i64))
}

/// `fallocate(fd, mode, offset, len)`: o `fallocate(2)` cru; o `posix_fallocate` do `os.py` usa o modo 0.
fn fallocate(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("fallocate", &kw)?;
    let fd = want_fd("fallocate", &args, 0)?;
    let mode = FallocFlags::from_bits_retain(want_int(arg("fallocate", &args, 1)?)? as u32);
    let offset = want_int(arg("fallocate", &args, 2)?)?;
    let len = want_int(arg("fallocate", &args, 3)?)?;
    sys::fallocate(fd, mode, offset, len).or_os(None)?;
    Ok(Value::None)
}

/// O instante de `utimensat`: `None` é "agora", `(segundos, nanossegundos)` é o instante.
fn want_set_time(fname: &str, args: &[Value], i: usize) -> PyResult<SetTime> {
    let (sec, nsec) = (arg(fname, args, i)?, arg(fname, args, i + 1)?);
    Ok(match (sec, nsec) {
        (Value::None, _) => SetTime::Now,
        _ => SetTime::At(TimeSpec { sec: want_int(sec)?, nsec: want_int(nsec)? as u32 }),
    })
}

/// `utimens(destino, asec, ansec, msec, mnsec, dir_fd, follow)`: `destino` é caminho ou fd; os tempos são
/// `None, None` para "agora" ou os segundos e nanossegundos inteiros (sem passar por `float`).
fn utimens(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("utimens", &kw)?;
    need_kernel()?;
    let atime = want_set_time("utimens", &args, 1)?;
    let mtime = want_set_time("utimens", &args, 3)?;
    if let Value::Int(fd) = arg("utimens", &args, 0)? {
        sys::current().futimens(Fd(*fd as i32), atime, mtime).or_os(None)?;
        return Ok(Value::None);
    }
    let path = path_bytes("utimens", args.first(), 0)?;
    let dir_fd = match arg("utimens", &args, 5)? {
        Value::None => Fd::CWD,
        v => Fd(want_int(v)? as i32),
    };
    let flags = if arg("utimens", &args, 6)?.is_true() { AtFlags::empty() } else { AtFlags::SYMLINK_NOFOLLOW };
    sys::current().utimensat(dir_fd, &path, atime, mtime, flags).or_os(Some(&path))?;
    Ok(Value::None)
}

fn getpriority(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("getpriority", &kw)?;
    let pid = want_pid("getpriority", &args, 0)?;
    Ok(int(sys::current().getpriority(pid).or_os(None)?))
}

fn setpriority(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("setpriority", &kw)?;
    let pid = want_pid("setpriority", &args, 0)?;
    let nice = want_int(arg("setpriority", &args, 1)?)? as i32;
    sys::current().setpriority(pid, nice).or_os(None)?;
    Ok(Value::None)
}

fn sched_yield(_vm: &mut Vm, _args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("sched_yield", &kw)?;
    need_kernel()?;
    sys::current().sched_yield();
    Ok(Value::None)
}

fn sched_getaffinity(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("sched_getaffinity", &kw)?;
    let pid = want_pid("sched_getaffinity", &args, 0)?;
    Ok(cpu_list(sys::current().sched_getaffinity_of(pid).or_os(None)?))
}

fn sched_setaffinity(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("sched_setaffinity", &kw)?;
    let pid = want_pid("sched_setaffinity", &args, 0)?;
    let cpus: Vec<usize> = match arg("sched_setaffinity", &args, 1)? {
        Value::List(l) => l.borrow().iter().map(|c| want_int(c).map(|n| n as usize)).collect::<PyResult<_>>()?,
        _ => return Err(crate::vm::type_error("sched_setaffinity: expected a list of CPUs")),
    };
    sys::current().sched_setaffinity(pid, &cpus).or_os(None)?;
    Ok(Value::None)
}

fn sched_getscheduler(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("sched_getscheduler", &kw)?;
    let pid = want_pid("sched_getscheduler", &args, 0)?;
    Ok(int(sys::sched_getscheduler(pid).or_os(None)?))
}

fn sched_setscheduler(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("sched_setscheduler", &kw)?;
    let pid = want_pid("sched_setscheduler", &args, 0)?;
    let policy = want_int(arg("sched_setscheduler", &args, 1)?)? as i32;
    let priority = want_int(arg("sched_setscheduler", &args, 2)?)? as i32;
    sys::sched_setscheduler(pid, policy, SchedParam { priority }).or_os(None)?;
    Ok(Value::None)
}

fn sched_getparam(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("sched_getparam", &kw)?;
    let pid = want_pid("sched_getparam", &args, 0)?;
    Ok(int(sys::sched_getparam(pid).or_os(None)?.priority))
}

fn sched_setparam(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("sched_setparam", &kw)?;
    let pid = want_pid("sched_setparam", &args, 0)?;
    let priority = want_int(arg("sched_setparam", &args, 1)?)? as i32;
    sys::sched_setparam(pid, SchedParam { priority }).or_os(None)?;
    Ok(Value::None)
}

fn sched_rr_get_interval(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("sched_rr_get_interval", &kw)?;
    let pid = want_pid("sched_rr_get_interval", &args, 0)?;
    Ok(Value::Float(sys::sched_rr_get_interval(pid).or_os(None)?.as_secs_f64()))
}

fn sched_get_priority_max(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("sched_get_priority_max", &kw)?;
    let policy = want_int(arg("sched_get_priority_max", &args, 0)?)? as i32;
    Ok(int(sys::sched_get_priority_max(policy).or_os(None)?))
}

fn sched_get_priority_min(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("sched_get_priority_min", &kw)?;
    let policy = want_int(arg("sched_get_priority_min", &args, 0)?)? as i32;
    Ok(int(sys::sched_get_priority_min(policy).or_os(None)?))
}

/// `(atual, máximo)` de um `RLIMIT_*`, com `-1` no `RLIM_INFINITY`.
fn getrlimit(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("getrlimit", &kw)?;
    need_kernel()?;
    let res = match want_int(arg("getrlimit", &args, 0)?)? {
        0 => Resource::Cpu,
        1 => Resource::Fsize,
        2 => Resource::Data,
        3 => Resource::Stack,
        4 => Resource::Core,
        5 => Resource::Rss,
        6 => Resource::Nproc,
        7 => Resource::Nofile,
        8 => Resource::Memlock,
        9 => Resource::As,
        10 => Resource::Locks,
        11 => Resource::Sigpending,
        12 => Resource::Msgqueue,
        13 => Resource::Nice,
        14 => Resource::Rtprio,
        15 => Resource::Rttime,
        _ => return Err(os_error(Errno::EINVAL, None)),
    };
    let lim = sys::current().getrlimit(res).or_os(None)?;
    let show = |v: u64| if v == sysabi::RLIM_INFINITY { int(-1) } else { int(v as i64) };
    Ok(Value::tuple(vec![show(lim.cur), show(lim.max)]))
}

/// `(utime, stime)` em segundos do processo (`0`) ou dos filhos já colhidos (`-1`).
fn getrusage(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("getrusage", &kw)?;
    need_kernel()?;
    let who = match want_int(arg("getrusage", &args, 0)?)? {
        0 => RusageWho::SelfProcess,
        -1 => RusageWho::Children,
        _ => return Err(os_error(Errno::EINVAL, None)),
    };
    let usage = sys::current().getrusage(who).or_os(None)?;
    Ok(Value::tuple(vec![Value::Float(usage.utime.as_secs_f64()), Value::Float(usage.stime.as_secs_f64())]))
}

/// `(colunas, linhas)` do terminal em `fd`: o `TIOCGWINSZ`.
fn winsize(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("winsize", &kw)?;
    let fd = want_fd("winsize", &args, 0)?;
    let ws = sys::current().tcgetwinsize(fd).or_os(None)?;
    Ok(Value::tuple(vec![int(ws.cols), int(ws.rows)]))
}

/// `TIOCGPTN`: o número do pty de um mestre.
fn pty_number(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("pty_number", &kw)?;
    let fd = want_fd("pty_number", &args, 0)?;
    Ok(int(sys::current().pty_number(fd).or_os(None)?))
}

/// `tcgetattr(fd)`: `(iflag, oflag, cflag, lflag, line, cc)` do `TCGETS`, com `cc` em `bytes` (`NCCS` do kernel).
/// O `termios.py` completa o que a glibc acrescenta (a lista de 32 e as velocidades).
fn tcgetattr(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("tcgetattr", &kw)?;
    let fd = want_fd("tcgetattr", &args, 0)?;
    let t = sys::current().tcgetattr(fd).or_os(None)?;
    Ok(Value::tuple(vec![int(t.c_iflag), int(t.c_oflag), int(t.c_cflag), int(t.c_lflag), int(t.c_line), Value::bytes(t.c_cc.to_vec())]))
}

/// `tcsetattr(fd, quando, iflag, oflag, cflag, lflag, line, cc)`: o `TCSETS`, `TCSETSW` ou `TCSETSF`.
fn tcsetattr(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("tcsetattr", &kw)?;
    let fd = want_fd("tcsetattr", &args, 0)?;
    let when = match want_int(arg("tcsetattr", &args, 1)?)? {
        0 => SetAttrWhen::Now,
        1 => SetAttrWhen::Drain,
        2 => SetAttrWhen::Flush,
        _ => return Err(os_error(Errno::EINVAL, None)),
    };
    let flag = |i| Ok::<u32, crate::vm::PyException>(want_int(arg("tcsetattr", &args, i)?)? as u32);
    let mut t = Termios { c_iflag: flag(2)?, c_oflag: flag(3)?, c_cflag: flag(4)?, c_lflag: flag(5)?, c_line: flag(6)? as u8, ..Termios::default() };
    let cc = arg("tcsetattr", &args, 7)?.bytes_like().map(|c| c.to_vec()).unwrap_or_default();
    let n = cc.len().min(t.c_cc.len());
    t.c_cc[..n].copy_from_slice(&cc[..n]);
    sys::current().tcsetattr(fd, when, &t).or_os(None)?;
    Ok(Value::None)
}

/// `tcsetwinsize(fd, linhas, colunas)`: o `TIOCSWINSZ` que muda só as duas dimensões (os pixels ficam).
fn tcsetwinsize(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("tcsetwinsize", &kw)?;
    let fd = want_fd("tcsetwinsize", &args, 0)?;
    let dim = |i| Ok::<u16, crate::vm::PyException>(want_int(arg("tcsetwinsize", &args, i)?)? as u16);
    let (rows, cols) = (dim(1)?, dim(2)?);
    let p = sys::current();
    let old = p.tcgetwinsize(fd).or_os(None)?;
    p.tcsetwinsize(fd, Winsize { rows, cols, ..old }).or_os(None)?;
    Ok(Value::None)
}

/// `tcflush(fd, fila)` e `tcflow(fd, ação)`: o `TCFLSH` e o `TCXONC`, que só diferem na chamada.
fn tc_int_arg(name: &str, args: &[Value], kw: &Kw, call: fn(&dyn Syscalls, Fd, i32) -> SysResult<()>) -> PyResult<Value> {
    no_kwargs(name, kw)?;
    let fd = want_fd(name, args, 0)?;
    let n = want_int(arg(name, args, 1)?)? as i32;
    call(&*sys::current(), fd, n).or_os(None)?;
    Ok(Value::None)
}

fn tcflush(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    tc_int_arg("tcflush", &args, &kw, |p, fd, queue| p.tcflush(fd, queue))
}

fn tcflow(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    tc_int_arg("tcflow", &args, &kw, |p, fd, action| p.tcflow(fd, action))
}

/// `TIOCSPTLCK` com `0`: o `unlockpt`.
fn pty_unlock(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("pty_unlock", &kw)?;
    let fd = want_fd("pty_unlock", &args, 0)?;
    sys::current().pty_set_lock(fd, false).or_os(None)?;
    Ok(Value::None)
}

/// `kill_many(quem, id, sinal)` para o que o `kill(2)` chama de `pid <= 0`: `quem` 0 é o grupo `id` (o do
/// chamador com `id` 0) e 1 é todo processo que o chamador pode sinalizar (`kill -1`).
fn kill_many(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("kill_many", &kw)?;
    need_kernel()?;
    let which = want_int(arg("kill_many", &args, 0)?)?;
    let id = want_int(arg("kill_many", &args, 1)?)? as i32;
    let sig = sysabi::Signal(want_int(arg("kill_many", &args, 2)?)? as i32);
    let target = if which == 0 { sysabi::KillTarget::Group(id) } else { sysabi::KillTarget::All };
    sys::current().kill(target, sig).map_err(|e| match e {
        Errno::ESRCH => crate::vm::exc("ProcessLookupError", format!("[Errno {}] {}", e.0, e.message())),
        _ => os_error(e, None),
    })?;
    // Sinal para o próprio grupo: o tratador do programa roda logo depois da chamada, entre duas instruções.
    crate::vm::request_signal_check();
    Ok(Value::None)
}

fn eventfd(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("eventfd", &kw)?;
    need_kernel()?;
    let initval = want_int(arg("eventfd", &args, 0)?)? as u32;
    let flags = want_int(arg("eventfd", &args, 1)?)? as u32;
    Ok(int(sys::current().eventfd(initval, flags).or_os(None)?.0))
}

fn timerfd_create(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("timerfd_create", &kw)?;
    need_kernel()?;
    let clock = want_int(arg("timerfd_create", &args, 0)?)? as i32;
    let flags = want_int(arg("timerfd_create", &args, 1)?)? as u32;
    Ok(int(sys::current().timerfd_create(clock, flags).or_os(None)?.0))
}

/// `timerfd_settime(fd, flags, valor_ns, intervalo_ns)`: `(restava_ns, intervalo_ns)` de antes.
fn timerfd_settime(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("timerfd_settime", &kw)?;
    let fd = want_fd("timerfd_settime", &args, 0)?;
    let flags = want_int(arg("timerfd_settime", &args, 1)?)? as u32;
    let value = want_int(arg("timerfd_settime", &args, 2)?)? as u64;
    let interval = want_int(arg("timerfd_settime", &args, 3)?)? as u64;
    let (left, every) = sys::current().timerfd_settime(fd, flags, value, interval).or_os(None)?;
    Ok(Value::tuple(vec![int(left as i64), int(every as i64)]))
}

fn timerfd_gettime(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("timerfd_gettime", &kw)?;
    let fd = want_fd("timerfd_gettime", &args, 0)?;
    let (left, every) = sys::current().timerfd_gettime(fd).or_os(None)?;
    Ok(Value::tuple(vec![int(left as i64), int(every as i64)]))
}

fn memfd_create(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("memfd_create", &kw)?;
    need_kernel()?;
    let name = path_bytes("memfd_create", args.first(), 0)?;
    let flags = want_int(arg("memfd_create", &args, 1)?)? as u32;
    Ok(int(sys::current().memfd_create(&name, flags).or_os(None)?.0))
}

fn chroot(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("chroot", &kw)?;
    need_kernel()?;
    let path = path_bytes("chroot", args.first(), 0)?;
    sys::current().chroot(&path).or_os(Some(&path))?;
    Ok(Value::None)
}

/// O alvo de um `*xattr`: um fd (`fgetxattr`, `EMPTY_PATH`) ou um caminho, que sem `follow` é o `l*xattr`; o
/// quarto valor é o que a mensagem de erro mostra (o caminho, ou o número do fd).
fn xattr_target(fname: &str, args: &[Value], follow_at: usize) -> PyResult<(Fd, Vec<u8>, AtFlags, Vec<u8>)> {
    need_kernel()?;
    Ok(match arg(fname, args, 0)? {
        Value::Int(fd) => (Fd(*fd as i32), Vec::new(), AtFlags::EMPTY_PATH, fd.to_string().into_bytes()),
        v => {
            let path = path_bytes(fname, Some(v), 0)?;
            let flags = if arg(fname, args, follow_at)?.is_true() { AtFlags::empty() } else { AtFlags::SYMLINK_NOFOLLOW };
            (Fd::CWD, path.clone(), flags, path)
        }
    })
}

/// `getxattr(alvo, nome, seguir)`.
fn getxattr(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("getxattr", &kw)?;
    let (dirfd, path, flags, label) = xattr_target("getxattr", &args, 2)?;
    let name = path_bytes("getxattr", args.get(1), 1)?;
    let value = sys::current().getxattr(dirfd, &path, flags, &name).or_os(Some(&label))?;
    Ok(Value::bytes(value))
}

/// `setxattr(alvo, nome, valor, flags, seguir)`.
fn setxattr(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("setxattr", &kw)?;
    let (dirfd, path, flags, label) = xattr_target("setxattr", &args, 4)?;
    let name = path_bytes("setxattr", args.get(1), 1)?;
    let Some(value) = arg("setxattr", &args, 2)?.bytes_like() else {
        return Err(crate::vm::type_error("a bytes-like object is required"));
    };
    let xflags = want_int(arg("setxattr", &args, 3)?)? as u32;
    sys::current().setxattr(dirfd, &path, flags, &name, &value, xflags).or_os(Some(&label))?;
    Ok(Value::None)
}

/// `listxattr(alvo, seguir)`: a lista dos nomes, em `bytes`.
fn listxattr(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("listxattr", &kw)?;
    let (dirfd, path, flags, label) = xattr_target("listxattr", &args, 1)?;
    let names = sys::current().listxattr(dirfd, &path, flags).or_os(Some(&label))?;
    Ok(Value::list(names.into_iter().map(Value::bytes).collect()))
}

/// `removexattr(alvo, nome, seguir)`.
fn removexattr(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("removexattr", &kw)?;
    let (dirfd, path, flags, label) = xattr_target("removexattr", &args, 2)?;
    let name = path_bytes("removexattr", args.get(1), 1)?;
    sys::current().removexattr(dirfd, &path, flags, &name).or_os(Some(&label))?;
    Ok(Value::None)
}

pub(crate) fn register(builder: ModuleBuilder) -> ModuleBuilder {
    builder
        .func("eventfd", eventfd)
        .func("timerfd_create", timerfd_create)
        .func("timerfd_settime", timerfd_settime)
        .func("timerfd_gettime", timerfd_gettime)
        .func("memfd_create", memfd_create)
        .func("chroot", chroot)
        .func("getxattr", getxattr)
        .func("setxattr", setxattr)
        .func("listxattr", listxattr)
        .func("removexattr", removexattr)
        .func("kill_many", kill_many)
        .func("getpgid", getpgid)
        .func("getsid", getsid)
        .func("tcgetpgrp", tcgetpgrp)
        .func("tcsetpgrp", tcsetpgrp)
        .func("fchdir", fchdir)
        .func("fchmod", fchmod)
        .func("fchown", fchown)
        .func("pread", pread)
        .func("pwrite", pwrite)
        .func("fallocate", fallocate)
        .func("utimens", utimens)
        .func("getpriority", getpriority)
        .func("setpriority", setpriority)
        .func("sched_yield", sched_yield)
        .func("sched_getaffinity", sched_getaffinity)
        .func("sched_setaffinity", sched_setaffinity)
        .func("sched_getscheduler", sched_getscheduler)
        .func("sched_setscheduler", sched_setscheduler)
        .func("sched_getparam", sched_getparam)
        .func("sched_setparam", sched_setparam)
        .func("sched_rr_get_interval", sched_rr_get_interval)
        .func("sched_get_priority_max", sched_get_priority_max)
        .func("sched_get_priority_min", sched_get_priority_min)
        .func("getrlimit", getrlimit)
        .func("getrusage", getrusage)
        .func("winsize", winsize)
        .func("pty_number", pty_number)
        .func("tcgetattr", tcgetattr)
        .func("tcsetattr", tcsetattr)
        .func("tcsetwinsize", tcsetwinsize)
        .func("tcflush", tcflush)
        .func("tcflow", tcflow)
        .func("pty_unlock", pty_unlock)
}
