//! Tipos compartilhados do porte do libyaml 0.2.5: marcas, tokens, eventos e erros.

use crate::vm::PyException;

/// Posição no fluxo, em caracteres (o `index` do libyaml conta caracteres, não bytes).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mark {
    pub index: usize,
    pub line: usize,
    pub column: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Utf16Le,
    Utf16Be,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScalarStyle {
    Any,
    Plain,
    SingleQuoted,
    DoubleQuoted,
    Literal,
    Folded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollectionStyle {
    Any,
    Block,
    Flow,
}

/// Falha do leitor, do scanner, do parser ou do `read` do programa.
pub enum YamlError {
    Reader { problem: &'static str, offset: usize, value: i64 },
    /// Erro do scanner (`from_parser == false`) ou do parser; `context` ausente quando o C passa NULL.
    Marked {
        from_parser: bool,
        context: Option<&'static str>,
        context_mark: Mark,
        problem: &'static str,
        problem_mark: Mark,
    },
    Py(PyException),
}

impl YamlError {
    pub fn scanner(context: Option<&'static str>, context_mark: Mark, problem: &'static str, problem_mark: Mark) -> YamlError {
        YamlError::Marked { from_parser: false, context, context_mark, problem, problem_mark }
    }

    pub fn parser(context: Option<&'static str>, context_mark: Mark, problem: &'static str, problem_mark: Mark) -> YamlError {
        YamlError::Marked { from_parser: true, context, context_mark, problem, problem_mark }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TT {
    StreamStart,
    StreamEnd,
    VersionDirective,
    TagDirective,
    DocumentStart,
    DocumentEnd,
    BlockSequenceStart,
    BlockMappingStart,
    BlockEnd,
    FlowSequenceStart,
    FlowSequenceEnd,
    FlowMappingStart,
    FlowMappingEnd,
    BlockEntry,
    FlowEntry,
    Key,
    Value,
    Alias,
    Anchor,
    Tag,
    Scalar,
}

/// Token do scanner. Os campos de dados são reaproveitados pelo tipo: `a` guarda o alias, a âncora,
/// o valor do escalar, o handle da tag ou da diretiva; `b` guarda o sufixo da tag ou o prefixo da
/// diretiva.
#[derive(Clone, Debug)]
pub struct Token {
    pub ty: TT,
    pub start: Mark,
    pub end: Mark,
    pub a: String,
    pub b: String,
    pub style: ScalarStyle,
    pub encoding: Encoding,
    pub major: i64,
    pub minor: i64,
}

impl Token {
    pub fn new(ty: TT, start: Mark, end: Mark) -> Token {
        Token { ty, start, end, a: String::new(), b: String::new(), style: ScalarStyle::Any, encoding: Encoding::Utf8, major: 0, minor: 0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ET {
    StreamStart,
    StreamEnd,
    DocumentStart,
    DocumentEnd,
    Alias,
    Scalar,
    SequenceStart,
    SequenceEnd,
    MappingStart,
    MappingEnd,
}

/// Evento do parser (saída) e do emissor (entrada).
#[derive(Clone, Debug)]
pub struct Event {
    pub ty: ET,
    pub start: Mark,
    pub end: Mark,
    pub encoding: Encoding,
    pub version: Option<(i64, i64)>,
    pub tags: Vec<(String, String)>,
    /// `implicit` de DOCUMENT-START, DOCUMENT-END, SEQUENCE-START e MAPPING-START.
    pub implicit: bool,
    pub anchor: Option<String>,
    pub tag: Option<String>,
    pub value: String,
    pub plain_implicit: bool,
    pub quoted_implicit: bool,
    pub style: ScalarStyle,
    pub collection_style: CollectionStyle,
}

impl Event {
    pub fn new(ty: ET, start: Mark, end: Mark) -> Event {
        Event {
            ty,
            start,
            end,
            encoding: Encoding::Utf8,
            version: None,
            tags: Vec::new(),
            implicit: false,
            anchor: None,
            tag: None,
            value: String::new(),
            plain_implicit: false,
            quoted_implicit: false,
            style: ScalarStyle::Any,
            collection_style: CollectionStyle::Any,
        }
    }
}

pub fn is_alpha(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

pub fn is_blank(c: char) -> bool {
    c == ' ' || c == '\t'
}

pub fn is_break(c: char) -> bool {
    matches!(c, '\r' | '\n' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

pub fn is_breakz(c: char) -> bool {
    is_break(c) || c == '\0'
}

pub fn is_blankz(c: char) -> bool {
    is_blank(c) || is_breakz(c)
}
