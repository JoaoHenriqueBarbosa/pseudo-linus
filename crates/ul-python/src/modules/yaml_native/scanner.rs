//! O scanner do libyaml (`scanner.c`): da sequência de caracteres para a fila de tokens, com a
//! pilha de indentação, as chaves simples potenciais e as mesmas mensagens de erro do C.

use super::reader::{Parser, SimpleKey};
use super::types::{is_alpha, is_blank, is_blankz, is_break, is_breakz, Encoding, Mark, ScalarStyle, Token, YamlError, TT};

/// Estado dos brancos e quebras que os escalares simples e entre aspas acumulam entre trechos de texto.
#[derive(Default)]
struct Blanks {
    leading_blanks: bool,
    whitespaces: String,
    leading_break: String,
    trailing_breaks: String,
}

impl Parser {
    fn scan_error(&self, context: Option<&'static str>, context_mark: Mark, problem: &'static str) -> YamlError {
        YamlError::scanner(context, context_mark, problem, self.mark)
    }

    /// `yaml_parser_scan`: o próximo token; `None` depois do fim do fluxo ou de um erro.
    pub fn scan(&mut self) -> Result<Option<Token>, YamlError> {
        if self.stream_end_produced || self.error {
            return Ok(None);
        }
        let result = self.scan_next();
        if result.is_err() {
            self.error = true;
        }
        result
    }

    fn scan_next(&mut self) -> Result<Option<Token>, YamlError> {
        if !self.token_available {
            self.fetch_more_tokens()?;
        }
        let Some(token) = self.tokens.pop_front() else { return Ok(None) };
        self.token_available = false;
        self.tokens_parsed += 1;
        if token.ty == TT::StreamEnd {
            self.stream_end_produced = true;
        }
        Ok(Some(token))
    }

    pub(super) fn fetch_more_tokens(&mut self) -> Result<(), YamlError> {
        loop {
            let mut need_more = self.tokens.is_empty();
            if !need_more {
                self.stale_simple_keys()?;
                need_more = self.simple_keys.iter().any(|k| k.possible && k.token_number == self.tokens_parsed);
            }
            if !need_more {
                break;
            }
            self.fetch_next_token()?;
        }
        self.token_available = true;
        Ok(())
    }

    fn stale_simple_keys(&mut self) -> Result<(), YamlError> {
        for i in 0..self.simple_keys.len() {
            let key = self.simple_keys[i];
            if key.possible && (key.mark.line < self.mark.line || key.mark.index + 1024 < self.mark.index) {
                if key.required {
                    return Err(self.scan_error(Some("while scanning a simple key"), key.mark, "could not find expected ':'"));
                }
                self.simple_keys[i].possible = false;
            }
        }
        Ok(())
    }

    fn save_simple_key(&mut self) -> Result<(), YamlError> {
        let required = self.flow_level == 0 && self.indent == self.mark.column as i64;
        if self.simple_key_allowed {
            let key = SimpleKey { possible: true, required, token_number: self.tokens_parsed + self.tokens.len(), mark: self.mark };
            self.remove_simple_key()?;
            if let Some(slot) = self.simple_keys.last_mut() {
                *slot = key;
            }
        }
        Ok(())
    }

    fn remove_simple_key(&mut self) -> Result<(), YamlError> {
        let Some(key) = self.simple_keys.last().copied() else { return Ok(()) };
        if key.possible && key.required {
            return Err(self.scan_error(Some("while scanning a simple key"), key.mark, "could not find expected ':'"));
        }
        if let Some(slot) = self.simple_keys.last_mut() {
            slot.possible = false;
        }
        Ok(())
    }

    fn increase_flow_level(&mut self) {
        self.simple_keys.push(SimpleKey::default());
        self.flow_level += 1;
    }

    fn decrease_flow_level(&mut self) {
        if self.flow_level > 0 {
            self.flow_level -= 1;
            self.simple_keys.pop();
        }
    }

    /// `number` é a posição absoluta do token na fila (`None` acrescenta ao fim).
    fn roll_indent(&mut self, column: i64, number: Option<usize>, ty: TT, mark: Mark) {
        if self.flow_level > 0 || self.indent >= column {
            return;
        }
        self.indents.push(self.indent);
        self.indent = column;
        let token = Token::new(ty, mark, mark);
        match number {
            None => self.tokens.push_back(token),
            Some(n) => self.tokens.insert(n - self.tokens_parsed, token),
        }
    }

    fn unroll_indent(&mut self, column: i64) {
        if self.flow_level > 0 {
            return;
        }
        while self.indent > column {
            self.tokens.push_back(Token::new(TT::BlockEnd, self.mark, self.mark));
            self.indent = self.indents.pop().unwrap_or(-1);
        }
    }

    /// Consome `width` caracteres e põe o token de indicador na fila.
    fn push_indicator(&mut self, ty: TT, width: usize) {
        let start = self.mark;
        for _ in 0..width {
            self.skip();
        }
        self.tokens.push_back(Token::new(ty, start, self.mark));
    }

    /// `---` e `...` no começo da linha, seguidos de espaço, quebra ou fim.
    fn document_indicator(&self) -> Option<TT> {
        if self.mark.column != 0 || !is_blankz(self.ch(3)) {
            return None;
        }
        match (self.ch(0), self.ch(1), self.ch(2)) {
            ('-', '-', '-') => Some(TT::DocumentStart),
            ('.', '.', '.') => Some(TT::DocumentEnd),
            _ => None,
        }
    }

    fn fetch_next_token(&mut self) -> Result<(), YamlError> {
        self.cache(1)?;
        if !self.stream_start_produced {
            self.fetch_stream_start();
            return Ok(());
        }
        self.scan_to_next_token()?;
        self.stale_simple_keys()?;
        self.unroll_indent(self.mark.column as i64);
        self.cache(4)?;
        let c = self.ch(0);
        let next_blankz = is_blankz(self.ch(1));
        if c == '\0' {
            return self.fetch_stream_end();
        }
        if self.mark.column == 0 && c == '%' {
            return self.fetch_directive();
        }
        if let Some(ty) = self.document_indicator() {
            return self.fetch_document_indicator(ty);
        }
        match c {
            '[' => return self.fetch_flow_collection_start(TT::FlowSequenceStart),
            '{' => return self.fetch_flow_collection_start(TT::FlowMappingStart),
            ']' => return self.fetch_flow_collection_end(TT::FlowSequenceEnd),
            '}' => return self.fetch_flow_collection_end(TT::FlowMappingEnd),
            ',' => return self.fetch_flow_entry(),
            '-' if next_blankz => return self.fetch_block_entry(),
            '?' if self.flow_level > 0 || next_blankz => return self.fetch_key(),
            ':' if self.flow_level > 0 || next_blankz => return self.fetch_value(),
            '*' => return self.fetch_anchor(TT::Alias),
            '&' => return self.fetch_anchor(TT::Anchor),
            '!' => return self.fetch_tag(),
            '|' if self.flow_level == 0 => return self.fetch_block_scalar(true),
            '>' if self.flow_level == 0 => return self.fetch_block_scalar(false),
            '\'' => return self.fetch_flow_scalar(true),
            '"' => return self.fetch_flow_scalar(false),
            _ => {}
        }
        let indicator = is_blankz(c) || "-?:,[]{}#&*!|>'\"%@`".contains(c);
        if !indicator
            || (c == '-' && !is_blank(self.ch(1)))
            || (self.flow_level == 0 && (c == '?' || c == ':') && !next_blankz)
        {
            return self.fetch_plain_scalar();
        }
        Err(self.scan_error(Some("while scanning for the next token"), self.mark, "found character that cannot start any token"))
    }

    fn fetch_stream_start(&mut self) {
        self.indent = -1;
        self.simple_keys.push(SimpleKey::default());
        self.simple_key_allowed = true;
        self.stream_start_produced = true;
        let mut token = Token::new(TT::StreamStart, self.mark, self.mark);
        token.encoding = self.encoding.unwrap_or(Encoding::Utf8);
        self.tokens.push_back(token);
    }

    fn fetch_stream_end(&mut self) -> Result<(), YamlError> {
        if self.mark.column != 0 {
            self.mark.column = 0;
            self.mark.line += 1;
        }
        self.unroll_indent(-1);
        self.remove_simple_key()?;
        self.simple_key_allowed = false;
        self.tokens.push_back(Token::new(TT::StreamEnd, self.mark, self.mark));
        Ok(())
    }

    fn fetch_directive(&mut self) -> Result<(), YamlError> {
        self.unroll_indent(-1);
        self.remove_simple_key()?;
        self.simple_key_allowed = false;
        let token = self.scan_directive()?;
        self.tokens.push_back(token);
        Ok(())
    }

    fn fetch_document_indicator(&mut self, ty: TT) -> Result<(), YamlError> {
        self.unroll_indent(-1);
        self.remove_simple_key()?;
        self.simple_key_allowed = false;
        self.push_indicator(ty, 3);
        Ok(())
    }

    fn fetch_flow_collection_start(&mut self, ty: TT) -> Result<(), YamlError> {
        self.save_simple_key()?;
        self.increase_flow_level();
        self.simple_key_allowed = true;
        self.push_indicator(ty, 1);
        Ok(())
    }

    fn fetch_flow_collection_end(&mut self, ty: TT) -> Result<(), YamlError> {
        self.remove_simple_key()?;
        self.decrease_flow_level();
        self.simple_key_allowed = false;
        self.push_indicator(ty, 1);
        Ok(())
    }

    fn fetch_flow_entry(&mut self) -> Result<(), YamlError> {
        self.remove_simple_key()?;
        self.simple_key_allowed = true;
        self.push_indicator(TT::FlowEntry, 1);
        Ok(())
    }

    fn fetch_block_entry(&mut self) -> Result<(), YamlError> {
        if self.flow_level == 0 {
            if !self.simple_key_allowed {
                return Err(self.scan_error(None, self.mark, "block sequence entries are not allowed in this context"));
            }
            self.roll_indent(self.mark.column as i64, None, TT::BlockSequenceStart, self.mark);
        }
        self.remove_simple_key()?;
        self.simple_key_allowed = true;
        self.push_indicator(TT::BlockEntry, 1);
        Ok(())
    }

    fn fetch_key(&mut self) -> Result<(), YamlError> {
        if self.flow_level == 0 {
            if !self.simple_key_allowed {
                return Err(self.scan_error(None, self.mark, "mapping keys are not allowed in this context"));
            }
            self.roll_indent(self.mark.column as i64, None, TT::BlockMappingStart, self.mark);
        }
        self.remove_simple_key()?;
        self.simple_key_allowed = self.flow_level == 0;
        self.push_indicator(TT::Key, 1);
        Ok(())
    }

    fn fetch_value(&mut self) -> Result<(), YamlError> {
        let key = self.simple_keys.last().copied().unwrap_or_default();
        if key.possible {
            self.tokens.insert(key.token_number - self.tokens_parsed, Token::new(TT::Key, key.mark, key.mark));
            self.roll_indent(key.mark.column as i64, Some(key.token_number), TT::BlockMappingStart, key.mark);
            if let Some(slot) = self.simple_keys.last_mut() {
                slot.possible = false;
            }
            self.simple_key_allowed = false;
        } else {
            if self.flow_level == 0 {
                if !self.simple_key_allowed {
                    return Err(self.scan_error(None, self.mark, "mapping values are not allowed in this context"));
                }
                self.roll_indent(self.mark.column as i64, None, TT::BlockMappingStart, self.mark);
            }
            self.simple_key_allowed = self.flow_level == 0;
        }
        self.push_indicator(TT::Value, 1);
        Ok(())
    }

    fn fetch_anchor(&mut self, ty: TT) -> Result<(), YamlError> {
        self.save_simple_key()?;
        self.simple_key_allowed = false;
        let token = self.scan_anchor(ty)?;
        self.tokens.push_back(token);
        Ok(())
    }

    fn fetch_tag(&mut self) -> Result<(), YamlError> {
        self.save_simple_key()?;
        self.simple_key_allowed = false;
        let token = self.scan_tag()?;
        self.tokens.push_back(token);
        Ok(())
    }

    fn fetch_block_scalar(&mut self, literal: bool) -> Result<(), YamlError> {
        self.remove_simple_key()?;
        self.simple_key_allowed = true;
        let token = self.scan_block_scalar(literal)?;
        self.tokens.push_back(token);
        Ok(())
    }

    fn fetch_flow_scalar(&mut self, single: bool) -> Result<(), YamlError> {
        self.save_simple_key()?;
        self.simple_key_allowed = false;
        let token = self.scan_flow_scalar(single)?;
        self.tokens.push_back(token);
        Ok(())
    }

    fn fetch_plain_scalar(&mut self) -> Result<(), YamlError> {
        self.save_simple_key()?;
        self.simple_key_allowed = false;
        let token = self.scan_plain_scalar()?;
        self.tokens.push_back(token);
        Ok(())
    }

    /// Consome espaços e tabs, e depois comentário até o fim da linha, quando `comment`.
    fn skip_blanks_and_comment(&mut self, tabs: bool, comment: bool) -> Result<(), YamlError> {
        self.cache(1)?;
        while self.ch(0) == ' ' || (tabs && self.ch(0) == '\t') {
            self.skip();
            self.cache(1)?;
        }
        if comment && self.ch(0) == '#' {
            while !is_breakz(self.ch(0)) {
                self.skip();
                self.cache(1)?;
            }
        }
        Ok(())
    }

    fn scan_to_next_token(&mut self) -> Result<(), YamlError> {
        loop {
            self.cache(1)?;
            if self.mark.column == 0 && self.ch(0) == '\u{FEFF}' {
                self.skip();
            }
            let tabs = self.flow_level > 0 || !self.simple_key_allowed;
            self.skip_blanks_and_comment(tabs, true)?;
            if !is_break(self.ch(0)) {
                return Ok(());
            }
            self.cache(2)?;
            self.skip_line();
            if self.flow_level == 0 {
                self.simple_key_allowed = true;
            }
        }
    }

    fn scan_directive(&mut self) -> Result<Token, YamlError> {
        let start_mark = self.mark;
        self.skip();
        let name = self.scan_directive_name(start_mark)?;
        let token = match name.as_str() {
            "YAML" => {
                let (major, minor) = self.scan_version_directive_value(start_mark)?;
                let mut token = Token::new(TT::VersionDirective, start_mark, self.mark);
                token.major = major;
                token.minor = minor;
                token
            }
            "TAG" => {
                let (handle, prefix) = self.scan_tag_directive_value(start_mark)?;
                let mut token = Token::new(TT::TagDirective, start_mark, self.mark);
                token.a = handle;
                token.b = prefix;
                token
            }
            _ => return Err(self.scan_error(Some("while scanning a directive"), start_mark, "found unknown directive name")),
        };
        self.skip_blanks_and_comment(true, true)?;
        if !is_breakz(self.ch(0)) {
            return Err(self.scan_error(Some("while scanning a directive"), start_mark, "did not find expected comment or line break"));
        }
        if is_break(self.ch(0)) {
            self.cache(2)?;
            self.skip_line();
        }
        Ok(token)
    }

    fn scan_directive_name(&mut self, start_mark: Mark) -> Result<String, YamlError> {
        let mut name = String::new();
        self.cache(1)?;
        while is_alpha(self.ch(0)) {
            self.read_char(&mut name);
            self.cache(1)?;
        }
        if name.is_empty() {
            return Err(self.scan_error(Some("while scanning a directive"), start_mark, "could not find expected directive name"));
        }
        if !is_blankz(self.ch(0)) {
            return Err(self.scan_error(Some("while scanning a directive"), start_mark, "found unexpected non-alphabetical character"));
        }
        Ok(name)
    }

    fn scan_version_directive_value(&mut self, start_mark: Mark) -> Result<(i64, i64), YamlError> {
        self.skip_blanks_and_comment(true, false)?;
        let major = self.scan_version_directive_number(start_mark)?;
        if self.ch(0) != '.' {
            return Err(self.scan_error(Some("while scanning a %YAML directive"), start_mark, "did not find expected digit or '.' character"));
        }
        self.skip();
        let minor = self.scan_version_directive_number(start_mark)?;
        Ok((major, minor))
    }

    fn scan_version_directive_number(&mut self, start_mark: Mark) -> Result<i64, YamlError> {
        let mut value = 0i64;
        let mut length = 0usize;
        self.cache(1)?;
        while let Some(digit) = self.ch(0).to_digit(10) {
            length += 1;
            if length > 9 {
                return Err(self.scan_error(Some("while scanning a %YAML directive"), start_mark, "found extremely long version number"));
            }
            value = value * 10 + i64::from(digit);
            self.skip();
            self.cache(1)?;
        }
        if length == 0 {
            return Err(self.scan_error(Some("while scanning a %YAML directive"), start_mark, "did not find expected version number"));
        }
        Ok(value)
    }

    fn scan_tag_directive_value(&mut self, start_mark: Mark) -> Result<(String, String), YamlError> {
        self.skip_blanks_and_comment(true, false)?;
        let handle = self.scan_tag_handle(true, start_mark)?;
        self.cache(1)?;
        if !is_blank(self.ch(0)) {
            return Err(self.scan_error(Some("while scanning a %TAG directive"), start_mark, "did not find expected whitespace"));
        }
        self.skip_blanks_and_comment(true, false)?;
        let prefix = self.scan_tag_uri(true, true, None, start_mark)?;
        self.cache(1)?;
        if !is_blankz(self.ch(0)) {
            return Err(self.scan_error(Some("while scanning a %TAG directive"), start_mark, "did not find expected whitespace or line break"));
        }
        Ok((handle, prefix))
    }

    fn scan_anchor(&mut self, ty: TT) -> Result<Token, YamlError> {
        let start_mark = self.mark;
        self.skip();
        let mut value = String::new();
        self.cache(1)?;
        while is_alpha(self.ch(0)) {
            self.read_char(&mut value);
            self.cache(1)?;
        }
        let end_mark = self.mark;
        let c = self.ch(0);
        if value.is_empty() || !(is_blankz(c) || "?:,]}%@`".contains(c)) {
            let context = if ty == TT::Anchor { "while scanning an anchor" } else { "while scanning an alias" };
            return Err(self.scan_error(Some(context), start_mark, "did not find expected alphabetic or numeric character"));
        }
        let mut token = Token::new(ty, start_mark, end_mark);
        token.a = value;
        Ok(token)
    }

    fn scan_tag(&mut self) -> Result<Token, YamlError> {
        let start_mark = self.mark;
        self.cache(2)?;
        let handle: String;
        let suffix: String;
        if self.ch(1) == '<' {
            self.skip();
            self.skip();
            suffix = self.scan_tag_uri(true, false, None, start_mark)?;
            if self.ch(0) != '>' {
                return Err(self.scan_error(Some("while scanning a tag"), start_mark, "did not find the expected '>'"));
            }
            self.skip();
            handle = String::new();
        } else {
            let first = self.scan_tag_handle(false, start_mark)?;
            if first.starts_with('!') && first.len() > 1 && first.ends_with('!') {
                suffix = self.scan_tag_uri(false, false, None, start_mark)?;
                handle = first;
            } else {
                let rest = self.scan_tag_uri(false, false, Some(first.as_str()), start_mark)?;
                // Caso especial: a tag `!` tem handle vazio e sufixo `!`.
                if rest.is_empty() {
                    handle = String::new();
                    suffix = "!".to_string();
                } else {
                    handle = "!".to_string();
                    suffix = rest;
                }
            }
        }
        self.cache(1)?;
        if !is_blankz(self.ch(0)) && (self.flow_level == 0 || self.ch(0) != ',') {
            return Err(self.scan_error(Some("while scanning a tag"), start_mark, "did not find expected whitespace or line break"));
        }
        let mut token = Token::new(TT::Tag, start_mark, self.mark);
        token.a = handle;
        token.b = suffix;
        Ok(token)
    }

    fn scan_tag_handle(&mut self, directive: bool, start_mark: Mark) -> Result<String, YamlError> {
        self.cache(1)?;
        if self.ch(0) != '!' {
            let context = if directive { "while scanning a tag directive" } else { "while scanning a tag" };
            return Err(self.scan_error(Some(context), start_mark, "did not find expected '!'"));
        }
        let mut handle = String::new();
        self.read_char(&mut handle);
        self.cache(1)?;
        while is_alpha(self.ch(0)) {
            self.read_char(&mut handle);
            self.cache(1)?;
        }
        if self.ch(0) == '!' {
            self.read_char(&mut handle);
        } else if directive && handle != "!" {
            return Err(self.scan_error(Some("while parsing a tag directive"), start_mark, "did not find expected '!'"));
        }
        Ok(handle)
    }

    fn scan_tag_uri(&mut self, uri_char: bool, directive: bool, head: Option<&str>, start_mark: Mark) -> Result<String, YamlError> {
        let mut bytes: Vec<u8> = Vec::new();
        let mut length = head.map_or(0, str::len);
        if let Some(head) = head.filter(|h| h.len() > 1) {
            bytes.extend_from_slice(&head.as_bytes()[1..]);
        }
        self.cache(1)?;
        loop {
            let c = self.ch(0);
            let allowed = is_alpha(c) || ";/?:@&=+$.%!~*'()".contains(c) || (uri_char && ",[]".contains(c));
            if !allowed {
                break;
            }
            if c == '%' {
                self.scan_uri_escapes(directive, start_mark, &mut bytes)?;
            } else {
                let mut one = String::new();
                self.read_char(&mut one);
                bytes.extend_from_slice(one.as_bytes());
            }
            length += 1;
            self.cache(1)?;
        }
        if length == 0 {
            let context = if directive { "while parsing a %TAG directive" } else { "while parsing a tag" };
            return Err(self.scan_error(Some(context), start_mark, "did not find expected tag URI"));
        }
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn scan_uri_escapes(&mut self, directive: bool, start_mark: Mark, out: &mut Vec<u8>) -> Result<(), YamlError> {
        let context = if directive { "while parsing a %TAG directive" } else { "while parsing a tag" };
        let mut width = 0usize;
        loop {
            self.cache(3)?;
            if !(self.ch(0) == '%' && self.ch(1).is_ascii_hexdigit() && self.ch(2).is_ascii_hexdigit()) {
                return Err(self.scan_error(Some(context), start_mark, "did not find URI escaped octet"));
            }
            let octet = (self.ch(1).to_digit(16).unwrap_or(0) * 16 + self.ch(2).to_digit(16).unwrap_or(0)) as u8;
            if width == 0 {
                width = match octet {
                    o if o & 0x80 == 0 => 1,
                    o if o & 0xE0 == 0xC0 => 2,
                    o if o & 0xF0 == 0xE0 => 3,
                    o if o & 0xF8 == 0xF0 => 4,
                    _ => 0,
                };
                if width == 0 {
                    return Err(self.scan_error(Some(context), start_mark, "found an incorrect leading UTF-8 octet"));
                }
            } else if octet & 0xC0 != 0x80 {
                return Err(self.scan_error(Some(context), start_mark, "found an incorrect trailing UTF-8 octet"));
            }
            out.push(octet);
            self.skip();
            self.skip();
            self.skip();
            width -= 1;
            if width == 0 {
                return Ok(());
            }
        }
    }

    /// Junta uma quebra de linha pendente e as quebras seguintes ao texto, dobrando como o YAML manda
    /// (usado pelos escalares simples e entre aspas).
    fn fold_breaks(string: &mut String, leading_break: &mut String, trailing_breaks: &mut String) {
        if leading_break.starts_with('\n') {
            if trailing_breaks.is_empty() {
                string.push(' ');
            } else {
                string.push_str(trailing_breaks);
                trailing_breaks.clear();
            }
            leading_break.clear();
        } else {
            string.push_str(leading_break);
            string.push_str(trailing_breaks);
            leading_break.clear();
            trailing_breaks.clear();
        }
    }

    fn scan_block_scalar(&mut self, literal: bool) -> Result<Token, YamlError> {
        let mut chomping = 0i32;
        let mut increment = 0i64;
        let mut indent = 0i64;
        let mut leading_blank = false;
        let start_mark = self.mark;
        self.skip();
        self.cache(1)?;
        let indentation_zero = "while scanning a block scalar";
        let zero_error = "found an indentation indicator equal to 0";
        if self.ch(0) == '+' || self.ch(0) == '-' {
            chomping = if self.ch(0) == '+' { 1 } else { -1 };
            self.skip();
            self.cache(1)?;
            if let Some(digit) = self.ch(0).to_digit(10) {
                if digit == 0 {
                    return Err(self.scan_error(Some(indentation_zero), start_mark, zero_error));
                }
                increment = i64::from(digit);
                self.skip();
            }
        } else if let Some(digit) = self.ch(0).to_digit(10) {
            if digit == 0 {
                return Err(self.scan_error(Some(indentation_zero), start_mark, zero_error));
            }
            increment = i64::from(digit);
            self.skip();
            self.cache(1)?;
            if self.ch(0) == '+' || self.ch(0) == '-' {
                chomping = if self.ch(0) == '+' { 1 } else { -1 };
                self.skip();
            }
        }
        self.skip_blanks_and_comment(true, true)?;
        if !is_breakz(self.ch(0)) {
            return Err(self.scan_error(Some(indentation_zero), start_mark, "did not find expected comment or line break"));
        }
        if is_break(self.ch(0)) {
            self.cache(2)?;
            self.skip_line();
        }
        let mut end_mark = self.mark;
        if increment > 0 {
            indent = if self.indent >= 0 { self.indent + increment } else { increment };
        }
        let mut string = String::new();
        let mut leading_break = String::new();
        let mut trailing_breaks = String::new();
        self.scan_block_scalar_breaks(&mut indent, &mut trailing_breaks, start_mark, &mut end_mark)?;
        self.cache(1)?;
        while self.mark.column as i64 == indent && self.ch(0) != '\0' {
            let trailing_blank = is_blank(self.ch(0));
            if !literal && leading_break.starts_with('\n') && !leading_blank && !trailing_blank {
                if trailing_breaks.is_empty() {
                    string.push(' ');
                }
                leading_break.clear();
            } else {
                string.push_str(&leading_break);
                leading_break.clear();
            }
            string.push_str(&trailing_breaks);
            trailing_breaks.clear();
            leading_blank = is_blank(self.ch(0));
            while !is_breakz(self.ch(0)) {
                self.read_char(&mut string);
                self.cache(1)?;
            }
            self.cache(2)?;
            self.read_line(&mut leading_break);
            self.scan_block_scalar_breaks(&mut indent, &mut trailing_breaks, start_mark, &mut end_mark)?;
        }
        if chomping != -1 {
            string.push_str(&leading_break);
        }
        if chomping == 1 {
            string.push_str(&trailing_breaks);
        }
        let mut token = Token::new(TT::Scalar, start_mark, end_mark);
        token.a = string;
        token.style = if literal { ScalarStyle::Literal } else { ScalarStyle::Folded };
        Ok(token)
    }

    fn scan_block_scalar_breaks(&mut self, indent: &mut i64, breaks: &mut String, start_mark: Mark, end_mark: &mut Mark) -> Result<(), YamlError> {
        let mut max_indent = 0i64;
        *end_mark = self.mark;
        loop {
            self.cache(1)?;
            while (*indent == 0 || (self.mark.column as i64) < *indent) && self.ch(0) == ' ' {
                self.skip();
                self.cache(1)?;
            }
            max_indent = max_indent.max(self.mark.column as i64);
            if (*indent == 0 || (self.mark.column as i64) < *indent) && self.ch(0) == '\t' {
                return Err(self.scan_error(
                    Some("while scanning a block scalar"),
                    start_mark,
                    "found a tab character where an indentation space is expected",
                ));
            }
            if !is_break(self.ch(0)) {
                break;
            }
            self.cache(2)?;
            self.read_line(breaks);
            *end_mark = self.mark;
        }
        if *indent == 0 {
            *indent = max_indent.max(self.indent + 1).max(1);
        }
        Ok(())
    }

    /// Valor de um escape `\x`, `\u` ou `\U` de escalar entre aspas duplas: o caractere e quantos
    /// dígitos hexadecimais seguem.
    fn escape_code(&self) -> Option<(Option<char>, usize)> {
        let simple = match self.ch(1) {
            '0' => '\0',
            'a' => '\x07',
            'b' => '\x08',
            't' | '\t' => '\t',
            'n' => '\n',
            'v' => '\x0B',
            'f' => '\x0C',
            'r' => '\r',
            'e' => '\x1B',
            ' ' => ' ',
            '"' => '"',
            '/' => '/',
            '\\' => '\\',
            'N' => '\u{85}',
            '_' => '\u{A0}',
            'L' => '\u{2028}',
            'P' => '\u{2029}',
            'x' => return Some((None, 2)),
            'u' => return Some((None, 4)),
            'U' => return Some((None, 8)),
            _ => return None,
        };
        Some((Some(simple), 0))
    }

    /// Consome os brancos e quebras de linha depois de um trecho de texto, acumulando em `blanks`.
    /// `plain_indent` é a indentação mínima do escalar simples: nele, uma tabulação num branco
    /// inicial aquém dela é erro (o escalar entre aspas passa `None`).
    fn scan_blanks_and_breaks(&mut self, blanks: &mut Blanks, plain_indent: Option<i64>, start_mark: Mark) -> Result<(), YamlError> {
        while is_blank(self.ch(0)) || is_break(self.ch(0)) {
            if is_blank(self.ch(0)) {
                if blanks.leading_blanks && self.ch(0) == '\t' && plain_indent.is_some_and(|indent| (self.mark.column as i64) < indent) {
                    return Err(self.scan_error(
                        Some("while scanning a plain scalar"),
                        start_mark,
                        "found a tab character that violates indentation",
                    ));
                }
                if blanks.leading_blanks {
                    self.skip();
                } else {
                    self.read_char(&mut blanks.whitespaces);
                }
            } else {
                self.cache(2)?;
                if blanks.leading_blanks {
                    self.read_line(&mut blanks.trailing_breaks);
                } else {
                    blanks.whitespaces.clear();
                    self.read_line(&mut blanks.leading_break);
                    blanks.leading_blanks = true;
                }
            }
            self.cache(1)?;
        }
        Ok(())
    }

    fn scan_flow_scalar(&mut self, single: bool) -> Result<Token, YamlError> {
        let start_mark = self.mark;
        self.skip();
        let quote = if single { '\'' } else { '"' };
        let mut string = String::new();
        let mut blanks = Blanks::default();
        loop {
            self.cache(4)?;
            if self.document_indicator().is_some() {
                return Err(self.scan_error(Some("while scanning a quoted scalar"), start_mark, "found unexpected document indicator"));
            }
            if self.ch(0) == '\0' {
                return Err(self.scan_error(Some("while scanning a quoted scalar"), start_mark, "found unexpected end of stream"));
            }
            self.cache(2)?;
            blanks.leading_blanks = false;
            while !is_blankz(self.ch(0)) {
                if single && self.ch(0) == '\'' && self.ch(1) == '\'' {
                    string.push('\'');
                    self.skip();
                    self.skip();
                } else if self.ch(0) == quote {
                    break;
                } else if !single && self.ch(0) == '\\' && is_break(self.ch(1)) {
                    self.cache(3)?;
                    self.skip();
                    self.skip_line();
                    blanks.leading_blanks = true;
                    break;
                } else if !single && self.ch(0) == '\\' {
                    self.scan_escape(start_mark, &mut string)?;
                } else {
                    self.read_char(&mut string);
                }
                self.cache(2)?;
            }
            self.cache(1)?;
            if self.ch(0) == quote {
                break;
            }
            self.cache(1)?;
            self.scan_blanks_and_breaks(&mut blanks, None, start_mark)?;
            if blanks.leading_blanks {
                Self::fold_breaks(&mut string, &mut blanks.leading_break, &mut blanks.trailing_breaks);
            } else {
                string.push_str(&blanks.whitespaces);
                blanks.whitespaces.clear();
            }
        }
        self.skip();
        let mut token = Token::new(TT::Scalar, start_mark, self.mark);
        token.a = string;
        token.style = if single { ScalarStyle::SingleQuoted } else { ScalarStyle::DoubleQuoted };
        Ok(token)
    }

    /// Escape de barra invertida dentro de aspas duplas, posicionado na `\`.
    fn scan_escape(&mut self, start_mark: Mark, string: &mut String) -> Result<(), YamlError> {
        let Some((simple, code_length)) = self.escape_code() else {
            return Err(self.scan_error(Some("while parsing a quoted scalar"), start_mark, "found unknown escape character"));
        };
        if let Some(c) = simple {
            string.push(c);
        }
        self.skip();
        self.skip();
        if code_length == 0 {
            return Ok(());
        }
        let mut value = 0u32;
        self.cache(code_length)?;
        for k in 0..code_length {
            let Some(digit) = self.ch(k).to_digit(16) else {
                return Err(self.scan_error(Some("while parsing a quoted scalar"), start_mark, "did not find expected hexdecimal number"));
            };
            value = (value << 4) + digit;
        }
        let Some(c) = char::from_u32(value) else {
            return Err(self.scan_error(Some("while parsing a quoted scalar"), start_mark, "found invalid Unicode character escape code"));
        };
        string.push(c);
        for _ in 0..code_length {
            self.skip();
        }
        Ok(())
    }

    fn scan_plain_scalar(&mut self) -> Result<Token, YamlError> {
        let mut string = String::new();
        let mut blanks = Blanks::default();
        let indent = self.indent + 1;
        let start_mark = self.mark;
        let mut end_mark = self.mark;
        loop {
            self.cache(4)?;
            if self.document_indicator().is_some() || self.ch(0) == '#' {
                break;
            }
            while !is_blankz(self.ch(0)) {
                let c = self.ch(0);
                if self.flow_level > 0 && c == ':' && ",?[]{}".contains(self.ch(1)) {
                    return Err(self.scan_error(Some("while scanning a plain scalar"), start_mark, "found unexpected ':'"));
                }
                if (c == ':' && is_blankz(self.ch(1))) || (self.flow_level > 0 && ",[]{}".contains(c)) {
                    break;
                }
                if blanks.leading_blanks || !blanks.whitespaces.is_empty() {
                    if blanks.leading_blanks {
                        Self::fold_breaks(&mut string, &mut blanks.leading_break, &mut blanks.trailing_breaks);
                        blanks.leading_blanks = false;
                    } else {
                        string.push_str(&blanks.whitespaces);
                        blanks.whitespaces.clear();
                    }
                }
                self.read_char(&mut string);
                end_mark = self.mark;
                self.cache(2)?;
            }
            if !(is_blank(self.ch(0)) || is_break(self.ch(0))) {
                break;
            }
            self.cache(1)?;
            self.scan_blanks_and_breaks(&mut blanks, Some(indent), start_mark)?;
            if self.flow_level == 0 && (self.mark.column as i64) < indent {
                break;
            }
        }
        let mut token = Token::new(TT::Scalar, start_mark, end_mark);
        token.a = string;
        token.style = ScalarStyle::Plain;
        if blanks.leading_blanks {
            self.simple_key_allowed = true;
        }
        Ok(token)
    }
}
