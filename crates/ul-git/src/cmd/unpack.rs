//! Troca de árvore de trabalho e índice: o two-way merge do `read-tree -m -u` (HEAD para o destino)
//! que o `checkout`, o `switch` e o `reset --merge` usam, e a versão forçada (`checkout -f`,
//! `reset --hard`). Quando algo local seria perdido, nada é tocado e as mensagens saem como no git.

use std::collections::{BTreeMap, BTreeSet};

use sysabi::FileType;

use crate::diff::{self, WtState};
use crate::error::{Fail, R, error};
use crate::hash::Oid;
use crate::ignore::Ignores;
use crate::index::{IEntry, Index};
use crate::os;
use crate::pathspec::Pathspec;
use crate::quote;
use crate::repo::Repo;
use crate::worktree;

pub struct Opts {
    /// Como as mensagens chamam a operação (`checkout`, `reset`).
    pub verb: &'static str,
    /// O que o usuário deveria fazer antes (`switch branches`, `reset`).
    pub advice: &'static str,
    /// Sobrescreve tudo o que for local.
    pub force: bool,
}

type Files = BTreeMap<Vec<u8>, (u32, Oid)>;

fn same(e: &IEntry, t: &(u32, Oid)) -> bool {
    e.mode == t.0 && e.oid == t.1
}

fn same_tree(a: &(u32, Oid), b: &(u32, Oid)) -> bool {
    a.0 == b.0 && a.1 == b.1
}

fn is_ignored(repo: &Repo, ign: &mut Option<Ignores>, path: &[u8], is_dir: bool) -> bool {
    ign.get_or_insert_with(|| Ignores::standard(repo)).is_ignored(path, is_dir)
}

/// Algum arquivo sob `dir` que o índice não conhece?
fn dir_has_untracked(idx: &Index, dir: &[u8]) -> bool {
    let Ok(entries) = os::read_dir(dir) else { return false };
    for e in entries {
        if e.name == b".git" {
            continue;
        }
        let full = os::join(dir, &e.name);
        let is_dir = os::lstat(&full).map(|s| s.file_type() == FileType::Directory).unwrap_or(false);
        if is_dir {
            if dir_has_untracked(idx, &full) {
                return true;
            }
        } else if !idx.has_path(&full) {
            return true;
        }
    }
    false
}

/// Há algo não rastreado no caminho do arquivo novo? `(tipo de rejeição, caminho)`.
fn verify_absent(repo: &Repo, idx: &Index, path: &[u8], ign: &mut Option<Ignores>) -> Option<(usize, Vec<u8>)> {
    let mut k = 0;
    while let Some(s) = path[k..].iter().position(|c| *c == b'/') {
        let anc = &path[..k + s];
        match os::lstat(anc) {
            Ok(st) => {
                if st.file_type() != FileType::Directory && !idx.has_path(anc) && !is_ignored(repo, ign, anc, false) {
                    return Some((3, anc.to_vec()));
                }
            }
            Err(_) => break,
        }
        k += s + 1;
    }
    match os::lstat(path) {
        Err(_) => None,
        Ok(st) => {
            if st.file_type() == FileType::Directory {
                if dir_has_untracked(idx, path) { Some((2, path.to_vec())) } else { None }
            } else if idx.has_path(path) || is_ignored(repo, ign, path, false) {
                None
            } else {
                Some((3, path.to_vec()))
            }
        }
    }
}

fn reject_message(kind: usize, list: &[Vec<u8>], o: &Opts, advice_on: bool) -> String {
    let mut m = match kind {
        0 | 1 => format!("Your local changes to the following files would be overwritten by {}:\n", o.verb),
        2 => "Updating the following directories would lose untracked files in them:\n".to_string(),
        _ => format!("The following untracked working tree files would be overwritten by {}:\n", o.verb),
    };
    for p in list {
        m.push('\t');
        m.push_str(&os::lossy(p));
        m.push('\n');
    }
    if advice_on {
        match kind {
            0 | 1 => m.push_str(&format!("Please commit your changes or stash them before you {}.\n", o.advice)),
            3 => m.push_str(&format!("Please move or remove them before you {}.\n", o.advice)),
            _ => {}
        }
    }
    m.push_str("Aborting");
    m
}

/// Leva índice e árvore de trabalho da árvore `old` pra `new`, mantendo o que é local. Devolve o
/// índice novo (quem chama grava). Se algo local seria sobrescrito, imprime o erro do git e
/// devolve `Fail::Exit(1)` sem ter tocado em nada.
pub fn switch_tree(repo: &Repo, idx: &Index, old: Option<&Oid>, new: &Oid, o: &Opts) -> R<Index> {
    let new_files: Files = repo.flatten_tree(new)?;
    let trust = repo.config.get_bool("core.filemode")?.unwrap_or(true);
    if o.force {
        return force_tree(repo, idx, &new_files, trust);
    }
    let unmerged = idx.unmerged_paths();
    if !unmerged.is_empty() {
        for p in &unmerged {
            os::outs(&format!("{}: needs merge\n", os::lossy(p)));
        }
        error("you need to resolve your current index first");
        return Err(Fail::Exit(1));
    }
    let old_files: Files = match old {
        Some(t) => repo.flatten_tree(t)?,
        None => Files::new(),
    };
    let initial = !idx.existed || idx.entries.is_empty();
    let mut paths: BTreeSet<Vec<u8>> = BTreeSet::new();
    paths.extend(old_files.keys().cloned());
    paths.extend(new_files.keys().cloned());
    paths.extend(idx.entries.iter().map(|e| e.path.clone()));

    let mut dels: Vec<Vec<u8>> = Vec::new();
    let mut ups: Vec<(Vec<u8>, u32, Oid)> = Vec::new();
    let mut rejects: [Vec<Vec<u8>>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
    let mut ign: Option<Ignores> = None;
    for path in &paths {
        let cur = idx.get(path);
        let ot = old_files.get(path);
        let nt = new_files.get(path);
        match cur {
            Some(c) => {
                let uptodate = |c: &IEntry| -> R<bool> { Ok(!matches!(diff::check_entry(repo, idx, c, trust)?, WtState::Changed(..))) };
                if (ot.is_none() && nt.is_none())
                    || (ot.is_none() && nt.is_some_and(|n| same(c, n)))
                    || (ot.is_some() && nt.is_some() && ot.zip(nt).is_some_and(|(a, b)| same_tree(a, b)))
                    || (ot.is_some() && nt.is_some() && nt.is_some_and(|n| same(c, n)))
                {
                    // Fica como está.
                } else if ot.is_some_and(|ot| same(c, ot)) && nt.is_none() {
                    if uptodate(c)? {
                        dels.push(path.clone());
                    } else {
                        rejects[1].push(path.clone());
                    }
                } else if ot.is_some_and(|ot| same(c, ot)) && nt.is_some_and(|n| !same(c, n)) {
                    if uptodate(c)? {
                        let (mode, oid) = nt.copied().unwrap_or((0, Oid::ZERO));
                        ups.push((path.clone(), mode, oid));
                    } else {
                        rejects[1].push(path.clone());
                    }
                } else {
                    rejects[0].push(path.clone());
                }
            }
            None => if let Some(n) = nt {
                if let Some(ot) = ot
                    && !initial
                {
                    // A remoção do caminho foi preparada no índice.
                    if !same_tree(ot, n) {
                        rejects[0].push(path.clone());
                    }
                    continue;
                }
                match verify_absent(repo, idx, path, &mut ign) {
                    Some((kind, p)) => {
                        if !rejects[kind].contains(&p) {
                            rejects[kind].push(p);
                        }
                    }
                    None => ups.push((path.clone(), n.0, n.1)),
                }
            },
        }
    }
    if rejects.iter().any(|r| !r.is_empty()) {
        let advice_on = repo.config.get_bool("advice.commitbeforemerge")?.unwrap_or(true);
        for (kind, list) in rejects.iter().enumerate() {
            if !list.is_empty() {
                error(&reject_message(kind, list, o, advice_on));
            }
        }
        return Err(Fail::Exit(1));
    }

    let mut out = idx.clone();
    for d in &dels {
        worktree::unlink_entry(d);
        out.remove(d);
    }
    for (path, mode, oid) in &ups {
        let e = worktree::checkout_entry(repo, path, *mode, oid)?;
        out.add(e);
    }
    Ok(out)
}

/// Versão forçada: o índice vira a árvore nova, arquivos locais são sobrescritos e os que sobram
/// do índice antigo somem. Não rastreados ficam.
fn force_tree(repo: &Repo, idx: &Index, new_files: &Files, trust: bool) -> R<Index> {
    let mut out = Index { version: 2, existed: idx.existed, mtime: idx.mtime, ..Index::default() };
    let mut removed: Vec<Vec<u8>> = Vec::new();
    for e in &idx.entries {
        if !new_files.contains_key(&e.path) && removed.last() != Some(&e.path) {
            removed.push(e.path.clone());
        }
    }
    for p in &removed {
        worktree::unlink_entry(p);
    }
    for (path, (mode, oid)) in new_files {
        let kept = match idx.get(path) {
            Some(c) if c.mode == *mode && c.oid == *oid => matches!(diff::check_entry(repo, idx, c, trust)?, WtState::Same),
            _ => false,
        };
        if kept && let Some(c) = idx.get(path) {
            out.add(c.clone());
        } else {
            let e = worktree::checkout_entry(repo, path, *mode, oid)?;
            out.add(e);
        }
    }
    Ok(out)
}

/// As linhas `M\tcaminho` do que ficou diferente da árvore `tree` (o `diff-index --name-status`
/// que o git mostra depois de trocar de ramo com mudanças locais).
pub fn local_changes(repo: &Repo, tree: &Oid) -> R<Vec<u8>> {
    let idx = Index::load(&repo.index_path())?;
    let pairs = diff::diff_tree_worktree(repo, Some(tree), &idx, &Pathspec::default())?;
    let fully = super::ls::quote_fully(repo);
    let mut out: Vec<u8> = Vec::new();
    for p in &pairs {
        out.push(p.status);
        out.push(b'\t');
        out.extend_from_slice(&quote::quote_c(p.path(), fully));
        out.push(b'\n');
    }
    Ok(out)
}
