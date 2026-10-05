//! Porte pseudo-linus: leitor de JSON igual ao `jv_parse.c` do jq 1.7.1 (substitui o `read.rs` do
//! jaq-json, que era o `hifijson`).
//!
//! Autômato byte a byte com as mesmas mensagens e a mesma contagem de linha e coluna, BOM, modo
//! `--seq` (RS 0x1e) e limite de profundidade 256. Números viram literais ([`Num::from_literal`]),
//! strings com UTF-8 inválido ganham U+FFFD.

use crate::{Map, Num, Rc, Val};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// `MAX_PARSING_DEPTH`.
pub const MAX_PARSING_DEPTH: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum St {
    Normal,
    Str,
    StrEscape,
    WaitingForRs,
}

enum Frame {
    Arr(Vec<Val>),
    Obj(Map),
    Key(Val),
}

/// Parser incremental (`struct jv_parser`).
pub struct Parser {
    stack: Vec<Frame>,
    next: Option<Val>,
    token: Vec<u8>,
    /// Linha corrente (para as mensagens de erro).
    pub line: i64,
    /// Coluna corrente (para as mensagens de erro).
    pub column: i64,
    st: St,
    last_ch_was_ws: bool,
    seq: bool,
    buf: Vec<u8>,
    pos: usize,
    partial: bool,
    has_buf: bool,
    eof: bool,
    bom_pos: usize,
    bom_bad: bool,
}

/// Resultado de [`Parser::next`].
pub enum Next {
    /// Um valor completo.
    Value(Val),
    /// Erro de parse, já formatado com linha e coluna.
    Error(String),
    /// Precisa de mais entrada (buffer parcial esgotado) ou acabou.
    None,
}

const UTF8_BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

impl Parser {
    /// `jv_parser_new`; `seq` liga o modo `--seq` (RS 0x1e).
    pub fn new(seq: bool) -> Parser {
        Parser {
            stack: Vec::new(),
            next: None,
            token: Vec::new(),
            line: 1,
            column: 0,
            st: if seq { St::WaitingForRs } else { St::Normal },
            last_ch_was_ws: false,
            seq,
            buf: Vec::new(),
            pos: 0,
            partial: false,
            has_buf: false,
            eof: false,
            bom_pos: 0,
            bom_bad: false,
        }
    }

    fn reset(&mut self) {
        self.stack.clear();
        self.next = None;
        self.token.clear();
        self.st = St::Normal;
    }

    /// Bytes ainda não consumidos do buffer atual (`jv_parser_remaining`).
    pub fn remaining(&self) -> usize {
        if !self.has_buf {
            0
        } else {
            self.buf.len() - self.pos
        }
    }

    /// `jv_parser_set_buf`.
    pub fn set_buf(&mut self, buf: &[u8], partial: bool) {
        let mut buf = buf;
        while !buf.is_empty() && self.bom_pos < UTF8_BOM.len() {
            if buf[0] == UTF8_BOM[self.bom_pos] {
                buf = &buf[1..];
                self.bom_pos += 1;
            } else {
                if self.bom_pos != 0 {
                    self.bom_bad = true;
                }
                self.bom_pos = UTF8_BOM.len();
            }
        }
        self.buf.clear();
        self.buf.extend_from_slice(buf);
        self.pos = 0;
        self.partial = partial;
        self.has_buf = true;
    }

    fn value(&mut self, v: Val) -> Result<(), &'static str> {
        if self.next.is_some() {
            return Err("Expected separator between values");
        }
        self.next = Some(v);
        Ok(())
    }

    fn parse_token(&mut self, ch: u8) -> Result<(), &'static str> {
        match ch {
            b'[' => {
                if self.stack.len() >= MAX_PARSING_DEPTH {
                    return Err("Exceeds depth limit for parsing");
                }
                if self.next.is_some() {
                    return Err("Expected separator between values");
                }
                self.stack.push(Frame::Arr(Vec::new()));
            }
            b'{' => {
                if self.stack.len() >= MAX_PARSING_DEPTH {
                    return Err("Exceeds depth limit for parsing");
                }
                if self.next.is_some() {
                    return Err("Expected separator between values");
                }
                self.stack.push(Frame::Obj(Map::default()));
            }
            b':' => {
                let Some(next) = self.next.take() else {
                    return Err("Expected string key before ':'");
                };
                if !matches!(self.stack.last(), Some(Frame::Obj(_))) {
                    self.next = Some(next);
                    return Err("':' not as part of an object");
                }
                if !matches!(next, Val::TStr(_)) {
                    self.next = Some(next);
                    return Err("Object keys must be strings");
                }
                self.stack.push(Frame::Key(next));
            }
            b',' => {
                let Some(next) = self.next.take() else {
                    return Err("Expected value before ','");
                };
                match self.stack.last_mut() {
                    None => {
                        self.next = Some(next);
                        return Err("',' not as part of an object or array");
                    }
                    Some(Frame::Arr(a)) => a.push(next),
                    Some(Frame::Key(_)) => {
                        let Some(Frame::Key(k)) = self.stack.pop() else { unreachable!() };
                        if let Some(Frame::Obj(o)) = self.stack.last_mut() {
                            o.insert(k, next);
                        }
                    }
                    Some(Frame::Obj(_)) => {
                        self.next = Some(next);
                        return Err("Objects must consist of key:value pairs");
                    }
                }
            }
            b']' => {
                let Some(Frame::Arr(_)) = self.stack.last() else {
                    return Err("Unmatched ']'");
                };
                if let Some(next) = self.next.take() {
                    if let Some(Frame::Arr(a)) = self.stack.last_mut() {
                        a.push(next);
                    }
                } else if let Some(Frame::Arr(a)) = self.stack.last() {
                    if !a.is_empty() {
                        return Err("Expected another array element");
                    }
                }
                let Some(Frame::Arr(a)) = self.stack.pop() else { unreachable!() };
                self.next = Some(Val::Arr(Rc::new(a)));
            }
            b'}' => {
                if self.stack.is_empty() {
                    return Err("Unmatched '}'");
                }
                if let Some(next) = self.next.take() {
                    if !matches!(self.stack.last(), Some(Frame::Key(_))) {
                        self.next = Some(next);
                        return Err("Objects must consist of key:value pairs");
                    }
                    let Some(Frame::Key(k)) = self.stack.pop() else { unreachable!() };
                    if let Some(Frame::Obj(o)) = self.stack.last_mut() {
                        o.insert(k, next);
                    }
                } else {
                    match self.stack.last() {
                        Some(Frame::Obj(o)) => {
                            if !o.is_empty() {
                                return Err("Expected another key-value pair");
                            }
                        }
                        _ => return Err("Unmatched '}'"),
                    }
                }
                let Some(Frame::Obj(o)) = self.stack.pop() else { unreachable!() };
                self.next = Some(Val::obj(o));
            }
            _ => {}
        }
        Ok(())
    }

    fn found_string(&mut self) -> Result<(), &'static str> {
        let tok = core::mem::take(&mut self.token);
        let mut out: Vec<u8> = Vec::new();
        let mut i = 0;
        while i < tok.len() {
            let c = tok[i];
            i += 1;
            if c == b'\\' {
                if i >= tok.len() {
                    return Err("Expected escape character at end of string");
                }
                let e = tok[i];
                i += 1;
                match e {
                    b'\\' | b'"' | b'/' => out.push(e),
                    b'b' => out.push(8),
                    b'f' => out.push(12),
                    b't' => out.push(b'\t'),
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b'u' => {
                        if i + 4 > tok.len() {
                            return Err("Invalid \\uXXXX escape");
                        }
                        let Some(mut cp) = unhex4(&tok[i..i + 4]) else {
                            return Err("Invalid characters in \\uXXXX escape");
                        };
                        i += 4;
                        if (0xD800..=0xDBFF).contains(&cp) {
                            if i + 6 > tok.len() || tok[i] != b'\\' || tok[i + 1] != b'u' {
                                return Err("Invalid \\uXXXX\\uXXXX surrogate pair escape");
                            }
                            let sur = unhex4(&tok[i + 2..i + 6]).unwrap_or(0);
                            if !(0xDC00..=0xDFFF).contains(&sur) {
                                return Err("Invalid \\uXXXX\\uXXXX surrogate pair escape");
                            }
                            i += 6;
                            cp = 0x10000 + (((cp - 0xD800) << 10) | (sur - 0xDC00));
                        }
                        if (0xDC00..=0xDFFF).contains(&cp) {
                            // Surrogate baixa sozinha: o jq troca por U+FFFD.
                            cp = 0xFFFD;
                        }
                        let ch = char::from_u32(cp).unwrap_or('\u{FFFD}');
                        let mut tmp = [0u8; 4];
                        out.extend_from_slice(ch.encode_utf8(&mut tmp).as_bytes());
                    }
                    _ => return Err("Invalid escape"),
                }
            } else {
                if c <= 0x1f {
                    return Err("Invalid string: control characters from U+0000 through U+001F must be escaped");
                }
                out.push(c);
            }
        }
        self.value(Val::from(utf8_lossy(&out)))
    }

    fn check_literal(&mut self) -> Result<(), &'static str> {
        if self.token.is_empty() {
            return Ok(());
        }
        let tok = core::mem::take(&mut self.token);
        let pattern: Option<(&[u8], Val)> = match tok[0] {
            b't' => Some((b"true", Val::Bool(true))),
            b'f' => Some((b"false", Val::Bool(false))),
            b'n' if tok.get(1) == Some(&b'u') => Some((b"null", Val::Null)),
            _ => None,
        };
        match pattern {
            Some((p, v)) => {
                if tok != p {
                    return Err("Invalid literal");
                }
                self.value(v)
            }
            None => {
                let text = String::from_utf8_lossy(&tok);
                match Num::from_literal(&text) {
                    Some(n) => self.value(Val::Num(n)),
                    None => Err("Invalid numeric literal"),
                }
            }
        }
    }

    fn check_done(&mut self) -> Option<Val> {
        if self.stack.is_empty() {
            self.next.take()
        } else {
            None
        }
    }

    fn check_truncation(&self) -> bool {
        !self.last_ch_was_ws && (!self.stack.is_empty() || !self.token.is_empty() || matches!(self.next, Some(Val::Num(_))))
    }

    fn scan(&mut self, ch: u8) -> Result<Option<Val>, &'static str> {
        self.column += 1;
        if ch == b'\n' {
            self.line += 1;
            self.column = 0;
        }
        if self.seq && ch == 0x1e {
            if self.check_truncation() {
                if self.check_literal().is_ok() && self.stack.is_empty() && matches!(self.next, Some(Val::Num(_))) {
                    return Err("Potentially truncated top-level numeric value");
                }
                return Err("Truncated value");
            }
            self.check_literal()?;
            if self.st == St::Normal {
                if let Some(v) = self.check_done() {
                    return Ok(Some(v));
                }
            }
            self.reset();
            return Ok(None);
        }
        let mut answer = None;
        self.last_ch_was_ws = false;
        if self.st == St::Normal {
            let cls = classify(ch);
            if cls == Cls::Ws {
                self.last_ch_was_ws = true;
            }
            if cls != Cls::Literal {
                self.check_literal()?;
                if let Some(v) = self.check_done() {
                    answer = Some(v);
                }
            }
            match cls {
                Cls::Literal => self.token.push(ch),
                Cls::Ws => {}
                Cls::Quote => self.st = St::Str,
                Cls::Structure => self.parse_token(ch)?,
            }
            if let Some(v) = self.check_done() {
                answer = Some(v);
            }
        } else if ch == b'"' && self.st == St::Str {
            self.found_string()?;
            self.st = St::Normal;
            if let Some(v) = self.check_done() {
                answer = Some(v);
            }
        } else {
            self.token.push(ch);
            self.st = if ch == b'\\' && self.st == St::Str { St::StrEscape } else { St::Str };
        }
        Ok(answer)
    }

    /// `jv_parser_next`.
    // O nome espelha o `jv_parser_next` do jq; o retorno é `Next`, não `Option`, então não cabe em
    // `Iterator`.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Next {
        if self.eof || !self.has_buf {
            return Next::None;
        }
        if self.bom_bad {
            if !self.seq {
                return Next::Error("Malformed BOM".into());
            }
            self.st = St::WaitingForRs;
            self.reset();
        }
        while self.pos < self.buf.len() {
            let ch = self.buf[self.pos];
            self.pos += 1;
            if self.st == St::WaitingForRs {
                if ch == b'\n' {
                    self.line += 1;
                    self.column = 0;
                } else {
                    self.column += 1;
                }
                if ch == 0x1e {
                    self.st = St::Normal;
                }
                continue;
            }
            match self.scan(ch) {
                Ok(Some(v)) => return Next::Value(v),
                Ok(None) => {}
                Err(msg) => {
                    if ch != 0x1e && self.seq {
                        let e = format!("{msg} at line {}, column {} (need RS to resync)", self.line, self.column);
                        self.reset();
                        self.st = St::WaitingForRs;
                        return Next::Error(e);
                    }
                    let e = format!("{msg} at line {}, column {}", self.line, self.column);
                    self.reset();
                    if !self.seq {
                        self.has_buf = false;
                        self.buf.clear();
                        self.pos = 0;
                    }
                    return Next::Error(e);
                }
            }
        }
        if self.partial {
            return Next::None;
        }
        self.eof = true;
        if self.st == St::WaitingForRs {
            return Next::Error(format!("Unfinished abandoned text at EOF at line {}, column {}", self.line, self.column));
        }
        if self.st != St::Normal {
            let e = format!("Unfinished string at EOF at line {}, column {}", self.line, self.column);
            self.reset();
            self.st = St::WaitingForRs;
            return Next::Error(e);
        }
        if let Err(msg) = self.check_literal() {
            let e = format!("{msg} at EOF at line {}, column {}", self.line, self.column);
            self.reset();
            self.st = St::WaitingForRs;
            return Next::Error(e);
        }
        if !self.stack.is_empty() {
            let e = format!("Unfinished JSON term at EOF at line {}, column {}", self.line, self.column);
            self.reset();
            self.st = St::WaitingForRs;
            return Next::Error(e);
        }
        match self.next.take() {
            Some(v) => {
                if self.seq && !self.last_ch_was_ws && matches!(v, Val::Num(_)) {
                    return Next::Error(format!(
                        "Potentially truncated top-level numeric value at EOF at line {}, column {}",
                        self.line, self.column
                    ));
                }
                Next::Value(v)
            }
            None => Next::None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cls {
    Literal,
    Ws,
    Structure,
    Quote,
}

fn classify(c: u8) -> Cls {
    match c {
        b' ' | b'\t' | b'\r' | b'\n' => Cls::Ws,
        b'"' => Cls::Quote,
        b'[' | b',' | b']' | b'{' | b':' | b'}' => Cls::Structure,
        _ => Cls::Literal,
    }
}

/// `utf8_coding_length` do jq: 1 para ASCII, 0xff para byte de continuação, 0 para inválido.
fn coding_length(b: u8) -> u8 {
    match b {
        0x00..=0x7f => 1,
        0x80..=0xbf => 0xff,
        0xc0 | 0xc1 => 0,
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => 0,
    }
}

/// `jvp_utf8_next`: próximo código e quantos bytes ele ocupa; `None` no código quando a sequência é
/// inválida (byte solto, continuação errada, forma longa demais, surrogate, fora do Unicode).
pub fn utf8_next(s: &[u8]) -> (Option<u32>, usize) {
    let first = s[0];
    let mut length = coding_length(first) as usize;
    if first & 0x80 == 0 {
        return (Some(first as u32), 1);
    }
    if length == 0 || length == 0xff {
        return (None, 1);
    }
    if length > s.len() {
        return (None, s.len());
    }
    let bits = match length {
        2 => 0x1f,
        3 => 0x0f,
        _ => 0x07,
    };
    let mut cp: u32 = (first & bits) as u32;
    let mut ok = true;
    for (i, &ch) in s.iter().enumerate().take(length).skip(1) {
        if coding_length(ch) != 0xff {
            ok = false;
            length = i;
            break;
        }
        cp = (cp << 6) | (ch & 0x3f) as u32;
    }
    let first_cp = [0, 0, 0x80, 0x800, 0x10000][length.min(4)];
    if !ok || cp < first_cp || (0xD800..=0xDFFF).contains(&cp) || cp > 0x10FFFF {
        return (None, length);
    }
    (Some(cp), length)
}

/// `jv_string_sized` com `jvp_string_copy_replace_bad`: cada sequência inválida (no critério do
/// `jvp_utf8_next`) vira um U+FFFD.
pub fn utf8_lossy(bytes: &[u8]) -> String {
    if let Ok(s) = core::str::from_utf8(bytes) {
        return s.to_string();
    }
    let mut out = String::with_capacity(bytes.len() + 8);
    let mut i = 0;
    while i < bytes.len() {
        let (cp, len) = utf8_next(&bytes[i..]);
        out.push(cp.and_then(char::from_u32).unwrap_or('\u{FFFD}'));
        i += len;
    }
    out
}

fn unhex4(h: &[u8]) -> Option<u32> {
    let mut r = 0u32;
    for &c in h {
        let n = match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => return None,
        };
        r = (r << 4) | n as u32;
    }
    Some(r)
}

/// `jv_parse_sized`: exatamente um valor (`fromjson`, `--argjson`, `--jsonargs`). A mensagem de erro
/// já vem com o sufixo "(while parsing '...')".
pub fn parse_single(text: &[u8]) -> Result<Val, String> {
    let mut p = Parser::new(false);
    p.set_buf(text, false);
    let shown = String::from_utf8_lossy(text);
    let r = match p.next() {
        Next::Value(v) => match p.next() {
            Next::Value(_) => Err("Unexpected extra JSON values".to_string()),
            Next::Error(e) => Err(e),
            Next::None => Ok(v),
        },
        Next::Error(e) => Err(e),
        Next::None => Err("Expected JSON value".to_string()),
    };
    r.map_err(|e| format!("{e} (while parsing '{shown}')"))
}

/// Todos os valores de um texto (`--slurpfile`), parando no primeiro erro (sem o sufixo).
pub fn parse_all(text: &[u8]) -> Result<Vec<Val>, String> {
    let mut p = Parser::new(false);
    p.set_buf(text, false);
    let mut out = Vec::new();
    loop {
        match p.next() {
            Next::Value(v) => out.push(v),
            Next::Error(e) => return Err(e),
            Next::None => return Ok(out),
        }
    }
}
