//! Tabela de descritores e descrições de arquivo aberto (`struct file`).
//!
//! Um fd aponta pra uma [`Ofd`] (open file description); `dup`, `fork` e `F_DUPFD` compartilham a mesma
//! descrição, então o deslocamento e as flags de status (O_APPEND, O_NONBLOCK) são compartilhados.
//! `FD_CLOEXEC` é por fd. A descrição fecha (e solta pipe, inode e travas) quando o último fd que aponta
//! pra ela fecha.

use std::collections::BTreeMap;
use std::sync::{Arc, Weak};
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::Mutex;
use sysabi::{Errno, Fd, FileType, OFlags};
use vfs::{FileHandle, Loc};

use crate::dev::Device;
use crate::pipe::PipeEnd;

static NEXT_OFD: AtomicU64 = AtomicU64::new(1);

/// O objeto aberto.
pub(crate) enum FileObj {
    /// Arquivo regular ou diretório do VFS.
    Vfs { loc: Loc, handle: Box<dyn FileHandle>, kind: FileType },
    /// `O_PATH`: só serve de dirfd, pra `fstat` e `fchdir`.
    Path { loc: Loc, kind: FileType },
    /// Pipe anônimo ou FIFO (com o lugar da FIFO no VFS).
    Pipe { end: PipeEnd, fifo: Option<Loc> },
    Dev { dev: Device, loc: Option<Loc> },
}

impl std::fmt::Debug for FileObj {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileObj::Vfs { loc, kind, .. } => write!(f, "Vfs({loc:?}, {kind:?})"),
            FileObj::Path { loc, .. } => write!(f, "Path({loc:?})"),
            FileObj::Pipe { end, .. } => write!(f, "Pipe(ino {}, r {}, w {})", end.pipe.ino, end.read, end.write),
            FileObj::Dev { dev, .. } => write!(f, "Dev({dev:?})"),
        }
    }
}

/// Estado mutável compartilhado de uma descrição.
#[derive(Debug, Clone, Copy)]
pub(crate) struct OfdState {
    /// Flags de status (O_APPEND, O_NONBLOCK, O_DSYNC, O_NOATIME...), sem o modo de acesso.
    pub status: OFlags,
    pub pos: u64,
    /// Posição no diretório (cookie do readdir).
    pub dir_cookie: u64,
}

/// Uma descrição de arquivo aberto.
pub(crate) struct Ofd {
    /// Dono das travas OFD.
    pub id: u64,
    pub obj: FileObj,
    pub readable: bool,
    pub writable: bool,
    /// O_ACCMODE original (pro F_GETFL).
    pub accmode: u32,
    pub st: Mutex<OfdState>,
    /// Tabela de travas do sandbox (pra soltar as travas desta descrição no último close).
    pub locks: Weak<crate::sandbox::LockTable>,
}

impl std::fmt::Debug for Ofd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Ofd({}, {:?})", self.id, self.obj)
    }
}

impl Drop for Ofd {
    fn drop(&mut self) {
        if let Some(t) = self.locks.upgrade() {
            t.release_owner(self.id);
        }
    }
}

impl Ofd {
    pub(crate) fn new(obj: FileObj, flags: OFlags, locks: Weak<crate::sandbox::LockTable>) -> Arc<Ofd> {
        let accmode = flags.bits() & OFlags::ACCMODE;
        let path_only = matches!(obj, FileObj::Path { .. });
        let (readable, writable) = if path_only {
            (false, false)
        } else {
            match accmode {
                0 => (true, false),
                1 => (false, true),
                2 => (true, true),
                _ => (false, false),
            }
        };
        let keep = OFlags::APPEND | OFlags::NONBLOCK | OFlags::DSYNC | OFlags::NOATIME | OFlags::SYNC | OFlags::PATH;
        Arc::new(Ofd {
            id: NEXT_OFD.fetch_add(1, Ordering::Relaxed),
            obj,
            readable,
            writable,
            accmode,
            st: Mutex::new(OfdState { status: flags & keep, pos: 0, dir_cookie: 0 }),
            locks,
        })
    }

    pub(crate) fn status(&self) -> OFlags {
        self.st.lock().status
    }

    pub(crate) fn nonblock(&self) -> bool {
        self.status().contains(OFlags::NONBLOCK)
    }

    pub(crate) fn loc(&self) -> Option<&Loc> {
        match &self.obj {
            FileObj::Vfs { loc, .. } | FileObj::Path { loc, .. } => Some(loc),
            FileObj::Pipe { fifo, .. } => fifo.as_ref(),
            FileObj::Dev { loc, .. } => loc.as_ref(),
        }
    }

    /// Bits de permissão do link em `/proc/<pid>/fd/N`.
    pub(crate) fn link_perm(&self) -> u32 {
        let mut p = 0;
        if self.readable {
            p |= 0o500;
        }
        if self.writable {
            p |= 0o300;
        }
        if matches!(self.obj, FileObj::Path { .. }) {
            p = 0o500;
        }
        p
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Slot {
    pub ofd: Arc<Ofd>,
    pub cloexec: bool,
}

/// A tabela de fds de um processo.
#[derive(Debug, Default, Clone)]
pub(crate) struct FdTable {
    fds: BTreeMap<i32, Slot>,
}

impl FdTable {
    pub(crate) fn get(&self, fd: Fd) -> Result<&Slot, Errno> {
        self.fds.get(&fd.0).ok_or(Errno::EBADF)
    }

    pub(crate) fn get_mut(&mut self, fd: Fd) -> Result<&mut Slot, Errno> {
        self.fds.get_mut(&fd.0).ok_or(Errno::EBADF)
    }

    pub(crate) fn ofd(&self, fd: Fd) -> Result<Arc<Ofd>, Errno> {
        Ok(self.get(fd)?.ofd.clone())
    }

    /// Menor fd livre `>= min` e `< limit` (RLIMIT_NOFILE): EMFILE se não há.
    pub(crate) fn lowest_free(&self, min: i32, limit: u64) -> Result<i32, Errno> {
        let mut cand = min;
        for (&fd, _) in self.fds.range(min..) {
            if fd != cand {
                break;
            }
            cand += 1;
        }
        if cand as u64 >= limit {
            return Err(Errno::EMFILE);
        }
        Ok(cand)
    }

    /// Instala no menor fd livre.
    pub(crate) fn install(&mut self, ofd: Arc<Ofd>, cloexec: bool, min: i32, limit: u64) -> Result<Fd, Errno> {
        let fd = self.lowest_free(min, limit)?;
        self.fds.insert(fd, Slot { ofd, cloexec });
        Ok(Fd(fd))
    }

    /// Põe num fd específico, devolvendo o que estava lá (pra fechar fora da trava).
    pub(crate) fn put(&mut self, fd: Fd, ofd: Arc<Ofd>, cloexec: bool) -> Option<Slot> {
        self.fds.insert(fd.0, Slot { ofd, cloexec })
    }

    pub(crate) fn remove(&mut self, fd: Fd) -> Option<Slot> {
        self.fds.remove(&fd.0)
    }

    pub(crate) fn fds(&self) -> Vec<Fd> {
        self.fds.keys().map(|&f| Fd(f)).collect()
    }

    pub(crate) fn len(&self) -> usize {
        self.fds.len()
    }

    /// Fecha os fds com FD_CLOEXEC (execve). Devolve as descrições pra soltar fora da trava.
    pub(crate) fn take_cloexec(&mut self) -> Vec<Slot> {
        let ids: Vec<i32> = self.fds.iter().filter(|(_, s)| s.cloexec).map(|(&f, _)| f).collect();
        ids.into_iter().filter_map(|f| self.fds.remove(&f)).collect()
    }

    pub(crate) fn take_all(&mut self) -> Vec<Slot> {
        std::mem::take(&mut self.fds).into_values().collect()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (Fd, &Slot)> {
        self.fds.iter().map(|(&f, s)| (Fd(f), s))
    }
}
