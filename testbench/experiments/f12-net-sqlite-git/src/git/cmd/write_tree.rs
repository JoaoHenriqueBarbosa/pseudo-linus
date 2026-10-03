//! `git write-tree`: trees a partir do índice, recursivamente.

use std::collections::BTreeMap;

use gix_hash::ObjectId;
use gix_object::tree::{Entry, EntryKind};

use super::{hex, out, with_repo};
use crate::git::store::{IndexMap, Repo};
use crate::shell::Ctx;

fn kind(mode: u32) -> EntryKind {
    match mode {
        0o100755 => EntryKind::BlobExecutable,
        0o120000 => EntryKind::Link,
        0o160000 => EntryKind::Commit,
        0o040000 => EntryKind::Tree,
        _ => EntryKind::Blob,
    }
}

fn build(repo: &mut Repo<'_>, files: &[(&str, u32, ObjectId)]) -> anyhow::Result<ObjectId> {
    let mut tree = gix_object::Tree { entries: Vec::new() };
    let mut dirs: BTreeMap<&str, Vec<(&str, u32, ObjectId)>> = BTreeMap::new();
    for (path, mode, id) in files {
        match path.split_once('/') {
            Some((dir, rest)) => dirs.entry(dir).or_default().push((rest, *mode, *id)),
            None => tree.entries.push(Entry { mode: kind(*mode).into(), filename: (*path).into(), oid: *id }),
        }
    }
    for (dir, sub) in dirs {
        let id = build(repo, &sub)?;
        tree.entries.push(Entry { mode: EntryKind::Tree.into(), filename: dir.into(), oid: id });
    }
    tree.entries.sort();
    repo.write_encoded(&tree)
}

/// Grava as trees do índice e devolve a raiz.
pub fn tree_from_index(repo: &mut Repo<'_>, map: &IndexMap) -> anyhow::Result<ObjectId> {
    let files: Vec<(&str, u32, ObjectId)> = map.iter().map(|(p, (m, id))| (p.as_str(), *m, *id)).collect();
    build(repo, &files)
}

pub fn run(_args: &[String], ctx: &mut Ctx<'_>) -> i32 {
    let mut id = None;
    let code = with_repo(ctx, |repo, _, _, _, _| {
        let map = repo.index_map()?;
        id = Some(tree_from_index(repo, &map)?);
        Ok(0)
    });
    if let Some(id) = id {
        out(ctx, &format!("{}\n", hex(&id)));
    }
    code
}
