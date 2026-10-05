//! `git status`: formato longo, curto (`-s`, `-b`) e porcelain v1, com detecção de renomeação entre
//! o HEAD e o índice, arquivos não rastreados, ignorados e caminhos em conflito.

use super::Git;
use crate::config;
use crate::diff::rename::RenameOpts;
use crate::diff::{self, DiffOpts, Pair};
use crate::error::{Fail, R};
use crate::graph::Graph;
use crate::ignore::Ignores;
use crate::index::Index;
use crate::opts::{self, Spec};
use crate::os;
use crate::pathspec::Pathspec;
use crate::quote;
use crate::repo::Repo;
use crate::worktree::{IgnoredMode, Scanner, UntrackedMode};

const SPECS: &[Spec] = &[
    opts::flag(Some(b's'), "short", "short"),
    opts::flag(Some(b'b'), "branch", "branch"),
    opts::optional(None, "porcelain", "porcelain"),
    opts::flag(None, "long", "long"),
    opts::optional(Some(b'u'), "untracked-files", "untracked"),
    opts::optional(None, "ignored", "ignored"),
    opts::short_flag(b'z', "nul"),
    opts::flag(None, "ahead-behind", "ahead-behind"),
    opts::flag(None, "renames", "renames"),
    opts::flag(None, "find-renames", "renames"),
];

/// O que o status coletou.
struct Status {
    /// Ainda não há commit.
    initial: bool,
    /// HEAD contra o índice, com renomeações, em ordem de caminho.
    staged: Vec<Pair>,
    /// Índice contra a árvore de trabalho, em ordem de caminho.
    unstaged: Vec<Pair>,
    /// Caminhos em conflito e os estágios presentes (bit 0 = base, 1 = nossa, 2 = deles).
    unmerged: Vec<(Vec<u8>, u8)>,
    untracked: Vec<Vec<u8>>,
    ignored: Vec<Vec<u8>>,
}

fn untracked_mode(text: &str) -> R<UntrackedMode> {
    match text {
        "no" => Ok(UntrackedMode::No),
        "normal" | "true" => Ok(UntrackedMode::Normal),
        "all" => Ok(UntrackedMode::All),
        other => Err(Fail::Fatal(format!("Invalid untracked files mode '{other}'"))),
    }
}

fn rename_opts(repo: &Repo, flag: Option<bool>) -> R<Option<RenameOpts>> {
    if flag == Some(false) {
        return Ok(None);
    }
    let value = repo.config.get("status.renames").or_else(|| repo.config.get("diff.renames")).map(|v| v.to_ascii_lowercase());
    match value.as_deref() {
        Some("copies") | Some("copy") => Ok(Some(RenameOpts { copies: true, ..RenameOpts::default() })),
        Some(v) if flag.is_none() && config::parse_bool(Some(v.as_bytes())) == Some(false) => Ok(None),
        _ => Ok(Some(RenameOpts::default())),
    }
}

fn by_path(pairs: &mut [Pair]) {
    pairs.sort_by(|a, b| a.path().cmp(b.path()));
}

fn collect(repo: &Repo, ps: &Pathspec, renames: Option<RenameOpts>, untracked: UntrackedMode, ignored: IgnoredMode) -> R<Status> {
    let idx = Index::load(&repo.index_path())?;
    let head_tree = super::diff_cmd::head_tree(repo)?;
    let initial = repo.head_oid()?.is_none();
    let mut staged = diff::diff_tree_index(repo, head_tree.as_ref(), &idx, ps)?;
    staged.retain(|p| p.status != b'U');
    let dopts = DiffOpts { renames, ..DiffOpts::default() };
    let mut staged = diff::postprocess(repo, staged, &dopts)?;
    by_path(&mut staged);
    let mut unstaged = diff::diff_index_worktree(repo, &idx, ps)?;
    unstaged.retain(|p| p.status != b'U');
    by_path(&mut unstaged);
    let mut unmerged: Vec<(Vec<u8>, u8)> = Vec::new();
    for e in &idx.entries {
        if e.stage == 0 || !ps.matches_simple(&e.path) {
            continue;
        }
        let bit = 1u8 << (e.stage - 1);
        match unmerged.last_mut() {
            Some((p, bits)) if *p == e.path => *bits |= bit,
            _ => unmerged.push((e.path.clone(), bit)),
        }
    }
    let mut scanner = Scanner { idx: &idx, ign: Ignores::standard(repo), ps, untracked, ignored, show_empty_dirs: false };
    let scan = scanner.run();
    Ok(Status { initial, staged, unstaged, unmerged, untracked: scan.untracked, ignored: scan.ignored })
}

// ---- ramo e upstream --------------------------------------------------------------------------

/// Upstream do ramo: nome curto e, se a ref existe, `(à frente, atrás)`.
pub struct Tracking {
    pub name: String,
    pub counts: Option<(usize, usize)>,
}

pub fn tracking(repo: &Repo, branch_ref: &str, ahead_behind: bool) -> R<Option<Tracking>> {
    let Some(branch) = branch_ref.strip_prefix("refs/heads/") else { return Ok(None) };
    let Some(up) = repo.upstream_ref(branch)? else { return Ok(None) };
    let name = up.strip_prefix("refs/remotes/").or_else(|| up.strip_prefix("refs/heads/")).unwrap_or(&up).to_string();
    let counts = match (repo.ref_oid(branch_ref)?, repo.ref_oid(&up)?) {
        (Some(h), Some(u)) => {
            if ahead_behind {
                Some(Graph::new(repo).ahead_behind(&h, &u)?)
            } else {
                Some((usize::from(h != u), 0))
            }
        }
        _ => None,
    };
    Ok(Some(Tracking { name, counts }))
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "commit" } else { "commits" }
}

/// Frase sobre o upstream, no fim da linha do ramo, no formato longo.
pub fn tracking_text(t: &Tracking, hints: bool) -> String {
    let name = &t.name;
    let mut out = String::new();
    match t.counts {
        None => {
            out.push_str(&format!("Your branch is based on '{name}', but the upstream is gone.\n"));
            if hints {
                out.push_str("  (use \"git branch --unset-upstream\" to fixup)\n");
            }
        }
        Some((0, 0)) => out.push_str(&format!("Your branch is up to date with '{name}'.\n")),
        Some((a, 0)) => {
            out.push_str(&format!("Your branch is ahead of '{name}' by {a} {}.\n", plural(a)));
            if hints {
                out.push_str("  (use \"git push\" to publish your local commits)\n");
            }
        }
        Some((0, b)) => {
            out.push_str(&format!("Your branch is behind '{name}' by {b} {}, and can be fast-forwarded.\n", plural(b)));
            if hints {
                out.push_str("  (use \"git pull\" to update your local branch)\n");
            }
        }
        Some((a, b)) => {
            out.push_str(&format!(
                "Your branch and '{name}' have diverged,\nand have {a} and {b} different commits each, respectively.\n"
            ));
            if hints {
                out.push_str("  (use \"git pull\" if you want to integrate the remote branch with yours)\n");
            }
        }
    }
    out
}

// ---- formato longo ----------------------------------------------------------------------------

fn put(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(s.as_bytes());
}

fn display(repo: &Repo, path: &[u8], fully: bool) -> Vec<u8> {
    quote::quote_c(&repo.display_path(path), fully)
}

fn staged_label(status: u8) -> &'static str {
    match status {
        b'A' => "new file:",
        b'D' => "deleted:",
        b'R' => "renamed:",
        b'C' => "copied:",
        b'T' => "typechange:",
        _ => "modified:",
    }
}

fn unmerged_label(bits: u8) -> &'static str {
    match bits {
        1 => "both deleted:",
        2 => "added by us:",
        3 => "deleted by them:",
        4 => "added by them:",
        5 => "deleted by us:",
        6 => "both added:",
        _ => "both modified:",
    }
}

fn entry(out: &mut Vec<u8>, label: &str, width: usize, name: &[u8]) {
    out.push(b'\t');
    put(out, &format!("{label:<width$}"));
    out.extend_from_slice(name);
    out.push(b'\n');
}

/// A primeira palavra do `.git/sequencer/todo`: o comando que a sequência está executando.
fn sequencer_command(repo: &Repo) -> Option<&'static str> {
    let todo = os::read_opt(&repo.path("sequencer/todo")).ok()??;
    let line = todo.split(|c| *c == b'\n').find(|l| !l.is_empty() && !l.starts_with(b"#"))?;
    let word = line.split(|c| *c == b' ').next()?;
    match word {
        b"pick" | b"p" => Some("pick"),
        b"revert" => Some("revert"),
        _ => None,
    }
}

/// O bloco "mesclagem / cherry-pick / revert em andamento" do status (o `wt_longstatus_print_state`).
fn state_text(repo: &Repo, has_unmerged: bool, hints: bool) -> String {
    let mut out = String::new();
    let read_head = |name: &str| -> Option<crate::hash::Oid> {
        let d = os::read_opt(&repo.path(name)).ok()??;
        crate::hash::Oid::from_hex(crate::object::trim_ascii(&d))
    };
    if os::exists(&repo.path("MERGE_HEAD")) {
        if has_unmerged {
            out.push_str("You have unmerged paths.\n");
            if hints {
                out.push_str("  (fix conflicts and run \"git commit\")\n  (use \"git merge --abort\" to abort the merge)\n");
            }
        } else {
            out.push_str("All conflicts fixed but you are still merging.\n");
            if hints {
                out.push_str("  (use \"git commit\" to conclude merge)\n");
            }
        }
        out.push('\n');
        return out;
    }
    let seq = sequencer_command(repo);
    let cherry_head = read_head("CHERRY_PICK_HEAD");
    let revert_head = read_head("REVERT_HEAD");
    let (name, head, picking) = if os::exists(&repo.path("CHERRY_PICK_HEAD")) || (seq == Some("pick") && revert_head.is_none()) {
        ("cherry-pick", cherry_head, true)
    } else if os::exists(&repo.path("REVERT_HEAD")) || seq == Some("revert") {
        ("revert", revert_head, false)
    } else {
        return out;
    };
    let _ = picking;
    match head {
        None => out.push_str(&format!("{} currently in progress.\n", if name == "revert" { "Revert" } else { "Cherry-pick" })),
        Some(h) => out.push_str(&format!("You are currently {} commit {}.\n", if name == "revert" { "reverting" } else { "cherry-picking" }, repo.abbrev_default(&h))),
    }
    if hints {
        if has_unmerged {
            out.push_str(&format!("  (fix conflicts and run \"git {name} --continue\")\n"));
        } else if head.is_none() {
            out.push_str(&format!("  (run \"git {name} --continue\" to continue)\n"));
        } else {
            out.push_str(&format!("  (all conflicts fixed: run \"git {name} --continue\")\n"));
        }
        out.push_str(&format!("  (use \"git {name} --skip\" to skip this patch)\n"));
        out.push_str(&format!("  (use \"git {name} --abort\" to cancel the {name} operation)\n"));
    }
    out.push('\n');
    out
}

#[allow(clippy::too_many_arguments)]
fn long_format(
    repo: &Repo,
    st: &Status,
    hints: bool,
    fully: bool,
    show_untracked: bool,
    template: bool,
    tracking_line: &str,
    branch_line: &str,
) -> R<Vec<u8>> {
    let mut out = Vec::new();
    put(&mut out, branch_line);
    put(&mut out, tracking_line);
    if !tracking_line.is_empty() {
        out.push(b'\n');
    }
    if st.initial {
        put(&mut out, if template { "\nInitial commit\n\n" } else { "\nNo commits yet\n\n" });
    }
    put(&mut out, &state_text(repo, !st.unmerged.is_empty(), hints));
    // `determine_whence`: com merge ou cherry-pick em andamento o wt-status não dá dica de unstage.
    let from_commit = !os::exists(&repo.path("MERGE_HEAD")) && !os::exists(&repo.path("CHERRY_PICK_HEAD"));
    if !st.staged.is_empty() {
        put(&mut out, "Changes to be committed:\n");
        if hints && from_commit {
            if st.initial {
                put(&mut out, "  (use \"git rm --cached <file>...\" to unstage)\n");
            } else {
                put(&mut out, "  (use \"git restore --staged <file>...\" to unstage)\n");
            }
        }
        for p in &st.staged {
            let mut name = display(repo, p.path(), fully);
            if matches!(p.status, b'R' | b'C') {
                let mut full = display(repo, &p.one.path, fully);
                full.extend_from_slice(b" -> ");
                full.extend_from_slice(&name);
                name = full;
            }
            entry(&mut out, staged_label(p.status), 12, &name);
        }
        out.push(b'\n');
    }
    if !st.unmerged.is_empty() {
        put(&mut out, "Unmerged paths:\n");
        if hints {
            if !from_commit {
            } else if !st.initial {
                put(&mut out, "  (use \"git restore --staged <file>...\" to unstage)\n");
            } else {
                put(&mut out, "  (use \"git rm --cached <file>...\" to unstage)\n");
            }
            if st.unmerged.iter().any(|(_, b)| matches!(b, 1 | 3 | 5)) {
                put(&mut out, "  (use \"git add/rm <file>...\" as appropriate to mark resolution)\n");
            } else {
                put(&mut out, "  (use \"git add <file>...\" to mark resolution)\n");
            }
        }
        for (path, bits) in &st.unmerged {
            entry(&mut out, unmerged_label(*bits), 17, &display(repo, path, fully));
        }
        out.push(b'\n');
    }
    if !st.unstaged.is_empty() {
        put(&mut out, "Changes not staged for commit:\n");
        if hints {
            if st.unstaged.iter().any(|p| p.status == b'D') {
                put(&mut out, "  (use \"git add/rm <file>...\" to update what will be committed)\n");
            } else {
                put(&mut out, "  (use \"git add <file>...\" to update what will be committed)\n");
            }
            put(&mut out, "  (use \"git restore <file>...\" to discard changes in working directory)\n");
        }
        for p in &st.unstaged {
            entry(&mut out, staged_label(p.status), 12, &display(repo, p.path(), fully));
        }
        out.push(b'\n');
    }
    if !st.untracked.is_empty() {
        put(&mut out, "Untracked files:\n");
        if hints {
            put(&mut out, "  (use \"git add <file>...\" to include in what will be committed)\n");
        }
        for path in &st.untracked {
            out.push(b'\t');
            out.extend_from_slice(&display(repo, path, fully));
            out.push(b'\n');
        }
        out.push(b'\n');
    }
    if !st.ignored.is_empty() {
        put(&mut out, "Ignored files:\n");
        if hints {
            put(&mut out, "  (use \"git add -f <file>...\" to include in what will be committed)\n");
        }
        for path in &st.ignored {
            out.push(b'\t');
            out.extend_from_slice(&display(repo, path, fully));
            out.push(b'\n');
        }
        out.push(b'\n');
    }
    // A linha final: o que falta para haver um commit.
    let committable = !st.staged.is_empty();
    if !committable && !template {
        let dirty = !st.unstaged.is_empty() || !st.unmerged.is_empty();
        let text = if dirty {
            if hints { "no changes added to commit (use \"git add\" and/or \"git commit -a\")" } else { "no changes added to commit" }
        } else if !st.untracked.is_empty() {
            if hints { "nothing added to commit but untracked files present (use \"git add\" to track)" } else { "nothing added to commit but untracked files present" }
        } else if st.initial {
            if hints { "nothing to commit (create/copy files and use \"git add\" to track)" } else { "nothing to commit" }
        } else if !show_untracked {
            if hints { "nothing to commit (use -u to show untracked files)" } else { "nothing to commit" }
        } else {
            "nothing to commit, working tree clean"
        };
        put(&mut out, text);
        out.push(b'\n');
    }
    Ok(out)
}

// ---- formato curto ----------------------------------------------------------------------------

/// Caminho no formato curto: aspas também quando há espaço; com `-z` o nome vai cru.
fn short_name(repo: &Repo, path: &[u8], relative: bool, fully: bool, nul: bool) -> Vec<u8> {
    let shown = if relative { repo.display_path(path) } else { path.to_vec() };
    if nul {
        return shown;
    }
    if shown.contains(&b' ') && !quote::needs_quote(&shown, fully) {
        let mut q = vec![b'"'];
        q.extend_from_slice(&shown);
        q.push(b'"');
        return q;
    }
    quote::quote_c(&shown, fully)
}

fn unmerged_code(bits: u8) -> &'static [u8; 2] {
    match bits {
        1 => b"DD",
        2 => b"AU",
        3 => b"UD",
        4 => b"UA",
        5 => b"DU",
        6 => b"AA",
        _ => b"UU",
    }
}

fn short_format(repo: &Repo, st: &Status, relative: bool, fully: bool, nul: bool) -> Vec<u8> {
    struct Row<'a> {
        path: &'a [u8],
        code: [u8; 2],
        orig: Option<&'a [u8]>,
    }
    let mut rows: Vec<Row<'_>> = Vec::new();
    for p in &st.staged {
        let orig = matches!(p.status, b'R' | b'C').then_some(p.one.path.as_slice());
        rows.push(Row { path: p.path(), code: [p.status, b' '], orig });
    }
    for p in &st.unstaged {
        match rows.iter_mut().find(|r| r.path == p.path()) {
            Some(r) => r.code[1] = p.status,
            None => rows.push(Row { path: p.path(), code: [b' ', p.status], orig: None }),
        }
    }
    for (path, bits) in &st.unmerged {
        rows.push(Row { path, code: *unmerged_code(*bits), orig: None });
    }
    rows.sort_by(|a, b| a.path.cmp(b.path));
    let mut out = Vec::new();
    for r in &rows {
        out.extend_from_slice(&r.code);
        out.push(b' ');
        match r.orig {
            Some(orig) if nul => {
                out.extend_from_slice(&short_name(repo, r.path, relative, fully, true));
                out.push(0);
                out.extend_from_slice(&short_name(repo, orig, relative, fully, true));
                out.push(0);
            }
            Some(orig) => {
                out.extend_from_slice(&short_name(repo, orig, relative, fully, false));
                out.extend_from_slice(b" -> ");
                out.extend_from_slice(&short_name(repo, r.path, relative, fully, false));
                out.push(b'\n');
            }
            None => {
                out.extend_from_slice(&short_name(repo, r.path, relative, fully, nul));
                out.push(if nul { 0 } else { b'\n' });
            }
        }
    }
    for (list, mark) in [(&st.untracked, "?? "), (&st.ignored, "!! ")] {
        for path in list {
            put(&mut out, mark);
            out.extend_from_slice(&short_name(repo, path, relative, fully, nul));
            out.push(if nul { 0 } else { b'\n' });
        }
    }
    out
}

/// A linha `## ...` do `-b`.
fn short_branch_line(repo: &Repo, head: &crate::refs::Head, initial: bool, ahead_behind: bool) -> R<String> {
    let mut line = String::from("## ");
    match head {
        crate::refs::Head::Detached(_) => line.push_str("HEAD (no branch)"),
        crate::refs::Head::Branch(name, _) => {
            let short = name.strip_prefix("refs/heads/").unwrap_or(name);
            if initial {
                line.push_str(&format!("No commits yet on {short}"));
            } else {
                line.push_str(short);
                if let Some(t) = tracking(repo, name, ahead_behind)? {
                    line.push_str(&format!("...{}", t.name));
                    match t.counts {
                        None => line.push_str(" [gone]"),
                        Some((0, 0)) => {}
                        Some((a, 0)) => line.push_str(&format!(" [ahead {a}]")),
                        Some((0, b)) => line.push_str(&format!(" [behind {b}]")),
                        Some((a, b)) => line.push_str(&format!(" [ahead {a}, behind {b}]")),
                    }
                }
            }
        }
    }
    line.push('\n');
    Ok(line)
}

pub fn run(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let p = opts::parse(SPECS, args, 0, usage)?;
    let repo = git.repo()?;
    let cfg = &repo.config;
    let porcelain = p.present("porcelain");
    if let Some(v) = p.value("porcelain")
        && v != b"v1"
    {
        return Err(Fail::Fatal(format!("unsupported porcelain version '{}'", os::lossy(v))));
    }
    let nul = p.has("nul");
    let short = p.has("short") || porcelain;
    let long = p.flag("long") == Some(true);
    let mode_text = match p.present("untracked") {
        true => p.value_str("untracked").unwrap_or_else(|| "all".to_string()),
        false => cfg.get("status.showuntrackedfiles").unwrap_or_else(|| "normal".to_string()),
    };
    let untracked = untracked_mode(&mode_text)?;
    let ignored = match p.present("ignored") {
        false => IgnoredMode::No,
        true => match p.value_str("ignored").as_deref() {
            None | Some("traditional") => IgnoredMode::Traditional,
            Some("matching") => IgnoredMode::Matching,
            Some("no") => IgnoredMode::No,
            Some(other) => return Err(Fail::Fatal(format!("Invalid ignored mode '{other}'"))),
        },
    };
    let renames = rename_opts(repo, p.flag("renames"))?;
    let ps = git.pathspec(&p.args)?;
    let st = collect(repo, &ps, renames, untracked, ignored)?;
    let fully = super::ls::quote_fully(repo);
    let hints = cfg.get_bool("advice.statushints")?.unwrap_or(true);
    let ahead_behind = p.flag("ahead-behind").unwrap_or_else(|| cfg.get_bool("status.aheadbehind").ok().flatten().unwrap_or(true));
    let head = repo.head()?;
    if short && !long {
        let show_branch = p.has("branch") || (!porcelain && cfg.get_bool("status.branch")?.unwrap_or(false));
        let mut out = Vec::new();
        if show_branch {
            let line = short_branch_line(repo, &head, st.initial, ahead_behind)?;
            if nul {
                out.extend_from_slice(line.trim_end_matches('\n').as_bytes());
                out.push(0);
            } else {
                out.extend_from_slice(line.as_bytes());
            }
        }
        out.extend_from_slice(&short_format(repo, &st, !porcelain, fully, nul));
        os::out(&out);
        return Ok(0);
    }
    let out = render_long(repo, &st, &head, untracked != UntrackedMode::No, false, hints, ahead_behind)?;
    os::out(&out);
    Ok(0)
}

/// O formato longo completo: linha do ramo, upstream, seções e a linha final.
fn render_long(repo: &Repo, st: &Status, head: &crate::refs::Head, show_untracked: bool, template: bool, hints: bool, ahead_behind: bool) -> R<Vec<u8>> {
    let fully = super::ls::quote_fully(repo);
    let (branch_line, tracking_line) = match head {
        crate::refs::Head::Detached(id) => {
            let (text, at) = super::branch::detached_from(repo, id)?;
            (format!("HEAD detached {} {text}\n", if at { "at" } else { "from" }), String::new())
        }
        crate::refs::Head::Branch(name, _) => {
            let short_name = name.strip_prefix("refs/heads/").unwrap_or(name);
            let line = format!("On branch {short_name}\n");
            let track = if st.initial { None } else { tracking(repo, name, ahead_behind)? };
            (line, track.map(|t| tracking_text(&t, hints)).unwrap_or_default())
        }
    };
    long_format(repo, st, hints, fully, show_untracked, template, &tracking_line, &branch_line)
}

/// Status longo na saída padrão, do jeito que o `git commit` o mostra quando não há o que gravar.
pub fn print_for_commit(repo: &Repo) -> R<()> {
    let cfg = &repo.config;
    let mode = untracked_mode(&cfg.get("status.showuntrackedfiles").unwrap_or_else(|| "normal".to_string()))?;
    let renames = rename_opts(repo, None)?;
    let st = collect(repo, &Pathspec::default(), renames, mode, IgnoredMode::No)?;
    let hints = cfg.get_bool("advice.statushints")?.unwrap_or(true);
    let head = repo.head()?;
    let out = render_long(repo, &st, &head, mode != UntrackedMode::No, false, hints, true)?;
    os::out(&out);
    Ok(())
}

/// O estado como comentário, para o modelo da mensagem de commit (sem dicas nem linha final):
/// cada linha com `# ` na frente.
pub fn commit_template(repo: &Repo, ps: &Pathspec) -> R<Vec<u8>> {
    let cfg = &repo.config;
    let mode = untracked_mode(&cfg.get("status.showuntrackedfiles").unwrap_or_else(|| "normal".to_string()))?;
    let renames = rename_opts(repo, None)?;
    let st = collect(repo, ps, renames, mode, IgnoredMode::No)?;
    let head = repo.head()?;
    let body = render_long(repo, &st, &head, mode != UntrackedMode::No, true, false, true)?;
    let mut out = Vec::new();
    for line in body.split_inclusive(|c| *c == b'\n') {
        if line == b"\n" {
            out.extend_from_slice(b"#\n");
        } else if line.starts_with(b"\t") {
            out.push(b'#');
            out.extend_from_slice(line);
        } else {
            out.extend_from_slice(b"# ");
            out.extend_from_slice(line);
        }
    }
    Ok(out)
}
