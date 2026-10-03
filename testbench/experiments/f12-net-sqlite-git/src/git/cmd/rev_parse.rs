//! `git rev-parse [--short] [--verify] [--abbrev-ref] [--show-toplevel] [--git-dir] <rev>...`.

use super::{hex, short_name, with_repo};
use crate::git::store::Head;
use crate::shell::Ctx;

pub fn run(args: &[String], ctx: &mut Ctx<'_>) -> i32 {
    with_repo(ctx, |repo, _env, out, err, _stdin| {
        let short = args.iter().any(|a| a == "--short");
        let verify = short || args.iter().any(|a| a == "--verify");
        let abbrev_ref = args.iter().any(|a| a == "--abbrev-ref");
        let revs: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
        for a in args {
            match a.as_str() {
                "--show-toplevel" => out.extend_from_slice(format!("{}\n", harness::CASE_DIR).as_bytes()),
                "--git-dir" => out.extend_from_slice(b".git\n"),
                "--is-inside-work-tree" => out.extend_from_slice(b"true\n"),
                _ => {}
            }
        }
        if verify && revs.len() != 1 {
            err.extend_from_slice(b"fatal: Needed a single revision\n");
            return Ok(128);
        }
        for r in revs {
            if abbrev_ref
                && r == "HEAD"
                && let Head::Branch(b, _) = repo.head()?
            {
                out.extend_from_slice(format!("{}\n", short_name(&b)).as_bytes());
                continue;
            }
            let id = repo.rev_parse(r).map_err(|_| anyhow::anyhow!("ambiguous argument '{r}': unknown revision or path not in the working tree."))?;
            let text = if short { repo.abbrev(&id) } else { hex(&id) };
            out.extend_from_slice(format!("{text}\n").as_bytes());
        }
        Ok(0)
    })
}
