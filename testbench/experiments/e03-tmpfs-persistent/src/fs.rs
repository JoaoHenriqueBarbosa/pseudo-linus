//! Inodes, estado persistente e operações do tmpfs.
//!
//! As operações são funções genéricas sobre [`Txn`], que é o acesso à tabela de inodes. O dono
//! único ([`Fs`]) implementa `Txn` direto; a sandbox fatiada de [`crate::concurrent`] implementa com
//! trava por fatia e pode pedir pra refazer a operação ([`Fail::NeedLock`]). Por isso toda operação
//! lê tudo o que precisa antes da primeira mutação.
//!
//! Semântica de referência: Linux 6.12, tmpfs (`shmem`). Tamanho de diretório = 20 bytes por
//! entrada mais 40 (o `BOGO_DIRENT_SIZE` do shmem), `nlink` de diretório = 2 + subdiretórios.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::content::Content;
use crate::errno::Errno;
use crate::maps::{Flavor, IntMap, NameMap, TableFamily};

pub use crate::maps::{Ino, Name};

pub const ROOT_INO: Ino = 1;
pub const NAME_MAX: usize = 255;
pub const BOGO_DIRENT_SIZE: u64 = 20;
/// `s_maxbytes` do tmpfs em 64 bits é `MAX_LFS_FILESIZE`; aqui o limite é menor (1 TiB) pra não
/// deixar um offset absurdo alocar a árvore de blocos inteira.
pub const MAX_FILE_SIZE: u64 = 1 << 40;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    File,
    Dir,
}

pub enum Data<F: Flavor> {
    File(F::Content),
    Dir { entries: F::Dir, parent: Ino },
}

impl<F: Flavor> Clone for Data<F> {
    fn clone(&self) -> Self {
        match self {
            Data::File(c) => Data::File(c.clone()),
            Data::Dir { entries, parent } => Data::Dir { entries: entries.clone(), parent: *parent },
        }
    }
}

pub struct Inode<F: Flavor> {
    pub mode: u32,
    pub nlink: u32,
    pub mtime: u64,
    pub ctime: u64,
    pub data: Data<F>,
}

impl<F: Flavor> Clone for Inode<F> {
    fn clone(&self) -> Self {
        Inode { mode: self.mode, nlink: self.nlink, mtime: self.mtime, ctime: self.ctime, data: self.data.clone() }
    }
}

impl<F: Flavor> Inode<F> {
    pub fn new_file(mode: u32, now: u64) -> Self {
        Inode { mode, nlink: 1, mtime: now, ctime: now, data: Data::File(F::Content::default()) }
    }

    pub fn new_dir(mode: u32, parent: Ino, now: u64) -> Self {
        Inode { mode, nlink: 2, mtime: now, ctime: now, data: Data::Dir { entries: F::Dir::default(), parent } }
    }

    pub fn kind(&self) -> Kind {
        match self.data {
            Data::File(_) => Kind::File,
            Data::Dir { .. } => Kind::Dir,
        }
    }

    pub fn size(&self) -> u64 {
        match &self.data {
            Data::File(c) => c.len(),
            Data::Dir { entries, .. } => (2 + entries.len() as u64) * BOGO_DIRENT_SIZE,
        }
    }

    pub fn entries(&self) -> Result<&F::Dir, Errno> {
        match &self.data {
            Data::Dir { entries, .. } => Ok(entries),
            Data::File(_) => Err(Errno::ENOTDIR),
        }
    }

    fn entries_mut(&mut self) -> &mut F::Dir {
        match &mut self.data {
            Data::Dir { entries, .. } => entries,
            Data::File(_) => unreachable!("entries_mut em arquivo: a operação devia ter checado antes"),
        }
    }

    fn content_mut(&mut self) -> &mut F::Content {
        match &mut self.data {
            Data::File(c) => c,
            Data::Dir { .. } => unreachable!("content_mut em diretório: a operação devia ter checado antes"),
        }
    }

    fn touch(&mut self, now: u64) {
        self.mtime = now;
        self.ctime = now;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stat {
    pub ino: Ino,
    pub kind: Kind,
    pub mode: u32,
    pub nlink: u32,
    pub size: u64,
    pub mtime: u64,
    pub ctime: u64,
}

/// Falha interna de uma operação: erro de verdade, ou "preciso da trava deste ino" (só a sandbox
/// fatiada produz; ela trava a fatia e refaz a operação).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fail {
    Errno(Errno),
    NeedLock(Ino),
}

impl From<Errno> for Fail {
    fn from(e: Errno) -> Fail {
        Fail::Errno(e)
    }
}

impl Fail {
    /// Converte pra errno num contexto em que `NeedLock` é impossível (dono único).
    pub fn errno(self) -> Errno {
        match self {
            Fail::Errno(e) => e,
            Fail::NeedLock(ino) => unreachable!("NeedLock({ino}) fora da sandbox fatiada"),
        }
    }
}

pub type Table<F> = <<F as Flavor>::Table as TableFamily>::Map<Arc<Inode<F>>>;

/// Acesso à tabela de inodes durante uma operação.
pub trait Txn<F: Flavor> {
    fn inode(&self, ino: Ino) -> Result<Option<&Arc<Inode<F>>>, Fail>;
    /// Referência mutável, com cópia do caminho (e do inode) se estiverem compartilhados.
    fn inode_mut(&mut self, ino: Ino) -> Result<Option<&mut Inode<F>>, Fail>;
    fn insert(&mut self, ino: Ino, inode: Inode<F>) -> Result<(), Fail>;
    fn remove(&mut self, ino: Ino) -> Result<(), Fail>;
    fn alloc_ino(&mut self) -> Ino;
    fn now(&mut self) -> u64;
    /// Marca ou desmarca um inode sem nome e ainda aberto.
    fn set_orphan(&mut self, ino: Ino, orphan: bool);
}

/// Estado persistente de uma sandbox. `clone()` é o snapshot: O(1), compartilha tudo.
pub struct Fs<F: Flavor> {
    table: Table<F>,
    next_ino: Ino,
    clock: u64,
    orphans: Arc<BTreeSet<Ino>>,
}

impl<F: Flavor> Clone for Fs<F> {
    fn clone(&self) -> Self {
        Fs { table: self.table.clone(), next_ino: self.next_ino, clock: self.clock, orphans: Arc::clone(&self.orphans) }
    }
}

impl<F: Flavor> Default for Fs<F> {
    fn default() -> Self {
        Fs::new()
    }
}

impl<F: Flavor> Fs<F> {
    pub fn new() -> Self {
        let mut table = Table::<F>::default();
        table.insert(ROOT_INO, Arc::new(Inode::new_dir(0o755, ROOT_INO, 0)));
        Fs { table, next_ino: ROOT_INO + 1, clock: 0, orphans: Arc::new(BTreeSet::new()) }
    }

    /// Monta a partir de partes (usado pela sandbox fatiada pra exportar o estado).
    pub fn from_parts(table: Table<F>, next_ino: Ino, clock: u64) -> Self {
        Fs { table, next_ino, clock, orphans: Arc::new(BTreeSet::new()) }
    }

    pub fn table(&self) -> &Table<F> {
        &self.table
    }

    pub fn next_ino(&self) -> Ino {
        self.next_ino
    }

    pub fn clock(&self) -> u64 {
        self.clock
    }

    pub fn inode_count(&self) -> usize {
        self.table.len()
    }

    pub fn orphans(&self) -> &BTreeSet<Ino> {
        &self.orphans
    }

    /// Avança o contador de inos e o relógio (nunca volta: restore não reaproveita ino).
    pub fn bump_counters(&mut self, next_ino: Ino, clock: u64) {
        self.next_ino = self.next_ino.max(next_ino);
        self.clock = self.clock.max(clock);
    }

    pub fn contains(&self, ino: Ino) -> bool {
        self.table.get(ino).is_some()
    }

    /// Remove um inode órfão (sem nome e sem fd aberto).
    pub fn drop_orphan(&mut self, ino: Ino) {
        if self.orphans.contains(&ino) {
            Arc::make_mut(&mut self.orphans).remove(&ino);
            self.table.remove(ino);
        }
    }
}

impl<F: Flavor> Txn<F> for Fs<F> {
    fn inode(&self, ino: Ino) -> Result<Option<&Arc<Inode<F>>>, Fail> {
        Ok(self.table.get(ino))
    }

    fn inode_mut(&mut self, ino: Ino) -> Result<Option<&mut Inode<F>>, Fail> {
        Ok(self.table.get_mut(ino).map(Arc::make_mut))
    }

    fn insert(&mut self, ino: Ino, inode: Inode<F>) -> Result<(), Fail> {
        self.table.insert(ino, Arc::new(inode));
        Ok(())
    }

    fn remove(&mut self, ino: Ino) -> Result<(), Fail> {
        self.table.remove(ino);
        Ok(())
    }

    fn alloc_ino(&mut self) -> Ino {
        let ino = self.next_ino;
        self.next_ino += 1;
        ino
    }

    fn now(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    fn set_orphan(&mut self, ino: Ino, orphan: bool) {
        let present = self.orphans.contains(&ino);
        if orphan != present {
            let set = Arc::make_mut(&mut self.orphans);
            if orphan {
                set.insert(ino);
            } else {
                set.remove(&ino);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Caminhos.

/// Último componente de um caminho, já classificado.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Last<'a> {
    /// O caminho é a raiz (`/`).
    Root,
    Dot,
    DotDot,
    Name(&'a [u8]),
}

/// Divide um caminho absoluto em componentes. `NAME_MAX` vale pra cada componente.
pub fn components(path: &[u8]) -> Result<Vec<&[u8]>, Errno> {
    if path.is_empty() {
        return Err(Errno::ENOENT);
    }
    if path[0] != b'/' {
        return Err(Errno::EINVAL);
    }
    let comps: Vec<&[u8]> = path.split(|&b| b == b'/').filter(|c| !c.is_empty()).collect();
    if comps.iter().any(|c| c.len() > NAME_MAX) {
        return Err(Errno::ENAMETOOLONG);
    }
    Ok(comps)
}

fn has_trailing_slash(path: &[u8]) -> bool {
    path.len() > 1 && path.ends_with(b"/")
}

pub fn get<F: Flavor, T: Txn<F> + ?Sized>(t: &T, ino: Ino) -> Result<&Inode<F>, Fail> {
    Ok(t.inode(ino)?.map(|a| &**a).ok_or(Errno::ENOENT)?)
}

/// Procura `name` no diretório `dir` (`.` e `..` inclusive).
pub fn lookup<F: Flavor, T: Txn<F> + ?Sized>(t: &T, dir: Ino, name: &[u8]) -> Result<Option<Ino>, Fail> {
    let inode = get(t, dir)?;
    match &inode.data {
        Data::Dir { entries, parent } => Ok(match name {
            b"." => Some(dir),
            b".." => Some(*parent),
            _ => entries.get(name),
        }),
        Data::File(_) => Err(Errno::ENOTDIR.into()),
    }
}

fn walk<F: Flavor, T: Txn<F> + ?Sized>(t: &T, comps: &[&[u8]]) -> Result<Ino, Fail> {
    let mut cur = ROOT_INO;
    for c in comps {
        cur = lookup(t, cur, c)?.ok_or(Errno::ENOENT)?;
    }
    Ok(cur)
}

/// Resolve o caminho inteiro. Barra final exige diretório.
pub fn resolve<F: Flavor, T: Txn<F> + ?Sized>(t: &T, path: &[u8]) -> Result<Ino, Fail> {
    let comps = components(path)?;
    let ino = walk(t, &comps)?;
    if has_trailing_slash(path) && get(t, ino)?.kind() != Kind::Dir {
        return Err(Errno::ENOTDIR.into());
    }
    Ok(ino)
}

/// Resolve o diretório pai e devolve o último componente. O pai tem que ser diretório (como no
/// `filename_parentat` do Linux).
pub fn resolve_parent<'p, F: Flavor, T: Txn<F> + ?Sized>(t: &T, path: &'p [u8]) -> Result<(Ino, Last<'p>), Fail> {
    let comps = components(path)?;
    let Some((last, init)) = comps.split_last() else {
        return Ok((ROOT_INO, Last::Root));
    };
    let parent = walk(t, init)?;
    get(t, parent)?.entries()?;
    let last = match *last {
        b"." => Last::Dot,
        b".." => Last::DotDot,
        name => Last::Name(name),
    };
    Ok((parent, last))
}

// ---------------------------------------------------------------------------------------------
// Operações sobre (diretório pai, nome).

fn live_dir<F: Flavor>(inode: &Inode<F>) -> Result<&F::Dir, Fail> {
    let entries = inode.entries()?;
    if inode.nlink == 0 {
        // Diretório removido enquanto alguém resolvia o caminho (só acontece na sandbox fatiada).
        return Err(Errno::ENOENT.into());
    }
    Ok(entries)
}

/// `open(O_CREAT)`: devolve o ino existente (ou EEXIST com `excl`), ou cria arquivo vazio.
pub fn create_at<F: Flavor, T: Txn<F> + ?Sized>(t: &mut T, dir: Ino, last: Last, mode: u32, excl: bool) -> Result<Ino, Fail> {
    let name = match last {
        Last::Name(n) => n,
        _ if excl => return Err(Errno::EEXIST.into()),
        _ => return Err(Errno::EISDIR.into()),
    };
    let existing = live_dir(get(t, dir)?)?.get(name);
    if let Some(ino) = existing {
        if excl {
            return Err(Errno::EEXIST.into());
        }
        if get(t, ino)?.kind() == Kind::Dir {
            return Err(Errno::EISDIR.into());
        }
        return Ok(ino);
    }
    let ino = t.alloc_ino();
    let now = t.now();
    t.insert(ino, Inode::new_file(mode, now))?;
    let d = t.inode_mut(dir)?.ok_or(Errno::ENOENT)?;
    d.entries_mut().insert(Arc::from(name), ino);
    d.touch(now);
    Ok(ino)
}

pub fn mkdir_at<F: Flavor, T: Txn<F> + ?Sized>(t: &mut T, dir: Ino, last: Last, mode: u32) -> Result<Ino, Fail> {
    let Last::Name(name) = last else {
        return Err(Errno::EEXIST.into());
    };
    if live_dir(get(t, dir)?)?.get(name).is_some() {
        return Err(Errno::EEXIST.into());
    }
    let ino = t.alloc_ino();
    let now = t.now();
    t.insert(ino, Inode::new_dir(mode, dir, now))?;
    let d = t.inode_mut(dir)?.ok_or(Errno::ENOENT)?;
    d.entries_mut().insert(Arc::from(name), ino);
    d.nlink += 1;
    d.touch(now);
    Ok(ino)
}

/// `link(target, dir/name)`. A ordem dos erros segue o `linkat`: nome já existente (EEXIST) antes
/// de alvo diretório (EPERM).
pub fn link_at<F: Flavor, T: Txn<F> + ?Sized>(t: &mut T, target: Ino, dir: Ino, last: Last) -> Result<(), Fail> {
    let Last::Name(name) = last else {
        return Err(Errno::EEXIST.into());
    };
    if live_dir(get(t, dir)?)?.get(name).is_some() {
        return Err(Errno::EEXIST.into());
    }
    let target_inode = get(t, target)?;
    if target_inode.kind() == Kind::Dir {
        return Err(Errno::EPERM.into());
    }
    if target_inode.nlink == 0 {
        return Err(Errno::ENOENT.into());
    }
    let now = t.now();
    let ti = t.inode_mut(target)?.ok_or(Errno::ENOENT)?;
    ti.nlink += 1;
    ti.ctime = now;
    let d = t.inode_mut(dir)?.ok_or(Errno::ENOENT)?;
    d.entries_mut().insert(Arc::from(name), target);
    d.touch(now);
    Ok(())
}

/// Tira um link de um arquivo; com `nlink` zerado, o inode some ou vira órfão se estiver aberto.
fn drop_link<F: Flavor, T: Txn<F> + ?Sized>(t: &mut T, ino: Ino, now: u64, is_open: &dyn Fn(Ino) -> bool) -> Result<(), Fail> {
    let inode = t.inode_mut(ino)?.ok_or(Errno::ENOENT)?;
    inode.nlink -= 1;
    inode.ctime = now;
    if inode.nlink == 0 {
        if is_open(ino) {
            t.set_orphan(ino, true);
        } else {
            t.remove(ino)?;
        }
    }
    Ok(())
}

pub fn unlink_at<F: Flavor, T: Txn<F> + ?Sized>(t: &mut T, dir: Ino, last: Last, is_open: &dyn Fn(Ino) -> bool) -> Result<(), Fail> {
    let Last::Name(name) = last else {
        return Err(Errno::EISDIR.into());
    };
    let child = live_dir(get(t, dir)?)?.get(name).ok_or(Errno::ENOENT)?;
    if get(t, child)?.kind() == Kind::Dir {
        return Err(Errno::EISDIR.into());
    }
    let now = t.now();
    let d = t.inode_mut(dir)?.ok_or(Errno::ENOENT)?;
    d.entries_mut().remove(name);
    d.touch(now);
    drop_link(t, child, now, is_open)
}

pub fn rmdir_at<F: Flavor, T: Txn<F> + ?Sized>(t: &mut T, dir: Ino, last: Last) -> Result<(), Fail> {
    let name = match last {
        Last::Root => return Err(Errno::EBUSY.into()),
        Last::Dot => return Err(Errno::EINVAL.into()),
        Last::DotDot => return Err(Errno::ENOTEMPTY.into()),
        Last::Name(n) => n,
    };
    let child = live_dir(get(t, dir)?)?.get(name).ok_or(Errno::ENOENT)?;
    let c = get(t, child)?;
    let entries = c.entries()?;
    if entries.len() > 0 {
        return Err(Errno::ENOTEMPTY.into());
    }
    let now = t.now();
    let d = t.inode_mut(dir)?.ok_or(Errno::ENOENT)?;
    d.entries_mut().remove(name);
    d.nlink -= 1;
    d.touch(now);
    t.remove(child)
}

/// `true` se `ancestor` é `ino` ou um ancestral dele.
fn is_ancestor<F: Flavor, T: Txn<F> + ?Sized>(t: &T, ancestor: Ino, mut ino: Ino) -> Result<bool, Fail> {
    loop {
        if ino == ancestor {
            return Ok(true);
        }
        if ino == ROOT_INO {
            return Ok(false);
        }
        ino = match &get(t, ino)?.data {
            Data::Dir { parent, .. } => *parent,
            Data::File(_) => return Ok(false),
        };
    }
}

/// `rename(sdir/sname, ddir/dname)` com a semântica do Linux: substitui destino existente
/// atomicamente, não faz nada se os dois nomes são o mesmo inode, e checa ancestralidade antes
/// do tipo do destino (como o `lock_rename` + `vfs_rename`).
pub fn rename_at<F: Flavor, T: Txn<F> + ?Sized>(
    t: &mut T,
    sdir: Ino,
    slast: Last,
    ddir: Ino,
    dlast: Last,
    is_open: &dyn Fn(Ino) -> bool,
) -> Result<(), Fail> {
    let (Last::Name(sname), Last::Name(dname)) = (slast, dlast) else {
        return Err(Errno::EBUSY.into());
    };
    let src = live_dir(get(t, sdir)?)?.get(sname).ok_or(Errno::ENOENT)?;
    let dst = live_dir(get(t, ddir)?)?.get(dname);
    if sdir != ddir {
        // Origem não pode ser ancestral do diretório de destino.
        if is_ancestor(t, src, ddir)? {
            return Err(Errno::EINVAL.into());
        }
        // Destino não pode ser ancestral do diretório de origem.
        if let Some(d) = dst
            && is_ancestor(t, d, sdir)?
        {
            return Err(Errno::ENOTEMPTY.into());
        }
    }
    if dst == Some(src) {
        return Ok(());
    }
    let src_kind = get(t, src)?.kind();
    let dst_kind = match dst {
        Some(d) => {
            let di = get(t, d)?;
            match (src_kind, di.kind()) {
                (Kind::Dir, Kind::File) => return Err(Errno::ENOTDIR.into()),
                (Kind::File, Kind::Dir) => return Err(Errno::EISDIR.into()),
                (Kind::Dir, Kind::Dir) if di.entries()?.len() > 0 => return Err(Errno::ENOTEMPTY.into()),
                (_, k) => Some(k),
            }
        }
        None => None,
    };
    let now = t.now();
    match (dst, dst_kind) {
        (Some(d), Some(Kind::Dir)) => {
            t.remove(d)?;
            t.inode_mut(ddir)?.ok_or(Errno::ENOENT)?.nlink -= 1;
        }
        (Some(d), Some(Kind::File)) => drop_link(t, d, now, is_open)?,
        _ => {}
    }
    let s = t.inode_mut(sdir)?.ok_or(Errno::ENOENT)?;
    s.entries_mut().remove(sname);
    s.touch(now);
    if src_kind == Kind::Dir && sdir != ddir {
        s.nlink -= 1;
    }
    let d = t.inode_mut(ddir)?.ok_or(Errno::ENOENT)?;
    d.entries_mut().insert(Arc::from(dname), src);
    d.touch(now);
    if src_kind == Kind::Dir && sdir != ddir {
        d.nlink += 1;
    }
    let si = t.inode_mut(src)?.ok_or(Errno::ENOENT)?;
    si.ctime = now;
    if let Data::Dir { parent, .. } = &mut si.data {
        *parent = ddir;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Operações sobre um inode.

pub fn write_ino<F: Flavor, T: Txn<F> + ?Sized>(t: &mut T, ino: Ino, off: u64, data: &[u8]) -> Result<usize, Fail> {
    if get(t, ino)?.kind() == Kind::Dir {
        return Err(Errno::EISDIR.into());
    }
    if data.is_empty() {
        return Ok(0);
    }
    if off.saturating_add(data.len() as u64) > MAX_FILE_SIZE {
        return Err(Errno::EFBIG.into());
    }
    let now = t.now();
    let inode = t.inode_mut(ino)?.ok_or(Errno::ENOENT)?;
    inode.content_mut().write_at(off, data);
    inode.touch(now);
    Ok(data.len())
}

pub fn truncate_ino<F: Flavor, T: Txn<F> + ?Sized>(t: &mut T, ino: Ino, len: u64) -> Result<(), Fail> {
    if get(t, ino)?.kind() == Kind::Dir {
        return Err(Errno::EISDIR.into());
    }
    if len > MAX_FILE_SIZE {
        return Err(Errno::EFBIG.into());
    }
    let now = t.now();
    let inode = t.inode_mut(ino)?.ok_or(Errno::ENOENT)?;
    inode.content_mut().set_len(len);
    inode.touch(now);
    Ok(())
}

pub fn read_ino<F: Flavor, T: Txn<F> + ?Sized>(t: &T, ino: Ino, off: u64, len: usize) -> Result<Vec<u8>, Fail> {
    match &get(t, ino)?.data {
        Data::File(c) => {
            let avail = c.len().saturating_sub(off).min(len as u64) as usize;
            let mut buf = vec![0; avail];
            let n = c.read_at(off, &mut buf);
            buf.truncate(n);
            Ok(buf)
        }
        Data::Dir { .. } => Err(Errno::EISDIR.into()),
    }
}

pub fn stat_ino<F: Flavor, T: Txn<F> + ?Sized>(t: &T, ino: Ino) -> Result<Stat, Fail> {
    let i = get(t, ino)?;
    Ok(Stat { ino, kind: i.kind(), mode: i.mode, nlink: i.nlink, size: i.size(), mtime: i.mtime, ctime: i.ctime })
}

/// Entradas do diretório ordenadas por nome (sem `.` e `..`).
pub fn readdir_ino<F: Flavor, T: Txn<F> + ?Sized>(t: &T, ino: Ino) -> Result<Vec<(Name, Ino)>, Fail> {
    let entries = get(t, ino)?.entries()?;
    let mut out = Vec::with_capacity(entries.len());
    entries.for_each(&mut |name, ino| out.push((Arc::clone(name), ino)));
    out.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}
