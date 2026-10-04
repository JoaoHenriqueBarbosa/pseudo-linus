//! Tradução da AST pra `regex_syntax::hir::Hir`, que o `regex-automata` compila (meta e DFA
//! preguiçoso). Só pra padrão sem referência: grupos viram concatenação (os submatches saem do
//! nosso motor, em [`crate::nfa`]).

use regex_syntax::hir::{Class, ClassBytes, ClassBytesRange, Hir, Look, Repetition};

use crate::ast::{Assertion, Node, Unit};
use crate::charclass::{CaseMode, CharSet, any_set, bracket_set, literal_set, without_separator};

/// O que muda a tradução além da AST.
#[derive(Clone, Copy, Debug)]
pub struct HirOptions {
    pub case: CaseMode,
    pub dot_newline: bool,
    pub dot_not_null: bool,
    /// `^` e `$` também casam junto de newline.
    pub newline_anchor: bool,
    /// Modo linha do grep: o separador nunca casa e as âncoras casam junto dele.
    pub separator: Option<u8>,
}

impl HirOptions {
    pub fn set(&self, cs: CharSet) -> CharSet {
        match self.separator {
            Some(sep) => without_separator(cs, sep),
            None => cs,
        }
    }
}

/// `None` quando a AST tem referência (o `regex-automata` não tem).
pub fn to_hir(node: &Node, o: &HirOptions) -> Option<Hir> {
    Some(match node {
        Node::Empty => Hir::empty(),
        Node::Lit(u) => {
            let is_separator = |c: char| c.is_ascii() && o.separator == Some(c as u8);
            if o.case == CaseMode::Sensitive
                && let Unit::Char(c) = u
                && !is_separator(*c)
            {
                let mut buf = [0u8; 4];
                Hir::literal(c.encode_utf8(&mut buf).as_bytes().to_vec())
            } else {
                set_hir(o.set(literal_set(*u, o.case)))
            }
        }
        Node::Any => set_hir(o.set(any_set(o.dot_newline, o.dot_not_null))),
        Node::Set(s) => set_hir(o.set(bracket_set(s, o.case))),
        Node::Assert(a) => Hir::look(look(*a, o)),
        Node::Group { inner, .. } => to_hir(inner, o)?,
        Node::Concat(items) => Hir::concat(items.iter().map(|n| to_hir(n, o)).collect::<Option<Vec<_>>>()?),
        Node::Alt(items) => Hir::alternation(items.iter().map(|n| to_hir(n, o)).collect::<Option<Vec<_>>>()?),
        Node::Repeat { inner, min, max } => {
            if let Some((a, required)) = repeated_assertion(node) {
                // Repetir uma asserção (visão do dfa.c: `^*`): zero vezes é vazio, uma ou mais é ela.
                return Some(if required { Hir::look(look(a, o)) } else { Hir::empty() });
            }
            let sub = to_hir(inner, o)?;
            Hir::repetition(Repetition { min: *min, max: *max, greedy: true, sub: Box::new(sub) })
        }
        Node::Backref(_) => return None,
    })
}

fn look(a: Assertion, o: &HirOptions) -> Look {
    let multi = o.newline_anchor || o.separator.is_some();
    match a {
        Assertion::LineStart if multi => Look::StartLF,
        Assertion::LineStart => Look::Start,
        Assertion::LineEnd if multi => Look::EndLF,
        Assertion::LineEnd => Look::End,
        Assertion::BufStart if o.separator.is_some() => Look::StartLF,
        Assertion::BufStart => Look::Start,
        Assertion::BufEnd if o.separator.is_some() => Look::EndLF,
        Assertion::BufEnd => Look::End,
        Assertion::WordBoundary => Look::WordUnicode,
        Assertion::NotWordBoundary => Look::WordUnicodeNegate,
        Assertion::WordStart => Look::WordStartUnicode,
        Assertion::WordEnd => Look::WordEndUnicode,
    }
}

/// `Repeat(Repeat(...(Assert(a))))`: a asserção e se ela precisa valer (todos os mínimos > 0).
pub fn repeated_assertion(n: &Node) -> Option<(Assertion, bool)> {
    match n {
        Node::Assert(a) => Some((*a, true)),
        Node::Repeat { inner, min, .. } => repeated_assertion(inner).map(|(a, req)| (a, req && *min > 0)),
        _ => None,
    }
}

fn set_hir(cs: CharSet) -> Hir {
    let chars = (!cs.chars.ranges().is_empty()).then(|| Hir::class(Class::Unicode(cs.chars.clone())));
    let bytes = cs.has_bytes().then(|| {
        let ranges = (0x80u16..=0xff)
            .filter(|&b| cs.bytes[b as usize])
            .map(|b| ClassBytesRange::new(b as u8, b as u8));
        Hir::class(Class::Bytes(ClassBytes::new(ranges)))
    });
    match (chars, bytes) {
        (Some(c), Some(b)) => Hir::alternation(vec![c, b]),
        (Some(c), None) => c,
        (None, Some(b)) => b,
        (None, None) => Hir::fail(),
    }
}
