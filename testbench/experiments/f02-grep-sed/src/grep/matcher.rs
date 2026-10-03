//! Casamento de linha pro grep: os padrões (`-e`, `-f`, `-F`, `-x`) viram um AST só (F01) e o
//! `grep_matcher::Matcher` busca linha a linha, com a semântica do `-w` do GNU (tenta casada mais curta
//! no mesmo início, depois o próximo início).

use f01_regex::ast::{Assertion, Node, Regex};
use f01_regex::emit::{EmitOptions, Flavor, emit};
use f01_regex::engines::{CompileError, Engine, Matcher as EngineMatcher, next_boundary};
use f01_regex::parse::{Dialect, parse};
use grep_matcher::{LineTerminator, Match, NoCaptures};

use super::args::{GrepOpts, Mode};

/// Qual motor casa as linhas.
pub enum MatcherKind {
    /// Motor do F01 (com o nosso parser e tradutor).
    Gnu(Box<dyn Engine>),
    /// `grep-regex` (o motor do ripgrep: crate `regex`), com o padrão traduzido pra sintaxe Rust.
    RipgrepRegex,
}

enum Built {
    Never,
    Engine(Box<dyn EngineMatcher>),
    Rust(grep_regex::RegexMatcher),
    Perl(fancy_regex::Regex),
}

pub struct LineMatcher {
    built: Built,
    /// `-w` com a semântica do GNU (só no motor do F01; o ripgrep usa a dele).
    gnu_word: bool,
}

#[derive(Debug)]
pub struct MatchError(pub String);

impl std::fmt::Display for MatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Falha ao montar o casador: erro de sintaxe (mensagem do GNU) ou construção sem suporte.
pub enum BuildError {
    Syntax(String),
    Unsupported(String),
}

fn literal(p: &str) -> Node {
    let chars: Vec<Node> = p.chars().map(Node::Char).collect();
    match chars.len() {
        0 => Node::Empty,
        1 => chars.into_iter().next().expect("um"),
        _ => Node::Concat(chars),
    }
}

/// Soma `offset` aos índices de grupo e de backref (pra juntar vários padrões numa alternação).
fn renumber(n: &Node, offset: usize) -> Node {
    match n {
        Node::Group { index, inner } => Node::Group { index: index + offset, inner: Box::new(renumber(inner, offset)) },
        Node::Backref(k) => Node::Backref(k + offset),
        Node::Concat(v) => Node::Concat(v.iter().map(|x| renumber(x, offset)).collect()),
        Node::Alt(v) => Node::Alt(v.iter().map(|x| renumber(x, offset)).collect()),
        Node::Repeat { inner, min, max } => Node::Repeat { inner: Box::new(renumber(inner, offset)), min: *min, max: *max },
        other => other.clone(),
    }
}

/// AST único dos padrões, já com `-x`. `None` se não há padrão (não casa nada).
pub fn combined_ast(opts: &GrepOpts) -> Result<Option<(Regex, Dialect)>, BuildError> {
    if opts.patterns.is_empty() {
        return Ok(None);
    }
    let dialect = if opts.mode == Mode::Extended { Dialect::GrepEre } else { Dialect::GrepBre };
    let mut branches = Vec::new();
    let mut groups = 0;
    for p in &opts.patterns {
        let re = if opts.mode == Mode::Fixed {
            Regex { root: literal(p), groups: 0 }
        } else {
            parse(p, dialect).map_err(|e| BuildError::Syntax(e.to_string()))?
        };
        branches.push(renumber(&re.root, groups));
        groups += re.groups;
    }
    let mut root = if branches.len() == 1 { branches.pop().expect("um") } else { Node::Alt(branches) };
    if opts.line {
        root = Node::Concat(vec![Node::Assert(Assertion::LineStart), root, Node::Assert(Assertion::LineEnd)]);
    }
    Ok(Some((Regex { root, groups }, dialect)))
}

impl LineMatcher {
    pub fn build(opts: &GrepOpts, kind: &MatcherKind) -> Result<LineMatcher, BuildError> {
        if opts.mode == Mode::Perl {
            // GNU grep -P usa PCRE; aqui: fancy-regex (motor F01) ou grep-regex (ripgrep), com o padrão cru.
            let pattern = opts.patterns.join("|");
            let built = match kind {
                MatcherKind::Gnu(_) => {
                    let p = if opts.icase { format!("(?i){pattern}") } else { pattern };
                    Built::Perl(fancy_regex::Regex::new(&p).map_err(|e| BuildError::Syntax(e.to_string()))?)
                }
                MatcherKind::RipgrepRegex => Built::Rust(
                    grep_regex::RegexMatcherBuilder::new()
                        .case_insensitive(opts.icase)
                        .word(opts.word)
                        .whole_line(opts.line)
                        .build(&pattern)
                        .map_err(|e| BuildError::Syntax(e.to_string()))?,
                ),
            };
            return Ok(LineMatcher { built, gnu_word: matches!(kind, MatcherKind::Gnu(_)) && opts.word });
        }
        let Some((re, dialect)) = combined_ast(opts)? else {
            return Ok(LineMatcher { built: Built::Never, gnu_word: false });
        };
        match kind {
            MatcherKind::Gnu(engine) => {
                let src = opts.patterns.join("\n");
                let m = engine.compile_spans(&re, &src, dialect, opts.icase).map_err(|e| match e {
                    CompileError::Unsupported(s) => BuildError::Unsupported(s),
                    CompileError::Engine(s) => BuildError::Syntax(s),
                })?;
                Ok(LineMatcher { built: Built::Engine(m), gnu_word: opts.word })
            }
            MatcherKind::RipgrepRegex => {
                // Sem `-x` no AST: o ripgrep tem o dele (whole_line).
                let mut plain = opts.clone();
                plain.line = false;
                let (re, _) = combined_ast(&plain)?.expect("há padrão");
                let emitted = emit(&re, Flavor::Rust, EmitOptions::default())
                    .map_err(|e| BuildError::Unsupported(e.to_string()))?;
                let m = grep_regex::RegexMatcherBuilder::new()
                    .case_insensitive(opts.icase)
                    .word(opts.word)
                    .whole_line(opts.line)
                    .line_terminator(Some(b'\n'))
                    .build(&emitted.pattern)
                    .map_err(|e| BuildError::Syntax(e.to_string()))?;
                Ok(LineMatcher { built: Built::Rust(m), gnu_word: false })
            }
        }
    }

    pub fn never(&self) -> bool {
        matches!(self.built, Built::Never)
    }

    fn raw(&self, line: &[u8], start: usize) -> Result<Option<(usize, usize)>, MatchError> {
        match &self.built {
            Built::Never => Ok(None),
            Built::Engine(m) => Ok(m.captures_at(line, start).map_err(MatchError)?.and_then(|c| c[0])),
            Built::Rust(m) => grep_matcher::Matcher::find_at(m, line, start)
                .map(|o| o.map(|m| (m.start(), m.end())))
                .map_err(|e| MatchError(e.to_string())),
            Built::Perl(re) => {
                let text = std::str::from_utf8(line).map_err(|_| MatchError("linha não é UTF-8".into()))?;
                if start > text.len() || !text.is_char_boundary(start) {
                    return Ok(None);
                }
                re.find_from_pos(text, start)
                    .map(|o| o.map(|m| (m.start(), m.end())))
                    .map_err(|e| MatchError(e.to_string()))
            }
        }
    }

    /// Primeira casada da linha a partir de `start` (com o `-w` do GNU quando é o caso).
    pub fn find_in_line(&self, line: &[u8], start: usize) -> Result<Option<(usize, usize)>, MatchError> {
        if !self.gnu_word {
            return self.raw(line, start);
        }
        let mut pos = start;
        loop {
            let Some((s, e)) = self.raw(line, pos)? else { return Ok(None) };
            let mut end = e;
            loop {
                if word_bounded(line, s, end) {
                    return Ok(Some((s, end)));
                }
                if end <= s {
                    break;
                }
                // Casada mais curta ancorada em `s` (o `re_match` com `not_eol` do GNU).
                match self.raw(&line[..end - 1], s)? {
                    Some((s2, e2)) if s2 == s && e2 < end => end = e2,
                    _ => break,
                }
            }
            if s >= line.len() {
                return Ok(None);
            }
            pos = next_boundary(line, s);
        }
    }

    /// Casadas não vazias na ordem do `grep -o`.
    pub fn only_matching(&self, line: &[u8]) -> Result<Vec<(usize, usize)>, MatchError> {
        let mut out = Vec::new();
        let mut cur = 0;
        while cur < line.len() {
            let Some((s, e)) = self.find_in_line(line, cur)? else { break };
            if s >= line.len() {
                break;
            }
            if e == s {
                cur = next_boundary(line, s);
            } else {
                out.push((s, e));
                cur = e;
            }
        }
        Ok(out)
    }
}

fn is_word_char(c: char) -> bool {
    c == '_' || c.is_alphanumeric()
}

fn word_bounded(line: &[u8], s: usize, e: usize) -> bool {
    let before = String::from_utf8_lossy(&line[..s]).chars().next_back();
    let after = String::from_utf8_lossy(&line[e..]).chars().next();
    !before.is_some_and(is_word_char) && !after.is_some_and(is_word_char)
}

impl grep_matcher::Matcher for LineMatcher {
    type Captures = NoCaptures;
    type Error = MatchError;

    fn find_at(&self, haystack: &[u8], at: usize) -> Result<Option<Match>, MatchError> {
        let mut ls = haystack[..at].iter().rposition(|&b| b == b'\n').map(|i| i + 1).unwrap_or(0);
        let mut pos = at;
        loop {
            let le = haystack[ls..].iter().position(|&b| b == b'\n').map(|i| ls + i).unwrap_or(haystack.len());
            if let Some((s, e)) = self.find_in_line(&haystack[ls..le], pos - ls)? {
                return Ok(Some(Match::new(ls + s, ls + e)));
            }
            if le >= haystack.len() {
                return Ok(None);
            }
            ls = le + 1;
            pos = ls;
        }
    }

    fn new_captures(&self) -> Result<NoCaptures, MatchError> {
        Ok(NoCaptures::new())
    }

    fn line_terminator(&self) -> Option<LineTerminator> {
        Some(LineTerminator::byte(b'\n'))
    }
}
