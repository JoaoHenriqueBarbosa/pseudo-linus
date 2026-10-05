//! `git log`, `show`, `whatchanged` e `rev-list` (o `shortlog` reaproveita a caminhada): a caminhada pelos commits (data de
//! commit decrescente, `A..B`, `^A`, `--all`, caminhos com a simplificação padrão do histórico),
//! os filtros (`--author`, `--grep`, `--since`...), os formatos (`oneline`, `short`, `medium`,
//! `full`, `fuller`, `raw` e `--format`) e o diff de cada commit.

use std::collections::{HashMap, HashSet};

use regex::bytes::Regex;

use super::Git;
use super::diff_cmd::{self, DIFF_SPECS};
use crate::date::{self, DateMode};
use crate::diff::{self, DiffOpts, Pair};
use crate::error::{Fail, R};
use crate::graph::{DateQueue, Graph};
use crate::hash::{Kind, Oid};
use crate::object::{self, Commit, Ident};
use crate::opts::{self, Parsed, Spec};
use crate::os;
use crate::pathspec::Pathspec;
use crate::re::{self, Flavor};
use crate::refs::Head;
use crate::repo::Repo;
use crate::rev;

const LOG_SPECS: &[Spec] = &[
    opts::value(Some(b'n'), "max-count", "max-count"),
    opts::value(None, "skip", "skip"),
    opts::flag(None, "oneline", "oneline"),
    opts::optional(None, "pretty", "pretty"),
    opts::optional(None, "format", "format"),
    opts::flag(None, "abbrev-commit", "abbrev-commit"),
    opts::optional(None, "decorate", "decorate"),
    opts::value(None, "date", "date"),
    opts::flag(None, "relative-date", "relative-date"),
    opts::value(None, "author", "author"),
    opts::value(None, "committer", "committer"),
    opts::value(None, "grep", "grep"),
    opts::flag(Some(b'i'), "regexp-ignore-case", "icase"),
    opts::flag(Some(b'E'), "extended-regexp", "extended"),
    opts::flag(Some(b'F'), "fixed-strings", "fixed"),
    opts::flag(None, "all-match", "all-match"),
    opts::flag(None, "invert-grep", "invert-grep"),
    opts::value(None, "since", "since"),
    opts::value(None, "after", "since"),
    opts::value(None, "until", "until"),
    opts::value(None, "before", "until"),
    opts::flag(None, "merges", "merges"),
    opts::flag(None, "no-merges", "no-merges"),
    opts::flag(None, "first-parent", "first-parent"),
    opts::flag(None, "reverse", "reverse"),
    opts::flag(None, "all", "all"),
    opts::flag(None, "branches", "branches"),
    opts::flag(None, "tags", "tags"),
    opts::flag(None, "remotes", "remotes"),
    opts::flag(Some(b'g'), "walk-reflogs", "reflog"),
    opts::flag(None, "parents", "parents"),
    opts::flag(None, "count", "count"),
    opts::flag(None, "graph", "graph"),
];

pub(crate) fn all_specs() -> Vec<Spec> {
    let mut v = DIFF_SPECS.to_vec();
    v.extend_from_slice(LOG_SPECS);
    v
}

fn number(usage: &str, name: &str, v: &[u8]) -> R<usize> {
    std::str::from_utf8(v)
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .ok_or_else(|| opts::usage_error(usage, &format!("{name} expects a numerical value")))
}

// ---- formatos ---------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
enum Fmt {
    Oneline,
    Short,
    Medium,
    Full,
    Fuller,
    Raw,
    /// Texto com `%...`; o bool é "terminador" (`tformat`) em vez de "separador" (`format:`).
    Custom(Vec<u8>, bool),
}

impl Fmt {
    fn terminator(&self) -> bool {
        matches!(self, Fmt::Oneline | Fmt::Custom(_, true))
    }
}

fn parse_pretty(s: &[u8]) -> R<Fmt> {
    Ok(match s {
        b"oneline" => Fmt::Oneline,
        b"short" => Fmt::Short,
        b"medium" | b"" => Fmt::Medium,
        b"full" => Fmt::Full,
        b"fuller" => Fmt::Fuller,
        b"raw" => Fmt::Raw,
        _ => {
            if let Some(f) = s.strip_prefix(b"format:") {
                Fmt::Custom(f.to_vec(), false)
            } else if let Some(f) = s.strip_prefix(b"tformat:") {
                Fmt::Custom(f.to_vec(), true)
            } else if s.contains(&b'%') {
                Fmt::Custom(s.to_vec(), true)
            } else {
                return Err(Fail::Fatal(format!("invalid --pretty format: {}", os::lossy(s))));
            }
        }
    })
}

/// Entrada de reflog mostrada por `log -g`.
struct Rl {
    selector: String,
    message: Vec<u8>,
    name: Vec<u8>,
    email: Vec<u8>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pad {
    Left,
    Right,
    Mid,
}

struct Ctx<'a> {
    repo: &'a Repo,
    fmt: Fmt,
    abbrev_commit: bool,
    abbrev: Option<usize>,
    date: DateMode,
    /// `Some(completo?)` quando `--decorate` está ligado.
    decorate: Option<bool>,
    decos: Option<HashMap<Oid, Vec<String>>>,
    diff: Option<DiffOpts>,
    ps: Pathspec,
    nul: bool,
    shown_one: bool,
    parents: bool,
    /// O desenho de `--graph`, quando ligado.
    graph: Option<GraphDraw>,
    /// O último registro mostrado não terminava em quebra de linha (`missing_newline` do git).
    missing_newline: bool,
}

impl Ctx<'_> {
    fn abbr(&self, id: &Oid) -> String {
        match self.abbrev {
            Some(n) => self.repo.abbrev(id, n),
            None => self.repo.abbrev_default(id),
        }
    }

    /// O id como a linha de cabeçalho o mostra (abreviado com `--abbrev-commit`).
    fn shown_id(&self, id: &Oid) -> String {
        if self.abbrev_commit { self.abbr(id) } else { id.hex() }
    }

    fn decorations(&mut self, id: &Oid, full: bool) -> R<Vec<String>> {
        if self.decos.is_none() {
            self.decos = Some(build_decorations(self.repo, full)?);
        }
        Ok(self.decos.as_ref().and_then(|m| m.get(id)).cloned().unwrap_or_default())
    }

    fn header_decoration(&mut self, id: &Oid) -> R<String> {
        match self.decorate {
            Some(full) => {
                let d = self.decorations(id, full)?;
                Ok(if d.is_empty() { String::new() } else { format!(" ({})", d.join(", ")) })
            }
            None => Ok(String::new()),
        }
    }

    fn print_commit(&mut self, id: &Oid, rl: Option<&Rl>) -> R<()> {
        if let Some(g) = self.graph.take() {
            return self.print_commit_graph(id, rl, g);
        }
        let term = if self.nul { 0 } else { b'\n' };
        let c = self.repo.read_commit(id)?;
        let mut out: Vec<u8> = Vec::new();
        if self.shown_one && !self.fmt.terminator() {
            out.push(term);
        }
        let record = self.render(id, &c, rl)?;
        out.extend_from_slice(&record);
        if self.fmt.terminator() {
            out.push(term);
        }
        let pairs = match &self.diff {
            Some(o) => commit_pairs(self.repo, &c, o, &self.ps)?,
            None => Vec::new(),
        };
        if !pairs.is_empty() && self.fmt != Fmt::Oneline {
            out.push(b'\n');
        }
        os::out(&out);
        if let Some(o) = &self.diff
            && !pairs.is_empty()
        {
            diff::emit(self.repo, &pairs, o)?;
        }
        self.shown_one = true;
        Ok(())
    }

    /// `print_commit` com `--graph`, na ordem do `show_log` do git: separador (com linha de
    /// preenchimento), linhas do grafo até a do commit, o registro com o prefixo do grafo antes de
    /// cada linha seguinte, o resto do grafo e o terminador.
    fn print_commit_graph(&mut self, id: &Oid, rl: Option<&Rl>, mut g: GraphDraw) -> R<()> {
        let term = if self.nul { 0 } else { b'\n' };
        let c = self.repo.read_commit(id)?;
        let mut out: Vec<u8> = Vec::new();
        if self.shown_one && !self.fmt.terminator() {
            if term == b'\n' && !self.missing_newline {
                out.extend_from_slice(&g.padding_line());
            }
            out.push(term);
        }
        // graph_show_commit
        if g.is_finished() {
            out.extend_from_slice(&g.padding_line());
        } else {
            loop {
                let (line, commit_line) = g.next_line();
                out.extend_from_slice(&line);
                if commit_line {
                    break;
                }
                out.push(b'\n');
                if g.is_finished() {
                    break;
                }
            }
        }
        let record = match self.render(id, &c, rl) {
            Ok(r) => r,
            Err(e) => {
                self.graph = Some(g);
                return Err(e);
            }
        };
        self.missing_newline = record.last() != Some(&b'\n');
        // graph_show_strbuf: o prefixo do grafo antes de cada linha menos a primeira.
        let mut p = 0;
        while p < record.len() {
            match record[p..].iter().position(|b| *b == b'\n') {
                Some(k) => {
                    let next = p + k + 1;
                    out.extend_from_slice(&record[p..next]);
                    if next < record.len() {
                        out.extend_from_slice(&g.next_line().0);
                    }
                    p = next;
                }
                None => {
                    out.extend_from_slice(&record[p..]);
                    break;
                }
            }
        }
        let newline_terminated = !self.missing_newline;
        if !g.is_finished() {
            if !newline_terminated {
                out.push(b'\n');
            }
            // graph_show_remainder
            loop {
                out.extend_from_slice(&g.next_line().0);
                if g.is_finished() {
                    break;
                }
                out.push(b'\n');
            }
            if newline_terminated {
                out.push(b'\n');
            }
        }
        if self.fmt.terminator() {
            if !self.missing_newline {
                out.extend_from_slice(&g.padding_line());
            }
            out.push(term);
        }
        self.graph = Some(g);
        let pairs = match &self.diff {
            Some(o) => commit_pairs(self.repo, &c, o, &self.ps)?,
            None => Vec::new(),
        };
        if !pairs.is_empty() && self.fmt != Fmt::Oneline {
            out.push(b'\n');
        }
        os::out(&out);
        if let Some(o) = &self.diff
            && !pairs.is_empty()
        {
            diff::emit(self.repo, &pairs, o)?;
        }
        self.shown_one = true;
        Ok(())
    }

    fn render(&mut self, id: &Oid, c: &Commit, rl: Option<&Rl>) -> R<Vec<u8>> {
        let fmt = self.fmt.clone();
        let mut out: Vec<u8> = Vec::new();
        match &fmt {
            Fmt::Oneline => {
                out.extend_from_slice(self.shown_id(id).as_bytes());
                out.extend_from_slice(self.header_decoration(id)?.as_bytes());
                match rl {
                    Some(r) => {
                        out.push(b' ');
                        out.extend_from_slice(r.selector.as_bytes());
                        out.extend_from_slice(b": ");
                        out.extend_from_slice(&r.message);
                    }
                    None => {
                        out.push(b' ');
                        out.extend_from_slice(&c.subject());
                    }
                }
            }
            Fmt::Custom(text, _) => out = self.expand(text, id, c, rl)?,
            _ => self.render_header(&mut out, &fmt, id, c, rl)?,
        }
        Ok(out)
    }

    fn render_header(&mut self, out: &mut Vec<u8>, fmt: &Fmt, id: &Oid, c: &Commit, rl: Option<&Rl>) -> R<()> {
        let deco = self.header_decoration(id)?;
        out.extend_from_slice(format!("commit {}{deco}\n", self.shown_id(id)).as_bytes());
        if *fmt == Fmt::Raw {
            out.extend_from_slice(format!("tree {}\n", c.tree).as_bytes());
            for p in &c.parents {
                out.extend_from_slice(format!("parent {p}\n").as_bytes());
            }
            out.extend_from_slice(b"author ");
            out.extend_from_slice(&c.author);
            out.extend_from_slice(b"\ncommitter ");
            out.extend_from_slice(&c.committer);
            out.extend_from_slice(b"\n\n");
            out.extend_from_slice(&indent_message(&c.message, false));
            return Ok(());
        }
        if let Some(r) = rl {
            let who = [r.name.clone(), b" <".to_vec(), r.email.clone(), b">".to_vec()].concat();
            out.extend_from_slice(format!("Reflog: {} ({})\nReflog message: ", r.selector, os::lossy(&who)).as_bytes());
            out.extend_from_slice(&r.message);
            out.push(b'\n');
        }
        if c.parents.len() > 1 {
            let ps: Vec<String> = c.parents.iter().map(|p| self.abbr(p)).collect();
            out.extend_from_slice(format!("Merge: {}\n", ps.join(" ")).as_bytes());
        }
        let author = c.author_ident();
        let committer = c.committer_ident();
        let when = |i: &Ident, mode: &DateMode| i.date.map(|t| date::show_date(t, i.tz, mode)).unwrap_or_default();
        match fmt {
            Fmt::Short => {
                out.extend_from_slice(format!("Author: {}\n", os::lossy(&author.name_email())).as_bytes());
            }
            Fmt::Medium => {
                out.extend_from_slice(format!("Author: {}\n", os::lossy(&author.name_email())).as_bytes());
                out.extend_from_slice(format!("Date:   {}\n", when(&author, &self.date)).as_bytes());
            }
            Fmt::Full => {
                out.extend_from_slice(format!("Author: {}\n", os::lossy(&author.name_email())).as_bytes());
                out.extend_from_slice(format!("Commit: {}\n", os::lossy(&committer.name_email())).as_bytes());
            }
            _ => {
                out.extend_from_slice(format!("Author:     {}\n", os::lossy(&author.name_email())).as_bytes());
                out.extend_from_slice(format!("AuthorDate: {}\n", when(&author, &self.date)).as_bytes());
                out.extend_from_slice(format!("Commit:     {}\n", os::lossy(&committer.name_email())).as_bytes());
                out.extend_from_slice(format!("CommitDate: {}\n", when(&committer, &self.date)).as_bytes());
            }
        }
        out.push(b'\n');
        out.extend_from_slice(&indent_message(&c.message, *fmt == Fmt::Short));
        Ok(())
    }

    /// Expande um `--format`.
    fn expand(&mut self, fmt: &[u8], id: &Oid, c: &Commit, rl: Option<&Rl>) -> R<Vec<u8>> {
        let mut out: Vec<u8> = Vec::new();
        let mut pad: Option<(Pad, usize, u8)> = None;
        let mut i = 0;
        while i < fmt.len() {
            if fmt[i] != b'%' {
                out.push(fmt[i]);
                i += 1;
                continue;
            }
            let Some(&c1) = fmt.get(i + 1) else {
                out.push(b'%');
                i += 1;
                continue;
            };
            // Largura: `%<(N)`, `%>(N)`, `%><(N)`, com `,trunc`, `,ltrunc` ou `,mtrunc`.
            if matches!(c1, b'<' | b'>') {
                let (kind, open) = if c1 == b'<' {
                    (Pad::Left, i + 2)
                } else if fmt.get(i + 2) == Some(&b'<') {
                    (Pad::Mid, i + 3)
                } else {
                    (Pad::Right, i + 2)
                };
                if fmt.get(open) == Some(&b'(')
                    && let Some(close) = fmt[open..].iter().position(|b| *b == b')')
                {
                    let spec = os::lossy(&fmt[open + 1..open + close]);
                    let mut parts = spec.split(',');
                    if let Some(w) = parts.next().and_then(|w| w.parse::<usize>().ok()) {
                        let trunc = match parts.next() {
                            Some("trunc") => 1,
                            Some("ltrunc") => 2,
                            Some("mtrunc") => 3,
                            _ => 0,
                        };
                        pad = Some((kind, w, trunc));
                        i = open + close + 1;
                        continue;
                    }
                }
                out.push(b'%');
                i += 1;
                continue;
            }
            let mut used = 2;
            let value: Option<Vec<u8>> = match c1 {
                b'%' => Some(b"%".to_vec()),
                b'n' => Some(b"\n".to_vec()),
                b'x' => match (fmt.get(i + 2).and_then(|b| crate::hash::hex_val(*b)), fmt.get(i + 3).and_then(|b| crate::hash::hex_val(*b))) {
                    (Some(a), Some(b)) => {
                        used = 4;
                        Some(vec![(a << 4) | b])
                    }
                    _ => None,
                },
                b'H' => Some(id.hex().into_bytes()),
                b'h' => Some(self.abbr(id).into_bytes()),
                b'T' => Some(c.tree.hex().into_bytes()),
                b't' => Some(self.abbr(&c.tree).into_bytes()),
                b'P' => Some(c.parents.iter().map(|p| p.hex()).collect::<Vec<_>>().join(" ").into_bytes()),
                b'p' => Some(c.parents.iter().map(|p| self.abbr(p)).collect::<Vec<_>>().join(" ").into_bytes()),
                b's' => Some(c.subject()),
                b'f' => Some(sanitized_subject(&c.subject())),
                b'b' => Some(object::body_of(&c.message)),
                b'B' => Some(c.message.clone()),
                b'e' => Some(Vec::new()),
                b'd' | b'D' => {
                    let full = self.decorate.unwrap_or(false);
                    let d = self.decorations(id, full)?;
                    Some(if d.is_empty() {
                        Vec::new()
                    } else if c1 == b'd' {
                        format!(" ({})", d.join(", ")).into_bytes()
                    } else {
                        d.join(", ").into_bytes()
                    })
                }
                b'a' | b'c' => match fmt.get(i + 2) {
                    Some(&f) if b"nNeElLdDrtiIsh".contains(&f) => {
                        used = 3;
                        let who = if c1 == b'a' { c.author_ident() } else { c.committer_ident() };
                        Some(self.person_field(&who, f))
                    }
                    _ => None,
                },
                b'g' => match fmt.get(i + 2) {
                    Some(&f) if b"dDsnNeE".contains(&f) => {
                        used = 3;
                        Some(match (rl, f) {
                            (Some(r), b'd') => r.selector.clone().into_bytes(),
                            (Some(r), b'D') => r.selector.clone().into_bytes(),
                            (Some(r), b's') => r.message.clone(),
                            (Some(r), b'n') | (Some(r), b'N') => r.name.clone(),
                            (Some(r), _) => r.email.clone(),
                            (None, _) => Vec::new(),
                        })
                    }
                    _ => None,
                },
                // Cores ficam de fora: a saída não é um terminal.
                b'C' => {
                    if fmt.get(i + 2) == Some(&b'(')
                        && let Some(close) = fmt[i + 2..].iter().position(|b| *b == b')')
                    {
                        used = 2 + close + 1;
                        Some(Vec::new())
                    } else {
                        let n = fmt[i + 2..].iter().take_while(|b| b.is_ascii_alphabetic()).count();
                        if n > 0 {
                            used = 2 + n;
                            Some(Vec::new())
                        } else {
                            None
                        }
                    }
                }
                b'w' => {
                    if fmt.get(i + 2) == Some(&b'(')
                        && let Some(close) = fmt[i + 2..].iter().position(|b| *b == b')')
                    {
                        used = 2 + close + 1;
                        Some(Vec::new())
                    } else {
                        None
                    }
                }
                _ => None,
            };
            match value {
                Some(v) => {
                    let v = match pad.take() {
                        Some((kind, w, trunc)) => pad_text(&v, kind, w, trunc),
                        None => v,
                    };
                    out.extend_from_slice(&v);
                    i += used;
                }
                None => {
                    out.push(b'%');
                    i += 1;
                }
            }
        }
        Ok(out)
    }

    fn person_field(&self, who: &Ident, field: u8) -> Vec<u8> {
        let when = |mode: &DateMode| who.date.map(|t| date::show_date(t, who.tz, mode)).unwrap_or_default().into_bytes();
        match field {
            b'n' | b'N' => who.name.clone(),
            b'e' | b'E' => who.email.clone(),
            b'l' | b'L' => who.email.split(|c| *c == b'@').next().unwrap_or(&[]).to_vec(),
            b'd' => when(&self.date),
            b'D' => when(&DateMode::Rfc),
            b'r' => who.date.map(|t| date::relative(t, os::now())).unwrap_or_default().into_bytes(),
            b't' => who.date.map(|t| t.to_string()).unwrap_or_default().into_bytes(),
            b'i' => when(&DateMode::Iso),
            b'I' => when(&DateMode::IsoStrict),
            b's' => when(&DateMode::Short),
            _ => when(&DateMode::Human),
        }
    }
}

/// `%f`: o assunto como nome de arquivo (sequências fora de `[A-Za-z0-9._]` viram um `-`).
fn sanitized_subject(subject: &[u8]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut pending = false;
    for &c in subject {
        if c.is_ascii_alphanumeric() || c == b'.' || c == b'_' {
            if pending && !out.is_empty() {
                out.push(b'-');
            }
            pending = false;
            out.push(c);
        } else {
            pending = true;
        }
    }
    while out.last() == Some(&b'.') {
        out.pop();
    }
    out
}

/// Aplica `%<(N)` e companhia: largura em caracteres.
fn pad_text(v: &[u8], kind: Pad, width: usize, trunc: u8) -> Vec<u8> {
    let text = String::from_utf8_lossy(v).into_owned();
    let chars: Vec<char> = text.chars().collect();
    let mut shown: Vec<char> = chars.clone();
    if chars.len() > width && trunc != 0 && width >= 2 {
        shown = match trunc {
            1 => chars[..width - 2].iter().copied().chain("..".chars()).collect(),
            2 => "..".chars().chain(chars[chars.len() - (width - 2)..].iter().copied()).collect(),
            _ => {
                let left = (width - 2) / 2;
                let right = width - 2 - left;
                chars[..left].iter().copied().chain("..".chars()).chain(chars[chars.len() - right..].iter().copied()).collect()
            }
        };
    }
    let fill = width.saturating_sub(shown.len());
    let body: String = shown.into_iter().collect();
    let padded = match kind {
        Pad::Left => format!("{body}{}", " ".repeat(fill)),
        Pad::Right => format!("{}{body}", " ".repeat(fill)),
        Pad::Mid => format!("{}{body}{}", " ".repeat(fill / 2), " ".repeat(fill - fill / 2)),
    };
    padded.into_bytes()
}

/// A mensagem recuada em 4 espaços: assunto, linha em branco recuada e o corpo. `subject_only`
/// (formato `short`) corta depois do assunto.
fn indent_message(msg: &[u8], subject_only: bool) -> Vec<u8> {
    let (subject, end) = object::subject_with(msg, b" ");
    let mut out = b"    ".to_vec();
    out.extend_from_slice(&subject);
    out.push(b'\n');
    if subject_only {
        return out;
    }
    let body = &msg[object::skip_blank_lines(msg, end)..];
    let body = object::rtrim(body);
    if body.is_empty() {
        return out;
    }
    out.extend_from_slice(b"    \n");
    for line in body.split(|c| *c == b'\n') {
        out.extend_from_slice(b"    ");
        out.extend_from_slice(object::rtrim(line));
        out.push(b'\n');
    }
    out
}

// ---- decoração --------------------------------------------------------------------------------

/// Nomes de ref por commit, na ordem do git: as refs em ordem alfabética entram pela frente (então
/// saem em ordem inversa), e o HEAD vem antes de todas, junto do ramo a que aponta.
fn build_decorations(repo: &Repo, full: bool) -> R<HashMap<Oid, Vec<String>>> {
    let mut map: HashMap<Oid, Vec<String>> = HashMap::new();
    for (name, oid) in repo.list_refs("refs/")? {
        let Some(target) = repo.peel_to_commit(&oid)? else { continue };
        let label = if full {
            if name.starts_with("refs/tags/") { format!("tag: {name}") } else { name.clone() }
        } else if let Some(t) = name.strip_prefix("refs/tags/") {
            format!("tag: {t}")
        } else if let Some(b) = name.strip_prefix("refs/heads/") {
            b.to_string()
        } else if let Some(r) = name.strip_prefix("refs/remotes/") {
            r.to_string()
        } else {
            name.clone()
        };
        map.entry(target).or_default().insert(0, label);
    }
    match repo.head()? {
        Head::Branch(name, Some(oid)) => {
            let shown = if full { name.clone() } else { name.strip_prefix("refs/heads/").unwrap_or(&name).to_string() };
            let list = map.entry(oid).or_default();
            list.retain(|l| *l != shown);
            list.insert(0, format!("HEAD -> {shown}"));
        }
        Head::Detached(oid) => map.entry(oid).or_default().insert(0, "HEAD".to_string()),
        Head::Branch(_, None) => {}
    }
    Ok(map)
}

// ---- filtros ----------------------------------------------------------------------------------

#[derive(Default)]
pub(crate) struct Filters {
    authors: Vec<Regex>,
    committers: Vec<Regex>,
    greps: Vec<Regex>,
    all_match: bool,
    invert: bool,
    since: Option<i64>,
    until: Option<i64>,
    /// `Some(true)` só merges; `Some(false)` sem merges.
    merges: Option<bool>,
}

impl Filters {
    pub(crate) fn from(p: &Parsed) -> R<Filters> {
        let flavor = if p.has("fixed") {
            Flavor::Fixed
        } else if p.has("extended") {
            Flavor::Extended
        } else {
            Flavor::Basic
        };
        let icase = p.has("icase");
        let compile = |key: &str| -> R<Vec<Regex>> {
            p.values(key)
                .iter()
                .map(|pat| re::compile(pat, flavor, icase).map_err(|e| Fail::Fatal(format!("command-line: Invalid regular expression '{}': {e}", os::lossy(pat)))))
                .collect()
        };
        let when_arg = |key: &str| -> R<Option<i64>> {
            match p.value_str(key) {
                None => Ok(None),
                Some(v) => date::approxidate(&v).map(Some).ok_or_else(|| Fail::Fatal(format!("invalid date format: {v}"))),
            }
        };
        let merges = if p.has("merges") {
            Some(true)
        } else if p.has("no-merges") {
            Some(false)
        } else {
            None
        };
        Ok(Filters {
            authors: compile("author")?,
            committers: compile("committer")?,
            greps: compile("grep")?,
            all_match: p.has("all-match"),
            invert: p.has("invert-grep"),
            since: when_arg("since")?,
            until: when_arg("until")?,
            merges,
        })
    }

    pub(crate) fn accepts(&self, c: &Commit) -> bool {
        if let Some(only) = self.merges
            && (c.parents.len() > 1) != only
        {
            return false;
        }
        let when = c.commit_date();
        if self.since.is_some_and(|s| when < s) || self.until.is_some_and(|u| when > u) {
            return false;
        }
        if !self.authors.is_empty() {
            let who = c.author_ident().name_email();
            if !self.authors.iter().any(|r| r.is_match(&who)) {
                return false;
            }
        }
        if !self.committers.is_empty() {
            let who = c.committer_ident().name_email();
            if !self.committers.iter().any(|r| r.is_match(&who)) {
                return false;
            }
        }
        if !self.greps.is_empty() {
            let hit = if self.all_match { self.greps.iter().all(|r| r.is_match(&c.message)) } else { self.greps.iter().any(|r| r.is_match(&c.message)) };
            if hit == self.invert {
                return false;
            }
        }
        true
    }
}

// ---- revisões e caminhada ---------------------------------------------------------------------

#[derive(Default)]
pub(crate) struct Revs {
    pub(crate) include: Vec<Oid>,
    pub(crate) exclude: Vec<Oid>,
    pub(crate) paths: Vec<Vec<u8>>,
    names: Vec<Vec<u8>>,
}

fn commit_of(repo: &Repo, spec: &[u8]) -> R<Option<Oid>> {
    repo.rev_parse_commit(spec)
}

/// Tenta tratar `arg` como revisão (`rev`, `^rev`, `A..B`, `A...B`). `false` se não é.
fn take_rev(repo: &Repo, arg: &[u8], r: &mut Revs) -> R<bool> {
    let find = |sep: &[u8]| arg.windows(sep.len()).position(|w| w == sep);
    if let Some(rest) = arg.strip_prefix(b"^") {
        return match commit_of(repo, rest)? {
            Some(c) => {
                r.exclude.push(c);
                Ok(true)
            }
            None => Err(Fail::Fatal(format!("bad revision '{}'", os::lossy(arg)))),
        };
    }
    let head = |s: &[u8]| -> Vec<u8> { if s.is_empty() { b"HEAD".to_vec() } else { s.to_vec() } };
    if let Some(i) = find(b"...") {
        let (Some(a), Some(b)) = (commit_of(repo, &head(&arg[..i]))?, commit_of(repo, &head(&arg[i + 3..]))?) else { return Ok(false) };
        let bases = Graph::new(repo).merge_bases(&a, &[b])?;
        r.include.push(a);
        r.include.push(b);
        r.exclude.extend(bases);
        return Ok(true);
    }
    if let Some(i) = find(b"..") {
        let (Some(a), Some(b)) = (commit_of(repo, &head(&arg[..i]))?, commit_of(repo, &head(&arg[i + 2..]))?) else { return Ok(false) };
        r.exclude.push(a);
        r.include.push(b);
        return Ok(true);
    }
    match commit_of(repo, arg)? {
        Some(c) => {
            r.include.push(c);
            r.names.push(arg.to_vec());
            Ok(true)
        }
        None => Ok(false),
    }
}

fn add_refs(repo: &Repo, prefix: &str, r: &mut Revs) -> R<()> {
    for (_, oid) in repo.list_refs(prefix)? {
        if let Some(c) = repo.peel_to_commit(&oid)? {
            r.include.push(c);
        }
    }
    Ok(())
}

pub(crate) fn resolve_revs(repo: &Repo, p: &Parsed) -> R<Revs> {
    let (before, after) = p.split_dashdash();
    let dashed = p.dashdash.is_some();
    let mut r = Revs::default();
    let mut in_paths = false;
    for a in &before {
        if !in_paths {
            if take_rev(repo, a, &mut r)? {
                continue;
            }
            if dashed {
                return Err(Fail::Fatal(format!("bad revision '{}'", os::lossy(a))));
            }
            in_paths = true;
        }
        let full = if a.starts_with(b"/") { a.clone() } else { os::join(&repo.prefix, a) };
        if !dashed && os::lstat(&full).is_err() {
            return Err(rev::bad_revision(a));
        }
        r.paths.push(a.clone());
    }
    r.paths.extend(after);
    if p.has("all") {
        add_refs(repo, "refs/", &mut r)?;
        if let Some(h) = repo.head_oid()? {
            r.include.push(h);
        }
    }
    if p.has("branches") {
        add_refs(repo, "refs/heads/", &mut r)?;
    }
    if p.has("tags") {
        add_refs(repo, "refs/tags/", &mut r)?;
    }
    if p.has("remotes") {
        add_refs(repo, "refs/remotes/", &mut r)?;
    }
    let any_refs_opt = p.has("all") || p.has("branches") || p.has("tags") || p.has("remotes");
    if r.include.is_empty() && !any_refs_opt && r.exclude.is_empty() {
        match repo.head()? {
            Head::Branch(name, None) => {
                let short = name.strip_prefix("refs/heads/").unwrap_or(&name).to_string();
                return Err(Fail::Fatal(format!("your current branch '{short}' does not have any commits yet")));
            }
            h => {
                if let Some(o) = h.oid() {
                    r.include.push(o);
                }
            }
        }
    }
    Ok(r)
}

/// Caminhada por data de commit (mais novo primeiro, empate na ordem de entrada). Com caminhos, a
/// simplificação padrão: commit igual a um dos pais nesses caminhos não aparece e só esse pai é
/// seguido.
pub(crate) struct Walker<'a> {
    repo: &'a Repo,
    g: Graph<'a>,
    queue: DateQueue,
    seen: HashSet<Oid>,
    excluded: HashSet<Oid>,
    first_parent: bool,
    ps: Option<&'a Pathspec>,
}

impl<'a> Walker<'a> {
    pub(crate) fn new(repo: &'a Repo, include: &[Oid], exclude: &[Oid], first_parent: bool, ps: Option<&'a Pathspec>) -> R<Walker<'a>> {
        let mut g = Graph::new(repo);
        let excluded = if exclude.is_empty() { HashSet::new() } else { g.reachable(exclude)? };
        let mut w = Walker { repo, g, queue: DateQueue::new(), seen: HashSet::new(), excluded, first_parent, ps };
        for id in include {
            if !w.excluded.contains(id) && w.seen.insert(*id) {
                let d = w.g.date(id)?;
                w.queue.push(d, *id);
            }
        }
        Ok(w)
    }

    /// Próximo commit e se ele deve aparecer.
    pub(crate) fn next(&mut self) -> R<Option<(Oid, bool)>> {
        let Some((_, id)) = self.queue.pop() else { return Ok(None) };
        let parents = self.g.parents(&id)?;
        let candidates: Vec<Oid> = if self.first_parent { parents.iter().take(1).copied().collect() } else { parents.clone() };
        let mut follow = candidates.clone();
        let mut show = true;
        if let Some(ps) = self.ps {
            let tree = self.repo.read_commit(&id)?.tree;
            if parents.is_empty() {
                show = !diff::diff_trees(self.repo, None, Some(&tree), ps)?.is_empty();
            } else {
                for p in &candidates {
                    let pt = self.repo.read_commit(p)?.tree;
                    if diff::diff_trees(self.repo, Some(&pt), Some(&tree), ps)?.is_empty() {
                        show = false;
                        follow = vec![*p];
                        break;
                    }
                }
            }
        }
        for p in follow {
            if !self.excluded.contains(&p) && self.seen.insert(p) {
                let d = self.g.date(&p)?;
                self.queue.push(d, p);
            }
        }
        Ok(Some((id, show)))
    }
}

/// O diff de um commit contra o primeiro pai (ou a tree vazia no commit raiz); merges não mostram.
fn commit_pairs(repo: &Repo, c: &Commit, o: &DiffOpts, ps: &Pathspec) -> R<Vec<Pair>> {
    let pairs = match c.parents.len() {
        0 => diff::diff_trees(repo, None, Some(&c.tree), ps)?,
        1 => {
            let pt = repo.tree_of(&c.parents[0])?;
            diff::diff_trees(repo, Some(&pt), Some(&c.tree), ps)?
        }
        _ => return Ok(Vec::new()),
    };
    diff::postprocess(repo, pairs, o)
}

/// Tudo que `log`, `show` e `rev-list` partilham depois de analisar as opções.
struct Setup {
    ctx_fmt: Fmt,
    abbrev_commit: bool,
    filters: Filters,
    dopts: DiffOpts,
    max_count: Option<usize>,
    skip: usize,
}

fn setup(git: &Git, p: &Parsed, default_fmt: Fmt, default_patch: bool) -> R<Setup> {
    let usage = git.usage();
    let dopts = diff_cmd::build_opts(git, p, false, default_patch)?;
    let mut fmt = default_fmt;
    let mut abbrev_commit = false;
    for h in &p.hits {
        match h.id {
            "oneline" => {
                fmt = Fmt::Oneline;
                abbrev_commit = true;
            }
            "pretty" => {
                fmt = if h.negated { Fmt::Medium } else { parse_pretty(h.value.as_deref().unwrap_or(b"medium"))? };
            }
            "format" => {
                fmt = match h.value.as_deref() {
                    Some(v) => match v {
                        b"oneline" | b"short" | b"medium" | b"full" | b"fuller" | b"raw" => parse_pretty(v)?,
                        _ => Fmt::Custom(v.strip_prefix(b"tformat:").or_else(|| v.strip_prefix(b"format:")).unwrap_or(v).to_vec(), !v.starts_with(b"format:")),
                    },
                    None => return Err(Fail::Fatal("invalid --format: missing format".into())),
                };
            }
            "abbrev-commit" => abbrev_commit = !h.negated,
            _ => {}
        }
    }
    let mut max_count = match p.value("max-count") {
        Some(v) => Some(number(usage, "-n", v)?),
        None => None,
    };
    for h in &p.hits {
        if h.id == "number" {
            max_count = Some(number(usage, "-<n>", h.value.as_deref().unwrap_or(b""))?);
        }
    }
    let skip = match p.value("skip") {
        Some(v) => number(usage, "--skip", v)?,
        None => 0,
    };
    Ok(Setup { ctx_fmt: fmt, abbrev_commit, filters: Filters::from(p)?, dopts, max_count, skip })
}

fn decorate_mode(git: &Git, p: &Parsed) -> R<Option<bool>> {
    let value: Option<String> = match p.hits.iter().rev().find(|h| h.id == "decorate") {
        Some(h) if h.negated => Some("no".to_string()),
        Some(h) => Some(h.value.as_deref().map(os::lossy).unwrap_or_else(|| "short".to_string())),
        None => git.config().get("log.decorate"),
    };
    Ok(match value.as_deref() {
        Some("short") | Some("true") | Some("yes") | Some("on") | Some("1") => Some(false),
        Some("full") => Some(true),
        Some("no") | Some("false") | Some("off") | Some("0") | Some("auto") | None => None,
        Some(other) => return Err(Fail::Fatal(format!("invalid --decorate option: {other}"))),
    })
}

fn date_mode(p: &Parsed) -> R<DateMode> {
    if p.has("relative-date") {
        return Ok(DateMode::Relative);
    }
    match p.value_str("date") {
        None => Ok(DateMode::Normal),
        Some(v) => DateMode::parse(&v).ok_or_else(|| Fail::Fatal(format!("unknown date format {v}"))),
    }
}

pub(crate) fn unrecognized(p: &Parsed) -> R<()> {
    if let Some(u) = p.unknown.first() {
        return Err(Fail::Fatal(format!("unrecognized argument: {}", os::lossy(u))));
    }
    Ok(())
}

fn make_ctx<'a>(git: &Git, repo: &'a Repo, p: &Parsed, s: &Setup, ps: Pathspec) -> R<Ctx<'a>> {
    let diff = if s.dopts.any_format() { Some(s.dopts.clone()) } else { None };
    Ok(Ctx {
        repo,
        fmt: s.ctx_fmt.clone(),
        abbrev_commit: s.abbrev_commit,
        abbrev: s.dopts.abbrev,
        date: date_mode(p)?,
        decorate: decorate_mode(git, p)?,
        decos: None,
        diff,
        ps,
        nul: p.has("nul"),
        shown_one: false,
        parents: p.has("parents"),
        graph: None,
        missing_newline: false,
    })
}

/// O laço de `log`: caminhada, filtros, `--skip`, `-n`, `--reverse`.
fn log_walk(repo: &Repo, p: &Parsed, s: &Setup, ctx: &mut Ctx<'_>, revs: &Revs, ps: Option<&Pathspec>) -> R<()> {
    if p.has("reflog") {
        return log_reflog(repo, p, s, ctx, revs);
    }
    if p.has("graph") {
        return log_graph_walk(repo, p, s, ctx, revs, ps);
    }
    let mut walker = Walker::new(repo, &revs.include, &revs.exclude, p.has("first-parent"), ps)?;
    let reverse = p.has("reverse");
    let mut skipped = 0usize;
    let mut shown = 0usize;
    let mut held: Vec<Oid> = Vec::new();
    while let Some((id, show)) = walker.next()? {
        if !show {
            continue;
        }
        let c = repo.read_commit(&id)?;
        if !s.filters.accepts(&c) {
            continue;
        }
        if skipped < s.skip {
            skipped += 1;
            continue;
        }
        if reverse {
            held.push(id);
        } else {
            ctx.print_commit(&id, None)?;
        }
        shown += 1;
        if s.max_count.is_some_and(|m| shown >= m) {
            break;
        }
    }
    for id in held.iter().rev() {
        ctx.print_commit(id, None)?;
    }
    Ok(())
}

/// `log --graph`: a caminhada inteira primeiro, depois a ordem topológica (que `--graph` liga) e o
/// desenho de cada commit mostrado. Os pais "interessantes" de um commit são os que também
/// apareceriam (estão na caminhada e passam nos filtros), mesmo que `-n` os corte.
fn log_graph_walk(repo: &Repo, p: &Parsed, s: &Setup, ctx: &mut Ctx<'_>, revs: &Revs, ps: Option<&Pathspec>) -> R<()> {
    let mut walker = Walker::new(repo, &revs.include, &revs.exclude, p.has("first-parent"), ps)?;
    let mut list: Vec<Oid> = Vec::new();
    let mut commits: HashMap<Oid, Commit> = HashMap::new();
    while let Some((id, show)) = walker.next()? {
        if !show {
            continue;
        }
        let c = repo.read_commit(&id)?;
        commits.insert(id, c);
        list.push(id);
    }
    let order = topo_sort(&list, &commits);
    let interesting: HashSet<Oid> = list.iter().filter(|id| s.filters.accepts(&commits[*id])).copied().collect();
    let first_parent = p.has("first-parent");
    ctx.graph = Some(GraphDraw::new());
    let mut skipped = 0usize;
    let mut shown = 0usize;
    for id in order {
        if !interesting.contains(&id) {
            continue;
        }
        if skipped < s.skip {
            skipped += 1;
            continue;
        }
        let mut parents: Vec<Oid> = commits[&id].parents.iter().filter(|par| interesting.contains(*par)).copied().collect();
        if first_parent {
            parents.truncate(1);
        }
        if let Some(g) = ctx.graph.as_mut() {
            g.update(id, parents);
        }
        ctx.print_commit(&id, None)?;
        shown += 1;
        if s.max_count.is_some_and(|m| shown >= m) {
            break;
        }
    }
    Ok(())
}

/// A ordem topológica do git (`REV_SORT_IN_GRAPH_ORDER`): grau de entrada contado só entre os
/// commits da lista, pontas na ordem original e uma pilha, de modo que o último pai empilhado de
/// um merge sai primeiro.
fn topo_sort(list: &[Oid], commits: &HashMap<Oid, Commit>) -> Vec<Oid> {
    let mut indegree: HashMap<Oid, usize> = list.iter().map(|id| (*id, 1)).collect();
    for id in list {
        for par in &commits[id].parents {
            if let Some(d) = indegree.get_mut(par) {
                *d += 1;
            }
        }
    }
    let mut stack: Vec<Oid> = list.iter().filter(|id| indegree[*id] == 1).copied().collect();
    stack.reverse();
    let mut out: Vec<Oid> = Vec::with_capacity(list.len());
    while let Some(id) = stack.pop() {
        for par in &commits[&id].parents {
            if let Some(d) = indegree.get_mut(par) {
                if *d == 0 {
                    continue;
                }
                *d -= 1;
                if *d == 1 {
                    stack.push(*par);
                }
            }
        }
        indegree.insert(id, 0);
        out.push(id);
    }
    out
}

// ---- grafo (graph.c) --------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum GState {
    Padding,
    Skip,
    PreCommit,
    Commit,
    PostMerge,
    Collapsing,
}

/// A máquina de estados do `graph.c`: colunas antes e depois do commit atual, o mapeamento de
/// cada posição na tela para a coluna de destino, e a linha que cada estado desenha.
struct GraphDraw {
    commit: Option<Oid>,
    parents: Vec<Oid>,
    prev_commit_index: isize,
    commit_index: isize,
    width: isize,
    expansion_row: isize,
    state: GState,
    prev_state: GState,
    columns: Vec<Oid>,
    new_columns: Vec<Oid>,
    mapping: Vec<isize>,
    old_mapping: Vec<isize>,
    mapping_size: usize,
    merge_layout: isize,
    edges_added: isize,
    prev_edges_added: isize,
}

const MERGE_CHARS: [char; 3] = ['/', '|', '\\'];

impl GraphDraw {
    fn new() -> GraphDraw {
        GraphDraw {
            commit: None,
            parents: Vec::new(),
            prev_commit_index: 0,
            commit_index: 0,
            width: 0,
            expansion_row: 0,
            state: GState::Padding,
            prev_state: GState::Padding,
            columns: Vec::new(),
            new_columns: Vec::new(),
            mapping: Vec::new(),
            old_mapping: Vec::new(),
            mapping_size: 0,
            merge_layout: 0,
            edges_added: 0,
            prev_edges_added: 0,
        }
    }

    fn num_parents(&self) -> isize {
        self.parents.len() as isize
    }

    fn is_finished(&self) -> bool {
        self.state == GState::Padding
    }

    fn set_state(&mut self, s: GState) {
        self.prev_state = self.state;
        self.state = s;
    }

    fn update(&mut self, commit: Oid, parents: Vec<Oid>) {
        self.commit = Some(commit);
        self.parents = parents;
        self.prev_commit_index = self.commit_index;
        self.update_columns(commit);
        self.expansion_row = 0;
        self.state = if self.state != GState::Padding {
            GState::Skip
        } else if self.needs_pre_commit_line() {
            GState::PreCommit
        } else {
            GState::Commit
        };
    }

    fn needs_pre_commit_line(&self) -> bool {
        self.num_parents() >= 3 && self.commit_index < self.columns.len() as isize - 1 && self.expansion_row < self.num_parents() - 2
    }

    fn update_columns(&mut self, commit: Oid) {
        std::mem::swap(&mut self.columns, &mut self.new_columns);
        self.new_columns.clear();
        let size = 2 * (self.columns.len() + self.parents.len());
        if self.mapping.len() < size {
            self.mapping.resize(size, -1);
            self.old_mapping.resize(size, -1);
        }
        self.mapping_size = size;
        for m in self.mapping.iter_mut().take(size) {
            *m = -1;
        }
        self.width = 0;
        self.prev_edges_added = self.edges_added;
        self.edges_added = 0;
        let n = self.columns.len();
        let mut seen = false;
        for i in 0..=n {
            let col_commit = if i == n {
                if seen {
                    break;
                }
                commit
            } else {
                self.columns[i]
            };
            if col_commit == commit {
                seen = true;
                self.commit_index = i as isize;
                self.merge_layout = -1;
                for par in self.parents.clone() {
                    self.insert_into_new_columns(par, i as isize);
                }
                // O commit ocupa ao menos duas posições, mesmo sem pais.
                if self.parents.is_empty() {
                    self.width += 2;
                }
            } else {
                self.insert_into_new_columns(col_commit, -1);
            }
        }
        while self.mapping_size > 1 && self.mapping[self.mapping_size - 1] < 0 {
            self.mapping_size -= 1;
        }
    }

    fn insert_into_new_columns(&mut self, commit: Oid, idx: isize) {
        let i = match self.new_columns.iter().position(|c| *c == commit) {
            Some(i) => i,
            None => {
                self.new_columns.push(commit);
                self.new_columns.len() - 1
            }
        } as isize;
        let mapping_idx;
        if self.num_parents() > 1 && idx > -1 && self.merge_layout == -1 {
            // Primeiro pai de um merge: o leiaute da linha do merge depende de o pai estar numa
            // coluna à esquerda.
            let dist = idx - i;
            let shift = if dist > 1 { 2 * dist - 3 } else { 1 };
            self.merge_layout = if dist > 0 { 0 } else { 1 };
            self.edges_added = self.num_parents() + self.merge_layout - 2;
            mapping_idx = self.width + (self.merge_layout - 1) * shift;
            self.width += 2 * self.merge_layout;
        } else if self.edges_added > 0 && self.width >= 2 && i == self.mapping[(self.width - 2) as usize] {
            // As arestas novas do merge se juntam já na última coluna existente.
            mapping_idx = self.width - 2;
            self.edges_added = -1;
        } else {
            mapping_idx = self.width;
            self.width += 2;
        }
        self.mapping[mapping_idx as usize] = i;
    }

    fn is_mapping_correct(&self) -> bool {
        self.mapping[..self.mapping_size].iter().enumerate().all(|(i, t)| *t < 0 || *t == (i / 2) as isize)
    }

    fn pad(&self, line: &mut String) {
        let w = line.chars().count() as isize;
        if w < self.width {
            line.push_str(&" ".repeat((self.width - w) as usize));
        }
    }

    /// `graph_padding_line`: deixa as linhas dos ramos como estão.
    fn padding_line(&mut self) -> Vec<u8> {
        let Some(commit) = self.commit else { return Vec::new() };
        if self.state != GState::Commit {
            return self.next_line().0;
        }
        let mut line = String::new();
        for col in &self.columns {
            line.push('|');
            if *col == commit && self.num_parents() > 2 {
                line.push_str(&" ".repeat(((self.num_parents() - 2) * 2) as usize));
            } else {
                line.push(' ');
            }
        }
        self.pad(&mut line);
        self.prev_state = GState::Padding;
        line.into_bytes()
    }

    /// `graph_next_line`: a próxima linha do grafo e se ela é a do commit.
    fn next_line(&mut self) -> (Vec<u8>, bool) {
        let mut line = String::new();
        let mut commit_line = false;
        match self.state {
            GState::Padding => {
                for _ in &self.new_columns {
                    line.push_str("| ");
                }
            }
            GState::Skip => {
                line.push_str("...");
                if self.needs_pre_commit_line() {
                    self.set_state(GState::PreCommit);
                } else {
                    self.set_state(GState::Commit);
                }
            }
            GState::PreCommit => self.pre_commit_line(&mut line),
            GState::Commit => {
                self.commit_line(&mut line);
                commit_line = true;
            }
            GState::PostMerge => self.post_merge_line(&mut line),
            GState::Collapsing => self.collapsing_line(&mut line),
        }
        self.pad(&mut line);
        (line.into_bytes(), commit_line)
    }

    fn pre_commit_line(&mut self, line: &mut String) {
        let commit = self.commit;
        let mut seen = false;
        for (i, col) in self.columns.iter().enumerate() {
            if Some(*col) == commit {
                seen = true;
                line.push('|');
                line.push_str(&" ".repeat(self.expansion_row as usize));
            } else if seen && self.expansion_row == 0 {
                if self.prev_state == GState::PostMerge && self.prev_commit_index < i as isize {
                    line.push('\\');
                } else {
                    line.push('|');
                }
            } else if seen && self.expansion_row > 0 {
                line.push('\\');
            } else {
                line.push('|');
            }
            line.push(' ');
        }
        self.expansion_row += 1;
        if !self.needs_pre_commit_line() {
            self.set_state(GState::Commit);
        }
    }

    fn commit_line(&mut self, line: &mut String) {
        let commit = self.commit;
        let n = self.columns.len();
        let mut seen = false;
        for i in 0..=n {
            let col_commit = if i == n {
                if seen {
                    break;
                }
                commit
            } else {
                Some(self.columns[i])
            };
            if col_commit == commit {
                seen = true;
                line.push('*');
                if self.num_parents() > 2 {
                    // graph_draw_octopus_merge
                    let dashed = self.num_parents() + self.merge_layout - 3;
                    for k in 0..dashed {
                        line.push('-');
                        line.push(if k == dashed - 1 { '.' } else { '-' });
                    }
                }
            } else if seen && self.edges_added > 1 {
                line.push('\\');
            } else if seen && self.edges_added == 1 {
                if self.prev_state == GState::PostMerge && self.prev_edges_added > 0 && self.prev_commit_index < i as isize {
                    line.push('\\');
                } else {
                    line.push('|');
                }
            } else if self.prev_state == GState::Collapsing && self.old_mapping.get(2 * i + 1) == Some(&(i as isize)) && self.mapping.get(2 * i).is_some_and(|m| *m < i as isize) {
                line.push('/');
            } else {
                line.push('|');
            }
            line.push(' ');
        }
        if self.num_parents() > 1 {
            self.set_state(GState::PostMerge);
        } else if self.is_mapping_correct() {
            self.set_state(GState::Padding);
        } else {
            self.set_state(GState::Collapsing);
        }
    }

    fn post_merge_line(&mut self, line: &mut String) {
        let commit = self.commit;
        let first_parent = self.parents.first().copied();
        let n = self.columns.len();
        let mut seen = false;
        let mut parent_col = false;
        for i in 0..=n {
            let col_commit = if i == n {
                if seen {
                    break;
                }
                commit
            } else {
                Some(self.columns[i])
            };
            if col_commit == commit {
                seen = true;
                let mut idx = self.merge_layout;
                for j in 0..self.num_parents() {
                    line.push(MERGE_CHARS[idx as usize]);
                    if idx == 2 {
                        if self.edges_added > 0 || j < self.num_parents() - 1 {
                            line.push(' ');
                        }
                    } else {
                        idx += 1;
                    }
                }
                if self.edges_added == 0 {
                    line.push(' ');
                }
            } else if seen {
                line.push(if self.edges_added > 0 { '\\' } else { '|' });
                line.push(' ');
            } else {
                line.push('|');
                if self.merge_layout != 0 || i as isize != self.commit_index - 1 {
                    line.push(if parent_col { '_' } else { ' ' });
                }
            }
            if col_commit.is_some() && col_commit == first_parent {
                parent_col = true;
            }
        }
        if self.is_mapping_correct() {
            self.set_state(GState::Padding);
        } else {
            self.set_state(GState::Collapsing);
        }
    }

    fn collapsing_line(&mut self, line: &mut String) {
        let size = self.mapping_size;
        std::mem::swap(&mut self.mapping, &mut self.old_mapping);
        for m in self.mapping.iter_mut().take(size) {
            *m = -1;
        }
        let mut used_horizontal = false;
        let mut horizontal_edge: isize = -1;
        let mut horizontal_edge_target: isize = -1;
        for i in 0..size {
            let target = self.old_mapping[i];
            if target < 0 {
                continue;
            }
            let ii = i as isize;
            if target * 2 == ii {
                // A coluna já está no lugar certo.
                self.mapping[i] = target;
            } else if self.mapping[i - 1] < 0 {
                // Nada à esquerda: anda uma posição para a esquerda.
                self.mapping[i - 1] = target;
                if horizontal_edge == -1 {
                    horizontal_edge = ii;
                    horizontal_edge_target = target;
                    let mut j = target * 2 + 3;
                    while j < ii - 2 {
                        self.mapping[j as usize] = target;
                        j += 2;
                    }
                }
            } else if self.mapping[i - 1] == target {
                // Já há à esquerda uma linha para o mesmo pai: as duas se juntam.
            } else {
                // Há uma linha de outro ramo à esquerda: cruza por cima dela.
                self.mapping[i - 2] = target;
                if horizontal_edge == -1 {
                    horizontal_edge_target = target;
                    horizontal_edge = ii - 1;
                    let mut j = target * 2 + 3;
                    while j < ii - 2 {
                        self.mapping[j as usize] = target;
                        j += 2;
                    }
                }
            }
        }
        self.old_mapping[..size].copy_from_slice(&self.mapping[..size]);
        if size > 0 && self.mapping[size - 1] < 0 {
            self.mapping_size -= 1;
        }
        for i in 0..self.mapping_size {
            let target = self.mapping[i];
            let ii = i as isize;
            if target < 0 {
                line.push(' ');
            } else if target * 2 == ii {
                line.push('|');
            } else if target == horizontal_edge_target && ii != horizontal_edge - 1 {
                // Só o primeiro segmento da aresta horizontal continua na linha seguinte.
                if ii != target * 2 + 3 {
                    self.mapping[i] = -1;
                }
                used_horizontal = true;
                line.push('_');
            } else {
                if used_horizontal && ii < horizontal_edge {
                    self.mapping[i] = -1;
                }
                line.push('/');
            }
        }
        if self.is_mapping_correct() {
            self.set_state(GState::Padding);
        }
    }
}

/// `Nome <e-mail>` em nome e e-mail.
fn split_name_email(who: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let Some(lt) = who.iter().position(|c| *c == b'<') else { return (who.to_vec(), Vec::new()) };
    let gt = who[lt..].iter().position(|c| *c == b'>').map(|g| g + lt).unwrap_or(who.len());
    (object::trim_ascii(&who[..lt]).to_vec(), who[lt + 1..gt].to_vec())
}

/// `log -g`: as entradas do reflog do primeiro nome dado (ou HEAD), da mais nova pra mais velha.
fn log_reflog(repo: &Repo, p: &Parsed, s: &Setup, ctx: &mut Ctx<'_>, revs: &Revs) -> R<()> {
    let base = revs.names.first().cloned().unwrap_or_else(|| b"HEAD".to_vec());
    let base_name = os::lossy(&base);
    let full = if base_name == "HEAD" { "HEAD".to_string() } else { repo.dwim_ref_name(&base_name)?.unwrap_or_else(|| base_name.clone()) };
    let entries = repo.read_reflog(&full);
    let mut skipped = 0usize;
    let mut shown = 0usize;
    let mut held: Vec<(Oid, Rl)> = Vec::new();
    for (n, e) in entries.iter().rev().enumerate() {
        let Some(c) = repo.try_read(&e.new)?.and_then(|(k, d)| if k == Kind::Commit { object::parse_commit(&d).ok() } else { None }) else { continue };
        if !s.filters.accepts(&c) {
            continue;
        }
        if skipped < s.skip {
            skipped += 1;
            continue;
        }
        let (name, email) = split_name_email(&e.who);
        let rl = Rl { selector: format!("{base_name}@{{{n}}}"), message: e.message.clone(), name, email };
        if p.has("reverse") {
            held.push((e.new, rl));
        } else {
            ctx.print_commit(&e.new, Some(&rl))?;
        }
        shown += 1;
        if s.max_count.is_some_and(|m| shown >= m) {
            break;
        }
    }
    for (id, rl) in held.iter().rev() {
        ctx.print_commit(id, Some(rl))?;
    }
    Ok(())
}

pub fn run_log(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let p = opts::parse(&all_specs(), args, opts::NUMBER | opts::KEEP_UNKNOWN, usage)?;
    unrecognized(&p)?;
    if p.has("graph") && p.has("reverse") {
        return Err(Fail::Fatal("options '--graph' and '--reverse' cannot be used together".into()));
    }
    let s = setup(git, &p, Fmt::Medium, false)?;
    let repo = git.repo()?;
    let revs = resolve_revs(repo, &p)?;
    // Os caminhos limitam a caminhada e também o diff que cada commit mostra.
    let ps = if revs.paths.is_empty() { None } else { Some(git.pathspec(&revs.paths)?) };
    let mut ctx = make_ctx(git, repo, &p, &s, ps.clone().unwrap_or_default())?;
    log_walk(repo, &p, &s, &mut ctx, &revs, ps.as_ref())?;
    Ok(0)
}

pub fn run_whatchanged(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let mut largs: Vec<Vec<u8>> = vec![b"--raw".to_vec(), b"--no-merges".to_vec()];
    largs.extend(args.iter().cloned());
    run_log(git, &largs)
}

// ---- show -------------------------------------------------------------------------------------

pub fn run_show(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let p = opts::parse(&all_specs(), args, opts::NUMBER | opts::KEEP_UNKNOWN, usage)?;
    unrecognized(&p)?;
    let s = setup(git, &p, Fmt::Medium, true)?;
    let repo = git.repo()?;
    let mut ctx = make_ctx(git, repo, &p, &s, Pathspec::default())?;
    let (before, after) = p.split_dashdash();
    let mut specs: Vec<Vec<u8>> = before;
    if specs.is_empty() {
        specs.push(b"HEAD".to_vec());
    }
    if !after.is_empty() {
        ctx.ps = git.pathspec(&after)?;
    }
    for spec in &specs {
        let id = repo.rev_parse(spec)?.ok_or_else(|| rev::bad_revision(spec))?;
        show_object(repo, &mut ctx, spec, &id)?;
    }
    Ok(0)
}

fn show_object(repo: &Repo, ctx: &mut Ctx<'_>, spec: &[u8], id: &Oid) -> R<()> {
    let (kind, data) = repo.read_object(id)?;
    match kind {
        Kind::Commit => ctx.print_commit(id, None),
        Kind::Blob => {
            os::out(&data);
            Ok(())
        }
        Kind::Tree => {
            let mut out = format!("tree {}\n\n", os::lossy(spec)).into_bytes();
            for e in repo.read_tree(id)? {
                out.extend_from_slice(&e.name);
                if e.is_tree() {
                    out.push(b'/');
                }
                out.push(b'\n');
            }
            os::out(&out);
            ctx.shown_one = true;
            Ok(())
        }
        Kind::Tag => {
            let t = object::parse_tag(&data).map_err(Fail::Fatal)?;
            let mut out: Vec<u8> = Vec::new();
            if ctx.shown_one {
                out.push(b'\n');
            }
            out.extend_from_slice(format!("tag {}\n", os::lossy(&t.name)).as_bytes());
            if let Some(tagger) = t.tagger.as_deref().and_then(object::parse_ident) {
                out.extend_from_slice(format!("Tagger: {}\n", os::lossy(&tagger.name_email())).as_bytes());
                if let Some(when) = tagger.date {
                    out.extend_from_slice(format!("Date:   {}\n", date::show_date(when, tagger.tz, &ctx.date)).as_bytes());
                }
            }
            out.push(b'\n');
            out.extend_from_slice(&t.message);
            if !t.message.ends_with(b"\n") {
                out.push(b'\n');
            }
            os::out(&out);
            ctx.shown_one = true;
            show_object(repo, ctx, spec, &t.object)
        }
    }
}

// ---- rev-list ---------------------------------------------------------------------------------

pub fn run_rev_list(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let p = opts::parse(&all_specs(), args, opts::NUMBER | opts::KEEP_UNKNOWN, usage)?;
    unrecognized(&p)?;
    let repo = git.repo()?;
    let nothing_asked = p.args.is_empty() && !p.has("all") && !p.has("branches") && !p.has("tags") && !p.has("remotes");
    if nothing_asked {
        opts::usage_to_stderr(usage);
        return Err(Fail::Exit(129));
    }
    let s = setup(git, &p, Fmt::Oneline, false)?;
    let custom_format = p.present("format") || p.present("pretty");
    let revs = resolve_revs(repo, &p)?;
    let ps = if revs.paths.is_empty() { None } else { Some(git.pathspec(&revs.paths)?) };
    let mut walker = Walker::new(repo, &revs.include, &revs.exclude, p.has("first-parent"), ps.as_ref())?;
    let mut ctx = make_ctx(git, repo, &p, &s, Pathspec::default())?;
    ctx.diff = None;
    let abbrev = p.has("abbrev-commit") || s.abbrev_commit;
    let count_only = p.has("count");
    let mut skipped = 0usize;
    let mut ids: Vec<Oid> = Vec::new();
    while let Some((id, show)) = walker.next()? {
        if !show {
            continue;
        }
        let c = repo.read_commit(&id)?;
        if !s.filters.accepts(&c) {
            continue;
        }
        if skipped < s.skip {
            skipped += 1;
            continue;
        }
        ids.push(id);
        if s.max_count.is_some_and(|m| ids.len() >= m) {
            break;
        }
    }
    if p.has("reverse") {
        ids.reverse();
    }
    if count_only {
        os::outs(&format!("{}\n", ids.len()));
        return Ok(0);
    }
    let mut out: Vec<u8> = Vec::new();
    for id in &ids {
        let shown = if abbrev { ctx.abbr(id) } else { id.hex() };
        if custom_format {
            let c = repo.read_commit(id)?;
            out.extend_from_slice(format!("commit {shown}\n").as_bytes());
            let body = match &s.ctx_fmt {
                Fmt::Custom(text, _) => ctx.expand(text, id, &c, None)?,
                Fmt::Oneline => c.subject(),
                other => {
                    let f = other.clone();
                    let mut b = Vec::new();
                    ctx.render_header(&mut b, &f, id, &c, None)?;
                    // O cabeçalho "commit ..." já saiu acima.
                    let nl = b.iter().position(|x| *x == b'\n').map(|n| n + 1).unwrap_or(0);
                    b.split_off(nl)
                }
            };
            out.extend_from_slice(&body);
            out.push(b'\n');
            continue;
        }
        out.extend_from_slice(shown.as_bytes());
        if ctx.parents {
            for par in repo.read_commit(id)?.parents {
                let text = if abbrev { ctx.abbr(&par) } else { par.hex() };
                out.push(b' ');
                out.extend_from_slice(text.as_bytes());
            }
        }
        out.push(b'\n');
    }
    os::out(&out);
    Ok(0)
}
