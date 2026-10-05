//! `git stash`: guarda as mudanças locais (índice, árvore de trabalho e, com `-u`/`-a`, os arquivos
//! não rastreados) em commits pendurados em `refs/stash`, e as devolve com `apply`, `pop` e `branch`.
//! Porta do `builtin/stash.c` sem processos filhos: a limpeza (`reset --hard`, `clean`), o `apply`
//! e o `status` final rodam dentro do processo. O modo `-p` (interativo) não existe.

use std::collections::BTreeMap;

use super::Git;
use super::add;
use crate::cmd::diff_cmd;
use crate::cmd::unpack::{self, Opts as UnpackOpts};
use crate::diff::{self, WtState};
use crate::error::{Fail, R, error};
use crate::hash::{EMPTY_TREE, Kind, Oid};
use crate::ident::{self, Who};
use crate::ignore::Ignores;
use crate::index::{IEntry, Index};
use crate::merge::{self, Opts as MergeOpts};
use crate::object::{self, Commit};
use crate::opts::{self, Spec};
use crate::os;
use crate::pathspec::Pathspec;
use crate::refs::Head;
use crate::repo::Repo;
use crate::worktree::{self, IgnoredMode, Scanner, UntrackedMode};
use crate::xmerge::Style;

const REF_STASH: &str = "refs/stash";

const PUSH_SPECS: &[Spec] = &[
    opts::flag(Some(b'k'), "keep-index", "keep-index"),
    opts::flag(Some(b'S'), "staged", "staged"),
    opts::flag(Some(b'p'), "patch", "patch"),
    opts::flag(Some(b'q'), "quiet", "quiet"),
    opts::flag(Some(b'u'), "include-untracked", "include-untracked"),
    opts::flag(Some(b'a'), "all", "all"),
    opts::value(Some(b'm'), "message", "message"),
    opts::value(None, "pathspec-from-file", "pathspec-from-file"),
    opts::flag(None, "pathspec-file-nul", "pathspec-file-nul"),
];

const SAVE_SPECS: &[Spec] = &[
    opts::flag(Some(b'k'), "keep-index", "keep-index"),
    opts::flag(Some(b'S'), "staged", "staged"),
    opts::flag(Some(b'p'), "patch", "patch"),
    opts::flag(Some(b'q'), "quiet", "quiet"),
    opts::flag(Some(b'u'), "include-untracked", "include-untracked"),
    opts::flag(Some(b'a'), "all", "all"),
    opts::value(Some(b'm'), "message", "message"),
];

const APPLY_SPECS: &[Spec] = &[opts::flag(Some(b'q'), "quiet", "quiet"), opts::flag(None, "index", "index")];
const DROP_SPECS: &[Spec] = &[opts::flag(Some(b'q'), "quiet", "quiet")];
const STORE_SPECS: &[Spec] = &[opts::flag(Some(b'q'), "quiet", "quiet"), opts::value(Some(b'm'), "message", "message")];

/// O que se sabe de uma entrada do stash (o `stash_info`).
struct Info {
    w_commit: Oid,
    b_commit: Oid,
    w_tree: Oid,
    b_tree: Oid,
    i_tree: Oid,
    u_tree: Option<Oid>,
    revision: String,
    is_stash_ref: bool,
}

/// `Err(code)` quando o comando já imprimiu o motivo e só falta sair.
type Step<T> = R<Result<T, i32>>;

fn say(msg: &str) {
    os::errs(&format!("{msg}\n"));
}

// ---- resolução de entradas ---------------------------------------------------------------------

/// `<nome>@{<n>}` com `n` numérico: o nome e o número.
fn reflog_selector(rev: &str) -> Option<(&str, usize)> {
    let at = rev.find("@{")?;
    let inner = rev[at + 2..].strip_suffix('}')?;
    if inner.is_empty() || !inner.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((&rev[..at], inner.parse().ok()?))
}

fn full_ref_name(repo: &Repo, name: &str) -> R<String> {
    if name.starts_with("refs/") {
        return Ok(name.to_string());
    }
    Ok(repo.dwim_ref_resolved(name)?.unwrap_or_else(|| format!("refs/{name}")))
}

fn resolve_revision(repo: &Repo, rev: &str) -> R<Option<Oid>> {
    if let Some((name, n)) = reflog_selector(rev) {
        let full = full_ref_name(repo, name)?;
        let entries = repo.read_reflog(&full);
        return match entries.len().checked_sub(n + 1) {
            Some(i) => Ok(Some(entries[i].new)),
            None => Err(Fail::Fatal(format!("log for '{name}' only has {} entries", entries.len()))),
        };
    }
    repo.rev_parse(rev.as_bytes())
}

/// O `get_stash_info`.
fn get_stash_info(repo: &Repo, args: &[String]) -> Step<Info> {
    if args.len() > 1 {
        let shown: String = args.iter().map(|a| format!(" '{a}'")).collect();
        say(&format!("Too many revisions specified:{shown}"));
        return Ok(Err(1));
    }
    let revision = match args.first() {
        None => {
            if repo.read_ref(REF_STASH)?.is_none() {
                say("No stash entries found.");
                return Ok(Err(1));
            }
            format!("{REF_STASH}@{{0}}")
        }
        Some(c) if !c.is_empty() && c.bytes().all(|b| b.is_ascii_digit()) => format!("{REF_STASH}@{{{c}}}"),
        Some(c) => c.clone(),
    };
    let Some(w_commit) = resolve_revision(repo, &revision)? else {
        error(&format!("{revision} is not a valid reference"));
        return Ok(Err(1));
    };
    let like = || Fail::Fatal(format!("'{revision}' is not a stash-like commit"));
    let c = repo.read_commit(&w_commit).map_err(|_| like())?;
    if c.parents.len() < 2 {
        return Err(like());
    }
    let b_commit = c.parents[0];
    let w_tree = c.tree;
    let b_tree = repo.tree_of(&b_commit).map_err(|_| like())?;
    let i_tree = repo.tree_of(&c.parents[1]).map_err(|_| like())?;
    let u_tree = match c.parents.get(2) {
        Some(u) => Some(repo.tree_of(u)?),
        None => None,
    };
    let symbolic = revision.split('@').next().unwrap_or("");
    let is_stash_ref = match repo.dwim_ref_resolved(symbolic)? {
        Some(full) => full == REF_STASH,
        None => symbolic == REF_STASH,
    };
    Ok(Ok(Info { w_commit, b_commit, w_tree, b_tree, i_tree, u_tree, revision, is_stash_ref }))
}

fn get_stash_info_assert(repo: &Repo, args: &[String]) -> Step<Info> {
    match get_stash_info(repo, args)? {
        Ok(info) => {
            if !info.is_stash_ref {
                error(&format!("'{}' is not a stash reference", info.revision));
                return Ok(Err(1));
            }
            Ok(Ok(info))
        }
        Err(c) => Ok(Err(c)),
    }
}

// ---- clear, drop e store -----------------------------------------------------------------------

fn do_clear_stash(repo: &Repo) -> R<()> {
    if repo.read_ref(REF_STASH)?.is_some() {
        repo.delete_ref(REF_STASH, None)?;
    }
    Ok(())
}

fn do_drop_stash(repo: &Repo, info: &Info, quiet: bool) -> R<i32> {
    let Some((name, n)) = reflog_selector(&info.revision) else {
        error(&format!("{}: Could not drop stash entry", info.revision));
        return Ok(1);
    };
    let full = full_ref_name(repo, name)?;
    let mut entries = repo.read_reflog(&full);
    let Some(pos) = entries.len().checked_sub(n + 1) else {
        error(&format!("{}: Could not drop stash entry", info.revision));
        return Ok(1);
    };
    entries.remove(pos);
    // O `EXPIRE_REFLOGS_REWRITE`: cada entrada que sobra passa a partir da anterior.
    let mut prev = Oid::ZERO;
    for e in entries.iter_mut() {
        e.old = prev;
        prev = e.new;
    }
    repo.write_reflog(&full, &entries)?;
    if let Some(last) = entries.last() {
        repo.set_ref_no_log(&full, last.new)?;
    }
    if !quiet {
        os::outs(&format!("Dropped {} ({})\n", info.revision, info.w_commit));
    }
    if repo.read_reflog(REF_STASH).is_empty() {
        do_clear_stash(repo)?;
    }
    Ok(0)
}

/// O `do_store_stash`: a ref e a linha de reflog (que o git cria mesmo fora das regras de sempre).
fn do_store_stash(repo: &Repo, w_commit: &Oid, msg: Option<&str>) -> R<()> {
    let like = || Fail::Fatal(format!("'{}' is not a stash-like commit", w_commit));
    let c = repo.read_commit(w_commit).map_err(|_| like())?;
    if c.parents.len() < 2 {
        return Err(like());
    }
    let msg = msg.unwrap_or("Created via \"git stash store\".");
    let old = repo.ref_oid(REF_STASH)?;
    repo.set_ref_no_log(REF_STASH, *w_commit)?;
    repo.append_reflog(REF_STASH, old.unwrap_or(Oid::ZERO), *w_commit, msg, true)
}

// ---- criação -----------------------------------------------------------------------------------

fn trust_exec(repo: &Repo) -> bool {
    repo.config.get_bool("core.filemode").ok().flatten().unwrap_or(true)
}

/// Há mudança nos arquivos rastreados (HEAD contra o índice, índice contra a árvore de trabalho)?
/// Sem commit nenhum, conta como mudança (o `check_changes_tracked_files` devolve -1).
fn tracked_changes(repo: &Repo, idx: &Index, ps: &Pathspec) -> R<bool> {
    let Some(head) = repo.head_oid()? else { return Ok(true) };
    let ht = repo.tree_of(&head)?;
    let not_sub = |p: &diff::Pair| !object::is_gitlink(p.one.mode) && !object::is_gitlink(p.two.mode);
    let staged = diff::diff_tree_index(repo, Some(&ht), idx, ps)?;
    if staged.iter().any(not_sub) {
        return Ok(true);
    }
    Ok(diff::diff_index_worktree(repo, idx, ps)?.iter().any(not_sub))
}

/// Os arquivos não rastreados (`-u`) ou também os ignorados (`-a`, `all`).
fn untracked_files(repo: &Repo, idx: &Index, ps: &Pathspec, all: bool) -> Vec<Vec<u8>> {
    let ignored = if all { IgnoredMode::Matching } else { IgnoredMode::No };
    let mut sc = Scanner { idx, ign: Ignores::standard(repo), ps, untracked: UntrackedMode::All, ignored, show_empty_dirs: false };
    let scan = sc.run();
    let mut out = scan.untracked;
    if all {
        for p in scan.ignored {
            if !p.ends_with(b"/") {
                out.push(p);
            }
        }
    }
    out.retain(|p| !p.ends_with(b"/"));
    out.sort();
    out.dedup();
    out
}

fn make_commit(repo: &Repo, tree: Oid, parents: Vec<Oid>, message: Vec<u8>) -> R<Oid> {
    let author = ident::ident(&repo.config, Who::Author, true)?;
    let committer = ident::ident(&repo.config, Who::Committer, true)?;
    let c = Commit { tree, parents, author: author.to_bytes(), committer: committer.to_bytes(), encoding: None, extra: Vec::new(), message };
    repo.write_object(Kind::Commit, &object::encode_commit(&c))
}

/// A árvore dos arquivos não rastreados (o `save_untracked_files`).
fn untracked_tree(repo: &Repo, files: &[Vec<u8>]) -> R<Oid> {
    let mut entries: Vec<(Vec<u8>, u32, Oid)> = Vec::new();
    for path in files {
        let st = os::lstat(path).map_err(|e| Fail::Fatal(format!("could not stat '{}': {}", os::lossy(path), e.message())))?;
        let data = diff::worktree_blob(path, &st)?;
        let oid = repo.write_object(Kind::Blob, &data)?;
        entries.push((path.clone(), os::git_mode_of(&st), oid));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let refs: Vec<(&[u8], u32, Oid)> = entries.iter().map(|(p, m, o)| (p.as_slice(), *m, *o)).collect();
    repo.write_tree_entries(&refs)
}

/// A árvore do stash: o índice mais, para cada arquivo que difere do HEAD na árvore de trabalho, o
/// conteúdo de lá (o `stash_working_tree`).
fn working_tree(repo: &Repo, idx: &Index, head_tree: &Oid, ps: &Pathspec) -> R<Oid> {
    let trust = trust_exec(repo);
    let head_files = repo.flatten_tree(head_tree)?;
    let mut w_idx = idx.clone();
    for e in &idx.entries {
        if e.stage != 0 || !ps.matches_simple(&e.path) || object::is_gitlink(e.mode) {
            continue;
        }
        let wt: Option<(u32, Oid)> = match diff::check_entry(repo, idx, e, trust)? {
            WtState::Same => Some((e.mode, e.oid)),
            WtState::Deleted => None,
            WtState::Changed(m, o) => Some((m, o)),
        };
        let same_as_head = match (head_files.get(&e.path), wt) {
            (None, None) => true,
            (Some(&(hm, ho)), Some((m, o))) => object::canon_mode(hm) == object::canon_mode(m) && ho == o,
            _ => false,
        };
        if same_as_head {
            continue;
        }
        match wt {
            None => {
                w_idx.remove(&e.path);
            }
            Some(_) => add::stage_path(repo, &mut w_idx, &e.path, trust, None, false)?,
        }
    }
    repo.write_tree_from_index(&w_idx)
}

struct Created {
    w_commit: Oid,
    i_tree: Oid,
    message: String,
}

/// O `do_create_stash`. `Err(1)` quando não há o que guardar ou algo falhou (já impresso).
fn do_create_stash(repo: &Repo, ps: &Pathspec, given: &str, include_untracked: u8, only_staged: bool, quiet: bool) -> Step<Created> {
    let ipath = repo.index_path();
    let idx = Index::load(&ipath)?;
    let Some(head) = repo.head_oid()? else {
        if !quiet {
            say("You do not have the initial commit yet");
        }
        return Ok(Err(1));
    };
    let untracked: Vec<Vec<u8>> = if include_untracked > 0 { untracked_files(repo, &idx, ps, include_untracked == 2) } else { Vec::new() };
    if !tracked_changes(repo, &idx, ps)? && untracked.is_empty() {
        return Ok(Err(1));
    }
    let branch_name = match repo.head()? {
        Head::Branch(name, _) => name.strip_prefix("refs/heads/").unwrap_or(&name).to_string(),
        Head::Detached(_) => "(no branch)".to_string(),
    };
    let head_commit = repo.read_commit(&head)?;
    let head_tree = head_commit.tree;
    let (subject, _) = object::subject_with(&head_commit.message, b" ");
    let msg = format!("{branch_name}: {} {}", repo.abbrev_default(&head), os::lossy(&subject));

    if idx.has_conflicts() {
        if !quiet {
            say("Cannot save the current index state");
        }
        return Ok(Err(1));
    }
    let i_tree = repo.write_tree_from_index(&idx)?;
    let i_commit = make_commit(repo, i_tree, vec![head], format!("index on {msg}\n").into_bytes())?;

    let u_commit = if include_untracked > 0 {
        let u_tree = untracked_tree(repo, &untracked)?;
        Some(make_commit(repo, u_tree, Vec::new(), format!("untracked files on {msg}\n").into_bytes())?)
    } else {
        None
    };

    let w_tree = if only_staged {
        let staged = diff::diff_tree_index(repo, Some(&head_tree), &idx, &Pathspec::default())?;
        if staged.is_empty() {
            if !quiet {
                say("No staged changes");
            }
            return Ok(Err(1));
        }
        i_tree
    } else {
        working_tree(repo, &idx, &head_tree, ps)?
    };

    let message = if given.is_empty() { format!("WIP on {msg}") } else { format!("On {branch_name}: {given}") };
    let mut parents = vec![head, i_commit];
    parents.extend(u_commit);
    let w_commit = make_commit(repo, w_tree, parents, message.clone().into_bytes())?;
    Ok(Ok(Created { w_commit, i_tree, message }))
}

// ---- push --------------------------------------------------------------------------------------

/// Devolve o caminho a um arquivo do índice ao estado do HEAD (índice e árvore de trabalho).
fn restore_to_head(repo: &Repo, idx: &mut Index, head_files: &BTreeMap<Vec<u8>, (u32, Oid)>, ps: &Pathspec) -> R<()> {
    let trust = trust_exec(repo);
    let mut paths: Vec<Vec<u8>> = Vec::new();
    for e in &idx.entries {
        if e.stage == 0 && ps.matches_simple(&e.path) && paths.last() != Some(&e.path) {
            paths.push(e.path.clone());
        }
    }
    for p in head_files.keys() {
        if ps.matches_simple(p) && !paths.contains(p) {
            paths.push(p.clone());
        }
    }
    for path in paths {
        let in_head = head_files.get(&path).copied();
        let entry = idx.get(&path).cloned();
        match (in_head, entry) {
            (Some((m, o)), Some(e)) => {
                let clean = e.mode == m && e.oid == o && matches!(diff::check_entry(repo, idx, &e, trust)?, WtState::Same);
                if !clean {
                    idx.add(worktree::checkout_entry(repo, &path, m, &o)?);
                }
            }
            (Some((m, o)), None) => idx.add(worktree::checkout_entry(repo, &path, m, &o)?),
            (None, Some(_)) => {
                idx.remove(&path);
                worktree::unlink_entry(&path);
            }
            (None, None) => {}
        }
    }
    Ok(())
}

/// O `checkout --no-overlay <árvore> -- <caminhos>`: índice e árvore de trabalho dos caminhos
/// passam a ser os da árvore.
fn restore_from_tree(repo: &Repo, idx: &mut Index, tree: &Oid, ps: &Pathspec) -> R<()> {
    let files = repo.flatten_tree(tree)?;
    let mut gone: Vec<Vec<u8>> = Vec::new();
    for e in &idx.entries {
        if e.stage == 0 && ps.matches_simple(&e.path) && !files.contains_key(&e.path) && gone.last() != Some(&e.path) {
            gone.push(e.path.clone());
        }
    }
    for path in gone {
        idx.remove(&path);
        worktree::unlink_entry(&path);
    }
    for (path, &(m, o)) in &files {
        if !ps.matches_simple(path) {
            continue;
        }
        let same = idx.get(path).is_some_and(|e| e.mode == m && e.oid == o);
        if !same || os::lstat(path).is_err() {
            idx.add(worktree::checkout_entry(repo, path, m, &o)?);
        }
    }
    Ok(())
}

struct PushArgs {
    message: Option<String>,
    quiet: bool,
    keep_index: i32,
    patch: bool,
    include_untracked: u8,
    only_staged: bool,
}

/// O `do_push_stash`.
fn do_push_stash(git: &Git, ps: &Pathspec, a: &PushArgs) -> R<i32> {
    let repo = git.repo()?;
    let mut keep_index = a.keep_index;
    let mut only_staged = a.only_staged;
    if a.patch && keep_index == -1 {
        keep_index = 1;
    }
    if a.patch && a.include_untracked > 0 {
        say("Can't use --patch and --include-untracked or --all at the same time");
        return Ok(1);
    }
    if a.patch {
        only_staged = false;
        return Err(Fail::Fatal("interactive stash needs a terminal; this sandbox has none".into()));
    }
    if only_staged && a.include_untracked > 0 {
        say("Can't use --staged and --include-untracked or --all at the same time");
        return Ok(1);
    }
    let ipath = repo.index_path();
    let mut idx = Index::load(&ipath)?;
    let trust = trust_exec(repo);
    if a.include_untracked == 0 && !ps.items.is_empty() {
        let mut seen = vec![false; ps.items.len()];
        for e in &idx.entries {
            let _ = ps.matches(&e.path, false, Some(&mut seen));
        }
        if let Some(it) = ps.unmatched(&seen).first() {
            error(&format!("pathspec '{}' did not match any file(s) known to git", os::lossy(&it.orig)));
            say("Did you forget to 'git add'?");
            return Ok(1);
        }
    }
    if worktree::refresh(&mut idx, trust) {
        idx.write(&ipath)?;
    }
    let untracked: Vec<Vec<u8>> = if a.include_untracked > 0 { untracked_files(repo, &idx, ps, a.include_untracked == 2) } else { Vec::new() };
    if !tracked_changes(repo, &idx, ps)? && untracked.is_empty() {
        if !a.quiet {
            os::outs("No local changes to save\n");
        }
        return Ok(0);
    }

    let given = a.message.clone().unwrap_or_default();
    let created = match do_create_stash(repo, ps, &given, a.include_untracked, only_staged, a.quiet)? {
        Ok(c) => c,
        Err(code) => return Ok(code),
    };
    do_store_stash(repo, &created.w_commit, Some(&created.message))?;
    if !a.quiet {
        os::outs(&format!("Saved working directory and index state {}\n", created.message));
    }

    let head = repo.head_oid()?;
    let head_tree = match head {
        Some(h) => repo.tree_of(&h)?,
        None => EMPTY_TREE,
    };
    let head_files = repo.flatten_tree(&head_tree)?;
    let mut idx = Index::load(&ipath)?;
    if only_staged {
        // Desfaz na árvore de trabalho o que estava preparado e tira o índice de lá.
        let staged = diff::diff_tree_index(repo, Some(&head_tree), &idx, &Pathspec::default())?;
        for p in &staged {
            let path = p.path().to_vec();
            if let Some(e) = idx.get(&path).cloned()
                && !matches!(diff::check_entry(repo, &idx, &e, trust)?, WtState::Same)
            {
                error(&format!("{}: does not match index", os::lossy(&path)));
                say("Cannot remove worktree changes");
                return Ok(1);
            }
        }
        if keep_index < 1 {
            restore_to_head(repo, &mut idx, &head_files, &Pathspec::default())?;
        }
        idx.write(&ipath)?;
        return Ok(0);
    }
    if a.include_untracked > 0 {
        for p in &untracked {
            if ps.items.is_empty() || ps.matches_simple(p) {
                worktree::unlink_entry(p);
            }
        }
    }
    if ps.items.is_empty() {
        let uo = UnpackOpts { verb: "reset", advice: "reset", force: true };
        let ni = unpack::switch_tree(repo, &idx, Some(&head_tree), &head_tree, &uo)?;
        ni.write(&ipath)?;
        super::revert::remove_branch_state(repo, false);
    } else {
        restore_to_head(repo, &mut idx, &head_files, ps)?;
        idx.write(&ipath)?;
    }
    if keep_index == 1 && created.i_tree != EMPTY_TREE {
        let mut idx = Index::load(&ipath)?;
        if ps.items.is_empty() {
            let uo = UnpackOpts { verb: "reset", advice: "reset", force: true };
            let ni = unpack::switch_tree(repo, &idx, Some(&head_tree), &created.i_tree, &uo)?;
            ni.write(&ipath)?;
        } else {
            restore_from_tree(repo, &mut idx, &created.i_tree, ps)?;
            idx.write(&ipath)?;
        }
    }
    Ok(0)
}

fn push_args(p: &opts::Parsed) -> PushArgs {
    let mut include_untracked = 0u8;
    for h in &p.hits {
        match (h.id, h.negated) {
            ("include-untracked", false) => include_untracked = 1,
            ("include-untracked", true) => include_untracked = 0,
            ("all", false) => include_untracked = 2,
            ("all", true) => include_untracked = 0,
            _ => {}
        }
    }
    PushArgs {
        message: p.value_str("message"),
        quiet: p.has("quiet"),
        keep_index: match p.flag("keep-index") {
            Some(true) => 1,
            Some(false) => 0,
            None => -1,
        },
        patch: p.has("patch"),
        include_untracked,
        only_staged: p.has("staged"),
    }
}

fn push_stash(git: &mut Git, args: &[Vec<u8>], push_assumed: bool) -> R<i32> {
    let usage = git.usage();
    let force_assume = args.first().is_some_and(|a| a == b"-p");
    let p = opts::parse(PUSH_SPECS, args, opts::KEEP_DASHDASH, usage)?;
    let mut rest: Vec<Vec<u8>> = p.args.clone();
    if let Some(first) = rest.first() {
        if first == b"--" {
            rest.remove(0);
        } else if push_assumed && !force_assume {
            return Err(Fail::Fatal(format!(
                "subcommand wasn't specified; 'push' can't be assumed due to unexpected token '{}'",
                os::lossy(first)
            )));
        }
    }
    let a = push_args(&p);
    let mut paths = rest;
    if let Some(f) = p.value("pathspec-from-file") {
        if a.patch {
            return Err(Fail::Fatal("options '--pathspec-from-file' and '--patch' cannot be used together".into()));
        }
        if a.only_staged {
            return Err(Fail::Fatal("options '--pathspec-from-file' and '--staged' cannot be used together".into()));
        }
        if !paths.is_empty() {
            return Err(Fail::Fatal("'--pathspec-from-file' and pathspec arguments cannot be used together".into()));
        }
        let data = if f == b"-" { os::stdin_all() } else { super::plumbing::read_user_file(git.repo()?, f)? };
        let sep = if p.has("pathspec-file-nul") { 0 } else { b'\n' };
        paths.extend(data.split(|c| *c == sep).filter(|l| !l.is_empty()).map(|l| l.to_vec()));
    } else if p.has("pathspec-file-nul") {
        return Err(Fail::Fatal("the option '--pathspec-file-nul' requires '--pathspec-from-file'".into()));
    }
    let ps = git.pathspec(&paths)?;
    do_push_stash(git, &ps, &a)
}

fn save_stash(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let p = opts::parse(SAVE_SPECS, args, opts::KEEP_DASHDASH, usage)?;
    let mut a = push_args(&p);
    let mut words: Vec<Vec<u8>> = p.args.clone();
    if words.first().is_some_and(|w| w == b"--") {
        words.remove(0);
    }
    if !words.is_empty() {
        let joined: Vec<String> = words.iter().map(|w| os::lossy(w)).collect();
        a.message = Some(joined.join(" "));
    }
    do_push_stash(git, &Pathspec::default(), &a)
}

// ---- create e store ----------------------------------------------------------------------------

fn create_stash(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let repo = git.repo()?;
    let words: Vec<String> = args.iter().map(|a| os::lossy(a)).collect();
    let idx = Index::load(&repo.index_path())?;
    let ps = Pathspec::default();
    if !tracked_changes(repo, &idx, &ps)? {
        return Ok(0);
    }
    match do_create_stash(repo, &ps, &words.join(" "), 0, false, false)? {
        Ok(c) => {
            os::outs(&format!("{}\n", c.w_commit));
            Ok(0)
        }
        Err(code) => Ok(code),
    }
}

fn store_stash(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let p = opts::parse(STORE_SPECS, args, opts::KEEP_UNKNOWN, usage)?;
    let repo = git.repo()?;
    let quiet = p.has("quiet");
    if p.args.len() != 1 {
        if !quiet {
            say("\"git stash store\" requires one <commit> argument");
        }
        return Ok(1);
    }
    let Some(oid) = repo.rev_parse(&p.args[0])? else {
        if !quiet {
            say(&format!("Cannot update {REF_STASH} with {}", os::lossy(&p.args[0])));
        }
        return Ok(1);
    };
    do_store_stash(repo, &oid, p.value_str("message").as_deref())?;
    Ok(0)
}

// ---- apply, pop e branch -----------------------------------------------------------------------

/// O `restore_untracked`: devolve os arquivos não rastreados sem sobrescrever nada.
fn restore_untracked(repo: &Repo, u_tree: &Oid) -> R<bool> {
    let files = repo.flatten_tree(u_tree)?;
    let mut ok = true;
    for (path, &(m, o)) in &files {
        if os::lstat(path).is_ok() {
            error(&format!("{} already exists, no checkout", os::lossy(path)));
            ok = false;
            continue;
        }
        worktree::checkout_entry(repo, path, m, &o)?;
    }
    Ok(ok)
}

/// O `unstage_changes_unless_new`: o que a mesclagem preparou volta a ser mudança não preparada,
/// menos os arquivos que `c_tree` não tinha.
fn unstage_changes_unless_new(repo: &Repo, c_tree: &Oid) -> R<()> {
    let ipath = repo.index_path();
    let mut idx = Index::load(&ipath)?;
    let pairs = diff::diff_tree_index(repo, Some(c_tree), &idx, &Pathspec::default())?;
    for p in pairs {
        if p.one.valid() {
            idx.add(IEntry::bare(p.one.path.clone(), p.one.oid, p.one.mode));
        }
    }
    idx.write(&ipath)
}

/// O `do_apply_stash`. `Ok(0)` limpo, `Ok(1)` com conflito ou falha (já impresso).
fn do_apply_stash(git: &mut Git, info: &Info, index: bool, quiet: bool) -> R<i32> {
    let mut ret = 0;
    let mut has_index = index;
    let c_tree: Oid;
    let mut index_tree = EMPTY_TREE;
    let head_tree: Oid;
    {
        let repo = git.repo()?;
        let ipath = repo.index_path();
        let mut idx = Index::load(&ipath)?;
        if worktree::refresh(&mut idx, trust_exec(repo)) {
            idx.write(&ipath)?;
        }
        if idx.has_conflicts() {
            error("cannot apply a stash in the middle of a merge");
            return Ok(1);
        }
        c_tree = repo.write_tree_from_index(&idx)?;
        head_tree = match repo.head_oid()? {
            Some(h) => repo.tree_of(&h)?,
            None => EMPTY_TREE,
        };
        if index {
            if info.b_tree == info.i_tree || c_tree == info.i_tree {
                has_index = false;
            } else {
                // O que estava preparado vai pro índice atual, se ele ainda for o da base.
                let staged = diff::diff_trees(repo, Some(&info.b_tree), Some(&info.i_tree), &Pathspec::default())?;
                let mut next = idx.clone();
                for p in &staged {
                    let path = p.path().to_vec();
                    let cur = idx.get(&path).map(|e| (e.mode, e.oid));
                    let before = if p.one.valid() { Some((p.one.mode, p.one.oid)) } else { None };
                    let after = if p.two.valid() { Some((p.two.mode, p.two.oid)) } else { None };
                    if cur != before && cur != after {
                        error("conflicts in index. Try without --index.");
                        return Ok(1);
                    }
                    match after {
                        Some((m, o)) => next.add(IEntry::bare(path, o, m)),
                        None => {
                            next.remove(&path);
                        }
                    }
                }
                index_tree = repo.write_tree_from_index(&next)?;
                // `reset --quiet --refresh`: o índice volta ao HEAD, a árvore de trabalho fica.
                let head_idx = repo.index_from_tree(&head_tree)?;
                head_idx.write(&ipath)?;
            }
        }

        let mut mo = MergeOpts::new("Updated upstream", "Stashed changes", "Stash base");
        if info.b_tree == c_tree {
            mo.branch1 = "Version stash was based on".to_string();
        }
        mo.style = match repo.config.get("merge.conflictstyle").as_deref() {
            Some("diff3") => Style::Diff3,
            Some("zdiff3") => Style::ZealousDiff3,
            _ => Style::Merge,
        };
        let outcome = merge::merge_trees(repo, &mo, 0, &info.b_tree, &c_tree, &info.w_tree)?;
        let base_for_checkout = if has_index { head_tree } else { c_tree };
        match merge::checkout_result(repo, &base_for_checkout, &outcome) {
            Ok(()) => {
                if !quiet {
                    outcome.display();
                }
                if !outcome.clean {
                    ret = 1;
                }
            }
            Err(Fail::Exit(1)) => ret = -1,
            Err(e) => return Err(e),
        }
        if ret != 0 {
            if index {
                say("Index was not unstashed.");
            }
        } else if has_index {
            let ni = repo.index_from_tree(&index_tree)?;
            ni.write(&ipath)?;
        } else {
            unstage_changes_unless_new(repo, &c_tree)?;
        }
        if let Some(u) = &info.u_tree
            && !restore_untracked(repo, u)?
        {
            error("could not restore untracked files from stash");
            ret = 1;
        }
    }
    if !quiet {
        super::status::run(git, &[])?;
    }
    Ok(if ret != 0 { 1 } else { 0 })
}

fn apply_stash(git: &mut Git, args: &[Vec<u8>], pop: bool) -> R<i32> {
    let usage = git.usage();
    let p = opts::parse(APPLY_SPECS, args, 0, usage)?;
    let quiet = p.has("quiet");
    let revs: Vec<String> = p.args.iter().map(|a| os::lossy(a)).collect();
    let info = {
        let repo = git.repo()?;
        let got = if pop { get_stash_info_assert(repo, &revs)? } else { get_stash_info(repo, &revs)? };
        match got {
            Ok(i) => i,
            Err(code) => return Ok(code),
        }
    };
    let res = do_apply_stash(git, &info, p.has("index"), quiet)?;
    if !pop {
        return Ok(res);
    }
    if res != 0 {
        os::outs("The stash entry is kept in case you need it again.\n");
        return Ok(1);
    }
    do_drop_stash(git.repo()?, &info, quiet)
}

fn drop_stash(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let p = opts::parse(DROP_SPECS, args, 0, usage)?;
    let repo = git.repo()?;
    let revs: Vec<String> = p.args.iter().map(|a| os::lossy(a)).collect();
    match get_stash_info_assert(repo, &revs)? {
        Ok(info) => do_drop_stash(repo, &info, p.has("quiet")),
        Err(code) => Ok(code),
    }
}

fn branch_stash(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let p = opts::parse(&[], args, 0, usage)?;
    let Some(branch) = p.args.first() else {
        say("No branch name specified");
        return Ok(1);
    };
    let branch = branch.clone();
    let revs: Vec<String> = p.args[1..].iter().map(|a| os::lossy(a)).collect();
    let info = match get_stash_info(git.repo()?, &revs)? {
        Ok(i) => i,
        Err(code) => return Ok(code),
    };
    let res = super::checkout::run_checkout(git, &[b"-b".to_vec(), branch, info.b_commit.hex().into_bytes()])?;
    if res != 0 {
        return Ok(res);
    }
    let res = do_apply_stash(git, &info, true, false)?;
    if res != 0 {
        return Ok(res);
    }
    if info.is_stash_ref {
        return do_drop_stash(git.repo()?, &info, false);
    }
    Ok(0)
}

// ---- list e show -------------------------------------------------------------------------------

fn list_stash(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let p = opts::parse(&[], args, opts::KEEP_UNKNOWN, usage)?;
    let repo = git.repo()?;
    if repo.read_ref(REF_STASH)?.is_none() {
        return Ok(0);
    }
    if !p.unknown.is_empty() || !p.args.is_empty() {
        return Err(Fail::Fatal("'git stash list' with log options is not supported by this git".into()));
    }
    let entries = repo.read_reflog(REF_STASH);
    let mut out = String::new();
    for (n, e) in entries.iter().rev().enumerate() {
        out.push_str(&format!("stash@{{{n}}}: {}\n", os::lossy(&e.message)));
    }
    os::outs(&out);
    Ok(0)
}

fn show_stash(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let mut specs: Vec<Spec> = diff_cmd::DIFF_SPECS.iter().filter(|s| !(s.short == Some(b'u') && s.long.is_none())).copied().collect();
    specs.push(opts::flag(Some(b'u'), "include-untracked", "include-untracked"));
    specs.push(opts::noneg(opts::flag(None, "only-untracked", "only-untracked")));
    let p = opts::parse(&specs, args, opts::KEEP_UNKNOWN | opts::KEEP_DASHDASH, usage)?;
    let revs: Vec<String> = p.args.iter().map(|a| os::lossy(a)).collect();
    let info = match get_stash_info(git.repo()?, &revs)? {
        Ok(i) => i,
        Err(code) => return Ok(code),
    };
    let cfg = git.config();
    let show_stat = cfg.get_bool("stash.showstat")?.unwrap_or(true);
    let show_patch = cfg.get_bool("stash.showpatch")?.unwrap_or(false);
    let mut untracked = if cfg.get_bool("stash.showincludeuntracked")?.unwrap_or(false) { 1 } else { 0 };
    for h in &p.hits {
        match h.id {
            "include-untracked" => untracked = 1,
            "only-untracked" => untracked = 2,
            _ => {}
        }
    }
    let given_opts = p.hits.iter().any(|h| !matches!(h.id, "include-untracked" | "only-untracked")) || !p.unknown.is_empty();
    let mut o = diff_cmd::build_opts(git, &p, false, true)?;
    if !given_opts {
        if !show_stat && !show_patch {
            return Ok(0);
        }
        o.stat = show_stat;
        o.patch = show_patch;
    }
    let repo = git.repo()?;
    let ps = Pathspec::default();
    let pairs = match untracked {
        2 => match &info.u_tree {
            Some(u) => diff::diff_trees(repo, None, Some(u), &ps)?,
            None => Vec::new(),
        },
        1 if info.u_tree.is_some() => {
            let u = info.u_tree.unwrap_or(EMPTY_TREE);
            let mut files = repo.flatten_tree(&info.w_tree)?;
            files.extend(repo.flatten_tree(&u)?);
            let refs: Vec<(&[u8], u32, Oid)> = files.iter().map(|(p, (m, o))| (p.as_slice(), *m, *o)).collect();
            let union = repo.write_tree_entries(&refs)?;
            diff::diff_trees(repo, Some(&info.b_tree), Some(&union), &ps)?
        }
        _ => diff::diff_trees(repo, Some(&info.b_tree), Some(&info.w_tree), &ps)?,
    };
    let pairs = diff::postprocess(repo, pairs, &o)?;
    diff::emit(repo, &pairs, &o)?;
    Ok(0)
}

// ---- o comando ---------------------------------------------------------------------------------

fn clear_stash(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let p = opts::parse(&[], args, opts::STOP_AT_NON_OPTION, usage)?;
    if !p.args.is_empty() {
        error("git stash clear with arguments is unimplemented");
        return Ok(1);
    }
    do_clear_stash(git.repo()?)?;
    Ok(0)
}

pub fn run(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let Some(first) = args.first() else {
        let code = push_stash(git, &[], false)?;
        return Ok(i32::from(code != 0));
    };
    let rest = &args[1..];
    let code = match first.as_slice() {
        b"apply" => apply_stash(git, rest, false)?,
        b"pop" => apply_stash(git, rest, true)?,
        b"clear" => clear_stash(git, rest)?,
        b"drop" => drop_stash(git, rest)?,
        b"branch" => branch_stash(git, rest)?,
        b"list" => list_stash(git, rest)?,
        b"show" => show_stash(git, rest)?,
        b"store" => store_stash(git, rest)?,
        b"create" => create_stash(git, rest)?,
        b"push" => push_stash(git, rest, false)?,
        b"save" => save_stash(git, rest)?,
        _ => push_stash(git, args, true)?,
    };
    Ok(i32::from(code != 0))
}
