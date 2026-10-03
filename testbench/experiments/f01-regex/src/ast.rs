//! AST de uma regex GNU (BRE ou ERE, dialeto do grep ou do sed), independente do motor.

use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    /// Expressão vazia (ramo vazio de alternação, grupo vazio, `x{0}` removido).
    Empty,
    Char(char),
    /// `.`
    Any,
    Set(Set),
    Assert(Assertion),
    /// Grupo de captura; `index` começa em 1.
    Group { index: usize, inner: Box<Node> },
    Concat(Vec<Node>),
    Alt(Vec<Node>),
    Repeat { inner: Box<Node>, min: u32, max: Option<u32> },
    Backref(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Assertion {
    /// `^` (no grep e no sed, início da linha).
    LineStart,
    /// `$`
    LineEnd,
    /// `` \` ``
    BufStart,
    /// `\'`
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Set {
    pub negated: bool,
    pub items: Vec<SetItem>,
    /// Veio de `\w`, `\W`, `\s` ou `\S` (extensão GNU), não de uma expressão de colchetes.
    pub escape: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetItem {
    Char(char),
    Range(char, char),
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
    /// `\w` do GNU: `[_[:alnum:]]`.
    Word,
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
            PosixClass::Word => "word",
        }
    }

    /// Faixas ASCII equivalentes (o que a classe significa na parte ASCII de C.UTF-8).
    pub fn ascii_ranges(self) -> &'static [(char, char)] {
        match self {
            PosixClass::Alpha => &[('A', 'Z'), ('a', 'z')],
            PosixClass::Digit => &[('0', '9')],
            PosixClass::Alnum => &[('0', '9'), ('A', 'Z'), ('a', 'z')],
            PosixClass::Upper => &[('A', 'Z')],
            PosixClass::Lower => &[('a', 'z')],
            PosixClass::Space => &[('\t', '\r'), (' ', ' ')],
            PosixClass::Blank => &[('\t', '\t'), (' ', ' ')],
            PosixClass::Punct => &[('!', '/'), (':', '@'), ('[', '`'), ('{', '~')],
            PosixClass::Print => &[(' ', '~')],
            PosixClass::Graph => &[('!', '~')],
            PosixClass::Cntrl => &[('\0', '\x1f'), ('\x7f', '\x7f')],
            PosixClass::Xdigit => &[('0', '9'), ('A', 'F'), ('a', 'f')],
            PosixClass::Word => &[('0', '9'), ('A', 'Z'), ('_', '_'), ('a', 'z')],
        }
    }

    /// Pertinência com a semântica do glibc em C.UTF-8 (aproximação: ASCII exato, Unicode pelas
    /// propriedades do Rust). Usada pelo gerador de amostras, não pelos motores.
    pub fn contains(self, c: char) -> bool {
        if c.is_ascii() {
            return self.ascii_ranges().iter().any(|&(a, b)| a <= c && c <= b);
        }
        match self {
            PosixClass::Alpha => c.is_alphabetic(),
            PosixClass::Alnum | PosixClass::Word => c.is_alphanumeric(),
            PosixClass::Upper => c.is_uppercase(),
            PosixClass::Lower => c.is_lowercase(),
            PosixClass::Space => c.is_whitespace() && !matches!(c, '\u{a0}' | '\u{2007}' | '\u{202f}'),
            PosixClass::Blank => c.is_whitespace() && !matches!(c, '\u{a0}' | '\u{2007}' | '\u{202f}' | '\u{2028}' | '\u{2029}'),
            PosixClass::Print => !c.is_control(),
            PosixClass::Graph => !c.is_control() && !c.is_whitespace(),
            PosixClass::Punct => !c.is_control() && !c.is_whitespace() && !c.is_alphanumeric(),
            PosixClass::Cntrl => c.is_control(),
            PosixClass::Digit | PosixClass::Xdigit => false,
        }
    }
}

impl Set {
    /// `\w`, `\W`, `\s`, `\S`.
    pub fn escape(class: PosixClass, negated: bool) -> Set {
        Set { negated, items: vec![SetItem::Class(class)], escape: true }
    }

    /// Pertinência aproximada (ver [`PosixClass::contains`]).
    pub fn contains(&self, c: char, icase: bool) -> bool {
        let test = |c: char| {
            self.items.iter().any(|item| match *item {
                SetItem::Char(x) => x == c,
                SetItem::Range(a, b) => a <= c && c <= b,
                SetItem::Class(k) => k.contains(c),
            })
        };
        let mut hit = test(c);
        if icase && !hit {
            hit = c.to_lowercase().any(test) || c.to_uppercase().any(test);
        }
        hit != self.negated
    }
}

/// Uma regex já analisada: AST mais metadados.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Regex {
    pub root: Node,
    /// Número de grupos de captura.
    pub groups: usize,
}

/// Classes de regex usadas pra quebrar a concordância por tipo de construção.
pub fn features(re: &Regex) -> BTreeSet<&'static str> {
    let mut out = BTreeSet::new();
    walk(&re.root, &mut |n, parent_repeat| {
        match n {
            Node::Char(c) if !c.is_ascii() => {
                out.insert("non-ascii");
            }
            Node::Set(s) => {
                if s.escape {
                    out.insert("gnu-escape");
                } else {
                    out.insert("bracket");
                    if s.items.iter().any(|i| matches!(i, SetItem::Class(_))) {
                        out.insert("posix-class");
                    }
                }
                if s.items.iter().any(|i| match i {
                    SetItem::Char(c) => !c.is_ascii(),
                    SetItem::Range(a, b) => !a.is_ascii() || !b.is_ascii(),
                    SetItem::Class(_) => false,
                }) {
                    out.insert("non-ascii");
                }
            }
            Node::Assert(a) => {
                out.insert(match a {
                    Assertion::LineStart | Assertion::LineEnd => "anchor",
                    Assertion::BufStart | Assertion::BufEnd => "buffer-anchor",
                    _ => "word-boundary",
                });
            }
            Node::Group { .. } => {
                out.insert("group");
            }
            Node::Alt(_) => {
                out.insert("alternation");
            }
            Node::Repeat { min, max, inner } => {
                let simple = matches!((min, max), (0, None) | (1, None) | (0, Some(1)));
                out.insert(if simple { "quantifier" } else { "interval" });
                if parent_repeat || matches!(**inner, Node::Repeat { .. }) {
                    out.insert("nested-repeat");
                }
            }
            Node::Backref(_) => {
                out.insert("backref");
            }
            _ => {}
        }
    });
    if out.is_empty() || is_literal(&re.root) {
        out.insert("literal");
    }
    out
}

fn is_literal(n: &Node) -> bool {
    match n {
        Node::Char(_) | Node::Empty => true,
        Node::Concat(v) => v.iter().all(is_literal),
        _ => false,
    }
}

/// Percorre a árvore; o segundo argumento diz se o nó está dentro de uma repetição.
pub fn walk<'a>(n: &'a Node, f: &mut dyn FnMut(&'a Node, bool)) {
    fn go<'a>(n: &'a Node, inside_repeat: bool, f: &mut dyn FnMut(&'a Node, bool)) {
        f(n, inside_repeat);
        match n {
            Node::Group { inner, .. } => go(inner, inside_repeat, f),
            Node::Repeat { inner, .. } => go(inner, true, f),
            Node::Concat(v) | Node::Alt(v) => v.iter().for_each(|c| go(c, inside_repeat, f)),
            _ => {}
        }
    }
    go(n, false, f)
}

pub fn uses_backref(re: &Regex) -> bool {
    let mut found = false;
    walk(&re.root, &mut |n, _| found |= matches!(n, Node::Backref(_)));
    found
}
