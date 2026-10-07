//! O motor de refs do `for-each-ref`, `branch` e `tag`: coleta, filtros (`--contains`, `--merged`,
//! `--points-at`, padrões), ordenação (`--sort`) e o formato `--format` com os átomos principais
//! (`%(refname)`, `%(objectname)`, `%(upstream:track)`, `%(if)`, `%(align)`...).

use std::collections::HashMap;
use std::rc::Rc;

use crate::date::{self, DateMode};
use crate::error::{Fail, R};
use crate::graph::Graph;
use crate::hash::{Kind, Oid};
use crate::object::{self, Commit, Tag};
use crate::opts;
use crate::os;
use crate::refs::Head;
use crate::repo::Repo;
use crate::wildmatch::{self, CASEFOLD, PATHNAME};

/// Uma ref candidata.
#[derive(Clone, Debug)]
pub struct Row {
    pub name: String,
    pub oid: Oid,
    pub symref: Option<String>,
}

/// Objeto lido, com o que os átomos precisam.
pub struct Loaded {
    pub kind: Kind,
    pub size: usize,
    pub commit: Option<Commit>,
    pub tag: Option<Tag>,
}

/// Como os valores dos átomos são citados na saída (`--shell`, `--perl`, `--python`, `--tcl`).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Quote {
    None,
    Shell,
    Perl,
    Python,
    Tcl,
}

// ---- átomos e formato -------------------------------------------------------------------------

/// Um átomo de valor: `%(nome:argumento)`, com `*` na frente pra olhar o objeto apontado pela tag.
#[derive(Clone, Debug)]
pub struct Field {
    pub name: String,
    pub arg: String,
    pub deref: bool,
}

#[derive(Clone, Debug)]
pub enum IfTest {
    NonEmpty,
    Equals(String),
    NotEquals(String),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AlignPos {
    Left,
    Right,
    Middle,
}

#[derive(Clone, Debug)]
pub enum Node {
    Lit(Vec<u8>),
    Field(Field),
    Color(String),
    If { test: IfTest, cond: Vec<Node>, then_: Vec<Node>, else_: Vec<Node> },
    Align { width: usize, pos: AlignPos, body: Vec<Node> },
}

/// Fichas da primeira passada do formato.
enum Tok {
    Lit(Vec<u8>),
    Atom(String),
}

const VALUE_ATOMS: &[&str] = &[
    "refname", "objectname", "objecttype", "objectsize", "tree", "parent", "numparent", "object", "type", "tag", "author",
    "authorname", "authoremail", "authordate", "committer", "committername", "committeremail", "committerdate", "tagger",
    "taggername", "taggeremail", "taggerdate", "creator", "creatordate", "subject", "body", "contents", "upstream", "push", "HEAD",
    "symref", "worktreepath",
];

fn tokenize(fmt: &[u8], usage: &str) -> R<Vec<Tok>> {
    let mut toks = Vec::new();
    let mut lit: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < fmt.len() {
        let c = fmt[i];
        if c != b'%' {
            lit.push(c);
            i += 1;
            continue;
        }
        match fmt.get(i + 1) {
            Some(b'%') => {
                lit.push(b'%');
                i += 2;
            }
            Some(b'(') => {
                let Some(close) = fmt[i + 2..].iter().position(|b| *b == b')') else {
                    let rest = os::lossy(&fmt[i..]);
                    return Err(opts::usage_error(usage, &format!("malformed format string {rest}")));
                };
                if !lit.is_empty() {
                    toks.push(Tok::Lit(std::mem::take(&mut lit)));
                }
                toks.push(Tok::Atom(os::lossy(&fmt[i + 2..i + 2 + close])));
                i += close + 3;
            }
            Some(b'x') if i + 3 < fmt.len() && crate::hash::hex_val(fmt[i + 2]).is_some() && crate::hash::hex_val(fmt[i + 3]).is_some() => {
                let hi = crate::hash::hex_val(fmt[i + 2]).unwrap_or(0);
                let lo = crate::hash::hex_val(fmt[i + 3]).unwrap_or(0);
                lit.push((hi << 4) | lo);
                i += 4;
            }
            _ => {
                lit.push(b'%');
                i += 1;
            }
        }
    }
    if !lit.is_empty() {
        toks.push(Tok::Lit(lit));
    }
    Ok(toks)
}

/// Fechamentos que interrompem a leitura de uma lista de nós.
#[derive(PartialEq, Eq, Clone, Copy)]
enum Stop {
    Eof,
    Then,
    Else,
    End,
}

fn parse_nodes(toks: &[Tok], pos: &mut usize, until: &[Stop]) -> R<(Vec<Node>, Stop)> {
    let mut out = Vec::new();
    while *pos < toks.len() {
        let t = &toks[*pos];
        *pos += 1;
        match t {
            Tok::Lit(l) => out.push(Node::Lit(l.clone())),
            Tok::Atom(a) => {
                let (name, arg) = match a.split_once(':') {
                    Some((n, r)) => (n.to_string(), r.to_string()),
                    None => (a.clone(), String::new()),
                };
                match name.as_str() {
                    "then" => {
                        if until.contains(&Stop::Then) {
                            return Ok((out, Stop::Then));
                        }
                        return Err(Fail::Fatal("format: %(then) atom used without an %(if) atom".into()));
                    }
                    "else" => {
                        if until.contains(&Stop::Else) {
                            return Ok((out, Stop::Else));
                        }
                        return Err(Fail::Fatal("format: %(else) atom used without an %(if) atom".into()));
                    }
                    "end" => {
                        if until.contains(&Stop::End) {
                            return Ok((out, Stop::End));
                        }
                        return Err(Fail::Fatal("format: %(end) atom used without corresponding atom".into()));
                    }
                    "if" => {
                        let test = if arg.is_empty() {
                            IfTest::NonEmpty
                        } else if let Some(v) = arg.strip_prefix("equals=") {
                            IfTest::Equals(v.to_string())
                        } else if let Some(v) = arg.strip_prefix("notequals=") {
                            IfTest::NotEquals(v.to_string())
                        } else {
                            return Err(Fail::Fatal(format!("unrecognized %(if) argument: {arg}")));
                        };
                        let (cond, stop) = parse_nodes(toks, pos, &[Stop::Then])?;
                        if stop != Stop::Then {
                            return Err(Fail::Fatal("format: %(if) atom used without a %(then) atom".into()));
                        }
                        let (then_, stop) = parse_nodes(toks, pos, &[Stop::Else, Stop::End])?;
                        let mut else_ = Vec::new();
                        if stop == Stop::Else {
                            let (e, stop2) = parse_nodes(toks, pos, &[Stop::End])?;
                            if stop2 != Stop::End {
                                return Err(Fail::Fatal("format: %(if) atom used without a %(end) atom".into()));
                            }
                            else_ = e;
                        } else if stop != Stop::End {
                            return Err(Fail::Fatal("format: %(if) atom used without a %(end) atom".into()));
                        }
                        out.push(Node::If { test, cond, then_, else_ });
                    }
                    "align" => {
                        let mut width = 0usize;
                        let mut apos = AlignPos::Left;
                        let mut have_width = false;
                        for part in arg.split(',').filter(|p| !p.is_empty()) {
                            if let Some(w) = part.strip_prefix("width=") {
                                width = w.parse().map_err(|_| Fail::Fatal("positive width expected with the %(align) atom".into()))?;
                                have_width = true;
                            } else if let Some(p) = part.strip_prefix("position=") {
                                apos = parse_align_pos(p)?;
                            } else if let Ok(w) = part.parse::<usize>() {
                                width = w;
                                have_width = true;
                            } else {
                                apos = parse_align_pos(part)?;
                            }
                        }
                        if !have_width {
                            return Err(Fail::Fatal("positive width expected with the %(align) atom".into()));
                        }
                        let (body, stop) = parse_nodes(toks, pos, &[Stop::End])?;
                        if stop != Stop::End {
                            return Err(Fail::Fatal("format: %(align) atom used without a %(end) atom".into()));
                        }
                        out.push(Node::Align { width, pos: apos, body });
                    }
                    "color" => out.push(Node::Color(arg)),
                    _ => {
                        let (deref, base) = match name.strip_prefix('*') {
                            Some(b) => (true, b.to_string()),
                            None => (false, name.clone()),
                        };
                        if !VALUE_ATOMS.contains(&base.as_str()) {
                            return Err(Fail::Fatal(format!("unknown field name: {name}")));
                        }
                        out.push(Node::Field(Field { name: base, arg, deref }));
                    }
                }
            }
        }
    }
    Ok((out, Stop::Eof))
}

fn parse_align_pos(p: &str) -> R<AlignPos> {
    match p {
        "left" => Ok(AlignPos::Left),
        "right" => Ok(AlignPos::Right),
        "middle" => Ok(AlignPos::Middle),
        other => Err(Fail::Fatal(format!("unrecognized position:{other}"))),
    }
}

/// Lê um `--format`. Erros de sintaxe saem como o git: `error:` e o uso (exit 129).
pub fn parse_format(fmt: &[u8], usage: &str) -> R<Vec<Node>> {
    let toks = tokenize(fmt, usage)?;
    let mut pos = 0;
    let (nodes, _) = parse_nodes(&toks, &mut pos, &[])?;
    Ok(nodes)
}

// ---- contexto ---------------------------------------------------------------------------------

pub struct Ctx<'a> {
    pub repo: &'a Repo,
    pub graph: Graph<'a>,
    pub color: bool,
    pub quote: Quote,
    objs: HashMap<Oid, Option<Rc<Loaded>>>,
    head_ref: Option<String>,
}

/// Estado de um ramo frente ao seu upstream.
pub struct Upstream {
    /// Nome completo da ref de acompanhamento (`refs/remotes/origin/main`).
    pub name: String,
    /// `(à frente, atrás)`, ou `None` se a ref não existe mais (`gone`).
    pub counts: Option<(usize, usize)>,
}

/// A ref de acompanhamento do upstream de um ramo, pelo `remote.<r>.fetch` (ou a própria ref local
/// se o remoto é `.`).
pub fn upstream_ref_name(repo: &Repo, branch: &str) -> Option<String> {
    let remote = repo.config.get(&format!("branch.{branch}.remote"))?;
    let merge = repo.config.get(&format!("branch.{branch}.merge"))?;
    if remote == "." {
        return Some(merge);
    }
    for spec in repo.config.get_all(&format!("remote.{remote}.fetch")).into_iter().flatten() {
        let spec = os::lossy(spec);
        let spec = spec.strip_prefix('+').unwrap_or(&spec);
        let Some((src, dst)) = spec.split_once(':') else { continue };
        if let Some(sp) = src.strip_suffix('*') {
            if let (Some(rest), Some(dp)) = (merge.strip_prefix(sp), dst.strip_suffix('*')) {
                return Some(format!("{dp}{rest}"));
            }
        } else if src == merge {
            return Some(dst.to_string());
        }
    }
    None
}

impl<'a> Ctx<'a> {
    pub fn new(repo: &'a Repo) -> R<Ctx<'a>> {
        let head_ref = match repo.head() {
            Ok(Head::Branch(name, _)) => Some(name),
            _ => None,
        };
        Ok(Ctx { repo, graph: Graph::new(repo), color: false, quote: Quote::None, objs: HashMap::new(), head_ref })
    }

    pub fn head_ref(&self) -> Option<&str> {
        self.head_ref.as_deref()
    }

    pub fn load(&mut self, oid: &Oid) -> R<Option<Rc<Loaded>>> {
        if let Some(o) = self.objs.get(oid) {
            return Ok(o.clone());
        }
        let loaded = match self.repo.try_read(oid)? {
            None => None,
            Some((kind, data)) => {
                let commit = if kind == Kind::Commit { object::parse_commit(&data).ok() } else { None };
                let tag = if kind == Kind::Tag { object::parse_tag(&data).ok() } else { None };
                Some(Rc::new(Loaded { kind, size: data.len(), commit, tag }))
            }
        };
        self.objs.insert(*oid, loaded.clone());
        Ok(loaded)
    }

    /// O objeto da ref, ou (com `*`) o que a tag aponta.
    fn view(&mut self, row: &Row, deref: bool) -> R<Option<(Oid, Rc<Loaded>)>> {
        let Some(base) = self.load(&row.oid)? else { return Ok(None) };
        if !deref {
            return Ok(Some((row.oid, base)));
        }
        let Some(tag) = &base.tag else { return Ok(None) };
        let target = tag.object;
        Ok(self.load(&target)?.map(|l| (target, l)))
    }

    /// Estado do upstream de uma ref `refs/heads/x`.
    pub fn upstream(&mut self, row_name: &str, oid: &Oid) -> R<Option<Upstream>> {
        let Some(branch) = row_name.strip_prefix("refs/heads/") else { return Ok(None) };
        let Some(up) = upstream_ref_name(self.repo, branch) else { return Ok(None) };
        let counts = match self.repo.ref_oid(&up)? {
            None => None,
            Some(u) => match (self.repo.peel_to_commit(oid)?, self.repo.peel_to_commit(&u)?) {
                (Some(a), Some(b)) => Some(self.graph.ahead_behind(&a, &b)?),
                _ => None,
            },
        };
        Ok(Some(Upstream { name: up, counts }))
    }

    /// Estado do destino de `push`: `branch.<n>.pushRemote`, `remote.pushDefault` ou o remoto do ramo.
    fn push_state(&mut self, row_name: &str, oid: &Oid) -> R<Option<Upstream>> {
        let Some(branch) = row_name.strip_prefix("refs/heads/") else { return Ok(None) };
        let cfg = &self.repo.config;
        let remote = cfg
            .get(&format!("branch.{branch}.pushremote"))
            .or_else(|| cfg.get("remote.pushdefault"))
            .or_else(|| cfg.get(&format!("branch.{branch}.remote")));
        let Some(remote) = remote else { return Ok(None) };
        if remote == "." {
            return Ok(None);
        }
        let name = format!("refs/remotes/{remote}/{branch}");
        let counts = match self.repo.ref_oid(&name)? {
            None => None,
            Some(u) => match (self.repo.peel_to_commit(oid)?, self.repo.peel_to_commit(&u)?) {
                (Some(a), Some(b)) => Some(self.graph.ahead_behind(&a, &b)?),
                _ => None,
            },
        };
        Ok(Some(Upstream { name, counts }))
    }

    // ---- saída ----------------------------------------------------------------------------

    /// Escreve o formato pra uma ref.
    pub fn render(&mut self, nodes: &[Node], row: &Row, out: &mut Vec<u8>) -> R<()> {
        self.render_depth(nodes, row, out, 0)
    }

    fn render_depth(&mut self, nodes: &[Node], row: &Row, out: &mut Vec<u8>, depth: usize) -> R<()> {
        for n in nodes {
            match n {
                Node::Lit(l) => out.extend_from_slice(l),
                Node::Field(f) => {
                    let v = self.field(row, f)?;
                    if depth == 0 {
                        let q = quote_value(self.quote, &v);
                        out.extend_from_slice(&q);
                    } else {
                        out.extend_from_slice(&v);
                    }
                }
                Node::Color(spec) => {
                    if self.color {
                        out.extend_from_slice(&color_code(spec)?);
                    }
                }
                Node::If { test, cond, then_, else_ } => {
                    let mut c = Vec::new();
                    self.render_depth(cond, row, &mut c, depth + 1)?;
                    let ok = match test {
                        IfTest::NonEmpty => !object::trim_ascii(&c).is_empty(),
                        IfTest::Equals(v) => c == v.as_bytes(),
                        IfTest::NotEquals(v) => c != v.as_bytes(),
                    };
                    let mut body = Vec::new();
                    self.render_depth(if ok { then_ } else { else_ }, row, &mut body, depth + 1)?;
                    if depth == 0 {
                        out.extend_from_slice(&quote_value(self.quote, &body));
                    } else {
                        out.extend_from_slice(&body);
                    }
                }
                Node::Align { width, pos, body } => {
                    let mut b = Vec::new();
                    self.render_depth(body, row, &mut b, depth + 1)?;
                    let shown = String::from_utf8_lossy(&b).chars().count();
                    let pad = width.saturating_sub(shown);
                    let (left, right) = match pos {
                        AlignPos::Left => (0, pad),
                        AlignPos::Right => (pad, 0),
                        AlignPos::Middle => (pad / 2, pad - pad / 2),
                    };
                    let mut padded = vec![b' '; left];
                    padded.extend_from_slice(&b);
                    padded.extend(std::iter::repeat_n(b' ', right));
                    if depth == 0 {
                        out.extend_from_slice(&quote_value(self.quote, &padded));
                    } else {
                        out.extend_from_slice(&padded);
                    }
                }
            }
        }
        Ok(())
    }

    /// Valor de um átomo pra uma ref.
    pub fn field(&mut self, row: &Row, f: &Field) -> R<Vec<u8>> {
        let name = f.name.as_str();
        match name {
            "refname" => {
                if f.deref {
                    return Ok(Vec::new());
                }
                let mut v = row.name.clone();
                for a in f.arg.split(',').filter(|s| !s.is_empty()) {
                    v = self.refname_arg(&v, &row.name, a)?;
                }
                Ok(v.into_bytes())
            }
            "symref" => {
                let Some(t) = &row.symref else { return Ok(Vec::new()) };
                match f.arg.as_str() {
                    "" => Ok(t.clone().into_bytes()),
                    "short" => Ok(super::plumbing::shorten_ref(self.repo, t).into_bytes()),
                    a => Err(Fail::Fatal(format!("unrecognized %(symref) argument: {a}"))),
                }
            }
            "HEAD" => Ok(if self.head_ref.as_deref() == Some(row.name.as_str()) { b"*".to_vec() } else { b" ".to_vec() }),
            "worktreepath" => {
                if self.head_ref.as_deref() == Some(row.name.as_str())
                    && let Some(wt) = &self.repo.work_tree
                {
                    let mut p = wt.clone();
                    while p.len() > 1 && p.ends_with(b"/") {
                        p.pop();
                    }
                    return Ok(p);
                }
                Ok(Vec::new())
            }
            "upstream" | "push" => self.upstream_field(row, f),
            "objectname" | "objecttype" | "objectsize" | "tree" | "parent" | "numparent" | "object" | "type" | "tag" => self.object_field(row, f),
            "subject" | "body" | "contents" => self.contents_field(row, f),
            _ => self.ident_field(row, f),
        }
    }

    fn refname_arg(&self, cur: &str, full: &str, arg: &str) -> R<String> {
        if arg == "short" {
            return Ok(super::plumbing::shorten_ref(self.repo, full));
        }
        let (left, n) = if let Some(n) = arg.strip_prefix("lstrip=").or_else(|| arg.strip_prefix("strip=")) {
            (true, n)
        } else if let Some(n) = arg.strip_prefix("rstrip=") {
            (false, n)
        } else {
            return Err(Fail::Fatal(format!("unrecognized %(refname) argument: {arg}")));
        };
        let n: i64 = n.parse().map_err(|_| Fail::Fatal(format!("positive value expected refname:{}", arg)))?;
        let parts: Vec<&str> = cur.split('/').collect();
        let len = parts.len() as i64;
        let strip = (if n >= 0 { n.min(len) } else { (len + n).max(0) }) as usize;
        Ok(if left { parts[strip..].join("/") } else { parts[..parts.len() - strip].join("/") })
    }

    fn upstream_field(&mut self, row: &Row, f: &Field) -> R<Vec<u8>> {
        if f.deref {
            return Ok(Vec::new());
        }
        let st = if f.name == "push" { self.push_state(&row.name, &row.oid)? } else { self.upstream(&row.name, &row.oid)? };
        let Some(st) = st else { return Ok(Vec::new()) };
        let mut mode = "";
        let mut nobracket = false;
        for a in f.arg.split(',').filter(|s| !s.is_empty()) {
            match a {
                "short" | "track" | "trackshort" | "remotename" | "remoteref" => mode = a,
                "nobracket" => nobracket = true,
                other => return Err(Fail::Fatal(format!("unrecognized %({}) argument: {other}", f.name))),
            }
        }
        let branch = row.name.strip_prefix("refs/heads/").unwrap_or(&row.name).to_string();
        let text = match mode {
            "" => st.name.clone(),
            "short" => super::plumbing::shorten_ref(self.repo, &st.name),
            "track" => match st.counts {
                None => if nobracket { "gone".to_string() } else { "[gone]".to_string() },
                Some((0, 0)) => String::new(),
                Some((a, 0)) => bracket(format!("ahead {a}"), nobracket),
                Some((0, b)) => bracket(format!("behind {b}"), nobracket),
                Some((a, b)) => bracket(format!("ahead {a}, behind {b}"), nobracket),
            },
            "trackshort" => match st.counts {
                None => String::new(),
                Some((0, 0)) => "=".to_string(),
                Some((_, 0)) => ">".to_string(),
                Some((0, _)) => "<".to_string(),
                Some(_) => "<>".to_string(),
            },
            "remotename" => {
                if f.name == "push" {
                    let cfg = &self.repo.config;
                    cfg.get(&format!("branch.{branch}.pushremote")).or_else(|| cfg.get("remote.pushdefault")).or_else(|| cfg.get(&format!("branch.{branch}.remote"))).unwrap_or_default()
                } else {
                    self.repo.config.get(&format!("branch.{branch}.remote")).unwrap_or_default()
                }
            }
            _ => {
                if f.name == "push" {
                    format!("refs/heads/{branch}")
                } else {
                    self.repo.config.get(&format!("branch.{branch}.merge")).unwrap_or_default()
                }
            }
        };
        Ok(text.into_bytes())
    }

    fn object_field(&mut self, row: &Row, f: &Field) -> R<Vec<u8>> {
        let Some((oid, obj)) = self.view(row, f.deref)? else { return Ok(Vec::new()) };
        let name = f.name.as_str();
        match name {
            "objectname" => match f.arg.as_str() {
                "" => Ok(oid.hex().into_bytes()),
                "short" => Ok(self.repo.abbrev_default(&oid).into_bytes()),
                a => match a.strip_prefix("short=").and_then(|n| n.parse::<usize>().ok()) {
                    Some(n) => Ok(self.repo.abbrev(&oid, n.max(4)).into_bytes()),
                    None => Err(Fail::Fatal(format!("unrecognized %(objectname) argument: {a}"))),
                },
            },
            "objecttype" => Ok(obj.kind.name().as_bytes().to_vec()),
            "objectsize" => Ok(obj.size.to_string().into_bytes()),
            "tree" => Ok(obj.commit.as_ref().map(|c| c.tree.hex().into_bytes()).unwrap_or_default()),
            "parent" => Ok(obj.commit.as_ref().map(|c| c.parents.iter().map(|p| p.hex()).collect::<Vec<_>>().join(" ").into_bytes()).unwrap_or_default()),
            "numparent" => Ok(obj.commit.as_ref().map(|c| c.parents.len().to_string().into_bytes()).unwrap_or_default()),
            "object" => Ok(obj.tag.as_ref().map(|t| t.object.hex().into_bytes()).unwrap_or_default()),
            "type" => Ok(obj.tag.as_ref().map(|t| t.kind.name().as_bytes().to_vec()).unwrap_or_default()),
            _ => Ok(obj.tag.as_ref().map(|t| t.name.clone()).unwrap_or_default()),
        }
    }

    fn contents_field(&mut self, row: &Row, f: &Field) -> R<Vec<u8>> {
        let Some((_, obj)) = self.view(row, f.deref)? else { return Ok(Vec::new()) };
        let msg: &[u8] = if let Some(c) = &obj.commit {
            &c.message
        } else if let Some(t) = &obj.tag {
            &t.message
        } else {
            return Ok(Vec::new());
        };
        match f.name.as_str() {
            "subject" => Ok(object::subject_of(msg)),
            "body" => Ok(object::body_of(msg)),
            _ => match f.arg.as_str() {
                "" => Ok(msg.to_vec()),
                "subject" => Ok(object::subject_of(msg)),
                "body" => Ok(object::body_of(msg)),
                "signature" => Ok(Vec::new()),
                a => match a.strip_prefix("lines=").and_then(|n| n.parse::<usize>().ok()) {
                    Some(n) => Ok(first_lines(msg, n)),
                    None => Err(Fail::Fatal(format!("unrecognized %(contents) argument: {a}"))),
                },
            },
        }
    }

    fn ident_field(&mut self, row: &Row, f: &Field) -> R<Vec<u8>> {
        let name = f.name.as_str();
        for who in ["author", "committer", "tagger", "creator"] {
            let Some(part) = name.strip_prefix(who) else { continue };
            if !matches!(part, "" | "name" | "email" | "date") {
                continue;
            }
            let Some((_, obj)) = self.view(row, f.deref)? else { return Ok(Vec::new()) };
            let line: Option<&Vec<u8>> = match who {
                "author" => obj.commit.as_ref().map(|c| &c.author),
                "committer" => obj.commit.as_ref().map(|c| &c.committer),
                "tagger" => obj.tag.as_ref().and_then(|t| t.tagger.as_ref()),
                _ => match (&obj.tag, &obj.commit) {
                    (Some(t), _) => t.tagger.as_ref(),
                    (None, Some(c)) => Some(&c.committer),
                    _ => None,
                },
            };
            let Some(line) = line else { return Ok(Vec::new()) };
            if part.is_empty() {
                return Ok(line.clone());
            }
            let Some(id) = object::parse_ident(line) else { return Ok(Vec::new()) };
            return match part {
                "name" => Ok(id.name),
                "email" => {
                    let mut email = id.email.clone();
                    for a in f.arg.split(',').filter(|s| !s.is_empty()) {
                        match a {
                            "trim" => {}
                            "localpart" => {
                                if let Some(at) = email.iter().position(|c| *c == b'@') {
                                    email.truncate(at);
                                }
                            }
                            "mailmap" => {}
                            other => return Err(Fail::Fatal(format!("unrecognized email option: {other}"))),
                        }
                    }
                    if f.arg.is_empty() {
                        let mut v = b"<".to_vec();
                        v.extend_from_slice(&email);
                        v.push(b'>');
                        return Ok(v);
                    }
                    Ok(email)
                }
                _ => {
                    let Some(t) = id.date else { return Ok(Vec::new()) };
                    let mode = if f.arg.is_empty() {
                        DateMode::Normal
                    } else {
                        DateMode::parse(&f.arg).ok_or_else(|| Fail::Fatal(format!("unknown date format {}", f.arg)))?
                    };
                    Ok(date::show_date(t, id.tz, &mode).into_bytes())
                }
            };
        }
        Err(Fail::Fatal(format!("unknown field name: {name}")))
    }
}

fn bracket(s: String, nobracket: bool) -> String {
    if nobracket { s } else { format!("[{s}]") }
}

/// As primeiras `n` linhas, juntas por `\n` mais quatro espaços (o `-n` do `git tag`).
fn first_lines(msg: &[u8], n: usize) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut sp = 0;
    let mut i = 0;
    while i < n && sp < msg.len() {
        if i > 0 {
            out.extend_from_slice(b"\n    ");
        }
        match msg[sp..].iter().position(|c| *c == b'\n') {
            Some(eol) => {
                out.extend_from_slice(&msg[sp..sp + eol]);
                sp += eol + 1;
            }
            None => {
                out.extend_from_slice(&msg[sp..]);
                break;
            }
        }
        i += 1;
    }
    out
}

/// Cita um valor pro modo pedido.
pub fn quote_value(q: Quote, v: &[u8]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    match q {
        Quote::None => out.extend_from_slice(v),
        Quote::Shell => {
            out.push(b'\'');
            for &c in v {
                if c == b'\'' {
                    out.extend_from_slice(b"'\\''");
                } else {
                    out.push(c);
                }
            }
            out.push(b'\'');
        }
        Quote::Perl | Quote::Python => {
            out.push(b'\'');
            for &c in v {
                if c == b'\\' || c == b'\'' {
                    out.push(b'\\');
                }
                out.push(c);
            }
            out.push(b'\'');
        }
        Quote::Tcl => {
            out.push(b'"');
            for &c in v {
                match c {
                    b'[' | b']' | b'{' | b'}' | b'$' | b'\\' | b'"' => {
                        out.push(b'\\');
                        out.push(c);
                    }
                    0x0c => out.extend_from_slice(b"\\f"),
                    b'\r' => out.extend_from_slice(b"\\r"),
                    b'\n' => out.extend_from_slice(b"\\n"),
                    b'\t' => out.extend_from_slice(b"\\t"),
                    0x0b => out.extend_from_slice(b"\\v"),
                    _ => out.push(c),
                }
            }
            out.push(b'"');
        }
    }
    out
}


/// Sequência ANSI de `%(color:...)`.
fn color_code(spec: &str) -> R<Vec<u8>> {
    if spec == "reset" {
        return Ok(b"\x1b[m".to_vec());
    }
    let mut attrs: Vec<String> = Vec::new();
    let mut colors: Vec<String> = Vec::new();
    for w in spec.split_whitespace() {
        let attr = match w {
            "bold" => Some("1"),
            "dim" => Some("2"),
            "italic" => Some("3"),
            "ul" => Some("4"),
            "blink" => Some("5"),
            "reverse" => Some("7"),
            "strike" => Some("9"),
            _ => None,
        };
        if let Some(a) = attr {
            attrs.push(a.to_string());
            continue;
        }
        let names = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"];
        let (bright, base) = match w.strip_prefix("bright") {
            Some(b) => (true, b),
            None => (false, w),
        };
        if let Some(i) = names.iter().position(|n| *n == base) {
            let fg = colors.is_empty();
            let code = match (fg, bright) {
                (true, false) => 30 + i,
                (true, true) => 90 + i,
                (false, false) => 40 + i,
                (false, true) => 100 + i,
            };
            colors.push(code.to_string());
        } else if w == "normal" {
            colors.push(if colors.is_empty() { "39".to_string() } else { "49".to_string() });
        } else {
            return Err(Fail::Fatal(format!("unrecognized color: %(color:{spec})")));
        }
    }
    attrs.extend(colors);
    if attrs.is_empty() {
        return Ok(Vec::new());
    }
    Ok(format!("\x1b[{}m", attrs.join(";")).into_bytes())
}

// ---- coleta e filtros -------------------------------------------------------------------------

/// O que entra na listagem.
#[derive(Default, Clone)]
pub struct Filter {
    /// Prefixos de ref aceitos (`refs/heads/`...); vazio = qualquer uma.
    pub kinds: Vec<String>,
    pub patterns: Vec<String>,
    pub exclude: Vec<String>,
    /// `for-each-ref`: o padrão casa por caminho (prefixo até a barra) ou como glob sem cruzar `/`.
    pub match_as_path: bool,
    pub ignore_case: bool,
    pub contains: Vec<Oid>,
    pub no_contains: Vec<Oid>,
    pub merged: Vec<Oid>,
    pub no_merged: Vec<Oid>,
    pub points_at: Vec<Oid>,
    /// Inclui `HEAD` e as pseudo-refs do topo (`ORIG_HEAD`...).
    pub include_root: bool,
}

fn starts_with_case(hay: &[u8], needle: &[u8], icase: bool) -> bool {
    hay.len() >= needle.len() && if icase { hay[..needle.len()].eq_ignore_ascii_case(needle) } else { hay[..needle.len()] == *needle }
}

/// O `match_name_as_path`: prefixo até uma barra, ou glob em que `*` não cruza `/`.
fn match_path(patterns: &[String], name: &str, icase: bool) -> bool {
    let nb = name.as_bytes();
    patterns.iter().any(|p| {
        let pb = p.as_bytes();
        if !pb.is_empty() && starts_with_case(nb, pb, icase) && (nb.len() == pb.len() || nb[pb.len()] == b'/' || pb[pb.len() - 1] == b'/') {
            return true;
        }
        wildmatch::wildmatch(pb, nb, PATHNAME | if icase { CASEFOLD } else { 0 })
    })
}

/// Padrão de `branch` e `tag`: casa com o nome sem o prefixo `refs/tags/`, `refs/heads/`...
fn match_short(patterns: &[String], name: &str, icase: bool) -> bool {
    let short = name
        .strip_prefix("refs/tags/")
        .or_else(|| name.strip_prefix("refs/heads/"))
        .or_else(|| name.strip_prefix("refs/remotes/"))
        .or_else(|| name.strip_prefix("refs/"))
        .unwrap_or(name);
    patterns.iter().any(|p| wildmatch::wildmatch(p.as_bytes(), short.as_bytes(), if icase { CASEFOLD } else { 0 }))
}

fn root_ref_names(repo: &Repo) -> Vec<String> {
    let mut names = vec!["HEAD".to_string()];
    if let Ok(entries) = os::read_dir(&repo.git_dir) {
        let mut v: Vec<String> = entries
            .into_iter()
            .map(|e| os::lossy(&e.name))
            .filter(|n| n != "HEAD" && crate::refs::is_pseudoref_syntax(n) && n.ends_with("_HEAD"))
            .collect();
        v.sort();
        names.extend(v);
    }
    names
}

pub fn keep_by_graph(ctx: &mut Ctx, f: &Filter, oid: &Oid) -> R<bool> {
    let graph_filters = !(f.contains.is_empty() && f.no_contains.is_empty() && f.merged.is_empty() && f.no_merged.is_empty());
    if graph_filters {
        let Some(c) = ctx.repo.peel_to_commit(oid)? else { return Ok(false) };
        if !f.contains.is_empty() {
            let mut any = false;
            for w in &f.contains {
                if ctx.graph.is_ancestor(w, &c)? {
                    any = true;
                    break;
                }
            }
            if !any {
                return Ok(false);
            }
        }
        for w in &f.no_contains {
            if ctx.graph.is_ancestor(w, &c)? {
                return Ok(false);
            }
        }
        if !f.merged.is_empty() {
            let mut any = false;
            for m in &f.merged {
                if ctx.graph.is_ancestor(&c, m)? {
                    any = true;
                    break;
                }
            }
            if !any {
                return Ok(false);
            }
        }
        for m in &f.no_merged {
            if ctx.graph.is_ancestor(&c, m)? {
                return Ok(false);
            }
        }
    }
    if !f.points_at.is_empty() {
        let peeled = ctx.repo.peel(oid, None)?;
        if !f.points_at.iter().any(|p| p == oid || Some(*p) == peeled) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Refs que passam pelo filtro, em ordem de nome (a ordenação final fica com [`sort_rows`]).
pub fn collect(ctx: &mut Ctx, f: &Filter) -> R<Vec<Row>> {
    let repo = ctx.repo;
    let mut rows: Vec<Row> = Vec::new();
    if f.include_root {
        for name in root_ref_names(repo) {
            if let Some((_, Some(oid))) = repo.resolve_ref(&name)? {
                let symref = repo.symref_target(&name)?;
                rows.push(Row { name, oid, symref });
            }
        }
    }
    for (name, oid) in repo.list_refs("refs/")? {
        if !f.kinds.is_empty() && !f.kinds.iter().any(|k| name.starts_with(k.as_str())) {
            continue;
        }
        if !f.patterns.is_empty() {
            let ok = if f.match_as_path { match_path(&f.patterns, &name, f.ignore_case) } else { match_short(&f.patterns, &name, f.ignore_case) };
            if !ok {
                continue;
            }
        }
        if !f.exclude.is_empty() && match_path(&f.exclude, &name, f.ignore_case) {
            continue;
        }
        let symref = repo.symref_target(&name)?;
        rows.push(Row { name, oid, symref });
    }
    let mut kept = Vec::new();
    for r in rows {
        if keep_by_graph(ctx, f, &r.oid)? {
            kept.push(r);
        }
    }
    Ok(kept)
}

/// `--contains <commit>` e companhia: o commit, com o erro do git.
pub fn commit_arg(repo: &Repo, spec: &[u8]) -> R<Oid> {
    match repo.rev_parse_commit(spec)? {
        Some(c) => Ok(c),
        None => Err(opts::error_only(&format!("malformed object name {}", os::lossy(spec)))),
    }
}

/// `--contains`, `--no-contains`, `--merged` e `--no-merged` sem commit, como último argumento,
/// valem `HEAD`.
pub fn lastarg_default(args: &[Vec<u8>]) -> Vec<Vec<u8>> {
    let mut v = args.to_vec();
    if let Some(last) = args.last()
        && matches!(last.as_slice(), b"--contains" | b"--no-contains" | b"--merged" | b"--no-merged")
    {
        v.push(b"HEAD".to_vec());
    }
    v
}

/// Preenche o filtro com `--contains`, `--no-contains`, `--merged`, `--no-merged` e `--points-at`.
pub fn apply_filter_opts(repo: &Repo, p: &opts::Parsed, f: &mut Filter) -> R<()> {
    for v in p.values("contains") {
        f.contains.push(commit_arg(repo, &v)?);
    }
    for v in p.values("no-contains") {
        f.no_contains.push(commit_arg(repo, &v)?);
    }
    for v in p.values("merged") {
        f.merged.push(commit_arg(repo, &v)?);
    }
    for v in p.values("no-merged") {
        f.no_merged.push(commit_arg(repo, &v)?);
    }
    for v in p.values("points-at") {
        match repo.rev_parse(&v)? {
            Some(o) => f.points_at.push(o),
            None => return Err(Fail::Fatal(format!("malformed object name {}", os::lossy(&v)))),
        }
    }
    f.ignore_case = p.has("ignore-case");
    Ok(())
}

// ---- ordem ------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct SortKey {
    pub field: Field,
    pub desc: bool,
    pub version: bool,
}

pub fn parse_sort_key(spec: &str) -> R<SortKey> {
    let (desc, s) = match spec.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, spec),
    };
    let (version, s) = match s.strip_prefix("version:").or_else(|| s.strip_prefix("v:")) {
        Some(r) => (true, r),
        None => (false, s),
    };
    let (deref, s) = match s.strip_prefix('*') {
        Some(r) => (true, r),
        None => (false, s),
    };
    let (name, arg) = match s.split_once(':') {
        Some((n, a)) => (n.to_string(), a.to_string()),
        None => (s.to_string(), String::new()),
    };
    if !VALUE_ATOMS.contains(&name.as_str()) {
        return Err(Fail::Fatal(format!("unknown field name: {spec}")));
    }
    Ok(SortKey { field: Field { name, arg, deref }, desc, version })
}

enum Sv {
    Num(i64),
    Str(Vec<u8>),
}

fn sort_value(ctx: &mut Ctx, row: &Row, k: &SortKey) -> R<Sv> {
    let name = k.field.name.as_str();
    if !k.version && (name.ends_with("date") || name == "objectsize" || name == "numparent") {
        let mut f = k.field.clone();
        if name.ends_with("date") {
            f.arg = "unix".to_string();
        }
        let v = ctx.field(row, &f)?;
        return Ok(Sv::Num(String::from_utf8_lossy(&v).trim().parse::<i64>().unwrap_or(0)));
    }
    Ok(Sv::Str(ctx.field(row, &k.field)?))
}

/// Comparação de versões: trechos de dígitos como número, o resto byte a byte.
fn version_cmp(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i].is_ascii_digit() && b[j].is_ascii_digit() {
            let si = i;
            while i < a.len() && a[i].is_ascii_digit() {
                i += 1;
            }
            let sj = j;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            let na = a[si..i].iter().position(|c| *c != b'0').map(|p| &a[si + p..i]).unwrap_or(&[]);
            let nb = b[sj..j].iter().position(|c| *c != b'0').map(|p| &b[sj + p..j]).unwrap_or(&[]);
            let o = na.len().cmp(&nb.len()).then_with(|| na.cmp(nb));
            if o != Ordering::Equal {
                return o;
            }
        } else {
            if a[i] != b[j] {
                return a[i].cmp(&b[j]);
            }
            i += 1;
            j += 1;
        }
    }
    (a.len() - i).cmp(&(b.len() - j))
}

fn cmp_sv(a: &Sv, b: &Sv, version: bool, icase: bool) -> std::cmp::Ordering {
    match (a, b) {
        (Sv::Num(x), Sv::Num(y)) => x.cmp(y),
        (Sv::Str(x), Sv::Str(y)) => {
            if version {
                version_cmp(x, y)
            } else if icase {
                x.to_ascii_lowercase().cmp(&y.to_ascii_lowercase())
            } else {
                x.cmp(y)
            }
        }
        _ => std::cmp::Ordering::Equal,
    }
}

/// Ordena: a última chave dada é a primária; empate cai no nome da ref.
pub fn sort_rows(ctx: &mut Ctx, rows: Vec<Row>, keys: &[SortKey], icase: bool) -> R<Vec<Row>> {
    let default_keys;
    let keys: &[SortKey] = if keys.is_empty() {
        default_keys = [SortKey { field: Field { name: "refname".to_string(), arg: String::new(), deref: false }, desc: false, version: false }];
        &default_keys
    } else {
        keys
    };
    let mut keyed: Vec<(Vec<Sv>, Row)> = Vec::new();
    for r in rows {
        let mut vals = Vec::new();
        for k in keys.iter().rev() {
            vals.push(sort_value(ctx, &r, k)?);
        }
        keyed.push((vals, r));
    }
    keyed.sort_by(|a, b| {
        for (i, k) in keys.iter().rev().enumerate() {
            let mut o = cmp_sv(&a.0[i], &b.0[i], k.version, icase);
            if k.desc {
                o = o.reverse();
            }
            if o != std::cmp::Ordering::Equal {
                return o;
            }
        }
        a.1.name.cmp(&b.1.name)
    });
    Ok(keyed.into_iter().map(|(_, r)| r).collect())
}

