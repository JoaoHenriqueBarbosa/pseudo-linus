//! `git restore` e o miolo de "arquivos de uma árvore ou do índice" que o `checkout -- <caminhos>`
//! também usa: copia entradas pro índice e/ou pra árvore de trabalho.

use std::collections::BTreeMap;

use super::Git;
use crate::diff::{self, WtState};
use crate::error::{Fail, R, error};
use crate::hash::Oid;
use crate::index::{IEntry, Index};
use crate::opts::{self, Spec};
use crate::os;
use crate::pathspec::Pathspec;
use crate::repo::Repo;
use crate::worktree;

const SPECS: &[Spec] = &[
    opts::value(Some(b's'), "source", "source"),
    opts::flag(Some(b'S'), "staged", "staged"),
    opts::flag(Some(b'W'), "worktree", "worktree"),
    opts::flag(None, "ignore-unmerged", "ignore-unmerged"),
    opts::flag(None, "overlay", "overlay"),
    opts::flag(Some(b'q'), "quiet", "quiet"),
    opts::optional(None, "recurse-submodules", "recurse-submodules"),
    opts::flag(None, "progress", "progress"),
    opts::flag(Some(b'm'), "merge", "merge"),
    opts::value(None, "conflict", "conflict"),
    opts::noneg(opts::flag(Some(b'2'), "ours", "ours")),
    opts::noneg(opts::flag(Some(b'3'), "theirs", "theirs")),
    opts::flag(Some(b'p'), "patch", "patch"),
    opts::flag(None, "ignore-skip-worktree-bits", "ignore-skip-worktree-bits"),
    opts::value(None, "pathspec-from-file", "pathspec-from-file"),
    opts::flag(None, "pathspec-file-nul", "pathspec-file-nul"),
];

/// De onde as entradas vêm.
pub enum Source {
    Index,
    Tree(Oid),
}

pub struct PathOpts {
    /// Modo overlay: só atualiza o que existe na origem (o `checkout`); sem ele, o que sumiu da
    /// origem some do destino (o `restore`).
    pub overlay: bool,
    pub update_index: bool,
    pub update_worktree: bool,
    /// Estágio de conflito a usar (`--ours` = 2, `--theirs` = 3).
    pub stage: Option<u8>,
    pub ignore_unmerged: bool,
}

/// Aplica as entradas da origem que casam com `ps`. Devolve quantos arquivos foram escritos e se o
/// índice mudou. Caminho sem casamento ou em conflito dá `Fail::Exit(1)` com as mensagens do git.
pub fn restore_paths(repo: &Repo, idx: &mut Index, source: &Source, ps: &Pathspec, o: &PathOpts) -> R<(usize, bool)> {
    let trust = repo.config.get_bool("core.filemode")?.unwrap_or(true);
    let mut seen = vec![false; ps.items.len()];
    let mut unmerged_errors = false;
    let mut src_files: BTreeMap<Vec<u8>, (u32, Oid)> = BTreeMap::new();
    match source {
        Source::Tree(t) => {
            src_files = repo.flatten_tree(t)?;
        }
        Source::Index => {
            let mut last_unmerged: Option<Vec<u8>> = None;
            for e in &idx.entries {
                if e.stage == 0 {
                    if !e.intent_to_add() {
                        src_files.insert(e.path.clone(), (e.mode, e.oid));
                    }
                    continue;
                }
                if !ps.matches(&e.path, false, None).is_some() {
                    continue;
                }
                match o.stage {
                    Some(st) if e.stage == st => {
                        src_files.insert(e.path.clone(), (e.mode, e.oid));
                    }
                    Some(_) => {}
                    None => {
                        if last_unmerged.as_deref() != Some(e.path.as_slice()) {
                            last_unmerged = Some(e.path.clone());
                            if !o.ignore_unmerged {
                                // Marcado abaixo, junto dos casamentos.
                            }
                        }
                    }
                }
            }
        }
    }

    // Caminhos alvo: o que a origem tem e casa, mais (sem overlay) o que o índice tem e casa.
    let mut targets: Vec<Vec<u8>> = Vec::new();
    for path in src_files.keys() {
        if ps.matches(path, false, Some(&mut seen)).is_some() {
            targets.push(path.clone());
        }
    }
    let mut unmerged_paths: Vec<Vec<u8>> = Vec::new();
    if matches!(source, Source::Index) && o.stage.is_none() {
        for e in &idx.entries {
            if e.stage != 0 && ps.matches(&e.path, false, Some(&mut seen)).is_some() && unmerged_paths.last() != Some(&e.path) {
                unmerged_paths.push(e.path.clone());
            }
        }
    }
    if !o.overlay && matches!(source, Source::Tree(_)) {
        for e in &idx.entries {
            if e.stage == 0 && !src_files.contains_key(&e.path) && ps.matches(&e.path, false, Some(&mut seen)).is_some() && !targets.contains(&e.path) {
                targets.push(e.path.clone());
            }
        }
    }
    let unmatched = ps.unmatched(&seen);
    if !unmatched.is_empty() {
        for it in unmatched {
            error(&format!("pathspec '{}' did not match any file(s) known to git", os::lossy(&it.orig)));
        }
        return Err(Fail::Exit(1));
    }
    for p in &unmerged_paths {
        if !o.ignore_unmerged {
            error(&format!("path '{}' is unmerged", os::lossy(p)));
            unmerged_errors = true;
        }
    }
    targets.sort();

    let mut written = 0usize;
    let mut dirty = false;
    for path in &targets {
        match src_files.get(path) {
            Some((mode, oid)) => {
                let cur = idx.get(path).cloned();
                if o.update_worktree {
                    let up_to_date = match &cur {
                        Some(c) if c.mode == *mode && c.oid == *oid => matches!(diff::check_entry(repo, idx, c, trust)?, WtState::Same),
                        _ => false,
                    };
                    if up_to_date {
                        if o.update_index && matches!(source, Source::Tree(_)) {
                            // Nada mudou no disco nem no índice.
                        }
                        continue;
                    }
                    let e = worktree::checkout_entry(repo, path, *mode, oid)?;
                    written += 1;
                    if o.update_index || matches!(source, Source::Index) {
                        idx.add(e);
                        dirty = true;
                    }
                } else if o.update_index {
                    let same = cur.as_ref().is_some_and(|c| c.mode == *mode && c.oid == *oid);
                    if !same {
                        idx.add(IEntry::bare(path.clone(), *oid, *mode));
                        dirty = true;
                    }
                }
            }
            None => {
                if o.update_index {
                    idx.remove(path);
                    dirty = true;
                }
                if o.update_worktree {
                    worktree::unlink_entry(path);
                }
            }
        }
    }
    if unmerged_errors {
        return Err(Fail::Exit(1));
    }
    Ok((written, dirty))
}

pub fn run(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let p = opts::parse(SPECS, args, 0, usage)?;
    let repo = git.repo()?;
    if p.has("patch") {
        return Err(Fail::Fatal("interactive restore needs a terminal; this sandbox has none".into()));
    }
    let mut paths = p.args.clone();
    if let Some(f) = p.value("pathspec-from-file") {
        let data = if f == b"-" { os::stdin_all() } else { super::plumbing::read_user_file(repo, f)? };
        let sep = if p.has("pathspec-file-nul") { 0 } else { b'\n' };
        paths.extend(data.split(|c| *c == sep).filter(|l| !l.is_empty()).map(|l| l.to_vec()));
    }
    if paths.is_empty() {
        return Err(Fail::Fatal("you must specify path(s) to restore".into()));
    }
    let staged = p.has("staged");
    let worktree_flag = p.has("worktree");
    let update_worktree = worktree_flag || !staged;
    let update_index = staged;
    let ours = p.has("ours");
    let theirs = p.has("theirs");
    if ours && theirs {
        return Err(Fail::Fatal("--ours/--theirs cannot be used together".into()));
    }
    if (ours || theirs) && staged {
        return Err(Fail::Fatal("--ours/--theirs is incompatible with --staged".into()));
    }
    let source = match p.value("source") {
        Some(s) => {
            let text = os::lossy(s);
            let Some(id) = repo.rev_parse(s)? else {
                return Err(Fail::Fatal(format!("could not resolve {text}")));
            };
            let Some(tree) = repo.peel_to_tree(&id)? else {
                return Err(Fail::Fatal(format!("could not resolve {text}")));
            };
            Source::Tree(tree)
        }
        None => {
            if staged {
                match repo.head_oid()? {
                    Some(h) => Source::Tree(repo.tree_of(&h)?),
                    None => Source::Tree(crate::hash::EMPTY_TREE),
                }
            } else {
                Source::Index
            }
        }
    };
    let ps = git.pathspec(&paths)?;
    let ipath = repo.index_path();
    let mut idx = Index::load(&ipath)?;
    let o = PathOpts {
        overlay: p.has("overlay"),
        update_index,
        update_worktree,
        stage: if ours {
            Some(2)
        } else if theirs {
            Some(3)
        } else {
            None
        },
        ignore_unmerged: p.has("ignore-unmerged"),
    };
    let (_, dirty) = restore_paths(repo, &mut idx, &source, &ps, &o)?;
    if dirty {
        idx.write(&ipath)?;
    }
    Ok(0)
}
