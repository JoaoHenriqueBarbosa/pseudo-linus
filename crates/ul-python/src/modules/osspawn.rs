//! `_os.posix_spawn`: o `posix_spawn(3)` do glibc sobre o `spawn` do kernel. O `os.posix_spawn` em Python
//! valida e converte os argumentos como o `posixmodule.c` e entrega aqui as ações de arquivo, o grupo, a
//! sessão, os ids e o escalonador já em formato de tupla; esta função só monta o `SpawnSpec`.

use sysabi::{sys, FdAction, OFlags, ProcAttrs, ProcessGroup, SchedParam, Signal, SpawnSched, SpawnSpec, Fd};

use crate::modules::osnative::{arg, need_kernel, path_bytes, want_bytes_list, OrOs};
use crate::modules::ModuleBuilder;
use crate::native_util::{no_kwargs, want_int};
use crate::object::{Kw, Value};
use crate::vm::{type_error, PyResult, Vm};

/// As `POSIX_SPAWN_*` do `os`: a primeira posição da tupla de cada ação.
const OPEN: i64 = 0;
const CLOSE: i64 = 1;
const DUP2: i64 = 2;
const CLOSEFROM: i64 = 3;

fn fd_at(t: &[Value], i: usize) -> PyResult<Fd> {
    Ok(Fd(want_int(t.get(i).ok_or_else(|| type_error("posix_spawn: file action too short"))?)? as i32))
}

fn parse_actions(v: &Value) -> PyResult<Vec<FdAction>> {
    let Value::List(list) = v else { return Err(type_error("posix_spawn: file actions must be a list")) };
    let mut out = Vec::new();
    for item in list.borrow().iter() {
        let Value::Tuple(t) = item else { return Err(type_error("posix_spawn: file action must be a tuple")) };
        out.push(match want_int(&t[0])? {
            OPEN => FdAction::Open {
                fd: fd_at(t, 1)?,
                path: path_bytes("posix_spawn", t.get(2), 2)?,
                flags: OFlags::from_bits_retain(want_int(&t[3])? as u32),
                mode: want_int(&t[4])? as u32,
            },
            CLOSE => FdAction::Close(fd_at(t, 1)?),
            DUP2 => FdAction::Dup2 { from: fd_at(t, 1)?, to: fd_at(t, 2)? },
            CLOSEFROM => FdAction::CloseFrom(fd_at(t, 1)?),
            _ => return Err(type_error("Unknown file_actions identifier")),
        });
    }
    Ok(out)
}

/// `posix_spawn(path, argv, env, actions, pgroup, resetids, setsid, sigdef, scheduler)` devolve o pid.
/// `env` `None` herda o ambiente do processo; `pgroup` é `-1` (fica no grupo), `0` (grupo novo) ou o grupo a
/// entrar; `scheduler` é `None` ou `(política ou None, prioridade)`.
fn posix_spawn(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("posix_spawn", &kw)?;
    need_kernel()?;
    let path = path_bytes("posix_spawn", args.first(), 0)?;
    let argv = want_bytes_list("posix_spawn", arg("posix_spawn", &args, 1)?)?;
    let env = match arg("posix_spawn", &args, 2)? {
        Value::None => None,
        v => Some(want_bytes_list("posix_spawn", v)?),
    };
    let fd_actions = parse_actions(arg("posix_spawn", &args, 3)?)?;
    let group = match want_int(arg("posix_spawn", &args, 4)?)? {
        n if n < 0 => ProcessGroup::Inherit,
        0 => ProcessGroup::New,
        n => ProcessGroup::Join(n as i32),
    };
    let reset_ids = arg("posix_spawn", &args, 5)?.is_true();
    let new_session = arg("posix_spawn", &args, 6)?.is_true();
    let reset_signals = match arg("posix_spawn", &args, 7)? {
        Value::List(l) => l.borrow().iter().map(|s| want_int(s).map(|n| Signal(n as i32))).collect::<PyResult<Vec<_>>>()?,
        _ => return Err(type_error("posix_spawn: setsigdef must be a list")),
    };
    let scheduler = match arg("posix_spawn", &args, 8)? {
        Value::None => None,
        Value::Tuple(t) => Some(SpawnSched {
            policy: match &t[0] {
                Value::None => None,
                v => Some(want_int(v)? as i32),
            },
            param: SchedParam { priority: want_int(&t[1])? as i32 },
        }),
        _ => return Err(type_error("posix_spawn: scheduler must be a tuple")),
    };
    let attrs = ProcAttrs { env, fd_actions, group, new_session, reset_signals, reset_ids, scheduler, ..ProcAttrs::default() };
    let pid = sys::current().spawn(SpawnSpec { path, argv, attrs }).or_os(None)?;
    Ok(Value::Int(i64::from(pid)))
}

pub(crate) fn register(builder: ModuleBuilder) -> ModuleBuilder {
    builder.func("posix_spawn", posix_spawn)
}
