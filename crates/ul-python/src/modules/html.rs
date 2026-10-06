//! Módulo `html` do CPython 3.13: `escape` e `unescape`.
//!
//! `unescape` conhece as referências numéricas (com a tabela de substituição do HTML5 para
//! 0x80 a 0x9F e os pontos de código inválidos) e um subconjunto das entidades nomeadas: as 96 do
//! Latin-1 (com e sem `;`), `amp`/`lt`/`gt`/`quot`/`apos` e algumas de pontuação, setas, letras
//! gregas e símbolos comuns. As outras (mais de 2000 no HTML5) ficam de fora e são devolvidas
//! como estão.

use std::rc::Rc;

use crate::modules::ModuleBuilder;
use crate::native_util::{bind, want_str};
use crate::object::{Kw, ModuleObj, Value};
use crate::vm::{PyResult, Vm};

fn escape(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("escape", args, kw, &["s", "quote"], 1)?;
    let text = want_str("escape", s[0].as_ref().unwrap())?;
    let quote = s[1].as_ref().map(|v| v.is_true()).unwrap_or(true);
    Ok(Value::str(escape_str(text, quote)))
}

pub fn escape_str(text: &str, quote: bool) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if quote => out.push_str("&quot;"),
            '\'' if quote => out.push_str("&#x27;"),
            c => out.push(c),
        }
    }
    out
}

/// Nomes das entidades Latin-1 de U+00A0 a U+00FF, em ordem.
const LATIN1: [&str; 96] = [
    "nbsp", "iexcl", "cent", "pound", "curren", "yen", "brvbar", "sect", "uml", "copy", "ordf", "laquo", "not", "shy",
    "reg", "macr", "deg", "plusmn", "sup2", "sup3", "acute", "micro", "para", "middot", "cedil", "sup1", "ordm",
    "raquo", "frac14", "frac12", "frac34", "iquest", "Agrave", "Aacute", "Acirc", "Atilde", "Auml", "Aring", "AElig",
    "Ccedil", "Egrave", "Eacute", "Ecirc", "Euml", "Igrave", "Iacute", "Icirc", "Iuml", "ETH", "Ntilde", "Ograve",
    "Oacute", "Ocirc", "Otilde", "Ouml", "times", "Oslash", "Ugrave", "Uacute", "Ucirc", "Uuml", "Yacute", "THORN",
    "szlig", "agrave", "aacute", "acirc", "atilde", "auml", "aring", "aelig", "ccedil", "egrave", "eacute", "ecirc",
    "euml", "igrave", "iacute", "icirc", "iuml", "eth", "ntilde", "ograve", "oacute", "ocirc", "otilde", "ouml",
    "divide", "oslash", "ugrave", "uacute", "ucirc", "uuml", "yacute", "thorn", "yuml",
];

/// Entidades que o HTML5 aceita também sem o `;` final.
fn is_legacy(name: &str) -> bool {
    matches!(name, "amp" | "lt" | "gt" | "quot" | "AMP" | "LT" | "GT" | "QUOT" | "COPY" | "REG") || LATIN1.contains(&name)
}

fn named(name: &str) -> Option<char> {
    let c = match name {
        "amp" | "AMP" => '&',
        "lt" | "LT" => '<',
        "gt" | "GT" => '>',
        "quot" | "QUOT" => '"',
        "COPY" => '\u{a9}',
        "REG" => '\u{ae}',
        "apos" => '\'',
        "euro" => '\u{20ac}',
        "hellip" => '\u{2026}',
        "mdash" => '\u{2014}',
        "ndash" => '\u{2013}',
        "lsquo" => '\u{2018}',
        "rsquo" => '\u{2019}',
        "sbquo" => '\u{201a}',
        "ldquo" => '\u{201c}',
        "rdquo" => '\u{201d}',
        "bdquo" => '\u{201e}',
        "dagger" => '\u{2020}',
        "Dagger" => '\u{2021}',
        "bull" => '\u{2022}',
        "permil" => '\u{2030}',
        "trade" => '\u{2122}',
        "larr" => '\u{2190}',
        "uarr" => '\u{2191}',
        "rarr" => '\u{2192}',
        "darr" => '\u{2193}',
        "harr" => '\u{2194}',
        "hearts" => '\u{2665}',
        "infin" => '\u{221e}',
        "ne" => '\u{2260}',
        "le" => '\u{2264}',
        "ge" => '\u{2265}',
        "alpha" => '\u{3b1}',
        "beta" => '\u{3b2}',
        "gamma" => '\u{3b3}',
        "delta" => '\u{3b4}',
        "lambda" => '\u{3bb}',
        "mu" => '\u{3bc}',
        "pi" => '\u{3c0}',
        "sigma" => '\u{3c3}',
        "omega" => '\u{3c9}',
        _ => {
            return LATIN1.iter().position(|n| *n == name).and_then(|i| char::from_u32(160 + i as u32));
        }
    };
    Some(c)
}

fn invalid_charref(n: u64) -> Option<&'static str> {
    Some(match n {
        0x00 => "\u{fffd}",
        0x0d => "\r",
        0x80 => "\u{20ac}",
        0x81 => "\u{81}",
        0x82 => "\u{201a}",
        0x83 => "\u{192}",
        0x84 => "\u{201e}",
        0x85 => "\u{2026}",
        0x86 => "\u{2020}",
        0x87 => "\u{2021}",
        0x88 => "\u{2c6}",
        0x89 => "\u{2030}",
        0x8a => "\u{160}",
        0x8b => "\u{2039}",
        0x8c => "\u{152}",
        0x8d => "\u{8d}",
        0x8e => "\u{17d}",
        0x8f => "\u{8f}",
        0x90 => "\u{90}",
        0x91 => "\u{2018}",
        0x92 => "\u{2019}",
        0x93 => "\u{201c}",
        0x94 => "\u{201d}",
        0x95 => "\u{2022}",
        0x96 => "\u{2013}",
        0x97 => "\u{2014}",
        0x98 => "\u{2dc}",
        0x99 => "\u{2122}",
        0x9a => "\u{161}",
        0x9b => "\u{203a}",
        0x9c => "\u{153}",
        0x9d => "\u{9d}",
        0x9e => "\u{17e}",
        0x9f => "\u{178}",
        _ => return None,
    })
}

fn invalid_codepoint(n: u64) -> bool {
    (1..=8).contains(&n)
        || (0xe..=0x1f).contains(&n)
        || (0x7f..=0x9f).contains(&n)
        || (0xfdd0..=0xfdef).contains(&n)
        || n == 0xb
        || (n <= 0x10ffff && matches!(n & 0xffff, 0xfffe | 0xffff))
}

fn charref(n: u64) -> String {
    if let Some(s) = invalid_charref(n) {
        return s.to_string();
    }
    if (0xd800..=0xdfff).contains(&n) || n > 0x10ffff {
        return "\u{fffd}".to_string();
    }
    if invalid_codepoint(n) {
        return String::new();
    }
    char::from_u32(n as u32).map(|c| c.to_string()).unwrap_or_default()
}

/// `html.unescape(s)`.
pub fn unescape_str(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '&' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let mut j = i + 1;
        if chars.get(j) == Some(&'#') {
            j += 1;
            let hex = matches!(chars.get(j), Some('x' | 'X'));
            if hex {
                j += 1;
            }
            let radix = if hex { 16 } else { 10 };
            let start = j;
            let mut acc: u64 = 0;
            while let Some(d) = chars.get(j).and_then(|c| c.to_digit(radix)) {
                acc = acc.saturating_mul(u64::from(radix)).saturating_add(u64::from(d));
                j += 1;
            }
            if j == start {
                out.push('&');
                i += 1;
                continue;
            }
            if chars.get(j) == Some(&';') {
                j += 1;
            }
            out.push_str(&charref(acc));
            i = j;
            continue;
        }
        let start = j;
        while j < chars.len() && j - start < 32 && !matches!(chars[j], '\t' | '\n' | '\u{c}' | ' ' | '<' | '&' | '#' | ';') {
            j += 1;
        }
        if j == start {
            out.push('&');
            i += 1;
            continue;
        }
        if chars.get(j) == Some(&';') {
            j += 1;
        }
        let s: Vec<char> = chars[start..j].to_vec();
        let whole: String = s.iter().collect();
        let mut replaced = false;
        if s.last() == Some(&';') {
            let name: String = s[..s.len() - 1].iter().collect();
            if let Some(c) = named(&name) {
                out.push(c);
                replaced = true;
            }
        } else if is_legacy(&whole) {
            if let Some(c) = named(&whole) {
                out.push(c);
                replaced = true;
            }
        }
        if !replaced {
            for x in (2..s.len()).rev() {
                let prefix: String = s[..x].iter().collect();
                if is_legacy(&prefix) {
                    if let Some(c) = named(&prefix) {
                        out.push(c);
                        out.extend(s[x..].iter());
                        replaced = true;
                        break;
                    }
                }
            }
        }
        if !replaced {
            out.push('&');
            out.push_str(&whole);
        }
        i = j;
    }
    out
}

fn unescape(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("unescape", args, kw, &["s"], 1)?;
    let text = want_str("unescape", s[0].as_ref().unwrap())?;
    Ok(Value::str(unescape_str(text)))
}

pub fn build(_vm: &mut Vm) -> Rc<ModuleObj> {
    ModuleBuilder::new("html").func("escape", escape).func("unescape", unescape).build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_cases() {
        assert_eq!(escape_str("<a href=\"x\">&</a>", true), "&lt;a href=&quot;x&quot;&gt;&amp;&lt;/a&gt;");
        assert_eq!(escape_str("it's", true), "it&#x27;s");
        assert_eq!(escape_str("it's \"q\"", false), "it's \"q\"");
    }

    #[test]
    fn escape_through_module_function() {
        let mut vm = Vm::new();
        let kw = vec![("quote".to_string(), Value::Bool(false))];
        let v = escape(&mut vm, vec![Value::str("a\"b<")], kw).unwrap();
        assert_eq!(crate::object::to_str(&v), "a\"b&lt;");
    }

    #[test]
    fn unescape_cases() {
        assert_eq!(unescape_str("&lt;b&gt; &amp; &#65; &#x42; &eacute; &hellip;"), "<b> & A B \u{e9} \u{2026}");
        assert_eq!(unescape_str("&copy x"), "\u{a9} x");
        assert_eq!(unescape_str("&bogus; &amp"), "&bogus; &");
        assert_eq!(unescape_str("&notit;"), "\u{ac}it;");
        assert_eq!(unescape_str("a & b &# c"), "a & b &# c");
        assert_eq!(unescape_str("&#0; &#x110000; &#128;"), "\u{fffd} \u{fffd} \u{20ac}");
        assert_eq!(unescape_str("plain"), "plain");
        assert_eq!(unescape_str("&#1;"), "");
    }
}
