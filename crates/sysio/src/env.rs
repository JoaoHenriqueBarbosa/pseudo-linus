//! `std::env` do pseudo-processo: argumentos, ambiente e diretório de trabalho, tudo do kernel do
//! pseudo-linus (o ambiente mora no processo, `setenv`/`unsetenv` são syscalls do `sysabi`).

use std::ffi::{OsStr, OsString};
use std::io;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

pub use std::env::{JoinPathsError, SplitPaths, VarError, consts, join_paths, split_paths};

use crate::errno::cvt;
use crate::proc;

pub fn args_os() -> std::vec::IntoIter<OsString> {
    proc::sys().argv().into_iter().map(OsString::from_vec).collect::<Vec<_>>().into_iter()
}

/// Como o std, entra em pânico se um argumento não for UTF-8.
pub fn args() -> std::vec::IntoIter<String> {
    args_os()
        .map(|a| a.into_string().unwrap_or_else(|a| panic!("argumento não é UTF-8: {a:?}")))
        .collect::<Vec<_>>()
        .into_iter()
}

pub fn var_os<K: AsRef<OsStr>>(key: K) -> Option<OsString> {
    let key = key.as_ref().as_bytes();
    if key.is_empty() || key.contains(&b'=') || key.contains(&0) {
        return None;
    }
    proc::sys().getenv(key).map(OsString::from_vec)
}

pub fn var<K: AsRef<OsStr>>(key: K) -> Result<String, VarError> {
    match var_os(key) {
        Some(v) => v.into_string().map_err(VarError::NotUnicode),
        None => Err(VarError::NotPresent),
    }
}

/// Ambiente na ordem do `environ` do processo.
pub fn vars_os() -> std::vec::IntoIter<(OsString, OsString)> {
    proc::sys()
        .environ()
        .into_iter()
        .filter_map(|kv| {
            // Como o std: uma entrada sem `=` (ou com nome vazio) é ignorada; `=` no primeiro byte
            // faz parte do nome.
            let pos = kv.iter().skip(1).position(|b| *b == b'=')? + 1;
            let (k, v) = kv.split_at(pos);
            Some((OsString::from_vec(k.to_vec()), OsString::from_vec(v[1..].to_vec())))
        })
        .collect::<Vec<_>>()
        .into_iter()
}

/// Como o std, entra em pânico se algum nome ou valor não for UTF-8.
pub fn vars() -> std::vec::IntoIter<(String, String)> {
    vars_os()
        .map(|(k, v)| {
            (
                k.into_string().unwrap_or_else(|k| panic!("variável não é UTF-8: {k:?}")),
                v.into_string().unwrap_or_else(|v| panic!("valor não é UTF-8: {v:?}")),
            )
        })
        .collect::<Vec<_>>()
        .into_iter()
}

/// Entradas cruas `NOME=valor`, exatamente como o kernel guarda (pra `env` e `printenv`).
pub fn environ_raw() -> Vec<Vec<u8>> {
    proc::sys().environ()
}

/// `setenv(3)`. Diferente do std, não é `unsafe`: o ambiente é do pseudo-processo, guardado no
/// kernel, e não há `environ` global compartilhado com outras threads do host.
pub fn set_var<K: AsRef<OsStr>, V: AsRef<OsStr>>(key: K, value: V) {
    let _ = try_set_var(key, value);
}

/// `setenv(3)` com o erro (EINVAL pra nome vazio ou com `=`).
pub fn try_set_var<K: AsRef<OsStr>, V: AsRef<OsStr>>(key: K, value: V) -> io::Result<()> {
    cvt(proc::sys().setenv(key.as_ref().as_bytes(), value.as_ref().as_bytes()))
}

pub fn remove_var<K: AsRef<OsStr>>(key: K) {
    let _ = proc::sys().unsetenv(key.as_ref().as_bytes());
}

pub fn current_dir() -> io::Result<PathBuf> {
    cvt(proc::sys().getcwd()).map(|p| PathBuf::from(OsString::from_vec(p)))
}

pub fn set_current_dir<P: AsRef<Path>>(path: P) -> io::Result<()> {
    cvt(proc::sys().chdir(path.as_ref().as_os_str().as_bytes()))
}

/// `TMPDIR` ou `/tmp`, como o std no Linux.
pub fn temp_dir() -> PathBuf {
    match var_os("TMPDIR") {
        Some(t) if !t.is_empty() => PathBuf::from(t),
        _ => PathBuf::from("/tmp"),
    }
}

/// `HOME`, ou o diretório do usuário no `/etc/passwd` do pseudo-linus.
pub fn home_dir() -> Option<PathBuf> {
    match var_os("HOME") {
        Some(h) if !h.is_empty() => Some(PathBuf::from(h)),
        _ => crate::users::passwd_by_uid(proc::sys().getuid()).map(|p| PathBuf::from(p.dir)),
    }
}

/// O pseudo-processo não tem executável em disco no sentido do host: devolve o `argv[0]`
/// resolvido no `PATH` do processo, que é o arquivo que o kernel executou.
pub fn current_exe() -> io::Result<PathBuf> {
    let argv0 = args_os().next().ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
    let resolved = crate::process::resolve_in_path(argv0.as_bytes(), None).ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
    crate::fs::canonicalize(Path::new(OsStr::from_bytes(&resolved)))
}
