//! `git reset`: move o HEAD (e o ramo) pra outro commit com `--soft`, `--mixed`, `--hard`,
//! `--merge` e `--keep`, ou restaura entradas do índice a partir de uma árvore (`reset <caminhos>`).

use super::Git;
use super::unpack::{self, Opts as UnpackOpts};
use crate::diff::{self, WtState};
use crate::error::{Fail, R};
use crate::hash::{EMPTY_BLOB, EMPTY_TREE, Oid};
use crate::index::{EXT_INTENT_TO_ADD, IEntry, Index};
use crate::opts::{self, Spec};
use crate::os;
use crate::repo::Repo;
use crate::worktree;

const SPECS: &[Spec] = &[
    opts::flag(Some(b'q'), "quiet", "quiet"),
    opts::flag(None, "refresh", "refresh"),
    opts::noneg(opts::flag(None, "mixed", "mixed")),
    opts::noneg(opts::flag(None, "soft", "soft")),
    opts::noneg(opts::flag(None, "hard", "hard")),
    opts::noneg(opts::flag(None, "merge", "merge")),
    opts::noneg(opts::flag(None, "keep", "keep")),
    opts::optional(None, "recurse-submodules", "recurse-submodules"),
    opts::flag(Some(b'p'), "patch", "patch"),
    opts::flag(Some(b'N'), "intent-to-add", "intent-to-add"),
    opts::value(None, "pathspec-from-file", "pathspec-from-file"),
    opts::flag(None, "pathspec-file-nul", "pathspec-file-nul"),
];

#[derive(Copy, Clone, PartialEq, Eq)]
enum Mode {
    Soft,
    Mixed,
    Hard,
    Merge,
    Keep,
}

impl Mode {
    fn name(self) -> &'static str {
        match self {
            Mode::Soft => "soft",
            Mode::Mixed => "mixed",
            Mode::Hard => "hard",
            Mode::Merge => "merge",
            Mode::Keep => "keep",
        }
    }
}

/// Imprime o que ficou diferente do índice (`M`, `D`, `U`) e atualiza o `stat` das entradas limpas.
fn refresh_report(repo: &Repo, idx: &mut Index, quiet: bool) -> R<()> {
    let trust = repo.config.get_bool("core.filemode")?.unwrap_or(true);
    let mut lines: Vec<String> = Vec::new();
    let mut last_unmerged: Option<Vec<u8>> = None;
    for e in &idx.entries {
        if e.stage != 0 {
            if last_unmerged.as_deref() != Some(e.path.as_slice()) {
                last_unmerged = Some(e.path.clone());
                lines.push(format!("U\t{}\n", os::lossy(&e.path)));
            }
            continue;
        }
        if e.skip_worktree() {
            continue;
        }
        if e.intent_to_add() {
            lines.push(format!("A\t{}\n", os::lossy(&e.path)));
            continue;
        }
        match diff::check_entry(repo, idx, e, trust)? {
            WtState::Same => {}
            WtState::Deleted => lines.push(format!("D\t{}\n", os::lossy(&e.path))),
            WtState::Changed(..) => lines.push(format!("M\t{}\n", os::lossy(&e.path))),
        }
    }
    if !quiet && !lines.is_empty() {
        os::outs("Unstaged changes after reset:\n");
        for l in &lines {
            os::outs(l);
        }
    }
    worktree::refresh(idx, trust);
    Ok(())
}

/// Índice com o conteúdo de `tree`, guardando o `stat` das entradas que não mudaram.
fn index_from_tree_keep_stat(repo: &Repo, old: &Index, tree: &Oid) -> R<Index> {
    let mut new = repo.index_from_tree(tree)?;
    new.existed = true;
    for e in new.entries.iter_mut() {
        if let Some(o) = old.get(&e.path)
            && o.oid == e.oid
            && o.mode == e.mode
        {
            *e = o.clone();
        }
    }
    Ok(new)
}

fn die_unmatched_rev(text: &str) -> Fail {
    Fail::Fatal(format!("Failed to resolve '{text}' as a valid tree."))
}

pub fn run(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let p = opts::parse(SPECS, args, 0, usage)?;
    let repo = git.repo()?;
    if p.has("patch") {
        return Err(Fail::Fatal("interactive reset needs a terminal; this sandbox has none".into()));
    }
    let quiet = p.has("quiet");
    let mode = p
        .hits
        .iter()
        .rev()
        .find_map(|h| match h.id {
            "soft" => Some(Mode::Soft),
            "mixed" => Some(Mode::Mixed),
            "hard" => Some(Mode::Hard),
            "merge" => Some(Mode::Merge),
            "keep" => Some(Mode::Keep),
            _ => None,
        })
        .unwrap_or(Mode::Mixed);
    let explicit_mode = p.hits.iter().any(|h| matches!(h.id, "soft" | "mixed" | "hard" | "merge" | "keep"));

    // `<commit>` e/ou caminhos.
    let has_dd = p.dashdash.is_some();
    let (before, after) = p.split_dashdash();
    let mut rev_text: Option<String> = None;
    let mut paths: Vec<Vec<u8>> = Vec::new();
    if has_dd {
        if before.len() > 1 {
            opts::usage_to_stderr(usage);
            return Err(Fail::Exit(129));
        }
        rev_text = before.first().map(|r| os::lossy(r));
        paths = after;
    } else if let Some(first) = p.args.first() {
        if repo.rev_parse(first)?.is_some() {
            rev_text = Some(os::lossy(first));
            paths = p.args[1..].to_vec();
        } else {
            let magic = first.first() == Some(&b':');
            if !magic && !os::exists(&os::join(&repo.prefix, first)) {
                return Err(crate::rev::bad_revision(first));
            }
            paths = p.args.clone();
        }
    }
    if let Some(f) = p.value("pathspec-from-file") {
        let data = if f == b"-" { os::stdin_all() } else { super::plumbing::read_user_file(repo, f)? };
        let sep = if p.has("pathspec-file-nul") { 0 } else { b'\n' };
        paths.extend(data.split(|c| *c == sep).filter(|l| !l.is_empty()).map(|l| l.to_vec()));
    }

    if !paths.is_empty() {
        if explicit_mode && mode != Mode::Mixed {
            return Err(Fail::Fatal(format!("Cannot do {} reset with paths.", mode.name())));
        }
        return reset_paths(git, &p, rev_text, &paths, quiet);
    }
    reset_commit(repo, &p, mode, rev_text, quiet)
}

fn reset_paths(git: &Git, p: &opts::Parsed, rev_text: Option<String>, paths: &[Vec<u8>], quiet: bool) -> R<i32> {
    let repo = git.repo()?;
    let tree = match &rev_text {
        Some(t) => {
            let Some(id) = repo.rev_parse(t.as_bytes())? else { return Err(die_unmatched_rev(t)) };
            match repo.peel_to_tree(&id)? {
                Some(tr) => tr,
                None => return Err(die_unmatched_rev(t)),
            }
        }
        None => match repo.head_oid()? {
            Some(h) => repo.tree_of(&h)?,
            None => EMPTY_TREE,
        },
    };
    let ps = git.pathspec(paths)?;
    let ipath = repo.index_path();
    let mut idx = Index::load(&ipath)?;
    let src = repo.flatten_tree(&tree)?;
    let intent = p.has("intent-to-add");
    for (path, (mode, oid)) in &src {
        if !ps.matches_simple(path) {
            continue;
        }
        let same = idx.get(path).is_some_and(|e| e.mode == *mode && e.oid == *oid);
        if !same {
            idx.add(IEntry::bare(path.clone(), *oid, *mode));
        }
    }
    let mut gone: Vec<Vec<u8>> = Vec::new();
    for e in &idx.entries {
        if ps.matches_simple(&e.path) && !src.contains_key(&e.path) && gone.last() != Some(&e.path) {
            gone.push(e.path.clone());
        }
    }
    for path in gone {
        if intent && let Some(e) = idx.get(&path).cloned() {
            let mut ne = e;
            ne.oid = EMPTY_BLOB;
            ne.ext |= EXT_INTENT_TO_ADD;
            idx.add(ne);
        } else {
            idx.remove(&path);
        }
    }
    if p.flag("refresh") != Some(false) {
        refresh_report(repo, &mut idx, quiet)?;
    }
    idx.write(&ipath)?;
    Ok(0)
}

fn reset_commit(repo: &Repo, p: &opts::Parsed, mode: Mode, rev_text: Option<String>, quiet: bool) -> R<i32> {
    if repo.work_tree.is_none() && mode != Mode::Soft {
        return Err(Fail::Fatal(format!("{} reset is not allowed in a bare repository", mode.name())));
    }
    let head_oid = repo.head_oid()?;
    let ipath = repo.index_path();
    let idx = Index::load(&ipath)?;
    if idx.has_conflicts() && matches!(mode, Mode::Soft | Mode::Keep) {
        return Err(Fail::Fatal(format!("Cannot do a {} reset in the middle of a merge.", mode.name())));
    }
    let target: Option<Oid> = match &rev_text {
        Some(t) => {
            let Some(c) = repo.rev_parse_commit(t.as_bytes())? else {
                return Err(Fail::Fatal(format!("Could not parse object '{t}'.")));
            };
            Some(c)
        }
        None => head_oid,
    };
    let Some(target) = target else {
        // Ramo ainda sem commit: o destino é a árvore vazia.
        match mode {
            Mode::Soft => {}
            Mode::Hard | Mode::Merge | Mode::Keep => {
                let uo = UnpackOpts { verb: "reset", advice: "reset", force: true };
                let ni = unpack::switch_tree(repo, &idx, None, &EMPTY_TREE, &uo)?;
                ni.write(&ipath)?;
            }
            Mode::Mixed => {
                let mut ni = Index { version: 2, existed: true, ..Index::default() };
                if p.flag("refresh") != Some(false) {
                    refresh_report(repo, &mut ni, quiet)?;
                }
                ni.write(&ipath)?;
            }
        }
        return Ok(0);
    };
    let new_tree = repo.tree_of(&target)?;
    let old_tree = match head_oid {
        Some(h) => Some(repo.tree_of(&h)?),
        None => None,
    };
    match mode {
        Mode::Soft => {}
        Mode::Mixed => {
            let mut ni = index_from_tree_keep_stat(repo, &idx, &new_tree)?;
            if p.flag("refresh") != Some(false) {
                refresh_report(repo, &mut ni, quiet)?;
            }
            ni.write(&ipath)?;
        }
        Mode::Hard => {
            let uo = UnpackOpts { verb: "reset", advice: "reset", force: true };
            let ni = unpack::switch_tree(repo, &idx, old_tree.as_ref(), &new_tree, &uo)?;
            ni.write(&ipath)?;
        }
        Mode::Merge | Mode::Keep => {
            let uo = UnpackOpts { verb: "reset", advice: "reset", force: false };
            let ni = unpack::switch_tree(repo, &idx, old_tree.as_ref(), &new_tree, &uo)?;
            ni.write(&ipath)?;
        }
    }
    match head_oid {
        Some(h) => repo.write_pseudoref("ORIG_HEAD", format!("{h}\n").as_bytes())?,
        None => repo.remove_pseudoref("ORIG_HEAD"),
    }
    let shown = rev_text.clone().unwrap_or_else(|| "HEAD".to_string());
    repo.update_ref("HEAD", target, None, &format!("reset: moving to {shown}"), false)?;
    if mode == Mode::Hard && !quiet {
        let c = repo.read_commit(&target)?;
        os::outs(&format!("HEAD is now at {} {}\n", repo.abbrev_default(&target), os::lossy(&c.subject())));
    }
    for f in ["MERGE_HEAD", "MERGE_MSG", "MERGE_MODE", "SQUASH_MSG", "CHERRY_PICK_HEAD", "REVERT_HEAD"] {
        let _ = os::unlink(&repo.path(f));
    }
    Ok(0)
}
