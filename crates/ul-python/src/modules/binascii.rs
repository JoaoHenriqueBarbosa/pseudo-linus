//! Módulo `binascii` do CPython 3.13: conversões entre binário e texto ASCII (hex, base64, CRC-32).
//!
//! Ficam de fora: `b2a_uu`/`a2b_uu`, `b2a_qp`/`a2b_qp`, `crc_hqx`. As funções `encode_base64`,
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

pub fn build(_vm: &mut Vm) -> Rc<ModuleObj> {
    ModuleBuilder::new("binascii")
        .func("hexlify", hexlify)
        .func("b2a_hex", hexlify)
        .func("unhexlify", unhexlify)
        .func("a2b_hex", unhexlify)
        .func("crc32", crc32)
        .func("b2a_base64", b2a_base64)
        .func("a2b_base64", a2b_base64)
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
