//! `git status [--porcelain | --short]` e o formato longo.

use std::collections::{BTreeMap, BTreeSet};

use gix_hash::ObjectId;
use gix_object::Kind;

use super::{short_name, with_repo};
use crate::git::store::{Head, IndexMap, Repo, hash_object, worktree_files};
use crate::shell::Ctx;

pub struct Changes {
    pub head: IndexMap,
    pub index: IndexMap,
    /// caminho -> (modo, id do blob, conteúdo)
    pub work: BTreeMap<String, (u32, ObjectId, Vec<u8>)>,
    /// (caminho, 'A' | 'M' | 'D'): HEAD contra índice
    pub staged: Vec<(String, char)>,
    /// (caminho, 'M' | 'D'): índice contra worktree
    pub unstaged: Vec<(String, char)>,
    /// Não rastreados, com diretórios inteiros colapsados em `dir/`.
    pub untracked: Vec<String>,
    pub has_commits: bool,
}

pub fn changes(repo: &Repo<'_>, files: &[(String, u32, Vec<u8>)]) -> anyhow::Result<Changes> {
    let head = repo.head_tree_map()?;
    let index = repo.index_map()?;
    let mut work = BTreeMap::new();
    for (p, m, d) in files {
        work.insert(p.clone(), (*m, hash_object(Kind::Blob, d)?, d.clone()));
    }
    let mut staged = Vec::new();
    let all: BTreeSet<&String> = head.keys().chain(index.keys()).collect();
    for p in all {
        match (head.get(p), index.get(p)) {
            (None, Some(_)) => staged.push((p.clone(), 'A')),
            (Some(_), None) => staged.push((p.clone(), 'D')),
            (Some(a), Some(b)) if a != b => staged.push((p.clone(), 'M')),
            _ => {}
        }
    }
    let mut unstaged = Vec::new();
    for (p, (m, id)) in &index {
        match work.get(p) {
            None => unstaged.push((p.clone(), 'D')),
            Some((wm, wid, _)) if wm != m || wid != id => unstaged.push((p.clone(), 'M')),
            _ => {}
        }
    }
    let mut untracked: Vec<String> = Vec::new();
    for p in work.keys().filter(|p| !index.contains_key(*p)) {
        let parts: Vec<&str> = p.split('/').collect();
        let mut shown = p.clone();
        for depth in 1..parts.len() {
            let dir = parts[..depth].join("/");
            let prefix = format!("{dir}/");
            if !index.keys().any(|k| k.starts_with(&prefix)) {
                shown = prefix;
                break;
            }
        }
        if !untracked.contains(&shown) {
            untracked.push(shown);
        }
    }
    untracked.sort();
    Ok(Changes { head, index, work, staged, unstaged, untracked, has_commits: repo.head_commit()?.is_some() })
}

fn label(c: char) -> &'static str {
    match c {
        'A' => "new file:   ",
        'D' => "deleted:    ",
        _ => "modified:   ",
    }
}

/// Formato longo do `git status`.
pub fn long_status(repo: &Repo<'_>, ch: &Changes) -> anyhow::Result<String> {
    let mut s = String::new();
    match repo.head()? {
        Head::Branch(b, _) => s.push_str(&format!("On branch {}\n", short_name(&b))),
        Head::Detached(id) => s.push_str(&format!("HEAD detached at {}\n", repo.abbrev(&id))),
    }
    if !ch.has_commits {
        s.push_str("\nNo commits yet\n\n");
    }
    if !ch.staged.is_empty() {
        s.push_str("Changes to be committed:\n");
        if ch.has_commits {
            s.push_str("  (use \"git restore --staged <file>...\" to unstage)\n");
        } else {
            s.push_str("  (use \"git rm --cached <file>...\" to unstage)\n");
        }
        for (p, c) in &ch.staged {
            s.push_str(&format!("\t{}{p}\n", label(*c)));
        }
        s.push('\n');
    }
    if !ch.unstaged.is_empty() {
        s.push_str("Changes not staged for commit:\n");
        if ch.unstaged.iter().any(|(_, c)| *c == 'D') {
            s.push_str("  (use \"git add/rm <file>...\" to update what will be committed)\n");
        } else {
            s.push_str("  (use \"git add <file>...\" to update what will be committed)\n");
        }
        s.push_str("  (use \"git restore <file>...\" to discard changes in working directory)\n");
        for (p, c) in &ch.unstaged {
            s.push_str(&format!("\t{}{p}\n", label(*c)));
        }
        s.push('\n');
    }
    if !ch.untracked.is_empty() {
        s.push_str("Untracked files:\n  (use \"git add <file>...\" to include in what will be committed)\n");
        for p in &ch.untracked {
            s.push_str(&format!("\t{p}\n"));
        }
        s.push('\n');
    }
    if ch.staged.is_empty() {
        if !ch.unstaged.is_empty() {
            s.push_str("no changes added to commit (use \"git add\" and/or \"git commit -a\")\n");
        } else if !ch.untracked.is_empty() {
            s.push_str("nothing added to commit but untracked files present (use \"git add\" to track)\n");
        } else if !ch.has_commits {
            s.push_str("nothing to commit (create/copy files and use \"git add\" to track)\n");
        } else {
            s.push_str("nothing to commit, working tree clean\n");
        }
    }
    Ok(s)
}

/// Formato `--porcelain` (v1) e `--short`, que coincidem sem cores.
pub fn short_status(ch: &Changes) -> String {
    let mut rows: BTreeMap<&String, (char, char)> = BTreeMap::new();
    for (p, c) in &ch.staged {
        rows.entry(p).or_insert((' ', ' ')).0 = *c;
    }
    for (p, c) in &ch.unstaged {
        rows.entry(p).or_insert((' ', ' ')).1 = *c;
    }
    let mut s = String::new();
    for (p, (x, y)) in rows {
        s.push_str(&format!("{x}{y} {p}\n"));
    }
    for p in &ch.untracked {
        s.push_str(&format!("?? {p}\n"));
    }
    s
}

pub fn run(args: &[String], ctx: &mut Ctx<'_>) -> i32 {
    let short = args.iter().any(|a| a == "--porcelain" || a == "--short" || a == "-s" || a.starts_with("--porcelain="));
    let files = worktree_files(ctx.fs);
    with_repo(ctx, |repo, _env, out, _err, _stdin| {
        let ch = changes(repo, &files)?;
        let text = if short { short_status(&ch) } else { long_status(repo, &ch)? };
        out.extend_from_slice(text.as_bytes());
        Ok(0)
    })
}
