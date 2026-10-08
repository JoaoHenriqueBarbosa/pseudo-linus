//! `_os` (terceira parte): `fcntl(2)`, `flock(2)` e `ioctl(2)` crus, que o módulo `fcntl` em Python usa. O
//! `fcntl.py` faz o que o `fcntlmodule.c` faz em C (a conversão dos argumentos, o buffer de 1024 bytes, a cópia
//! de volta para o `bytearray`); aqui fica só a chamada de sistema, com o laço de `EINTR` do PEP 475 nas que
//! esperam.

use sysabi::{sys, Errno, Fd, Flock, LockCmd, OFlags, Winsize};

use crate::modules::osnative::{arg, os_error, want_fd, OrOs};
use crate::modules::ModuleBuilder;
use crate::native_util::{no_kwargs, want_int};
use crate::object::{Kw, Value};
use crate::vm::{type_error, PyResult, Vm};

const F_DUPFD: i64 = 0;
const F_GETFD: i64 = 1;
const F_SETFD: i64 = 2;
const F_GETFL: i64 = 3;
const F_SETFL: i64 = 4;
const F_GETLK: i64 = 5;
const F_SETLK: i64 = 6;
const F_SETLKW: i64 = 7;
const F_OFD_GETLK: i64 = 36;
const F_OFD_SETLK: i64 = 37;
const F_OFD_SETLKW: i64 = 38;
const F_DUPFD_CLOEXEC: i64 = 1030;
const F_SETPIPE_SZ: i64 = 1031;
const F_GETPIPE_SZ: i64 = 1032;
const FD_CLOEXEC: i64 = 1;

const TIOCGWINSZ: i64 = 0x5413;
const TIOCSWINSZ: i64 = 0x5414;
const FIONREAD: i64 = 0x541B;
const FIONBIO: i64 = 0x5421;

/// O buffer local do `fcntl.ioctl` e do `fcntl.fcntl` do CPython (`IOCTL_BUFSZ`, `FCNTL_BUFSZ`).
const BUF_SIZE: usize = 1024;

fn lock_command(cmd: i64) -> Option<LockCmd> {
    Some(match cmd {
        F_GETLK => LockCmd::Get,
        F_SETLK => LockCmd::Set,
        F_SETLKW => LockCmd::SetWait,
        F_OFD_GETLK => LockCmd::OfdGet,
        F_OFD_SETLK => LockCmd::OfdSet,
        F_OFD_SETLKW => LockCmd::OfdSetWait,
        _ => return None,
    })
}

/// Repete a chamada enquanto um sinal capturado a interrompe: o tratador roda e a espera recomeça (PEP 475).
fn retry<T>(vm: &mut Vm, mut call: impl FnMut() -> Result<T, Errno>) -> PyResult<T> {
    loop {
        match call() {
            Err(Errno::EINTR) => vm.deliver_signals()?,
            other => return other.or_os(None),
        }
    }
}

/// O `fcntl(2)` de comando com argumento inteiro. Um comando de trava com inteiro no lugar do ponteiro é
/// EFAULT (o ponteiro nulo ou solto), depois de o descritor ser conferido; um comando que o kernel não
/// conhece é EINVAL.
fn int_command(fd: Fd, cmd: i64, arg: i64) -> Result<i64, Errno> {
    let p = sys::current();
    // O argumento chega como `unsigned int` (o `"I"` do CPython) e o kernel o lê como `int`.
    let arg = arg as u32 as i32;
    match cmd {
        F_DUPFD | F_DUPFD_CLOEXEC => p.dup_min(fd, Fd(arg), cmd == F_DUPFD_CLOEXEC).map(|new| i64::from(new.0)),
        F_GETFD => Ok(i64::from(p.get_cloexec(fd)?) * FD_CLOEXEC),
        F_SETFD => p.set_cloexec(fd, i64::from(arg) & FD_CLOEXEC != 0).map(|()| 0),
        F_GETFL => Ok(i64::from(p.get_status_flags(fd)?.bits())),
        F_SETFL => p.set_status_flags(fd, OFlags::from_bits_retain(arg as u32)).map(|()| 0),
        F_GETPIPE_SZ => p.pipe_size(fd).map(|n| n as i64),
        F_SETPIPE_SZ => p.set_pipe_size(fd, arg as u32).map(|n| n as i64),
        _ => {
            p.get_cloexec(fd)?;
            Err(if lock_command(cmd).is_some() { Errno::EFAULT } else { Errno::EINVAL })
        }
    }
}

/// `fcntl(fd, cmd, arg)` com `arg` inteiro: o valor que o `fcntl(2)` devolve.
fn fcntl(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("fcntl", &kw)?;
    let fd = want_fd("fcntl", &args, 0)?;
    let cmd = want_int(arg("fcntl", &args, 1)?)?;
    let value = want_int(arg("fcntl", &args, 2)?)?;
    Ok(Value::Int(retry(vm, || int_command(fd, cmd, value))?))
}

fn read_flock(buf: &[u8]) -> Flock {
    let i16_at = |at: usize| i16::from_le_bytes([buf[at], buf[at + 1]]);
    let i64_at = |at: usize| i64::from_le_bytes(buf[at..at + 8].try_into().expect("oito bytes"));
    Flock {
        l_type: i16_at(0),
        whence: i16_at(2),
        start: i64_at(8),
        len: i64_at(16),
        pid: i32::from_le_bytes(buf[24..28].try_into().expect("quatro bytes")),
    }
}

fn write_flock(buf: &mut [u8], f: &Flock) {
    buf[0..2].copy_from_slice(&f.l_type.to_le_bytes());
    buf[2..4].copy_from_slice(&f.whence.to_le_bytes());
    buf[8..16].copy_from_slice(&f.start.to_le_bytes());
    buf[16..24].copy_from_slice(&f.len.to_le_bytes());
    buf[24..28].copy_from_slice(&f.pid.to_le_bytes());
}

/// `fcntl(fd, cmd, buffer)`: os `bytes` de volta. As travas (`F_GETLK`, `F_SETLK`, `F_SETLKW`, `F_OFD_*`) leem
/// e escrevem o `struct flock` no começo do buffer; o resto dos comandos ignora o ponteiro, e o `fcntl.fcntl`
/// devolve o buffer como veio.
fn fcntl_buffer(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("fcntl_buffer", &kw)?;
    let fd = want_fd("fcntl_buffer", &args, 0)?;
    let cmd = want_int(arg("fcntl_buffer", &args, 1)?)?;
    let given = arg("fcntl_buffer", &args, 2)?;
    let data = given.bytes_like().ok_or_else(|| type_error("fcntl_buffer: expected a bytes-like object"))?;
    let mut buf = vec![0u8; BUF_SIZE.max(data.len())];
    buf[..data.len()].copy_from_slice(&data);
    match lock_command(cmd) {
        Some(lock) => {
            let flock = read_flock(&buf);
            let result = retry(vm, || sys::current().fcntl_lock(fd, lock, flock))?;
            write_flock(&mut buf, &result);
        }
        None => {
            retry(vm, || int_command(fd, cmd, 0))?;
        }
    }
    buf.truncate(data.len());
    Ok(Value::bytes(buf))
}

/// `flock(fd, operation)`.
fn flock(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("flock", &kw)?;
    let fd = want_fd("flock", &args, 0)?;
    let op = want_int(arg("flock", &args, 1)?)? as u32;
    retry(vm, || sys::current().flock(fd, op))?;
    Ok(Value::None)
}

fn put_i32(buf: &mut [u8], v: i32) {
    buf[..4].copy_from_slice(&v.to_le_bytes());
}

/// `ioctl(fd, request, buffer ou None)`: os bytes do buffer depois da chamada (vazio quando o argumento é
/// um inteiro). `FIONREAD`, `FIONBIO`, `TIOCGWINSZ` e `TIOCSWINSZ` têm o ponteiro como argumento, então com
/// inteiro dão EFAULT depois do resto das conferências; qualquer outro pedido é ENOTTY.
fn ioctl(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("ioctl", &kw)?;
    let fd = want_fd("ioctl", &args, 0)?;
    let request = want_int(arg("ioctl", &args, 1)?)?;
    let data = match arg("ioctl", &args, 2)? {
        Value::None => None,
        v => Some(v.bytes_like().ok_or_else(|| type_error("ioctl: expected a bytes-like object"))?),
    };
    let len = data.as_ref().map_or(0, |d| d.len());
    let mut buf = vec![0u8; BUF_SIZE.max(len)];
    if let Some(d) = &data {
        buf[..len].copy_from_slice(d);
    }
    let p = sys::current();
    p.get_cloexec(fd).or_os(None)?;
    let efault = || os_error(Errno::EFAULT, None);
    match request {
        FIONREAD => {
            let n = p.fionread(fd).or_os(None)?;
            if data.is_none() {
                return Err(efault());
            }
            put_i32(&mut buf, n as i32);
        }
        FIONBIO => {
            if data.is_none() {
                return Err(efault());
            }
            let on = i32::from_le_bytes(buf[..4].try_into().expect("quatro bytes")) != 0;
            let mut flags = p.get_status_flags(fd).or_os(None)?;
            flags.set(OFlags::NONBLOCK, on);
            p.set_status_flags(fd, flags).or_os(None)?;
        }
        TIOCGWINSZ => {
            let ws = p.tcgetwinsize(fd).or_os(None)?;
            if data.is_none() {
                return Err(efault());
            }
            for (i, v) in [ws.rows, ws.cols, ws.xpixel, ws.ypixel].into_iter().enumerate() {
                buf[i * 2..i * 2 + 2].copy_from_slice(&v.to_le_bytes());
            }
        }
        TIOCSWINSZ => {
            if data.is_none() {
                return Err(efault());
            }
            let at = |i: usize| u16::from_le_bytes([buf[i * 2], buf[i * 2 + 1]]);
            p.tcsetwinsize(fd, Winsize { rows: at(0), cols: at(1), xpixel: at(2), ypixel: at(3) }).or_os(None)?;
        }
        _ => return Err(os_error(Errno::ENOTTY, None)),
    }
    buf.truncate(len);
    Ok(Value::bytes(buf))
}

pub(crate) fn register(builder: ModuleBuilder) -> ModuleBuilder {
    builder
        .func("fcntl", fcntl)
        .func("fcntl_buffer", fcntl_buffer)
        .func("flock", flock)
        .func("ioctl", ioctl)
}
