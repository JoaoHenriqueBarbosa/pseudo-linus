//! `_os`: as chamadas de sistema cruas que os módulos `os`, `io`, `pathlib`, `shutil` e afins (em
//! Python embutido) usam. Tudo passa pelas `sysabi::sys`, ou seja, pelo VFS e pelo pseudo-processo
//! do sandbox. Caminhos aceitam `str` e `bytes`; descritores são `int`.

use std::rc::Rc;

use sysabi::{sys, AtFlags, Errno, Fd, FileType, OFlags, Whence};

use crate::modules::ModuleBuilder;
use crate::native_util::{no_kwargs, want_int};
use crate::object::{Kw, ModuleObj, Value};
use crate::vm::{exc, type_error, PyException, PyResult, Vm};

/// `OSError` (ou a subclasse certa) para um `errno`, com a mensagem do `strerror`.
fn os_error(e: Errno, path: Option<&str>) -> PyException {
    let kind = match e {
        Errno::ENOENT => "FileNotFoundError",
        Errno::EEXIST => "FileExistsError",
        Errno::EISDIR => "IsADirectoryError",
        Errno::ENOTDIR => "NotADirectoryError",
        Errno::EACCES | Errno::EPERM => "PermissionError",
        Errno::EAGAIN => "BlockingIOError",
        Errno::EPIPE => "BrokenPipeError",
        Errno::ECHILD => "ChildProcessError",
        Errno::ECONNREFUSED => "ConnectionRefusedError",
        Errno::ECONNRESET => "ConnectionResetError",
        _ => "OSError",
    };
    let msg = match path {
        Some(p) => format!("[Errno {}] {}: '{p}'", e.0, e.message()),
        None => format!("[Errno {}] {}", e.0, e.message()),
    };
    exc(kind, msg)
}

fn path_bytes(fname: &str, v: &Value) -> PyResult<Vec<u8>> {
    match v {
        Value::Str(s) => Ok(s.as_str().as_bytes().to_vec()),
        Value::Bytes(b) => Ok(b.to_vec()),
        other => Err(type_error(format!("{fname}: path should be string, bytes or os.PathLike, not {}", other.type_name()))),
    }
}

fn shown(p: &[u8]) -> String {
    String::from_utf8_lossy(p).into_owned()
}

fn arg<'a>(fname: &str, args: &'a [Value], i: usize) -> PyResult<&'a Value> {
    args.get(i).ok_or_else(|| type_error(format!("{fname}() missing required argument (pos {})", i + 1)))
}

fn getcwd(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("getcwd", &kw)?;
    let _ = args;
    let cwd = sys::current().getcwd().map_err(|e| os_error(e, None))?;
    Ok(Value::str(shown(&cwd)))
}

fn chdir(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("chdir", &kw)?;
    let p = path_bytes("chdir", arg("chdir", &args, 0)?)?;
    sys::current().chdir(&p).map_err(|e| os_error(e, Some(&shown(&p))))?;
    Ok(Value::None)
}

fn listdir(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("listdir", &kw)?;
    let p = match args.first() {
        Some(v) => path_bytes("listdir", v)?,
        None => b".".to_vec(),
    };
    let entries = sys::read_dir(&p).map_err(|e| os_error(e, Some(&shown(&p))))?;
    Ok(Value::list(entries.into_iter().map(|e| Value::str(shown(&e.name))).collect()))
}

/// `scandir`: lista de `(nome, tipo)` onde tipo é `"f"`, `"d"`, `"l"` ou `"o"`.
fn scandir(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("scandir", &kw)?;
    let p = match args.first() {
        Some(v) => path_bytes("scandir", v)?,
        None => b".".to_vec(),
    };
    let entries = sys::read_dir(&p).map_err(|e| os_error(e, Some(&shown(&p))))?;
    Ok(Value::list(
        entries
            .into_iter()
            .map(|e| {
                let kind = match e.kind {
                    FileType::Regular => "f",
                    FileType::Directory => "d",
                    FileType::Symlink => "l",
                    _ => "o",
                };
                Value::tuple(vec![Value::str(shown(&e.name)), Value::str(kind)])
            })
            .collect(),
    ))
}

fn stat_tuple(st: &sysabi::Stat) -> Value {
    let t = |ts: &sysabi::TimeSpec| Value::Float(ts.sec as f64 + f64::from(ts.nsec) / 1e9);
    Value::tuple(vec![
        Value::Int(i64::from(st.mode)),
        Value::Int(st.ino as i64),
        Value::Int(st.dev as i64),
        Value::Int(st.nlink as i64),
        Value::Int(i64::from(st.uid)),
        Value::Int(i64::from(st.gid)),
        Value::Int(st.size as i64),
        t(&st.atime),
        t(&st.mtime),
        t(&st.ctime),
    ])
}

/// `stat(path)` como tupla `(mode, ino, dev, nlink, uid, gid, size, atime, mtime, ctime)`.
fn stat(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("stat", &kw)?;
    let p = path_bytes("stat", arg("stat", &args, 0)?)?;
    let follow = args.get(1).is_none_or(Value::is_true);
    let st = if follow { sys::stat(&p) } else { sys::lstat(&p) };
    st.map(|s| stat_tuple(&s)).map_err(|e| os_error(e, Some(&shown(&p))))
}

fn fstat(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("fstat", &kw)?;
    let fd = want_int(arg("fstat", &args, 0)?)? as i32;
    sys::current().fstat(Fd(fd)).map(|s| stat_tuple(&s)).map_err(|e| os_error(e, None))
}

fn mkdir(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("mkdir", &kw)?;
    let p = path_bytes("mkdir", arg("mkdir", &args, 0)?)?;
    let mode = args.get(1).map_or(Ok(0o777), want_int)? as u32;
    sys::current().mkdirat(Fd::CWD, &p, mode).map_err(|e| os_error(e, Some(&shown(&p))))?;
    Ok(Value::None)
}

fn unlink(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("unlink", &kw)?;
    let p = path_bytes("unlink", arg("unlink", &args, 0)?)?;
    sys::current().unlinkat(Fd::CWD, &p, AtFlags::empty()).map_err(|e| os_error(e, Some(&shown(&p))))?;
    Ok(Value::None)
}

fn rmdir(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("rmdir", &kw)?;
    let p = path_bytes("rmdir", arg("rmdir", &args, 0)?)?;
    sys::current().unlinkat(Fd::CWD, &p, AtFlags::REMOVEDIR).map_err(|e| os_error(e, Some(&shown(&p))))?;
    Ok(Value::None)
}

fn rename(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("rename", &kw)?;
    let a = path_bytes("rename", arg("rename", &args, 0)?)?;
    let b = path_bytes("rename", arg("rename", &args, 1)?)?;
    sys::current()
        .renameat2(Fd::CWD, &a, Fd::CWD, &b, sysabi::RenameFlags::empty())
        .map_err(|e| os_error(e, Some(&shown(&a))))?;
    Ok(Value::None)
}

fn readlink(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("readlink", &kw)?;
    let p = path_bytes("readlink", arg("readlink", &args, 0)?)?;
    let t = sys::current().readlinkat(Fd::CWD, &p).map_err(|e| os_error(e, Some(&shown(&p))))?;
    Ok(Value::str(shown(&t)))
}

fn symlink(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("symlink", &kw)?;
    let target = path_bytes("symlink", arg("symlink", &args, 0)?)?;
    let p = path_bytes("symlink", arg("symlink", &args, 1)?)?;
    sys::current().symlinkat(&target, Fd::CWD, &p).map_err(|e| os_error(e, Some(&shown(&p))))?;
    Ok(Value::None)
}

fn chmod(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("chmod", &kw)?;
    let p = path_bytes("chmod", arg("chmod", &args, 0)?)?;
    let mode = want_int(arg("chmod", &args, 1)?)? as u32;
    sys::current().fchmodat(Fd::CWD, &p, mode, AtFlags::empty()).map_err(|e| os_error(e, Some(&shown(&p))))?;
    Ok(Value::None)
}

/// `chown(path, uid, gid)` e `lchown`: `-1` deixa o dono ou o grupo como está.
fn chown_at(fname: &'static str, args: Vec<Value>, kw: Kw, flags: AtFlags) -> PyResult<Value> {
    no_kwargs(fname, &kw)?;
    let p = path_bytes(fname, arg(fname, &args, 0)?)?;
    let id = |i: usize| -> PyResult<Option<u32>> {
        let n = want_int(arg(fname, &args, i)?)?;
        Ok(if n < 0 { None } else { Some(n as u32) })
    };
    let (uid, gid) = (id(1)?, id(2)?);
    sys::current().fchownat(Fd::CWD, &p, uid, gid, flags).map_err(|e| os_error(e, Some(&shown(&p))))?;
    Ok(Value::None)
}

fn chown(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    chown_at("chown", args, kw, AtFlags::empty())
}

fn lchown(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    chown_at("lchown", args, kw, AtFlags::SYMLINK_NOFOLLOW)
}

/// `access(path, mode)` como booleano.
fn access(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("access", &kw)?;
    let p = path_bytes("access", arg("access", &args, 0)?)?;
    let mode = args.get(1).map_or(Ok(0), want_int)? as u32;
    let m = sysabi::AccessMode::from_bits_truncate(mode);
    Ok(Value::Bool(sys::current().faccessat(Fd::CWD, &p, m, AtFlags::empty()).is_ok()))
}

fn getenv(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("getenv", &kw)?;
    let name = path_bytes("getenv", arg("getenv", &args, 0)?)?;
    Ok(match sys::current().getenv(&name) {
        Some(v) => Value::str(shown(&v)),
        None => Value::None,
    })
}

/// O ambiente inteiro como lista de pares `(nome, valor)`.
fn environ(_vm: &mut Vm, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    let items = sys::try_current()
        .map(|s| s.environ())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|kv| {
            let at = kv.iter().position(|&b| b == b'=')?;
            Some(Value::tuple(vec![Value::str(shown(&kv[..at])), Value::str(shown(&kv[at + 1..]))]))
        })
        .collect();
    Ok(Value::list(items))
}

fn setenv(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("putenv", &kw)?;
    let k = path_bytes("putenv", arg("putenv", &args, 0)?)?;
    let v = path_bytes("putenv", arg("putenv", &args, 1)?)?;
    sys::current().setenv(&k, &v).map_err(|e| os_error(e, None))?;
    Ok(Value::None)
}

fn unsetenv(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("unsetenv", &kw)?;
    let k = path_bytes("unsetenv", arg("unsetenv", &args, 0)?)?;
    sys::current().unsetenv(&k).map_err(|e| os_error(e, None))?;
    Ok(Value::None)
}

/// `open(path, flags, mode)` cru: devolve o descritor.
fn open(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("open", &kw)?;
    let p = path_bytes("open", arg("open", &args, 0)?)?;
    let flags = want_int(arg("open", &args, 1)?)? as u32;
    let mode = args.get(2).map_or(Ok(0o666), want_int)? as u32;
    let fd = sys::open(&p, OFlags::from_bits_truncate(flags) | OFlags::CLOEXEC, mode)
        .map_err(|e| os_error(e, Some(&shown(&p))))?;
    Ok(Value::Int(i64::from(fd.0)))
}

fn close(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("close", &kw)?;
    let fd = want_int(arg("close", &args, 0)?)? as i32;
    sys::close(Fd(fd)).map_err(|e| os_error(e, None))?;
    Ok(Value::None)
}

/// `read(fd, n)`: até `n` bytes (`n < 0` lê até o fim).
fn read(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("read", &kw)?;
    let fd = Fd(want_int(arg("read", &args, 0)?)? as i32);
    let n = want_int(arg("read", &args, 1)?)?;
    if n < 0 {
        return sys::read_to_end(fd).map(Value::bytes).map_err(|e| os_error(e, None));
    }
    let mut buf = vec![0u8; n as usize];
    let got = sys::read(fd, &mut buf).map_err(|e| os_error(e, None))?;
    buf.truncate(got);
    Ok(Value::bytes(buf))
}

fn write(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("write", &kw)?;
    let fd = Fd(want_int(arg("write", &args, 0)?)? as i32);
    let data: Vec<u8> = match arg("write", &args, 1)? {
        v @ (Value::Bytes(_) | Value::ByteArray(_) | Value::Instance(_)) if v.bytes_like().is_some() => {
            v.bytes_like().map(|b| b.to_vec()).unwrap_or_default()
        }
        Value::Str(s) => s.as_str().as_bytes().to_vec(),
        other => return Err(type_error(format!("a bytes-like object is required, not '{}'", other.type_name()))),
    };
    sys::write_all(fd, &data).map_err(|e| os_error(e, None))?;
    Ok(Value::Int(data.len() as i64))
}

fn lseek(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("lseek", &kw)?;
    let fd = Fd(want_int(arg("lseek", &args, 0)?)? as i32);
    let off = want_int(arg("lseek", &args, 1)?)?;
    let whence = match want_int(arg("lseek", &args, 2)?)? {
        0 => Whence::Set,
        1 => Whence::Cur,
        _ => Whence::End,
    };
    let pos = sys::current().lseek(fd, off, whence).map_err(|e| os_error(e, None))?;
    Ok(Value::Int(pos as i64))
}

fn isatty(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("isatty", &kw)?;
    let fd = Fd(want_int(arg("isatty", &args, 0)?)? as i32);
    Ok(Value::Bool(sys::current().isatty(fd)))
}

fn getpid(_vm: &mut Vm, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    Ok(Value::Int(i64::from(sys::current().getpid())))
}

fn getppid(_vm: &mut Vm, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    Ok(Value::Int(i64::from(sys::current().getppid())))
}

fn ftruncate(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("ftruncate", &kw)?;
    let fd = Fd(want_int(arg("ftruncate", &args, 0)?)? as i32);
    let n = want_int(arg("ftruncate", &args, 1)?)?;
    sys::current().ftruncate(fd, n as u64).map_err(|e| os_error(e, None))?;
    Ok(Value::None)
}

/// `clock(kind)` devolve `(segundos, nanossegundos)`; `kind`: 0 real, 1 monotônico, 2 CPU do processo.
fn clock(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("clock", &kw)?;
    let which = match want_int(arg("clock", &args, 0)?)? {
        0 => sysabi::Clock::Realtime,
        1 => sysabi::Clock::Monotonic,
        _ => sysabi::Clock::ProcessCpuTime,
    };
    let Some(process) = sys::try_current() else {
        // Sem pseudo-processo (testes unitários): o relógio do hospedeiro.
        static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        let d = match which {
            sysabi::Clock::Realtime => {
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default()
            }
            _ => START.get_or_init(std::time::Instant::now).elapsed(),
        };
        return Ok(Value::tuple(vec![Value::Int(d.as_secs() as i64), Value::Int(i64::from(d.subsec_nanos()))]));
    };
    let t = process.clock_gettime(which).map_err(|e| os_error(e, None))?;
    Ok(Value::tuple(vec![Value::Int(t.sec), Value::Int(i64::from(t.nsec))]))
}

fn sleep(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("sleep", &kw)?;
    let secs = match arg("sleep", &args, 0)? {
        Value::Float(f) => *f,
        other => want_int(other)? as f64,
    };
    if secs < 0.0 {
        return Err(exc("ValueError", "sleep length must be non-negative"));
    }
    // Dorme em fatias até o prazo do `signal.alarm`, para o tratador rodar na hora e o resto do sono continuar.
    let total = (secs * 1e9) as i64;
    let started = crate::vm::monotonic_ns();
    loop {
        let (Some(t0), Some(now)) = (started, crate::vm::monotonic_ns()) else { break };
        let left = total - (now - t0);
        if left <= 0 {
            return Ok(Value::None);
        }
        let at = crate::vm::ALARM_AT_NS.load(std::sync::atomic::Ordering::Relaxed);
        let chunk = if at != 0 && at > now && at - now < left { at - now } else { left };
        match sys::current().nanosleep(std::time::Duration::from_nanos(chunk as u64)) {
            Ok(()) | Err(Errno::EINTR) => {}
            Err(e) => return Err(os_error(e, None)),
        }
        vm.deliver_signals()?;
    }
    sys::current().nanosleep(std::time::Duration::from_secs_f64(secs)).map_err(|e| os_error(e, None))?;
    vm.deliver_signals()?;
    Ok(Value::None)
}

/// `_alarm(segundos)`: agenda o SIGALRM (0 cancela) e devolve os segundos que faltavam do alarme anterior.
fn alarm(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("_alarm", &kw)?;
    let secs = want_int(arg("_alarm", &args, 0)?)?;
    let now = crate::vm::monotonic_ns().unwrap_or(0);
    let previous = crate::vm::ALARM_AT_NS.load(std::sync::atomic::Ordering::Relaxed);
    let remaining = if previous > now { (previous - now + 999_999_999) / 1_000_000_000 } else { 0 };
    let at = if secs > 0 { now + secs * 1_000_000_000 } else { 0 };
    crate::vm::ALARM_AT_NS.store(at, std::sync::atomic::Ordering::Relaxed);
    if at != 0 {
        let _ = crate::vm::SIGNAL_THREAD.set(std::thread::current().id());
        crate::vm::SIGNALS_ARMED.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    Ok(Value::Int(remaining))
}

fn urandom(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("urandom", &kw)?;
    let n = want_int(arg("urandom", &args, 0)?)?;
    if n < 0 {
        return Err(exc("ValueError", "negative argument not allowed"));
    }
    let mut buf = vec![0u8; n as usize];
    let mut filled = 0;
    while filled < buf.len() {
        let got = sys::current().getrandom(&mut buf[filled..]).map_err(|e| os_error(e, None))?;
        if got == 0 {
            break;
        }
        filled += got;
    }
    Ok(Value::bytes(buf))
}

/// `utime(path, atime, mtime)`, com segundos (float ou int); `None` nos dois significa agora.
fn utime(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("utime", &kw)?;
    let path = path_bytes("utime", arg("utime", &args, 0)?)?;
    let at = |v: Option<&Value>| -> PyResult<sysabi::SetTime> {
        Ok(match v {
            None | Some(Value::None) => sysabi::SetTime::Now,
            Some(Value::Float(f)) => sysabi::SetTime::At(sysabi::TimeSpec {
                sec: f.floor() as i64,
                nsec: ((f - f.floor()) * 1e9) as u32,
            }),
            Some(other) => sysabi::SetTime::At(sysabi::TimeSpec { sec: want_int(other)?, nsec: 0 }),
        })
    };
    let (a, m) = (at(args.get(1))?, at(args.get(2))?);
    sys::current()
        .utimensat(Fd::CWD, &path, a, m, AtFlags::empty())
        .map_err(|e| os_error(e, Some(&shown(&path))))?;
    Ok(Value::None)
}

fn want_bytes_list(fname: &str, v: &Value) -> PyResult<Vec<Vec<u8>>> {
    let items = match v {
        Value::List(l) => l.borrow().clone(),
        Value::Tuple(t) => t.to_vec(),
        other => return Err(type_error(format!("{fname}: expected a list, not {}", other.type_name()))),
    };
    items.iter().map(|i| path_bytes(fname, i)).collect()
}

/// `spawn(path, argv, env, cwd, dups, closes)` devolve o pid. `env` e `cwd` podem ser `None` (herda);
/// `dups` é lista de `(de, para)` aplicada na ordem e `closes` a lista de fds fechados no filho.
fn spawn(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("spawn", &kw)?;
    let path = path_bytes("spawn", arg("spawn", &args, 0)?)?;
    let argv = want_bytes_list("spawn", arg("spawn", &args, 1)?)?;
    let env = match arg("spawn", &args, 2)? {
        Value::None => None,
        v => Some(want_bytes_list("spawn", v)?),
    };
    let cwd = match arg("spawn", &args, 3)? {
        Value::None => None,
        v => Some(path_bytes("spawn", v)?),
    };
    let mut fd_actions = Vec::new();
    if let Value::List(l) = arg("spawn", &args, 4)? {
        for pair in l.borrow().iter() {
            let Value::Tuple(t) = pair else { return Err(type_error("spawn: dups must be (from, to) pairs")) };
            fd_actions.push(sysabi::FdAction::Dup2 {
                from: Fd(want_int(&t[0])? as i32),
                to: Fd(want_int(&t[1])? as i32),
            });
        }
    }
    if let Value::List(l) = arg("spawn", &args, 5)? {
        for fd in l.borrow().iter() {
            fd_actions.push(sysabi::FdAction::Close(Fd(want_int(fd)? as i32)));
        }
    }
    let spec = sysabi::SpawnSpec {
        path: path.clone(),
        argv,
        attrs: sysabi::ProcAttrs { env, cwd, fd_actions, ..sysabi::ProcAttrs::default() },
    };
    let pid = sys::current().spawn(spec).map_err(|e| os_error(e, Some(&shown(&path))))?;
    Ok(Value::Int(i64::from(pid)))
}

/// `wait(pid, nohang)` devolve `(pid, código)` (código negativo = morto pelo sinal) ou `None` se
/// `nohang` e o filho ainda roda.
fn wait(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("wait", &kw)?;
    let pid = want_int(arg("wait", &args, 0)?)? as i32;
    let nohang = matches!(args.get(1), Some(Value::Bool(true)));
    let opts = if nohang { sysabi::WaitOptions::NOHANG } else { sysabi::WaitOptions::empty() };
    let target = if pid < 0 { sysabi::WaitTarget::Any } else { sysabi::WaitTarget::Pid(pid) };
    loop {
        match sys::current().wait4(target, opts) {
            Ok(None) => return Ok(Value::None),
            Ok(Some((p, st))) => {
                let code = match st {
                    sysabi::WaitStatus::Exited(c) => i64::from(c),
                    sysabi::WaitStatus::Signaled { signal, .. } => -i64::from(signal.0),
                    _ => continue,
                };
                return Ok(Value::tuple(vec![Value::Int(i64::from(p)), Value::Int(code)]));
            }
            // Sinal capturado no meio da espera: roda o tratador e espera de novo (PEP 475).
            Err(Errno::EINTR) => vm.deliver_signals()?,
            Err(e) => return Err(os_error(e, None)),
        }
    }
}

fn pipe(_vm: &mut Vm, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    let (r, w) = sys::current().pipe2(OFlags::CLOEXEC).map_err(|e| os_error(e, None))?;
    Ok(Value::tuple(vec![Value::Int(i64::from(r.0)), Value::Int(i64::from(w.0))]))
}

/// `os.set_blocking(fd, blocking)`: liga ou desliga `O_NONBLOCK` no descritor.
fn set_blocking(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("set_blocking", &kw)?;
    let fd = want_int(arg("set_blocking", &args, 0)?)? as i32;
    let blocking = arg("set_blocking", &args, 1)?.is_true();
    let flags = if blocking { OFlags::empty() } else { OFlags::NONBLOCK };
    sys::current().set_status_flags(Fd(fd), flags).map_err(|e| os_error(e, None))?;
    Ok(Value::None)
}

fn kill_proc(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("kill", &kw)?;
    let pid = want_int(arg("kill", &args, 0)?)? as i32;
    let sig = want_int(arg("kill", &args, 1)?)? as i32;
    sys::current()
        .kill(sysabi::KillTarget::Pid(pid), sysabi::Signal(sig))
        .map_err(|e| match e {
            Errno::ESRCH => exc("ProcessLookupError", format!("[Errno {}] {}", e.0, e.message())),
            _ => os_error(e, None),
        })?;
    // Sinal para si mesmo: o tratador do programa roda já, como no CPython.
    vm.deliver_signals()?;
    Ok(Value::None)
}

/// `_sigaction(sinal, modo)`: 0 padrão, 1 ignorar, 2 capturar (o programa trata via `signal.signal`).
fn sigaction(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("_sigaction", &kw)?;
    let sig = want_int(arg("_sigaction", &args, 0)?)? as i32;
    let disposition = match want_int(arg("_sigaction", &args, 1)?)? {
        0 => sysabi::SigDisposition::Default,
        1 => sysabi::SigDisposition::Ignore,
        _ => sysabi::SigDisposition::Catch,
    };
    sys::current().sigaction(sysabi::Signal(sig), disposition).map_err(|e| os_error(e, None))?;
    if disposition == sysabi::SigDisposition::Catch {
        let _ = crate::vm::SIGNAL_THREAD.set(std::thread::current().id());
        crate::vm::SIGNALS_ARMED.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    Ok(Value::None)
}

/// `_take_signals()`: os sinais capturados que chegaram desde a última chamada.
fn take_signals(_vm: &mut Vm, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    let sigs = sys::current().take_caught_signals();
    Ok(Value::list(sigs.into_iter().map(|s| Value::Int(i64::from(s.0))).collect()))
}

// ---- TCP de loopback entre processos (o `_net` usa quando o par não está neste interpretador) ----
// Os fds nascem não bloqueantes: quem espera é o `_net`, pelo `tcp_poll`, para não travar as threads
// cooperativas do interpretador.

/// Sem pseudo-processo (o interpretador embutido nos testes) não há kernel: ENOSYS, e o `_net` fica só
/// com a rede dentro do interpretador.
fn need_kernel() -> PyResult<()> {
    if sys::try_current().is_none() {
        return Err(os_error(Errno::ENOSYS, None));
    }
    Ok(())
}

fn want_port(fname: &str, args: &[Value], i: usize) -> PyResult<u16> {
    need_kernel()?;
    u16::try_from(want_int(arg(fname, args, i)?)?).map_err(|_| exc("OverflowError", format!("{fname}(): port must be 0-65535.")))
}

/// `tcp_listen(port, backlog)`: `(fd, porta)`.
fn tcp_listen(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("tcp_listen", &kw)?;
    let port = want_port("tcp_listen", &args, 0)?;
    let backlog = want_int(arg("tcp_listen", &args, 1)?)?.clamp(0, i64::from(u32::MAX)) as u32;
    let (fd, port) = sys::tcp_listen(port, backlog, true, true).map_err(|e| os_error(e, None))?;
    Ok(Value::tuple(vec![Value::Int(i64::from(fd.0)), Value::Int(i64::from(port))]))
}

/// `tcp_accept(fd)`: `(fd, porta do par)`, ou `None` sem conexão pronta.
fn tcp_accept(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("tcp_accept", &kw)?;
    let fd = Fd(want_int(arg("tcp_accept", &args, 0)?)? as i32);
    match sys::tcp_accept(fd, true, true) {
        Ok((fd, peer)) => Ok(Value::tuple(vec![Value::Int(i64::from(fd.0)), Value::Int(i64::from(peer))])),
        Err(Errno::EAGAIN) => Ok(Value::None),
        Err(e) => Err(os_error(e, None)),
    }
}

/// `tcp_connect(port)`: `(fd, porta local)`.
fn tcp_connect(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("tcp_connect", &kw)?;
    let port = want_port("tcp_connect", &args, 0)?;
    let (fd, local) = sys::tcp_connect(port, true, true).map_err(|e| os_error(e, None))?;
    Ok(Value::tuple(vec![Value::Int(i64::from(fd.0)), Value::Int(i64::from(local))]))
}

/// `tcp_shutdown(fd, read, write)`.
fn tcp_shutdown(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("tcp_shutdown", &kw)?;
    let fd = Fd(want_int(arg("tcp_shutdown", &args, 0)?)? as i32);
    let read = arg("tcp_shutdown", &args, 1)?.is_true();
    let write = arg("tcp_shutdown", &args, 2)?.is_true();
    sys::tcp_shutdown(fd, read, write).map_err(|e| os_error(e, None))?;
    Ok(Value::None)
}

/// `tcp_recv(fd, n)`: até `n` bytes (`b''` no fim), ou `None` se ainda não chegou nada.
fn tcp_recv(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("tcp_recv", &kw)?;
    let fd = Fd(want_int(arg("tcp_recv", &args, 0)?)? as i32);
    let n = want_int(arg("tcp_recv", &args, 1)?)?.max(0) as usize;
    let mut buf = vec![0u8; n];
    match sys::read(fd, &mut buf) {
        Ok(got) => {
            buf.truncate(got);
            Ok(Value::bytes(buf))
        }
        Err(Errno::EAGAIN) => Ok(Value::None),
        Err(e) => Err(os_error(e, None)),
    }
}

/// `tcp_send(fd, data)`: escreve tudo. Com o pipe cheio espera o par (em outro processo) esvaziar.
fn tcp_send(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("tcp_send", &kw)?;
    let fd = Fd(want_int(arg("tcp_send", &args, 0)?)? as i32);
    let data = arg("tcp_send", &args, 1)?.bytes_like().map(|b| b.to_vec()).unwrap_or_default();
    let mut off = 0;
    while off < data.len() {
        match sys::write(fd, &data[off..]) {
            Ok(n) => off += n,
            Err(Errno::EAGAIN) => {
                let mut pfd = [sysabi::PollFd { fd, events: sysabi::PollEvents::OUT, revents: sysabi::PollEvents::empty() }];
                sys::current().poll(&mut pfd, None).map_err(|e| os_error(e, None))?;
            }
            Err(e) => return Err(os_error(e, None)),
        }
    }
    Ok(Value::Int(data.len() as i64))
}

/// `tcp_poll(fds, timeout)`: os fds de `fds` prontos para leitura (dado, conexão, EOF), esperando até
/// `timeout` segundos (`None`, sem limite).
fn tcp_poll(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("tcp_poll", &kw)?;
    let list = match arg("tcp_poll", &args, 0)? {
        Value::List(l) => l.borrow().clone(),
        Value::Tuple(t) => t.to_vec(),
        other => return Err(type_error(format!("tcp_poll: expected list, not {}", other.type_name()))),
    };
    let timeout = match arg("tcp_poll", &args, 1)? {
        Value::None => None,
        Value::Float(f) => Some(std::time::Duration::from_secs_f64(f.max(0.0))),
        v => Some(std::time::Duration::from_secs(want_int(&v)?.max(0) as u64)),
    };
    let mut pfds = Vec::with_capacity(list.len());
    for v in &list {
        pfds.push(sysabi::PollFd { fd: Fd(want_int(v)? as i32), events: sysabi::PollEvents::IN, revents: sysabi::PollEvents::empty() });
    }
    sys::current().poll(&mut pfds, timeout).map_err(|e| os_error(e, None))?;
    let ready = pfds.iter().filter(|p| !p.revents.is_empty()).map(|p| Value::Int(i64::from(p.fd.0))).collect();
    Ok(Value::list(ready))
}

pub fn build(_vm: &mut Vm) -> Rc<ModuleObj> {
    ModuleBuilder::new("_os")
        .func("tcp_listen", tcp_listen)
        .func("tcp_accept", tcp_accept)
        .func("tcp_connect", tcp_connect)
        .func("tcp_shutdown", tcp_shutdown)
        .func("tcp_recv", tcp_recv)
        .func("tcp_send", tcp_send)
        .func("tcp_poll", tcp_poll)
        .func("getcwd", getcwd)
        .func("chdir", chdir)
        .func("listdir", listdir)
        .func("scandir", scandir)
        .func("stat", stat)
        .func("fstat", fstat)
        .func("mkdir", mkdir)
        .func("unlink", unlink)
        .func("rmdir", rmdir)
        .func("rename", rename)
        .func("readlink", readlink)
        .func("symlink", symlink)
        .func("chmod", chmod)
        .func("chown", chown)
        .func("lchown", lchown)
        .func("access", access)
        .func("getenv", getenv)
        .func("environ", environ)
        .func("putenv", setenv)
        .func("unsetenv", unsetenv)
        .func("open", open)
        .func("close", close)
        .func("read", read)
        .func("write", write)
        .func("lseek", lseek)
        .func("set_blocking", set_blocking)
        .func("isatty", isatty)
        .func("getpid", getpid)
        .func("getppid", getppid)
        .func("ftruncate", ftruncate)
        .func("clock", clock)
        .func("sleep", sleep)
        .func("urandom", urandom)
        .func("utime", utime)
        .func("spawn", spawn)
        .func("wait", wait)
        .func("pipe", pipe)
        .func("kill", kill_proc)
        .func("_sigaction", sigaction)
        .func("_alarm", alarm)
        .func("_take_signals", take_signals)
        .value("O_RDONLY", Value::Int(0))
        .value("O_WRONLY", Value::Int(0o1))
        .value("O_RDWR", Value::Int(0o2))
        .value("O_CREAT", Value::Int(0o100))
        .value("O_EXCL", Value::Int(0o200))
        .value("O_TRUNC", Value::Int(0o1000))
        .value("O_APPEND", Value::Int(0o2000))
        .build()
}
