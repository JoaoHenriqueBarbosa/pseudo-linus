//! VFS em memória: tabela de inodes + diretórios como mapa nome -> inode, com resolução de caminho no
//! estilo do namei do Linux (symlink em componente intermediário sempre seguido, no último só quando
//! pedido, ELOOP depois de 40, ENOTDIR quando um componente intermediário não é diretório).
//!
//! Convenções que imitam o ext4 do oráculo (é ele que está por baixo do overlay do container):
//! diretório tem `st_size` 4096 e 8 blocos de 512 bytes; arquivo regular ocupa blocos de 4 KiB;
//! link simbólico curto não ocupa bloco. Todo mundo é root (uid 0, gid 0) e root ignora rwx.

use std::collections::{BTreeMap, VecDeque};
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::errno::*;

pub type Ino = u64;

/// Inode da raiz, igual ao ext4.
pub const ROOT: Ino = 2;
/// Número de dispositivo fixo (só aparece em comparações de identidade de arquivo).
pub const DEV: u64 = 0x803;
pub const MAX_SYMLINKS: usize = 40;

#[derive(Clone, Debug)]
pub enum NodeKind {
    File(Vec<u8>),
    Dir(BTreeMap<OsString, Ino>),
    Symlink(OsString),
}

#[derive(Clone, Debug)]
pub struct Inode {
    pub kind: NodeKind,
    /// Só os bits de permissão (07777).
    pub perm: u32,
    pub uid: u32,
    pub gid: u32,
    pub atime: SystemTime,
    pub mtime: SystemTime,
    pub ctime: SystemTime,
    /// Quantas entradas de diretório apontam pra este inode (arquivos e links).
    pub links: u32,
}

/// Retrato de um inode, o equivalente a `struct stat`.
#[derive(Clone, Debug)]
pub struct Stat {
    pub dev: u64,
    pub ino: Ino,
    pub mode: u32,
    pub nlink: u64,
    pub uid: u32,
    pub gid: u32,
    pub size: u64,
    pub blocks: u64,
    pub blksize: u64,
    pub atime: SystemTime,
    pub mtime: SystemTime,
    pub ctime: SystemTime,
}

/// Fim de uma resolução de caminho.
#[derive(Clone, Debug)]
pub struct Walk {
    /// Inode do último componente (None quando ele não existe, mas o pai existe).
    pub ino: Option<Ino>,
    /// Diretório pai do último componente.
    pub parent: Ino,
    /// Nome do último componente (None quando o caminho termina em `/`, `.` ou `..`).
    pub name: Option<OsString>,
    /// Caminho absoluto canônico do diretório pai.
    pub parent_path: PathBuf,
}

impl Walk {
    pub fn path(&self) -> PathBuf {
        match &self.name {
            Some(n) => self.parent_path.join(n),
            None => self.parent_path.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Vfs {
    inodes: BTreeMap<Ino, Inode>,
    next_ino: Ino,
}

impl Vfs {
    pub fn new(now: SystemTime) -> Vfs {
        let mut inodes = BTreeMap::new();
        inodes.insert(ROOT, dir_inode(0o755, now));
        Vfs { inodes, next_ino: 1_000 }
    }

    pub fn inode(&self, ino: Ino) -> &Inode {
        self.inodes.get(&ino).expect("inode existente")
    }

    pub fn inode_mut(&mut self, ino: Ino) -> &mut Inode {
        self.inodes.get_mut(&ino).expect("inode existente")
    }

    pub fn stat(&self, ino: Ino) -> Stat {
        let node = self.inode(ino);
        let (ty, size, blocks, nlink) = match &node.kind {
            NodeKind::File(d) => {
                let size = d.len() as u64;
                (S_IFREG, size, size.div_ceil(4096) * 8, node.links as u64)
            }
            NodeKind::Dir(entries) => {
                let subdirs = entries
                    .values()
                    .filter(|i| matches!(self.inode(**i).kind, NodeKind::Dir(_)))
                    .count() as u64;
                (S_IFDIR, 4096, 8, 2 + subdirs)
            }
            NodeKind::Symlink(t) => (S_IFLNK, t.len() as u64, 0, node.links as u64),
        };
        Stat {
            dev: DEV,
            ino,
            mode: ty | node.perm,
            nlink,
            uid: node.uid,
            gid: node.gid,
            size,
            blocks,
            blksize: 4096,
            atime: node.atime,
            mtime: node.mtime,
            ctime: node.ctime,
        }
    }

    fn alloc(&mut self, inode: Inode) -> Ino {
        let ino = self.next_ino;
        self.next_ino += 1;
        self.inodes.insert(ino, inode);
        ino
    }

    fn entries(&self, dir: Ino) -> Result<&BTreeMap<OsString, Ino>, i32> {
        match &self.inode(dir).kind {
            NodeKind::Dir(e) => Ok(e),
            _ => Err(ENOTDIR),
        }
    }

    fn entries_mut(&mut self, dir: Ino) -> Result<&mut BTreeMap<OsString, Ino>, i32> {
        match &mut self.inode_mut(dir).kind {
            NodeKind::Dir(e) => Ok(e),
            _ => Err(ENOTDIR),
        }
    }

    pub fn lookup(&self, dir: Ino, name: &OsStr) -> Option<Ino> {
        self.entries(dir).ok()?.get(name).copied()
    }

    /// Lista (nome, inode) de um diretório, em ordem de bytes do nome.
    pub fn list(&self, dir: Ino) -> Result<Vec<(OsString, Ino)>, i32> {
        Ok(self.entries(dir)?.iter().map(|(n, i)| (n.clone(), *i)).collect())
    }

    /// Resolve `path` relativo a `cwd` (absoluto e canônico). `follow` decide se um symlink no último
    /// componente é seguido.
    pub fn walk(&self, cwd: &Path, path: &Path, follow: bool) -> Result<Walk, i32> {
        let bytes = path.as_os_str().as_bytes();
        if bytes.is_empty() {
            return Err(ENOENT);
        }
        let trailing_slash = bytes.len() > 1 && bytes.ends_with(b"/");
        // Pilha de (inode, nome) do caminho atual, a partir da raiz.
        let mut stack: Vec<(Ino, OsString)> = vec![(ROOT, OsString::new())];
        let mut queue: VecDeque<OsString> = VecDeque::new();
        if !path.is_absolute() {
            for c in cwd.components() {
                if let Component::Normal(n) = c {
                    let cur = stack.last().expect("raiz").0;
                    let child = self.lookup(cur, n).ok_or(ENOENT)?;
                    stack.push((child, n.to_os_string()));
                }
            }
        }
        push_components(&mut queue, path);
        let mut links = 0usize;
        let mut last_name: Option<OsString> = None;
        while let Some(comp) = queue.pop_front() {
            match comp.as_bytes() {
                b"/" => {
                    stack.truncate(1);
                    last_name = None;
                }
                b"." => last_name = None,
                b".." => {
                    if stack.len() > 1 {
                        stack.pop();
                    }
                    last_name = None;
                }
                _ => {
                    let cur = stack.last().expect("raiz").0;
                    let entries = self.entries(cur)?;
                    let is_last = queue.is_empty();
                    match entries.get(&comp) {
                        None => {
                            if is_last {
                                return Ok(Walk {
                                    ino: None,
                                    parent: cur,
                                    name: Some(comp),
                                    parent_path: stack_path(&stack),
                                });
                            }
                            return Err(ENOENT);
                        }
                        Some(&child) => {
                            if let NodeKind::Symlink(target) = &self.inode(child).kind
                                && (!is_last || follow || trailing_slash)
                            {
                                links += 1;
                                if links > MAX_SYMLINKS {
                                    return Err(ELOOP);
                                }
                                let mut prefix = VecDeque::new();
                                push_components(&mut prefix, Path::new(target));
                                while let Some(c) = prefix.pop_back() {
                                    queue.push_front(c);
                                }
                                if queue.is_empty() {
                                    // Alvo vazio: o Linux devolve ENOENT.
                                    return Err(ENOENT);
                                }
                                continue;
                            }
                            if is_last {
                                if trailing_slash && !matches!(self.inode(child).kind, NodeKind::Dir(_)) {
                                    return Err(ENOTDIR);
                                }
                                return Ok(Walk {
                                    ino: Some(child),
                                    parent: cur,
                                    name: Some(comp),
                                    parent_path: stack_path(&stack),
                                });
                            }
                            if !matches!(self.inode(child).kind, NodeKind::Dir(_)) {
                                return Err(ENOTDIR);
                            }
                            stack.push((child, comp));
                            last_name = None;
                        }
                    }
                }
            }
        }
        let _ = last_name;
        // Terminou em "/", "." ou "..": o resultado é o topo da pilha.
        let ino = stack.last().expect("raiz").0;
        let parent = if stack.len() > 1 { stack[stack.len() - 2].0 } else { ino };
        Ok(Walk { ino: Some(ino), parent, name: None, parent_path: stack_path(&stack) })
    }

    /// Resolve e exige que o último componente exista.
    pub fn resolve(&self, cwd: &Path, path: &Path, follow: bool) -> Result<(Ino, PathBuf), i32> {
        let w = self.walk(cwd, path, follow)?;
        match w.ino {
            Some(ino) => Ok((ino, w.path())),
            None => Err(ENOENT),
        }
    }

    pub fn create_file(&mut self, parent: Ino, name: &OsStr, perm: u32, now: SystemTime) -> Result<Ino, i32> {
        let ino = self.alloc(Inode {
            kind: NodeKind::File(Vec::new()),
            perm,
            uid: 0,
            gid: 0,
            atime: now,
            mtime: now,
            ctime: now,
            links: 1,
        });
        self.link_entry(parent, name, ino, now)?;
        Ok(ino)
    }

    pub fn mkdir(&mut self, parent: Ino, name: &OsStr, perm: u32, now: SystemTime) -> Result<Ino, i32> {
        if self.lookup(parent, name).is_some() {
            return Err(EEXIST);
        }
        let ino = self.alloc(dir_inode(perm, now));
        self.link_entry(parent, name, ino, now)?;
        Ok(ino)
    }

    pub fn symlink(&mut self, parent: Ino, name: &OsStr, target: &OsStr, now: SystemTime) -> Result<Ino, i32> {
        if self.lookup(parent, name).is_some() {
            return Err(EEXIST);
        }
        let ino = self.alloc(Inode {
            kind: NodeKind::Symlink(target.to_os_string()),
            perm: 0o777,
            uid: 0,
            gid: 0,
            atime: now,
            mtime: now,
            ctime: now,
            links: 1,
        });
        self.link_entry(parent, name, ino, now)?;
        Ok(ino)
    }

    /// `link(2)`: mais uma entrada pro mesmo inode.
    pub fn link_existing(&mut self, parent: Ino, name: &OsStr, ino: Ino, now: SystemTime) -> Result<(), i32> {
        self.link_entry(parent, name, ino, now)?;
        let node = self.inode_mut(ino);
        node.links += 1;
        node.ctime = now;
        Ok(())
    }

    fn link_entry(&mut self, parent: Ino, name: &OsStr, ino: Ino, now: SystemTime) -> Result<(), i32> {
        if name.len() > 255 {
            return Err(ENAMETOOLONG);
        }
        self.entries_mut(parent)?.insert(name.to_os_string(), ino);
        let p = self.inode_mut(parent);
        p.mtime = now;
        p.ctime = now;
        Ok(())
    }

    /// Remove uma entrada que não é diretório.
    pub fn unlink(&mut self, parent: Ino, name: &OsStr, now: SystemTime) -> Result<(), i32> {
        let ino = self.lookup(parent, name).ok_or(ENOENT)?;
        if matches!(self.inode(ino).kind, NodeKind::Dir(_)) {
            return Err(EISDIR);
        }
        self.entries_mut(parent)?.remove(name);
        let p = self.inode_mut(parent);
        p.mtime = now;
        p.ctime = now;
        let node = self.inode_mut(ino);
        node.links = node.links.saturating_sub(1);
        node.ctime = now;
        Ok(())
    }

    pub fn rmdir(&mut self, parent: Ino, name: &OsStr, now: SystemTime) -> Result<(), i32> {
        let ino = self.lookup(parent, name).ok_or(ENOENT)?;
        match &self.inode(ino).kind {
            NodeKind::Dir(e) if e.is_empty() => {}
            NodeKind::Dir(_) => return Err(ENOTEMPTY),
            _ => return Err(ENOTDIR),
        }
        self.entries_mut(parent)?.remove(name);
        let p = self.inode_mut(parent);
        p.mtime = now;
        p.ctime = now;
        self.inodes.remove(&ino);
        Ok(())
    }

    /// `rename(2)` entre dois pais já resolvidos.
    pub fn rename(
        &mut self,
        from_parent: Ino,
        from_name: &OsStr,
        to_parent: Ino,
        to_name: &OsStr,
        now: SystemTime,
    ) -> Result<(), i32> {
        let ino = self.lookup(from_parent, from_name).ok_or(ENOENT)?;
        let src_is_dir = matches!(self.inode(ino).kind, NodeKind::Dir(_));
        if let Some(existing) = self.lookup(to_parent, to_name) {
            if existing == ino {
                return Ok(());
            }
            match (&self.inode(existing).kind, src_is_dir) {
                (NodeKind::Dir(e), true) if e.is_empty() => {
                    self.inodes.remove(&existing);
                }
                (NodeKind::Dir(_), true) => return Err(ENOTEMPTY),
                (NodeKind::Dir(_), false) => return Err(EISDIR),
                (_, true) => return Err(ENOTDIR),
                (_, false) => {
                    let n = self.inode_mut(existing);
                    n.links = n.links.saturating_sub(1);
                }
            }
        }
        self.entries_mut(from_parent)?.remove(from_name);
        self.entries_mut(to_parent)?.insert(to_name.to_os_string(), ino);
        for p in [from_parent, to_parent] {
            let node = self.inode_mut(p);
            node.mtime = now;
            node.ctime = now;
        }
        self.inode_mut(ino).ctime = now;
        Ok(())
    }

    /// Garante que `path` (absoluto, sem symlink) exista como diretório, criando o que faltar.
    pub fn ensure_dir(&mut self, path: &Path, perm: u32, now: SystemTime) -> Ino {
        let mut cur = ROOT;
        for c in path.components() {
            if let Component::Normal(n) = c {
                cur = match self.lookup(cur, n) {
                    Some(i) => i,
                    None => self.mkdir(cur, n, perm, now).expect("mkdir no VFS novo"),
                };
            }
        }
        cur
    }

    /// Carrega uma árvore (caminhos relativos em ordem de pai antes de filho) em `dir` (absoluto),
    /// com mtime `fixture_time`. Links simbólicos ficam com `now`, igual ao `MemTree::materialize` da
    /// bancada (que não acerta o mtime deles); `dir` também fica com `now`.
    pub fn load_tree<'a>(
        &mut self,
        dir: &Path,
        entries: impl IntoIterator<Item = (&'a str, TreeEntry)>,
        fixture_time: SystemTime,
        now: SystemTime,
    ) {
        let base = self.ensure_dir(dir, 0o755, now);
        for (rel, entry) in entries {
            let rel_path = Path::new(rel);
            let parent_path = rel_path.parent().unwrap_or(Path::new(""));
            let mut parent = base;
            for c in parent_path.components() {
                if let Component::Normal(n) = c {
                    parent = match self.lookup(parent, n) {
                        Some(i) => i,
                        None => self.mkdir(parent, n, 0o755, fixture_time).expect("mkdir"),
                    };
                }
            }
            let name = rel_path.file_name().expect("nome").to_os_string();
            match entry {
                TreeEntry::Dir { mode } => {
                    let ino = match self.lookup(parent, &name) {
                        Some(i) => i,
                        None => self.mkdir(parent, &name, mode, fixture_time).expect("mkdir"),
                    };
                    self.inode_mut(ino).perm = mode & 0o7777;
                }
                TreeEntry::File { mode, data } => {
                    let ino = self.create_file(parent, &name, mode & 0o7777, fixture_time).expect("criar");
                    if let NodeKind::File(d) = &mut self.inode_mut(ino).kind {
                        *d = data;
                    }
                }
                TreeEntry::Symlink { target } => {
                    self.symlink(parent, &name, OsStr::new(&target), now).expect("symlink");
                }
            }
        }
        // Igual ao materialize: todo mundo da fixture termina com o mtime fixo (diretórios inclusive,
        // já que o materialize acerta os tempos depois de criar os filhos).
        let mut stack = vec![base];
        while let Some(dir_ino) = stack.pop() {
            for (_, child) in self.list(dir_ino).unwrap_or_default() {
                let node = self.inode_mut(child);
                match node.kind {
                    NodeKind::Symlink(_) => {}
                    NodeKind::Dir(_) => {
                        node.mtime = fixture_time;
                        node.atime = fixture_time;
                        stack.push(child);
                    }
                    NodeKind::File(_) => {
                        node.mtime = fixture_time;
                        node.atime = fixture_time;
                    }
                }
            }
        }
        let root = self.inode_mut(base);
        root.mtime = now;
        root.ctime = now;
    }

    /// Retrato de `dir`: caminhos relativos e o conteúdo de cada entrada.
    pub fn snapshot(&self, dir: &Path) -> Vec<(String, TreeEntry)> {
        let mut out = Vec::new();
        let Ok((base, _)) = self.resolve(Path::new("/"), dir, true) else {
            return out;
        };
        let mut stack = vec![(base, String::new())];
        while let Some((ino, prefix)) = stack.pop() {
            for (name, child) in self.list(ino).unwrap_or_default() {
                let rel = if prefix.is_empty() {
                    name.to_string_lossy().into_owned()
                } else {
                    format!("{prefix}/{}", name.to_string_lossy())
                };
                let node = self.inode(child);
                match &node.kind {
                    NodeKind::Dir(_) => {
                        out.push((rel.clone(), TreeEntry::Dir { mode: node.perm }));
                        stack.push((child, rel));
                    }
                    NodeKind::File(d) => out.push((rel, TreeEntry::File { mode: node.perm, data: d.clone() })),
                    NodeKind::Symlink(t) => {
                        out.push((rel, TreeEntry::Symlink { target: t.to_string_lossy().into_owned() }))
                    }
                }
            }
        }
        out
    }
}

/// Entrada de uma árvore carregada ou retratada (o formato de fixture da bancada, sem depender dela).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeEntry {
    Dir { mode: u32 },
    File { mode: u32, data: Vec<u8> },
    Symlink { target: String },
}

fn dir_inode(perm: u32, now: SystemTime) -> Inode {
    Inode {
        kind: NodeKind::Dir(BTreeMap::new()),
        perm,
        uid: 0,
        gid: 0,
        atime: now,
        mtime: now,
        ctime: now,
        links: 1,
    }
}

fn stack_path(stack: &[(Ino, OsString)]) -> PathBuf {
    let mut p = PathBuf::from("/");
    for (_, n) in &stack[1..] {
        p.push(n);
    }
    p
}

/// Quebra um caminho em componentes crus: "/" pra raiz, "." e ".." literais, e nomes.
fn push_components(queue: &mut VecDeque<OsString>, path: &Path) {
    let bytes = path.as_os_str().as_bytes();
    if bytes.starts_with(b"/") {
        queue.push_back(OsString::from("/"));
    }
    for part in bytes.split(|b| *b == b'/') {
        if part.is_empty() {
            continue;
        }
        queue.push_back(OsStr::from_bytes(part).to_os_string());
    }
}

/// Converte segundos desde a época em `SystemTime`.
pub fn epoch(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vfs {
        let now = epoch(2_000_000_000);
        let mut v = Vfs::new(now);
        let file = |data: &str, mode| TreeEntry::File { mode, data: data.as_bytes().to_vec() };
        let link = |t: &str| TreeEntry::Symlink { target: t.to_string() };
        let tree = vec![
            ("a.txt", file("abc", 0o644)),
            ("d/e/f.txt", file("x", 0o600)),
            ("link", link("d/e")),
            ("abs", link("/work/case/a.txt")),
            ("loop", link("loop")),
        ];
        v.load_tree(Path::new("/work/case"), tree, epoch(1_768_478_400), now);
        v
    }

    #[test]
    fn walk_follows_symlinks_and_detects_errors() {
        let v = sample();
        let cwd = Path::new("/work/case");
        let (ino, path) = v.resolve(cwd, Path::new("link/f.txt"), true).unwrap();
        assert_eq!(path, Path::new("/work/case/d/e/f.txt"));
        assert_eq!(v.stat(ino).size, 1);
        assert_eq!(v.resolve(cwd, Path::new("abs"), true).unwrap().1, Path::new("/work/case/a.txt"));
        assert_eq!(v.resolve(cwd, Path::new("loop"), true).unwrap_err(), ELOOP);
        assert_eq!(v.resolve(cwd, Path::new("a.txt/x"), true).unwrap_err(), ENOTDIR);
        assert_eq!(v.resolve(cwd, Path::new("a.txt/"), true).unwrap_err(), ENOTDIR);
        assert_eq!(v.resolve(cwd, Path::new("nope/x"), true).unwrap_err(), ENOENT);
        assert_eq!(v.resolve(cwd, Path::new("d/../a.txt"), true).unwrap().1, Path::new("/work/case/a.txt"));
        let lstat = v.stat(v.resolve(cwd, Path::new("link"), false).unwrap().0);
        assert_eq!(lstat.mode & S_IFMT, S_IFLNK);
        assert_eq!(v.stat(v.resolve(cwd, Path::new("d"), true).unwrap().0).nlink, 3);
    }

    #[test]
    fn snapshot_roundtrip() {
        let v = sample();
        let tree: std::collections::BTreeMap<String, TreeEntry> =
            v.snapshot(Path::new("/work/case")).into_iter().collect();
        assert_eq!(tree.get("d/e/f.txt"), Some(&TreeEntry::File { mode: 0o600, data: b"x".to_vec() }));
        assert!(matches!(tree.get("link"), Some(TreeEntry::Symlink { .. })));
        assert_eq!(tree.get("d/e"), Some(&TreeEntry::Dir { mode: 0o755 }));
    }
}
