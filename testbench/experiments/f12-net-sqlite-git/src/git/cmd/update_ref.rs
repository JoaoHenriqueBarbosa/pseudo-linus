//! `git update-ref [-d] <ref> <novo>`.

use super::with_repo;
use crate::shell::Ctx;

pub fn run(args: &[String], ctx: &mut Ctx<'_>) -> i32 {
    with_repo(ctx, |repo, _env, _out, _err, _stdin| {
        let pos: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
        if args.iter().any(|a| a == "-d") {
            let name = pos.first().ok_or_else(|| anyhow::anyhow!("usage: git update-ref -d <refname>"))?;
            repo.fs.entries.remove(&format!(".git/{name}"));
            return Ok(0);
        }
        let (Some(name), Some(value)) = (pos.first(), pos.get(1)) else {
            anyhow::bail!("usage: git update-ref <refname> <new-oid>");
        };
        let id = repo.rev_parse(value)?;
        repo.write_ref(name, &id)?;
        Ok(0)
    })
}
