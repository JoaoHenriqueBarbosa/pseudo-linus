//! Módulo `binascii` do CPython 3.13: conversões entre binário e texto ASCII (hex, base64, uuencode,
//! quoted-printable, CRC-32 e CRC-CCITT). As funções `encode_base64`,
//! `decode_base64` e `want_bytes` são públicas para o módulo `base64` reaproveitar.

use std::rc::Rc;

use crate::modules::ModuleBuilder;
use crate::native_util::bind;
use crate::object::{Kw, ModuleObj, Value};
use crate::vm::{exc, type_error, PyException, PyResult, Vm};

/// O núcleo base64 e CRC-32 mora no `ul-common`; os outros módulos continuam importando daqui.
pub use ul_common::codec::{base64_decode as decode_base64, base64_encode as encode_base64, crc32_update};
pub use ul_common::codec::BASE64_STANDARD as B64_ALPHABET;

pub fn binascii_error(msg: impl Into<String>) -> PyException {
    exc("binascii.Error", msg)
}

/// Bytes do argumento ou `TypeError: a bytes-like object is required, not 'str'`.
pub fn want_bytes(v: &Value) -> PyResult<Vec<u8>> {
    match v {
        Value::Bytes(_) | Value::ByteArray(_) | Value::Instance(_) if v.bytes_like().is_some() => {
            Ok(v.bytes_like().map(|b| b.to_vec()).unwrap_or_default())
        }
        other => Err(type_error(format!("a bytes-like object is required, not '{}'", other.type_name()))),
    }
}

/// `bytes` ou `str` ASCII (as funções `a2b_*` aceitam os dois).
pub fn want_ascii_or_bytes(v: &Value) -> PyResult<Vec<u8>> {
    match v {
        Value::Str(s) => {
            if s.as_str().is_ascii() {
                Ok(s.as_str().as_bytes().to_vec())
            } else {
                Err(exc("ValueError", "string argument should contain only ASCII characters"))
            }
        }
        other => want_bytes(other),
    }
}

fn hex_digit(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Hexadecimal minúsculo; `sep` e `bytes_per_sep` como em `bytes.hex(sep, bytes_per_sep)`.
fn to_hex(data: &[u8], sep: Option<u8>, bytes_per_sep: i64) -> Vec<u8> {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let n = bytes_per_sep.unsigned_abs() as usize;
    let mut out = Vec::with_capacity(data.len() * 2);
    for (i, b) in data.iter().enumerate() {
        if let Some(s) = sep {
            if i > 0 && n > 0 {
                let boundary = if bytes_per_sep > 0 { (data.len() - i) % n == 0 } else { i % n == 0 };
                if boundary {
                    out.push(s);
                }
            }
        }
        out.push(DIGITS[(b >> 4) as usize]);
        out.push(DIGITS[(b & 15) as usize]);
    }
    out
}

/// Decodifica hexadecimal; o erro é a mensagem do `binascii.Error`.
pub fn from_hex(data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() % 2 != 0 {
        return Err("Odd-length string".to_string());
    }
    let mut out = Vec::with_capacity(data.len() / 2);
    for pair in data.chunks(2) {
        match (hex_digit(pair[0]), hex_digit(pair[1])) {
            (Some(h), Some(l)) => out.push((h << 4) | l),
            _ => return Err("Non-hexadecimal digit found".to_string()),
        }
    }
    Ok(out)
}

fn hexlify(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("hexlify", args, kw, &["data", "sep", "bytes_per_sep"], 1)?;
    let data = want_bytes(s[0].as_ref().unwrap())?;
    let sep = match &s[1] {
        None => None,
        Some(v) => {
            let b = want_ascii_or_bytes(v)?;
            if b.len() != 1 {
                return Err(exc("ValueError", "sep must be length 1."));
            }
            Some(b[0])
        }
    };
    let bps = match &s[2] {
        None => 1,
        Some(v) => crate::native_util::want_int(v)?,
    };
    Ok(Value::bytes(to_hex(&data, sep, bps)))
}

fn unhexlify(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("unhexlify", args, kw, &["hexstr"], 1)?;
    let data = want_ascii_or_bytes(s[0].as_ref().unwrap())?;
    from_hex(&data).map(Value::bytes).map_err(binascii_error)
}

fn crc32(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("crc32", args, kw, &["data", "crc"], 1)?;
    let data = want_bytes(s[0].as_ref().unwrap())?;
    let init = match &s[1] {
        None => 0u32,
        Some(v) => crate::native_util::want_int(v)? as u32,
    };
    Ok(Value::Int(i64::from(crc32_update(init, &data))))
}

fn b2a_base64(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("b2a_base64", args, kw, &["data", "newline"], 1)?;
    let data = want_bytes(s[0].as_ref().unwrap())?;
    let newline = s[1].as_ref().map(|v| v.is_true()).unwrap_or(true);
    let mut out = encode_base64(&data, B64_ALPHABET, true);
    if newline {
        out.push(b'\n');
    }
    Ok(Value::bytes(out))
}

fn a2b_base64(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("a2b_base64", args, kw, &["data", "strict_mode"], 1)?;
    let data = want_ascii_or_bytes(s[0].as_ref().unwrap())?;
    let strict = s[1].as_ref().map(|v| v.is_true()).unwrap_or(false);
    decode_base64(&data, strict).map(Value::bytes).map_err(binascii_error)
}

/// `crc_hqx(data, crc, /)`: o CRC-CCITT (polinômio 0x1021) de `binhex`, sem reflexão.
fn crc_hqx(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("crc_hqx", args, kw, &["data", "crc"], 2)?;
    let data = want_bytes(s[0].as_ref().unwrap())?;
    // Só os 16 bits baixos do valor inicial entram na conta.
    let mut crc = (crate::native_util::want_int(s[1].as_ref().unwrap())? & 0xffff) as u32;
    for byte in data {
        crc ^= u32::from(byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x1021 } else { crc << 1 };
        }
        crc &= 0xffff;
    }
    Ok(Value::Int(i64::from(crc)))
}

/// `b2a_uu(data, /, *, backtick=False)`: uma linha de uuencode, de até 45 bytes.
fn b2a_uu(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("b2a_uu", args, kw, &["data", "backtick"], 1)?;
    let data = want_bytes(s[0].as_ref().unwrap())?;
    let backtick = s[1].as_ref().is_some_and(Value::is_true);
    if data.len() > 45 {
        return Err(binascii_error("At most 45 bytes at once"));
    }
    let glyph = |bits: u8| if backtick && bits == 0 { b'`' } else { bits + b' ' };
    let mut out = vec![glyph(data.len() as u8)];
    for chunk in data.chunks(3) {
        // O CPython completa o último grupo com bytes nulos: cada trio vira sempre quatro caracteres.
        let mut group = [0u8; 3];
        group[..chunk.len()].copy_from_slice(chunk);
        let word = u32::from(group[0]) << 16 | u32::from(group[1]) << 8 | u32::from(group[2]);
        out.extend((0..4).map(|i| glyph(((word >> (18 - 6 * i)) & 0x3f) as u8)));
    }
    out.push(b'\n');
    Ok(Value::bytes(out))
}

/// `a2b_uu(data, /)`: uma linha de uuencode; o que faltar no fim vira bytes nulos, como no CPython.
fn a2b_uu(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("a2b_uu", args, kw, &["data"], 1)?;
    let data = want_ascii_or_bytes(s[0].as_ref().unwrap())?;
    let mut bin_len = (i32::from(data.first().copied().unwrap_or(0)) - 32) & 0o77;
    let mut ascii_len = data.len() as i64 - 1;
    let mut pos = 1;
    let (mut leftbits, mut leftchar) = (0u32, 0u32);
    let mut out = Vec::new();
    while bin_len > 0 {
        let this = if ascii_len > 0 { data[pos] } else { 0 };
        let sextet = if this == b'\n' || this == b'\r' || ascii_len <= 0 {
            0
        } else if this < b' ' || this > b' ' + 64 {
            return Err(binascii_error("Illegal char"));
        } else {
            u32::from(this - b' ') & 0o77
        };
        leftchar = (leftchar << 6) | sextet;
        leftbits += 6;
        if leftbits >= 8 {
            leftbits -= 8;
            out.push(((leftchar >> leftbits) & 0xff) as u8);
            leftchar &= (1 << leftbits) - 1;
            bin_len -= 1;
        }
        ascii_len -= 1;
        pos += 1;
    }
    while ascii_len > 0 {
        if !matches!(data[pos], b' ' | b'`' | b'\n' | b'\r') {
            return Err(binascii_error("Trailing garbage"));
        }
        ascii_len -= 1;
        pos += 1;
    }
    Ok(Value::bytes(out))
}

/// `a2b_qp(data, /, header=False)`: quoted-printable para bytes.
fn a2b_qp(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("a2b_qp", args, kw, &["data", "header"], 1)?;
    let data = want_ascii_or_bytes(s[0].as_ref().unwrap())?;
    let header = s[1].as_ref().is_some_and(Value::is_true);
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        match data[i] {
            b'=' => {
                i += 1;
                if i >= data.len() {
                    break;
                }
                if data[i] == b'\n' || data[i] == b'\r' {
                    // Quebra de linha suave: some até o fim da linha.
                    if data[i] != b'\n' {
                        while i < data.len() && data[i] != b'\n' {
                            i += 1;
                        }
                    }
                    if i < data.len() {
                        i += 1;
                    }
                } else if data[i] == b'=' {
                    out.push(b'=');
                    i += 1;
                } else if let (Some(high), Some(low)) = (hex_digit(data[i]), data.get(i + 1).copied().and_then(hex_digit)) {
                    out.push((high << 4) | low);
                    i += 2;
                } else {
                    out.push(b'=');
                }
            }
            b'_' if header => {
                out.push(b' ');
                i += 1;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    Ok(Value::bytes(out))
}

/// `b2a_qp(data, /, quotetabs=False, istext=True, header=False)`: bytes para quoted-printable, em linhas de até 76
/// caracteres; o fim de linha da saída segue o da primeira linha da entrada (`\r\n` ou `\n`).
fn b2a_qp(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    const MAX_LINE_SIZE: usize = 76;
    let s = bind("b2a_qp", args, kw, &["data", "quotetabs", "istext", "header"], 1)?;
    let data = want_bytes(s[0].as_ref().unwrap())?;
    let quotetabs = s[1].as_ref().is_some_and(Value::is_true);
    let istext = s[2].as_ref().map_or(true, Value::is_true);
    let header = s[3].as_ref().is_some_and(Value::is_true);
    let crlf = data.iter().position(|&b| b == b'\n').is_some_and(|p| p > 0 && data[p - 1] == b'\r');
    let push_hex = |out: &mut Vec<u8>, byte: u8| {
        const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
        out.extend([b'=', DIGITS[usize::from(byte >> 4)], DIGITS[usize::from(byte & 15)]]);
    };
    let soft_break = |out: &mut Vec<u8>| {
        out.push(b'=');
        if crlf {
            out.push(b'\r');
        }
        out.push(b'\n');
    };
    let mut out = Vec::with_capacity(data.len() * 2);
    let (mut i, mut linelen) = (0, 0);
    while i < data.len() {
        let byte = data[i];
        let next = data.get(i + 1).copied();
        let last = i + 1 == data.len();
        let must_quote = byte > 126
            || byte == b'='
            || (header && byte == b'_')
            || (byte == b'.' && linelen == 0 && (last || matches!(next, Some(b'\n' | b'\r' | 0))))
            || (!istext && (byte == b'\r' || byte == b'\n'))
            || ((byte == b'\t' || byte == b' ') && last)
            || (byte < 33 && byte != b'\r' && byte != b'\n' && (quotetabs || (byte != b'\t' && byte != b' ')));
        if must_quote {
            if linelen + 3 >= MAX_LINE_SIZE {
                soft_break(&mut out);
                linelen = 0;
            }
            push_hex(&mut out, byte);
            i += 1;
            linelen += 3;
        } else if istext && (byte == b'\n' || (byte == b'\r' && next == Some(b'\n'))) {
            linelen = 0;
            // Espaço ou tab antes do fim de linha: o último byte já escrito vira escape.
            if let Some(&tail) = out.last().filter(|&&t| t == b' ' || t == b'\t') {
                out.pop();
                push_hex(&mut out, tail);
            }
            if crlf {
                out.push(b'\r');
            }
            out.push(b'\n');
            i += if byte == b'\r' { 2 } else { 1 };
        } else {
            if !last && next != Some(b'\n') && linelen + 1 >= MAX_LINE_SIZE {
                soft_break(&mut out);
                linelen = 0;
            }
            linelen += 1;
            out.push(if header && byte == b' ' { b'_' } else { byte });
            i += 1;
        }
    }
    Ok(Value::bytes(out))
}

pub fn build(_vm: &mut Vm) -> Rc<ModuleObj> {
    ModuleBuilder::new("binascii")
        .func("hexlify", hexlify)
        .func("b2a_hex", hexlify)
        .func("unhexlify", unhexlify)
        .func("a2b_hex", unhexlify)
        .func("crc32", crc32)
        .func("crc_hqx", crc_hqx)
        .func("b2a_base64", b2a_base64)
        .func("a2b_base64", a2b_base64)
        .func("b2a_uu", b2a_uu)
        .func("a2b_uu", a2b_uu)
        .func("b2a_qp", b2a_qp)
        .func("a2b_qp", a2b_qp)
        .value("Error", Value::Builtin("binascii.Error"))
        .value("Incomplete", Value::Builtin("binascii.Incomplete"))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{repr, NativeFnPtr};

    fn call(f: NativeFnPtr, args: Vec<Value>) -> PyResult<Value> {
        let mut vm = Vm::new();
        f(&mut vm, args, Vec::new())
    }

    fn b(s: &[u8]) -> Value {
        Value::bytes(s.to_vec())
    }

    #[test]
    fn hex_roundtrip() {
        assert_eq!(repr(&call(hexlify, vec![b(b"\x00\xffAb")]).unwrap()), "b'00ff4162'");
        assert_eq!(repr(&call(unhexlify, vec![b(b"00ff4162")]).unwrap()), "b'\\x00\\xffAb'");
        assert_eq!(repr(&call(unhexlify, vec![Value::str("4142")]).unwrap()), "b'AB'");
        let e = call(unhexlify, vec![b(b"abc")]).unwrap_err();
        assert_eq!((e.kind, e.msg.as_str()), ("binascii.Error", "Odd-length string"));
        let e = call(unhexlify, vec![b(b"zz")]).unwrap_err();
        assert_eq!(e.msg, "Non-hexadecimal digit found");
    }

    #[test]
    fn hex_with_separator() {
        let r = call(hexlify, vec![b(b"\x01\x02\x03"), Value::str("-")]).unwrap();
        assert_eq!(repr(&r), "b'01-02-03'");
    }

    #[test]
    fn crc32_known_values() {
        assert_eq!(repr(&call(crc32, vec![b(b"123456789")]).unwrap()), "3421780262");
        assert_eq!(repr(&call(crc32, vec![b(b"")]).unwrap()), "0");
    }

    #[test]
    fn base64_functions() {
        assert_eq!(repr(&call(b2a_base64, vec![b(b"hello")]).unwrap()), "b'aGVsbG8=\\n'");
        assert_eq!(repr(&call(a2b_base64, vec![b(b"aGVsbG8=\n")]).unwrap()), "b'hello'");
        let e = call(a2b_base64, vec![b(b"aGVsbG8")]).unwrap_err();
        assert_eq!((e.kind, e.msg.as_str()), ("binascii.Error", "Incorrect padding"));
    }
}
