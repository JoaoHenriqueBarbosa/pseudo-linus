//! `git rm`: tira caminhos do índice e da árvore de trabalho, com as recusas do git quando há
//! modificações locais ou conteúdo só no índice (a menos que `-f`).

use std::collections::BTreeMap;

use super::Git;
use crate::diff::{self, WtState};
use crate::error::{Fail, R, error};
use crate::hash::Oid;
use crate::index::Index;
use crate::opts::{self, Spec};
use crate::os;
use crate::pathspec::{self, Hit};
use crate::repo::Repo;
use crate::worktree;

const SPECS: &[Spec] = &[
    opts::flag(Some(b'n'), "dry-run", "dry-run"),
    opts::flag(Some(b'q'), "quiet", "quiet"),
    opts::flag(None, "cached", "cached"),
    opts::flag(Some(b'f'), "force", "force"),
    opts::short_flag(b'r', "recursive"),
    opts::flag(None, "ignore-unmatch", "ignore-unmatch"),
    opts::flag(None, "sparse", "sparse"),
    opts::value(None, "pathspec-from-file", "pathspec-from-file"),
    opts::flag(None, "pathspec-file-nul", "pathspec-file-nul"),
];

/// Nível de casamento na ordem do git: recursivo < glob < exato.
fn rank(h: Hit) -> u8 {
    match h {
        Hit::Recursive => 1,
        Hit::Fnmatch => 2,
        Hit::Exact => 3,
    }
}

/// Um bloco de erro (`error: ...` com a lista de arquivos recuados e a dica). `true` se havia algo.
fn report(files: &[Vec<u8>], one: &str, many: &str, advice: &str) -> bool {
    if files.is_empty() {
        return false;
    }
    let mut msg = String::from(if files.len() == 1 { one } else { many });
    for f in files {
        msg.push_str("\n    ");
        msg.push_str(&os::lossy(f));
    }
    msg.push_str(advice);
    error(&msg);
    true
}

/// Compara o conteúdo do índice com o HEAD e com o arquivo (o `check_local_mod` do git).
fn check_local_mod(repo: &Repo, idx: &Index, paths: &[Vec<u8>], head: &BTreeMap<Vec<u8>, (u32, Oid)>, have_head: bool, cached: bool) -> R<bool> {
    let trust = repo.config.get_bool("core.filemode")?.unwrap_or(true);
    let mut staged: Vec<Vec<u8>> = Vec::new();
    let mut in_index: Vec<Vec<u8>> = Vec::new();
    let mut local: Vec<Vec<u8>> = Vec::new();
    for path in paths {
        let Some(e) = idx.get(path) else { continue };
        // Já sumiu da árvore de trabalho (ou virou diretório): nada a perder.
        let local_changes = match diff::check_entry(repo, idx, e, trust)? {
            WtState::Deleted => continue,
            WtState::Same => false,
            WtState::Changed(..) => true,
        };
        let staged_changes = if !have_head {
            true
        } else {
            match head.get(path) {
                Some((mode, oid)) => *oid != e.oid || *mode != e.mode,
                None => true,
            }
        };
        if local_changes && staged_changes {
            if !cached || !e.intent_to_add() {
                staged.push(path.clone());
            }
        } else if !cached {
            if staged_changes {
                in_index.push(path.clone());
            }
            if local_changes {
                local.push(path.clone());
            }
        }
    }
    let mut errs = false;
    errs |= report(
        &staged,
        "the following file has staged content different from both the\nfile and the HEAD:",
        "the following files have staged content different from both the\nfiles and the HEAD:",
        "\n(use -f to force removal)",
    );
    errs |= report(
        &in_index,
        "the following file has changes staged in the index:",
        "the following files have changes staged in the index:",
        "\n(use --cached to keep the file, or -f to force removal)",
    );
    errs |= report(
        &local,
        "the following file has local modifications:",
        "the following files have local modifications:",
        "\n(use --cached to keep the file, or -f to force removal)",
    );
    Ok(errs)
}

pub fn run(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let p = opts::parse(SPECS, args, 0, usage)?;
    let repo = git.repo()?;
    let mut paths = p.args.clone();
    if let Some(f) = p.value("pathspec-from-file") {
        let data = if f == b"-" { os::stdin_all() } else { super::plumbing::read_user_file(repo, f)? };
        let sep = if p.has("pathspec-file-nul") { 0 } else { b'\n' };
        paths.extend(data.split(|c| *c == sep).filter(|l| !l.is_empty()).map(|l| l.to_vec()));
    }
    if paths.is_empty() {
        return Err(Fail::Fatal("No pathspec was given. Which files should I remove?".into()));
    }
    let ps = git.pathspec(&paths)?;
    let ipath = repo.index_path();
    let mut idx = Index::load(&ipath)?;
    let quiet = p.has("quiet");
    let dry = p.has("dry-run");
    let cached = p.has("cached");
    let force = p.has("force");
    let recursive = p.has("recursive");
    let ignore_unmatch = p.has("ignore-unmatch");

    // Entradas que casam, e o melhor nível de casamento de cada item da pathspec.
    let mut best: Vec<u8> = vec![0; ps.items.len()];
    let mut list: Vec<Vec<u8>> = Vec::new();
    for e in &idx.entries {
        if ps.matches(&e.path, false, None).is_none() {
            continue;
        }
        for (i, it) in ps.items.iter().enumerate() {
            if it.magic & pathspec::EXCLUDE != 0 {
                continue;
            }
            if let Some(h) = pathspec::match_item(it, &e.path, false) {
                best[i] = best[i].max(rank(h));
            }
        }
        if list.last() != Some(&e.path) {
            list.push(e.path.clone());
        }
    }
    let mut seen_any = false;
    for (i, it) in ps.items.iter().enumerate() {
        if it.magic & pathspec::EXCLUDE != 0 {
            continue;
        }
        if best[i] != 0 {
            seen_any = true;
        } else if ignore_unmatch {
            continue;
        } else {
            return Err(Fail::Fatal(format!("pathspec '{}' did not match any files", os::lossy(&it.orig))));
        }
        if !recursive && best[i] == 1 {
            let shown = if it.orig.is_empty() { b".".to_vec() } else { it.orig.clone() };
            return Err(Fail::Fatal(format!("not removing '{}' recursively without -r", os::lossy(&shown))));
        }
    }
    if !seen_any {
        return Ok(0);
    }

    if !force {
        let head_tree = super::diff_cmd::head_tree(repo)?;
        let have_head = head_tree.is_some();
        let head_files = match &head_tree {
            Some(t) => repo.flatten_tree(t)?,
            None => BTreeMap::new(),
        };
        if check_local_mod(repo, &idx, &list, &head_files, have_head, cached)? {
            return Ok(1);
        }
    }

    for path in &list {
        if !quiet {
            os::outs(&format!("rm '{}'\n", os::lossy(path)));
        }
        if dry {
            continue;
        }
        if !cached {
            worktree::unlink_entry(path);
        }
        idx.remove(path);
    }
    if !dry {
        idx.write(&ipath)?;
    }
    Ok(0)
}
