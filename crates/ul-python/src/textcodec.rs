//! Codecs de texto além de UTF-8, ASCII e Latin-1: os de byte único do CPython (cp125x, iso8859-x,
//! koi8, mac...), UTF-16/32, UTF-8 com BOM, `unicode_escape` e `raw_unicode_escape`.
//!
//! As tabelas dos codecs de byte único saem de `textcodec_tables.rs` (gerado a partir do `codecs` do
//! CPython 3.13). `lookup` aceita o nome do jeito que `encodings.search_function` normaliza.

use crate::object::{code_points, push_cp, ExcObj, Value};
use std::rc::Rc;
use crate::textcodec_tables::{ALIASES, SINGLE};
use crate::vm::{exc, PyResult};

const UNDEFINED: u16 = 0xFFFF;

#[derive(Clone, Copy)]
pub enum Codec {
    Single(&'static [u16; 256]),
    Utf16 { big: Option<bool>, name: &'static str },
    Utf32 { big: Option<bool>, name: &'static str },
    Utf8Sig,
    Utf7,
    UnicodeEscape,
    RawUnicodeEscape,
    Punycode,
    Idna,
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
        "utf_7" | "utf7" | "u7" | "unicode_1_1_utf_7" => return Some(Codec::Utf7),
        "unicode_escape" | "unicodeescape" => return Some(Codec::UnicodeEscape),
        "raw_unicode_escape" => return Some(Codec::RawUnicodeEscape),
        "punycode" => return Some(Codec::Punycode),
        "idna" => return Some(Codec::Idna),
        _ => {}
    }
    let canon = ALIASES.iter().find(|(a, _)| *a == norm).map(|(_, c)| *c)?;
    SINGLE.iter().find(|(n, _)| *n == canon).map(|(_, t)| Codec::Single(t))
}

/// Como o CPython escreve um código-ponto nas mensagens e no `backslashreplace`.
pub fn escape_cp(v: u32) -> String {
    if v <= 0xff {
        format!("\\x{v:02x}")
    } else if v <= 0xffff {
        format!("\\u{v:04x}")
    } else {
        format!("\\U{v:08x}")
    }
}

/// O que um handler de `errors` devolve para um trecho que não codifica.
enum Replacement {
    /// Bytes que entram direto na saída (`surrogateescape`, `surrogatepass`).
    Raw(Vec<u8>),
    /// Texto ASCII que passa pelo próprio codec (em UTF-16/32 cada caractere vira uma unidade).
    Text(Vec<u8>),
}

/// Um codec que codifica por código-ponto e trata os trechos sem representação (`Runs::encode`).
/// `put` grava o código-ponto e devolve `true`, ou devolve `false` sem gravar nada se ele não
/// codifica; `pass` é o `surrogatepass` do codec (só UTF-8/16/32 o têm); `prefix_escape` é o
/// atalho do `surrogateescape` nos codecs UTF-8, ASCII e Latin-1, que grava o prefixo escapável
/// do trecho e só reclama do resto.
pub struct Runs<'a> {
    pub name: &'a str,
    pub reason: &'a str,
    pub put: &'a dyn Fn(u32, &mut Vec<u8>) -> bool,
    pub pass: Option<&'a dyn Fn(u32, &mut Vec<u8>)>,
    pub prefix_escape: bool,
    /// `true` quando o codec entrega ao handler o trecho inteiro que não codifica (UTF-8, ASCII,
    /// Latin-1, charmap); `false` no UTF-16/32 do CPython, que falha em um surrogate por vez.
    pub group: bool,
}

/// O `UnicodeEncodeError` / `UnicodeDecodeError` do CPython: `args` são `(encoding, object, start,
/// end, reason)`, de onde saem a mensagem e os atributos.
pub(crate) fn unicode_error(kind: &'static str, encoding: &str, object: Value, start: usize, end: usize, reason: &str) -> crate::vm::PyException {
    let args = vec![Value::str(encoding), object, Value::Int(start as i64), Value::Int(end as i64), Value::str(reason)];
    crate::vm::PyException::from_value(&Value::Exception(Rc::new(ExcObj::new(kind, args))))
}

impl Runs<'_> {
    fn error(&self, s: &str, start: usize, len: usize) -> crate::vm::PyException {
        unicode_error("UnicodeEncodeError", self.name, Value::str(s), start, start + len, self.reason)
    }

    /// Resposta do `errors` a um trecho que não codifica: `None` quando o handler recusa (o erro
    /// original sobe), `Err` para handler desconhecido.
    fn fallback(&self, errors: &str, run: &[u32]) -> PyResult<Option<Replacement>> {
        let text = |f: &dyn Fn(u32) -> String| Some(Replacement::Text(run.iter().flat_map(|&cp| f(cp).into_bytes()).collect()));
        Ok(match errors {
            "strict" => None,
            "ignore" => text(&|_| String::new()),
            "replace" => text(&|_| "?".to_string()),
            "backslashreplace" => text(&escape_cp),
            "xmlcharrefreplace" => text(&|cp| format!("&#{cp};")),
            "namereplace" => text(&|cp| match char::from_u32(cp).and_then(unicode_names2::name) {
                Some(n) => format!("\\N{{{n}}}"),
                None => escape_cp(cp),
            }),
            "surrogateescape" if run.iter().all(|cp| (0xDC80..=0xDCFF).contains(cp)) => {
                Some(Replacement::Raw(run.iter().map(|cp| (cp - 0xDC00) as u8).collect()))
            }
            "surrogateescape" => None,
            "surrogatepass" => self.pass.filter(|_| run.iter().all(|cp| (0xD800..=0xDFFF).contains(cp))).map(|pass| {
                let mut raw = Vec::new();
                run.iter().for_each(|&cp| pass(cp, &mut raw));
                Replacement::Raw(raw)
            }),
            other => return Err(exc("LookupError", format!("unknown error handler name '{other}'"))),
        })
    }

    /// Codifica `s` em `out`; as posições dos erros contam código-pontos.
    pub fn encode(&self, s: &str, errors: &str) -> PyResult<Vec<u8>> {
        let cps: Vec<u32> = code_points(s).collect();
        let mut out = Vec::with_capacity(cps.len());
        let mut i = 0;
        while i < cps.len() {
            if (self.put)(cps[i], &mut out) {
                i += 1;
                continue;
            }
            let mut scratch = Vec::new();
            let mut end = i + 1;
            while self.group && end < cps.len() && !(self.put)(cps[end], &mut scratch) {
                end += 1;
            }
            let mut start = i;
            if self.prefix_escape && errors == "surrogateescape" {
                while start < end && (0xDC80..=0xDCFF).contains(&cps[start]) {
                    out.push((cps[start] - 0xDC00) as u8);
                    start += 1;
                }
            }
            if start < end {
                let run = &cps[start..end];
                match self.fallback(errors, run)? {
                    Some(Replacement::Raw(bytes)) => out.extend(bytes),
                    Some(Replacement::Text(ascii)) => {
                        if !ascii.iter().all(|&b| (self.put)(u32::from(b), &mut out)) {
                            return Err(self.error(s, start, run.len()));
                        }
                    }
                    None => return Err(self.error(s, start, run.len())),
                }
            }
            i = end;
        }
        Ok(out)
    }
}

/// ASCII (`limit` 128) e Latin-1 (`limit` 256): o código-ponto vira o próprio byte.
pub fn encode_ucs1(name: &str, limit: u32, s: &str, errors: &str) -> PyResult<Vec<u8>> {
    let put = |cp: u32, out: &mut Vec<u8>| {
        let fits = cp < limit;
        if fits {
            out.push(cp as u8);
        }
        fits
    };
    let reason = format!("ordinal not in range({limit})");
    Runs { name, reason: &reason, put: &put, pass: None, prefix_escape: true, group: true }.encode(s, errors)
}

/// UTF-8: surrogates não codificam, só `surrogatepass` os grava (3 bytes).
pub fn encode_utf8(s: &str, errors: &str) -> PyResult<Vec<u8>> {
    // Texto que não tem o byte 0xF4 não guarda surrogate nem par de escape: os bytes já são UTF-8.
    if !s.as_bytes().contains(&0xF4) {
        return Ok(s.as_bytes().to_vec());
    }
    let put = |cp: u32, out: &mut Vec<u8>| {
        char::from_u32(cp).is_some_and(|c| {
            out.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
            true
        })
    };
    let pass = |cp: u32, out: &mut Vec<u8>| {
        out.extend([0xE0 | (cp >> 12) as u8, 0x80 | ((cp >> 6) & 0x3F) as u8, 0x80 | (cp & 0x3F) as u8]);
    };
    Runs { name: "utf-8", reason: "surrogates not allowed", put: &put, pass: Some(&pass), prefix_escape: true, group: true }
        .encode(s, errors)
}

/// UTF-16 e UTF-32 (`wide`): unidades de 2 ou 4 bytes na ordem `big`; a unidade de 2 bytes quebra o que passa de
/// U+FFFF em par de surrogates, e o `surrogatepass` grava o surrogate como unidade.
fn encode_utf16_or_32(s: &str, big: bool, name: &str, errors: &str, wide: bool) -> PyResult<Vec<u8>> {
    let push = |v: u32, out: &mut Vec<u8>| match (wide, big) {
        (true, true) => out.extend_from_slice(&v.to_be_bytes()),
        (true, false) => out.extend_from_slice(&v.to_le_bytes()),
        (false, true) => out.extend_from_slice(&(v as u16).to_be_bytes()),
        (false, false) => out.extend_from_slice(&(v as u16).to_le_bytes()),
    };
    let put = |cp: u32, out: &mut Vec<u8>| {
        let Some(c) = char::from_u32(cp) else { return false };
        if wide {
            push(cp, out);
        } else {
            c.encode_utf16(&mut [0; 2]).iter().for_each(|&u| push(u32::from(u), out));
        }
        true
    };
    let pass = |cp: u32, out: &mut Vec<u8>| push(cp, out);
    Runs { name, reason: "surrogates not allowed", put: &put, pass: Some(&pass), prefix_escape: false, group: false }.encode(s, errors)
}

/// Aplica o handler `errors` aos bytes `data[bad]` que não decodificam: acrescenta o texto a
/// `out` e devolve quantos bytes consumiu, ou levanta o `UnicodeDecodeError` do codec `codec`
/// (com `data` inteiro como `object`). O `surrogateescape` recusa bytes ASCII e consome no
/// máximo 4; os outros consomem o trecho todo.
pub(crate) fn decode_bad(
    out: &mut String,
    errors: &str,
    data: &[u8],
    bad: std::ops::Range<usize>,
    codec: &str,
    reason: &str,
) -> PyResult<usize> {
    let bytes = &data[bad.clone()];
    let consumed = match errors {
        "ignore" => bytes.len(),
        "replace" => {
            out.push('\u{FFFD}');
            bytes.len()
        }
        "surrogateescape" => {
            let escapable = bytes.iter().take(4).take_while(|b| **b >= 0x80).count();
            for &b in &bytes[..escapable] {
                push_cp(out, 0xDC00 + u32::from(b));
            }
            escapable
        }
        "backslashreplace" => {
            bytes.iter().for_each(|b| out.push_str(&format!("\\x{b:02x}")));
            bytes.len()
        }
        "strict" | "surrogatepass" => 0,
        other => return Err(exc("LookupError", format!("unknown error handler name '{other}'"))),
    };
    if consumed == 0 {
        return Err(unicode_error("UnicodeDecodeError", codec, Value::bytes(data), bad.start, bad.end, reason));
    }
    Ok(consumed)
}

/// O `surrogatepass` na decodificação: o valor `cp` lido dos bytes, se é um surrogate, entra no
/// texto como código-ponto (e `true` diz que foi aceito); fora da faixa o erro estrito segue.
fn push_surrogate(out: &mut String, cp: u32) -> bool {
    let is_surrogate = (0xD800..=0xDFFF).contains(&cp);
    if is_surrogate {
        push_cp(out, cp);
    }
    is_surrogate
}

pub fn encode(codec: &Codec, s: &str, errors: &str) -> PyResult<Vec<u8>> {
    match codec {
        Codec::Single(table) => encode_single(table, s, errors),
        Codec::Utf16 { big, name } => {
            let mut out = if big.is_none() { vec![0xFF, 0xFE] } else { Vec::new() };
            out.extend(encode_utf16_or_32(s, big.unwrap_or(false), name, errors, false)?);
            Ok(out)
        }
        Codec::Utf32 { big, name } => {
            let mut out = if big.is_none() { vec![0xFF, 0xFE, 0, 0] } else { Vec::new() };
            out.extend(encode_utf16_or_32(s, big.unwrap_or(false), name, errors, true)?);
            Ok(out)
        }
        Codec::Utf8Sig => {
            let mut out = vec![0xEF, 0xBB, 0xBF];
            out.extend(encode_utf8(s, errors)?);
            Ok(out)
        }
        Codec::Utf7 => Ok(encode_utf7(s)),
        Codec::UnicodeEscape => Ok(encode_unicode_escape(s, false)),
        Codec::RawUnicodeEscape => Ok(encode_unicode_escape(s, true)),
        Codec::Punycode => Ok(crate::idna::punycode_encode(&code_points(s).collect::<Vec<u32>>())),
        Codec::Idna => crate::idna::idna_encode(s, errors),
    }
}

fn encode_single(table: &[u16; 256], s: &str, errors: &str) -> PyResult<Vec<u8>> {
    let put = |cp: u32, out: &mut Vec<u8>| {
        let byte = if cp < 0x10000 && cp != u32::from(UNDEFINED) {
            table.iter().position(|&u| u32::from(u) == cp)
        } else {
            None
        };
        byte.is_some_and(|b| {
            out.push(b as u8);
            true
        })
    };
    Runs { name: "charmap", reason: "character maps to <undefined>", put: &put, pass: None, prefix_escape: false, group: true }
        .encode(s, errors)
}

pub fn decode(codec: &Codec, data: &[u8], errors: &str) -> PyResult<String> {
    match codec {
        Codec::Single(table) => {
            let mut out = String::with_capacity(data.len());
            for (i, &b) in data.iter().enumerate() {
                match table[b as usize] {
                    UNDEFINED => {
                        decode_bad(&mut out, errors, data, i..i + 1, "charmap", "character maps to <undefined>")?;
                    }
                    u => out.push(char::from_u32(u32::from(u)).unwrap_or('\u{FFFD}')),
                }
            }
            Ok(out)
        }
        Codec::Utf16 { big, name } => decode_utf16(data, *big, name, errors, true).map(|(text, _)| text),
        Codec::Utf32 { big, name } => decode_utf32(data, *big, name, errors, true).map(|(text, _)| text),
        Codec::Utf8Sig => {
            // Como o encodings/utf_8_sig.py: o BOM sai antes e o erro do `utf-8` vê só o resto
            // (object sem o BOM, posições relativas a ele).
            let body = data.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(data);
            crate::methods::bytesm::decode_utf8(body, errors)
        }
        Codec::Utf7 => decode_utf7(data, errors, true).map(|(text, _)| text),
        Codec::UnicodeEscape => decode_unicode_escape(data, false, errors, true).map(|(text, _)| text),
        Codec::RawUnicodeEscape => decode_unicode_escape(data, true, errors, true).map(|(text, _)| text),
        Codec::Punycode => crate::idna::punycode_decode(data, errors),
        Codec::Idna => crate::idna::idna_decode(data, errors),
    }
}

const UTF7_BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn utf7_is_base64(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'+' || b == b'/'
}

fn utf7_from_base64(b: u8) -> u32 {
    UTF7_BASE64.iter().position(|&c| c == b).unwrap_or(0) as u32
}

/// `ENCODE_DIRECT` do unicodeobject.c com o conjunto O e os espaços diretos (como o `PyUnicode_EncodeUTF7`).
fn utf7_encode_direct(cp: u32) -> bool {
    matches!(cp, 9 | 10 | 13 | 0x20..=0x7E) && !matches!(cp, 0x2B | 0x5C | 0x7E)
}

/// `PyUnicode_EncodeUTF7`: base64 modificado sobre UTF-16, surrogates soltos entram como unidade.
pub fn encode_utf7(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let (mut in_shift, mut bits, mut buffer) = (false, 0u32, 0u64);
    let emit = |unit: u32, bits: &mut u32, buffer: &mut u64, out: &mut Vec<u8>| {
        *bits += 16;
        *buffer = (*buffer << 16) | u64::from(unit);
        while *bits >= 6 {
            out.push(UTF7_BASE64[((*buffer >> (*bits - 6)) & 0x3F) as usize]);
            *bits -= 6;
        }
    };
    for cp in code_points(s) {
        let direct = utf7_encode_direct(cp);
        if in_shift && direct {
            if bits > 0 {
                out.push(UTF7_BASE64[((buffer << (6 - bits)) & 0x3F) as usize]);
                buffer = 0;
                bits = 0;
            }
            in_shift = false;
            if utf7_is_base64(cp as u8) || cp == u32::from(b'-') {
                out.push(b'-');
            }
            out.push(cp as u8);
            continue;
        }
        if !in_shift {
            if cp == u32::from(b'+') {
                out.extend_from_slice(b"+-");
                continue;
            }
            if direct {
                out.push(cp as u8);
                continue;
            }
            out.push(b'+');
            in_shift = true;
        }
        if cp >= 0x1_0000 {
            let v = cp - 0x1_0000;
            emit(0xD800 + (v >> 10), &mut bits, &mut buffer, &mut out);
            emit(0xDC00 + (v & 0x3FF), &mut bits, &mut buffer, &mut out);
        } else {
            emit(cp, &mut bits, &mut buffer, &mut out);
        }
    }
    if bits > 0 {
        out.push(UTF7_BASE64[((buffer << (6 - bits)) & 0x3F) as usize]);
    }
    if in_shift {
        out.push(b'-');
    }
    out
}

/// `PyUnicode_DecodeUTF7Stateful`: o texto e quantos bytes foram consumidos (com `final` falso, uma
/// sequência de shift aberta fica para a próxima chamada).
pub fn decode_utf7(data: &[u8], errors: &str, final_: bool) -> PyResult<(String, usize)> {
    let mut out = String::with_capacity(data.len());
    let (mut s, mut start, mut shift_out_start) = (0usize, 0usize, 0usize);
    let (mut in_shift, mut bits, mut buffer, mut surrogate) = (false, 0u32, 0u32, 0u32);
    while s < data.len() {
        let ch = data[s];
        let mut reason: Option<&str> = None;
        if in_shift {
            if utf7_is_base64(ch) {
                buffer = (buffer << 6) | utf7_from_base64(ch);
                bits += 6;
                s += 1;
                if bits >= 16 {
                    let unit = buffer >> (bits - 16);
                    bits -= 16;
                    buffer &= (1 << bits) - 1;
                    if surrogate != 0 {
                        if (0xDC00..0xE000).contains(&unit) {
                            push_cp(&mut out, 0x1_0000 + ((surrogate - 0xD800) << 10) + (unit - 0xDC00));
                            surrogate = 0;
                            continue;
                        }
                        push_cp(&mut out, surrogate);
                        surrogate = 0;
                    }
                    if (0xD800..0xDC00).contains(&unit) {
                        surrogate = unit;
                    } else {
                        push_cp(&mut out, unit);
                    }
                }
            } else {
                in_shift = false;
                if bits >= 6 {
                    s += 1;
                    reason = Some("partial character in shift sequence");
                } else if bits > 0 && buffer != 0 {
                    s += 1;
                    reason = Some("non-zero padding bits in shift sequence");
                } else {
                    if surrogate != 0 && ch <= 127 {
                        push_cp(&mut out, surrogate);
                    }
                    surrogate = 0;
                    if ch == b'-' {
                        s += 1;
                    }
                }
            }
        } else if ch == b'+' {
            start = s;
            s += 1;
            if data.get(s) == Some(&b'-') {
                s += 1;
                out.push('+');
            } else if data.get(s).is_some_and(|&b| !utf7_is_base64(b)) {
                s += 1;
                reason = Some("ill-formed sequence");
            } else {
                in_shift = true;
                surrogate = 0;
                shift_out_start = out.len();
                bits = 0;
                buffer = 0;
            }
        } else if ch <= 127 {
            s += 1;
            out.push(ch as char);
        } else {
            start = s;
            s += 1;
            reason = Some("unexpected special character");
        }
        if let Some(reason) = reason {
            s = start + decode_bad(&mut out, errors, data, start..s, "utf7", reason)?;
        }
    }
    if in_shift && final_ {
        in_shift = false;
        if surrogate != 0 || bits >= 6 || (bits > 0 && buffer != 0) {
            decode_bad(&mut out, errors, data, start..data.len(), "utf7", "unterminated shift sequence")?;
        }
    }
    if !in_shift {
        return Ok((out, s));
    }
    // Sem `final`: a sequência aberta volta inteira na próxima chamada, então o texto que ela já
    // produziu (ASCII ou não) é descartado aqui e sai quando ela for decodificada de novo.
    out.truncate(shift_out_start);
    Ok((out, start))
}

/// `PyUnicode_DecodeUTF16Stateful`: o texto e quantos bytes foram consumidos (com `final_` falso, a
/// unidade incompleta e o par de surrogates aberto no fim ficam para a próxima chamada).
pub(crate) fn decode_utf16(data: &[u8], big: Option<bool>, name: &str, errors: &str, final_: bool) -> PyResult<(String, usize)> {
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
            push_cp(&mut out, u32::from(u));
            i += 2;
            continue;
        }
        let (reason, len) = if u >= 0xDC00 {
            ("illegal encoding", 2)
        } else if i + 3 >= body.len() {
            if !final_ {
                return Ok((out, base + i));
            }
            ("unexpected end of data", body.len() - i)
        } else {
            let lo = unit(i + 2);
            if (0xDC00..0xE000).contains(&lo) {
                let v = 0x10000 + ((u32::from(u) - 0xD800) << 10) + (u32::from(lo) - 0xDC00);
                push_cp(&mut out, v);
                i += 4;
                continue;
            }
            ("illegal UTF-16 surrogate", 2)
        };
        if errors == "surrogatepass" && push_surrogate(&mut out, u32::from(u)) {
            i += 2;
            continue;
        }
        i += decode_bad(&mut out, errors, data, base + i..base + i + len, name, reason)?;
    }
    if !final_ {
        return Ok((out, base + i));
    }
    while i < body.len() {
        i += decode_bad(&mut out, errors, data, base + i..base + body.len(), name, "truncated data")?;
    }
    Ok((out, data.len()))
}

/// `PyUnicode_DecodeUTF32Stateful`: como `decode_utf16`, com a palavra incompleta do fim guardada.
pub(crate) fn decode_utf32(data: &[u8], big: Option<bool>, name: &str, errors: &str, final_: bool) -> PyResult<(String, usize)> {
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
        i += match char::from_u32(v) {
            Some(_) => {
                push_cp(&mut out, v);
                4
            }
            None if errors == "surrogatepass" && push_surrogate(&mut out, v) => 4,
            None => {
                let reason = if v > 0x10FFFF { "code point not in range(0x110000)" } else { "code point in surrogate code point range(0xd800, 0xe000)" };
                decode_bad(&mut out, errors, data, base + i..base + i + 4, name, reason)?
            }
        };
    }
    if !final_ {
        return Ok((out, base + i));
    }
    while i < body.len() {
        i += decode_bad(&mut out, errors, data, base + i..base + body.len(), name, "truncated data")?;
    }
    Ok((out, data.len()))
}

fn encode_unicode_escape(s: &str, raw: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for v in code_points(s) {
        if raw {
            if v < 0x100 {
                out.push(v as u8);
            } else {
                out.extend(escape_cp(v).bytes());
            }
            continue;
        }
        match v {
            0x5C => out.extend(b"\\\\"),
            0x09 => out.extend(b"\\t"),
            0x0A => out.extend(b"\\n"),
            0x0D => out.extend(b"\\r"),
            0x20..=0x7E => out.push(v as u8),
            _ => out.extend(escape_cp(v).bytes()),
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

/// `_PyUnicode_DecodeUnicodeEscapeStateful` e o `raw`: o texto e quantos bytes foram consumidos (com
/// `final_` falso, o escape que o fim dos dados interrompe fica para a próxima chamada).
pub(crate) fn decode_unicode_escape(data: &[u8], raw: bool, errors: &str, final_: bool) -> PyResult<(String, usize)> {
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
        let fail = |out: &mut String, start: usize, len: usize, reason: &str| -> PyResult<usize> {
            decode_bad(out, errors, data, start..start + len, codec, reason)
        };
        let Some(&next) = data.get(i + 1) else {
            if raw {
                out.push('\\');
                i += 1;
                continue;
            }
            if !final_ {
                return Ok((out, i));
            }
            i += fail(&mut out, i, 1, "\\ at end of string")?;
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
                let digits = data.get(i + 2..i + 2 + n).and_then(hex_value);
                match digits.filter(|v| *v <= 0x10FFFF) {
                    Some(v) => {
                        push_cp(&mut out, v);
                        i += 2 + n;
                    }
                    None => {
                        let reason = match next {
                            b'x' => "truncated \\xXX escape",
                            b'u' => "truncated \\uXXXX escape",
                            _ => "truncated \\UXXXXXXXX escape",
                        };
                        let avail = data[i + 2..].iter().take(n).take_while(|c| c.is_ascii_hexdigit()).count();
                        if !final_ && digits.is_none() && i + 2 + avail == data.len() {
                            return Ok((out, i));
                        }
                        let reason = if digits.is_some() { "illegal Unicode character" } else { reason };
                        i += fail(&mut out, i, 2 + avail, reason)?;
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
                        push_cp(&mut out, u32::from(c));
                        i += 4 + e;
                    }
                    None => {
                        if !final_ && end.is_none() {
                            return Ok((out, i));
                        }
                        let len = end.map_or(data.len() - i, |e| e + 4);
                        i += fail(&mut out, i, len, "unknown Unicode character name")?;
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
    Ok((out, data.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::cp_to_str;

    #[test]
    fn utf8_encode_distinguishes_surrogates_from_real_chars() {
        assert_eq!(encode_utf8(&cp_to_str(0x10_FFFF), "strict").unwrap(), [0xF4, 0x8F, 0xBF, 0xBF]);
        assert_eq!(encode_utf8(&cp_to_str(0x10_D800), "strict").unwrap(), [0xF4, 0x8D, 0xA0, 0x80]);
        let lone = format!("a{}", cp_to_str(0xDC80));
        assert_eq!(encode_utf8(&lone, "surrogateescape").unwrap(), [b'a', 0x80]);
        assert_eq!(encode_utf8(&lone, "surrogatepass").unwrap(), [b'a', 0xED, 0xB2, 0x80]);
        assert_eq!(encode_utf8(&lone, "replace").unwrap(), b"a?");
        assert_eq!(encode_utf8(&lone, "backslashreplace").unwrap(), b"a\\udc80");
        assert!(encode_utf8(&lone, "strict").is_err());
        assert!(encode_utf8(&lone, "nonexistent").is_err());
    }

    #[test]
    fn utf16_and_utf32_handle_supplementary_and_surrogates() {
        let codec = lookup("utf-16-le").unwrap();
        assert_eq!(encode(&codec, &cp_to_str(0x10_FFFF), "strict").unwrap(), [0xFF, 0xDB, 0xFF, 0xDF]);
        assert_eq!(decode(&codec, &[0xFF, 0xDB, 0xFF, 0xDF], "strict").unwrap(), cp_to_str(0x10_FFFF));
        let lone = cp_to_str(0xD800);
        assert_eq!(encode(&codec, &lone, "surrogatepass").unwrap(), [0x00, 0xD8]);
        assert_eq!(decode(&codec, &[0x00, 0xD8], "surrogatepass").unwrap(), lone);
        assert!(encode(&codec, &lone, "strict").is_err());
        let codec = lookup("utf-32-be").unwrap();
        assert_eq!(decode(&codec, &[0, 0x10, 0xD8, 0x00], "strict").unwrap(), cp_to_str(0x10_D800));
    }

    #[test]
    fn utf7_matches_cpython() {
        use crate::object::cp_to_str;
        assert_eq!(encode_utf7(&cp_to_str(0xD800)), b"+2AA-");
        assert_eq!(encode_utf7(&cp_to_str(0x1_0000)), b"+2ADcAA-");
        assert_eq!(encode_utf7("a+b~c\u{e9}d"), b"a+-b+AH4-c+AOk-d");
        assert_eq!(decode_utf7(b"+2ADcAA-", "strict", true).unwrap().0, cp_to_str(0x1_0000));
        assert_eq!(decode_utf7(b"+2AA-", "strict", true).unwrap().0, cp_to_str(0xD800));
        assert_eq!(decode_utf7(b"a+-b~", "strict", true).unwrap().0, "a+b~");
        for bad in [&b"+@"[..], b"+2AA", b"+AGEA-", b"+AGF-", b"\xff"] {
            assert!(decode_utf7(bad, "strict", true).is_err());
        }
        assert_eq!(decode_utf7(b"+@x", "replace", true).unwrap().0, "\u{fffd}x");
        assert_eq!(decode_utf7(b"+AGF-x", "ignore", true).unwrap().0, "ax");
        assert_eq!(decode_utf7(b"a+AOk", "strict", false).unwrap(), ("a".to_string(), 1));
        // A sequência aberta volta inteira, mesmo quando o que ela já produziu é ASCII.
        assert_eq!(decode_utf7(b"+AGE", "strict", false).unwrap(), (String::new(), 0));
        assert_eq!(decode_utf7(b"x+AGE", "strict", false).unwrap(), ("x".to_string(), 1));
    }

    #[test]
    fn ascii_and_latin1_handlers_count_code_points() {
        assert_eq!(encode_ucs1("ascii", 128, "a\u{e9}b", "replace").unwrap(), b"a?b");
        assert_eq!(encode_ucs1("ascii", 128, "a\u{e9}b", "xmlcharrefreplace").unwrap(), b"a&#233;b");
        let escaped = format!("x{}", cp_to_str(0xDCFF));
        assert_eq!(encode_ucs1("latin-1", 256, &escaped, "surrogateescape").unwrap(), [b'x', 0xFF]);
        assert!(encode_ucs1("latin-1", 256, "\u{20ac}", "strict").is_err());
    }
}
