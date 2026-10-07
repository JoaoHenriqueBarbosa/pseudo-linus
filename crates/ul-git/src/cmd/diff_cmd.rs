//! `git diff` e a família do plumbing (`diff-index`, `diff-files`, `diff-tree`), mais as opções de
//! saída de diff que `log`, `show` e `status` reaproveitam.

use std::cell::RefCell;
use std::collections::BTreeMap;

use super::Git;
use crate::config::Config;
use crate::diff::rename::{self, RenameOpts};
use crate::diff::{self, DiffOpts, Pair, Side};
use crate::error::{Fail, R, error_exit};
use crate::graph::Graph;
use crate::hash::{self, Kind, Oid};
use crate::index::Index;
use crate::object;
use crate::odb::Odb;
use crate::opts::{self, Parsed, Spec};
use crate::os;
use crate::pathspec::Pathspec;
use crate::repo::Repo;
use crate::rev;

/// Opções de saída de diff, comuns a todos os comandos que mostram diferenças.
pub const DIFF_SPECS: &[Spec] = &[
    opts::flag(Some(b'p'), "patch", "patch"),
    opts::short_flag(b'u', "patch"),
    opts::flag(Some(b's'), "no-patch", "no-patch"),
    opts::optional(None, "stat", "stat"),
    opts::flag(None, "numstat", "numstat"),
    opts::flag(None, "shortstat", "shortstat"),
    opts::flag(None, "name-only", "name-only"),
    opts::flag(None, "name-status", "name-status"),
    opts::flag(None, "raw", "raw"),
    opts::flag(None, "summary", "summary"),
    opts::flag(None, "patch-with-stat", "patch-with-stat"),
    opts::value(Some(b'U'), "unified", "unified"),
    opts::value(None, "inter-hunk-context", "inter-hunk-context"),
    opts::flag(None, "default-prefix", "default-prefix"),
    opts::flag(None, "no-prefix", "no-prefix"),
    opts::value(None, "src-prefix", "src-prefix"),
    opts::value(None, "dst-prefix", "dst-prefix"),
    opts::optional(None, "abbrev", "abbrev"),
    opts::flag(None, "full-index", "full-index"),
    opts::flag(None, "binary", "binary"),
    opts::flag(Some(b'a'), "text", "text"),
    opts::flag(Some(b'w'), "ignore-all-space", "ws-all"),
    opts::flag(Some(b'b'), "ignore-space-change", "ws-change"),
    opts::flag(None, "ignore-space-at-eol", "ws-eol"),
    opts::flag(None, "ignore-cr-at-eol", "ws-cr"),
    opts::optional(Some(b'M'), "find-renames", "find-renames"),
    opts::optional(Some(b'C'), "find-copies", "find-copies"),
    opts::flag(None, "renames", "renames"),
    opts::short_flag(b'R', "reverse"),
    opts::value(None, "diff-filter", "diff-filter"),
    opts::optional(None, "relative", "relative"),
    opts::short_flag(b'z', "nul"),
    opts::flag(None, "quiet", "quiet"),
    opts::flag(None, "exit-code", "exit-code"),
    opts::optional(None, "color", "color"),
    opts::flag(None, "ext-diff", "ext-diff"),
    opts::flag(None, "textconv", "textconv"),
    opts::value(None, "stat-width", "stat-width"),
    opts::value(None, "stat-name-width", "stat-name-width"),
    opts::value(None, "stat-graph-width", "stat-graph-width"),
    opts::value(None, "stat-count", "stat-count"),
];

fn number(usage: &str, name: &str, v: &[u8]) -> R<usize> {
    std::str::from_utf8(v)
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .ok_or_else(|| opts::usage_error(usage, &format!("{name} expects a numerical value")))
}

/// Monta as opções de saída a partir do que o parser viu. `plumbing` desliga a leitura de `diff.*`
/// e a detecção de renomeação (que o plumbing só faz com `-M`); `default_patch` liga o patch
/// quando nenhum formato foi pedido (o `git diff` faz, o `git log` não).
pub fn build_opts(git: &Git, p: &Parsed, plumbing: bool, default_patch: bool) -> R<DiffOpts> {
    let usage = git.usage();
    let cfg = git.config();
    let mut o = DiffOpts::default();
    if let Some(r) = &git.repo {
        o.quote_fully = super::ls::quote_fully(r);
    }
    if !plumbing {
        if let Some(n) = cfg.get_int("diff.context")? {
            o.hunk.context = n.max(0) as usize;
        }
        if let Some(n) = cfg.get_int("diff.interhunkcontext")? {
            o.hunk.interhunk = n.max(0) as usize;
        }
        if cfg.get_bool("diff.noprefix")?.unwrap_or(false) {
            o.src_prefix.clear();
            o.dst_prefix.clear();
        }
        let mut limit = RenameOpts::default().limit;
        if let Some(n) = cfg.get_int("diff.renamelimit")? {
            limit = n.max(0) as usize;
        }
        let renames = cfg.get("diff.renames").map(|v| v.to_ascii_lowercase());
        match renames.as_deref() {
            Some("copies") | Some("copy") => o.renames = Some(RenameOpts { copies: true, limit, ..RenameOpts::default() }),
            _ => {
                if cfg.get_bool("diff.renames")?.unwrap_or(true) {
                    o.renames = Some(RenameOpts { limit, ..RenameOpts::default() });
                }
            }
        }
    }
    let mut suppressed = false;
    let clear_formats = |o: &mut DiffOpts| {
        o.patch = false;
        o.stat = false;
        o.numstat = false;
        o.shortstat = false;
        o.name_only = false;
        o.name_status = false;
        o.raw = false;
        o.summary = false;
    };
    for h in &p.hits {
        let val = h.value.as_deref();
        match h.id {
            "patch" => {
                if h.negated {
                    clear_formats(&mut o);
                    suppressed = true;
                } else {
                    o.patch = true;
                }
            }
            "no-patch" => {
                clear_formats(&mut o);
                suppressed = true;
            }
            "stat" => {
                o.stat = true;
                if let Some(v) = val {
                    let text = os::lossy(v);
                    let mut parts = text.split(',');
                    if let Some(w) = parts.next().filter(|w| !w.is_empty()) {
                        o.stat_width = Some(number(usage, "--stat", w.as_bytes())?);
                    }
                    if let Some(w) = parts.next().filter(|w| !w.is_empty()) {
                        o.stat_name_width = Some(number(usage, "--stat", w.as_bytes())?);
                    }
                    if let Some(w) = parts.next().filter(|w| !w.is_empty()) {
                        o.stat_count = Some(number(usage, "--stat", w.as_bytes())?);
                    }
                }
            }
            "numstat" => o.numstat = true,
            "shortstat" => o.shortstat = true,
            "name-only" => o.name_only = true,
            "name-status" => o.name_status = true,
            "raw" => o.raw = true,
            "summary" => o.summary = true,
            "patch-with-stat" => {
                o.patch = true;
                o.stat = true;
            }
            "unified" => o.hunk.context = number(usage, "switch `U'", val.unwrap_or(b""))?,
            "inter-hunk-context" => o.hunk.interhunk = number(usage, "--inter-hunk-context", val.unwrap_or(b""))?,
            "default-prefix" => {
                o.src_prefix = b"a/".to_vec();
                o.dst_prefix = b"b/".to_vec();
            }
            "no-prefix" => {
                o.src_prefix.clear();
                o.dst_prefix.clear();
            }
            "src-prefix" => o.src_prefix = val.unwrap_or(b"").to_vec(),
            "dst-prefix" => o.dst_prefix = val.unwrap_or(b"").to_vec(),
            "abbrev" => {
                o.abbrev = if h.negated {
                    Some(40)
                } else {
                    match val {
                        Some(v) => Some(number(usage, "--abbrev", v)?.clamp(4, 40)),
                        None => None,
                    }
                }
            }
            "full-index" => o.full_index = !h.negated,
            "binary" => o.binary = !h.negated,
            "text" => o.text = !h.negated,
            "ws-all" => o.ws.all = !h.negated,
            "ws-change" => o.ws.change = !h.negated,
            "ws-eol" => o.ws.at_eol = !h.negated,
            "ws-cr" => o.ws.cr_at_eol = !h.negated,
            "find-renames" => {
                if h.negated {
                    o.renames = None;
                } else {
                    let text = os::lossy(val.unwrap_or(b""));
                    let score = rename::parse_score(&text).ok_or_else(|| Fail::Fatal(format!("invalid argument to -M: {text}")))?;
                    let copies = o.renames.is_some_and(|r| r.copies);
                    o.renames = Some(RenameOpts { min_score: score, copies, ..o.renames.unwrap_or_default() });
                }
            }
            "find-copies" => {
                if h.negated {
                    if let Some(r) = o.renames.as_mut() {
                        r.copies = false;
                    }
                } else {
                    let text = os::lossy(val.unwrap_or(b""));
                    let score = rename::parse_score(&text).ok_or_else(|| Fail::Fatal(format!("invalid argument to -C: {text}")))?;
                    o.renames = Some(RenameOpts { min_score: score, copies: true, ..o.renames.unwrap_or_default() });
                }
            }
            "renames" => {
                if h.negated {
                    o.renames = None;
                } else if o.renames.is_none() {
                    o.renames = Some(RenameOpts::default());
                }
            }
            "reverse" => o.reverse = true,
            "diff-filter" => o.filter = Some(val.unwrap_or(b"").to_vec()),
            "relative" => {
                if h.negated {
                    o.relative = None;
                } else {
                    o.relative = match val {
                        Some(v) => Some(v.to_vec()),
                        None => git.repo.as_ref().map(|r| r.prefix.clone()).filter(|p| !p.is_empty()),
                    };
                }
            }
            "nul" => o.null_terminated = true,
            "quiet" => {
                o.quiet = true;
                o.exit_code = true;
            }
            "exit-code" => o.exit_code = true,
            "color" => {
                let v = val.map(os::lossy);
                if !h.negated && !matches!(v.as_deref(), Some("never") | Some("auto") | Some("false") | Some("no")) {
                    return Err(Fail::Fatal("color output is not supported by this git".into()));
                }
            }
            "stat-width" => o.stat_width = Some(number(usage, "--stat-width", val.unwrap_or(b""))?),
            "stat-name-width" => o.stat_name_width = Some(number(usage, "--stat-name-width", val.unwrap_or(b""))?),
            "stat-graph-width" => o.stat_graph_width = Some(number(usage, "--stat-graph-width", val.unwrap_or(b""))?),
            "stat-count" => o.stat_count = Some(number(usage, "--stat-count", val.unwrap_or(b""))?),
            _ => {}
        }
    }
    if plumbing && !p.present("find-renames") && !p.present("find-copies") && !p.present("renames") {
        o.renames = None;
    }
    if default_patch && !suppressed && !o.any_format() {
        o.patch = true;
    }
    Ok(o)
}

/// Código de saída depois de mostrar o diff (`--exit-code` e `--quiet`).
pub fn exit_for(o: &DiffOpts, has_diff: bool) -> i32 {
    if o.exit_code && has_diff { 1 } else { 0 }
}

/// A tree que o HEAD guarda (`None` num repositório sem commits).
pub fn head_tree(repo: &Repo) -> R<Option<Oid>> {
    match repo.head_oid()? {
        Some(c) => Ok(Some(repo.tree_of(&c)?)),
        None => Ok(None),
    }
}

fn tree_of_spec(repo: &Repo, spec: &[u8]) -> R<Option<Oid>> {
    match repo.rev_parse(spec)? {
        Some(id) => repo.peel_to_tree(&id),
        None => Ok(None),
    }
}

/// Árvores que um argumento nomeia: uma pra `rev`, duas pra `A..B` e `A...B`. `None` se não é
/// revisão.
fn trees_of_arg(repo: &Repo, arg: &[u8]) -> R<Option<Vec<Oid>>> {
    let find = |sep: &[u8]| arg.windows(sep.len()).position(|w| w == sep);
    if let Some(i) = find(b"...") {
        let (l, r) = (&arg[..i], &arg[i + 3..]);
        let l = if l.is_empty() { b"HEAD".as_slice() } else { l };
        let r = if r.is_empty() { b"HEAD".as_slice() } else { r };
        let (Some(a), Some(b)) = (repo.rev_parse_commit(l)?, repo.rev_parse_commit(r)?) else { return Ok(None) };
        let bases = Graph::new(repo).merge_bases(&a, &[b])?;
        let Some(base) = bases.first() else {
            return Err(Fail::Fatal(format!("{}: no merge base", os::lossy(arg))));
        };
        return Ok(Some(vec![repo.tree_of(base)?, repo.tree_of(&b)?]));
    }
    if let Some(i) = find(b"..") {
        let (l, r) = (&arg[..i], &arg[i + 2..]);
        let l = if l.is_empty() { b"HEAD".as_slice() } else { l };
        let r = if r.is_empty() { b"HEAD".as_slice() } else { r };
        let (Some(a), Some(b)) = (tree_of_spec(repo, l)?, tree_of_spec(repo, r)?) else { return Ok(None) };
        return Ok(Some(vec![a, b]));
    }
    Ok(tree_of_spec(repo, arg)?.map(|t| vec![t]))
}

/// Separa revisões de caminhos como o git: as revisões vêm primeiro; o primeiro argumento que não
/// é revisão começa os caminhos (que precisam existir na árvore de trabalho, a menos que haja
/// `--`). Devolve as árvores e os caminhos.
fn split_revs_and_paths(repo: &Repo, p: &Parsed) -> R<(Vec<Oid>, Vec<Vec<u8>>)> {
    let (before, after) = p.split_dashdash();
    let dashed = p.dashdash.is_some();
    let mut trees: Vec<Oid> = Vec::new();
    let mut paths: Vec<Vec<u8>> = Vec::new();
    let mut in_paths = false;
    for a in &before {
        if !in_paths {
            match trees_of_arg(repo, a)? {
                Some(mut t) => {
                    trees.append(&mut t);
                    continue;
                }
                None => {
                    if dashed {
                        return Err(Fail::Fatal(format!("bad revision '{}'", os::lossy(a))));
                    }
                    in_paths = true;
                }
            }
        }
        let full = if a.starts_with(b"/") { a.clone() } else { os::join(&repo.prefix, a) };
        if !dashed && os::lstat(&full).is_err() {
            return Err(rev::bad_revision(a));
        }
        paths.push(a.clone());
    }
    paths.extend(after);
    Ok((trees, paths))
}

/// `git diff`.
pub fn run_diff(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let mut specs: Vec<Spec> = DIFF_SPECS.to_vec();
    specs.push(opts::flag(None, "cached", "cached"));
    specs.push(opts::flag(None, "staged", "cached"));
    specs.push(opts::flag(None, "no-index", "no-index"));
    let p = opts::parse(&specs, args, 0, usage)?;
    let o = build_opts(git, &p, false, true)?;
    if git.repo.is_none() || p.has("no-index") {
        return no_index(git, &p, o);
    }
    let repo = git.repo()?;
    let cached = p.has("cached");
    let (trees, paths) = split_revs_and_paths(repo, &p)?;
    let ps = git.pathspec(&paths)?;
    let idx = Index::load(&repo.index_path())?;
    let pairs = match (cached, trees.len()) {
        (false, 0) => diff::diff_index_worktree(repo, &idx, &ps)?,
        (false, 1) => diff::diff_tree_worktree(repo, Some(&trees[0]), &idx, &ps)?,
        (false, 2) => diff::diff_trees(repo, Some(&trees[0]), Some(&trees[1]), &ps)?,
        (true, 0) => {
            let head = head_tree(repo)?;
            diff::diff_tree_index(repo, head.as_ref(), &idx, &ps)?
        }
        (true, 1) => diff::diff_tree_index(repo, Some(&trees[0]), &idx, &ps)?,
        _ => return Err(Fail::Fatal("too many revisions given (combined diffs are not supported)".into())),
    };
    let pairs = diff::postprocess(repo, pairs, &o)?;
    let has = diff::emit(repo, &pairs, &o)?;
    Ok(exit_for(&o, has))
}

// ---- --no-index -------------------------------------------------------------------------------

/// Um repositório de mentira: o diff entre arquivos soltos só precisa da configuração e de uma
/// pasta de objetos que não existe (a abreviação cai no tamanho padrão).
fn detached_repo(cfg: &Config) -> Repo {
    Repo {
        git_dir_display: Vec::new(),
        git_dir: Vec::new(),
        common_dir: Vec::new(),
        work_tree: None,
        prefix: Vec::new(),
        bare: false,
        inside_git_dir: false,
        odb: Odb::new(b"/nonexistent/objects".to_vec()),
        config: cfg.clone(),
        packed_cache: RefCell::new(None),
    }
}

fn disk_side(path: &[u8]) -> R<Side> {
    let st = os::lstat(path).map_err(|_| Fail::Fatal(format!("Could not access '{}'", os::lossy(path))))?;
    let data = diff::worktree_blob(path, &st)?;
    Ok(Side { path: path.to_vec(), mode: os::git_mode_of(&st), oid: hash::hash_object(Kind::Blob, &data), wt: true })
}

/// Arquivos de uma pasta, recursivo, como caminho relativo.
fn list_files(dir: &[u8], rel: &[u8], out: &mut BTreeMap<Vec<u8>, ()>) {
    let Ok(entries) = os::read_dir(dir) else { return };
    for e in entries {
        let child = os::join(dir, &e.name);
        let child_rel = os::join(rel, &e.name);
        if e.kind == sysabi::FileType::Directory {
            list_files(&child, &child_rel, out);
        } else {
            out.insert(child_rel, ());
        }
    }
}

fn no_index(git: &Git, p: &Parsed, mut o: DiffOpts) -> R<i32> {
    let (before, after) = p.split_dashdash();
    let mut paths = before;
    paths.extend(after);
    if paths.len() != 2 {
        os::errs("usage: git diff --no-index [<options>] <path> <path>\n");
        return Err(Fail::Exit(129));
    }
    o.exit_code = true;
    let repo = detached_repo(git.config());
    let (a, b) = (&paths[0], &paths[1]);
    let mut pairs: Vec<Pair> = Vec::new();
    for path in [a, b] {
        if os::lstat(path).is_err() {
            return error_exit(&format!("Could not access '{}'", os::lossy(path)), 1);
        }
    }
    if os::is_dir(a) && os::is_dir(b) {
        let mut names: BTreeMap<Vec<u8>, ()> = BTreeMap::new();
        let mut fa: BTreeMap<Vec<u8>, ()> = BTreeMap::new();
        let mut fb: BTreeMap<Vec<u8>, ()> = BTreeMap::new();
        list_files(a, b"", &mut fa);
        list_files(b, b"", &mut fb);
        names.extend(fa.keys().map(|k| (k.clone(), ())));
        names.extend(fb.keys().map(|k| (k.clone(), ())));
        for name in names.keys() {
            let pa = os::join(a, name);
            let pb = os::join(b, name);
            let one = if fa.contains_key(name) { disk_side(&pa)? } else { Side::absent(&pa) };
            let two = if fb.contains_key(name) { disk_side(&pb)? } else { Side::absent(&pb) };
            pairs.push(Pair::new(one, two));
        }
    } else if os::is_dir(a) || os::is_dir(b) {
        return Err(Fail::Fatal(format!("cannot compare a directory with a file: '{}' and '{}'", os::lossy(a), os::lossy(b))));
    } else {
        pairs.push(Pair::new(disk_side(a)?, disk_side(b)?));
    }
    pairs.retain(|p| !(p.one.valid() && p.two.valid() && p.one.oid == p.two.oid && p.one.mode == p.two.mode));
    let pairs = diff::postprocess(&repo, pairs, &o)?;
    let has = diff::emit(&repo, &pairs, &o)?;
    Ok(exit_for(&o, has))
}

// ---- plumbing ---------------------------------------------------------------------------------

/// Opções do plumbing: ids completos no `--raw`, que é o formato padrão.
fn plumbing_opts(git: &Git, p: &Parsed) -> R<DiffOpts> {
    let mut o = build_opts(git, p, true, false)?;
    if !o.any_format() && !o.quiet {
        o.raw = true;
    }
    o.raw_full = !p.present("abbrev");
    Ok(o)
}

/// `git diff-index [--cached] <tree-ish> [<path>...]`.
pub fn run_diff_index(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let mut specs: Vec<Spec> = DIFF_SPECS.to_vec();
    specs.push(opts::flag(None, "cached", "cached"));
    specs.push(opts::short_flag(b'm', "m"));
    let p = opts::parse(&specs, args, 0, usage)?;
    let o = plumbing_opts(git, &p)?;
    let repo = git.repo()?;
    let Some(spec) = p.args.first() else {
        return Err(opts::usage_error(usage, "<tree-ish> required"));
    };
    let tree = tree_of_spec(repo, spec)?.ok_or_else(|| Fail::Fatal(format!("bad object {}", os::lossy(spec))))?;
    let ps = git.pathspec(&p.args[1..])?;
    let idx = Index::load(&repo.index_path())?;
    let pairs = if p.has("cached") {
        diff::diff_tree_index(repo, Some(&tree), &idx, &ps)?
    } else {
        diff::diff_tree_worktree(repo, Some(&tree), &idx, &ps)?
    };
    let pairs = diff::postprocess(repo, pairs, &o)?;
    let has = diff::emit(repo, &pairs, &o)?;
    Ok(exit_for(&o, has))
}

/// `git diff-files [<path>...]`.
pub fn run_diff_files(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let p = opts::parse(DIFF_SPECS, args, 0, usage)?;
    let o = plumbing_opts(git, &p)?;
    let repo = git.repo()?;
    let ps = git.pathspec(&p.args)?;
    let idx = Index::load(&repo.index_path())?;
    let pairs = diff::diff_index_worktree(repo, &idx, &ps)?;
    let pairs = diff::postprocess(repo, pairs, &o)?;
    let has = diff::emit(repo, &pairs, &o)?;
    Ok(exit_for(&o, has))
}

/// Diferença só do primeiro nível de duas trees (o `diff-tree` sem `-r`): as pastas aparecem como
/// uma entrada de modo 040000.
fn shallow_pairs(repo: &Repo, a: Option<&Oid>, b: Option<&Oid>) -> R<Vec<Pair>> {
    let load = |t: Option<&Oid>| -> R<BTreeMap<Vec<u8>, (u32, Oid)>> {
        let mut m = BTreeMap::new();
        if let Some(t) = t {
            for e in repo.read_tree(t)? {
                m.insert(e.name.clone(), (e.mode, e.oid));
            }
        }
        Ok(m)
    };
    let ma = load(a)?;
    let mb = load(b)?;
    let mut names: Vec<(Vec<u8>, bool)> = Vec::new();
    for (n, (m, _)) in &mb {
        names.push((n.clone(), object::is_tree_mode(*m)));
    }
    for (n, (m, _)) in &ma {
        if !mb.contains_key(n) {
            names.push((n.clone(), object::is_tree_mode(*m)));
        }
    }
    names.sort_by(|x, y| object::tree_entry_cmp(&x.0, x.1, &y.0, y.1));
    let mut out = Vec::new();
    for (name, _) in names {
        let one = match ma.get(&name) {
            Some((m, id)) => Side { path: name.clone(), mode: *m, oid: *id, wt: false },
            None => Side::absent(&name),
        };
        let two = match mb.get(&name) {
            Some((m, id)) => Side { path: name.clone(), mode: *m, oid: *id, wt: false },
            None => Side::absent(&name),
        };
        if one.valid() && two.valid() && one.oid == two.oid && one.mode == two.mode {
            continue;
        }
        out.push(Pair::new(one, two));
    }
    Ok(out)
}

/// `git diff-tree [-r] [--root] [--no-commit-id] <tree-ish> [<tree-ish>] [<path>...]`.
pub fn run_diff_tree(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let mut specs: Vec<Spec> = DIFF_SPECS.to_vec();
    specs.push(opts::short_flag(b'r', "recursive"));
    specs.push(opts::flag(None, "root", "root"));
    specs.push(opts::flag(None, "no-commit-id", "no-commit-id"));
    specs.push(opts::short_flag(b'm', "m"));
    let p = opts::parse(&specs, args, 0, usage)?;
    let o = plumbing_opts(git, &p)?;
    let repo = git.repo()?;
    let Some(first) = p.args.first() else {
        return Err(opts::usage_fatal(usage, "<tree-ish> required"));
    };
    let id = repo.rev_parse(first)?.ok_or_else(|| rev::bad_revision(first))?;
    let second = match p.args.get(1) {
        Some(s) => repo.rev_parse(s)?,
        None => None,
    };
    let recursive = p.has("recursive") || o.patch || o.stat || o.numstat || o.shortstat || o.name_only || o.name_status || o.summary;
    let mut path_args: Vec<Vec<u8>> = p.args[1..].to_vec();
    let walk = |a: Option<&Oid>, b: Option<&Oid>, ps: &Pathspec| -> R<Vec<Pair>> {
        if recursive || !ps.items.is_empty() { diff::diff_trees(repo, a, b, ps) } else { shallow_pairs(repo, a, b) }
    };
    if let Some(other) = second {
        path_args.remove(0);
        let ps = git.pathspec(&path_args)?;
        let a = repo.tree_of(&id)?;
        let b = repo.tree_of(&other)?;
        let pairs = diff::postprocess(repo, walk(Some(&a), Some(&b), &ps)?, &o)?;
        let has = diff::emit(repo, &pairs, &o)?;
        return Ok(exit_for(&o, has));
    }
    // Um só argumento: o commit contra o pai.
    let ps = git.pathspec(&path_args)?;
    let Some(commit_id) = repo.peel_to_commit(&id)? else {
        return Err(Fail::Fatal(format!("{}: not a commit", os::lossy(first))));
    };
    let commit = repo.read_commit(&commit_id)?;
    let tree = commit.tree;
    let parents: Vec<Option<Oid>> = match commit.parents.len() {
        0 if p.has("root") => vec![None],
        0 => Vec::new(),
        1 => vec![Some(repo.tree_of(&commit.parents[0])?)],
        _ if p.has("m") => {
            let mut v = Vec::new();
            for par in &commit.parents {
                v.push(Some(repo.tree_of(par)?));
            }
            v
        }
        _ => Vec::new(),
    };
    let mut any = false;
    for parent in &parents {
        let pairs = diff::postprocess(repo, walk(parent.as_ref(), Some(&tree), &ps)?, &o)?;
        if pairs.is_empty() {
            continue;
        }
        if !p.has("no-commit-id") {
            os::outs(&format!("{commit_id}\n"));
        }
        diff::emit(repo, &pairs, &o)?;
        any = true;
    }
    Ok(exit_for(&o, any))
}
