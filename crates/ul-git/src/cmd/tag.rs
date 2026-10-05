//! `git tag`: cria tags leves e anotadas, lista (`-l`, `-n`, `--format`, filtros), remove (`-d`) e
//! verifica (`-v`, só o formato do objeto: não há gpg aqui).

use super::Git;
use super::reffmt::{self, Ctx, Filter};
use crate::error::{Fail, R, error};
use crate::hash::{Kind, Oid};
use crate::ident::{self, Who};
use crate::msg::{self, Cleanup};
use crate::object::{self, Tag};
use crate::opts::{self, Arg, Spec};
use crate::os;
use crate::refs;
use crate::repo::Repo;

const SPECS: &[Spec] = &[
    opts::flag(Some(b'l'), "list", "list"),
    Spec { short: Some(b'n'), long: None, arg: Arg::Optional, negatable: false, id: "n" },
    opts::flag(Some(b'd'), "delete", "delete"),
    opts::flag(Some(b'v'), "verify", "verify"),
    opts::flag(Some(b'a'), "annotate", "annotate"),
    opts::value(Some(b'm'), "message", "message"),
    opts::value(Some(b'F'), "file", "file"),
    opts::value(None, "trailer", "trailer"),
    opts::flag(Some(b'e'), "edit", "edit"),
    opts::flag(Some(b's'), "sign", "sign"),
    opts::value(None, "cleanup", "cleanup"),
    opts::value(Some(b'u'), "local-user", "local-user"),
    opts::flag(Some(b'f'), "force", "force"),
    opts::flag(None, "create-reflog", "create-reflog"),
    opts::optional(None, "column", "column"),
    opts::noneg(opts::value(None, "contains", "contains")),
    opts::noneg(opts::value(None, "no-contains", "no-contains")),
    opts::noneg(opts::value(None, "merged", "merged")),
    opts::noneg(opts::value(None, "no-merged", "no-merged")),
    opts::flag(None, "omit-empty", "omit-empty"),
    opts::value(None, "sort", "sort"),
    opts::value(None, "points-at", "points-at"),
    opts::value(None, "format", "format"),
    opts::optional(None, "color", "color"),
    opts::flag(Some(b'i'), "ignore-case", "ignore-case"),
];

pub fn run(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let args = reffmt::lastarg_default(args);
    let p = opts::parse(SPECS, &args, 0, usage)?;
    let repo = git.repo()?;

    // Modo exclusivo: -l, -d ou -v (o último a aparecer reclama do anterior).
    let mut cmdmode: Option<(&'static str, &'static str)> = None;
    for h in &p.hits {
        if h.negated {
            continue;
        }
        let (id, shown) = match h.id {
            "list" => ("list", "-l"),
            "delete" => ("delete", "-d"),
            "verify" => ("verify", "-v"),
            _ => continue,
        };
        match cmdmode {
            Some((prev_id, prev_shown)) if prev_id != id => {
                return Err(opts::error_only(&format!("options '{shown}' and '{prev_shown}' cannot be used together")));
            }
            _ => cmdmode = Some((id, shown)),
        }
    }
    let lines: Option<usize> = if p.present("n") {
        match p.value("n") {
            None => Some(1),
            Some(v) => match os::lossy(v).parse::<usize>() {
                Ok(n) => Some(n),
                Err(_) => return Err(opts::error_only("switch `n' expects a numerical value")),
            },
        }
    } else {
        None
    };
    let filtering = ["contains", "no-contains", "merged", "no-merged", "points-at"].iter().any(|id| p.present(id));
    let mode = match cmdmode {
        Some((id, _)) => id,
        None => {
            if p.args.is_empty() || lines.is_some() || filtering {
                "list"
            } else {
                "create"
            }
        }
    };
    match mode {
        "list" => list(repo, &p, lines),
        "delete" => delete(repo, &p),
        "verify" => verify(repo, &p),
        _ => create(git, &p),
    }
}

fn list(repo: &Repo, p: &opts::Parsed, lines: Option<usize>) -> R<i32> {
    let usage = crate::usage::of("tag");
    let mut filter = Filter { kinds: vec!["refs/tags/".to_string()], ..Filter::default() };
    filter.patterns = p.args.iter().map(|a| os::lossy(a)).collect();
    reffmt::apply_filter_opts(repo, p, &mut filter)?;
    let mut keys = Vec::new();
    let sorts = p.values("sort");
    if sorts.is_empty() {
        for v in repo.config.get_all("tag.sort").into_iter().flatten() {
            keys.push(reffmt::parse_sort_key(&os::lossy(v))?);
        }
    } else {
        for s in sorts {
            keys.push(reffmt::parse_sort_key(&os::lossy(&s))?);
        }
    }
    let fmt: Vec<u8> = match p.value("format") {
        Some(f) => f.to_vec(),
        None => match lines {
            Some(n) => format!("%(align:15)%(refname:strip=2)%(end) %(contents:lines={n})").into_bytes(),
            None => b"%(refname:strip=2)".to_vec(),
        },
    };
    let nodes = reffmt::parse_format(&fmt, usage)?;
    let mut ctx = Ctx::new(repo)?;
    ctx.color = super::for_each_ref::color_enabled(p)?;
    let rows = reffmt::collect(&mut ctx, &filter)?;
    let rows = reffmt::sort_rows(&mut ctx, rows, &keys, filter.ignore_case)?;
    let omit_empty = p.has("omit-empty");
    let mut out: Vec<u8> = Vec::new();
    for row in &rows {
        let mut line = Vec::new();
        ctx.render(&nodes, row, &mut line)?;
        if omit_empty && line.is_empty() {
            continue;
        }
        out.extend_from_slice(&line);
        out.push(b'\n');
    }
    os::out(&out);
    Ok(0)
}

fn delete(repo: &Repo, p: &opts::Parsed) -> R<i32> {
    let mut code = 0;
    for a in &p.args {
        let name = os::lossy(a);
        let full = format!("refs/tags/{name}");
        match repo.ref_oid(&full)? {
            Some(oid) => {
                let shown = repo.abbrev_default(&oid);
                repo.delete_ref(&full, None)?;
                os::outs(&format!("Deleted tag '{name}' (was {shown})\n"));
            }
            None => {
                error(&format!("tag '{name}' not found."));
                code = 1;
            }
        }
    }
    Ok(code)
}

fn verify(repo: &Repo, p: &opts::Parsed) -> R<i32> {
    let mut code = 0;
    for a in &p.args {
        let name = os::lossy(a);
        let full = format!("refs/tags/{name}");
        let Some(oid) = repo.ref_oid(&full)? else {
            error(&format!("tag '{name}' not found."));
            code = 1;
            continue;
        };
        let (kind, data) = repo.read_object(&oid)?;
        if kind != Kind::Tag {
            error(&format!("{name}: cannot verify a non-tag object of type {}.", kind.name()));
            code = 1;
            continue;
        }
        os::out(&data);
        error("no signature found");
        code = 1;
    }
    Ok(code)
}

fn create(git: &Git, p: &opts::Parsed) -> R<i32> {
    let repo = git.repo()?;
    let sign = p.has("sign") || p.value("local-user").is_some();
    let messages = p.values("message");
    let file = p.value("file").map(|f| f.to_vec());
    let edit = p.has("edit");
    let annotate = p.has("annotate") || sign || !messages.is_empty() || file.is_some() || edit;
    if p.args.is_empty() {
        opts::usage_to_stderr(git.usage());
        return Err(Fail::Exit(129));
    }
    if p.args.len() > 2 {
        return Err(Fail::Fatal("too many arguments".into()));
    }
    let name = os::lossy(&p.args[0]);
    let object_ref: &[u8] = p.args.get(1).map(|v| v.as_slice()).unwrap_or(b"HEAD");
    let force = p.has("force");
    let Some(target) = repo.rev_parse(object_ref)? else {
        return Err(Fail::Fatal(format!("Failed to resolve '{}' as a valid ref.", os::lossy(object_ref))));
    };
    let full = format!("refs/tags/{name}");
    if !refs::check_refname_format(&full, false, false) {
        return Err(Fail::Fatal(format!("'{name}' is not a valid tag name.")));
    }
    let prev = repo.ref_oid(&full)?;
    if prev.is_some() && !force {
        return Err(Fail::Fatal(format!("tag '{name}' already exists")));
    }

    let new_id: Oid = if annotate {
        let mut buf: Vec<u8> = Vec::new();
        for m in &messages {
            if !buf.is_empty() {
                buf.push(b'\n');
            }
            buf.extend_from_slice(m);
            if !buf.is_empty() && !buf.ends_with(b"\n") {
                buf.push(b'\n');
            }
        }
        if let Some(f) = &file {
            buf = if f == b"-" { os::stdin_all() } else { super::plumbing::read_user_file(repo, f)? };
        }
        let given = !messages.is_empty() || file.is_some();
        let mut cleanup = if edit || !given { Cleanup::Strip } else { Cleanup::Whitespace };
        if let Some(c) = p.value_str("cleanup") {
            cleanup = Cleanup::parse(&c).ok_or_else(|| Fail::Fatal(format!("Invalid cleanup mode {c}")))?;
        }
        let editmsg = repo.path("TAG_EDITMSG");
        if edit || !given {
            if super::misc::editor(&repo.config).is_none() {
                error("Terminal is dumb, but EDITOR unset");
                os::errs("Please supply the message using either -m or -F option.\n");
                return Ok(1);
            }
            let mut text = buf.clone();
            text.extend_from_slice(format!("\n#\n# Write a message for tag:\n#   {name}\n# Lines starting with '#' will be ignored.\n").as_bytes());
            os::write(&editmsg, &text, 0o666).map_err(|e| Fail::Fatal(format!("could not write '{}': {}", os::lossy(&editmsg), e.message())))?;
            crate::editor::edit_file(&repo.config, &editmsg)?;
            buf = os::read(&editmsg).map_err(|e| Fail::Fatal(format!("could not read '{}': {}", os::lossy(&editmsg), e.message())))?;
        }
        let message = msg::cleanup(&buf, cleanup, b"#");
        if !given && message.is_empty() {
            return Err(Fail::Fatal("no tag message?".into()));
        }
        if sign {
            os::write(&editmsg, &message, 0o666).map_err(|e| Fail::Fatal(format!("could not write '{}': {}", os::lossy(&editmsg), e.message())))?;
            error("cannot run gpg: No such file or directory");
            error("gpg failed to sign the data:\n(no gpg output)");
            error("unable to sign the tag");
            os::errs(&format!("The tag message has been left in {}/TAG_EDITMSG\n", os::lossy(&repo.git_dir_display)));
            return Err(Fail::Exit(128));
        }
        let kind = repo.object_kind(&target)?.unwrap_or(Kind::Commit);
        let tagger = ident::ident(&repo.config, Who::Committer, true)?;
        let tag = Tag { object: target, kind, name: name.clone().into_bytes(), tagger: Some(tagger.to_bytes()), message };
        repo.write_object(Kind::Tag, &object::encode_tag(&tag))?
    } else {
        target
    };

    repo.update_ref(&full, new_id, None, "", true)?;
    if let Some(old) = prev
        && force
        && old != new_id
    {
        os::outs(&format!("Updated tag '{name}' (was {})\n", repo.abbrev_default(&old)));
    }
    Ok(0)
}
