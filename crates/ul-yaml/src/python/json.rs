//! `json.loads` do CPython 3.13: o scanner em C (`Modules/_json.c`: `scan_once_unicode`,
//! `scanstring_unicode`, `_match_number_unicode`) e o `decode` em Python por cima. As posições dos
//! erros e as mensagens são as do original.
//!
//! O scanner é iterativo (pilha explícita) pra aguentar o aninhamento que o Python aguenta (a
//! trava dele é a de recursão em C, 9998 níveis) sem estourar a pilha da thread.

use num_bigint::BigInt;

use crate::py::{Py, make_dict};

use super::text::surrogate_to_char;

/// Níveis de contêiner aninhados que o `_Py_EnterRecursiveCall` do 3.13 deixa passar.
const MAX_DEPTH: usize = 9998;

/// `sys.get_int_max_str_digits()` de fábrica.
const INT_MAX_STR_DIGITS: usize = 4300;

/// Como o `json.loads` falha, com o que a tela do erro precisa pra montar o traceback.
#[derive(Debug)]
pub enum LoadError {
    /// `JSONDecodeError` levantado dentro do scanner em C (aparece no frame de `scan_once`).
    Scan { msg: &'static str, pos: usize },
    /// `StopIteration` do scanner: o `raw_decode` a troca por "Expecting value".
    Stop { pos: usize },
    /// "Extra data", levantado pelo `decode`.
    Extra { pos: usize },
    /// "Unexpected UTF-8 BOM", levantado pelo `loads`.
    Bom,
    /// `ValueError` do limite de dígitos de inteiro.
    Value(String),
    /// `RecursionError`: `array` ou `object`.
    Recursion(&'static str),
}

fn is_ws(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r')
}

fn skip_ws(s: &[char], mut idx: usize) -> usize {
    while idx < s.len() && is_ws(s[idx]) {
        idx += 1;
    }
    idx
}

/// `json.loads(texto)`.
pub fn loads(s: &[char]) -> Result<Py, LoadError> {
    if s.first() == Some(&'\u{feff}') {
        return Err(LoadError::Bom);
    }
    let start = skip_ws(s, 0);
    let (value, end) = scan_once(s, start)?;
    let end = skip_ws(s, end);
    if end != s.len() {
        return Err(LoadError::Extra { pos: end });
    }
    Ok(value)
}

/// `s[idx..]` começa com `word` (ASCII)?
fn matches_at(s: &[char], idx: usize, word: &str) -> bool {
    let n = word.len();
    s.len() >= idx + n && s[idx..idx + n].iter().copied().eq(word.chars())
}

/// Quatro dígitos hexadecimais a partir de `from`; o erro aponta pra `err_pos`.
fn hex4(s: &[char], from: usize, err_pos: usize) -> Result<u32, LoadError> {
    let mut code: u32 = 0;
    for &c in &s[from..from + 4] {
        match c.to_digit(16) {
            Some(d) => code = (code << 4) | d,
            None => return Err(LoadError::Scan { msg: "Invalid \\uXXXX escape", pos: err_pos }),
        }
    }
    Ok(code)
}

fn code_to_char(code: u32) -> char {
    if (0xD800..=0xDFFF).contains(&code) {
        surrogate_to_char(code)
    } else {
        char::from_u32(code).unwrap_or('\u{fffd}')
    }
}

/// `scanstring_unicode` (modo estrito): `end` é o índice logo depois da aspa de abertura. Devolve o
/// texto e o índice logo depois da aspa de fecho.
fn scan_string(s: &[char], mut end: usize) -> Result<(String, usize), LoadError> {
    let begin = end - 1;
    let len = s.len();
    let mut out = String::new();
    loop {
        // O fim da cadeia ou o próximo escape.
        let mut next = end;
        while next < len {
            let c = s[next];
            if c == '"' || c == '\\' {
                break;
            }
            if (c as u32) <= 0x1f {
                return Err(LoadError::Scan { msg: "Invalid control character at", pos: next });
            }
            next += 1;
        }
        if next >= len {
            return Err(LoadError::Scan { msg: "Unterminated string starting at", pos: begin });
        }
        out.extend(s[end..next].iter());
        let delim = s[next];
        next += 1;
        if delim == '"' {
            return Ok((out, next));
        }
        if next == len {
            return Err(LoadError::Scan { msg: "Unterminated string starting at", pos: begin });
        }
        let esc = s[next];
        let decoded: char;
        if esc == 'u' {
            next += 1;
            end = next + 4;
            if end >= len {
                return Err(LoadError::Scan { msg: "Invalid \\uXXXX escape", pos: next - 1 });
            }
            let mut code = hex4(s, next, next - 1)?;
            // Par substituto: `😀` vira um caractere só.
            if (0xD800..0xDC00).contains(&code) && end + 6 < len && s[end] == '\\' && s[end + 1] == 'u' {
                let low = hex4(s, end + 2, end + 1)?;
                if (0xDC00..0xE000).contains(&low) {
                    code = 0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00);
                    end += 6;
                }
            }
            decoded = code_to_char(code);
        } else {
            end = next + 1;
            decoded = match esc {
                '"' => '"',
                '\\' => '\\',
                '/' => '/',
                'b' => '\u{8}',
                'f' => '\u{c}',
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                _ => return Err(LoadError::Scan { msg: "Invalid \\escape", pos: end - 2 }),
            };
        }
        out.push(decoded);
    }
}

/// `_match_number_unicode`: `start` aponta pro primeiro caractere do número (existe).
fn scan_number(s: &[char], start: usize) -> Result<(Py, usize), LoadError> {
    let end_idx = s.len() - 1;
    let is_digit = |i: usize| s[i].is_ascii_digit();
    let mut idx = start;
    if s[idx] == '-' {
        idx += 1;
        if idx > end_idx {
            return Err(LoadError::Stop { pos: start });
        }
    }
    if ('1'..='9').contains(&s[idx]) {
        idx += 1;
        while idx <= end_idx && is_digit(idx) {
            idx += 1;
        }
    } else if s[idx] == '0' {
        idx += 1;
    } else {
        return Err(LoadError::Stop { pos: start });
    }
    let mut is_float = false;
    if idx < end_idx && s[idx] == '.' && is_digit(idx + 1) {
        is_float = true;
        idx += 2;
        while idx <= end_idx && is_digit(idx) {
            idx += 1;
        }
    }
    if idx < end_idx && (s[idx] == 'e' || s[idx] == 'E') {
        let e_start = idx;
        idx += 1;
        if idx < end_idx && (s[idx] == '-' || s[idx] == '+') {
            idx += 1;
        }
        while idx <= end_idx && is_digit(idx) {
            idx += 1;
        }
        if is_digit(idx - 1) {
            is_float = true;
        } else {
            idx = e_start;
        }
    }
    let text: String = s[start..idx].iter().collect();
    if is_float {
        // A gramática acima só deixa passar literais que o Rust lê (estouro vira infinito, como no Python).
        return Ok((Py::Float(text.parse().unwrap_or(0.0)), idx));
    }
    let digits = text.trim_start_matches('-').len();
    if digits > INT_MAX_STR_DIGITS {
        return Err(LoadError::Value(format!(
            "Exceeds the limit ({INT_MAX_STR_DIGITS} digits) for integer string conversion: value has {digits} digits; use sys.set_int_max_str_digits() to increase the limit"
        )));
    }
    Ok((Py::Int(text.parse::<BigInt>().unwrap_or_default()), idx))
}

/// Um nível de contêiner aberto.
enum Frame {
    Array(Vec<Py>),
    Object { pairs: Vec<(Py, Py)>, key: String },
}

/// A chave de um objeto: aspas, texto, `:` e o espaço depois dele. Deixa `idx` no começo do valor.
fn read_key(s: &[char], idx: &mut usize) -> Result<String, LoadError> {
    if *idx >= s.len() || s[*idx] != '"' {
        return Err(LoadError::Scan { msg: "Expecting property name enclosed in double quotes", pos: *idx });
    }
    let (key, next) = scan_string(s, *idx + 1)?;
    let mut i = skip_ws(s, next);
    if i >= s.len() || s[i] != ':' {
        return Err(LoadError::Scan { msg: "Expecting ':' delimiter", pos: i });
    }
    i += 1;
    *idx = skip_ws(s, i);
    Ok(key)
}

/// `scan_once_unicode`: o valor que começa em `start` e o índice logo depois dele.
pub fn scan_once(s: &[char], start: usize) -> Result<(Py, usize), LoadError> {
    let len = s.len();
    let mut stack: Vec<Frame> = Vec::new();
    let mut idx = start;
    loop {
        // Fase A: lê um valor em `idx`, ou abre um contêiner e volta pra ler o primeiro elemento.
        if idx >= len {
            return Err(LoadError::Stop { pos: idx });
        }
        let mut value = match s[idx] {
            '"' => {
                let (text, next) = scan_string(s, idx + 1)?;
                idx = next;
                Py::Str(text)
            }
            '{' => {
                if stack.len() + 1 > MAX_DEPTH {
                    return Err(LoadError::Recursion("object"));
                }
                idx = skip_ws(s, idx + 1);
                if idx < len && s[idx] == '}' {
                    idx += 1;
                    Py::Dict(Vec::new())
                } else {
                    let key = read_key(s, &mut idx)?;
                    stack.push(Frame::Object { pairs: Vec::new(), key });
                    continue;
                }
            }
            '[' => {
                if stack.len() + 1 > MAX_DEPTH {
                    return Err(LoadError::Recursion("array"));
                }
                idx = skip_ws(s, idx + 1);
                if idx < len && s[idx] == ']' {
                    idx += 1;
                    Py::List(Vec::new())
                } else {
                    stack.push(Frame::Array(Vec::new()));
                    continue;
                }
            }
            'n' if matches_at(s, idx, "null") => {
                idx += 4;
                Py::None
            }
            't' if matches_at(s, idx, "true") => {
                idx += 4;
                Py::Bool(true)
            }
            'f' if matches_at(s, idx, "false") => {
                idx += 5;
                Py::Bool(false)
            }
            'N' if matches_at(s, idx, "NaN") => {
                idx += 3;
                Py::Float(f64::NAN)
            }
            'I' if matches_at(s, idx, "Infinity") => {
                idx += 8;
                Py::Float(f64::INFINITY)
            }
            '-' if matches_at(s, idx, "-Infinity") => {
                idx += 9;
                Py::Float(f64::NEG_INFINITY)
            }
            _ => {
                let (number, next) = scan_number(s, idx)?;
                idx = next;
                number
            }
        };
        // Fase B: entrega o valor ao contêiner aberto; fechar um pode fechar o de fora.
        loop {
            match stack.pop() {
                None => return Ok((value, idx)),
                Some(Frame::Array(mut items)) => {
                    items.push(value);
                    idx = skip_ws(s, idx);
                    if idx < len && s[idx] == ']' {
                        idx += 1;
                        value = Py::List(items);
                        continue;
                    }
                    if idx >= len || s[idx] != ',' {
                        return Err(LoadError::Scan { msg: "Expecting ',' delimiter", pos: idx });
                    }
                    let comma = idx;
                    idx = skip_ws(s, idx + 1);
                    if idx < len && s[idx] == ']' {
                        return Err(LoadError::Scan { msg: "Illegal trailing comma before end of array", pos: comma });
                    }
                    stack.push(Frame::Array(items));
                    break;
                }
                Some(Frame::Object { mut pairs, key }) => {
                    pairs.push((Py::Str(key), value));
                    idx = skip_ws(s, idx);
                    if idx < len && s[idx] == '}' {
                        idx += 1;
                        // As chaves são sempre texto: `make_dict` só junta as repetidas.
                        value = make_dict(pairs).unwrap_or(Py::Dict(Vec::new()));
                        continue;
                    }
                    if idx >= len || s[idx] != ',' {
                        return Err(LoadError::Scan { msg: "Expecting ',' delimiter", pos: idx });
                    }
                    let comma = idx;
                    idx = skip_ws(s, idx + 1);
                    if idx < len && s[idx] == '}' {
                        return Err(LoadError::Scan { msg: "Illegal trailing comma before end of object", pos: comma });
                    }
                    let key = read_key(s, &mut idx)?;
                    stack.push(Frame::Object { pairs, key });
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    #[test]
    fn scans_values() {
        let v = loads(&chars("[1, 2.5, null, true, \"a\\u00e9\", {\"k\": [ ]}]\n")).unwrap();
        match v {
            Py::List(items) => assert_eq!(items.len(), 6),
            other => panic!("esperava lista, veio {other:?}"),
        }
    }

    #[test]
    fn error_positions() {
        assert!(matches!(loads(&chars("[1,]")), Err(LoadError::Scan { pos: 2, .. })));
        assert!(matches!(loads(&chars("[1] x")), Err(LoadError::Extra { pos: 4 })));
        assert!(matches!(loads(&chars("\n")), Err(LoadError::Stop { pos: 1 })));
        assert!(matches!(loads(&chars("\"a\\qb\"")), Err(LoadError::Scan { pos: 2, .. })));
        assert!(matches!(loads(&chars("[01]")), Err(LoadError::Scan { pos: 2, .. })));
        assert!(matches!(loads(&chars("\u{feff}[1]")), Err(LoadError::Bom)));
    }
}
