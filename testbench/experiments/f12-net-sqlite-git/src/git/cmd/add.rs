//! `git add [-A] <pathspec>...`: grava os blobs e atualiza o índice, inclusive remoções dentro do
//! pathspec (comportamento do git 2.x).

use gix_object::Kind;

use super::with_repo;
use crate::git::store::{worktree_files, worktree_sizes};
use crate::shell::{Ctx, rel};

/// `path` casa com o pathspec `spec` (caminho exato, diretório, ou `.` pra tudo).
pub fn matches(spec: &str, path: &str) -> bool {
    spec.is_empty() || spec == "." || path == spec || path.starts_with(&format!("{}/", spec.trim_end_matches('/')))
}

pub fn run(args: &[String], ctx: &mut Ctx<'_>) -> i32 {
    let mut specs: Vec<String> = Vec::new();
    for a in args {
        match a.as_str() {
            "-A" | "--all" => specs.push(String::new()),
            "--" => {}
            s => specs.push(rel(s)),
        }
    }
    if specs.is_empty() {
        ctx.stderr.extend_from_slice(b"Nothing specified, nothing added.\n");
        return 0;
    }
    let files = worktree_files(ctx.fs);
    let sizes = worktree_sizes(ctx.fs);
    with_repo(ctx, |repo, _env, _out, _err, _stdin| {
        let mut map = repo.index_map()?;
        for spec in &specs {
            let hit_worktree = files.iter().any(|(p, _, _)| matches(spec, p));
            let hit_index = map.keys().any(|p| matches(spec, p));
            if !hit_worktree && !hit_index {
                anyhow::bail!("pathspec '{spec}' did not match any files");
            }
        }
        for (path, mode, data) in &files {
            if specs.iter().any(|s| matches(s, path)) {
                let id = repo.write_object(Kind::Blob, data)?;
                map.insert(path.clone(), (*mode, id));
            }
        }
        let present: std::collections::BTreeSet<&String> = files.iter().map(|(p, _, _)| p).collect();
        map.retain(|p, _| present.contains(p) || !specs.iter().any(|s| matches(s, p)));
        repo.set_index_map(&map, &sizes)?;
        Ok(0)
    })
}
