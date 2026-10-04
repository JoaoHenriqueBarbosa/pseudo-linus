//! PROVISÓRIO: citação e escapes mínimos pra desenvolvimento do núcleo, até chegar o módulo de sala
//! limpa (`src/quote.rs`). Não é entregue.

fn push_utf8(out: &mut Vec<u8>, cp: u32) {
    if let Some(c) = char::from_u32(cp) {
        let mut b = [0u8; 4];
        out.extend_from_slice(c.encode_utf8(&mut b).as_bytes());
    }
}

/// Decodifica escapes; `echo_mode` muda as regras de octal (`\0nnn`) e liga `\c`.
fn decode(s: &[u8], ansi: bool, utf8: bool) -> (Vec<u8>, bool) {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        if s[i] != b'\\' || i + 1 >= s.len() {
            out.push(s[i]);
            i += 1;
            continue;
        }
        let c = s[i + 1];
        i += 2;
        match c {
            b'a' => out.push(7),
            b'b' => out.push(8),
            b'e' | b'E' => out.push(27),
            b'f' => out.push(12),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'v' => out.push(11),
            b'\\' => out.push(b'\\'),
            b'\'' if ansi => out.push(b'\''),
            b'"' if ansi => out.push(b'"'),
            b'?' if ansi => out.push(b'?'),
            b'c' if !ansi => return (out, true),
            b'c' if ansi && i < s.len() => {
                out.push(s[i] & 0x1f);
                i += 1;
            }
            b'x' => {
                let mut v = 0u32;
                let mut n = 0;
                while n < 2 && i < s.len() && s[i].is_ascii_hexdigit() {
                    v = v * 16 + (s[i] as char).to_digit(16).unwrap_or(0);
                    i += 1;
                    n += 1;
                }
                if n == 0 {
                    out.extend_from_slice(b"\\x");
                } else {
                    out.push(v as u8);
                }
            }
            b'u' | b'U' => {
                let max = if c == b'u' { 4 } else { 8 };
                let mut v = 0u32;
                let mut n = 0;
                while n < max && i < s.len() && s[i].is_ascii_hexdigit() {
                    v = v * 16 + (s[i] as char).to_digit(16).unwrap_or(0);
                    i += 1;
                    n += 1;
                }
                if n == 0 {
                    out.push(b'\\');
                    out.push(c);
                } else if utf8 || v < 0x80 {
                    push_utf8(&mut out, v);
                } else {
                    out.extend_from_slice(format!("\\u{v:04X}").as_bytes());
                }
            }
            b'0'..=b'7' => {
                let mut v = (c - b'0') as u32;
                let max = if !ansi && c == b'0' { 3 } else { 2 };
                let mut n = 0;
                while n < max && i < s.len() && (b'0'..=b'7').contains(&s[i]) {
                    v = v * 8 + (s[i] - b'0') as u32;
                    i += 1;
                    n += 1;
                }
                out.push(v as u8);
            }
            other => {
                out.push(b'\\');
                out.push(other);
            }
        }
    }
    (out, false)
}

pub fn decode_ansi_c(body: &[u8], utf8: bool) -> Vec<u8> {
    decode(body, true, utf8).0
}

pub fn decode_echo(s: &[u8]) -> (Vec<u8>, bool) {
    decode(s, false, true)
}

pub fn decode_printf_b(s: &[u8]) -> (Vec<u8>, bool) {
    decode(s, false, true)
}

fn needs_ansi(s: &[u8]) -> bool {
    s.iter().any(|c| *c < 0x20 || *c == 0x7f)
}

fn ansi_quote(s: &[u8]) -> Vec<u8> {
    let mut out = b"$'".to_vec();
    for &c in s {
        match c {
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\t' => out.extend_from_slice(b"\\t"),
            b'\r' => out.extend_from_slice(b"\\r"),
            7 => out.extend_from_slice(b"\\a"),
            8 => out.extend_from_slice(b"\\b"),
            11 => out.extend_from_slice(b"\\v"),
            12 => out.extend_from_slice(b"\\f"),
            27 => out.extend_from_slice(b"\\E"),
            b'\'' => out.extend_from_slice(b"\\'"),
            b'\\' => out.extend_from_slice(b"\\\\"),
            c if c < 0x20 || c == 0x7f => out.extend_from_slice(format!("\\{c:03o}").as_bytes()),
            c => out.push(c),
        }
    }
    out.push(b'\'');
    out
}

pub fn printf_q(s: &[u8], _utf8: bool) -> Vec<u8> {
    if s.is_empty() {
        return b"''".to_vec();
    }
    if needs_ansi(s) {
        return ansi_quote(s);
    }
    let mut out = Vec::new();
    for (i, &c) in s.iter().enumerate() {
        let special = matches!(c, b' ' | b'\t' | b'!' | b'"' | b'#' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b',' | b';' | b'<' | b'=' | b'>' | b'?' | b'[' | b'\\' | b']' | b'^' | b'`' | b'{' | b'|' | b'}')
            || (c == b'~' && i == 0);
        if special {
            out.push(b'\\');
        }
        out.push(c);
    }
    out
}

pub fn single_quote(s: &[u8]) -> Vec<u8> {
    let mut out = b"'".to_vec();
    for &c in s {
        if c == b'\'' {
            out.extend_from_slice(b"'\\''");
        } else {
            out.push(c);
        }
    }
    out.push(b'\'');
    out
}

pub fn double_quote_value(s: &[u8]) -> Vec<u8> {
    if needs_ansi(s) {
        return ansi_quote(s);
    }
    let mut out = b"\"".to_vec();
    for &c in s {
        if matches!(c, b'"' | b'\\' | b'$' | b'`') {
            out.push(b'\\');
        }
        out.push(c);
    }
    out.push(b'"');
    out
}

fn plain(s: &[u8]) -> bool {
    !s.is_empty()
        && s.iter().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.' | b'/' | b':' | b'+' | b'@' | b'%' | b',' | b'=') || *c >= 0x80)
}

pub fn set_value(s: &[u8]) -> Vec<u8> {
    if needs_ansi(s) {
        return ansi_quote(s);
    }
    if plain(s) { s.to_vec() } else { single_quote(s) }
}

pub fn xtrace_word(s: &[u8]) -> Vec<u8> {
    if needs_ansi(s) {
        return ansi_quote(s);
    }
    if plain(s) && !s.contains(&b'=') { s.to_vec() } else if s.is_empty() { b"''".to_vec() } else if plain(s) { s.to_vec() } else { single_quote(s) }
}
