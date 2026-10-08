//! `_codecs`: as funções sem estado dos codecs (UTF-7/8/16/32, ASCII, Latin-1, `charmap`, `escape` e
//! `unicode_escape`), o registro de codecs (`register`, `unregister`, `lookup`, `encode`, `decode`) e o
//! registro dos tratadores de erro (`register_error`, `lookup_error`), como o módulo C do CPython 3.13.
//!
//! O trabalho de cada codec é o de `textcodec` (o mesmo de `str.encode` e `bytes.decode`); aqui ficam a
//! checagem de argumentos do Argument Clinic, a contabilidade de `final` e dos bytes consumidos, e o
//! estado por interpretador. O `codecs.py` embutido registra o buscador dos codecs da imagem.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::methods::bytesm::{decode_bytes, decode_utf8_stateful};
use crate::modules::binascii::want_bytes;
use crate::modules::ModuleBuilder;
use crate::native_util::{bind, clinic_str_arg};
use crate::object::{code_points, cp_to_str, is, no_attribute, push_cp, Dict, ExtObject, Kw, ModuleObj, NativeFn, NativeFnPtr, Value};
use crate::textcodec::{self, escape_cp, Codec, Runs};
use crate::vm::{exc, type_error, PyException, PyResult, Vm};

// ---------------------------------------------------------------------------------------------
// Argumentos
// ---------------------------------------------------------------------------------------------

/// A checagem de posicionais do Argument Clinic: nenhum nomeado e entre `min` e `max` argumentos.
fn arity(name: &str, args: &[Value], kw: &Kw, min: usize, max: usize) -> PyResult<()> {
    if !kw.is_empty() {
        return Err(type_error(format!("_codecs.{name}() takes no keyword arguments")));
    }
    let n = args.len();
    if (min..=max).contains(&n) {
        return Ok(());
    }
    let (qualifier, bound) = match (n < min, min == max) {
        (_, true) => ("", min),
        (true, false) => ("at least ", min),
        (false, false) => ("at most ", max),
    };
    Err(type_error(format!("{name} expected {qualifier}{bound} argument{}, got {n}", if bound == 1 { "" } else { "s" })))
}

/// O argumento único de uma função `METH_O`.
fn one_arg(name: &str, args: Vec<Value>, kw: &Kw) -> PyResult<Value> {
    if !kw.is_empty() {
        return Err(type_error(format!("_codecs.{name}() takes no keyword arguments")));
    }
    let n = args.len();
    match <[Value; 1]>::try_from(args) {
        Ok([only]) => Ok(only),
        Err(_) => Err(type_error(format!("_codecs.{name}() takes exactly one argument ({n} given)"))),
    }
}

/// `errors`: `str` ou `None` (que vale `strict`).
fn errors_arg(name: &str, pos: usize, v: Option<&Value>) -> PyResult<String> {
    match v {
        None | Some(Value::None) => Ok("strict".to_string()),
        Some(Value::Str(s)) if s.as_str().contains('\0') => Err(exc("ValueError", "embedded null character")),
        Some(Value::Str(s)) => Ok(s.as_str().to_string()),
        Some(other) => Err(type_error(format!("{name}() argument {pos} must be str or None, not {}", other.type_name()))),
    }
}

fn int_arg(v: Option<&Value>, default: i64) -> PyResult<i64> {
    match v {
        None => Ok(default),
        Some(Value::Int(i)) => Ok(*i),
        Some(Value::Bool(b)) => Ok(i64::from(*b)),
        Some(Value::Big(_)) => Err(exc("OverflowError", "Python int too large to convert to C int")),
        Some(other) => Err(type_error(format!("'{}' object cannot be interpreted as an integer", other.type_name()))),
    }
}

fn text_arg<'a>(name: &str, v: &'a Value) -> PyResult<&'a str> {
    match v {
        Value::Str(s) => Ok(s.as_str()),
        other => Err(type_error(format!("{name}() argument 1 must be str, not {}", other.type_name()))),
    }
}

/// `Py_buffer(accept={str, buffer})`: um `str` entra como os bytes do UTF-8.
fn buffer_or_str(v: &Value) -> PyResult<Vec<u8>> {
    match v {
        Value::Str(s) => textcodec::encode_utf8(s.as_str(), "strict"),
        other => want_bytes(other),
    }
}

fn str_and_len(text: String, used: usize) -> Value {
    Value::tuple(vec![Value::str(text), Value::Int(used as i64)])
}

fn bytes_and_len(data: Vec<u8>, used: usize) -> Value {
    Value::tuple(vec![Value::bytes(data), Value::Int(used as i64)])
}

/// O nome do codec UTF-16/32 que o CPython põe nas mensagens: o da ordem de bytes fixa, ou o genérico.
fn utf_name(width: usize, big: Option<bool>) -> &'static str {
    match (width, big) {
        (2, None) => "utf-16",
        (2, Some(false)) => "utf-16-le",
        (2, Some(true)) => "utf-16-be",
        (_, None) => "utf-32",
        (_, Some(false)) => "utf-32-le",
        (_, Some(true)) => "utf-32-be",
    }
}

/// `byteorder`: 0 detecta o BOM (e escreve um), negativo é little-endian, positivo big-endian.
fn byte_order(order: i64) -> Option<bool> {
    match order.signum() {
        0 => None,
        -1 => Some(false),
        _ => Some(true),
    }
}

// ---------------------------------------------------------------------------------------------
// Decodificação
// ---------------------------------------------------------------------------------------------

/// `*_decode(data, errors=None, final=False, /)`: `(texto, bytes consumidos)`.
fn decode_stateful(
    name: &str,
    args: Vec<Value>,
    kw: Kw,
    want: fn(&Value) -> PyResult<Vec<u8>>,
    default_final: bool,
    decode: impl Fn(&[u8], &str, bool) -> PyResult<(String, usize)>,
) -> PyResult<Value> {
    arity(name, &args, &kw, 1, 3)?;
    let data = want(&args[0])?;
    let errors = errors_arg(name, 2, args.get(1))?;
    let final_ = args.get(2).map_or(default_final, Value::is_true);
    let (text, used) = decode(&data, &errors, final_)?;
    Ok(str_and_len(text, used))
}

macro_rules! decoder {
    ($fname:ident, $pyname:literal, $want:expr, $default_final:expr, $decode:expr) => {
        fn $fname(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
            decode_stateful($pyname, args, kw, $want, $default_final, $decode)
        }
    };
}

decoder!(utf_8_decode, "utf_8_decode", want_bytes, false, decode_utf8_stateful);
decoder!(utf_7_decode, "utf_7_decode", want_bytes, false, textcodec::decode_utf7);
decoder!(utf_16_decode, "utf_16_decode", want_bytes, false, |d, e, f| textcodec::decode_utf16(d, None, utf_name(2, None), e, f));
decoder!(utf_16_le_decode, "utf_16_le_decode", want_bytes, false, |d, e, f| {
    textcodec::decode_utf16(d, Some(false), utf_name(2, Some(false)), e, f)
});
decoder!(utf_16_be_decode, "utf_16_be_decode", want_bytes, false, |d, e, f| {
    textcodec::decode_utf16(d, Some(true), utf_name(2, Some(true)), e, f)
});
decoder!(utf_32_decode, "utf_32_decode", want_bytes, false, |d, e, f| textcodec::decode_utf32(d, None, utf_name(4, None), e, f));
decoder!(utf_32_le_decode, "utf_32_le_decode", want_bytes, false, |d, e, f| {
    textcodec::decode_utf32(d, Some(false), utf_name(4, Some(false)), e, f)
});
decoder!(utf_32_be_decode, "utf_32_be_decode", want_bytes, false, |d, e, f| {
    textcodec::decode_utf32(d, Some(true), utf_name(4, Some(true)), e, f)
});
decoder!(unicode_escape_decode, "unicode_escape_decode", buffer_or_str, true, |d, e, f| {
    textcodec::decode_unicode_escape(d, false, e, f)
});
decoder!(raw_unicode_escape_decode, "raw_unicode_escape_decode", buffer_or_str, true, |d, e, f| {
    textcodec::decode_unicode_escape(d, true, e, f)
});

/// `ascii_decode` e `latin_1_decode`: `(data, errors=None, /)`, tudo consumido.
fn simple_decode(name: &str, encoding: &str, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    arity(name, &args, &kw, 1, 2)?;
    let data = want_bytes(&args[0])?;
    let errors = errors_arg(name, 2, args.get(1))?;
    Ok(str_and_len(decode_bytes(&data, encoding, &errors)?, data.len()))
}

fn ascii_decode(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    simple_decode("ascii_decode", "ascii", args, kw)
}

fn latin_1_decode(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    simple_decode("latin_1_decode", "latin-1", args, kw)
}

/// A ordem de bytes que o BOM de `data` anuncia (-1 little-endian, 1 big-endian, 0 sem BOM).
fn bom_order(data: &[u8], width: usize) -> i64 {
    let (little, big): (&[u8], &[u8]) =
        if width == 2 { (&[0xFF, 0xFE][..], &[0xFE, 0xFF][..]) } else { (&[0xFF, 0xFE, 0, 0][..], &[0, 0, 0xFE, 0xFF][..]) };
    if data.starts_with(little) {
        -1
    } else if data.starts_with(big) {
        1
    } else {
        0
    }
}

/// `utf_16_ex_decode` e `utf_32_ex_decode`: `(texto, consumidos, ordem de bytes detectada)`.
fn ex_decode(name: &str, width: usize, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    arity(name, &args, &kw, 1, 4)?;
    let data = want_bytes(&args[0])?;
    let errors = errors_arg(name, 2, args.get(1))?;
    let order = int_arg(args.get(2), 0)?;
    let final_ = args.get(3).is_some_and(Value::is_true);
    let big = byte_order(order);
    let (text, used) = if width == 2 {
        textcodec::decode_utf16(&data, big, utf_name(2, big), &errors, final_)?
    } else {
        textcodec::decode_utf32(&data, big, utf_name(4, big), &errors, final_)?
    };
    let detected = if order == 0 { bom_order(&data, width) } else { order };
    Ok(Value::tuple(vec![Value::str(text), Value::Int(used as i64), Value::Int(detected)]))
}

fn utf_16_ex_decode(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    ex_decode("utf_16_ex_decode", 2, args, kw)
}

fn utf_32_ex_decode(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    ex_decode("utf_32_ex_decode", 4, args, kw)
}

// ---------------------------------------------------------------------------------------------
// Codificação
// ---------------------------------------------------------------------------------------------

/// `*_encode(str, errors=None, ..., /)`: `(bytes, caracteres consumidos)`. `max` é o número de
/// posicionais que a função aceita (`byteorder` conta nas de UTF-16/32).
fn encode_with(
    name: &str,
    args: Vec<Value>,
    kw: Kw,
    max: usize,
    encode: impl Fn(&str, &str) -> PyResult<Vec<u8>>,
) -> PyResult<Value> {
    arity(name, &args, &kw, 1, max)?;
    let text = text_arg(name, &args[0])?;
    let errors = errors_arg(name, 2, args.get(1))?;
    Ok(bytes_and_len(encode(text, &errors)?, code_points(text).count()))
}

macro_rules! encoder {
    ($fname:ident, $pyname:literal, $encode:expr) => {
        fn $fname(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
            encode_with($pyname, args, kw, 2, $encode)
        }
    };
}

encoder!(utf_8_encode, "utf_8_encode", textcodec::encode_utf8);
encoder!(utf_7_encode, "utf_7_encode", |s, _| Ok(textcodec::encode_utf7(s)));
encoder!(ascii_encode, "ascii_encode", |s, e| textcodec::encode_ucs1("ascii", 128, s, e));
encoder!(latin_1_encode, "latin_1_encode", |s, e| textcodec::encode_ucs1("latin-1", 256, s, e));
encoder!(unicode_escape_encode, "unicode_escape_encode", |s, e| textcodec::encode(&Codec::UnicodeEscape, s, e));
encoder!(raw_unicode_escape_encode, "raw_unicode_escape_encode", |s, e| textcodec::encode(&Codec::RawUnicodeEscape, s, e));

/// O codec UTF-16 (`width` 2) ou UTF-32 da ordem de bytes `order` (ver `byte_order`).
fn wide_codec(width: usize, order: i64) -> Codec {
    let big = byte_order(order);
    let name = utf_name(width, big);
    if width == 2 {
        Codec::Utf16 { big, name }
    } else {
        Codec::Utf32 { big, name }
    }
}

/// `utf_16_encode` e `utf_32_encode`: `(str, errors=None, byteorder=0, /)`; as variantes `_le` e
/// `_be` fixam a ordem e não têm o terceiro argumento.
fn wide_encode(name: &str, width: usize, order: Option<i64>, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let (max, order) = match order {
        Some(fixed) => (2, fixed),
        None => (3, int_arg(args.get(2), 0)?),
    };
    let codec = wide_codec(width, order);
    encode_with(name, args, kw, max, move |s, e| textcodec::encode(&codec, s, e))
}

macro_rules! wide_encoder {
    ($fname:ident, $pyname:literal, $width:literal, $order:expr) => {
        fn $fname(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
            wide_encode($pyname, $width, $order, args, kw)
        }
    };
}

wide_encoder!(utf_16_encode, "utf_16_encode", 2, None);
wide_encoder!(utf_16_le_encode, "utf_16_le_encode", 2, Some(-1));
wide_encoder!(utf_16_be_encode, "utf_16_be_encode", 2, Some(1));
wide_encoder!(utf_32_encode, "utf_32_encode", 4, None);
wide_encoder!(utf_32_le_encode, "utf_32_le_encode", 4, Some(-1));
wide_encoder!(utf_32_be_encode, "utf_32_be_encode", 4, Some(1));

fn readbuffer_encode(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    arity("readbuffer_encode", &args, &kw, 1, 2)?;
    let data = match &args[0] {
        Value::Str(_) => buffer_or_str(&args[0])?,
        other => want_bytes(other)?,
    };
    errors_arg("readbuffer_encode", 2, args.get(1))?;
    let used = data.len();
    Ok(bytes_and_len(data, used))
}

// ---------------------------------------------------------------------------------------------
// escape_decode / escape_encode
// ---------------------------------------------------------------------------------------------

fn hex_digit(b: u8) -> Option<u8> {
    char::from(b).to_digit(16).map(|d| d as u8)
}

/// `_PyBytes_DecodeEscape`: os bytes e o texto do primeiro escape inválido (que o CPython avisa).
fn unescape_bytes(data: &[u8], errors: &str) -> PyResult<(Vec<u8>, Option<String>)> {
    let mut out = Vec::with_capacity(data.len());
    let mut warning: Option<String> = None;
    let mut i = 0;
    while i < data.len() {
        let b = data[i];
        i += 1;
        if b != b'\\' {
            out.push(b);
            continue;
        }
        let Some(&c) = data.get(i) else {
            return Err(exc("ValueError", "Trailing \\ in string"));
        };
        i += 1;
        match c {
            b'\n' => {}
            b'\\' | b'\'' | b'"' => out.push(c),
            b'a' => out.push(7),
            b'b' => out.push(8),
            b't' => out.push(9),
            b'n' => out.push(10),
            b'v' => out.push(11),
            b'f' => out.push(12),
            b'r' => out.push(13),
            b'0'..=b'7' => {
                let first = i - 1;
                let mut v = u32::from(c - b'0');
                for _ in 0..2 {
                    match data.get(i) {
                        Some(&d) if (b'0'..=b'7').contains(&d) => {
                            v = (v << 3) + u32::from(d - b'0');
                            i += 1;
                        }
                        _ => break,
                    }
                }
                if v > 0o377 && warning.is_none() {
                    let digits = String::from_utf8_lossy(&data[first..i.min(first + 3)]).into_owned();
                    warning = Some(format!("invalid octal escape sequence '\\{digits}'"));
                }
                out.push(v as u8);
            }
            b'x' => {
                let pair = data.get(i..i + 2).and_then(|d| Some((hex_digit(d[0])?, hex_digit(d[1])?)));
                if let Some((hi, lo)) = pair {
                    out.push((hi << 4) | lo);
                    i += 2;
                    continue;
                }
                match errors {
                    "strict" => return Err(exc("ValueError", format!("invalid \\x escape at position {}", i - 2))),
                    "replace" => out.push(b'?'),
                    "ignore" => {}
                    other => {
                        return Err(exc("ValueError", format!("decoding error; unknown error handling code: {other}")))
                    }
                }
                for _ in 0..2 {
                    if data.get(i).is_some_and(|d| d.is_ascii_hexdigit()) {
                        i += 1;
                    }
                }
            }
            _ => {
                if warning.is_none() {
                    warning = Some(format!("invalid escape sequence '\\{}'", char::from(c)));
                }
                out.push(b'\\');
                i -= 1;
            }
        }
    }
    Ok((out, warning))
}

fn warn_deprecated(vm: &mut Vm, message: String) -> PyResult<()> {
    let warnings = crate::modules::import_value(vm, "warnings")?;
    let warn = vm.load_attr(&warnings, "warn")?;
    vm.call(&warn, vec![Value::str(message), Value::Builtin("DeprecationWarning")], Vec::new())?;
    Ok(())
}

fn escape_decode(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    arity("escape_decode", &args, &kw, 1, 2)?;
    let data = buffer_or_str(&args[0])?;
    let errors = errors_arg("escape_decode", 2, args.get(1))?;
    let (decoded, warning) = unescape_bytes(&data, &errors)?;
    if let Some(message) = warning {
        warn_deprecated(vm, message)?;
    }
    Ok(bytes_and_len(decoded, data.len()))
}

fn escape_encode(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    arity("escape_encode", &args, &kw, 1, 2)?;
    let Value::Bytes(data) = &args[0] else {
        return Err(type_error(format!("escape_encode() argument 1 must be bytes, not {}", args[0].type_name())));
    };
    errors_arg("escape_encode", 2, args.get(1))?;
    let mut out = Vec::with_capacity(data.len());
    for &b in data.iter() {
        match b {
            b'\\' | b'\'' => out.extend([b'\\', b]),
            b'\t' => out.extend(b"\\t"),
            b'\n' => out.extend(b"\\n"),
            b'\r' => out.extend(b"\\r"),
            0x20..=0x7e => out.push(b),
            _ => out.extend(format!("\\x{b:02x}").bytes()),
        }
    }
    Ok(bytes_and_len(out, data.len()))
}

// ---------------------------------------------------------------------------------------------
// charmap
// ---------------------------------------------------------------------------------------------

/// O `EncodingMap` que `charmap_build` devolve: código-ponto para byte.
struct EncodingMap {
    map: HashMap<u32, u8>,
}

impl ExtObject for EncodingMap {
    fn type_name(&self) -> &'static str {
        "EncodingMap"
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn methods(&self) -> &'static [&'static str] {
        &["size"]
    }

    fn call_method(&self, _vm: &mut Vm, name: &str, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        match name {
            "size" => Ok(Value::Int(self.map.len() as i64)),
            other => Err(no_attribute("EncodingMap", other)),
        }
    }
}

fn charmap_build(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let table = one_arg("charmap_build", args, &kw)?;
    let Value::Str(table) = &table else {
        return Err(type_error(format!("charmap_build() argument must be str, not {}", table.type_name())));
    };
    let mut map = HashMap::new();
    for (index, cp) in code_points(table.as_str()).enumerate().take(256) {
        if cp != 0xFFFE {
            map.insert(cp, index as u8);
        }
    }
    // Só a tabela de 256 posições vira `EncodingMap`; as demais ficam num dicionário.
    if code_points(table.as_str()).count() == 256 {
        return Ok(Value::Ext(Rc::new(EncodingMap { map })));
    }
    let mut dict = Dict::default();
    for (index, cp) in code_points(table.as_str()).enumerate() {
        if cp != 0xFFFE {
            dict.set(Value::Int(i64::from(cp)), Value::Int(index as i64))?;
        }
    }
    Ok(Value::dict(dict))
}

fn is_lookup_error(e: &PyException) -> bool {
    matches!(e.kind, "KeyError" | "IndexError" | "LookupError")
}

/// O que um mapeamento de decodificação dá para um byte.
enum Mapped {
    Undefined,
    Text(String),
}

fn classify_mapped(v: Value) -> PyResult<Mapped> {
    match v {
        Value::None => Ok(Mapped::Undefined),
        Value::Int(n) if !(0..=0x10FFFF).contains(&n) => Err(type_error("character mapping must be in range(0x110000)")),
        Value::Int(0xFFFE) => Ok(Mapped::Undefined),
        Value::Int(n) => {
            let mut text = String::new();
            push_cp(&mut text, n as u32);
            Ok(Mapped::Text(text))
        }
        Value::Str(s) if s.as_str() == "\u{FFFE}" => Ok(Mapped::Undefined),
        Value::Str(s) => Ok(Mapped::Text(s.as_str().to_string())),
        _ => Err(type_error("character mapping must return integer, None or str")),
    }
}

fn charmap_decode(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    arity("charmap_decode", &args, &kw, 1, 3)?;
    let data = want_bytes(&args[0])?;
    let errors = errors_arg("charmap_decode", 2, args.get(1))?;
    let Some(mapping) = args.get(2).filter(|m| !matches!(m, Value::None)) else {
        return Ok(str_and_len(data.iter().map(|&b| char::from(b)).collect(), data.len()));
    };
    let table: Option<Vec<u32>> = match mapping {
        Value::Str(s) => Some(code_points(s.as_str()).collect()),
        _ => None,
    };
    let mut out = String::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        let byte = data[i];
        let mapped = match &table {
            Some(cps) => match cps.get(usize::from(byte)) {
                Some(&cp) if cp != 0xFFFE => classify_mapped(Value::Int(i64::from(cp)))?,
                _ => Mapped::Undefined,
            },
            None => match crate::vm::subscript(mapping, &Value::Int(i64::from(byte))) {
                Ok(v) => classify_mapped(v)?,
                Err(e) if is_lookup_error(&e) => Mapped::Undefined,
                Err(e) => return Err(e),
            },
        };
        match mapped {
            Mapped::Text(text) => {
                out.push_str(&text);
                i += 1;
            }
            Mapped::Undefined => {
                i += textcodec::decode_bad(&mut out, &errors, &data, i..i + 1, "charmap", "character maps to <undefined>")?;
            }
        }
    }
    Ok(str_and_len(out, data.len()))
}

/// Um `encode` por código-ponto sobre `put`, com o nome e o motivo do `charmap` do CPython.
fn charmap_runs(put: &dyn Fn(u32, &mut Vec<u8>) -> bool, text: &str, errors: &str) -> PyResult<Vec<u8>> {
    Runs { name: "charmap", reason: "character maps to <undefined>", put, pass: None, prefix_escape: false, group: true }
        .encode(text, errors)
}

/// O que um mapeamento de codificação dá para um código-ponto: um byte, bytes, ou nada.
fn charmap_lookup(mapping: &Value, cp: u32) -> PyResult<Option<Vec<u8>>> {
    match crate::vm::subscript(mapping, &Value::Int(i64::from(cp))) {
        Ok(Value::Int(n)) if (0..256).contains(&n) => Ok(Some(vec![n as u8])),
        Ok(Value::Int(_)) => Err(type_error("character mapping must be in range(256)")),
        Ok(Value::Bytes(b)) => Ok(Some(b.to_vec())),
        Ok(Value::None) => Ok(None),
        Ok(other) => Err(type_error(format!("character mapping must return integer, bytes or None, not {}", other.type_name()))),
        Err(e) if is_lookup_error(&e) => Ok(None),
        Err(e) => Err(e),
    }
}

fn charmap_encode(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    arity("charmap_encode", &args, &kw, 1, 3)?;
    let text = text_arg("charmap_encode", &args[0])?;
    let errors = errors_arg("charmap_encode", 2, args.get(1))?;
    let encoded = match args.get(2).filter(|m| !matches!(m, Value::None)) {
        None => textcodec::encode_ucs1("latin-1", 256, text, &errors)?,
        Some(Value::Ext(ext)) if ext.as_any().is_some_and(|a| a.is::<EncodingMap>()) => {
            let map = ext.as_any().and_then(|a| a.downcast_ref::<EncodingMap>()).map(|m| &m.map);
            let put = |cp: u32, out: &mut Vec<u8>| {
                map.and_then(|m| m.get(&cp)).is_some_and(|&b| {
                    out.push(b);
                    true
                })
            };
            charmap_runs(&put, text, &errors)?
        }
        Some(mapping) => {
            let mut known: HashMap<u32, Vec<u8>> = HashMap::new();
            let mut seen: HashSet<u32> = HashSet::new();
            for cp in code_points(text) {
                if seen.insert(cp) {
                    if let Some(bytes) = charmap_lookup(mapping, cp)? {
                        known.insert(cp, bytes);
                    }
                }
            }
            // Os trechos de substituição (`?`, `&#...;`, `\N{...}`) também passam pelo mapeamento.
            if !matches!(errors.as_str(), "strict" | "ignore") {
                for cp in 0x20..0x7F {
                    if seen.insert(cp) {
                        if let Ok(Some(bytes)) = charmap_lookup(mapping, cp) {
                            known.insert(cp, bytes);
                        }
                    }
                }
            }
            let put = |cp: u32, out: &mut Vec<u8>| {
                known.get(&cp).is_some_and(|b| {
                    out.extend_from_slice(b);
                    true
                })
            };
            charmap_runs(&put, text, &errors)?
        }
    };
    Ok(bytes_and_len(encoded, code_points(text).count()))
}

// ---------------------------------------------------------------------------------------------
// Tratadores de erro
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Encode,
    Decode,
    Translate,
}

/// O trecho que falhou, lido da exceção Unicode que o codec entrega ao tratador.
struct Fault {
    kind: Kind,
    /// Os código-pontos do texto (`Encode`, `Translate`).
    cps: Vec<u32>,
    /// Os bytes (`Decode`).
    bytes: Vec<u8>,
    start: usize,
    end: usize,
    encoding: String,
}

fn usize_attr(vm: &mut Vm, obj: &Value, name: &str) -> PyResult<usize> {
    match vm.load_attr(obj, name)? {
        Value::Int(n) => Ok(n.max(0) as usize),
        other => Err(type_error(format!("an integer is required (got type {})", other.type_name()))),
    }
}

fn unicode_fault(vm: &mut Vm, value: &Value) -> PyResult<Option<Fault>> {
    let Value::Exception(e) = value else { return Ok(None) };
    let kind = match e.kind {
        "UnicodeEncodeError" => Kind::Encode,
        "UnicodeDecodeError" => Kind::Decode,
        "UnicodeTranslateError" => Kind::Translate,
        _ => return Ok(None),
    };
    let object = vm.load_attr(value, "object")?;
    let (cps, bytes) = match (&object, kind) {
        (Value::Str(s), Kind::Encode | Kind::Translate) => (code_points(s.as_str()).collect::<Vec<u32>>(), Vec::new()),
        (other, Kind::Decode) => (Vec::new(), want_bytes(other)?),
        (other, _) => return Err(type_error(format!("exception object must be str, not {}", other.type_name()))),
    };
    let len = if kind == Kind::Decode { bytes.len() } else { cps.len() };
    let start = usize_attr(vm, value, "start")?.min(len);
    let end = usize_attr(vm, value, "end")?.min(len).max(start);
    let encoding = match (kind, vm.load_attr(value, "encoding")) {
        (Kind::Translate, _) => String::new(),
        (_, Ok(Value::Str(s))) => s.as_str().to_string(),
        _ => String::new(),
    };
    Ok(Some(Fault { kind, cps, bytes, start, end, encoding }))
}

fn wrong_exception(value: &Value) -> PyException {
    type_error(format!("don't know how to handle {} in error callback", value.type_name()))
}

/// A exceção do tratador, ou `TypeError` se não é Unicode.
fn take_fault(vm: &mut Vm, name: &str, args: Vec<Value>, kw: &Kw) -> PyResult<(Value, Fault)> {
    let value = one_arg(name, args, kw)?;
    match unicode_fault(vm, &value)? {
        Some(fault) => Ok((value, fault)),
        None => Err(wrong_exception(&value)),
    }
}

fn reply(replacement: Value, end: usize) -> Value {
    Value::tuple(vec![replacement, Value::Int(end as i64)])
}

fn strict_errors(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let value = one_arg("strict_errors", args, &kw)?;
    match value {
        Value::Exception(_) => Err(PyException::from_value(&value)),
        _ => Err(type_error("codec must pass exception instance")),
    }
}

fn ignore_errors(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let (_, fault) = take_fault(vm, "ignore_errors", args, &kw)?;
    Ok(reply(Value::str(""), fault.end))
}

fn replace_errors(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let (_, fault) = take_fault(vm, "replace_errors", args, &kw)?;
    let count = fault.end - fault.start;
    let text = match fault.kind {
        Kind::Encode => "?".repeat(count),
        Kind::Decode => "\u{FFFD}".to_string(),
        Kind::Translate => "\u{FFFD}".repeat(count),
    };
    Ok(reply(Value::str(text), fault.end))
}

/// Os handlers que só existem para a codificação (`xmlcharrefreplace`, `namereplace`).
fn encode_fault(vm: &mut Vm, name: &str, args: Vec<Value>, kw: &Kw) -> PyResult<Fault> {
    let (value, fault) = take_fault(vm, name, args, kw)?;
    if fault.kind == Kind::Encode {
        Ok(fault)
    } else {
        Err(wrong_exception(&value))
    }
}

fn xmlcharrefreplace_errors(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let fault = encode_fault(vm, "xmlcharrefreplace_errors", args, &kw)?;
    let text: String = fault.cps[fault.start..fault.end].iter().map(|cp| format!("&#{cp};")).collect();
    Ok(reply(Value::str(text), fault.end))
}

fn backslashreplace_errors(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let (_, fault) = take_fault(vm, "backslashreplace_errors", args, &kw)?;
    let text: String = match fault.kind {
        Kind::Decode => fault.bytes[fault.start..fault.end].iter().map(|b| format!("\\x{b:02x}")).collect(),
        Kind::Encode | Kind::Translate => fault.cps[fault.start..fault.end].iter().map(|&cp| escape_cp(cp)).collect(),
    };
    Ok(reply(Value::str(text), fault.end))
}

fn namereplace_errors(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let fault = encode_fault(vm, "namereplace_errors", args, &kw)?;
    let text: String = fault.cps[fault.start..fault.end]
        .iter()
        .map(|&cp| match char::from_u32(cp).and_then(unicode_names2::name) {
            Some(n) => format!("\\N{{{n}}}"),
            None => escape_cp(cp),
        })
        .collect();
    Ok(reply(Value::str(text), fault.end))
}

fn surrogateescape_handler(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let (value, fault) = take_fault(vm, "surrogateescape", args, &kw)?;
    match fault.kind {
        Kind::Encode => {
            let mut out = Vec::new();
            for &cp in &fault.cps[fault.start..fault.end] {
                if !(0xDC80..=0xDCFF).contains(&cp) {
                    return Err(PyException::from_value(&value));
                }
                out.push((cp - 0xDC00) as u8);
            }
            if out.is_empty() {
                return Err(PyException::from_value(&value));
            }
            let end = fault.end;
            Ok(reply(Value::bytes(out), end))
        }
        Kind::Decode => {
            let window = &fault.bytes[fault.start..(fault.start + 4).min(fault.bytes.len())];
            let escapable = window.iter().take_while(|&&b| b >= 0x80).count();
            if escapable == 0 {
                return Err(PyException::from_value(&value));
            }
            let mut text = String::new();
            window[..escapable].iter().for_each(|&b| push_cp(&mut text, 0xDC00 + u32::from(b)));
            Ok(reply(Value::str(text), fault.start + escapable))
        }
        Kind::Translate => Err(wrong_exception(&value)),
    }
}

/// O UTF que o nome de codec do erro designa: a largura da unidade em bytes (1 para UTF-8) e se é
/// big-endian. É o `_Py_normalize_encoding` seguido das comparações do `surrogatepass` do CPython.
fn surrogatepass_codec(encoding: &str) -> Option<(usize, bool)> {
    let normalized = normalize_encoding(encoding).replace('.', "_");
    match normalized.as_str() {
        "utf_8" | "utf8" => Some((1, false)),
        "utf_16" | "utf_16_le" | "utf_16le" | "utf16" | "utf16le" => Some((2, false)),
        "utf_16_be" | "utf_16be" | "utf16be" => Some((2, true)),
        "utf_32" | "utf_32_le" | "utf_32le" | "utf32" | "utf32le" => Some((4, false)),
        "utf_32_be" | "utf_32be" | "utf32be" => Some((4, true)),
        _ => None,
    }
}

fn surrogatepass_handler(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let (value, fault) = take_fault(vm, "surrogatepass", args, &kw)?;
    if fault.kind == Kind::Translate {
        return Err(wrong_exception(&value));
    }
    let fail = || PyException::from_value(&value);
    let Some((width, big)) = surrogatepass_codec(&fault.encoding) else { return Err(fail()) };
    match fault.kind {
        Kind::Encode => {
            let mut out = Vec::new();
            for &cp in &fault.cps[fault.start..fault.end] {
                if !(0xD800..=0xDFFF).contains(&cp) {
                    return Err(fail());
                }
                match (width, big) {
                    (1, _) => out.extend([0xE0 | (cp >> 12) as u8, 0x80 | ((cp >> 6) & 0x3F) as u8, 0x80 | (cp & 0x3F) as u8]),
                    (2, false) => out.extend((cp as u16).to_le_bytes()),
                    (2, true) => out.extend((cp as u16).to_be_bytes()),
                    (_, false) => out.extend(cp.to_le_bytes()),
                    (_, true) => out.extend(cp.to_be_bytes()),
                }
            }
            Ok(reply(Value::bytes(out), fault.end))
        }
        Kind::Decode => {
            let chunk = &fault.bytes[fault.start..];
            let needed = if width == 1 { 3 } else { width };
            if chunk.len() < needed {
                return Err(fail());
            }
            let cp = match (width, big) {
                (1, _) if chunk[0] & 0xF0 == 0xE0 && chunk[1] & 0xC0 == 0x80 && chunk[2] & 0xC0 == 0x80 => {
                    (u32::from(chunk[0] & 0x0F) << 12) | (u32::from(chunk[1] & 0x3F) << 6) | u32::from(chunk[2] & 0x3F)
                }
                (1, _) => return Err(fail()),
                (2, false) => u32::from(u16::from_le_bytes([chunk[0], chunk[1]])),
                (2, true) => u32::from(u16::from_be_bytes([chunk[0], chunk[1]])),
                (_, false) => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
                (_, true) => u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
            };
            if !(0xD800..=0xDFFF).contains(&cp) {
                return Err(fail());
            }
            let used = if width == 1 { 3 } else { width };
            Ok(reply(Value::str(cp_to_str(cp)), fault.start + used))
        }
        Kind::Translate => Err(wrong_exception(&value)),
    }
}

// ---------------------------------------------------------------------------------------------
// Registros
// ---------------------------------------------------------------------------------------------

thread_local! {
    /// As funções de busca, na ordem de registro.
    static SEARCH: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
    /// O `CodecInfo` já achado, pelo nome normalizado.
    static CACHE: RefCell<HashMap<String, Value>> = RefCell::new(HashMap::new());
    /// Os tratadores de erro por nome, criados com os embutidos na primeira consulta.
    static HANDLERS: RefCell<Option<HashMap<String, Value>>> = const { RefCell::new(None) };
}

/// O `__text_signature__` dos tratadores de erro embutidos: no CPython são `PyMethodDef` de `Python/codecs.c`
/// (sem módulo dono), cujo `ml_doc` abre com `($self, object, /)`.
pub(crate) fn handler_signature(name: &str) -> Option<&'static str> {
    matches!(
        name,
        "strict_errors" | "ignore_errors" | "replace_errors" | "xmlcharrefreplace_errors" | "backslashreplace_errors" | "namereplace_errors"
    )
    .then_some("($self, object, /)")
}

fn builtin_handlers() -> HashMap<String, Value> {
    let table = [
        ("strict", "strict_errors", strict_errors as NativeFnPtr),
        ("ignore", "ignore_errors", ignore_errors as NativeFnPtr),
        ("replace", "replace_errors", replace_errors as NativeFnPtr),
        ("xmlcharrefreplace", "xmlcharrefreplace_errors", xmlcharrefreplace_errors as NativeFnPtr),
        ("backslashreplace", "backslashreplace_errors", backslashreplace_errors as NativeFnPtr),
        ("namereplace", "namereplace_errors", namereplace_errors as NativeFnPtr),
        ("surrogateescape", "surrogateescape", surrogateescape_handler as NativeFnPtr),
        ("surrogatepass", "surrogatepass", surrogatepass_handler as NativeFnPtr),
    ];
    table
        .into_iter()
        .map(|(key, name, f)| (key.to_string(), Value::NativeFn(Rc::new(NativeFn { name, f }))))
        .collect()
}

fn with_handlers<R>(f: impl FnOnce(&mut HashMap<String, Value>) -> R) -> R {
    HANDLERS.with(|h| f(h.borrow_mut().get_or_insert_with(builtin_handlers)))
}

fn register_error(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    arity("register_error", &args, &kw, 2, 2)?;
    let Value::Str(name) = &args[0] else {
        return Err(type_error(format!("register_error() argument 1 must be str, not {}", args[0].type_name())));
    };
    if !crate::builtins::is_callable(&args[1]) {
        return Err(type_error("handler must be callable"));
    }
    with_handlers(|h| h.insert(name.as_str().to_string(), args[1].clone()));
    Ok(Value::None)
}

fn lookup_error(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let name = one_arg("lookup_error", args, &kw)?;
    let Value::Str(name) = &name else {
        return Err(type_error(format!("lookup_error() argument must be str, not {}", name.type_name())));
    };
    with_handlers(|h| h.get(name.as_str()).cloned())
        .ok_or_else(|| exc("LookupError", format!("unknown error handler name '{}'", name.as_str())))
}

fn register(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let search = one_arg("register", args, &kw)?;
    if !crate::builtins::is_callable(&search) {
        return Err(type_error("argument must be callable"));
    }
    SEARCH.with(|s| s.borrow_mut().push(search));
    Ok(Value::None)
}

fn unregister(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let search = one_arg("unregister", args, &kw)?;
    SEARCH.with(|s| s.borrow_mut().retain(|f| !is(f, &search)));
    CACHE.with(|c| c.borrow_mut().clear());
    Ok(Value::None)
}

/// `_Py_normalize_encoding` com minúsculas: letras ASCII, dígitos e `.` ficam, e cada sequência dos
/// demais caracteres vira um único `_` entre dois caracteres que ficam.
fn normalize_encoding(encoding: &str) -> String {
    let mut out = String::with_capacity(encoding.len());
    let mut punctuation = false;
    for c in encoding.chars() {
        if c.is_ascii_alphanumeric() || c == '.' {
            if punctuation && !out.is_empty() {
                out.push('_');
            }
            punctuation = false;
            out.push(c.to_ascii_lowercase());
        } else {
            punctuation = true;
        }
    }
    out
}

fn is_four_tuple(vm: &mut Vm, v: &Value) -> bool {
    match v {
        Value::Tuple(t) => t.len() == 4,
        Value::Instance(_) => matches!(vm.call(&Value::Builtin("len"), vec![v.clone()], Vec::new()), Ok(Value::Int(4))),
        _ => false,
    }
}

/// `_PyCodec_Lookup`: o `CodecInfo` de `encoding`, do cache ou da primeira função de busca que o conhece.
fn find_codec(vm: &mut Vm, encoding: &str) -> PyResult<Value> {
    if encoding.contains('\0') {
        return Err(exc("ValueError", "embedded null character"));
    }
    // O buscador dos codecs da imagem é registrado pelo `codecs.py`; importá-lo garante que ele exista
    // mesmo quando o programa só importou `_codecs`.
    crate::modules::import(vm, "codecs");
    let key = normalize_encoding(encoding);
    if let Some(hit) = CACHE.with(|c| c.borrow().get(&key).cloned()) {
        return Ok(hit);
    }
    let searchers = SEARCH.with(|s| s.borrow().clone());
    if searchers.is_empty() {
        return Err(exc("LookupError", "no codec search functions registered: can't find encoding"));
    }
    for search in searchers {
        let found = vm.call(&search, vec![Value::str(key.clone())], Vec::new())?;
        if matches!(found, Value::None) {
            continue;
        }
        if !is_four_tuple(vm, &found) {
            return Err(type_error("codec search functions must return 4-tuples"));
        }
        CACHE.with(|c| c.borrow_mut().insert(key, found.clone()));
        return Ok(found);
    }
    Err(exc("LookupError", format!("unknown encoding: {encoding}")))
}

fn lookup(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let encoding = one_arg("lookup", args, &kw)?;
    let Value::Str(encoding) = &encoding else {
        return Err(type_error(format!("lookup() argument must be str, not {}", encoding.type_name())));
    };
    find_codec(vm, encoding.as_str())
}

/// `encode` e `decode` do módulo: o codificador (`index` 0) ou o decodificador (1) do codec, chamado
/// com `(obj, errors)`, e só o primeiro item do par que ele devolve.
fn apply_codec(vm: &mut Vm, name: &str, index: i64, args: Vec<Value>, kw: Kw, bad_result: &str) -> PyResult<Value> {
    let slots = bind(name, args, kw, &["obj", "encoding", "errors"], 1)?;
    let encoding = clinic_str_arg(name, "encoding", &slots[1], "utf-8")?;
    let errors = clinic_str_arg(name, "errors", &slots[2], "strict")?;
    let obj = slots[0].clone().unwrap_or(Value::None);
    let info = find_codec(vm, &encoding)?;
    let func = crate::vm::subscript(&info, &Value::Int(index))?;
    match vm.call(&func, vec![obj, Value::str(errors)], Vec::new())? {
        Value::Tuple(pair) if pair.len() == 2 => Ok(pair[0].clone()),
        _ => Err(type_error(bad_result)),
    }
}

fn encode(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    apply_codec(vm, "encode", 0, args, kw, "encoder must return a tuple (object, integer)")
}

fn decode(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    apply_codec(vm, "decode", 1, args, kw, "decoder must return a tuple (object,integer)")
}

pub fn build(_vm: &mut Vm) -> Rc<ModuleObj> {
    ModuleBuilder::new("_codecs")
        .func("register", register)
        .func("unregister", unregister)
        .func("lookup", lookup)
        .func("encode", encode)
        .func("decode", decode)
        .func("register_error", register_error)
        .func("lookup_error", lookup_error)
        .func("utf_8_decode", utf_8_decode)
        .func("utf_8_encode", utf_8_encode)
        .func("utf_7_decode", utf_7_decode)
        .func("utf_7_encode", utf_7_encode)
        .func("utf_16_decode", utf_16_decode)
        .func("utf_16_le_decode", utf_16_le_decode)
        .func("utf_16_be_decode", utf_16_be_decode)
        .func("utf_16_ex_decode", utf_16_ex_decode)
        .func("utf_16_encode", utf_16_encode)
        .func("utf_16_le_encode", utf_16_le_encode)
        .func("utf_16_be_encode", utf_16_be_encode)
        .func("utf_32_decode", utf_32_decode)
        .func("utf_32_le_decode", utf_32_le_decode)
        .func("utf_32_be_decode", utf_32_be_decode)
        .func("utf_32_ex_decode", utf_32_ex_decode)
        .func("utf_32_encode", utf_32_encode)
        .func("utf_32_le_encode", utf_32_le_encode)
        .func("utf_32_be_encode", utf_32_be_encode)
        .func("ascii_decode", ascii_decode)
        .func("ascii_encode", ascii_encode)
        .func("latin_1_decode", latin_1_decode)
        .func("latin_1_encode", latin_1_encode)
        .func("charmap_build", charmap_build)
        .func("charmap_decode", charmap_decode)
        .func("charmap_encode", charmap_encode)
        .func("escape_decode", escape_decode)
        .func("escape_encode", escape_encode)
        .func("unicode_escape_decode", unicode_escape_decode)
        .func("unicode_escape_encode", unicode_escape_encode)
        .func("raw_unicode_escape_decode", raw_unicode_escape_decode)
        .func("raw_unicode_escape_encode", raw_unicode_escape_encode)
        .func("readbuffer_encode", readbuffer_encode)
        .build()
}
