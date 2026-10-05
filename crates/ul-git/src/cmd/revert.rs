//! `git cherry-pick` e `git revert`: aplicam (ou desfazem) commits em cima do HEAD com a mesma
//! mesclagem do `git merge` (`merge.rs`), gravam o commit com o autor original (no cherry-pick),
//! param nos conflitos deixando `CHERRY_PICK_HEAD` ou `REVERT_HEAD`, `MERGE_MSG` e o índice com os
//! estágios, e guardam a fila em `.git/sequencer` quando são vários commits (`--continue`, `--skip`,
//! `--abort` e `--quit`). Porta do `builtin/revert.c` e do trecho do `sequencer.c` que o cherry-pick
//! e o revert usam (o rebase interativo não faz parte).

use std::collections::HashSet;

use super::Git;
use crate::cmd::unpack::{self, Opts as UnpackOpts};
use crate::diff::rename;
use crate::error::{Fail, R, error, hint, warning};
use crate::graph::{DateQueue, Graph};
use crate::hash::{EMPTY_TREE, Kind, Oid};
use crate::ident::{self, Who};
use crate::index::Index;
use crate::merge::{self, Opts as MergeOpts};
use crate::msg::{self, Cleanup};
use crate::object::{self, Commit};
use crate::opts::{self, Spec};
use crate::os;
use crate::repo::Repo;
use crate::xmerge::{Favor, Style};

#[derive(Copy, Clone, PartialEq, Eq)]
enum Action {
    Pick,
    Revert,
}

impl Action {
    fn name(self) -> &'static str {
        match self {
            Action::Pick => "cherry-pick",
            Action::Revert => "revert",
        }
    }

    /// A palavra da fila (`.git/sequencer/todo`).
    fn word(self) -> &'static str {
        match self {
            Action::Pick => "pick",
            Action::Revert => "revert",
        }
    }
}

const BASE_SPECS: &[Spec] = &[
    opts::flag(None, "quit", "quit"),
    opts::flag(None, "continue", "continue"),
    opts::flag(None, "abort", "abort"),
    opts::flag(None, "skip", "skip"),
    opts::value(None, "cleanup", "cleanup"),
    opts::flag(Some(b'n'), "no-commit", "no-commit"),
    opts::flag(Some(b'e'), "edit", "edit"),
    opts::short_flag(b'r', "noop"),
    opts::flag(Some(b's'), "signoff", "signoff"),
    opts::value(Some(b'm'), "mainline", "mainline"),
    opts::flag(None, "rerere-autoupdate", "rerere-autoupdate"),
    opts::value(None, "strategy", "strategy"),
    opts::value(Some(b'X'), "strategy-option", "strategy-option"),
    opts::optional(Some(b'S'), "gpg-sign", "gpg-sign"),
];

const PICK_SPECS: &[Spec] = &[
    opts::short_flag(b'x', "record-origin"),
    opts::flag(None, "ff", "ff"),
    opts::flag(None, "allow-empty", "allow-empty"),
    opts::flag(None, "allow-empty-message", "allow-empty-message"),
    opts::flag(None, "keep-redundant-commits", "keep-redundant-commits"),
    opts::noneg(opts::value(None, "empty", "empty")),
];

const REVERT_SPECS: &[Spec] = &[opts::flag(None, "reference", "reference")];

/// As opções do `cherry-pick` e do `revert` (o `replay_opts`).
#[derive(Clone)]
struct Ro {
    no_commit: bool,
    /// Negativo = não escolhido.
    edit: i32,
    signoff: bool,
    mainline: usize,
    allow_ff: bool,
    record_origin: bool,
    allow_empty: bool,
    allow_empty_message: bool,
    keep_redundant: bool,
    drop_redundant: bool,
    strategy: Option<String>,
    xopts: Vec<String>,
    cleanup: Option<String>,
    reference: bool,
}

impl Default for Ro {
    fn default() -> Ro {
        Ro {
            no_commit: false,
            edit: -1,
            signoff: false,
            mainline: 0,
            allow_ff: false,
            record_origin: false,
            allow_empty: false,
            allow_empty_message: false,
            keep_redundant: false,
            drop_redundant: false,
            strategy: None,
            xopts: Vec::new(),
            cleanup: None,
            reference: false,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum Mode {
    Run,
    Quit,
    Continue,
    Abort,
    Skip,
}

/// `error(<msg>)` e a morte do `cherry-pick failed` / `revert failed` (exit 128).
fn failed(action: Action, msg: &str) -> Fail {
    error(msg);
    Fail::Fatal(format!("{} failed", action.name()))
}

fn write_file(path: &[u8], data: &[u8]) -> R<()> {
    os::write(path, data, 0o666).map_err(|e| Fail::Fatal(format!("could not write '{}': {}", os::lossy(path), e.message())))
}

// ---- mensagens ---------------------------------------------------------------------------------

/// A primeira linha depois das em branco: o `find_commit_subject`.
fn first_line(msg: &[u8]) -> Vec<u8> {
    let start = object::skip_blank_lines(msg, 0);
    let rest = &msg[start..];
    let end = rest.iter().position(|c| *c == b'\n').unwrap_or(rest.len());
    rest[..end].to_vec()
}

/// O último parágrafo é um bloco de rodapé (`Chave: valor`)? O `has_conforming_footer`.
fn has_conforming_footer(msg: &[u8]) -> bool {
    let text = object::rtrim(msg);
    let lines: Vec<&[u8]> = text.split(|c| *c == b'\n').collect();
    let Some(blank) = lines.iter().rposition(|l| object::rtrim(l).is_empty()) else { return false };
    let para = &lines[blank + 1..];
    if para.is_empty() {
        return false;
    }
    let (mut trailers, mut others, mut recognized) = (0usize, 0usize, false);
    for l in para {
        if l.starts_with(b" ") || l.starts_with(b"\t") {
            continue;
        }
        if l.starts_with(b"(cherry picked from commit ") || l.starts_with(b"Signed-off-by: ") {
            recognized = true;
            trailers += 1;
            continue;
        }
        let colon = l.iter().position(|c| *c == b':');
        let is_trailer = colon.is_some_and(|p| p > 0 && l[..p].iter().all(|c| c.is_ascii_alphanumeric() || *c == b'-'));
        if is_trailer {
            trailers += 1;
        } else {
            others += 1;
        }
    }
    trailers > 0 && (recognized || trailers * 3 >= others)
}

/// `Reapply "x"` ou `Revert "x"` e o `This reverts commit ...` do revert.
fn revert_message(repo: &Repo, o: &Ro, id: &Oid, subject: &[u8], parent: Option<&Oid>, is_merge: bool) -> Vec<u8> {
    let mut m: Vec<u8> = Vec::new();
    let subject_s = os::lossy(subject);
    let refer = |oid: &Oid| -> String {
        if !o.reference {
            return oid.hex();
        }
        // `--reference`: `abreviado ("assunto", data)`.
        match repo.read_commit(oid) {
            Ok(c) => {
                let a = c.author_ident();
                let when = a.date.map(|t| crate::date::show_date(t, a.tz, &crate::date::DateMode::Short)).unwrap_or_default();
                format!("{} (\"{}\", {})", repo.abbrev_default(oid), os::lossy(&first_line(&c.message)), when)
            }
            Err(_) => oid.hex(),
        }
    };
    if o.reference {
        m.extend_from_slice(b"# *** SAY WHY WE ARE REVERTING ON THE TITLE LINE ***");
    } else if let Some(orig) = subject_s.strip_prefix("Revert \"")
        && !orig.starts_with("Revert \"")
    {
        m.extend_from_slice(format!("Reapply \"{orig}").as_bytes());
    } else {
        m.extend_from_slice(format!("Revert \"{subject_s}\"").as_bytes());
    }
    m.extend_from_slice(format!("\n\nThis reverts commit {}", refer(id)).as_bytes());
    if is_merge && let Some(p) = parent {
        m.extend_from_slice(format!(", reversing\nchanges made to {}", refer(p)).as_bytes());
    }
    m.extend_from_slice(b".\n");
    m
}

/// O `# Conflicts:` que o `MERGE_MSG` ganha quando a mesclagem para.
fn append_conflicts_hint(repo: &Repo, msg: &mut Vec<u8>) -> R<()> {
    let idx = Index::load(&repo.index_path())?;
    msg.extend_from_slice(b"\n# Conflicts:\n");
    let mut last: Option<Vec<u8>> = None;
    for path in idx.unmerged_paths() {
        if last.as_ref() == Some(&path) {
            continue;
        }
        msg.extend_from_slice(b"#\t");
        msg.extend_from_slice(&path);
        msg.push(b'\n');
        last = Some(path);
    }
    Ok(())
}

fn print_advice(repo: &Repo, o: &Ro, action: Action) -> R<()> {
    if !repo.config.get_bool("advice.mergeconflict")?.unwrap_or(true) {
        return Ok(());
    }
    if o.no_commit {
        hint("after resolving the conflicts, mark the corrected paths\nwith 'git add <paths>' or 'git rm <paths>'\nDisable this message with \"git config advice.mergeConflict false\"");
    } else {
        let n = action.name();
        hint(&format!(
            "After resolving the conflicts, mark them with\n\"git add/rm <pathspec>\", then run\n\"git {n} --continue\".\nYou can instead skip this commit with \"git {n} --skip\".\nTo abort and get back to the state before \"git {n}\",\nrun \"git {n} --abort\".\nDisable this message with \"git config advice.mergeConflict false\""
        ));
    }
    Ok(())
}

/// O `error_dirty_index`: índice em conflito ou diferente do HEAD.
fn error_dirty_index(repo: &Repo, action: Action) -> R<Fail> {
    let unmerged = Index::load(&repo.index_path())?.has_conflicts();
    if unmerged {
        let what = if action == Action::Pick { "Cherry-picking" } else { "Reverting" };
        error(&format!("{what} is not possible because you have unmerged files."));
        if repo.config.get_bool("advice.resolveconflict")?.unwrap_or(true) {
            hint("Fix them up in the work tree, and then use 'git add/rm <file>'\nas appropriate to mark resolution and make a commit.");
        }
    } else {
        error(&format!("your local changes would be overwritten by {}.", action.name()));
        if repo.config.get_bool("advice.commitbeforemerge")?.unwrap_or(true) {
            hint("commit your changes or stash them to proceed.");
        }
    }
    Ok(Fail::Fatal(format!("{} failed", action.name())))
}

// ---- um commit ---------------------------------------------------------------------------------

/// O que um `do_pick_commit` deixou: terminou, parou (código de saída) ou precisa do `git commit`
/// pra mostrar o erro ou abrir o editor.
enum Pick {
    Done,
    Stop(i32),
    RunCommit(Vec<Vec<u8>>),
}

fn merge_options(o: &Ro, ancestor: &str, branch2: &str, repo: &Repo) -> R<MergeOpts> {
    let mut m = MergeOpts::new("HEAD", branch2, ancestor);
    m.style = match repo.config.get("merge.conflictstyle").as_deref() {
        Some("diff3") => Style::Diff3,
        Some("zdiff3") => Style::ZealousDiff3,
        _ => Style::Merge,
    };
    for x in &o.xopts {
        let bad = || Fail::Fatal(format!("unknown strategy option: -X{x}"));
        match x.as_str() {
            "ours" => m.favor = Favor::Ours,
            "theirs" => m.favor = Favor::Theirs,
            "no-renames" => m.detect_renames = false,
            "renames" | "find-renames" => m.detect_renames = true,
            "patience" | "histogram" | "minimal" | "no-renormalize" | "no-directory-renames" => {}
            s => {
                let score = s.strip_prefix("find-renames=").or_else(|| s.strip_prefix("rename-threshold="));
                if let Some(v) = score {
                    m.detect_renames = true;
                    m.rename_score = rename::parse_score(v).ok_or_else(bad)?;
                } else if s.starts_with("diff-algorithm=") {
                    // O algoritmo de diff fica o Myers.
                } else {
                    return Err(bad());
                }
            }
        }
    }
    Ok(m)
}

/// O `checkout_fast_forward`: leva índice e árvore de trabalho de `from` pra `to`.
fn checkout_fast_forward(repo: &Repo, from: Option<&Oid>, to: &Oid) -> R<bool> {
    let ipath = repo.index_path();
    let idx = Index::load(&ipath)?;
    let old = match from {
        Some(f) => repo.tree_of(f)?,
        None => EMPTY_TREE,
    };
    let new = repo.tree_of(to)?;
    let uo = UnpackOpts { verb: "merge", advice: "merge", force: false };
    match unpack::switch_tree(repo, &idx, Some(&old), &new, &uo) {
        Ok(i) => {
            i.write(&ipath)?;
            Ok(true)
        }
        Err(Fail::Exit(1)) => Ok(false),
        Err(e) => Err(e),
    }
}

fn reflog_action(action: Action) -> String {
    os::getenv_str("GIT_REFLOG_ACTION").unwrap_or_else(|| action.name().to_string())
}

/// Os argumentos do `git commit` que o `run_git_commit` monta quando o commit direto não serve.
fn commit_args(o: &Ro, mfile: &[u8], edit: bool, allow_empty: bool) -> Vec<Vec<u8>> {
    let mut a: Vec<Vec<u8>> = vec![b"-n".to_vec(), b"-F".to_vec(), mfile.to_vec()];
    if let Some(cl) = &o.cleanup {
        a.push(format!("--cleanup={cl}").into_bytes());
    }
    if edit {
        a.push(b"-e".to_vec());
    } else if o.cleanup.is_none() && !o.signoff && !o.record_origin {
        a.push(b"--cleanup=verbatim".to_vec());
    }
    if allow_empty {
        a.push(b"--allow-empty".to_vec());
    }
    if !edit {
        a.push(b"--allow-empty-message".to_vec());
    }
    if o.signoff {
        a.push(b"-s".to_vec());
    }
    a
}

/// O `do_pick_commit`.
fn do_pick(repo: &Repo, o: &Ro, action: Action, commit: &Oid) -> R<Pick> {
    let ipath = repo.index_path();
    let idx = Index::load(&ipath)?;
    let head = repo.head_oid()?;
    let unborn = head.is_none();
    let head_tree: Oid;
    if o.no_commit {
        if idx.has_conflicts() {
            return Err(failed(action, "your index file is unmerged."));
        }
        head_tree = repo.write_tree_from_index(&idx)?;
    } else {
        head_tree = match head {
            Some(h) => repo.tree_of(&h)?,
            None => EMPTY_TREE,
        };
        if idx.has_conflicts() || repo.write_tree_from_index(&idx)? != head_tree {
            return Err(error_dirty_index(repo, action)?);
        }
    }

    let c = repo.read_commit(commit)?;
    let is_merge = c.parents.len() > 1;
    let parent: Option<Oid> = if is_merge {
        if o.mainline == 0 {
            return Err(failed(action, &format!("commit {} is a merge but no -m option was given.", commit.hex())));
        }
        match c.parents.get(o.mainline - 1) {
            Some(p) => Some(*p),
            None => return Err(failed(action, &format!("commit {} does not have parent {}", commit.hex(), o.mainline))),
        }
    } else if o.mainline > 1 {
        return Err(failed(action, &format!("commit {} does not have parent {}", commit.hex(), o.mainline)));
    } else {
        c.parents.first().copied()
    };

    // Avanço rápido (`--ff`): o pai já é o HEAD.
    if o.allow_ff && ((parent.is_some() && parent == head) || (parent.is_none() && unborn)) {
        if !checkout_fast_forward(repo, head.as_ref(), commit)? {
            return Err(Fail::Fatal(format!("{} failed", action.name())));
        }
        let msg = format!("{}: fast-forward", reflog_action(action));
        repo.update_ref("HEAD", *commit, Some(head), &msg, false)?;
        return Ok(Pick::Done);
    }

    let short = repo.abbrev_default(commit);
    let subject = first_line(&c.message);
    let label = format!("{short} ({})", os::lossy(&subject));
    let parent_label = format!("parent of {label}");
    let start = object::skip_blank_lines(&c.message, 0);
    let mut message: Vec<u8>;
    let (base, next, base_label, next_label) = match action {
        Action::Revert => {
            message = revert_message(repo, o, commit, &subject, parent.as_ref(), is_merge);
            (Some(*commit), parent, label.clone(), parent_label.clone())
        }
        Action::Pick => {
            message = c.message[start..].to_vec();
            if o.record_origin {
                if !message.is_empty() && !message.ends_with(b"\n") {
                    message.push(b'\n');
                }
                if !has_conforming_footer(&message) {
                    message.push(b'\n');
                }
                message.extend_from_slice(format!("(cherry picked from commit {})\n", commit.hex()).as_bytes());
            }
            (parent, Some(*commit), parent_label.clone(), label.clone())
        }
    };
    if o.signoff {
        let who = ident::ident(&repo.config, Who::Committer, true)?;
        super::commit::append_signoff(&mut message, &who);
    }

    // A mesclagem.
    let base_tree = match base {
        Some(b) => repo.tree_of(&b)?,
        None => EMPTY_TREE,
    };
    let next_tree = match next {
        Some(n) => repo.tree_of(&n)?,
        None => EMPTY_TREE,
    };
    let mopts = merge_options(o, &base_label, &next_label, repo)?;
    let outcome = merge::merge_trees(repo, &mopts, 0, &base_tree, &head_tree, &next_tree)?;
    match merge::checkout_result(repo, &head_tree, &outcome) {
        Ok(()) => {}
        Err(Fail::Exit(1)) => return Err(Fail::Fatal(format!("{} failed", action.name()))),
        Err(e) => return Err(e),
    }
    repo.write_pseudoref("AUTO_MERGE", format!("{}\n", outcome.tree).as_bytes())?;
    outcome.display();
    let clean = outcome.clean;
    if !clean {
        append_conflicts_hint(repo, &mut message)?;
    }
    let mfile = repo.path("MERGE_MSG");
    write_file(&mfile, &message)?;

    if action == Action::Pick && !o.no_commit {
        repo.write_pseudoref("CHERRY_PICK_HEAD", format!("{commit}\n").as_bytes())?;
    }
    if action == Action::Revert && ((o.no_commit && clean) || !clean) {
        repo.write_pseudoref("REVERT_HEAD", format!("{commit}\n").as_bytes())?;
    }
    if !clean {
        let verb = if action == Action::Revert { "revert" } else { "apply" };
        error(&format!("could not {verb} {short}... {}", os::lossy(&subject)));
        print_advice(repo, o, action)?;
        return Ok(Pick::Stop(1));
    }
    if o.no_commit {
        return Ok(Pick::Done);
    }

    // Ficou vazio depois da mesclagem?
    let now = Index::load(&ipath)?;
    let tree = repo.write_tree_from_index(&now)?;
    let index_unchanged = tree == head_tree;
    let originally_empty = {
        let ptree = match c.parents.first() {
            Some(p) => repo.tree_of(p)?,
            None => EMPTY_TREE,
        };
        ptree == c.tree
    };
    // 0 = parar, 1 = permitido, 2 = descartar.
    let allow = if !index_unchanged {
        0
    } else if o.keep_redundant {
        1
    } else if originally_empty {
        i32::from(o.allow_empty)
    } else if o.drop_redundant {
        2
    } else {
        0
    };
    if allow == 2 {
        let _ = os::unlink(&repo.path("CHERRY_PICK_HEAD"));
        let _ = os::unlink(&mfile);
        os::errs(&format!("dropping {} {} -- patch contents already upstream\n", commit.hex(), os::lossy(&subject)));
        return Ok(Pick::Done);
    }
    let allow_flag = allow == 1;
    let edit = if o.edit < 0 { action == Action::Revert && os::sysc().isatty(sysabi::Fd::STDIN) } else { o.edit > 0 };
    if edit {
        return Ok(Pick::RunCommit(commit_args(o, &mfile, true, allow_flag)));
    }

    // O commit na hora (o `try_to_commit`).
    let mut text = message.clone();
    if let Some(cl) = &o.cleanup {
        let mode = Cleanup::parse(cl).unwrap_or(Cleanup::Whitespace);
        text = msg::cleanup(&text, mode, b"#");
    }
    let empty_message = object::rtrim(&text).is_empty();
    if (!allow_flag && index_unchanged) || (empty_message && !o.allow_empty_message) {
        return Ok(Pick::RunCommit(commit_args(o, &mfile, false, allow_flag)));
    }
    let committer = ident::ident(&repo.config, Who::Committer, true)?;
    let author_bytes = match action {
        Action::Pick => c.author.clone(),
        Action::Revert => ident::ident(&repo.config, Who::Author, true)?.to_bytes(),
    };
    let new = Commit { tree, parents: head.into_iter().collect(), author: author_bytes, committer: committer.to_bytes(), encoding: None, extra: Vec::new(), message: text.clone() };
    let id = repo.write_object(Kind::Commit, &object::encode_commit(&new))?;
    let line = object::subject_of(&text);
    repo.update_ref("HEAD", id, Some(head), &format!("{}: {}", reflog_action(action), os::lossy(&line)), false)?;
    let _ = os::unlink(&repo.path("CHERRY_PICK_HEAD"));
    let _ = os::unlink(&mfile);
    let author = new.author_ident();
    super::commit::print_summary(repo, &id, &new, &author, &committer, true)?;
    Ok(Pick::Done)
}

/// Aplica um commit; quando precisa do `git commit` (editor, vazio, mensagem vazia), chama o
/// comando dentro do processo e devolve o código dele.
fn pick_one(git: &mut Git, o: &Ro, action: Action, commit: &Oid) -> R<i32> {
    let pick = do_pick(git.repo()?, o, action, commit)?;
    match pick {
        Pick::Done => Ok(0),
        Pick::Stop(code) => Ok(code),
        Pick::RunCommit(args) => super::commit::run(git, &args),
    }
}

// ---- o estado do sequenciador ------------------------------------------------------------------

fn seq_dir(repo: &Repo) -> Vec<u8> {
    repo.path("sequencer")
}

fn todo_path(repo: &Repo) -> Vec<u8> {
    repo.path("sequencer/todo")
}

/// Uma linha da fila: o commit e o texto como está no arquivo.
struct Item {
    oid: Oid,
    line: String,
}

/// O comando da primeira linha da fila, se houver fila.
fn last_command(repo: &Repo) -> Option<Action> {
    let data = os::read_opt(&todo_path(repo)).ok().flatten()?;
    let first = data.split(|c| *c == b'\n').next()?;
    let word = first.split(|c| *c == b' ').next()?;
    match word {
        b"pick" | b"p" => Some(Action::Pick),
        b"revert" | b"r" => Some(Action::Revert),
        _ => None,
    }
}

/// O `create_seq_dir`: recusa começar outra sequência por cima de uma em andamento.
fn create_seq_dir(repo: &Repo, action: Action) -> R<()> {
    let advise_skip = os::exists(&repo.path("CHERRY_PICK_HEAD")) || os::exists(&repo.path("REVERT_HEAD"));
    if let Some(last) = last_command(repo) {
        let (msg, name) = match last {
            Action::Revert => ("revert is already in progress", "revert"),
            Action::Pick => ("cherry-pick is already in progress", "cherry-pick"),
        };
        error(msg);
        if repo.config.get_bool("advice.sequencerinuse")?.unwrap_or(true) {
            hint(&format!("try \"git {name} (--continue | {}--abort | --quit)\"", if advise_skip { "--skip | " } else { "" }));
        }
        return Err(Fail::Fatal(format!("{} failed", action.name())));
    }
    let dir = seq_dir(repo);
    if let Err(e) = os::mkdir(&dir, 0o777) {
        return Err(failed(action, &format!("could not create sequencer directory '{}': {}", os::lossy(&dir), e.message())));
    }
    Ok(())
}

fn save_head(repo: &Repo, head: Option<Oid>) -> R<()> {
    let hex = head.unwrap_or(Oid::ZERO).hex();
    write_file(&repo.path("sequencer/head"), format!("{hex}\n").as_bytes())
}

/// Aspas só quando o valor precisa, como o `git config` escreve.
fn config_value(v: &str) -> String {
    let needs = v.is_empty() || v.starts_with(' ') || v.ends_with(' ') || v.contains(['"', '\\', '#', ';', '\n', '\t']);
    if !needs {
        return v.to_string();
    }
    let mut out = String::from("\"");
    for ch in v.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// O `save_opts`: só as opções que não são o padrão, num arquivo no formato de configuração.
fn save_opts(repo: &Repo, o: &Ro) -> R<()> {
    let mut lines: Vec<String> = Vec::new();
    let mut put = |k: &str, v: String| lines.push(format!("\t{k} = {}\n", config_value(&v)));
    if o.no_commit {
        put("no-commit", "true".into());
    }
    if o.edit >= 0 {
        let v = if o.edit > 0 { "true" } else { "false" };
        put("edit", v.to_string());
    }
    if o.allow_empty {
        put("allow-empty", "true".into());
    }
    if o.allow_empty_message {
        put("allow-empty-message", "true".into());
    }
    if o.keep_redundant {
        put("keep-redundant-commits", "true".into());
    }
    if o.drop_redundant {
        put("drop-redundant-commits", "true".into());
    }
    if o.signoff {
        put("signoff", "true".into());
    }
    if o.record_origin {
        put("record-origin", "true".into());
    }
    if o.allow_ff {
        put("allow-ff", "true".into());
    }
    if o.mainline > 0 {
        put("mainline", o.mainline.to_string());
    }
    if let Some(s) = &o.strategy {
        put("strategy", s.clone());
    }
    for x in &o.xopts {
        put("strategy-option", x.clone());
    }
    if lines.is_empty() {
        return Ok(());
    }
    let mut text = String::from("[options]\n");
    for l in lines {
        text.push_str(&l);
    }
    write_file(&repo.path("sequencer/opts"), text.as_bytes())
}

/// O `read_populate_opts`: as opções que o início da sequência guardou.
fn load_saved_opts(repo: &Repo, o: &mut Ro) {
    let Some(data) = os::read_opt(&repo.path("sequencer/opts")).ok().flatten() else { return };
    for raw in os::lossy(&data).lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('[') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let (k, v) = (k.trim(), v.trim().trim_matches('"'));
        let truth = v == "true";
        match k {
            "no-commit" => o.no_commit = truth,
            "edit" => o.edit = i32::from(truth),
            "allow-empty" => o.allow_empty = truth,
            "allow-empty-message" => o.allow_empty_message = truth,
            "keep-redundant-commits" => o.keep_redundant = truth,
            "drop-redundant-commits" => o.drop_redundant = truth,
            "signoff" => o.signoff = truth,
            "record-origin" => o.record_origin = truth,
            "allow-ff" => o.allow_ff = truth,
            "mainline" => o.mainline = v.parse().unwrap_or(0),
            "strategy" => o.strategy = Some(v.to_string()),
            "strategy-option" => o.xopts.push(v.to_string()),
            _ => {}
        }
    }
    if o.keep_redundant {
        o.allow_empty = true;
    }
}

fn save_todo(repo: &Repo, items: &[Item]) -> R<()> {
    let mut text = String::new();
    for it in items {
        text.push_str(&it.line);
        text.push('\n');
    }
    write_file(&todo_path(repo), text.as_bytes())
}

/// O `update_abort_safety_file`: o HEAD de agora, pra o `--abort` saber se alguém mexeu nele.
fn update_abort_safety(repo: &Repo) -> R<()> {
    if !os::is_dir(&seq_dir(repo)) {
        return Ok(());
    }
    let hex = repo.head_oid()?.map(|h| h.hex()).unwrap_or_default();
    write_file(&repo.path("sequencer/abort-safety"), format!("{hex}\n").as_bytes())
}

/// O `rollback_is_safe`: o HEAD ainda é o que a sequência deixou?
fn rollback_is_safe(repo: &Repo) -> R<bool> {
    let path = repo.path("sequencer/abort-safety");
    let expected: Option<Oid> = match os::read_opt(&path) {
        Ok(Some(data)) => {
            let t = object::trim_ascii(&data).to_vec();
            if t.is_empty() {
                None
            } else {
                match Oid::from_hex(&t) {
                    Some(o) => Some(o),
                    None => return Err(Fail::Fatal(format!("could not parse {}", os::lossy(&path)))),
                }
            }
        }
        Ok(None) => None,
        Err(e) => return Err(Fail::Fatal(format!("could not read '{}': {}", os::lossy(&path), e.message()))),
    };
    Ok(repo.head_oid()? == expected)
}

/// O `have_finished_the_last_pick`: a fila tem no máximo uma linha.
fn have_finished_the_last_pick(repo: &Repo) -> bool {
    let Some(data) = os::read_opt(&todo_path(repo)).ok().flatten() else { return false };
    match data.iter().position(|c| *c == b'\n') {
        None => true,
        Some(i) => i + 1 >= data.len(),
    }
}

/// O `sequencer_post_commit_cleanup`: some o `CHERRY_PICK_HEAD`/`REVERT_HEAD` e, se o último commit
/// da fila já foi gravado, a pasta `sequencer`.
pub(crate) fn post_commit_cleanup(repo: &Repo, verbose: bool) {
    let mut need = false;
    for (name, what) in [("CHERRY_PICK_HEAD", "a cherry picking"), ("REVERT_HEAD", "a revert")] {
        let p = repo.path(name);
        if os::exists(&p) {
            if os::unlink(&p).is_ok() && verbose {
                warning(&format!("cancelling {what} in progress"));
            }
            need = true;
        }
    }
    if need && have_finished_the_last_pick(repo) {
        let _ = os::remove_tree(&seq_dir(repo));
    }
}

/// O `remove_branch_state`: tudo que um cherry-pick, revert, merge ou squash em andamento deixa.
pub(crate) fn remove_branch_state(repo: &Repo, verbose: bool) {
    post_commit_cleanup(repo, verbose);
    for f in ["SQUASH_MSG", "MERGE_HEAD", "MERGE_RR", "MERGE_MSG", "MERGE_MODE", "AUTO_MERGE"] {
        let _ = os::unlink(&repo.path(f));
    }
}

/// O `sequencer_remove_state`.
fn remove_seq_state(repo: &Repo) {
    let _ = os::remove_tree(&seq_dir(repo));
}

// ---- a fila ------------------------------------------------------------------------------------

/// Os commits em ordem de data (o mais novo primeiro) alcançáveis de `include` e não de `exclude`.
fn walk_range(repo: &Repo, include: &[Oid], exclude: &[Oid]) -> R<Vec<Oid>> {
    let mut g = Graph::new(repo);
    let hidden: HashSet<Oid> = if exclude.is_empty() { HashSet::new() } else { g.reachable(exclude)? };
    let mut queue = DateQueue::new();
    let mut seen: HashSet<Oid> = HashSet::new();
    for id in include {
        if !hidden.contains(id) && seen.insert(*id) {
            let d = g.date(id)?;
            queue.push(d, *id);
        }
    }
    let mut out: Vec<Oid> = Vec::new();
    while let Some((_, c)) = queue.pop() {
        out.push(c);
        for p in g.parents(&c)? {
            if hidden.contains(&p) || !seen.insert(p) {
                continue;
            }
            let d = g.date(&p)?;
            queue.push(d, p);
        }
    }
    Ok(out)
}

fn bad_revision(arg: &[u8]) -> Fail {
    Fail::Fatal(format!("bad revision '{}'", os::lossy(arg)))
}

fn resolve_one(repo: &Repo, spec: &[u8], whole: &[u8]) -> R<Oid> {
    let s: &[u8] = if spec.is_empty() { b"HEAD" } else { spec };
    match repo.rev_parse(s)? {
        Some(o) => Ok(o),
        None => Err(bad_revision(whole)),
    }
}

fn peel_commit(repo: &Repo, id: &Oid) -> R<Option<Oid>> {
    repo.peel_to_commit(id)
}

/// Os commits pedidos na linha de comando, na ordem em que vão ser aplicados, e se foi um commit só.
fn resolve_revisions(repo: &Repo, args: &[Vec<u8>], action: Action) -> R<(Vec<Oid>, bool)> {
    let mut include: Vec<Oid> = Vec::new();
    let mut exclude: Vec<Oid> = Vec::new();
    let mut plain: Vec<Oid> = Vec::new();
    let mut walk = false;
    let find = |a: &[u8], sep: &[u8]| a.windows(sep.len()).position(|w| w == sep);
    for (n, raw) in args.iter().enumerate() {
        let a: Vec<u8> = if n == 0 && raw == b"-" { b"@{-1}".to_vec() } else { raw.clone() };
        let name = a.clone();
        let to_commit = |id: Oid| -> R<Oid> {
            match peel_commit(repo, &id)? {
                Some(c) => Ok(c),
                None => {
                    let kind = repo.object_kind(&id)?.map(Kind::name).unwrap_or("object");
                    Err(failed(action, &format!("{}: can't cherry-pick a {kind}", os::lossy(&name))))
                }
            }
        };
        if let Some(rest) = a.strip_prefix(b"^") {
            let id = resolve_one(repo, rest, &a)?;
            exclude.push(to_commit(id)?);
            walk = true;
        } else if let Some(i) = find(&a, b"...") {
            let l = resolve_one(repo, &a[..i], &a)?;
            let r = resolve_one(repo, &a[i + 3..], &a)?;
            let (l, r) = (to_commit(l)?, to_commit(r)?);
            exclude.extend(Graph::new(repo).merge_bases(&l, &[r])?);
            include.push(l);
            include.push(r);
            walk = true;
        } else if let Some(i) = find(&a, b"..") {
            let l = resolve_one(repo, &a[..i], &a)?;
            let r = resolve_one(repo, &a[i + 2..], &a)?;
            exclude.push(to_commit(l)?);
            include.push(to_commit(r)?);
            walk = true;
        } else {
            let id = resolve_one(repo, &a, &a)?;
            let c = to_commit(id)?;
            include.push(c);
            plain.push(c);
        }
    }
    if !walk {
        let single = args.len() == 1;
        return Ok((plain, single));
    }
    let mut list = walk_range(repo, &include, &exclude)?;
    if action == Action::Pick {
        list.reverse();
    }
    Ok((list, false))
}

fn item_for(repo: &Repo, action: Action, oid: &Oid) -> R<Item> {
    let c = repo.read_commit(oid)?;
    let subject = first_line(&c.message);
    Ok(Item { oid: *oid, line: format!("{} {} {}", action.word(), repo.abbrev_default(oid), os::lossy(&subject)) })
}

/// O `read_populate_todo` do cherry-pick e do revert.
fn read_populate_todo(repo: &Repo, action: Action) -> R<Vec<Item>> {
    let path = todo_path(repo);
    let data = match os::read(&path) {
        Ok(d) => d,
        Err(e) => return Err(failed(action, &format!("could not open '{}': {}", os::lossy(&path), e.message()))),
    };
    let mut items: Vec<Item> = Vec::new();
    for raw in os::lossy(&data).lines() {
        let line = raw.trim_end_matches('\r');
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut words = line.splitn(3, ' ');
        let word = words.next().unwrap_or("");
        let hex = words.next().unwrap_or("");
        let cmd = match word {
            "pick" | "p" => Action::Pick,
            "revert" | "r" => Action::Revert,
            _ => return Err(failed(action, &format!("unusable instruction sheet: '{}'", os::lossy(&path)))),
        };
        let oid = match repo.rev_parse_commit(hex.as_bytes())? {
            Some(o) if !hex.is_empty() => o,
            _ => return Err(failed(action, &format!("unusable instruction sheet: '{}'", os::lossy(&path)))),
        };
        if cmd != action {
            let m = if action == Action::Pick { "cannot cherry-pick during a revert." } else { "cannot revert during a cherry-pick." };
            return Err(failed(action, m));
        }
        items.push(Item { oid, line: line.to_string() });
    }
    if items.is_empty() {
        return Err(failed(action, "no commits parsed."));
    }
    Ok(items)
}

/// O `pick_commits`: aplica a fila a partir de `current`, gravando o que falta antes de cada passo.
fn pick_commits(git: &mut Git, o: &Ro, action: Action, items: &[Item], mut current: usize) -> R<i32> {
    while current < items.len() {
        {
            let repo = git.repo()?;
            save_todo(repo, &items[current..])?;
            update_abort_safety(repo)?;
        }
        let res = pick_one(git, o, action, &items[current].oid)?;
        current += 1;
        if res != 0 {
            return Ok(res);
        }
    }
    remove_seq_state(git.repo()?);
    Ok(0)
}

// ---- os modos ----------------------------------------------------------------------------------

/// O `index_differs_from(HEAD)`: o índice (ou um conflito) difere do HEAD.
fn index_differs_from_head(repo: &Repo) -> R<bool> {
    let idx = Index::load(&repo.index_path())?;
    if idx.has_conflicts() {
        return Ok(true);
    }
    let head_tree = match repo.head_oid()? {
        Some(h) => repo.tree_of(&h)?,
        None => EMPTY_TREE,
    };
    Ok(repo.write_tree_from_index(&idx)? != head_tree)
}

/// O `continue_single_pick`: o `git commit` do que estiver parado.
fn continue_single_pick(git: &mut Git, action: Action) -> R<i32> {
    let repo = git.repo()?;
    if !os::exists(&repo.path("CHERRY_PICK_HEAD")) && !os::exists(&repo.path("REVERT_HEAD")) {
        return Err(failed(action, "no cherry-pick or revert in progress"));
    }
    super::commit::run(git, &[])
}

/// O `sequencer_continue`.
fn sequencer_continue(git: &mut Git, cli: &Ro, action: Action) -> R<i32> {
    if !os::exists(&todo_path(git.repo()?)) {
        return continue_single_pick(git, action);
    }
    let (o, items) = {
        let repo = git.repo()?;
        let mut o = Ro::default();
        load_saved_opts(repo, &mut o);
        if cli.cleanup.is_some() {
            o.cleanup = cli.cleanup.clone();
        }
        (o, read_populate_todo(repo, action)?)
    };
    let stopped = {
        let repo = git.repo()?;
        os::exists(&repo.path("CHERRY_PICK_HEAD")) || os::exists(&repo.path("REVERT_HEAD"))
    };
    if stopped {
        let res = continue_single_pick(git, action)?;
        if res != 0 {
            return Ok(res);
        }
    }
    {
        let repo = git.repo()?;
        if index_differs_from_head(repo)? {
            return Err(error_dirty_index(repo, action)?);
        }
    }
    pick_commits(git, &o, action, &items, 1)
}

/// O `sequencer_skip`.
fn sequencer_skip(git: &mut Git, cli: &Ro, action: Action) -> R<i32> {
    {
        let repo = git.repo()?;
        let in_effect = match action {
            Action::Pick => os::exists(&repo.path("CHERRY_PICK_HEAD")),
            Action::Revert => os::exists(&repo.path("REVERT_HEAD")),
        };
        if !in_effect {
            return Err(failed(action, &format!("no {} in progress", action.name())));
        }
        let Some(head) = repo.head_oid()? else {
            return Err(failed(action, "cannot resolve HEAD"));
        };
        super::merge::reset_merge(repo, Some(&head))?;
        if !os::is_dir(&seq_dir(repo)) {
            return Ok(0);
        }
    }
    sequencer_continue(git, cli, action)
}

/// O `rollback_single_pick`: desfaz um cherry-pick ou revert de um commit só.
fn rollback_single_pick(repo: &Repo, action: Action) -> R<i32> {
    if !os::exists(&repo.path("CHERRY_PICK_HEAD")) && !os::exists(&repo.path("REVERT_HEAD")) {
        return Err(failed(action, "no cherry-pick or revert in progress"));
    }
    let Some(head) = repo.head_oid()? else {
        return Err(failed(action, "cannot abort from a branch yet to be born"));
    };
    super::merge::reset_merge(repo, Some(&head))?;
    Ok(0)
}

/// O `sequencer_rollback`: volta ao HEAD de antes da sequência.
fn sequencer_rollback(repo: &Repo, action: Action) -> R<i32> {
    let path = repo.path("sequencer/head");
    let data = match os::read_opt(&path) {
        Ok(Some(d)) => d,
        Ok(None) => return rollback_single_pick(repo, action),
        Err(e) => return Err(failed(action, &format!("cannot open '{}': {}", os::lossy(&path), e.message()))),
    };
    let first = data.split(|c| *c == b'\n').next().unwrap_or(&[]);
    let Some(oid) = Oid::from_hex(first) else {
        return Err(failed(action, &format!("stored pre-cherry-pick HEAD file '{}' is corrupt", os::lossy(&path))));
    };
    if oid.is_zero() {
        return Err(failed(action, "cannot abort from a branch yet to be born"));
    }
    if !rollback_is_safe(repo)? {
        warning("You seem to have moved HEAD. Not rewinding, check your HEAD!");
    } else {
        super::merge::reset_merge(repo, Some(&oid))?;
    }
    remove_seq_state(repo);
    Ok(0)
}

// ---- a linha de comando ------------------------------------------------------------------------

fn verify_opt_compatible(me: &str, base_opt: &str, list: &[(&str, bool)]) -> R<()> {
    match list.iter().find(|(_, on)| *on) {
        Some((name, _)) => Err(Fail::Fatal(format!("{me}: {name} cannot be used with {base_opt}"))),
        None => Ok(()),
    }
}

/// As opções e o modo (`--continue` e companhia).
fn parse_ro(action: Action, p: &opts::Parsed) -> R<(Ro, Mode)> {
    let me = action.name();
    let mut o = Ro {
        no_commit: p.has("no-commit"),
        edit: match p.flag("edit") {
            Some(true) => 1,
            Some(false) => 0,
            None => -1,
        },
        signoff: p.has("signoff"),
        ..Default::default()
    };
    if let Some(v) = p.value("mainline") {
        match std::str::from_utf8(v).ok().and_then(|s| s.parse::<i64>().ok()) {
            Some(n) if n > 0 => o.mainline = n as usize,
            _ => return Err(opts::error_only("option `mainline' expects a number greater than zero")),
        }
    }
    o.strategy = p.value_str("strategy");
    o.xopts = p.values("strategy-option").iter().map(|v| os::lossy(v)).collect();
    o.cleanup = p.value_str("cleanup");
    if let Some(cl) = &o.cleanup
        && Cleanup::parse(cl).is_none()
        && cl != "default"
    {
        return Err(Fail::Fatal(format!("Invalid cleanup mode {cl}")));
    }
    let mut empty_given = false;
    match action {
        Action::Pick => {
            o.record_origin = p.has("record-origin");
            o.allow_ff = p.has("ff");
            o.allow_empty = p.has("allow-empty");
            o.allow_empty_message = p.has("allow-empty-message");
            o.keep_redundant = p.has("keep-redundant-commits");
            if let Some(v) = p.value("empty") {
                empty_given = true;
                match v {
                    b"stop" => {}
                    b"drop" => o.drop_redundant = true,
                    b"keep" => o.keep_redundant = true,
                    other => return Err(opts::error_only(&format!("invalid value for '--empty': '{}'", os::lossy(other)))),
                }
            }
        }
        Action::Revert => o.reference = p.has("reference"),
    }
    if o.keep_redundant {
        o.allow_empty = true;
    }

    let mode = if p.has("quit") {
        Mode::Quit
    } else if p.has("continue") {
        Mode::Continue
    } else if p.has("abort") {
        Mode::Abort
    } else if p.has("skip") {
        Mode::Skip
    } else {
        Mode::Run
    };
    let rerere = p.flag("rerere-autoupdate");
    let base = match mode {
        Mode::Quit => Some("--quit"),
        Mode::Continue => Some("--continue"),
        Mode::Skip => Some("--skip"),
        Mode::Abort => Some("--abort"),
        Mode::Run => None,
    };
    if let Some(base) = base {
        verify_opt_compatible(
            me,
            base,
            &[
                ("--no-commit", o.no_commit),
                ("--signoff", o.signoff),
                ("--mainline", o.mainline != 0),
                ("--strategy", o.strategy.is_some()),
                ("--strategy-option", !o.xopts.is_empty()),
                ("-x", o.record_origin),
                ("--ff", o.allow_ff),
                ("--rerere-autoupdate", rerere == Some(true)),
                ("--no-rerere-autoupdate", rerere == Some(false)),
                ("--keep-redundant-commits", o.keep_redundant),
                ("--empty", empty_given),
            ],
        )?;
    }
    if o.allow_ff {
        verify_opt_compatible(me, "--ff", &[("--signoff", o.signoff), ("--no-commit", o.no_commit), ("-x", o.record_origin), ("--edit", o.edit > 0)])?;
    }
    Ok((o, mode))
}

fn usage_exit(usage: &str) -> Fail {
    opts::usage_to_stderr(usage);
    Fail::Exit(129)
}

fn run_sequencer(git: &mut Git, args: &[Vec<u8>], action: Action) -> R<i32> {
    let usage = git.usage();
    let mut specs: Vec<Spec> = BASE_SPECS.to_vec();
    match action {
        Action::Pick => specs.extend_from_slice(PICK_SPECS),
        Action::Revert => specs.extend_from_slice(REVERT_SPECS),
    }
    let p = opts::parse(&specs, args, opts::KEEP_UNKNOWN, usage)?;
    let (o, mode) = parse_ro(action, &p)?;
    if mode != Mode::Run {
        if !p.args.is_empty() || !p.unknown.is_empty() {
            return Err(usage_exit(usage));
        }
        return match mode {
            Mode::Quit => {
                let repo = git.repo()?;
                remove_seq_state(repo);
                remove_branch_state(repo, false);
                Ok(0)
            }
            Mode::Continue => sequencer_continue(git, &o, action),
            Mode::Abort => sequencer_rollback(git.repo()?, action),
            Mode::Skip => sequencer_skip(git, &o, action),
            Mode::Run => Ok(0),
        };
    }
    if p.args.is_empty() || !p.unknown.is_empty() {
        return Err(usage_exit(usage));
    }

    // Os commits pedidos.
    let (commits, single) = {
        let repo = git.repo()?;
        let (list, single) = resolve_revisions(repo, &p.args, action)?;
        if list.is_empty() {
            return Err(failed(action, "empty commit set passed"));
        }
        (list, single)
    };
    if single {
        // Um commit só: não mexe no estado do sequenciador, só deixa o `CHERRY_PICK_HEAD`.
        return pick_one(git, &o, action, &commits[0]);
    }

    // Uma sequência nova.
    let items = {
        let repo = git.repo()?;
        let mut items: Vec<Item> = Vec::new();
        for c in &commits {
            items.push(item_for(repo, action, c)?);
        }
        create_seq_dir(repo, action)?;
        let head = repo.head_oid()?;
        if head.is_none() && action == Action::Revert {
            return Err(failed(action, "can't revert as initial commit"));
        }
        save_head(repo, head)?;
        save_opts(repo, &o)?;
        update_abort_safety(repo)?;
        items
    };
    pick_commits(git, &o, action, &items, 0)
}

pub fn run_cherry_pick(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    run_sequencer(git, args, Action::Pick)
}

pub fn run_revert(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    run_sequencer(git, args, Action::Revert)
}
