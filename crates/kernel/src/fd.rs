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
    Path { loc: Loc },
    /// Pipe anônimo ou FIFO (com o lugar da FIFO no VFS).
    Pipe { end: PipeEnd, fifo: Option<Loc> },
    Dev { dev: Device, loc: Option<Loc> },
    /// Socket TCP de loopback em escuta.
    Listener(Arc<crate::net::Listener>),
    /// Conexão TCP de loopback estabelecida.
    Stream(crate::net::Conn),
    /// Socket do domínio Unix, em qualquer estado.
    Unix(Arc<crate::unix::UnixSock>),
    /// Socket UDP de loopback.
    Udp(Arc<crate::udp::UdpSock>),
    /// `epoll_create1`: a lista de interesse (`anon_inode:[eventpoll]`).
    Epoll(Arc<crate::epoll::Epoll>),
    /// `pidfd_open`: legível quando o processo termina (`anon_inode:[pidfd]`).
    Pidfd(crate::pidfd::Pidfd),
    /// `eventfd2` e `timerfd_create`: o contador ou o relógio atrás de um fd `anon_inode`.
    Anon(crate::anon::Anon),
}

impl FileObj {
    /// TCP, Unix ou UDP: o que o `setsockopt` e o `getsockopt` aceitam.
    pub(crate) fn is_socket(&self) -> bool {
        matches!(self, FileObj::Listener(_) | FileObj::Stream(_) | FileObj::Unix(_) | FileObj::Udp(_))
    }
}

impl std::fmt::Debug for FileObj {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileObj::Vfs { loc, kind, .. } => write!(f, "Vfs({loc:?}, {kind:?})"),
            FileObj::Path { loc, .. } => write!(f, "Path({loc:?})"),
            FileObj::Pipe { end, .. } => write!(f, "Pipe(ino {}, r {}, w {})", end.pipe.ino, end.read, end.write),
            FileObj::Dev { dev, .. } => write!(f, "Dev({dev:?})"),
            FileObj::Listener(l) => write!(f, "Listener({})", l.port()),
            FileObj::Stream(c) => write!(f, "Stream({} -> {})", c.local, c.peer),
            FileObj::Unix(u) => write!(f, "Unix(ino {})", u.ident.ino),
            FileObj::Udp(u) => write!(f, "Udp(ino {})", u.ident.ino),
            FileObj::Epoll(_) => write!(f, "Epoll"),
            FileObj::Pidfd(p) => write!(f, "Pidfd({})", p.pid()),
            FileObj::Anon(a) => write!(f, "{a:?}"),
        }
    }
}

/// O que o `struct sock` guarda além do objeto de transporte: o erro pendente (`sk_err`, lido e zerado por
/// `SO_ERROR`), o `connect` não bloqueante em andamento (`SS_CONNECTING`) ou recusado, e as opções
/// (`setsockopt`) como `(nível, nome) -> bytes`. É da descrição, então todos os fds de `dup` o veem.
#[derive(Debug, Default)]
pub(crate) struct SockMeta {
    pub error: i32,
    /// O `connect` devolveu EINPROGRESS e o próximo `connect` o conclui (ou entrega a recusa).
    pub connecting: bool,
    /// O SYN foi recusado: o `tcp_poll` dá `IN|OUT|HUP` até o `connect` seguinte.
    pub failed: bool,
    /// O TCP já estabeleceu a conexão: `tcp_init_buffer_space` chamou o `tcp_sndbuf_expand`, e o `SO_SNDBUF` que
    /// ninguém definiu (sem `SOCK_SNDBUF_LOCK`) lê o valor ampliado, não o padrão do `tcp_wmem`.
    pub sndbuf_expanded: bool,
    pub opts: BTreeMap<(i32, i32), Vec<u8>>,
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
    /// Flags de abertura que o `f_flags` guarda e `status` não (`O_DIRECTORY`, `O_NOFOLLOW`): só o
    /// `/proc/<pid>/fdinfo` as mostra.
    pub open_extra: u32,
    pub st: Mutex<OfdState>,
    /// Estado de socket (só tem sentido quando `obj` é um socket).
    pub sock: Mutex<SockMeta>,
    /// Tabela de travas do sandbox (pra soltar as travas desta descrição no último close).
    pub locks: Weak<crate::sandbox::LockTable>,
    /// Os epolls que têm esta descrição na lista de interesse (o `f_ep` do `struct file`): o `eventpoll_release`
    /// do último close tira a entrada de cada um e solta o parker dela das filas deste arquivo.
    pub(crate) watchers: Mutex<Vec<Weak<crate::epoll::Epoll>>>,
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
        let watchers = std::mem::take(self.watchers.get_mut());
        for epoll in watchers.iter().filter_map(Weak::upgrade) {
            epoll.release(self);
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
            open_extra: flags.bits() & (OFlags::DIRECTORY.bits() | OFlags::NOFOLLOW.bits()),
            st: Mutex::new(OfdState { status: flags & keep, pos: 0, dir_cookie: 0 }),
            sock: Mutex::new(SockMeta::default()),
            locks,
            watchers: Mutex::new(Vec::new()),
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
            FileObj::Listener(_) | FileObj::Stream(_) | FileObj::Unix(_) | FileObj::Udp(_) | FileObj::Epoll(_) | FileObj::Pidfd(_) | FileObj::Anon(_) => None,
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
    /// Maior fd já instalado: a tabela do Linux cresce em potências de 2 e não encolhe (`FDSize`).
    high: i32,
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
        self.high = self.high.max(fd);
        self.fds.insert(fd, Slot { ofd, cloexec });
        Ok(Fd(fd))
    }

    /// Põe num fd específico, devolvendo o que estava lá (pra fechar fora da trava).
    pub(crate) fn put(&mut self, fd: Fd, ofd: Arc<Ofd>, cloexec: bool) -> Option<Slot> {
        self.high = self.high.max(fd.0);
        self.fds.insert(fd.0, Slot { ofd, cloexec })
    }

    pub(crate) fn remove(&mut self, fd: Fd) -> Option<Slot> {
        self.fds.remove(&fd.0)
    }

    pub(crate) fn fds(&self) -> Vec<Fd> {
        self.fds.keys().map(|&f| Fd(f)).collect()
    }

    /// `max_fds` da tabela do Linux (`FDSize`): 64 no começo, e ao passar disso o menor múltiplo de 128
    /// que é potência de 2 vezes 128 e cobre o maior fd (`expand_fdtable`).
    pub(crate) fn fdsize(&self) -> u32 {
        let mut max = 64u32;
        let high = self.high.max(0) as u32;
        while high >= max {
            max = 128 * (high / 128 + 1).next_power_of_two();
        }
        max
    }

    /// Fecha os fds com FD_CLOEXEC (execve). Devolve as descrições pra soltar fora da trava.
    pub(crate) fn take_cloexec(&mut self) -> Vec<Slot> {
        let ids: Vec<i32> = self.fds.iter().filter(|(_, s)| s.cloexec).map(|(&f, _)| f).collect();
        ids.into_iter().filter_map(|f| self.fds.remove(&f)).collect()
    }

    pub(crate) fn take_all(&mut self) -> Vec<Slot> {
        std::mem::take(&mut self.fds).into_values().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fdsize_grows_like_expand_fdtable() {
        let size = |high: i32| FdTable { high, ..FdTable::default() }.fdsize();
        assert_eq!(size(0), 64);
        assert_eq!(size(63), 64);
        assert_eq!(size(64), 128);
        assert_eq!(size(127), 128);
        assert_eq!(size(128), 256);
        assert_eq!(size(255), 256);
        assert_eq!(size(256), 512);
        assert_eq!(size(1000), 1024);
    }
}
