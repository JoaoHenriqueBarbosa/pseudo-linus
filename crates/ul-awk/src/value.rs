//! Valores do awk: número, texto, texto de entrada ("strnum"), não inicializado e regex tipada, com as
//! conversões e a comparação do gawk 5.2.1.

use std::cmp::Ordering;
use std::rc::Rc;

pub type Str = Rc<[u8]>;

#[derive(Clone, Debug)]
#[derive(Default)]
pub enum Value {
    /// Variável nunca atribuída: vale `""` e `0` ao mesmo tempo.
    #[default]
    Uninit,
    Num(f64),
    Str(Str),
    /// Texto vindo da entrada (campos, `getline`, `split`, `ARGV`, `ENVIRON`, `-v`): se tiver cara de
    /// número, compara como número.
    StrNum(Str),
    /// Regex tipada (`@/.../`): índice na tabela de regexes e o texto.
    Regex(u32, Str),
    /// Valor booleano do `mkbool` (número 0 ou 1 com o tipo `number|bool`).
    Bool(bool),
}


pub fn empty_str() -> Str {
    Rc::from(&b""[..])
}

pub fn str_from(s: &[u8]) -> Str {
    Rc::from(s)
}

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

/// NaN com o sinal pedido (o gawk escreve `-nan`/`+nan` conforme o bit de sinal).
pub fn nan(negative: bool) -> f64 {
    if negative { f64::from_bits(0xfff8_0000_0000_0000) } else { f64::from_bits(0x7ff8_0000_0000_0000) }
}

/// Valores especiais `+inf`, `-inf`, `+nan`, `-nan` (com sinal, quatro caracteres), como o gawk aceita.
fn ieee_magic(s: &[u8]) -> Option<f64> {
    if s.len() != 4 || !matches!(s[0], b'+' | b'-') {
        return None;
    }
    let neg = s[0] == b'-';
    let word = s[1..].to_ascii_lowercase();
    match &word[..] {
        b"inf" => Some(if neg { f64::NEG_INFINITY } else { f64::INFINITY }),
        b"nan" => Some(nan(neg)),
        _ => None,
    }
}

/// Comprimento do maior prefixo numérico decimal em `s` (sem espaços iniciais), ou 0.
fn numeric_prefix_len(s: &[u8]) -> usize {
    let mut i = 0;
    if matches!(s.first(), Some(b'+' | b'-')) {
        i = 1;
    }
    let digits_start = i;
    while s.get(i).is_some_and(|c| c.is_ascii_digit()) {
        i += 1;
    }
    let mut ndigits = i - digits_start;
    if s.get(i) == Some(&b'.') {
        let mut j = i + 1;
        while s.get(j).is_some_and(|c| c.is_ascii_digit()) {
            j += 1;
        }
        ndigits += j - i - 1;
        if ndigits == 0 {
            return 0;
        }
        i = j;
    }
    if ndigits == 0 {
        return 0;
    }
    if matches!(s.get(i), Some(b'e' | b'E')) {
        let mut j = i + 1;
        if matches!(s.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        if s.get(j).is_some_and(|c| c.is_ascii_digit()) {
            while s.get(j).is_some_and(|c| c.is_ascii_digit()) {
                j += 1;
            }
            i = j;
        }
    }
    i
}

fn parse_prefix(s: &[u8]) -> f64 {
    let s = std::str::from_utf8(s).unwrap_or("0");
    s.parse::<f64>().unwrap_or(0.0)
}

/// Texto pra número como o gawk (sem `--non-decimal-data`): prefixo decimal, `+inf`/`-nan` aceitos só
/// com sinal, hexadecimal não.
pub fn str_to_num(s: &[u8]) -> f64 {
    let start = s.iter().position(|c| !is_space(*c)).unwrap_or(s.len());
    let t = &s[start..];
    if t.len() >= 4 {
        let end = t.iter().rposition(|c| !is_space(*c)).map(|i| i + 1).unwrap_or(0);
        if let Some(v) = ieee_magic(&t[..end]) {
            return v;
        }
    }
    let n = numeric_prefix_len(t);
    if n == 0 {
        return 0.0;
    }
    parse_prefix(&t[..n])
}

/// O texto inteiro é um número (com espaços em volta)? É o teste do "strnum".
pub fn looks_numeric(s: &[u8]) -> bool {
    let start = s.iter().position(|c| !is_space(*c));
    let Some(start) = start else { return false };
    let end = s.iter().rposition(|c| !is_space(*c)).map(|i| i + 1).unwrap_or(start);
    let t = &s[start..end];
    if ieee_magic(t).is_some() {
        return true;
    }
    let n = numeric_prefix_len(t);
    n > 0 && n == t.len()
}

/// Potência como o gawk: expoente inteiro por multiplicações sucessivas, o resto pelo `pow`.
pub fn pow(x: f64, y: f64) -> f64 {
    if y == y.trunc() && y.abs() < 1e18 {
        let mut n = y.abs() as u64;
        let mut base = x;
        let mut acc = 1.0f64;
        while n > 0 {
            if n & 1 == 1 {
                acc *= base;
            }
            base *= base;
            n >>= 1;
        }
        if y < 0.0 { 1.0 / acc } else { acc }
    } else {
        x.powf(y)
    }
}

impl Value {
    pub fn num(n: f64) -> Value {
        Value::Num(n)
    }

    pub fn from_bytes(s: &[u8]) -> Value {
        Value::Str(Rc::from(s))
    }

    pub fn strnum(s: &[u8]) -> Value {
        Value::StrNum(Rc::from(s))
    }

    pub fn to_num(&self) -> f64 {
        match self {
            Value::Uninit => 0.0,
            Value::Num(n) => *n,
            Value::Str(s) | Value::StrNum(s) => str_to_num(s),
            Value::Regex(..) => 0.0,
            Value::Bool(b) => *b as i32 as f64,
        }
    }

    /// Verdadeiro/falso do awk.
    pub fn truthy(&self) -> bool {
        match self {
            Value::Uninit => false,
            Value::Num(n) => *n != 0.0,
            Value::Str(s) => !s.is_empty(),
            Value::StrNum(s) => {
                if looks_numeric(s) {
                    str_to_num(s) != 0.0
                } else {
                    !s.is_empty()
                }
            }
            Value::Regex(..) => true,
            Value::Bool(b) => *b,
        }
    }

    /// É número pra comparação (número, strnum com cara de número, não inicializado)?
    pub fn is_numeric(&self) -> bool {
        match self {
            Value::Num(_) | Value::Bool(_) => true,
            Value::StrNum(s) => looks_numeric(s),
            _ => false,
        }
    }

    pub fn is_uninit(&self) -> bool {
        matches!(self, Value::Uninit)
    }
}

/// Comparação de dois números com NaN como o gawk: qualquer comparação com NaN é falsa, então a
/// ordem devolvida faz `==`, `<` e `>` falharem.
pub fn cmp_num(a: f64, b: f64) -> Option<Ordering> {
    a.partial_cmp(&b)
}

pub fn cmp_bytes(a: &[u8], b: &[u8]) -> Ordering {
    a.cmp(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions() {
        assert_eq!(str_to_num(b" 12 "), 12.0);
        assert_eq!(str_to_num(b"1e3x"), 1000.0);
        assert_eq!(str_to_num(b".5"), 0.5);
        assert_eq!(str_to_num(b"+.5e+2"), 50.0);
        assert_eq!(str_to_num(b"0x1A"), 0.0);
        assert_eq!(str_to_num(b"inf"), 0.0);
        assert_eq!(str_to_num(b"+inf"), f64::INFINITY);
        assert_eq!(str_to_num(b"+infinity"), 0.0);
        assert!(str_to_num(b"-nan").is_nan());
        assert_eq!(str_to_num(b"1e500"), f64::INFINITY);
        assert!(looks_numeric(b" 1e3 "));
        assert!(!looks_numeric(b"1e3x"));
        assert!(!looks_numeric(b""));
        assert!(!looks_numeric(b"."));
        assert!(looks_numeric(b"1."));
        assert!(looks_numeric(b"+5"));
    }

    #[test]
    fn power() {
        assert_eq!(pow(2.0, 10.0), 1024.0);
        assert_eq!(pow(2.0, -1.0), 0.5);
    }
}
