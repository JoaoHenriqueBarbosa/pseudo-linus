//! O leitor do libyaml (`reader.c`): detecção de codificação, decodificação UTF-8 e UTF-16 em blocos
//! de 16384 bytes e o teste de caracteres permitidos. O estado do scanner e do parser vive na mesma
//! estrutura, como no `yaml_parser_t`.

use std::collections::VecDeque;

use super::parser::ParseState;
use super::types::{Encoding, Mark, Token, YamlError};
use crate::vm::PyException;

/// `INPUT_RAW_BUFFER_SIZE` do libyaml: o `read` é chamado com o espaço que sobra neste buffer.
const RAW_BUFFER_SIZE: usize = 16384;

/// Fonte de bytes: recebe o espaço livre e devolve até esse tanto (vazio indica o fim).
pub type Source = Box<dyn FnMut(usize) -> Result<Vec<u8>, PyException>>;

#[derive(Clone, Copy, Default)]
pub(super) struct SimpleKey {
    pub possible: bool,
    pub required: bool,
    pub token_number: usize,
    pub mark: Mark,
}

pub struct Parser {
    source: Source,
    raw: Vec<u8>,
    raw_ptr: usize,
    eof: bool,
    pub(super) encoding: Option<Encoding>,
    offset: usize,
    buffer: Vec<char>,
    bpos: usize,
    pub(super) mark: Mark,
    pub(super) error: bool,
    // scanner
    pub(super) stream_start_produced: bool,
    pub(super) stream_end_produced: bool,
    pub(super) flow_level: usize,
    pub(super) tokens: VecDeque<Token>,
    pub(super) tokens_parsed: usize,
    pub(super) token_available: bool,
    pub(super) indent: i64,
    pub(super) indents: Vec<i64>,
    pub(super) simple_key_allowed: bool,
    pub(super) simple_keys: Vec<SimpleKey>,
    // parser
    pub(super) state: ParseState,
    pub(super) states: Vec<ParseState>,
    pub(super) marks: Vec<Mark>,
    pub(super) tag_directives: Vec<(String, String)>,
}

impl Parser {
    pub fn new(source: Source) -> Parser {
        Parser {
            source,
            raw: Vec::new(),
            raw_ptr: 0,
            eof: false,
            encoding: None,
            offset: 0,
            buffer: Vec::new(),
            bpos: 0,
            mark: Mark::default(),
            error: false,
            stream_start_produced: false,
            stream_end_produced: false,
            flow_level: 0,
            tokens: VecDeque::new(),
            tokens_parsed: 0,
            token_available: false,
            indent: -1,
            indents: Vec::new(),
            simple_key_allowed: false,
            simple_keys: Vec::new(),
            state: ParseState::StreamStart,
            states: Vec::new(),
            marks: Vec::new(),
            tag_directives: Vec::new(),
        }
    }

    /// Entrada que já está toda na memória (`yaml_parser_set_input_string`).
    pub fn from_bytes(data: Vec<u8>) -> Parser {
        let mut pos = 0;
        Parser::new(Box::new(move |size| {
            let n = size.min(data.len() - pos);
            let chunk = data[pos..pos + n].to_vec();
            pos += n;
            Ok(chunk)
        }))
    }

    pub(super) fn unread(&self) -> usize {
        self.buffer.len().saturating_sub(self.bpos)
    }

    /// O caractere `k` posições adiante; o NUL do fim do fluxo vale também além do buffer.
    pub(super) fn ch(&self, k: usize) -> char {
        self.buffer.get(self.bpos + k).copied().unwrap_or('\0')
    }

    /// `CACHE(parser, n)`: garante `n` caracteres não lidos (ou o NUL do fim).
    pub(super) fn cache(&mut self, n: usize) -> Result<(), YamlError> {
        if self.unread() >= n {
            Ok(())
        } else {
            self.update_buffer(n)
        }
    }

    pub(super) fn skip(&mut self) {
        self.mark.index += 1;
        self.mark.column += 1;
        self.bpos += 1;
    }

    pub(super) fn skip_line(&mut self) {
        let width = if self.ch(0) == '\r' && self.ch(1) == '\n' { 2 } else { 1 };
        if width == 2 || super::types::is_break(self.ch(0)) {
            self.mark.index += width;
            self.mark.column = 0;
            self.mark.line += 1;
            self.bpos += width;
        }
    }

    /// `READ`: copia um caractere para `out`.
    pub(super) fn read_char(&mut self, out: &mut String) {
        out.push(self.ch(0));
        self.skip();
    }

    /// `READ_LINE`: CR LF, CR, LF e NEL viram LF; LS e PS ficam como estão.
    pub(super) fn read_line(&mut self, out: &mut String) {
        let c = self.ch(0);
        if (c == '\r' && self.ch(1) == '\n') || super::types::is_break(c) {
            out.push(if c == '\u{2028}' || c == '\u{2029}' { c } else { '\n' });
            self.skip_line();
        }
    }

    fn update_raw_buffer(&mut self) -> Result<(), YamlError> {
        if self.raw_ptr == 0 && self.raw.len() == RAW_BUFFER_SIZE {
            return Ok(());
        }
        if self.eof {
            return Ok(());
        }
        if self.raw_ptr > 0 {
            self.raw.drain(..self.raw_ptr);
            self.raw_ptr = 0;
        }
        let capacity = RAW_BUFFER_SIZE - self.raw.len();
        let chunk = (self.source)(capacity).map_err(YamlError::Py)?;
        let n = chunk.len().min(capacity);
        self.raw.extend_from_slice(&chunk[..n]);
        if n == 0 {
            self.eof = true;
        }
        Ok(())
    }

    fn determine_encoding(&mut self) -> Result<(), YamlError> {
        while !self.eof && self.raw.len() - self.raw_ptr < 3 {
            self.update_raw_buffer()?;
        }
        let rest = &self.raw[self.raw_ptr..];
        let (encoding, bom) = if rest.starts_with(&[0xFF, 0xFE]) {
            (Encoding::Utf16Le, 2)
        } else if rest.starts_with(&[0xFE, 0xFF]) {
            (Encoding::Utf16Be, 2)
        } else if rest.starts_with(&[0xEF, 0xBB, 0xBF]) {
            (Encoding::Utf8, 3)
        } else {
            (Encoding::Utf8, 0)
        };
        self.encoding = Some(encoding);
        self.raw_ptr += bom;
        self.offset += bom;
        Ok(())
    }

    fn reader_error(&self, problem: &'static str, offset: usize, value: i64) -> YamlError {
        YamlError::Reader { problem, offset, value }
    }

    /// Decodifica um caractere do buffer cru: `Ok(None)` quando faltam bytes (e ainda não é o fim).
    fn decode_char(&self) -> Result<Option<(u32, usize)>, YamlError> {
        let raw = &self.raw[self.raw_ptr..];
        let off = self.offset;
        match self.encoding.unwrap_or(Encoding::Utf8) {
            Encoding::Utf8 => {
                let octet = raw[0];
                let width = match octet {
                    o if o & 0x80 == 0 => 1,
                    o if o & 0xE0 == 0xC0 => 2,
                    o if o & 0xF0 == 0xE0 => 3,
                    o if o & 0xF8 == 0xF0 => 4,
                    _ => 0,
                };
                if width == 0 {
                    return Err(self.reader_error("invalid leading UTF-8 octet", off, i64::from(octet)));
                }
                if width > raw.len() {
                    if self.eof {
                        return Err(self.reader_error("incomplete UTF-8 octet sequence", off, -1));
                    }
                    return Ok(None);
                }
                let mut value = match width {
                    1 => u32::from(octet & 0x7F),
                    2 => u32::from(octet & 0x1F),
                    3 => u32::from(octet & 0x0F),
                    _ => u32::from(octet & 0x07),
                };
                for (k, &octet) in raw.iter().enumerate().take(width).skip(1) {
                    if octet & 0xC0 != 0x80 {
                        return Err(self.reader_error("invalid trailing UTF-8 octet", off + k, i64::from(octet)));
                    }
                    value = (value << 6) + u32::from(octet & 0x3F);
                }
                let proper = match width {
                    1 => true,
                    2 => value >= 0x80,
                    3 => value >= 0x800,
                    _ => value >= 0x10000,
                };
                if !proper {
                    return Err(self.reader_error("invalid length of a UTF-8 sequence", off, -1));
                }
                if (0xD800..=0xDFFF).contains(&value) || value > 0x10FFFF {
                    return Err(self.reader_error("invalid Unicode character", off, i64::from(value)));
                }
                Ok(Some((value, width)))
            }
            utf16 => {
                let unit = |i: usize| -> u32 {
                    if utf16 == Encoding::Utf16Le {
                        u32::from(raw[i]) | (u32::from(raw[i + 1]) << 8)
                    } else {
                        u32::from(raw[i + 1]) | (u32::from(raw[i]) << 8)
                    }
                };
                if raw.len() < 2 {
                    if self.eof {
                        return Err(self.reader_error("incomplete UTF-16 character", off, -1));
                    }
                    return Ok(None);
                }
                let mut value = unit(0);
                if value & 0xFC00 == 0xDC00 {
                    return Err(self.reader_error("unexpected low surrogate area", off, i64::from(value)));
                }
                if value & 0xFC00 == 0xD800 {
                    if raw.len() < 4 {
                        if self.eof {
                            return Err(self.reader_error("incomplete UTF-16 surrogate pair", off, -1));
                        }
                        return Ok(None);
                    }
                    let value2 = unit(2);
                    if value2 & 0xFC00 != 0xDC00 {
                        return Err(self.reader_error("expected low surrogate area", off + 2, i64::from(value2)));
                    }
                    value = 0x10000 + ((value & 0x3FF) << 10) + (value2 & 0x3FF);
                    Ok(Some((value, 4)))
                } else {
                    Ok(Some((value, 2)))
                }
            }
        }
    }

    /// `yaml_parser_update_buffer`: decodifica o buffer cru até haver `length` caracteres.
    pub(super) fn update_buffer(&mut self, length: usize) -> Result<(), YamlError> {
        if self.eof && self.raw_ptr == self.raw.len() {
            return Ok(());
        }
        if self.unread() >= length {
            return Ok(());
        }
        if self.encoding.is_none() {
            self.determine_encoding()?;
        }
        if self.bpos > 0 {
            self.buffer.drain(..self.bpos);
            self.bpos = 0;
        }
        let mut first = true;
        while self.unread() < length {
            if !first || self.raw_ptr == self.raw.len() {
                self.update_raw_buffer()?;
            }
            first = false;
            while self.raw_ptr != self.raw.len() {
                let Some((value, width)) = self.decode_char()? else { break };
                let allowed = matches!(value, 0x09 | 0x0A | 0x0D | 0x20..=0x7E | 0x85 | 0xA0..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF);
                if !allowed {
                    return Err(self.reader_error("control characters are not allowed", self.offset, i64::from(value)));
                }
                self.raw_ptr += width;
                self.offset += width;
                self.buffer.push(char::from_u32(value).unwrap_or('\u{FFFD}'));
            }
            if self.eof {
                self.buffer.push('\0');
                return Ok(());
            }
        }
        Ok(())
    }
}
