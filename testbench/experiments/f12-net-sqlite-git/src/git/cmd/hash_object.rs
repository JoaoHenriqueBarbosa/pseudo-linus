//! `git hash-object [-w] [-t <tipo>] [--stdin] [<arquivo>...]`.

use gix_object::Kind;

use super::{fatal, hex, out};
use crate::git::store::{Repo, hash_object};
use crate::shell::{Ctx, rel};

pub fn run(args: &[String], ctx: &mut Ctx<'_>) -> i32 {
    let mut write = false;
    let mut kind = Kind::Blob;
    let mut stdin = false;
    let mut files = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-w" => write = true,
            "--stdin" => stdin = true,
            "-t" => {
                i += 1;
                kind = match args.get(i).map(String::as_str) {
                    Some("blob") => Kind::Blob,
                    Some("tree") => Kind::Tree,
                    Some("commit") => Kind::Commit,
                    Some("tag") => Kind::Tag,
                    _ => return fatal(ctx, "invalid object type"),
                };
            }
            f => files.push(f.to_string()),
        }
        i += 1;
    }
    let mut inputs: Vec<Vec<u8>> = Vec::new();
    if stdin {
        inputs.push(ctx.stdin.to_vec());
    }
    for f in &files {
        match ctx.fs.read(&rel(f)) {
            Some(d) => inputs.push(d.to_vec()),
            None => return fatal(ctx, &format!("could not open '{f}' for reading: No such file or directory")),
        }
    }
    for data in inputs {
        let id = if write {
            let Some(mut repo) = Repo::open(ctx.fs) else {
                return fatal(ctx, "not a git repository (or any of the parent directories): .git");
            };
            match repo.write_object(kind, &data) {
                Ok(id) => id,
                Err(e) => return fatal(ctx, &e.to_string()),
            }
        } else {
            match hash_object(kind, &data) {
                Ok(id) => id,
                Err(e) => return fatal(ctx, &e.to_string()),
            }
        };
        out(ctx, &format!("{}\n", hex(&id)));
    }
    0
}
