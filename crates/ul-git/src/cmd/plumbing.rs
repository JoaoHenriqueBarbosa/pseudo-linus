//! Comandos de baixo nível: `hash-object`, `write-tree`, `commit-tree`, `update-ref`,
//! `symbolic-ref`, `show-ref`, `update-index`, `read-tree`, `merge-base`, `check-ref-format`,
//! `check-ignore`, `count-objects`.

use super::Git;
use crate::error::{Fail, R, error};
use crate::graph::Graph;
use crate::hash::{self, Kind, Oid};
use crate::ident::{self, Who};
use crate::ignore::Ignores;
use crate::index::{IEntry, Index};
use crate::object;
use crate::opts::{self, Spec};
use crate::os;
use crate::refs;
use crate::repo::Repo;

// ---- hash-object ------------------------------------------------------------------------------

const HASH_OBJECT: &[Spec] = &[
    opts::short_value(b't', "type"),
    opts::short_flag(b'w', "write"),
    opts::flag(None, "stdin", "stdin"),
    opts::flag(None, "stdin-paths", "stdin-paths"),
    opts::flag(None, "no-filters", "no-filters"),
    opts::flag(None, "literally", "literally"),
    opts::value(None, "path", "path"),
];

/// Valida o formato do objeto como o `hash-object` faz sem `--literally`.
fn check_format(kind: Kind, data: &[u8]) -> Result<(), String> {
    match kind {
        Kind::Blob => Ok(()),
        Kind::Tree => object::parse_tree(data).map(|_| ()).map_err(|_| "corrupt tree".to_string()),
        Kind::Commit => object::parse_commit(data).map(|_| ()).map_err(|_| "corrupt commit".to_string()),
        Kind::Tag => object::parse_tag(data).map(|_| ()).map_err(|_| "corrupt tag".to_string()),
    }
}

pub fn hash_object(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let p = opts::parse(HASH_OBJECT, args, 0, usage)?;
    let kind = match p.value("type") {
        None => Kind::Blob,
        Some(t) => Kind::from_name(t).ok_or_else(|| Fail::Fatal(format!("invalid object type \"{}\"", os::lossy(t))))?,
    };
    let write = p.has("write");
    if write && git.repo.is_none() {
        return Err(crate::repo::not_a_repository());
    }
    let prefix = git.repo.as_ref().map(|r| r.prefix.clone()).unwrap_or_default();
    let mut inputs: Vec<(Option<Vec<u8>>, Vec<u8>)> = Vec::new();
    if p.has("stdin") {
        inputs.push((None, os::stdin_all()));
    }
    let mut paths = p.args.clone();
    if p.has("stdin-paths") {
        for l in os::stdin_all().split(|c| *c == b'\n') {
            if !l.is_empty() {
                paths.push(l.to_vec());
            }
        }
    }
    for path in paths {
        let full = if path.starts_with(b"/") || prefix.is_empty() { path.clone() } else { os::join(&prefix, &path) };
        let st = os::lstat(&full).map_err(|e| Fail::Fatal(format!("could not open '{}' for reading: {}", os::lossy(&path), e.message())))?;
        let data = if st.file_type() == sysabi::FileType::Symlink && kind == Kind::Blob {
            os::readlink(&full).map_err(|e| Fail::Fatal(format!("readlink {}: {}", os::lossy(&path), e.message())))?
        } else {
            os::read(&full).map_err(|e| Fail::Fatal(format!("could not open '{}' for reading: {}", os::lossy(&path), e.message())))?
        };
        inputs.push((Some(path), data));
    }
    let mut out = String::new();
    for (_, data) in inputs {
        if !p.has("literally")
            && let Err(e) = check_format(kind, &data)
        {
            return Err(Fail::Fatal(e));
        }
        let id = if write { git.repo()?.write_object(kind, &data)? } else { hash::hash_object(kind, &data) };
        out.push_str(&id.hex());
        out.push('\n');
    }
    os::outs(&out);
    Ok(0)
}

// ---- write-tree -------------------------------------------------------------------------------

pub fn write_tree(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let specs = [opts::flag(None, "missing-ok", "missing-ok"), opts::value(None, "prefix", "prefix")];
    let p = opts::parse(&specs, args, 0, usage)?;
    if !p.args.is_empty() {
        return Err(opts::usage_fatal(usage, "too many arguments"));
    }
    let repo = git.repo()?;
    let idx = Index::load(&repo.index_path())?;
    if idx.has_conflicts() {
        for path in idx.unmerged_paths() {
            let e = idx.stages(&path)[0];
            error(&format!("{}: unmerged ({})", os::lossy(&path), e.oid));
        }
        return Err(Fail::Fatal("git-write-tree: error building trees".into()));
    }
    if !p.has("missing-ok") {
        for e in &idx.entries {
            if !object::is_gitlink(e.mode) && !e.intent_to_add() && !repo.odb.exists(&e.oid) {
                error(&format!("invalid object {:06o} {} for '{}'", e.mode, e.oid, os::lossy(&e.path)));
                return Err(Fail::Fatal("git-write-tree: error building trees".into()));
            }
        }
    }
    let mut id = repo.write_tree_from_index(&idx)?;
    if let Some(pre) = p.value("prefix") {
        let pre = pre.strip_suffix(b"/").unwrap_or(pre);
        match repo.tree_lookup(&id, pre)? {
            Some((m, t)) if object::is_tree_mode(m) => id = t,
            _ => return Err(Fail::Fatal(format!("git-write-tree: prefix {} not found", os::lossy(pre)))),
        }
    }
    os::outs(&format!("{id}\n"));
    Ok(0)
}

// ---- commit-tree ------------------------------------------------------------------------------

pub fn commit_tree(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let specs = [
        opts::short_value(b'p', "parent"),
        opts::short_value(b'm', "message"),
        opts::short_value(b'F', "file"),
        opts::optional(Some(b'S'), "gpg-sign", "gpg-sign"),
    ];
    let p = opts::parse(&specs, args, 0, usage)?;
    let repo = git.repo()?;
    if p.args.len() != 1 {
        return Err(opts::usage_fatal(usage, "must give exactly one tree"));
    }
    let tree_spec = &p.args[0];
    let tree = repo.rev_parse(tree_spec)?.ok_or_else(|| Fail::Fatal(format!("not a valid object name {}", os::lossy(tree_spec))))?;
    let tree = repo.peel_to_tree(&tree)?.ok_or_else(|| Fail::Fatal(format!("{} is not a valid 'tree' object", os::lossy(tree_spec))))?;
    let mut parents: Vec<Oid> = Vec::new();
    let mut msg: Vec<u8> = Vec::new();
    let mut have_msg = false;
    for h in &p.hits {
        let v = h.value.clone().unwrap_or_default();
        match h.id {
            "parent" => {
                let id = repo.rev_parse(&v)?.ok_or_else(|| Fail::Fatal(format!("not a valid object name {}", os::lossy(&v))))?;
                let c = repo.peel_to_commit(&id)?.ok_or_else(|| Fail::Fatal(format!("{} is not a valid 'commit' object", os::lossy(&v))))?;
                if parents.contains(&c) {
                    error(&format!("duplicate parent {c} ignored"));
                } else {
                    parents.push(c);
                }
            }
            "message" => {
                if !msg.is_empty() {
                    msg.push(b'\n');
                }
                msg.extend_from_slice(&v);
                if !msg.ends_with(b"\n") {
                    msg.push(b'\n');
                }
                have_msg = true;
            }
            "file" => {
                if !msg.is_empty() {
                    msg.push(b'\n');
                }
                let data = if v == b"-" { os::stdin_all() } else { read_user_file(repo, &v)? };
                msg.extend_from_slice(&data);
                if !msg.is_empty() && !msg.ends_with(b"\n") {
                    msg.push(b'\n');
                }
                have_msg = true;
            }
            _ => {}
        }
    }
    if !have_msg {
        msg = os::stdin_all();
    }
    let author = ident::ident(&repo.config, Who::Author, true)?;
    let committer = ident::ident(&repo.config, Who::Committer, true)?;
    let c = object::Commit { tree, parents, author: author.to_bytes(), committer: committer.to_bytes(), encoding: None, extra: Vec::new(), message: msg };
    let id = repo.write_object(Kind::Commit, &object::encode_commit(&c))?;
    os::outs(&format!("{id}\n"));
    Ok(0)
}

/// Lê arquivo dado pelo usuário (relativo ao cwd original).
pub fn read_user_file(repo: &Repo, path: &[u8]) -> R<Vec<u8>> {
    let full = if path.starts_with(b"/") || repo.prefix.is_empty() { path.to_vec() } else { os::join(&repo.prefix, path) };
    os::read(&full).map_err(|e| Fail::Fatal(format!("could not open '{}' for reading: {}", os::lossy(path), e.message())))
}

// ---- update-ref -------------------------------------------------------------------------------

pub fn update_ref(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let specs = [
        opts::short_value(b'm', "msg"),
        opts::short_flag(b'd', "delete"),
        opts::flag(None, "no-deref", "no-deref"),
        opts::flag(None, "stdin", "stdin"),
        opts::short_flag(b'z', "z"),
        opts::flag(None, "create-reflog", "create-reflog"),
    ];
    let p = opts::parse(&specs, args, 0, usage)?;
    let repo = git.repo()?;
    let msg = p.value_str("msg").unwrap_or_default();
    let resolve = |v: &[u8]| -> R<Oid> { repo.rev_parse(v)?.ok_or_else(|| Fail::Fatal(format!("{}: not a valid SHA1", os::lossy(v)))) };
    let wrap = |name: &str, e: Fail| -> Fail {
        match e {
            Fail::Fatal(m) => Fail::Fatal(format!("update_ref failed for ref '{name}': {m}")),
            other => other,
        }
    };
    if p.has("stdin") {
        return update_ref_stdin(repo, &msg, p.has("z"));
    }
    if p.has("delete") {
        if p.args.is_empty() || p.args.len() > 2 {
            return Err(opts::usage_fatal(usage, "-d requires one or two arguments"));
        }
        let name = os::lossy(&p.args[0]);
        let old = match p.args.get(1) {
            Some(v) => Some(resolve(v)?),
            None => None,
        };
        let target = if p.has("no-deref") { name.clone() } else { repo.resolve_ref(&name)?.map(|(n, _)| n).unwrap_or(name.clone()) };
        repo.delete_ref(&target, old).map_err(|e| wrap(&name, e))?;
        return Ok(0);
    }
    if p.args.len() < 2 || p.args.len() > 3 {
        return Err(opts::usage_fatal(usage, "update-ref requires two or three arguments"));
    }
    let name = os::lossy(&p.args[0]);
    let new = resolve(&p.args[1])?;
    let old = match p.args.get(2) {
        None => None,
        Some(v) if v.is_empty() => Some(None),
        Some(v) => {
            let id = resolve(v)?;
            Some(if id.is_zero() { None } else { Some(id) })
        }
    };
    if p.has("create-reflog") {
        let _ = os::mkdir_parents(&repo.path(&format!("logs/{name}")));
        let _ = os::append(&repo.path(&format!("logs/{name}")), b"", 0o666);
    }
    repo.update_ref(&name, new, old, &msg, p.has("no-deref")).map_err(|e| wrap(&name, e))?;
    Ok(0)
}

fn update_ref_stdin(repo: &Repo, msg: &str, z: bool) -> R<i32> {
    let input = os::stdin_all();
    let lines: Vec<&[u8]> = if z { input.split(|c| *c == 0).collect() } else { input.split(|c| *c == b'\n').collect() };
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let words: Vec<&[u8]> = line.split(|c| *c == b' ').collect();
        let get = |i: usize| -> R<Option<Oid>> {
            match words.get(i) {
                None => Ok(None),
                Some(w) if w.is_empty() => Ok(None),
                Some(w) => Ok(Some(repo.rev_parse(w)?.ok_or_else(|| Fail::Fatal(format!("invalid new value for ref {}: {}", os::lossy(words[1]), os::lossy(w))))?)),
            }
        };
        match words[0] {
            b"update" => {
                let name = os::lossy(words.get(1).copied().unwrap_or_default());
                let new = get(2)?.ok_or_else(|| Fail::Fatal(format!("update {name}: missing <new-oid>")))?;
                let old = if words.len() > 3 { Some(get(3)?) } else { None };
                repo.update_ref(&name, new, old, msg, false)?;
            }
            b"create" => {
                let name = os::lossy(words.get(1).copied().unwrap_or_default());
                let new = get(2)?.ok_or_else(|| Fail::Fatal(format!("create {name}: missing <new-oid>")))?;
                repo.update_ref(&name, new, Some(None), msg, false)?;
            }
            b"delete" => {
                let name = os::lossy(words.get(1).copied().unwrap_or_default());
                repo.delete_ref(&name, get(2)?)?;
            }
            b"verify" => {
                let name = os::lossy(words.get(1).copied().unwrap_or_default());
                let want = get(2)?;
                if repo.ref_oid(&name)? != want {
                    return Err(Fail::Fatal(format!("cannot lock ref '{name}': reference is missing but expected {}", want.unwrap_or(Oid::ZERO))));
                }
            }
            b"start" | b"prepare" | b"commit" | b"abort" | b"option" => {}
            other => return Err(Fail::Fatal(format!("unknown command: {}", os::lossy(other)))),
        }
    }
    Ok(0)
}

// ---- symbolic-ref -----------------------------------------------------------------------------

pub fn symbolic_ref(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let specs = [
        opts::flag(Some(b'q'), "quiet", "quiet"),
        opts::flag(Some(b'd'), "delete", "delete"),
        opts::flag(None, "short", "short"),
        opts::flag(None, "recurse", "recurse"),
        opts::short_value(b'm', "msg"),
    ];
    let p = opts::parse(&specs, args, 0, usage)?;
    let repo = git.repo()?;
    let quiet = p.has("quiet");
    if p.has("delete") {
        let Some(name) = p.args.first() else { return Err(opts::usage_fatal(usage, "Refusing to delete nothing")) };
        let name = os::lossy(name);
        if repo.symref_target(&name)?.is_none() {
            if quiet {
                return Ok(1);
            }
            return Err(Fail::Fatal(format!("Cannot delete {name}, not a symbolic ref")));
        }
        if name == "HEAD" {
            return Err(Fail::Fatal("deleting 'HEAD' is not allowed".into()));
        }
        let _ = os::unlink(&repo.ref_file(&name));
        return Ok(0);
    }
    match p.args.len() {
        1 => {
            let name = os::lossy(&p.args[0]);
            let mut target = match repo.symref_target(&name)? {
                Some(t) => t,
                None => {
                    if quiet {
                        return Ok(1);
                    }
                    return Err(Fail::Fatal(format!("ref {name} is not a symbolic ref")));
                }
            };
            if p.flag("recurse") != Some(false) {
                while let Some(t) = repo.symref_target(&target)? {
                    target = t;
                }
            }
            if p.has("short") {
                target = shorten_ref(repo, &target);
            }
            os::outs(&format!("{target}\n"));
            Ok(0)
        }
        2 => {
            let name = os::lossy(&p.args[0]);
            let target = os::lossy(&p.args[1]);
            if name == "HEAD" && !target.starts_with("refs/") {
                return Err(Fail::Fatal("Refusing to point HEAD outside of refs/".to_string()));
            }
            if !refs::check_refname_format(&target, true, false) {
                return Err(Fail::Fatal(format!("Refusing to set '{name}' to invalid ref '{target}'")));
            }
            let msg = p.value_str("msg");
            repo.set_symref(&name, &target, msg.as_deref())?;
            Ok(0)
        }
        _ => Err(opts::usage_help(usage)),
    }
}

/// O `shorten_unambiguous_ref` do git.
pub fn shorten_ref(repo: &Repo, name: &str) -> String {
    const RULES: [(&str, &str); 5] = [
        ("refs/", ""),
        ("refs/tags/", ""),
        ("refs/heads/", ""),
        ("refs/remotes/", ""),
        ("refs/remotes/", "/HEAD"),
    ];
    // Do mais específico pro mais geral: a primeira regra que dá um nome curto sem ambiguidade.
    for (i, (pre, suf)) in RULES.iter().enumerate().rev() {
        let Some(rest) = name.strip_prefix(pre) else { continue };
        let Some(short) = rest.strip_suffix(suf) else { continue };
        if short.is_empty() {
            continue;
        }
        // Nenhuma regra anterior (mais forte) pode casar com outro ref existente.
        let ambiguous = RULES[..i].iter().any(|(p2, s2)| {
            let candidate = format!("{p2}{short}{s2}");
            candidate != name && repo.read_ref(&candidate).ok().flatten().is_some()
        }) || (i > 0 && repo.read_ref(short).ok().flatten().is_some() && short != name);
        if !ambiguous {
            return short.to_string();
        }
    }
    name.to_string()
}

// ---- show-ref ---------------------------------------------------------------------------------

pub fn show_ref(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let specs = [
        opts::flag(None, "tags", "tags"),
        opts::flag(None, "heads", "heads"),
        opts::flag(None, "branches", "heads"),
        opts::flag(None, "head", "head"),
        opts::flag(None, "verify", "verify"),
        opts::flag(None, "exists", "exists"),
        opts::optional(Some(b's'), "hash", "hash"),
        opts::optional(None, "abbrev", "abbrev"),
        opts::flag(Some(b'd'), "dereference", "deref"),
        opts::flag(Some(b'q'), "quiet", "quiet"),
        opts::flag(None, "exclude-existing", "exclude-existing"),
    ];
    let p = opts::parse(&specs, args, 0, usage)?;
    let repo = git.repo()?;
    let quiet = p.has("quiet");
    let abbrev: Option<usize> = if p.present("hash") {
        p.value("hash").and_then(|v| os::lossy(v).parse().ok()).or(Some(40))
    } else {
        None
    };
    let abbrev_len = p.value("abbrev").and_then(|v| os::lossy(v).parse::<usize>().ok()).or_else(|| p.present("abbrev").then(|| repo.abbrev_len()));
    let fmt_id = |id: &Oid| -> String {
        match (abbrev, abbrev_len) {
            (Some(n), _) if n < 40 => repo.abbrev(id, n.max(4)),
            (_, Some(n)) => repo.abbrev(id, n.max(4)),
            _ => id.hex(),
        }
    };
    let hash_only = p.present("hash");
    let show = |name: &str, id: &Oid, out: &mut String| {
        if quiet {
            return;
        }
        if hash_only {
            out.push_str(&fmt_id(id));
        } else {
            out.push_str(&format!("{} {name}", fmt_id(id)));
        }
        out.push('\n');
    };
    let mut out = String::new();
    if p.has("exists") {
        let name = os::lossy(p.args.first().map(|v| v.as_slice()).unwrap_or_default());
        return Ok(if repo.read_ref(&name)?.is_some() {
            0
        } else {
            error(&"reference does not exist".to_string());
            2
        });
    }
    if p.has("verify") {
        let mut code = 0;
        for a in &p.args {
            let name = os::lossy(a);
            if (name.starts_with("refs/") || name == "HEAD") && let Some(id) = repo.ref_oid(&name)? {
                show(&name, &id, &mut out);
                if p.has("deref")
                    && let Some(pid) = repo.peel(&id, None)?
                    && pid != id
                {
                    show(&format!("{name}^{{}}"), &pid, &mut out);
                }
            } else {
                if !quiet {
                    os::outs(&out);
                    out.clear();
                    return Err(Fail::Fatal(format!("'{name}' - not a valid ref")));
                }
                code = 1;
            }
        }
        os::outs(&out);
        return Ok(code);
    }
    let mut found = false;
    if p.has("head")
        && let Some(id) = repo.head_oid()?
    {
        show("HEAD", &id, &mut out);
        found = true;
    }
    for (name, id) in repo.list_refs("refs/")? {
        if (p.has("tags") || p.has("heads")) && !((p.has("tags") && name.starts_with("refs/tags/")) || (p.has("heads") && name.starts_with("refs/heads/"))) {
            continue;
        }
        if !p.args.is_empty() {
            let ok = p.args.iter().any(|pat| {
                let pat = os::lossy(pat);
                name == pat || name.ends_with(&format!("/{pat}"))
            });
            if !ok {
                continue;
            }
        }
        found = true;
        show(&name, &id, &mut out);
        if p.has("deref")
            && let Some(pid) = repo.peel(&id, None)?
            && pid != id
        {
            show(&format!("{name}^{{}}"), &pid, &mut out);
        }
    }
    os::outs(&out);
    Ok(if found { 0 } else { 1 })
}

// ---- update-index -----------------------------------------------------------------------------

pub fn update_index(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let repo = git.repo()?;
    let mut idx = Index::load(&repo.index_path())?;
    let trust = repo.config.get_bool("core.filemode")?.unwrap_or(true);
    let mut add = false;
    let mut remove = false;
    let mut force_remove = false;
    let mut chmod: Option<bool> = None;
    let mut i = 0;
    let mut changed = false;
    let mut quiet = false;
    while i < args.len() {
        let a = os::lossy(&args[i]);
        i += 1;
        match a.as_str() {
            "--add" => add = true,
            "--remove" => remove = true,
            "--force-remove" => force_remove = true,
            "-q" => quiet = true,
            "--refresh" | "--really-refresh" => {
                if crate::worktree::refresh(&mut idx, trust) {
                    changed = true;
                }
                // Avisa dos que precisam de update.
                for e in &idx.entries {
                    if e.stage == 0
                        && let crate::diff::WtState::Changed(..) | crate::diff::WtState::Deleted = crate::diff::check_entry(repo, &idx, e, trust)?
                        && !quiet
                    {
                        os::outs(&format!("{}: needs update\n", os::lossy(&e.path)));
                    }
                }
            }
            "--cacheinfo" => {
                let Some(v) = args.get(i) else { return Err(opts::usage_error(usage, "option 'cacheinfo' expects <mode>,<sha1>,<path>")) };
                let parts: Vec<&[u8]> = v.splitn(3, |c| *c == b',').collect();
                let (mode, id, path) = if parts.len() == 3 {
                    i += 1;
                    (parts[0].to_vec(), parts[1].to_vec(), parts[2].to_vec())
                } else {
                    let m = args.get(i).cloned().unwrap_or_default();
                    let s = args.get(i + 1).cloned().unwrap_or_default();
                    let pth = args.get(i + 2).cloned().unwrap_or_default();
                    i += 3;
                    (m, s, pth)
                };
                let mode = u32::from_str_radix(&os::lossy(&mode), 8).map_err(|_| Fail::Fatal("git update-index: --cacheinfo cannot add bad mode".into()))?;
                let id = Oid::from_hex(&id).ok_or_else(|| Fail::Fatal(format!("git update-index: --cacheinfo cannot add {}", os::lossy(&path))))?;
                idx.add(IEntry::bare(path, id, object::canon_mode(mode)));
                changed = true;
            }
            "--chmod=+x" => chmod = Some(true),
            "--chmod=-x" => chmod = Some(false),
            "--assume-unchanged" | "--no-assume-unchanged" | "--skip-worktree" | "--no-skip-worktree" => {
                let flag = a.clone();
                while i < args.len() && !args[i].starts_with(b"-") {
                    let path = repo.rel_from_prefix(&args[i]).unwrap_or_default();
                    if let Ok(k) = idx.pos(&path, 0) {
                        let e = &mut idx.entries[k];
                        match flag.as_str() {
                            "--assume-unchanged" => e.flags |= crate::index::FLAG_ASSUME_VALID,
                            "--no-assume-unchanged" => e.flags &= !crate::index::FLAG_ASSUME_VALID,
                            "--skip-worktree" => e.ext |= crate::index::EXT_SKIP_WORKTREE,
                            _ => e.ext &= !crate::index::EXT_SKIP_WORKTREE,
                        }
                        changed = true;
                    } else {
                        return Err(Fail::Fatal(format!("Unable to mark file {}", os::lossy(&args[i]))));
                    }
                    i += 1;
                }
            }
            "--index-info" => {
                for line in os::stdin_all().split(|c| *c == b'\n') {
                    let Some(tab) = line.iter().position(|c| *c == b'\t') else { continue };
                    let meta: Vec<&[u8]> = line[..tab].split(|c| *c == b' ').collect();
                    let path = line[tab + 1..].to_vec();
                    let mode = u32::from_str_radix(&os::lossy(meta[0]), 8).unwrap_or(0);
                    let id = meta.iter().find_map(|m| Oid::from_hex(m)).unwrap_or(Oid::ZERO);
                    if mode == 0 {
                        idx.remove(&path);
                    } else {
                        idx.add(IEntry::bare(path, id, mode));
                    }
                    changed = true;
                }
            }
            "--" => {}
            _ if a.starts_with('-') => return Err(opts::usage_error(usage, &format!("unknown option '{}'", a.trim_start_matches('-')))),
            _ => {
                let path = repo.rel_from_prefix(&args[i - 1]).ok_or_else(|| Fail::Fatal(format!("'{a}' is outside repository")))?;
                if force_remove {
                    idx.remove(&path);
                    changed = true;
                    continue;
                }
                match os::lstat(&path) {
                    Ok(st) => {
                        if st.file_type() == sysabi::FileType::Directory {
                            error(&format!("{a}: is a directory - add files inside instead"));
                            return Err(Fail::Fatal(format!("Unable to process path {a}")));
                        }
                        if idx.get(&path).is_none() && !add {
                            error(&format!("{a}: cannot add to the index - missing --add option?"));
                            return Err(Fail::Fatal(format!("Unable to process path {a}")));
                        }
                        let data = crate::diff::worktree_blob(&path, &st)?;
                        let id = repo.write_object(Kind::Blob, &data)?;
                        let old = idx.get(&path).map(|e| e.mode);
                        let mut mode = crate::index::mode_for(&st, old, trust);
                        if let Some(x) = chmod {
                            mode = if x { object::MODE_EXEC } else { object::MODE_BLOB };
                        }
                        idx.add(IEntry::from_stat(path, id, mode, &st));
                        changed = true;
                    }
                    Err(_) => {
                        if remove {
                            idx.remove(&path);
                            changed = true;
                        } else if let Some(x) = chmod
                            && let Ok(k) = idx.pos(&path, 0)
                        {
                            idx.entries[k].mode = if x { object::MODE_EXEC } else { object::MODE_BLOB };
                            changed = true;
                        } else {
                            error(&format!("{a}: does not exist and --remove not passed"));
                            return Err(Fail::Fatal(format!("Unable to process path {a}")));
                        }
                    }
                }
            }
        }
    }
    if changed {
        idx.write(&repo.index_path())?;
    }
    Ok(0)
}

// ---- read-tree --------------------------------------------------------------------------------

pub fn read_tree(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let specs = [
        opts::short_flag(b'm', "merge"),
        opts::flag(None, "reset", "reset"),
        opts::short_flag(b'u', "update"),
        opts::short_flag(b'i', "index-only"),
        opts::value(None, "prefix", "prefix"),
        opts::flag(None, "empty", "empty"),
        opts::short_flag(b'n', "dry-run"),
        opts::flag(None, "dry-run", "dry-run"),
        opts::flag(None, "no-sparse-checkout", "nosparse"),
        opts::short_flag(b'v', "verbose"),
        opts::flag(None, "trivial", "trivial"),
        opts::flag(None, "aggressive", "aggressive"),
    ];
    let p = opts::parse(&specs, args, 0, usage)?;
    let repo = git.repo()?;
    let ipath = repo.index_path();
    if p.has("empty") {
        if !p.has("dry-run") {
            Index { version: 2, ..Index::default() }.write(&ipath)?;
        }
        return Ok(0);
    }
    if p.args.is_empty() {
        return Err(opts::usage_help(usage));
    }
    if p.args.len() > 1 {
        return Err(Fail::Fatal("read-tree with more than one tree is not supported here; use git merge".into()));
    }
    let id = repo.rev_parse(&p.args[0])?.ok_or_else(|| Fail::Fatal(format!("Not a valid object name {}", os::lossy(&p.args[0]))))?;
    let tree = repo.peel_to_tree(&id)?.ok_or_else(|| Fail::Fatal(format!("failed to unpack tree object {}", os::lossy(&p.args[0]))))?;
    if p.has("dry-run") {
        return Ok(0);
    }
    if let Some(pre) = p.value("prefix") {
        let mut idx = Index::load(&ipath)?;
        let mut pre = pre.to_vec();
        if !pre.is_empty() && !pre.ends_with(b"/") {
            pre.push(b'/');
        }
        for (path, (mode, oid)) in repo.flatten_tree(&tree)? {
            let full = [pre.as_slice(), path.as_slice()].concat();
            if idx.get(&full).is_some() {
                return Err(Fail::Fatal(format!("Entry '{}' overlaps with '{}'.  Cannot bind.", os::lossy(&full), os::lossy(&full))));
            }
            idx.add(IEntry::bare(full, oid, mode));
        }
        idx.write(&ipath)?;
        return Ok(0);
    }
    let mut new = repo.index_from_tree(&tree)?;
    if p.has("update") {
        let old = Index::load(&ipath)?;
        let wt = repo.work_tree()?.to_vec();
        let _ = wt;
        for e in &old.entries {
            if new.get(&e.path).is_none() {
                crate::worktree::unlink_entry(&e.path);
            }
        }
        for e in new.entries.iter_mut() {
            let same = old.get(&e.path).is_some_and(|o| o.oid == e.oid && o.mode == e.mode);
            if same && os::exists(&e.path) {
                if let Some(o) = old.get(&e.path) {
                    *e = o.clone();
                }
                continue;
            }
            *e = crate::worktree::checkout_entry(repo, &e.path, e.mode, &e.oid)?;
        }
    } else if !p.has("reset") || p.has("merge") {
        // Mantém o stat das entradas iguais.
        let old = Index::load(&ipath)?;
        for e in new.entries.iter_mut() {
            if let Some(o) = old.get(&e.path)
                && o.oid == e.oid
                && o.mode == e.mode
            {
                *e = o.clone();
            }
        }
    }
    new.write(&ipath)?;
    Ok(0)
}

// ---- merge-base -------------------------------------------------------------------------------

pub fn merge_base(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let specs = [
        opts::flag(Some(b'a'), "all", "all"),
        opts::flag(None, "octopus", "octopus"),
        opts::flag(None, "independent", "independent"),
        opts::flag(None, "is-ancestor", "is-ancestor"),
        opts::flag(None, "fork-point", "fork-point"),
    ];
    let p = opts::parse(&specs, args, 0, usage)?;
    let repo = git.repo()?;
    let mut commits = Vec::new();
    for a in &p.args {
        let id = repo.rev_parse_commit(a)?.ok_or_else(|| Fail::Fatal(format!("Not a valid object name {}", os::lossy(a))))?;
        commits.push(id);
    }
    let mut g = Graph::new(repo);
    if p.has("is-ancestor") {
        if commits.len() != 2 {
            return Err(opts::usage_fatal(usage, "--is-ancestor takes exactly two commits"));
        }
        return Ok(if g.is_ancestor(&commits[0], &commits[1])? { 0 } else { 1 });
    }
    if p.has("independent") {
        let mut out = String::new();
        for c in g.independent(&commits)? {
            out.push_str(&format!("{c}\n"));
        }
        os::outs(&out);
        return Ok(0);
    }
    if p.has("fork-point") {
        if commits.is_empty() {
            return Err(opts::usage_help(usage));
        }
        let upstream = os::lossy(&p.args[0]);
        let head = match commits.get(1) {
            Some(c) => *c,
            None => repo.head_oid()?.ok_or_else(|| Fail::Fatal("No such ref: 'HEAD'".into()))?,
        };
        let full = repo.dwim_ref(&upstream)?.map(|(n, _)| n).unwrap_or(upstream);
        let mut log: Vec<Oid> = repo.read_reflog(&full).iter().map(|e| e.new).collect();
        log.reverse();
        log.insert(0, commits[0]);
        for cand in log {
            if g.is_ancestor(&cand, &head)? {
                os::outs(&format!("{cand}\n"));
                return Ok(0);
            }
        }
        return Ok(1);
    }
    if commits.len() < 2 && !p.has("octopus") {
        return Err(opts::usage_help(usage));
    }
    let bases = if p.has("octopus") {
        let mut acc = vec![commits[0]];
        for c in &commits[1..] {
            let mut next = Vec::new();
            for a in &acc {
                next.extend(g.merge_bases(a, &[*c])?);
            }
            acc = next;
        }
        acc
    } else {
        g.merge_bases(&commits[0], &commits[1..])?
    };
    if bases.is_empty() {
        return Ok(1);
    }
    let mut out = String::new();
    for b in if p.has("all") { &bases[..] } else { &bases[..1] } {
        out.push_str(&format!("{b}\n"));
    }
    os::outs(&out);
    Ok(0)
}

// ---- check-ref-format -------------------------------------------------------------------------

pub fn check_ref_format(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let mut onelevel = false;
    let mut pattern = false;
    let mut normalize = false;
    let mut i = 0;
    while i < args.len() && args[i].starts_with(b"--") {
        match args[i].as_slice() {
            b"--allow-onelevel" => onelevel = true,
            b"--no-allow-onelevel" => onelevel = false,
            b"--refspec-pattern" => pattern = true,
            b"--normalize" | b"--print" => normalize = true,
            b"--branch" => {
                let name = os::lossy(args.get(i + 1).map(|v| v.as_slice()).unwrap_or_default());
                let mut resolved = name.clone();
                if let Some(n) = name.strip_prefix("@{-").and_then(|r| r.strip_suffix('}')).and_then(|r| r.parse::<usize>().ok())
                    && let Some(repo) = git.repo.as_ref()
                        && let Some(b) = repo.nth_prior_branch(n)?
                    {
                        resolved = b;
                    }
                if !refs::valid_branch_name(&resolved) {
                    return Err(Fail::Fatal(format!("'{name}' is not a valid branch name")));
                }
                os::outs(&format!("{resolved}\n"));
                return Ok(0);
            }
            _ => break,
        }
        i += 1;
    }
    let Some(name) = args.get(i) else { return Err(opts::usage_help(git.usage())) };
    let mut name = os::lossy(name);
    if normalize {
        let collapsed: Vec<&str> = name.split('/').filter(|c| !c.is_empty()).collect();
        name = collapsed.join("/");
    }
    if !refs::check_refname_format(&name, onelevel, pattern) {
        return Ok(1);
    }
    if normalize {
        os::outs(&format!("{name}\n"));
    }
    Ok(0)
}

// ---- check-ignore -----------------------------------------------------------------------------

pub fn check_ignore(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let specs = [
        opts::flag(Some(b'q'), "quiet", "quiet"),
        opts::flag(Some(b'v'), "verbose", "verbose"),
        opts::flag(None, "stdin", "stdin"),
        opts::short_flag(b'z', "z"),
        opts::flag(Some(b'n'), "non-matching", "non-matching"),
        opts::flag(None, "no-index", "no-index"),
    ];
    let p = opts::parse(&specs, args, 0, usage)?;
    let repo = git.repo()?;
    let mut paths = p.args.clone();
    if p.has("stdin") {
        let input = os::stdin_all();
        let sep = if p.has("z") { 0 } else { b'\n' };
        paths.extend(input.split(|c| *c == sep).filter(|l| !l.is_empty()).map(|l| l.to_vec()));
    }
    if paths.is_empty() {
        return Err(Fail::Fatal("no path specified".into()));
    }
    if p.has("quiet") && paths.len() > 1 {
        return Err(Fail::Fatal("--quiet is only valid with a single pathname".into()));
    }
    if p.has("non-matching") && !p.has("verbose") {
        return Err(Fail::Fatal("--non-matching is only valid with --verbose".into()));
    }
    let idx = Index::load(&repo.index_path())?;
    let mut ign = Ignores::standard(repo);
    let mut any = false;
    let mut out = Vec::new();
    let term = if p.has("z") { 0 } else { b'\n' };
    for orig in &paths {
        let rel = repo.rel_from_prefix(orig).ok_or_else(|| Fail::Fatal(format!("{}: '{}' is outside repository at '{}'", os::lossy(orig), os::lossy(orig), os::lossy(repo.work_tree.as_deref().unwrap_or_default()))))?;
        let tracked = !p.has("no-index") && idx.has_path(&rel);
        let is_dir = os::is_dir(&rel);
        let m = if tracked { None } else { ign.matching(&rel, is_dir) };
        let shown = os::lossy(orig);
        match m {
            Some(pat) if !pat.negative || p.has("verbose") => {
                if !pat.negative {
                    any = true;
                }
                if p.has("quiet") {
                    continue;
                }
                if p.has("verbose") {
                    if pat.negative && !p.has("non-matching") {
                        // Negação casou: com -v mostra a regra mesmo assim.
                    }
                    out.extend_from_slice(&pat.source);
                    out.extend_from_slice(format!(":{}:", pat.line).as_bytes());
                    out.extend_from_slice(&pat.text);
                    out.push(if p.has("z") { 0 } else { b'\t' });
                }
                out.extend_from_slice(shown.as_bytes());
                out.push(term);
            }
            _ => {
                if p.has("non-matching") && !p.has("quiet") {
                    out.extend_from_slice(b"::");
                    out.push(if p.has("z") { 0 } else { b'\t' });
                    out.extend_from_slice(shown.as_bytes());
                    out.push(term);
                }
            }
        }
    }
    os::out(&out);
    Ok(if any { 0 } else { 1 })
}

// ---- count-objects ----------------------------------------------------------------------------

pub fn count_objects(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let specs = [opts::flag(Some(b'v'), "verbose", "verbose"), opts::flag(Some(b'H'), "human-readable", "human")];
    let p = opts::parse(&specs, args, 0, usage)?;
    let repo = git.repo()?;
    let mut count = 0u64;
    let mut kb = 0u64;
    for id in repo.odb.loose_ids() {
        count += 1;
        let hex = id.hex();
        let path = repo.common(&format!("objects/{}/{}", &hex[..2], &hex[2..]));
        if let Ok(st) = os::lstat(&path) {
            kb += st.blocks * 512 / 1024;
        }
    }
    let human = |k: u64| -> String {
        if k < 1024 {
            format!("{k}.00 KiB")
        } else {
            format!("{:.2} MiB", k as f64 / 1024.0)
        }
    };
    if !p.has("verbose") {
        if p.has("human") {
            os::outs(&format!("{count} objects, {}\n", human(kb)));
        } else {
            os::outs(&format!("{count} objects, {kb} kilobytes\n"));
        }
        return Ok(0);
    }
    let packs = repo.odb.pack_info();
    let in_pack: usize = packs.iter().map(|(_, n, _)| n).sum();
    let size_pack: u64 = packs.iter().map(|(_, _, s)| s / 1024).sum();
    let mut out = String::new();
    out.push_str(&format!("count: {count}\nsize: {}\nin-pack: {in_pack}\npacks: {}\nsize-pack: {}\n", if p.has("human") { human(kb) } else { kb.to_string() }, packs.len(), if p.has("human") { human(size_pack) } else { size_pack.to_string() }));
    out.push_str("prune-packable: 0\ngarbage: 0\nsize-garbage: 0\n");
    os::outs(&out);
    Ok(0)
}

/// Acrescenta ao índice o resultado de `rev_parse` com mensagem de erro do git.
pub fn resolve_or_die(repo: &Repo, spec: &[u8]) -> R<Oid> {
    repo.rev_parse(spec)?.ok_or_else(|| crate::rev::bad_revision(spec))
}

pub use hash::EMPTY_TREE;
