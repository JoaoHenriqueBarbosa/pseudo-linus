//! Parser das regex do GNU, porte do `regcomp.c` do glibc 2.41 (`peek_token`, `parse_reg_exp`,
//! `parse_branch`, `parse_expression`, `parse_sub_exp`, `parse_dup_op`, `fetch_number`,
//! `parse_bracket_exp`, `parse_bracket_element`, `parse_bracket_symbol`), dirigido pelos bits de
//! [`Syntax`].
//!
//! Há duas visões do mesmo padrão:
//!
//! - [`View::Glibc`]: a do `regcomp`, que dá os spans do `grep -o`, os grupos do sed e os erros.
//!   Com `RE_ICASE` o glibc passa o padrão pra maiúsculas antes de analisar (o caractere depois de
//!   `\` e os nomes de classe ficam como estão), e é isso que faz `grep -i '[Z-a]'` dar "Invalid
//!   range end"; esta visão reproduz isso.
//! - [`View::Dfa`]: a do `dfa.c`, que decide quais linhas casam no grep quando não há
//!   referência. Diferenças: no ERE, operador de repetição depois de uma âncora se aplica à âncora
//!   (`^*a` casa `a` em qualquer lugar) e, no começo da expressão, ao vazio; e os avisos
//!   `* at start of expression` e afins saem daqui.

use crate::ast::{Assertion, Node, PosixClass, Set, SetItem, Unit};
use crate::error::{ErrorCode, Warning};
use crate::syntax::Syntax;

/// `RE_DUP_MAX` do glibc.
pub const DUP_MAX: i64 = 0x7fff;

/// Tamanho máximo do nome em `[:nome:]`, `[.x.]`, `[=x=]` (`BRACKET_NAME_BUF_SIZE`).
const BRACKET_NAME_BUF_SIZE: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Glibc,
    Dfa,
}

/// Resultado da análise.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parsed {
    pub root: Node,
    /// `re_nsub`: número de grupos.
    pub nsub: usize,
    /// Avisos do `dfa.c` (só na visão [`View::Dfa`]).
    pub warnings: Vec<Warning>,
    /// `[:alpha:]` fora de colchetes (só na visão [`View::Dfa`]).
    pub confusing_brackets: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tk {
    End,
    BackSlash,
    Char,
    Alt,
    /// Índice a partir de 0, como o `opr.idx` do glibc.
    Backref(usize),
    Anchor(Assertion),
    Word(bool),
    Space(bool),
    OpenSub,
    CloseSub,
    Plus,
    Question,
    Star,
    OpenDup,
    CloseDup,
    OpenBracket,
    Period,
}

#[derive(Clone, Copy, Debug)]
struct Token {
    kind: Tk,
    /// `opr.c`: o caractere do token (o seguinte à barra, nos escapes).
    c: Unit,
    len: usize,
}

/// Analisa `pattern` com os bits de `syntax`.
pub fn parse(pattern: &[Unit], syntax: Syntax, view: View) -> Result<Parsed, ErrorCode> {
    let icase = syntax.contains(Syntax::ICASE) && view == View::Glibc;
    let translated: Vec<Unit> = if icase { pattern.iter().map(|&u| upper_unit(u)).collect() } else { pattern.to_vec() };
    let mut p = Parser {
        s: &translated,
        raw: pattern,
        pos: 0,
        syntax,
        view,
        nsub: 0,
        completed: 0,
        laststart: true,
        warnings: Vec::new(),
        confusing: false,
    };
    let mut tok = p.fetch(syntax | Syntax::CARET_ANCHORS_HERE);
    let root = p.parse_reg_exp(&mut tok, 0)?.unwrap_or(Node::Empty);
    Ok(Parsed { root, nsub: p.nsub, warnings: p.warnings, confusing_brackets: p.confusing })
}

/// `towupper` de um caractere com mapeamento simples (o glibc em C.UTF-8 usa os mapeamentos simples
/// do Unicode; mapeamento pra mais de um caractere deixa o caractere como está).
pub fn to_upper(c: char) -> char {
    let mut it = c.to_uppercase();
    match (it.next(), it.next()) {
        (Some(u), None) => u,
        _ => c,
    }
}

fn upper_unit(u: Unit) -> Unit {
    match u {
        Unit::Char(c) => Unit::Char(to_upper(c)),
        b => b,
    }
}

struct Parser<'a> {
    /// Padrão como o glibc enxerga (`mbs`): em maiúsculas com `RE_ICASE`.
    s: &'a [Unit],
    /// Padrão original (`raw_mbs`), pros escapes e nomes de classe.
    raw: &'a [Unit],
    pos: usize,
    syntax: Syntax,
    view: View,
    nsub: usize,
    /// `completed_bkref_map`: bit n ligado quando o grupo de índice n (0..=8) já fechou.
    completed: u32,
    /// `lex.laststart` do `dfa.c` (só na visão do dfa).
    laststart: bool,
    warnings: Vec<Warning>,
    confusing: bool,
}

impl Parser<'_> {
    fn at(&self, i: usize) -> Option<Unit> {
        self.s.get(i).copied()
    }

    fn peek_token(&self, i: usize, syntax: Syntax) -> Token {
        let Some(c) = self.at(i) else {
            return Token { kind: Tk::End, c: Unit::Char('\0'), len: 0 };
        };
        if c.is('\\') {
            if i + 1 >= self.s.len() {
                return Token { kind: Tk::BackSlash, c, len: 1 };
            }
            // `re_string_peek_byte_case`: o caractere depois da barra é o original.
            let c2 = self.raw[i + 1];
            let gnu = !syntax.contains(Syntax::NO_GNU_OPS);
            let kind = match c2 {
                Unit::Char('|') if !syntax.contains(Syntax::LIMITED_OPS) && !syntax.contains(Syntax::NO_BK_VBAR) => Tk::Alt,
                Unit::Char(d @ '1'..='9') if !syntax.contains(Syntax::NO_BK_REFS) => Tk::Backref(d as usize - '1' as usize),
                Unit::Char('<') if gnu => Tk::Anchor(Assertion::WordStart),
                Unit::Char('>') if gnu => Tk::Anchor(Assertion::WordEnd),
                Unit::Char('b') if gnu => Tk::Anchor(Assertion::WordBoundary),
                Unit::Char('B') if gnu => Tk::Anchor(Assertion::NotWordBoundary),
                Unit::Char('w') if gnu => Tk::Word(false),
                Unit::Char('W') if gnu => Tk::Word(true),
                Unit::Char('s') if gnu => Tk::Space(false),
                Unit::Char('S') if gnu => Tk::Space(true),
                Unit::Char('`') if gnu => Tk::Anchor(Assertion::BufStart),
                Unit::Char('\'') if gnu => Tk::Anchor(Assertion::BufEnd),
                Unit::Char('(') if !syntax.contains(Syntax::NO_BK_PARENS) => Tk::OpenSub,
                Unit::Char(')') if !syntax.contains(Syntax::NO_BK_PARENS) => Tk::CloseSub,
                Unit::Char('+') if !syntax.contains(Syntax::LIMITED_OPS) && syntax.contains(Syntax::BK_PLUS_QM) => Tk::Plus,
                Unit::Char('?') if !syntax.contains(Syntax::LIMITED_OPS) && syntax.contains(Syntax::BK_PLUS_QM) => Tk::Question,
                Unit::Char('{') if syntax.contains(Syntax::INTERVALS) && !syntax.contains(Syntax::NO_BK_BRACES) => Tk::OpenDup,
                Unit::Char('}') if syntax.contains(Syntax::INTERVALS) && !syntax.contains(Syntax::NO_BK_BRACES) => Tk::CloseDup,
                _ => Tk::Char,
            };
            return Token { kind, c: c2, len: 2 };
        }
        let kind = match c {
            Unit::Char('\n') if syntax.contains(Syntax::NEWLINE_ALT) => Tk::Alt,
            Unit::Char('|') if !syntax.contains(Syntax::LIMITED_OPS) && syntax.contains(Syntax::NO_BK_VBAR) => Tk::Alt,
            Unit::Char('*') => Tk::Star,
            Unit::Char('+') if !syntax.contains(Syntax::LIMITED_OPS) && !syntax.contains(Syntax::BK_PLUS_QM) => Tk::Plus,
            Unit::Char('?') if !syntax.contains(Syntax::LIMITED_OPS) && !syntax.contains(Syntax::BK_PLUS_QM) => Tk::Question,
            Unit::Char('{') if syntax.contains(Syntax::INTERVALS) && syntax.contains(Syntax::NO_BK_BRACES) => Tk::OpenDup,
            Unit::Char('}') if syntax.contains(Syntax::INTERVALS) && syntax.contains(Syntax::NO_BK_BRACES) => Tk::CloseDup,
            Unit::Char('(') if syntax.contains(Syntax::NO_BK_PARENS) => Tk::OpenSub,
            Unit::Char(')') if syntax.contains(Syntax::NO_BK_PARENS) => Tk::CloseSub,
            Unit::Char('[') => Tk::OpenBracket,
            Unit::Char('.') => Tk::Period,
            Unit::Char('^') => {
                let anchor_here = syntax.intersects(Syntax::CONTEXT_INDEP_ANCHORS | Syntax::CARET_ANCHORS_HERE)
                    || i == 0
                    || (syntax.contains(Syntax::NEWLINE_ALT) && self.s[i - 1].is('\n'));
                if anchor_here { Tk::Anchor(Assertion::LineStart) } else { Tk::Char }
            }
            Unit::Char('$') => {
                if !syntax.contains(Syntax::CONTEXT_INDEP_ANCHORS) && i + 1 != self.s.len() {
                    let next = self.peek_token(i + 1, syntax);
                    if next.kind != Tk::Alt && next.kind != Tk::CloseSub {
                        Tk::Char
                    } else {
                        Tk::Anchor(Assertion::LineEnd)
                    }
                } else {
                    Tk::Anchor(Assertion::LineEnd)
                }
            }
            _ => Tk::Char,
        };
        Token { kind, c, len: 1 }
    }

    fn fetch(&mut self, syntax: Syntax) -> Token {
        let t = self.peek_token(self.pos, syntax);
        self.pos += t.len;
        t
    }

    fn parse_reg_exp(&mut self, tok: &mut Token, nest: usize) -> Result<Option<Node>, ErrorCode> {
        let initial = self.completed;
        let first = self.parse_branch(tok, nest)?;
        if tok.kind != Tk::Alt {
            return Ok(first);
        }
        let mut alts = vec![first.unwrap_or(Node::Empty)];
        while tok.kind == Tk::Alt {
            self.laststart = true;
            *tok = self.fetch(self.syntax | Syntax::CARET_ANCHORS_HERE);
            let branch = if tok.kind != Tk::Alt && tok.kind != Tk::End && (nest == 0 || tok.kind != Tk::CloseSub) {
                let accumulated = self.completed;
                self.completed = initial;
                let b = self.parse_branch(tok, nest)?;
                self.completed |= accumulated;
                b
            } else {
                None
            };
            alts.push(branch.unwrap_or(Node::Empty));
        }
        Ok(Some(Node::Alt(alts)))
    }

    fn parse_branch(&mut self, tok: &mut Token, nest: usize) -> Result<Option<Node>, ErrorCode> {
        let mut items = Vec::new();
        if let Some(n) = self.parse_expression(tok, nest)? {
            items.push(n);
        }
        while tok.kind != Tk::Alt && tok.kind != Tk::End && (nest == 0 || tok.kind != Tk::CloseSub) {
            if let Some(n) = self.parse_expression(tok, nest)? {
                items.push(n);
            }
        }
        Ok(match items.len() {
            0 => None,
            1 => items.pop(),
            _ => Some(Node::Concat(items)),
        })
    }

    fn literal(&self, tok: &Token) -> Node {
        Node::Lit(tok.c)
    }

    fn parse_expression(&mut self, tok: &mut Token, nest: usize) -> Result<Option<Node>, ErrorCode> {
        let syntax = self.syntax;
        let dfa = self.view == View::Dfa;
        let tree: Node = match tok.kind {
            Tk::Char => {
                self.laststart = false;
                self.literal(tok)
            }
            Tk::OpenSub => {
                self.laststart = true;
                let t = self.parse_sub_exp(tok, nest + 1)?;
                self.laststart = false;
                t
            }
            Tk::OpenBracket => {
                self.laststart = false;
                Node::Set(self.parse_bracket_exp()?)
            }
            Tk::Backref(idx) => {
                if self.completed & (1 << idx) == 0 {
                    return Err(ErrorCode::Subreg);
                }
                self.laststart = false;
                Node::Backref(idx + 1)
            }
            Tk::OpenDup | Tk::Star | Tk::Plus | Tk::Question => {
                if tok.kind == Tk::OpenDup && syntax.contains(Syntax::CONTEXT_INVALID_DUP) {
                    return Err(ErrorCode::BadRepeat);
                }
                if syntax.contains(Syntax::CONTEXT_INVALID_OPS) {
                    return Err(ErrorCode::BadRepeat);
                } else if syntax.contains(Syntax::CONTEXT_INDEP_OPS) {
                    if dfa {
                        // dfa.c: a repetição se aplica ao vazio.
                        while matches!(tok.kind, Tk::Star | Tk::Plus | Tk::Question | Tk::OpenDup) {
                            if let Dup::RolledBack(_) = self.parse_dup_op(None, tok)? {
                                break;
                            }
                        }
                        return Ok(None);
                    }
                    *tok = self.fetch(syntax);
                    return self.parse_expression(tok, nest);
                }
                self.laststart = false;
                self.literal(tok)
            }
            Tk::CloseSub => {
                if !syntax.contains(Syntax::UNMATCHED_RIGHT_PAREN_ORD) {
                    return Err(ErrorCode::RParen);
                }
                self.laststart = false;
                self.literal(tok)
            }
            Tk::CloseDup => {
                self.laststart = false;
                self.literal(tok)
            }
            Tk::Anchor(a) => {
                // Âncora não aceita repetição no glibc: `^*` é `^` seguido de `*`. No dfa.c ela é
                // um átomo que aceita repetição, mas a âncora não muda o `laststart`: no BRE, um
                // `*` ou `\{` logo depois de uma âncora no começo continua literal.
                if !dfa || (self.laststart && !syntax.contains(Syntax::CONTEXT_INDEP_OPS)) {
                    *tok = self.fetch(syntax);
                    return Ok(Some(Node::Assert(a)));
                }
                Node::Assert(a)
            }
            Tk::Period => {
                self.laststart = false;
                Node::Any
            }
            Tk::Word(neg) => {
                self.laststart = false;
                Node::Set(Set::word(neg))
            }
            Tk::Space(neg) => {
                self.laststart = false;
                Node::Set(Set::space(neg))
            }
            Tk::Alt | Tk::End => return Ok(None),
            Tk::BackSlash => return Err(ErrorCode::Escape),
        };
        *tok = self.fetch(syntax);
        let mut tree = Some(tree);
        while matches!(tok.kind, Tk::Star | Tk::Plus | Tk::Question | Tk::OpenDup) {
            match self.parse_dup_op(tree, tok)? {
                Dup::Applied(t) => tree = t,
                Dup::RolledBack(t) => {
                    tree = t;
                    break;
                }
            }
            if syntax.contains(Syntax::CONTEXT_INVALID_DUP) && matches!(tok.kind, Tk::Star | Tk::OpenDup) {
                return Err(ErrorCode::BadRepeat);
            }
        }
        Ok(tree)
    }

    fn parse_sub_exp(&mut self, tok: &mut Token, nest: usize) -> Result<Node, ErrorCode> {
        let cur = self.nsub;
        self.nsub += 1;
        *tok = self.fetch(self.syntax | Syntax::CARET_ANCHORS_HERE);
        let inner = if tok.kind == Tk::CloseSub {
            None
        } else {
            let t = self.parse_reg_exp(tok, nest)?;
            if tok.kind != Tk::CloseSub {
                return Err(ErrorCode::Paren);
            }
            t
        };
        if cur <= 8 {
            self.completed |= 1 << cur;
        }
        Ok(Node::Group { index: cur + 1, inner: Box::new(inner.unwrap_or(Node::Empty)) })
    }

    /// `fetch_number`: -1 se não houve dígito, -2 se inválido.
    fn fetch_number(&mut self, tok: &mut Token) -> i64 {
        let mut num: i64 = -1;
        loop {
            *tok = self.fetch(self.syntax);
            if tok.kind == Tk::End {
                return -2;
            }
            if tok.kind == Tk::CloseDup || tok.c.is(',') {
                return num;
            }
            let digit = match tok.c {
                Unit::Char(d @ '0'..='9') if tok.kind == Tk::Char => Some(d as i64 - '0' as i64),
                _ => None,
            };
            num = match digit {
                None => -2,
                Some(_) if num == -2 => -2,
                Some(d) if num == -1 => d,
                Some(d) => (DUP_MAX + 1).min(num * 10 + d),
            };
        }
    }

    fn parse_dup_op(&mut self, elem: Option<Node>, tok: &mut Token) -> Result<Dup, ErrorCode> {
        let start_pos = self.pos;
        let start_tok = *tok;
        let dfa = self.view == View::Dfa;
        let indep = self.syntax.contains(Syntax::CONTEXT_INDEP_OPS);
        let (min, max): (i64, i64);
        if tok.kind == Tk::OpenDup {
            let mut start = self.fetch_number(tok);
            if start == -1 {
                if tok.kind == Tk::Char && tok.c.is(',') {
                    start = 0;
                } else {
                    return Err(ErrorCode::BadBrace);
                }
            }
            let mut end: i64 = 0;
            if start != -2 {
                end = if tok.kind == Tk::CloseDup {
                    start
                } else if tok.kind == Tk::Char && tok.c.is(',') {
                    self.fetch_number(tok)
                } else {
                    -2
                };
            }
            if start == -2 || end == -2 {
                if !self.syntax.contains(Syntax::INVALID_INTERVAL_ORD) {
                    return Err(if tok.kind == Tk::End { ErrorCode::Brace } else { ErrorCode::BadBrace });
                }
                // Rollback: o `{` vira caractere comum.
                self.pos = start_pos;
                *tok = Token { kind: Tk::Char, c: start_tok.c, len: start_tok.len };
                return Ok(Dup::RolledBack(elem));
            }
            if (end != -1 && start > end) || tok.kind != Tk::CloseDup {
                return Err(ErrorCode::BadBrace);
            }
            if DUP_MAX < if end == -1 { start } else { end } {
                return Err(ErrorCode::Size);
            }
            if dfa {
                if self.laststart && indep {
                    self.warnings.push(Warning::BraceAtStart);
                }
                self.laststart = false;
            }
            min = start;
            max = end;
        } else {
            if dfa && self.laststart && indep {
                self.warnings.push(match tok.kind {
                    Tk::Star => Warning::StarAtStart,
                    Tk::Plus => Warning::PlusAtStart,
                    _ => Warning::QuestionAtStart,
                });
            }
            min = if tok.kind == Tk::Plus { 1 } else { 0 };
            max = if tok.kind == Tk::Question { 1 } else { -1 };
        }
        *tok = self.fetch(self.syntax);
        let Some(elem) = elem else { return Ok(Dup::Applied(None)) };
        if min == 0 && max == 0 {
            return Ok(Dup::Applied(None));
        }
        Ok(Dup::Applied(Some(Node::Repeat {
            inner: Box::new(elem),
            min: min as u32,
            max: if max == -1 { None } else { Some(max as u32) },
        })))
    }

    // ---------------------------------------------------------------- colchetes

    /// `peek_token_bracket`.
    fn peek_bracket(&self, i: usize) -> BTok {
        let Some(c) = self.at(i) else { return BTok { kind: BK::End, c: Unit::Char('\0'), len: 0 } };
        if c.is('\\') && self.syntax.contains(Syntax::BACKSLASH_ESCAPE_IN_LISTS) && i + 1 < self.s.len() {
            // A barra escapa o caractere seguinte; o token tem tamanho 1 depois de pular a barra.
            return BTok { kind: BK::Char, c: self.s[i + 1], len: 2 };
        }
        if c.is('[') {
            let c2 = self.at(i + 1);
            let kind = match c2 {
                Some(Unit::Char('.')) => Some(BK::OpenColl),
                Some(Unit::Char('=')) => Some(BK::OpenEquiv),
                Some(Unit::Char(':')) if self.syntax.contains(Syntax::CHAR_CLASSES) => Some(BK::OpenClass),
                _ => None,
            };
            return match kind {
                Some(k) => BTok { kind: k, c: c2.unwrap_or(c), len: 2 },
                None => BTok { kind: BK::Char, c, len: 1 },
            };
        }
        match c {
            Unit::Char(']') => BTok { kind: BK::Close, c, len: 1 },
            Unit::Char('^') => BTok { kind: BK::NonMatch, c, len: 1 },
            Unit::Char('-') => {
                // V7: `---` dentro de colchetes é um hífen só.
                if i + 2 < self.s.len() && self.s[i + 1].is('-') && self.s[i + 2].is('-') {
                    BTok { kind: BK::Char, c, len: 3 }
                } else {
                    BTok { kind: BK::Range, c, len: 1 }
                }
            }
            _ => BTok { kind: BK::Char, c, len: 1 },
        }
    }

    /// `parse_bracket_exp`; `self.pos` está logo depois do `[`.
    fn parse_bracket_exp(&mut self) -> Result<Set, ErrorCode> {
        let open = self.pos;
        let mut items: Vec<SetItem> = Vec::new();
        let mut non_match = false;
        let mut tok = self.peek_bracket(self.pos);
        if tok.kind == BK::End {
            return Err(ErrorCode::BadPattern);
        }
        if tok.kind == BK::NonMatch {
            non_match = true;
            if self.syntax.contains(Syntax::HAT_LISTS_NOT_NEWLINE) {
                items.push(SetItem::Unit(Unit::Char('\n')));
            }
            self.pos += tok.len;
            tok = self.peek_bracket(self.pos);
            if tok.kind == BK::End {
                return Err(ErrorCode::BadPattern);
            }
        }
        // O primeiro `]` é caractere comum.
        if tok.kind == BK::Close {
            tok.kind = BK::Char;
        }
        let mut first_round = true;
        loop {
            let start_elem = self.parse_bracket_element(tok, first_round)?;
            first_round = false;
            tok = self.peek_bracket(self.pos);
            let mut is_range = false;
            let mut tok2 = tok;
            if !matches!(start_elem, Elem::Class(_) | Elem::Equiv(_)) {
                if tok.kind == BK::End {
                    return Err(ErrorCode::Brack);
                }
                if tok.kind == BK::Range {
                    self.pos += tok.len;
                    tok2 = self.peek_bracket(self.pos);
                    if tok2.kind == BK::End {
                        return Err(ErrorCode::Brack);
                    }
                    if tok2.kind == BK::Close {
                        // O último `-` é caractere comum.
                        self.pos -= tok.len;
                        tok.kind = BK::Char;
                    } else {
                        is_range = true;
                    }
                }
            }
            if is_range {
                let end_elem = self.parse_bracket_element(tok2, true)?;
                tok = self.peek_bracket(self.pos);
                items.push(self.build_range(start_elem, end_elem)?);
            } else {
                match start_elem {
                    Elem::Unit(u) => items.push(SetItem::Unit(u)),
                    Elem::Coll(name) => {
                        if name.len() != 1 {
                            return Err(ErrorCode::Collate);
                        }
                        items.push(SetItem::Unit(name[0]));
                    }
                    Elem::Equiv(name) => {
                        // Sem regras de colação (C.UTF-8): só um byte.
                        if name.len() != 1 || !is_single_byte(name[0]) {
                            return Err(ErrorCode::Collate);
                        }
                        items.push(SetItem::Unit(name[0]));
                    }
                    Elem::Class(name) => {
                        let mut name = name;
                        if self.syntax.contains(Syntax::ICASE) && (name == "upper" || name == "lower") {
                            name = "alpha".to_string();
                        }
                        match PosixClass::from_name(&name) {
                            Some(k) => items.push(SetItem::Class(k)),
                            None => return Err(ErrorCode::Ctype),
                        }
                    }
                }
            }
            if tok.kind == BK::End {
                return Err(ErrorCode::Brack);
            }
            if tok.kind == BK::Close {
                break;
            }
        }
        self.pos += tok.len;
        if self.view == View::Dfa && self.colon_warning(open) {
            self.confusing = true;
        }
        Ok(Set { negated: non_match, items })
    }

    /// `parse_bracket_element`.
    fn parse_bracket_element(&mut self, tok: BTok, accept_hyphen: bool) -> Result<Elem, ErrorCode> {
        // Caractere multibyte é consumido direto, qualquer que seja o token.
        if let Some(Unit::Char(c)) = self.at(self.pos)
            && !c.is_ascii()
        {
            self.pos += 1;
            return Ok(Elem::Unit(Unit::Char(c)));
        }
        self.pos += tok.len;
        match tok.kind {
            BK::OpenColl | BK::OpenClass | BK::OpenEquiv => return self.parse_bracket_symbol(tok.kind),
            BK::Range if !accept_hyphen => {
                let t2 = self.peek_bracket(self.pos);
                if t2.kind != BK::Close {
                    return Err(ErrorCode::Range);
                }
            }
            _ => {}
        }
        Ok(Elem::Unit(tok.c))
    }

    /// `parse_bracket_symbol`: `[:nome:]`, `[.x.]`, `[=x=]`; `self.pos` está depois de `[:`.
    fn parse_bracket_symbol(&mut self, kind: BK) -> Result<Elem, ErrorCode> {
        let delim = match kind {
            BK::OpenColl => '.',
            BK::OpenEquiv => '=',
            _ => ':',
        };
        if self.pos >= self.s.len() {
            return Err(ErrorCode::Brack);
        }
        let mut name: Vec<Unit> = Vec::new();
        loop {
            if name.len() >= BRACKET_NAME_BUF_SIZE {
                return Err(ErrorCode::Brack);
            }
            // Nomes de classe saem do padrão original (`re_string_fetch_byte_case`).
            let ch = if kind == BK::OpenClass { self.raw[self.pos] } else { self.s[self.pos] };
            self.pos += 1;
            if self.pos >= self.s.len() {
                return Err(ErrorCode::Brack);
            }
            if ch.is(delim) && self.s[self.pos].is(']') {
                break;
            }
            name.push(ch);
        }
        self.pos += 1;
        Ok(match kind {
            BK::OpenColl => Elem::Coll(name),
            BK::OpenEquiv => Elem::Equiv(name),
            _ => Elem::Class(name.iter().filter_map(|u| u.as_char()).collect()),
        })
    }

    /// `build_range_exp` (versão do glibc, sem regras de colação: C.UTF-8).
    fn build_range(&self, start: Elem, end: Elem) -> Result<SetItem, ErrorCode> {
        let point = |e: &Elem| -> Result<Unit, ErrorCode> {
            match e {
                Elem::Class(_) | Elem::Equiv(_) => Err(ErrorCode::Range),
                Elem::Coll(name) if name.len() == 1 => Ok(name[0]),
                Elem::Coll(_) => Err(ErrorCode::Collate),
                Elem::Unit(u) => Ok(*u),
            }
        };
        let (lo, hi) = (point(&start)?, point(&end)?);
        // Caractere multibyte não tem sequência de colação em C.UTF-8, e byte fora do UTF-8 vira WEOF
        // no `btowc` do `parse_byte`: os dois dão REG_ECOLLATE.
        let seq = |u: Unit| -> Result<u32, ErrorCode> {
            match u {
                Unit::Char(c) if c.is_ascii() => Ok(c as u32),
                Unit::Byte(b) if b.is_ascii() => Ok(b as u32),
                Unit::Char(_) | Unit::Byte(_) => Err(ErrorCode::Collate),
            }
        };
        let (a, b) = (seq(lo)?, seq(hi)?);
        // A visão do `dfa.c` não traduz o padrão, mas a validade da faixa é a do `regcomp`, que com
        // RE_ICASE a vê em maiúsculas: `[a-Z]` passa (vira `[A-Z]`) e `[Z-a]` não.
        let (ca, cb) = if self.view == View::Dfa && self.syntax.contains(Syntax::ICASE) {
            (seq(upper_unit(lo))?, seq(upper_unit(hi))?)
        } else {
            (a, b)
        };
        if self.syntax.contains(Syntax::NO_EMPTY_RANGES) && ca > cb {
            return Err(ErrorCode::Range);
        }
        let (lo, hi, a, b) = if (ca, cb) != (a, b) && a > b { (upper_unit(lo), upper_unit(hi), ca, cb) } else { (lo, hi, a, b) };
        if a > b {
            // Faixa vazia sem NO_EMPTY_RANGES: não casa nada.
            return Ok(SetItem::ByteRange(1, 0));
        }
        Ok(match (lo, hi) {
            (Unit::Char(x), Unit::Char(y)) => SetItem::Range(x, y),
            _ => SetItem::ByteRange(a as u8, b as u8),
        })
    }

    /// Estado de aviso de dois-pontos do `parse_bracket_exp` do `dfa.c`: `[:alpha:]` sozinho
    /// (começa e termina com `:`, tem outro caractere, sem faixas nem classes).
    fn colon_warning(&self, open: usize) -> bool {
        self.colon_state(&self.s[open..self.pos]) == Some(7)
    }

    /// Laço do `parse_bracket_exp` do `dfa.c` reduzido ao `colon_warning_state`. `s` é o corpo do
    /// colchete depois do `[`, incluindo o `]` final. Bits: 1 = começa com `:`, 2 = termina com
    /// `:`, 4 = tem outro caractere, 8 = tem faixa, classe ou elemento de colação.
    fn colon_state(&self, s: &[Unit]) -> Option<u32> {
        let esc = self.syntax.contains(Syntax::BACKSLASH_ESCAPE_IN_LISTS);
        let classes = self.syntax.contains(Syntax::CHAR_CLASSES);
        let get = |i: usize| s.get(i).copied();
        let mut i = 0usize;
        let mut c = get(i)?;
        i += 1;
        if c.is('^') {
            c = get(i)?;
            i += 1;
        }
        let mut state: u32 = c.is(':') as u32;
        loop {
            let mut c1: Option<Unit> = None;
            state &= !2;
            let mut symbol = false;
            if c.is('[') {
                let x = get(i)?;
                i += 1;
                c1 = Some(x);
                if (x.is(':') && classes) || x.is('.') || x.is('=') {
                    loop {
                        let y = get(i)?;
                        i += 1;
                        if i >= s.len() || (y == x && get(i) == Some(Unit::Char(']'))) {
                            break;
                        }
                    }
                    // O `]` que fecha o nome.
                    get(i)?;
                    i += 1;
                    state |= 8;
                    c1 = Some(get(i)?);
                    i += 1;
                    symbol = true;
                }
            }
            if !symbol {
                if c.is('\\') && esc {
                    c = get(i)?;
                    i += 1;
                }
                if c1.is_none() {
                    c1 = Some(get(i)?);
                    i += 1;
                }
                let mut ranged = false;
                if c1 == Some(Unit::Char('-')) {
                    let mut c2 = get(i)?;
                    i += 1;
                    if c2.is('[') && get(i) == Some(Unit::Char('.')) {
                        c2 = Unit::Char(']');
                    }
                    if c2.is(']') {
                        // `[x-]`: o hífen fica como próximo caractere.
                        i -= 1;
                    } else {
                        if c2.is('\\') && esc {
                            c2 = get(i)?;
                            i += 1;
                        }
                        state |= 8;
                        c1 = Some(get(i)?);
                        i += 1;
                        ranged = c != c2;
                    }
                }
                if !ranged {
                    state |= if c.is(':') { 2 } else { 4 };
                }
            }
            c = c1?;
            if c.is(']') {
                break;
            }
        }
        Some(state)
    }
}

fn is_single_byte(u: Unit) -> bool {
    match u {
        Unit::Char(c) => c.is_ascii(),
        Unit::Byte(_) => true,
    }
}

enum Dup {
    Applied(Option<Node>),
    /// `{` inválido virou literal (egrep): o laço de repetição para aqui.
    RolledBack(Option<Node>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BK {
    End,
    Char,
    Close,
    NonMatch,
    Range,
    OpenColl,
    OpenEquiv,
    OpenClass,
}

#[derive(Clone, Copy, Debug)]
struct BTok {
    kind: BK,
    c: Unit,
    len: usize,
}

#[derive(Clone, Debug)]
enum Elem {
    Unit(Unit),
    Coll(Vec<Unit>),
    Equiv(Vec<Unit>),
    Class(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::decode;

    fn p(s: &str, syn: Syntax) -> Node {
        parse(&decode(s.as_bytes()), syn, View::Glibc).unwrap_or_else(|e| panic!("{s}: {e:?}")).root
    }

    fn e(s: &str, syn: Syntax) -> ErrorCode {
        parse(&decode(s.as_bytes()), syn, View::Glibc).expect_err(s)
    }

    fn lit(c: char) -> Node {
        Node::Lit(Unit::Char(c))
    }

    fn cat(s: &str) -> Node {
        Node::Concat(s.chars().map(lit).collect())
    }

    #[test]
    fn bre_basics() {
        let g = Syntax::GREP;
        let sed = Syntax::SED_BASIC;
        assert_eq!(p("a*", g), Node::Repeat { inner: Box::new(lit('a')), min: 0, max: None });
        assert_eq!(p("*a", g), cat("*a"));
        assert_eq!(p("a^", g), cat("a^"));
        assert!(matches!(p("\\(^a\\)", g), Node::Group { .. }));
        assert_eq!(p("a$b", g), cat("a$b"));
        assert_eq!(p("x\\{2,3\\}", g), Node::Repeat { inner: Box::new(lit('x')), min: 2, max: Some(3) });
        assert_eq!(p("x\\{,3\\}", g), Node::Repeat { inner: Box::new(lit('x')), min: 0, max: Some(3) });
        assert_eq!(e("a\\{1", g), ErrorCode::Brace);
        assert_eq!(e("a\\)", g), ErrorCode::RParen);
        assert_eq!(e("\\(a", g), ErrorCode::Paren);
        assert_eq!(e("\\1", g), ErrorCode::Subreg);
        assert_eq!(e("a\\", g), ErrorCode::Escape);
        assert_eq!(e("a**", sed), ErrorCode::BadRepeat);
        assert!(parse(&decode(b"a**"), g, View::Glibc).is_ok());
        assert_eq!(e("\\{1\\}a", sed), ErrorCode::BadRepeat);
        assert_eq!(e("[", g), ErrorCode::BadPattern);
        assert_eq!(e("[a", g), ErrorCode::Brack);
        assert_eq!(p("a\\{0\\}b", g), lit('b'));
    }

    #[test]
    fn ere_basics() {
        let g = Syntax::EGREP;
        let sed = Syntax::SED_EXTENDED;
        assert_eq!(p("*a", g), lit('a'));
        assert_eq!(e("*a", sed), ErrorCode::BadRepeat);
        assert_eq!(p("a{1", g), cat("a{1"));
        assert_eq!(e("a{1", sed), ErrorCode::Brace);
        assert_eq!(e("a{}", g), ErrorCode::BadBrace);
        assert_eq!(p("a)", g), cat("a)"));
        assert_eq!(e("a)", sed), ErrorCode::RParen);
        assert_eq!(e("a{2,1}", g), ErrorCode::BadBrace);
        assert_eq!(e("a{32768}", g), ErrorCode::Size);
        assert!(matches!(p("(a)\\1", g), Node::Concat(_)));
        assert_eq!(p("a|", g), Node::Alt(vec![lit('a'), Node::Empty]));
        assert_eq!(e("(a)|\\1", g), ErrorCode::Subreg);
    }

    #[test]
    fn brackets() {
        let g = Syntax::EGREP;
        let set = |s: &str| match p(s, g) {
            Node::Set(s) => s,
            other => panic!("{other:?}"),
        };
        let u = |c: char| SetItem::Unit(Unit::Char(c));
        assert_eq!(set("[]a]").items, vec![u(']'), u('a')]);
        assert_eq!(set("[^]a]").items, vec![u(']'), u('a')]);
        assert_eq!(set("[a-]").items, vec![u('a'), u('-')]);
        assert_eq!(set("[[:digit:]x]").items, vec![SetItem::Class(PosixClass::Digit), u('x')]);
        assert_eq!(set("[\\]").items, vec![u('\\')]);
        assert_eq!(set("[a---]").items, vec![u('a'), u('-')]);
        assert_eq!(e("[z-a]", g), ErrorCode::Range);
        assert_eq!(e("[[:foo:]]", g), ErrorCode::Ctype);
        assert_eq!(e("[a", g), ErrorCode::Brack);
        assert_eq!(e("[a-b-c]", g), ErrorCode::Range);
        assert_eq!(e("[à-ú]", g), ErrorCode::Collate);
        assert_eq!(e("[[=é=]]", g), ErrorCode::Collate);
        assert_eq!(parse(&decode(b"[\xe9-z]"), g, View::Glibc).expect_err("[\\351-z]"), ErrorCode::Collate);
        // -i: o padrão vai pra maiúsculas antes da análise.
        assert_eq!(e("[Z-a]", g | Syntax::ICASE), ErrorCode::Range);
        assert!(parse(&decode(b"[a-Z]"), g | Syntax::ICASE, View::Dfa).is_ok());
        assert_eq!(parse(&decode(b"[Z-a]"), g | Syntax::ICASE, View::Dfa).expect_err("[Z-a]"), ErrorCode::Range);
        assert!(parse(&decode(b"[Z-a]"), g, View::Glibc).is_ok());
        // awk: barra escapa dentro de colchetes.
        assert_eq!(set_of("[\\]a]", Syntax::GNU_AWK).items, vec![u(']'), u('a')]);
    }

    fn set_of(s: &str, syn: Syntax) -> Set {
        match parse(&decode(s.as_bytes()), syn, View::Glibc).unwrap().root {
            Node::Set(s) => s,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn dfa_view_and_warnings() {
        let g = Syntax::EGREP;
        let dfa = |s: &str| parse(&decode(s.as_bytes()), g, View::Dfa).unwrap();
        // ^* no ERE: o dfa.c aplica a repetição à âncora.
        let v = dfa("^*a");
        assert_eq!(v.warnings, vec![Warning::StarAtStart]);
        assert!(matches!(&v.root, Node::Concat(items) if matches!(items[0], Node::Repeat { .. })));
        assert_eq!(p("^*a", g), Node::Concat(vec![Node::Assert(Assertion::LineStart), lit('a')]));
        assert_eq!(dfa("*a").warnings, vec![Warning::StarAtStart]);
        assert_eq!(dfa("+a").warnings, vec![Warning::PlusAtStart]);
        assert_eq!(dfa("a|?b").warnings, vec![Warning::QuestionAtStart]);
        assert_eq!(dfa("{2}a").warnings, vec![Warning::BraceAtStart]);
        assert!(dfa("a*").warnings.is_empty());
        assert!(dfa("[:space:]").confusing_brackets);
        assert!(!dfa("[[:space:]]").confusing_brackets);
        assert!(!dfa("[:a-b:]").confusing_brackets);
        assert!(!dfa("[:]").confusing_brackets);
        let b = parse(&decode(b"*a"), Syntax::GREP, View::Dfa).unwrap();
        assert!(b.warnings.is_empty());
    }

    #[test]
    fn emacs_and_awk_syntaxes() {
        // emacs (find -regex): \( \| são grupo e alternação, + e ? operadores, sem classes nem {}.
        let em = Syntax::EMACS;
        assert!(matches!(p("a+", em), Node::Repeat { min: 1, .. }));
        assert!(matches!(p("\\(a\\|b\\)", em), Node::Group { .. }));
        assert_eq!(p("a{2}", em), cat("a{2}"));
        // [[:alpha:]] sem CHAR_CLASSES é colchete com '[', ':', 'a'... seguido de ']'.
        assert!(matches!(p("[[:alpha:]]", em), Node::Concat(_)));
        // gawk: sem CONTEXT_INDEP_OPS, '*' no começo é literal.
        assert_eq!(p("*a", Syntax::GNU_AWK), cat("*a"));
    }
}
