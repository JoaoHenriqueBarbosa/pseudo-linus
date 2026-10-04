// Portado do PyYAML 6.0.2 (yaml/scanner.py, yaml/reader.py e yaml/error.py), Copyright (c) 2017-2021
// Ingy döt Net, Copyright (c) 2006-2016 Kirill Simonov, licença MIT. Modificado no pseudo-linus
// (2026, MIT): Rust seguro, e as mensagens e o tratamento de tabulação são os do libyaml 0.2.5 (o
// yq do Debian carrega com o `CSafeLoader`, que usa o scanner em C).

//! Scanner do YAML: texto em tokens, com as marcas (linha, coluna) de cada um.

use std::collections::{BTreeMap, VecDeque};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mark {
    pub index: usize,
    pub line: usize,
    pub column: usize,
}

/// Um erro com o formato do `MarkedYAMLError` do PyYAML (ou uma exceção simples do Python, quando
/// não há marcas). Fica numa caixa pra o `Result` dos caminhos quentes continuar pequeno.
#[derive(Clone, Debug)]
pub struct YamlError(Box<ErrorData>);

impl std::ops::Deref for YamlError {
    type Target = ErrorData;
    fn deref(&self) -> &ErrorData {
        &self.0
    }
}

#[derive(Clone, Debug)]
pub struct ErrorData {
    /// O nome da classe da exceção (`ScannerError`, `ParserError`, `ValueError`...).
    pub kind: &'static str,
    pub context: Option<String>,
    pub context_mark: Option<Mark>,
    pub problem: Option<String>,
    pub problem_mark: Option<Mark>,
}

impl YamlError {
    pub fn marked(kind: &'static str, context: Option<&str>, context_mark: Option<Mark>, problem: &str, problem_mark: Mark) -> YamlError {
        YamlError(Box::new(ErrorData {
            kind,
            context: context.map(str::to_string),
            context_mark,
            problem: Some(problem.to_string()),
            problem_mark: Some(problem_mark),
        }))
    }

    /// Exceção do Python sem marcas (`ValueError: ...`).
    pub fn plain(kind: &'static str, msg: impl Into<String>) -> YamlError {
        YamlError(Box::new(ErrorData { kind, context: None, context_mark: None, problem: Some(msg.into()), problem_mark: None }))
    }
}

impl ErrorData {

    /// O `str()` da exceção, com as marcas no formato do `Mark.__str__` sem trecho (o `CParser`
    /// não guarda o buffer).
    pub fn message(&self, name: &str) -> String {
        let where_ = |m: &Mark| format!("  in \"{name}\", line {}, column {}", m.line + 1, m.column + 1);
        let mut lines: Vec<String> = Vec::new();
        if let Some(c) = &self.context {
            lines.push(c.clone());
        }
        if let Some(cm) = &self.context_mark {
            let same = match (&self.problem, &self.problem_mark) {
                (Some(_), Some(pm)) => pm.line == cm.line && pm.column == cm.column,
                _ => false,
            };
            if !same {
                lines.push(where_(cm));
            }
        }
        if let Some(p) = &self.problem {
            lines.push(p.clone());
        }
        if let Some(pm) = &self.problem_mark {
            lines.push(where_(pm));
        }
        lines.join("\n")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    StreamStart,
    StreamEnd,
    Directive { name: String, value: DirValue },
    DocumentStart,
    DocumentEnd,
    BlockSequenceStart,
    BlockMappingStart,
    BlockEnd,
    FlowSequenceStart,
    FlowMappingStart,
    FlowSequenceEnd,
    FlowMappingEnd,
    Key,
    Value,
    BlockEntry,
    FlowEntry,
    Alias(String),
    Anchor(String),
    /// (handle, suffix)
    Tag(Option<String>, String),
    Scalar { value: String, plain: bool, style: Option<char> },
}

#[derive(Clone, Debug, PartialEq)]
pub enum DirValue {
    None,
    Yaml(u32, u32),
    Tag(String, String),
}

#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    pub start: Mark,
    pub end: Mark,
}

#[derive(Clone, Debug)]
struct SimpleKey {
    token_number: usize,
    required: bool,
    index: usize,
    line: usize,
    mark: Mark,
    column: usize,
}

type R<T> = Result<T, YamlError>;

fn err(context: Option<&str>, context_mark: Option<Mark>, problem: &str, mark: Mark) -> YamlError {
    YamlError::marked("ScannerError", context, context_mark, problem, mark)
}

/// `'\0 \t\r\n\x85  '`
pub fn is_blankz(c: char) -> bool {
    matches!(c, '\0' | ' ' | '\t' | '\r' | '\n' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

/// `'\0\r\n\x85  '`
fn is_breakz(c: char) -> bool {
    matches!(c, '\0' | '\r' | '\n' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

/// `'\r\n\x85  '`
fn is_break(c: char) -> bool {
    matches!(c, '\r' | '\n' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

pub struct Scanner {
    buf: Vec<char>,
    pointer: usize,
    line: usize,
    column: usize,
    done: bool,
    flow_level: usize,
    tokens: VecDeque<Token>,
    tokens_taken: usize,
    indent: isize,
    indents: Vec<isize>,
    allow_simple_key: bool,
    possible_simple_keys: BTreeMap<usize, SimpleKey>,
}

impl Scanner {
    pub fn new(text: &str) -> Scanner {
        let mut buf: Vec<char> = text.chars().collect();
        buf.push('\0');
        let mut s = Scanner {
            buf,
            pointer: 0,
            line: 0,
            column: 0,
            done: false,
            flow_level: 0,
            tokens: VecDeque::new(),
            tokens_taken: 0,
            indent: -1,
            indents: Vec::new(),
            allow_simple_key: true,
            possible_simple_keys: BTreeMap::new(),
        };
        let m = s.mark();
        s.tokens.push_back(Token { tok: Tok::StreamStart, start: m, end: m });
        s
    }

    // ---- leitor ----

    fn peek(&self, k: usize) -> char {
        self.buf.get(self.pointer + k).copied().unwrap_or('\0')
    }

    fn prefix(&self, l: usize) -> String {
        let end = (self.pointer + l).min(self.buf.len());
        self.buf[self.pointer..end].iter().collect()
    }

    fn forward(&mut self, n: usize) {
        for _ in 0..n {
            let ch = self.peek(0);
            self.pointer += 1;
            if matches!(ch, '\n' | '\u{85}' | '\u{2028}' | '\u{2029}') || (ch == '\r' && self.peek(0) != '\n') {
                self.line += 1;
                self.column = 0;
            } else if ch != '\u{feff}' {
                self.column += 1;
            }
        }
    }

    pub fn mark(&self) -> Mark {
        Mark { index: self.pointer, line: self.line, column: self.column }
    }

    // ---- interface ----

    pub fn check_token(&mut self) -> R<Option<&Tok>> {
        while self.need_more_tokens()? {
            self.fetch_more_tokens()?;
        }
        Ok(self.tokens.front().map(|t| &t.tok))
    }

    pub fn peek_token(&mut self) -> R<Option<&Token>> {
        while self.need_more_tokens()? {
            self.fetch_more_tokens()?;
        }
        Ok(self.tokens.front())
    }

    pub fn get_token(&mut self) -> R<Option<Token>> {
        while self.need_more_tokens()? {
            self.fetch_more_tokens()?;
        }
        let t = self.tokens.pop_front();
        if t.is_some() {
            self.tokens_taken += 1;
        }
        Ok(t)
    }

    fn need_more_tokens(&mut self) -> R<bool> {
        if self.done {
            return Ok(false);
        }
        if self.tokens.is_empty() {
            return Ok(true);
        }
        self.stale_possible_simple_keys()?;
        Ok(self.next_possible_simple_key() == Some(self.tokens_taken))
    }

    fn fetch_more_tokens(&mut self) -> R<()> {
        self.scan_to_next_token();
        self.stale_possible_simple_keys()?;
        self.unwind_indent(self.column as isize);
        let ch = self.peek(0);
        if ch == '\0' {
            return self.fetch_stream_end();
        }
        if ch == '%' && self.column == 0 {
            return self.fetch_directive();
        }
        if ch == '-' && self.check_document_indicator("---") {
            return self.fetch_document_indicator(Tok::DocumentStart);
        }
        if ch == '.' && self.check_document_indicator("...") {
            return self.fetch_document_indicator(Tok::DocumentEnd);
        }
        match ch {
            '[' => return self.fetch_flow_collection_start(Tok::FlowSequenceStart),
            '{' => return self.fetch_flow_collection_start(Tok::FlowMappingStart),
            ']' => return self.fetch_flow_collection_end(Tok::FlowSequenceEnd),
            '}' => return self.fetch_flow_collection_end(Tok::FlowMappingEnd),
            ',' => return self.fetch_flow_entry(),
            _ => {}
        }
        if ch == '-' && is_blankz(self.peek(1)) {
            return self.fetch_block_entry();
        }
        if ch == '?' && (self.flow_level > 0 || is_blankz(self.peek(1))) {
            return self.fetch_key();
        }
        if ch == ':' && (self.flow_level > 0 || is_blankz(self.peek(1))) {
            return self.fetch_value();
        }
        match ch {
            '*' => return self.fetch_alias(),
            '&' => return self.fetch_anchor(),
            '!' => return self.fetch_tag(),
            '|' if self.flow_level == 0 => return self.fetch_block_scalar(false),
            '>' if self.flow_level == 0 => return self.fetch_block_scalar(true),
            '\'' => return self.fetch_flow_scalar(false),
            '"' => return self.fetch_flow_scalar(true),
            _ => {}
        }
        if self.check_plain() {
            return self.fetch_plain();
        }
        Err(err(
            Some("while scanning for the next token"),
            Some(self.mark()),
            "found character that cannot start any token",
            self.mark(),
        ))
    }

    // ---- chaves simples ----

    fn next_possible_simple_key(&self) -> Option<usize> {
        self.possible_simple_keys.values().map(|k| k.token_number).min()
    }

    fn stale_possible_simple_keys(&mut self) -> R<()> {
        let levels: Vec<usize> = self.possible_simple_keys.keys().copied().collect();
        for level in levels {
            let key = self.possible_simple_keys[&level].clone();
            if key.line != self.line || self.pointer - key.index > 1024 {
                if key.required {
                    return Err(err(Some("while scanning a simple key"), Some(key.mark), "could not find expected ':'", self.mark()));
                }
                self.possible_simple_keys.remove(&level);
            }
        }
        Ok(())
    }

    fn save_possible_simple_key(&mut self) -> R<()> {
        let required = self.flow_level == 0 && self.indent == self.column as isize;
        if self.allow_simple_key {
            self.remove_possible_simple_key()?;
            let token_number = self.tokens_taken + self.tokens.len();
            let key = SimpleKey {
                token_number,
                required,
                index: self.pointer,
                line: self.line,
                column: self.column,
                mark: self.mark(),
            };
            self.possible_simple_keys.insert(self.flow_level, key);
        }
        Ok(())
    }

    fn remove_possible_simple_key(&mut self) -> R<()> {
        if let Some(key) = self.possible_simple_keys.remove(&self.flow_level)
            && key.required
        {
            return Err(err(Some("while scanning a simple key"), Some(key.mark), "could not find expected ':'", self.mark()));
        }
        Ok(())
    }

    // ---- indentação ----

    fn unwind_indent(&mut self, column: isize) {
        if self.flow_level > 0 {
            return;
        }
        while self.indent > column {
            let m = self.mark();
            self.indent = self.indents.pop().unwrap_or(-1);
            self.tokens.push_back(Token { tok: Tok::BlockEnd, start: m, end: m });
        }
    }

    fn add_indent(&mut self, column: isize) -> bool {
        if self.indent < column {
            self.indents.push(self.indent);
            self.indent = column;
            return true;
        }
        false
    }

    // ---- buscadores ----

    fn push(&mut self, tok: Tok, start: Mark, end: Mark) {
        self.tokens.push_back(Token { tok, start, end });
    }

    fn fetch_stream_end(&mut self) -> R<()> {
        self.unwind_indent(-1);
        self.remove_possible_simple_key()?;
        self.allow_simple_key = false;
        self.possible_simple_keys.clear();
        let m = self.mark();
        self.push(Tok::StreamEnd, m, m);
        self.done = true;
        Ok(())
    }

    fn fetch_directive(&mut self) -> R<()> {
        self.unwind_indent(-1);
        self.remove_possible_simple_key()?;
        self.allow_simple_key = false;
        let t = self.scan_directive()?;
        self.tokens.push_back(t);
        Ok(())
    }

    fn check_document_indicator(&self, ind: &str) -> bool {
        self.column == 0 && self.prefix(3) == ind && is_blankz(self.peek(3))
    }

    fn fetch_document_indicator(&mut self, tok: Tok) -> R<()> {
        self.unwind_indent(-1);
        self.remove_possible_simple_key()?;
        self.allow_simple_key = false;
        let start = self.mark();
        self.forward(3);
        let end = self.mark();
        self.push(tok, start, end);
        Ok(())
    }

    fn fetch_flow_collection_start(&mut self, tok: Tok) -> R<()> {
        self.save_possible_simple_key()?;
        self.flow_level += 1;
        self.allow_simple_key = true;
        let start = self.mark();
        self.forward(1);
        let end = self.mark();
        self.push(tok, start, end);
        Ok(())
    }

    fn fetch_flow_collection_end(&mut self, tok: Tok) -> R<()> {
        self.remove_possible_simple_key()?;
        self.flow_level = self.flow_level.saturating_sub(1);
        self.allow_simple_key = false;
        let start = self.mark();
        self.forward(1);
        let end = self.mark();
        self.push(tok, start, end);
        Ok(())
    }

    fn fetch_flow_entry(&mut self) -> R<()> {
        self.allow_simple_key = true;
        self.remove_possible_simple_key()?;
        let start = self.mark();
        self.forward(1);
        let end = self.mark();
        self.push(Tok::FlowEntry, start, end);
        Ok(())
    }

    fn fetch_block_entry(&mut self) -> R<()> {
        if self.flow_level == 0 {
            if !self.allow_simple_key {
                return Err(err(None, None, "block sequence entries are not allowed in this context", self.mark()));
            }
            if self.add_indent(self.column as isize) {
                let m = self.mark();
                self.push(Tok::BlockSequenceStart, m, m);
            }
        }
        self.allow_simple_key = true;
        self.remove_possible_simple_key()?;
        let start = self.mark();
        self.forward(1);
        let end = self.mark();
        self.push(Tok::BlockEntry, start, end);
        Ok(())
    }

    fn fetch_key(&mut self) -> R<()> {
        if self.flow_level == 0 {
            if !self.allow_simple_key {
                return Err(err(None, None, "mapping keys are not allowed in this context", self.mark()));
            }
            if self.add_indent(self.column as isize) {
                let m = self.mark();
                self.push(Tok::BlockMappingStart, m, m);
            }
        }
        self.allow_simple_key = self.flow_level == 0;
        self.remove_possible_simple_key()?;
        let start = self.mark();
        self.forward(1);
        let end = self.mark();
        self.push(Tok::Key, start, end);
        Ok(())
    }

    fn fetch_value(&mut self) -> R<()> {
        if let Some(key) = self.possible_simple_keys.remove(&self.flow_level) {
            let at = key.token_number - self.tokens_taken;
            self.tokens.insert(at, Token { tok: Tok::Key, start: key.mark, end: key.mark });
            if self.flow_level == 0 && self.add_indent(key.column as isize) {
                self.tokens.insert(at, Token { tok: Tok::BlockMappingStart, start: key.mark, end: key.mark });
            }
            self.allow_simple_key = false;
        } else {
            if self.flow_level == 0 {
                if !self.allow_simple_key {
                    return Err(err(None, None, "mapping values are not allowed in this context", self.mark()));
                }
                if self.add_indent(self.column as isize) {
                    let m = self.mark();
                    self.push(Tok::BlockMappingStart, m, m);
                }
            }
            self.allow_simple_key = self.flow_level == 0;
            self.remove_possible_simple_key()?;
        }
        let start = self.mark();
        self.forward(1);
        let end = self.mark();
        self.push(Tok::Value, start, end);
        Ok(())
    }

    fn fetch_alias(&mut self) -> R<()> {
        self.save_possible_simple_key()?;
        self.allow_simple_key = false;
        let t = self.scan_anchor(true)?;
        self.tokens.push_back(t);
        Ok(())
    }

    fn fetch_anchor(&mut self) -> R<()> {
        self.save_possible_simple_key()?;
        self.allow_simple_key = false;
        let t = self.scan_anchor(false)?;
        self.tokens.push_back(t);
        Ok(())
    }

    fn fetch_tag(&mut self) -> R<()> {
        self.save_possible_simple_key()?;
        self.allow_simple_key = false;
        let t = self.scan_tag()?;
        self.tokens.push_back(t);
        Ok(())
    }

    fn fetch_block_scalar(&mut self, folded: bool) -> R<()> {
        self.allow_simple_key = true;
        self.remove_possible_simple_key()?;
        let t = self.scan_block_scalar(folded)?;
        self.tokens.push_back(t);
        Ok(())
    }

    fn fetch_flow_scalar(&mut self, double: bool) -> R<()> {
        self.save_possible_simple_key()?;
        self.allow_simple_key = false;
        let t = self.scan_flow_scalar(double)?;
        self.tokens.push_back(t);
        Ok(())
    }

    fn fetch_plain(&mut self) -> R<()> {
        self.save_possible_simple_key()?;
        self.allow_simple_key = false;
        let t = self.scan_plain()?;
        self.tokens.push_back(t);
        Ok(())
    }

    fn check_plain(&self) -> bool {
        let ch = self.peek(0);
        !(is_blankz(ch) || "-?:,[]{}#&*!|>'\"%@`".contains(ch))
            || (!is_blankz(self.peek(1)) && (ch == '-' || (self.flow_level == 0 && (ch == '?' || ch == ':'))))
    }

    // ---- varredores ----

    fn scan_to_next_token(&mut self) {
        if self.pointer == 0 && self.peek(0) == '\u{feff}' {
            self.forward(1);
        }
        loop {
            // libyaml: tabulação também é espaço em fluxo ou onde não pode começar chave simples.
            while self.peek(0) == ' ' || ((self.flow_level > 0 || !self.allow_simple_key) && self.peek(0) == '\t') {
                self.forward(1);
            }
            if self.peek(0) == '#' {
                while !is_breakz(self.peek(0)) {
                    self.forward(1);
                }
            }
            if !self.scan_line_break().is_empty() {
                if self.flow_level == 0 {
                    self.allow_simple_key = true;
                }
            } else {
                break;
            }
        }
    }

    fn scan_directive(&mut self) -> R<Token> {
        let start = self.mark();
        self.forward(1);
        let name = self.scan_directive_name(start)?;
        let value;
        let end;
        if name == "YAML" {
            value = self.scan_yaml_directive_value(start)?;
            end = self.mark();
        } else if name == "TAG" {
            value = self.scan_tag_directive_value(start)?;
            end = self.mark();
        } else {
            value = DirValue::None;
            end = self.mark();
            while !is_breakz(self.peek(0)) {
                self.forward(1);
            }
        }
        self.scan_directive_ignored_line(start)?;
        Ok(Token { tok: Tok::Directive { name, value }, start, end })
    }

    fn directive_err(&self, start: Mark, problem: &str) -> YamlError {
        err(Some("while scanning a directive"), Some(start), problem, self.mark())
    }

    fn scan_directive_name(&mut self, start: Mark) -> R<String> {
        let mut length = 0;
        while is_word(self.peek(length)) {
            length += 1;
        }
        if length == 0 {
            return Err(self.directive_err(start, "could not find expected directive name"));
        }
        let value = self.prefix(length);
        self.forward(length);
        if !is_blankz(self.peek(0)) || self.peek(0) == '\t' {
            return Err(self.directive_err(start, "found unexpected non-alphabetical character"));
        }
        Ok(value)
    }

    fn scan_yaml_directive_value(&mut self, start: Mark) -> R<DirValue> {
        while self.peek(0) == ' ' {
            self.forward(1);
        }
        let major = self.scan_yaml_directive_number(start)?;
        if self.peek(0) != '.' {
            return Err(self.directive_err(start, "did not find expected digit or '.' character"));
        }
        self.forward(1);
        let minor = self.scan_yaml_directive_number(start)?;
        if !is_blankz(self.peek(0)) {
            return Err(self.directive_err(start, "did not find expected digit or '.' character"));
        }
        Ok(DirValue::Yaml(major, minor))
    }

    fn scan_yaml_directive_number(&mut self, start: Mark) -> R<u32> {
        if !self.peek(0).is_ascii_digit() {
            return Err(self.directive_err(start, "did not find expected version number"));
        }
        let mut length = 0;
        while self.peek(length).is_ascii_digit() {
            length += 1;
        }
        if length > 9 {
            return Err(self.directive_err(start, "found extremely long version number"));
        }
        let v = self.prefix(length).parse().unwrap_or(0);
        self.forward(length);
        Ok(v)
    }

    fn scan_tag_directive_value(&mut self, start: Mark) -> R<DirValue> {
        while self.peek(0) == ' ' {
            self.forward(1);
        }
        let handle = self.scan_tag_handle(true, start)?;
        if self.peek(0) != ' ' {
            return Err(self.directive_err(start, "did not find expected whitespace"));
        }
        while self.peek(0) == ' ' {
            self.forward(1);
        }
        let prefix = self.scan_tag_uri(true, start)?;
        if !is_blankz(self.peek(0)) {
            return Err(self.directive_err(start, "did not find expected whitespace or line break"));
        }
        Ok(DirValue::Tag(handle, prefix))
    }

    fn scan_directive_ignored_line(&mut self, start: Mark) -> R<()> {
        while self.peek(0) == ' ' {
            self.forward(1);
        }
        if self.peek(0) == '#' {
            while !is_breakz(self.peek(0)) {
                self.forward(1);
            }
        }
        if !is_breakz(self.peek(0)) {
            return Err(self.directive_err(start, "did not find expected comment or line break"));
        }
        self.scan_line_break();
        Ok(())
    }

    fn scan_anchor(&mut self, alias: bool) -> R<Token> {
        let start = self.mark();
        self.forward(1);
        let mut length = 0;
        while is_word(self.peek(length)) {
            length += 1;
        }
        let ctx = if alias { "while scanning an alias" } else { "while scanning an anchor" };
        let value = self.prefix(length);
        self.forward(length);
        let ch = self.peek(0);
        if length == 0 || !(is_blankz(ch) || "?:,]}%@`".contains(ch)) {
            return Err(err(Some(ctx), Some(start), "did not find expected alphabetic or numeric character", self.mark()));
        }
        let end = self.mark();
        let tok = if alias { Tok::Alias(value) } else { Tok::Anchor(value) };
        Ok(Token { tok, start, end })
    }

    fn scan_tag(&mut self) -> R<Token> {
        let start = self.mark();
        let ch = self.peek(1);
        let handle;
        let suffix;
        if ch == '<' {
            handle = None;
            self.forward(2);
            suffix = self.scan_tag_uri(false, start)?;
            if self.peek(0) != '>' {
                return Err(err(Some("while scanning a tag"), Some(start), "did not find the expected '>'", self.mark()));
            }
            self.forward(1);
        } else if is_blankz(ch) {
            handle = None;
            suffix = "!".to_string();
            self.forward(1);
        } else {
            let mut length = 1;
            let mut use_handle = false;
            let mut c = ch;
            while !is_blankz(c) || c == '\t' {
                if c == '!' {
                    use_handle = true;
                    break;
                }
                length += 1;
                c = self.peek(length);
            }
            if use_handle {
                handle = Some(self.scan_tag_handle(false, start)?);
            } else {
                handle = Some("!".to_string());
                self.forward(1);
            }
            suffix = self.scan_tag_uri(false, start)?;
        }
        if !is_blankz(self.peek(0)) {
            return Err(err(Some("while scanning a tag"), Some(start), "did not find expected whitespace or line break", self.mark()));
        }
        let end = self.mark();
        Ok(Token { tok: Tok::Tag(handle, suffix), start, end })
    }

    fn scan_block_scalar(&mut self, folded: bool) -> R<Token> {
        let mut chunks = String::new();
        let start = self.mark();
        self.forward(1);
        let (chomping, increment) = self.scan_block_scalar_indicators(start)?;
        self.scan_block_scalar_ignored_line(start)?;
        let mut min_indent = self.indent + 1;
        if min_indent < 1 {
            min_indent = 1;
        }
        let (mut breaks, mut end, indent);
        match increment {
            None => {
                let (b, max_indent, e) = self.scan_block_scalar_indentation();
                breaks = b;
                end = e;
                indent = min_indent.max(max_indent as isize);
            }
            Some(inc) => {
                indent = min_indent + inc as isize - 1;
                let (b, e) = self.scan_block_scalar_breaks(indent);
                breaks = b;
                end = e;
            }
        }
        let mut line_break = String::new();
        while self.column as isize == indent && self.peek(0) != '\0' {
            for b in &breaks {
                chunks.push_str(b);
            }
            let leading_non_space = !matches!(self.peek(0), ' ' | '\t');
            let mut length = 0;
            while !is_breakz(self.peek(length)) {
                length += 1;
            }
            chunks.push_str(&self.prefix(length));
            self.forward(length);
            line_break = self.scan_line_break();
            let (b, e) = self.scan_block_scalar_breaks(indent);
            breaks = b;
            end = e;
            if self.column as isize == indent && self.peek(0) != '\0' {
                if folded && line_break == "\n" && leading_non_space && !matches!(self.peek(0), ' ' | '\t') {
                    if breaks.is_empty() {
                        chunks.push(' ');
                    }
                } else {
                    chunks.push_str(&line_break);
                }
            } else {
                break;
            }
        }
        if chomping != Some(false) {
            chunks.push_str(&line_break);
        }
        if chomping == Some(true) {
            for b in &breaks {
                chunks.push_str(b);
            }
        }
        Ok(Token { tok: Tok::Scalar { value: chunks, plain: false, style: Some(if folded { '>' } else { '|' }) }, start, end })
    }

    fn scan_block_scalar_indicators(&mut self, start: Mark) -> R<(Option<bool>, Option<u32>)> {
        let ctx = Some("while scanning a block scalar");
        let mut chomping = None;
        let mut increment = None;
        let ch = self.peek(0);
        if ch == '+' || ch == '-' {
            chomping = Some(ch == '+');
            self.forward(1);
            let c = self.peek(0);
            if let Some(d) = c.to_digit(10) {
                if d == 0 {
                    return Err(err(ctx, Some(start), "found an indentation indicator equal to 0", self.mark()));
                }
                increment = Some(d);
                self.forward(1);
            }
        } else if let Some(d) = ch.to_digit(10) {
            if d == 0 {
                return Err(err(ctx, Some(start), "found an indentation indicator equal to 0", self.mark()));
            }
            increment = Some(d);
            self.forward(1);
            let c = self.peek(0);
            if c == '+' || c == '-' {
                chomping = Some(c == '+');
                self.forward(1);
            }
        }
        Ok((chomping, increment))
    }

    fn scan_block_scalar_ignored_line(&mut self, start: Mark) -> R<()> {
        while self.peek(0) == ' ' || self.peek(0) == '\t' {
            self.forward(1);
        }
        if self.peek(0) == '#' {
            while !is_breakz(self.peek(0)) {
                self.forward(1);
            }
        }
        if !is_breakz(self.peek(0)) {
            return Err(err(Some("while scanning a block scalar"), Some(start), "did not find expected comment or line break", self.mark()));
        }
        self.scan_line_break();
        Ok(())
    }

    fn scan_block_scalar_indentation(&mut self) -> (Vec<String>, usize, Mark) {
        let mut chunks = Vec::new();
        let mut max_indent = 0;
        let mut end = self.mark();
        while matches!(self.peek(0), ' ') || is_break(self.peek(0)) {
            if self.peek(0) != ' ' {
                chunks.push(self.scan_line_break());
                end = self.mark();
            } else {
                self.forward(1);
                if self.column > max_indent {
                    max_indent = self.column;
                }
            }
        }
        (chunks, max_indent, end)
    }

    fn scan_block_scalar_breaks(&mut self, indent: isize) -> (Vec<String>, Mark) {
        let mut chunks = Vec::new();
        let mut end = self.mark();
        while (self.column as isize) < indent && self.peek(0) == ' ' {
            self.forward(1);
        }
        while is_break(self.peek(0)) {
            chunks.push(self.scan_line_break());
            end = self.mark();
            while (self.column as isize) < indent && self.peek(0) == ' ' {
                self.forward(1);
            }
        }
        (chunks, end)
    }

    fn scan_flow_scalar(&mut self, double: bool) -> R<Token> {
        let mut chunks = String::new();
        let start = self.mark();
        let quote = self.peek(0);
        self.forward(1);
        self.scan_flow_scalar_non_spaces(double, start, &mut chunks)?;
        while self.peek(0) != quote {
            self.scan_flow_scalar_spaces(start, &mut chunks)?;
            self.scan_flow_scalar_non_spaces(double, start, &mut chunks)?;
        }
        self.forward(1);
        let end = self.mark();
        Ok(Token { tok: Tok::Scalar { value: chunks, plain: false, style: Some(if double { '"' } else { '\'' }) }, start, end })
    }

    fn scan_flow_scalar_non_spaces(&mut self, double: bool, start: Mark, chunks: &mut String) -> R<()> {
        // O libyaml usa "parsing" (e não "scanning") nos erros de escape.
        let ctx = Some("while parsing a quoted scalar");
        loop {
            let mut length = 0;
            while !(matches!(self.peek(length), '\'' | '"' | '\\') || is_blankz(self.peek(length))) {
                length += 1;
            }
            if length > 0 {
                chunks.push_str(&self.prefix(length));
                self.forward(length);
            }
            let ch = self.peek(0);
            if !double && ch == '\'' && self.peek(1) == '\'' {
                chunks.push('\'');
                self.forward(2);
            } else if (double && ch == '\'') || (!double && (ch == '"' || ch == '\\')) {
                chunks.push(ch);
                self.forward(1);
            } else if double && ch == '\\' {
                let backslash = self.mark();
                self.forward(1);
                let c = self.peek(0);
                let rep = match c {
                    '0' => Some('\0'),
                    'a' => Some('\u{7}'),
                    'b' => Some('\u{8}'),
                    't' | '\t' => Some('\t'),
                    'n' => Some('\n'),
                    'v' => Some('\u{b}'),
                    'f' => Some('\u{c}'),
                    'r' => Some('\r'),
                    'e' => Some('\u{1b}'),
                    ' ' => Some(' '),
                    '"' => Some('"'),
                    '\\' => Some('\\'),
                    '/' => Some('/'),
                    'N' => Some('\u{85}'),
                    '_' => Some('\u{a0}'),
                    'L' => Some('\u{2028}'),
                    'P' => Some('\u{2029}'),
                    _ => None,
                };
                if let Some(r) = rep {
                    chunks.push(r);
                    self.forward(1);
                } else if let Some(len) = match c {
                    'x' => Some(2),
                    'u' => Some(4),
                    'U' => Some(8),
                    _ => None,
                } {
                    self.forward(1);
                    for k in 0..len {
                        if !self.peek(k).is_ascii_hexdigit() {
                            return Err(err(ctx, Some(start), "did not find expected hexdecimal number", self.mark()));
                        }
                    }
                    let code = u32::from_str_radix(&self.prefix(len), 16).unwrap_or(0);
                    match char::from_u32(code) {
                        Some(c) => chunks.push(c),
                        None => return Err(err(ctx, Some(start), "found invalid Unicode character escape code", self.mark())),
                    }
                    self.forward(len);
                } else if is_break(c) {
                    self.scan_line_break();
                    self.scan_flow_scalar_breaks(start, chunks)?;
                } else {
                    return Err(err(ctx, Some(start), "found unknown escape character", backslash));
                }
            } else {
                return Ok(());
            }
        }
    }

    fn scan_flow_scalar_spaces(&mut self, start: Mark, chunks: &mut String) -> R<()> {
        let mut length = 0;
        while matches!(self.peek(length), ' ' | '\t') {
            length += 1;
        }
        let whitespaces = self.prefix(length);
        self.forward(length);
        let ch = self.peek(0);
        if ch == '\0' {
            return Err(err(Some("while scanning a quoted scalar"), Some(start), "found unexpected end of stream", self.mark()));
        } else if is_break(ch) {
            let line_break = self.scan_line_break();
            let mut breaks = String::new();
            let n = self.scan_flow_scalar_breaks(start, &mut breaks)?;
            if line_break != "\n" {
                chunks.push_str(&line_break);
            } else if n == 0 {
                chunks.push(' ');
            }
            chunks.push_str(&breaks);
        } else {
            chunks.push_str(&whitespaces);
        }
        Ok(())
    }

    /// Devolve quantas quebras leu.
    fn scan_flow_scalar_breaks(&mut self, start: Mark, chunks: &mut String) -> R<usize> {
        let mut n = 0;
        loop {
            let prefix = self.prefix(3);
            if (prefix == "---" || prefix == "...") && is_blankz(self.peek(3)) {
                return Err(err(Some("while scanning a quoted scalar"), Some(start), "found unexpected document indicator", self.mark()));
            }
            while matches!(self.peek(0), ' ' | '\t') {
                self.forward(1);
            }
            if is_break(self.peek(0)) {
                chunks.push_str(&self.scan_line_break());
                n += 1;
            } else {
                return Ok(n);
            }
        }
    }

    fn scan_plain(&mut self) -> R<Token> {
        let mut chunks = String::new();
        let start = self.mark();
        let mut end = start;
        let indent = self.indent + 1;
        let mut spaces: Option<String> = Some(String::new());
        loop {
            let mut length = 0;
            if self.peek(0) == '#' {
                break;
            }
            loop {
                let ch = self.peek(length);
                let next = self.peek(length + 1);
                if is_blankz(ch)
                    || (ch == ':' && (is_blankz(next) || (self.flow_level > 0 && ",[]{}".contains(next))))
                    || (self.flow_level > 0 && ",?[]{}".contains(ch))
                {
                    break;
                }
                length += 1;
            }
            if length == 0 {
                break;
            }
            self.allow_simple_key = false;
            if let Some(s) = &spaces {
                chunks.push_str(s);
            }
            chunks.push_str(&self.prefix(length));
            self.forward(length);
            end = self.mark();
            spaces = self.scan_plain_spaces();
            match &spaces {
                None => break,
                Some(s) if s.is_empty() => break,
                _ => {}
            }
            if self.peek(0) == '#' || (self.flow_level == 0 && (self.column as isize) < indent) {
                break;
            }
        }
        Ok(Token { tok: Tok::Scalar { value: chunks, plain: true, style: None }, start, end })
    }

    /// `None` quando para num separador de documento (o `return` sem valor do PyYAML).
    fn scan_plain_spaces(&mut self) -> Option<String> {
        let mut chunks = String::new();
        let mut length = 0;
        while matches!(self.peek(length), ' ' | '\t') {
            length += 1;
        }
        let whitespaces = self.prefix(length);
        self.forward(length);
        let ch = self.peek(0);
        if is_break(ch) {
            let line_break = self.scan_line_break();
            self.allow_simple_key = true;
            let at_separator = |s: &Scanner| {
                let p = s.prefix(3);
                (p == "---" || p == "...") && is_blankz(s.peek(3))
            };
            if at_separator(self) {
                return None;
            }
            let mut breaks = String::new();
            let mut nbreaks = 0;
            while self.peek(0) == ' ' || is_break(self.peek(0)) {
                if self.peek(0) == ' ' {
                    self.forward(1);
                } else {
                    breaks.push_str(&self.scan_line_break());
                    nbreaks += 1;
                    if at_separator(self) {
                        return None;
                    }
                }
            }
            if line_break != "\n" {
                chunks.push_str(&line_break);
            } else if nbreaks == 0 {
                chunks.push(' ');
            }
            chunks.push_str(&breaks);
        } else if !whitespaces.is_empty() {
            chunks.push_str(&whitespaces);
        }
        Some(chunks)
    }

    fn scan_tag_handle(&mut self, directive: bool, start: Mark) -> R<String> {
        let ctx = if directive { "while scanning a %TAG directive" } else { "while scanning a tag" };
        if self.peek(0) != '!' {
            return Err(err(Some(ctx), Some(start), "did not find expected '!'", self.mark()));
        }
        let mut length = 1;
        let mut ch = self.peek(length);
        if ch != ' ' {
            while is_word(ch) {
                length += 1;
                ch = self.peek(length);
            }
            if ch != '!' {
                if directive {
                    self.forward(length);
                    return Err(err(Some(ctx), Some(start), "did not find expected '!'", self.mark()));
                }
                let value = self.prefix(length);
                self.forward(length);
                return Ok(value);
            }
            length += 1;
        }
        let value = self.prefix(length);
        self.forward(length);
        Ok(value)
    }

    fn scan_tag_uri(&mut self, directive: bool, start: Mark) -> R<String> {
        let ctx = if directive { "while parsing a %TAG directive" } else { "while parsing a tag" };
        let mut chunks = String::new();
        let mut length = 0;
        let mut ch = self.peek(length);
        while ch.is_ascii_alphanumeric() || "-;/?:@&=+$,_.!~*'()[]%".contains(ch) {
            if ch == '%' {
                chunks.push_str(&self.prefix(length));
                self.forward(length);
                length = 0;
                chunks.push_str(&self.scan_uri_escapes(ctx, start)?);
            } else {
                length += 1;
            }
            ch = self.peek(length);
        }
        if length > 0 {
            chunks.push_str(&self.prefix(length));
            self.forward(length);
        }
        if chunks.is_empty() {
            return Err(err(Some(ctx), Some(start), "did not find expected tag URI", self.mark()));
        }
        Ok(chunks)
    }

    fn scan_uri_escapes(&mut self, ctx: &str, start: Mark) -> R<String> {
        let mut codes = Vec::new();
        while self.peek(0) == '%' {
            self.forward(1);
            for k in 0..2 {
                if !self.peek(k).is_ascii_hexdigit() {
                    return Err(err(Some(ctx), Some(start), "did not find URI escaped octet", self.mark()));
                }
            }
            codes.push(u8::from_str_radix(&self.prefix(2), 16).unwrap_or(0));
            self.forward(2);
        }
        String::from_utf8(codes).map_err(|_| err(Some(ctx), Some(start), "found an incorrect leading UTF-8 octet", self.mark()))
    }

    fn scan_line_break(&mut self) -> String {
        let ch = self.peek(0);
        if matches!(ch, '\r' | '\n' | '\u{85}') {
            if self.prefix(2) == "\r\n" {
                self.forward(2);
            } else {
                self.forward(1);
            }
            return "\n".to_string();
        } else if matches!(ch, '\u{2028}' | '\u{2029}') {
            self.forward(1);
            return ch.to_string();
        }
        String::new()
    }
}
