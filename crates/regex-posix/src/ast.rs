//! Árvore de uma regex já analisada, independente do motor que vai executá-la.
//!
//! A forma segue a árvore do `regcomp.c` do glibc (alternação e concatenação na ordem do padrão,
//! repetição com mínimo e máximo, grupos numerados a partir de 1), pra que o motor de submatches
//! possa reproduzir as escolhas do glibc.

/// Uma unidade do padrão: um caractere UTF-8 válido ou um byte que não forma UTF-8 (que casa só
/// com ele mesmo, como no glibc em C.UTF-8).
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Unit {
    Char(char),
    Byte(u8),
}

impl Unit {
    pub fn as_char(self) -> Option<char> {
        match self {
            Unit::Char(c) => Some(c),
            Unit::Byte(_) => None,
        }
    }

    /// Byte único do glibc (`opr.c`): o caractere se for ASCII, o próprio byte se for inválido.
    pub fn is(self, c: char) -> bool {
        self == Unit::Char(c)
    }
}

/// Decodifica um padrão em unidades: UTF-8 válido vira caractere, cada byte inválido vira
/// [`Unit::Byte`].
pub fn decode(pattern: &[u8]) -> Vec<Unit> {
    let mut out = Vec::new();
    let mut rest = pattern;
    while !rest.is_empty() {
        match std::str::from_utf8(rest) {
            Ok(s) => {
                out.extend(s.chars().map(Unit::Char));
                break;
            }
            Err(e) => {
                let good = e.valid_up_to();
                let s = std::str::from_utf8(&rest[..good]).unwrap_or_default();
                out.extend(s.chars().map(Unit::Char));
                let bad = e.error_len().unwrap_or(rest.len() - good);
                for &b in &rest[good..good + bad] {
                    out.push(Unit::Byte(b));
                }
                rest = &rest[good + bad..];
            }
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    /// Casa o vazio (ramo vazio de alternação, grupo vazio).
    Empty,
    Lit(Unit),
    /// `.`
    Any,
    Set(Set),
    Assert(Assertion),
    /// Grupo de captura; `index` começa em 1.
    Group { index: usize, inner: Box<Node> },
    Concat(Vec<Node>),
    Alt(Vec<Node>),
    /// Repetição gulosa; `max: None` é ilimitado.
    Repeat { inner: Box<Node>, min: u32, max: Option<u32> },
    /// Referência a um grupo (1 a 9).
    Backref(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Assertion {
    /// `^`: início do texto (e depois de newline com `newline_anchor`).
    LineStart,
    /// `$`: fim do texto (e antes de newline com `newline_anchor`).
    LineEnd,
    /// `` \` ``: início do texto.
    BufStart,
    /// `\'`: fim do texto.
    BufEnd,
    /// `\b`
    WordBoundary,
    /// `\B`
    NotWordBoundary,
    /// `\<`
    WordStart,
    /// `\>`
    WordEnd,
}

/// Expressão de colchetes (ou `\w`, `\W`, `\s`, `\S`, que o glibc monta como colchetes).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Set {
    pub negated: bool,
    pub items: Vec<SetItem>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetItem {
    Unit(Unit),
    /// Faixa de caracteres (pontas ASCII, como o glibc exige em C.UTF-8).
    Range(char, char),
    /// Faixa de bytes inválidos (pontas são bytes soltos do padrão).
    ByteRange(u8, u8),
    Class(PosixClass),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PosixClass {
    Alpha,
    Digit,
    Alnum,
    Upper,
    Lower,
    Space,
    Blank,
    Punct,
    Print,
    Graph,
    Cntrl,
    Xdigit,
}

impl PosixClass {
    pub fn from_name(name: &str) -> Option<PosixClass> {
        Some(match name {
            "alpha" => PosixClass::Alpha,
            "digit" => PosixClass::Digit,
            "alnum" => PosixClass::Alnum,
            "upper" => PosixClass::Upper,
            "lower" => PosixClass::Lower,
            "space" => PosixClass::Space,
            "blank" => PosixClass::Blank,
            "punct" => PosixClass::Punct,
            "print" => PosixClass::Print,
            "graph" => PosixClass::Graph,
            "cntrl" => PosixClass::Cntrl,
            "xdigit" => PosixClass::Xdigit,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            PosixClass::Alpha => "alpha",
            PosixClass::Digit => "digit",
            PosixClass::Alnum => "alnum",
            PosixClass::Upper => "upper",
            PosixClass::Lower => "lower",
            PosixClass::Space => "space",
            PosixClass::Blank => "blank",
            PosixClass::Punct => "punct",
            PosixClass::Print => "print",
            PosixClass::Graph => "graph",
            PosixClass::Cntrl => "cntrl",
            PosixClass::Xdigit => "xdigit",
        }
    }
}

impl Set {
    /// `\w` e `\W`: `[_[:alnum:]]` e o complemento (o glibc ignora `HAT_LISTS_NOT_NEWLINE` aqui).
    pub fn word(negated: bool) -> Set {
        Set { negated, items: vec![SetItem::Class(PosixClass::Alnum), SetItem::Unit(Unit::Char('_'))] }
    }

    /// `\s` e `\S`: `[[:space:]]` e o complemento.
    pub fn space(negated: bool) -> Set {
        Set { negated, items: vec![SetItem::Class(PosixClass::Space)] }
    }
}

/// Percorre a árvore em pré-ordem.
pub fn walk<'a>(n: &'a Node, f: &mut dyn FnMut(&'a Node)) {
    f(n);
    match n {
        Node::Group { inner, .. } | Node::Repeat { inner, .. } => walk(inner, f),
        Node::Concat(v) | Node::Alt(v) => v.iter().for_each(|c| walk(c, f)),
        _ => {}
    }
}

pub fn has_backref(n: &Node) -> bool {
    let mut found = false;
    walk(n, &mut |x| found |= matches!(x, Node::Backref(_)));
    found
}

pub fn has_word_assertion(n: &Node) -> bool {
    let mut found = false;
    walk(n, &mut |x| {
        found |= matches!(
            x,
            Node::Assert(Assertion::WordBoundary | Assertion::NotWordBoundary | Assertion::WordStart | Assertion::WordEnd)
        )
    });
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_keeps_invalid_bytes() {
        assert_eq!(decode(b"a\xffb"), vec![Unit::Char('a'), Unit::Byte(0xff), Unit::Char('b')]);
        assert_eq!(decode("é".as_bytes()), vec![Unit::Char('é')]);
        assert_eq!(decode(b"\xc3"), vec![Unit::Byte(0xc3)]);
    }
}
