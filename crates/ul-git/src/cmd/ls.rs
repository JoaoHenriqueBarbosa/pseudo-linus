//! `git ls-files` e `git ls-tree`.

use super::Git;
use crate::diff::{self, WtState};
use crate::error::{Fail, R, error};
use crate::ignore::Ignores;
use crate::index::Index;
use crate::object;
use crate::opts::{self, Spec};
use crate::os;
use crate::pathspec::Pathspec;
use crate::quote;
use crate::repo::{Repo, relative_to};
use crate::worktree::{IgnoredMode, Scanner, UntrackedMode};

const LS_FILES: &[Spec] = &[
    opts::flag(Some(b'c'), "cached", "cached"),
    opts::flag(Some(b'd'), "deleted", "deleted"),
    opts::flag(Some(b'm'), "modified", "modified"),
    opts::flag(Some(b'o'), "others", "others"),
    opts::flag(Some(b'i'), "ignored", "ignored"),
    opts::flag(Some(b's'), "stage", "stage"),
    opts::flag(Some(b'u'), "unmerged", "unmerged"),
    opts::flag(Some(b'k'), "killed", "killed"),
    opts::short_flag(b'z', "z"),
    opts::short_flag(b't', "tag"),
    opts::short_flag(b'v', "lower-tag"),
    opts::short_flag(b'f', "fsmonitor-tag"),
    opts::flag(None, "exclude-standard", "exclude-standard"),
    opts::value(Some(b'x'), "exclude", "exclude"),
    opts::value(Some(b'X'), "exclude-from", "exclude-from"),
    opts::value(None, "exclude-per-directory", "exclude-per-dir"),
    opts::flag(None, "directory", "directory"),
    opts::flag(None, "no-empty-directory", "no-empty-directory"),
    opts::flag(None, "empty-directory", "empty-directory"),
    opts::flag(None, "error-unmatch", "error-unmatch"),
    opts::flag(None, "full-name", "full-name"),
    opts::optional(None, "abbrev", "abbrev"),
    opts::value(None, "format", "format"),
    opts::flag(None, "deduplicate", "dedup"),
    opts::flag(None, "eol", "eol"),
    opts::flag(None, "recurse-submodules", "recurse"),
    opts::flag(None, "sparse", "sparse"),
    opts::value(None, "with-tree", "with-tree"),
    opts::flag(None, "resolve-undo", "resolve-undo"),
    opts::flag(None, "debug", "debug"),
];

pub fn ls_files(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let p = opts::parse(LS_FILES, args, 0, usage)?;
    let repo = git.repo()?;
    let full = p.has("full-name");
    let ps = git.pathspec(&p.args)?;
    // Sem pathspec, fica no diretório atual.
    let ps = if p.args.is_empty() && !repo.prefix.is_empty() {
        git.pathspec(&[b".".to_vec()])?
    } else {
        ps
    };
    let idx = Index::load(&repo.index_path())?;
    let trust = repo.config.get_bool("core.filemode")?.unwrap_or(true);
    let z = p.has("z");
    let term = if z { 0 } else { b'\n' };
    let tag = p.has("tag") || p.has("lower-tag");
    let abbrev: Option<usize> = if p.present("abbrev") { Some(p.value("abbrev").and_then(|v| os::lossy(v).parse().ok()).unwrap_or_else(|| repo.abbrev_len())) } else { None };
    let show_stage = p.has("stage") || p.has("unmerged");
    let others = p.has("others");
    let ignored = p.has("ignored");
    let mut show_cached = p.has("cached");
    let (deleted, modified, unmerged, killed) = (p.has("deleted"), p.has("modified"), p.has("unmerged"), p.has("killed"));
    if !(show_cached || deleted || modified || others || unmerged || killed || p.has("stage")) {
        show_cached = true;
    }
    if p.has("stage") {
        show_cached = true;
    }
    if ignored && !others && !show_cached {
        return Err(Fail::Fatal("ls-files -i must be used with either -o or -c".into()));
    }
    let mut ign = Ignores::standard(repo);
    if !p.has("exclude-standard") {
        ign = Ignores::none();
    }
    for x in p.values("exclude") {
        ign.add_cmdline(&x);
    }
    for f in p.values("exclude-from") {
        let data = os::read(&f).map_err(|e| Fail::Fatal(format!("cannot use {} as an exclude file: {}", os::lossy(&f), e.message())))?;
        ign.add_file_patterns(&data, &f);
    }
    let name_of = |path: &[u8]| -> Vec<u8> {
        let shown = if full { path.to_vec() } else { relative_to(path, &repo.prefix) };
        if z { shown } else { quote::quote_c(&shown, quote_fully(repo)) }
    };
    let fmt_id = |id: &crate::hash::Oid| -> String {
        match abbrev {
            Some(n) => repo.abbrev(id, n),
            None => id.hex(),
        }
    };
    let mut out = Vec::new();
    let mut seen = vec![false; ps.items.len()];
    // Rastreados.
    let mut last_path: Vec<u8> = Vec::new();
    for e in &idx.entries {
        if ps.matches(&e.path, false, Some(&mut seen)).is_none() {
            continue;
        }
        let is_ignored = ignored && ign.is_ignored(&e.path, false);
        if ignored && !is_ignored {
            continue;
        }
        if show_cached || (unmerged && e.stage != 0) {
            if unmerged && !show_cached && e.stage == 0 {
                continue;
            }
            if p.has("dedup") && last_path == e.path {
                continue;
            }
            if tag {
                let t = if e.stage != 0 {
                    "M "
                } else if e.skip_worktree() {
                    "S "
                } else {
                    "H "
                };
                let t = if p.has("lower-tag") && e.assume_valid() { t.to_ascii_lowercase() } else { t.to_string() };
                out.extend_from_slice(t.as_bytes());
            }
            if let Some(f) = p.value("format") {
                out.extend_from_slice(&format_entry(f, e, &name_of(&e.path), &fmt_id(&e.oid)));
            } else if show_stage {
                out.extend_from_slice(format!("{:06o} {} {}\t", e.mode, fmt_id(&e.oid), e.stage).as_bytes());
                out.extend_from_slice(&name_of(&e.path));
            } else {
                out.extend_from_slice(&name_of(&e.path));
            }
            out.push(term);
            last_path = e.path.clone();
        }
        if (deleted || modified) && e.stage == 0 {
            let st = diff::check_entry(repo, &idx, e, trust)?;
            let is_del = matches!(st, WtState::Deleted);
            let is_mod = !matches!(st, WtState::Same);
            if deleted && is_del {
                if tag {
                    out.extend_from_slice(b"R ");
                }
                out.extend_from_slice(&name_of(&e.path));
                out.push(term);
            }
            if modified && is_mod && !(p.has("dedup") && deleted && is_del) {
                if tag {
                    out.extend_from_slice(b"C ");
                }
                out.extend_from_slice(&name_of(&e.path));
                out.push(term);
            }
        }
    }
    if others {
        let mode = if p.has("directory") { UntrackedMode::Normal } else { UntrackedMode::All };
        let mut sc = Scanner {
            idx: &idx,
            ign,
            ps: &ps,
            untracked: if ignored { UntrackedMode::No } else { mode },
            ignored: if ignored { IgnoredMode::Matching } else { IgnoredMode::No },
            show_empty_dirs: p.has("directory") && !p.has("no-empty-directory"),
        };
        let res = sc.run();
        let list = if ignored { res.ignored } else { res.untracked };
        for path in list {
            let mut s2 = vec![false; ps.items.len()];
            if ps.matches(path.strip_suffix(b"/").unwrap_or(&path), path.ends_with(b"/"), Some(&mut s2)).is_none() {
                continue;
            }
            for (i, v) in s2.iter().enumerate() {
                if *v {
                    seen[i] = true;
                }
            }
            if tag {
                out.extend_from_slice(b"? ");
            }
            out.extend_from_slice(&name_of(&path));
            out.push(term);
        }
    }
    os::out(&out);
    if p.has("error-unmatch") {
        let missing = ps.unmatched(&seen);
        if !missing.is_empty() {
            for m in &missing {
                error(&format!("pathspec '{}' did not match any file(s) known to git", os::lossy(&m.orig)));
            }
            os::errs("Did you forget to 'git add'?\n");
            return Ok(1);
        }
    }
    Ok(0)
}

pub fn quote_fully(repo: &Repo) -> bool {
    repo.config.get_bool("core.quotepath").ok().flatten().unwrap_or(true)
}

fn format_entry(fmt: &[u8], e: &crate::index::IEntry, name: &[u8], id: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < fmt.len() {
        if fmt[i..].starts_with(b"%(")
            && let Some(end) = fmt[i..].iter().position(|c| *c == b')')
        {
            match &fmt[i + 2..i + end] {
                b"objectmode" => out.extend_from_slice(format!("{:06o}", e.mode).as_bytes()),
                b"objectname" => out.extend_from_slice(id.as_bytes()),
                b"objecttype" => out.extend_from_slice(object::kind_of_mode(e.mode).name().as_bytes()),
                b"objectsize" => out.extend_from_slice(e.size.to_string().as_bytes()),
                b"stage" => out.extend_from_slice(e.stage.to_string().as_bytes()),
                b"path" => out.extend_from_slice(name),
                b"ctime" => out.extend_from_slice(e.ctime.0.to_string().as_bytes()),
                b"mtime" => out.extend_from_slice(e.mtime.0.to_string().as_bytes()),
                other => {
                    out.extend_from_slice(b"%(");
                    out.extend_from_slice(other);
                    out.push(b')');
                }
            }
            i += end + 1;
            continue;
        }
        if fmt[i] == b'%' && fmt.get(i + 1) == Some(&b'x') && i + 3 < fmt.len() + 1 {
            if let (Some(a), Some(b)) = (fmt.get(i + 2).and_then(|c| crate::hash::hex_val(*c)), fmt.get(i + 3).and_then(|c| crate::hash::hex_val(*c))) {
                out.push((a << 4) | b);
                i += 4;
                continue;
            }
        }
        if fmt[i] == b'%' && fmt.get(i + 1) == Some(&b'n') {
            out.push(b'\n');
            i += 2;
            continue;
        }
        out.push(fmt[i]);
        i += 1;
    }
    out
}

// ---- ls-tree ----------------------------------------------------------------------------------

const LS_TREE: &[Spec] = &[
    opts::short_flag(b'd', "dirs"),
    opts::short_flag(b'r', "recurse"),
    opts::short_flag(b't', "show-trees"),
    opts::flag(Some(b'l'), "long", "long"),
    opts::short_flag(b'z', "z"),
    opts::flag(None, "name-only", "name-only"),
    opts::flag(None, "name-status", "name-only"),
    opts::flag(None, "object-only", "object-only"),
    opts::flag(None, "full-name", "full-name"),
    opts::flag(None, "full-tree", "full-tree"),
    opts::optional(None, "abbrev", "abbrev"),
    opts::value(None, "format", "format"),
];

pub fn ls_tree(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let p = opts::parse(LS_TREE, args, 0, usage)?;
    let repo = git.repo()?;
    if p.args.is_empty() {
        return Err(opts::usage_help(usage));
    }
    let spec = &p.args[0];
    let id = repo.rev_parse(spec)?.ok_or_else(|| Fail::Fatal(format!("Not a valid object name {}", os::lossy(spec))))?;
    let tree = repo.peel_to_tree(&id)?.ok_or_else(|| Fail::Fatal("not a tree object".into()))?;
    let full_tree = p.has("full-tree");
    let full_name = p.has("full-name") || full_tree;
    let prefix: Vec<u8> = if full_tree { Vec::new() } else { repo.prefix.clone() };
    // Pathspecs do ls-tree são prefixos literais relativos ao cwd.
    let mut pats: Vec<Vec<u8>> = Vec::new();
    for a in &p.args[1..] {
        let joined = [prefix.as_slice(), a.as_slice()].concat();
        let n = crate::pathspec::normalize_rel(&joined).ok_or_else(|| Fail::Fatal(format!("'{}' is outside repository", os::lossy(a))))?;
        pats.push(n);
    }
    if pats.is_empty() && !prefix.is_empty() {
        pats.push(prefix.clone());
    }
    let recurse = p.has("recurse");
    let show_trees = p.has("show-trees");
    let dirs_only = p.has("dirs");
    let z = p.has("z");
    let abbrev: Option<usize> = if p.present("abbrev") { Some(p.value("abbrev").and_then(|v| os::lossy(v).parse().ok()).unwrap_or_else(|| repo.abbrev_len())) } else { None };
    let mut out = Vec::new();
    walk_ls_tree(repo, &tree, b"", &pats, recurse, show_trees, dirs_only, &mut |path, e| {
        let shown = if full_name { path.to_vec() } else { relative_to(path, &prefix) };
        let name = if z { shown } else { quote::quote_c(&shown, quote_fully(repo)) };
        let k = object::kind_of_mode(e.mode);
        let id = match abbrev {
            Some(n) => repo.abbrev(&e.oid, n),
            None => e.oid.hex(),
        };
        if let Some(f) = p.value("format") {
            let mut line = f.to_vec();
            let size = if k == crate::hash::Kind::Blob { repo.try_read(&e.oid).ok().flatten().map(|(_, d)| d.len().to_string()).unwrap_or_default() } else { "-".into() };
            for (pat, val) in [
                (&b"%(objectmode)"[..], format!("{:06o}", e.mode).into_bytes()),
                (b"%(objecttype)", k.name().as_bytes().to_vec()),
                (b"%(objectname)", id.clone().into_bytes()),
                (b"%(objectsize:padded)", format!("{size:>7}").into_bytes()),
                (b"%(objectsize)", size.into_bytes()),
                (b"%(path)", name.clone()),
            ] {
                line = replace_all(&line, pat, &val);
            }
            out.extend_from_slice(&line);
        } else if p.has("name-only") {
            out.extend_from_slice(&name);
        } else if p.has("object-only") {
            out.extend_from_slice(id.as_bytes());
        } else {
            out.extend_from_slice(format!("{:06o} {} {}", e.mode, k.name(), id).as_bytes());
            if p.has("long") {
                let size = if k == crate::hash::Kind::Blob {
                    repo.try_read(&e.oid).ok().flatten().map(|(_, d)| d.len().to_string()).unwrap_or_else(|| "-".into())
                } else {
                    "-".into()
                };
                out.extend_from_slice(format!(" {size:>7}").as_bytes());
            }
            out.push(b'\t');
            out.extend_from_slice(&name);
        }
        out.push(if z { 0 } else { b'\n' });
    })?;
    os::out(&out);
    Ok(0)
}

fn replace_all(hay: &[u8], pat: &[u8], val: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < hay.len() {
        if hay[i..].starts_with(pat) {
            out.extend_from_slice(val);
            i += pat.len();
        } else {
            out.push(hay[i]);
            i += 1;
        }
    }
    out
}

/// Percorre a tree no estilo do `ls-tree`: sem pathspec mostra o nível de cima; com pathspec
/// desce só no que leva até ela.
#[allow(clippy::too_many_arguments)]
fn walk_ls_tree(
    repo: &Repo,
    tree: &crate::hash::Oid,
    base: &[u8],
    pats: &[Vec<u8>],
    recurse: bool,
    show_trees: bool,
    dirs_only: bool,
    f: &mut dyn FnMut(&[u8], &object::TreeEntry),
) -> R<()> {
    for e in repo.read_tree(tree)? {
        let path = [base, e.name.as_slice()].concat();
        let is_tree = e.is_tree();
        // Relação com as pathspecs: casa, é pai de uma, ou nada.
        let (matched, leads) = if pats.is_empty() {
            (true, false)
        } else {
            let mut m = false;
            let mut l = false;
            for pat in pats {
                // `dir/` pede o conteúdo do diretório; `dir` pede a própria entrada.
                let contents = pat.ends_with(b"/");
                let pat = pat.strip_suffix(b"/").unwrap_or(pat);
                if pat.is_empty() {
                    m = true;
                } else if path == pat {
                    if contents && is_tree {
                        l = true;
                    } else {
                        m = true;
                    }
                } else if path.starts_with(pat) && path.get(pat.len()) == Some(&b'/') {
                    m = true;
                } else if pat.starts_with(&path) && pat.get(path.len()) == Some(&b'/') && is_tree {
                    l = true;
                }
            }
            (m, l)
        };
        if !matched && !leads {
            continue;
        }
        let mut sub = path.clone();
        sub.push(b'/');
        if is_tree {
            let descend = leads || (recurse && matched);
            if descend {
                if show_trees && (matched || leads) && recurse {
                    f(&path, &e);
                }
                walk_ls_tree(repo, &e.oid, &sub, pats, recurse, show_trees, dirs_only, f)?;
                continue;
            }
            if matched {
                f(&path, &e);
            }
        } else if matched && !dirs_only {
            f(&path, &e);
        }
    }
    Ok(())
}

/// Lista arquivos rastreados sob a pathspec (pra quem precisa só dos caminhos).
pub fn tracked_paths(idx: &Index, ps: &Pathspec) -> Vec<Vec<u8>> {
    idx.entries.iter().filter(|e| ps.matches_simple(&e.path)).map(|e| e.path.clone()).collect()
}
