//! Impressão de valores jaq no formato exato do jq 1.7.1 (o que o yq 3.4.3 mostra), mais a
//! representação de float do Python (`repr`), que é o literal que o yq entrega ao jq.
//!
//! Regras medidas no oráculo:
//! - número que veio de literal (entrada ou programa) sai na forma canônica do decNumber
//!   (`1e+16` vira `1E+16`, `1000.0` fica `1000.0`, `0.00001` fica `0.00001`);
//! - número calculado sai pelo `jvp_dtoa_fmt` (dígitos mínimos, sem `.0`, expoente com dois dígitos);
//! - infinito vira `1.7976931348623157e+308` e NaN vira `null`;
//! - strings escapam `"`, `\`, controles (`\b \f \n \r \t`, o resto como `\u00XX`) e DEL; o resto sai cru.

use jaq_json::{Num, Val};

/// Como indentar a saída.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Indent {
    Compact,
    Spaces(usize),
    Tab,
}

#[derive(Clone, Copy, Debug)]
pub struct JqStyle {
    pub indent: Indent,
    pub sort_keys: bool,
    pub ascii: bool,
}

impl Default for JqStyle {
    fn default() -> Self {
        JqStyle { indent: Indent::Spaces(2), sort_keys: false, ascii: false }
    }
}

/// Valor inteiro em JSON, do jeito que o jq imprime.
pub fn write_value(out: &mut String, v: &Val, style: &JqStyle) {
    write_level(out, v, style, 0);
}

fn indent_unit(style: &JqStyle) -> Option<String> {
    match style.indent {
        Indent::Compact | Indent::Spaces(0) => None,
        Indent::Spaces(n) => Some(" ".repeat(n)),
        Indent::Tab => Some("\t".into()),
    }
}

fn write_level(out: &mut String, v: &Val, style: &JqStyle, level: usize) {
    let unit = indent_unit(style);
    match v {
        Val::Null => out.push_str("null"),
        Val::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Val::Num(n) => out.push_str(&format_num(n)),
        Val::TStr(b) | Val::BStr(b) => write_string(out, &String::from_utf8_lossy(b), style.ascii),
        Val::Arr(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                if let Some(u) = &unit {
                    out.push('\n');
                    out.push_str(&u.repeat(level + 1));
                }
                write_level(out, item, style, level + 1);
            }
            if let Some(u) = &unit {
                out.push('\n');
                out.push_str(&u.repeat(level));
            }
            out.push(']');
        }
        Val::Obj(map) => {
            if map.is_empty() {
                out.push_str("{}");
                return;
            }
            let mut entries: Vec<(&Val, &Val)> = map.iter().collect();
            if style.sort_keys {
                entries.sort_by_key(|e| key_text(e.0));
            }
            out.push('{');
            for (i, (k, val)) in entries.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                if let Some(u) = &unit {
                    out.push('\n');
                    out.push_str(&u.repeat(level + 1));
                }
                write_string(out, &key_text(k), style.ascii);
                out.push(':');
                if unit.is_some() {
                    out.push(' ');
                }
                write_level(out, val, style, level + 1);
            }
            if let Some(u) = &unit {
                out.push('\n');
                out.push_str(&u.repeat(level));
            }
            out.push('}');
        }
    }
}

/// Texto de uma chave de objeto (no jq, chave é sempre string).
pub fn key_text(k: &Val) -> String {
    match k {
        Val::TStr(b) | Val::BStr(b) => String::from_utf8_lossy(b).into_owned(),
        other => {
            let mut s = String::new();
            write_level(&mut s, other, &JqStyle { indent: Indent::Compact, ..JqStyle::default() }, 0);
            s
        }
    }
}

/// String JSON com as regras de escape do jq.
pub fn write_string(out: &mut String, s: &str, ascii: bool) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c if ascii && !c.is_ascii() => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Número como o jq 1.7.1 imprime.
pub fn format_num(n: &Num) -> String {
    match n {
        Num::Int(i) => i.to_string(),
        Num::BigInt(b) => b.to_string(),
        Num::Float(f) => jq_dtoa(*f),
        Num::Dec(lit) => decnumber_canonical(lit).unwrap_or_else(|| jq_dtoa(lit.parse::<f64>().unwrap_or(f64::NAN))),
    }
}

/// Dígitos mínimos (round-trip) e posição do ponto decimal: valor = 0.d1d2... x 10^decpt.
fn shortest_digits(f: f64) -> (String, i32) {
    let sci = format!("{:e}", f.abs());
    let (mantissa, exp) = sci.split_once('e').expect("formato {:e}");
    let digits: String = mantissa.chars().filter(|c| c.is_ascii_digit()).collect();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0".to_string() } else { digits.to_string() };
    let exp: i32 = exp.parse().expect("expoente");
    (digits, exp + 1)
}

fn exponent_text(e: i32) -> String {
    let sign = if e < 0 { '-' } else { '+' };
    format!("{sign}{:02}", e.abs())
}

/// `jvp_dtoa_fmt` do jq: usado pra número calculado (não literal).
pub fn jq_dtoa(f: f64) -> String {
    if f.is_nan() {
        return "null".into();
    }
    let f = if f.is_infinite() { f64::MAX.copysign(f) } else { f };
    if f == 0.0 {
        return if f.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    let (digits, decpt) = shortest_digits(f);
    let sign = if f < 0.0 { "-" } else { "" };
    let nd = digits.len() as i32;
    let body = if decpt <= -4 || decpt > nd + 15 {
        let (first, rest) = digits.split_at(1);
        let frac = if rest.is_empty() { String::new() } else { format!(".{rest}") };
        format!("{first}{frac}e{}", exponent_text(decpt - 1))
    } else if decpt <= 0 {
        format!("0.{}{digits}", "0".repeat((-decpt) as usize))
    } else if decpt < nd {
        format!("{}.{}", &digits[..decpt as usize], &digits[decpt as usize..])
    } else {
        format!("{digits}{}", "0".repeat((decpt - nd) as usize))
    };
    format!("{sign}{body}")
}

/// `repr(float)` do Python 3 (o que o `json.dumps` do yq escreve).
pub fn python_repr(f: f64) -> String {
    if f.is_nan() {
        return "NaN".into();
    }
    if f.is_infinite() {
        return if f > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    if f == 0.0 {
        return if f.is_sign_negative() { "-0.0".into() } else { "0.0".into() };
    }
    let (digits, decpt) = shortest_digits(f);
    let sign = if f < 0.0 { "-" } else { "" };
    let nd = digits.len() as i32;
    let body = if decpt <= -4 || decpt > 16 {
        let (first, rest) = digits.split_at(1);
        let frac = if rest.is_empty() { String::new() } else { format!(".{rest}") };
        format!("{first}{frac}e{}", exponent_text(decpt - 1))
    } else if decpt <= 0 {
        format!("0.{}{digits}", "0".repeat((-decpt) as usize))
    } else if decpt < nd {
        format!("{}.{}", &digits[..decpt as usize], &digits[decpt as usize..])
    } else {
        format!("{digits}{}.0", "0".repeat((decpt - nd) as usize))
    };
    format!("{sign}{body}")
}

/// Forma canônica "to-scientific-string" do decNumber pra um literal numérico JSON.
pub fn decnumber_canonical(lit: &str) -> Option<String> {
    let (sign, rest) = match lit.strip_prefix('-') {
        Some(r) => ("-", r),
        None => ("", lit.strip_prefix('+').unwrap_or(lit)),
    };
    let (mant, exp) = match rest.find(['e', 'E']) {
        Some(i) => (&rest[..i], rest[i + 1..].parse::<i64>().ok()?),
        None => (rest, 0),
    };
    let (int_part, frac_part) = mant.split_once('.').unwrap_or((mant, ""));
    if int_part.is_empty() && frac_part.is_empty() {
        return None;
    }
    if !int_part.chars().chain(frac_part.chars()).all(|c| c.is_ascii_digit()) {
        return None;
    }
    let all: String = format!("{int_part}{frac_part}");
    let coeff = all.trim_start_matches('0');
    let coeff = if coeff.is_empty() { "0" } else { coeff };
    let e = exp - frac_part.len() as i64;
    let n = coeff.len() as i64;
    let adjusted = e + n - 1;
    let body = if e <= 0 && adjusted >= -6 {
        if e == 0 {
            coeff.to_string()
        } else {
            let point = n + e; // dígitos antes do ponto
            if point > 0 {
                format!("{}.{}", &coeff[..point as usize], &coeff[point as usize..])
            } else {
                format!("0.{}{coeff}", "0".repeat((-point) as usize))
            }
        }
    } else {
        let (first, rest) = coeff.split_at(1);
        let frac = if rest.is_empty() { String::new() } else { format!(".{rest}") };
        let esign = if adjusted >= 0 { '+' } else { '-' };
        format!("{first}{frac}E{esign}{}", adjusted.abs())
    };
    Some(format!("{sign}{body}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decnumber_matches_oracle_samples() {
        // Pares (literal do Python, saída do jq 1.7.1) tirados do golden do yq.
        for (lit, want) in [
            ("1000.0", "1000.0"),
            ("1e+16", "1E+16"),
            ("1e+20", "1E+20"),
            ("1e-05", "0.00001"),
            ("1.5e-07", "1.5E-7"),
            ("6.02e+23", "6.02E+23"),
            ("123456789.12345679", "123456789.12345679"),
            ("0.01", "0.01"),
            ("100000000000000000000", "100000000000000000000"),
            ("-0.5", "-0.5"),
        ] {
            assert_eq!(decnumber_canonical(lit).as_deref(), Some(want), "{lit}");
        }
    }

    #[test]
    fn python_repr_rules() {
        assert_eq!(python_repr(1000.0), "1000.0");
        assert_eq!(python_repr(1e16), "1e+16");
        assert_eq!(python_repr(1e15), "1000000000000000.0");
        assert_eq!(python_repr(0.0001), "0.0001");
        assert_eq!(python_repr(0.00001), "1e-05");
        assert_eq!(python_repr(6.02e23), "6.02e+23");
        assert_eq!(python_repr(-0.5), "-0.5");
        // O literal tem mais dígitos do que o f64 guarda de propósito: o repr do Python arredonda pro mais curto.
        assert_eq!(python_repr("123456789.123456789".parse().expect("f64")), "123456789.12345679");
    }

    #[test]
    fn dtoa_rules() {
        assert_eq!(jq_dtoa(2.0), "2");
        assert_eq!(jq_dtoa(2000.0), "2000");
        assert_eq!(jq_dtoa(2.5), "2.5");
        assert_eq!(jq_dtoa(-0.4), "-0.4");
        assert_eq!(jq_dtoa(0.1 - 0.5), "-0.4");
        assert_eq!(jq_dtoa(f64::INFINITY), "1.7976931348623157e+308");
        assert_eq!(jq_dtoa(1e17), "1e+17");
        assert_eq!(jq_dtoa(1e15), "1000000000000000");
        assert_eq!(jq_dtoa(0.00001), "1e-05");
        assert_eq!(jq_dtoa(f64::NAN), "null");
    }

    #[test]
    fn pretty_and_compact_layout() {
        let v = jaq_json::read::parse_single(br#"{"a":[1,{"b":[]}],"c":{}}"#).unwrap();
        let mut compact = String::new();
        write_value(&mut compact, &v, &JqStyle { indent: Indent::Compact, ..JqStyle::default() });
        assert_eq!(compact, r#"{"a":[1,{"b":[]}],"c":{}}"#);
        let mut pretty = String::new();
        write_value(&mut pretty, &v, &JqStyle::default());
        assert_eq!(pretty, "{\n  \"a\": [\n    1,\n    {\n      \"b\": []\n    }\n  ],\n  \"c\": {}\n}");
    }

    #[test]
    fn string_escapes_like_jq() {
        let mut s = String::new();
        write_string(&mut s, "t\t\u{1}\u{7f}é\"", false);
        assert_eq!(s, "\"t\\t\\u0001\\u007fé\\\"\"");
    }
}
