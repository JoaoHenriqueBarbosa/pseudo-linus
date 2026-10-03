//! `std::env` do processo corrente: argumentos, ambiente e diretório de trabalho.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};

use crate::proc::{self, lock};

pub use std::env::VarError;

pub fn args_os() -> std::vec::IntoIter<OsString> {
    proc::current().args.clone().into_iter()
}

pub fn args() -> std::vec::IntoIter<String> {
    proc::current()
        .args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .into_iter()
}

pub fn var_os<K: AsRef<OsStr>>(key: K) -> Option<OsString> {
    lock(&proc::current().env).get(key.as_ref()).cloned()
}

pub fn var<K: AsRef<OsStr>>(key: K) -> Result<String, VarError> {
    match var_os(key) {
        Some(v) => v.into_string().map_err(VarError::NotUnicode),
        None => Err(VarError::NotPresent),
    }
}

pub fn vars_os() -> std::vec::IntoIter<(OsString, OsString)> {
    lock(&proc::current().env)
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect::<Vec<_>>()
        .into_iter()
}

pub fn vars() -> std::vec::IntoIter<(String, String)> {
    vars_os()
        .map(|(k, v)| (k.to_string_lossy().into_owned(), v.to_string_lossy().into_owned()))
        .collect::<Vec<_>>()
        .into_iter()
}

pub fn snapshot() -> BTreeMap<OsString, OsString> {
    lock(&proc::current().env).clone()
}

pub fn set_var<K: AsRef<OsStr>, V: AsRef<OsStr>>(key: K, value: V) {
    lock(&proc::current().env).insert(key.as_ref().to_os_string(), value.as_ref().to_os_string());
}

pub fn remove_var<K: AsRef<OsStr>>(key: K) {
    lock(&proc::current().env).remove(key.as_ref());
}

pub fn current_dir() -> io::Result<PathBuf> {
    Ok(lock(&proc::current().cwd).clone())
}

pub fn set_current_dir<P: AsRef<Path>>(path: P) -> io::Result<()> {
    let abs = crate::fs::canonicalize(path.as_ref())?;
    if !crate::fs::metadata(&abs)?.is_dir() {
        return Err(crate::errno::err(crate::errno::ENOTDIR));
    }
    *lock(&proc::current().cwd) = abs;
    Ok(())
}

/// Sem `TMPDIR`, o `/tmp` do VFS.
pub fn temp_dir() -> PathBuf {
    var_os("TMPDIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"))
}

pub fn home_dir() -> Option<PathBuf> {
    var_os("HOME").map(PathBuf::from)
}
