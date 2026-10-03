//! `git diff` (índice contra worktree) e `git diff --cached` (HEAD contra índice).

use std::collections::{BTreeMap, BTreeSet};

use gix_hash::ObjectId;

use super::status::changes;
use super::with_repo;
use crate::git::store::{Repo, worktree_files};
use crate::git::textdiff::hunks;
use crate::shell::Ctx;

type Side = Option<(u32, ObjectId, Vec<u8>)>;

fn short(id: Option<&ObjectId>) -> String {
    match id {
        Some(id) => id.to_hex().to_string()[..7].to_string(),
        None => "0000000".into(),
    }
}

/// Um arquivo no formato do git.
pub fn file_diff(path: &str, a: &Side, b: &Side) -> String {
    let mut s = format!("diff --git a/{path} b/{path}\n");
    let (am, bm) = (a.as_ref().map(|x| x.0), b.as_ref().map(|x| x.0));
    match (am, bm) {
        (None, Some(m)) => s.push_str(&format!("new file mode {m:o}\n")),
        (Some(m), None) => s.push_str(&format!("deleted file mode {m:o}\n")),
        (Some(x), Some(y)) if x != y => s.push_str(&format!("old mode {x:o}\nnew mode {y:o}\n")),
        _ => {}
    }
    let (ad, bd): (&[u8], &[u8]) = (a.as_ref().map(|x| x.2.as_slice()).unwrap_or(&[]), b.as_ref().map(|x| x.2.as_slice()).unwrap_or(&[]));
    let ids_differ = a.as_ref().map(|x| x.1) != b.as_ref().map(|x| x.1);
    if ids_differ {
        s.push_str(&format!("index {}..{}", short(a.as_ref().map(|x| &x.1)), short(b.as_ref().map(|x| &x.1))));
        if am == bm {
            s.push_str(&format!(" {:o}", am.unwrap_or(0)));
        }
        s.push('\n');
    }
    if ad.is_empty() && bd.is_empty() || !ids_differ {
        return s;
    }
    if ad.contains(&0) || bd.contains(&0) {
        s.push_str(&format!("Binary files {} and {} differ\n", if a.is_some() { format!("a/{path}") } else { "/dev/null".into() }, if b.is_some() { format!("b/{path}") } else { "/dev/null".into() }));
        return s;
    }
    s.push_str(&format!("--- {}\n", if a.is_some() { format!("a/{path}") } else { "/dev/null".into() }));
    s.push_str(&format!("+++ {}\n", if b.is_some() { format!("b/{path}") } else { "/dev/null".into() }));
    s.push_str(&String::from_utf8_lossy(&hunks(ad, bd)));
    s
}

fn side(repo: &Repo<'_>, e: Option<&(u32, ObjectId)>) -> anyhow::Result<Side> {
    Ok(match e {
        Some((m, id)) => Some((*m, *id, repo.read_object(id)?.1)),
        None => None,
    })
}

pub fn run(args: &[String], ctx: &mut Ctx<'_>) -> i32 {
    let cached = args.iter().any(|a| a == "--cached" || a == "--staged");
    let files = worktree_files(ctx.fs);
    with_repo(ctx, |repo, _env, out, _err, _stdin| {
        let ch = changes(repo, &files)?;
        let mut text = String::new();
        if cached {
            let paths: BTreeSet<&String> = ch.staged.iter().map(|(p, _)| p).collect();
            for p in paths {
                text.push_str(&file_diff(p, &side(repo, ch.head.get(p))?, &side(repo, ch.index.get(p))?));
            }
        } else {
            let paths: BTreeMap<&String, char> = ch.unstaged.iter().map(|(p, c)| (p, *c)).collect();
            for p in paths.keys() {
                let a = side(repo, ch.index.get(*p))?;
                let b: Side = ch.work.get(*p).map(|(m, id, d)| (*m, *id, d.clone()));
                text.push_str(&file_diff(p, &a, &b));
            }
        }
        out.extend_from_slice(text.as_bytes());
        Ok(0)
    })
}
