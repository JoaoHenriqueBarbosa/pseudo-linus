//! `git ls-files [-s | --stage]`.

use super::{hex, mode_str, with_repo};
use crate::shell::Ctx;

pub fn run(args: &[String], ctx: &mut Ctx<'_>) -> i32 {
    let stage = args.iter().any(|a| a == "-s" || a == "--stage");
    with_repo(ctx, |repo, _env, out, _err, _stdin| {
        for (path, (mode, id)) in repo.index_map()? {
            let line = if stage { format!("{} {} 0\t{path}\n", mode_str(mode), hex(&id)) } else { format!("{path}\n") };
            out.extend_from_slice(line.as_bytes());
        }
        Ok(0)
    })
}
