//! `git merge`: avanço rápido, mesclagem de três vias pela estratégia `ort` (a de `recursive` e a
//! `ours` também), `--squash`, `--no-commit`, `--abort`, `--quit`, `--continue`, a mensagem do commit
//! de mesclagem (`Merge branch 'x' into y`, tags, `--log`) e o estado em `.git` (`MERGE_HEAD`,
//! `MERGE_MSG`, `MERGE_MODE`, `ORIG_HEAD`, `AUTO_MERGE`, `SQUASH_MSG`). Não faz a mesclagem de várias
//! cabeças (octopus) nem o `--autostash`.

use std::collections::BTreeSet;

use super::Git;
use crate::cmd::unpack::{self, Opts as UnpackOpts};
use crate::config;
use crate::date::{self, DateMode};
use crate::diff::rename::RenameOpts;
use crate::diff::{self, DiffOpts, WtState};
use crate::editor;
use crate::error::{Fail, R, error, hint};
use crate::graph::{DateQueue, Graph};
use crate::hash::{EMPTY_TREE, Kind, Oid};
use crate::ident::{self, Who};
use crate::index::Index;
use crate::merge::{self, Opts as MergeOpts};
use crate::msg::{self, Cleanup};
use crate::object::{self, Commit};
use crate::opts::{self, Spec};
use crate::os;
use crate::pathspec::Pathspec;
use crate::repo::Repo;
use crate::wildmatch;
use crate::worktree;
use crate::xmerge::{Favor, Style};

const SPECS: &[Spec] = &[
    opts::short_flag(b'n', "no-diffstat"),
    opts::flag(None, "stat", "stat"),
    opts::flag(None, "summary", "summary"),
    opts::optional(None, "log", "log"),
    opts::flag(None, "squash", "squash"),
    opts::flag(None, "commit", "commit"),
    opts::flag(Some(b'e'), "edit", "edit"),
    opts::value(None, "cleanup", "cleanup"),
    opts::flag(None, "ff", "ff"),
    opts::noneg(opts::flag(None, "ff-only", "ff-only")),
    opts::flag(None, "rerere-autoupdate", "rerere-autoupdate"),
    opts::flag(None, "verify-signatures", "verify-signatures"),
    opts::value(Some(b's'), "strategy", "strategy"),
    opts::value(Some(b'X'), "strategy-option", "strategy-option"),
    opts::value(Some(b'm'), "message", "message"),
    opts::noneg(opts::value(Some(b'F'), "file", "file")),
    opts::value(None, "into-name", "into-name"),
    opts::flag(Some(b'v'), "verbose", "verbose"),
    opts::flag(Some(b'q'), "quiet", "quiet"),
    opts::flag(None, "abort", "abort"),
    opts::flag(None, "quit", "quit"),
    opts::flag(None, "continue", "continue"),
    opts::flag(None, "allow-unrelated-histories", "allow-unrelated-histories"),
    opts::flag(None, "progress", "progress"),
    opts::optional(Some(b'S'), "gpg-sign", "gpg-sign"),
    opts::flag(None, "autostash", "autostash"),
    opts::flag(None, "overwrite-ignore", "overwrite-ignore"),
    opts::flag(None, "signoff", "signoff"),
    opts::flag(None, "no-verify", "no-verify"),
];

/// Quantas entradas do `--log` quando o valor não vem.
const DEFAULT_MERGE_LOG_LEN: i64 = 20;

#[derive(Copy, Clone, PartialEq, Eq)]
enum Ff {
    No,
    Allow,
    Only,
}

/// As opções e a configuração do `git merge`.
struct Mo {
    show_diffstat: bool,
    shortlog_len: i64,
    squash: bool,
    /// -1 = não escolhido.
    option_commit: i32,
    option_edit: i32,
    ff: Ff,
    have_message: bool,
    merge_msg: Vec<u8>,
    verbosity: i32,
    cleanup_arg: Option<String>,
    strategies: Vec<String>,
    xopts: Vec<String>,
    into_name: Option<String>,
    allow_unrelated: bool,
    signoff: bool,
    no_verify: bool,
    autostash: bool,
}

fn usage_msg_opt(usage: &str, msg: &str) -> Fail {
    os::err_line("fatal: ", msg);
    os::errs("\n");
    opts::usage_to_stderr(usage);
    Fail::Exit(129)
}

fn parse_state(repo: &Repo, p: &opts::Parsed, usage: &str) -> R<Mo> {
    let cfg = &repo.config;
    let mut m = Mo {
        show_diffstat: true,
        shortlog_len: -1,
        squash: false,
        option_commit: -1,
        option_edit: -1,
        ff: Ff::Allow,
        have_message: false,
        merge_msg: Vec::new(),
        verbosity: 0,
        cleanup_arg: cfg.get("commit.cleanup"),
        strategies: Vec::new(),
        xopts: Vec::new(),
        into_name: None,
        allow_unrelated: false,
        signoff: false,
        no_verify: false,
        autostash: cfg.get_bool("merge.autostash")?.unwrap_or(false),
    };
    if let Some(v) = cfg.get_bool("merge.stat")? {
        m.show_diffstat = v;
    }
    if let Some(v) = cfg.get_bool("merge.diffstat")? {
        m.show_diffstat = v;
    }
    let mut log_cfg: i64 = 0;
    for key in ["merge.log", "merge.summary"] {
        if let Some(Some(v)) = cfg.raw(key) {
            match config::parse_bool(Some(v)) {
                Some(b) => log_cfg = if b { DEFAULT_MERGE_LOG_LEN } else { 0 },
                None => match config::parse_int(v) {
                    Some(n) if n >= 0 => log_cfg = n,
                    _ => return Err(Fail::Fatal(format!("{key}: negative length {}", os::lossy(v)))),
                },
            }
        }
    }
    if let Some(v) = cfg.get("merge.ff") {
        match config::parse_bool(Some(v.as_bytes())) {
            Some(true) => m.ff = Ff::Allow,
            Some(false) => m.ff = Ff::No,
            None => {
                if v == "only" {
                    m.ff = Ff::Only;
                }
            }
        }
    }
    for h in &p.hits {
        match h.id {
            "no-diffstat" => m.show_diffstat = false,
            "stat" | "summary" => m.show_diffstat = !h.negated,
            "log" => {
                m.shortlog_len = if h.negated {
                    0
                } else {
                    match &h.value {
                        None => DEFAULT_MERGE_LOG_LEN,
                        Some(v) => match std::str::from_utf8(v).ok().and_then(|s| s.parse::<i64>().ok()) {
                            Some(n) => n,
                            None => return Err(opts::usage_error(usage, "option `log' expects a numerical value")),
                        },
                    }
                };
            }
            "squash" => m.squash = !h.negated,
            "commit" => m.option_commit = i32::from(!h.negated),
            "edit" => m.option_edit = i32::from(!h.negated),
            "cleanup" => m.cleanup_arg = if h.negated { None } else { h.value.as_deref().map(os::lossy) },
            "ff" => m.ff = if h.negated { Ff::No } else { Ff::Allow },
            "ff-only" => m.ff = Ff::Only,
            "strategy" => {
                if h.negated {
                    m.strategies.clear();
                } else if let Some(v) = &h.value {
                    let name = os::lossy(v);
                    if !matches!(name.as_str(), "ort" | "recursive" | "ours" | "octopus" | "resolve" | "subtree") {
                        os::errs(&format!("Could not find merge strategy '{name}'.\n"));
                        os::errs("Available strategies are: octopus ours recursive resolve subtree.\n");
                        return Err(Fail::Exit(1));
                    }
                    m.strategies.push(name);
                }
            }
            "strategy-option" => {
                if h.negated {
                    m.xopts.clear();
                } else if let Some(v) = &h.value {
                    m.xopts.push(os::lossy(v));
                }
            }
            "message" => {
                if h.negated {
                    m.merge_msg.clear();
                } else if let Some(v) = &h.value {
                    if !m.merge_msg.is_empty() {
                        m.merge_msg.extend_from_slice(b"\n\n");
                    }
                    m.merge_msg.extend_from_slice(v);
                    m.have_message = true;
                }
            }
            "file" => {
                if let Some(v) = &h.value {
                    if !m.merge_msg.is_empty() {
                        m.merge_msg.push(b'\n');
                    }
                    let full = if v.starts_with(b"/") || repo.prefix.is_empty() { v.clone() } else { os::join(&repo.prefix, v) };
                    match os::read(&full) {
                        Ok(d) => m.merge_msg.extend_from_slice(&d),
                        Err(_) => {
                            error(&format!("could not read file '{}'", os::lossy(v)));
                            return Err(Fail::Exit(129));
                        }
                    }
                    m.have_message = true;
                }
            }
            "into-name" => m.into_name = if h.negated { None } else { h.value.as_deref().map(os::lossy) },
            "verbose" => m.verbosity = if h.negated { 0 } else { m.verbosity + 1 },
            "quiet" => m.verbosity = if h.negated { 0 } else { m.verbosity - 1 },
            "allow-unrelated-histories" => m.allow_unrelated = !h.negated,
            "autostash" => m.autostash = !h.negated,
            "signoff" => m.signoff = !h.negated,
            "no-verify" => m.no_verify = !h.negated,
            _ => {}
        }
    }
    if m.shortlog_len < 0 {
        m.shortlog_len = log_cfg;
    }
    Ok(m)
}

// ---- estado em .git ---------------------------------------------------------------------------

/// `remove_merge_branch_state`: o que a mesclagem em andamento deixa em `.git`.
fn remove_merge_branch_state(repo: &Repo) {
    for f in ["MERGE_HEAD", "MERGE_RR", "MERGE_MSG", "MERGE_MODE", "AUTO_MERGE"] {
        let _ = os::unlink(&repo.path(f));
    }
}

fn write_merge_heads(repo: &Repo, heads: &[Oid], ff: Ff) -> R<()> {
    let mut buf = String::new();
    for h in heads {
        buf.push_str(&format!("{h}\n"));
    }
    os::write(&repo.path("MERGE_HEAD"), buf.as_bytes(), 0o666).map_err(|e| Fail::Fatal(format!("could not write '{}': {}", os::lossy(&repo.path("MERGE_HEAD")), e.message())))?;
    let mode: &[u8] = if ff == Ff::No { b"no-ff" } else { b"" };
    os::write(&repo.path("MERGE_MODE"), mode, 0o666).map_err(|e| Fail::Fatal(format!("could not write '{}': {}", os::lossy(&repo.path("MERGE_MODE")), e.message())))
}

fn write_merge_state(repo: &Repo, mo: &Mo, heads: &[Oid]) -> R<()> {
    write_merge_heads(repo, heads, mo.ff)?;
    let mut text = mo.merge_msg.clone();
    text.push(b'\n');
    os::write(&repo.path("MERGE_MSG"), &text, 0o666).map_err(|e| Fail::Fatal(format!("could not write '{}': {}", os::lossy(&repo.path("MERGE_MSG")), e.message())))
}

fn abort_commit(repo: &Repo, mo: &Mo, heads: &[Oid], err_msg: Option<&str>) -> Fail {
    if let Some(m) = err_msg {
        error(m);
    }
    os::errs("Not committing merge; use 'git commit' to complete the merge.\n");
    match write_merge_state(repo, mo, heads) {
        Ok(()) => Fail::Exit(1),
        Err(f) => f,
    }
}

fn hooks_dir(repo: &Repo) -> Vec<u8> {
    match repo.config.get_bytes("core.hookspath") {
        Some(p) if p.starts_with(b"/") => p,
        Some(p) => os::absolute(&p),
        None => repo.common("hooks"),
    }
}

/// O que o `get_cleanup_mode` escolhe: `default` vira `strip` quando há editor e `whitespace`
/// quando não.
fn cleanup_mode(arg: Option<&str>, use_editor: bool) -> R<Cleanup> {
    match arg {
        None | Some("default") => Ok(if use_editor { Cleanup::Strip } else { Cleanup::Whitespace }),
        Some(other) => Cleanup::parse(other).ok_or_else(|| Fail::Fatal(format!("Invalid cleanup mode {other}"))),
    }
}

// ---- a cabeça mesclada e a mensagem ------------------------------------------------------------

/// O que o usuário pediu pra mesclar.
struct Remote {
    /// Como foi escrito: vai nos marcadores de conflito e no reflog.
    name: String,
    /// O objeto antes de descascar (o id da tag anotada): vai no `MERGE_HEAD`.
    obj: Oid,
    commit: Oid,
}

/// `@{-N}` e `-` viram o nome do ramo anterior (o `strbuf_branchname`).
fn expand_branchname(repo: &Repo, name: &str) -> R<String> {
    let n = name.strip_prefix("@{-").and_then(|r| r.strip_suffix('}')).and_then(|n| n.parse::<usize>().ok());
    match n {
        Some(n) => Ok(repo.nth_prior_branch(n)?.unwrap_or_else(|| name.to_string())),
        None => Ok(name.to_string()),
    }
}

fn get_merge_parent(repo: &Repo, name: &str) -> R<Option<Remote>> {
    let Some(obj) = repo.rev_parse(name.as_bytes())? else { return Ok(None) };
    let Some(commit) = repo.peel_to_commit(&obj)? else { return Ok(None) };
    Ok(Some(Remote { name: name.to_string(), obj, commit }))
}

/// O `help_unknown_ref`: o erro e, se houver, os ramos de acompanhamento remoto de mesmo nome.
fn help_unknown_ref(repo: &Repo, name: &str, cmd: &str, why: &str) -> R<Fail> {
    let mut msg = format!("{cmd}: {name} - {why}\n");
    let mut similar: Vec<String> = Vec::new();
    for (full, _) in repo.list_refs("refs/remotes/")? {
        if full.rsplit('/').next() == Some(name) {
            similar.push(super::plumbing::shorten_ref(repo, &full));
        }
    }
    if !similar.is_empty() {
        msg.push_str(if similar.len() == 1 { "\nDid you mean this?\n" } else { "\nDid you mean one of these?\n" });
        for s in &similar {
            msg.push_str(&format!("\t{s}\n"));
        }
    }
    os::errs(&msg);
    Ok(Fail::Exit(1))
}

/// A linha do `merge_name`: de onde vem a cabeça (`branch 'x' of .`, `tag 'v1' of .`, `commit 'abc'`).
fn merge_name(repo: &Repo, arg: &str, r: &Remote) -> R<String> {
    let remote = expand_branchname(repo, arg)?;
    if let Some((found, _)) = repo.dwim_ref(&remote)? {
        if found.starts_with("refs/heads/") {
            return Ok(format!("branch '{remote}' of ."));
        }
        if found.starts_with("refs/tags/") {
            return Ok(format!("tag '{remote}' of ."));
        }
        if found.starts_with("refs/remotes/") {
            return Ok(format!("remote-tracking branch '{remote}' of ."));
        }
    }
    // `nome^^^` ou `nome~N`: o começo do ramo `nome`.
    let mut len = 0usize;
    let mut early = false;
    let bytes = remote.as_bytes();
    let mut end = bytes.len();
    while end > 0 && bytes[end - 1] == b'^' {
        len += 1;
        end -= 1;
    }
    if len > 0 {
        early = true;
    } else if let Some(pos) = remote.rfind('~') {
        let mut seen_nonzero = false;
        let mut count = 1usize;
        let mut rest_ok = true;
        for c in remote[pos + 1..].bytes() {
            if c.is_ascii_digit() {
                seen_nonzero |= c != b'0';
                count += 1;
            } else {
                rest_ok = false;
                break;
            }
        }
        if rest_ok {
            len = count;
            if seen_nonzero || len == 1 {
                early = true;
            }
        }
    }
    if len > 0 && len <= remote.len() {
        let base = &remote[..remote.len() - len];
        if repo.ref_oid(&format!("refs/heads/{base}"))?.is_some() {
            return Ok(format!("branch '{base}'{} of .", if early { " (early part)" } else { "" }));
        }
    }
    if repo.object_kind(&r.obj)? == Some(Kind::Tag) {
        return Ok(format!("tag '{remote}'"));
    }
    Ok(format!("commit '{remote}'"))
}

fn dest_suppressed(repo: &Repo, dest: &str) -> bool {
    let pats = repo.config.get_all("merge.suppressdest");
    if pats.is_empty() {
        return ["main", "master"].iter().any(|p| wildmatch::wildmatch(p.as_bytes(), dest.as_bytes(), wildmatch::PATHNAME));
    }
    let mut active: Vec<Vec<u8>> = Vec::new();
    for p in pats {
        match p {
            Some([]) => active.clear(),
            Some(v) => active.push(v.to_vec()),
            None => {}
        }
    }
    active.iter().any(|p| wildmatch::wildmatch(p, dest.as_bytes(), wildmatch::PATHNAME))
}

/// As linhas do `git log` (formato médio) dos commits de `tip` que o `hidden` não alcança, do mais
/// novo pro mais antigo (a ordem por data do `rev-list`).
fn range_commits(repo: &Repo, hidden: &Oid, tip: &Oid) -> R<Vec<Oid>> {
    let mut graph = Graph::new(repo);
    let seen_hidden = graph.reachable(&[*hidden])?;
    let mut out: Vec<Oid> = Vec::new();
    let mut queue = DateQueue::new();
    let mut seen: BTreeSet<Oid> = BTreeSet::new();
    if !seen_hidden.contains(tip) {
        queue.push(graph.date(tip)?, *tip);
        seen.insert(*tip);
    }
    while let Some((_, c)) = queue.pop() {
        out.push(c);
        for p in graph.parents(&c)? {
            if seen_hidden.contains(&p) || !seen.insert(p) {
                continue;
            }
            queue.push(graph.date(&p)?, p);
        }
    }
    Ok(out)
}

/// O `fmt_merge_msg` para uma cabeça só.
fn fmt_merge_msg(repo: &Repo, mo: &Mo, line: &str, r: &Remote, head: &Oid, current_branch: &str) -> R<Vec<u8>> {
    let mut out: Vec<u8> = Vec::new();
    let (comment, src) = match line.find(" of ") {
        Some(i) => (&line[..i], Some(&line[i + 4..])),
        None => (line, None),
    };
    let is_local_origin = src == Some(".");
    if !mo.have_message {
        let mut title = String::from("Merge ");
        match src {
            None => title.push_str(line),
            Some(s) => {
                if let Some(o) = comment.strip_prefix("branch ") {
                    title.push_str(&format!("branch {o}"));
                } else if let Some(o) = comment.strip_prefix("tag ") {
                    title.push_str(&format!("tag {o}"));
                } else if let Some(o) = comment.strip_prefix("remote-tracking branch ") {
                    title.push_str(&format!("remote-tracking branch {o}"));
                } else {
                    title.push_str(&format!("commit {comment}"));
                }
                if s != "." {
                    title.push_str(&format!(" of {s}"));
                }
            }
        }
        let dest = mo.into_name.clone().unwrap_or_else(|| current_branch.to_string());
        if !dest_suppressed(repo, &dest) {
            title.push_str(&format!(" into {dest}"));
        }
        title.push('\n');
        out.extend_from_slice(title.as_bytes());
    }
    // O corpo de uma tag anotada.
    if repo.object_kind(&r.obj)? == Some(Kind::Tag) {
        let tag = repo.read_tag(&r.obj)?;
        let mut body = tag.message.clone();
        if !body.is_empty() {
            if !body.ends_with(b"\n") {
                body.push(b'\n');
            }
            out.push(b'\n');
            out.extend_from_slice(&body);
        }
    }
    if mo.shortlog_len > 0 {
        let origin: String = match src {
            Some(_) if is_local_origin => {
                let o = comment.strip_prefix("branch ").or_else(|| comment.strip_prefix("tag ")).or_else(|| comment.strip_prefix("remote-tracking branch ")).unwrap_or(comment);
                if o.len() >= 2 && o.starts_with('\'') && o.ends_with('\'') { o[1..o.len() - 1].to_string() } else { o.to_string() }
            }
            _ => line.to_string(),
        };
        let commits = range_commits(repo, head, &r.commit)?;
        let limit = mo.shortlog_len as usize;
        let mut subjects: Vec<String> = Vec::new();
        let mut count = 0usize;
        for c in &commits {
            let cm = repo.read_commit(c)?;
            if cm.parents.len() > 1 {
                continue;
            }
            count += 1;
            if subjects.len() > limit {
                continue;
            }
            let subject = object::subject_of(&cm.message);
            subjects.push(if subject.is_empty() { c.hex() } else { os::lossy(&subject) });
        }
        let mut s = if count > limit { format!("\n* {origin}: ({count} commits)\n") } else { format!("\n* {origin}:\n") };
        for (i, sub) in subjects.iter().enumerate() {
            if i >= limit {
                s.push_str("  ...\n");
            } else {
                s.push_str(&format!("  {sub}\n"));
            }
        }
        out.extend_from_slice(s.as_bytes());
    }
    Ok(out)
}

// ---- saída e mensagens do fim -----------------------------------------------------------------

/// O `git log` (formato médio) de um commit: o que o `SQUASH_MSG` leva por commit.
fn medium_format(repo: &Repo, id: &Oid) -> R<Vec<u8>> {
    let c = repo.read_commit(id)?;
    let author = c.author_ident();
    let mut out: Vec<u8> = Vec::new();
    if c.parents.len() > 1 {
        let ps: Vec<String> = c.parents.iter().map(|p| repo.abbrev_default(p)).collect();
        out.extend_from_slice(format!("Merge: {}\n", ps.join(" ")).as_bytes());
    }
    out.extend_from_slice(format!("Author: {}\n", os::lossy(&author.name_email())).as_bytes());
    if let Some(t) = author.date {
        out.extend_from_slice(format!("Date:   {}\n", date::show_date(t, author.tz, &DateMode::Normal)).as_bytes());
    }
    out.push(b'\n');
    let (subject, end) = object::subject_with(&c.message, b" ");
    out.extend_from_slice(b"    ");
    out.extend_from_slice(&subject);
    out.push(b'\n');
    let body = &c.message[object::skip_blank_lines(&c.message, end)..];
    let body = object::rtrim(body);
    if !body.is_empty() {
        out.extend_from_slice(b"    \n");
        for line in body.split(|ch| *ch == b'\n') {
            out.extend_from_slice(b"    ");
            out.extend_from_slice(object::rtrim(line));
            out.push(b'\n');
        }
    }
    Ok(out)
}

fn squash_message(repo: &Repo, head: &Oid, remote: &Remote) -> R<()> {
    os::outs("Squash commit -- not updating HEAD\n");
    let mut out: Vec<u8> = b"Squashed commit of the following:\n".to_vec();
    for c in range_commits(repo, head, &remote.commit)? {
        out.push(b'\n');
        out.extend_from_slice(format!("commit {c}\n").as_bytes());
        out.extend_from_slice(&medium_format(repo, &c)?);
    }
    let path = repo.path("SQUASH_MSG");
    os::write(&path, &out, 0o666).map_err(|e| Fail::Fatal(format!("could not write '{}': {}", os::lossy(&path), e.message())))
}

/// O diffstat e o resumo (`create mode`...) de um commit contra o outro, com renomeações.
fn show_diffstat(repo: &Repo, from: &Oid, to: &Oid) -> R<()> {
    let ft = repo.tree_of(from)?;
    let tt = repo.tree_of(to)?;
    let pairs = diff::diff_trees(repo, Some(&ft), Some(&tt), &Pathspec::default())?;
    let o = DiffOpts { stat: true, summary: true, renames: Some(RenameOpts::default()), ..DiffOpts::default() };
    let pairs = diff::postprocess(repo, pairs, &o)?;
    diff::emit(repo, &pairs, &o)?;
    Ok(())
}

/// O `finish` do git: a mensagem, o HEAD (ou a mensagem do `--squash`), o diffstat e o hook.
fn finish(repo: &Repo, mo: &Mo, head: &Oid, remote: &Remote, new_head: Option<&Oid>, msg: Option<&str>, action: &str) -> R<()> {
    let mut reflog = action.to_string();
    if let Some(m) = msg {
        if mo.verbosity >= 0 {
            os::outs(&format!("{m}\n"));
        }
        reflog = format!("{action}: {m}");
    }
    if mo.squash {
        squash_message(repo, head, remote)?;
    } else if mo.verbosity >= 0 && mo.merge_msg.is_empty() {
        os::outs("No merge message -- not updating HEAD\n");
    } else if let Some(nh) = new_head {
        repo.update_ref("HEAD", *nh, Some(Some(*head)), &reflog, false)?;
    }
    if let Some(nh) = new_head
        && mo.show_diffstat
    {
        show_diffstat(repo, head, nh)?;
    }
    let flag: &[u8] = if mo.squash { b"1" } else { b"0" };
    editor::run_hook(&hooks_dir(repo), "post-merge", &[flag], &[])?;
    Ok(())
}

/// A frase do `die_ff_impossible`.
fn die_ff_impossible(repo: &Repo) -> R<Fail> {
    if repo.config.get_bool("advice.diverging")?.unwrap_or(true) {
        hint("Diverging branches can't be fast-forwarded, you need to either:\n\n\tgit merge --no-ff\n\nor:\n\n\tgit rebase\n\nDisable this message with \"git config advice.diverging false\"");
    }
    Ok(Fail::Fatal("Not possible to fast-forward, aborting.".into()))
}

fn die_resolve_conflict(repo: &Repo) -> R<Fail> {
    error("Merging is not possible because you have unmerged files.");
    if repo.config.get_bool("advice.resolveconflict")?.unwrap_or(true) {
        hint("Fix them up in the work tree, and then use 'git add/rm <file>'\nas appropriate to mark resolution and make a commit.");
    }
    Ok(Fail::Fatal("Exiting because of an unresolved conflict.".into()))
}

/// `suggest_conflicts`: os caminhos em conflito entram no fim do `MERGE_MSG`.
fn suggest_conflicts(repo: &Repo) -> R<i32> {
    let idx = Index::load(&repo.index_path())?;
    let mut s: Vec<u8> = b"\n# Conflicts:\n".to_vec();
    for path in idx.unmerged_paths() {
        s.extend_from_slice(b"#\t");
        s.extend_from_slice(&path);
        s.push(b'\n');
    }
    let file = repo.path("MERGE_MSG");
    os::append(&file, &s, 0o666).map_err(|e| Fail::Fatal(format!("could not open '{}' for appending: {}", os::lossy(&file), e.message())))?;
    os::outs("Automatic merge failed; fix conflicts and then commit the result.\n");
    Ok(1)
}

/// O que o `git commit` pede pra assinar: o `Signed-off-by` no fim da mensagem.
fn signoff(msg: &mut Vec<u8>, who: &object::Ident) {
    super::commit::append_signoff(msg, who);
}

/// `prepare_to_commit`: grava `MERGE_HEAD`/`MERGE_MSG`, roda os hooks, abre o editor se pedido e
/// devolve a mensagem final. Mensagem vazia ou editor que falha deixam a mesclagem em andamento.
fn prepare_to_commit(repo: &Repo, mo: &Mo, heads: &[Oid], edit: bool, cleanup: Cleanup) -> R<Vec<u8>> {
    let hooks = hooks_dir(repo);
    let ipath = repo.index_path();
    if !mo.no_verify
        && let Some(code) = editor::run_hook(&hooks, "pre-merge-commit", &[], &[("GIT_INDEX_FILE", ipath.as_slice())])?
        && code != 0
    {
        return Err(abort_commit(repo, mo, heads, None));
    }
    let mut msg = mo.merge_msg.clone();
    if edit {
        msg.push(b'\n');
        msg.extend_from_slice(
            b"# Please enter a commit message to explain why this merge is necessary,\n# especially if it merges an updated upstream into a topic branch.\n#\n# Lines starting with '#' will be ignored, and an empty message aborts\n# the commit.\n",
        );
    }
    if mo.signoff {
        let who = ident::ident(&repo.config, Who::Committer, true)?;
        signoff(&mut msg, &who);
    }
    write_merge_heads(repo, heads, mo.ff)?;
    let mfile = repo.path("MERGE_MSG");
    os::write(&mfile, &msg, 0o666).map_err(|e| Fail::Fatal(format!("could not write '{}': {}", os::lossy(&mfile), e.message())))?;
    if let Some(code) = editor::run_hook(&hooks, "prepare-commit-msg", &[mfile.as_slice(), b"merge"], &[("GIT_INDEX_FILE", ipath.as_slice())])?
        && code != 0
    {
        return Err(abort_commit(repo, mo, heads, None));
    }
    if edit && editor::edit_file(&repo.config, &mfile).is_err() {
        return Err(abort_commit(repo, mo, heads, None));
    }
    if !mo.no_verify
        && let Some(code) = editor::run_hook(&hooks, "commit-msg", &[mfile.as_slice()], &[])?
        && code != 0
    {
        return Err(abort_commit(repo, mo, heads, None));
    }
    let text = os::read(&mfile).map_err(|e| Fail::Fatal(format!("Could not read from '{}': {}", os::lossy(&mfile), e.message())))?;
    let mut cleaned = msg::cleanup(&text, cleanup, b"#");
    if cleaned.is_empty() {
        return Err(abort_commit(repo, mo, heads, Some("Empty commit message.")));
    }
    if !cleaned.ends_with(b"\n") {
        cleaned.push(b'\n');
    }
    Ok(cleaned)
}

/// `reset --merge` (o `--abort` da mesclagem e o rollback do cherry-pick): o índice e a árvore de
/// trabalho vão ao `target` (o HEAD se `None`), menos as mudanças ainda não preparadas dos arquivos
/// que a mesclagem não tocou. O ORIG_HEAD guarda o HEAD de antes e o reflog ganha a linha
/// `reset: moving to ...`.
pub(crate) fn reset_merge(repo: &Repo, target: Option<&Oid>) -> R<()> {
    let Some(head) = repo.head_oid()? else {
        return Err(Fail::Fatal("Could not reset index file to revision 'HEAD'.".into()));
    };
    let dest = target.copied().unwrap_or(head);
    let spec = if target.is_some() { dest.hex() } else { "HEAD".to_string() };
    let want_tree = repo.flatten_tree(&repo.tree_of(&dest)?)?;
    let ipath = repo.index_path();
    let idx = Index::load(&ipath)?;
    let trust = repo.config.get_bool("core.filemode")?.unwrap_or(true);
    let mut out = Index { version: 2, existed: idx.existed, mtime: idx.mtime, ..Index::default() };
    let not_uptodate = |path: &[u8]| Fail::Fatal(format!("Entry '{}' not uptodate. Cannot merge.\nfatal: Could not reset index file to revision '{spec}'.", os::lossy(path)));
    let mut seen: BTreeSet<Vec<u8>> = BTreeSet::new();
    let mut i = 0;
    while i < idx.entries.len() {
        let path = idx.entries[i].path.clone();
        let mut j = i;
        while j < idx.entries.len() && idx.entries[j].path == path {
            j += 1;
        }
        let group = &idx.entries[i..j];
        i = j;
        seen.insert(path.clone());
        let want = want_tree.get(&path);
        if group.len() == 1 && group[0].stage == 0 {
            let e = &group[0];
            match want {
                Some(&(m, o)) if e.mode == m && e.oid == o => out.insert_raw(e.clone()),
                Some(&(m, o)) => {
                    if matches!(diff::check_entry(repo, &idx, e, trust)?, WtState::Changed(..)) {
                        return Err(not_uptodate(&path));
                    }
                    out.add(worktree::checkout_entry(repo, &path, m, &o)?);
                }
                None => {
                    if matches!(diff::check_entry(repo, &idx, e, trust)?, WtState::Changed(..)) {
                        return Err(not_uptodate(&path));
                    }
                    worktree::unlink_entry(&path);
                }
            }
        } else {
            match want {
                Some(&(m, o)) => out.add(worktree::checkout_entry(repo, &path, m, &o)?),
                None => worktree::unlink_entry(&path),
            }
        }
    }
    for (path, &(m, o)) in &want_tree {
        if !seen.contains(path) {
            out.add(worktree::checkout_entry(repo, path, m, &o)?);
        }
    }
    out.write(&ipath)?;
    repo.write_pseudoref("ORIG_HEAD", format!("{head}\n").as_bytes())?;
    repo.update_ref("HEAD", dest, None, &format!("reset: moving to {spec}"), false)?;
    super::revert::remove_branch_state(repo, false);
    Ok(())
}


// ---- a mesclagem ------------------------------------------------------------------------------

fn conflict_style(repo: &Repo) -> R<Style> {
    match repo.config.get("merge.conflictstyle").as_deref() {
        None | Some("merge") => Ok(Style::Merge),
        Some("diff3") => Ok(Style::Diff3),
        Some("zdiff3") => Ok(Style::ZealousDiff3),
        Some(other) => Err(Fail::Fatal(format!("unknown style '{other}' given for 'merge.conflictstyle'"))),
    }
}

/// As opções da mesclagem de conteúdo a partir do `-X`.
fn merge_options(repo: &Repo, mo: &Mo, branch2: &str) -> R<MergeOpts> {
    let mut o = MergeOpts::new("HEAD", branch2, "");
    o.style = conflict_style(repo)?;
    for x in &mo.xopts {
        let bad = || Fail::Fatal(format!("unknown strategy option: -X{x}"));
        match x.as_str() {
            "ours" => o.favor = Favor::Ours,
            "theirs" => o.favor = Favor::Theirs,
            "no-renames" => o.detect_renames = false,
            "renames" | "find-renames" => o.detect_renames = true,
            "patience" | "histogram" | "minimal" | "no-renormalize" | "no-directory-renames" => {}
            s => {
                let score = s.strip_prefix("find-renames=").or_else(|| s.strip_prefix("rename-threshold="));
                if let Some(v) = score {
                    o.detect_renames = true;
                    o.rename_score = crate::diff::rename::parse_score(v).ok_or_else(bad)?;
                } else if s.starts_with("diff-algorithm=") {
                    // O algoritmo de diff fica o Myers.
                } else {
                    return Err(bad());
                }
            }
        }
    }
    Ok(o)
}

/// `merge.defaultToUpstream`: o `dst` do refspec de busca do remoto que casa com `src`.
fn map_fetch(repo: &Repo, remote: &str, src: &str) -> Option<String> {
    for spec in repo.config.get_all(&format!("remote.{remote}.fetch")).into_iter().flatten() {
        let spec = os::lossy(spec);
        let spec = spec.strip_prefix('+').unwrap_or(&spec);
        let Some((s, d)) = spec.split_once(':') else { continue };
        if let (Some(spre), Some(dpre)) = (s.strip_suffix('*'), d.strip_suffix('*')) {
            if let Some(rest) = src.strip_prefix(spre) {
                return Some(format!("{dpre}{rest}"));
            }
        } else if s == src {
            return Some(d.to_string());
        }
    }
    None
}

fn setup_with_upstream(repo: &Repo, branch: Option<&str>) -> R<Vec<String>> {
    let Some(b) = branch else { return Err(Fail::Fatal("No current branch.".into())) };
    let Some(remote) = repo.config.get(&format!("branch.{b}.remote")) else {
        return Err(Fail::Fatal("No remote for the current branch.".into()));
    };
    let merges: Vec<String> = repo.config.get_all(&format!("branch.{b}.merge")).into_iter().flatten().map(os::lossy).collect();
    if merges.is_empty() {
        return Err(Fail::Fatal("No default upstream defined for the current branch.".into()));
    }
    let mut out = Vec::new();
    for m in merges {
        if remote == "." {
            out.push(m);
        } else {
            match map_fetch(repo, &remote, &m) {
                Some(d) => out.push(d),
                None => return Err(Fail::Fatal(format!("No remote-tracking branch for {m} from {remote}"))),
            }
        }
    }
    Ok(out)
}

fn default_edit_option(mo: &Mo) -> R<i32> {
    if mo.have_message {
        return Ok(0);
    }
    if let Some(e) = os::getenv("GIT_MERGE_AUTOEDIT") {
        return match config::parse_bool(Some(&e)) {
            Some(b) => Ok(i32::from(b)),
            None => Err(Fail::Fatal(format!("Bad value '{}' in environment 'GIT_MERGE_AUTOEDIT'", os::lossy(&e)))),
        };
    }
    let s = os::sysc();
    Ok(i32::from(s.isatty(sysabi::Fd::STDIN) && s.isatty(sysabi::Fd::STDOUT)))
}

fn strategy_failed(name: &str) -> R<i32> {
    os::errs(&format!("Merge with strategy {name} failed.\n"));
    Ok(2)
}

/// Cria o commit de mesclagem com os pais dados e devolve o id.
fn commit_merge(repo: &Repo, tree: Oid, parents: Vec<Oid>, message: Vec<u8>) -> R<Oid> {
    let author = ident::ident(&repo.config, Who::Author, true)?;
    let committer = ident::ident(&repo.config, Who::Committer, true)?;
    let c = Commit { tree, parents, author: author.to_bytes(), committer: committer.to_bytes(), encoding: None, extra: Vec::new(), message };
    repo.write_object(Kind::Commit, &object::encode_commit(&c))
}

pub fn run(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let p = opts::parse(SPECS, args, 0, usage)?;
    let repo = git.repo()?;
    let mut mo = parse_state(repo, &p, usage)?;
    let one_arg = args.len() == 1;

    if p.has("abort") {
        if !one_arg {
            return Err(usage_msg_opt(usage, "--abort expects no arguments"));
        }
        if !os::exists(&repo.path("MERGE_HEAD")) {
            return Err(Fail::Fatal("There is no merge to abort (MERGE_HEAD missing).".into()));
        }
        reset_merge(repo, None)?;
        return Ok(0);
    }
    if p.has("quit") {
        if !one_arg {
            return Err(usage_msg_opt(usage, "--quit expects no arguments"));
        }
        remove_merge_branch_state(repo);
        return Ok(0);
    }
    if p.has("continue") {
        if !one_arg {
            return Err(usage_msg_opt(usage, "--continue expects no arguments"));
        }
        if !os::exists(&repo.path("MERGE_HEAD")) {
            return Err(Fail::Fatal("There is no merge in progress (MERGE_HEAD missing).".into()));
        }
        return super::commit::run(git, &[]);
    }

    let ipath = repo.index_path();
    let idx = Index::load(&ipath)?;
    if idx.has_conflicts() {
        return Err(die_resolve_conflict(repo)?);
    }
    let advice = repo.config.get_bool("advice.resolveconflict")?.unwrap_or(true);
    if os::exists(&repo.path("MERGE_HEAD")) {
        let m = if advice {
            "You have not concluded your merge (MERGE_HEAD exists).\nPlease, commit your changes before you merge."
        } else {
            "You have not concluded your merge (MERGE_HEAD exists)."
        };
        return Err(Fail::Fatal(m.into()));
    }
    if os::exists(&repo.path("CHERRY_PICK_HEAD")) {
        let m = if advice {
            "You have not concluded your cherry-pick (CHERRY_PICK_HEAD exists).\nPlease, commit your changes before you merge."
        } else {
            "You have not concluded your cherry-pick (CHERRY_PICK_HEAD exists)."
        };
        return Err(Fail::Fatal(m.into()));
    }

    let head = repo.head()?;
    let branch_name: Option<String> = head.branch().map(|b| b.strip_prefix("refs/heads/").unwrap_or(b).to_string());
    if mo.option_edit < 0 {
        mo.option_edit = default_edit_option(&mo)?;
    }
    let edit = mo.option_edit > 0;
    let cleanup = cleanup_mode(mo.cleanup_arg.as_deref(), edit)?;
    if mo.verbosity < 0 {
        mo.show_diffstat = false;
    }
    if mo.squash {
        if mo.ff == Ff::No {
            return Err(Fail::Fatal("options '--squash' and '--no-ff.' cannot be used together".into()));
        }
        if mo.option_commit > 0 {
            return Err(Fail::Fatal("options '--squash' and '--commit.' cannot be used together".into()));
        }
        mo.option_commit = 0;
    }
    if mo.option_commit < 0 {
        mo.option_commit = 1;
    }
    let mut targets: Vec<String> = p.args.iter().map(|a| os::lossy(a)).collect();
    if targets.is_empty() {
        if !repo.config.get_bool("merge.defaulttoupstream")?.unwrap_or(true) {
            return Err(Fail::Fatal("No commit specified and merge.defaultToUpstream not set.".into()));
        }
        targets = setup_with_upstream(repo, branch_name.as_deref())?;
    } else if targets.len() == 1 && targets[0] == "-" {
        targets[0] = "@{-1}".to_string();
    }
    if mo.autostash {
        return Err(Fail::Fatal("--autostash is not supported by this git".into()));
    }

    let mut remotes: Vec<Remote> = Vec::new();
    for t in &targets {
        match get_merge_parent(repo, t)? {
            Some(r) => remotes.push(r),
            None => return Err(help_unknown_ref(repo, t, "merge", "not something we can merge")?),
        }
    }

    // Sem commits no ramo atual: a mesclagem vira um avanço direto.
    let Some(head_oid) = head.oid() else {
        if mo.squash {
            return Err(Fail::Fatal("Squash commit into empty head not supported yet".into()));
        }
        if mo.ff == Ff::No {
            return Err(Fail::Fatal("Non-fast-forward commit does not make sense into an empty head".into()));
        }
        if remotes.len() > 1 {
            return Err(Fail::Fatal("Can merge only exactly one commit into empty head".into()));
        }
        let r = &remotes[0];
        let new_tree = repo.tree_of(&r.commit)?;
        let uo = UnpackOpts { verb: "merge", advice: "merge", force: false };
        let new_idx = match unpack::switch_tree(repo, &idx, Some(&EMPTY_TREE), &new_tree, &uo) {
            Ok(i) => i,
            Err(Fail::Exit(1)) => return Ok(1),
            Err(e) => return Err(e),
        };
        new_idx.write(&ipath)?;
        repo.update_ref("HEAD", r.commit, None, "initial pull", false)?;
        return Ok(0);
    };
    let head_tree = repo.tree_of(&head_oid)?;

    // As cabeças que sobram: as que o HEAD já tem, ou que outra cabeça já tem, não contam.
    let mut graph = Graph::new(repo);
    let mut keep: Vec<usize> = Vec::new();
    for (i, r) in remotes.iter().enumerate() {
        if graph.is_ancestor(&r.commit, &head_oid)? {
            continue;
        }
        let mut covered = false;
        for (j, o) in remotes.iter().enumerate() {
            if i == j {
                continue;
            }
            if (o.commit == r.commit && j < i) || (o.commit != r.commit && graph.is_ancestor(&r.commit, &o.commit)?) {
                covered = true;
                break;
            }
        }
        if !covered {
            keep.push(i);
        }
    }
    if keep.len() > 1 {
        return Err(Fail::Fatal("merging more than one commit at once (octopus) is not supported by this git".into()));
    }
    if mo.strategies.len() > 1 {
        return Err(Fail::Fatal("trying more than one merge strategy is not supported by this git".into()));
    }
    let strategy = mo.strategies.first().cloned().unwrap_or_else(|| "ort".to_string());
    if matches!(strategy.as_str(), "octopus" | "resolve" | "subtree") {
        return Err(Fail::Fatal(format!("the '{strategy}' merge strategy is not supported by this git")));
    }
    if strategy == "ours" {
        mo.ff = Ff::No;
    }

    // A mensagem do commit de mesclagem.
    let single = keep.first().map(|&i| &remotes[i]);
    if let Some(r) = single
        && (!mo.have_message || mo.shortlog_len > 0)
    {
        let line = merge_name(repo, &r.name, r)?;
        let current = branch_name.clone().unwrap_or_else(|| "HEAD".to_string());
        let gen_msg = fmt_merge_msg(repo, &mo, &line, r, &head_oid, &current)?;
        mo.merge_msg.extend_from_slice(&gen_msg);
        if !mo.merge_msg.is_empty() && !mo.merge_msg.ends_with(b"\n") {
            mo.merge_msg.push(b'\n');
        }
        mo.merge_msg.pop();
    }

    let action = os::getenv_str("GIT_REFLOG_ACTION").unwrap_or_else(|| {
        let names: Vec<&str> = keep.iter().map(|&i| remotes[i].name.as_str()).collect();
        format!("merge {}", names.join(" "))
    });

    // O que a mesclagem tem que mexer.
    let bases: Vec<Oid> = match single {
        Some(r) => graph.merge_bases(&head_oid, &[r.commit])?,
        None => Vec::new(),
    };
    repo.write_pseudoref("ORIG_HEAD", format!("{head_oid}\n").as_bytes())?;

    let Some(remote) = single else {
        if mo.verbosity >= 0 {
            os::outs(if mo.squash { "Already up to date. (nothing to squash)\n" } else { "Already up to date.\n" });
        }
        remove_merge_branch_state(repo);
        return Ok(0);
    };
    if bases.is_empty() {
        if !mo.allow_unrelated {
            return Err(Fail::Fatal("refusing to merge unrelated histories".into()));
        }
    } else if bases.len() == 1 && bases[0] == remote.commit {
        if mo.verbosity >= 0 {
            os::outs(if mo.squash { "Already up to date. (nothing to squash)\n" } else { "Already up to date.\n" });
        }
        remove_merge_branch_state(repo);
        return Ok(0);
    } else if mo.ff != Ff::No && bases.len() == 1 && bases[0] == head_oid {
        let msg = if mo.have_message { "Fast-forward (no commit created; -m option ignored)" } else { "Fast-forward" };
        if mo.verbosity >= 0 {
            os::outs(&format!("Updating {}..{}\n", repo.abbrev_default(&head_oid), repo.abbrev_default(&remote.commit)));
        }
        let new_tree = repo.tree_of(&remote.commit)?;
        let uo = UnpackOpts { verb: "merge", advice: "merge", force: false };
        let new_idx = match unpack::switch_tree(repo, &idx, Some(&head_tree), &new_tree, &uo) {
            Ok(i) => i,
            Err(Fail::Exit(1)) => return Ok(1),
            Err(e) => return Err(e),
        };
        new_idx.write(&ipath)?;
        finish(repo, &mo, &head_oid, remote, Some(&remote.commit), Some(msg), &action)?;
        remove_merge_branch_state(repo);
        return Ok(0);
    }
    if mo.ff == Ff::Only {
        return Err(die_ff_impossible(repo)?);
    }

    // Uma mesclagem de verdade: precisa de identidade.
    ident::ident(&repo.config, Who::Committer, true)?;
    let mut cur = Index::load(&ipath)?;
    let trust = repo.config.get_bool("core.filemode")?.unwrap_or(true);
    if worktree::refresh(&mut cur, trust) {
        cur.write(&ipath)?;
    }
    let names = merge::unclean_names(repo, &head_tree)?;
    if !names.is_empty() {
        error(&format!("Your local changes to the following files would be overwritten by merge:\n  {}", os::lossy(&names)));
        return strategy_failed(&strategy);
    }
    
    let clean: bool = if strategy == "ours" {
        true
    } else {
        let mopts = merge_options(repo, &mo, &remote.name)?;
        let ordered: Vec<Oid> = bases.iter().rev().copied().collect();
        let outcome = merge::merge_commits(repo, &mopts, &head_oid, &remote.commit, &ordered)?;
        match merge::checkout_result(repo, &head_tree, &outcome) {
            Ok(()) => {}
            Err(Fail::Exit(1)) => return strategy_failed(&strategy),
            Err(e) => return Err(e),
        }
        repo.write_pseudoref("AUTO_MERGE", format!("{}\n", outcome.tree).as_bytes())?;
        outcome.display();
        outcome.clean
    };

    let heads = [remote.obj];
    if clean && mo.option_commit != 0 {
        // Deu certo e é pra gravar: o commit de mesclagem.
        let cur = Index::load(&ipath)?;
        let result_tree = repo.write_tree_from_index(&cur)?;
        let head_subsumed = graph.is_ancestor(&head_oid, &remote.commit)?;
        let mut parents: Vec<Oid> = vec![remote.commit];
        if !head_subsumed || mo.ff == Ff::No {
            parents.insert(0, head_oid);
        }
        let message = prepare_to_commit(repo, &mo, &heads, edit, cleanup)?;
        let new_commit = commit_merge(repo, result_tree, parents, message)?;
        let text = format!("Merge made by the '{strategy}' strategy.");
        finish(repo, &mo, &head_oid, remote, Some(&new_commit), Some(&text), &action)?;
        remove_merge_branch_state(repo);
        return Ok(0);
    }
    if mo.squash {
        finish(repo, &mo, &head_oid, remote, None, None, &action)?;
    } else {
        write_merge_state(repo, &mo, &heads)?;
    }
    if clean {
        os::errs("Automatic merge went well; stopped before committing as requested\n");
        return Ok(0);
    }
    suggest_conflicts(repo)
}



