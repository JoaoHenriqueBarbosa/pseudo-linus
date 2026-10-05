//! `git describe` (builtin/describe.c): o nome de um commit a partir da tag mais próxima, no
//! formato `<tag>-<n>-g<abreviação>`. A busca é a do git: caminha do commit pros pais em ordem de
//! data, junta até `--candidates` tags (10 por padrão), conta a distância de cada uma e escolhe a de
//! menor distância (empate: a encontrada primeiro), terminando a contagem da escolhida depois.

use std::collections::{HashMap, VecDeque};

use super::Git;
use crate::diff;
use crate::error::{Fail, R, warning};
use crate::hash::{Kind, Oid};
use crate::index::Index;
use crate::object::{self, Tag};
use crate::opts::{self, Spec};
use crate::os;
use crate::pathspec::Pathspec;
use crate::repo::Repo;
use crate::wildmatch;

const SPECS: &[Spec] = &[
    opts::flag(None, "all", "all"),
    opts::flag(None, "tags", "tags"),
    opts::flag(None, "long", "long"),
    opts::flag(None, "first-parent", "first-parent"),
    opts::optional(None, "abbrev", "abbrev"),
    opts::flag(None, "exact-match", "exact-match"),
    opts::value(None, "candidates", "candidates"),
    opts::value(None, "match", "match"),
    opts::value(None, "exclude", "exclude"),
    opts::flag(None, "always", "always"),
    opts::optional(None, "dirty", "dirty"),
    opts::optional(None, "broken", "broken"),
];

/// Maior número de candidatas (o `MAX_TAGS`: um bit de flag por candidata).
const MAX_TAGS: usize = 26;
/// Commit já enfileirado.
const SEEN: u32 = 1;

/// Um nome conhecido, indexado pelo commit (ou objeto) a que a ref descasca (o `commit_name`).
struct Name {
    /// O nome mostrado: sem `refs/tags/`, ou sem `refs/` com `--all`.
    path: String,
    /// O id da própria ref (a tag anotada, ou o commit numa tag leve).
    oid: Oid,
    /// 2: tag anotada; 1: tag leve; 0: outra ref (`--all`).
    prio: u8,
    tag: Option<Tag>,
    name_checked: bool,
    misnamed: bool,
}

/// Uma candidata da busca (o `possible_tag`).
struct Candidate {
    name: Oid,
    depth: usize,
    found_order: usize,
    flag_within: u32,
}

struct Options {
    all: bool,
    tags: bool,
    long: bool,
    first_parent: bool,
    always: bool,
    /// `None`: tamanho padrão do repositório; `Some(0)`: sem sufixo.
    abbrev: Option<usize>,
    max_candidates: usize,
}

/// Data do tagger de uma tag anotada (0 sem tagger), como o `tag->date`.
fn tag_date(t: &Tag) -> i64 {
    t.tagger.as_deref().and_then(object::parse_ident).and_then(|i| i.date).unwrap_or(0)
}

fn read_tag_opt(repo: &Repo, oid: &Oid) -> Option<Tag> {
    match repo.try_read(oid) {
        Ok(Some((Kind::Tag, d))) => object::parse_tag(&d).ok(),
        _ => None,
    }
}

/// O `replace_name`: a nova ref toma o lugar da que já descasca no mesmo objeto?
fn replace_name(repo: &Repo, e: Option<&mut Name>, prio: u8, oid: &Oid, tagp: &mut Option<Tag>) -> bool {
    let Some(e) = e else { return true };
    if e.prio < prio {
        return true;
    }
    if e.prio == 2 && prio == 2 {
        // Várias tags anotadas no mesmo commit: fica a mais nova.
        if e.tag.is_none() {
            match read_tag_opt(repo, &e.oid) {
                Some(t) => e.tag = Some(t),
                None => return true,
            }
        }
        let Some(t) = read_tag_opt(repo, oid) else { return false };
        let newer = tag_date(e.tag.as_ref().expect("lida acima")) < tag_date(&t);
        *tagp = Some(t);
        if newer {
            return true;
        }
    }
    false
}

/// O `get_name` sobre todas as refs, em ordem de nome.
fn load_names(repo: &Repo, all: bool, patterns: &[Vec<u8>], excludes: &[Vec<u8>]) -> R<HashMap<Oid, Name>> {
    let mut names: HashMap<Oid, Name> = HashMap::new();
    for (path, oid) in repo.list_refs("refs/")? {
        let (is_tag, to_match): (bool, Option<&str>) = if let Some(rest) = path.strip_prefix("refs/tags/") {
            (true, Some(rest))
        } else if all {
            let m = path.strip_prefix("refs/heads/").or_else(|| path.strip_prefix("refs/remotes/"));
            // Com padrões, só refs de tipo conhecido entram.
            if (!patterns.is_empty() || !excludes.is_empty()) && m.is_none() {
                continue;
            }
            (false, m)
        } else {
            continue;
        };
        if !excludes.is_empty() {
            let Some(m) = to_match else { continue };
            if excludes.iter().any(|x| wildmatch::wildmatch(x, m.as_bytes(), 0)) {
                continue;
            }
        }
        if !patterns.is_empty() {
            let Some(m) = to_match else { continue };
            if !patterns.iter().any(|x| wildmatch::wildmatch(x, m.as_bytes(), 0)) {
                continue;
            }
        }
        let peeled = repo.peel(&oid, None)?.unwrap_or(oid);
        let annotated = peeled != oid;
        let prio = if annotated {
            2
        } else if is_tag {
            1
        } else {
            0
        };
        let shown = if all { path["refs/".len()..].to_string() } else { path["refs/tags/".len()..].to_string() };
        let mut tag: Option<Tag> = None;
        if replace_name(repo, names.get_mut(&peeled), prio, &oid, &mut tag) {
            names.insert(peeled, Name { path: shown, oid, prio, tag, name_checked: false, misnamed: false });
        }
    }
    Ok(names)
}

/// O `append_name`: o nome da tag (o de dentro do objeto, avisando se difere do da ref).
fn append_name(repo: &Repo, n: &mut Name, all: bool, dst: &mut String) -> R<()> {
    if n.prio == 2 && n.tag.is_none() {
        match read_tag_opt(repo, &n.oid) {
            Some(t) => n.tag = Some(t),
            None => return Err(Fail::Fatal(format!("annotated tag {} not available", n.path))),
        }
    }
    if let Some(t) = &n.tag
        && !n.name_checked
    {
        let expected = if all { n.path.get(5..).unwrap_or("") } else { n.path.as_str() };
        if t.name != expected.as_bytes() {
            warning(&format!("tag '{}' is externally known as '{}'", n.path, os::lossy(&t.name)));
            n.misnamed = true;
        }
        n.name_checked = true;
    }
    match &n.tag {
        Some(t) => {
            if all {
                dst.push_str("tags/");
            }
            dst.push_str(&os::lossy(&t.name));
        }
        None => dst.push_str(&n.path),
    }
    Ok(())
}

/// Abreviação única com o tamanho pedido (0 é o id inteiro, como o `find_unique_abbrev`).
fn abbrev_of(repo: &Repo, oid: &Oid, abbrev: Option<usize>) -> String {
    match abbrev {
        None => repo.abbrev_default(oid),
        Some(0) => oid.hex(),
        Some(n) => repo.abbrev(oid, n),
    }
}

fn append_suffix(repo: &Repo, depth: usize, oid: &Oid, abbrev: Option<usize>, dst: &mut String) {
    dst.push_str(&format!("-{depth}-g{}", abbrev_of(repo, oid, abbrev)));
}

/// Commits já lidos: pais e data do committer.
struct Graph<'a> {
    repo: &'a Repo,
    commits: HashMap<Oid, (Vec<Oid>, i64)>,
    flags: HashMap<Oid, u32>,
}

impl Graph<'_> {
    fn info(&mut self, id: &Oid) -> R<(Vec<Oid>, i64)> {
        if let Some(c) = self.commits.get(id) {
            return Ok(c.clone());
        }
        let c = self.repo.read_commit(id)?;
        let v = (c.parents.clone(), c.commit_date());
        self.commits.insert(*id, v.clone());
        Ok(v)
    }

    fn flags(&self, id: &Oid) -> u32 {
        self.flags.get(id).copied().unwrap_or(0)
    }

    fn add_flags(&mut self, id: &Oid, f: u32) {
        *self.flags.entry(*id).or_insert(0) |= f;
    }

    /// O `commit_list_insert_by_date`: depois dos de data maior ou igual.
    fn insert_by_date(&mut self, list: &mut VecDeque<Oid>, id: Oid) -> R<()> {
        let date = self.info(&id)?.1;
        let mut pos = list.len();
        for (i, other) in list.iter().enumerate() {
            if self.commits.get(other).map(|c| c.1).unwrap_or(0) < date {
                pos = i;
                break;
            }
        }
        list.insert(pos, id);
        Ok(())
    }

    /// Enfileira os pais de `c` ainda não vistos e passa a eles as flags de `c`.
    fn push_parents(&mut self, c: &Oid, list: &mut VecDeque<Oid>, first_parent: bool) -> R<()> {
        let parents = self.info(c)?.0;
        let cf = self.flags(c);
        for p in parents {
            if self.flags(&p) & SEEN == 0 {
                self.insert_by_date(list, p)?;
            }
            self.add_flags(&p, cf);
            if first_parent {
                break;
            }
        }
        Ok(())
    }
}

/// O `finish_depth_computation`: continua a caminhada até a melhor candidata cobrir tudo o que
/// falta, contando os commits fora dela.
fn finish_depth(g: &mut Graph, list: &mut VecDeque<Oid>, best: &mut Candidate) -> R<usize> {
    let mut seen = 0;
    while let Some(c) = list.pop_front() {
        seen += 1;
        if g.flags(&c) & best.flag_within != 0 {
            if list.iter().all(|i| g.flags(i) & best.flag_within != 0) {
                break;
            }
        } else {
            best.depth += 1;
        }
        g.push_parents(&c, list, false)?;
    }
    Ok(seen)
}

/// O `describe_commit`.
fn describe_commit(repo: &Repo, names: &mut HashMap<Oid, Name>, o: &Options, cmit: Oid, suffix: &str) -> R<String> {
    let mut dst = String::new();
    if let Some(n) = names.get_mut(&cmit)
        && (o.tags || o.all || n.prio == 2)
    {
        // A própria ref nomeia o commit.
        append_name(repo, n, o.all, &mut dst)?;
        if n.misnamed || o.long {
            let target = n.tag.as_ref().map(|t| t.object).unwrap_or(cmit);
            append_suffix(repo, 0, &target, o.abbrev, &mut dst);
        }
        dst.push_str(suffix);
        return Ok(dst);
    }
    if o.max_candidates == 0 {
        return Err(Fail::Fatal(format!("no tag exactly matches '{cmit}'")));
    }
    let mut g = Graph { repo, commits: HashMap::new(), flags: HashMap::new() };
    let mut list: VecDeque<Oid> = VecDeque::new();
    g.add_flags(&cmit, SEEN);
    g.info(&cmit)?;
    list.push_back(cmit);
    let mut matches: Vec<Candidate> = Vec::new();
    let mut annotated = 0;
    let mut unannotated = 0;
    let mut seen_commits = 0;
    let mut gave_up_on: Option<Oid> = None;
    while let Some(c) = list.pop_front() {
        seen_commits += 1;
        if let Some(n) = names.get(&c) {
            if !o.tags && !o.all && n.prio < 2 {
                unannotated += 1;
            } else if matches.len() < o.max_candidates {
                let found_order = matches.len() + 1;
                let flag_within = 1u32 << found_order;
                matches.push(Candidate { name: c, depth: seen_commits - 1, found_order, flag_within });
                g.add_flags(&c, flag_within);
                if n.prio == 2 {
                    annotated += 1;
                }
            } else {
                gave_up_on = Some(c);
                break;
            }
        }
        let cf = g.flags(&c);
        for t in matches.iter_mut() {
            if cf & t.flag_within == 0 {
                t.depth += 1;
            }
        }
        // O último caminho que restava já está coberto pelas candidatas.
        if annotated > 0 && list.is_empty() {
            break;
        }
        g.push_parents(&c, &mut list, o.first_parent)?;
    }
    if matches.is_empty() {
        if o.always {
            dst.push_str(&abbrev_of(repo, &cmit, o.abbrev));
            dst.push_str(suffix);
            return Ok(dst);
        }
        if unannotated > 0 {
            return Err(Fail::Fatal(format!("No annotated tags can describe '{cmit}'.\nHowever, there were unannotated tags: try --tags.")));
        }
        return Err(Fail::Fatal(format!("No tags can describe '{cmit}'.\nTry --always, or create some tags.")));
    }
    // Ordenação estável por distância e, no empate, pela ordem em que foram achadas.
    matches.sort_by(|a, b| a.depth.cmp(&b.depth).then(a.found_order.cmp(&b.found_order)));
    if let Some(c) = gave_up_on {
        g.insert_by_date(&mut list, c)?;
    }
    let mut best = matches.swap_remove(0);
    finish_depth(&mut g, &mut list, &mut best)?;
    let n = names.get_mut(&best.name).expect("candidata conhecida");
    append_name(repo, n, o.all, &mut dst)?;
    if n.misnamed || o.abbrev != Some(0) {
        append_suffix(repo, best.depth, &cmit, o.abbrev, &mut dst);
    }
    dst.push_str(suffix);
    Ok(dst)
}

/// A árvore de trabalho ou o índice diferem do HEAD (o `diff-index --quiet HEAD --`)?
fn worktree_dirty(repo: &Repo) -> R<bool> {
    repo.work_tree()?;
    let idx = Index::load(&repo.index_path())?;
    let Some(head) = repo.head_oid()? else { return Ok(true) };
    let ht = repo.tree_of(&head)?;
    let ps = Pathspec::default();
    if !diff::diff_tree_index(repo, Some(&ht), &idx, &ps)?.is_empty() {
        return Ok(true);
    }
    Ok(!diff::diff_index_worktree(repo, &idx, &ps)?.is_empty())
}

/// `git describe [--all] [--tags] [--contains] [--abbrev=<n>] [<commit-ish>...]` e `--dirty`.
pub fn run(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let p = opts::parse(SPECS, args, 0, usage)?;
    let repo = git.repo()?;
    let abbrev: Option<usize> = match p.hits.iter().rev().find(|h| h.id == "abbrev") {
        None => None,
        Some(h) if h.negated => Some(0),
        Some(h) => match h.value.as_deref() {
            None => None,
            Some(v) => match os::lossy(v).parse::<i64>() {
                Ok(n) if n <= 0 => Some(0),
                Ok(n) => Some((n as usize).clamp(4, 40)),
                Err(_) => return Err(opts::usage_error(usage, "option `abbrev' expects a numerical value")),
            },
        },
    };
    let mut max_candidates: i64 = match p.value("candidates") {
        None => 10,
        Some(v) => match os::lossy(v).parse::<i64>() {
            Ok(n) => n,
            Err(_) => return Err(opts::usage_error(usage, "option `candidates' expects a numerical value")),
        },
    };
    if p.has("exact-match") {
        max_candidates = 0;
    }
    let max_candidates = max_candidates.clamp(0, MAX_TAGS as i64) as usize;
    let o = Options {
        all: p.has("all"),
        tags: p.has("tags"),
        long: p.has("long"),
        first_parent: p.has("first-parent"),
        always: p.has("always"),
        abbrev,
        max_candidates,
    };
    if o.long && o.abbrev == Some(0) {
        return Err(Fail::Fatal("options '--long' and '--abbrev=0' cannot be used together".into()));
    }
    let dirty_hit = p.hits.iter().rev().find(|h| h.id == "dirty").filter(|h| !h.negated);
    let broken_hit = p.hits.iter().rev().find(|h| h.id == "broken").filter(|h| !h.negated);
    let names_args: Vec<Vec<u8>> = p.args.clone();
    let mut suffix = String::new();
    if names_args.is_empty() {
        if let Some(h) = broken_hit.or(dirty_hit) {
            let mark = h.value.as_deref().map(os::lossy).unwrap_or_else(|| if h.id == "broken" { "-broken".into() } else { "-dirty".into() });
            if worktree_dirty(repo)? {
                suffix = mark;
            }
        }
    } else if dirty_hit.is_some() {
        return Err(Fail::Fatal("option '--dirty' and commit-ishes cannot be used together".into()));
    } else if broken_hit.is_some() {
        return Err(Fail::Fatal("option '--broken' and commit-ishes cannot be used together".into()));
    }
    let mut names = load_names(repo, o.all, &p.values("match"), &p.values("exclude"))?;
    if names.is_empty() && !o.always {
        return Err(Fail::Fatal("No names found, cannot describe anything.".into()));
    }
    let targets: Vec<Vec<u8>> = if names_args.is_empty() { vec![b"HEAD".to_vec()] } else { names_args };
    for arg in &targets {
        let Some(oid) = repo.rev_parse(arg)? else {
            return Err(Fail::Fatal(format!("Not a valid object name {}", os::lossy(arg))));
        };
        let Some(cmit) = repo.peel_to_commit(&oid)? else {
            return Err(Fail::Fatal(format!("{} is neither a commit nor blob", os::lossy(arg))));
        };
        let line = describe_commit(repo, &mut names, &o, cmit, &suffix)?;
        os::outs(&format!("{line}\n"));
    }
    Ok(0)
}
