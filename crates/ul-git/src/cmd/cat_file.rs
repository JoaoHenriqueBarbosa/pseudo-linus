//! `git cat-file`: tipo, tamanho, existência, conteúdo e os modos `--batch`.

use super::Git;
use crate::error::{Fail, R};
use crate::hash::{Kind, Oid};
use crate::object;
use crate::opts::{self, Spec};
use crate::os;
use crate::repo::Repo;

const SPECS: &[Spec] = &[
    opts::short_flag(b't', "type"),
    opts::short_flag(b's', "size"),
    opts::short_flag(b'e', "exists"),
    opts::short_flag(b'p', "pretty"),
    opts::optional(None, "batch", "batch"),
    opts::optional(None, "batch-check", "batch-check"),
    opts::optional(None, "batch-command", "batch-command"),
    opts::flag(None, "batch-all-objects", "all"),
    opts::flag(None, "buffer", "buffer"),
    opts::flag(None, "unordered", "unordered"),
    opts::flag(None, "follow-symlinks", "follow"),
    opts::flag(None, "allow-unknown-type", "allow-unknown"),
    opts::flag(None, "textconv", "textconv"),
    opts::flag(None, "filters", "filters"),
    opts::value(None, "path", "path"),
    opts::flag(None, "use-mailmap", "mailmap"),
    opts::flag(None, "mailmap", "mailmap"),
    opts::short_flag(b'Z', "nul"),
];

fn invalid_name(spec: &[u8]) -> Fail {
    Fail::Fatal(format!("Not a valid object name {}", os::lossy(spec)))
}

/// Conteúdo "bonito" (o `-p`): tree listada, o resto cru.
pub fn pretty(repo: &Repo, kind: Kind, data: &[u8]) -> R<Vec<u8>> {
    if kind != Kind::Tree {
        return Ok(data.to_vec());
    }
    let mut out = Vec::new();
    for e in object::parse_tree(data).map_err(Fail::Fatal)? {
        let k = object::kind_of_mode(e.mode);
        out.extend_from_slice(format!("{:06o} {} {}\t", e.mode, k.name(), e.oid).as_bytes());
        out.extend_from_slice(&e.name);
        out.push(b'\n');
    }
    let _ = repo;
    Ok(out)
}

pub fn run(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let p = opts::parse(SPECS, args, 0, usage)?;
    let repo = git.repo()?;
    let batch = ["batch", "batch-check", "batch-command"].into_iter().find(|b| p.present(b));
    if let Some(mode) = batch {
        return run_batch(repo, mode, p.value(mode).map(|v| v.to_vec()), p.has("all"), p.has("nul"));
    }
    let mode = ["type", "size", "exists", "pretty"].into_iter().find(|m| p.has(m));
    match mode {
        Some(m) => {
            if p.args.len() != 1 {
                return Err(opts::usage_fatal(usage, &format!("<object> required with '-{}'", &m[..1])));
            }
            let spec = &p.args[0];
            let Some(id) = repo.rev_parse(spec)? else { return Err(invalid_name(spec)) };
            let obj = repo.try_read(&id)?;
            match m {
                "exists" => Ok(if obj.is_some() { 0 } else { 1 }),
                _ => {
                    let Some((kind, data)) = obj else { return Err(invalid_name(spec)) };
                    match m {
                        "type" => os::outs(&format!("{}\n", kind.name())),
                        "size" => os::outs(&format!("{}\n", data.len())),
                        _ => os::out(&pretty(repo, kind, &data)?),
                    }
                    Ok(0)
                }
            }
        }
        None => {
            if p.args.len() != 2 {
                return Err(opts::usage_help(usage));
            }
            let want = Kind::from_name(&p.args[0]).ok_or_else(|| Fail::Fatal(format!("invalid object type \"{}\"", os::lossy(&p.args[0]))))?;
            let spec = &p.args[1];
            let Some(id) = repo.rev_parse(spec)? else { return Err(invalid_name(spec)) };
            let peeled = repo.peel(&id, Some(want))?;
            let Some(target) = peeled else {
                return Err(Fail::Fatal(format!("git cat-file {}: bad file", os::lossy(spec))));
            };
            let (_, data) = repo.read_object(&target)?;
            os::out(&data);
            Ok(0)
        }
    }
}

/// Formato do `--batch`: `%(objectname) %(objecttype) %(objectsize)` e afins.
fn batch_line(fmt: &[u8], id: &Oid, kind: Kind, size: usize, rest: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < fmt.len() {
        if fmt[i..].starts_with(b"%(") {
            if let Some(end) = fmt[i..].iter().position(|c| *c == b')') {
                let atom = &fmt[i + 2..i + end];
                match atom {
                    b"objectname" => out.extend_from_slice(id.hex().as_bytes()),
                    b"objecttype" => out.extend_from_slice(kind.name().as_bytes()),
                    b"objectsize" | b"objectsize:disk" => out.extend_from_slice(size.to_string().as_bytes()),
                    b"rest" => out.extend_from_slice(rest),
                    b"deltabase" => out.extend_from_slice(Oid::ZERO.hex().as_bytes()),
                    _ => {
                        out.extend_from_slice(&fmt[i..i + end + 1]);
                    }
                }
                i += end + 1;
                continue;
            }
        }
        out.push(fmt[i]);
        i += 1;
    }
    out
}

fn run_batch(repo: &Repo, mode: &str, fmt: Option<Vec<u8>>, all: bool, nul: bool) -> R<i32> {
    let fmt = fmt.unwrap_or_else(|| b"%(objectname) %(objecttype) %(objectsize)".to_vec());
    let with_contents = mode == "batch";
    let mut out = Vec::new();
    let emit = |out: &mut Vec<u8>, id: Oid, input: &[u8], rest: &[u8], contents: bool| -> R<()> {
        match repo.try_read(&id)? {
            Some((kind, data)) => {
                out.extend_from_slice(&batch_line(&fmt, &id, kind, data.len(), rest));
                out.push(b'\n');
                if contents {
                    out.extend_from_slice(&data);
                    out.push(b'\n');
                }
            }
            None => {
                out.extend_from_slice(input);
                out.extend_from_slice(b" missing\n");
            }
        }
        Ok(())
    };
    if all {
        let mut ids = repo.odb.loose_ids();
        ids.extend(repo.odb.packed_ids());
        ids.sort();
        ids.dedup();
        for id in ids {
            emit(&mut out, id, id.hex().as_bytes(), b"", with_contents)?;
        }
        os::out(&out);
        return Ok(0);
    }
    let input = os::stdin_all();
    let sep = if nul { 0 } else { b'\n' };
    for line in input.split(|c| *c == sep) {
        if line.is_empty() {
            continue;
        }
        let (cmd_contents, spec_line) = if mode == "batch-command" {
            if let Some(r) = line.strip_prefix(b"contents ") {
                (true, r)
            } else if let Some(r) = line.strip_prefix(b"info ") {
                (false, r)
            } else if line == b"flush" {
                continue;
            } else {
                return Err(Fail::Fatal(format!("unknown command: '{}'", os::lossy(line))));
            }
        } else {
            (with_contents, line)
        };
        // Com %(rest) no formato, o objeto é a primeira palavra.
        let (spec, rest) = if fmt.windows(7).any(|w| w == b"%(rest)") {
            match spec_line.iter().position(|c| c.is_ascii_whitespace()) {
                Some(sp) => (&spec_line[..sp], &spec_line[sp + 1..]),
                None => (spec_line, &b""[..]),
            }
        } else {
            (spec_line, &b""[..])
        };
        match repo.rev_parse(spec) {
            Ok(Some(id)) => emit(&mut out, id, spec, rest, cmd_contents)?,
            _ => {
                out.extend_from_slice(spec);
                out.extend_from_slice(b" missing\n");
            }
        }
    }
    os::out(&out);
    Ok(0)
}
