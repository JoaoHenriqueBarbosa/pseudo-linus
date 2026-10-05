//! `git checkout` e `git switch`: trocam de ramo ou commit (com as mensagens, o reflog e os avisos de
//! HEAD destacado do git) e, no `checkout`, restauram arquivos do índice ou de uma árvore.

use super::Git;
use super::branch::{self, TrackMode};
use super::reffmt;
use super::restore::{self, PathOpts, Source};
use super::status;
use super::unpack::{self, Opts as UnpackOpts};
use crate::error::{Fail, R, hint};
use crate::graph::Graph;
use crate::hash::Oid;
use crate::index::Index;
use crate::opts::{self, Spec};
use crate::os;
use crate::refs::{self, Head};
use crate::repo::Repo;

const CHECKOUT_SPECS: &[Spec] = &[
    opts::short_value(b'b', "new-branch"),
    opts::short_value(b'B', "new-branch-force"),
    opts::short_flag(b'l', "create-reflog"),
    opts::flag(None, "guess", "guess"),
    opts::flag(None, "overlay", "overlay"),
    opts::flag(Some(b'q'), "quiet", "quiet"),
    opts::optional(None, "recurse-submodules", "recurse-submodules"),
    opts::flag(None, "progress", "progress"),
    opts::flag(Some(b'm'), "merge", "merge"),
    opts::value(None, "conflict", "conflict"),
    opts::flag(Some(b'd'), "detach", "detach"),
    opts::optional(Some(b't'), "track", "track"),
    opts::flag(Some(b'f'), "force", "force"),
    opts::value(None, "orphan", "orphan"),
    opts::flag(None, "overwrite-ignore", "overwrite-ignore"),
    opts::flag(None, "ignore-other-worktrees", "ignore-other-worktrees"),
    opts::noneg(opts::flag(Some(b'2'), "ours", "ours")),
    opts::noneg(opts::flag(Some(b'3'), "theirs", "theirs")),
    opts::flag(Some(b'p'), "patch", "patch"),
    opts::flag(None, "ignore-skip-worktree-bits", "ignore-skip-worktree-bits"),
    opts::value(None, "pathspec-from-file", "pathspec-from-file"),
    opts::flag(None, "pathspec-file-nul", "pathspec-file-nul"),
];

const SWITCH_SPECS: &[Spec] = &[
    opts::value(Some(b'c'), "create", "new-branch"),
    opts::value(Some(b'C'), "force-create", "new-branch-force"),
    opts::flag(None, "guess", "guess"),
    opts::flag(None, "discard-changes", "discard-changes"),
    opts::flag(Some(b'q'), "quiet", "quiet"),
    opts::optional(None, "recurse-submodules", "recurse-submodules"),
    opts::flag(None, "progress", "progress"),
    opts::flag(Some(b'm'), "merge", "merge"),
    opts::value(None, "conflict", "conflict"),
    opts::flag(Some(b'd'), "detach", "detach"),
    opts::optional(Some(b't'), "track", "track"),
    opts::flag(Some(b'f'), "force", "force"),
    opts::value(None, "orphan", "orphan"),
    opts::flag(None, "overwrite-ignore", "overwrite-ignore"),
    opts::flag(None, "ignore-other-worktrees", "ignore-other-worktrees"),
];

const DETACH_ADVICE: &str = "\nYou are in 'detached HEAD' state. You can look around, make experimental\nchanges and commit them, and you can discard any commits you make in this\nstate without impacting any branches by switching back to a branch.\n\nIf you want to create a new branch to retain commits you create, you may\ndo so (now or later) by using -c with the switch command. Example:\n\n  git switch -c <new-branch-name>\n\nOr undo this operation with:\n\n  git switch -\n\nTurn off this advice by setting config variable advice.detachedHead to false\n\n";

/// Opções comuns ao `checkout` e ao `switch`.
struct Co {
    quiet: bool,
    force: bool,
    merge: bool,
    detach: bool,
    new_branch: Option<String>,
    force_new: bool,
    orphan: Option<String>,
    track: TrackMode,
    guess: bool,
}

/// Para onde o usuário mandou ir.
struct Target {
    /// Como foi escrito (ou o nome do ramo, para `-`).
    name: String,
    /// `refs/heads/<nome>` se é um ramo que existe.
    path: Option<String>,
    commit: Oid,
}

/// Onde o HEAD estava.
struct OldHead {
    /// Ramo (`refs/heads/x`), ou `None` se destacado.
    path: Option<String>,
    oid: Option<Oid>,
}

/// `-` e `@{-N}` viram o nome do ramo anterior.
fn expand_previous(repo: &Repo, text: &str) -> R<String> {
    let n = if text == "-" {
        Some(1)
    } else {
        text.strip_prefix("@{-").and_then(|r| r.strip_suffix('}')).and_then(|n| n.parse::<usize>().ok())
    };
    match n {
        Some(n) => Ok(repo.nth_prior_branch(n)?.unwrap_or_else(|| text.to_string())),
        None => Ok(text.to_string()),
    }
}

fn resolve_target(repo: &Repo, text: &str) -> R<Option<Target>> {
    let name = expand_previous(repo, text)?;
    let full = format!("refs/heads/{name}");
    if refs::check_refname_format(&full, false, false)
        && let Some(oid) = repo.ref_oid(&full)?
    {
        let commit = repo.peel_to_commit(&oid)?;
        return Ok(commit.map(|c| Target { name, path: Some(full), commit: c }));
    }
    match repo.rev_parse_commit(name.as_bytes())? {
        Some(c) => Ok(Some(Target { name, path: None, commit: c })),
        None => Ok(None),
    }
}

/// O ramo de acompanhamento remoto único com esse nome (`--guess`).
fn guess_remote(repo: &Repo, name: &str) -> R<Option<String>> {
    let mut found: Vec<String> = Vec::new();
    for (full, _) in repo.list_refs("refs/remotes/")? {
        let Some(rest) = full.strip_prefix("refs/remotes/") else { continue };
        if let Some((_, tail)) = rest.split_once('/')
            && tail == name
        {
            found.push(full);
        }
    }
    match found.len() {
        0 => Ok(None),
        1 => Ok(found.pop()),
        n => Err(Fail::Fatal(format!("'{name}' matched multiple ({n}) remote tracking branches"))),
    }
}

fn describe_detached(repo: &Repo, prefix: &str, oid: &Oid) -> R<()> {
    let c = repo.read_commit(oid)?;
    os::errs(&format!("{prefix} {} {}\n", repo.abbrev_default(oid), os::lossy(&c.subject())));
    Ok(())
}

/// Aviso de commits que ficam pra trás ao sair de um HEAD destacado.
fn orphaned_warning(repo: &Repo, old: &Oid, new: Option<&Oid>) -> R<()> {
    let mut graph = Graph::new(repo);
    let mut tips: Vec<Oid> = Vec::new();
    for (_, id) in repo.list_refs("refs/")? {
        if let Some(c) = repo.peel_to_commit(&id)? {
            tips.push(c);
        }
    }
    if let Some(n) = new {
        tips.push(*n);
    }
    let covered = graph.reachable(&tips)?;
    if covered.contains(old) {
        return describe_detached(repo, "Previous HEAD position was", old);
    }
    let mut lost: Vec<(i64, usize, Oid)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut stack = vec![*old];
    while let Some(c) = stack.pop() {
        if covered.contains(&c) || !seen.insert(c) {
            continue;
        }
        let (date, parents) = graph.info(&c)?;
        lost.push((date, lost.len(), c));
        stack.extend(parents);
    }
    lost.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let n = lost.len();
    let mut msg = format!("Warning: you are leaving {n} commit{} behind, not connected to\nany of your branches:\n\n", if n == 1 { "" } else { "s" });
    for (_, _, c) in &lost {
        let commit = repo.read_commit(c)?;
        msg.push_str(&format!("  {} {}\n", repo.abbrev_default(c), os::lossy(&commit.subject())));
    }
    msg.push('\n');
    if repo.config.get_bool("advice.detachedhead")?.unwrap_or(true) {
        msg.push_str(&format!(
            "If you want to keep {} by creating a new branch, this may be a good time\nto do so with:\n\n git branch <new-branch-name> {}\n\n",
            if n == 1 { "it" } else { "them" },
            repo.abbrev_default(old)
        ));
    }
    os::errs(&msg);
    Ok(())
}

/// "Your branch is ahead of ..." depois de entrar num ramo com upstream.
fn report_tracking(repo: &Repo, branch_ref: &str) -> R<()> {
    if let Some(t) = status::tracking(repo, branch_ref, true)? {
        let hints = repo.config.get_bool("advice.statushints")?.unwrap_or(true);
        os::outs(&status::tracking_text(&t, hints));
    }
    Ok(())
}

fn old_head(repo: &Repo) -> R<OldHead> {
    Ok(match repo.head()? {
        Head::Branch(path, oid) => OldHead { path: Some(path), oid },
        Head::Detached(oid) => OldHead { path: None, oid: Some(oid) },
    })
}

/// O que o `switch` recusa no meio de um merge, rebase etc.
fn die_if_operation_in_progress(repo: &Repo) -> R<()> {
    let checks: [(&str, &str, &str); 5] = [
        ("MERGE_HEAD", "merging", "\"git merge --quit\" or \"git worktree add\""),
        ("rebase-merge", "rebasing", "\"git rebase --quit\" or \"git worktree add\""),
        ("CHERRY_PICK_HEAD", "cherry-picking", "\"git cherry-pick --quit\" or \"git worktree add\""),
        ("REVERT_HEAD", "reverting", "\"git revert --quit\" or \"git worktree add\""),
        ("BISECT_LOG", "bisecting", "\"git bisect reset\""),
    ];
    for (file, what, consider) in checks {
        if os::exists(&repo.path(file)) || (file == "rebase-merge" && os::exists(&repo.path("rebase-apply"))) {
            return Err(Fail::Fatal(format!("cannot switch branch while {what}\nConsider {consider}.")));
        }
    }
    Ok(())
}

// ---- trocar de ramo ---------------------------------------------------------------------------

/// A troca em si. `target` é o destino dado (ou `None`), `start` o ponto de partida do ramo novo
/// quando difere do destino.
fn switch_to(repo: &Repo, co: &Co, target: Option<Target>, new_branch_start: Option<String>) -> R<i32> {
    let old = old_head(repo)?;
    let quiet = co.quiet;

    // Ramo atual ainda sem commit: `-b` só dá outro nome a ele.
    if let Some(nb) = &co.new_branch
        && target.is_none()
        && old.oid.is_none()
        && old.path.is_some()
    {
        branch::check_branch_name(repo, nb)?;
        let full = format!("refs/heads/{nb}");
        if repo.read_ref(&full)?.is_some() && !co.force_new {
            return Err(Fail::Fatal(format!("a branch named '{nb}' already exists")));
        }
        repo.set_symref("HEAD", &full, None)?;
        if !quiet {
            os::errs(&format!("Switched to a new branch '{nb}'\n"));
        }
        return Ok(0);
    }

    // Destino efetivo: o dado, ou o próprio HEAD.
    let (target_name, target_path, target_commit, explicit) = match &target {
        Some(t) => (t.name.clone(), t.path.clone(), Some(t.commit), true),
        None => ("HEAD".to_string(), None, old.oid, false),
    };
    if !explicit && target_commit.is_none() && co.orphan.is_none() {
        return Err(Fail::Fatal("You are on a branch yet to be born".into()));
    }
    let start_text = new_branch_start.clone().unwrap_or_else(|| target_name.clone());

    // Árvore nova (o ramo órfão sem ponto de partida fica com o que tem).
    let new_tree: Option<Oid> = match (&co.orphan, explicit) {
        (Some(_), false) => None,
        _ => match target_commit {
            Some(c) => Some(repo.tree_of(&c)?),
            None => None,
        },
    };
    let do_merge = explicit && new_tree.is_some();
    let had_upstream = co.new_branch.as_ref().map(|nb| reffmt::upstream_ref_name(repo, nb).is_some()).unwrap_or(false);
    let mut local_changes: Vec<u8> = Vec::new();
    if do_merge && let Some(nt) = new_tree {
        let ipath = repo.index_path();
        let idx = Index::load(&ipath)?;
        let old_tree = match old.oid {
            Some(o) => Some(repo.tree_of(&o)?),
            None => None,
        };
        let uo = UnpackOpts { verb: "checkout", advice: "switch branches", force: co.force };
        let new_idx = unpack::switch_tree(repo, &idx, old_tree.as_ref(), &nt, &uo)?;
        new_idx.write(&ipath)?;
        if !co.force && !quiet {
            local_changes = unpack::local_changes(repo, &nt)?;
        }
    }
    if !local_changes.is_empty() {
        os::out(&local_changes);
    }
    if !quiet
        && old.path.is_none()
        && let (Some(oo), Some(nc)) = (old.oid, target_commit)
        && oo != nc
    {
        orphaned_warning(repo, &oo, Some(&nc))?;
    }

    // Atualização das refs.
    let mut new_path = if co.detach { None } else { target_path.clone() };
    let mut new_name = target_name.clone();
    let mut branch_existed = false;
    let mut created: Option<String> = None;
    if let Some(nb) = &co.new_branch {
        let Some(commit) = target_commit else {
            return Err(Fail::Fatal(format!("Cannot switch branch to a non-commit '{start_text}'")));
        };
        let ref_name = if explicit { repo.dwim_ref_name(&start_text)? } else { None };
        let start = branch::Start { text: start_text.clone(), oid: commit, ref_name };
        branch_existed = branch::create_branch(repo, nb, &start, co.force_new, co.force_new, co.track, quiet)?;
        new_name = nb.clone();
        new_path = Some(format!("refs/heads/{nb}"));
        created = Some(nb.clone());
    } else if let Some(ob) = &co.orphan {
        branch::check_branch_name(repo, ob)?;
        let full = format!("refs/heads/{ob}");
        if repo.read_ref(&full)?.is_some() {
            return Err(Fail::Fatal(format!("a branch named '{ob}' already exists")));
        }
        repo.set_symref("HEAD", &full, None)?;
        if !quiet {
            os::errs(&format!("Switched to a new branch '{ob}'\n"));
        }
        return Ok(0);
    }

    let old_desc = match (&old.path, old.oid) {
        (Some(p), _) => Some(p.strip_prefix("refs/heads/").unwrap_or(p).to_string()),
        (None, Some(o)) => Some(o.hex()),
        _ => None,
    };
    let msg = format!("checkout: moving from {} to {new_name}", old_desc.unwrap_or_else(|| "(invalid)".to_string()));
    let new_commit = target_commit;
    if new_name == "HEAD" && new_path.is_none() && !co.detach {
        // Nada a fazer.
    } else if co.detach || new_path.is_none() {
        let Some(nc) = new_commit else { return Err(Fail::Fatal("You are on a branch yet to be born".into())) };
        let unchanged = old.path.is_none() && old.oid == Some(nc);
        if !unchanged {
            repo.update_ref("HEAD", nc, None, &msg, true)?;
        }
        if !quiet {
            if old.path.is_some() && !co.detach && repo.config.get_bool("advice.detachedhead")?.unwrap_or(true) {
                os::errs(&format!("Note: switching to '{new_name}'.\n{DETACH_ADVICE}"));
            }
            describe_detached(repo, "HEAD is now at", &nc)?;
        }
    } else if let Some(np) = &new_path {
        let Some(nc) = new_commit else { return Err(Fail::Fatal("You are on a branch yet to be born".into())) };
        repo.set_symref("HEAD", np, None)?;
        repo.log_ref_update("HEAD", old.oid.unwrap_or(Oid::ZERO), nc, &msg)?;
        if !quiet {
            let short = np.strip_prefix("refs/heads/").unwrap_or(np);
            if old.path.as_deref() == Some(np.as_str()) {
                if co.force_new {
                    os::errs(&format!("Reset branch '{short}'\n"));
                } else {
                    os::errs(&format!("Already on '{short}'\n"));
                }
            } else if created.is_some() {
                if branch_existed {
                    os::errs(&format!("Switched to and reset branch '{short}'\n"));
                } else {
                    os::errs(&format!("Switched to a new branch '{short}'\n"));
                }
            } else {
                os::errs(&format!("Switched to branch '{short}'\n"));
            }
        }
    }
    for f in ["MERGE_HEAD", "MERGE_MSG", "MERGE_MODE", "SQUASH_MSG"] {
        let _ = os::unlink(&repo.path(f));
    }
    if !quiet && !co.detach {
        let report = match &new_path {
            Some(_) => created.is_none() || had_upstream,
            None => false,
        };
        if report && let Some(np) = &new_path {
            report_tracking(repo, np)?;
        }
    }
    Ok(0)
}

// ---- checkout ---------------------------------------------------------------------------------

fn common(p: &opts::Parsed, is_switch: bool) -> R<Co> {
    let new_branch = p.value_str("new-branch");
    let new_branch_force = p.value_str("new-branch-force");
    let orphan = p.value_str("orphan");
    let given = [new_branch.is_some(), new_branch_force.is_some(), orphan.is_some()].iter().filter(|b| **b).count();
    if given > 1 {
        let msg = if is_switch { "options '-c', '-C', and '--orphan' cannot be used together" } else { "options '-b', '-B', and '--orphan' cannot be used together" };
        return Err(Fail::Fatal(msg.into()));
    }
    let detach = p.has("detach");
    if detach && given > 0 {
        let msg = if is_switch { "'--detach' cannot be used with '-c/-C/--orphan'" } else { "'--detach' cannot be used with '-b/-B/--orphan'" };
        return Err(Fail::Fatal(msg.into()));
    }
    let force_new = new_branch_force.is_some();
    Ok(Co {
        quiet: p.has("quiet"),
        force: p.has("force") || p.has("discard-changes"),
        merge: p.has("merge"),
        detach,
        new_branch: new_branch_force.or(new_branch),
        force_new,
        orphan,
        track: branch::track_mode(p)?,
        guess: p.flag("guess") != Some(false),
    })
}

pub fn run_checkout(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let p = opts::parse(CHECKOUT_SPECS, args, 0, usage)?;
    let repo = git.repo()?;
    if p.has("patch") {
        return Err(Fail::Fatal("interactive checkout needs a terminal; this sandbox has none".into()));
    }
    let mut co = common(&p, false)?;
    let has_dd = p.dashdash.is_some();
    let (before, after) = p.split_dashdash();
    let mut paths: Vec<Vec<u8>> = Vec::new();
    let mut rev_text: Option<String> = None;
    let mut target: Option<Target> = None;
    let mut start_override: Option<String> = None;

    // `--track` sem `-b`: o nome do ramo sai do argumento (`origin/dev` vira `dev`).
    if p.present("track") && p.flag("track") != Some(false) && co.new_branch.is_none() {
        let Some(first) = before.first() else { return Err(Fail::Fatal("--track needs a branch name".into())) };
        let text = os::lossy(first);
        let t = text.strip_prefix("refs/").unwrap_or(&text);
        let t = t.strip_prefix("remotes/").unwrap_or(t);
        match t.split_once('/') {
            Some((_, rest)) if !rest.is_empty() => co.new_branch = Some(rest.to_string()),
            _ => return Err(Fail::Fatal("missing branch name; try -b".into())),
        }
    }

    if has_dd {
        if before.len() > 1 {
            return Err(Fail::Fatal("only one reference expected".into()));
        }
        if let Some(r) = before.first() {
            rev_text = Some(os::lossy(r));
        }
        paths = after;
    } else if let Some(first) = p.args.first() {
        let text = os::lossy(first);
        match resolve_target(repo, &text)? {
            Some(t) => {
                rev_text = Some(text);
                target = Some(t);
                paths = p.args[1..].to_vec();
            }
            None => {
                let mut guessed = false;
                if p.args.len() == 1 && co.guess && !os::exists(first) && co.new_branch.is_none() && co.orphan.is_none() && !co.detach && !text.contains('/') {
                    let guess_on = repo.config.get_bool("checkout.guess")?.unwrap_or(true);
                    if guess_on && let Some(full) = guess_remote(repo, &text)? {
                        let short = full.strip_prefix("refs/remotes/").unwrap_or(&full).to_string();
                        co.new_branch = Some(text.clone());
                        co.track = TrackMode::Explicit;
                        target = resolve_target(repo, &short)?;
                        start_override = Some(short.clone());
                        rev_text = Some(short);
                        guessed = true;
                    }
                }
                if !guessed {
                    paths = p.args.clone();
                }
            }
        }
    }
    if let Some(f) = p.value("pathspec-from-file") {
        let data = if f == b"-" { os::stdin_all() } else { super::plumbing::read_user_file(repo, f)? };
        let sep = if p.has("pathspec-file-nul") { 0 } else { b'\n' };
        paths.extend(data.split(|c| *c == sep).filter(|l| !l.is_empty()).map(|l| l.to_vec()));
    }

    if !paths.is_empty() {
        if co.detach {
            return Err(Fail::Fatal(format!("git checkout: --detach does not take a path argument '{}'", os::lossy(&paths[0]))));
        }
        if let Some(nb) = co.new_branch.as_ref().or(co.orphan.as_ref()) {
            return Err(Fail::Fatal(format!("Cannot update paths and switch to branch '{nb}' at the same time.")));
        }
        return checkout_paths(git, &p, rev_text, &paths, has_dd);
    }
    if p.has("ours") || p.has("theirs") {
        return Err(Fail::Fatal("'--ours/--theirs' cannot be used with switching branches".into()));
    }
    if has_dd && rev_text.is_some() && target.is_none() {
        let text = rev_text.clone().unwrap_or_default();
        target = resolve_target(repo, &text)?;
        if target.is_none() {
            return Err(Fail::Fatal(format!("invalid reference: {text}")));
        }
    }
    let _ = co.merge;
    switch_to(repo, &co, target, start_override)
}

fn checkout_paths(git: &Git, p: &opts::Parsed, rev_text: Option<String>, paths: &[Vec<u8>], has_dd: bool) -> R<i32> {
    let repo = git.repo()?;
    let source = match &rev_text {
        Some(t) => {
            let Some(id) = repo.rev_parse(t.as_bytes())? else {
                return Err(Fail::Fatal(format!("invalid reference: {t}")));
            };
            let Some(tree) = repo.peel_to_tree(&id)? else {
                return Err(Fail::Fatal(format!("reference is not a tree: {t}")));
            };
            Source::Tree(tree)
        }
        None => Source::Index,
    };
    let ps = git.pathspec(paths)?;
    let ipath = repo.index_path();
    let mut idx = Index::load(&ipath)?;
    let overlay = p.flag("overlay") != Some(false);
    let o = PathOpts {
        overlay,
        update_index: matches!(source, Source::Tree(_)),
        update_worktree: true,
        stage: if p.has("ours") {
            Some(2)
        } else if p.has("theirs") {
            Some(3)
        } else {
            None
        },
        ignore_unmerged: p.has("force"),
    };
    let (written, dirty) = restore::restore_paths(repo, &mut idx, &source, &ps, &o)?;
    if dirty {
        idx.write(&ipath)?;
    }
    if !p.has("quiet") && !has_dd {
        let plural = if written == 1 { "path" } else { "paths" };
        match &source {
            Source::Index => os::errs(&format!("Updated {written} {plural} from the index\n")),
            Source::Tree(t) => os::errs(&format!("Updated {written} {plural} from {}\n", repo.abbrev_default(t))),
        }
    }
    Ok(0)
}

// ---- switch -----------------------------------------------------------------------------------

pub fn run_switch(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let p = opts::parse(SWITCH_SPECS, args, 0, usage)?;
    let repo = git.repo()?;
    let co = common(&p, true)?;
    if p.args.len() > 1 {
        return Err(Fail::Fatal("only one reference expected".into()));
    }
    if p.args.is_empty() && co.new_branch.is_none() && co.orphan.is_none() {
        return Err(Fail::Fatal("missing branch or commit argument".into()));
    }
    if !co.force {
        die_if_operation_in_progress(repo)?;
    }
    let mut co = co;
    let mut target: Option<Target> = None;
    let mut start_override: Option<String> = None;
    if let Some(first) = p.args.first() {
        let text = os::lossy(first);
        target = resolve_target(repo, &text)?;
        if target.is_none() {
            let mut guessed = false;
            if co.guess && co.new_branch.is_none() && co.orphan.is_none() && !co.detach && !text.contains('/') {
                let guess_on = repo.config.get_bool("checkout.guess")?.unwrap_or(true);
                if guess_on && let Some(full) = guess_remote(repo, &text)? {
                    let short = full.strip_prefix("refs/remotes/").unwrap_or(&full).to_string();
                    co.new_branch = Some(text.clone());
                    co.track = TrackMode::Explicit;
                    target = resolve_target(repo, &short)?;
                    start_override = Some(short);
                    guessed = true;
                }
            }
            if !guessed {
                return Err(Fail::Fatal(format!("invalid reference: {text}")));
            }
        }
        if let Some(t) = &target
            && t.path.is_none()
            && !co.detach
            && co.new_branch.is_none()
            && co.orphan.is_none()
        {
            let kind = if repo.read_ref(&format!("refs/tags/{}", t.name))?.is_some() {
                "tag"
            } else if repo.read_ref(&format!("refs/remotes/{}", t.name))?.is_some() {
                "remote branch"
            } else {
                "commit"
            };
            os::flush_out();
            error_fatal(&format!("a branch is expected, got {kind} '{}'", t.name));
            hint("If you want to detach HEAD at the commit, try again with the --detach option.");
            return Err(Fail::Exit(128));
        }
    }
    switch_to(repo, &co, target, start_override)
}

fn error_fatal(msg: &str) {
    os::err_line("fatal: ", msg);
}
