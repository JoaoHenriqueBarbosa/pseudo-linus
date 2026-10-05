//! `git add` e o que pôr arquivo no índice significa (reaproveitado por commit, stash, mv...).

use super::Git;
use crate::diff::{self, WtState};
use crate::error::{Fail, R, hint, warning};
use crate::hash::{Kind, Oid};
use crate::ignore::Ignores;
use crate::index::{self, EXT_INTENT_TO_ADD, IEntry, Index};
use crate::object;
use crate::opts::{self, Spec};
use crate::os;
use crate::pathspec::{Hit, Pathspec};
use crate::repo::Repo;
use crate::worktree::{IgnoredMode, Scanner, UntrackedMode};

const SPECS: &[Spec] = &[
    opts::flag(Some(b'n'), "dry-run", "dry-run"),
    opts::flag(Some(b'v'), "verbose", "verbose"),
    opts::flag(Some(b'i'), "interactive", "interactive"),
    opts::flag(Some(b'p'), "patch", "patch"),
    opts::flag(Some(b'e'), "edit", "edit"),
    opts::flag(Some(b'f'), "force", "force"),
    opts::flag(Some(b'u'), "update", "update"),
    opts::flag(None, "renormalize", "renormalize"),
    opts::flag(Some(b'N'), "intent-to-add", "intent-to-add"),
    opts::flag(Some(b'A'), "all", "all"),
    opts::flag(None, "ignore-removal", "ignore-removal"),
    opts::flag(None, "refresh", "refresh"),
    opts::flag(None, "ignore-errors", "ignore-errors"),
    opts::flag(None, "ignore-missing", "ignore-missing"),
    opts::flag(None, "sparse", "sparse"),
    opts::value(None, "chmod", "chmod"),
    opts::value(None, "pathspec-from-file", "pathspec-from-file"),
    opts::flag(None, "pathspec-file-nul", "pathspec-file-nul"),
];

/// HEAD de um repositório aninhado (pra adicionar como gitlink).
pub fn nested_head(path: &[u8]) -> Option<Oid> {
    let gd = os::join(path, b".git");
    let dir = if os::is_dir(&gd) {
        gd
    } else {
        let d = os::read_opt(&gd).ok()??;
        let rest = object::trim_ascii(d.strip_prefix(b"gitdir: ")?);
        if rest.starts_with(b"/") { rest.to_vec() } else { os::join(path, rest) }
    };
    let head = os::read_opt(&os::join(&dir, b"HEAD")).ok()??;
    let head = object::trim_ascii(&head);
    match head.strip_prefix(b"ref: ") {
        Some(r) => {
            if let Ok(Some(v)) = os::read_opt(&os::join(&dir, r)) {
                return Oid::from_hex(object::trim_ascii(&v));
            }
            // Ref empacotada.
            let packed = os::read_opt(&os::join(&dir, b"packed-refs")).ok()??;
            for line in packed.split(|c| *c == b'\n') {
                if line.len() > 41 && &line[41..] == r {
                    return Oid::from_hex(&line[..40]);
                }
            }
            None
        }
        None => Oid::from_hex(head),
    }
}

/// Grava o conteúdo atual de `path` (relativo ao topo) no índice.
pub fn stage_path(repo: &Repo, idx: &mut Index, path: &[u8], trust_exec: bool, chmod: Option<bool>, intent: bool) -> R<()> {
    let st = os::lstat(path).map_err(|e| Fail::Fatal(format!("unable to stat '{}': {}", os::lossy(path), e.message())))?;
    if st.file_type() == sysabi::FileType::Directory {
        let id = nested_head(path).ok_or_else(|| Fail::Fatal(format!("'{}' does not have a commit checked out", os::lossy(path))))?;
        idx.add(IEntry::from_stat(path.to_vec(), id, object::MODE_GITLINK, &st));
        return Ok(());
    }
    let old = idx.get(path).map(|e| e.mode);
    let mut mode = index::mode_for(&st, old, trust_exec);
    if let Some(x) = chmod
        && object::is_reg(mode)
    {
        mode = if x { object::MODE_EXEC } else { object::MODE_BLOB };
    }
    let mut e = if intent {
        let mut e = IEntry::from_stat(path.to_vec(), crate::hash::EMPTY_BLOB, mode, &st);
        e.ext |= EXT_INTENT_TO_ADD;
        e
    } else {
        let data = diff::worktree_blob(path, &st)?;
        let id = repo.write_object(Kind::Blob, &data)?;
        IEntry::from_stat(path.to_vec(), id, mode, &st)
    };
    if let Some(prev) = idx.get(path) {
        e.flags |= prev.flags & index::FLAG_ASSUME_VALID;
    }
    idx.add(e);
    Ok(())
}

const EMBEDDED_HINT: &str = "You've added another git repository inside your current repository.\nClones of the outer repository will not contain the contents of\nthe embedded repository and will not know how to obtain it.\nIf you meant to add a submodule, use:\n\n\tgit submodule add <url> {P}\n\nIf you added this path by mistake, you can remove it from the\nindex with:\n\n\tgit rm --cached {P}\n\nSee \"git help submodule\" for more information.\nDisable this message with \"git config advice.addEmbeddedRepo false\"";

pub fn run(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let p = opts::parse(SPECS, args, 0, usage)?;
    if p.has("interactive") || p.has("patch") || p.has("edit") {
        return Err(Fail::Fatal("interactive add needs a terminal; this sandbox has none (use `git add <path>`)".into()));
    }
    let repo = git.repo()?;
    let mut paths = p.args.clone();
    if let Some(f) = p.value("pathspec-from-file") {
        let data = if f == b"-" { os::stdin_all() } else { super::plumbing::read_user_file(repo, f)? };
        let sep = if p.has("pathspec-file-nul") { 0 } else { b'\n' };
        paths.extend(data.split(|c| *c == sep).filter(|l| !l.is_empty()).map(|l| l.to_vec()));
    }
    let chmod = match p.value("chmod") {
        None => None,
        Some(b"+x") => Some(true),
        Some(b"-x") => Some(false),
        Some(v) => return Err(Fail::Fatal(format!("--chmod param '{}' must be either -x or +x", os::lossy(v)))),
    };
    let update = p.has("update");
    let all = p.flag("all");
    let take_removals = !p.has("ignore-removal") && all != Some(false);
    if paths.is_empty() && !update && all != Some(true) {
        if chmod.is_some() || p.has("refresh") {
            return Ok(0);
        }
        os::outs("Nothing specified, nothing added.\n");
        if repo.config.get_bool("advice.addemptypathspec")?.unwrap_or(true) {
            hint("Maybe you wanted to say 'git add .'?\nDisable this message with \"git config advice.addEmptyPathspec false\"");
        }
        return Ok(0);
    }
    // `-A`/`-u` sem pathspec valem pra árvore toda.
    let ps = if paths.is_empty() { Pathspec::default() } else { git.pathspec(&paths)? };
    let trust = repo.config.get_bool("core.filemode")?.unwrap_or(true);
    let ipath = repo.index_path();
    let mut idx = Index::load(&ipath)?;
    let dry = p.has("dry-run");
    let verbose = p.has("verbose") || dry;
    let force = p.has("force");
    let mut seen = vec![false; ps.items.len()];
    let mut out = String::new();
    let mut changed = false;
    if p.has("refresh") {
        if crate::worktree::refresh(&mut idx, trust) {
            idx.write(&ipath)?;
        }
        return Ok(0);
    }
    // Rastreados: modificações e remoções.
    let tracked: Vec<IEntry> = idx.entries.clone();
    let mut last: Option<Vec<u8>> = None;
    for e in &tracked {
        if ps.matches(&e.path, false, Some(&mut seen)).is_none() {
            continue;
        }
        if last.as_deref() == Some(e.path.as_slice()) {
            continue;
        }
        last = Some(e.path.clone());
        if e.stage != 0 {
            // Conflito resolvido no arquivo: entra o conteúdo atual.
            if os::lstat(&e.path).is_ok() {
                if verbose {
                    out.push_str(&format!("add '{}'\n", os::lossy(&e.path)));
                }
                if !dry {
                    stage_path(repo, &mut idx, &e.path, trust, chmod, false)?;
                    changed = true;
                }
            } else if take_removals {
                if verbose {
                    out.push_str(&format!("remove '{}'\n", os::lossy(&e.path)));
                }
                if !dry {
                    idx.remove(&e.path);
                    changed = true;
                }
            }
            continue;
        }
        let state = diff::check_entry(repo, &idx, e, trust)?;
        match state {
            WtState::Deleted => {
                if take_removals {
                    if verbose {
                        out.push_str(&format!("remove '{}'\n", os::lossy(&e.path)));
                    }
                    if !dry {
                        idx.remove(&e.path);
                        changed = true;
                    }
                }
            }
            WtState::Changed(..) => {
                if verbose {
                    out.push_str(&format!("add '{}'\n", os::lossy(&e.path)));
                }
                if !dry {
                    stage_path(repo, &mut idx, &e.path, trust, chmod, false)?;
                    changed = true;
                }
            }
            WtState::Same => {
                if let Some(x) = chmod
                    && !dry
                    && object::is_reg(e.mode)
                {
                    let want = if x { object::MODE_EXEC } else { object::MODE_BLOB };
                    if let Ok(k) = idx.pos(&e.path, 0)
                        && idx.entries[k].mode != want
                    {
                        idx.entries[k].mode = want;
                        changed = true;
                    }
                } else if e.intent_to_add() && !dry {
                    stage_path(repo, &mut idx, &e.path, trust, chmod, false)?;
                    changed = true;
                }
            }
        }
    }
    // Não rastreados.
    let mut ignored_named: Vec<Vec<u8>> = Vec::new();
    if !update {
        let mut sc = Scanner {
            idx: &idx,
            ign: Ignores::standard(repo),
            ps: &ps,
            untracked: UntrackedMode::All,
            ignored: if force { IgnoredMode::No } else { IgnoredMode::Matching },
            show_empty_dirs: false,
        };
        if force {
            sc.ign = Ignores::none();
        }
        let res = sc.run();
        let mut new_paths: Vec<Vec<u8>> = Vec::new();
        for path in res.untracked {
            let is_dir = path.ends_with(b"/");
            let clean = path.strip_suffix(b"/").unwrap_or(&path).to_vec();
            if ps.matches(&clean, is_dir, Some(&mut seen)).is_none() {
                continue;
            }
            new_paths.push(clean);
        }
        // Ignorados citados explicitamente na pathspec.
        for path in res.ignored {
            let clean = path.strip_suffix(b"/").unwrap_or(&path).to_vec();
            for (i, it) in ps.items.iter().enumerate() {
                if let Some(h) = crate::pathspec::match_item(it, &clean, path.ends_with(b"/"))
                    && (h == Hit::Exact || (!it.path.is_empty() && !crate::wildmatch::has_glob(&it.path) && clean.starts_with(&it.path)))
                {
                    seen[i] = true;
                    if !ignored_named.contains(&it.path) && h == Hit::Exact {
                        ignored_named.push(clean.clone());
                    }
                }
            }
        }
        for path in new_paths {
            let st = os::lstat(&path).ok();
            if st.as_ref().is_some_and(|s| s.file_type() == sysabi::FileType::Directory) {
                // Repositório aninhado.
                warning(&format!("adding embedded git repository: {}", os::lossy(&path)));
                if repo.config.get_bool("advice.addembeddedrepo")?.unwrap_or(true) {
                    hint(&EMBEDDED_HINT.replace("{P}", &os::lossy(&path)));
                }
            }
            if verbose {
                out.push_str(&format!("add '{}'\n", os::lossy(&path)));
            }
            if !dry {
                stage_path(repo, &mut idx, &path, trust, chmod, p.has("intent-to-add"))?;
                changed = true;
            }
        }
    }
    os::outs(&out);
    // Pathspec que não casou com nada.
    if !p.has("ignore-missing") {
        if let Some(it) = ps.unmatched(&seen).into_iter().next() {
            // Caminho que existe só como diretório vazio também não casa.
            if changed && !dry {
                idx.write(&ipath)?;
            }
            return Err(Fail::Fatal(format!("pathspec '{}' did not match any files", os::lossy(&it.orig))));
        }
    }
    if changed && !dry {
        idx.write(&ipath)?;
    }
    if !ignored_named.is_empty() {
        let mut msg = String::from("The following paths are ignored by one of your .gitignore files:\n");
        for pth in &ignored_named {
            msg.push_str(&os::lossy(&repo.display_path(pth)));
            msg.push('\n');
        }
        os::errs(&msg);
        if repo.config.get_bool("advice.addignoredfile")?.unwrap_or(true) {
            hint("Use -f if you really want to add them.\nDisable this message with \"git config advice.addIgnoredFile false\"");
        }
        return Ok(1);
    }
    Ok(0)
}
