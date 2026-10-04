//! `git rev-parse`: revisões, intervalos e as perguntas sobre o repositório (`--git-dir`,
//! `--show-toplevel`, `--is-inside-work-tree`...).

use super::Git;
use crate::config::sq_quote;
use crate::error::{Fail, R};
use crate::hash::Oid;
use crate::os;
use crate::repo::Repo;
use crate::rev;

fn need_repo(git: &Git) -> R<&Repo> {
    git.repo()
}

/// Caminho relativo de `target` (absoluto) visto do cwd original.
fn relative_from(cwd: &[u8], target: &[u8]) -> Vec<u8> {
    let a: Vec<&[u8]> = cwd.split(|c| *c == b'/').filter(|c| !c.is_empty()).collect();
    let b: Vec<&[u8]> = target.split(|c| *c == b'/').filter(|c| !c.is_empty()).collect();
    let common = a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count();
    let mut parts: Vec<Vec<u8>> = Vec::new();
    for _ in common..a.len() {
        parts.push(b"..".to_vec());
    }
    for c in &b[common..] {
        parts.push(c.to_vec());
    }
    if parts.is_empty() {
        return b".".to_vec();
    }
    parts.join(&b'/')
}

pub fn run(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let mut out: Vec<u8> = Vec::new();
    let mut verify = false;
    let mut quiet = false;
    let mut short: Option<usize> = None;
    let mut abbrev_ref: Option<String> = None;
    let mut symbolic = false;
    let mut symbolic_full = false;
    let mut not = false;
    let mut revs_only = false;
    let mut no_revs = false;
    let mut flags_only = false;
    let mut no_flags = false;
    let mut sq = false;
    let mut default: Option<Vec<u8>> = None;
    let mut seen_dashdash = false;
    let mut verified: Vec<Oid> = Vec::new();
    let mut absolute_paths = false;
    let mut i = 0;
    // Saída de um id de acordo com as opções.
    let mut push_line = |out: &mut Vec<u8>, s: &[u8]| {
        out.extend_from_slice(s);
        out.push(b'\n');
    };
    let cwd_orig = match &git.repo {
        Some(r) => match &r.work_tree {
            Some(wt) => os::join(wt, &r.prefix),
            None => os::getcwd().unwrap_or_default(),
        },
        None => os::getcwd().unwrap_or_default(),
    };
    let cwd_orig = cwd_orig.strip_suffix(b"/").map(|s| s.to_vec()).unwrap_or(cwd_orig);
    while i < args.len() {
        let a = args[i].clone();
        i += 1;
        if seen_dashdash {
            if !no_revs && !revs_only {
                push_line(&mut out, &a);
            }
            continue;
        }
        let s = os::lossy(&a);
        if s == "--" {
            seen_dashdash = true;
            if !revs_only && !no_flags {
                push_line(&mut out, b"--");
            }
            continue;
        }
        if s.starts_with('-') && s.len() > 1 {
            match s.as_str() {
                "--verify" => verify = true,
                "-q" | "--quiet" => quiet = true,
                // git-rev-parse(1): `--short` é o `--verify` com o nome abreviado.
                "--short" => {
                    short = Some(0);
                    verify = true;
                }
                "--symbolic" => symbolic = true,
                "--symbolic-full-name" => symbolic_full = true,
                "--abbrev-ref" => abbrev_ref = Some("loose".into()),
                "--not" => not = !not,
                "--revs-only" => revs_only = true,
                "--no-revs" => no_revs = true,
                "--flags" => flags_only = true,
                "--no-flags" => no_flags = true,
                "--sq" => sq = true,
                "--default" => {
                    default = args.get(i).cloned();
                    i += 1;
                }
                "--git-dir" => {
                    let r = need_repo(git)?;
                    if absolute_paths {
                        push_line(&mut out, &r.git_dir);
                    } else {
                        let d = &r.git_dir_display;
                        let shown = if d.starts_with(b"/") || d == b"." || d == b".git" { d.clone() } else { d.clone() };
                        push_line(&mut out, &shown);
                    }
                }
                "--absolute-git-dir" => {
                    let r = need_repo(git)?;
                    push_line(&mut out, &r.git_dir);
                }
                "--git-common-dir" => {
                    let r = need_repo(git)?;
                    if r.common_dir == r.git_dir {
                        let shown = if r.prefix.is_empty() && r.work_tree.is_some() { r.git_dir_display.clone() } else { relative_from(&cwd_orig, &r.common_dir) };
                        push_line(&mut out, &shown);
                    } else {
                        push_line(&mut out, &relative_from(&cwd_orig, &r.common_dir));
                    }
                }
                "--show-toplevel" => {
                    let r = need_repo(git)?;
                    match &r.work_tree {
                        Some(w) => push_line(&mut out, w),
                        None => return Err(Fail::Fatal("this operation must be run in a work tree".into())),
                    }
                }
                "--show-prefix" => {
                    let r = need_repo(git)?;
                    push_line(&mut out, &r.prefix);
                }
                "--show-cdup" => {
                    let r = need_repo(git)?;
                    let n = r.prefix.iter().filter(|c| **c == b'/').count();
                    push_line(&mut out, "../".repeat(n).as_bytes());
                }
                "--is-inside-work-tree" => {
                    let inside = git.repo.as_ref().is_some_and(|r| r.work_tree.is_some());
                    if git.repo.is_none() {
                        need_repo(git)?;
                    }
                    push_line(&mut out, if inside { b"true" } else { b"false" });
                }
                "--is-inside-git-dir" => {
                    let r = need_repo(git)?;
                    push_line(&mut out, if r.inside_git_dir { b"true" } else { b"false" });
                }
                "--is-bare-repository" => {
                    let r = need_repo(git)?;
                    push_line(&mut out, if r.bare { b"true" } else { b"false" });
                }
                "--is-shallow-repository" => {
                    let r = need_repo(git)?;
                    let shallow = os::exists(&r.path("shallow"));
                    push_line(&mut out, if shallow { b"true" } else { b"false" });
                }
                "--show-superproject-working-tree" => {}
                "--show-object-format" | "--show-object-format=storage" | "--show-object-format=input" | "--show-object-format=output" => push_line(&mut out, b"sha1"),
                "--show-ref-format" => push_line(&mut out, b"files"),
                "--local-env-vars" => {
                    for v in [
                        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
                        "GIT_CONFIG",
                        "GIT_CONFIG_PARAMETERS",
                        "GIT_CONFIG_COUNT",
                        "GIT_OBJECT_DIRECTORY",
                        "GIT_DIR",
                        "GIT_WORK_TREE",
                        "GIT_IMPLICIT_WORK_TREE",
                        "GIT_GRAFT_FILE",
                        "GIT_INDEX_FILE",
                        "GIT_NO_REPLACE_OBJECTS",
                        "GIT_REPLACE_REF_BASE",
                        "GIT_PREFIX",
                        "GIT_SHALLOW_FILE",
                        "GIT_COMMON_DIR",
                    ] {
                        push_line(&mut out, v.as_bytes());
                    }
                }
                "--sq-quote" => {
                    let mut line = Vec::new();
                    for w in &args[i..] {
                        line.push(b' ');
                        line.extend_from_slice(&sq_quote(w));
                    }
                    push_line(&mut out, &line);
                    os::out(&out);
                    return Ok(0);
                }
                "--all" | "--branches" | "--tags" | "--remotes" => {
                    let r = need_repo(git)?;
                    let prefix = match s.as_str() {
                        "--all" => "refs/",
                        "--branches" => "refs/heads/",
                        "--tags" => "refs/tags/",
                        _ => "refs/remotes/",
                    };
                    for (name, id) in r.list_refs(prefix)? {
                        if s == "--all" && name.starts_with("refs/stash") {
                            // O --all inclui o stash, como o git.
                        }
                        let shown = if symbolic || symbolic_full { name.into_bytes() } else { id.hex().into_bytes() };
                        push_line(&mut out, &shown);
                    }
                    if s == "--all"
                        && let Some(h) = r.head_oid()?
                        && r.current_branch()?.is_none()
                    {
                        let _ = h;
                    }
                }
                "--path-format=absolute" => absolute_paths = true,
                "--path-format=relative" => absolute_paths = false,
                "--git-path" => {
                    let r = need_repo(git)?;
                    let rel = args.get(i).cloned().unwrap_or_default();
                    i += 1;
                    let shared = !(rel.starts_with(b"HEAD") || rel.starts_with(b"index") || rel.starts_with(b"logs/HEAD") || rel == b"ORIG_HEAD");
                    let base = if shared { r.common_dir.clone() } else { r.git_dir.clone() };
                    let full = os::join(&base, &rel);
                    let shown = if absolute_paths { full } else { relative_from(&cwd_orig, &full) };
                    push_line(&mut out, &shown);
                }
                "--resolve-git-dir" => {
                    let d = args.get(i).cloned().unwrap_or_default();
                    i += 1;
                    if crate::repo::is_git_directory(&d) {
                        push_line(&mut out, &d);
                    } else {
                        return Err(Fail::Fatal(format!("not a gitdir '{}'", os::lossy(&d))));
                    }
                }
                _ => {
                    if let Some(n) = s.strip_prefix("--short=") {
                        short = Some(n.parse().unwrap_or(7));
                        verify = true;
                    } else if let Some(m) = s.strip_prefix("--abbrev-ref=") {
                        abbrev_ref = Some(m.to_string());
                    } else if let Some(d) = s.strip_prefix("--default=") {
                        default = Some(d.as_bytes().to_vec());
                    } else if let Some(t) = s.strip_prefix("--since=").or_else(|| s.strip_prefix("--after=")) {
                        let t = crate::date::approxidate(t).unwrap_or(0);
                        push_line(&mut out, format!("--max-age={t}").as_bytes());
                    } else if let Some(t) = s.strip_prefix("--until=").or_else(|| s.strip_prefix("--before=")) {
                        let t = crate::date::approxidate(t).unwrap_or(0);
                        push_line(&mut out, format!("--min-age={t}").as_bytes());
                    } else if let Some(pat) = s.strip_prefix("--branches=").or_else(|| s.strip_prefix("--tags=")).or_else(|| s.strip_prefix("--glob=")) {
                        let r = need_repo(git)?;
                        let base = if s.starts_with("--branches=") {
                            "refs/heads/"
                        } else if s.starts_with("--tags=") {
                            "refs/tags/"
                        } else {
                            ""
                        };
                        let full = format!("{base}{pat}");
                        for (name, id) in r.list_refs("refs/")? {
                            if crate::wildmatch::wildmatch(full.as_bytes(), name.as_bytes(), crate::wildmatch::PATHNAME) || name.starts_with(&format!("{full}/")) {
                                push_line(&mut out, id.hex().as_bytes());
                            }
                        }
                    } else if !no_flags && !revs_only {
                        // Flags desconhecidas passam adiante (rev-parse é usado por scripts).
                        push_line(&mut out, &a);
                    }
                }
            }
            continue;
        }
        if flags_only {
            continue;
        }
        // Revisão (ou caminho).
        let r = need_repo(git)?;
        let handled = rev_arg(r, &a, &mut out, short, &abbrev_ref, symbolic, symbolic_full, not, verify, &mut verified)?;
        if handled {
            continue;
        }
        if verify {
            if quiet {
                return Ok(1);
            }
            return Err(Fail::Fatal("Needed a single revision".into()));
        }
        // Não é revisão: caminho que existe passa como está; senão é erro.
        let path_exists = r.work_tree.is_some() && r.rel_from_prefix(&a).is_some_and(|p| os::exists(if p.is_empty() { b"." } else { &p }));
        if !path_exists {
            push_line(&mut out, &a);
            os::out(&out);
            return Err(rev::bad_revision(&a));
        }
        if !revs_only {
            push_line(&mut out, &a);
        }
        // Depois do primeiro caminho, o resto é caminho também.
        seen_dashdash = true;
    }
    if verify {
        if verified.len() != 1 {
            if let Some(d) = &default
                && verified.is_empty()
            {
                let r = need_repo(git)?;
                if rev_arg(r, d, &mut out, short, &abbrev_ref, symbolic, symbolic_full, not, true, &mut verified)? {
                    os::out(&out);
                    return Ok(0);
                }
            }
            if quiet {
                return Ok(1);
            }
            return Err(Fail::Fatal("Needed a single revision".into()));
        }
    } else if let Some(d) = default
        && verified.is_empty()
    {
        let r = need_repo(git)?;
        rev_arg(r, &d, &mut out, short, &abbrev_ref, symbolic, symbolic_full, not, false, &mut verified)?;
    }
    let _ = sq;
    os::out(&out);
    Ok(0)
}

/// Uma revisão (ou intervalo). Devolve se reconheceu.
#[allow(clippy::too_many_arguments)]
fn rev_arg(
    r: &Repo,
    a: &[u8],
    out: &mut Vec<u8>,
    short: Option<usize>,
    abbrev_ref: &Option<String>,
    symbolic: bool,
    symbolic_full: bool,
    not: bool,
    verify: bool,
    verified: &mut Vec<Oid>,
) -> R<bool> {
    let fmt = |id: &Oid| -> Vec<u8> {
        match short {
            Some(0) => r.abbrev_default(id).into_bytes(),
            Some(n) => r.abbrev(id, n.max(4)).into_bytes(),
            None => id.hex().into_bytes(),
        }
    };
    let line = |out: &mut Vec<u8>, neg: bool, body: &[u8]| {
        if neg {
            out.push(b'^');
        }
        out.extend_from_slice(body);
        out.push(b'\n');
    };
    if !verify {
        // `A...B` e `A..B`.
        if let Some(pos) = find(a, b"...") {
            let (l, rr) = (&a[..pos], &a[pos + 3..]);
            let l = if l.is_empty() { b"HEAD".as_slice() } else { l };
            let rr = if rr.is_empty() { b"HEAD".as_slice() } else { rr };
            if let (Some(x), Some(y)) = (r.rev_parse_commit(l)?, r.rev_parse_commit(rr)?) {
                line(out, false, &fmt(&y));
                line(out, false, &fmt(&x));
                let mut g = crate::graph::Graph::new(r);
                for b in g.merge_bases(&x, &[y])? {
                    line(out, true, &fmt(&b));
                }
                return Ok(true);
            }
        } else if let Some(pos) = find(a, b"..") {
            let (l, rr) = (&a[..pos], &a[pos + 2..]);
            let l = if l.is_empty() { b"HEAD".as_slice() } else { l };
            let rr = if rr.is_empty() { b"HEAD".as_slice() } else { rr };
            if let (Some(x), Some(y)) = (r.rev_parse(l)?, r.rev_parse(rr)?) {
                line(out, false, &fmt(&y));
                line(out, true, &fmt(&x));
                return Ok(true);
            }
        }
        if let Some(rest) = a.strip_prefix(b"^")
            && !rest.is_empty()
            && let Some(id) = r.rev_parse(rest)?
        {
            line(out, !not, &fmt(&id));
            return Ok(true);
        }
        // `A^@` (pais) e `A^!` (A sem os pais).
        if let Some(base) = a.strip_suffix(b"^@") {
            if let Some(c) = r.rev_parse_commit(base)? {
                for p in r.parents(&c)? {
                    line(out, not, &fmt(&p));
                }
                return Ok(true);
            }
        }
        if let Some(base) = a.strip_suffix(b"^!") {
            if let Some(c) = r.rev_parse_commit(base)? {
                line(out, not, &fmt(&c));
                for p in r.parents(&c)? {
                    line(out, !not, &fmt(&p));
                }
                return Ok(true);
            }
        }
    }
    let Some(id) = r.rev_parse(a)? else { return Ok(false) };
    if verify && !r.odb.exists(&id) {
        return Ok(false);
    }
    verified.push(id);
    if let Some(mode) = abbrev_ref {
        let name = os::lossy(a);
        match r.dwim_ref_name(&name)? {
            Some(full) => {
                let full = match r.resolve_ref(&full)? {
                    Some((f, _)) => f,
                    None => full,
                };
                let shown = if full == "HEAD" { "HEAD".to_string() } else { super::plumbing::shorten_ref(r, &full) };
                let _ = mode;
                line(out, not, shown.as_bytes());
            }
            None => line(out, not, a),
        }
        return Ok(true);
    }
    if symbolic_full {
        let name = os::lossy(a);
        match r.dwim_ref_name(&name)? {
            Some(full) => {
                let full = match r.resolve_ref(&full)? {
                    Some((f, _)) => f,
                    None => full,
                };
                line(out, not, full.as_bytes());
            }
            None => {}
        }
        return Ok(true);
    }
    if symbolic {
        line(out, not, a);
        return Ok(true);
    }
    line(out, not, &fmt(&id));
    Ok(true)
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}
