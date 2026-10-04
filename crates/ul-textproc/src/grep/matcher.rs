//! Casamento de linhas do grep: porte do `EGexecute` (`dfasearch.c`) sobre o regex-posix.
//!
//! Duas regex: `select` (modo linha, varre o buffer inteiro e escolhe linhas; usa a visão do
//! `dfa.c` quando não há referência, e `-x` vira `^(...)$`), e `exact` (visão do glibc, aplicada a
//! uma linha só, pros spans do `-o` e pro laço do `-w`).

use std::sync::Arc;

use regex_posix::{ExecFlags, Regex, RegexBuilder, Syntax, charclass, nfa};

/// Como os padrões são interpretados.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Basic,
    Extended,
    Fixed,
    Perl,
}

pub struct LineMatcher {
    kind: Kind,
}

enum Kind {
    Posix { select: Regex, exact: Regex, words: bool },
    /// `-P`: PCRE no GNU; aqui o `fancy-regex` (backtracking leftmost-first, lookaround,
    /// referências), com o padrão montado como o `Pcompile` monta.
    Perl(fancy_regex::Regex),
}

/// Erro de montagem do `-P`, com a mensagem do PCRE2 que o grep imprime.
#[derive(Debug)]
pub struct PerlError(pub String);

/// O que é preciso pra montar o casador.
pub struct MatcherSpec<'a> {
    pub mode: Mode,
    pub patterns: &'a [Vec<u8>],
    pub icase: bool,
    pub words: bool,
    pub lines: bool,
    pub eol: u8,
}

impl LineMatcher {
    /// `-P` (`Pcompile`): um padrão só, `-x` vira `^(?:...)$`, `-w` vira `(?<!\w)(?:...)(?!\w)`, `\d`
    /// casa só dígito ASCII (`PCRE2_EXTRA_ASCII_BSD`) e `$` só no fim (`PCRE2_DOLLAR_ENDONLY`, que é o
    /// `$` do `fancy-regex` fora do modo multilinha).
    pub fn build_perl(spec: &MatcherSpec<'_>) -> Result<LineMatcher, PerlError> {
        let pattern = spec.patterns.first().cloned().unwrap_or_default();
        let text = String::from_utf8(pattern).map_err(|_| PerlError("UTF-8 error: illegal byte (0xfe or 0xff)".into()))?;
        let body = ascii_digits(&text);
        let full = if spec.lines {
            format!("^(?:{body})$")
        } else if spec.words {
            format!("(?<!\\w)(?:{body})(?!\\w)")
        } else {
            body
        };
        let re = fancy_regex::RegexBuilder::new(&full)
            .case_insensitive(spec.icase)
            .backtrack_limit(10_000_000)
            .build()
            .map_err(|e| PerlError(pcre_message(&e)))?;
        Ok(LineMatcher { kind: Kind::Perl(re) })
    }

    /// Os padrões já foram validados (erros de sintaxe saem antes, padrão por padrão).
    pub fn build(spec: &MatcherSpec<'_>) -> Result<LineMatcher, regex_posix::Error> {
        let hook: Arc<dyn Fn() + Send + Sync> = Arc::new(sysabi::sys::checkpoint);
        let pats: Vec<&[u8]> = spec.patterns.iter().map(|p| p.as_slice()).collect();
        let syntax = match spec.mode {
            Mode::Extended => Syntax::EGREP,
            _ => Syntax::GREP,
        };
        let base = RegexBuilder::new(syntax).icase(spec.icase).checkpoint(hook).no_sub(true);
        let build = |b: RegexBuilder| -> Result<Regex, regex_posix::Error> {
            if spec.mode == Mode::Fixed { b.build_literals(&pats) } else { b.build_many(&pats).map_err(|(_, e)| e) }
        };
        let exact = build(base.clone())?;
        let sel_base = base.line_separator(Some(spec.eol)).whole_line(spec.lines);
        let select = if spec.mode == Mode::Fixed || exact.has_backrefs() {
            build(sel_base)?
        } else {
            build(sel_base.dfa_view(true))?
        };
        Ok(LineMatcher { kind: Kind::Posix { select, exact, words: spec.words && !spec.lines } })
    }

    /// Primeira linha selecionada em `buf[p..lim]` (`lim` logo depois do fim de linha da última
    /// linha completa). Devolve início e fim (depois do fim de linha) da linha.
    pub fn next_line(&self, buf: &[u8], p: usize, lim: usize, eol: u8) -> Option<(usize, usize)> {
        let (select, words) = match &self.kind {
            Kind::Posix { select, words, .. } => (select, *words),
            Kind::Perl(re) => {
                // O `Pexecute` vai linha a linha.
                let mut ls = p;
                while ls < lim {
                    let le = buf[ls..lim].iter().position(|&b| b == eol).map(|i| ls + i + 1).unwrap_or(lim);
                    if perl_find(re, &buf[ls..le - 1], 0).is_some() {
                        return Some((ls, le));
                    }
                    ls = le;
                }
                return None;
            }
        };
        let hay = &buf[..lim];
        let mut from = p;
        while from < lim {
            let m = select.find_at(hay, from)?;
            if m.start >= lim {
                return None;
            }
            let ls = buf[p..m.start].iter().rposition(|&b| b == eol).map(|i| p + i + 1).unwrap_or(p).max(from);
            let le = buf[m.start..lim].iter().position(|&b| b == eol).map(|i| m.start + i + 1).unwrap_or(lim);
            if !words || self.find_in_line(&buf[ls..le - 1], 0).is_some() {
                return Some((ls, le));
            }
            from = le;
        }
        None
    }

    /// Casada exata numa linha (sem o fim de linha) a partir de `cur`, com o `-w` do GNU.
    pub fn find_in_line(&self, line: &[u8], cur: usize) -> Option<(usize, usize)> {
        let (exact, words) = match &self.kind {
            Kind::Posix { exact, words, .. } => (exact, *words),
            Kind::Perl(re) => return perl_find(re, line, cur),
        };
        let m = exact.find_at(line, cur)?;
        if !words {
            return Some((m.start, m.end));
        }
        let ptr = cur;
        let (mut start, mut len) = (m.start, m.len());
        loop {
            if !word_next(line, start + len) && !word_prev(line, start) {
                return Some((start, start + len));
            }
            let mut shorter: Option<usize> = None;
            if len > 0 {
                len -= 1;
                // `re_match(beg, match + len - ptr, match - beg)` com `not_eol`: o GNU passa o
                // comprimento relativo a `ptr`, não ao começo da linha.
                let trunc = (start + len).saturating_sub(ptr);
                if start <= trunc {
                    let f = ExecFlags { not_bol: false, not_eol: true };
                    shorter = exact.longest_at(&line[..trunc], start, f).map(|e| e - start);
                }
            }
            match shorter {
                Some(l) if l > 0 => len = l,
                _ => {
                    if start >= line.len() {
                        return None;
                    }
                    let m = exact.find_at(line, start + 1)?;
                    start = m.start;
                    len = m.len();
                }
            }
        }
    }
}

/// Primeira casada do `-P` numa linha a partir de `cur`. Erro em tempo de execução (limite de
/// backtracking) conta como "não casa", como o `PCRE2_ERROR_MATCHLIMIT` faz o grep seguir.
fn perl_find(re: &fancy_regex::Regex, line: &[u8], cur: usize) -> Option<(usize, usize)> {
    match re.find_from_pos(line, cur) {
        Ok(Some(m)) => Some((m.start(), m.end())),
        _ => None,
    }
}

/// `\d` e `\D` só ASCII (`PCRE2_EXTRA_ASCII_BSD`): troca fora de colchetes por `[0-9]`/`[^0-9]` e
/// dentro por `0-9`.
fn ascii_digits(p: &str) -> String {
    let chars: Vec<char> = p.chars().collect();
    let mut out = String::with_capacity(p.len());
    let mut in_class = false;
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '\\' && i + 1 < chars.len() {
            let n = chars[i + 1];
            match (n, in_class) {
                ('d', false) => out.push_str("[0-9]"),
                ('D', false) => out.push_str("[^0-9]"),
                ('d', true) => out.push_str("0-9"),
                _ => {
                    out.push('\\');
                    out.push(n);
                }
            }
            i += 2;
            continue;
        }
        if ch == '[' && !in_class {
            in_class = true;
            out.push(ch);
            i += 1;
            // `]` logo depois de `[` ou `[^` é literal.
            if chars.get(i) == Some(&'^') {
                out.push('^');
                i += 1;
            }
            if chars.get(i) == Some(&']') {
                out.push(']');
                i += 1;
            }
            continue;
        }
        if ch == ']' && in_class {
            in_class = false;
        }
        out.push(ch);
        i += 1;
    }
    out
}

/// Mensagem do PCRE2 10.46 equivalente ao erro do `fancy-regex` (as mais comuns).
fn pcre_message(e: &fancy_regex::Error) -> String {
    use fancy_regex::{CompileError, Error, ParseError};
    match e {
        Error::ParseError(_, pe) => match pe {
            ParseError::UnclosedOpenParen => "missing closing parenthesis".into(),
            ParseError::InvalidRepeat | ParseError::TargetNotRepeatable => {
                "quantifier does not follow a repeatable item".into()
            }
            ParseError::TrailingBackslash => "\\ at end of pattern".into(),
            ParseError::InvalidClass => "missing terminating ] for character class".into(),
            ParseError::InvalidBackref => "reference to non-existent subpattern".into(),
            ParseError::InvalidEscape(_) => "unrecognized character follows \\".into(),
            ParseError::GeneralParseError(s) if s.contains("unmatched") || s.contains("unopened") => {
                "unmatched closing parenthesis".into()
            }
            other => other.to_string(),
        },
        Error::CompileError(ce) => match ce.as_ref() {
            CompileError::LookBehindNotConst => "lookbehind assertion is not fixed length".into(),
            CompileError::InvalidBackref(_) => "reference to non-existent subpattern".into(),
            other => other.to_string(),
        },
        other => other.to_string(),
    }
}

/// `wordchar`: `_` ou `iswalnum`.
fn is_word(c: char) -> bool {
    c == '_' || charclass::class_contains(charclass::posix_class(regex_posix::ast::PosixClass::Alnum), c)
}

fn word_next(line: &[u8], pos: usize) -> bool {
    matches!(nfa::decode_at(line, pos), Some((regex_posix::ast::Unit::Char(c), _)) if is_word(c))
}

fn word_prev(line: &[u8], pos: usize) -> bool {
    matches!(nfa::decode_before(line, pos), Some(regex_posix::ast::Unit::Char(c)) if is_word(c))
}
