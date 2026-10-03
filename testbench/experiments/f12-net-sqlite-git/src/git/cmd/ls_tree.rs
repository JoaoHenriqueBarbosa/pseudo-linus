//! `git ls-tree [-r] [--name-only] <tree-ish>`.

use super::{hex, kind_of_mode, mode_str, with_repo};
use crate::shell::Ctx;

pub fn run(args: &[String], ctx: &mut Ctx<'_>) -> i32 {
    let recursive = args.iter().any(|a| a == "-r");
    let names = args.iter().any(|a| a == "--name-only");
    let spec = args.iter().find(|a| !a.starts_with('-')).cloned().unwrap_or_else(|| "HEAD".into());
    with_repo(ctx, |repo, _env, out, _err, _stdin| {
        let id = repo.rev_parse(&spec)?;
        let tree_id = match repo.read_object(&id)?.0 {
            gix_object::Kind::Commit => repo.commit(&id)?.tree,
            _ => id,
        };
        let rows: Vec<(String, u32, gix_hash::ObjectId)> = if recursive {
            let mut files = Vec::new();
            repo.flatten_tree(&tree_id, "", &mut files)?;
            files
        } else {
            repo.tree(&tree_id)?.entries.into_iter().map(|e| (e.filename.to_string(), e.mode.value() as u32, e.oid)).collect()
        };
        for (path, mode, oid) in rows {
            let line = if names { format!("{path}\n") } else { format!("{} {} {}\t{path}\n", mode_str(mode), kind_of_mode(mode), hex(&oid)) };
            out.extend_from_slice(line.as_bytes());
        }
        Ok(0)
    })
}
