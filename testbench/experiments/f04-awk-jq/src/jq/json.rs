//! JSON com o comportamento observável do jq 1.7.1, por cima dos valores do jaq (`jaq_json::Val`).
//!
//! - Leitor: porte do autômato de `src/jv_parse.c` (mesmas mensagens de erro, mesma contagem de linha e
//!   coluna), produzindo `Val`. Números literais viram `Num::Dec` com a forma canônica do decNumber
//!   (`1e2` vira `1E+2`), que é o que o jq 1.7.1 imprime para literais não modificados.
//! - Escritor: porte de `jv_dump_term` (indentação, `--tab`, `-S`, `-a`, escapes).
//! - Números calculados: porte de `jvp_dtoa_fmt` (17 dígitos significativos, expoente com sinal).

use std::fmt::Write as _;

use jaq_json::{Map, Num, Rc, Val};

// ---------------------------------------------------------------------------------------------
// Números
// ---------------------------------------------------------------------------------------------

/// Literal numérico como o decNumber o entende.
#[derive(Clone, Debug, PartialEq)]
pub enum DecLit {
    Finite { neg: bool, coeff: String, exp: i64 },
    Inf { neg: bool },
    NaN,
}

/// `decNumberFromString`: sinal opcional, dígitos com ponto opcional, expoente opcional; ou Inf/NaN.
pub fn parse_dec(s: &str) -> Option<DecLit> {
    let b = s.as_bytes();
    let mut i = 0;
    let mut neg = false;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        neg = b[i] == b'-';
        i += 1;
    }
    let rest = &s[i..];
    let lower = rest.to_ascii_lowercase();
    if lower == "inf" || lower == "infinity" {
        return Some(DecLit::Inf { neg });
    }
    if lower.starts_with("nan") || lower.starts_with("snan") {
        let digits = lower.trim_start_matches('s').trim_start_matches("nan");
        if digits.bytes().all(|c| c.is_ascii_digit()) {
            return Some(DecLit::NaN);
        }
        return None;
    }
    let mut digits = String::new();
    let mut frac_digits: i64 = 0;
    let mut seen_dot = false;
    let mut any_digit = false;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_digit() {
            digits.push(c as char);
            any_digit = true;
            if seen_dot {
                frac_digits += 1;
            }
        } else if c == b'.' && !seen_dot {
            seen_dot = true;
        } else {
            break;
        }
        i += 1;
    }
    if !any_digit {
        return None;
    }
    let mut exp: i64 = 0;
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        let mut eneg = false;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            eneg = b[i] == b'-';
            i += 1;
        }
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if start == i {
            return None;
        }
        let e: i64 = s[start..i].parse().ok()?;
        exp = if eneg { -e } else { e };
    }
    if i != b.len() {
        return None;
    }
    let trimmed = digits.trim_start_matches('0');
    let coeff = if trimmed.is_empty() { "0".to_string() } else { trimmed.to_string() };
    Some(DecLit::Finite { neg, coeff, exp: exp - frac_digits })
}

/// `decNumberToString` (to-scientific-string).
pub fn dec_to_string(neg: bool, coeff: &str, exp: i64) -> String {
    let mut out = String::new();
    if neg {
        out.push('-');
    }
    let n = coeff.len() as i64;
    let adjusted = exp + n - 1;
    if exp <= 0 && adjusted >= -6 {
        if exp == 0 {
            out.push_str(coeff);
        } else {
            let point = n + exp; // dígitos antes do ponto
            if point > 0 {
                out.push_str(&coeff[..point as usize]);
                out.push('.');
                out.push_str(&coeff[point as usize..]);
            } else {
                out.push_str("0.");
                for _ in 0..(-point) {
                    out.push('0');
                }
                out.push_str(coeff);
            }
        }
    } else {
        out.push_str(&coeff[..1]);
        if n > 1 {
            out.push('.');
            out.push_str(&coeff[1..]);
        }
        out.push('E');
        if adjusted >= 0 {
            out.push('+');
        }
        let _ = write!(out, "{adjusted}");
    }
    out
}

/// Literal numérico (da entrada ou do programa) para `Val`, preservando a forma canônica do jq.
pub fn literal_to_val(lit: &DecLit) -> Val {
    match lit {
        DecLit::NaN => Val::Num(Num::Float(f64::NAN)),
        DecLit::Inf { neg } => Val::Num(Num::Float(if *neg { f64::NEG_INFINITY } else { f64::INFINITY })),
        DecLit::Finite { neg, coeff, exp } => {
            if *exp == 0 && !(*neg && coeff == "0") {
                let text = if *neg { format!("-{coeff}") } else { coeff.clone() };
                if let Some(n) = Num::from_str_radix(&text, 10) {
                    return Val::Num(n);
                }
            }
            Val::Num(Num::Dec(Rc::new(dec_to_string(*neg, coeff, *exp))))
        }
    }
}

/// `jvp_dtoa_fmt`: formatação de double do jq (dígitos mais curtos que fazem ida e volta).
pub fn dtoa_fmt(x: f64) -> String {
    if x.is_nan() {
        return "null".into();
    }
    let x = x.clamp(-f64::MAX, f64::MAX);
    if x == 0.0 {
        return if x.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    // `{:e}` dá a mantissa mais curta que faz ida e volta, igual ao modo 0 do dtoa.
    let sci = format!("{:e}", x.abs());
    let (mant, e) = sci.split_once('e').expect("formato científico");
    let e: i32 = e.parse().expect("expoente");
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let ndig = digits.len() as i32;
    let decpt = e + 1;
    let mut out = String::new();
    if x < 0.0 {
        out.push('-');
    }
    if decpt <= -4 || decpt > ndig + 15 {
        out.push_str(&digits[..1]);
        if ndig > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('e');
        let ex = decpt - 1;
        if ex < 0 {
            out.push('-');
        } else {
            out.push('+');
        }
        let ex = ex.abs();
        // Pelo menos dois dígitos no expoente.
        let _ = write!(out, "{ex:02}");
    } else if decpt <= 0 {
        out.push_str("0.");
        for _ in 0..(-decpt) {
            out.push('0');
        }
        out.push_str(&digits);
    } else if decpt >= ndig {
        out.push_str(&digits);
        for _ in 0..(decpt - ndig) {
            out.push('0');
        }
    } else {
        out.push_str(&digits[..decpt as usize]);
        out.push('.');
        out.push_str(&digits[decpt as usize..]);
    }
    out
}

/// Texto de um número como o jq 1.7.1 imprime.
pub fn num_to_string(n: &Num) -> String {
    match n {
        Num::Int(i) => i.to_string(),
        Num::BigInt(b) => b.to_string(),
        Num::Float(f) => dtoa_fmt(*f),
        Num::Dec(s) => match parse_dec(s) {
            Some(DecLit::Finite { neg, coeff, exp }) => dec_to_string(neg, &coeff, exp),
            Some(DecLit::NaN) => "null".into(),
            Some(DecLit::Inf { neg }) => dtoa_fmt(if neg { f64::NEG_INFINITY } else { f64::INFINITY }),
            None => s.to_string(),
        },
    }
}

/// Valor numérico em f64 (o que o jq usa em comparações e aritmética).
pub fn num_to_f64(n: &Num) -> f64 {
    match n {
        Num::Int(i) => *i as f64,
        Num::BigInt(b) => b.to_string().parse().unwrap_or(f64::NAN),
        Num::Float(f) => *f,
        Num::Dec(s) => s.parse().unwrap_or(f64::NAN),
    }
}

// ---------------------------------------------------------------------------------------------
// Escritor
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct DumpOpts {
    /// Espaços por nível; 0 = compacto. Ignorado com `tab`.
    pub indent: usize,
    pub tab: bool,
    pub sort_keys: bool,
    pub ascii: bool,
}

impl DumpOpts {
    pub const COMPACT: DumpOpts = DumpOpts { indent: 0, tab: false, sort_keys: false, ascii: false };

    fn pretty(&self) -> bool {
        self.tab || self.indent > 0
    }
}

pub fn dump(v: &Val, opts: &DumpOpts) -> String {
    let mut out = String::new();
    dump_into(&mut out, v, opts, 0);
    out
}

fn put_indent(out: &mut String, n: usize, opts: &DumpOpts) {
    if opts.tab {
        for _ in 0..n {
            out.push('\t');
        }
    } else {
        for _ in 0..n * opts.indent {
            out.push(' ');
        }
    }
}

pub fn dump_string(out: &mut String, bytes: &[u8], ascii: bool) {
    let s = String::from_utf8_lossy(bytes);
    out.push('"');
    for c in s.chars() {
        let code = c as u32;
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\n' => out.push_str("\\n"),
            '\u{c}' => out.push_str("\\f"),
            _ if code < 0x20 || code == 0x7f => {
                let _ = write!(out, "\\u{code:04x}");
            }
            _ if code > 0x7e && ascii => {
                if code <= 0xffff {
                    let _ = write!(out, "\\u{code:04x}");
                } else {
                    let c2 = code - 0x10000;
                    let _ = write!(out, "\\u{:04x}\\u{:04x}", 0xD800 | ((c2 & 0xffc00) >> 10), 0xDC00 | (c2 & 0x3ff));
                }
            }
            _ => out.push(c),
        }
    }
    out.push('"');
}

fn dump_into(out: &mut String, v: &Val, opts: &DumpOpts, level: usize) {
    match v {
        Val::Null => out.push_str("null"),
        Val::Bool(true) => out.push_str("true"),
        Val::Bool(false) => out.push_str("false"),
        Val::Num(n) => out.push_str(&num_to_string(n)),
        Val::TStr(b) | Val::BStr(b) => dump_string(out, b, opts.ascii),
        Val::Arr(a) => {
            if a.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            if opts.pretty() {
                out.push('\n');
                put_indent(out, level + 1, opts);
            }
            for (i, x) in a.iter().enumerate() {
                if i != 0 {
                    if opts.pretty() {
                        out.push_str(",\n");
                        put_indent(out, level + 1, opts);
                    } else {
                        out.push(',');
                    }
                }
                dump_into(out, x, opts, level + 1);
            }
            if opts.pretty() {
                out.push('\n');
                put_indent(out, level, opts);
            }
            out.push(']');
        }
        Val::Obj(o) => {
            if o.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            if opts.pretty() {
                out.push('\n');
                put_indent(out, level + 1, opts);
            }
            let mut entries: Vec<(&Val, &Val)> = o.iter().collect();
            if opts.sort_keys {
                entries.sort_by(|a, b| key_bytes(a.0).cmp(key_bytes(b.0)));
            }
            for (i, (k, x)) in entries.into_iter().enumerate() {
                if i != 0 {
                    if opts.pretty() {
                        out.push_str(",\n");
                        put_indent(out, level + 1, opts);
                    } else {
                        out.push(',');
                    }
                }
                match k {
                    Val::TStr(b) | Val::BStr(b) => dump_string(out, b, opts.ascii),
                    // O jaq aceita chave não string; o jq nunca chega aqui (erro antes).
                    other => dump_string(out, dump(other, &DumpOpts::COMPACT).as_bytes(), opts.ascii),
                }
                out.push(':');
                if opts.pretty() {
                    out.push(' ');
                }
                dump_into(out, x, opts, level + 1);
            }
            if opts.pretty() {
                out.push('\n');
                put_indent(out, level, opts);
            }
            out.push('}');
        }
    }
}

fn key_bytes(k: &Val) -> &[u8] {
    match k {
        Val::TStr(b) | Val::BStr(b) => b,
        _ => &[],
    }
}

/// `jv_dump_string_trunc` com buffer de 15 bytes: 11 bytes e "..." quando não cabe.
pub fn dump_trunc(v: &Val) -> String {
    let s = dump(v, &DumpOpts::COMPACT);
    const BUF: usize = 15;
    if s.len() < BUF {
        return s;
    }
    // O jq corta por byte, mesmo no meio de um caractere UTF-8.
    let mut out = s.as_bytes()[..BUF - 1 - 3].to_vec();
    out.extend_from_slice(b"...");
    String::from_utf8_lossy(&out).into_owned()
}

pub fn type_name(v: &Val) -> &'static str {
    match v {
        Val::Null => "null",
        Val::Bool(_) => "boolean",
        Val::Num(_) => "number",
        Val::TStr(_) | Val::BStr(_) => "string",
        Val::Arr(_) => "array",
        Val::Obj(_) => "object",
    }
}

// ---------------------------------------------------------------------------------------------
// Leitor (porte de jv_parse.c, sem o modo --stream)
// ---------------------------------------------------------------------------------------------

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

pub struct Parser {
    stack: Vec<Frame>,
    next: Option<Val>,
    token: Vec<u8>,
    pub line: i64,
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

/// Resultado de `Parser::next`.
pub enum Next {
    Value(Val),
    /// Erro de parse, já formatado com linha e coluna.
    Error(String),
    /// Precisa de mais entrada (buffer parcial esgotado) ou acabou.
    None,
}

const UTF8_BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

impl Parser {
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

    pub fn remaining(&self) -> usize {
        if !self.has_buf { 0 } else { self.buf.len() - self.pos }
    }

    pub fn set_buf(&mut self, buf: &[u8], partial: bool) {
        let mut buf = buf;
        while !buf.is_empty() && self.bom_pos < UTF8_BOM.len() {
            if buf[0] == UTF8_BOM[self.bom_pos] {
                buf = &buf[1..];
                self.bom_pos += 1;
            } else {
                if self.bom_pos == 0 {
                    self.bom_pos = UTF8_BOM.len();
                } else {
                    self.bom_bad = true;
                    self.bom_pos = UTF8_BOM.len();
                }
            }
        }
        self.buf = buf.to_vec();
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
                if self.stack.len() >= 256 {
                    return Err("Exceeds depth limit for parsing");
                }
                if self.next.is_some() {
                    return Err("Expected separator between values");
                }
                self.stack.push(Frame::Arr(Vec::new()));
            }
            b'{' => {
                if self.stack.len() >= 256 {
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
                } else if let Some(Frame::Arr(a)) = self.stack.last()
                    && !a.is_empty()
                {
                    return Err("Expected another array element");
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
        let tok = std::mem::take(&mut self.token);
        let mut out: Vec<u8> = Vec::with_capacity(tok.len());
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
        let s = String::from_utf8_lossy(&out).into_owned();
        self.value(Val::utf8_str(s))
    }

    fn check_literal(&mut self) -> Result<(), &'static str> {
        if self.token.is_empty() {
            return Ok(());
        }
        let tok = std::mem::take(&mut self.token);
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
                match parse_dec(&text) {
                    Some(lit) => self.value(literal_to_val(&lit)),
                    None => Err("Invalid numeric literal"),
                }
            }
        }
    }

    fn check_done(&mut self) -> Option<Val> {
        if self.stack.is_empty() { self.next.take() } else { None }
    }

    fn check_truncation(&self) -> bool {
        !self.last_ch_was_ws
            && (!self.stack.is_empty() || !self.token.is_empty() || matches!(self.next, Some(Val::Num(_))))
    }

    /// `scan`: Ok(Some(v)) quando sai um valor.
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
            if self.st == St::Normal
                && let Some(v) = self.check_done()
            {
                return Ok(Some(v));
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
        let mut last_ch = 0u8;
        while self.pos < self.buf.len() {
            let ch = self.buf[self.pos];
            self.pos += 1;
            last_ch = ch;
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
                        self.st = St::WaitingForRs;
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
        let _ = last_ch;
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

/// `jv_parse_sized`: exatamente um valor (usado por --argjson, --jsonargs e fromjson).
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

/// Todos os valores de um texto (usado por --slurpfile). Erro sem o sufixo "while parsing".
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

#[cfg(test)]
mod tests {
    use super::*;

    fn canon(s: &str) -> String {
        match parse_dec(s).unwrap() {
            DecLit::Finite { neg, coeff, exp } => dec_to_string(neg, &coeff, exp),
            other => format!("{other:?}"),
        }
    }

    #[test]
    fn decnumber_canonical_forms_match_jq() {
        assert_eq!(canon("1.0"), "1.0");
        assert_eq!(canon("1.50"), "1.50");
        assert_eq!(canon("100"), "100");
        assert_eq!(canon("1e2"), "1E+2");
        assert_eq!(canon("1E-2"), "0.01");
        assert_eq!(canon("0.10e1"), "1.0");
        assert_eq!(canon("3.000"), "3.000");
        assert_eq!(canon("1e1000"), "1E+1000");
        assert_eq!(canon("1e-7"), "1E-7");
        assert_eq!(canon("1e-5"), "0.00001");
        assert_eq!(canon("1.5e300"), "1.5E+300");
        assert_eq!(canon("-0"), "-0");
    }

    #[test]
    fn dtoa_matches_jq() {
        assert_eq!(dtoa_fmt(0.30000000000000004), "0.30000000000000004");
        assert_eq!(dtoa_fmt(1e20), "1e+20");
        assert_eq!(dtoa_fmt(12345678901234567890123.0), "12345678901234568000000");
        assert_eq!(dtoa_fmt(f64::INFINITY), "1.7976931348623157e+308");
        assert_eq!(dtoa_fmt(3.0), "3");
        assert_eq!(dtoa_fmt(0.5), "0.5");
        assert_eq!(dtoa_fmt(1e19), "1e+19");
        assert_eq!(dtoa_fmt(-1.5), "-1.5");
        assert_eq!(dtoa_fmt(1e-5), "1e-05");
    }

    #[test]
    fn parser_errors_match_jq() {
        let mut p = Parser::new(false);
        p.set_buf(b"{\"a\": 1}\n{bad json\n", false);
        assert!(matches!(p.next(), Next::Value(_)));
        match p.next() {
            Next::Error(e) => assert_eq!(e, "Invalid numeric literal at line 2, column 5"),
            _ => panic!("esperava erro"),
        }
        let mut p = Parser::new(false);
        p.set_buf(b"[1, 2,]", false);
        match p.next() {
            Next::Error(e) => assert_eq!(e, "Expected another array element at line 1, column 7"),
            _ => panic!("esperava erro"),
        }
        let mut p = Parser::new(false);
        p.set_buf(b"[1, 2", false);
        match p.next() {
            Next::Error(e) => assert_eq!(e, "Unfinished JSON term at EOF at line 1, column 5"),
            _ => panic!("esperava erro"),
        }
        assert_eq!(
            parse_single(b"{bad").unwrap_err(),
            "Invalid numeric literal at EOF at line 1, column 4 (while parsing '{bad')"
        );
    }

    #[test]
    fn dump_formats() {
        let v = parse_single(br#"{"a":[1,{"b":2}],"c":{}}"#).unwrap();
        let pretty = DumpOpts { indent: 2, tab: false, sort_keys: false, ascii: false };
        assert_eq!(dump(&v, &pretty), "{\n  \"a\": [\n    1,\n    {\n      \"b\": 2\n    }\n  ],\n  \"c\": {}\n}");
        assert_eq!(dump(&v, &DumpOpts::COMPACT), r#"{"a":[1,{"b":2}],"c":{}}"#);
        let s = parse_single(r#""\u0001\u007fé😀""#.as_bytes()).unwrap();
        let ascii = DumpOpts { ascii: true, ..DumpOpts::COMPACT };
        // Montado com format! pra o fonte não carregar escapes \u literais.
        let bs = '\\';
        let expected = format!("\"{bs}u0001{bs}u007f{bs}u00e9{bs}ud83d{bs}ude00\"");
        assert_eq!(dump(&s, &ascii), expected);
    }
}
