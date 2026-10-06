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
        let mut dict = Dict::new();
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
// `_json`: o caminho rápido do `json.dumps`/`json.loads` com as opções padrão
// ---------------------------------------------------------------------------

/// `_json.loads(s)`: `(True, valor)` ou `(False, mensagem, posição)` para erro de sintaxe (com a mensagem
/// do scanner em C do CPython, que o chamador entrega ao `JSONDecodeError`); `None` quando o texto precisa
/// do decodificador em Python (inteiro grande, surrogate solitário, profundidade).
fn fast_loads(_vm: &mut crate::vm::Vm, args: Vec<Value>, kw: crate::object::Kw) -> crate::vm::PyResult<Value> {
    crate::native_util::no_kwargs("loads", &kw)?;
    crate::native_util::exactly("loads", &args, 1)?;
    let Value::Str(s) = &args[0] else { return Ok(Value::None) };
    Ok(match loads(s.as_str()) {
        Ok(v) => Value::tuple(vec![Value::Bool(true), v]),
        Err(JsonError { syntax: Some((msg, pos)), .. }) => {
            Value::tuple(vec![Value::Bool(false), Value::str(msg), Value::Int(pos as i64)])
        }
        Err(_) => Value::None,
    })
}

/// `_json.dumps(obj, ensure_ascii)`: o texto, ou `None` quando o objeto precisa do codificador em Python
/// (tipos sem representação direta, referência circular, chaves inválidas, profundidade).
fn fast_dumps(_vm: &mut crate::vm::Vm, args: Vec<Value>, kw: crate::object::Kw) -> crate::vm::PyResult<Value> {
    crate::native_util::no_kwargs("dumps", &kw)?;
    crate::native_util::exactly("dumps", &args, 2)?;
    Ok(match dumps(&args[0], args[1].is_true()) {
        Ok(text) => Value::str(text),
        Err(_) => Value::None,
    })
}

pub fn build(_vm: &mut crate::vm::Vm) -> Rc<crate::object::ModuleObj> {
    crate::modules::ModuleBuilder::new("_json").func("loads", fast_loads).func("dumps", fast_dumps).build()
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
        let mut dict = Dict::new();
        dict.set(Value::str("a"), Value::Int(1)).unwrap();
        dict.set(Value::Int(2), Value::Bool(true)).unwrap();
        dict.set(Value::Float(1.5), Value::None).unwrap();
        dict.set(Value::None, Value::Int(0)).unwrap();
        assert_eq!(d(&Value::dict(dict)), r#"{"a": 1, "2": true, "1.5": null, "null": 0}"#);
        assert_eq!(d(&Value::dict(Dict::new())), "{}");
    }

    #[test]
    fn dumps_errors() {
        assert_eq!(dumps(&Value::bytes(vec![1u8]), true).unwrap_err(), "Object of type bytes is not JSON serializable");
        assert_eq!(dumps(&Value::set(Set::new()), true).unwrap_err(), "Object of type set is not JSON serializable");
        let mut dict = Dict::new();
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
