//! Motor de expressões regulares do módulo `re` (sintaxe do `re` do CPython 3.13).
//!
//! O padrão é lido por um analisador recursivo (só a profundidade de parênteses aninhados recorre)
//! que produz uma árvore, compilada para um programa linear. O casador é uma máquina de
//! retrocesso com pilha explícita: `.*`, `[a-z]+` e laços em geral não recorrem, então textos
//! longos não estouram a pilha nativa. Só lookaround e grupos atômicos chamam o casador de novo
//! (profundidade limitada pelo aninhamento do padrão).
//!
//! Contra retrocesso exponencial: repetições de um único caractere (`.*`, `\d+`, `[^x]*?`) são
//! uma instrução própria, e, quando o programa não tem retrorreferência, lookaround, grupo
//! atômico nem laço que aceite vazio, o casador memoriza os estados `(desvio, posição)` já
//! esgotados (um padrão como `(a+)+b` cai de exponencial para polinomial).
//!
//! O texto é uma fatia de pontos de código (`&[char]`); todos os índices são em pontos de código.
//! Fora desta versão: padrões em `bytes`, `\N{nome}` e o modo LOCALE.

pub const I: u32 = 2;
pub const L: u32 = 4;
pub const M: u32 = 8;
pub const S: u32 = 16;
pub const U: u32 = 32;
pub const X: u32 = 64;
pub const A: u32 = 256;

pub const UNSET: usize = usize::MAX;
const INF: usize = usize::MAX;
const MAX_PROG: usize = 4_000_000;
const MEMO_AFTER_STEPS: usize = 4096;
const MEMO_MAX_BITS: usize = 1 << 27;

// ---------------------------------------------------------------------------
// Erros
// ---------------------------------------------------------------------------

/// Erro de padrão. `value_error` marca os que o CPython levanta como `ValueError` (e não `re.error`).
#[derive(Debug, Clone)]
pub struct ReError {
    pub msg: String,
    pub pos: Option<usize>,
    pub value_error: bool,
}

impl ReError {
    pub fn at(msg: impl Into<String>, pos: usize) -> ReError {
        ReError { msg: msg.into(), pos: Some(pos), value_error: false }
    }

    pub fn bare(msg: impl Into<String>) -> ReError {
        ReError { msg: msg.into(), pos: None, value_error: false }
    }

    fn value(msg: impl Into<String>) -> ReError {
        ReError { msg: msg.into(), pos: None, value_error: true }
    }

    /// A mensagem como o CPython a mostra: `msg at position N` e, se o padrão tem quebra de
    /// linha, `(line L, column C)`.
    pub fn format(&self, pattern: &[char]) -> String {
        match self.pos {
            None => self.msg.clone(),
            Some(pos) => {
                let mut s = format!("{} at position {}", self.msg, pos);
                if pattern.contains(&'\n') {
                    let upto = pos.min(pattern.len());
                    let line = pattern[..upto].iter().filter(|c| **c == '\n').count() + 1;
                    let col = match pattern[..upto].iter().rposition(|c| *c == '\n') {
                        Some(l) => pos - l,
                        None => pos + 1,
                    };
                    s.push_str(&format!(" (line {}, column {})", line, col));
                }
                s
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Classes de caracteres
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClassKind {
    Digit,
    NotDigit,
    Word,
    NotWord,
    Space,
    NotSpace,
}

#[derive(Debug, Clone)]
enum SetItem {
    Ch(char),
    Range(char, char),
    Class(ClassKind),
}

#[derive(Debug, Clone)]
struct CharSet {
    negate: bool,
    items: Vec<SetItem>,
    fl: u32,
}

/// Zeros dos blocos de dígitos decimais (Nd) fora do ASCII: cada bloco tem 10 dígitos.
const ND_ZEROS: &[u32] = &[
    0x0660, 0x06F0, 0x07C0, 0x0966, 0x09E6, 0x0A66, 0x0AE6, 0x0B66, 0x0BE6, 0x0C66, 0x0CE6, 0x0D66, 0x0DE6,
    0x0E50, 0x0ED0, 0x0F20, 0x1040, 0x1090, 0x17E0, 0x1810, 0x1946, 0x19D0, 0x1A80, 0x1A90, 0x1B50, 0x1BB0,
    0x1C40, 0x1C50, 0xA620, 0xA8D0, 0xA900, 0xA9D0, 0xA9F0, 0xAA50, 0xABF0, 0xFF10, 0x104A0, 0x11066,
];

fn is_decimal(c: char, ascii: bool) -> bool {
    if c.is_ascii_digit() {
        return true;
    }
    if ascii || (c as u32) < 0x660 {
        return false;
    }
    let v = c as u32;
    if (0x1D7CE..=0x1D7FF).contains(&v) {
        return true;
    }
    ND_ZEROS.iter().any(|z| v >= *z && v < *z + 10)
}

fn is_word(c: char, ascii: bool) -> bool {
    if ascii {
        c.is_ascii_alphanumeric() || c == '_'
    } else {
        c.is_alphanumeric() || c == '_'
    }
}

fn is_space(c: char, ascii: bool) -> bool {
    if ascii {
        matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{b}' | '\u{c}')
    } else {
        c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
    }
}

fn class_match(k: ClassKind, c: char, ascii: bool) -> bool {
    match k {
        ClassKind::Digit => is_decimal(c, ascii),
        ClassKind::NotDigit => !is_decimal(c, ascii),
        ClassKind::Word => is_word(c, ascii),
        ClassKind::NotWord => !is_word(c, ascii),
        ClassKind::Space => is_space(c, ascii),
        ClassKind::NotSpace => !is_space(c, ascii),
    }
}

fn lower1(c: char, ascii: bool) -> char {
    if ascii || c.is_ascii() {
        return c.to_ascii_lowercase();
    }
    let mut it = c.to_lowercase();
    match (it.next(), it.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}

fn upper1(c: char, ascii: bool) -> char {
    if ascii || c.is_ascii() {
        return c.to_ascii_uppercase();
    }
    let mut it = c.to_uppercase();
    match (it.next(), it.next()) {
        (Some(u), None) => u,
        _ => c,
    }
}

/// Chave de comparação sem diferença de maiúsculas (IGNORECASE).
fn fold(c: char, ascii: bool) -> char {
    if !ascii && c == '\u{17f}' {
        return 's';
    }
    lower1(c, ascii)
}

impl CharSet {
    fn test(&self, c: char) -> bool {
        let ascii = self.fl & A != 0;
        self.items.iter().any(|it| match it {
            SetItem::Ch(x) => *x == c,
            SetItem::Range(a, b) => *a <= c && c <= *b,
            SetItem::Class(k) => class_match(*k, c, ascii),
        })
    }

    fn matches(&self, c: char) -> bool {
        let mut r = self.test(c);
        if !r && self.fl & I != 0 {
            let ascii = self.fl & A != 0;
            let lo = lower1(c, ascii);
            let up = upper1(c, ascii);
            r = (lo != c && self.test(lo)) || (up != c && self.test(up));
        }
        r != self.negate
    }
}

// ---------------------------------------------------------------------------
// Árvore
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AssertKind {
    Bol,
    MBol,
    Eol,
    MEol,
    StartText,
    EndText,
    WordB,
    NotWordB,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RepMode {
    Greedy,
    Lazy,
    Possessive,
}

enum Node {
    Empty,
    Char(char, u32),
    Any(u32),
    Set(CharSet),
    Cat(Vec<Node>),
    Alt(Vec<Node>),
    Group(Option<usize>, Box<Node>),
    Repeat { node: Box<Node>, min: usize, max: usize, mode: RepMode },
    Assert(AssertKind, u32),
    Backref(usize, u32),
    Look { behind: Option<usize>, neg: bool, node: Box<Node> },
    Atomic(Box<Node>),
    Cond { group: usize, yes: Box<Node>, no: Box<Node> },
}

/// Largura mínima e máxima (em caracteres) que o nó pode consumir; `None` = ilimitada.
fn width(n: &Node) -> (usize, Option<usize>) {
    match n {
        Node::Empty | Node::Assert(..) | Node::Look { .. } => (0, Some(0)),
        Node::Char(..) | Node::Any(_) | Node::Set(_) => (1, Some(1)),
        Node::Cat(v) => {
            let mut lo = 0usize;
            let mut hi = Some(0usize);
            for x in v {
                let (a, b) = width(x);
                lo = lo.saturating_add(a);
                hi = match (hi, b) {
                    (Some(h), Some(b)) => Some(h.saturating_add(b)),
                    _ => None,
                };
            }
            (lo, hi)
        }
        Node::Alt(v) => {
            let mut lo = usize::MAX;
            let mut hi = Some(0usize);
            for x in v {
                let (a, b) = width(x);
                lo = lo.min(a);
                hi = match (hi, b) {
                    (Some(h), Some(b)) => Some(h.max(b)),
                    _ => None,
                };
            }
            (if lo == usize::MAX { 0 } else { lo }, hi)
        }
        Node::Group(_, b) | Node::Atomic(b) => width(b),
        Node::Repeat { node, min, max, .. } => {
            let (a, b) = width(node);
            let lo = a.saturating_mul(*min);
            let hi = if *max == INF {
                if b == Some(0) {
                    Some(0)
                } else {
                    None
                }
            } else {
                b.map(|x| x.saturating_mul(*max))
            };
            (lo, hi)
        }
        Node::Backref(..) => (0, None),
        Node::Cond { yes, no, .. } => {
            let (a1, b1) = width(yes);
            let (a2, b2) = width(no);
            let hi = match (b1, b2) {
                (Some(x), Some(y)) => Some(x.max(y)),
                _ => None,
            };
            (a1.min(a2), hi)
        }
    }
}

// ---------------------------------------------------------------------------
// Analisador
// ---------------------------------------------------------------------------

struct Parser<'a> {
    p: &'a [char],
    i: usize,
    /// Flags em vigor neste ponto do padrão.
    fl: u32,
    /// Flags globais vindas de `(?i)` no começo.
    global: u32,
    ngroups: usize,
    names: Vec<(String, usize)>,
    open: Vec<usize>,
}

enum Elem {
    Ch(char),
    Class(ClassKind),
}

fn is_identifier(s: &str) -> bool {
    let mut it = s.chars();
    match it.next() {
        Some(c) if c == '_' || c.is_alphabetic() => {}
        _ => return false,
    }
    it.all(|c| c == '_' || c.is_alphanumeric())
}

fn parse_count(s: &str) -> Result<usize, ReError> {
    match s.parse::<u64>() {
        Ok(n) if n < 4_294_967_295 => Ok(n as usize),
        _ => Err(ReError::bare("the repetition number is too large")),
    }
}

fn cat(mut items: Vec<Node>) -> Node {
    match items.len() {
        0 => Node::Empty,
        1 => items.pop().unwrap_or(Node::Empty),
        _ => Node::Cat(items),
    }
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<char> {
        self.p.get(self.i).copied()
    }

    fn peek_at(&self, off: usize) -> Option<char> {
        self.p.get(self.i + off).copied()
    }

    fn digits(&mut self) -> String {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                s.push(c);
                self.i += 1;
            } else {
                break;
            }
        }
        s
    }

    fn skip_verbose(&mut self) {
        if self.fl & X == 0 {
            return;
        }
        while let Some(c) = self.peek() {
            if matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{b}' | '\u{c}') {
                self.i += 1;
            } else if c == '#' {
                while let Some(d) = self.peek() {
                    self.i += 1;
                    if d == '\n' {
                        break;
                    }
                }
            } else {
                break;
            }
        }
    }

    fn expect_close(&mut self, start: usize) -> Result<(), ReError> {
        if self.peek() == Some(')') {
            self.i += 1;
            Ok(())
        } else {
            Err(ReError::at("missing ), unterminated subpattern", start))
        }
    }

    /// Corpo de um grupo sem captura/lookaround: alternância até o `)`, restaurando as flags.
    fn scoped(&mut self, start: usize) -> Result<Node, ReError> {
        let saved = self.fl;
        let body = self.parse_alt(false)?;
        self.fl = saved;
        self.expect_close(start)?;
        Ok(body)
    }

    fn parse_alt(&mut self, top: bool) -> Result<Node, ReError> {
        let mut branches: Vec<Node> = Vec::new();
        loop {
            let first = top && branches.is_empty();
            let seq = self.parse_seq(first)?;
            branches.push(seq);
            if self.peek() == Some('|') {
                self.i += 1;
            } else {
                break;
            }
        }
        if branches.len() == 1 {
            Ok(branches.pop().unwrap_or(Node::Empty))
        } else {
            Ok(Node::Alt(branches))
        }
    }

    fn parse_seq(&mut self, first: bool) -> Result<Node, ReError> {
        let mut items: Vec<Node> = Vec::new();
        loop {
            self.skip_verbose();
            let c = match self.peek() {
                Some(c) => c,
                None => break,
            };
            if c == '|' || c == ')' {
                break;
            }
            let start = self.i;
            self.i += 1;
            let fl = self.fl;
            match c {
                '(' => {
                    let at_start = first && items.is_empty();
                    if let Some(n) = self.parse_group(start, at_start)? {
                        items.push(n);
                    }
                }
                '[' => {
                    let n = self.parse_set(start)?;
                    items.push(n);
                }
                '.' => items.push(Node::Any(fl)),
                '^' => items.push(Node::Assert(if fl & M != 0 { AssertKind::MBol } else { AssertKind::Bol }, fl)),
                '$' => items.push(Node::Assert(if fl & M != 0 { AssertKind::MEol } else { AssertKind::Eol }, fl)),
                '\\' => {
                    let n = self.parse_escape(start)?;
                    items.push(n);
                }
                '*' | '+' | '?' | '{' => {
                    let (min, max) = match c {
                        '*' => (0, INF),
                        '+' => (1, INF),
                        '?' => (0, 1),
                        _ => {
                            if self.peek() == Some('}') {
                                items.push(Node::Char('{', fl));
                                continue;
                            }
                            let save = self.i;
                            let lo = self.digits();
                            let mut comma = false;
                            let mut hi = String::new();
                            if self.peek() == Some(',') {
                                self.i += 1;
                                comma = true;
                                hi = self.digits();
                            }
                            if self.peek() != Some('}') {
                                self.i = save;
                                items.push(Node::Char('{', fl));
                                continue;
                            }
                            self.i += 1;
                            let min = if lo.is_empty() { 0 } else { parse_count(&lo)? };
                            let max = if comma {
                                if hi.is_empty() {
                                    INF
                                } else {
                                    parse_count(&hi)?
                                }
                            } else {
                                min
                            };
                            if max < min {
                                return Err(ReError::at("min repeat greater than max repeat", start + 1));
                            }
                            (min, max)
                        }
                    };
                    match items.last() {
                        None | Some(Node::Assert(..)) => return Err(ReError::at("nothing to repeat", start)),
                        Some(Node::Repeat { .. }) => return Err(ReError::at("multiple repeat", start)),
                        _ => {}
                    }
                    let mode = if self.peek() == Some('?') {
                        self.i += 1;
                        RepMode::Lazy
                    } else if self.peek() == Some('+') {
                        self.i += 1;
                        RepMode::Possessive
                    } else {
                        RepMode::Greedy
                    };
                    if let Some(node) = items.pop() {
                        items.push(Node::Repeat { node: Box::new(node), min, max, mode });
                    }
                }
                _ => items.push(Node::Char(c, fl)),
            }
        }
        Ok(cat(items))
    }

    fn parse_group(&mut self, start: usize, first: bool) -> Result<Option<Node>, ReError> {
        if self.i >= self.p.len() {
            return Err(ReError::at("missing ), unterminated subpattern", start));
        }
        if self.peek() != Some('?') {
            self.ngroups += 1;
            let k = self.ngroups;
            self.open.push(k);
            let body = self.scoped(start)?;
            self.open.pop();
            return Ok(Some(Node::Group(Some(k), Box::new(body))));
        }
        let qpos = self.i;
        self.i += 1;
        let c = match self.peek() {
            Some(c) => c,
            None => return Err(ReError::at("unexpected end of pattern", self.p.len())),
        };
        self.i += 1;
        match c {
            ':' => {
                let body = self.scoped(start)?;
                Ok(Some(Node::Group(None, Box::new(body))))
            }
            '#' => loop {
                match self.peek() {
                    None => return Err(ReError::at("missing ), unterminated comment", start)),
                    Some(')') => {
                        self.i += 1;
                        return Ok(None);
                    }
                    Some(_) => self.i += 1,
                }
            },
            '=' | '!' => {
                let body = self.scoped(start)?;
                Ok(Some(Node::Look { behind: None, neg: c == '!', node: Box::new(body) }))
            }
            '<' => match self.peek() {
                Some(d) if d == '=' || d == '!' => {
                    self.i += 1;
                    let body = self.scoped(start)?;
                    let (mn, mx) = width(&body);
                    if Some(mn) != mx {
                        return Err(ReError::bare("look-behind requires fixed-width pattern"));
                    }
                    Ok(Some(Node::Look { behind: Some(mn), neg: d == '!', node: Box::new(body) }))
                }
                Some(d) => Err(ReError::at(format!("unknown extension ?<{d}"), qpos)),
                None => Err(ReError::at("unexpected end of pattern", self.p.len())),
            },
            '>' => {
                let body = self.scoped(start)?;
                Ok(Some(Node::Atomic(Box::new(body))))
            }
            'P' => self.parse_p(start, qpos),
            '(' => self.parse_cond(start),
            'a' | 'i' | 'L' | 'm' | 's' | 'u' | 'x' | '-' => {
                self.i -= 1;
                self.parse_flags(start, first)
            }
            other => Err(ReError::at(format!("unknown extension ?{other}"), qpos)),
        }
    }

    /// Lê até `term` (que é consumido); o nome não pode ser vazio.
    fn until(&mut self, term: char) -> Result<String, ReError> {
        let begin = self.i;
        let mut s = String::new();
        loop {
            match self.peek() {
                None => {
                    return Err(if s.is_empty() {
                        ReError::at("missing group name", self.p.len())
                    } else {
                        ReError::at(format!("missing {term}, unterminated name"), begin)
                    });
                }
                Some(ch) => {
                    self.i += 1;
                    if ch == term {
                        if s.is_empty() {
                            return Err(ReError::at("missing group name", self.i - 1));
                        }
                        return Ok(s);
                    }
                    s.push(ch);
                }
            }
        }
    }

    fn lookup_name(&self, name: &str) -> Option<usize> {
        self.names.iter().find(|(n, _)| n == name).map(|(_, k)| *k)
    }

    fn parse_p(&mut self, start: usize, qpos: usize) -> Result<Option<Node>, ReError> {
        match self.peek() {
            Some('<') => {
                self.i += 1;
                let name_start = self.i;
                let name = self.until('>')?;
                if !is_identifier(&name) {
                    return Err(ReError::at(format!("bad character in group name '{name}'"), name_start));
                }
                if let Some(old) = self.lookup_name(&name) {
                    return Err(ReError::at(
                        format!("redefinition of group name '{}' as group {}; was group {}", name, self.ngroups + 1, old),
                        name_start,
                    ));
                }
                self.ngroups += 1;
                let k = self.ngroups;
                self.names.push((name, k));
                self.open.push(k);
                let body = self.scoped(start)?;
                self.open.pop();
                Ok(Some(Node::Group(Some(k), Box::new(body))))
            }
            Some('=') => {
                self.i += 1;
                let name_start = self.i;
                let name = self.until(')')?;
                if !is_identifier(&name) {
                    return Err(ReError::at(format!("bad character in group name '{name}'"), name_start));
                }
                let k = match self.lookup_name(&name) {
                    Some(k) => k,
                    None => return Err(ReError::at(format!("unknown group name '{name}'"), name_start)),
                };
                if self.open.contains(&k) {
                    return Err(ReError::at("cannot refer to an open group", name_start));
                }
                Ok(Some(Node::Backref(k, self.fl)))
            }
            Some(d) => Err(ReError::at(format!("unknown extension ?P{d}"), qpos)),
            None => Err(ReError::at("unexpected end of pattern", self.p.len())),
        }
    }

    fn parse_cond(&mut self, start: usize) -> Result<Option<Node>, ReError> {
        let name_start = self.i;
        let name = self.until(')')?;
        let g = if name.chars().all(|c| c.is_ascii_digit()) {
            let n = match name.parse::<usize>() {
                Ok(n) if n > 0 => n,
                _ => return Err(ReError::at("bad group number", name_start)),
            };
            if n > self.ngroups {
                return Err(ReError::at(format!("invalid group reference {n}"), name_start));
            }
            n
        } else {
            if !is_identifier(&name) {
                return Err(ReError::at(format!("bad character in group name '{name}'"), name_start));
            }
            match self.lookup_name(&name) {
                Some(k) => k,
                None => return Err(ReError::at(format!("unknown group name '{name}'"), name_start)),
            }
        };
        let yes = self.parse_seq(false)?;
        let no = if self.peek() == Some('|') {
            self.i += 1;
            let n = self.parse_seq(false)?;
            if self.peek() == Some('|') {
                return Err(ReError::at("conditional backref with more than two branches", self.i));
            }
            n
        } else {
            Node::Empty
        };
        self.expect_close(start)?;
        Ok(Some(Node::Cond { group: g, yes: Box::new(yes), no: Box::new(no) }))
    }

    fn parse_flags(&mut self, start: usize, first: bool) -> Result<Option<Node>, ReError> {
        let mut add = 0u32;
        let mut del = 0u32;
        while let Some(ch) = self.peek() {
            let f = match ch {
                'a' => A,
                'i' => I,
                'L' => L,
                'm' => M,
                's' => S,
                'u' => U,
                'x' => X,
                _ => break,
            };
            if f == L {
                return Err(ReError::at("bad inline flags: cannot use 'L' flag with a str pattern", self.i));
            }
            add |= f;
            self.i += 1;
        }
        if add & A != 0 && add & U != 0 {
            return Err(ReError::at("bad inline flags: flags 'a', 'u' and 'L' are incompatible", self.i));
        }
        match self.peek() {
            Some(')') => {
                self.i += 1;
                if !first {
                    return Err(ReError::at("global flags not at the start of the expression", start));
                }
                self.fl |= add;
                if add & A != 0 {
                    self.fl &= !U;
                }
                if add & U != 0 {
                    self.fl &= !A;
                }
                self.global |= add;
                return Ok(None);
            }
            Some('-') => {
                self.i += 1;
                while let Some(ch) = self.peek() {
                    let f = match ch {
                        'i' => I,
                        'm' => M,
                        's' => S,
                        'x' => X,
                        'a' | 'u' | 'L' => {
                            return Err(ReError::at("bad inline flags: cannot turn off flags 'a', 'u' and 'L'", self.i));
                        }
                        _ => break,
                    };
                    del |= f;
                    self.i += 1;
                }
                if del == 0 {
                    return Err(ReError::at("missing flag", self.i));
                }
                match self.peek() {
                    Some(':') => self.i += 1,
                    Some(ch) if ch.is_alphabetic() => return Err(ReError::at("unknown flag", self.i)),
                    _ => return Err(ReError::at("missing :", self.i)),
                }
            }
            Some(':') => self.i += 1,
            Some(ch) if ch.is_alphabetic() => return Err(ReError::at("unknown flag", self.i)),
            _ => return Err(ReError::at("missing -, : or )", self.i)),
        }
        let saved = self.fl;
        self.fl = (self.fl | add) & !del;
        if add & A != 0 {
            self.fl &= !U;
        }
        if add & U != 0 {
            self.fl &= !A;
        }
        let body = self.parse_alt(false)?;
        self.fl = saved;
        self.expect_close(start)?;
        Ok(Some(Node::Group(None, Box::new(body))))
    }

    fn hex_esc(&mut self, start: usize, n: usize) -> Result<char, ReError> {
        let mut v: u32 = 0;
        let mut cnt = 0;
        while cnt < n {
            match self.peek().and_then(|d| d.to_digit(16)) {
                Some(d) => {
                    v = v.wrapping_mul(16).wrapping_add(d);
                    self.i += 1;
                    cnt += 1;
                }
                None => break,
            }
        }
        let text = |p: &[char], a: usize, b: usize| -> String { p[a..b].iter().collect() };
        if cnt < n {
            return Err(ReError::at(format!("incomplete escape {}", text(self.p, start, self.i)), start));
        }
        match char::from_u32(v) {
            Some(c) => Ok(c),
            None => Err(ReError::at(format!("bad escape {}", text(self.p, start, self.i)), start)),
        }
    }

    fn parse_escape(&mut self, start: usize) -> Result<Node, ReError> {
        let c = match self.peek() {
            Some(c) => c,
            None => return Err(ReError::at("bad escape (end of pattern)", start)),
        };
        self.i += 1;
        let fl = self.fl;
        let class = |k: ClassKind| Node::Set(CharSet { negate: false, items: vec![SetItem::Class(k)], fl });
        match c {
            'A' => Ok(Node::Assert(AssertKind::StartText, fl)),
            'Z' => Ok(Node::Assert(AssertKind::EndText, fl)),
            'b' => Ok(Node::Assert(AssertKind::WordB, fl)),
            'B' => Ok(Node::Assert(AssertKind::NotWordB, fl)),
            'd' => Ok(class(ClassKind::Digit)),
            'D' => Ok(class(ClassKind::NotDigit)),
            'w' => Ok(class(ClassKind::Word)),
            'W' => Ok(class(ClassKind::NotWord)),
            's' => Ok(class(ClassKind::Space)),
            'S' => Ok(class(ClassKind::NotSpace)),
            'a' => Ok(Node::Char('\u{7}', fl)),
            'f' => Ok(Node::Char('\u{c}', fl)),
            'n' => Ok(Node::Char('\n', fl)),
            'r' => Ok(Node::Char('\r', fl)),
            't' => Ok(Node::Char('\t', fl)),
            'v' => Ok(Node::Char('\u{b}', fl)),
            'N' => {
                if self.peek() != Some('{') {
                    return Err(ReError::at("missing {", self.i));
                }
                let open = self.i;
                let close = (open..self.p.len()).find(|&k| self.p[k] == '}');
                let Some(close) = close else {
                    return Err(ReError::at("missing }, unterminated name", open + 1));
                };
                let name: String = self.p[open + 1..close].iter().collect();
                match unicode_names2::character(&name) {
                    Some(ch) => {
                        self.i = close + 1;
                        Ok(Node::Char(ch, fl))
                    }
                    None => Err(ReError::at(format!("undefined character name '{name}'"), start)),
                }
            }
            'x' => Ok(Node::Char(self.hex_esc(start, 2)?, fl)),
            'u' => Ok(Node::Char(self.hex_esc(start, 4)?, fl)),
            'U' => Ok(Node::Char(self.hex_esc(start, 8)?, fl)),
            '0' => {
                let mut v = 0u32;
                let mut n = 0;
                while n < 2 {
                    match self.peek().and_then(|d| d.to_digit(8)) {
                        Some(d) => {
                            v = v * 8 + d;
                            self.i += 1;
                            n += 1;
                        }
                        None => break,
                    }
                }
                Ok(Node::Char(char::from_u32(v).unwrap_or('\0'), fl))
            }
            '1'..='9' => {
                let mut g = c.to_digit(10).unwrap_or(0) as usize;
                if let Some(d2) = self.peek() {
                    if d2.is_ascii_digit() {
                        if ('0'..='3').contains(&c) && ('0'..='7').contains(&d2) {
                            if let Some(d3) = self.peek_at(1) {
                                if ('0'..='7').contains(&d3) {
                                    let v = c.to_digit(8).unwrap_or(0) * 64
                                        + d2.to_digit(8).unwrap_or(0) * 8
                                        + d3.to_digit(8).unwrap_or(0);
                                    self.i += 2;
                                    return Ok(Node::Char(char::from_u32(v).unwrap_or('\0'), fl));
                                }
                            }
                        }
                        g = g * 10 + d2.to_digit(10).unwrap_or(0) as usize;
                        self.i += 1;
                    }
                }
                if g > self.ngroups {
                    return Err(ReError::at(format!("invalid group reference {g}"), start + 1));
                }
                if self.open.contains(&g) {
                    return Err(ReError::at("cannot refer to an open group", start + 1));
                }
                Ok(Node::Backref(g, fl))
            }
            c if c.is_ascii_alphabetic() => Err(ReError::at(format!("bad escape \\{c}"), start)),
            c => Ok(Node::Char(c, fl)),
        }
    }

    fn class_escape(&mut self, start: usize) -> Result<Elem, ReError> {
        let c = match self.peek() {
            Some(c) => c,
            None => return Err(ReError::at("bad escape (end of pattern)", start)),
        };
        self.i += 1;
        match c {
            'd' => Ok(Elem::Class(ClassKind::Digit)),
            'D' => Ok(Elem::Class(ClassKind::NotDigit)),
            'w' => Ok(Elem::Class(ClassKind::Word)),
            'W' => Ok(Elem::Class(ClassKind::NotWord)),
            's' => Ok(Elem::Class(ClassKind::Space)),
            'S' => Ok(Elem::Class(ClassKind::NotSpace)),
            'a' => Ok(Elem::Ch('\u{7}')),
            'b' => Ok(Elem::Ch('\u{8}')),
            'f' => Ok(Elem::Ch('\u{c}')),
            'n' => Ok(Elem::Ch('\n')),
            'r' => Ok(Elem::Ch('\r')),
            't' => Ok(Elem::Ch('\t')),
            'v' => Ok(Elem::Ch('\u{b}')),
            'x' => Ok(Elem::Ch(self.hex_esc(start, 2)?)),
            'u' => Ok(Elem::Ch(self.hex_esc(start, 4)?)),
            'U' => Ok(Elem::Ch(self.hex_esc(start, 8)?)),
            '0'..='7' => {
                let mut v = c.to_digit(8).unwrap_or(0);
                let mut n = 0;
                while n < 2 {
                    match self.peek().and_then(|d| d.to_digit(8)) {
                        Some(d) => {
                            v = v * 8 + d;
                            self.i += 1;
                            n += 1;
                        }
                        None => break,
                    }
                }
                if v > 0o377 {
                    let esc: String = self.p[start..self.i].iter().collect();
                    return Err(ReError::at(format!("octal escape value {esc} outside of range 0-0o377"), start));
                }
                Ok(Elem::Ch(char::from_u32(v).unwrap_or('\0')))
            }
            '8' | '9' => Err(ReError::at(format!("bad escape \\{c}"), start)),
            c if c.is_ascii_alphabetic() => Err(ReError::at(format!("bad escape \\{c}"), start)),
            c => Ok(Elem::Ch(c)),
        }
    }

    fn parse_set(&mut self, start: usize) -> Result<Node, ReError> {
        let fl = self.fl;
        let mut negate = false;
        if self.peek() == Some('^') {
            negate = true;
            self.i += 1;
        }
        let mut items: Vec<SetItem> = Vec::new();
        let mut first = true;
        loop {
            if self.i >= self.p.len() {
                return Err(ReError::at("unterminated character set", start));
            }
            let es = self.i;
            let c = self.p[self.i];
            self.i += 1;
            if c == ']' && !first {
                break;
            }
            first = false;
            let lo = if c == '\\' { self.class_escape(es)? } else { Elem::Ch(c) };
            let lo_end = self.i;
            if self.peek() == Some('-') {
                if self.i + 1 >= self.p.len() {
                    return Err(ReError::at("unterminated character set", start));
                }
                if self.p[self.i + 1] == ']' {
                    match lo {
                        Elem::Ch(a) => items.push(SetItem::Ch(a)),
                        Elem::Class(k) => items.push(SetItem::Class(k)),
                    }
                    items.push(SetItem::Ch('-'));
                    self.i += 1;
                    continue;
                }
                self.i += 1;
                let hs = self.i;
                let hc = self.p[self.i];
                self.i += 1;
                let hi = if hc == '\\' { self.class_escape(hs)? } else { Elem::Ch(hc) };
                let lo_txt: String = self.p[es..lo_end].iter().collect();
                let hi_txt: String = self.p[hs..self.i].iter().collect();
                match (lo, hi) {
                    (Elem::Ch(a), Elem::Ch(b)) => {
                        if a > b {
                            return Err(ReError::at(format!("bad character range {lo_txt}-{hi_txt}"), es));
                        }
                        items.push(SetItem::Range(a, b));
                    }
                    _ => {
                        return Err(ReError::at(format!("bad character range {lo_txt}-{hi_txt}"), es));
                    }
                }
            } else {
                match lo {
                    Elem::Ch(a) => items.push(SetItem::Ch(a)),
                    Elem::Class(k) => items.push(SetItem::Class(k)),
                }
            }
        }
        Ok(Node::Set(CharSet { negate, items, fl }))
    }
}

// ---------------------------------------------------------------------------
// Programa
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
enum Atom {
    Char(char),
    CharI(char, bool),
    Any,
    AnyNl,
    Set(usize),
}

#[derive(Debug, Clone, Copy)]
enum Inst {
    One(Atom),
    Rep { atom: Atom, min: usize, max: usize, mode: RepMode },
    Split { a: usize, b: usize, id: usize },
    Jmp(usize),
    Open(usize),
    Close(usize),
    Assert(AssertKind, bool),
    Backref { g: usize, icase: bool, ascii: bool },
    Look { behind: Option<usize>, neg: bool, end: usize },
    Atomic { end: usize },
    SubMatch,
    Mark(usize),
    EmptyExit { reg: usize, exit: usize },
    Cond { g: usize, else_pc: usize },
    Match,
}

fn atom_ok(sets: &[CharSet], a: Atom, c: char) -> bool {
    match a {
        Atom::Char(x) => c == x,
        Atom::CharI(x, ascii) => fold(c, ascii) == x,
        Atom::Any => c != '\n',
        Atom::AnyNl => true,
        Atom::Set(i) => sets[i].matches(c),
    }
}

struct Compiler {
    insts: Vec<Inst>,
    sets: Vec<CharSet>,
    nregs: usize,
    nsplits: usize,
    reg_base: usize,
    memo_ok: bool,
}

impl Compiler {
    fn push(&mut self, i: Inst) -> usize {
        self.insts.push(i);
        self.insts.len() - 1
    }

    fn push_split(&mut self) -> usize {
        let id = self.nsplits;
        self.nsplits += 1;
        self.push(Inst::Split { a: 0, b: 0, id })
    }

    fn set_split(&mut self, at: usize, a: usize, b: usize) {
        if let Inst::Split { id, .. } = self.insts[at] {
            self.insts[at] = Inst::Split { a, b, id };
        }
    }

    fn new_reg(&mut self) -> usize {
        let r = self.reg_base + self.nregs;
        self.nregs += 1;
        r
    }

    /// Instrução de um só caractere para os nós "simples".
    fn atom(&mut self, n: &Node) -> Option<Atom> {
        match n {
            Node::Char(c, fl) => {
                if fl & I != 0 {
                    let ascii = fl & A != 0;
                    Some(Atom::CharI(fold(*c, ascii), ascii))
                } else {
                    Some(Atom::Char(*c))
                }
            }
            Node::Any(fl) => Some(if fl & S != 0 { Atom::AnyNl } else { Atom::Any }),
            Node::Set(cs) => {
                self.sets.push(cs.clone());
                Some(Atom::Set(self.sets.len() - 1))
            }
            _ => None,
        }
    }

    fn emit(&mut self, n: &Node) -> Result<(), ReError> {
        if self.insts.len() > MAX_PROG {
            return Err(ReError::bare("pattern too large"));
        }
        match n {
            Node::Empty => {}
            Node::Char(..) | Node::Any(_) | Node::Set(_) => {
                if let Some(a) = self.atom(n) {
                    self.push(Inst::One(a));
                }
            }
            Node::Cat(v) => {
                for x in v {
                    self.emit(x)?;
                }
            }
            Node::Alt(v) => {
                let mut jumps: Vec<usize> = Vec::new();
                for (idx, b) in v.iter().enumerate() {
                    if idx + 1 < v.len() {
                        let s = self.push_split();
                        self.emit(b)?;
                        jumps.push(self.push(Inst::Jmp(0)));
                        let next = self.insts.len();
                        self.set_split(s, s + 1, next);
                    } else {
                        self.emit(b)?;
                    }
                }
                let end = self.insts.len();
                for j in jumps {
                    self.insts[j] = Inst::Jmp(end);
                }
            }
            Node::Group(None, b) => self.emit(b)?,
            Node::Group(Some(k), b) => {
                self.push(Inst::Open(*k));
                self.emit(b)?;
                self.push(Inst::Close(*k));
            }
            Node::Repeat { node, min, max, mode } => self.emit_repeat(node, *min, *max, *mode)?,
            Node::Assert(k, fl) => {
                self.push(Inst::Assert(*k, fl & A != 0));
            }
            Node::Backref(g, fl) => {
                self.memo_ok = false;
                self.push(Inst::Backref { g: *g, icase: fl & I != 0, ascii: fl & A != 0 });
            }
            Node::Look { behind, neg, node } => {
                self.memo_ok = false;
                let l = self.push(Inst::Look { behind: *behind, neg: *neg, end: 0 });
                self.emit(node)?;
                self.push(Inst::SubMatch);
                let end = self.insts.len();
                self.insts[l] = Inst::Look { behind: *behind, neg: *neg, end };
            }
            Node::Atomic(b) => {
                self.memo_ok = false;
                let l = self.push(Inst::Atomic { end: 0 });
                self.emit(b)?;
                self.push(Inst::SubMatch);
                let end = self.insts.len();
                self.insts[l] = Inst::Atomic { end };
            }
            Node::Cond { group, yes, no } => {
                self.memo_ok = false;
                let c = self.push(Inst::Cond { g: *group, else_pc: 0 });
                self.emit(yes)?;
                let j = self.push(Inst::Jmp(0));
                let else_pc = self.insts.len();
                self.emit(no)?;
                let end = self.insts.len();
                self.insts[c] = Inst::Cond { g: *group, else_pc };
                self.insts[j] = Inst::Jmp(end);
            }
        }
        Ok(())
    }

    fn emit_repeat(&mut self, node: &Node, min: usize, max: usize, mode: RepMode) -> Result<(), ReError> {
        if let Some(atom) = self.atom(node) {
            self.push(Inst::Rep { atom, min, max, mode });
            return Ok(());
        }
        if mode == RepMode::Possessive {
            self.memo_ok = false;
            let l = self.push(Inst::Atomic { end: 0 });
            self.emit_repeat(node, min, max, RepMode::Greedy)?;
            self.push(Inst::SubMatch);
            let end = self.insts.len();
            self.insts[l] = Inst::Atomic { end };
            return Ok(());
        }
        let greedy = mode == RepMode::Greedy;
        let can_empty = width(node).0 == 0;
        if can_empty {
            self.memo_ok = false;
        }
        if max == INF {
            if min == 0 {
                let l1 = self.push_split();
                let reg = if can_empty {
                    let r = self.new_reg();
                    self.push(Inst::Mark(r));
                    Some(r)
                } else {
                    None
                };
                self.emit(node)?;
                let ee = match reg {
                    Some(r) => Some(self.push(Inst::EmptyExit { reg: r, exit: 0 })),
                    None => None,
                };
                self.push(Inst::Jmp(l1));
                let l3 = self.insts.len();
                if greedy {
                    self.set_split(l1, l1 + 1, l3);
                } else {
                    self.set_split(l1, l3, l1 + 1);
                }
                if let (Some(e), Some(r)) = (ee, reg) {
                    self.insts[e] = Inst::EmptyExit { reg: r, exit: l3 };
                }
            } else {
                for _ in 0..min - 1 {
                    self.emit(node)?;
                }
                let l1 = self.insts.len();
                let reg = if can_empty {
                    let r = self.new_reg();
                    self.push(Inst::Mark(r));
                    Some(r)
                } else {
                    None
                };
                self.emit(node)?;
                let ee = match reg {
                    Some(r) => Some(self.push(Inst::EmptyExit { reg: r, exit: 0 })),
                    None => None,
                };
                let s = self.push_split();
                let l2 = self.insts.len();
                if greedy {
                    self.set_split(s, l1, l2);
                } else {
                    self.set_split(s, l2, l1);
                }
                if let (Some(e), Some(r)) = (ee, reg) {
                    self.insts[e] = Inst::EmptyExit { reg: r, exit: l2 };
                }
            }
        } else {
            for _ in 0..min {
                self.emit(node)?;
            }
            let mut splits: Vec<usize> = Vec::new();
            for _ in min..max {
                let s = self.push_split();
                splits.push(s);
                self.emit(node)?;
            }
            let end = self.insts.len();
            for s in splits {
                if greedy {
                    self.set_split(s, s + 1, end);
                } else {
                    self.set_split(s, end, s + 1);
                }
            }
        }
        Ok(())
    }
}

/// Padrão compilado.
pub struct Regex {
    prog: Vec<Inst>,
    sets: Vec<CharSet>,
    /// Quantidade de grupos de captura (sem contar o grupo 0).
    pub ngroups: usize,
    /// Grupos nomeados na ordem de definição.
    pub group_names: Vec<(String, usize)>,
    /// Flags efetivas (as passadas, as globais inline e `U` implícito em padrão `str`).
    pub flags: u32,
    nslots: usize,
    last_slot: usize,
    memo_ok: bool,
    nsplits: usize,
    first: Option<char>,
}

/// Compila `pattern` com `flags`.
pub fn compile(pattern: &[char], flags: u32) -> Result<Regex, ReError> {
    if flags & L != 0 {
        return Err(ReError::value("cannot use LOCALE flag with a str pattern"));
    }
    if flags & A != 0 && flags & U != 0 {
        return Err(ReError::value("ASCII and UNICODE flags are incompatible"));
    }
    let mut p = Parser { p: pattern, i: 0, fl: flags, global: 0, ngroups: 0, names: Vec::new(), open: Vec::new() };
    let node = p.parse_alt(true)?;
    if p.i < pattern.len() {
        return Err(ReError::at("unbalanced parenthesis", p.i));
    }
    let mut eff = flags | p.global;
    if eff & A != 0 && eff & U != 0 {
        return Err(ReError::value("ASCII and UNICODE flags are incompatible"));
    }
    if eff & A == 0 {
        eff |= U;
    }
    let ng = p.ngroups;
    let last_slot = 2 * (ng + 1);
    let mut c = Compiler {
        insts: Vec::new(),
        sets: Vec::new(),
        nregs: 0,
        nsplits: 0,
        reg_base: last_slot + 1,
        memo_ok: true,
    };
    c.emit(&node)?;
    c.push(Inst::Match);
    let first = match c.insts.first() {
        Some(Inst::One(Atom::Char(ch))) => Some(*ch),
        _ => None,
    };
    let nslots = last_slot + 1 + c.nregs;
    Ok(Regex {
        prog: c.insts,
        sets: c.sets,
        ngroups: ng,
        group_names: p.names,
        flags: eff,
        nslots,
        last_slot,
        memo_ok: c.memo_ok,
        nsplits: c.nsplits,
        first,
    })
}

// ---------------------------------------------------------------------------
// Casador
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Ancorado em `pos`.
    Match,
    /// Tenta cada posição a partir de `pos`.
    Search,
    /// Ancorado em `pos` e tem de terminar em `endpos`.
    Fullmatch,
}

/// Resultado de um casamento: spans por grupo (índice 0 = casamento inteiro) e `lastindex`.
#[derive(Clone, Debug)]
pub struct Captures {
    pub spans: Vec<Option<(usize, usize)>>,
    pub lastindex: Option<usize>,
}

enum Bt {
    Branch { pc: usize, pos: usize },
    Restore { idx: usize, old: usize },
    RepGreedy { next: usize, lo: usize, cur: usize },
    RepLazy { pc: usize, pos: usize, left: usize },
}

struct Matcher<'a> {
    re: &'a Regex,
    text: &'a [char],
    end: usize,
    full: bool,
    /// Se diferente de `UNSET`, um casamento que termine nesta posição é recusado (vazio sem avanço).
    adv_from: usize,
    steps: usize,
    memo: Vec<u64>,
}

impl<'a> Matcher<'a> {
    fn assert_ok(&self, kind: AssertKind, ascii: bool, pos: usize) -> bool {
        let t = self.text;
        let end = self.end;
        match kind {
            AssertKind::Bol | AssertKind::StartText => pos == 0,
            AssertKind::MBol => pos == 0 || t[pos - 1] == '\n',
            AssertKind::Eol => pos == end || (pos + 1 == end && t[pos] == '\n'),
            AssertKind::MEol => pos == end || t[pos] == '\n',
            AssertKind::EndText => pos == end,
            AssertKind::WordB | AssertKind::NotWordB => {
                let a = pos > 0 && is_word(t[pos - 1], ascii);
                let b = pos < end && is_word(t[pos], ascii);
                (a != b) == (kind == AssertKind::WordB)
            }
        }
    }

    fn run(&mut self, start_pc: usize, start_pos: usize, slots: &mut Vec<usize>) -> Option<usize> {
        let re = self.re;
        let text = self.text;
        let mut stack: Vec<Bt> = Vec::new();
        let mut pc = start_pc;
        let mut pos = start_pos;
        loop {
            let mut fail = false;
            match &re.prog[pc] {
                Inst::One(a) => {
                    if pos < self.end && atom_ok(&re.sets, *a, text[pos]) {
                        pos += 1;
                        pc += 1;
                    } else {
                        fail = true;
                    }
                }
                Inst::Rep { atom, min, max, mode } => {
                    let avail = self.end.saturating_sub(pos);
                    let limit = if *max == INF { avail } else { (*max).min(avail) };
                    if *mode == RepMode::Lazy {
                        let mut ok = *min <= limit;
                        if ok {
                            for k in 0..*min {
                                if !atom_ok(&re.sets, *atom, text[pos + k]) {
                                    ok = false;
                                    break;
                                }
                            }
                        }
                        if !ok {
                            fail = true;
                        } else {
                            let left = if *max == INF { INF } else { *max - *min };
                            if left > 0 {
                                stack.push(Bt::RepLazy { pc, pos: pos + *min, left });
                            }
                            pos += *min;
                            pc += 1;
                        }
                    } else {
                        let mut n = 0;
                        while n < limit && atom_ok(&re.sets, *atom, text[pos + n]) {
                            n += 1;
                        }
                        if n < *min {
                            fail = true;
                        } else {
                            let lo = pos + *min;
                            let cur = pos + n;
                            if *mode == RepMode::Greedy && cur > lo {
                                stack.push(Bt::RepGreedy { next: pc + 1, lo, cur });
                            }
                            pos = cur;
                            pc += 1;
                        }
                    }
                }
                Inst::Split { a, b, id } => {
                    self.steps += 1;
                    let mut pruned = false;
                    if re.memo_ok {
                        if self.memo.is_empty() && self.steps > MEMO_AFTER_STEPS {
                            if let Some(bits) = re.nsplits.checked_mul(self.end + 1) {
                                if bits <= MEMO_MAX_BITS {
                                    self.memo = vec![0u64; bits / 64 + 1];
                                }
                            }
                        }
                        if !self.memo.is_empty() {
                            let bit = *id * (self.end + 1) + pos;
                            let w = bit / 64;
                            let m = 1u64 << (bit % 64);
                            if self.memo[w] & m != 0 {
                                pruned = true;
                            } else {
                                self.memo[w] |= m;
                            }
                        }
                    }
                    if pruned {
                        fail = true;
                    } else {
                        stack.push(Bt::Branch { pc: *b, pos });
                        pc = *a;
                    }
                }
                Inst::Jmp(t) => pc = *t,
                Inst::Open(k) => {
                    stack.push(Bt::Restore { idx: 2 * *k, old: slots[2 * *k] });
                    slots[2 * *k] = pos;
                    pc += 1;
                }
                Inst::Close(k) => {
                    stack.push(Bt::Restore { idx: 2 * *k + 1, old: slots[2 * *k + 1] });
                    stack.push(Bt::Restore { idx: re.last_slot, old: slots[re.last_slot] });
                    slots[2 * *k + 1] = pos;
                    slots[re.last_slot] = *k;
                    pc += 1;
                }
                Inst::Assert(k, ascii) => {
                    if self.assert_ok(*k, *ascii, pos) {
                        pc += 1;
                    } else {
                        fail = true;
                    }
                }
                Inst::Backref { g, icase, ascii } => {
                    let (s, e) = (slots[2 * *g], slots[2 * *g + 1]);
                    if s == UNSET || e == UNSET || e < s {
                        fail = true;
                    } else {
                        let len = e - s;
                        if pos + len > self.end {
                            fail = true;
                        } else {
                            let mut ok = true;
                            for k in 0..len {
                                let (x, y) = (text[s + k], text[pos + k]);
                                let same = if *icase { fold(x, *ascii) == fold(y, *ascii) } else { x == y };
                                if !same {
                                    ok = false;
                                    break;
                                }
                            }
                            if ok {
                                pos += len;
                                pc += 1;
                            } else {
                                fail = true;
                            }
                        }
                    }
                }
                Inst::Look { behind, neg, end } => {
                    let saved = slots.clone();
                    let r = match behind {
                        Some(w) => {
                            if pos >= *w {
                                self.run(pc + 1, pos - *w, slots)
                            } else {
                                None
                            }
                        }
                        None => self.run(pc + 1, pos, slots),
                    };
                    match (r.is_some(), *neg) {
                        (true, false) => {
                            for i in 0..slots.len() {
                                if slots[i] != saved[i] {
                                    stack.push(Bt::Restore { idx: i, old: saved[i] });
                                }
                            }
                            pc = *end;
                        }
                        (true, true) => {
                            *slots = saved;
                            fail = true;
                        }
                        (false, false) => fail = true,
                        (false, true) => pc = *end,
                    }
                }
                Inst::Atomic { end } => {
                    let saved = slots.clone();
                    match self.run(pc + 1, pos, slots) {
                        Some(e) => {
                            for i in 0..slots.len() {
                                if slots[i] != saved[i] {
                                    stack.push(Bt::Restore { idx: i, old: saved[i] });
                                }
                            }
                            pos = e;
                            pc = *end;
                        }
                        None => fail = true,
                    }
                }
                Inst::SubMatch => return Some(pos),
                Inst::Mark(r) => {
                    stack.push(Bt::Restore { idx: *r, old: slots[*r] });
                    slots[*r] = pos;
                    pc += 1;
                }
                Inst::EmptyExit { reg, exit } => {
                    if slots[*reg] == pos {
                        pc = *exit;
                    } else {
                        pc += 1;
                    }
                }
                Inst::Cond { g, else_pc } => {
                    if slots[2 * *g] != UNSET && slots[2 * *g + 1] != UNSET {
                        pc += 1;
                    } else {
                        pc = *else_pc;
                    }
                }
                Inst::Match => {
                    if (self.full && pos != self.end) || pos == self.adv_from {
                        fail = true;
                    } else {
                        return Some(pos);
                    }
                }
            }
            if fail {
                loop {
                    match stack.pop() {
                        None => return None,
                        Some(Bt::Restore { idx, old }) => slots[idx] = old,
                        Some(Bt::Branch { pc: p, pos: q }) => {
                            pc = p;
                            pos = q;
                            break;
                        }
                        Some(Bt::RepGreedy { next, lo, cur }) => {
                            let np = cur - 1;
                            if np > lo {
                                stack.push(Bt::RepGreedy { next, lo, cur: np });
                            }
                            pc = next;
                            pos = np;
                            break;
                        }
                        Some(Bt::RepLazy { pc: rp, pos: q, left }) => {
                            if left > 0 && q < self.end {
                                if let Inst::Rep { atom, .. } = &re.prog[rp] {
                                    if atom_ok(&re.sets, *atom, text[q]) {
                                        stack.push(Bt::RepLazy { pc: rp, pos: q + 1, left: left - 1 });
                                        pc = rp + 1;
                                        pos = q + 1;
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

impl Regex {
    /// Índice do grupo de nome `name`.
    pub fn group_index(&self, name: &str) -> Option<usize> {
        self.group_names.iter().find(|(n, _)| n == name).map(|(_, k)| *k)
    }

    /// Executa o casamento sobre `text[..endpos]` a partir de `pos`. `must_advance` recusa um
    /// casamento vazio em `pos` (usado entre casamentos consecutivos de `finditer`, `sub`...).
    pub fn exec(&self, text: &[char], pos: usize, endpos: usize, mode: Mode, must_advance: bool) -> Option<Captures> {
        let end = endpos.min(text.len());
        if pos > end {
            return None;
        }
        let mut m = Matcher {
            re: self,
            text,
            end,
            full: mode == Mode::Fullmatch,
            adv_from: if must_advance { pos } else { UNSET },
            steps: 0,
            memo: Vec::new(),
        };
        let mut slots = vec![UNSET; self.nslots];
        let mut start = pos;
        loop {
            if mode == Mode::Search {
                if let Some(fc) = self.first {
                    while start < end && text[start] != fc {
                        start += 1;
                    }
                    if start >= end {
                        return None;
                    }
                }
            }
            for s in slots.iter_mut() {
                *s = UNSET;
            }
            slots[0] = start;
            if let Some(e) = m.run(0, start, &mut slots) {
                let mut spans: Vec<Option<(usize, usize)>> = Vec::with_capacity(self.ngroups + 1);
                spans.push(Some((start, e)));
                for k in 1..=self.ngroups {
                    let (a, b) = (slots[2 * k], slots[2 * k + 1]);
                    spans.push(if a != UNSET && b != UNSET && a <= b { Some((a, b)) } else { None });
                }
                let li = slots[self.last_slot];
                return Some(Captures { spans, lastindex: if li == UNSET { None } else { Some(li) } });
            }
            if mode != Mode::Search || start >= end {
                return None;
            }
            start += 1;
            m.adv_from = UNSET;
        }
    }
}

/// Estado de uma varredura de casamentos consecutivos (`findall`, `finditer`, `sub`, `split`),
/// com a regra do Python 3.7+: casamento vazio é aceito logo após um não vazio, mas não duas
/// vezes na mesma posição.
pub struct IterState {
    pub pos: usize,
    pub endpos: usize,
    pub must_advance: bool,
    pub done: bool,
}

impl IterState {
    pub fn new(pos: usize, endpos: usize) -> IterState {
        IterState { pos, endpos, must_advance: false, done: false }
    }

    pub fn next(&mut self, re: &Regex, text: &[char]) -> Option<Captures> {
        if self.done || self.pos > self.endpos {
            self.done = true;
            return None;
        }
        match re.exec(text, self.pos, self.endpos, Mode::Search, self.must_advance) {
            None => {
                self.done = true;
                None
            }
            Some(c) => {
                let (s, e) = c.spans[0].unwrap_or((self.pos, self.pos));
                self.pos = e;
                self.must_advance = e == s;
                Some(c)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Testes
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn cs(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    fn comp(p: &str, fl: u32) -> Regex {
        match compile(&cs(p), fl) {
            Ok(r) => r,
            Err(e) => panic!("padrão {p:?} recusado: {}", e.format(&cs(p))),
        }
    }

    fn err(p: &str) -> String {
        match compile(&cs(p), 0) {
            Ok(_) => panic!("padrão {p:?} deveria falhar"),
            Err(e) => e.format(&cs(p)),
        }
    }

    fn run_mode(p: &str, fl: u32, t: &str, mode: Mode) -> Option<(usize, usize)> {
        let r = comp(p, fl);
        let text = cs(t);
        r.exec(&text, 0, text.len(), mode, false).and_then(|c| c.spans[0])
    }

    fn search(p: &str, fl: u32, t: &str) -> Option<(usize, usize)> {
        run_mode(p, fl, t, Mode::Search)
    }

    fn spans(p: &str, t: &str) -> Vec<Option<(usize, usize)>> {
        let r = comp(p, 0);
        let text = cs(t);
        r.exec(&text, 0, text.len(), Mode::Search, false).map(|c| c.spans).unwrap_or_default()
    }

    #[test]
    fn literais_e_ponto() {
        assert_eq!(search("abc", 0, "xxabcxx"), Some((2, 5)));
        assert_eq!(search("a.c", 0, "a\nc"), None);
        assert_eq!(search("a.c", S, "a\nc"), Some((0, 3)));
        assert_eq!(search("a.c", 0, "abc"), Some((0, 3)));
    }

    #[test]
    fn ancoras() {
        assert_eq!(search("^b", 0, "a\nb"), None);
        assert_eq!(search("^b", M, "a\nb"), Some((2, 3)));
        assert_eq!(search("a$", 0, "a\n"), Some((0, 1)));
        assert_eq!(search("a$", 0, "a\nb"), None);
        assert_eq!(search("a$", M, "a\nb"), Some((0, 1)));
        assert_eq!(search("\\Aab\\Z", 0, "ab"), Some((0, 2)));
        assert_eq!(search("\\Aab\\Z", 0, "ab\n"), None);
        assert_eq!(search("\\bfoo\\b", 0, "a foo b"), Some((2, 5)));
        assert_eq!(search("\\Boo", 0, "foo"), Some((1, 3)));
    }

    #[test]
    fn classes_de_escape() {
        assert_eq!(search("\\d+", 0, "abc123def"), Some((3, 6)));
        assert_eq!(search("\\D+", 0, "12ab34"), Some((2, 4)));
        assert_eq!(search("\\d", 0, "x\u{663}y"), Some((1, 2)));
        assert_eq!(search("\\d", A, "x\u{663}y"), None);
        assert_eq!(search("\\w+", 0, "h\u{e9}llo w"), Some((0, 5)));
        assert_eq!(search("\\w+", A, "h\u{e9}llo w"), Some((0, 1)));
        assert_eq!(search("\\W+", 0, "ab, cd"), Some((2, 4)));
        assert_eq!(search("\\s+", 0, "a \t b"), Some((1, 4)));
        assert_eq!(search("\\S+", 0, "  ab  "), Some((2, 4)));
    }

    #[test]
    fn conjuntos() {
        assert_eq!(search("[a-c]+", 0, "xxabcabcxx"), Some((2, 8)));
        assert_eq!(search("[^a-c]+", 0, "abcxyzabc"), Some((3, 6)));
        assert_eq!(search("[\\d.]+", 0, "ab1.5cd"), Some((2, 5)));
        assert_eq!(search("[]a]+", 0, "x]a]y"), Some((1, 4)));
        assert_eq!(search("[a\\-z]+", 0, "b-az"), Some((1, 4)));
        assert_eq!(search("[a-]+", 0, "b-a-"), Some((1, 4)));
        assert_eq!(search("[\\x41-\\x43]+", 0, "xABCx"), Some((1, 4)));
        assert_eq!(search("(?i)[a-c]+", 0, "xABCx"), Some((1, 4)));
        assert_eq!(search("[^\\s]+", 0, " ab "), Some((1, 3)));
    }

    #[test]
    fn grupos_e_retrorreferencias() {
        assert_eq!(spans("(a)(b)?", "ac"), vec![Some((0, 1)), Some((0, 1)), None]);
        assert_eq!(spans("(?:a)(b)", "ab"), vec![Some((0, 2)), Some((1, 2))]);
        assert_eq!(search("(?P<x>a)(?P=x)", 0, "baab"), Some((1, 3)));
        assert_eq!(comp("(?P<x>a)(?P<y>b)", 0).group_names, vec![("x".to_string(), 1), ("y".to_string(), 2)]);
        assert_eq!(search("(a)\\1", 0, "aa"), Some((0, 2)));
        assert_eq!(search("(a)\\1", 0, "ab"), None);
        assert_eq!(search("(?i)(a)\\1", 0, "aA"), Some((0, 2)));
        assert_eq!(comp("(a)(b)(c)", 0).ngroups, 3);
    }

    #[test]
    fn lastindex() {
        let li = |p: &str, t: &str| {
            let r = comp(p, 0);
            let text = cs(t);
            r.exec(&text, 0, text.len(), Mode::Search, false).and_then(|c| c.lastindex)
        };
        assert_eq!(li("(a)(b)", "ab"), Some(2));
        assert_eq!(li("((a))", "a"), Some(1));
        assert_eq!(li("(a)|(b)", "b"), Some(2));
        assert_eq!(li("a", "a"), None);
    }

    #[test]
    fn lookaround() {
        assert_eq!(search("a(?=b)", 0, "ab"), Some((0, 1)));
        assert_eq!(search("a(?!b)", 0, "abac"), Some((2, 3)));
        assert_eq!(search("(?<=a)b", 0, "ab"), Some((1, 2)));
        assert_eq!(search("(?<!a)b", 0, "abxb"), Some((3, 4)));
        assert_eq!(search("(?<=ab)c", 0, "abc"), Some((2, 3)));
        assert_eq!(search("(?<=a)b", 0, "b"), None);
        assert_eq!(spans("(?=(a))a", "a"), vec![Some((0, 1)), Some((0, 1))]);
    }

    #[test]
    fn atomico_e_possessivo() {
        assert_eq!(search("(?>a+)ab", 0, "aaab"), None);
        assert_eq!(search("(?:a+)ab", 0, "aaab"), Some((0, 4)));
        assert_eq!(search("a*+a", 0, "aaa"), None);
        assert_eq!(search("a++b", 0, "aaab"), Some((0, 4)));
        assert_eq!(search("a?+a", 0, "a"), None);
        assert_eq!(search("(?:ab)*+c", 0, "ababc"), Some((0, 5)));
        assert_eq!(search("(?:ab)*+ab", 0, "ababab"), None);
    }

    #[test]
    fn quantificadores() {
        assert_eq!(search("a{2}", 0, "aaa"), Some((0, 2)));
        assert_eq!(search("a{2,}", 0, "aaa"), Some((0, 3)));
        assert_eq!(search("a{1,2}", 0, "aaa"), Some((0, 2)));
        assert_eq!(search("a{,2}", 0, "aaa"), Some((0, 2)));
        assert_eq!(search("a+?", 0, "aaa"), Some((0, 1)));
        assert_eq!(search("a{2,3}?", 0, "aaaa"), Some((0, 2)));
        assert_eq!(search("a??b", 0, "ab"), Some((0, 2)));
        assert_eq!(search("<.*?>", 0, "<a><b>"), Some((0, 3)));
        assert_eq!(search("<.*>", 0, "<a><b>"), Some((0, 6)));
        assert_eq!(search("ax{0}b", 0, "ab"), Some((0, 2)));
        assert_eq!(search("a{x}", 0, "a{x}"), Some((0, 4)));
        assert_eq!(search("a{", 0, "a{"), Some((0, 2)));
        assert_eq!(search("(?:ab){2}", 0, "ababab"), Some((0, 4)));
        assert_eq!(search("(?:ab){1,2}c", 0, "ababc"), Some((0, 5)));
        assert_eq!(search("(ab)+", 0, "xababx"), Some((1, 5)));
        assert_eq!(search("(ab)*?c", 0, "ababc"), Some((0, 5)));
    }

    #[test]
    fn alternancia() {
        assert_eq!(search("cat|dog", 0, "hotdog"), Some((3, 6)));
        assert_eq!(search("a|", 0, "b"), Some((0, 0)));
        assert_eq!(search("(?:a|ab)c", 0, "abc"), Some((0, 3)));
        assert_eq!(run_mode("a|ab", 0, "ab", Mode::Fullmatch), Some((0, 2)));
        assert_eq!(run_mode("a|ab", 0, "ab", Mode::Match), Some((0, 1)));
    }

    #[test]
    fn flags_inline() {
        assert_eq!(search("(?i)abc", 0, "xABC"), Some((1, 4)));
        assert_eq!(search("(?i:a)b", 0, "Ab"), Some((0, 2)));
        assert_eq!(search("(?i:a)b", 0, "AB"), None);
        assert_eq!(search("(?s).", 0, "\n"), Some((0, 1)));
        assert_eq!(search("(?x) a b # c", 0, "ab"), Some((0, 2)));
        assert_eq!(search("(?x)[ ]a", 0, " a"), Some((0, 2)));
        assert_eq!(search("(?a)\\w", 0, "\u{e9}"), None);
        assert_eq!(search("(?m)^b", 0, "a\nb"), Some((2, 3)));
        assert_eq!(search("(?i)a(?-i:b)", 0, "AB"), None);
        assert_eq!(search("(?i)a(?-i:b)", 0, "Ab"), Some((0, 2)));
        assert_eq!(search("a(?#comentario)b", 0, "ab"), Some((0, 2)));
        assert_eq!(search("abc", I, "ABC"), Some((0, 3)));
        assert_eq!(search("\u{e9}", I, "\u{c9}"), Some((0, 1)));
        assert_eq!(search("\u{e9}", I | A, "\u{c9}"), None);
        assert_eq!(comp("a", 0).flags, U);
        assert_eq!(comp("a", I).flags, I | U);
        assert_eq!(comp("(?i)a", 0).flags, I | U);
        assert_eq!(comp("a", A).flags, A);
    }

    #[test]
    fn escapes() {
        assert_eq!(search("\\x41", 0, "xA"), Some((1, 2)));
        assert_eq!(search("\\u00e9", 0, "\u{e9}"), Some((0, 1)));
        assert_eq!(search("\\U0001F600", 0, "\u{1F600}"), Some((0, 1)));
        assert_eq!(search("\\t", 0, "a\tb"), Some((1, 2)));
        assert_eq!(search("\\.", 0, "a.b"), Some((1, 2)));
        assert_eq!(search("\\101", 0, "A"), Some((0, 1)));
        assert_eq!(search("\\0", 0, "a\u{0}"), Some((1, 2)));
        assert_eq!(search("a\\|b", 0, "a|b"), Some((0, 3)));
        assert_eq!(search("\\n\\r\\f\\v\\a", 0, "\n\r\u{c}\u{b}\u{7}"), Some((0, 5)));
        assert_eq!(search("[\\b]", 0, "a\u{8}"), Some((1, 2)));
    }

    #[test]
    fn condicional() {
        assert_eq!(search("(a)?(?(1)b|c)", 0, "ab"), Some((0, 2)));
        assert_eq!(search("(a)?(?(1)b|c)", 0, "c"), Some((0, 1)));
        assert_eq!(search("^(a)?(?(1)b|c)$", 0, "ac"), None);
    }

    #[test]
    fn laco_vazio() {
        assert_eq!(search("(a*)*b", 0, "aab"), Some((0, 3)));
        assert_eq!(search("(a*)*", 0, "b"), Some((0, 0)));
        assert_eq!(search("(?:a?)*b", 0, "aab"), Some((0, 3)));
        assert_eq!(search("(a|)+x", 0, "aax"), Some((0, 3)));
        assert_eq!(search("(?:)*a", 0, "a"), Some((0, 1)));
    }

    #[test]
    fn pos_endpos() {
        let r = comp("^a", 0);
        let t = cs("ba");
        assert!(r.exec(&t, 1, 2, Mode::Search, false).is_none());
        let r = comp("a$", 0);
        let t = cs("aab");
        assert_eq!(r.exec(&t, 0, 2, Mode::Search, false).and_then(|c| c.spans[0]), Some((1, 2)));
        let r = comp("b", 0);
        assert_eq!(r.exec(&t, 2, 3, Mode::Match, false).and_then(|c| c.spans[0]), Some((2, 3)));
        assert!(r.exec(&t, 3, 2, Mode::Search, false).is_none());
        let r = comp("\\bb", 0);
        assert!(r.exec(&t, 2, 3, Mode::Match, false).is_none());
    }

    #[test]
    fn textos_longos_sem_estouro() {
        let big = "a".repeat(1_000_000);
        assert_eq!(search(".*", S, &big), Some((0, 1_000_000)));
        assert_eq!(search("a*", 0, &big), Some((0, 1_000_000)));
        assert_eq!(search(".*?$", S, &big), Some((0, 1_000_000)));
        let ab = format!("{}c", "ab".repeat(100_000));
        assert_eq!(search("(a|b)*c", 0, &ab), Some((0, 200_001)));
        assert_eq!(search("(?:a|b)*c", 0, &ab), Some((0, 200_001)));
        assert_eq!(search("(?:ab)+c", 0, &ab), Some((0, 200_001)));
        assert_eq!(search("[ab]+c", 0, &ab), Some((0, 200_001)));
    }

    #[test]
    fn retrocesso_catastrofico_contido() {
        let a = "a".repeat(40);
        assert_eq!(search("(a+)+b", 0, &a), None);
        let x = "x".repeat(40);
        assert_eq!(search("(x+x+)+y", 0, &x), None);
        assert_eq!(search("(?:a|aa)+b", 0, &a), None);
    }

    #[test]
    fn erros_de_padrao() {
        assert_eq!(err("["), "unterminated character set at position 0");
        assert_eq!(err("(" ), "missing ), unterminated subpattern at position 0");
        assert_eq!(err("*"), "nothing to repeat at position 0");
        assert_eq!(err("a|*"), "nothing to repeat at position 2");
        assert_eq!(err(")"), "unbalanced parenthesis at position 0");
        assert_eq!(err("a**"), "multiple repeat at position 2");
        assert_eq!(err("\\q"), "bad escape \\q at position 0");
        assert_eq!(err("a\\"), "bad escape (end of pattern) at position 1");
        assert_eq!(
            err("(?P<n>a)(?P<n>b)"),
            "redefinition of group name 'n' as group 2; was group 1 at position 12"
        );
        assert_eq!(err("(?P=x)"), "unknown group name 'x' at position 4");
        assert_eq!(err("\\1"), "invalid group reference 1 at position 1");
        assert_eq!(err("a{3,1}"), "min repeat greater than max repeat at position 2");
        assert_eq!(err("[z-a]"), "bad character range z-a at position 1");
        assert_eq!(err("a(?i)"), "global flags not at the start of the expression at position 1");
        assert_eq!(err("(?<=a+)b"), "look-behind requires fixed-width pattern");
        assert_eq!(err("^*"), "nothing to repeat at position 1");
        assert_eq!(err("\\x4"), "incomplete escape \\x4 at position 0");
        assert_eq!(err("(?z)"), "unknown extension ?z at position 1");
        assert_eq!(err("a\n("), "missing ), unterminated subpattern at position 2 (line 2, column 1)");
    }

    #[test]
    fn iteracao_de_casamentos_vazios() {
        let r = comp("x*", 0);
        let t = cs("abxd");
        let mut st = IterState::new(0, t.len());
        let mut found = Vec::new();
        while let Some(c) = st.next(&r, &t) {
            found.push(c.spans[0].unwrap());
        }
        assert_eq!(found, vec![(0, 0), (1, 1), (2, 3), (3, 3), (4, 4)]);
    }
}
