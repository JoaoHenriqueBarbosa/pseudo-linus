//! Diff entre trees, índice e árvore de trabalho, e as saídas do `git diff` (patch, `--stat`,
//! `--numstat`, `--shortstat`, `--name-only`, `--name-status`, `--raw`, `--summary`).

pub mod rename;
pub mod text;

use std::collections::BTreeMap;
use std::rc::Rc;

use crate::error::{Fail, R};
use crate::hash::{self, Kind, Oid};
use crate::index::{self, Index};
use crate::object::{self, MODE_GITLINK};
use crate::os;
use crate::pathspec::Pathspec;
use crate::quote;
use crate::repo::Repo;
use text::{HunkOpts, Ws};

/// Um lado de um par.
#[derive(Clone, Debug, Default)]
pub struct Side {
    pub path: Vec<u8>,
    /// 0 = não existe deste lado.
    pub mode: u32,
    pub oid: Oid,
    /// O conteúdo está no arquivo da árvore de trabalho (e `oid` foi calculado dele).
    pub wt: bool,
}

impl Side {
    pub fn valid(&self) -> bool {
        self.mode != 0
    }

    pub fn absent(path: &[u8]) -> Side {
        Side { path: path.to_vec(), mode: 0, oid: Oid::ZERO, wt: false }
    }
}

#[derive(Clone, Debug)]
pub struct Pair {
    pub one: Side,
    pub two: Side,
    /// `A`, `D`, `M`, `T`, `R`, `C`, `U`.
    pub status: u8,
    /// Semelhança (0..60000) de R/C.
    pub score: u32,
}

impl Pair {
    pub fn new(one: Side, two: Side) -> Pair {
        let status = if !one.valid() {
            b'A'
        } else if !two.valid() {
            b'D'
        } else if (one.mode & 0o170000) != (two.mode & 0o170000) {
            b'T'
        } else {
            b'M'
        };
        Pair { one, two, status, score: 0 }
    }

    pub fn unmerged(path: &[u8]) -> Pair {
        Pair { one: Side::absent(path), two: Side::absent(path), status: b'U', score: 0 }
    }

    /// Caminho que identifica o par (o novo, ou o antigo se foi removido).
    pub fn path(&self) -> &[u8] {
        if self.two.valid() || self.status == b'U' { &self.two.path } else { &self.one.path }
    }

    pub fn similarity_pct(&self) -> u32 {
        self.score * 100 / rename::MAX_SCORE
    }
}

/// Conteúdo de um lado (blob, arquivo, link simbólico ou gitlink).
pub fn content_of(repo: &Repo, side: &Side) -> R<Rc<Vec<u8>>> {
    if !side.valid() {
        return Ok(Rc::new(Vec::new()));
    }
    if object::is_gitlink(side.mode) {
        return Ok(Rc::new(format!("Subproject commit {}\n", side.oid).into_bytes()));
    }
    if side.wt {
        if object::is_link(side.mode) {
            return os::readlink(&side.path).map(Rc::new).map_err(|e| Fail::Fatal(format!("readlink {}: {}", os::lossy(&side.path), e.message())));
        }
        return os::read(&side.path).map(Rc::new).map_err(|e| Fail::Fatal(format!("could not read '{}': {}", os::lossy(&side.path), e.message())));
    }
    if side.oid.is_zero() {
        return Ok(Rc::new(Vec::new()));
    }
    let (_, d) = repo.read_object(&side.oid)?;
    Ok(d)
}

/// Lê um arquivo da árvore de trabalho (caminho relativo ao topo) como o git o guardaria.
pub fn worktree_blob(path: &[u8], st: &sysabi::Stat) -> R<Vec<u8>> {
    if st.file_type() == sysabi::FileType::Symlink {
        return os::readlink(path).map_err(|e| Fail::Fatal(format!("readlink({}): {}", os::lossy(path), e.message())));
    }
    os::read(path).map_err(|e| Fail::Fatal(format!("open(\"{}\"): {}", os::lossy(path), e.message())))
}

// ---- produtores de pares ----------------------------------------------------------------------

/// Arquivos de uma tree (opcional), filtrados pela pathspec.
fn tree_files(repo: &Repo, tree: Option<&Oid>, ps: &Pathspec) -> R<BTreeMap<Vec<u8>, (u32, Oid)>> {
    let mut out = BTreeMap::new();
    if let Some(t) = tree {
        walk_tree(repo, t, b"", ps, &mut out)?;
    }
    Ok(out)
}

fn walk_tree(repo: &Repo, tree: &Oid, prefix: &[u8], ps: &Pathspec, out: &mut BTreeMap<Vec<u8>, (u32, Oid)>) -> R<()> {
    for e in repo.read_tree(tree)? {
        let mut path = prefix.to_vec();
        path.extend_from_slice(&e.name);
        if e.is_tree() {
            if ps.may_match_under(&path) {
                path.push(b'/');
                walk_tree(repo, &e.oid, &path, ps, out)?;
            }
        } else if ps.matches_simple(&path) {
            out.insert(path, (e.mode, e.oid));
        }
    }
    Ok(())
}

/// Pares entre duas listas de (caminho -> modo, id), em ordem de caminho.
fn pairs_between(a: &BTreeMap<Vec<u8>, (u32, Oid)>, b: &BTreeMap<Vec<u8>, (u32, Oid)>) -> Vec<Pair> {
    let mut out = Vec::new();
    let mut ia = a.iter().peekable();
    let mut ib = b.iter().peekable();
    loop {
        match (ia.peek(), ib.peek()) {
            (None, None) => break,
            (Some((pa, va)), Some((pb, vb))) if pa == pb => {
                if va != vb {
                    let one = Side { path: pa.to_vec(), mode: va.0, oid: va.1, wt: false };
                    let two = Side { path: pb.to_vec(), mode: vb.0, oid: vb.1, wt: false };
                    // Troca de tipo arquivo <-> link vira T; com gitlink, remoção + criação.
                    if object::is_gitlink(va.0) != object::is_gitlink(vb.0) {
                        out.push(Pair::new(one.clone(), Side::absent(pa)));
                        out.push(Pair::new(Side::absent(pb), two));
                    } else {
                        out.push(Pair::new(one, two));
                    }
                }
                ia.next();
                ib.next();
            }
            (Some((pa, va)), Some((pb, _))) if pa < pb => {
                out.push(Pair::new(Side { path: pa.to_vec(), mode: va.0, oid: va.1, wt: false }, Side::absent(pa)));
                ia.next();
            }
            (Some((pa, va)), None) => {
                out.push(Pair::new(Side { path: pa.to_vec(), mode: va.0, oid: va.1, wt: false }, Side::absent(pa)));
                ia.next();
            }
            (_, Some((pb, vb))) => {
                out.push(Pair::new(Side::absent(pb), Side { path: pb.to_vec(), mode: vb.0, oid: vb.1, wt: false }));
                ib.next();
            }
        }
    }
    out
}

/// tree x tree.
pub fn diff_trees(repo: &Repo, a: Option<&Oid>, b: Option<&Oid>, ps: &Pathspec) -> R<Vec<Pair>> {
    let fa = tree_files(repo, a, ps)?;
    let fb = tree_files(repo, b, ps)?;
    Ok(pairs_between(&fa, &fb))
}

/// Entradas de estágio 0 do índice (e caminhos em conflito à parte).
fn index_files(idx: &Index, ps: &Pathspec) -> (BTreeMap<Vec<u8>, (u32, Oid)>, Vec<Vec<u8>>) {
    let mut files = BTreeMap::new();
    let mut unmerged = Vec::new();
    for e in &idx.entries {
        if !ps.matches_simple(&e.path) {
            continue;
        }
        if e.stage != 0 {
            if unmerged.last() != Some(&e.path) {
                unmerged.push(e.path.clone());
            }
            continue;
        }
        if e.intent_to_add() {
            continue;
        }
        files.insert(e.path.clone(), (e.mode, e.oid));
    }
    (files, unmerged)
}

/// tree x índice (`diff --cached`). Caminhos em conflito viram pares `U`.
pub fn diff_tree_index(repo: &Repo, tree: Option<&Oid>, idx: &Index, ps: &Pathspec) -> R<Vec<Pair>> {
    let ft = tree_files(repo, tree, ps)?;
    let (fi, unmerged) = index_files(idx, ps);
    let mut pairs = pairs_between(&ft, &fi);
    if !unmerged.is_empty() {
        pairs.retain(|p| !unmerged.iter().any(|u| u == p.path()));
        for u in unmerged {
            pairs.push(Pair::unmerged(&u));
        }
        pairs.sort_by(|a, b| a.path().cmp(b.path()));
    }
    Ok(pairs)
}

/// Estado de um arquivo da árvore de trabalho frente a uma entrada do índice.
pub enum WtState {
    Same,
    Deleted,
    /// Mudou: modo e id do conteúdo atual.
    Changed(u32, Oid),
}

/// Compara a entrada com o arquivo (stat primeiro, conteúdo se precisar).
pub fn check_entry(_repo: &Repo, idx: &Index, e: &index::IEntry, trust_exec: bool) -> R<WtState> {
    let st = match os::lstat(&e.path) {
        Ok(s) => s,
        Err(sysabi::Errno::ENOENT | sysabi::Errno::ENOTDIR) => return Ok(WtState::Deleted),
        Err(err) => return Err(Fail::Fatal(format!("lstat({}): {}", os::lossy(&e.path), err.message()))),
    };
    if object::is_gitlink(e.mode) {
        if st.file_type() != sysabi::FileType::Directory {
            return Ok(if st.file_type() == sysabi::FileType::Regular || st.file_type() == sysabi::FileType::Symlink {
                let data = worktree_blob(&e.path, &st)?;
                WtState::Changed(os::git_mode_of(&st), hash::hash_object(Kind::Blob, &data))
            } else {
                WtState::Deleted
            });
        }
        let head = gitlink_head(&e.path);
        return Ok(match head {
            Some(h) if h != e.oid => WtState::Changed(MODE_GITLINK, h),
            _ => WtState::Same,
        });
    }
    // Diretório no lugar do arquivo conta como removido.
    if st.file_type() == sysabi::FileType::Directory {
        return Ok(WtState::Deleted);
    }
    let new_mode = index::mode_for(&st, Some(e.mode), trust_exec);
    if index::stat_matches(e, &st, trust_exec) && !idx.is_racy(e) && !e.intent_to_add() {
        return Ok(WtState::Same);
    }
    let data = worktree_blob(&e.path, &st)?;
    let id = hash::hash_object(Kind::Blob, &data);
    if id == e.oid && object::canon_mode(new_mode) == object::canon_mode(e.mode) && !e.intent_to_add() {
        return Ok(WtState::Same);
    }
    Ok(WtState::Changed(new_mode, id))
}

/// HEAD de um repositório aninhado (submódulo).
fn gitlink_head(path: &[u8]) -> Option<Oid> {
    let gd = os::join(path, b".git");
    let dir = if os::is_dir(&gd) {
        gd
    } else {
        let d = os::read_opt(&gd).ok()??;
        let rest = d.strip_prefix(b"gitdir: ")?;
        let rest = object::trim_ascii(rest);
        if rest.starts_with(b"/") { rest.to_vec() } else { os::join(path, rest) }
    };
    let head = os::read_opt(&os::join(&dir, b"HEAD")).ok()??;
    let head = object::trim_ascii(&head);
    if let Some(r) = head.strip_prefix(b"ref: ") {
        let v = os::read_opt(&os::join(&dir, r)).ok()??;
        return Oid::from_hex(object::trim_ascii(&v));
    }
    Oid::from_hex(head)
}

/// índice x árvore de trabalho (`git diff`).
pub fn diff_index_worktree(repo: &Repo, idx: &Index, ps: &Pathspec) -> R<Vec<Pair>> {
    let trust = repo.config.get_bool("core.filemode")?.unwrap_or(true);
    let mut out = Vec::new();
    let mut last_unmerged: Option<Vec<u8>> = None;
    for e in &idx.entries {
        if !ps.matches_simple(&e.path) {
            continue;
        }
        if e.stage != 0 {
            if last_unmerged.as_deref() != Some(e.path.as_slice()) {
                out.push(Pair::unmerged(&e.path));
                last_unmerged = Some(e.path.clone());
            }
            continue;
        }
        if e.skip_worktree() {
            continue;
        }
        let one = Side { path: e.path.clone(), mode: e.mode, oid: e.oid, wt: false };
        if e.intent_to_add() {
            match os::lstat(&e.path) {
                Ok(st) => {
                    let data = worktree_blob(&e.path, &st)?;
                    let two = Side { path: e.path.clone(), mode: index::mode_for(&st, None, trust), oid: hash::hash_object(Kind::Blob, &data), wt: true };
                    out.push(Pair::new(Side::absent(&e.path), two));
                }
                Err(_) => out.push(Pair::new(Side::absent(&e.path), Side::absent(&e.path))),
            }
            continue;
        }
        match check_entry(repo, idx, e, trust)? {
            WtState::Same => {}
            WtState::Deleted => out.push(Pair::new(one, Side::absent(&e.path))),
            WtState::Changed(mode, id) => {
                let two = Side { path: e.path.clone(), mode, oid: id, wt: !object::is_gitlink(mode) };
                out.push(Pair::new(one, two));
            }
        }
    }
    Ok(out)
}

/// tree x árvore de trabalho (`git diff HEAD`), usando o índice pra saber o que é rastreado.
pub fn diff_tree_worktree(repo: &Repo, tree: Option<&Oid>, idx: &Index, ps: &Pathspec) -> R<Vec<Pair>> {
    let trust = repo.config.get_bool("core.filemode")?.unwrap_or(true);
    let ft = tree_files(repo, tree, ps)?;
    let mut fw: BTreeMap<Vec<u8>, (u32, Oid)> = BTreeMap::new();
    let mut unmerged = Vec::new();
    for e in &idx.entries {
        if !ps.matches_simple(&e.path) {
            continue;
        }
        if e.stage != 0 {
            if unmerged.last() != Some(&e.path) {
                unmerged.push(e.path.clone());
            }
            continue;
        }
        match check_entry(repo, idx, e, trust)? {
            WtState::Same => {
                if !e.intent_to_add() {
                    fw.insert(e.path.clone(), (e.mode, e.oid));
                }
            }
            WtState::Deleted => {}
            WtState::Changed(m, id) => {
                fw.insert(e.path.clone(), (m, id));
            }
        }
    }
    let mut pairs = pairs_between(&ft, &fw);
    // O lado novo vem da árvore de trabalho quando difere do índice.
    for p in &mut pairs {
        if p.two.valid()
            && let Some(e) = idx.get(&p.two.path)
            && (e.oid != p.two.oid || e.mode != p.two.mode)
        {
            p.two.wt = !object::is_gitlink(p.two.mode);
        }
    }
    if !unmerged.is_empty() {
        pairs.retain(|p| !unmerged.iter().any(|u| u == p.path()));
        for u in unmerged {
            pairs.push(Pair::unmerged(&u));
        }
        pairs.sort_by(|a, b| a.path().cmp(b.path()));
    }
    Ok(pairs)
}

// ---- opções e saída ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct DiffOpts {
    pub patch: bool,
    pub stat: bool,
    pub numstat: bool,
    pub shortstat: bool,
    pub name_only: bool,
    pub name_status: bool,
    pub raw: bool,
    pub summary: bool,
    pub no_patch: bool,
    pub hunk: HunkOpts,
    pub ws: Ws,
    pub renames: Option<rename::RenameOpts>,
    /// Comprimento de abreviação (`None` = o padrão do repositório).
    pub abbrev: Option<usize>,
    pub full_index: bool,
    pub binary: bool,
    pub text: bool,
    pub src_prefix: Vec<u8>,
    pub dst_prefix: Vec<u8>,
    pub stat_width: Option<usize>,
    pub stat_name_width: Option<usize>,
    pub stat_graph_width: Option<usize>,
    pub stat_count: Option<usize>,
    pub reverse: bool,
    pub filter: Option<Vec<u8>>,
    pub relative: Option<Vec<u8>>,
    pub null_terminated: bool,
    pub quote_fully: bool,
    pub line_prefix: Vec<u8>,
    pub exit_code: bool,
    pub quiet: bool,
    /// Ids completos no `--raw` (plumbing).
    pub raw_full: bool,
}

impl Default for DiffOpts {
    fn default() -> Self {
        DiffOpts {
            patch: false,
            stat: false,
            numstat: false,
            shortstat: false,
            name_only: false,
            name_status: false,
            raw: false,
            summary: false,
            no_patch: false,
            hunk: HunkOpts::default(),
            ws: Ws::default(),
            renames: None,
            abbrev: None,
            full_index: false,
            binary: false,
            text: false,
            src_prefix: b"a/".to_vec(),
            dst_prefix: b"b/".to_vec(),
            stat_width: None,
            stat_name_width: None,
            stat_graph_width: None,
            stat_count: None,
            reverse: false,
            filter: None,
            relative: None,
            null_terminated: false,
            quote_fully: true,
            line_prefix: Vec::new(),
            exit_code: false,
            quiet: false,
            raw_full: false,
        }
    }
}

impl DiffOpts {
    /// Alguma saída além do patch pedida?
    pub fn any_format(&self) -> bool {
        self.patch || self.stat || self.numstat || self.shortstat || self.name_only || self.name_status || self.raw || self.summary
    }
}

/// Aplica renomeação, `-R`, `--diff-filter` e `--relative` na fila.
pub fn postprocess(repo: &Repo, mut pairs: Vec<Pair>, o: &DiffOpts) -> R<Vec<Pair>> {
    if o.reverse {
        for p in &mut pairs {
            std::mem::swap(&mut p.one, &mut p.two);
            p.status = match p.status {
                b'A' => b'D',
                b'D' => b'A',
                s => s,
            };
        }
    }
    if let Some(r) = &o.renames {
        pairs = rename::detect(repo, pairs, r)?;
    }
    if let Some(rel) = &o.relative {
        let mut pre = rel.clone();
        if !pre.is_empty() && !pre.ends_with(b"/") {
            pre.push(b'/');
        }
        pairs.retain(|p| p.path().starts_with(&pre) || (p.status == b'R' && p.one.path.starts_with(&pre)));
        for p in &mut pairs {
            for s in [&mut p.one, &mut p.two] {
                if let Some(r) = s.path.strip_prefix(pre.as_slice()) {
                    s.path = r.to_vec();
                }
            }
        }
    }
    if let Some(f) = &o.filter {
        pairs = apply_filter(pairs, f);
    }
    Ok(pairs)
}

/// `--diff-filter=ACDMRTUXB` (maiúsculas incluem, minúsculas excluem).
pub fn apply_filter(pairs: Vec<Pair>, f: &[u8]) -> Vec<Pair> {
    let include: Vec<u8> = f.iter().copied().filter(|c| c.is_ascii_uppercase()).collect();
    let exclude: Vec<u8> = f.iter().copied().filter(|c| c.is_ascii_lowercase()).map(|c| c.to_ascii_uppercase()).collect();
    pairs
        .into_iter()
        .filter(|p| {
            let s = p.status;
            if exclude.contains(&s) {
                return false;
            }
            include.is_empty() || include.contains(&s)
        })
        .collect()
}

fn abbrev_of(repo: &Repo, id: &Oid, o: &DiffOpts) -> String {
    if o.full_index {
        return id.hex();
    }
    let len = o.abbrev.unwrap_or_else(|| repo.abbrev_len());
    if id.is_zero() {
        return "0".repeat(len.min(40));
    }
    repo.abbrev(id, len)
}

fn q(path: &[u8], o: &DiffOpts) -> Vec<u8> {
    if o.null_terminated { path.to_vec() } else { quote::quote_c(path, o.quote_fully) }
}

/// Nome de renomeação no stat e no `--summary` (formato observado no oráculo): os diretórios em
/// comum no começo e no fim saem fora das chaves (`src/{a.rs => b.rs}`, `{lib => x/y}/x.txt`);
/// sem nada em comum fica `a => b`; caminho que precisa de aspas sai inteiro entre aspas.
pub fn pprint_rename(a: &[u8], b: &[u8], fully: bool) -> Vec<u8> {
    if quote::needs_quote(a, fully) || quote::needs_quote(b, fully) {
        let mut out = quote::quote_c(a, fully);
        out.extend_from_slice(b" => ");
        out.extend_from_slice(&quote::quote_c(b, fully));
        return out;
    }
    // Prefixo comum até (e incluindo) a última barra dele.
    let same = a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count();
    let head = a[..same].iter().rposition(|c| *c == b'/').map_or(0, |i| i + 1);
    // Sufixo comum que começa numa barra; pode reaproveitar a barra do fim do prefixo.
    let floor = head.saturating_sub(1);
    let mut tail = 0;
    let mut k = 1;
    while k <= a.len() && k <= b.len() && a.len() - k >= floor && b.len() - k >= floor && a[a.len() - k] == b[b.len() - k] {
        if a[a.len() - k] == b'/' {
            tail = k;
        }
        k += 1;
    }
    let mid = |s: &[u8]| -> Vec<u8> {
        let end = s.len() - tail;
        if end > head { s[head..end].to_vec() } else { Vec::new() }
    };
    let mut out = Vec::new();
    let braces = head + tail > 0;
    if braces {
        out.extend_from_slice(&a[..head]);
        out.push(b'{');
    }
    out.extend_from_slice(&mid(a));
    out.extend_from_slice(b" => ");
    out.extend_from_slice(&mid(b));
    if braces {
        out.push(b'}');
        out.extend_from_slice(&a[a.len() - tail..]);
    }
    out
}

/// Estatística de um par.
#[derive(Clone, Debug)]
pub struct FileStat {
    pub name: Vec<u8>,
    pub added: usize,
    pub deleted: usize,
    pub binary: bool,
    pub unmerged: bool,
    pub renamed: bool,
}

pub fn file_stat(repo: &Repo, p: &Pair, o: &DiffOpts) -> R<FileStat> {
    let renamed = matches!(p.status, b'R' | b'C');
    let name = if renamed { pprint_rename(&p.one.path, &p.two.path, o.quote_fully) } else { quote::quote_c(p.path(), o.quote_fully) };
    if p.status == b'U' {
        return Ok(FileStat { name, added: 0, deleted: 0, binary: false, unmerged: true, renamed });
    }
    if object::is_gitlink(p.one.mode) || object::is_gitlink(p.two.mode) {
        let (a, d) = (usize::from(p.two.valid()), usize::from(p.one.valid()));
        return Ok(FileStat { name, added: a, deleted: d, binary: false, unmerged: false, renamed });
    }
    let same = p.one.valid() && p.two.valid() && p.one.oid == p.two.oid && !p.one.oid.is_zero();
    if same {
        return Ok(FileStat { name, added: 0, deleted: 0, binary: false, unmerged: false, renamed });
    }
    let a = content_of(repo, &p.one)?;
    let b = content_of(repo, &p.two)?;
    if !o.text && (text::is_binary(&a) || text::is_binary(&b)) {
        return Ok(FileStat { name, added: b.len(), deleted: a.len(), binary: true, unmerged: false, renamed });
    }
    let (added, deleted) = text::count(&a, &b, o.ws);
    Ok(FileStat { name, added, deleted, binary: false, unmerged: false, renamed })
}

fn digits(n: usize) -> usize {
    n.to_string().len()
}

fn display_width(s: &[u8]) -> usize {
    String::from_utf8_lossy(s).chars().count()
}

/// Larguras das colunas do stat. Regra medida no oráculo (scratch `git-stat-probe*.sh`): a linha é
/// ` nome | N grafo` e usa no máximo `W - 1` colunas. O nome quer o comprimento do maior nome; o
/// grafo quer a maior mudança (ou o texto de binário). Se não cabe, o grafo cai pra
/// `3W/8 - nw - 6` (nunca abaixo de 6) e o nome fica com o resto; se o nome inteiro couber nesse
/// resto, o grafo volta a ocupar tudo o que o nome não usa.
struct StatLayout {
    name: usize,
    number: usize,
    graph: usize,
    max_change: usize,
}

fn stat_layout(stats: &[FileStat], o: &DiffOpts) -> StatLayout {
    let longest = stats.iter().map(|f| display_width(&f.name)).max().unwrap_or(0);
    let max_change = stats.iter().filter(|f| !f.binary && !f.unmerged).map(|f| f.added + f.deleted).max().unwrap_or(0);
    let has_binary = stats.iter().any(|f| f.binary);
    // O que vem depois de "Bin" (` X -> Y bytes`) ocupa a área do grafo.
    let binary_text = stats.iter().filter(|f| f.binary).map(|f| 10 + digits(f.deleted) + digits(f.added)).max().unwrap_or(0);
    let unmerged = stats.iter().any(|f| f.unmerged);
    let mut number = digits(max_change);
    if has_binary {
        number = number.max(3);
    }
    let _ = unmerged;
    let mut width = match o.stat_width {
        Some(w) => w,
        None => term_columns().saturating_sub(o.line_prefix.len()),
    };
    width = width.max(22 + number);
    let mut graph = max_change.max(binary_text);
    if let Some(g) = o.stat_graph_width {
        graph = graph.min(g);
    }
    let mut name = match o.stat_name_width {
        Some(n) if n < longest => n,
        _ => longest,
    };
    let fixed = number + 6;
    if name + fixed + graph > width {
        let share = (width * 3 / 8).saturating_sub(fixed).max(6);
        graph = graph.min(share);
        if let Some(g) = o.stat_graph_width {
            graph = graph.min(g);
        }
        let room = width.saturating_sub(fixed + graph);
        if name > room {
            name = room;
        } else {
            graph = width.saturating_sub(fixed + name);
        }
    }
    StatLayout { name, number, graph, max_change }
}

/// Quantas colunas de grafo cabem pra `n` mudanças: qualquer mudança ganha ao menos uma, e a maior
/// ocupa o grafo inteiro.
fn bar(n: usize, l: &StatLayout) -> usize {
    if n == 0 || l.max_change == 0 {
        0
    } else {
        1 + n * (l.graph - 1) / l.max_change
    }
}

/// Nome cortado pra caber: `...` e o fim do caminho, a partir da primeira barra que sobrar.
fn fit_name(name: &[u8], width: usize) -> (Vec<u8>, usize) {
    if display_width(name) <= width {
        return (name.to_vec(), width - display_width(name));
    }
    let keep = width.saturating_sub(3);
    let s = String::from_utf8_lossy(name);
    let chars: Vec<char> = s.chars().collect();
    let tail: String = chars[chars.len() - keep.min(chars.len())..].iter().collect();
    let tail = match tail.find('/') {
        Some(p) => tail[p..].to_string(),
        None => tail,
    };
    let mut out = b"...".to_vec();
    out.extend_from_slice(tail.as_bytes());
    let used = 3 + tail.chars().count();
    (out, width.saturating_sub(used))
}

/// O bloco `--stat`.
pub fn format_stat(stats: &[FileStat], o: &DiffOpts) -> Vec<u8> {
    let mut out = Vec::new();
    if stats.is_empty() {
        return out;
    }
    let shown = o.stat_count.unwrap_or(stats.len()).min(stats.len());
    let l = stat_layout(&stats[..shown], o);
    for f in &stats[..shown] {
        let (name, pad) = fit_name(&f.name, l.name);
        out.extend_from_slice(&o.line_prefix);
        out.push(b' ');
        out.extend_from_slice(&name);
        out.extend(std::iter::repeat_n(b' ', pad));
        out.extend_from_slice(b" | ");
        if f.unmerged {
            out.extend_from_slice(format!("{:>w$}\n", "Unmerged", w = l.number).as_bytes());
            continue;
        }
        if f.binary {
            out.extend_from_slice(format!("{:>w$}", "Bin", w = l.number).as_bytes());
            if f.added == 0 && f.deleted == 0 {
                out.push(b'\n');
            } else {
                out.extend_from_slice(format!(" {} -> {} bytes\n", f.deleted, f.added).as_bytes());
            }
            continue;
        }
        let total = f.added + f.deleted;
        let (plus, minus) = if l.graph >= l.max_change {
            (f.added, f.deleted)
        } else {
            // A parte menor é escalada; a maior fica com o que sobra do total escalado (no mínimo
            // uma coluna pra cada lado que mudou).
            let mut cols = bar(total, &l);
            if f.added > 0 && f.deleted > 0 {
                cols = cols.max(2);
            }
            if f.added < f.deleted {
                let p = bar(f.added, &l);
                (p, cols - p)
            } else {
                let m = bar(f.deleted, &l);
                (cols - m, m)
            }
        };
        out.extend_from_slice(format!("{total:>w$}", w = l.number).as_bytes());
        if total > 0 {
            out.push(b' ');
        }
        out.extend(std::iter::repeat_n(b'+', plus));
        out.extend(std::iter::repeat_n(b'-', minus));
        out.push(b'\n');
    }
    if shown < stats.len() {
        out.extend_from_slice(&o.line_prefix);
        out.extend_from_slice(b" ...\n");
    }
    let counted: Vec<&FileStat> = stats.iter().filter(|f| !f.unmerged).collect();
    let adds = counted.iter().filter(|f| !f.binary).map(|f| f.added).sum();
    let dels = counted.iter().filter(|f| !f.binary).map(|f| f.deleted).sum();
    out.extend_from_slice(&o.line_prefix);
    out.extend_from_slice(&summary_line(counted.len(), adds, dels));
    out
}

/// ` N files changed, X insertions(+), Y deletions(-)`.
pub fn summary_line(files: usize, adds: usize, dels: usize) -> Vec<u8> {
    if files == 0 {
        return b" 0 files changed\n".to_vec();
    }
    let mut s = if files == 1 { format!(" {files} file changed") } else { format!(" {files} files changed") };
    if adds > 0 || dels == 0 {
        s.push_str(&if adds == 1 { format!(", {adds} insertion(+)") } else { format!(", {adds} insertions(+)") });
    }
    if dels > 0 || adds == 0 {
        s.push_str(&if dels == 1 { format!(", {dels} deletion(-)") } else { format!(", {dels} deletions(-)") });
    }
    s.push('\n');
    s.into_bytes()
}

pub fn term_columns() -> usize {
    if let Some(c) = os::getenv_str("COLUMNS")
        && let Ok(n) = c.trim().parse::<usize>()
        && n > 0
    {
        return n;
    }
    if let Ok(ws) = os::sysc().tcgetwinsize(sysabi::Fd::STDOUT)
        && ws.cols > 0
    {
        return ws.cols as usize;
    }
    80
}

/// Linhas do `--summary` de um par.
pub fn summary_lines(p: &Pair, o: &DiffOpts) -> Vec<u8> {
    let mut out = Vec::new();
    let fully = o.quote_fully;
    let mode_change = |out: &mut Vec<u8>, show_name: bool| {
        if p.one.mode != 0 && p.two.mode != 0 && p.one.mode != p.two.mode {
            out.extend_from_slice(&o.line_prefix);
            out.extend_from_slice(format!(" mode change {:06o} => {:06o}", p.one.mode, p.two.mode).as_bytes());
            if show_name {
                out.push(b' ');
                out.extend_from_slice(&quote::quote_c(&p.two.path, fully));
            }
            out.push(b'\n');
        }
    };
    match p.status {
        b'D' => {
            out.extend_from_slice(&o.line_prefix);
            out.extend_from_slice(format!(" delete mode {:06o} ", p.one.mode).as_bytes());
            out.extend_from_slice(&quote::quote_c(&p.one.path, fully));
            out.push(b'\n');
        }
        b'A' => {
            out.extend_from_slice(&o.line_prefix);
            out.extend_from_slice(format!(" create mode {:06o} ", p.two.mode).as_bytes());
            out.extend_from_slice(&quote::quote_c(&p.two.path, fully));
            out.push(b'\n');
        }
        b'R' | b'C' => {
            out.extend_from_slice(&o.line_prefix);
            out.extend_from_slice(if p.status == b'R' { b" rename " } else { b" copy " });
            out.extend_from_slice(&pprint_rename(&p.one.path, &p.two.path, fully));
            out.extend_from_slice(format!(" ({}%)\n", p.similarity_pct()).as_bytes());
            mode_change(&mut out, false);
        }
        _ => mode_change(&mut out, true),
    }
    out
}

/// O patch de um par.
pub fn patch_of(repo: &Repo, p: &Pair, o: &DiffOpts) -> R<Vec<u8>> {
    let mut out = Vec::new();
    let lp = &o.line_prefix;
    let fully = o.quote_fully;
    if p.status == b'U' {
        out.extend_from_slice(lp);
        out.extend_from_slice(b"* Unmerged path ");
        out.extend_from_slice(&quote::quote_c(&p.two.path, fully));
        out.push(b'\n');
        return Ok(out);
    }
    let a_path = if p.one.valid() { &p.one.path } else { &p.two.path };
    let b_path = if p.two.valid() { &p.two.path } else { &p.one.path };
    let a_name = quote::quote_two(&o.src_prefix, a_path, fully);
    let b_name = quote::quote_two(&o.dst_prefix, b_path, fully);
    let mut header = Vec::new();
    header.extend_from_slice(lp);
    header.extend_from_slice(b"diff --git ");
    header.extend_from_slice(&a_name);
    header.push(b' ');
    header.extend_from_slice(&b_name);
    header.push(b'\n');
    let line = |h: &mut Vec<u8>, s: &[u8]| {
        h.extend_from_slice(lp);
        h.extend_from_slice(s);
        h.push(b'\n');
    };
    if !p.one.valid() {
        line(&mut header, format!("new file mode {:06o}", p.two.mode).as_bytes());
    } else if !p.two.valid() {
        line(&mut header, format!("deleted file mode {:06o}", p.one.mode).as_bytes());
    } else {
        if p.one.mode != p.two.mode {
            line(&mut header, format!("old mode {:06o}", p.one.mode).as_bytes());
            line(&mut header, format!("new mode {:06o}", p.two.mode).as_bytes());
        }
        if p.status == b'R' || p.status == b'C' {
            line(&mut header, format!("similarity index {}%", p.similarity_pct()).as_bytes());
            let word = if p.status == b'R' { "rename" } else { "copy" };
            let mut l = format!("{word} from ").into_bytes();
            l.extend_from_slice(&quote::quote_c(&p.one.path, fully));
            line(&mut header, &l);
            let mut l = format!("{word} to ").into_bytes();
            l.extend_from_slice(&quote::quote_c(&p.two.path, fully));
            line(&mut header, &l);
        }
    }
    let same_content = p.one.valid() && p.two.valid() && p.one.oid == p.two.oid;
    if same_content {
        out.extend_from_slice(&header);
        return Ok(out);
    }
    let mut idx_line = format!("index {}..{}", abbrev_of(repo, &p.one.oid, o), abbrev_of(repo, &p.two.oid, o));
    if p.one.valid() && p.two.valid() && p.one.mode == p.two.mode {
        idx_line.push_str(&format!(" {:06o}", p.one.mode));
    }
    line(&mut header, idx_line.as_bytes());
    let a = content_of(repo, &p.one)?;
    let b = content_of(repo, &p.two)?;
    let minus = if p.one.valid() { quote::quote_two(&o.src_prefix, &p.one.path, fully) } else { b"/dev/null".to_vec() };
    let plus = if p.two.valid() { quote::quote_two(&o.dst_prefix, &p.two.path, fully) } else { b"/dev/null".to_vec() };
    if !o.text && (text::is_binary(&a) || text::is_binary(&b)) {
        out.extend_from_slice(&header);
        out.extend_from_slice(lp);
        out.extend_from_slice(b"Binary files ");
        out.extend_from_slice(if p.one.valid() { &minus } else { b"/dev/null" });
        out.extend_from_slice(b" and ");
        out.extend_from_slice(if p.two.valid() { &plus } else { b"/dev/null" });
        out.extend_from_slice(b" differ\n");
        return Ok(out);
    }
    let al = text::split_lines(&a);
    let bl = text::split_lines(&b);
    let ch = text::changes(&al, &bl, o.ws);
    let hs = text::hunks(&al, &bl, &ch, &o.hunk);
    if hs.is_empty() {
        // Só espaço mudou (com -w) ou nada: o git ainda mostra o cabeçalho quando há mudança de
        // modo ou é criação/remoção.
        if !p.one.valid() || !p.two.valid() || p.one.mode != p.two.mode || o.ws == Ws::default() {
            out.extend_from_slice(&header);
        }
        return Ok(out);
    }
    out.extend_from_slice(&header);
    out.extend_from_slice(lp);
    out.extend_from_slice(b"--- ");
    out.extend_from_slice(&minus);
    out.push(b'\n');
    out.extend_from_slice(lp);
    out.extend_from_slice(b"+++ ");
    out.extend_from_slice(&plus);
    out.push(b'\n');
    if lp.is_empty() {
        text::write_hunks(&mut out, &hs);
    } else {
        let mut body = Vec::new();
        text::write_hunks(&mut body, &hs);
        for l in body.split_inclusive(|c| *c == b'\n') {
            out.extend_from_slice(lp);
            out.extend_from_slice(l);
        }
    }
    Ok(out)
}

/// Linha `--raw`.
pub fn raw_line(repo: &Repo, p: &Pair, o: &DiffOpts) -> Vec<u8> {
    let ab = |id: &Oid| -> String {
        if o.raw_full || o.full_index {
            id.hex()
        } else {
            abbrev_of(repo, id, o)
        }
    };
    let mut out = o.line_prefix.clone();
    if p.status == b'U' {
        out.extend_from_slice(format!(":000000 000000 {} {} U", ab(&Oid::ZERO), ab(&Oid::ZERO)).as_bytes());
    } else {
        // O lado da árvore de trabalho não tem id gravado: o git mostra zeros.
        let two_id = if p.two.wt { Oid::ZERO } else { p.two.oid };
        out.extend_from_slice(format!(":{:06o} {:06o} {} {} ", p.one.mode, p.two.mode, ab(&p.one.oid), ab(&two_id)).as_bytes());
        out.push(p.status);
        if matches!(p.status, b'R' | b'C') {
            out.extend_from_slice(format!("{:03}", p.similarity_pct()).as_bytes());
        }
    }
    let term = if o.null_terminated { 0 } else { b'\t' };
    out.push(term);
    if matches!(p.status, b'R' | b'C') {
        out.extend_from_slice(&q(&p.one.path, o));
        out.push(term);
        out.extend_from_slice(&q(&p.two.path, o));
    } else {
        out.extend_from_slice(&q(p.path(), o));
    }
    out.push(if o.null_terminated { 0 } else { b'\n' });
    out
}

/// Escreve a fila no formato pedido. Devolve se havia diferença.
pub fn emit(repo: &Repo, pairs: &[Pair], o: &DiffOpts) -> R<bool> {
    let has = !pairs.is_empty();
    if o.quiet {
        return Ok(has);
    }
    let mut out = Vec::new();
    let nl = if o.null_terminated { 0 } else { b'\n' };
    if o.raw {
        for p in pairs {
            out.extend_from_slice(&raw_line(repo, p, o));
        }
    }
    if o.name_only {
        for p in pairs {
            out.extend_from_slice(&o.line_prefix);
            out.extend_from_slice(&q(p.path(), o));
            out.push(nl);
        }
    }
    if o.name_status {
        for p in pairs {
            out.extend_from_slice(&o.line_prefix);
            out.push(p.status);
            if matches!(p.status, b'R' | b'C') {
                out.extend_from_slice(format!("{:03}", p.similarity_pct()).as_bytes());
                out.push(if o.null_terminated { 0 } else { b'\t' });
                out.extend_from_slice(&q(&p.one.path, o));
            }
            out.push(if o.null_terminated { 0 } else { b'\t' });
            out.extend_from_slice(&q(p.path(), o));
            out.push(nl);
        }
    }
    let mut separator = !out.is_empty();
    if o.stat || o.numstat || o.shortstat {
        let mut stats = Vec::new();
        for p in pairs {
            stats.push(file_stat(repo, p, o)?);
        }
        if o.numstat {
            for (p, f) in pairs.iter().zip(&stats) {
                if f.unmerged {
                    continue;
                }
                out.extend_from_slice(&o.line_prefix);
                if f.binary {
                    out.extend_from_slice(b"-\t-\t");
                } else {
                    out.extend_from_slice(format!("{}\t{}\t", f.added, f.deleted).as_bytes());
                }
                if f.renamed {
                    if o.null_terminated {
                        out.push(0);
                        out.extend_from_slice(&p.one.path);
                        out.push(0);
                        out.extend_from_slice(&p.two.path);
                    } else {
                        out.extend_from_slice(&f.name);
                    }
                } else {
                    out.extend_from_slice(&q(p.path(), o));
                }
                out.push(nl);
            }
        }
        if o.stat {
            out.extend_from_slice(&format_stat(&stats, o));
        }
        if o.shortstat && !stats.is_empty() {
            let files = stats.iter().filter(|f| !f.unmerged).count();
            let adds: usize = stats.iter().filter(|f| !f.binary && !f.unmerged).map(|f| f.added).sum();
            let dels: usize = stats.iter().filter(|f| !f.binary && !f.unmerged).map(|f| f.deleted).sum();
            out.extend_from_slice(&o.line_prefix);
            out.extend_from_slice(&summary_line(files, adds, dels));
        }
        separator = separator || !stats.is_empty();
    }
    if o.summary {
        for p in pairs {
            out.extend_from_slice(&summary_lines(p, o));
        }
        separator = separator || !pairs.is_empty();
    }
    if o.patch && !pairs.is_empty() {
        if separator {
            out.extend_from_slice(&o.line_prefix);
            out.push(b'\n');
        }
        for p in pairs {
            out.extend_from_slice(&patch_of(repo, p, o)?);
        }
    }
    os::out(&out);
    Ok(has)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_names() {
        assert_eq!(pprint_rename(b"a.txt", b"b.txt", true), b"a.txt => b.txt");
        assert_eq!(pprint_rename(b"src/a.rs", b"src/b.rs", true), b"src/{a.rs => b.rs}");
        assert_eq!(pprint_rename(b"a/x.rs", b"b/x.rs", true), b"{a => b}/x.rs");
        assert_eq!(pprint_rename(b"x/a/y", b"x/b/y", true), b"x/{a => b}/y");
        assert_eq!(pprint_rename(b"a", b"dir/a", true), b"a => dir/a");
    }

    #[test]
    fn stat_summary() {
        assert_eq!(summary_line(1, 1, 1), b" 1 file changed, 1 insertion(+), 1 deletion(-)\n");
        assert_eq!(summary_line(3, 3, 0), b" 3 files changed, 3 insertions(+)\n");
        assert_eq!(summary_line(2, 0, 0), b" 2 files changed, 0 insertions(+), 0 deletions(-)\n");
    }
}
