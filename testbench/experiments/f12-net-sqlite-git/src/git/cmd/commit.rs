//! `git commit [-q] [-a] -m <mensagem>`: write-tree + commit-tree + atualização do ramo, com o
//! resumo (`[master abc1234] msg`, estatística e `create mode`).

use std::collections::BTreeSet;

use gix_object::Kind;

use super::status::{changes, long_status};
use super::write_tree::tree_from_index;
use super::{short_name, signature, with_repo};
use crate::git::store::{Head, IndexMap, Repo, worktree_files, worktree_sizes};
use crate::git::textdiff::line_stats;
use crate::shell::Ctx;

/// Resumo de estatística entre duas árvores achatadas, como o `git commit` imprime.
pub fn summary(repo: &Repo<'_>, old: &IndexMap, new: &IndexMap) -> anyhow::Result<String> {
    let paths: BTreeSet<&String> = old.keys().chain(new.keys()).collect();
    let (mut files, mut ins, mut del) = (0u32, 0u32, 0u32);
    let mut modes = String::new();
    for p in paths {
        let (a, b) = (old.get(p), new.get(p));
        if a == b {
            continue;
        }
        files += 1;
        let data = |e: Option<&(u32, gix_hash::ObjectId)>| -> anyhow::Result<Vec<u8>> {
            Ok(match e {
                Some((_, id)) => repo.read_object(id)?.1,
                None => Vec::new(),
            })
        };
        let (i, d) = line_stats(&data(a)?, &data(b)?);
        ins += i;
        del += d;
        match (a, b) {
            (None, Some((m, _))) => modes.push_str(&format!(" create mode {m:06o} {p}\n")),
            (Some((m, _)), None) => modes.push_str(&format!(" delete mode {m:06o} {p}\n")),
            (Some((ma, _)), Some((mb, _))) if ma != mb => modes.push_str(&format!(" mode change {ma:06o} => {mb:06o} {p}\n")),
            _ => {}
        }
    }
    let mut s = if files == 1 { " 1 file changed".to_string() } else { format!(" {files} files changed") };
    if ins > 0 || del == 0 {
        s.push_str(&if ins == 1 { ", 1 insertion(+)".to_string() } else { format!(", {ins} insertions(+)") });
    }
    if del > 0 || ins == 0 {
        s.push_str(&if del == 1 { ", 1 deletion(-)".to_string() } else { format!(", {del} deletions(-)") });
    }
    Ok(format!("{s}\n{modes}"))
}

pub fn run(args: &[String], ctx: &mut Ctx<'_>) -> i32 {
    let mut quiet = false;
    let mut all = false;
    let mut messages: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if let Some(m) = a.strip_prefix("--message=") {
            messages.push(m.to_string());
        } else if a == "--quiet" {
            quiet = true;
        } else if a == "--all" {
            all = true;
        } else if a.starts_with('-') && !a.starts_with("--") {
            let flags: Vec<char> = a[1..].chars().collect();
            for (k, f) in flags.iter().enumerate() {
                match f {
                    'q' => quiet = true,
                    'a' => all = true,
                    'm' => {
                        let rest: String = flags[k + 1..].iter().collect();
                        if rest.is_empty() {
                            i += 1;
                            messages.push(args.get(i).cloned().unwrap_or_default());
                        } else {
                            messages.push(rest);
                        }
                        break;
                    }
                    _ => {}
                }
            }
        }
        i += 1;
    }
    let files = worktree_files(ctx.fs);
    let sizes = worktree_sizes(ctx.fs);
    with_repo(ctx, |repo, env, out, _err, _stdin| {
        if all {
            let mut map = repo.index_map()?;
            let mut changed = Vec::new();
            for (p, (m, _)) in &map {
                match files.iter().find(|(fp, _, _)| fp == p) {
                    Some((_, wm, data)) => changed.push((p.clone(), Some((*wm, data.clone())), *m)),
                    None => changed.push((p.clone(), None, *m)),
                }
            }
            for (p, w, _) in changed {
                match w {
                    Some((wm, data)) => {
                        let id = repo.write_object(Kind::Blob, &data)?;
                        map.insert(p, (wm, id));
                    }
                    None => {
                        map.remove(&p);
                    }
                }
            }
            repo.set_index_map(&map, &sizes)?;
        }
        let index = repo.index_map()?;
        let tree = tree_from_index(repo, &index)?;
        let parent = repo.head_commit()?;
        let parent_tree = match parent {
            Some(p) => Some(repo.commit(&p)?.tree),
            None => None,
        };
        if parent_tree == Some(tree) || (parent.is_none() && index.is_empty()) {
            let ch = changes(repo, &files)?;
            out.extend_from_slice(long_status(repo, &ch)?.as_bytes());
            return Ok(1);
        }
        let message = messages.join("\n\n") + "\n";
        let commit = gix_object::Commit {
            tree,
            parents: parent.into_iter().collect::<Vec<_>>().into(),
            author: signature(env, "AUTHOR"),
            committer: signature(env, "COMMITTER"),
            encoding: None,
            message: message.clone().into(),
            extra_headers: Vec::new(),
        };
        let old = repo.head_tree_map()?;
        let id = repo.write_encoded(&commit)?;
        repo.write_ref("HEAD", &id)?;
        if !quiet {
            let branch = match repo.head()? {
                Head::Branch(b, _) => short_name(&b).to_string(),
                Head::Detached(_) => "detached HEAD".to_string(),
            };
            let root = if parent.is_none() { " (root-commit)" } else { "" };
            let subject = message.lines().next().unwrap_or("");
            out.extend_from_slice(format!("[{branch}{root} {}] {subject}\n", repo.abbrev(&id)).as_bytes());
            out.extend_from_slice(summary(repo, &old, &index)?.as_bytes());
        }
        Ok(0)
    })
}
