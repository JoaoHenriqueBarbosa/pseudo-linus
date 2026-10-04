//! Montagens e o namespace de montagens de um sandbox.
//!
//! [`Loc`] é o `struct path` do Linux: uma montagem mais um inode dentro do sistema de arquivos dela. A
//! resolução de caminho troca de montagem ao entrar num ponto de montagem e ao subir com `..` pela raiz de
//! uma montagem, como o `follow_dotdot` e o `traverse_mounts` do kernel.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use bitflags::bitflags;
use parking_lot::RwLock;

use crate::fs::FileSystem;
use crate::types::*;

bitflags! {
    /// Flags de montagem (`MS_*`) que mudam a semântica.
    #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
    pub struct MountFlags: u32 {
        const RDONLY = 1;
        const NOSUID = 2;
        const NODEV = 4;
        const NOEXEC = 8;
        const NOATIME = 0x400;
        const NODIRATIME = 0x800;
        const RELATIME = 0x20_0000;
    }
}

impl MountFlags {
    /// O texto de opções genéricas de `/proc/mounts` ("rw,nosuid,nodev,noexec,relatime").
    pub fn describe(self) -> String {
        let mut parts = vec![if self.contains(MountFlags::RDONLY) { "ro" } else { "rw" }];
        if self.contains(MountFlags::NOSUID) {
            parts.push("nosuid");
        }
        if self.contains(MountFlags::NODEV) {
            parts.push("nodev");
        }
        if self.contains(MountFlags::NOEXEC) {
            parts.push("noexec");
        }
        if self.contains(MountFlags::NOATIME) {
            parts.push("noatime");
        } else if self.contains(MountFlags::RELATIME) {
            parts.push("relatime");
        }
        parts.join(",")
    }
}

/// Uma montagem (`struct mount` + `vfsmount`).
pub struct Mount {
    pub id: u32,
    pub fs: Arc<dyn FileSystem>,
    /// Montagem pai e o inode, nela, onde esta está montada. `None` na raiz do namespace.
    pub parent: Option<(Arc<Mount>, Ino)>,
    pub flags: MountFlags,
    /// Primeiro campo de `/proc/mounts` ("tmpfs", "proc").
    pub source: String,
    /// Opções próprias do sistema de arquivos, depois das genéricas ("size=65536k,mode=755,inode64").
    pub fs_options: String,
}

impl std::fmt::Debug for Mount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Mount({}, {})", self.id, self.fs.fs_type())
    }
}

impl Mount {
    pub fn read_only(&self) -> bool {
        self.flags.contains(MountFlags::RDONLY)
    }

    pub fn root(self: &Arc<Self>) -> Loc {
        Loc { mnt: self.clone(), ino: self.fs.root_ino() }
    }
}

/// Um lugar no namespace: montagem e inode (`struct path`).
#[derive(Clone)]
pub struct Loc {
    pub mnt: Arc<Mount>,
    pub ino: Ino,
}

impl Loc {
    pub fn fs(&self) -> &Arc<dyn FileSystem> {
        &self.mnt.fs
    }

    /// Diz se é a raiz da própria montagem.
    pub fn is_mount_root(&self) -> bool {
        self.ino == self.mnt.fs.root_ino()
    }
}

impl PartialEq for Loc {
    fn eq(&self, other: &Loc) -> bool {
        self.mnt.id == other.mnt.id && self.ino == other.ino
    }
}

impl Eq for Loc {}

impl std::fmt::Debug for Loc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Loc(mnt {}, ino {})", self.mnt.id, self.ino)
    }
}

/// Um [`Loc`] que segura o inode vivo (cwd, raiz, `exe` de um processo): um diretório removido que é o
/// cwd de alguém continua existindo, morto, até ninguém mais usar.
pub struct PinnedLoc {
    loc: Loc,
}

impl PinnedLoc {
    pub fn new(loc: Loc) -> PinnedLoc {
        loc.fs().pin(loc.ino);
        PinnedLoc { loc }
    }

    pub fn loc(&self) -> &Loc {
        &self.loc
    }
}

impl Clone for PinnedLoc {
    fn clone(&self) -> PinnedLoc {
        PinnedLoc::new(self.loc.clone())
    }
}

impl Drop for PinnedLoc {
    fn drop(&mut self) {
        self.loc.fs().unpin(self.loc.ino);
    }
}

impl std::ops::Deref for PinnedLoc {
    type Target = Loc;
    fn deref(&self) -> &Loc {
        &self.loc
    }
}

impl std::fmt::Debug for PinnedLoc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.loc.fmt(f)
    }
}

struct NsInner {
    /// Na ordem de montagem (a de `/proc/mounts`).
    mounts: Vec<Arc<Mount>>,
    /// (montagem pai, inode do ponto) -> montagem por cima. Só a de cima de cada ponto.
    children: HashMap<(u32, Ino), Arc<Mount>>,
}

/// O namespace de montagens de um sandbox.
pub struct Namespace {
    root: Arc<Mount>,
    inner: RwLock<NsInner>,
    next_id: AtomicU32,
}

impl std::fmt::Debug for Namespace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Namespace({:?})", self.mounts())
    }
}

impl Namespace {
    /// Namespace com `fs` na raiz.
    pub fn new(fs: Arc<dyn FileSystem>, flags: MountFlags, source: &str, fs_options: &str) -> Arc<Namespace> {
        let root = Arc::new(Mount {
            id: 1,
            fs,
            parent: None,
            flags,
            source: source.to_string(),
            fs_options: fs_options.to_string(),
        });
        Arc::new(Namespace {
            root: root.clone(),
            inner: RwLock::new(NsInner { mounts: vec![root], children: HashMap::new() }),
            next_id: AtomicU32::new(2),
        })
    }

    /// Raiz do namespace.
    pub fn root(&self) -> Loc {
        self.root.root()
    }

    pub fn root_mount(&self) -> &Arc<Mount> {
        &self.root
    }

    /// Monta `fs` em `at`, que precisa ser diretório.
    pub fn mount(
        &self,
        at: &Loc,
        fs: Arc<dyn FileSystem>,
        flags: MountFlags,
        source: &str,
        fs_options: &str,
    ) -> SysResult<Arc<Mount>> {
        let cx_free_attr = at.fs().getattr(&crate::ops::kernel_caller(at.clone()), at.ino)?;
        if !is_dir(cx_free_attr.mode) {
            return Err(Errno::ENOTDIR);
        }
        let m = Arc::new(Mount {
            id: self.next_id.fetch_add(1, Ordering::Relaxed),
            fs,
            parent: Some((at.mnt.clone(), at.ino)),
            flags,
            source: source.to_string(),
            fs_options: fs_options.to_string(),
        });
        let mut g = self.inner.write();
        g.mounts.push(m.clone());
        g.children.insert((at.mnt.id, at.ino), m.clone());
        Ok(m)
    }

    /// Montagens, na ordem em que foram feitas.
    pub fn mounts(&self) -> Vec<Arc<Mount>> {
        self.inner.read().mounts.clone()
    }

    /// Montagem que cobre `loc`, se houver.
    pub fn mounted_on(&self, loc: &Loc) -> Option<Arc<Mount>> {
        self.inner.read().children.get(&(loc.mnt.id, loc.ino)).cloned()
    }

    /// Diz se `loc` é ponto de montagem (EBUSY pra rmdir, unlink e rename).
    pub fn is_mountpoint(&self, loc: &Loc) -> bool {
        self.inner.read().children.contains_key(&(loc.mnt.id, loc.ino))
    }

    /// Desce pelas montagens empilhadas em `loc` (`traverse_mounts`).
    pub fn follow_mounts(&self, mut loc: Loc) -> Loc {
        let g = self.inner.read();
        while let Some(m) = g.children.get(&(loc.mnt.id, loc.ino)) {
            loc = m.root();
        }
        loc
    }
}
