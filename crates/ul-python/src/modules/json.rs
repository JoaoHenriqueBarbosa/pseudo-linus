//! Módulo `json` do CPython 3.13: `dumps` (sem indent, separadores `', '` e `': '`) e `loads`.
//!
//! Limitações conhecidas: inteiros fora de `i64` não são suportados no `loads`, e uma metade
//! substituta (surrogate) solitária num `\uXXXX` vira U+FFFD, porque `String` do Rust não
//! guarda surrogates.

use std::rc::Rc;

use crate::object::{float_repr, repr, Dict, Value};

// ---------------------------------------------------------------------------
// dumps
// ---------------------------------------------------------------------------

/// `json.dumps(v, ensure_ascii=...)`. O erro é a mensagem do `TypeError`/`ValueError`.
pub fn dumps(v: &Value, ensure_ascii: bool) -> Result<String, String> {
    let mut out = String::new();
    let mut stack: Vec<usize> = Vec::new();
    encode(v, ensure_ascii, &mut out, &mut stack)?;
    Ok(out)
}

fn float_json(x: f64) -> String {
    if x.is_nan() {
        "NaN".to_string()
    } else if x.is_infinite() {
        if x > 0.0 { "Infinity".to_string() } else { "-Infinity".to_string() }
    } else {
        float_repr(x)
    }
}

fn encode_str(s: &str, ensure_ascii: bool, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c if ensure_ascii && crate::object::char_surrogate(c).is_some() => {
                out.push_str(&format!("\\u{:04x}", crate::object::char_surrogate(c).unwrap_or(0)))
            }
            c if ensure_ascii && (c as u32) >= 0x7f => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{:04x}", unit));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn key_string(k: &Value) -> Result<String, String> {
    match k {
        Value::Str(s) => Ok(s.as_str().to_string()),
        Value::Int(_) => Ok(repr(k)),
        Value::Float(x) => Ok(float_json(*x)),
        Value::Bool(true) => Ok("true".to_string()),
        Value::Bool(false) => Ok("false".to_string()),
        Value::None => Ok("null".to_string()),
        other => Err(format!("keys must be str, int, float, bool or None, not {}", other.type_name())),
    }
}

fn enter(stack: &mut Vec<usize>, id: usize) -> Result<(), String> {
    if stack.contains(&id) {
        return Err("Circular reference detected".to_string());
    }
    // Profundidade além do limite do CPython: o chamador cai no codificador em Python (RecursionError).
    if stack.len() >= MAX_DEPTH {
        return Err("maximum recursion depth exceeded while encoding a JSON object".to_string());
    }
    stack.push(id);
    Ok(())
}

fn encode(v: &Value, ascii: bool, out: &mut String, stack: &mut Vec<usize>) -> Result<(), String> {
    match v {
        Value::None => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Int(_) => out.push_str(&repr(v)),
        Value::Float(x) => out.push_str(&float_json(*x)),
        Value::Str(s) => encode_str(s.as_str(), ascii, out),
        Value::List(l) => {
            let id = Rc::as_ptr(l) as *const () as usize;
            enter(stack, id)?;
            out.push('[');
            for (i, item) in l.borrow().iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                encode(item, ascii, out, stack)?;
            }
            out.push(']');
            stack.pop();
        }
        Value::Tuple(t) => {
            let id = Rc::as_ptr(t) as *const () as usize;
            enter(stack, id)?;
            out.push('[');
            for (i, item) in t.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                encode(item, ascii, out, stack)?;
            }
            out.push(']');
            stack.pop();
        }
        Value::Dict(d) => {
            let id = Rc::as_ptr(d) as *const () as usize;
            enter(stack, id)?;
            out.push('{');
            for (i, (k, val)) in d.borrow().iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                let key = key_string(k)?;
                encode_str(&key, ascii, out);
                out.push_str(": ");
                encode(val, ascii, out, stack)?;
            }
            out.push('}');
            stack.pop();
        }
        other => return Err(format!("Object of type {} is not JSON serializable", other.type_name())),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// loads
// ---------------------------------------------------------------------------

/// `json.JSONDecodeError`: a mensagem já inclui o sufixo `: line L column C (char N)`.
/// `syntax` guarda a mensagem crua e a posição dos erros de sintaxe (o que o `JSONDecodeError` recebe);
/// `None` marca o que o parser nativo não cobre (inteiro grande, surrogate solitário, profundidade), e que
/// o chamador deve entregar ao decodificador em Python.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonError {
    pub msg: String,
    pub syntax: Option<(String, usize)>,
}

impl JsonError {
    fn unsupported(msg: &str) -> JsonError {
        JsonError { msg: msg.to_string(), syntax: None }
    }
}

const MAX_DEPTH: usize = 900;

struct Parser {
    s: Vec<char>,
}

impl Parser {
    fn err(&self, msg: &str, pos: usize) -> JsonError {
        let pos = pos.min(self.s.len());
        let before = &self.s[..pos];
        let line = 1 + before.iter().filter(|&&c| c == '\n').count();
        let col = match before.iter().rposition(|&c| c == '\n') {
            Some(i) => pos - i,
            None => pos + 1,
        };
        JsonError { msg: format!("{msg}: line {line} column {col} (char {pos})"), syntax: Some((msg.to_string(), pos)) }
    }

    fn skip_ws(&self, mut i: usize) -> usize {
        while matches!(self.s.get(i), Some(' ' | '\t' | '\n' | '\r')) {
            i += 1;
        }
        i
    }

    fn starts_with(&self, i: usize, lit: &str) -> bool {
        let mut k = i;
        for c in lit.chars() {
            if self.s.get(k) != Some(&c) {
                return false;
            }
            k += 1;
        }
        true
    }

    fn value(&self, i: usize, depth: usize) -> Result<(Value, usize), JsonError> {
        if depth > MAX_DEPTH {
            return Err(JsonError::unsupported("maximum recursion depth exceeded while decoding a JSON document"));
        }
        let Some(&c) = self.s.get(i) else {
            return Err(self.err("Expecting value", i));
        };
        match c {
            '"' => {
                let (st, end) = self.string(i + 1)?;
                Ok((Value::str(st), end))
            }
            '{' => self.object(i + 1, depth),
            '[' => self.array(i + 1, depth),
            'n' if self.starts_with(i, "null") => Ok((Value::None, i + 4)),
            't' if self.starts_with(i, "true") => Ok((Value::Bool(true), i + 4)),
            'f' if self.starts_with(i, "false") => Ok((Value::Bool(false), i + 5)),
            'N' if self.starts_with(i, "NaN") => Ok((Value::Float(f64::NAN), i + 3)),
            'I' if self.starts_with(i, "Infinity") => Ok((Value::Float(f64::INFINITY), i + 8)),
            '-' if self.starts_with(i, "-Infinity") => Ok((Value::Float(f64::NEG_INFINITY), i + 9)),
            '-' | '0'..='9' => self.number(i),
            _ => Err(self.err("Expecting value", i)),
        }
    }

    fn number(&self, start: usize) -> Result<(Value, usize), JsonError> {
        let digit = |k: usize| matches!(self.s.get(k), Some('0'..='9'));
        let mut i = start;
        if self.s.get(i) == Some(&'-') {
            i += 1;
        }
        if !digit(i) {
            return Err(self.err("Expecting value", start));
        }
        if self.s[i] == '0' {
            i += 1;
        } else {
            while digit(i) {
                i += 1;
            }
        }
        let mut is_float = false;
        if self.s.get(i) == Some(&'.') && digit(i + 1) {
            is_float = true;
            i += 1;
            while digit(i) {
                i += 1;
            }
        }
        if matches!(self.s.get(i), Some('e' | 'E')) {
            let mut k = i + 1;
            if matches!(self.s.get(k), Some('+' | '-')) {
                k += 1;
            }
            if digit(k) {
                is_float = true;
                while digit(k) {
                    k += 1;
                }
                i = k;
            }
        }
        let text: String = self.s[start..i].iter().collect();
        if is_float {
            let x: f64 = text.parse().map_err(|_| self.err("Expecting value", start))?;
            Ok((Value::Float(x), i))
        } else {
            match text.parse::<i64>() {
                Ok(n) => Ok((Value::Int(n), i)),
                Err(_) => Err(JsonError::unsupported("integer outside i64 not supported yet")),
            }
        }
    }

    fn hex4(&self, at: usize) -> Option<u32> {
        if at + 4 > self.s.len() {
            return None;
        }
        let mut v = 0u32;
        for k in 0..4 {
            v = v * 16 + self.s[at + k].to_digit(16)?;
        }
        Some(v)
    }

    /// `begin` é o índice logo após a aspa de abertura. Devolve o texto e o índice após a aspa final.
    fn string(&self, begin: usize) -> Result<(String, usize), JsonError> {
        let mut out = String::new();
        let mut i = begin;
        loop {
            let Some(&c) = self.s.get(i) else {
                return Err(self.err("Unterminated string starting at", begin - 1));
            };
            match c {
                '"' => return Ok((out, i + 1)),
                '\\' => {
                    let Some(&e) = self.s.get(i + 1) else {
                        return Err(self.err("Unterminated string starting at", begin - 1));
                    };
                    match e {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'b' => out.push('\u{8}'),
                        'f' => out.push('\u{c}'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'u' => {
                            let Some(mut cp) = self.hex4(i + 2) else {
                                return Err(self.err("Invalid \\uXXXX escape", i + 1));
                            };
                            i += 6;
                            if (0xD800..0xDC00).contains(&cp) && self.s.get(i) == Some(&'\\') && self.s.get(i + 1) == Some(&'u') {
                                let Some(lo) = self.hex4(i + 2) else {
                                    return Err(self.err("Invalid \\uXXXX escape", i + 1));
                                };
                                if (0xDC00..0xE000).contains(&lo) {
                                    cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                                    i += 6;
                                }
                            }
                            // Uma metade substituta solitária não cabe numa `String` do Rust: erro, para que o
                            // chamador use o decodificador em Python em vez de trocá-la por U+FFFD.
                            let Some(ch) = char::from_u32(cp) else {
                                return Err(JsonError::unsupported("Lone surrogate"));
                            };
                            out.push(ch);
                            continue;
                        }
                        _ => return Err(self.err("Invalid \\escape", i)),
                    }
                    i += 2;
                }
                c if (c as u32) < 0x20 => return Err(self.err("Invalid control character at", i)),
                c => {
                    out.push(c);
                    i += 1;
                }
            }
        }
    }

    fn array(&self, mut i: usize, depth: usize) -> Result<(Value, usize), JsonError> {
        let mut items = Vec::new();
        i = self.skip_ws(i);
        if self.s.get(i) == Some(&']') {
            return Ok((Value::list(items), i + 1));
        }
        loop {
            let (v, end) = self.value(i, depth + 1)?;
            items.push(v);
            i = self.skip_ws(end);
            match self.s.get(i) {
                Some(']') => return Ok((Value::list(items), i + 1)),
                Some(',') => {
                    let comma = i;
                    i = self.skip_ws(i + 1);
                    if self.s.get(i) == Some(&']') {
                        return Err(self.err("Illegal trailing comma before end of array", comma));
                    }
                }
                _ => return Err(self.err("Expecting ',' delimiter", i)),
            }
        }
    }

    fn object(&self, mut i: usize, depth: usize) -> Result<(Value, usize), JsonError> {
        let mut dict = Dict::default();
        i = self.skip_ws(i);
        if self.s.get(i) == Some(&'}') {
            return Ok((Value::dict(dict), i + 1));
        }
        loop {
            if self.s.get(i) != Some(&'"') {
                return Err(self.err("Expecting property name enclosed in double quotes", i));
            }
            let (key, end) = self.string(i + 1)?;
            i = self.skip_ws(end);
            if self.s.get(i) != Some(&':') {
                return Err(self.err("Expecting ':' delimiter", i));
            }
            i = self.skip_ws(i + 1);
            let (v, end) = self.value(i, depth + 1)?;
            dict.set(Value::str(key), v).map_err(|_| JsonError::unsupported("invalid dict key"))?;
            i = self.skip_ws(end);
            match self.s.get(i) {
                Some('}') => return Ok((Value::dict(dict), i + 1)),
                Some(',') => {
                    let comma = i;
                    i = self.skip_ws(i + 1);
                    if self.s.get(i) == Some(&'}') {
                        return Err(self.err("Illegal trailing comma before end of object", comma));
                    }
                }
                _ => return Err(self.err("Expecting ',' delimiter", i)),
            }
        }
    }
}

/// `json.loads(s)`.
pub fn loads(s: &str) -> Result<Value, JsonError> {
    let p = Parser { s: s.chars().collect() };
    let start = p.skip_ws(0);
    let (v, end) = p.value(start, 0)?;
    let end = p.skip_ws(end);
    if end != p.s.len() {
        return Err(p.err("Extra data", end));
    }
    Ok(v)
}

// ---------------------------------------------------------------------------
// `_json`: porte do `Modules/_json.c` do CPython 3.13.5
// ---------------------------------------------------------------------------
//
// As classes `_json.Scanner` e `_json.Encoder` vivem no `_json` embutido em Python
// (`modules/py/_json.py`), que guarda os atributos e chama estas funções: o trabalho é todo daqui,
// na ordem e com as mensagens do C.

use std::cell::RefCell;

use crate::object::{char_surrogate, surrogate_to_char, Kw};
use crate::vm::{exc, type_error, PyException, PyResult, Vm};

/// Folga de pilha do C no 3.13 (`Py_C_RECURSION_LIMIT`) que sobra para os contêineres aninhados,
/// medida no Debian: `json.loads` aceita 9998 níveis e `json.dumps` 9997.
const DECODE_DEPTH: usize = 9998;
const ENCODE_DEPTH: usize = 9998;

thread_local! {
    static C_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// `allow_nan` de cada `_json.Encoder`, que o C guarda sem expor como atributo. A chave é o
    /// endereço da instância, regravada a cada `make_encoder`.
    static ALLOW_NAN: RefCell<std::collections::HashMap<usize, bool>> = RefCell::new(Default::default());
}

fn instance_key(v: &Value) -> usize {
    match v {
        Value::Instance(i) => Rc::as_ptr(i) as *const () as usize,
        _ => 0,
    }
}

/// `_Py_EnterRecursiveCall(where)`: o guarda de cada contêiner aninhado.
fn enter_recursive(limit: usize, place: &str) -> PyResult<()> {
    let d = C_DEPTH.with(|c| c.get());
    if d >= limit {
        return Err(exc("RecursionError", format!("maximum recursion depth exceeded{place}")));
    }
    C_DEPTH.with(|c| c.set(d + 1));
    Ok(())
}

fn leave_recursive() {
    C_DEPTH.with(|c| c.set(c.get().saturating_sub(1)));
}

/// O código-ponto de um caractere do `str` da VM (os surrogates moram no plano 16).
fn code_point(c: char) -> u32 {
    char_surrogate(c).unwrap_or(c as u32)
}

/// O texto de um `str` ou de uma instância de subclasse de `str`.
fn str_payload(v: &Value) -> Option<Rc<crate::object::PyStr>> {
    match v {
        Value::Str(s) => Some(s.clone()),
        Value::Instance(i) => match &*i.payload.borrow() {
            Some(Value::Str(s)) => Some(s.clone()),
            _ => None,
        },
        _ => None,
    }
}

fn payload(v: &Value) -> Value {
    match v {
        Value::Instance(i) => i.payload.borrow().clone().unwrap_or_else(|| v.clone()),
        other => other.clone(),
    }
}

/// `raise_errmsg`: `json.decoder.JSONDecodeError(msg, s, end)`.
fn raise_errmsg(vm: &mut Vm, msg: &str, s: &Value, end: usize) -> PyException {
    let made = (|| -> PyResult<Value> {
        let module = crate::modules::import_value(vm, "json.decoder")?;
        let cls = vm.load_attr(&module, "JSONDecodeError")?;
        vm.call(&cls, vec![Value::str(msg), s.clone(), Value::Int(end as i64)], Vec::new())
    })();
    match made {
        Ok(e) => PyException::from_value(&e),
        Err(e) => e,
    }
}

fn raise_stop_iteration(idx: usize) -> PyException {
    crate::generator::stop_iteration(Value::Int(idx as i64))
}

fn first_arg_not_string(v: &Value) -> PyException {
    type_error(format!("first argument must be a string, not {}", v.type_name()))
}

/// `ascii_escape_unicode`.
fn ascii_escape(s: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        let c = code_point(ch);
        if (0x20..=0x7e).contains(&c) && c != u32::from(b'\\') && c != u32::from(b'"') {
            out.push(ch);
            continue;
        }
        out.push('\\');
        match c {
            0x5c => out.push('\\'),
            0x22 => out.push('"'),
            0x08 => out.push('b'),
            0x0c => out.push('f'),
            0x0a => out.push('n'),
            0x0d => out.push('r'),
            0x09 => out.push('t'),
            _ => {
                let mut c = c;
                if c >= 0x10000 {
                    let v = 0xd800 + ((c - 0x10000) >> 10);
                    out.push('u');
                    for k in [12, 8, 4, 0] {
                        out.push(HEX[((v >> k) & 0xf) as usize] as char);
                    }
                    c = 0xdc00 + ((c - 0x10000) & 0x3ff);
                    out.push('\\');
                }
                out.push('u');
                for k in [12, 8, 4, 0] {
                    out.push(HEX[((c >> k) & 0xf) as usize] as char);
                }
            }
        }
    }
    out.push('"');
    out
}

/// `escape_unicode`.
fn escape(s: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) <= 0x1f => {
                out.push_str("\\u00");
                out.push(HEX[((c as u32) >> 4) as usize] as char);
                out.push(HEX[((c as u32) & 0xf) as usize] as char);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `scanstring_unicode`: o texto e o índice depois da aspa final.
fn scanstring_unicode(vm: &mut Vm, s: &Value, buf: &[char], end: i64, strict: bool) -> PyResult<(String, usize)> {
    let len = buf.len();
    if end < 0 || len < end as usize {
        return Err(exc("ValueError", "end is out of bounds"));
    }
    let mut end = end as usize;
    let begin = end.wrapping_sub(1);
    let mut out = String::new();
    loop {
        let mut next = end;
        let mut c = 0u32;
        while next < len {
            c = code_point(buf[next]);
            if c == u32::from(b'"') || c == u32::from(b'\\') {
                break;
            }
            if c <= 0x1f && strict {
                return Err(raise_errmsg(vm, "Invalid control character at", s, next));
            }
            next += 1;
        }
        if next == len {
            c = 0;
        }
        if c != u32::from(b'"') && c != u32::from(b'\\') {
            return Err(raise_errmsg(vm, "Unterminated string starting at", s, begin));
        }
        out.extend(&buf[end..next]);
        next += 1;
        if c == u32::from(b'"') {
            end = next;
            break;
        }
        if next == len {
            return Err(raise_errmsg(vm, "Unterminated string starting at", s, begin));
        }
        let e = code_point(buf[next]);
        let ch: char;
        if e != u32::from(b'u') {
            end = next + 1;
            ch = match char::from_u32(e) {
                Some('"') => '"',
                Some('\\') => '\\',
                Some('/') => '/',
                Some('b') => '\u{8}',
                Some('f') => '\u{c}',
                Some('n') => '\n',
                Some('r') => '\r',
                Some('t') => '\t',
                _ => return Err(raise_errmsg(vm, "Invalid \\escape", s, end - 2)),
            };
        } else {
            next += 1;
            end = next + 4;
            if end >= len {
                return Err(raise_errmsg(vm, "Invalid \\uXXXX escape", s, next - 1));
            }
            let hex = |at: usize| buf[at].to_digit(16).filter(|_| buf[at].is_ascii_hexdigit());
            let mut cp = 0u32;
            while next < end {
                let Some(d) = hex(next) else {
                    return Err(raise_errmsg(vm, "Invalid \\uXXXX escape", s, end - 5));
                };
                cp = (cp << 4) | d;
                next += 1;
            }
            // Par de surrogates: o `\uDC00` seguinte junta-se ao alto; senão o alto fica sozinho.
            if (0xd800..0xdc00).contains(&cp) && end + 6 < len && buf[next] == '\\' && buf[next + 1] == 'u' {
                next += 2;
                end += 6;
                let mut c2 = 0u32;
                while next < end {
                    let Some(d) = hex(next) else {
                        return Err(raise_errmsg(vm, "Invalid \\uXXXX escape", s, end - 5));
                    };
                    c2 = (c2 << 4) | d;
                    next += 1;
                }
                if (0xdc00..0xe000).contains(&c2) {
                    cp = 0x10000 + (((cp - 0xd800) << 10) | (c2 - 0xdc00));
                } else {
                    end -= 6;
                }
            }
            ch = char::from_u32(cp).unwrap_or_else(|| surrogate_to_char(cp));
        }
        out.push(ch);
    }
    Ok((out, end))
}

/// Os atributos de um `_json.Scanner` lidos uma vez por chamada.
struct Scanner {
    strict: bool,
    object_hook: Value,
    object_pairs_hook: Value,
    parse_float: Value,
    parse_int: Value,
    parse_constant: Value,
    memo: std::collections::HashMap<String, Value>,
}

fn skip_ws(buf: &[char], mut idx: usize) -> usize {
    while idx < buf.len() && matches!(buf[idx], ' ' | '\t' | '\n' | '\r') {
        idx += 1;
    }
    idx
}

/// `_parse_object_unicode`.
fn parse_object(vm: &mut Vm, sc: &mut Scanner, s: &Value, buf: &[char], mut idx: usize) -> PyResult<(Value, usize)> {
    let has_pairs_hook = !matches!(sc.object_pairs_hook, Value::None);
    let mut pairs: Vec<Value> = Vec::new();
    let mut dict = Dict::default();
    idx = skip_ws(buf, idx);
    if idx >= buf.len() || buf[idx] != '}' {
        loop {
            if idx >= buf.len() || buf[idx] != '"' {
                return Err(raise_errmsg(vm, "Expecting property name enclosed in double quotes", s, idx));
            }
            let (text, next) = scanstring_unicode(vm, s, buf, idx as i64 + 1, sc.strict)?;
            let key = sc.memo.entry(text.clone()).or_insert_with(|| Value::str(text)).clone();
            idx = skip_ws(buf, next);
            if idx >= buf.len() || buf[idx] != ':' {
                return Err(raise_errmsg(vm, "Expecting ':' delimiter", s, idx));
            }
            idx = skip_ws(buf, idx + 1);
            let (val, next) = scan_once(vm, sc, s, buf, idx as i64)?;
            if has_pairs_hook {
                pairs.push(Value::tuple(vec![key, val]));
            } else {
                dict.set(key, val).map_err(|_| exc("SystemError", "dict insertion failed"))?;
            }
            idx = skip_ws(buf, next);
            if idx < buf.len() && buf[idx] == '}' {
                break;
            }
            if idx >= buf.len() || buf[idx] != ',' {
                return Err(raise_errmsg(vm, "Expecting ',' delimiter", s, idx));
            }
            let comma = idx;
            idx = skip_ws(buf, idx + 1);
            if idx < buf.len() && buf[idx] == '}' {
                return Err(raise_errmsg(vm, "Illegal trailing comma before end of object", s, comma));
            }
        }
    }
    let next = idx + 1;
    if has_pairs_hook {
        let hook = sc.object_pairs_hook.clone();
        return Ok((vm.call(&hook, vec![Value::list(pairs)], Vec::new())?, next));
    }
    let dict = Value::dict(dict);
    if !matches!(sc.object_hook, Value::None) {
        let hook = sc.object_hook.clone();
        return Ok((vm.call(&hook, vec![dict], Vec::new())?, next));
    }
    Ok((dict, next))
}

/// `_parse_array_unicode`.
fn parse_array(vm: &mut Vm, sc: &mut Scanner, s: &Value, buf: &[char], mut idx: usize) -> PyResult<(Value, usize)> {
    let mut items = Vec::new();
    idx = skip_ws(buf, idx);
    if idx >= buf.len() || buf[idx] != ']' {
        loop {
            let (val, next) = scan_once(vm, sc, s, buf, idx as i64)?;
            items.push(val);
            idx = skip_ws(buf, next);
            if idx < buf.len() && buf[idx] == ']' {
                break;
            }
            if idx >= buf.len() || buf[idx] != ',' {
                return Err(raise_errmsg(vm, "Expecting ',' delimiter", s, idx));
            }
            let comma = idx;
            idx = skip_ws(buf, idx + 1);
            if idx < buf.len() && buf[idx] == ']' {
                return Err(raise_errmsg(vm, "Illegal trailing comma before end of array", s, comma));
            }
        }
    }
    Ok((Value::list(items), idx + 1))
}

/// `_match_number_unicode`.
fn match_number(vm: &mut Vm, sc: &mut Scanner, buf: &[char], start: usize) -> PyResult<(Value, usize)> {
    let end_idx = buf.len() as i64 - 1;
    let digit = |i: i64| i >= 0 && i <= end_idx && buf[i as usize].is_ascii_digit();
    let mut idx = start as i64;
    if buf[idx as usize] == '-' {
        idx += 1;
        if idx > end_idx {
            return Err(raise_stop_iteration(start));
        }
    }
    match buf[idx as usize] {
        '1'..='9' => {
            idx += 1;
            while digit(idx) {
                idx += 1;
            }
        }
        '0' => idx += 1,
        _ => return Err(raise_stop_iteration(start)),
    }
    let mut is_float = false;
    if idx < end_idx && buf[idx as usize] == '.' && digit(idx + 1) {
        is_float = true;
        idx += 2;
        while digit(idx) {
            idx += 1;
        }
    }
    if idx < end_idx && matches!(buf[idx as usize], 'e' | 'E') {
        let e_start = idx;
        idx += 1;
        if idx < end_idx && matches!(buf[idx as usize], '-' | '+') {
            idx += 1;
        }
        while digit(idx) {
            idx += 1;
        }
        if digit(idx - 1) {
            is_float = true;
        } else {
            idx = e_start;
        }
    }
    let text: String = buf[start..idx as usize].iter().collect();
    let custom = if is_float && !matches!(sc.parse_float, Value::Builtin("float")) {
        Some(sc.parse_float.clone())
    } else if !is_float && !matches!(sc.parse_int, Value::Builtin("int")) {
        Some(sc.parse_int.clone())
    } else {
        None
    };
    let value = match custom {
        Some(f) => vm.call(&f, vec![Value::str(text)], Vec::new())?,
        None if is_float => {
            Value::Float(text.parse().map_err(|_| exc("ValueError", format!("could not convert string to float: '{text}'")))?)
        }
        None => vm.call(&Value::Builtin("int"), vec![Value::str(text)], Vec::new())?,
    };
    Ok((value, idx as usize))
}

/// `scan_once_unicode`.
fn scan_once(vm: &mut Vm, sc: &mut Scanner, s: &Value, buf: &[char], idx: i64) -> PyResult<(Value, usize)> {
    if idx < 0 {
        return Err(exc("ValueError", "idx cannot be negative"));
    }
    let idx = idx as usize;
    let length = buf.len();
    if idx >= length {
        return Err(raise_stop_iteration(idx));
    }
    let at = |k: usize, lit: &str| lit.chars().enumerate().all(|(j, c)| buf[k + j] == c);
    match buf[idx] {
        '"' => {
            let (text, next) = scanstring_unicode(vm, s, buf, idx as i64 + 1, sc.strict)?;
            return Ok((Value::str(text), next));
        }
        '{' => {
            enter_recursive(DECODE_DEPTH, " while decoding a JSON object from a unicode string")?;
            let r = parse_object(vm, sc, s, buf, idx + 1);
            leave_recursive();
            return r;
        }
        '[' => {
            enter_recursive(DECODE_DEPTH, " while decoding a JSON array from a unicode string")?;
            let r = parse_array(vm, sc, s, buf, idx + 1);
            leave_recursive();
            return r;
        }
        'n' if idx + 3 < length && at(idx, "null") => return Ok((Value::None, idx + 4)),
        't' if idx + 3 < length && at(idx, "true") => return Ok((Value::Bool(true), idx + 4)),
        'f' if idx + 4 < length && at(idx, "false") => return Ok((Value::Bool(false), idx + 5)),
        'N' if idx + 2 < length && at(idx, "NaN") => return parse_constant(vm, sc, "NaN", idx),
        'I' if idx + 7 < length && at(idx, "Infinity") => return parse_constant(vm, sc, "Infinity", idx),
        '-' if idx + 8 < length && at(idx, "-Infinity") => return parse_constant(vm, sc, "-Infinity", idx),
        _ => {}
    }
    match_number(vm, sc, buf, idx)
}

/// `_parse_constant`: `parse_constant("NaN")` e afins.
fn parse_constant(vm: &mut Vm, sc: &mut Scanner, constant: &str, idx: usize) -> PyResult<(Value, usize)> {
    let f = sc.parse_constant.clone();
    let v = vm.call(&f, vec![Value::str(constant)], Vec::new())?;
    Ok((v, idx + constant.chars().count()))
}

/// `_json_native.scan_once(scanner, string, idx)`: o `scanner_call`.
fn native_scan_once(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let mut args = args.into_iter();
    let scanner = args.next().ok_or_else(|| exc("SystemError", "scanner missing"))?;
    let bound = crate::native_util::bind("scan_once", args.collect(), kw, &["string", "idx"], 2)?;
    let (s, idx) = (bound[0].clone().unwrap_or(Value::None), bound[1].clone().unwrap_or(Value::None));
    let idx = crate::native_util::want_int(&idx)?;
    let Some(text) = str_payload(&s) else { return Err(first_arg_not_string(&s)) };
    let mut sc = Scanner {
        strict: vm.load_attr(&scanner, "strict")?.is_true(),
        object_hook: vm.load_attr(&scanner, "object_hook")?,
        object_pairs_hook: vm.load_attr(&scanner, "object_pairs_hook")?,
        parse_float: vm.load_attr(&scanner, "parse_float")?,
        parse_int: vm.load_attr(&scanner, "parse_int")?,
        parse_constant: vm.load_attr(&scanner, "parse_constant")?,
        memo: Default::default(),
    };
    let buf: Vec<char> = text.as_str().chars().collect();
    let (v, next) = scan_once(vm, &mut sc, &s, &buf, idx)?;
    Ok(Value::tuple(vec![v, Value::Int(next as i64)]))
}

/// `scanstring(string, end, strict=True)`.
fn native_scanstring(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    crate::native_util::no_kwargs("scanstring", &kw)?;
    if args.len() < 2 {
        return Err(type_error(format!("scanstring() takes at least 2 arguments ({} given)", args.len())));
    }
    if args.len() > 3 {
        return Err(type_error(format!("scanstring() takes at most 3 arguments ({} given)", args.len())));
    }
    let end = crate::native_util::want_int(&args[1])?;
    let strict = args.get(2).is_none_or(|v| v.is_true());
    let Some(text) = str_payload(&args[0]) else { return Err(first_arg_not_string(&args[0])) };
    let buf: Vec<char> = text.as_str().chars().collect();
    let (out, next) = scanstring_unicode(vm, &args[0], &buf, end, strict)?;
    Ok(Value::tuple(vec![Value::str(out), Value::Int(next as i64)]))
}

fn basestring_arg(name: &str, args: &[Value], kw: &Kw) -> PyResult<Rc<crate::object::PyStr>> {
    crate::native_util::no_kwargs(name, kw)?;
    if args.len() != 1 {
        return Err(type_error(format!("{name}() takes exactly one argument ({} given)", args.len())));
    }
    str_payload(&args[0]).ok_or_else(|| first_arg_not_string(&args[0]))
}

fn native_encode_basestring_ascii(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = basestring_arg("encode_basestring_ascii", &args, &kw)?;
    Ok(Value::str(ascii_escape(s.as_str())))
}

fn native_encode_basestring(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = basestring_arg("encode_basestring", &args, &kw)?;
    Ok(Value::str(escape(s.as_str())))
}

/// `make_scanner(context)`: valida os argumentos e devolve o contexto.
fn native_scanner_args(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let bound = crate::native_util::bind("make_scanner", args, kw, &["context"], 1)?;
    Ok(bound[0].clone().unwrap_or(Value::None))
}

/// `make_encoder(...)` (`"OOOOUUppp:make_encoder"`): os nove valores validados; o `allow_nan` fica
/// guardado para a instância `this`.
fn native_encoder_args(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    const NAMES: [&str; 9] =
        ["markers", "default", "encoder", "indent", "key_separator", "item_separator", "sort_keys", "skipkeys", "allow_nan"];
    let mut args = args.into_iter();
    let this = args.next().ok_or_else(|| exc("SystemError", "encoder missing"))?;
    let bound = crate::native_util::bind("make_encoder", args.collect(), kw, &NAMES, 9)?;
    let mut out: Vec<Value> = bound.into_iter().map(|v| v.unwrap_or(Value::None)).collect();
    if !matches!(out[0], Value::None | Value::Dict(_)) {
        return Err(type_error(format!("make_encoder() argument 1 must be dict or None, not {}", out[0].type_name())));
    }
    for i in [4, 5] {
        if str_payload(&out[i]).is_none() {
            return Err(type_error(format!("make_encoder() argument {} must be str, not {}", i + 1, out[i].type_name())));
        }
    }
    for v in out.iter_mut().skip(6) {
        *v = Value::Bool(v.is_true());
    }
    let allow_nan = out[8].is_true();
    ALLOW_NAN.with(|m| m.borrow_mut().insert(instance_key(&this), allow_nan));
    Ok(Value::tuple(out))
}

/// Os atributos de um `_json.Encoder`.
struct Encoder {
    markers: Value,
    default: Value,
    encoder: Value,
    indent: Option<String>,
    key_separator: String,
    item_separator: String,
    sort_keys: bool,
    skipkeys: bool,
    allow_nan: bool,
    /// `fast_encode`: o `encoder` é o `encode_basestring(_ascii)` do próprio `_json`.
    fast: Option<bool>,
}

impl Encoder {
    fn ident(&self, vm: &mut Vm, obj: &Value) -> PyResult<Option<Value>> {
        let Value::Dict(markers) = &self.markers else { return Ok(None) };
        let ident = crate::builtins::b_id(vm, vec![obj.clone()], Vec::new())?;
        if markers.borrow().contains(&ident).unwrap_or(false) {
            return Err(exc("ValueError", "Circular reference detected"));
        }
        markers.borrow_mut().set(ident.clone(), obj.clone()).map_err(|_| exc("SystemError", "markers"))?;
        Ok(Some(ident))
    }

    fn forget(&self, ident: Option<Value>) {
        if let (Some(ident), Value::Dict(markers)) = (ident, &self.markers) {
            let _ = markers.borrow_mut().remove(&ident);
        }
    }

    /// `encoder_encode_float`.
    fn float(&self, x: f64, obj: &Value) -> PyResult<String> {
        if !x.is_finite() {
            if !self.allow_nan {
                return Err(exc(
                    "ValueError",
                    format!("Out of range float values are not JSON compliant: {}", crate::object::repr(obj)),
                ));
            }
            return Ok(if x > 0.0 { "Infinity" } else if x < 0.0 { "-Infinity" } else { "NaN" }.to_string());
        }
        Ok(float_repr(x))
    }

    /// `encoder_encode_string`.
    fn string(&self, vm: &mut Vm, obj: &Value) -> PyResult<String> {
        if let (Some(ascii), Some(s)) = (self.fast, str_payload(obj)) {
            return Ok(if ascii { ascii_escape(s.as_str()) } else { escape(s.as_str()) });
        }
        let encoded = vm.call(&self.encoder, vec![obj.clone()], Vec::new())?;
        match str_payload(&encoded) {
            Some(s) => Ok(s.as_str().to_string()),
            None => Err(type_error(format!("encoder() must return a string, not {}", encoded.type_name()))),
        }
    }

    /// `encoder_listencode_obj`.
    fn obj(&self, vm: &mut Vm, out: &mut String, obj: &Value, newline_indent: &str) -> PyResult<()> {
        match obj {
            Value::None => out.push_str("null"),
            Value::Bool(true) => out.push_str("true"),
            Value::Bool(false) => out.push_str("false"),
            _ => match payload(obj) {
                Value::Str(_) => out.push_str(&self.string(vm, obj)?),
                v @ (Value::Int(_) | Value::Big(_)) => out.push_str(&repr(&v)),
                Value::Float(x) => out.push_str(&self.float(x, obj)?),
                Value::List(_) | Value::Tuple(_) => {
                    enter_recursive(ENCODE_DEPTH, " while encoding a JSON object")?;
                    let r = self.list(vm, out, obj, newline_indent);
                    leave_recursive();
                    r?;
                }
                Value::Dict(_) => {
                    enter_recursive(ENCODE_DEPTH, " while encoding a JSON object")?;
                    let r = self.dict(vm, out, obj, newline_indent);
                    leave_recursive();
                    r?;
                }
                _ => {
                    let ident = self.ident(vm, obj)?;
                    let newobj = vm.call(&self.default, vec![obj.clone()], Vec::new())?;
                    enter_recursive(ENCODE_DEPTH, " while encoding a JSON object")?;
                    let r = self.obj(vm, out, &newobj, newline_indent);
                    leave_recursive();
                    r?;
                    self.forget(ident);
                }
            },
        }
        Ok(())
    }

    /// `encoder_encode_key_value`.
    #[allow(clippy::too_many_arguments)]
    fn key_value(
        &self,
        vm: &mut Vm,
        out: &mut String,
        first: &mut bool,
        key: &Value,
        value: &Value,
        newline_indent: &str,
        item_separator: &str,
    ) -> PyResult<()> {
        let keystr: Value = match key {
            Value::None => Value::str("null"),
            Value::Bool(b) => Value::str(if *b { "true" } else { "false" }),
            _ => match payload(key) {
                Value::Str(_) => key.clone(),
                Value::Float(x) => Value::str(self.float(x, key)?),
                v @ (Value::Int(_) | Value::Big(_)) => Value::str(repr(&v)),
                _ if self.skipkeys => return Ok(()),
                _ => {
                    return Err(type_error(format!(
                        "keys must be str, int, float, bool or None, not {}",
                        key.type_name()
                    )))
                }
            },
        };
        if *first {
            *first = false;
            if self.indent.is_some() {
                out.push_str(newline_indent);
            }
        } else {
            out.push_str(item_separator);
        }
        out.push_str(&self.string(vm, &keystr)?);
        out.push_str(&self.key_separator);
        self.obj(vm, out, value, newline_indent)
    }

    /// `encoder_listencode_dict`.
    fn dict(&self, vm: &mut Vm, out: &mut String, dct: &Value, newline_indent: &str) -> PyResult<()> {
        let Value::Dict(d) = payload(dct) else { return Ok(()) };
        if d.borrow().is_empty() {
            out.push_str("{}");
            return Ok(());
        }
        let ident = self.ident(vm, dct)?;
        out.push('{');
        let (new_indent, separator) = match &self.indent {
            Some(ind) => {
                let n = format!("{newline_indent}{ind}");
                let sep = format!("{}{n}", self.item_separator);
                (n, sep)
            }
            None => (newline_indent.to_string(), self.item_separator.clone()),
        };
        let mut first = true;
        let exact = matches!(dct, Value::Dict(_));
        if self.sort_keys || !exact {
            // `PyMapping_Items` e, com `sort_keys`, o `list.sort` dos pares.
            let items_fn = vm.load_attr(dct, "items")?;
            let items = vm.call(&items_fn, Vec::new(), Vec::new())?;
            let items = vm.call(&Value::Builtin("list"), vec![items], Vec::new())?;
            if self.sort_keys {
                let sort = vm.load_attr(&items, "sort")?;
                vm.call(&sort, Vec::new(), Vec::new())?;
            }
            let Value::List(list) = &items else { return Ok(()) };
            let list = list.borrow().clone();
            for item in list {
                let Value::Tuple(pair) = &item else {
                    return Err(exc("ValueError", "items must return 2-tuples"));
                };
                if pair.len() != 2 {
                    return Err(exc("ValueError", "items must return 2-tuples"));
                }
                self.key_value(vm, out, &mut first, &pair[0], &pair[1], &new_indent, &separator)?;
            }
        } else {
            let entries: Vec<(Value, Value)> = d.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            for (k, v) in entries {
                self.key_value(vm, out, &mut first, &k, &v, &new_indent, &separator)?;
            }
        }
        self.forget(ident);
        if self.indent.is_some() && !first {
            out.push_str(newline_indent);
        }
        out.push('}');
        Ok(())
    }

    /// `encoder_listencode_list`.
    fn list(&self, vm: &mut Vm, out: &mut String, seq: &Value, newline_indent: &str) -> PyResult<()> {
        let items: Vec<Value> = match payload(seq) {
            Value::List(l) => l.borrow().clone(),
            Value::Tuple(t) => t.to_vec(),
            _ => Vec::new(),
        };
        if items.is_empty() {
            out.push_str("[]");
            return Ok(());
        }
        let ident = self.ident(vm, seq)?;
        out.push('[');
        let (new_indent, separator) = match &self.indent {
            Some(ind) => {
                let n = format!("{newline_indent}{ind}");
                out.push_str(&n);
                let sep = format!("{}{n}", self.item_separator);
                (n, sep)
            }
            None => (newline_indent.to_string(), self.item_separator.clone()),
        };
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                out.push_str(&separator);
            }
            self.obj(vm, out, item, &new_indent)?;
        }
        self.forget(ident);
        if self.indent.is_some() {
            out.push_str(newline_indent);
        }
        out.push(']');
        Ok(())
    }
}

/// `_json_native.encode(encoder, obj, _current_indent_level)`: o `encoder_call`, que devolve `(texto,)`.
fn native_encode(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let mut args = args.into_iter();
    let this = args.next().ok_or_else(|| exc("SystemError", "encoder missing"))?;
    let bound = crate::native_util::bind("_iterencode", args.collect(), kw, &["obj", "_current_indent_level"], 2)?;
    let obj = bound[0].clone().unwrap_or(Value::None);
    let level = crate::native_util::want_int(&bound[1].clone().unwrap_or(Value::None))?;
    let get = |vm: &mut Vm, n: &str| vm.load_attr(&this, n);
    let encoder = get(vm, "encoder")?;
    let fast = match &encoder {
        Value::NativeFn(f) if f.f as usize == native_encode_basestring_ascii as *const () as usize => Some(true),
        Value::NativeFn(f) if f.f as usize == native_encode_basestring as *const () as usize => Some(false),
        _ => None,
    };
    let indent_v = get(vm, "indent")?;
    let text_of = |v: &Value| str_payload(v).map(|s| s.as_str().to_string()).unwrap_or_default();
    let enc = Encoder {
        markers: get(vm, "markers")?,
        default: get(vm, "default")?,
        encoder,
        indent: if matches!(indent_v, Value::None) { None } else { Some(text_of(&indent_v)) },
        key_separator: text_of(&get(vm, "key_separator")?),
        item_separator: text_of(&get(vm, "item_separator")?),
        sort_keys: get(vm, "sort_keys")?.is_true(),
        skipkeys: get(vm, "skipkeys")?.is_true(),
        allow_nan: ALLOW_NAN.with(|m| m.borrow().get(&instance_key(&this)).copied().unwrap_or(true)),
        fast,
    };
    // `_create_newline_indent`: `"\n" + indent * nível`.
    let newline_indent = match &enc.indent {
        Some(ind) => format!("\n{}", ind.repeat(level.max(0) as usize)),
        None => String::new(),
    };
    let mut out = String::new();
    enc.obj(vm, &mut out, &obj, &newline_indent)?;
    Ok(Value::tuple(vec![Value::str(out)]))
}

pub fn build(_vm: &mut crate::vm::Vm) -> Rc<crate::object::ModuleObj> {
    crate::modules::ModuleBuilder::new("_json_native")
        .func("scanstring", native_scanstring)
        .func("encode_basestring_ascii", native_encode_basestring_ascii)
        .func("encode_basestring", native_encode_basestring)
        .func("scan_once", native_scan_once)
        .func("encode", native_encode)
        .func("scanner_args", native_scanner_args)
        .func("encoder_args", native_encoder_args)
        .build()
}

// ---------------------------------------------------------------------------
// testes
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::Set;

    fn d(v: &Value) -> String {
        dumps(v, true).unwrap()
    }

    fn lerr(s: &str) -> String {
        loads(s).unwrap_err().msg
    }

    #[test]
    fn dumps_scalars() {
        assert_eq!(d(&Value::None), "null");
        assert_eq!(d(&Value::Bool(true)), "true");
        assert_eq!(d(&Value::Bool(false)), "false");
        assert_eq!(d(&Value::Int(-42)), "-42");
        assert_eq!(d(&Value::Float(1.5)), "1.5");
        assert_eq!(d(&Value::Float(1e16)), "1e+16");
        assert_eq!(d(&Value::Float(f64::NAN)), "NaN");
        assert_eq!(d(&Value::Float(f64::INFINITY)), "Infinity");
        assert_eq!(d(&Value::Float(f64::NEG_INFINITY)), "-Infinity");
    }

    #[test]
    fn dumps_strings() {
        assert_eq!(d(&Value::str("a\"b\\c\n\r\t\u{8}\u{c}\u{1}")), r#""a\"b\\c\n\r\t\b\f\u0001""#);
        assert_eq!(d(&Value::str("é😀\u{7f}")), "\"\\u00e9\\ud83d\\ude00\\u007f\"");
        assert_eq!(dumps(&Value::str("é😀\u{7f}\u{1}"), false).unwrap(), "\"é😀\u{7f}\\u0001\"");
    }

    #[test]
    fn dumps_containers() {
        let l = Value::list(vec![Value::Int(1), Value::tuple(vec![Value::None, Value::str("x")])]);
        assert_eq!(d(&l), r#"[1, [null, "x"]]"#);
        assert_eq!(d(&Value::list(vec![])), "[]");
        let mut dict = Dict::default();
        dict.set(Value::str("a"), Value::Int(1)).unwrap();
        dict.set(Value::Int(2), Value::Bool(true)).unwrap();
        dict.set(Value::Float(1.5), Value::None).unwrap();
        dict.set(Value::None, Value::Int(0)).unwrap();
        assert_eq!(d(&Value::dict(dict)), r#"{"a": 1, "2": true, "1.5": null, "null": 0}"#);
        assert_eq!(d(&Value::dict(Dict::default())), "{}");
    }

    #[test]
    fn dumps_errors() {
        assert_eq!(dumps(&Value::bytes(vec![1u8]), true).unwrap_err(), "Object of type bytes is not JSON serializable");
        assert_eq!(dumps(&Value::set(Set::new()), true).unwrap_err(), "Object of type set is not JSON serializable");
        let mut dict = Dict::default();
        dict.set(Value::tuple(vec![]), Value::Int(1)).unwrap();
        assert_eq!(dumps(&Value::dict(dict), true).unwrap_err(), "keys must be str, int, float, bool or None, not tuple");
        let l = Value::list(vec![]);
        if let Value::List(rc) = &l {
            rc.borrow_mut().push(l.clone());
        }
        assert_eq!(dumps(&l, true).unwrap_err(), "Circular reference detected");
        // O mesmo objeto repetido sem ciclo é válido.
        let inner = Value::list(vec![Value::Int(1)]);
        assert_eq!(d(&Value::list(vec![inner.clone(), inner])), "[[1], [1]]");
    }

    #[test]
    fn loads_literals_and_numbers() {
        assert_eq!(repr(&loads(" null ").unwrap()), "None");
        assert_eq!(repr(&loads("true").unwrap()), "True");
        assert_eq!(repr(&loads("false").unwrap()), "False");
        assert_eq!(repr(&loads("-12").unwrap()), "-12");
        assert_eq!(repr(&loads("0").unwrap()), "0");
        assert_eq!(repr(&loads("1.5").unwrap()), "1.5");
        assert_eq!(repr(&loads("1e5").unwrap()), "100000.0");
        assert_eq!(repr(&loads("-2.5E-1").unwrap()), "-0.25");
        assert_eq!(repr(&loads("NaN").unwrap()), "nan");
        assert_eq!(repr(&loads("Infinity").unwrap()), "inf");
        assert_eq!(repr(&loads("-Infinity").unwrap()), "-inf");
        assert_eq!(lerr("12345678901234567890"), "integer outside i64 not supported yet");
    }

    #[test]
    fn loads_strings() {
        assert_eq!(repr(&loads(r#""a\"\\\/\b\f\n\r\t""#).unwrap()), "'a\"\\\\/\\x08\\x0c\\n\\r\\t'");
        assert_eq!(repr(&loads(r#""é""#).unwrap()), "'é'");
        assert_eq!(repr(&loads(r#""😀""#).unwrap()), "'😀'");
    }

    #[test]
    fn loads_containers() {
        let v = loads(r#" {"b": [1, 2.0, "x"], "a": {}, "c": []} "#).unwrap();
        assert_eq!(repr(&v), "{'b': [1, 2.0, 'x'], 'a': {}, 'c': []}");
        assert_eq!(repr(&loads("[]").unwrap()), "[]");
        assert_eq!(repr(&loads("{}").unwrap()), "{}");
    }

    #[test]
    fn loads_errors() {
        assert_eq!(lerr(""), "Expecting value: line 1 column 1 (char 0)");
        assert_eq!(lerr("  x"), "Expecting value: line 1 column 3 (char 2)");
        assert_eq!(lerr("-"), "Expecting value: line 1 column 1 (char 0)");
        assert_eq!(lerr("{1: 2}"), "Expecting property name enclosed in double quotes: line 1 column 2 (char 1)");
        assert_eq!(lerr(r#"{"a" 1}"#), "Expecting ':' delimiter: line 1 column 6 (char 5)");
        assert_eq!(lerr(r#"{"a": 1 "b": 2}"#), "Expecting ',' delimiter: line 1 column 9 (char 8)");
        assert_eq!(lerr("[1 2]"), "Expecting ',' delimiter: line 1 column 4 (char 3)");
        assert_eq!(lerr("[1,"), "Expecting value: line 1 column 4 (char 3)");
        assert_eq!(lerr("[1,]"), "Illegal trailing comma before end of array: line 1 column 3 (char 2)");
        assert_eq!(lerr(r#"{"a": 1,}"#), "Illegal trailing comma before end of object: line 1 column 8 (char 7)");
        assert_eq!(lerr(r#""abc"#), "Unterminated string starting at: line 1 column 1 (char 0)");
        assert_eq!(lerr("\"a\nb\""), "Invalid control character at: line 1 column 3 (char 2)");
        assert_eq!(lerr(r#""a\qb""#), "Invalid \\escape: line 1 column 3 (char 2)");
        assert_eq!(lerr(r#""\u12""#), "Invalid \\uXXXX escape: line 1 column 3 (char 2)");
        assert_eq!(lerr("1 2"), "Extra data: line 1 column 3 (char 2)");
        assert_eq!(lerr("01"), "Extra data: line 1 column 2 (char 1)");
        assert_eq!(lerr("[1,\n  2,\n  x]"), "Expecting value: line 3 column 3 (char 11)");
        assert_eq!(lerr("\"é\" x"), "Extra data: line 1 column 5 (char 4)");
    }
}
