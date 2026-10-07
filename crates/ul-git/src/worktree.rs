//! A árvore de trabalho: varredura de arquivos não rastreados e ignorados (o `read_directory` do
//! git, com o colapso de diretório do status) e a escrita e remoção de arquivos a partir de blobs.

use sysabi::FileType;

use crate::error::{Fail, R};
use crate::hash::Oid;
use crate::ignore::Ignores;
use crate::index::{IEntry, Index};
use crate::object;
use crate::os;
use crate::pathspec::Pathspec;
use crate::repo::Repo;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum UntrackedMode {
    No,
    /// Diretório sem nada rastreado aparece como `dir/`.
    Normal,
    /// Cada arquivo.
    All,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum IgnoredMode {
    No,
    /// `--ignored` (tradicional): diretório inteiro ignorado vira `dir/`.
    Traditional,
    /// `--ignored=matching`: o caminho que casou com o padrão.
    Matching,
}

#[derive(Default, Debug)]
pub struct Scan {
    /// Caminhos relativos ao topo; diretórios terminam em `/`.
    pub untracked: Vec<Vec<u8>>,
    pub ignored: Vec<Vec<u8>>,
}

pub struct Scanner<'a> {
    pub idx: &'a Index,
    pub ign: Ignores,
    pub ps: &'a Pathspec,
    pub untracked: UntrackedMode,
    pub ignored: IgnoredMode,
    /// Mostrar diretório vazio (o `git clean -d` e o `ls-files --directory` sem `--no-empty-directory`).
    pub show_empty_dirs: bool,
}

pub(crate) fn sorted_dir(path: &[u8]) -> Vec<(Vec<u8>, FileType)> {
    let p = if path.is_empty() { b".".as_slice() } else { path };
    let mut v: Vec<(Vec<u8>, FileType)> = match os::read_dir(p) {
        Ok(e) => e.into_iter().map(|d| (d.name, d.kind)).collect(),
        Err(_) => Vec::new(),
    };
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

pub(crate) fn join_rel(dir: &[u8], name: &[u8]) -> Vec<u8> {
    if dir.is_empty() {
        name.to_vec()
    } else {
        let mut p = dir.to_vec();
        p.push(b'/');
        p.extend_from_slice(name);
        p
    }
}

impl Scanner<'_> {
    /// Varre a partir do topo (o cwd do processo é o topo da árvore de trabalho).
    pub fn run(&mut self) -> Scan {
        let mut s = Scan::default();
        if self.untracked == UntrackedMode::No && self.ignored == IgnoredMode::No {
            return s;
        }
        self.walk(b"", &mut s);
        s.untracked.sort();
        s.untracked.dedup();
        s.ignored.sort();
        s.ignored.dedup();
        s
    }

    fn kind_of(&self, path: &[u8], kind: FileType) -> FileType {
        if kind == FileType::Symlink || kind == FileType::Directory || kind == FileType::Regular {
            return kind;
        }
        os::lstat(path).map(|st| st.file_type()).unwrap_or(kind)
    }

    fn walk(&mut self, dir: &[u8], s: &mut Scan) {
        for (name, kind) in sorted_dir(dir) {
            if name == b".git" {
                continue;
            }
            let path = join_rel(dir, &name);
            let kind = self.kind_of(&path, kind);
            if kind == FileType::Directory {
                self.visit_dir(&path, s);
            } else {
                if self.idx.has_path(&path) {
                    continue;
                }
                if !self.ps.matches_simple(&path) {
                    continue;
                }
                if self.ign.is_ignored_here(&path, false) {
                    if self.ignored != IgnoredMode::No {
                        s.ignored.push(path);
                    }
                    continue;
                }
                if self.untracked != UntrackedMode::No {
                    s.untracked.push(path);
                }
            }
        }
    }

    fn visit_dir(&mut self, path: &[u8], s: &mut Scan) {
        // Submódulo rastreado.
        if let Some(e) = self.idx.get(path)
            && object::is_gitlink(e.mode)
        {
            return;
        }
        if !self.ps.may_match_under(path) && self.ps.matches(path, true, None).is_none() {
            return;
        }
        let mut slash = path.to_vec();
        slash.push(b'/');
        if self.idx.has_dir(path) {
            if self.ign.is_ignored_here(path, true) && self.ignored != IgnoredMode::No {
                // Diretório ignorado com arquivos rastreados: os não rastreados dentro são ignorados.
                let mut inner = Scan::default();
                let saved = (self.untracked, self.ignored);
                self.collect_all(path, &mut inner);
                self.untracked = saved.0;
                self.ignored = saved.1;
                s.ignored.extend(inner.untracked);
                return;
            }
            self.walk(path, s);
            return;
        }
        let ignored = self.ign.is_ignored_here(path, true);
        if ignored {
            if self.ignored != IgnoredMode::No {
                match self.ignored {
                    IgnoredMode::Matching => s.ignored.push(slash),
                    _ => {
                        if self.untracked == UntrackedMode::All {
                            let mut inner = Vec::new();
                            self.all_files(path, &mut inner);
                            if inner.is_empty() {
                                if self.show_empty_dirs {
                                    s.ignored.push(slash);
                                }
                            } else {
                                s.ignored.extend(inner);
                            }
                        } else {
                            s.ignored.push(slash);
                        }
                    }
                }
            }
            return;
        }
        // Repositório aninhado.
        if os::exists(&os::join(path, b".git")) {
            if self.untracked != UntrackedMode::No && self.ps.matches(path, true, None).is_some() {
                s.untracked.push(slash);
            }
            return;
        }
        match self.untracked {
            UntrackedMode::All => {
                let before = s.untracked.len();
                self.walk(path, s);
                let _ = before;
            }
            UntrackedMode::Normal => {
                let mut inner = Scan::default();
                self.walk(path, &mut inner);
                let whole_dir = self.ps.matches(path, true, None).is_some();
                if !inner.untracked.is_empty() {
                    if whole_dir {
                        s.untracked.push(slash);
                    } else {
                        s.untracked.extend(inner.untracked);
                    }
                } else if self.show_empty_dirs && inner.ignored.is_empty() && whole_dir {
                    s.untracked.push(slash);
                }
                s.ignored.extend(inner.ignored);
            }
            UntrackedMode::No => {
                if self.ignored != IgnoredMode::No {
                    let mut inner = Scan::default();
                    let saved = self.untracked;
                    self.untracked = UntrackedMode::All;
                    self.walk(path, &mut inner);
                    self.untracked = saved;
                    s.ignored.extend(inner.ignored);
                }
            }
        }
    }

    /// Todos os arquivos não rastreados dentro de `dir` (pra diretório ignorado).
    fn all_files(&mut self, dir: &[u8], out: &mut Vec<Vec<u8>>) {
        for (name, kind) in sorted_dir(dir) {
            if name == b".git" {
                continue;
            }
            let path = join_rel(dir, &name);
            let kind = self.kind_of(&path, kind);
            if kind == FileType::Directory {
                self.all_files(&path, out);
            } else if !self.idx.has_path(&path) {
                out.push(path);
            }
        }
    }

    fn collect_all(&mut self, dir: &[u8], s: &mut Scan) {
        let mut v = Vec::new();
        self.all_files(dir, &mut v);
        s.untracked.extend(v);
    }
}

/// Arquivos não rastreados (com `dir/` colapsado no modo normal), respeitando ignorados.
pub fn untracked(repo: &Repo, idx: &Index, ps: &Pathspec, mode: UntrackedMode) -> Vec<Vec<u8>> {
    untracked_using(Ignores::standard(repo), idx, ps, mode)
}

/// Os não rastreados com as regras de ignorar dadas (`Ignores::none()` traz os ignorados também).
pub fn untracked_using(ign: Ignores, idx: &Index, ps: &Pathspec, mode: UntrackedMode) -> Vec<Vec<u8>> {
    let mut sc = Scanner { idx, ign, ps, untracked: mode, ignored: IgnoredMode::No, show_empty_dirs: false };
    sc.run().untracked
}

// ---- escrita de arquivos ----------------------------------------------------------------------

/// Remove o que estiver no caminho (arquivo, link ou diretório inteiro).
pub fn remove_path(path: &[u8]) -> R<()> {
    match os::lstat(path) {
        Ok(st) if st.file_type() == FileType::Directory => {
            os::remove_tree(path).map_err(|e| Fail::Fatal(format!("unable to remove directory {}: {}", os::lossy(path), e.message())))
        }
        Ok(_) => os::unlink(path).map_err(|e| Fail::Fatal(format!("unable to unlink {}: {}", os::lossy(path), e.message()))),
        Err(_) => Ok(()),
    }
}

/// Garante os diretórios pais, trocando arquivo que esteja no caminho por diretório.
fn make_parents(path: &[u8]) -> R<()> {
    let mut k = 0;
    while let Some(s) = path[k..].iter().position(|c| *c == b'/') {
        let d = &path[..k + s];
        match os::lstat(d) {
            Ok(st) if st.file_type() == FileType::Directory => {}
            Ok(_) => {
                os::unlink(d).map_err(|e| Fail::Fatal(format!("unable to unlink {}: {}", os::lossy(d), e.message())))?;
                os::mkdir(d, 0o777).map_err(|e| Fail::Fatal(format!("unable to create directory {}: {}", os::lossy(d), e.message())))?;
            }
            Err(_) => {
                os::mkdir(d, 0o777).map_err(|e| Fail::Fatal(format!("unable to create directory {}: {}", os::lossy(d), e.message())))?;
            }
        }
        k += s + 1;
    }
    Ok(())
}

/// Escreve um blob no caminho (relativo ao topo) e devolve a entrada de índice com o `stat` novo.
pub fn checkout_entry(repo: &Repo, path: &[u8], mode: u32, oid: &Oid) -> R<IEntry> {
    make_parents(path)?;
    if object::is_gitlink(mode) {
        if !os::is_dir(path) {
            remove_path(path)?;
            os::mkdir(path, 0o777).map_err(|e| Fail::Fatal(format!("unable to create directory {}: {}", os::lossy(path), e.message())))?;
        }
        let st = os::lstat(path).map_err(|e| Fail::Fatal(e.message().to_string()))?;
        return Ok(IEntry::from_stat(path.to_vec(), *oid, mode, &st));
    }
    let (_, data) = repo.read_object(oid)?;
    if let Ok(st) = os::lstat(path) {
        if st.file_type() == FileType::Directory {
            remove_path(path)?;
        } else {
            let _ = os::unlink(path);
        }
    }
    if object::is_link(mode) {
        os::symlink(&data, path).map_err(|e| Fail::Fatal(format!("unable to create symlink {}: {}", os::lossy(path), e.message())))?;
    } else {
        let perm = if mode & 0o111 != 0 { 0o777 } else { 0o666 };
        os::write(path, &data, perm).map_err(|e| Fail::Fatal(format!("unable to create file {}: {}", os::lossy(path), e.message())))?;
    }
    let st = os::lstat(path).map_err(|e| Fail::Fatal(format!("unable to stat just-written file {}: {}", os::lossy(path), e.message())))?;
    Ok(IEntry::from_stat(path.to_vec(), *oid, mode, &st))
}

/// Apaga o arquivo e os diretórios que ficarem vazios.
pub fn unlink_entry(path: &[u8]) {
    match os::lstat(path) {
        Ok(st) if st.file_type() == FileType::Directory => {
            // Submódulo: só se vazio.
            let _ = os::rmdir(path);
        }
        Ok(_) => {
            let _ = os::unlink(path);
        }
        Err(_) => return,
    }
    let mut d = os::dirname(path).to_vec();
    while !d.is_empty() {
        if os::rmdir(&d).is_err() {
            break;
        }
        d = os::dirname(&d).to_vec();
    }
}

/// Atualiza o `stat` de entradas cujo conteúdo não mudou (o `refresh_index`). Devolve se mudou algo.
pub fn refresh(idx: &mut Index, trust_exec: bool) -> bool {
    let mut changed = false;
    let racy_ref = idx.mtime;
    let mut updates: Vec<(usize, sysabi::Stat)> = Vec::new();
    for (i, e) in idx.entries.iter().enumerate() {
        if e.stage != 0 || e.intent_to_add() || e.skip_worktree() || object::is_gitlink(e.mode) {
            continue;
        }
        let Ok(st) = os::lstat(&e.path) else { continue };
        let racy = match racy_ref {
            None => true,
            Some(m) => (m.sec as u32, m.nsec) <= e.mtime,
        };
        if crate::index::stat_matches(e, &st, trust_exec) && !racy {
            continue;
        }
        if st.file_type() == FileType::Directory {
            continue;
        }
        let Ok(data) = crate::diff::worktree_blob(&e.path, &st) else { continue };
        let id = crate::hash::hash_object(crate::hash::Kind::Blob, &data);
        let mode = crate::index::mode_for(&st, Some(e.mode), trust_exec);
        if id == e.oid && object::canon_mode(mode) == object::canon_mode(e.mode) {
            updates.push((i, st));
        }
    }
    for (i, st) in updates {
        let e = &mut idx.entries[i];
        let before = e.clone();
        e.set_stat(&st);
        if *e != before {
            changed = true;
        }
    }
    changed
}
