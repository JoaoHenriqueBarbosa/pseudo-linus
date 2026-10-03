//! Parser das regex do GNU nos quatro dialetos que interessam: BRE e ERE do grep 3.11 e do sed 4.9.
//!
//! Segue o `regcomp.c` do gnulib/glibc (o `peek_token`, `parse_expression`, `parse_dup_op` e
//! `parse_bracket_exp` de lá), com os bits de sintaxe que cada ferramenta liga:
//!
//! - grep `-G`: `RE_SYNTAX_GREP` = POSIX_BASIC + NEWLINE_ALT, sem CONTEXT_INVALID_DUP;
//! - grep `-E`: `RE_SYNTAX_EGREP` = POSIX_EXTENDED + INVALID_INTERVAL_ORD + NEWLINE_ALT, sem
//!   CONTEXT_INVALID_OPS;
//! - sed: POSIX_BASIC; sed `-E`: POSIX_EXTENDED sem UNMATCHED_RIGHT_PAREN_ORD. Antes de compilar,
//!   o sed converte `\n`, `\t`, `\xHH` etc. em caracteres (`normalize_text`).
//!
//! O grep ainda passa o padrão pelo `dfa.c`, que rejeita `[:space:]` fora de colchetes; isso também
//! está aqui.

use serde::{Deserialize, Serialize};

use crate::ast::{Assertion, Node, PosixClass, Regex, Set, SetItem};

/// Maior contagem aceita em `{n,m}` (`RE_DUP_MAX` do glibc).
pub const DUP_MAX: i64 = 0x7fff;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Dialect {
    GrepBre,
    GrepEre,
    SedBre,
    SedEre,
}

impl Dialect {
    pub fn ere(self) -> bool {
        matches!(self, Dialect::GrepEre | Dialect::SedEre)
    }

    pub fn sed(self) -> bool {
        matches!(self, Dialect::SedBre | Dialect::SedEre)
    }

    pub fn label(self) -> &'static str {
        match self {
            Dialect::GrepBre => "grep-bre",
            Dialect::GrepEre => "grep-ere",
            Dialect::SedBre => "sed-bre",
            Dialect::SedEre => "sed-ere",
        }
    }

    fn context_indep_ops(self) -> bool {
        self.ere()
    }

    fn context_invalid_ops(self) -> bool {
        self == Dialect::SedEre
    }

    fn context_invalid_dup(self) -> bool {
        self == Dialect::SedBre
    }

    fn invalid_interval_ord(self) -> bool {
        self == Dialect::GrepEre
    }

    fn unmatched_right_paren_ord(self) -> bool {
        self == Dialect::GrepEre
    }

    fn newline_alt(self) -> bool {
        !self.sed()
    }
}

/// Códigos de erro do `regcomp`, com a mensagem que o glibc imprime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorCode {
    Collate,
    Ctype,
    Escape,
    Subreg,
    Brack,
    Paren,
    Brace,
    BadBr,
    Range,
    BadRpt,
    Size,
    RParen,
    /// `[:space:]` fora de colchetes (erro do `dfa.c`, só no grep).
    ConfusingBracket,
}

impl ErrorCode {
    pub fn message(self) -> &'static str {
        match self {
            ErrorCode::Collate => "Invalid collation character",
            ErrorCode::Ctype => "Invalid character class name",
            ErrorCode::Escape => "Trailing backslash",
            ErrorCode::Subreg => "Invalid back reference",
            ErrorCode::Brack => "Unmatched [, [^, [:, [., or [=",
            ErrorCode::Paren => "Unmatched ( or \\(",
            ErrorCode::Brace => "Unmatched \\{",
            ErrorCode::BadBr => "Invalid content of \\{\\}",
            ErrorCode::Range => "Invalid range end",
            ErrorCode::BadRpt => "Invalid preceding regular expression",
            ErrorCode::Size => "Regular expression too big",
            ErrorCode::RParen => "Unmatched ) or \\)",
            ErrorCode::ConfusingBracket => "character class syntax is [[:space:]], not [:space:]",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub code: ErrorCode,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code.message())
    }
}

impl std::error::Error for ParseError {}

fn err<T>(code: ErrorCode) -> Result<T, ParseError> {
    Err(ParseError { code })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Char,
    Alt,
    Star,
    Plus,
    Question,
    OpenDup,
    CloseDup,
    OpenGroup,
    CloseGroup,
    OpenBracket,
    Period,
    Anchor(Assertion),
    Word(bool),
    Space(bool),
    Backref(usize),
    End,
    TrailingBackslash,
}

#[derive(Clone, Copy, Debug)]
struct Token {
    kind: Kind,
    /// O caractere "cru" do token (`opr.c` no glibc), usado quando um operador vira literal.
    raw: char,
    len: usize,
}

/// Analisa `pattern` no dialeto dado, como o `regcomp` do glibc (é o que dá os spans do `grep -o`,
/// os grupos do sed e os erros de sintaxe).
pub fn parse(pattern: &str, dialect: Dialect) -> Result<Regex, ParseError> {
    parse_view(pattern, dialect, false)
}

/// Como o `dfa.c` do grep enxerga o padrão: no `grep -E`, um operador de repetição em posição
/// inicial (`*a`, `{1}a`, `^*a`) se aplica à âncora anterior ou ao vazio, em vez de ser pulado como
/// no glibc. É o `dfa.c` que decide quais linhas casam quando não há backref; o `-o` usa o glibc.
pub fn parse_dfa_view(pattern: &str, dialect: Dialect) -> Result<Regex, ParseError> {
    parse_view(pattern, dialect, dialect == Dialect::GrepEre)
}

fn parse_view(pattern: &str, dialect: Dialect, dfa_view: bool) -> Result<Regex, ParseError> {
    let text: Vec<char> = if dialect.sed() { sed_normalize(pattern) } else { pattern.chars().collect() };
    let mut p = Parser { s: text, pos: 0, dialect, nsub: 0, completed: 0, dfa_view };
    let mut tok = p.fetch(false);
    let root = p.parse_reg_exp(&mut tok, 0)?;
    Ok(Regex { root, groups: p.nsub })
}

/// `normalize_text(..., TEXT_REGEX)` do sed 4.9 com `posixicity == POSIXLY_EXTENDED` (o padrão):
/// `\a \f \n \r \t \v`, `\dNNN`, `\oNNN`, `\xHH` e `\cX` viram o caractere; o resto fica como está.
pub fn sed_normalize(pattern: &str) -> Vec<char> {
    let s: Vec<char> = pattern.chars().collect();
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c != '\\' || i + 1 >= s.len() {
            out.push(c);
            i += 1;
            continue;
        }
        let e = s[i + 1];
        let simple = match e {
            'a' => Some('\x07'),
            'f' => Some('\x0c'),
            'n' | '\n' => Some('\n'),
            'r' => Some('\r'),
            't' => Some('\t'),
            'v' => Some('\x0b'),
            _ => None,
        };
        if let Some(ch) = simple {
            out.push(ch);
            i += 2;
            continue;
        }
        let base = match e {
            'd' => Some(10u32),
            'x' => Some(16),
            'o' => Some(8),
            _ => None,
        };
        if let Some(base) = base {
            // convert_number: no máximo os dígitos que cabem até 255.
            let mut n: u32 = 0;
            let mut max: u32 = 1;
            let mut j = i + 2;
            while j < s.len() && max <= 255 {
                match s[j].to_digit(16) {
                    Some(d) if d < base => {
                        n = n * base + d;
                        j += 1;
                        max *= base;
                    }
                    _ => break,
                }
            }
            if j == i + 2 {
                out.push(e);
            } else {
                out.push(char::from((n & 0xff) as u8));
            }
            i = j;
            continue;
        }
        if e == 'c' {
            if i + 2 < s.len() {
                let x = s[i + 2].to_ascii_uppercase() as u32 ^ 0x40;
                out.push(char::from_u32(x).unwrap_or('?'));
                i += 3;
            } else {
                out.push('\\');
                i += 2;
            }
            continue;
        }
        out.push('\\');
        out.push(e);
        i += 2;
    }
    out
}

struct Parser {
    s: Vec<char>,
    pos: usize,
    dialect: Dialect,
    nsub: usize,
    /// Bit n ligado quando o grupo n (1..=9) já fechou (`completed_bkref_map`).
    completed: u32,
    /// Ver [`parse_dfa_view`].
    dfa_view: bool,
}

impl Parser {
    fn peek_at(&self, i: usize, caret_here: bool) -> Token {
        let d = self.dialect;
        let ere = d.ere();
        let Some(&c) = self.s.get(i) else {
            return Token { kind: Kind::End, raw: '\0', len: 0 };
        };
        if c == '\\' {
            let Some(&c2) = self.s.get(i + 1) else {
                return Token { kind: Kind::TrailingBackslash, raw: '\\', len: 1 };
            };
            let kind = match c2 {
                '|' if !ere => Kind::Alt,
                '1'..='9' => Kind::Backref(c2 as usize - '0' as usize),
                '<' => Kind::Anchor(Assertion::WordStart),
                '>' => Kind::Anchor(Assertion::WordEnd),
                'b' => Kind::Anchor(Assertion::WordBoundary),
                'B' => Kind::Anchor(Assertion::NotWordBoundary),
                '`' => Kind::Anchor(Assertion::BufStart),
                '\'' => Kind::Anchor(Assertion::BufEnd),
                'w' => Kind::Word(false),
                'W' => Kind::Word(true),
                's' => Kind::Space(false),
                'S' => Kind::Space(true),
                '(' if !ere => Kind::OpenGroup,
                ')' if !ere => Kind::CloseGroup,
                '+' if !ere => Kind::Plus,
                '?' if !ere => Kind::Question,
                '{' if !ere => Kind::OpenDup,
                '}' if !ere => Kind::CloseDup,
                _ => Kind::Char,
            };
            return Token { kind, raw: c2, len: 2 };
        }
        let kind = match c {
            '\n' if d.newline_alt() => Kind::Alt,
            '|' if ere => Kind::Alt,
            '*' => Kind::Star,
            '+' if ere => Kind::Plus,
            '?' if ere => Kind::Question,
            '{' if ere => Kind::OpenDup,
            '}' if ere => Kind::CloseDup,
            '(' if ere => Kind::OpenGroup,
            ')' if ere => Kind::CloseGroup,
            '[' => Kind::OpenBracket,
            '.' => Kind::Period,
            '^' => {
                if !ere && !caret_here && i != 0 {
                    let prev = self.s[i - 1];
                    if d.newline_alt() && prev == '\n' {
                        Kind::Anchor(Assertion::LineStart)
                    } else {
                        Kind::Char
                    }
                } else {
                    Kind::Anchor(Assertion::LineStart)
                }
            }
            '$' => {
                if !ere && i + 1 != self.s.len() {
                    let next = self.peek_at(i + 1, false);
                    if next.kind != Kind::Alt && next.kind != Kind::CloseGroup {
                        Kind::Char
                    } else {
                        Kind::Anchor(Assertion::LineEnd)
                    }
                } else {
                    Kind::Anchor(Assertion::LineEnd)
                }
            }
            _ => Kind::Char,
        };
        Token { kind, raw: c, len: 1 }
    }

    fn fetch(&mut self, caret_here: bool) -> Token {
        let t = self.peek_at(self.pos, caret_here);
        self.pos += t.len;
        t
    }

    fn parse_reg_exp(&mut self, tok: &mut Token, nest: usize) -> Result<Node, ParseError> {
        let initial = self.completed;
        let first = self.parse_branch(tok, nest)?;
        if tok.kind != Kind::Alt {
            return Ok(first);
        }
        let mut alts = vec![first];
        while tok.kind == Kind::Alt {
            *tok = self.fetch(true);
            let branch = if tok.kind != Kind::Alt
                && tok.kind != Kind::End
                && (nest == 0 || tok.kind != Kind::CloseGroup)
            {
                let accumulated = self.completed;
                self.completed = initial;
                let b = self.parse_branch(tok, nest)?;
                self.completed |= accumulated;
                b
            } else {
                Node::Empty
            };
            alts.push(branch);
        }
        Ok(Node::Alt(alts))
    }

    fn parse_branch(&mut self, tok: &mut Token, nest: usize) -> Result<Node, ParseError> {
        let mut items = vec![self.parse_expression(tok, nest)?];
        while tok.kind != Kind::Alt && tok.kind != Kind::End && (nest == 0 || tok.kind != Kind::CloseGroup) {
            items.push(self.parse_expression(tok, nest)?);
        }
        items.retain(|n| *n != Node::Empty);
        Ok(match items.len() {
            0 => Node::Empty,
            1 => items.pop().expect("um item"),
            _ => Node::Concat(items),
        })
    }

    fn parse_expression(&mut self, tok: &mut Token, nest: usize) -> Result<Node, ParseError> {
        let d = self.dialect;
        let mut tree = match tok.kind {
            Kind::Char => Node::Char(tok.raw),
            Kind::Period => Node::Any,
            Kind::OpenGroup => self.parse_sub_exp(tok, nest + 1)?,
            Kind::OpenBracket => Node::Set(self.parse_bracket()?),
            Kind::Backref(n) => {
                if self.completed & (1 << n) == 0 {
                    return err(ErrorCode::Subreg);
                }
                Node::Backref(n)
            }
            Kind::OpenDup | Kind::Star | Kind::Plus | Kind::Question => {
                if tok.kind == Kind::OpenDup && d.context_invalid_dup() {
                    return err(ErrorCode::BadRpt);
                }
                if d.context_invalid_ops() && !d.context_invalid_dup() {
                    return err(ErrorCode::BadRpt);
                } else if d.context_indep_ops() && self.dfa_view {
                    // dfa.c: a repetição se aplica ao vazio (o intervalo é consumido inteiro).
                    while matches!(tok.kind, Kind::Star | Kind::Plus | Kind::Question | Kind::OpenDup) {
                        if let Dup::RolledBack(_) = self.parse_dup_op(Node::Empty, tok)? {
                            break;
                        }
                    }
                    return Ok(Node::Empty);
                } else if d.context_indep_ops() {
                    *tok = self.fetch(false);
                    return self.parse_expression(tok, nest);
                }
                Node::Char(tok.raw)
            }
            Kind::CloseGroup => {
                if !d.unmatched_right_paren_ord() {
                    return err(ErrorCode::RParen);
                }
                Node::Char(tok.raw)
            }
            Kind::CloseDup => Node::Char(tok.raw),
            Kind::Anchor(a) if self.dfa_view => Node::Assert(a),
            Kind::Anchor(a) => {
                *tok = self.fetch(false);
                return Ok(Node::Assert(a));
            }
            Kind::Word(neg) => Node::Set(Set::escape(PosixClass::Word, neg)),
            Kind::Space(neg) => Node::Set(Set::escape(PosixClass::Space, neg)),
            Kind::Alt | Kind::End => return Ok(Node::Empty),
            Kind::TrailingBackslash => return err(ErrorCode::Escape),
        };
        *tok = self.fetch(false);
        while matches!(tok.kind, Kind::Star | Kind::Plus | Kind::Question | Kind::OpenDup) {
            match self.parse_dup_op(tree, tok)? {
                Dup::Applied(t) => tree = t,
                Dup::RolledBack(t) => {
                    tree = t;
                    break;
                }
            }
            if d.context_invalid_dup() && matches!(tok.kind, Kind::Star | Kind::OpenDup) {
                return err(ErrorCode::BadRpt);
            }
        }
        Ok(tree)
    }

    fn parse_sub_exp(&mut self, tok: &mut Token, nest: usize) -> Result<Node, ParseError> {
        let cur = self.nsub;
        self.nsub += 1;
        *tok = self.fetch(true);
        let inner = if tok.kind == Kind::CloseGroup {
            Node::Empty
        } else {
            let t = self.parse_reg_exp(tok, nest)?;
            if tok.kind != Kind::CloseGroup {
                return err(ErrorCode::Paren);
            }
            t
        };
        if cur < 9 {
            self.completed |= 1 << (cur + 1);
        }
        Ok(Node::Group { index: cur + 1, inner: Box::new(inner) })
    }

    /// Lê um número de `{n,m}`: -1 se nada, -2 se inválido. Devolve também o token que encerrou.
    fn fetch_number(&mut self) -> (i64, Token) {
        let mut num: i64 = -1;
        loop {
            let t = self.fetch(false);
            if t.kind == Kind::End {
                return (-2, t);
            }
            if t.kind == Kind::CloseDup || t.raw == ',' {
                return (num, t);
            }
            num = if t.kind != Kind::Char || !t.raw.is_ascii_digit() || num == -2 {
                -2
            } else if num == -1 {
                t.raw as i64 - '0' as i64
            } else {
                (DUP_MAX + 1).min(num * 10 + t.raw as i64 - '0' as i64)
            };
        }
    }

    fn parse_dup_op(&mut self, elem: Node, tok: &mut Token) -> Result<Dup, ParseError> {
        let start_pos = self.pos;
        let start_tok = *tok;
        let (min, max);
        if tok.kind == Kind::OpenDup {
            let (mut start, t1) = self.fetch_number();
            let mut last = t1;
            if start == -1 {
                if t1.kind == Kind::Char && t1.raw == ',' {
                    start = 0;
                } else {
                    return err(ErrorCode::BadBr);
                }
            }
            let mut end: i64 = 0;
            if start != -2 {
                end = if last.kind == Kind::CloseDup {
                    start
                } else if last.kind == Kind::Char && last.raw == ',' {
                    let (e, t2) = self.fetch_number();
                    last = t2;
                    e
                } else {
                    -2
                };
            }
            if start == -2 || end == -2 {
                if !self.dialect.invalid_interval_ord() {
                    return err(if last.kind == Kind::End { ErrorCode::Brace } else { ErrorCode::BadBr });
                }
                self.pos = start_pos;
                *tok = Token { kind: Kind::Char, raw: start_tok.raw, len: start_tok.len };
                return Ok(Dup::RolledBack(elem));
            }
            if (end != -1 && start > end) || last.kind != Kind::CloseDup {
                return err(ErrorCode::BadBr);
            }
            if DUP_MAX < if end == -1 { start } else { end } {
                return err(ErrorCode::Size);
            }
            min = start as u32;
            max = if end == -1 { None } else { Some(end as u32) };
        } else {
            min = if tok.kind == Kind::Plus { 1 } else { 0 };
            max = if tok.kind == Kind::Question { Some(1) } else { None };
        }
        *tok = self.fetch(false);
        if elem == Node::Empty {
            return Ok(Dup::Applied(Node::Empty));
        }
        Ok(Dup::Applied(Node::Repeat { inner: Box::new(elem), min, max }))
    }

    fn bracket_char(&self, i: usize) -> Option<char> {
        self.s.get(i).copied()
    }

    /// Expressão de colchetes; `self.pos` está logo depois do `[`.
    fn parse_bracket(&mut self) -> Result<Set, ParseError> {
        let open = self.pos;
        let mut negated = false;
        if self.bracket_char(self.pos) == Some('^') {
            negated = true;
            self.pos += 1;
        }
        let mut items = Vec::new();
        let mut first = true;
        loop {
            let Some(c) = self.bracket_char(self.pos) else {
                return err(ErrorCode::Brack);
            };
            if c == ']' && !first {
                self.pos += 1;
                break;
            }
            let start = self.bracket_element(first)?;
            first = false;
            // Faixa?
            let is_class = matches!(start, Elem::Class(_) | Elem::Equiv(_));
            let mut range_end = None;
            if !is_class {
                match self.bracket_char(self.pos) {
                    None => return err(ErrorCode::Brack),
                    Some('-') => match self.bracket_char(self.pos + 1) {
                        None => return err(ErrorCode::Brack),
                        Some(']') => {}
                        Some(_) => {
                            self.pos += 1;
                            range_end = Some(self.bracket_element(true)?);
                        }
                    },
                    Some(_) => {}
                }
            }
            match (start, range_end) {
                (s, Some(e)) => {
                    let lo = s.range_point()?;
                    let hi = e.range_point()?;
                    // Em C.UTF-8 o glibc não tem sequência de colação pra caractere fora do ASCII:
                    // `[à-ú]` dá "Invalid collation character".
                    if !lo.is_ascii() || !hi.is_ascii() {
                        return err(ErrorCode::Collate);
                    }
                    if lo > hi {
                        return err(ErrorCode::Range);
                    }
                    items.push(SetItem::Range(lo, hi));
                }
                (Elem::Char(c), None) | (Elem::Coll(c), None) | (Elem::Equiv(c), None) => items.push(SetItem::Char(c)),
                (Elem::Class(k), None) => items.push(SetItem::Class(k)),
            }
        }
        if !self.dialect.sed() {
            self.check_confusing(open)?;
        }
        Ok(Set { negated, items, escape: false })
    }

    fn bracket_element(&mut self, accept_hyphen: bool) -> Result<Elem, ParseError> {
        let c = self.bracket_char(self.pos).expect("checado antes");
        if c == '['
            && let Some(delim @ (':' | '.' | '=')) = self.bracket_char(self.pos + 1)
        {
            self.pos += 2;
            let mut name = String::new();
            loop {
                let Some(ch) = self.bracket_char(self.pos) else {
                    return err(ErrorCode::Brack);
                };
                self.pos += 1;
                if self.pos >= self.s.len() {
                    return err(ErrorCode::Brack);
                }
                if ch == delim && self.bracket_char(self.pos) == Some(']') {
                    break;
                }
                name.push(ch);
            }
            self.pos += 1;
            return match delim {
                ':' => match PosixClass::from_name(&name) {
                    Some(k) => Ok(Elem::Class(k)),
                    None => err(ErrorCode::Ctype),
                },
                _ => {
                    let mut chars = name.chars();
                    match (chars.next(), chars.next()) {
                        (Some(ch), None) if ch.len_utf8() == 1 => {
                            Ok(if delim == '.' { Elem::Coll(ch) } else { Elem::Equiv(ch) })
                        }
                        _ => err(ErrorCode::Collate),
                    }
                }
            };
        }
        if c == '-' && !accept_hyphen && self.bracket_char(self.pos + 1) != Some(']') {
            return err(ErrorCode::Range);
        }
        self.pos += 1;
        Ok(Elem::Char(c))
    }

    /// `dfa.c`: `[:alpha:]` sozinho (começa e termina com `:`, sem faixas nem classes) é erro no grep.
    fn check_confusing(&self, open: usize) -> Result<(), ParseError> {
        let body: Vec<char> = self.s[open..self.pos - 1].to_vec();
        let body = if body.first() == Some(&'^') { &body[1..] } else { &body[..] };
        if body.len() >= 2
            && body[0] == ':'
            && body[body.len() - 1] == ':'
            && body.iter().any(|&c| c != ':')
            && !body.contains(&'[')
            && !body.windows(3).any(|w| w[1] == '-' && w[0] != ':' && w[2] != ':')
        {
            return err(ErrorCode::ConfusingBracket);
        }
        Ok(())
    }
}

enum Dup {
    Applied(Node),
    /// `{` inválido virou literal (grep -E); o laço de repetição para aqui.
    RolledBack(Node),
}

#[derive(Clone, Copy, Debug)]
enum Elem {
    Char(char),
    Coll(char),
    Equiv(char),
    Class(PosixClass),
}

impl Elem {
    fn range_point(self) -> Result<char, ParseError> {
        match self {
            Elem::Char(c) | Elem::Coll(c) => Ok(c),
            Elem::Equiv(_) | Elem::Class(_) => err(ErrorCode::Range),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str, d: Dialect) -> Node {
        parse(s, d).unwrap_or_else(|e| panic!("{s}: {e}")).root
    }

    fn e(s: &str, d: Dialect) -> ErrorCode {
        parse(s, d).expect_err(s).code
    }

    #[test]
    fn bre_basics() {
        use Dialect::*;
        assert_eq!(p("a*", GrepBre), Node::Repeat { inner: Box::new(Node::Char('a')), min: 0, max: None });
        // `*` no começo é literal em BRE.
        assert_eq!(p("*a", GrepBre), Node::Concat(vec![Node::Char('*'), Node::Char('a')]));
        // `^` no meio é literal em BRE, âncora depois de `\(` e `\|`.
        assert_eq!(p("a^", GrepBre), Node::Concat(vec![Node::Char('a'), Node::Char('^')]));
        assert!(matches!(p("\\(^a\\)", GrepBre), Node::Group { .. }));
        assert_eq!(p("a$b", GrepBre), Node::Concat(vec![Node::Char('a'), Node::Char('$'), Node::Char('b')]));
        assert_eq!(p("x\\{2,3\\}", GrepBre), Node::Repeat { inner: Box::new(Node::Char('x')), min: 2, max: Some(3) });
        assert_eq!(p("x\\{,3\\}", GrepBre), Node::Repeat { inner: Box::new(Node::Char('x')), min: 0, max: Some(3) });
        assert_eq!(e("a\\{1", GrepBre), ErrorCode::Brace);
        assert_eq!(e("a\\)", GrepBre), ErrorCode::RParen);
        assert_eq!(e("\\(a", GrepBre), ErrorCode::Paren);
        assert_eq!(e("\\1", GrepBre), ErrorCode::Subreg);
        assert_eq!(e("a\\", GrepBre), ErrorCode::Escape);
        assert_eq!(e("a**", SedBre), ErrorCode::BadRpt);
        assert!(parse("a**", GrepBre).is_ok());
        assert_eq!(e("\\{1\\}a", SedBre), ErrorCode::BadRpt);
    }

    #[test]
    fn ere_basics() {
        use Dialect::*;
        assert_eq!(p("*a", GrepEre), Node::Char('a'));
        assert_eq!(e("*a", SedEre), ErrorCode::BadRpt);
        assert_eq!(p("a{1", GrepEre), Node::Concat(vec![Node::Char('a'), Node::Char('{'), Node::Char('1')]));
        assert_eq!(e("a{1", SedEre), ErrorCode::Brace);
        assert_eq!(e("a{}", GrepEre), ErrorCode::BadBr);
        assert_eq!(p("a)", GrepEre), Node::Concat(vec![Node::Char('a'), Node::Char(')')]));
        assert_eq!(e("a)", SedEre), ErrorCode::RParen);
        assert_eq!(e("a{2,1}", GrepEre), ErrorCode::BadBr);
        assert_eq!(e("a{32768}", GrepEre), ErrorCode::Size);
        assert!(matches!(p("(a)\\1", GrepEre), Node::Concat(_)));
        assert_eq!(p("a|", GrepEre), Node::Alt(vec![Node::Char('a'), Node::Empty]));
    }

    #[test]
    fn brackets() {
        use Dialect::*;
        let set = |s: &str| match p(s, GrepEre) {
            Node::Set(s) => s,
            other => panic!("{other:?}"),
        };
        assert_eq!(set("[]a]").items, vec![SetItem::Char(']'), SetItem::Char('a')]);
        assert_eq!(set("[^]a]").items, vec![SetItem::Char(']'), SetItem::Char('a')]);
        assert_eq!(set("[a-]").items, vec![SetItem::Char('a'), SetItem::Char('-')]);
        assert_eq!(set("[[:digit:]x]").items, vec![SetItem::Class(PosixClass::Digit), SetItem::Char('x')]);
        assert_eq!(set("[\\]").items, vec![SetItem::Char('\\')]);
        assert_eq!(e("[z-a]", GrepEre), ErrorCode::Range);
        assert_eq!(e("[[:foo:]]", GrepEre), ErrorCode::Ctype);
        assert_eq!(e("[a", GrepEre), ErrorCode::Brack);
        assert_eq!(e("[:space:]", GrepEre), ErrorCode::ConfusingBracket);
        assert!(parse("[:space:]", SedEre).is_ok());
        assert_eq!(e("[a-b-c]", GrepEre), ErrorCode::Range);
    }

    #[test]
    fn sed_escapes() {
        assert_eq!(sed_normalize("a\\tb\\n"), vec!['a', '\t', 'b', '\n']);
        assert_eq!(sed_normalize("\\x41\\d066\\o103"), vec!['A', 'B', 'C']);
        assert_eq!(sed_normalize("\\.\\("), vec!['\\', '.', '\\', '(']);
        assert_eq!(p("a\\tb", Dialect::SedBre), Node::Concat(vec![Node::Char('a'), Node::Char('\t'), Node::Char('b')]));
    }
}
