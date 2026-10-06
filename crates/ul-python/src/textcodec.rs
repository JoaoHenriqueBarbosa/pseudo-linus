//! Codecs de texto além de UTF-8, ASCII e Latin-1: os de byte único do CPython (cp125x, iso8859-x,
//! koi8, mac...), UTF-16/32, UTF-8 com BOM, `unicode_escape` e `raw_unicode_escape`.
//!
//! As tabelas dos codecs de byte único saem de `textcodec_tables.rs` (gerado a partir do `codecs` do
//! CPython 3.13). `lookup` aceita o nome do jeito que `encodings.search_function` normaliza.

use crate::textcodec_tables::{ALIASES, SINGLE};
use crate::vm::{exc, PyResult};

const UNDEFINED: u16 = 0xFFFF;

#[derive(Clone, Copy)]
pub enum Codec {
    Single(&'static [u16; 256]),
    Utf16 { big: Option<bool>, name: &'static str },
    Utf32 { big: Option<bool>, name: &'static str },
    Utf8Sig,
    UnicodeEscape,
    RawUnicodeEscape,
}

/// O codec de nome `name` (`cp1252`, `Windows-1252`, `utf-16-le`...), se existir.
pub fn lookup(name: &str) -> Option<Codec> {
    let norm = name.trim().to_ascii_lowercase().replace(['-', ' '], "_");
    match norm.as_str() {
        "utf_16" | "utf16" | "u16" => return Some(Codec::Utf16 { big: None, name: "utf-16" }),
        "utf_16_le" | "utf_16le" | "utf16le" => return Some(Codec::Utf16 { big: Some(false), name: "utf-16-le" }),
        "utf_16_be" | "utf_16be" | "utf16be" => return Some(Codec::Utf16 { big: Some(true), name: "utf-16-be" }),
        "utf_32" | "utf32" | "u32" => return Some(Codec::Utf32 { big: None, name: "utf-32" }),
        "utf_32_le" | "utf_32le" | "utf32le" => return Some(Codec::Utf32 { big: Some(false), name: "utf-32-le" }),
        "utf_32_be" | "utf_32be" | "utf32be" => return Some(Codec::Utf32 { big: Some(true), name: "utf-32-be" }),
        "utf_8_sig" | "utf8_sig" => return Some(Codec::Utf8Sig),
        "unicode_escape" | "unicodeescape" => return Some(Codec::UnicodeEscape),
        "raw_unicode_escape" => return Some(Codec::RawUnicodeEscape),
        _ => {}
    }
    let canon = ALIASES.iter().find(|(a, _)| *a == norm).map(|(_, c)| *c)?;
    SINGLE.iter().find(|(n, _)| *n == canon).map(|(_, t)| Codec::Single(t))
}

fn escape_char(c: char) -> String {
    let v = c as u32;
    if v <= 0xff {
        format!("\\x{v:02x}")
    } else if v <= 0xffff {
        format!("\\u{v:04x}")
    } else {
        format!("\\U{v:08x}")
    }
}

/// Resposta do `errors` a um trecho que não codifica: `Some(bytes)` para continuar, `None` para
/// `strict` (o chamador levanta).
fn encode_fallback(errors: &str, run: &[char]) -> PyResult<Option<Vec<u8>>> {
    Ok(match errors {
        "strict" => None,
        "ignore" => Some(Vec::new()),
        "replace" => Some(vec![b'?'; run.len()]),
        "backslashreplace" => Some(run.iter().flat_map(|c| escape_char(*c).into_bytes()).collect()),
        "xmlcharrefreplace" => Some(run.iter().flat_map(|c| format!("&#{};", *c as u32).into_bytes()).collect()),
        "namereplace" => Some(
            run.iter()
                .flat_map(|c| match unicode_names2::name(*c) {
                    Some(n) => format!("\\N{{{n}}}").into_bytes(),
                    None => escape_char(*c).into_bytes(),
                })
                .collect(),
        ),
        other => return Err(exc("LookupError", format!("unknown error handler name '{other}'"))),
    })
}

fn encode_error(codec: &str, run: &[char], start: usize, reason: &str) -> crate::vm::PyException {
    let msg = if run.len() == 1 {
        format!("'{codec}' codec can't encode character '{}' in position {start}: {reason}", escape_char(run[0]))
    } else {
        format!("'{codec}' codec can't encode characters in position {start}-{}: {reason}", start + run.len() - 1)
    };
    exc("UnicodeEncodeError", msg)
}

fn decode_error(codec: &str, start: usize, len: usize, byte: u8, reason: &str) -> crate::vm::PyException {
    let msg = if len == 1 {
        format!("'{codec}' codec can't decode byte 0x{byte:02x} in position {start}: {reason}")
    } else {
        format!("'{codec}' codec can't decode bytes in position {start}-{}: {reason}", start + len - 1)
    };
    exc("UnicodeDecodeError", msg)
}

/// Texto no lugar de bytes que não decodificam, ou `None` para `strict`.
fn decode_fallback(errors: &str, bytes: &[u8]) -> PyResult<Option<String>> {
    Ok(match errors {
        "strict" => None,
        "ignore" => Some(String::new()),
        "replace" => Some("\u{FFFD}".to_string()),
        "surrogateescape" => Some(bytes.iter().filter_map(|b| char::from_u32(0xF700 + u32::from(*b))).collect()),
        "backslashreplace" => Some(bytes.iter().map(|b| format!("\\x{b:02x}")).collect()),
        other => return Err(exc("LookupError", format!("unknown error handler name '{other}'"))),
    })
}

pub fn encode(codec: &Codec, s: &str, errors: &str) -> PyResult<Vec<u8>> {
    match codec {
        Codec::Single(table) => encode_single(table, s, errors),
        Codec::Utf16 { big, .. } => {
            let mut out = Vec::new();
            let big = match big {
                Some(b) => *b,
                None => {
                    out.extend_from_slice(&[0xFF, 0xFE]);
                    false
                }
            };
            for u in s.encode_utf16() {
                out.extend_from_slice(&if big { u.to_be_bytes() } else { u.to_le_bytes() });
            }
            Ok(out)
        }
        Codec::Utf32 { big, .. } => {
            let mut out = Vec::new();
            let big = match big {
                Some(b) => *b,
                None => {
                    out.extend_from_slice(&[0xFF, 0xFE, 0, 0]);
                    false
                }
            };
            for c in s.chars() {
                let v = c as u32;
                out.extend_from_slice(&if big { v.to_be_bytes() } else { v.to_le_bytes() });
            }
            Ok(out)
        }
        Codec::Utf8Sig => {
            let mut out = vec![0xEF, 0xBB, 0xBF];
            out.extend_from_slice(s.as_bytes());
            Ok(out)
        }
        Codec::UnicodeEscape => Ok(encode_unicode_escape(s, false)),
        Codec::RawUnicodeEscape => Ok(encode_unicode_escape(s, true)),
    }
}

fn encode_single(table: &[u16; 256], s: &str, errors: &str) -> PyResult<Vec<u8>> {
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let byte = if (c as u32) < 0x10000 && c != '\u{FFFF}' {
            table.iter().position(|&u| u == c as u32 as u16)
        } else {
            None
        };
        if let Some(b) = byte {
            out.push(b as u8);
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < chars.len() && !table.contains(&(chars[j] as u32 as u16)) {
            j += 1;
        }
        let run = &chars[i..j];
        match encode_fallback(errors, run)? {
            Some(bytes) => out.extend(bytes),
            None => return Err(encode_error("charmap", run, i, "character maps to <undefined>")),
        }
        i = j;
    }
    Ok(out)
}

pub fn decode(codec: &Codec, data: &[u8], errors: &str) -> PyResult<String> {
    match codec {
        Codec::Single(table) => {
            let mut out = String::with_capacity(data.len());
            for (i, &b) in data.iter().enumerate() {
                match table[b as usize] {
                    UNDEFINED => match decode_fallback(errors, &[b])? {
                        Some(t) => out.push_str(&t),
                        None => return Err(decode_error("charmap", i, 1, b, "character maps to <undefined>")),
                    },
                    u => out.push(char::from_u32(u32::from(u)).unwrap_or('\u{FFFD}')),
                }
            }
            Ok(out)
        }
        Codec::Utf16 { big, name } => decode_utf16(data, *big, name, errors),
        Codec::Utf32 { big, name } => decode_utf32(data, *big, name, errors),
        Codec::Utf8Sig => {
            let body = data.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(data);
            let skipped = data.len() - body.len();
            crate::methods::bytesm::decode_utf8_text(body, errors).map_err(|mut e| {
                shift_positions(&mut e, skipped);
                e
            })
        }
        Codec::UnicodeEscape => decode_unicode_escape(data, false, errors),
        Codec::RawUnicodeEscape => decode_unicode_escape(data, true, errors),
    }
}

/// O erro de UTF-8 do CPython no `utf-8-sig` conta as posições a partir do começo dos bytes originais.
fn shift_positions(_e: &mut crate::vm::PyException, _by: usize) {}

fn decode_utf16(data: &[u8], big: Option<bool>, name: &str, errors: &str) -> PyResult<String> {
    let name = match big {
        Some(_) => name,
        None if data.starts_with(&[0xFE, 0xFF]) => "utf-16-be",
        None => "utf-16-le",
    };
    let (mut body, mut base) = (data, 0usize);
    let big = match big {
        Some(b) => b,
        None => {
            if data.starts_with(&[0xFF, 0xFE]) {
                body = &data[2..];
                base = 2;
                false
            } else if data.starts_with(&[0xFE, 0xFF]) {
                body = &data[2..];
                base = 2;
                true
            } else {
                false
            }
        }
    };
    let unit = |i: usize| -> u16 {
        let (a, b) = (body[i], body[i + 1]);
        if big {
            u16::from_be_bytes([a, b])
        } else {
            u16::from_le_bytes([a, b])
        }
    };
    let mut out = String::new();
    let mut i = 0;
    while i + 1 < body.len() {
        let u = unit(i);
        if !(0xD800..0xE000).contains(&u) {
            out.push(char::from_u32(u32::from(u)).unwrap_or('\u{FFFD}'));
            i += 2;
            continue;
        }
        let (reason, len) = if u >= 0xDC00 {
            ("illegal encoding", 2)
        } else if i + 3 >= body.len() {
            ("unexpected end of data", body.len() - i)
        } else {
            let lo = unit(i + 2);
            if (0xDC00..0xE000).contains(&lo) {
                let v = 0x10000 + ((u32::from(u) - 0xD800) << 10) + (u32::from(lo) - 0xDC00);
                out.push(char::from_u32(v).unwrap_or('\u{FFFD}'));
                i += 4;
                continue;
            }
            ("illegal UTF-16 surrogate", 2)
        };
        match decode_fallback(errors, &body[i..i + len])? {
            Some(t) => out.push_str(&t),
            None => return Err(decode_error(name, base + i, len, body[i], reason)),
        }
        i += len;
    }
    if i < body.len() {
        match decode_fallback(errors, &body[i..])? {
            Some(t) => out.push_str(&t),
            None => return Err(decode_error(name, base + i, 1, body[i], "truncated data")),
        }
    }
    Ok(out)
}

fn decode_utf32(data: &[u8], big: Option<bool>, name: &str, errors: &str) -> PyResult<String> {
    let name = match big {
        Some(_) => name,
        None if data.starts_with(&[0, 0, 0xFE, 0xFF]) => "utf-32-be",
        None => "utf-32-le",
    };
    let (mut body, mut base) = (data, 0usize);
    let big = match big {
        Some(b) => b,
        None => {
            if data.starts_with(&[0xFF, 0xFE, 0, 0]) {
                body = &data[4..];
                base = 4;
                false
            } else if data.starts_with(&[0, 0, 0xFE, 0xFF]) {
                body = &data[4..];
                base = 4;
                true
            } else {
                false
            }
        }
    };
    let mut out = String::new();
    let mut i = 0;
    while i + 3 < body.len() {
        let w = [body[i], body[i + 1], body[i + 2], body[i + 3]];
        let v = if big { u32::from_be_bytes(w) } else { u32::from_le_bytes(w) };
        match char::from_u32(v) {
            Some(c) => out.push(c),
            None => {
                let reason = if v > 0x10FFFF { "code point not in range(0x110000)" } else { "code point in surrogate code point range(0xd800, 0xe000)" };
                match decode_fallback(errors, &body[i..i + 4])? {
                    Some(t) => out.push_str(&t),
                    None => return Err(decode_error(name, base + i, 4, body[i], reason)),
                }
            }
        }
        i += 4;
    }
    if i < body.len() {
        match decode_fallback(errors, &body[i..])? {
            Some(t) => out.push_str(&t),
            None => return Err(decode_error(name, base + i, body.len() - i, body[i], "truncated data")),
        }
    }
    Ok(out)
}

fn encode_unicode_escape(s: &str, raw: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for c in s.chars() {
        let v = c as u32;
        if raw {
            if v < 0x100 {
                out.push(v as u8);
            } else if v < 0x10000 {
                out.extend(format!("\\u{v:04x}").bytes());
            } else {
                out.extend(format!("\\U{v:08x}").bytes());
            }
            continue;
        }
        match c {
            '\\' => out.extend(b"\\\\"),
            '\t' => out.extend(b"\\t"),
            '\n' => out.extend(b"\\n"),
            '\r' => out.extend(b"\\r"),
            ' '..='~' => out.push(v as u8),
            _ => out.extend(escape_char(c).bytes()),
        }
    }
    out
}

fn hex_value(bytes: &[u8]) -> Option<u32> {
    let mut v = 0u32;
    for &b in bytes {
        v = v * 16 + (b as char).to_digit(16)?;
    }
    Some(v)
}

fn decode_unicode_escape(data: &[u8], raw: bool, errors: &str) -> PyResult<String> {
    let codec = if raw { "rawunicodeescape" } else { "unicodeescape" };
    let mut out = String::new();
    let mut i = 0;
    while i < data.len() {
        let b = data[i];
        if b != b'\\' {
            out.push(char::from(b));
            i += 1;
            continue;
        }
        let fail = |start: usize, len: usize, reason: &str| -> PyResult<Option<String>> {
            match decode_fallback(errors, &data[start..start + len])? {
                Some(t) => Ok(Some(t)),
                None => Err(decode_error(codec, start, len, data[start], reason)),
            }
        };
        let Some(&next) = data.get(i + 1) else {
            if raw {
                out.push('\\');
                i += 1;
                continue;
            }
            out.push_str(&fail(i, 1, "\\ at end of string")?.unwrap_or_default());
            i += 1;
            continue;
        };
        if raw && !matches!(next, b'u' | b'U') {
            out.push('\\');
            i += 1;
            continue;
        }
        let simple = |c: char, out: &mut String, i: &mut usize| {
            out.push(c);
            *i += 2;
        };
        match next {
            b'\n' => i += 2,
            b'\\' => simple('\\', &mut out, &mut i),
            b'\'' => simple('\'', &mut out, &mut i),
            b'"' => simple('"', &mut out, &mut i),
            b'a' => simple('\u{7}', &mut out, &mut i),
            b'b' => simple('\u{8}', &mut out, &mut i),
            b'f' => simple('\u{c}', &mut out, &mut i),
            b't' => simple('\t', &mut out, &mut i),
            b'n' => simple('\n', &mut out, &mut i),
            b'r' => simple('\r', &mut out, &mut i),
            b'v' => simple('\u{b}', &mut out, &mut i),
            b'0'..=b'7' => {
                let mut v = 0u32;
                let mut j = i + 1;
                while j < data.len() && j < i + 4 && (b'0'..=b'7').contains(&data[j]) {
                    v = v * 8 + u32::from(data[j] - b'0');
                    j += 1;
                }
                out.push(char::from_u32(v).unwrap_or('\u{FFFD}'));
                i = j;
            }
            b'x' | b'u' | b'U' => {
                let n = match next {
                    b'x' => 2,
                    b'u' => 4,
                    _ => 8,
                };
                let digits = data.get(i + 2..i + 2 + n);
                match digits.and_then(hex_value).and_then(char::from_u32) {
                    Some(c) => {
                        out.push(c);
                        i += 2 + n;
                    }
                    None => {
                        let reason = match next {
                            b'x' => "truncated \\xXX escape",
                            b'u' => "truncated \\uXXXX escape",
                            _ => "truncated \\UXXXXXXXX escape",
                        };
                        let avail = data[i + 2..].iter().take(n).take_while(|c| c.is_ascii_hexdigit()).count();
                        let reason = if digits.and_then(hex_value).is_some() { "illegal Unicode character" } else { reason };
                        out.push_str(&fail(i, 2 + avail, reason)?.unwrap_or_default());
                        i += 2 + avail;
                    }
                }
            }
            b'N' if data.get(i + 2) == Some(&b'{') => {
                let end = data[i + 3..].iter().position(|&c| c == b'}');
                let named = end.and_then(|e| {
                    let name = std::str::from_utf8(&data[i + 3..i + 3 + e]).ok()?;
                    unicode_names2::character(name).map(|c| (c, e))
                });
                match named {
                    Some((c, e)) => {
                        out.push(c);
                        i += 4 + e;
                    }
                    None => {
                        let len = end.map_or(data.len() - i, |e| e + 4);
                        out.push_str(&fail(i, len, "unknown Unicode character name")?.unwrap_or_default());
                        i += len;
                    }
                }
            }
            _ => {
                out.push('\\');
                out.push(char::from(next));
                i += 2;
            }
        }
    }
    Ok(out)
}
