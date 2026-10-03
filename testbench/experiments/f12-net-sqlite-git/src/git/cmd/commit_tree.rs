//! `git commit-tree <tree> [-p <pai>]... -m <mensagem>`.

use super::{hex, signature, with_repo};
use crate::shell::Ctx;

pub fn run(args: &[String], ctx: &mut Ctx<'_>) -> i32 {
    with_repo(ctx, |repo, env, out, _err, stdin| {
        let mut tree = None;
        let mut parents = Vec::new();
        let mut messages: Vec<String> = Vec::new();
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "-p" => {
                    i += 1;
                    parents.push(repo.rev_parse(args.get(i).map(String::as_str).unwrap_or(""))?);
                }
                "-m" => {
                    i += 1;
                    messages.push(args.get(i).cloned().unwrap_or_default());
                }
                t => tree = Some(repo.rev_parse(t)?),
            }
            i += 1;
        }
        let tree = tree.ok_or_else(|| anyhow::anyhow!("must specify a tree object"))?;
        let message = if messages.is_empty() { String::from_utf8_lossy(stdin).into_owned() } else { messages.join("\n\n") + "\n" };
        let commit = gix_object::Commit {
            tree,
            parents: parents.into(),
            author: signature(env, "AUTHOR"),
            committer: signature(env, "COMMITTER"),
            encoding: None,
            message: message.into(),
            extra_headers: Vec::new(),
        };
        let id = repo.write_encoded(&commit)?;
        out.extend_from_slice(format!("{}\n", hex(&id)).as_bytes());
        Ok(0)
    })
}
