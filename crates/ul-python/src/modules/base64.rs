//! Módulo `base64` do CPython 3.13: base64, base32 e base16 sobre `bytes`.
//!
//! Depende de `modules::binascii` (a codificação e a decodificação base64 moram lá). Ficam de fora:
//! `a85encode`/`b85encode` e família, `b32hexencode`/`b32hexdecode`, `encode`/`decode` de arquivos.

use std::rc::Rc;

use ul_common::codec::BASE64_URL;

use crate::modules::binascii::{
    binascii_error, decode_base64, encode_base64, from_hex, want_ascii_or_bytes, want_bytes, B64_ALPHABET,
};
use crate::modules::ModuleBuilder;
use crate::native_util::{bind, exactly, no_kwargs};
use crate::object::{Kw, ModuleObj, Value};
use crate::vm::{type_error, PyResult, Vm};

const B32_ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

fn alt_alphabet(altchars: &Value) -> PyResult<[u8; 64]> {
    let alt = want_ascii_or_bytes(altchars)?;
    if alt.len() != 2 {
        return Err(type_error(format!("expected length 2, got {}", alt.len())));
    }
    let mut a = *B64_ALPHABET;
    a[62] = alt[0];
    a[63] = alt[1];
    Ok(a)
}

fn b64encode(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("b64encode", args, kw, &["s", "altchars"], 1)?;
    let data = want_bytes(s[0].as_ref().unwrap())?;
    let alphabet = match &s[1] {
        None | Some(Value::None) => *B64_ALPHABET,
        Some(v) => alt_alphabet(v)?,
    };
    Ok(Value::bytes(encode_base64(&data, &alphabet, true)))
}

fn b64decode(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("b64decode", args, kw, &["s", "altchars", "validate"], 1)?;
    let mut data = want_ascii_or_bytes(s[0].as_ref().unwrap())?;
    if let Some(v) = &s[1] {
        if !matches!(v, Value::None) {
            let alt = want_ascii_or_bytes(v)?;
            if alt.len() != 2 {
                return Err(type_error(format!("expected length 2, got {}", alt.len())));
            }
            for c in data.iter_mut() {
                if *c == alt[0] {
                    *c = b'+';
                } else if *c == alt[1] {
                    *c = b'/';
                }
            }
        }
    }
    let strict = s[2].as_ref().map(|v| v.is_true()).unwrap_or(false);
    decode_base64(&data, strict).map(Value::bytes).map_err(binascii_error)
}

fn standard_b64encode(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("standard_b64encode", &kw)?;
    exactly("standard_b64encode", &args, 1)?;
    Ok(Value::bytes(encode_base64(&want_bytes(&args[0])?, B64_ALPHABET, true)))
}

fn standard_b64decode(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("standard_b64decode", &kw)?;
    exactly("standard_b64decode", &args, 1)?;
    decode_base64(&want_ascii_or_bytes(&args[0])?, false).map(Value::bytes).map_err(binascii_error)
}

fn urlsafe_b64encode(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("urlsafe_b64encode", &kw)?;
    exactly("urlsafe_b64encode", &args, 1)?;
    Ok(Value::bytes(encode_base64(&want_bytes(&args[0])?, BASE64_URL, true)))
}

fn urlsafe_b64decode(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("urlsafe_b64decode", &kw)?;
    exactly("urlsafe_b64decode", &args, 1)?;
    let mut data = want_ascii_or_bytes(&args[0])?;
    for c in data.iter_mut() {
        if *c == b'-' {
            *c = b'+';
        } else if *c == b'_' {
            *c = b'/';
        }
    }
    decode_base64(&data, false).map(Value::bytes).map_err(binascii_error)
}

fn encodebytes(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("encodebytes", &kw)?;
    exactly("encodebytes", &args, 1)?;
    let data = want_bytes(&args[0])?;
    let mut out = Vec::new();
    for chunk in data.chunks(57) {
        out.extend(encode_base64(chunk, B64_ALPHABET, true));
        out.push(b'\n');
    }
    Ok(Value::bytes(out))
}

fn decodebytes(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("decodebytes", &kw)?;
    exactly("decodebytes", &args, 1)?;
    decode_base64(&want_bytes(&args[0])?, false).map(Value::bytes).map_err(binascii_error)
}

fn b32encode_raw(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for chunk in data.chunks(5) {
        let mut buf = [0u8; 5];
        buf[..chunk.len()].copy_from_slice(chunk);
        let n = buf.iter().fold(0u64, |a, &b| (a << 8) | u64::from(b));
        let chars = match chunk.len() {
            1 => 2,
            2 => 4,
            3 => 5,
            4 => 7,
            _ => 8,
        };
        for i in 0..8 {
            if i < chars {
                out.push(B32_ALPHABET[((n >> (35 - 5 * i)) & 31) as usize]);
            } else {
                out.push(b'=');
            }
        }
    }
    out
}

fn b32encode(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("b32encode", &kw)?;
    exactly("b32encode", &args, 1)?;
    Ok(Value::bytes(b32encode_raw(&want_bytes(&args[0])?)))
}

fn b32decode(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("b32decode", args, kw, &["s", "casefold", "map01"], 1)?;
    let mut data = want_ascii_or_bytes(s[0].as_ref().unwrap())?;
    let casefold = s[1].as_ref().map(|v| v.is_true()).unwrap_or(false);
    if data.len() % 8 != 0 {
        return Err(binascii_error("Incorrect padding"));
    }
    if let Some(m) = &s[2] {
        if !matches!(m, Value::None) {
            let m = want_ascii_or_bytes(m)?;
            if m.len() != 1 {
                return Err(type_error(format!("expected length 1, got {}", m.len())));
            }
            for c in data.iter_mut() {
                if *c == b'0' {
                    *c = b'O';
                } else if *c == b'1' {
                    *c = m[0];
                }
            }
        }
    }
    if casefold {
        data.make_ascii_uppercase();
    }
    let total = data.len();
    while data.last() == Some(&b'=') {
        data.pop();
    }
    let padchars = total - data.len();
    if !matches!(padchars, 0 | 1 | 3 | 4 | 6) {
        return Err(binascii_error("Incorrect padding"));
    }
    let mut out = Vec::new();
    let mut acc: u64 = 0;
    let mut bits: u32 = 0;
    for &c in &data {
        let Some(v) = B32_ALPHABET.iter().position(|&a| a == c) else {
            return Err(binascii_error("Non-base32 digit found"));
        };
        acc = (acc << 5) | v as u64;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xff) as u8);
            acc &= (1u64 << bits) - 1;
        }
    }
    Ok(Value::bytes(out))
}

fn b16encode(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("b16encode", &kw)?;
    exactly("b16encode", &args, 1)?;
    let data = want_bytes(&args[0])?;
    const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = Vec::with_capacity(data.len() * 2);
    for b in data {
        out.push(DIGITS[(b >> 4) as usize]);
        out.push(DIGITS[(b & 15) as usize]);
    }
    Ok(Value::bytes(out))
}

fn b16decode(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("b16decode", args, kw, &["s", "casefold"], 1)?;
    let mut data = want_ascii_or_bytes(s[0].as_ref().unwrap())?;
    if s[1].as_ref().map(|v| v.is_true()).unwrap_or(false) {
        data.make_ascii_uppercase();
    }
    if data.iter().any(|c| !matches!(c, b'0'..=b'9' | b'A'..=b'F')) {
        return Err(binascii_error("Non-base16 digit found"));
    }
    from_hex(&data).map(Value::bytes).map_err(binascii_error)
}

pub fn build(_vm: &mut Vm) -> Rc<ModuleObj> {
    ModuleBuilder::new("_base64")
        .func("b64encode", b64encode)
        .func("b64decode", b64decode)
        .func("standard_b64encode", standard_b64encode)
        .func("standard_b64decode", standard_b64decode)
        .func("urlsafe_b64encode", urlsafe_b64encode)
        .func("urlsafe_b64decode", urlsafe_b64decode)
        .func("b32encode", b32encode)
        .func("b32decode", b32decode)
        .func("b16encode", b16encode)
        .func("b16decode", b16decode)
        .func("encodebytes", encodebytes)
        .func("decodebytes", decodebytes)
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

    fn r(f: NativeFnPtr, args: Vec<Value>) -> String {
        repr(&call(f, args).unwrap())
    }

    #[test]
    fn b64_roundtrip() {
        assert_eq!(r(b64encode, vec![b(b"hello")]), "b'aGVsbG8='");
        assert_eq!(r(b64decode, vec![b(b"aGVsbG8=")]), "b'hello'");
        assert_eq!(r(b64decode, vec![Value::str("aGVsbG8=")]), "b'hello'");
        assert_eq!(r(b64encode, vec![b(b"")]), "b''");
        assert_eq!(r(standard_b64encode, vec![b(b"foobar")]), "b'Zm9vYmFy'");
    }

    #[test]
    fn b64_urlsafe() {
        assert_eq!(r(urlsafe_b64encode, vec![b(b"\xfb\xff")]), "b'-_8='");
        assert_eq!(r(urlsafe_b64decode, vec![b(b"-_8=")]), "b'\\xfb\\xff'");
        assert_eq!(r(b64encode, vec![b(b"\xfb\xff")]), "b'+/8='");
    }

    #[test]
    fn b64_errors() {
        let e = call(b64decode, vec![b(b"aGVsbG8")]).unwrap_err();
        assert_eq!((e.kind, e.msg.as_str()), ("binascii.Error", "Incorrect padding"));
        let e = call(b64encode, vec![Value::str("x")]).unwrap_err();
        assert_eq!(e.kind, "TypeError");
        assert_eq!(e.msg, "a bytes-like object is required, not 'str'");
        let kw = vec![("validate".to_string(), Value::Bool(true))];
        let mut vm = Vm::new();
        let e = b64decode(&mut vm, vec![b(b"aG!Vs")], kw).unwrap_err();
        assert_eq!(e.msg, "Only base64 data is allowed");
        // Sem validate, o caractere fora do alfabeto é descartado.
        assert_eq!(r(b64decode, vec![b(b"aGVs\nbG8=")]), "b'hello'");
    }

    #[test]
    fn b32_and_b16() {
        assert_eq!(r(b32encode, vec![b(b"foobar")]), "b'MZXW6YTBOI======'");
        assert_eq!(r(b32decode, vec![b(b"MZXW6YTBOI======")]), "b'foobar'");
        assert_eq!(r(b32encode, vec![b(b"f")]), "b'MY======'");
        assert_eq!(r(b32decode, vec![b(b"MY======")]), "b'f'");
        assert_eq!(r(b16encode, vec![b(b"foo")]), "b'666F6F'");
        assert_eq!(r(b16decode, vec![b(b"666F6F")]), "b'foo'");
        let e = call(b16decode, vec![b(b"666f6f")]).unwrap_err();
        assert_eq!(e.msg, "Non-base16 digit found");
        assert_eq!(r(b16decode, vec![b(b"666f6f"), Value::Bool(true)]), "b'foo'");
    }
}
