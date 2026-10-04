//! Valores com a semântica do Python que o yq vê: o que o construtor do PyYAML devolve, o que o
//! `json` do Python escreve pro jq e o que ele lê de volta da saída do jq.
//!
//! - Dicionário guarda a ordem de inserção e compara chaves como o Python (`1 == 1.0 == True`; a
//!   chave fica a primeira inserida e o valor o último);
//! - `json.dumps`: `float.__repr__` (o jq 1.7 preserva o literal de um número que não mudou, então
//!   `1000.0` e `1e+16` aparecem na saída), `Infinity`/`NaN`, chaves não texto convertidas;
//! - `json.loads`: número com ponto ou expoente vira float, senão inteiro de qualquer tamanho.

use num_bigint::BigInt;
use num_traits::{ToPrimitive, Zero};

use crate::scanner::YamlError;

#[derive(Clone, Debug)]
pub enum Py {
    None,
    Bool(bool),
    Int(BigInt),
    Float(f64),
    Str(String),
    /// Data ou data e hora (`!!timestamp`), já no formato do `isoformat()`.
    Date(String, &'static str),
    List(Vec<Py>),
    Dict(Vec<(Py, Py)>),
}

impl Py {
    pub fn type_name(&self) -> &'static str {
        match self {
            Py::None => "NoneType",
            Py::Bool(_) => "bool",
            Py::Int(_) => "int",
            Py::Float(_) => "float",
            Py::Str(_) => "str",
            Py::Date(_, t) => t,
            Py::List(_) => "list",
            Py::Dict(_) => "dict",
        }
    }

    /// Valor numérico pra comparar chaves (bool e int e float se misturam no Python).
    fn number(&self) -> Option<f64> {
        match self {
            Py::Bool(b) => Some(f64::from(u8::from(*b))),
            Py::Int(i) => i.to_f64(),
            Py::Float(f) => Some(*f),
            _ => None,
        }
    }

    /// Igualdade de chave de dicionário do Python.
    pub fn key_eq(&self, other: &Py) -> bool {
        match (self, other) {
            (Py::None, Py::None) => true,
            (Py::Str(a), Py::Str(b)) => a == b,
            (Py::Date(a, _), Py::Date(b, _)) => a == b,
            (Py::Int(a), Py::Int(b)) => a == b,
            (Py::Bool(a), Py::Bool(b)) => a == b,
            (Py::Int(a), Py::Bool(b)) | (Py::Bool(b), Py::Int(a)) => *a == BigInt::from(u8::from(*b)),
            _ => match (self.number(), other.number()) {
                (Some(x), Some(y)) => x == y,
                _ => false,
            },
        }
    }
}

/// `dict(pairs)` do Python.
pub fn make_dict(pairs: Vec<(Py, Py)>) -> Result<Py, YamlError> {
    let mut out: Vec<(Py, Py)> = Vec::with_capacity(pairs.len());
    for (k, v) in pairs {
        if matches!(k, Py::List(_) | Py::Dict(_)) {
            return Err(YamlError::plain("TypeError", format!("unhashable type: '{}'", k.type_name())));
        }
        match out.iter_mut().find(|(ek, _)| ek.key_eq(&k)) {
            Some(slot) => slot.1 = v,
            None => out.push((k, v)),
        }
    }
    Ok(Py::Dict(out))
}

/// `repr(float)` do Python.
pub fn float_repr(f: f64) -> String {
    if f.is_nan() {
        return "nan".to_string();
    }
    if f.is_infinite() {
        return if f > 0.0 { "inf".to_string() } else { "-inf".to_string() };
    }
    if f == 0.0 {
        return if f.is_sign_negative() { "-0.0".to_string() } else { "0.0".to_string() };
    }
    // Dígitos mais curtos que voltam ao mesmo double, com o expoente decimal.
    let e = format!("{:e}", f.abs());
    let (mant, exp) = e.split_once('e').unwrap_or((&e, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let digits: String = mant.chars().filter(char::is_ascii_digit).collect();
    let sign = if f < 0.0 { "-" } else { "" };
    if (-4..16).contains(&exp) {
        let n = digits.len() as i32;
        let s = if exp >= 0 {
            let int_len = exp + 1;
            if n <= int_len {
                format!("{digits}{}.0", "0".repeat((int_len - n) as usize))
            } else {
                format!("{}.{}", &digits[..int_len as usize], &digits[int_len as usize..])
            }
        } else {
            format!("0.{}{digits}", "0".repeat((-exp - 1) as usize))
        };
        format!("{sign}{s}")
    } else {
        let m = if digits.len() > 1 { format!("{}.{}", &digits[..1], &digits[1..]) } else { digits };
        let es = if exp < 0 { '-' } else { '+' };
        format!("{sign}{m}e{es}{:02}", exp.abs())
    }
}

fn json_str(s: &str, out: &mut String) {
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
            c => out.push(c),
        }
    }
    out.push('"');
}

fn float_json(f: f64) -> String {
    if f.is_nan() {
        "NaN".to_string()
    } else if f.is_infinite() {
        if f > 0.0 { "Infinity".to_string() } else { "-Infinity".to_string() }
    } else {
        float_repr(f)
    }
}

/// `json.dumps(obj, cls=JSONDateTimeEncoder)` (os separadores não importam: o jq relê).
pub fn to_json(v: &Py, out: &mut String) -> Result<(), YamlError> {
    match v {
        Py::None => out.push_str("null"),
        Py::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Py::Int(i) => out.push_str(&i.to_string()),
        Py::Float(f) => out.push_str(&float_json(*f)),
        Py::Str(s) | Py::Date(s, _) => json_str(s, out),
        Py::List(l) => {
            out.push('[');
            for (i, x) in l.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                to_json(x, out)?;
            }
            out.push(']');
        }
        Py::Dict(d) => {
            out.push('{');
            for (i, (k, x)) in d.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                let key = match k {
                    Py::Str(s) => s.clone(),
                    Py::Bool(b) => (if *b { "true" } else { "false" }).to_string(),
                    Py::None => "null".to_string(),
                    Py::Int(n) => n.to_string(),
                    Py::Float(f) => float_json(*f),
                    other => {
                        return Err(YamlError::plain(
                            "TypeError",
                            format!("keys must be str, int, float, bool or None, not {}", other.type_name()),
                        ));
                    }
                };
                json_str(&key, out);
                out.push_str(": ");
                to_json(x, out)?;
            }
            out.push('}');
        }
    }
    Ok(())
}

// ---- leitura (json.JSONDecoder.raw_decode) ----

pub struct JsonError {
    pub msg: String,
    pub pos: usize,
}

/// Lê um valor JSON a partir de `pos` (sem pular espaço inicial, como o `raw_decode`). Devolve o
/// valor e a posição logo depois dele.
pub fn raw_decode(s: &[char], pos: usize) -> Result<(Py, usize), JsonError> {
    let mut p = Decoder { s, pos };
    let v = p.value()?;
    Ok((v, p.pos))
}

struct Decoder<'a> {
    s: &'a [char],
    pos: usize,
}

impl Decoder<'_> {
    fn peek(&self) -> Option<char> {
        self.s.get(self.pos).copied()
    }

    fn fail<T>(&self, msg: &str) -> Result<T, JsonError> {
        Err(JsonError { msg: msg.to_string(), pos: self.pos })
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
            self.pos += 1;
        }
    }

    fn value(&mut self) -> Result<Py, JsonError> {
        match self.peek() {
            Some('"') => Ok(Py::Str(self.string()?)),
            Some('{') => self.object(),
            Some('[') => self.array(),
            Some('n') if self.lit("null") => Ok(Py::None),
            Some('t') if self.lit("true") => Ok(Py::Bool(true)),
            Some('f') if self.lit("false") => Ok(Py::Bool(false)),
            Some('N') if self.lit("NaN") => Ok(Py::Float(f64::NAN)),
            Some('I') if self.lit("Infinity") => Ok(Py::Float(f64::INFINITY)),
            Some('-') if self.s.get(self.pos + 1) == Some(&'I') => {
                self.pos += 1;
                if self.lit("Infinity") { Ok(Py::Float(f64::NEG_INFINITY)) } else { self.fail("Expecting value") }
            }
            Some(c) if c == '-' || c.is_ascii_digit() => self.number(),
            _ => self.fail("Expecting value"),
        }
    }

    fn lit(&mut self, w: &str) -> bool {
        let n = w.chars().count();
        if self.s.len() >= self.pos + n && self.s[self.pos..self.pos + n].iter().copied().eq(w.chars()) {
            self.pos += n;
            return true;
        }
        false
    }

    fn number(&mut self) -> Result<Py, JsonError> {
        let start = self.pos;
        if self.peek() == Some('-') {
            self.pos += 1;
        }
        match self.peek() {
            Some('0') => self.pos += 1,
            Some(c) if c.is_ascii_digit() => {
                while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    self.pos += 1;
                }
            }
            _ => {
                self.pos = start;
                return self.fail("Expecting value");
            }
        }
        let mut is_float = false;
        if self.peek() == Some('.') && self.s.get(self.pos + 1).is_some_and(char::is_ascii_digit) {
            is_float = true;
            self.pos += 1;
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            let save = self.pos;
            self.pos += 1;
            if matches!(self.peek(), Some('+' | '-')) {
                self.pos += 1;
            }
            if self.peek().is_some_and(|c| c.is_ascii_digit()) {
                is_float = true;
                while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    self.pos += 1;
                }
            } else {
                self.pos = save;
            }
        }
        let text: String = self.s[start..self.pos].iter().collect();
        if is_float {
            Ok(Py::Float(text.parse().unwrap_or(0.0)))
        } else {
            Ok(Py::Int(text.parse().unwrap_or_else(|_| BigInt::zero())))
        }
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.pos += 1;
        let mut out = String::new();
        loop {
            let Some(c) = self.peek() else { return self.fail("Unterminated string starting at") };
            self.pos += 1;
            match c {
                '"' => return Ok(out),
                '\\' => {
                    let Some(e) = self.peek() else { return self.fail("Unterminated string starting at") };
                    self.pos += 1;
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
                            let hex = |d: &mut Decoder| -> Option<u32> {
                                let h: String = d.s.get(d.pos..d.pos + 4)?.iter().collect();
                                d.pos += 4;
                                u32::from_str_radix(&h, 16).ok()
                            };
                            let Some(mut code) = hex(self) else { return self.fail("Invalid \\uXXXX escape") };
                            if (0xd800..0xdc00).contains(&code)
                                && self.s.get(self.pos) == Some(&'\\')
                                && self.s.get(self.pos + 1) == Some(&'u')
                            {
                                let save = self.pos;
                                self.pos += 2;
                                match hex(self) {
                                    Some(lo) if (0xdc00..0xe000).contains(&lo) => {
                                        code = 0x10000 + ((code - 0xd800) << 10) + (lo - 0xdc00);
                                    }
                                    _ => self.pos = save,
                                }
                            }
                            out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                        }
                        _ => return self.fail("Invalid \\escape"),
                    }
                }
                c => out.push(c),
            }
        }
    }

    fn array(&mut self) -> Result<Py, JsonError> {
        self.pos += 1;
        let mut v = Vec::new();
        self.ws();
        if self.peek() == Some(']') {
            self.pos += 1;
            return Ok(Py::List(v));
        }
        loop {
            self.ws();
            v.push(self.value()?);
            self.ws();
            match self.peek() {
                Some(',') => self.pos += 1,
                Some(']') => {
                    self.pos += 1;
                    return Ok(Py::List(v));
                }
                _ => return self.fail("Expecting ',' delimiter"),
            }
        }
    }

    fn object(&mut self) -> Result<Py, JsonError> {
        self.pos += 1;
        let mut pairs = Vec::new();
        self.ws();
        if self.peek() == Some('}') {
            self.pos += 1;
            return Ok(Py::Dict(pairs));
        }
        loop {
            self.ws();
            if self.peek() != Some('"') {
                return self.fail("Expecting property name enclosed in double quotes");
            }
            let k = self.string()?;
            self.ws();
            if self.peek() != Some(':') {
                return self.fail("Expecting ':' delimiter");
            }
            self.pos += 1;
            self.ws();
            let v = self.value()?;
            match pairs.iter_mut().find(|(ek, _): &&mut (Py, Py)| matches!(ek, Py::Str(s) if *s == k)) {
                Some(slot) => slot.1 = v,
                None => pairs.push((Py::Str(k), v)),
            }
            self.ws();
            match self.peek() {
                Some(',') => self.pos += 1,
                Some('}') => {
                    self.pos += 1;
                    return Ok(Py::Dict(pairs));
                }
                _ => return self.fail("Expecting ',' delimiter"),
            }
        }
    }
}

/// Linha e coluna (1-based) de `pos`, como o `JSONDecodeError`.
pub fn json_error_text(s: &[char], e: &JsonError) -> String {
    let before = &s[..e.pos.min(s.len())];
    let line = before.iter().filter(|&&c| c == '\n').count() + 1;
    let col = match before.iter().rposition(|&c| c == '\n') {
        Some(p) => e.pos - p,
        None => e.pos + 1,
    };
    format!("{}: line {line} column {col} (char {})", e.msg, e.pos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::excessive_precision)] // o literal é o do YAML de teste, mais preciso que o double
    fn python_float_repr() {
        assert_eq!(float_repr(1000.0), "1000.0");
        assert_eq!(float_repr(6.02e23), "6.02e+23");
        assert_eq!(float_repr(1e16), "1e+16");
        assert_eq!(float_repr(1e15), "1000000000000000.0");
        assert_eq!(float_repr(0.0001), "0.0001");
        assert_eq!(float_repr(0.00001), "1e-05");
        assert_eq!(float_repr(1.5e-7), "1.5e-07");
        assert_eq!(float_repr(123456789.123456789), "123456789.12345679");
        assert_eq!(float_repr(-0.5), "-0.5");
        assert_eq!(float_repr(0.1), "0.1");
        assert_eq!(float_repr(2.5), "2.5");
    }
}
