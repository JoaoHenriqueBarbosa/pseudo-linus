//! O `Env` do leveldb sobre o sistema de arquivos do pseudo-processo: o banco do Local Storage grava no perfil do
//! `--user-data-dir` pelas mesmas chamadas de sistema que o resto do navegador. Os arquivos saem com modo `0600` e os
//! diretórios com `0700`, como o Chromium os cria (ele roda com umask 077; medido no oráculo).

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use sysabi::{sys, AtFlags, Errno, Fd, OFlags, RenameFlags};
use ul_common::fsutil::mkdir_p;
use ul_leveldb::env::Env;
use ul_leveldb::{Error, Result};

const FILE_MODE: u32 = 0o600;
const DIR_MODE: u32 = 0o700;

/// O `flock` do `LOCK` de cada banco aberto: o descritor fica aberto enquanto o banco vive.
#[derive(Default)]
pub struct SysEnv {
    locks: Mutex<HashMap<String, Fd>>,
}

fn io_error(path: &str, errno: Errno) -> Error {
    Error::Io(format!("{path}: {errno:?}"))
}

impl SysEnv {
    pub fn new() -> SysEnv {
        SysEnv::default()
    }

    fn open_for_writing(path: &str, extra: OFlags) -> Result<Fd> {
        sys::open(path.as_bytes(), OFlags::WRONLY | OFlags::CREAT | OFlags::CLOEXEC | extra, FILE_MODE).map_err(|errno| io_error(path, errno))
    }
}

impl Env for SysEnv {
    fn create_dir_all(&self, path: &str) -> Result<()> {
        if self.exists(path) {
            return Ok(());
        }
        mkdir_p(path.as_bytes(), DIR_MODE).map_err(|errno| io_error(path, errno))?;
        sys::current().fchmodat(Fd::CWD, path.as_bytes(), DIR_MODE, AtFlags::empty()).map_err(|errno| io_error(path, errno))
    }

    fn exists(&self, path: &str) -> bool {
        sysabi::util::exists(path.as_bytes())
    }

    fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        sys::read_file(path.as_bytes()).map_err(|errno| io_error(path, errno))
    }

    fn write_file(&self, path: &str, data: &[u8]) -> Result<()> {
        sysabi::util::write_file(path.as_bytes(), data, FILE_MODE).map_err(|errno| io_error(path, errno))
    }

    fn append_file(&self, path: &str, data: &[u8]) -> Result<()> {
        let fd = SysEnv::open_for_writing(path, OFlags::APPEND)?;
        let written = sys::write_all(fd, data);
        let closed = sys::close(fd);
        written.and(closed).map_err(|errno| io_error(path, errno))
    }

    fn file_size(&self, path: &str) -> Result<u64> {
        sys::stat(path.as_bytes()).map(|stat| stat.size).map_err(|errno| io_error(path, errno))
    }

    fn rename(&self, from: &str, to: &str) -> Result<()> {
        sys::current().renameat2(Fd::CWD, from.as_bytes(), Fd::CWD, to.as_bytes(), RenameFlags::empty()).map_err(|errno| io_error(from, errno))
    }

    fn remove_file(&self, path: &str) -> Result<()> {
        sys::current().unlinkat(Fd::CWD, path.as_bytes(), AtFlags::empty()).map_err(|errno| io_error(path, errno))
    }

    fn list_dir(&self, path: &str) -> Result<Vec<String>> {
        let entries = sys::read_dir(path.as_bytes()).map_err(|errno| io_error(path, errno))?;
        Ok(entries.into_iter().map(|entry| String::from_utf8_lossy(&entry.name).into_owned()).collect())
    }

    fn lock_file(&self, path: &str) -> Result<()> {
        let fd = sys::open(path.as_bytes(), OFlags::RDWR | OFlags::CREAT | OFlags::CLOEXEC, FILE_MODE).map_err(|errno| io_error(path, errno))?;
        match sys::current().flock(fd, sysabi::fcntl::LOCK_EX | sysabi::fcntl::LOCK_NB) {
            // Um kernel sem `flock` deixa o banco abrir sem trava, como um sistema de arquivos que não a suporta.
            Ok(()) | Err(Errno::ENOSYS) => {}
            Err(errno) => {
                let _ = sys::close(fd);
                return Err(Error::Io(format!("lock {path}: {errno:?}")));
            }
        }
        self.locks.lock().unwrap_or_else(PoisonError::into_inner).insert(path.to_owned(), fd);
        Ok(())
    }

    fn unlock_file(&self, path: &str) {
        let fd = self.locks.lock().unwrap_or_else(PoisonError::into_inner).remove(path);
        // O banco pode ser solto depois de o pseudo-processo sair (num destrutor de thread): o kernel já fechou o
        // descritor e liberou a trava, e não há processo para pedir isso de novo.
        if let (Some(fd), Some(process)) = (fd, sys::try_current()) {
            let _ = process.flock(fd, sysabi::fcntl::LOCK_UN);
            let _ = process.close(fd);
        }
    }

    fn now_micros(&self) -> u64 {
        ul_common::time::realtime_nanos() as u64 / 1000
    }
}
