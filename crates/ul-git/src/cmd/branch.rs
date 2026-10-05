//! `git branch`: lista (`-v`, `-vv`, `-a`, `-r`, `--merged`, `--contains`...), cria, remove (`-d`,
//! `-D`), renomeia e copia (`-m`, `-M`, `-c`, `-C`), mexe no upstream e mostra o ramo atual.

use super::Git;
use super::reffmt::{self, Ctx, Filter, Row, Upstream};
use crate::column;
use crate::config::{self, ConfigFile, Edit, Editor, Scope};
use crate::error::{Fail, R, error, hint, warning};
use crate::graph::Graph;
use crate::hash::Oid;
use crate::object;
use crate::opts::{self, Spec};
use crate::os;
use crate::refs::{self, Head};
use crate::repo::Repo;

const SPECS: &[Spec] = &[
    opts::flag(Some(b'v'), "verbose", "verbose"),
    opts::flag(Some(b'q'), "quiet", "quiet"),
    opts::optional(Some(b't'), "track", "track"),
    opts::value(Some(b'u'), "set-upstream-to", "set-upstream-to"),
    opts::flag(None, "unset-upstream", "unset-upstream"),
    opts::optional(None, "color", "color"),
    opts::noneg(opts::flag(Some(b'r'), "remotes", "remotes")),
    opts::noneg(opts::value(None, "contains", "contains")),
    opts::noneg(opts::value(None, "no-contains", "no-contains")),
    opts::optional(None, "abbrev", "abbrev"),
    opts::noneg(opts::flag(Some(b'a'), "all", "all")),
    opts::flag(Some(b'd'), "delete", "delete"),
    opts::short_flag(b'D', "force-delete"),
    opts::flag(Some(b'm'), "move", "move"),
    opts::short_flag(b'M', "force-move"),
    opts::flag(None, "omit-empty", "omit-empty"),
    opts::flag(Some(b'c'), "copy", "copy"),
    opts::short_flag(b'C', "force-copy"),
    opts::flag(Some(b'l'), "list", "list"),
    opts::flag(None, "show-current", "show-current"),
    opts::flag(None, "create-reflog", "create-reflog"),
    opts::flag(None, "edit-description", "edit-description"),
    opts::flag(Some(b'f'), "force", "force"),
    opts::noneg(opts::value(None, "merged", "merged")),
    opts::noneg(opts::value(None, "no-merged", "no-merged")),
    opts::optional(None, "column", "column"),
    opts::value(None, "sort", "sort"),
    opts::value(None, "points-at", "points-at"),
    opts::flag(Some(b'i'), "ignore-case", "ignore-case"),
    opts::flag(None, "recurse-submodules", "recurse-submodules"),
    opts::value(None, "format", "format"),
];

// ---- configuração local -----------------------------------------------------------------------

fn load_local(repo: &Repo) -> R<(Vec<u8>, ConfigFile)> {
    let path = repo.common("config");
    let file = match ConfigFile::load(&path, path.clone(), Scope::Local)? {
        Some(f) => f,
        None => ConfigFile { path: path.clone(), scope: Scope::Local, data: Vec::new(), entries: Vec::new(), sections: Vec::new(), command_line: false },
    };
    Ok((path, file))
}

fn save_config(path: &[u8], data: &[u8]) -> R<()> {
    os::write_locked(path, data).map_err(Fail::Fatal)
}

/// `git config <chave> <valor>` no arquivo do repositório.
pub fn config_set(repo: &Repo, key: &str, value: &str) -> R<()> {
    let (path, file) = load_local(repo)?;
    let parts = config::parse_key(key).map_err(Fail::Fatal)?;
    match (Editor { file: &file }).set(&parts, Some(value.as_bytes()), false, true, None) {
        Edit::Ok(data) => save_config(&path, &data),
        Edit::Fail(c) => Err(Fail::Exit(c)),
    }
}

/// `git config --unset-all <chave>`; ausente não é erro.
pub fn config_unset(repo: &Repo, key: &str) -> R<()> {
    let (path, file) = load_local(repo)?;
    let parts = config::parse_key(key).map_err(Fail::Fatal)?;
    if let Edit::Ok(data) = (Editor { file: &file }).unset(&parts, true, None) {
        save_config(&path, &data)?;
    }
    Ok(())
}

/// Tira a seção `branch.<nome>` do arquivo de configuração.
pub fn config_remove_branch(repo: &Repo, name: &str) -> R<()> {
    let (path, file) = load_local(repo)?;
    if let Some(data) = (Editor { file: &file }).remove_section(&format!("branch.{name}")) {
        save_config(&path, &data)?;
    }
    Ok(())
}

fn config_rename_branch(repo: &Repo, old: &str, new: &str) -> R<()> {
    let (path, file) = load_local(repo)?;
    let parts = config::parse_key(&format!("branch.{new}.x")).map_err(Fail::Fatal)?;
    if let Some(data) = (Editor { file: &file }).rename_section(&format!("branch.{old}"), &parts) {
        save_config(&path, &data)?;
    }
    Ok(())
}

fn config_copy_branch(repo: &Repo, old: &str, new: &str) -> R<()> {
    let (_, file) = load_local(repo)?;
    let prefix = format!("branch.{old}.");
    let items: Vec<(String, Vec<u8>)> =
        file.entries.iter().filter(|e| e.key.starts_with(&prefix)).filter_map(|e| e.value.clone().map(|v| (e.key[prefix.len()..].to_string(), v))).collect();
    for (k, v) in items {
        config_set(repo, &format!("branch.{new}.{k}"), &os::lossy(&v))?;
    }
    Ok(())
}

// ---- nomes, caminhos e acompanhamento ---------------------------------------------------------

fn short_branch(full: &str) -> &str {
    full.strip_prefix("refs/heads/").unwrap_or(full)
}

/// Caminho da árvore de trabalho como o git o mostra nas mensagens de "used by worktree".
fn worktree_path(repo: &Repo) -> String {
    let mut p = repo.work_tree.clone().unwrap_or_else(|| repo.git_dir.clone());
    while p.len() > 1 && p.ends_with(b"/") {
        p.pop();
    }
    os::lossy(&p)
}

/// Valida o nome (o `strbuf_check_branch_ref`), com a mensagem e a dica do git.
pub fn check_branch_name(repo: &Repo, name: &str) -> R<()> {
    if refs::valid_branch_name(name) {
        return Ok(());
    }
    if repo.config.get_bool("advice.refsyntax")?.unwrap_or(true) {
        os::flush_out();
        os::err_line("fatal: ", &format!("'{name}' is not a valid branch name"));
        hint("See `man git check-ref-format`\nDisable this message with \"git config advice.refSyntax false\"");
        return Err(Fail::Exit(128));
    }
    Err(Fail::Fatal(format!("'{name}' is not a valid branch name")))
}

/// Como acompanhar `full_ref` (remoto, ref de mesclagem, nome mostrado, é local?).
pub fn tracking_for(repo: &Repo, full_ref: &str) -> Option<(String, String, String, bool)> {
    if let Some(rest) = full_ref.strip_prefix("refs/heads/") {
        return Some((".".to_string(), full_ref.to_string(), rest.to_string(), true));
    }
    let shown = full_ref.strip_prefix("refs/remotes/")?;
    let mut remotes: Vec<String> = Vec::new();
    for (_, e) in repo.config.entries() {
        if let Some(mid) = e.key.strip_prefix("remote.").and_then(|r| r.strip_suffix(".fetch"))
            && !remotes.iter().any(|r| r == mid)
        {
            remotes.push(mid.to_string());
        }
    }
    for remote in remotes {
        for spec in repo.config.get_all(&format!("remote.{remote}.fetch")).into_iter().flatten() {
            let spec = os::lossy(spec);
            let spec = spec.strip_prefix('+').unwrap_or(&spec);
            let Some((src, dst)) = spec.split_once(':') else { continue };
            if let (Some(sp), Some(dp)) = (src.strip_suffix('*'), dst.strip_suffix('*')) {
                if let Some(rest) = full_ref.strip_prefix(dp) {
                    return Some((remote, format!("{sp}{rest}"), shown.to_string(), false));
                }
            } else if dst == full_ref {
                return Some((remote, src.to_string(), shown.to_string(), false));
            }
        }
    }
    None
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum TrackMode {
    /// Segue `branch.autoSetupMerge`.
    Auto,
    /// `--track`: acompanha e reclama se não dá.
    Explicit,
    /// `--no-track`.
    No,
}

pub fn track_mode(p: &opts::Parsed) -> R<TrackMode> {
    if !p.present("track") {
        return Ok(TrackMode::Auto);
    }
    if p.flag("track") == Some(false) {
        return Ok(TrackMode::No);
    }
    match p.value("track") {
        None | Some(b"direct") | Some(b"inherit") => Ok(TrackMode::Explicit),
        Some(_) => Err(opts::error_only("option `--track' expects \"direct\" or \"inherit\"")),
    }
}

/// Grava `branch.<nome>.remote` e `.merge` conforme o ponto de partida.
pub fn setup_tracking(repo: &Repo, branch: &str, start_text: &str, start_ref: Option<&str>, mode: TrackMode, quiet: bool) -> R<()> {
    if mode == TrackMode::No {
        return Ok(());
    }
    let not_a_branch = || Fail::Fatal(format!("cannot set up tracking information; starting point '{start_text}' is not a branch"));
    let Some(sref) = start_ref else {
        return if mode == TrackMode::Explicit { Err(not_a_branch()) } else { Ok(()) };
    };
    let Some((remote, merge, shown, is_local)) = tracking_for(repo, sref) else {
        return if mode == TrackMode::Explicit { Err(not_a_branch()) } else { Ok(()) };
    };
    if mode == TrackMode::Auto {
        let allowed = match repo.config.get("branch.autosetupmerge").map(|v| v.to_ascii_lowercase()).as_deref() {
            Some("always") => true,
            Some("inherit") => !is_local,
            Some(v) if config::parse_bool(Some(v.as_bytes())) == Some(false) => false,
            _ => !is_local,
        };
        if !allowed {
            return Ok(());
        }
    }
    config_set(repo, &format!("branch.{branch}.remote"), &remote)?;
    config_set(repo, &format!("branch.{branch}.merge"), &merge)?;
    if !quiet {
        os::outs(&format!("branch '{branch}' set up to track '{shown}'.\n"));
    }
    Ok(())
}

/// Ponto de partida de um ramo novo: o commit e, se o nome é de uma ref, qual.
pub struct Start {
    pub text: String,
    pub oid: Oid,
    pub ref_name: Option<String>,
}

pub fn resolve_start(repo: &Repo, text: &str) -> R<Start> {
    let Some(oid) = repo.rev_parse_commit(text.as_bytes())? else {
        return Err(Fail::Fatal(format!("not a valid object name: '{text}'")));
    };
    let ref_name = repo.dwim_ref_resolved(text)?;
    Ok(Start { text: text.to_string(), oid, ref_name })
}

/// Cria (ou, com `force`, reaponta) um ramo. `clobber_head` deixa mexer no ramo atual
/// (`checkout -B`). Devolve se o ramo já existia.
pub fn create_branch(repo: &Repo, name: &str, start: &Start, force: bool, clobber_head: bool, mode: TrackMode, quiet: bool) -> R<bool> {
    check_branch_name(repo, name)?;
    let full = format!("refs/heads/{name}");
    let existed = repo.read_ref(&full)?.is_some();
    if existed && !force {
        return Err(Fail::Fatal(format!("a branch named '{name}' already exists")));
    }
    if existed && !clobber_head && repo.current_branch()?.as_deref() == Some(full.as_str()) {
        return Err(Fail::Fatal(format!("cannot force update the branch '{name}' used by worktree at '{}'", worktree_path(repo))));
    }
    let msg = if existed { format!("branch: Reset to {}", start.text) } else { format!("branch: Created from {}", start.text) };
    // Como o `dwim_branch_start`: o `--track` sem ramo de partida morre antes de criar o ramo.
    if mode == TrackMode::Explicit && start.ref_name.as_deref().and_then(|r| tracking_for(repo, r)).is_none() {
        return Err(Fail::Fatal(format!("cannot set up tracking information; starting point '{}' is not a branch", start.text)));
    }
    let expect = if existed { None } else { Some(None) };
    repo.update_ref(&full, start.oid, expect, &msg, true)?;
    setup_tracking(repo, name, &start.text, start.ref_name.as_deref(), mode, quiet)?;
    Ok(existed)
}

/// Como o HEAD destacado é descrito: `(texto, ainda está lá)`, pelo último `checkout: moving from`
/// do reflog do HEAD.
pub fn detached_from(repo: &Repo, head: &Oid) -> R<(String, bool)> {
    let log = repo.read_reflog("HEAD");
    for e in log.iter().rev() {
        let msg = String::from_utf8_lossy(&e.message).into_owned();
        let Some(rest) = msg.strip_prefix("checkout: moving from ") else { continue };
        let Some(at) = rest.find(" to ") else { continue };
        let target = &rest[at + 4..];
        let mut from: Option<String> = None;
        if target != "HEAD"
            && let Some((full, oid)) = repo.dwim_ref(target)?
            && (oid == e.new || repo.peel_to_commit(&oid)? == Some(e.new))
        {
            let name = full.strip_prefix("refs/tags/").or_else(|| full.strip_prefix("refs/remotes/")).unwrap_or(&full);
            from = Some(name.to_string());
        }
        let text = from.unwrap_or_else(|| repo.abbrev_default(&e.new));
        return Ok((text, *head == e.new));
    }
    Ok((repo.abbrev_default(head), true))
}

fn detached_text(repo: &Repo, head: &Oid) -> R<String> {
    let (text, at) = detached_from(repo, head)?;
    Ok(format!("(HEAD detached {} {text})", if at { "at" } else { "from" }))
}

// ---- ponto de entrada -------------------------------------------------------------------------

pub fn run(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let args = reffmt::lastarg_default(args);
    let p = opts::parse(SPECS, &args, 0, usage)?;
    // O git valida o valor do `--track` no callback da opção, antes de qualquer modo.
    track_mode(&p)?;
    let repo = git.repo()?;
    let quiet = p.has("quiet");
    // `column.ui`/`column.branch`, depois cada `--column` na ordem, como os callbacks do git.
    let mut colopts = column::from_config(&repo.config, "branch")?;
    for h in p.hits.iter().filter(|h| h.id == "column") {
        column::parse_option(&mut colopts, h.negated, h.value.as_deref())?;
    }
    column::finalize(&mut colopts);
    if p.count("verbose") > 0 {
        if column::explicitly_enabled(colopts) {
            return Err(Fail::Fatal("options '--column' and '--verbose' cannot be used together".into()));
        }
        colopts = 0;
    }

    let force = p.has("force");
    let delete = p.has("delete") || p.has("force-delete");
    let rename = p.has("move") || p.has("force-move");
    let copy = p.has("copy") || p.has("force-copy");
    let edit = p.has("edit-description");
    let set_up = p.value("set-upstream-to").map(|v| v.to_vec());
    let unset_up = p.has("unset-upstream");
    let show_current = p.has("show-current");
    let filters = ["contains", "no-contains", "merged", "no-merged", "points-at"].iter().any(|id| p.present(id));

    if show_current {
        if let Some(b) = repo.current_branch()? {
            os::outs(&format!("{}\n", short_branch(&b)));
        }
        return Ok(0);
    }
    if delete {
        let force_delete = p.has("force-delete") || force;
        return delete_branches(repo, &p, force_delete, quiet);
    }
    if rename || copy {
        let forced = p.has("force-move") || p.has("force-copy") || force;
        return rename_or_copy(repo, &p, copy, forced);
    }
    if let Some(up) = set_up {
        return set_upstream(repo, &p, &up, quiet);
    }
    if unset_up {
        return unset_upstream(repo, &p);
    }
    if edit {
        return edit_description(repo, &p);
    }
    let mut list = p.has("list");
    if p.args.is_empty() || filters {
        list = true;
    }
    if list {
        return list_branches(repo, &p, usage, colopts);
    }
    if p.args.len() > 2 {
        opts::usage_to_stderr(usage);
        return Err(Fail::Exit(129));
    }
    create(repo, &p, force, quiet)
}

fn create(repo: &Repo, p: &opts::Parsed, force: bool, quiet: bool) -> R<i32> {
    let name = os::lossy(&p.args[0]);
    let head = repo.head()?;
    let start_text = match p.args.get(1) {
        Some(s) => os::lossy(s),
        None => match head.branch() {
            Some(b) => short_branch(b).to_string(),
            None => "HEAD".to_string(),
        },
    };
    let mode = track_mode(p)?;
    // O nome é validado antes do ponto de partida, como o git.
    check_branch_name(repo, &name)?;
    let start = resolve_start(repo, &start_text)?;
    create_branch(repo, &name, &start, force, false, mode, quiet)?;
    if p.has("create-reflog") && !repo.has_reflog(&format!("refs/heads/{name}")) {
        repo.append_reflog(&format!("refs/heads/{name}"), Oid::ZERO, start.oid, &format!("branch: Created from {}", start.text), true)?;
    }
    Ok(0)
}

// ---- remoção ----------------------------------------------------------------------------------

fn branch_merged(repo: &Repo, graph: &mut Graph, name: &str, oid: &Oid) -> R<bool> {
    let Some(rev) = repo.peel_to_commit(oid)? else { return Ok(true) };
    let upstream = match reffmt::upstream_ref_name(repo, name) {
        Some(u) => match repo.ref_oid(&u)? {
            Some(o) => repo.peel_to_commit(&o)?.map(|c| (u, c)),
            None => None,
        },
        None => None,
    };
    let head = match repo.head_oid()? {
        Some(h) => repo.peel_to_commit(&h)?,
        None => None,
    };
    let reference = match &upstream {
        Some((_, c)) => Some(*c),
        None => head,
    };
    let merged = match reference {
        Some(r) => graph.is_ancestor(&rev, &r)?,
        None => false,
    };
    if let Some((up_name, _)) = &upstream {
        let head_merged = match head {
            Some(h) => graph.is_ancestor(&rev, &h)?,
            None => false,
        };
        if merged && !head_merged {
            warning(&format!("deleting branch '{name}' that has been merged to\n         '{up_name}', but not yet merged to HEAD"));
        } else if !merged && head_merged {
            warning(&format!("not deleting branch '{name}' that is not yet merged to\n         '{up_name}', even though it is merged to HEAD"));
        }
    }
    Ok(merged)
}

fn delete_branches(repo: &Repo, p: &opts::Parsed, force: bool, quiet: bool) -> R<i32> {
    if p.args.is_empty() {
        return Err(Fail::Fatal("branch name required".into()));
    }
    let remote = p.has("remotes");
    let (prefix, what) = if remote { ("refs/remotes/", "remote-tracking branch") } else { ("refs/heads/", "branch") };
    let current = repo.current_branch()?;
    let mut graph = Graph::new(repo);
    let mut code = 0;
    for a in &p.args {
        let name = os::lossy(a);
        let full = format!("{prefix}{name}");
        if !remote && current.as_deref() == Some(full.as_str()) {
            error(&format!("cannot delete branch '{name}' used by worktree at '{}'", worktree_path(repo)));
            code = 1;
            continue;
        }
        let Some(oid) = repo.ref_oid(&full)? else {
            error(&format!("{what} '{name}' not found"));
            code = 1;
            continue;
        };
        if !force && !remote && !branch_merged(repo, &mut graph, &name, &oid)? {
            error(&format!("the branch '{name}' is not fully merged"));
            if repo.config.get_bool("advice.forcedeletebranch")?.unwrap_or(true) {
                hint(&format!("If you are sure you want to delete it, run 'git branch -D {name}'\nDisable this message with \"git config advice.forceDeleteBranch false\""));
            }
            code = 1;
            continue;
        }
        let shown = repo.abbrev_default(&oid);
        repo.delete_ref(&full, Some(oid))?;
        if !remote {
            config_remove_branch(repo, &name)?;
        }
        if !quiet {
            os::outs(&format!("Deleted {what} {name} (was {shown}).\n"));
        }
    }
    Ok(code)
}

// ---- renomear e copiar ------------------------------------------------------------------------

fn rename_or_copy(repo: &Repo, p: &opts::Parsed, copy: bool, force: bool) -> R<i32> {
    let head = repo.head()?;
    let current = head.branch().map(|b| short_branch(b).to_string());
    let (old, new) = match p.args.len() {
        0 => return Err(Fail::Fatal("branch name required".into())),
        1 => match current.clone() {
            Some(c) => (c, os::lossy(&p.args[0])),
            None => {
                return Err(Fail::Fatal(format!("cannot {} the current branch while not on any", if copy { "copy" } else { "rename" })));
            }
        },
        2 => (os::lossy(&p.args[0]), os::lossy(&p.args[1])),
        _ => return Err(Fail::Fatal(format!("too many arguments for a {} operation", if copy { "copy" } else { "rename" }))),
    };
    let old_full = format!("refs/heads/{old}");
    let new_full = format!("refs/heads/{new}");
    let old_oid = repo.ref_oid(&old_full)?;
    let old_is_current = current.as_deref() == Some(old.as_str());
    // Ramo atual ainda sem commit: só o HEAD muda de nome.
    let unborn_current = old_oid.is_none() && old_is_current && !copy;
    if old_oid.is_none() && !unborn_current {
        return Err(Fail::Fatal(format!("no branch named '{old}'")));
    }
    check_branch_name(repo, &new)?;
    let new_existed = repo.read_ref(&new_full)?.is_some();
    if new_existed && !force && old != new {
        return Err(Fail::Fatal(format!("a branch named '{new}' already exists")));
    }
    if new_existed && old != new && current.as_deref() == Some(new.as_str()) {
        return Err(Fail::Fatal(format!("cannot force update the branch '{new}' used by worktree at '{}'", worktree_path(repo))));
    }
    if unborn_current {
        repo.set_symref("HEAD", &new_full, None)?;
        return Ok(0);
    }
    let Some(oid) = old_oid else { return Ok(0) };
    if copy {
        repo.set_ref_no_log(&new_full, oid)?;
        let old_log = repo.common(&format!("logs/{old_full}"));
        if let Ok(Some(data)) = os::read_opt(&old_log) {
            let new_log = repo.common(&format!("logs/{new_full}"));
            let _ = os::mkdir_parents(&new_log);
            let _ = os::write(&new_log, &data, 0o666);
        }
        repo.append_reflog(&new_full, oid, oid, &format!("Branch: copied {old_full} to {new_full}"), true)?;
        config_copy_branch(repo, &old, &new)?;
        return Ok(0);
    }
    if old == new {
        return Ok(0);
    }
    if new_existed {
        repo.delete_ref(&new_full, None)?;
        config_remove_branch(repo, &new)?;
    }
    let had_log = repo.stash_reflog(&old_full)?;
    repo.delete_ref(&old_full, None)?;
    if had_log {
        repo.unstash_reflog(&new_full)?;
    }
    repo.set_ref_no_log(&new_full, oid)?;
    let msg = format!("Branch: renamed {old_full} to {new_full}");
    repo.append_reflog(&new_full, oid, oid, &msg, true)?;
    config_rename_branch(repo, &old, &new)?;
    if old_is_current {
        repo.set_symref("HEAD", &new_full, None)?;
        repo.append_reflog("HEAD", oid, Oid::ZERO, &msg, true)?;
        repo.append_reflog("HEAD", Oid::ZERO, oid, &msg, true)?;
    }
    Ok(0)
}

// ---- upstream e descrição ---------------------------------------------------------------------

fn set_upstream(repo: &Repo, p: &opts::Parsed, up: &[u8], quiet: bool) -> R<i32> {
    let up_text = os::lossy(up);
    if p.args.len() > 1 {
        return Err(Fail::Fatal("too many arguments to set new upstream".into()));
    }
    let branch = match p.args.first() {
        Some(b) => os::lossy(b),
        None => match repo.current_branch()? {
            Some(b) => short_branch(&b).to_string(),
            None => {
                return Err(Fail::Fatal(format!("could not set upstream of HEAD to {up_text} when it does not point to any branch")));
            }
        },
    };
    let full = format!("refs/heads/{branch}");
    if repo.read_ref(&full)?.is_none() {
        return Err(Fail::Fatal(format!("branch '{branch}' does not exist")));
    }
    let Some(up_full) = repo.dwim_ref_resolved(&up_text)? else {
        let mut msg = format!("the requested upstream branch '{up_text}' does not exist");
        if repo.config.get_bool("advice.setupstreamfailure")?.unwrap_or(true) {
            msg.push_str(
                "\nhint:\nhint: If you are planning on basing your work on an upstream\nhint: branch that already exists at the remote, you may need to\nhint: run \"git fetch\" to retrieve it.\nhint:\nhint: If you are planning to push out a new local branch that\nhint: will track its remote counterpart, you may want to use\nhint: \"git push -u\" to set the upstream config as you push.\nhint: Disable this message with \"git config advice.setUpstreamFailure false\"",
            );
        }
        return Err(Fail::Fatal(msg));
    };
    if up_full == full {
        warning(&format!("not setting branch '{branch}' as its own upstream"));
        return Ok(0);
    }
    setup_tracking(repo, &branch, &up_text, Some(&up_full), TrackMode::Explicit, quiet)?;
    Ok(0)
}

fn unset_upstream(repo: &Repo, p: &opts::Parsed) -> R<i32> {
    if p.args.len() > 1 {
        return Err(Fail::Fatal("too many arguments to unset upstream".into()));
    }
    let branch = match p.args.first() {
        Some(b) => os::lossy(b),
        None => match repo.current_branch()? {
            Some(b) => short_branch(&b).to_string(),
            None => return Err(Fail::Fatal("could not unset upstream of HEAD when it does not point to any branch".into())),
        },
    };
    let has_remote = repo.config.get(&format!("branch.{branch}.remote")).is_some();
    let has_merge = repo.config.get(&format!("branch.{branch}.merge")).is_some();
    if !has_remote && !has_merge {
        return Err(Fail::Fatal(format!("branch '{branch}' has no upstream information")));
    }
    config_unset(repo, &format!("branch.{branch}.remote"))?;
    config_unset(repo, &format!("branch.{branch}.merge"))?;
    Ok(0)
}

fn edit_description(repo: &Repo, p: &opts::Parsed) -> R<i32> {
    let branch = match p.args.first() {
        Some(b) => os::lossy(b),
        None => match repo.current_branch()? {
            Some(b) => short_branch(&b).to_string(),
            None => return Err(Fail::Fatal("cannot edit description of the current branch while not on any".into())),
        },
    };
    let full = format!("refs/heads/{branch}");
    if repo.read_ref(&full)?.is_none() {
        return Err(Fail::Fatal(format!("no branch named '{branch}'")));
    }
    let key = format!("branch.{branch}.description");
    let mut text: Vec<u8> = repo.config.get_bytes(&key).unwrap_or_default();
    if !text.is_empty() && !text.ends_with(b"\n") {
        text.push(b'\n');
    }
    text.extend_from_slice(format!("\n# Please edit the description for the branch\n#   {branch}\n# Lines starting with '#' will be stripped.\n").as_bytes());
    let path = repo.path("BRANCH_DESCRIPTION");
    os::write(&path, &text, 0o666).map_err(|e| Fail::Fatal(format!("could not write '{}': {}", os::lossy(&path), e.message())))?;
    crate::editor::edit_file(&repo.config, &path)?;
    let edited = os::read(&path).map_err(|e| Fail::Fatal(format!("could not read '{}': {}", os::lossy(&path), e.message())))?;
    let _ = os::unlink(&path);
    let cleaned = crate::msg::cleanup(&edited, crate::msg::Cleanup::Strip, b"#");
    if cleaned.is_empty() {
        config_unset(repo, &key)?;
    } else {
        config_set(repo, &key, &os::lossy(&cleaned))?;
    }
    Ok(0)
}

// ---- listagem ---------------------------------------------------------------------------------

struct Item {
    shown: String,
    current: bool,
    oid: Option<Oid>,
    row_name: String,
    /// `-> alvo` das refs simbólicas (o `origin/HEAD`).
    symref: Option<String>,
}

fn tracking_text(repo: &Repo, up: &Upstream, vv: bool) -> String {
    let short = super::plumbing::shorten_ref(repo, &up.name);
    let state = match up.counts {
        None => Some("gone".to_string()),
        Some((0, 0)) => None,
        Some((a, 0)) => Some(format!("ahead {a}")),
        Some((0, b)) => Some(format!("behind {b}")),
        Some((a, b)) => Some(format!("ahead {a}, behind {b}")),
    };
    match (vv, state) {
        (true, Some(s)) => format!("[{short}: {s}] "),
        (true, None) => format!("[{short}] "),
        (false, Some(s)) => format!("[{s}] "),
        (false, None) => String::new(),
    }
}

fn subject_of(ctx: &mut Ctx, oid: &Oid) -> R<Vec<u8>> {
    let Some(obj) = ctx.load(oid)? else { return Ok(Vec::new()) };
    if let Some(c) = &obj.commit {
        return Ok(object::subject_of(&c.message));
    }
    if let Some(t) = &obj.tag {
        return Ok(object::subject_of(&t.message));
    }
    Ok(Vec::new())
}

fn list_branches(repo: &Repo, p: &opts::Parsed, usage: &str, colopts: u32) -> R<i32> {
    let all = p.has("all");
    let remotes = p.has("remotes");
    let mut filter = Filter {
        kinds: if all {
            vec!["refs/heads/".to_string(), "refs/remotes/".to_string()]
        } else if remotes {
            vec!["refs/remotes/".to_string()]
        } else {
            vec!["refs/heads/".to_string()]
        },
        patterns: p.args.iter().map(|a| os::lossy(a)).collect(),
        ..Default::default()
    };
    reffmt::apply_filter_opts(repo, p, &mut filter)?;
    let mut keys = Vec::new();
    let sorts = p.values("sort");
    if sorts.is_empty() {
        for v in repo.config.get_all("branch.sort").into_iter().flatten() {
            keys.push(reffmt::parse_sort_key(&os::lossy(v))?);
        }
    } else {
        for s in sorts {
            keys.push(reffmt::parse_sort_key(&os::lossy(&s))?);
        }
    }
    let mut ctx = Ctx::new(repo)?;
    ctx.color = super::for_each_ref::color_enabled(p)?;
    let rows: Vec<Row> = reffmt::collect(&mut ctx, &filter)?;
    let rows = reffmt::sort_rows(&mut ctx, rows, &keys, filter.ignore_case)?;

    if let Some(fmt) = p.value("format") {
        let nodes = reffmt::parse_format(fmt, usage)?;
        let omit_empty = p.has("omit-empty");
        let mut lines: Vec<Vec<u8>> = Vec::new();
        for row in &rows {
            let mut line = Vec::new();
            ctx.render(&nodes, row, &mut line)?;
            // Em colunas a linha vazia entra mesmo com `--omit-empty`, como no git.
            if omit_empty && line.is_empty() && !column::active(colopts) {
                continue;
            }
            lines.push(line);
        }
        emit_lines(&lines, colopts);
        return Ok(0);
    }

    let verbose = p.count("verbose");
    let abbrev: Option<usize> = if p.present("abbrev") {
        if p.flag("abbrev") == Some(false) {
            Some(40)
        } else {
            p.value("abbrev").map(|v| os::lossy(v).parse::<usize>().unwrap_or(7).clamp(4, 40))
        }
    } else {
        None
    };
    let mut items: Vec<Item> = Vec::new();
    // O HEAD destacado abre a lista dos locais.
    if !remotes && let Head::Detached(h) = repo.head()? {
        let shown = detached_text(repo, &h)?;
        let pattern_ok = filter.patterns.is_empty() || filter.patterns.iter().any(|pat| crate::wildmatch::wildmatch(pat.as_bytes(), b"HEAD", if filter.ignore_case { crate::wildmatch::CASEFOLD } else { 0 }));
        if pattern_ok && reffmt::keep_by_graph(&mut ctx, &filter, &h)? {
            items.push(Item { shown, current: true, oid: Some(h), row_name: "HEAD".to_string(), symref: None });
        }
    }
    for row in &rows {
        let (current, shown) = if let Some(b) = row.name.strip_prefix("refs/heads/") {
            (ctx.head_ref() == Some(row.name.as_str()), b.to_string())
        } else if let Some(r) = row.name.strip_prefix("refs/remotes/") {
            (false, if all { format!("remotes/{r}") } else { r.to_string() })
        } else {
            (false, row.name.clone())
        };
        let symref = row.symref.as_ref().map(|t| t.strip_prefix("refs/remotes/").or_else(|| t.strip_prefix("refs/heads/")).unwrap_or(t).to_string());
        items.push(Item { shown, current, oid: Some(row.oid), row_name: row.name.clone(), symref });
    }
    let width = items.iter().map(|i| i.shown.chars().count()).max().unwrap_or(0);
    let mut lines: Vec<Vec<u8>> = Vec::new();
    for it in &items {
        let mut line = String::new();
        line.push_str(if it.current { "* " } else { "  " });
        line.push_str(&it.shown);
        if let Some(t) = &it.symref {
            line.push_str(" -> ");
            line.push_str(t);
            lines.push(line.into_bytes());
            continue;
        }
        if verbose > 0
            && let Some(oid) = it.oid
        {
            let pad = width.saturating_sub(it.shown.chars().count());
            line.push_str(&" ".repeat(pad));
            let hash = match abbrev {
                Some(n) if n >= 40 => oid.hex(),
                Some(n) => repo.abbrev(&oid, n),
                None => repo.abbrev_default(&oid),
            };
            line.push(' ');
            line.push_str(&hash);
            line.push(' ');
            if it.row_name != "HEAD"
                && let Some(up) = ctx.upstream(&it.row_name, &oid)?
            {
                line.push_str(&tracking_text(repo, &up, verbose > 1));
            }
            let mut bytes = line.into_bytes();
            bytes.extend_from_slice(&subject_of(&mut ctx, &oid)?);
            lines.push(bytes);
            continue;
        }
        lines.push(line.into_bytes());
    }
    emit_lines(&lines, colopts);
    Ok(0)
}

/// Escreve as linhas da listagem, em colunas quando ligadas (`print_columns` sem opções).
fn emit_lines(lines: &[Vec<u8>], colopts: u32) {
    if column::active(colopts) {
        os::out(&column::print_columns(lines, colopts, &column::Options { padding: 1, ..Default::default() }));
        return;
    }
    let mut out = Vec::new();
    for l in lines {
        out.extend_from_slice(l);
        out.push(b'\n');
    }
    os::out(&out);
}
