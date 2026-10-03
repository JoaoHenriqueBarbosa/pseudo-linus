//! Verificação diferencial: a mesma sequência de operações em três alvos.
//!
//! - [`Model`]: referência simples, um `BTreeMap` de caminhos (com ids de arquivo pra hardlink).
//! - [`RealFs`]: um diretório num tmpfs de verdade do host (`/dev/shm`), via `std::fs`. É o que
//!   prova que o modelo tem a semântica do Linux (incluindo a ordem dos errnos).
//! - [`Ours`]: o [`Vfs`] de um flavor.
//!
//! Snapshot e restore só existem no modelo e no nosso; o alvo real ignora essas operações e quem
//! compara com ele gera sequências sem elas.

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::os::unix::fs::{FileExt, MetadataExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::errno::Errno;
use crate::fs::{BOGO_DIRENT_SIZE, Fs, Kind};
use crate::maps::Flavor;
use crate::vfs::{Fd, Vfs};
use crate::workload::Rng;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    Mkdir(String),
    Create(String),
    Write(String, u64, Vec<u8>),
    Truncate(String, u64),
    Read(String, u64, usize),
    Unlink(String),
    Rmdir(String),
    Rename(String, String),
    Link(String, String),
    Stat(String),
    Readdir(String),
    Open(String),
    Close(usize),
    PWrite(usize, u64, Vec<u8>),
    PRead(usize, u64, usize),
    FStat(usize),
    Snapshot,
    Restore(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Out {
    Unit,
    Bytes(Vec<u8>),
    Stat { kind: Kind, nlink: u32, size: u64 },
    Names(Vec<Vec<u8>>),
    /// Operação sem efeito (fd inexistente, restore sem snapshot).
    Skipped,
}

pub type OpResult = Result<Out, Errno>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub kind: Kind,
    pub nlink: u32,
    pub size: u64,
    pub content: Option<Vec<u8>>,
}

pub type Dump = BTreeMap<String, Node>;

pub trait Target {
    fn apply(&mut self, op: &Op) -> OpResult;
    fn dump(&mut self) -> Dump;
}

fn parent_of(path: &str) -> &str {
    match path.rfind('/') {
        Some(0) => "/",
        Some(i) => &path[..i],
        None => "/",
    }
}

fn child_prefix(dir: &str) -> String {
    if dir == "/" { "/".to_string() } else { format!("{dir}/") }
}

fn data_write(content: &mut Vec<u8>, off: u64, data: &[u8]) {
    if data.is_empty() {
        return;
    }
    let end = off as usize + data.len();
    if end > content.len() {
        content.resize(end, 0);
    }
    content[off as usize..end].copy_from_slice(data);
}

fn data_read(content: &[u8], off: u64, len: usize) -> Vec<u8> {
    let start = (off as usize).min(content.len());
    let end = (start + len).min(content.len());
    content[start..end].to_vec()
}

// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MEntry {
    Dir,
    File(u64),
}

/// Estado inteiro do modelo (o snapshot dele é uma cópia funda disto).
type ModelState = (BTreeMap<String, MEntry>, BTreeMap<u64, Vec<u8>>);

/// Modelo de referência: caminhos absolutos normalizados (sem `.`, `..`, barra dupla ou final).
#[derive(Default)]
pub struct Model {
    paths: BTreeMap<String, MEntry>,
    files: BTreeMap<u64, Vec<u8>>,
    fds: Vec<u64>,
    snaps: Vec<ModelState>,
    next_id: u64,
}

impl Model {
    pub fn new() -> Model {
        let mut paths = BTreeMap::new();
        paths.insert("/".to_string(), MEntry::Dir);
        Model { paths, files: BTreeMap::new(), fds: Vec::new(), snaps: Vec::new(), next_id: 2 }
    }

    fn alloc(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Erros do caminho até o pai: ENOENT se um ancestral falta, ENOTDIR se é arquivo.
    fn walk_parent(&self, path: &str) -> Result<(), Errno> {
        let mut prefix = String::new();
        let comps: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();
        for c in comps.iter().take(comps.len().saturating_sub(1)) {
            prefix.push('/');
            prefix.push_str(c);
            match self.paths.get(&prefix) {
                None => return Err(Errno::ENOENT),
                Some(MEntry::File(_)) => return Err(Errno::ENOTDIR),
                Some(MEntry::Dir) => {}
            }
        }
        Ok(())
    }

    fn children(&self, dir: &str) -> Vec<(&String, &MEntry)> {
        let prefix = child_prefix(dir);
        self.paths
            .range(prefix.clone()..)
            .take_while(|(k, _)| k.starts_with(&prefix))
            .filter(|(k, _)| k.len() > prefix.len() && !k[prefix.len()..].contains('/'))
            .collect()
    }

    fn nlink_of(&self, id: u64) -> u32 {
        self.paths.values().filter(|e| **e == MEntry::File(id)).count() as u32
    }

    fn gc(&mut self, id: u64) {
        if self.nlink_of(id) == 0 && !self.fds.contains(&id) {
            self.files.remove(&id);
        }
    }

    fn stat_of(&self, path: &str, e: MEntry) -> Out {
        match e {
            MEntry::Dir => {
                let children = self.children(path);
                let subdirs = children.iter().filter(|(_, e)| **e == MEntry::Dir).count() as u32;
                Out::Stat { kind: Kind::Dir, nlink: 2 + subdirs, size: (2 + children.len() as u64) * BOGO_DIRENT_SIZE }
            }
            MEntry::File(id) => {
                Out::Stat { kind: Kind::File, nlink: self.nlink_of(id), size: self.files[&id].len() as u64 }
            }
        }
    }

    fn file_at(&self, path: &str) -> Result<u64, Errno> {
        self.walk_parent(path)?;
        match self.paths.get(path) {
            None => Err(Errno::ENOENT),
            Some(MEntry::Dir) => Err(Errno::EISDIR),
            Some(MEntry::File(id)) => Ok(*id),
        }
    }

    fn fd(&self, slot: usize) -> Option<u64> {
        if self.fds.is_empty() { None } else { Some(self.fds[slot % self.fds.len()]) }
    }

    fn live_fd(&self, slot: usize) -> Result<Option<u64>, Errno> {
        match self.fd(slot) {
            None => Ok(None),
            Some(id) if self.files.contains_key(&id) => Ok(Some(id)),
            Some(_) => Err(Errno::ESTALE),
        }
    }

    fn rename(&mut self, a: &str, b: &str) -> OpResult {
        self.walk_parent(a)?;
        self.walk_parent(b)?;
        let src = *self.paths.get(a).ok_or(Errno::ENOENT)?;
        let dst = self.paths.get(b).copied();
        if parent_of(a) != parent_of(b) {
            if b.starts_with(&format!("{a}/")) {
                return Err(Errno::EINVAL);
            }
            if dst.is_some() && a.starts_with(&format!("{b}/")) {
                return Err(Errno::ENOTEMPTY);
            }
        }
        if a == b || (matches!(src, MEntry::File(_)) && dst == Some(src)) {
            return Ok(Out::Unit);
        }
        match (src, dst) {
            (MEntry::Dir, Some(MEntry::File(_))) => return Err(Errno::ENOTDIR),
            (MEntry::File(_), Some(MEntry::Dir)) => return Err(Errno::EISDIR),
            (MEntry::Dir, Some(MEntry::Dir)) if !self.children(b).is_empty() => return Err(Errno::ENOTEMPTY),
            _ => {}
        }
        self.paths.remove(b);
        let prefix = format!("{a}/");
        let moved: Vec<String> =
            self.paths.keys().filter(|k| k.as_str() == a || k.starts_with(&prefix)).cloned().collect();
        for k in moved {
            let e = self.paths.remove(&k).expect("chave");
            self.paths.insert(format!("{b}{}", &k[a.len()..]), e);
        }
        if let Some(MEntry::File(id)) = dst {
            self.gc(id);
        }
        Ok(Out::Unit)
    }
}

impl Target for Model {
    fn apply(&mut self, op: &Op) -> OpResult {
        match op {
            Op::Mkdir(p) => {
                self.walk_parent(p)?;
                if self.paths.contains_key(p) {
                    return Err(Errno::EEXIST);
                }
                self.alloc();
                self.paths.insert(p.clone(), MEntry::Dir);
                Ok(Out::Unit)
            }
            Op::Create(p) => {
                self.walk_parent(p)?;
                match self.paths.get(p) {
                    Some(MEntry::Dir) => Err(Errno::EISDIR),
                    Some(MEntry::File(_)) => Ok(Out::Unit),
                    None => {
                        let id = self.alloc();
                        self.files.insert(id, Vec::new());
                        self.paths.insert(p.clone(), MEntry::File(id));
                        Ok(Out::Unit)
                    }
                }
            }
            Op::Write(p, off, data) => {
                let id = self.file_at(p)?;
                data_write(self.files.get_mut(&id).expect("arquivo"), *off, data);
                Ok(Out::Unit)
            }
            Op::Truncate(p, len) => {
                let id = self.file_at(p)?;
                self.files.get_mut(&id).expect("arquivo").resize(*len as usize, 0);
                Ok(Out::Unit)
            }
            Op::Read(p, off, len) => {
                let id = self.file_at(p)?;
                Ok(Out::Bytes(data_read(&self.files[&id], *off, *len)))
            }
            Op::Unlink(p) => {
                self.walk_parent(p)?;
                match self.paths.get(p).copied() {
                    None => Err(Errno::ENOENT),
                    Some(MEntry::Dir) => Err(Errno::EISDIR),
                    Some(MEntry::File(id)) => {
                        self.paths.remove(p);
                        self.gc(id);
                        Ok(Out::Unit)
                    }
                }
            }
            Op::Rmdir(p) => {
                if p == "/" {
                    return Err(Errno::EBUSY);
                }
                self.walk_parent(p)?;
                match self.paths.get(p) {
                    None => Err(Errno::ENOENT),
                    Some(MEntry::File(_)) => Err(Errno::ENOTDIR),
                    Some(MEntry::Dir) if !self.children(p).is_empty() => Err(Errno::ENOTEMPTY),
                    Some(MEntry::Dir) => {
                        self.paths.remove(p);
                        Ok(Out::Unit)
                    }
                }
            }
            Op::Rename(a, b) => self.rename(a, b),
            Op::Link(a, b) => {
                self.walk_parent(a)?;
                let src = *self.paths.get(a).ok_or(Errno::ENOENT)?;
                self.walk_parent(b)?;
                if self.paths.contains_key(b) {
                    return Err(Errno::EEXIST);
                }
                match src {
                    MEntry::Dir => Err(Errno::EPERM),
                    MEntry::File(id) => {
                        self.paths.insert(b.clone(), MEntry::File(id));
                        Ok(Out::Unit)
                    }
                }
            }
            Op::Stat(p) => {
                self.walk_parent(p)?;
                let e = *self.paths.get(p).ok_or(Errno::ENOENT)?;
                Ok(self.stat_of(p, e))
            }
            Op::Readdir(p) => {
                self.walk_parent(p)?;
                match self.paths.get(p) {
                    None => Err(Errno::ENOENT),
                    Some(MEntry::File(_)) => Err(Errno::ENOTDIR),
                    Some(MEntry::Dir) => {
                        let prefix = child_prefix(p);
                        Ok(Out::Names(self.children(p).iter().map(|(k, _)| k.as_bytes()[prefix.len()..].to_vec()).collect()))
                    }
                }
            }
            Op::Open(p) => {
                let id = self.file_at(p)?;
                self.fds.push(id);
                Ok(Out::Unit)
            }
            Op::Close(slot) => {
                if self.fds.is_empty() {
                    return Ok(Out::Skipped);
                }
                let id = self.fds.remove(slot % self.fds.len());
                self.gc(id);
                Ok(Out::Unit)
            }
            Op::PWrite(slot, off, data) => match self.live_fd(*slot)? {
                None => Ok(Out::Skipped),
                Some(id) => {
                    data_write(self.files.get_mut(&id).expect("arquivo"), *off, data);
                    Ok(Out::Unit)
                }
            },
            Op::PRead(slot, off, len) => match self.live_fd(*slot)? {
                None => Ok(Out::Skipped),
                Some(id) => Ok(Out::Bytes(data_read(&self.files[&id], *off, *len))),
            },
            Op::FStat(slot) => match self.live_fd(*slot)? {
                None => Ok(Out::Skipped),
                Some(id) => Ok(Out::Stat { kind: Kind::File, nlink: self.nlink_of(id), size: self.files[&id].len() as u64 }),
            },
            Op::Snapshot => {
                self.snaps.push((self.paths.clone(), self.files.clone()));
                Ok(Out::Unit)
            }
            Op::Restore(i) => {
                if self.snaps.is_empty() {
                    return Ok(Out::Skipped);
                }
                let (paths, files) = self.snaps[i % self.snaps.len()].clone();
                self.paths = paths;
                self.files = files;
                let ids: Vec<u64> = self.files.keys().copied().collect();
                for id in ids {
                    self.gc(id);
                }
                Ok(Out::Unit)
            }
        }
    }

    fn dump(&mut self) -> Dump {
        let mut out = Dump::new();
        let entries: Vec<(String, MEntry)> = self.paths.iter().map(|(k, v)| (k.clone(), *v)).collect();
        for (path, e) in entries {
            let Out::Stat { kind, nlink, size } = self.stat_of(&path, e) else { unreachable!() };
            let content = match e {
                MEntry::File(id) => Some(self.files[&id].clone()),
                MEntry::Dir => None,
            };
            out.insert(path, Node { kind, nlink, size, content });
        }
        out
    }
}

// ---------------------------------------------------------------------------------------------

/// Diretório de verdade num tmpfs do host. Apagado no `Drop`.
pub struct RealFs {
    root: PathBuf,
    fds: Vec<File>,
}

static REAL_COUNTER: AtomicU64 = AtomicU64::new(0);

impl RealFs {
    /// Cria um diretório vazio em `/dev/shm` (tmpfs), ou no temporário do sistema se não houver.
    pub fn new() -> std::io::Result<RealFs> {
        let base = if std::path::Path::new("/dev/shm").is_dir() { PathBuf::from("/dev/shm") } else { std::env::temp_dir() };
        let n = REAL_COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = base.join(format!("e03-check-{}-{n}", std::process::id()));
        if root.exists() {
            std::fs::remove_dir_all(&root)?;
        }
        std::fs::create_dir(&root)?;
        Ok(RealFs { root, fds: Vec::new() })
    }

    pub fn root(&self) -> &std::path::Path {
        &self.root
    }

    fn at(&self, p: &str) -> PathBuf {
        if p == "/" { self.root.clone() } else { self.root.join(&p[1..]) }
    }

    fn fd(&self, slot: usize) -> Option<&File> {
        if self.fds.is_empty() { None } else { Some(&self.fds[slot % self.fds.len()]) }
    }

    fn stat_meta(m: &std::fs::Metadata) -> Out {
        Out::Stat {
            kind: if m.is_dir() { Kind::Dir } else { Kind::File },
            nlink: m.nlink() as u32,
            size: m.len(),
        }
    }

    fn read_all(f: &File, off: u64, len: usize) -> std::io::Result<Vec<u8>> {
        let mut buf = vec![0; len];
        let mut done = 0;
        while done < len {
            let n = f.read_at(&mut buf[done..], off + done as u64)?;
            if n == 0 {
                break;
            }
            done += n;
        }
        buf.truncate(done);
        Ok(buf)
    }
}

impl Drop for RealFs {
    fn drop(&mut self) {
        self.fds.clear();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn io<T>(r: std::io::Result<T>) -> Result<T, Errno> {
    r.map_err(|e| Errno::from_io(&e))
}

impl Target for RealFs {
    fn apply(&mut self, op: &Op) -> OpResult {
        match op {
            Op::Mkdir(p) => io(std::fs::create_dir(self.at(p))).map(|_| Out::Unit),
            // `O_CREAT | O_WRONLY` sem `O_TRUNC`: arquivo existente fica como está.
            Op::Create(p) => {
                io(OpenOptions::new().write(true).create(true).truncate(false).open(self.at(p))).map(|_| Out::Unit)
            }
            Op::Write(p, off, data) => {
                let f = io(OpenOptions::new().write(true).open(self.at(p)))?;
                io(f.write_all_at(data, *off)).map(|_| Out::Unit)
            }
            Op::Truncate(p, len) => {
                let f = io(OpenOptions::new().write(true).open(self.at(p)))?;
                io(f.set_len(*len)).map(|_| Out::Unit)
            }
            Op::Read(p, off, len) => {
                let f = io(File::open(self.at(p)))?;
                io(RealFs::read_all(&f, *off, *len)).map(Out::Bytes)
            }
            Op::Unlink(p) => io(std::fs::remove_file(self.at(p))).map(|_| Out::Unit),
            Op::Rmdir(p) => io(std::fs::remove_dir(self.at(p))).map(|_| Out::Unit),
            Op::Rename(a, b) => io(std::fs::rename(self.at(a), self.at(b))).map(|_| Out::Unit),
            Op::Link(a, b) => io(std::fs::hard_link(self.at(a), self.at(b))).map(|_| Out::Unit),
            Op::Stat(p) => io(std::fs::symlink_metadata(self.at(p))).map(|m| RealFs::stat_meta(&m)),
            Op::Readdir(p) => {
                let rd = io(std::fs::read_dir(self.at(p)))?;
                let mut names = Vec::new();
                for e in rd {
                    names.push(io(e)?.file_name().into_encoded_bytes());
                }
                names.sort();
                Ok(Out::Names(names))
            }
            Op::Open(p) => {
                let f = io(OpenOptions::new().read(true).write(true).open(self.at(p)))?;
                self.fds.push(f);
                Ok(Out::Unit)
            }
            Op::Close(slot) => {
                if self.fds.is_empty() {
                    return Ok(Out::Skipped);
                }
                let n = self.fds.len();
                drop(self.fds.remove(slot % n));
                Ok(Out::Unit)
            }
            Op::PWrite(slot, off, data) => match self.fd(*slot) {
                None => Ok(Out::Skipped),
                Some(f) => io(f.write_all_at(data, *off)).map(|_| Out::Unit),
            },
            Op::PRead(slot, off, len) => match self.fd(*slot) {
                None => Ok(Out::Skipped),
                Some(f) => io(RealFs::read_all(f, *off, *len)).map(Out::Bytes),
            },
            Op::FStat(slot) => match self.fd(*slot) {
                None => Ok(Out::Skipped),
                Some(f) => io(f.metadata()).map(|m| RealFs::stat_meta(&m)),
            },
            Op::Snapshot | Op::Restore(_) => Ok(Out::Skipped),
        }
    }

    fn dump(&mut self) -> Dump {
        let mut out = Dump::new();
        let mut stack = vec!["/".to_string()];
        while let Some(path) = stack.pop() {
            let real = self.at(&path);
            let meta = std::fs::symlink_metadata(&real).expect("metadata");
            let kind = if meta.is_dir() { Kind::Dir } else { Kind::File };
            let content = if meta.is_dir() {
                for e in std::fs::read_dir(&real).expect("read_dir") {
                    let name = e.expect("entrada").file_name().into_string().expect("nome UTF-8");
                    stack.push(if path == "/" { format!("/{name}") } else { format!("{path}/{name}") });
                }
                None
            } else {
                Some(std::fs::read(&real).expect("read"))
            };
            out.insert(path, Node { kind, nlink: meta.nlink() as u32, size: meta.len(), content });
        }
        out
    }
}

// ---------------------------------------------------------------------------------------------

/// O nosso tmpfs como alvo.
pub struct Ours<F: Flavor> {
    pub vfs: Vfs<F>,
    fds: Vec<Fd>,
    snaps: Vec<Fs<F>>,
}

impl<F: Flavor> Default for Ours<F> {
    fn default() -> Self {
        Ours::new()
    }
}

impl<F: Flavor> Ours<F> {
    pub fn new() -> Self {
        Ours { vfs: Vfs::new(), fds: Vec::new(), snaps: Vec::new() }
    }

    fn fd(&self, slot: usize) -> Option<Fd> {
        if self.fds.is_empty() { None } else { Some(self.fds[slot % self.fds.len()]) }
    }

    fn stat_out(s: crate::fs::Stat) -> Out {
        Out::Stat { kind: s.kind, nlink: s.nlink, size: s.size }
    }
}

impl<F: Flavor> Target for Ours<F> {
    fn apply(&mut self, op: &Op) -> OpResult {
        let v = &mut self.vfs;
        match op {
            Op::Mkdir(p) => v.mkdir(p.as_bytes(), 0o755).map(|_| Out::Unit),
            Op::Create(p) => v.create(p.as_bytes(), 0o644).map(|_| Out::Unit),
            Op::Write(p, off, data) => v.write(p.as_bytes(), *off, data).map(|_| Out::Unit),
            Op::Truncate(p, len) => v.truncate(p.as_bytes(), *len).map(|_| Out::Unit),
            Op::Read(p, off, len) => v.read(p.as_bytes(), *off, *len).map(Out::Bytes),
            Op::Unlink(p) => v.unlink(p.as_bytes()).map(|_| Out::Unit),
            Op::Rmdir(p) => v.rmdir(p.as_bytes()).map(|_| Out::Unit),
            Op::Rename(a, b) => v.rename(a.as_bytes(), b.as_bytes()).map(|_| Out::Unit),
            Op::Link(a, b) => v.link(a.as_bytes(), b.as_bytes()).map(|_| Out::Unit),
            Op::Stat(p) => v.stat(p.as_bytes()).map(Ours::<F>::stat_out),
            Op::Readdir(p) => v.readdir(p.as_bytes()).map(|e| Out::Names(e.into_iter().map(|(n, _)| n.to_vec()).collect())),
            Op::Open(p) => {
                let fd = v.open(p.as_bytes(), false)?;
                self.fds.push(fd);
                Ok(Out::Unit)
            }
            Op::Close(slot) => {
                if self.fds.is_empty() {
                    return Ok(Out::Skipped);
                }
                let n = self.fds.len();
                let fd = self.fds.remove(slot % n);
                self.vfs.close(fd).map(|_| Out::Unit)
            }
            Op::PWrite(slot, off, data) => match self.fd(*slot) {
                None => Ok(Out::Skipped),
                Some(fd) => self.vfs.pwrite(fd, *off, data).map(|_| Out::Unit),
            },
            Op::PRead(slot, off, len) => match self.fd(*slot) {
                None => Ok(Out::Skipped),
                Some(fd) => self.vfs.pread(fd, *off, *len).map(Out::Bytes),
            },
            Op::FStat(slot) => match self.fd(*slot) {
                None => Ok(Out::Skipped),
                Some(fd) => self.vfs.fstat(fd).map(Ours::<F>::stat_out),
            },
            Op::Snapshot => {
                self.snaps.push(self.vfs.snapshot());
                Ok(Out::Unit)
            }
            Op::Restore(i) => {
                if self.snaps.is_empty() {
                    return Ok(Out::Skipped);
                }
                let snap = self.snaps[i % self.snaps.len()].clone();
                self.vfs.restore(&snap);
                Ok(Out::Unit)
            }
        }
    }

    fn dump(&mut self) -> Dump {
        let mut out = Dump::new();
        let mut stack = vec!["/".to_string()];
        while let Some(path) = stack.pop() {
            let st = self.vfs.stat(path.as_bytes()).expect("stat no dump");
            let content = match st.kind {
                Kind::Dir => {
                    for (name, _) in self.vfs.readdir(path.as_bytes()).expect("readdir no dump") {
                        let name = String::from_utf8(name.to_vec()).expect("nome UTF-8");
                        stack.push(if path == "/" { format!("/{name}") } else { format!("{path}/{name}") });
                    }
                    None
                }
                Kind::File => Some(self.vfs.read(path.as_bytes(), 0, st.size as usize).expect("read no dump")),
            };
            out.insert(path, Node { kind: st.kind, nlink: st.nlink, size: st.size, content });
        }
        out
    }
}

// ---------------------------------------------------------------------------------------------

/// Divergência entre dois alvos numa sequência.
#[derive(Clone, Debug)]
pub struct Divergence {
    pub step: Option<usize>,
    pub op: Option<Op>,
    pub left: String,
    pub right: String,
}

/// Roda `ops` nos dois alvos e devolve a primeira divergência (de resultado ou do retrato final).
pub fn compare(ops: &[Op], left: &mut dyn Target, right: &mut dyn Target) -> Option<Divergence> {
    for (i, op) in ops.iter().enumerate() {
        let a = left.apply(op);
        let b = right.apply(op);
        if a != b {
            return Some(Divergence { step: Some(i), op: Some(op.clone()), left: format!("{a:?}"), right: format!("{b:?}") });
        }
    }
    let a = left.dump();
    let b = right.dump();
    if a != b {
        let diff: Vec<String> = a
            .keys()
            .chain(b.keys())
            .filter(|k| a.get(*k) != b.get(*k))
            .take(3)
            .map(|k| format!("{k}: {:?} vs {:?}", a.get(k).map(short), b.get(k).map(short)))
            .collect();
        return Some(Divergence { step: None, op: None, left: diff.join("; "), right: String::new() });
    }
    None
}

fn short(n: &Node) -> String {
    format!("{:?} nlink={} size={}", n.kind, n.nlink, n.size)
}

// ---------------------------------------------------------------------------------------------
// Gerador de sequências (o mesmo universo de caminhos do proptest dos testes).

pub const NAMES: &[&str] = &["a", "b", "c"];
pub const MAX_OFF: u64 = 13_000;
pub const MAX_LEN: usize = 9_000;

pub fn random_path(rng: &mut Rng) -> String {
    let depth = 1 + rng.below(3) as usize;
    (0..depth).map(|_| format!("/{}", NAMES[rng.below(NAMES.len() as u64) as usize])).collect()
}

pub fn pattern(len: usize, seed: u8) -> Vec<u8> {
    (0..len).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed)).collect()
}

/// Sequência aleatória de `n` operações; `snapshots` liga Snapshot/Restore (só pra modelo x nosso).
pub fn random_ops(rng: &mut Rng, n: usize, snapshots: bool) -> Vec<Op> {
    let kinds = if snapshots { 18 } else { 16 };
    (0..n)
        .map(|_| {
            let off = rng.below(MAX_OFF);
            let len = rng.below(MAX_LEN as u64) as usize;
            let seed = rng.below(256) as u8;
            let slot = rng.below(8) as usize;
            match rng.below(kinds) {
                0 | 1 => Op::Mkdir(random_path(rng)),
                2 | 3 => Op::Create(random_path(rng)),
                4 => Op::Write(random_path(rng), off, pattern(len, seed)),
                5 => Op::Truncate(random_path(rng), off),
                6 => Op::Read(random_path(rng), off, len),
                7 => Op::Unlink(random_path(rng)),
                8 => Op::Rmdir(random_path(rng)),
                9 => Op::Rename(random_path(rng), random_path(rng)),
                10 => Op::Link(random_path(rng), random_path(rng)),
                11 => Op::Stat(random_path(rng)),
                12 => Op::Readdir(random_path(rng)),
                13 => Op::Open(random_path(rng)),
                14 => match rng.below(4) {
                    0 => Op::Close(slot),
                    1 => Op::PRead(slot, off, len),
                    _ => Op::FStat(slot),
                },
                15 => Op::PWrite(slot, off, pattern(len, seed)),
                16 => Op::Snapshot,
                _ => Op::Restore(slot),
            }
        })
        .collect()
}
