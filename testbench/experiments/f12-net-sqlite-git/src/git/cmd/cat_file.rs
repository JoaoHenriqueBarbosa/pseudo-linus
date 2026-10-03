//! `git cat-file (-t | -s | -p | -e) <objeto>`.

use gix_object::Kind;

use super::{hex, kind_of_mode, mode_str, with_repo};
use crate::git::store::Repo;
use crate::shell::Ctx;

/// Saída do `cat-file -p` (tree formatada, demais tipos crus).
pub fn pretty(repo: &Repo<'_>, kind: Kind, data: &[u8]) -> anyhow::Result<Vec<u8>> {
    if kind != Kind::Tree {
        return Ok(data.to_vec());
    }
    let tree: gix_object::Tree = crate::git::store::gx(gix_object::TreeRef::from_bytes(data, crate::git::store::HASH))?.into();
    let _ = repo;
    let mut s = String::new();
    for e in tree.entries {
        let m = e.mode.value() as u32;
        s.push_str(&format!("{} {} {}\t{}\n", mode_str(m), kind_of_mode(m), hex(&e.oid), e.filename));
    }
    Ok(s.into_bytes())
}

pub fn run(args: &[String], ctx: &mut Ctx<'_>) -> i32 {
    with_repo(ctx, |repo, _env, out, _err, _stdin| {
        let flag = args.first().map(String::as_str).unwrap_or("");
        let spec = args.get(1).map(String::as_str).unwrap_or("");
        let id = match repo.rev_parse(spec) {
            Ok(id) => id,
            Err(_) if flag == "-e" => return Ok(1),
            Err(_) => anyhow::bail!("Not a valid object name {spec}"),
        };
        let (kind, data) = repo.read_object(&id)?;
        match flag {
            "-t" => out.extend_from_slice(format!("{kind}\n").as_bytes()),
            "-s" => out.extend_from_slice(format!("{}\n", data.len()).as_bytes()),
            "-e" => {}
            "-p" => out.extend_from_slice(&pretty(repo, kind, &data)?),
            other => {
                // `git cat-file <tipo> <objeto>`
                if other != kind.to_string() {
                    anyhow::bail!("git cat-file {other}: bad file");
                }
                out.extend_from_slice(&data);
            }
        }
        Ok(0)
    })
}
