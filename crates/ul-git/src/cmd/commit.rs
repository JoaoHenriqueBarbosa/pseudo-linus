//! `git commit`: monta o índice a gravar (`-a`, caminhos), obtém a mensagem (`-m`, `-F`, editor,
//! `--amend`, MERGE_MSG), cria o objeto, move o ramo e imprime o resumo.

use super::Git;
use super::add;
use super::status;
use crate::date::{self, DateMode};
use crate::diff::rename::RenameOpts;
use crate::diff::{self, DiffOpts, WtState};
use crate::editor;
use crate::error::{Fail, R, error, hint};
use crate::hash::{Kind, Oid};
use crate::ident::{self, Who};
use crate::index::{IEntry, Index};
use crate::msg::{self, Cleanup};
use crate::object::{self, Commit, Ident};
use crate::opts::{self, Spec};
use crate::os;
use crate::pathspec::Pathspec;
use crate::refs::Head;
use crate::repo::Repo;

const SPECS: &[Spec] = &[
    opts::value(Some(b'm'), "message", "message"),
    opts::value(Some(b'F'), "file", "file"),
    opts::flag(Some(b'a'), "all", "all"),
    opts::flag(Some(b'q'), "quiet", "quiet"),
    opts::flag(None, "amend", "amend"),
    opts::flag(None, "allow-empty", "allow-empty"),
    opts::flag(None, "allow-empty-message", "allow-empty-message"),
    opts::flag(Some(b'e'), "edit", "edit"),
    opts::value(None, "author", "author"),
    opts::value(None, "date", "date"),
    opts::flag(Some(b's'), "signoff", "signoff"),
    opts::flag(Some(b'n'), "no-verify", "no-verify"),
    opts::flag(None, "reset-author", "reset-author"),
    opts::value(None, "cleanup", "cleanup"),
    opts::flag(Some(b'o'), "only", "only"),
    opts::flag(Some(b'i'), "include", "include"),
    opts::flag(None, "status", "status"),
];

/// O aviso de um cherry-pick que ficou vazio (`empty_cherry_pick_advice` do git).
const EMPTY_CHERRY_PICK_ADVICE: &str = "The previous cherry-pick is now empty, possibly due to conflict resolution.\nIf you wish to commit it anyway, use:\n\n    git commit --allow-empty\n\n";
const EMPTY_CHERRY_PICK_ADVICE_SINGLE: &str = "Otherwise, please use 'git cherry-pick --skip'\n";
const EMPTY_CHERRY_PICK_ADVICE_MULTI: &str = "and then use:\n\n    git cherry-pick --continue\n\nto resume cherry-picking the remaining commits.\nIf you wish to skip this commit, use:\n\n    git cherry-pick --skip\n\n";

/// Pasta dos hooks (`core.hooksPath` ou `<repositório>/hooks`).
fn hooks_dir(repo: &Repo) -> Vec<u8> {
    match repo.config.get_bytes("core.hookspath") {
        Some(p) if p.starts_with(b"/") => p,
        Some(p) => os::absolute(&p),
        None => repo.common("hooks"),
    }
}

/// `-a`: o que mudou ou sumiu entre os arquivos rastreados entra no índice.
fn stage_tracked(repo: &Repo, idx: &mut Index) -> R<()> {
    let trust = repo.config.get_bool("core.filemode")?.unwrap_or(true);
    let entries: Vec<IEntry> = idx.entries.clone();
    let mut last: Option<Vec<u8>> = None;
    for e in &entries {
        if e.stage != 0 || last.as_deref() == Some(e.path.as_slice()) {
            continue;
        }
        last = Some(e.path.clone());
        match diff::check_entry(repo, idx, e, trust)? {
            WtState::Deleted => {
                idx.remove(&e.path);
            }
            WtState::Changed(..) => add::stage_path(repo, idx, &e.path, trust, None, false)?,
            WtState::Same => {}
        }
    }
    Ok(())
}

/// Caminhos nomeados: entram no índice do usuário o conteúdo atual (ou a remoção) e, sem `-i`, o
/// commit leva só eles em cima do HEAD. Devolve o índice do commit (`None` = o próprio índice).
fn stage_paths(repo: &Repo, idx: &mut Index, ps: &Pathspec, head_tree: Option<&Oid>, include: bool) -> R<Option<Index>> {
    let trust = repo.config.get_bool("core.filemode")?.unwrap_or(true);
    let head_idx = match head_tree {
        Some(t) => repo.index_from_tree(t)?,
        None => Index { version: 2, ..Index::default() },
    };
    let mut seen = vec![false; ps.items.len()];
    let mut matched: Vec<Vec<u8>> = Vec::new();
    for path in idx.entries.iter().map(|e| &e.path).chain(head_idx.entries.iter().map(|e| &e.path)) {
        if ps.matches(path, false, Some(&mut seen)).is_some() && !matched.contains(path) {
            matched.push(path.clone());
        }
    }
    if let Some(it) = ps.unmatched(&seen).first() {
        error(&format!("pathspec '{}' did not match any file(s) known to git", os::lossy(&it.orig)));
        return Err(Fail::Exit(1));
    }
    for path in &matched {
        if os::lstat(path).is_ok() {
            add::stage_path(repo, idx, path, trust, None, false)?;
        } else {
            idx.remove(path);
        }
    }
    if include {
        return Ok(None);
    }
    let mut commit_idx = head_idx;
    for path in &matched {
        match idx.get(path) {
            Some(e) => commit_idx.add(e.clone()),
            None => {
                commit_idx.remove(path);
            }
        }
    }
    Ok(Some(commit_idx))
}

/// `Signed-off-by:` no fim da mensagem: sem linha em branco se o último parágrafo já é de trailers.
pub fn append_signoff(msg: &mut Vec<u8>, who: &Ident) {
    let mut line = b"Signed-off-by: ".to_vec();
    line.extend_from_slice(&who.name_email());
    let keep = object::rtrim(msg).len();
    msg.truncate(keep);
    if msg.is_empty() {
        msg.extend_from_slice(b"\n\n");
        msg.extend_from_slice(&line);
        msg.push(b'\n');
        return;
    }
    let text = msg.clone();
    let paragraphs: Vec<&[u8]> = text.split(|c| *c == b'\n').collect();
    let last_break = paragraphs.iter().rposition(|l| l.is_empty());
    let tail: &[&[u8]] = match last_break {
        Some(i) => &paragraphs[i + 1..],
        None => &paragraphs[..],
    };
    let is_trailer = |l: &[u8]| {
        let Some(colon) = l.iter().position(|c| *c == b':') else { return false };
        colon > 0 && l[..colon].iter().all(|c| c.is_ascii_alphanumeric() || *c == b'-') && l.get(colon + 1).is_some_and(|c| c.is_ascii_whitespace())
    };
    let trailer_block = last_break.is_some() && !tail.is_empty() && tail.iter().all(|l| is_trailer(l));
    if tail.last().is_some_and(|l| *l == line.as_slice()) {
        msg.push(b'\n');
        return;
    }
    msg.push(b'\n');
    if !trailer_block {
        msg.push(b'\n');
    }
    msg.extend_from_slice(&line);
    msg.push(b'\n');
}

/// `--author="Nome <e-mail>"`.
fn parse_author(text: &[u8]) -> R<(Vec<u8>, Vec<u8>)> {
    let bad = || Fail::Fatal(format!("--author '{}' is not 'Name <email>' and matches no existing author", os::lossy(text)));
    let lt = text.iter().position(|c| *c == b'<').ok_or_else(bad)?;
    let gt = text[lt..].iter().position(|c| *c == b'>').map(|g| g + lt).ok_or_else(bad)?;
    let name = object::trim_ascii(&text[..lt]).to_vec();
    let email = text[lt + 1..gt].to_vec();
    if name.is_empty() {
        return Err(bad());
    }
    Ok((name, email))
}

pub fn run(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let p = opts::parse(SPECS, args, 0, usage)?;
    let repo = git.repo()?;
    let quiet = p.has("quiet");
    let amend = p.has("amend");
    let all = p.has("all");
    if all && !p.args.is_empty() {
        let names: Vec<String> = p.args.iter().map(|a| os::lossy(a)).collect();
        return Err(Fail::Fatal(format!("paths '{}' with -a does not make sense", names.join(" "))));
    }
    let merge_head = os::read_opt(&repo.path("MERGE_HEAD")).ok().flatten();
    // `CHERRY_PICK_HEAD`: um cherry-pick parou e este commit o conclui (autor e reflog diferentes).
    let cherry_pick_head: Option<Oid> = match os::read_opt(&repo.path("CHERRY_PICK_HEAD")).ok().flatten() {
        Some(data) if merge_head.is_none() => Oid::from_hex(object::trim_ascii(&data)),
        _ => None,
    };
    let cherry_pick = cherry_pick_head.is_some();
    let head = repo.head_oid()?;
    let head_commit: Option<Commit> = match head {
        Some(h) => Some(repo.read_commit(&h)?),
        None => None,
    };
    if amend {
        if head_commit.is_none() {
            return Err(Fail::Fatal("You have nothing to amend.".into()));
        }
        if merge_head.is_some() {
            return Err(Fail::Fatal("You are in the middle of a merge -- cannot amend.".into()));
        }
    }
    let head_tree = head_commit.as_ref().map(|c| c.tree);
    if !p.args.is_empty() && !all && !p.has("include") {
        if merge_head.is_some() {
            return Err(Fail::Fatal("cannot do a partial commit during a merge.".into()));
        }
        if cherry_pick {
            return Err(Fail::Fatal("cannot do a partial commit during a cherry-pick.".into()));
        }
    }
    // O índice que vai virar a tree.
    let ipath = repo.index_path();
    let mut idx = Index::load(&ipath)?;
    let mut commit_idx: Option<Index> = None;
    let mut index_dirty = false;
    if all {
        stage_tracked(repo, &mut idx)?;
        index_dirty = true;
    } else if !p.args.is_empty() {
        let ps = git.pathspec(&p.args)?;
        commit_idx = stage_paths(repo, &mut idx, &ps, head_tree.as_ref(), p.has("include"))?;
        index_dirty = true;
    }
    let tree_idx: &Index = commit_idx.as_ref().unwrap_or(&idx);
    if tree_idx.has_conflicts() {
        // O refresh do índice lista os caminhos em conflito (`U<TAB>caminho`) antes do erro.
        let mut listing = String::new();
        let mut last: Option<Vec<u8>> = None;
        for path in tree_idx.unmerged_paths() {
            if last.as_ref() == Some(&path) {
                continue;
            }
            listing.push_str(&format!("U\t{}\n", os::lossy(&path)));
            last = Some(path);
        }
        os::outs(&listing);
        error("Committing is not possible because you have unmerged files.");
        hint("Fix them up in the work tree, and then use 'git add/rm <file>'\nas appropriate to mark resolution and make a commit.");
        return Err(Fail::Fatal("Exiting because of an unresolved conflict.".into()));
    }

    // Hook pre-commit, com o índice que vai ser gravado.
    let hooks = hooks_dir(repo);
    if p.flag("no-verify") != Some(true) {
        let temp = repo.path("next-index.tmp");
        let hook_exists = os::stat(&os::join(&hooks, b"pre-commit")).is_ok();
        let env_path = if hook_exists {
            os::write(&temp, &tree_idx.encode(), 0o666).map_err(|e| Fail::Fatal(format!("unable to write new index file: {}", e.message())))?;
            temp.clone()
        } else {
            ipath.clone()
        };
        let result = editor::run_hook(&hooks, "pre-commit", &[], &[("GIT_INDEX_FILE", env_path.as_slice())]);
        if hook_exists {
            let _ = os::unlink(&temp);
        }
        if let Some(code) = result? {
            if code != 0 {
                return Ok(1);
            }
        }
    }

    let tree = repo.write_tree_from_index(tree_idx)?;
    let nothing = match head_tree {
        Some(t) => t == tree,
        None => tree_idx.entries.is_empty(),
    };
    if nothing && !amend && !p.has("allow-empty") && merge_head.is_none() {
        status::print_for_commit(repo)?;
        if cherry_pick {
            os::errs(EMPTY_CHERRY_PICK_ADVICE);
            if os::exists(&repo.path("sequencer")) {
                os::errs(EMPTY_CHERRY_PICK_ADVICE_MULTI);
            } else {
                os::errs(EMPTY_CHERRY_PICK_ADVICE_SINGLE);
            }
        }
        return Ok(1);
    }

    // Quem assina.
    let committer = ident::ident(&repo.config, Who::Committer, true)?;
    let date_given = p.value("date").is_some();
    // Concluindo um cherry-pick, o autor (e a data) são os do commit escolhido.
    let author_from_pick: Option<Ident> = match cherry_pick_head {
        Some(cp) if !amend && !p.has("reset-author") => Some(repo.read_commit(&cp)?.author_ident()),
        _ => None,
    };
    let from_pick = author_from_pick.is_some();
    let mut author = match author_from_pick {
        Some(a) => a,
        None => match (&head_commit, amend && !p.has("reset-author")) {
            (Some(c), true) => c.author_ident(),
            _ => ident::ident(&repo.config, Who::Author, true)?,
        },
    };
    if let Some(a) = p.value("author") {
        let (name, email) = parse_author(a)?;
        author.name = ident::without_crud(&name);
        author.email = ident::without_crud(&email);
    }
    if let Some(d) = p.value("date") {
        let (t, tz) = ident::parse_ident_date(d)?;
        author.date = Some(t);
        author.tz = tz;
        author.tz_raw = Vec::new();
    }

    // A mensagem.
    let given: Option<Vec<u8>> = {
        let ms = p.values("message");
        if !ms.is_empty() {
            let mut m = ms.join(&b"\n\n"[..]);
            m.push(b'\n');
            Some(m)
        } else if let Some(f) = p.value("file") {
            let data = if f == b"-" { os::stdin_all() } else { super::plumbing::read_user_file(repo, f)? };
            Some(data)
        } else {
            None
        }
    };
    let use_editor = match p.flag("edit") {
        Some(v) => v,
        None => given.is_none(),
    };
    let mut message: Vec<u8> = match &given {
        Some(m) => m.clone(),
        None => {
            if amend {
                head_commit.as_ref().map(|c| c.message.clone()).unwrap_or_default()
            } else if let Ok(Some(m)) = os::read_opt(&repo.path("MERGE_MSG")) {
                m
            } else if let Ok(Some(m)) = os::read_opt(&repo.path("SQUASH_MSG")) {
                m
            } else {
                Vec::new()
            }
        }
    };
    if p.has("signoff") {
        append_signoff(&mut message, &committer);
    }
    let editmsg = repo.path("COMMIT_EDITMSG");
    let mut edited = false;
    if use_editor {
        let mut text = message.clone();
        text.push(b'\n');
        text.extend_from_slice(
            b"# Please enter the commit message for your changes. Lines starting\n# with '#' will be ignored, and an empty message aborts the commit.\n#\n",
        );
        text.extend_from_slice(&status::commit_template(repo, &Pathspec::default())?);
        os::write(&editmsg, &text, 0o666).map_err(|e| Fail::Fatal(format!("could not write '{}': {}", os::lossy(&editmsg), e.message())))?;
        if editor::edit_file(&repo.config, &editmsg).is_err() {
            os::errs("Please supply the message using either -m or -F option.\n");
            return Ok(1);
        }
        message = os::read(&editmsg).map_err(|e| Fail::Fatal(format!("could not read '{}': {}", os::lossy(&editmsg), e.message())))?;
        edited = true;
    }
    let mode = match p.value_str("cleanup").or_else(|| repo.config.get("commit.cleanup")) {
        Some(v) => match v.as_str() {
            "default" => None,
            other => Some(Cleanup::parse(other).ok_or_else(|| Fail::Fatal(format!("Invalid cleanup mode {other}")))?),
        },
        None => None,
    };
    let mode = mode.unwrap_or(if edited { Cleanup::Strip } else { Cleanup::Whitespace });
    let mut message = msg::cleanup(&message, mode, b"#");
    if message.is_empty() && !p.has("allow-empty-message") {
        os::errs("Aborting commit due to empty commit message.\n");
        return Ok(1);
    }
    if !message.is_empty() && !message.ends_with(b"\n") {
        message.push(b'\n');
    }
    os::write(&editmsg, &message, 0o666).map_err(|e| Fail::Fatal(format!("could not write '{}': {}", os::lossy(&editmsg), e.message())))?;
    if p.flag("no-verify") != Some(true) {
        if let Some(code) = editor::run_hook(&hooks, "commit-msg", &[editmsg.as_slice()], &[])? {
            if code != 0 {
                return Ok(1);
            }
            message = os::read(&editmsg).unwrap_or(message);
        }
    }

    // O commit.
    let mut parents: Vec<Oid> = match (&head_commit, amend) {
        (Some(c), true) => c.parents.clone(),
        (Some(_), false) => head.into_iter().collect(),
        (None, _) => Vec::new(),
    };
    let mut merging = false;
    if !amend && let Some(data) = &merge_head {
        for line in data.split(|c| *c == b'\n') {
            if let Some(id) = Oid::from_hex(object::trim_ascii(line))
                && !parents.contains(&id)
            {
                parents.push(id);
                merging = true;
            }
        }
    }
    let commit = Commit {
        tree,
        parents: parents.clone(),
        author: author.to_bytes(),
        committer: committer.to_bytes(),
        encoding: None,
        extra: Vec::new(),
        message: message.clone(),
    };
    let id = repo.write_object(Kind::Commit, &object::encode_commit(&commit))?;
    let subject = object::subject_of(&message);
    let kind = if amend {
        "commit (amend)"
    } else if merging {
        "commit (merge)"
    } else if parents.is_empty() {
        "commit (initial)"
    } else if cherry_pick {
        "commit (cherry-pick)"
    } else {
        "commit"
    };
    repo.update_ref("HEAD", id, Some(head), &format!("{kind}: {}", os::lossy(&subject)), false)?;
    if index_dirty {
        idx.write(&ipath)?;
    }
    // O `CHERRY_PICK_HEAD` e o `REVERT_HEAD` saem (e a fila do sequenciador, se este era o último).
    for name in ["MERGE_HEAD", "MERGE_MSG", "MERGE_MODE", "SQUASH_MSG", "AUTO_MERGE"] {
        let _ = os::unlink(&repo.path(name));
    }
    super::revert::post_commit_cleanup(repo, false);
    editor::run_hook(&hooks, "post-commit", &[], &[])?;
    if !quiet {
        print_summary(repo, &id, &commit, &author, &committer, amend || date_given || from_pick)?;
    }
    Ok(0)
}

/// `[ramo (root-commit) abc1234] assunto`, autor se difere, data se importa e o resumo da mudança.
pub(crate) fn print_summary(repo: &Repo, id: &Oid, commit: &Commit, author: &Ident, committer: &Ident, show_date: bool) -> R<()> {
    let place = match repo.head()? {
        Head::Branch(name, _) => name.strip_prefix("refs/heads/").unwrap_or(&name).to_string(),
        Head::Detached(_) => "detached HEAD".to_string(),
    };
    let root = if commit.parents.is_empty() { " (root-commit)" } else { "" };
    let mut out = format!("[{place}{root} {}] {}\n", repo.abbrev_default(id), os::lossy(&commit.subject()));
    if author.name_email() != committer.name_email() {
        out.push_str(&format!(" Author: {}\n", os::lossy(&author.name_email())));
    }
    if show_date && let Some(t) = author.date {
        out.push_str(&format!(" Date: {}\n", date::show_date(t, author.tz, &DateMode::Normal)));
    }
    os::outs(&out);
    let parent_tree = match commit.parents.first() {
        Some(p) => Some(repo.tree_of(p)?),
        None => None,
    };
    let ps = Pathspec::default();
    let pairs = diff::diff_trees(repo, parent_tree.as_ref(), Some(&commit.tree), &ps)?;
    let renames = match repo.config.get_bool("diff.renames") {
        Ok(Some(false)) => None,
        _ => Some(RenameOpts::default()),
    };
    let o = DiffOpts { shortstat: true, summary: true, renames, ..DiffOpts::default() };
    let pairs = diff::postprocess(repo, pairs, &o)?;
    diff::emit(repo, &pairs, &o)?;
    Ok(())
}
