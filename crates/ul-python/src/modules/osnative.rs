//! `_os`: as chamadas de sistema cruas que os módulos `os`, `io`, `pathlib`, `shutil` e afins (em
//! Python embutido) usam. Tudo passa pelas `sysabi::sys`, ou seja, pelo VFS e pelo pseudo-processo
//! do sandbox. Caminhos aceitam `str` e `bytes`; descritores são `int`.

use std::net::IpAddr;
use std::rc::Rc;

use sysabi::{sys, AtFlags, Errno, Fd, FileType, MsgFlags, OFlags, Whence};

use crate::modules::ModuleBuilder;
use crate::native_util::{no_kwargs, want_int};
use crate::object::{Kw, ModuleObj, Value};
use crate::vm::{exc, type_error, PyException, PyResult, Vm};

/// Um endereço de socket de internet: `(ip, porta)`.
type InetAddr = (IpAddr, u16);

/// O nome da subclasse de `OSError` para um `errno`.
fn error_kind(e: Errno) -> &'static str {
    match e {
        Errno::ENOENT => "FileNotFoundError",
        Errno::EEXIST => "FileExistsError",
        Errno::EISDIR => "IsADirectoryError",
        Errno::ENOTDIR => "NotADirectoryError",
        Errno::EACCES | Errno::EPERM => "PermissionError",
        Errno::EAGAIN => "BlockingIOError",
        Errno::EPIPE => "BrokenPipeError",
        Errno::ECHILD => "ChildProcessError",
        Errno::ESRCH => "ProcessLookupError",
        Errno::EINTR => "InterruptedError",
        Errno::ETIMEDOUT => "TimeoutError",
        Errno::ECONNABORTED => "ConnectionAbortedError",
        Errno::ECONNREFUSED => "ConnectionRefusedError",
        Errno::ECONNRESET => "ConnectionResetError",
        _ => "OSError",
    }
}

/// `OSError` (ou a subclasse certa) para um `errno`, com a mensagem do `strerror`.
pub(crate) fn os_error(e: Errno, path: Option<&str>) -> PyException {
    let msg = match path {
        Some(p) => format!("[Errno {}] {}: '{p}'", e.0, e.message()),
        None => format!("[Errno {}] {}", e.0, e.message()),
    };
    exc(error_kind(e), msg)
}

/// O erro de uma chamada ao sistema como `OSError`, com o caminho envolvido quando há um.
pub(crate) trait OrOs<T> {
    fn or_os(self, path: Option<&[u8]>) -> PyResult<T>;
}

impl<T> OrOs<T> for Result<T, Errno> {
    fn or_os(self, path: Option<&[u8]>) -> PyResult<T> {
        self.map_err(|e| os_error(e, path.map(String::from_utf8_lossy).as_deref()))
    }
}

/// Uma chamada sem resultado vira `None` do Python; o erro, `OSError` com o caminho envolvido.
fn unit_or_os(r: Result<(), Errno>, path: Option<&[u8]>) -> PyResult<Value> {
    r.or_os(path).map(|()| Value::None)
}

/// As chamadas que não bloqueiam (sockets em `O_NONBLOCK`): `EAGAIN` não é erro, vira o valor `again`
/// (`None` quase sempre) e o resultado bom passa por `ok`.
fn or_again<T>(r: Result<T, Errno>, ok: impl FnOnce(T) -> Value, again: Value) -> PyResult<Value> {
    match r {
        Ok(v) => Ok(ok(v)),
        Err(Errno::EAGAIN) => Ok(again),
        Err(e) => Err(os_error(e, None)),
    }
}

/// O caminho do argumento `v` (o posicional `i` de `fname`, que falta como `TypeError`): `str` ou
/// `bytes`.
pub(crate) fn path_bytes(fname: &str, v: Option<&Value>, i: usize) -> PyResult<Vec<u8>> {
    match v {
        Some(Value::Str(s)) => crate::textcodec::encode_utf8(s.as_str(), "surrogateescape"),
        Some(Value::Bytes(b)) => Ok(b.to_vec()),
        Some(other) => Err(type_error(format!("{fname}: path should be string, bytes or os.PathLike, not {}", other.type_name()))),
        None => Err(missing(fname, i)),
    }
}

fn missing(fname: &str, i: usize) -> PyException {
    type_error(format!("{fname}() missing required argument (pos {})", i + 1))
}

/// Bytes do sistema (nome, caminho, ambiente) como `str` do Python: o `os.fsdecode`, UTF-8 com
/// `surrogateescape`.
fn os_str(p: &[u8]) -> Value {
    Value::str(crate::methods::bytesm::decode_utf8(p, "surrogateescape").unwrap_or_else(|_| String::from_utf8_lossy(p).into_owned()))
}

/// Nome de entrada de diretório: `bytes` quando o caminho listado era `bytes`, `str` senão.
fn dir_name(name: &[u8], as_bytes: bool) -> Value {
    if as_bytes {
        Value::bytes(name)
    } else {
        os_str(name)
    }
}

pub(crate) fn arg<'a>(fname: &str, args: &'a [Value], i: usize) -> PyResult<&'a Value> {
    args.get(i).ok_or_else(|| missing(fname, i))
}

/// Os itens de uma lista ou tupla; qualquer outro valor é `None`, e quem chama escolhe a mensagem do `TypeError`.
fn seq_items(v: &Value) -> Option<Vec<Value>> {
    match v {
        Value::List(l) => Some(l.borrow().clone()),
        Value::Tuple(t) => Some(t.to_vec()),
        _ => None,
    }
}

/// Os argumentos posicionais de uma função nativa, lidos em nome dela (o nome entra nas mensagens de erro).
struct Args<'a> {
    name: &'a str,
    args: &'a [Value],
}

impl<'a> Args<'a> {
    /// O posicional `i`, que falta como `TypeError`.
    fn at(&self, i: usize) -> PyResult<&'a Value> {
        arg(self.name, self.args, i)
    }

    fn int(&self, i: usize) -> PyResult<i64> {
        want_int(self.at(i)?)
    }

    fn int32(&self, i: usize) -> PyResult<i32> {
        Ok(self.int(i)? as i32)
    }

    /// Um tamanho: o negativo vale zero.
    fn size(&self, i: usize) -> PyResult<usize> {
        Ok(self.int(i)?.max(0) as usize)
    }

    fn fd(&self, i: usize) -> PyResult<Fd> {
        Ok(Fd(self.int32(i)?))
    }

    /// O descritor de uma chamada que precisa do kernel do sandbox.
    fn kernel_fd(&self, i: usize) -> PyResult<Fd> {
        want_fd(self.name, self.args, i)
    }

    fn flag(&self, i: usize) -> PyResult<bool> {
        Ok(self.at(i)?.is_true())
    }

    /// Um booleano opcional: ausente é falso.
    fn opt_flag(&self, i: usize) -> bool {
        self.args.get(i).is_some_and(Value::is_true)
    }

    fn path(&self, i: usize) -> PyResult<Vec<u8>> {
        path_bytes(self.name, self.args.get(i), i)
    }

    /// Um objeto `bytes` (ou parecido).
    fn bytes(&self, i: usize) -> PyResult<Vec<u8>> {
        let v = self.at(i)?;
        v.bytes_like().map(|b| b.to_vec()).ok_or_else(|| type_error(format!("{}: expected bytes, not {}", self.name, v.type_name())))
    }

    /// Um `bytes` que também pode faltar ou ser `None`.
    fn bytes_or_none(&self, i: usize) -> PyResult<Option<Vec<u8>>> {
        match self.args.get(i) {
            None | Some(Value::None) => Ok(None),
            Some(_) => self.bytes(i).map(Some),
        }
    }

    /// Os bits de `MSG_*` que o kernel trata; o resto (`MSG_OOB`, `MSG_DONTROUTE`...) não muda nada num fluxo TCP local.
    fn msg_flags(&self, i: usize) -> PyResult<MsgFlags> {
        Ok(MsgFlags::from_bits_truncate(self.int(i)? as u32))
    }

    fn ip(&self, i: usize) -> PyResult<IpAddr> {
        match self.at(i)? {
            Value::Str(s) => s.as_str().split('%').next().unwrap_or("").parse().map_err(|_| os_error(Errno::EINVAL, None)),
            other => Err(type_error(format!("{}: expected str, not {}", self.name, other.type_name()))),
        }
    }

    fn port(&self, i: usize) -> PyResult<u16> {
        need_kernel()?;
        u16::try_from(self.int(i)?).map_err(|_| exc("OverflowError", format!("{}(): port must be 0-65535.", self.name)))
    }
}

/// Define uma função nativa do `_os` (`fn(vm, args, kw)`), com o prólogo que todas repetiam: a política das
/// palavras-chave e a leitura dos posicionais em `Args`. O corpo recebe os nomes que o chamador dá.
///
/// - `strict`: nenhuma palavra-chave é aceita (`TypeError` do CPython);
/// - `lax`: as palavras-chave são ignoradas;
/// - `at`: só `dir_fd` é aceita e chega ao corpo já como `Fd`;
/// - `kw`: o corpo recebe as palavras-chave e as trata.
macro_rules! native {
    ($(#[$meta:meta])* strict $name:ident $py:literal |$vm:ident, $a:ident| $body:block) => {
        $(#[$meta])*
        fn $name($vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
            no_kwargs($py, &kw)?;
            let $a = Args { name: $py, args: &args };
            $body
        }
    };
    ($(#[$meta:meta])* lax $name:ident $py:literal |$vm:ident, $a:ident| $body:block) => {
        $(#[$meta])*
        fn $name($vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
            let $a = Args { name: $py, args: &args };
            $body
        }
    };
    ($(#[$meta:meta])* at $name:ident $py:literal |$vm:ident, $a:ident, $dir_fd:ident| $body:block) => {
        $(#[$meta])*
        fn $name($vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
            let $dir_fd = kw_dir_fd($py, &kw)?;
            let $a = Args { name: $py, args: &args };
            $body
        }
    };
    ($(#[$meta:meta])* kw $name:ident $py:literal |$vm:ident, $a:ident, $kw:ident| $body:block) => {
        $(#[$meta])*
        fn $name($vm: &mut Vm, args: Vec<Value>, $kw: Kw) -> PyResult<Value> {
            let $a = Args { name: $py, args: &args };
            $body
        }
    };
}

/// Só as palavras-chave em `names`, na ordem delas; qualquer outra é `TypeError`.
fn kwopts(fname: &str, kw: &Kw, names: &[&str]) -> PyResult<Vec<Option<Value>>> {
    let mut out = vec![None; names.len()];
    for (k, v) in kw {
        match names.iter().position(|n| n == k) {
            Some(i) => out[i] = Some(v.clone()),
            None => return Err(type_error(format!("{fname}() got an unexpected keyword argument '{k}'"))),
        }
    }
    Ok(out)
}

/// O `dir_fd` das funções `*at`: ausente ou `None` é o diretório corrente.
fn dir_fd_of(v: &Option<Value>) -> PyResult<Fd> {
    match v {
        None | Some(Value::None) => Ok(Fd::CWD),
        Some(v) => Ok(Fd(want_int(v)? as i32)),
    }
}

/// Para as funções cuja única palavra-chave é `dir_fd`.
fn kw_dir_fd(fname: &str, kw: &Kw) -> PyResult<Fd> {
    dir_fd_of(&kwopts(fname, kw, &["dir_fd"])?[0])
}

/// As palavras-chave das funções de dois caminhos (`rename`, `link`): `src_dir_fd`, `dst_dir_fd` e as
/// `extra`, devolvidas na ordem (os dois primeiros já como `Fd`).
fn two_path_opts(fname: &str, kw: &Kw, extra: &[&str]) -> PyResult<(Fd, Fd, Vec<Option<Value>>)> {
    let names: Vec<&str> = ["src_dir_fd", "dst_dir_fd"].into_iter().chain(extra.iter().copied()).collect();
    let o = kwopts(fname, kw, &names)?;
    Ok((dir_fd_of(&o[0])?, dir_fd_of(&o[1])?, o))
}

/// Erro de função com dois caminhos (`link`, `rename`): `[Errno N] msg: 'a' -> 'b'`.
fn os_error2(e: Errno, a: &[u8], b: &[u8]) -> PyException {
    exc(error_kind(e), format!("[Errno {}] {}: '{}' -> '{}'", e.0, e.message(), String::from_utf8_lossy(a), String::from_utf8_lossy(b)))
}

fn two_path_result(r: Result<(), Errno>, a: &[u8], b: &[u8]) -> PyResult<Value> {
    r.map_err(|e| os_error2(e, a, b)).map(|()| Value::None)
}

/// O descritor como `int` do Python.
fn fd_value(fd: Fd) -> Value {
    Value::Int(i64::from(fd.0))
}

/// Lista o diretório aberto em `fd` (o `getdents` de um `opendir(fd)`), sem `.` e `..`.
fn read_dir_fd(fd: Fd) -> Result<Vec<sysabi::DirEntry>, Errno> {
    let sys = sys::current();
    let dup = sys.openat(fd, b".", OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC, 0)?;
    let mut out = Vec::new();
    let res = loop {
        match sys.getdents(dup) {
            Ok(batch) if batch.is_empty() => break Ok(()),
            Ok(batch) => out.extend(batch.into_iter().filter(|e| e.name != b"." && e.name != b"..")),
            Err(e) => break Err(e),
        }
    };
    let _ = sys.close(dup);
    res.map(|()| out)
}

/// O caminho de `listdir`/`scandir`: `str`, `bytes` ou um descritor de diretório.
fn dir_entries(fname: &str, v: Option<&Value>) -> PyResult<Vec<sysabi::DirEntry>> {
    match v {
        Some(Value::Int(fd)) => read_dir_fd(Fd(*fd as i32)).or_os(None),
        Some(v) => {
            let p = path_bytes(fname, Some(v), 0)?;
            sys::read_dir(&p).or_os(Some(&p))
        }
        None => sys::read_dir(b".").or_os(Some(b".")),
    }
}

/// A lista das entradas do diretório do primeiro argumento, cada uma montada por `entry` (que sabe se os
/// nomes saem como `bytes`).
fn dir_listing(a: &Args, entry: impl Fn(sysabi::DirEntry, bool) -> Value) -> PyResult<Value> {
    let entries = dir_entries(a.name, a.args.first())?;
    let as_bytes = matches!(a.args.first(), Some(Value::Bytes(_)));
    Ok(Value::list(entries.into_iter().map(|e| entry(e, as_bytes)).collect()))
}

native! {
    strict getcwd "getcwd" |_vm, _a| {
        let cwd = sys::current().getcwd().or_os(None)?;
        Ok(os_str(&cwd))
    }
}

native! {
    strict chdir "chdir" |_vm, a| {
        let p = a.path(0)?;
        unit_or_os(sys::current().chdir(&p), Some(&p))
    }
}

native! {
    strict listdir "listdir" |_vm, a| {
        dir_listing(&a, |e, as_bytes| dir_name(&e.name, as_bytes))
    }
}

native! {
    /// `scandir`: lista de `(nome, tipo)` onde tipo é `"f"`, `"d"`, `"l"` ou `"o"`.
    strict scandir "scandir" |_vm, a| {
        dir_listing(&a, |e, as_bytes| {
            let kind = match e.kind {
                FileType::Regular => "f",
                FileType::Directory => "d",
                FileType::Symlink => "l",
                _ => "o",
            };
            Value::tuple(vec![dir_name(&e.name, as_bytes), Value::str(kind)])
        })
    }
}

fn stat_tuple(st: &sysabi::Stat) -> Value {
    let t = |ts: &sysabi::TimeSpec| Value::Float(ts.sec as f64 + f64::from(ts.nsec) / 1e9);
    // Os `st_*time_ns` exatos: o `float` dos segundos perde os nanossegundos.
    let ns = |ts: &sysabi::TimeSpec| Value::Int(ts.sec * 1_000_000_000 + i64::from(ts.nsec));
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
        ns(&st.atime),
        ns(&st.mtime),
        ns(&st.ctime),
        Value::Int(st.blksize as i64),
        Value::Int(st.blocks as i64),
        Value::Int(st.rdev as i64),
    ])
}

native! {
    /// `stat(path)` como tupla `(mode, ino, dev, nlink, uid, gid, size, atime, mtime, ctime, atime_ns, mtime_ns,
    /// ctime_ns, blksize, blocks, rdev)`.
    at stat "stat" |_vm, a, dir_fd| {
        if let Value::Int(fd) = a.at(0)? {
            return sys::current().fstat(Fd(*fd as i32)).map(|s| stat_tuple(&s)).or_os(None);
        }
        let p = a.path(0)?;
        let follow = a.args.get(1).is_none_or(Value::is_true);
        let flags = if follow { AtFlags::empty() } else { AtFlags::SYMLINK_NOFOLLOW };
        let st = sys::current().fstatat(dir_fd, &p, flags);
        st.map(|s| stat_tuple(&s)).or_os(Some(&p))
    }
}

native! {
    strict fstat "fstat" |_vm, a| {
        let fd = a.fd(0)?;
        sys::current().fstat(fd).map(|s| stat_tuple(&s)).or_os(None)
    }
}

native! {
    at mkdir "mkdir" |_vm, a, dir_fd| {
        let p = a.path(0)?;
        let mode = a.args.get(1).map_or(Ok(0o777), want_int)? as u32;
        unit_or_os(sys::current().mkdirat(dir_fd, &p, mode), Some(&p))
    }
}

/// `unlink` e `rmdir`: o `unlinkat` do caminho do primeiro argumento, com `flags` dizendo qual dos dois.
fn remove_at(a: &Args, dir_fd: Fd, flags: AtFlags) -> PyResult<Value> {
    let p = a.path(0)?;
    unit_or_os(sys::current().unlinkat(dir_fd, &p, flags), Some(&p))
}

native! {
    at unlink "unlink" |_vm, a, dir_fd| {
        remove_at(&a, dir_fd, AtFlags::empty())
    }
}

native! {
    at rmdir "rmdir" |_vm, a, dir_fd| {
        remove_at(&a, dir_fd, AtFlags::REMOVEDIR)
    }
}

native! {
    kw rename "rename" |_vm, a, kw| {
        let (sd, dd, _) = two_path_opts("rename", &kw, &[])?;
        let (src, dst) = (a.path(0)?, a.path(1)?);
        two_path_result(sys::current().renameat2(sd, &src, dd, &dst, sysabi::RenameFlags::empty()), &src, &dst)
    }
}

native! {
    /// `link(src, dst, src_dir_fd=, dst_dir_fd=, follow_symlinks=)`.
    kw link "link" |_vm, a, kw| {
        let (sd, dd, o) = two_path_opts("link", &kw, &["follow_symlinks"])?;
        let flags = if o[2].as_ref().is_some_and(Value::is_true) { AtFlags::SYMLINK_FOLLOW } else { AtFlags::empty() };
        let (src, dst) = (a.path(0)?, a.path(1)?);
        two_path_result(sys::current().linkat(sd, &src, dd, &dst, flags), &src, &dst)
    }
}

native! {
    /// `mknod(path, mode, device, dir_fd=)`; o `mkfifo` do `os` passa `S_IFIFO` no modo.
    at mknod "mknod" |_vm, a, dir_fd| {
        let p = a.path(0)?;
        let mode = a.args.get(1).map_or(Ok(0o600), want_int)? as u32;
        let dev = a.args.get(2).map_or(Ok(0), want_int)? as u64;
        unit_or_os(sys::current().mknodat(dir_fd, &p, mode, dev), Some(&p))
    }
}

native! {
    at readlink "readlink" |_vm, a, dir_fd| {
        let p = a.path(0)?;
        let t = sys::current().readlinkat(dir_fd, &p).or_os(Some(&p))?;
        Ok(os_str(&t))
    }
}

native! {
    at symlink "symlink" |_vm, a, dir_fd| {
        let target = a.path(0)?;
        let p = a.path(1)?;
        unit_or_os(sys::current().symlinkat(&target, dir_fd, &p), Some(&p))
    }
}

native! {
    at chmod "chmod" |_vm, a, dir_fd| {
        let p = a.path(0)?;
        let mode = a.int(1)? as u32;
        unit_or_os(sys::current().fchmodat(dir_fd, &p, mode, AtFlags::empty()), Some(&p))
    }
}

/// `chown(path, uid, gid)` e `lchown`: `-1` deixa o dono ou o grupo como está.
fn chown_at(a: &Args, dir_fd: Fd, flags: AtFlags) -> PyResult<Value> {
    let p = a.path(0)?;
    let id = |i: usize| -> PyResult<Option<u32>> {
        let n = a.int(i)?;
        Ok(if n < 0 { None } else { Some(n as u32) })
    };
    let (uid, gid) = (id(1)?, id(2)?);
    unit_or_os(sys::current().fchownat(dir_fd, &p, uid, gid, flags), Some(&p))
}

native! {
    at chown "chown" |_vm, a, dir_fd| {
        chown_at(&a, dir_fd, AtFlags::empty())
    }
}

native! {
    at lchown "lchown" |_vm, a, dir_fd| {
        chown_at(&a, dir_fd, AtFlags::SYMLINK_NOFOLLOW)
    }
}

native! {
    /// `access(path, mode)` como booleano.
    at access "access" |_vm, a, dir_fd| {
        let p = a.path(0)?;
        let mode = a.args.get(1).map_or(Ok(0), want_int)? as u32;
        let m = sysabi::AccessMode::from_bits_truncate(mode);
        Ok(Value::Bool(sys::current().faccessat(dir_fd, &p, m, AtFlags::empty()).is_ok()))
    }
}

native! {
    strict getenv "getenv" |_vm, a| {
        let name = a.path(0)?;
        Ok(match sys::current().getenv(&name) {
            Some(v) => os_str(&v),
            None => Value::None,
        })
    }
}

native! {
    /// O ambiente inteiro como lista de pares `(nome, valor)`.
    lax environ "environ" |_vm, _a| {
        let items = sys::try_current()
            .map(|s| s.environ())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|kv| {
                let at = kv.iter().position(|&b| b == b'=')?;
                Some(Value::tuple(vec![os_str(&kv[..at]), os_str(&kv[at + 1..])]))
            })
            .collect();
        Ok(Value::list(items))
    }
}

native! {
    strict setenv "putenv" |_vm, a| {
        let k = a.path(0)?;
        let v = a.path(1)?;
        unit_or_os(sys::current().setenv(&k, &v), None)
    }
}

native! {
    strict unsetenv "unsetenv" |_vm, a| {
        let k = a.path(0)?;
        unit_or_os(sys::current().unsetenv(&k), None)
    }
}

native! {
    /// `open(path, flags, mode)` cru: devolve o descritor.
    at open "open" |_vm, a, dir_fd| {
        let p = a.path(0)?;
        let flags = a.int(1)? as u32;
        let mode = a.args.get(2).map_or(Ok(0o666), want_int)? as u32;
        let fd = sys::current().openat(dir_fd, &p, OFlags::from_bits_truncate(flags) | OFlags::CLOEXEC, mode)
            .or_os(Some(&p))?;
        Ok(fd_value(fd))
    }
}

native! {
    strict close "close" |_vm, a| {
        let fd = a.fd(0)?;
        unit_or_os(sys::close(fd), None)
    }
}

native! {
    /// `read(fd, n)`: até `n` bytes (`n < 0` lê até o fim).
    strict read "read" |_vm, a| {
        let fd = a.fd(0)?;
        let n = a.int(1)?;
        if n < 0 {
            return sys::read_to_end(fd).map(Value::bytes).or_os(None);
        }
        let mut buf = vec![0u8; n as usize];
        let got = sys::read(fd, &mut buf).or_os(None)?;
        buf.truncate(got);
        Ok(Value::bytes(buf))
    }
}

native! {
    strict write "write" |_vm, a| {
        let fd = a.fd(0)?;
        let data: Vec<u8> = match a.at(1)? {
            v @ (Value::Bytes(_) | Value::ByteArray(_) | Value::Instance(_)) if v.bytes_like().is_some() => {
                v.bytes_like().map(|b| b.to_vec()).unwrap_or_default()
            }
            other => return Err(type_error(format!("a bytes-like object is required, not '{}'", other.type_name()))),
        };
        // Um `write(2)` só, como o CPython: a escrita parcial (pipe não bloqueante, disco cheio) devolve o que coube.
        let written = loop {
            match sys::write(fd, &data) {
                Err(Errno::EINTR) => {}
                other => break other.or_os(None)?,
            }
        };
        Ok(Value::Int(written as i64))
    }
}

native! {
    strict lseek "lseek" |_vm, a| {
        let fd = a.fd(0)?;
        let off = a.int(1)?;
        let whence = match a.int(2)? {
            0 => Whence::Set,
            1 => Whence::Cur,
            _ => Whence::End,
        };
        let pos = sys::current().lseek(fd, off, whence).or_os(None)?;
        Ok(Value::Int(pos as i64))
    }
}

native! {
    strict isatty "isatty" |_vm, a| {
        let fd = a.fd(0)?;
        Ok(Value::Bool(sys::current().isatty(fd)))
    }
}

native! {
    /// `uname()` do kernel do sandbox: `(sysname, nodename, release, version, machine)`.
    strict uname "uname" |_vm, _a| {
        let u = sys::current().uname();
        Ok(Value::tuple(
            [&u.sysname, &u.nodename, &u.release, &u.version, &u.machine].iter().map(|f| os_str(f)).collect(),
        ))
    }
}

native! {
    /// `statvfs(path ou fd)`: `(bsize, frsize, blocks, bfree, bavail, files, ffree, favail, flag, namemax, fsid)`.
    strict statvfs "statvfs" |_vm, a| {
        let st = match a.at(0)? {
            Value::Int(fd) => sys::current().fstatfs(Fd(*fd as i32)).or_os(None)?,
            v => {
                let p = path_bytes("statvfs", Some(v), 0)?;
                sys::current().statfs(&p).or_os(Some(&p))?
            }
        };
        let n = |v: u64| Value::Int(v as i64);
        Ok(Value::tuple(vec![
            n(st.bsize),
            n(st.frsize),
            n(st.blocks),
            n(st.bfree),
            n(st.bavail),
            n(st.files),
            n(st.ffree),
            n(st.ffree),
            n(st.flags),
            n(st.namelen),
            n(0),
        ]))
    }
}

native! {
    lax getpid "getpid" |_vm, _a| {
        // Sem pseudo-processo (o interpretador embutido nos testes) não há pid: o `logging` pede um a cada registro.
        Ok(Value::Int(sys::try_current().map_or(1, |s| i64::from(s.getpid()))))
    }
}

native! {
    lax getppid "getppid" |_vm, _a| {
        Ok(Value::Int(i64::from(sys::current().getppid())))
    }
}

/// `-1` do Python é o "não muda" das chamadas `setre*`/`setres*` (`(uid_t) -1`).
fn want_id(fname: &str, args: &[Value], i: usize) -> PyResult<u32> {
    let n = want_int(arg(fname, args, i)?)?;
    match n {
        -1 => Ok(sysabi::ID_UNCHANGED),
        0..=0xffff_fffe => Ok(n as u32),
        n if n < 0 => Err(exc("OverflowError", format!("{} is less than minimum", if fname.contains("gid") || fname.ends_with("groups") { "gid" } else { "uid" }))),
        _ => Err(exc("OverflowError", format!("{} is greater than maximum", if fname.contains("gid") || fname.ends_with("groups") { "gid" } else { "uid" }))),
    }
}

fn ids3(t: (u32, u32, u32)) -> Value {
    Value::tuple(vec![Value::Int(i64::from(t.0)), Value::Int(i64::from(t.1)), Value::Int(i64::from(t.2))])
}

native! {
    /// `getuid`, `geteuid`, `getgid`, `getegid`, `getresuid`, `getresgid`, `getgroups`: as credenciais do
    /// processo, como o kernel as tem.
    lax creds "_creds" |_vm, a| {
        let s = sys::current();
        let which = match a.args.first() {
            Some(Value::Str(w)) => w.as_str().to_string(),
            _ => return Err(type_error("_creds: expected a name")),
        };
        Ok(match which.as_str() {
            "uid" => Value::Int(i64::from(s.getuid())),
            "euid" => Value::Int(i64::from(s.geteuid())),
            "gid" => Value::Int(i64::from(s.getgid())),
            "egid" => Value::Int(i64::from(s.getegid())),
            "resuid" => ids3(s.getresuid()),
            "resgid" => ids3(s.getresgid()),
            "groups" => Value::list(s.getgroups().into_iter().map(|g| Value::Int(i64::from(g))).collect()),
            _ => return Err(type_error("_creds: unknown name")),
        })
    }
}

native! {
    /// `setuid`, `setgid`, `setreuid`, `setregid`, `setresuid`, `setresgid` (e `seteuid`/`setegid`, que
    /// a glibc faz com `setresuid(-1, e, -1)`).
    strict setids "_setids" |_vm, a| {
        let which = match a.args.first() {
            Some(Value::Str(w)) => w.as_str().to_string(),
            _ => return Err(type_error("_setids: expected a name")),
        };
        let rest = &a.args[1..];
        let s = sys::current();
        let u = sysabi::ID_UNCHANGED;
        let r = match which.as_str() {
            "setuid" => s.setuid(want_id("setuid", rest, 0)?),
            "setgid" => s.setgid(want_id("setgid", rest, 0)?),
            "seteuid" => s.setresuid(u, want_id("seteuid", rest, 0)?, u),
            "setegid" => s.setresgid(u, want_id("setegid", rest, 0)?, u),
            "setreuid" => s.setreuid(want_id("setreuid", rest, 0)?, want_id("setreuid", rest, 1)?),
            "setregid" => s.setregid(want_id("setregid", rest, 0)?, want_id("setregid", rest, 1)?),
            "setresuid" => s.setresuid(want_id("setresuid", rest, 0)?, want_id("setresuid", rest, 1)?, want_id("setresuid", rest, 2)?),
            "setresgid" => s.setresgid(want_id("setresgid", rest, 0)?, want_id("setresgid", rest, 1)?, want_id("setresgid", rest, 2)?),
            "setgroups" => {
                let first = arg("setgroups", rest, 0)?;
                let Some(items) = seq_items(first) else {
                    return Err(type_error(format!("setgroups argument must be a sequence, not {}", first.type_name())));
                };
                let mut gids = Vec::with_capacity(items.len());
                for (i, _) in items.iter().enumerate() {
                    gids.push(want_id("setgroups", &items, i)?);
                }
                s.setgroups(&gids)
            }
            _ => return Err(type_error("_setids: unknown name")),
        };
        unit_or_os(r, None)
    }
}

native! {
    /// `os.fsync(fd)` (e `fdatasync`): grava no disco o que o descritor tem pendente.
    strict fsync "fsync" |_vm, a| {
        let fd = a.fd(0)?;
        unit_or_os(sys::current().fsync(fd), None)
    }
}

native! {
    /// `os.umask(mask)`: troca a máscara de criação do processo e devolve a anterior.
    strict umask "umask" |_vm, a| {
        if a.args.len() != 1 {
            return Err(type_error(format!("umask() takes exactly one argument ({} given)", a.args.len())));
        }
        let mask = want_int(&a.args[0])?;
        Ok(Value::Int(i64::from(sys::current().umask((mask as u32) & 0o777))))
    }
}

native! {
    strict ftruncate "ftruncate" |_vm, a| {
        let fd = a.fd(0)?;
        let n = a.int(1)?;
        unit_or_os(sys::current().ftruncate(fd, n as u64), None)
    }
}

native! {
    /// `clock(kind)` devolve `(segundos, nanossegundos)`; `kind`: 0 real, 1 monotônico, 2 CPU do processo.
    strict clock "clock" |_vm, a| {
        let which = match a.int(0)? {
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
        let t = process.clock_gettime(which).or_os(None)?;
        Ok(Value::tuple(vec![Value::Int(t.sec), Value::Int(i64::from(t.nsec))]))
    }
}

native! {
    strict sleep "sleep" |vm, a| {
        let secs = match a.at(0)? {
            Value::Float(f) => *f,
            other => want_int(other)? as f64,
        };
        if secs < 0.0 {
            return Err(exc("ValueError", "sleep length must be non-negative"));
        }
        // Dorme em fatias até o fim: o tratador de um sinal que interrompe o sono roda na hora e o resto continua.
        let total = (secs * 1e9) as i64;
        let started = crate::vm::monotonic_ns();
        loop {
            let (Some(t0), Some(now)) = (started, crate::vm::monotonic_ns()) else { break };
            let left = total - (now - t0);
            if left <= 0 {
                return Ok(Value::None);
            }
            match sys::current().nanosleep(std::time::Duration::from_nanos(left as u64)) {
                Ok(()) | Err(Errno::EINTR) => {}
                Err(e) => return Err(os_error(e, None)),
            }
            vm.deliver_signals()?;
        }
        sys::current().nanosleep(std::time::Duration::from_secs_f64(secs)).or_os(None)?;
        vm.deliver_signals()?;
        Ok(Value::None)
    }
}

native! {
    /// `_alarm(segundos)`: o `alarm(2)` do kernel (0 cancela): devolve os segundos que faltavam do alarme anterior.
    strict alarm "_alarm" |_vm, a| {
        need_kernel()?;
        // O CPython passa o `int` do argumento como `unsigned int` ao `alarm`.
        let seconds = a.int32(0)? as u32;
        let previous = sys::current().alarm(seconds).or_os(None)?;
        crate::vm::arm_signals();
        Ok(Value::Int(i64::from(previous)))
    }
}

native! {
    /// `_setitimer(which, valor_s, valor_us, intervalo_s, intervalo_us)`: o `setitimer(2)`; devolve o que o
    /// relógio tinha como `(valor_s, valor_us, intervalo_s, intervalo_us)`. O erro vira `OSError` (o `signal.py`
    /// o troca por `ItimerError`).
    strict setitimer "_setitimer" |_vm, a| {
        need_kernel()?;
        let which = a.int32(0)?;
        let field = |i| a.int(i);
        let new = sysabi::Itimerval { value_sec: field(1)?, value_usec: field(2)?, interval_sec: field(3)?, interval_usec: field(4)? };
        let old = sys::current().setitimer(which, new).or_os(None)?;
        crate::vm::arm_signals();
        Ok(itimerval_tuple(old))
    }
}

native! {
    /// `_getitimer(which)`: `(valor_s, valor_us, intervalo_s, intervalo_us)` do relógio.
    strict getitimer "_getitimer" |_vm, a| {
        need_kernel()?;
        let which = a.int32(0)?;
        Ok(itimerval_tuple(sys::current().getitimer(which).or_os(None)?))
    }
}

fn itimerval_tuple(t: sysabi::Itimerval) -> Value {
    Value::tuple([t.value_sec, t.value_usec, t.interval_sec, t.interval_usec].map(Value::Int).to_vec())
}

native! {
    strict urandom "urandom" |_vm, a| {
        let n = a.int(0)?;
        if n < 0 {
            return Err(exc("ValueError", "negative argument not allowed"));
        }
        let mut buf = vec![0u8; n as usize];
        let mut filled = 0;
        while filled < buf.len() {
            let got = sys::current().getrandom(&mut buf[filled..]).or_os(None)?;
            if got == 0 {
                break;
            }
            filled += got;
        }
        Ok(Value::bytes(buf))
    }
}

native! {
    /// `utime(path, atime, mtime)`, com segundos (float ou int); `None` nos dois significa agora.
    at utime "utime" |_vm, a, dir_fd| {
        let path = a.path(0)?;
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
        let (atime, mtime) = (at(a.args.get(1))?, at(a.args.get(2))?);
        unit_or_os(sys::current().utimensat(dir_fd, &path, atime, mtime, AtFlags::empty()), Some(&path))
    }
}

native! {
    /// `execve(path ou fd, argv, env)`: troca o programa do pseudo-processo (só volta com erro). `env`
    /// é a lista `NOME=valor` ou `None` (herda). O fd vai como `/proc/self/fd/N`, como o `fexecve` da glibc.
    strict execve "execve" |_vm, a| {
        let path = match a.at(0)? {
            Value::Int(fd) => format!("/proc/self/fd/{fd}").into_bytes(),
            v => path_bytes("execve", Some(v), 0)?,
        };
        let argv = want_bytes_list("execve", a.at(1)?)?;
        let env = match a.args.get(2) {
            None | Some(Value::None) => None,
            Some(v) => Some(want_bytes_list("execve", v)?),
        };
        Err(os_error(sys::current().execve(&path, &argv, env.as_deref()), None))
    }
}

pub(crate) fn want_bytes_list(fname: &str, v: &Value) -> PyResult<Vec<Vec<u8>>> {
    let Some(items) = seq_items(v) else {
        return Err(type_error(format!("{fname}: expected a list, not {}", v.type_name())));
    };
    items.iter().map(|i| path_bytes(fname, Some(i), 0)).collect()
}

native! {
    /// `spawn(path, argv, env, cwd, dups, closes, restore_signals, new_session, pgid)` devolve o pid. `env` e `cwd`
    /// podem ser `None` (herda); `dups` é lista de `(de, para)` aplicada na ordem (`de == para` só tira o
    /// `FD_CLOEXEC`) e `closes` a lista de fds fechados no filho, depois dos `dups`.
    strict spawn "spawn" |_vm, a| {
        let path = a.path(0)?;
        let argv = want_bytes_list("spawn", a.at(1)?)?;
        let env = match a.at(2)? {
            Value::None => None,
            v => Some(want_bytes_list("spawn", v)?),
        };
        let cwd = match a.at(3)? {
            Value::None => None,
            v => Some(path_bytes("spawn", Some(v), 0)?),
        };
        let mut fd_actions = Vec::new();
        if let Value::List(l) = a.at(4)? {
            for pair in l.borrow().iter() {
                let Value::Tuple(t) = pair else { return Err(type_error("spawn: dups must be (from, to) pairs")) };
                fd_actions.push(sysabi::FdAction::Dup2 {
                    from: Fd(want_int(&t[0])? as i32),
                    to: Fd(want_int(&t[1])? as i32),
                });
            }
        }
        if let Value::List(l) = a.at(5)? {
            for fd in l.borrow().iter() {
                fd_actions.push(sysabi::FdAction::Close(Fd(want_int(fd)? as i32)));
            }
        }
        // `restore_signals` (sétimo argumento, verdadeiro por padrão): o filho volta a ter SIGPIPE e SIGXFSZ no
        // padrão, que o interpretador ignora desde a partida.
        let restore = a.args.get(6).is_none_or(|v| !matches!(v, Value::Bool(false)));
        let reset_signals = if restore { vec![sysabi::Signal::SIGPIPE, sysabi::Signal::SIGXFSZ] } else { Vec::new() };
        // Oitavo argumento: `setsid()` no filho. Nono: o `pgid_to_set` de `_posixsubprocess.fork_exec` (`-1` ou
        // `None`: fica no grupo do pai; `0`: grupo novo com o próprio pid; outro valor: entra nesse grupo).
        let new_session = a.opt_flag(7);
        let group = match a.args.get(8) {
            None | Some(Value::None) => sysabi::ProcessGroup::Inherit,
            Some(v) => match want_int(v)? {
                n if n < 0 => sysabi::ProcessGroup::Inherit,
                0 => sysabi::ProcessGroup::New,
                n => sysabi::ProcessGroup::Join(n as i32),
            },
        };
        let spec = sysabi::SpawnSpec {
            path: path.clone(),
            argv,
            attrs: sysabi::ProcAttrs { env, cwd, fd_actions, reset_signals, group, new_session, ..sysabi::ProcAttrs::default() },
        };
        let pid = sys::current().spawn(spec).or_os(Some(&path))?;
        Ok(Value::Int(i64::from(pid)))
    }
}

/// O status cru do `wait(2)` (`WIFEXITED`, `WTERMSIG`, `WIFSTOPPED`...): o código de saída no byte de cima,
/// o sinal que matou (com `0x80` se houve core dump), `0x7f` com o sinal que parou no byte de cima, ou
/// `0xffff` de um filho continuado.
fn raw_wait_status(st: sysabi::WaitStatus) -> i64 {
    match st {
        sysabi::WaitStatus::Exited(c) => i64::from(c & 0xff) << 8,
        sysabi::WaitStatus::Signaled { signal, core_dumped } => i64::from(signal.0) | if core_dumped { 0x80 } else { 0 },
        sysabi::WaitStatus::Stopped(s) => i64::from(s.0) << 8 | 0x7f,
        sysabi::WaitStatus::Continued => 0xffff,
    }
}

/// O `si_code`/`si_status` de um filho no `siginfo_t` do `waitid` (`CLD_*`).
fn wait_info_code(st: sysabi::WaitStatus) -> (i64, i64) {
    match st {
        sysabi::WaitStatus::Exited(c) => (1, i64::from(c & 0xff)),
        sysabi::WaitStatus::Signaled { signal, core_dumped } => (if core_dumped { 3 } else { 2 }, i64::from(signal.0)),
        sysabi::WaitStatus::Stopped(s) => (5, i64::from(s.0)),
        sysabi::WaitStatus::Continued => (6, i64::from(sysabi::Signal::SIGCONT.0)),
    }
}

/// `__WNOTHREAD`, `__WCLONE` e `__WALL`: o `wait4` os aceita e aqui não mudam nada.
const WAIT_EXTENSION_BITS: i64 = 0x2000_0000 | 0x4000_0000 | 0x8000_0000;

/// As opções de uma espera como `WaitOptions`; bit desconhecido é `EINVAL`.
fn wait_options(options: i64) -> PyResult<sysabi::WaitOptions> {
    u32::try_from(options).ok().and_then(sysabi::WaitOptions::from_bits).ok_or_else(|| os_error(Errno::EINVAL, None))
}

/// Espera um filho do kernel: repete a chamada `once` quando um sinal capturado a interrompe, depois de rodar o
/// tratador (PEP 475).
fn wait_retrying<T>(vm: &mut Vm, mut once: impl FnMut() -> Result<T, Errno>) -> PyResult<T> {
    loop {
        match once() {
            Ok(found) => return Ok(found),
            Err(Errno::EINTR) => vm.deliver_signals()?,
            Err(e) => return Err(os_error(e, None)),
        }
    }
}

/// O `wait4(2)` com o `pid` do `waitpid(2)` (`-1` qualquer filho, `0` o grupo do chamador, `< -1` o
/// grupo `-pid`): o filho que mudou, ou `None` se `WNOHANG` e nada mudou.
fn wait_child(vm: &mut Vm, a: &Args) -> PyResult<Option<sysabi::WaitInfo>> {
    let pid = a.int32(0)?;
    let opts = wait_options(a.int(1)? & !WAIT_EXTENSION_BITS)?;
    // Os bits do `waitid` não existem no `wait4`.
    if opts.intersects(sysabi::WaitOptions::EXITED | sysabi::WaitOptions::NOWAIT) {
        return Err(os_error(Errno::EINVAL, None));
    }
    let target = match pid {
        -1 => sysabi::WaitTarget::Any,
        0 => sysabi::WaitTarget::Group(0),
        p if p < -1 => sysabi::WaitTarget::Group(-p),
        p => sysabi::WaitTarget::Pid(p),
    };
    wait_retrying(vm, || sys::current().wait4_info(target, opts))
}

native! {
    /// `wait(pid, options)` devolve `(pid, status cru)` ou `None` se `WNOHANG` e nada mudou.
    strict wait "wait" |vm, a| {
        Ok(match wait_child(vm, &a)? {
            None => Value::None,
            Some(i) => Value::tuple(vec![Value::Int(i64::from(i.pid)), Value::Int(raw_wait_status(i.status))]),
        })
    }
}

/// O uso de CPU de um `Rusage` como `(utime, stime, maxrss)`, os campos que o sandbox mede.
fn rusage_values(ru: &sysabi::Rusage) -> [Value; 3] {
    [Value::Float(ru.utime.as_secs_f64()), Value::Float(ru.stime.as_secs_f64()), Value::Int(ru.maxrss_kib as i64)]
}

native! {
    /// `wait4(pid, options)` devolve `(pid, status cru, utime, stime, maxrss)` do filho colhido, ou `None` se
    /// `WNOHANG` e nada mudou.
    strict wait4 "wait4" |vm, a| {
        Ok(match wait_child(vm, &a)? {
            None => Value::None,
            Some(i) => {
                let [utime, stime, maxrss] = rusage_values(&i.rusage);
                Value::tuple(vec![Value::Int(i64::from(i.pid)), Value::Int(raw_wait_status(i.status)), utime, stime, maxrss])
            }
        })
    }
}

native! {
    /// `child_rusage()` devolve `(utime, stime, maxrss)` dos filhos já colhidos (`RUSAGE_CHILDREN`).
    strict child_rusage "child_rusage" |_vm, _a| {
        let ru = sys::current().getrusage(sysabi::RusageWho::Children).or_os(None)?;
        Ok(Value::tuple(rusage_values(&ru).into()))
    }
}

native! {
    /// `waitid(idtype, id, options)` devolve `(si_pid, si_uid, si_signo, si_status, si_code)` ou `None` se o
    /// `siginfo_t` voltou zerado (`WNOHANG` sem evento).
    strict waitid "waitid" |vm, a| {
        let which = a.int(0)?;
        let id = a.int(1)?;
        let options = a.int(2)?;
        let target = match (which, i32::try_from(id)) {
            (0, _) => sysabi::WaitIdTarget::All,
            (1, Ok(id)) => sysabi::WaitIdTarget::Pid(id),
            (2, Ok(id)) => sysabi::WaitIdTarget::Group(id),
            (3, Ok(id)) => sysabi::WaitIdTarget::Pidfd(Fd(id)),
            _ => return Err(os_error(Errno::EINVAL, None)),
        };
        let opts = wait_options(options)?;
        Ok(match wait_retrying(vm, || sys::current().waitid(target, opts))? {
            None => Value::None,
            Some(info) => {
                let (code, status) = wait_info_code(info.status);
                let sigchld = i64::from(sysabi::Signal::SIGCHLD.0);
                let fields = [i64::from(info.pid), i64::from(info.uid), sigchld, status, code];
                Value::tuple(fields.into_iter().map(Value::Int).collect())
            }
        })
    }
}

native! {
    /// `child_dup2(de, para)`: o `dup2(2)` do filho de `_posixsubprocess.fork_exec`; `de == para` só tira o `FD_CLOEXEC`
    /// (o descritor fica herdável) e o fd novo nunca herda o `FD_CLOEXEC`.
    strict child_dup2 "child_dup2" |_vm, a| {
        let from = a.kernel_fd(0)?;
        let to = a.kernel_fd(1)?;
        if from == to {
            sys::current().set_cloexec(to, false).or_os(None)?;
        } else {
            sys::current().dup3(from, to, false).or_os(None)?;
        }
        Ok(fd_value(to))
    }
}

native! {
    /// `setsid()`: o id da sessão nova.
    strict setsid "setsid" |_vm, _a| {
        need_kernel()?;
        Ok(Value::Int(i64::from(sys::current().setsid().or_os(None)?)))
    }
}

native! {
    /// `setpgid(pid, pgid)`.
    strict setpgid "setpgid" |_vm, a| {
        need_kernel()?;
        let pid = a.int32(0)?;
        let pgid = a.int32(1)?;
        unit_or_os(sys::current().setpgid(pid, pgid), None)
    }
}

native! {
    /// `os.get_blocking(fd)`: `false` quando o descritor está em `O_NONBLOCK`.
    strict get_blocking "get_blocking" |_vm, a| {
        let fd = a.kernel_fd(0)?;
        let flags = sys::current().get_status_flags(fd).or_os(None)?;
        Ok(Value::Bool(!flags.contains(OFlags::NONBLOCK)))
    }
}

native! {
    /// `pidfd_open(pid, flags)`: o fd que fica legível quando o processo termina.
    strict pidfd_open "pidfd_open" |_vm, a| {
        need_kernel()?;
        let pid = a.int32(0)?;
        let flags = a.int(1)? as u32;
        let fd = sys::current().pidfd_open(pid, flags).or_os(None)?;
        Ok(fd_value(fd))
    }
}

native! {
    lax pipe "pipe" |_vm, _a| {
        let (r, w) = sys::current().pipe2(OFlags::CLOEXEC).or_os(None)?;
        Ok(Value::tuple(vec![fd_value(r), fd_value(w)]))
    }
}

native! {
    /// `os.set_blocking(fd, blocking)`: liga ou desliga `O_NONBLOCK` no descritor e deixa as outras flags de status
    /// (`O_APPEND`...) como estão, como o `FIONBIO`.
    strict set_blocking "set_blocking" |_vm, a| {
        let fd = a.kernel_fd(0)?;
        let blocking = a.flag(1)?;
        let process = sys::current();
        let mut flags = process.get_status_flags(fd).or_os(None)?;
        flags.set(OFlags::NONBLOCK, !blocking);
        unit_or_os(process.set_status_flags(fd, flags), None)
    }
}

native! {
    strict kill_proc "kill" |_vm, a| {
        let pid = a.int32(0)?;
        let sig = a.int32(1)?;
        sys::current()
            .kill(sysabi::KillTarget::Pid(pid), sysabi::Signal(sig))
            .map_err(|e| match e {
                Errno::ESRCH => exc("ProcessLookupError", format!("[Errno {}] {}", e.0, e.message())),
                _ => os_error(e, None),
            })?;
        // Sinal para si mesmo: o tratador do programa roda logo depois da chamada, entre duas instruções (o ponto de
        // verificação do CPython), no quadro do laço.
        crate::vm::request_signal_check();
        Ok(Value::None)
    }
}

native! {
    /// `_sigaction(sinal, modo)`: 0 padrão, 1 ignorar, 2 capturar (o programa trata via `signal.signal`).
    strict sigaction "_sigaction" |_vm, a| {
        let sig = a.int32(0)?;
        let disposition = match a.int(1)? {
            0 => sysabi::SigDisposition::Default,
            1 => sysabi::SigDisposition::Ignore,
            _ => sysabi::SigDisposition::Catch,
        };
        sys::current().sigaction(sysabi::Signal(sig), disposition).or_os(None)?;
        if disposition == sysabi::SigDisposition::Catch {
            crate::vm::arm_signals();
        }
        Ok(Value::None)
    }
}

native! {
    /// `_take_signals()`: os sinais capturados que chegaram desde a última chamada.
    lax take_signals "_take_signals" |_vm, _a| {
        let sigs = sys::current().take_caught_signals();
        Ok(Value::list(sigs.into_iter().map(|s| Value::Int(i64::from(s.0))).collect()))
    }
}

// ---- sockets: cada socket do Python é um fd do kernel do sandbox desde a criação ----
// Os fds nascem bloqueantes, como no Linux; o `_socket` liga `O_NONBLOCK` pelo timeout (como o CPython) e
// espera a prontidão pelo `poll` antes de chamar o kernel, para não travar as threads cooperativas.

/// Sem pseudo-processo (o interpretador embutido nos testes) não há kernel: ENOSYS.
pub(crate) fn need_kernel() -> PyResult<()> {
    if sys::try_current().is_none() {
        return Err(os_error(Errno::ENOSYS, None));
    }
    Ok(())
}

/// `tcp_socket(v6)` e `udp_socket(v6)`: o fd de um socket de internet sem endereço.
fn inet_socket(a: &Args, create: fn(bool, bool, bool) -> Result<Fd, Errno>) -> PyResult<Value> {
    need_kernel()?;
    let v6 = a.flag(0)?;
    Ok(fd_value(create(v6, false, true).or_os(None)?))
}

/// `(fd, ip, port)`: os três primeiros argumentos de `bind` e `connect` de socket de internet.
fn inet_endpoint(a: &Args) -> PyResult<(Fd, IpAddr, u16)> {
    Ok((a.kernel_fd(0)?, a.ip(1)?, a.port(2)?))
}

/// `tcp_bind_fd(fd, ip, port, reuse_addr)` e `udp_bind(fd, ip, port, reuse)`: a porta.
fn inet_bind(a: &Args, bind: fn(Fd, IpAddr, u16, bool) -> Result<u16, Errno>) -> PyResult<Value> {
    let (fd, ip, port) = inet_endpoint(a)?;
    let port = bind(fd, ip, port, a.opt_flag(3)).or_os(None)?;
    Ok(Value::Int(i64::from(port)))
}

/// `tcp_connect_fd(fd, ip, port)` e `udp_connect`: levanta o `OSError` do `connect` (EINPROGRESS inclusive).
fn inet_connect(a: &Args, connect: fn(Fd, IpAddr, u16) -> Result<(), Errno>) -> PyResult<Value> {
    let (fd, ip, port) = inet_endpoint(a)?;
    unit_or_os(connect(fd, ip, port), None)
}

/// `tcp_names(fd)` e `udp_names(fd)`: `((ip, porta), (ip, porta) ou None)`.
fn inet_names(a: &Args, names: fn(Fd) -> Result<(InetAddr, Option<InetAddr>), Errno>) -> PyResult<Value> {
    let fd = a.kernel_fd(0)?;
    let (me, peer) = names(fd).or_os(None)?;
    Ok(Value::tuple(vec![addr_value(me), peer.map_or(Value::None, addr_value)]))
}

native! {
    /// `tcp_socket(v6)`: o fd de um socket TCP sem endereço.
    strict tcp_socket "tcp_socket" |_vm, a| {
        inet_socket(&a, sys::tcp_socket)
    }
}

native! {
    /// `tcp_bind_fd(fd, ip, port, reuse_addr)`: a porta.
    strict tcp_bind_fd "tcp_bind_fd" |_vm, a| {
        inet_bind(&a, sys::tcp_bind_fd)
    }
}

native! {
    /// `tcp_connect_fd(fd, ip, port)`: levanta o `OSError` do `connect` (EINPROGRESS inclusive).
    strict tcp_connect_fd "tcp_connect_fd" |_vm, a| {
        inet_connect(&a, sys::tcp_connect_fd)
    }
}

native! {
    /// `tcp_names(fd)`: `((ip, porta), (ip, porta) ou None)`.
    strict tcp_names "tcp_names" |_vm, a| {
        inet_names(&a, sys::tcp_names)
    }
}

native! {
    /// `sock_info(fd)`: `(domínio, tipo, protocolo, em escuta)`.
    strict sock_info "sock_info" |_vm, a| {
        let fd = a.kernel_fd(0)?;
        let (domain, ty, proto, listening) = sys::sock_info(fd).or_os(None)?;
        Ok(Value::tuple(vec![
            Value::Int(i64::from(domain)),
            Value::Int(i64::from(ty)),
            Value::Int(i64::from(proto)),
            Value::Bool(listening),
        ]))
    }
}

native! {
    /// `sock_error(fd)`: o `SO_ERROR`, que a leitura zera.
    strict sock_error "sock_error" |_vm, a| {
        let fd = a.kernel_fd(0)?;
        Ok(Value::Int(i64::from(sys::sock_error(fd).or_os(None)?)))
    }
}

native! {
    /// `sock_setopt(fd, level, name, value)`: guarda o valor (bytes) da opção.
    strict sock_setopt "sock_setopt" |_vm, a| {
        let fd = a.kernel_fd(0)?;
        let level = a.int32(1)?;
        let name = a.int32(2)?;
        let value = a.bytes(3)?;
        unit_or_os(sys::sock_setopt(fd, level, name, &value), None)
    }
}

native! {
    /// `sock_getopt(fd, level, name)`: o valor guardado (bytes), ou `None`.
    strict sock_getopt "sock_getopt" |_vm, a| {
        let fd = a.kernel_fd(0)?;
        let level = a.int32(1)?;
        let name = a.int32(2)?;
        Ok(opt_bytes(sys::sock_getopt(fd, level, name).or_os(None)?))
    }
}

native! {
    /// `sock_recv(fd, n, flags)`: até `n` bytes de um socket de fluxo, com `MSG_PEEK`, `MSG_DONTWAIT` e `MSG_WAITALL`.
    strict sock_recv "sock_recv" |_vm, a| {
        let fd = a.kernel_fd(0)?;
        let n = a.size(1)?;
        let flags = a.msg_flags(2)?;
        Ok(Value::bytes(sys::sock_recv(fd, n, flags).or_os(None)?))
    }
}

native! {
    /// `sock_send(fd, data, flags)`: uma escrita num socket de fluxo, com `MSG_DONTWAIT` e `MSG_NOSIGNAL`.
    strict sock_send "sock_send" |_vm, a| {
        let fd = a.kernel_fd(0)?;
        let data = a.bytes(1)?;
        let flags = a.msg_flags(2)?;
        Ok(Value::Int(sys::sock_send(fd, &data, flags).or_os(None)? as i64))
    }
}

native! {
    /// `write_some(fd, data)`: uma única escrita (parcial ou EAGAIN num fd não bloqueante).
    strict write_some "write_some" |_vm, a| {
        let fd = a.kernel_fd(0)?;
        let data = a.bytes(1)?;
        Ok(Value::Int(sys::write(fd, &data).or_os(None)? as i64))
    }
}

native! {
    /// `_exit(status)`: o `_exit(2)`, sem `atexit`, sem finalizadores e sem descarregar o stdout. O desvio do
    /// kernel (`ExitUnwind`) desempilha a thread do interpretador até o pseudo-processo.
    strict exit_now "_exit" |_vm, a| {
        let status = a.int(0)?;
        need_kernel()?;
        sys::exit(status as i32)
    }
}

native! {
    /// `fork()`: não cria o processo aqui. Pede ao laço de instruções que copie o estado do interpretador e
    /// retome a cópia no filho (`crate::fork`); o resultado (o pid no pai, 0 no filho) entra na pilha como se
    /// esta função o tivesse devolvido.
    strict fork "fork" |_vm, a| {
        if !a.args.is_empty() {
            return Err(type_error(format!("fork() takes no arguments ({} given)", a.args.len())));
        }
        need_kernel()?;
        Err(crate::fork::suspend(crate::fork::SuspendRequest::Fork))
    }
}

native! {
    /// `_fork_settle()`: o pai espera o filho do último `fork` ficar de pé (ver `crate::fork::settle_fork`); o
    /// `os._fork_with_hooks` a chama depois dos ganchos `after_in_parent` e antes do aviso de threads.
    lax fork_settle "_fork_settle" |_vm, _a| {
        crate::fork::settle_fork();
        Ok(Value::None)
    }
}

native! {
    /// `openpty(não_herdável=True)`: `(mestre, escravo)` como o `openpty` da glibc: abre `/dev/ptmx`,
    /// `grantpt`, `unlockpt` e abre o escravo que o `ptsname` aponta, sem terminal de controle. O `os.openpty`
    /// os quer não herdáveis; o `forkpty` fica com os fds como a glibc os abre (herdáveis).
    strict openpty "openpty" |_vm, a| {
        need_kernel()?;
        let mut flags = OFlags::RDWR | OFlags::NOCTTY;
        if a.args.first().map_or(true, Value::is_true) {
            flags |= OFlags::CLOEXEC;
        }
        let master = sys::posix_openpt(flags).or_os(None)?;
        let slave = sys::grantpt(master)
            .and_then(|()| sys::unlockpt(master))
            .and_then(|()| sys::ptsname(master))
            .and_then(|name| sys::open(&name, flags, 0));
        match slave {
            Ok(slave) => Ok(Value::tuple(vec![fd_value(master), fd_value(slave)])),
            Err(e) => {
                let _ = sys::close(master);
                Err(os_error(e, None))
            }
        }
    }
}

native! {
    /// `login_tty(fd)`, o da glibc: sessão nova (a falha do `setsid` não conta), `fd` vira o terminal de
    /// controle e a entrada, a saída e o erro padrão; o `fd` original fecha se passa de 2.
    strict login_tty "login_tty" |_vm, a| {
        let fd = a.kernel_fd(0)?;
        let process = sys::current();
        let _ = process.setsid();
        process.tiocsctty(fd, true).or_os(None)?;
        for target in 0..=2 {
            if fd.0 != target {
                process.dup3(fd, Fd(target), false).or_os(None)?;
            }
        }
        if fd.0 > 2 {
            sys::close(fd).or_os(None)?;
        }
        Ok(Value::None)
    }
}

native! {
    /// `dup(fd)`: o menor fd livre, não herdável (como o `os.dup` e o `_socket.dup`).
    strict dup "dup" |_vm, a| {
        let fd = a.kernel_fd(0)?;
        let new = sys::current().dup_min(fd, Fd(0), true).or_os(None)?;
        Ok(fd_value(new))
    }
}

native! {
    /// `dup2(fd, fd2, inheritable)`, o `os.dup2`: `fd2` vira cópia de `fd` (o `fd2` aberto fecha antes), herdável ou
    /// não (`dup2` ou `dup3` com `O_CLOEXEC`). Com `fd == fd2` e herdável o `dup2` não faz nada além de conferir `fd`;
    /// o `dup3` com os dois iguais é EINVAL, como no CPython.
    strict dup2 "dup2" |_vm, a| {
        let fd = a.kernel_fd(0)?;
        let fd2 = a.kernel_fd(1)?;
        let inheritable = a.flag(2)?;
        let process = sys::current();
        if inheritable && fd == fd2 {
            process.get_cloexec(fd).or_os(None)?;
        } else {
            process.dup3(fd, fd2, !inheritable).or_os(None)?;
        }
        Ok(fd_value(fd2))
    }
}

native! {
    /// `get_inheritable(fd)`: `true` sem `FD_CLOEXEC`.
    strict get_inheritable "get_inheritable" |_vm, a| {
        let fd = a.kernel_fd(0)?;
        Ok(Value::Bool(!sys::current().get_cloexec(fd).or_os(None)?))
    }
}

native! {
    /// `set_inheritable(fd, inheritable)`: tira ou põe o `FD_CLOEXEC`.
    strict set_inheritable "set_inheritable" |_vm, a| {
        let fd = a.kernel_fd(0)?;
        let inheritable = a.flag(1)?;
        unit_or_os(sys::current().set_cloexec(fd, !inheritable), None)
    }
}

/// `tcp_listen_bound(fd, backlog)` e `unix_listen`: põe em escuta o socket de `fd` com a fila do segundo argumento.
fn listen_with(a: &Args, fd: Fd, listen: fn(Fd, u32) -> Result<(), Errno>) -> PyResult<Value> {
    let backlog = a.int(1)?.clamp(0, i64::from(u32::MAX)) as u32;
    unit_or_os(listen(fd, backlog), None)
}

native! {
    /// `tcp_listen_bound(fd, backlog)`: põe em escuta o socket ligado.
    strict tcp_listen_bound "tcp_listen_bound" |_vm, a| {
        let fd = a.fd(0)?;
        listen_with(&a, fd, sys::tcp_listen_bound)
    }
}

native! {
    /// `tcp_accept(fd)`: `(fd, porta do par)`, ou `None` sem conexão pronta.
    strict tcp_accept "tcp_accept" |_vm, a| {
        let fd = a.fd(0)?;
        or_again(sys::tcp_accept(fd, false, true), |(fd, peer)| Value::tuple(vec![fd_value(fd), Value::Int(i64::from(peer))]), Value::None)
    }
}

native! {
    /// `tcp_shutdown(fd, read, write)`.
    strict tcp_shutdown "tcp_shutdown" |_vm, a| {
        let fd = a.fd(0)?;
        let read = a.flag(1)?;
        let write = a.flag(2)?;
        unit_or_os(sys::tcp_shutdown(fd, read, write), None)
    }
}

/// O prazo de uma espera, em segundos (`None`, sem limite).
fn wait_timeout(v: &Value) -> PyResult<Option<std::time::Duration>> {
    Ok(match v {
        Value::None => None,
        Value::Float(f) => Some(std::time::Duration::from_secs_f64(f.max(0.0).min(1.0e15))),
        v => Some(std::time::Duration::from_secs(want_int(v)?.max(0) as u64)),
    })
}

/// A lista de `fds` (ou pares) do primeiro argumento de `poll` e `tcp_poll`.
fn poll_list(a: &Args) -> PyResult<Vec<Value>> {
    let v = a.at(0)?;
    seq_items(v).ok_or_else(|| type_error(format!("{}: expected list, not {}", a.name, v.type_name())))
}

native! {
    /// `tcp_poll(fds, timeout)`: os fds de `fds` prontos para leitura (dado, conexão, EOF), esperando até
    /// `timeout` segundos (`None`, sem limite).
    strict tcp_poll "tcp_poll" |_vm, a| {
        let list = poll_list(&a)?;
        let timeout = wait_timeout(a.at(1)?)?;
        let mut pfds = Vec::with_capacity(list.len());
        for v in &list {
            pfds.push(sysabi::PollFd { fd: Fd(want_int(v)? as i32), events: sysabi::PollEvents::IN, revents: sysabi::PollEvents::empty() });
        }
        sys::current().poll(&mut pfds, timeout).or_os(None)?;
        let ready = pfds.iter().filter(|p| !p.revents.is_empty()).map(|p| fd_value(p.fd)).collect();
        Ok(Value::list(ready))
    }
}

native! {
    /// `poll(fds, timeout)`: o `poll(2)` do kernel. `fds` é uma lista de pares `(fd, events)`; devolve a lista dos
    /// `revents` na mesma ordem. `timeout` em segundos (`None`, sem limite). Fd inválido volta como `POLLNVAL`.
    strict poll "poll" |_vm, a| {
        need_kernel()?;
        let list = poll_list(&a)?;
        let timeout = wait_timeout(a.at(1)?)?;
        let mut pfds = Vec::with_capacity(list.len());
        for v in &list {
            let Value::Tuple(t) = v else { return Err(type_error("poll: expected (fd, events) pairs")) };
            let events = sysabi::PollEvents::from_bits_truncate(want_int(arg("poll", t, 1)?)? as u16);
            pfds.push(sysabi::PollFd { fd: Fd(want_int(arg("poll", t, 0)?)? as i32), events, revents: sysabi::PollEvents::empty() });
        }
        sys::current().poll(&mut pfds, timeout).or_os(None)?;
        Ok(Value::list(pfds.iter().map(|p| Value::Int(i64::from(p.revents.bits()))).collect()))
    }
}

native! {
    /// `epoll_create()`: o fd de um epoll novo, com `O_CLOEXEC` (o `select.epoll` do CPython o cria sempre assim).
    strict epoll_create "epoll_create" |_vm, _a| {
        need_kernel()?;
        let fd = sys::current().epoll_create1(true).or_os(None)?;
        Ok(fd_value(fd))
    }
}

native! {
    /// `epoll_ctl(epfd, op, fd, events)`: o `epoll_ctl(2)` do kernel; o dado do usuário é o próprio `fd`, como no
    /// `select.epoll` do CPython.
    strict epoll_ctl "epoll_ctl" |_vm, a| {
        let epfd = a.kernel_fd(0)?;
        let op = a.int32(1)?;
        let fd = a.kernel_fd(2)?;
        let events = a.int(3)? as u32;
        let event = sysabi::EpollEvent { events, data: u64::from(fd.0 as u32) };
        unit_or_os(sys::current().epoll_ctl(epfd, op, fd, event), None)
    }
}

native! {
    /// `epoll_wait(epfd, maxevents, timeout)`: a lista de pares `(fd, events)` prontos; `timeout` em segundos
    /// (`None`, sem limite).
    strict epoll_wait "epoll_wait" |_vm, a| {
        let epfd = a.kernel_fd(0)?;
        let max = a.size(1)?;
        let timeout = wait_timeout(a.at(2)?)?;
        let ready = sys::current().epoll_wait(epfd, max, timeout).or_os(None)?;
        Ok(Value::list(
            ready.iter().map(|e| Value::tuple(vec![Value::Int(i64::from(e.data as u32 as i32)), Value::Int(i64::from(e.events))])).collect(),
        ))
    }
}

// ---- sockets do domínio Unix (o `_socket` usa para todo `AF_UNIX`) ----
// Nomes vão e voltam em bytes (o `sun_path`); os fds nascem não bloqueantes, e EAGAIN vira `None`.

pub(crate) fn want_fd(fname: &str, args: &[Value], i: usize) -> PyResult<Fd> {
    need_kernel()?;
    Ok(Fd(want_int(arg(fname, args, i)?)? as i32))
}

fn opt_bytes(b: Option<Vec<u8>>) -> Value {
    b.map_or(Value::None, Value::bytes)
}

native! {
    /// `unix_socket(type)`: o fd.
    strict unix_socket "unix_socket" |_vm, a| {
        need_kernel()?;
        let ty = a.int(0)? as u8;
        let fd = sys::unix_socket(ty, false, true).or_os(None)?;
        Ok(fd_value(fd))
    }
}

native! {
    /// `unix_socketpair(type)`: os dois fds.
    strict unix_socketpair "unix_socketpair" |_vm, a| {
        need_kernel()?;
        let ty = a.int(0)? as u8;
        let (x, y) = sys::unix_socketpair(ty, false, true).or_os(None)?;
        Ok(Value::tuple(vec![fd_value(x), fd_value(y)]))
    }
}

native! {
    /// `unix_bind(fd, name)`.
    strict unix_bind "unix_bind" |_vm, a| {
        let fd = a.kernel_fd(0)?;
        let name = a.bytes(1)?;
        unit_or_os(sys::unix_bind(fd, &name), None)
    }
}

native! {
    /// `unix_listen(fd, backlog)`.
    strict unix_listen "unix_listen" |_vm, a| {
        let fd = a.kernel_fd(0)?;
        listen_with(&a, fd, sys::unix_listen)
    }
}

native! {
    /// `unix_accept(fd)`: o fd da conexão, ou `None` sem conexão pronta.
    strict unix_accept "unix_accept" |_vm, a| {
        let fd = a.kernel_fd(0)?;
        or_again(sys::unix_accept(fd, false, true), fd_value, Value::None)
    }
}

native! {
    /// `unix_connect(fd, name)`: `True`, ou `False` se a fila de quem escuta está cheia.
    strict unix_connect "unix_connect" |_vm, a| {
        let fd = a.kernel_fd(0)?;
        let name = a.bytes(1)?;
        or_again(sys::unix_connect(fd, &name), |()| Value::Bool(true), Value::Bool(false))
    }
}

native! {
    /// `unix_names(fd)`: `(nome, nome do par, conectado)`, os nomes em bytes ou `None`.
    strict unix_names "unix_names" |_vm, a| {
        let fd = a.kernel_fd(0)?;
        let (me, peer, connected) = sys::unix_names(fd).or_os(None)?;
        Ok(Value::tuple(vec![opt_bytes(me), opt_bytes(peer), Value::Bool(connected)]))
    }
}

native! {
    /// `unix_sendto(fd, data, name)`: os bytes enviados, ou `None` com a fila do destino cheia.
    strict unix_sendto "unix_sendto" |_vm, a| {
        let fd = a.kernel_fd(0)?;
        let data = a.bytes(1)?;
        let name = a.bytes_or_none(2)?;
        or_again(sys::unix_sendto(fd, &data, name.as_deref()), |n| Value::Int(n as i64), Value::None)
    }
}

native! {
    /// `unix_recvfrom(fd, n, peek)`: `(dados, nome de quem enviou)`, ou `None` se não chegou nada.
    strict unix_recvfrom "unix_recvfrom" |_vm, a| {
        let fd = a.kernel_fd(0)?;
        let n = a.size(1)?;
        let peek = a.opt_flag(2);
        or_again(sys::unix_recvfrom(fd, n, peek), |(data, from)| Value::tuple(vec![Value::bytes(data), opt_bytes(from)]), Value::None)
    }
}

native! {
    /// `unix_sendmsg(fd, dados, nome, controle, flags)`: o `sendmsg` de um socket Unix com o `msg_control` cru
    /// (`SCM_RIGHTS`, `SCM_CREDENTIALS`). Devolve os bytes enviados, ou `None` se bloquearia.
    strict unix_sendmsg "unix_sendmsg" |_vm, a| {
        let fd = a.kernel_fd(0)?;
        let data = a.bytes(1)?;
        let name = a.bytes_or_none(2)?;
        let control = a.bytes(3)?;
        let flags = a.msg_flags(4)?;
        or_again(sys::unix_sendmsg(fd, &data, name.as_deref(), &control, flags), |n| Value::Int(n as i64), Value::None)
    }
}

native! {
    /// `unix_recvmsg(fd, n, tamanho do controle, flags)`: `(dados, nome de quem enviou, controle, flags de saída)`,
    /// ou `None` se não chegou nada. Os descritores recebidos já estão na tabela do processo.
    strict unix_recvmsg "unix_recvmsg" |_vm, a| {
        let fd = a.kernel_fd(0)?;
        let n = a.size(1)?;
        let control_len = a.size(2)?;
        let flags = a.msg_flags(3)?;
        or_again(
            sys::unix_recvmsg(fd, n, control_len, flags),
            |got| {
                Value::tuple(vec![
                    Value::bytes(got.data),
                    opt_bytes(got.name),
                    Value::bytes(got.control),
                    Value::Int(i64::from(got.flags.bits())),
                ])
            },
            Value::None,
        )
    }
}

// ---- UDP de loopback (o `_socket` usa para todo `SOCK_DGRAM` de `AF_INET`/`AF_INET6`) ----

/// Dono de um fd de socket do kernel: quando o último objeto Python que o segura morre, o fd fecha,
/// como o `sock_dealloc` do CPython fecha o socket que ninguém mais referencia. `close()` fecha antes;
/// `detach()` solta o fd sem fechar (para passar a outro dono).
struct FdGuard {
    fd: std::cell::Cell<i32>,
}

impl Drop for FdGuard {
    fn drop(&mut self) {
        let fd = self.fd.replace(-1);
        if fd >= 0 && sys::try_current().is_some() {
            let _ = sys::close(Fd(fd));
        }
    }
}

/// Refaz o dono do fd a partir da imagem do heap. O fd é o mesmo número na tabela herdada do filho.
pub(crate) fn restore_image(_tag: &str, state: &(dyn std::any::Any + Send + Sync), _refs: Vec<Value>) -> Option<Value> {
    let fd = *state.downcast_ref::<i32>()?;
    Some(Value::Ext(Rc::new(FdGuard { fd: std::cell::Cell::new(fd) })))
}

impl crate::object::ExtObject for FdGuard {
    fn type_name(&self) -> &'static str {
        "fdguard"
    }

    fn image(&self) -> Option<crate::object::ExtImage> {
        crate::object::OpaqueImage::image("fd_guard", self.fd.get(), Vec::new())
    }

    fn methods(&self) -> &'static [&'static str] {
        &["close", "detach"]
    }

    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<Result<Value, PyException>> {
        (name == "fd").then(|| Ok(Value::Int(i64::from(self.fd.get()))))
    }

    fn call_method(&self, _vm: &mut Vm, name: &str, _args: Vec<Value>, _kw: Kw) -> Result<Value, PyException> {
        let fd = self.fd.replace(-1);
        match name {
            "close" => {
                if fd >= 0 {
                    sys::close(Fd(fd)).or_os(None)?;
                }
                Ok(Value::None)
            }
            _ => Ok(Value::Int(i64::from(fd))),
        }
    }
}

native! {
    /// `fdguard(fd)`: o dono do fd.
    strict fdguard "fdguard" |_vm, a| {
        let fd = a.int32(0)?;
        Ok(Value::Ext(Rc::new(FdGuard { fd: std::cell::Cell::new(fd) })))
    }
}

fn addr_value((ip, port): InetAddr) -> Value {
    Value::tuple(vec![Value::str(ip.to_string()), Value::Int(i64::from(port))])
}

native! {
    /// `udp_socket(v6)`: o fd.
    strict udp_socket "udp_socket" |_vm, a| {
        inet_socket(&a, sys::udp_socket)
    }
}

native! {
    /// `udp_bind(fd, ip, port, reuse)`: a porta.
    strict udp_bind "udp_bind" |_vm, a| {
        inet_bind(&a, sys::udp_bind)
    }
}

native! {
    /// `udp_connect(fd, ip, port)`.
    strict udp_connect "udp_connect" |_vm, a| {
        inet_connect(&a, sys::udp_connect)
    }
}

native! {
    /// `udp_sendto(fd, data, ip, port)`: sem `ip` (`None`), para o par do `connect`.
    strict udp_sendto "udp_sendto" |_vm, a| {
        let fd = a.kernel_fd(0)?;
        let data = a.bytes(1)?;
        let dst = match a.args.get(2) {
            None | Some(Value::None) => None,
            Some(_) => Some((a.ip(2)?, a.port(3)?)),
        };
        let n = sys::udp_sendto(fd, &data, dst).or_os(None)?;
        Ok(Value::Int(n as i64))
    }
}

native! {
    /// `udp_recvfrom(fd, n, peek)`: `(dados, (ip, porta))`, ou `None` se não chegou nada.
    strict udp_recvfrom "udp_recvfrom" |_vm, a| {
        let fd = a.kernel_fd(0)?;
        let n = a.size(1)?;
        let peek = a.opt_flag(2);
        or_again(sys::udp_recvfrom(fd, n, peek), |(data, ip, port)| Value::tuple(vec![Value::bytes(data), addr_value((ip, port))]), Value::None)
    }
}

native! {
    /// `udp_names(fd)`: `((ip, porta), (ip, porta) ou None)`.
    strict udp_names "udp_names" |_vm, a| {
        inet_names(&a, sys::udp_names)
    }
}

pub fn build(_vm: &mut Vm) -> Rc<ModuleObj> {
    crate::modules::osspawn::register(crate::modules::osfcntl::register(crate::modules::osextra::register(ModuleBuilder::new("_os"))))
        .func("fdguard", fdguard)
        .func("dup", dup)
        .func("dup2", dup2)
        .func("get_inheritable", get_inheritable)
        .func("set_inheritable", set_inheritable)
        .func("write_some", write_some)
        .func("sock_info", sock_info)
        .func("sock_error", sock_error)
        .func("sock_setopt", sock_setopt)
        .func("sock_getopt", sock_getopt)
        .func("sock_recv", sock_recv)
        .func("sock_send", sock_send)
        .func("udp_socket", udp_socket)
        .func("udp_bind", udp_bind)
        .func("udp_connect", udp_connect)
        .func("udp_sendto", udp_sendto)
        .func("udp_recvfrom", udp_recvfrom)
        .func("udp_names", udp_names)
        .func("unix_socket", unix_socket)
        .func("unix_socketpair", unix_socketpair)
        .func("unix_bind", unix_bind)
        .func("unix_listen", unix_listen)
        .func("unix_accept", unix_accept)
        .func("unix_connect", unix_connect)
        .func("unix_names", unix_names)
        .func("unix_sendto", unix_sendto)
        .func("unix_recvfrom", unix_recvfrom)
        .func("unix_sendmsg", unix_sendmsg)
        .func("unix_recvmsg", unix_recvmsg)
        .func("tcp_socket", tcp_socket)
        .func("tcp_bind_fd", tcp_bind_fd)
        .func("tcp_connect_fd", tcp_connect_fd)
        .func("tcp_names", tcp_names)
        .func("tcp_listen_bound", tcp_listen_bound)
        .func("tcp_accept", tcp_accept)
        .func("tcp_shutdown", tcp_shutdown)
        .func("tcp_poll", tcp_poll)
        .func("poll", poll)
        .func("epoll_create", epoll_create)
        .func("epoll_ctl", epoll_ctl)
        .func("epoll_wait", epoll_wait)
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
        .func("link", link)
        .func("mknod", mknod)
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
        .func("uname", uname)
        .func("statvfs", statvfs)
        .func("getppid", getppid)
        .func("_creds", creds)
        .func("_setids", setids)
        .func("umask", umask)
        .func("fsync", fsync)
        .func("ftruncate", ftruncate)
        .func("clock", clock)
        .func("sleep", sleep)
        .func("urandom", urandom)
        .func("utime", utime)
        .func("spawn", spawn)
        .func("execve", execve)
        .func("wait", wait)
        .func("wait4", wait4)
        .func("child_rusage", child_rusage)
        .func("child_dup2", child_dup2)
        .func("setsid", setsid)
        .func("setpgid", setpgid)
        .func("waitid", waitid)
        .func("fork", fork)
        .func("_fork_settle", fork_settle)
        .func("_exit", exit_now)
        .func("openpty", openpty)
        .func("login_tty", login_tty)
        .func("pipe", pipe)
        .func("pidfd_open", pidfd_open)
        .func("get_blocking", get_blocking)
        .func("kill", kill_proc)
        .func("_sigaction", sigaction)
        .func("_alarm", alarm)
        .func("_setitimer", setitimer)
        .func("_getitimer", getitimer)
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
