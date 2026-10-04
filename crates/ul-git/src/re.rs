//! Regex POSIX (BRE e ERE, com as extensões GNU descritas no manual do grep: `\|`, `\+`, `\?`,
//! `\<`, `\>`, `\w`, `\b`...) traduzidas pra sintaxe do crate `regex`. Referência: o capítulo
//! "Regular Expressions" do POSIX.1-2017 e o manual do GNU grep. Referência para trás (`\1`) não é
//! suportada pelo motor e dá erro de compilação.

use regex::bytes::{Regex, RegexBuilder};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Flavor {
    Basic,
    Extended,
    /// `-F`: texto literal.
    Fixed,
    /// `-P`: a sintaxe do motor, quase igual à do PCRE no que agentes usam.
    Perl,
}

/// Traduz uma expressão de colchetes começando em `i` (logo depois do `[`); devolve o fim.
fn bracket(p: &[u8], mut i: usize, out: &mut String) -> Result<usize, String> {
    out.push('[');
    if p.get(i) == Some(&b'^') {
        out.push('^');
        i += 1;
    }
    let mut first = true;
    loop {
        let Some(&c) = p.get(i) else { return Err("Unmatched [, [^, [:, [., or [=".into()) };
        if c == b']' && !first {
            out.push(']');
            return Ok(i + 1);
        }
        first = false;
        if c == b'[' && matches!(p.get(i + 1), Some(b':') | Some(b'=') | Some(b'.')) {
            let kind = p[i + 1];
            let mut j = i + 2;
            while j + 1 < p.len() && !(p[j] == kind && p[j + 1] == b']') {
                j += 1;
            }
            if j + 1 >= p.len() {
                return Err("Unmatched [, [^, [:, [., or [=".into());
            }
            let name = &p[i + 2..j];
            match kind {
                b':' => {
                    let n = std::str::from_utf8(name).map_err(|_| "Invalid character class name".to_string())?;
                    if !matches!(n, "alpha" | "digit" | "alnum" | "upper" | "lower" | "space" | "blank" | "punct" | "print" | "graph" | "cntrl" | "xdigit") {
                        return Err("Invalid character class name".into());
                    }
                    out.push_str("[:");
                    out.push_str(n);
                    out.push_str(":]");
                }
                _ => {
                    for &b in name {
                        push_class_literal(out, b);
                    }
                }
            }
            i = j + 2;
            continue;
        }
        if c == b'-' && p.get(i + 1) != Some(&b']') && out.len() > 1 && !out.ends_with('[') && !out.ends_with('^') {
            out.push('-');
            i += 1;
            continue;
        }
        push_class_literal(out, c);
        i += 1;
    }
}

fn push_class_literal(out: &mut String, b: u8) {
    match b {
        b'\\' | b'[' | b']' | b'^' | b'-' | b'&' | b'~' => {
            out.push('\\');
            out.push(b as char);
        }
        _ => push_byte(out, b),
    }
}

fn push_byte(out: &mut String, b: u8) {
    if b.is_ascii() {
        out.push(b as char);
    } else {
        out.push_str(&format!("\\x{b:02X}"));
    }
}

fn push_literal(out: &mut String, b: u8) {
    if b".^$*+?()[]{}|\\#&~-".contains(&b) || b == b' ' {
        out.push('\\');
        out.push(b as char);
    } else {
        push_byte(out, b);
    }
}

/// Junta bytes UTF-8 válidos de volta em caracteres (o tradutor emite `\xNN` só pra bytes soltos).
fn translate(p: &[u8], flavor: Flavor) -> Result<String, String> {
    let ere = flavor == Flavor::Extended;
    let mut out = String::new();
    let mut i = 0;
    // `*` no começo de expressão (ou depois de `(`, `|`, `^`) é literal.
    let mut at_start = true;
    let mut depth = 0usize;
    while i < p.len() {
        let c = p[i];
        // Sequências UTF-8 inteiras passam como texto.
        if c >= 0x80 {
            let len = match c {
                0xC0..=0xDF => 2,
                0xE0..=0xEF => 3,
                0xF0..=0xF7 => 4,
                _ => 1,
            };
            if let Some(chunk) = p.get(i..i + len)
                && let Ok(s) = std::str::from_utf8(chunk)
            {
                out.push_str(&regex::escape(s));
                i += len;
                at_start = false;
                continue;
            }
            push_byte(&mut out, c);
            i += 1;
            at_start = false;
            continue;
        }
        match c {
            b'\\' => {
                let Some(&n) = p.get(i + 1) else { return Err("Trailing backslash".into()) };
                i += 2;
                match n {
                    b'(' if !ere => {
                        out.push('(');
                        depth += 1;
                        at_start = true;
                        continue;
                    }
                    b')' if !ere => {
                        if depth == 0 {
                            return Err("Unmatched ) or \\)".into());
                        }
                        depth -= 1;
                        out.push(')');
                    }
                    b'{' if !ere => out.push('{'),
                    b'}' if !ere => out.push('}'),
                    b'|' if !ere => {
                        out.push('|');
                        at_start = true;
                        continue;
                    }
                    b'+' | b'?' if !ere => out.push(n as char),
                    b'<' => out.push_str(r"\b{start}"),
                    b'>' => out.push_str(r"\b{end}"),
                    b'`' => out.push_str(r"\A"),
                    b'\'' => out.push_str(r"\z"),
                    b'w' | b'W' | b's' | b'S' | b'b' | b'B' => {
                        out.push('\\');
                        out.push(n as char);
                    }
                    b'1'..=b'9' => return Err("back-references are not supported".into()),
                    _ => push_literal(&mut out, n),
                }
                at_start = false;
            }
            b'[' => {
                i = bracket(p, i + 1, &mut out)?;
                at_start = false;
            }
            b'.' => {
                out.push('.');
                i += 1;
                at_start = false;
            }
            b'*' => {
                if at_start {
                    push_literal(&mut out, c);
                } else {
                    out.push('*');
                }
                i += 1;
                at_start = false;
            }
            b'^' => {
                if ere || at_start {
                    out.push('^');
                } else {
                    push_literal(&mut out, c);
                }
                i += 1;
                // Depois de `^` no começo, `*` continua literal em BRE.
            }
            b'$' => {
                let at_end = i + 1 == p.len() || (!ere && (p[i + 1..].starts_with(b"\\)") || p[i + 1..].starts_with(b"\\|")));
                if ere || at_end {
                    out.push('$');
                } else {
                    push_literal(&mut out, c);
                }
                i += 1;
                at_start = false;
            }
            b'(' | b')' | b'|' | b'+' | b'?' | b'{' | b'}' if ere => {
                match c {
                    b'(' => {
                        depth += 1;
                        out.push('(');
                        i += 1;
                        at_start = true;
                        continue;
                    }
                    b')' => {
                        if depth == 0 {
                            push_literal(&mut out, c);
                        } else {
                            depth -= 1;
                            out.push(')');
                        }
                    }
                    b'|' => {
                        out.push('|');
                        i += 1;
                        at_start = true;
                        continue;
                    }
                    b'{' => {
                        // Intervalo válido? Senão é literal (como o GNU).
                        let rest = &p[i + 1..];
                        let close = rest.iter().position(|x| *x == b'}');
                        let valid = !at_start
                            && close.is_some_and(|k| {
                                let body = &rest[..k];
                                !body.is_empty() && body.iter().all(|x| x.is_ascii_digit() || *x == b',') && body.iter().filter(|x| **x == b',').count() <= 1 && body[0] != b','
                            });
                        if valid {
                            out.push('{');
                        } else {
                            push_literal(&mut out, c);
                        }
                    }
                    b'}' => {
                        if out.contains('{') {
                            out.push('}');
                        } else {
                            push_literal(&mut out, c);
                        }
                    }
                    _ => {
                        if at_start {
                            push_literal(&mut out, c);
                        } else {
                            out.push(c as char);
                        }
                    }
                }
                i += 1;
                at_start = false;
            }
            _ => {
                push_literal(&mut out, c);
                i += 1;
                at_start = false;
            }
        }
    }
    if depth > 0 {
        return Err("Unmatched ( or \\(".into());
    }
    Ok(out)
}

/// Compila. O erro vem no texto curto do motor.
pub fn compile(pattern: &[u8], flavor: Flavor, icase: bool) -> Result<Regex, String> {
    let src = match flavor {
        Flavor::Fixed => {
            let mut s = String::new();
            for chunk in pattern.utf8_chunks() {
                s.push_str(&regex::escape(chunk.valid()));
                for b in chunk.invalid() {
                    push_byte(&mut s, *b);
                }
            }
            s
        }
        Flavor::Perl => String::from_utf8_lossy(pattern).into_owned(),
        _ => translate(pattern, flavor)?,
    };
    RegexBuilder::new(&src).case_insensitive(icase).build().map_err(|e| {
        let msg = e.to_string();
        msg.lines().last().unwrap_or("Invalid regular expression").trim().to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(p: &str, f: Flavor, s: &str) -> bool {
        compile(p.as_bytes(), f, false).ok().unwrap().is_match(s.as_bytes())
    }

    #[test]
    fn bre_and_ere() {
        assert!(m("a\\(b\\)*c", Flavor::Basic, "abbc"));
        assert!(m("a(b)c", Flavor::Basic, "a(b)c"));
        assert!(m("foo\\|bar", Flavor::Basic, "xbar"));
        assert!(m("a+b", Flavor::Basic, "a+b"));
        assert!(!m("a+b", Flavor::Basic, "aab"));
        assert!(m("a+b", Flavor::Extended, "aab"));
        assert!(m("*x", Flavor::Basic, "*x"));
        assert!(m("[]a]", Flavor::Extended, "]"));
        assert!(m("[[:digit:]]\\{2\\}", Flavor::Basic, "a12"));
        assert!(m("\\<fn\\>", Flavor::Basic, "pub fn x"));
        assert!(!m("\\<fn\\>", Flavor::Basic, "pubfnx"));
        assert!(m("a.c", Flavor::Fixed, "xa.c"));
        assert!(!m("a.c", Flavor::Fixed, "abc"));
        assert!(m("^TODO", Flavor::Extended, "TODO: x"));
        assert!(m("ção", Flavor::Basic, "função"));
        assert!(m("a{x", Flavor::Extended, "a{x"));
    }
}
