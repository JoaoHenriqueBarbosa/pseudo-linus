//! Métodos de `bytes` (`Objects/bytesobject.c`).
//!
//! Só `bytes` imutável existe no núcleo (sem `bytearray`/`memoryview`), então "bytes-like" aqui é
//! `Value::Bytes`. Maiúsculas e minúsculas são só ASCII, como no CPython.

use std::rc::Rc;

use crate::native_util::{bind, clinic_str_arg, want_int, int_or};
use crate::object::{push_cp, Kw, NativeFnPtr, Value};
use crate::vm::{exc, iterate, type_error, PyResult, Vm};

fn this(args: &[Value]) -> PyResult<Rc<[u8]>> {
    match args.first() {
        Some(v @ (Value::Bytes(_) | Value::ByteArray(_))) => Ok(v.bytes_like().unwrap_or_else(|| Rc::from(&[][..]))),
        _ => Err(type_error("descriptor requires a 'bytes' object")),
    }
}

fn nokw(fname: &str, kw: &Kw) -> PyResult<()> {
    if kw.is_empty() {
        Ok(())
    } else {
        Err(type_error(format!("bytes.{fname}() takes no keyword arguments")))
    }
}

fn argc(fname: &str, rest: &[Value], min: usize, max: usize) -> PyResult<()> {
    let n = rest.len();
    if n >= min && n <= max {
        return Ok(());
    }
    let plural = |k: usize| if k == 1 { "" } else { "s" };
    let msg = if min == max {
        if min == 0 {
            format!("bytes.{fname}() takes no arguments ({n} given)")
        } else if min == 1 {
            format!("bytes.{fname}() takes exactly one argument ({n} given)")
        } else {
            format!("bytes.{fname}() takes exactly {min} arguments ({n} given)")
        }
    } else if n < min {
        format!("{fname} expected at least {min} argument{}, got {n}", plural(min))
    } else {
        format!("{fname} expected at most {max} argument{}, got {n}", plural(max))
    };
    Err(type_error(msg))
}

fn want_bytes(v: &Value) -> PyResult<Rc<[u8]>> {
    match v.bytes_like() {
        Some(b) => Ok(b),
        None => Err(type_error(format!("a bytes-like object is required, not '{}'", v.type_name()))),
    }
}

/// Argumento `sub` de `find`/`count`: bytes ou um inteiro de 0 a 255.
fn sub_arg(v: &Value) -> PyResult<Vec<u8>> {
    match v {
        Value::Bytes(_) | Value::ByteArray(_) => Ok(v.bytes_like().map(|b| b.to_vec()).unwrap_or_default()),
        Value::Int(_) | Value::Bool(_) => {
            let i = want_int(v)?;
            if (0..256).contains(&i) {
                Ok(vec![i as u8])
            } else {
                Err(exc("ValueError", "byte must be in range(0, 256)"))
            }
        }
        other => {
            Err(type_error(format!("argument should be integer or bytes-like object, not '{}'", other.type_name())))
        }
    }
}

/// `start`/`end` de fatia: negativo soma o tamanho; tudo é limitado a `0..=len`.
fn bound(v: Option<&Value>, len: usize, default: usize) -> PyResult<usize> {
    match v {
        None | Some(Value::None) => Ok(default),
        Some(x) => {
            let i = want_int(x)?;
            if i < 0 {
                let j = i.saturating_add(len as i64);
                Ok(if j < 0 { 0 } else { j as usize })
            } else {
                Ok(i.min(len as i64) as usize)
            }
        }
    }
}

fn find_from(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() {
        return if from <= hay.len() { Some(from) } else { None };
    }
    if hay.len() < needle.len() {
        return None;
    }
    let last = hay.len() - needle.len();
    let mut i = from;
    while i <= last {
        if &hay[i..i + needle.len()] == needle {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

// ----------------------------------------------------------------------------------------------
// decode

#[derive(Clone, Copy, PartialEq)]
enum Codec {
    Utf8,
    Ascii,
    Latin1,
    Cp437,
}

fn codec_of(name: &str) -> Option<Codec> {
    let norm = name.trim().to_ascii_lowercase().replace('_', "-");
    match norm.as_str() {
        "cp437" | "437" | "ibm437" => Some(Codec::Cp437),
        "utf-8" | "utf8" | "u8" | "utf" => Some(Codec::Utf8),
        "ascii" | "us-ascii" | "646" => Some(Codec::Ascii),
        "latin-1" | "latin1" | "iso-8859-1" | "iso8859-1" | "8859" | "cp819" | "latin" | "l1" => Some(Codec::Latin1),
        _ => None,
    }
}

/// Acrescenta UTF-8 válido a `out` na codificação dos `str` da VM: só o texto com o byte 0xF4
/// (plano 16) pode conter os chars que colidem com os surrogates guardados e o próprio U+10FFFF.
pub(crate) fn push_valid_utf8(out: &mut String, valid: &str) {
    if valid.as_bytes().contains(&0xF4) {
        valid.chars().for_each(|c| push_cp(out, u32::from(c)));
    } else {
        out.push_str(valid);
    }
}

/// O surrogate que o `surrogatepass` lê nos bytes `ED A0..BF 80..BF` (3 bytes, o que o UTF-8
/// estrito recusa).
fn utf8_surrogate(bytes: &[u8]) -> Option<u32> {
    match bytes {
        [0xED, b1 @ 0xA0..=0xBF, b2 @ 0x80..=0xBF, ..] => Some(0xD000 | (u32::from(b1 & 0x3F) << 6) | u32::from(b2 & 0x3F)),
        _ => None,
    }
}

pub(crate) fn decode_utf8(data: &[u8], errors: &str) -> PyResult<String> {
    decode_utf8_stateful(data, errors, true).map(|(text, _)| text)
}

/// `PyUnicode_DecodeUTF8Stateful`: o texto e quantos bytes foram consumidos (com `final_` falso, a
/// sequência válida que o fim dos dados interrompe fica para a próxima chamada).
pub(crate) fn decode_utf8_stateful(data: &[u8], errors: &str, final_: bool) -> PyResult<(String, usize)> {
    let mut out = String::new();
    let mut pos = 0usize;
    while pos < data.len() {
        match std::str::from_utf8(&data[pos..]) {
            Ok(s) => {
                push_valid_utf8(&mut out, s);
                break;
            }
            Err(e) => {
                let valid = e.valid_up_to();
                push_valid_utf8(&mut out, std::str::from_utf8(&data[pos..pos + valid]).unwrap_or(""));
                let bad_start = pos + valid;
                let first = data[bad_start];
                // `ED A0..BF` no fim dos dados é o começo de um surrogate que o `surrogatepass` aceitaria:
                // com `final_` falso o CPython o deixa para a próxima chamada, como uma sequência truncada.
                let surrogate_prefix = matches!(&data[bad_start..], [0xED, 0xA0..=0xBF]);
                let (bad_len, reason) = match e.error_len() {
                    Some(_) if surrogate_prefix && !final_ => return Ok((out, bad_start)),
                    Some(n) => {
                        let r = if (0x80..=0xc1).contains(&first) || first >= 0xf5 {
                            "invalid start byte"
                        } else {
                            "invalid continuation byte"
                        };
                        (n, r)
                    }
                    None if !final_ => return Ok((out, bad_start)),
                    None => (data.len() - bad_start, "unexpected end of data"),
                };
                if let Some(cp) = utf8_surrogate(&data[bad_start..]).filter(|_| errors == "surrogatepass") {
                    push_cp(&mut out, cp);
                    pos = bad_start + 3;
                    continue;
                }
                pos = bad_start
                    + crate::textcodec::decode_bad(&mut out, errors, data, bad_start..bad_start + bad_len, "utf-8", reason)?;
            }
        }
    }
    Ok((out, data.len()))
}

fn decode_ascii(data: &[u8], errors: &str) -> PyResult<String> {
    let mut out = String::with_capacity(data.len());
    for (i, &b) in data.iter().enumerate() {
        if b < 0x80 {
            out.push(b as char);
        } else {
            crate::textcodec::decode_bad(&mut out, errors, data, i..i + 1, "ascii", "ordinal not in range(128)")?;
        }
    }
    Ok(out)
}

fn decode(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let data = this(&args)?;
    let slots = bind("decode", args[1..].to_vec(), kw, &["encoding", "errors"], 0)?;
    let encoding = clinic_str_arg("decode", "encoding", &slots[0], "utf-8")?;
    let errors = clinic_str_arg("decode", "errors", &slots[1], "strict")?;
    Ok(Value::str(decode_bytes(&data, &encoding, &errors)?))
}

/// `bytes.decode(encoding, errors)`: o codec pelo nome (UTF-8, ASCII, Latin-1, cp437 e o resto).
pub(crate) fn decode_bytes(data: &[u8], encoding: &str, errors: &str) -> PyResult<String> {
    let Some(codec) = codec_of(encoding) else {
        return match crate::textcodec::lookup(encoding) {
            Some(c) => crate::textcodec::decode(&c, data, errors),
            None => Err(exc("LookupError", format!("unknown encoding: {encoding}"))),
        };
    };
    Ok(match codec {
        Codec::Utf8 => decode_utf8(data, errors)?,
        Codec::Ascii => decode_ascii(data, errors)?,
        Codec::Latin1 => data.iter().map(|&b| b as char).collect(),
        Codec::Cp437 => data.iter().map(|&b| crate::cp437::decode_byte(b)).collect(),
    })
}

// ----------------------------------------------------------------------------------------------
// hex

fn hex(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let data = this(&args)?;
    let slots = bind("hex", args[1..].to_vec(), kw, &["sep", "bytes_per_sep"], 0)?;
    let sep: Option<String> = match &slots[0] {
        None => None,
        Some(Value::Str(s)) => {
            if s.len() != 1 {
                return Err(exc("ValueError", "sep must be length 1."));
            }
            Some(s.as_str().to_string())
        }
        Some(Value::Bytes(b)) if b.len() == 1 => Some((b[0] as char).to_string()),
        Some(Value::Bytes(_)) => return Err(exc("ValueError", "sep must be length 1.")),
        Some(other) => {
            return Err(type_error(format!("sep must be str or bytes, not {}", other.type_name())));
        }
    };
    let per = match &slots[1] {
        None => 1,
        Some(v) => want_int(v)?,
    };
    let n = per.unsigned_abs() as usize;
    let len = data.len();
    let mut out = String::new();
    for (i, b) in data.iter().enumerate() {
        if let Some(s) = &sep {
            if i > 0 && n > 0 {
                let boundary = if per > 0 { (len - i) % n == 0 } else { i % n == 0 };
                if boundary {
                    out.push_str(s);
                }
            }
        }
        out.push_str(&format!("{b:02x}"));
    }
    Ok(Value::str(out))
}

// ----------------------------------------------------------------------------------------------
// split, strip

pub(super) fn split(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let data = this(&args)?;
    let slots = bind("split", args[1..].to_vec(), kw, &["sep", "maxsplit"], 0)?;
    let maxsplit = match &slots[1] {
        None => -1,
        Some(v) => want_int(v)?,
    };
    let mut parts: Vec<Value> = Vec::new();
    match &slots[0] {
        None | Some(Value::None) => {
            let n = data.len();
            let mut i = 0usize;
            let mut count = 0i64;
            loop {
                while i < n && is_ws(data[i]) {
                    i += 1;
                }
                if i >= n {
                    break;
                }
                if maxsplit >= 0 && count >= maxsplit {
                    parts.push(Value::bytes(data[i..].to_vec()));
                    break;
                }
                let mut j = i;
                while j < n && !is_ws(data[j]) {
                    j += 1;
                }
                parts.push(Value::bytes(data[i..j].to_vec()));
                i = j;
                count += 1;
            }
        }
        Some(sep) => {
            let sep = want_bytes(sep)?;
            if sep.is_empty() {
                return Err(exc("ValueError", "empty separator"));
            }
            let mut start = 0usize;
            let mut count = 0i64;
            while maxsplit < 0 || count < maxsplit {
                match find_from(&data, &sep, start) {
                    Some(p) => {
                        parts.push(Value::bytes(data[start..p].to_vec()));
                        start = p + sep.len();
                        count += 1;
                    }
                    None => break,
                }
            }
            parts.push(Value::bytes(data[start..].to_vec()));
        }
    }
    Ok(Value::list(parts))
}

fn strip_impl(fname: &str, args: Vec<Value>, kw: Kw, left: bool, right: bool) -> PyResult<Value> {
    nokw(fname, &kw)?;
    argc(fname, &args[1..], 0, 1)?;
    let data = this(&args)?;
    let chars: Option<Rc<[u8]>> = match args.get(1) {
        None | Some(Value::None) => None,
        Some(v) => Some(want_bytes(v)?),
    };
    let strip_me = |b: u8| match &chars {
        None => is_ws(b),
        Some(c) => c.contains(&b),
    };
    let mut lo = 0usize;
    let mut hi = data.len();
    if left {
        while lo < hi && strip_me(data[lo]) {
            lo += 1;
        }
    }
    if right {
        while hi > lo && strip_me(data[hi - 1]) {
            hi -= 1;
        }
    }
    Ok(Value::bytes(data[lo..hi].to_vec()))
}

pub(super) fn strip(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    strip_impl("strip", args, kw, true, true)
}

pub(super) fn lstrip(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    strip_impl("lstrip", args, kw, true, false)
}

pub(super) fn rstrip(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    strip_impl("rstrip", args, kw, false, true)
}

// ----------------------------------------------------------------------------------------------
// startswith, endswith

fn affix(fname: &str, args: Vec<Value>, kw: Kw, at_start: bool) -> PyResult<Value> {
    nokw(fname, &kw)?;
    argc(fname, &args[1..], 1, 3)?;
    let data = this(&args)?;
    let len = data.len();
    let start = bound(args.get(2), len, 0)?;
    let end = bound(args.get(3), len, len)?;
    let candidates: Vec<Rc<[u8]>> = match &args[1] {
        Value::Bytes(_) | Value::ByteArray(_) => vec![want_bytes(&args[1])?],
        Value::Tuple(items) => {
            let mut v = Vec::new();
            for it in items.iter() {
                match it {
                    Value::Bytes(_) | Value::ByteArray(_) => v.push(want_bytes(it)?),
                    other => {
                        return Err(type_error(format!(
                            "a bytes-like object is required, not '{}'",
                            other.type_name()
                        )))
                    }
                }
            }
            v
        }
        other => {
            return Err(type_error(format!(
                "{fname} first arg must be bytes or a tuple of bytes, not {}",
                other.type_name()
            )))
        }
    };
    if start > len {
        return Ok(Value::Bool(false));
    }
    let window: &[u8] = if end >= start { &data[start..end] } else { &[] };
    let hit = candidates.iter().any(|p| if at_start { window.starts_with(p) } else { window.ends_with(p) });
    Ok(Value::Bool(hit))
}

fn startswith(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    affix("startswith", args, kw, true)
}

fn endswith(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    affix("endswith", args, kw, false)
}

// ----------------------------------------------------------------------------------------------
// find, count, replace, join, upper, lower

fn find(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("find", &kw)?;
    argc("find", &args[1..], 1, 3)?;
    let data = this(&args)?;
    let sub = sub_arg(&args[1])?;
    let len = data.len();
    let start = bound(args.get(2), len, 0)?;
    let end = bound(args.get(3), len, len)?;
    if start > len || end < start {
        return Ok(Value::Int(-1));
    }
    match find_from(&data[..end], &sub, start) {
        Some(p) => Ok(Value::Int(p as i64)),
        None => Ok(Value::Int(-1)),
    }
}

fn count(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("count", &kw)?;
    argc("count", &args[1..], 1, 3)?;
    let data = this(&args)?;
    let sub = sub_arg(&args[1])?;
    let len = data.len();
    let start = bound(args.get(2), len, 0)?;
    let end = bound(args.get(3), len, len)?;
    if start > len || end < start {
        return Ok(Value::Int(0));
    }
    let window = &data[start..end];
    if sub.is_empty() {
        return Ok(Value::Int(window.len() as i64 + 1));
    }
    let mut n = 0i64;
    let mut i = 0usize;
    while let Some(p) = find_from(window, &sub, i) {
        n += 1;
        i = p + sub.len();
    }
    Ok(Value::Int(n))
}

pub(super) fn replace(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let data = this(&args)?;
    let slots = bind("replace", args[1..].to_vec(), kw, &["old", "new", "count"], 2)?;
    let (Some(old), Some(new)) = (&slots[0], &slots[1]) else {
        return Err(type_error("replace() missing required arguments"));
    };
    let old = want_bytes(old)?;
    let new = want_bytes(new)?;
    let limit = match &slots[2] {
        None => -1,
        Some(v) => want_int(v)?,
    };
    let mut out: Vec<u8> = Vec::with_capacity(data.len());
    let mut done = 0i64;
    if old.is_empty() {
        for &b in data.iter() {
            if limit < 0 || done < limit {
                out.extend_from_slice(&new);
                done += 1;
            }
            out.push(b);
        }
        if limit < 0 || done < limit {
            out.extend_from_slice(&new);
        }
        return Ok(Value::bytes(out));
    }
    let mut i = 0usize;
    while limit < 0 || done < limit {
        match find_from(&data, &old, i) {
            Some(p) => {
                out.extend_from_slice(&data[i..p]);
                out.extend_from_slice(&new);
                i = p + old.len();
                done += 1;
            }
            None => break,
        }
    }
    out.extend_from_slice(&data[i..]);
    Ok(Value::bytes(out))
}

pub(super) fn join(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("join", &kw)?;
    argc("join", &args[1..], 1, 1)?;
    let sep = this(&args)?;
    let items = iterate(&args[1])?;
    let mut out: Vec<u8> = Vec::new();
    for (i, it) in items.iter().enumerate() {
        match it {
            Value::Bytes(_) | Value::ByteArray(_) => {
                if i > 0 {
                    out.extend_from_slice(&sep);
                }
                out.extend_from_slice(&want_bytes(it)?);
            }
            other => {
                return Err(type_error(format!(
                    "sequence item {i}: expected a bytes-like object, {} found",
                    other.type_name()
                )))
            }
        }
    }
    Ok(Value::bytes(out))
}

pub(super) fn upper(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("upper", &kw)?;
    argc("upper", &args[1..], 0, 0)?;
    Ok(Value::bytes(this(&args)?.to_ascii_uppercase()))
}

pub(super) fn lower(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("lower", &kw)?;
    argc("lower", &args[1..], 0, 0)?;
    Ok(Value::bytes(this(&args)?.to_ascii_lowercase()))
}

fn rfind_impl(fname: &str, args: &[Value], kw: &Kw) -> PyResult<Option<usize>> {
    nokw(fname, kw)?;
    argc(fname, &args[1..], 1, 3)?;
    let data = this(args)?;
    let sub = sub_arg(&args[1])?;
    let len = data.len();
    let start = bound(args.get(2), len, 0)?;
    let end = bound(args.get(3), len, len)?;
    if start > len || end < start || end - start < sub.len() {
        return Ok(None);
    }
    let window = &data[start..end];
    let mut i = window.len() - sub.len();
    loop {
        if &window[i..i + sub.len()] == sub.as_slice() {
            return Ok(Some(start + i));
        }
        if i == 0 {
            return Ok(None);
        }
        i -= 1;
    }
}

fn rfind(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    Ok(Value::Int(rfind_impl("rfind", &args, &kw)?.map_or(-1, |p| p as i64)))
}

fn rindex(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    match rfind_impl("rindex", &args, &kw)? {
        Some(p) => Ok(Value::Int(p as i64)),
        None => Err(exc("ValueError", "subsection not found")),
    }
}

fn index(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    match find(vm, args, kw)? {
        Value::Int(-1) => Err(exc("ValueError", "subsection not found")),
        other => Ok(other),
    }
}

fn partition_impl(fname: &str, args: Vec<Value>, kw: Kw, last: bool) -> PyResult<Value> {
    nokw(fname, &kw)?;
    argc(fname, &args[1..], 1, 1)?;
    let data = this(&args)?;
    let sep = want_bytes(&args[1])?;
    if sep.is_empty() {
        return Err(exc("ValueError", "empty separator"));
    }
    let at = if last {
        (0..=data.len().saturating_sub(sep.len())).rev().find(|&i| data.len() >= sep.len() && data[i..i + sep.len()] == sep[..])
    } else {
        find_from(&data, &sep, 0)
    };
    Ok(match at {
        Some(p) => Value::tuple(vec![
            Value::bytes(data[..p].to_vec()),
            Value::bytes(sep.to_vec()),
            Value::bytes(data[p + sep.len()..].to_vec()),
        ]),
        None if last => Value::tuple(vec![Value::bytes(Vec::new()), Value::bytes(Vec::new()), Value::bytes(data.to_vec())]),
        None => Value::tuple(vec![Value::bytes(data.to_vec()), Value::bytes(Vec::new()), Value::bytes(Vec::new())]),
    })
}

pub(super) fn partition(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    partition_impl("partition", args, kw, false)
}

pub(super) fn rpartition(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    partition_impl("rpartition", args, kw, true)
}

pub(super) fn rsplit(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let data = this(&args)?;
    let slots = bind("rsplit", args[1..].to_vec(), kw, &["sep", "maxsplit"], 0)?;
    let maxsplit = match &slots[1] {
        None => -1,
        Some(v) => want_int(v)?,
    };
    if maxsplit < 0 {
        let mut fwd = vec![Value::Bytes(data.clone())];
        if let Some(s) = &slots[0] {
            fwd.push(s.clone());
        }
        return split(vm, fwd, Vec::new());
    }
    let mut parts: Vec<Value> = Vec::new();
    let mut end = data.len();
    let mut count = 0i64;
    match &slots[0] {
        None | Some(Value::None) => {
            while count < maxsplit {
                while end > 0 && is_ws(data[end - 1]) {
                    end -= 1;
                }
                if end == 0 {
                    break;
                }
                let mut s = end;
                while s > 0 && !is_ws(data[s - 1]) {
                    s -= 1;
                }
                parts.push(Value::bytes(data[s..end].to_vec()));
                end = s;
                count += 1;
            }
            while end > 0 && is_ws(data[end - 1]) {
                end -= 1;
            }
            if end > 0 {
                parts.push(Value::bytes(data[..end].to_vec()));
            }
        }
        Some(sep) => {
            let sep = want_bytes(sep)?;
            if sep.is_empty() {
                return Err(exc("ValueError", "empty separator"));
            }
            while count < maxsplit && end >= sep.len() {
                match (0..=end - sep.len()).rev().find(|&i| data[i..i + sep.len()] == sep[..]) {
                    Some(p) => {
                        parts.push(Value::bytes(data[p + sep.len()..end].to_vec()));
                        end = p;
                        count += 1;
                    }
                    None => break,
                }
            }
            parts.push(Value::bytes(data[..end].to_vec()));
        }
    }
    parts.reverse();
    Ok(Value::list(parts))
}

pub(super) fn splitlines(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let data = this(&args)?;
    let slots = bind("splitlines", args[1..].to_vec(), kw, &["keepends"], 0)?;
    let keep = slots[0].as_ref().is_some_and(Value::is_true);
    let mut out = Vec::new();
    let mut i = 0;
    let mut start = 0;
    while i < data.len() {
        let c = data[i];
        if c == b'\n' || c == b'\r' {
            let mut j = i + 1;
            if c == b'\r' && j < data.len() && data[j] == b'\n' {
                j += 1;
            }
            out.push(Value::bytes(data[start..if keep { j } else { i }].to_vec()));
            start = j;
            i = j;
        } else {
            i += 1;
        }
    }
    if start < data.len() {
        out.push(Value::bytes(data[start..].to_vec()));
    }
    Ok(Value::list(out))
}

pub(super) fn removeprefix(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("removeprefix", &kw)?;
    argc("removeprefix", &args[1..], 1, 1)?;
    let data = this(&args)?;
    let p = want_bytes(&args[1])?;
    Ok(Value::bytes(if data.starts_with(&p) { data[p.len()..].to_vec() } else { data.to_vec() }))
}

pub(super) fn removesuffix(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("removesuffix", &kw)?;
    argc("removesuffix", &args[1..], 1, 1)?;
    let data = this(&args)?;
    let p = want_bytes(&args[1])?;
    Ok(Value::bytes(if !p.is_empty() && data.ends_with(&p) { data[..data.len() - p.len()].to_vec() } else { data.to_vec() }))
}

/// `center`/`ljust`/`rjust`: 0 esquerda (preenche à direita), 1 direita, 2 centro.
fn justify(fname: &str, args: Vec<Value>, kw: Kw, mode: u8) -> PyResult<Value> {
    nokw(fname, &kw)?;
    argc(fname, &args[1..], 1, 2)?;
    let data = this(&args)?;
    let width = want_int(&args[1])?.max(0) as usize;
    let fill = match args.get(2) {
        None => b' ',
        Some(v @ (Value::Bytes(_) | Value::ByteArray(_))) if v.bytes_like().is_some_and(|b| b.len() == 1) => {
            v.bytes_like().map_or(b' ', |b| b[0])
        }
        Some(_) => return Err(type_error(format!("{fname}() argument 2 must be a byte string of length 1"))),
    };
    if width <= data.len() {
        return Ok(Value::bytes(data.to_vec()));
    }
    let pad = width - data.len();
    let left = match mode {
        0 => 0,
        1 => pad,
        _ => pad / 2 + (pad & width & 1),
    };
    let mut out = vec![fill; left];
    out.extend_from_slice(&data);
    out.resize(width, fill);
    Ok(Value::bytes(out))
}

pub(super) fn ljust(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    justify("ljust", args, kw, 0)
}

pub(super) fn rjust(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    justify("rjust", args, kw, 1)
}

pub(super) fn center(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    justify("center", args, kw, 2)
}

pub(super) fn zfill(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("zfill", &kw)?;
    argc("zfill", &args[1..], 1, 1)?;
    let data = this(&args)?;
    let width = want_int(&args[1])?.max(0) as usize;
    if width <= data.len() {
        return Ok(Value::bytes(data.to_vec()));
    }
    let pad = width - data.len();
    let (sign, rest) = match data.first() {
        Some(b'+' | b'-') => (&data[..1], &data[1..]),
        _ => (&data[..0], &data[..]),
    };
    let mut out = sign.to_vec();
    out.extend(std::iter::repeat_n(b'0', pad));
    out.extend_from_slice(rest);
    Ok(Value::bytes(out))
}

fn predicate(fname: &str, args: &[Value], kw: &Kw, f: fn(&[u8]) -> bool) -> PyResult<Value> {
    nokw(fname, kw)?;
    argc(fname, &args[1..], 0, 0)?;
    Ok(Value::Bool(f(&this(args)?)))
}

fn isalpha(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    predicate("isalpha", &args, &kw, |d| !d.is_empty() && d.iter().all(u8::is_ascii_alphabetic))
}

fn isdigit(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    predicate("isdigit", &args, &kw, |d| !d.is_empty() && d.iter().all(u8::is_ascii_digit))
}

fn isalnum(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    predicate("isalnum", &args, &kw, |d| !d.is_empty() && d.iter().all(u8::is_ascii_alphanumeric))
}

fn isspace(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    predicate("isspace", &args, &kw, |d| !d.is_empty() && d.iter().all(|&b| is_ws(b)))
}

fn isupper(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    predicate("isupper", &args, &kw, |d| d.iter().any(u8::is_ascii_uppercase) && !d.iter().any(u8::is_ascii_lowercase))
}

fn islower(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    predicate("islower", &args, &kw, |d| d.iter().any(u8::is_ascii_lowercase) && !d.iter().any(u8::is_ascii_uppercase))
}

fn istitle(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    predicate("istitle", &args, &kw, |d| {
        let mut prev_cased = false;
        let mut any = false;
        for b in d {
            if b.is_ascii_uppercase() == prev_cased && (b.is_ascii_uppercase() || b.is_ascii_lowercase()) {
                return false;
            }
            prev_cased = b.is_ascii_alphabetic();
            any |= prev_cased;
        }
        any
    })
}

fn isascii(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    predicate("isascii", &args, &kw, |d| d.is_ascii())
}

pub(super) fn swapcase(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("swapcase", &kw)?;
    argc("swapcase", &args[1..], 0, 0)?;
    let d = this(&args)?;
    Ok(Value::bytes(
        d.iter()
            .map(|&b| if b.is_ascii_lowercase() { b.to_ascii_uppercase() } else { b.to_ascii_lowercase() })
            .collect::<Vec<u8>>(),
    ))
}

pub(super) fn capitalize(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("capitalize", &kw)?;
    argc("capitalize", &args[1..], 0, 0)?;
    let d = this(&args)?;
    Ok(Value::bytes(
        d.iter().enumerate().map(|(i, b)| if i == 0 { b.to_ascii_uppercase() } else { b.to_ascii_lowercase() }).collect::<Vec<u8>>(),
    ))
}

pub(super) fn title(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("title", &kw)?;
    argc("title", &args[1..], 0, 0)?;
    let d = this(&args)?;
    let mut prev_alpha = false;
    let out: Vec<u8> = d
        .iter()
        .map(|&b| {
            let r = if prev_alpha { b.to_ascii_lowercase() } else { b.to_ascii_uppercase() };
            prev_alpha = b.is_ascii_alphabetic();
            r
        })
        .collect();
    Ok(Value::bytes(out))
}

pub(super) fn expandtabs(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let data = this(&args)?;
    let slots = bind("expandtabs", args[1..].to_vec(), kw, &["tabsize"], 0)?;
    let tab = int_or(slots[0].as_ref(), 8)?.max(0) as usize;
    let mut out = Vec::new();
    let mut col = 0usize;
    for &b in data.iter() {
        match b {
            b'\t' => {
                if tab > 0 {
                    let n = tab - col % tab;
                    out.extend(std::iter::repeat_n(b' ', n));
                    col += n;
                }
            }
            b'\n' | b'\r' => {
                out.push(b);
                col = 0;
            }
            _ => {
                out.push(b);
                col += 1;
            }
        }
    }
    Ok(Value::bytes(out))
}

pub(super) fn translate(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let data = this(&args)?;
    let slots = bind("translate", args[1..].to_vec(), kw, &["table", "delete"], 1)?;
    let table: Option<Rc<[u8]>> = match &slots[0] {
        Some(Value::None) | None => None,
        Some(v) => {
            let t = want_bytes(v)?;
            if t.len() != 256 {
                return Err(exc("ValueError", "translation table must be 256 characters long"));
            }
            Some(t)
        }
    };
    let delete: Rc<[u8]> = match &slots[1] {
        Some(v) => want_bytes(v)?,
        None => Rc::from(Vec::new()),
    };
    let out: Vec<u8> = data
        .iter()
        .filter(|b| !delete.contains(b))
        .map(|&b| table.as_ref().map_or(b, |t| t[b as usize]))
        .collect();
    Ok(Value::bytes(out))
}

pub const TABLE: &[(&str, NativeFnPtr)] = &[
    ("rfind", rfind),
    ("rindex", rindex),
    ("index", index),
    ("partition", partition),
    ("rpartition", rpartition),
    ("rsplit", rsplit),
    ("splitlines", splitlines),
    ("removeprefix", removeprefix),
    ("removesuffix", removesuffix),
    ("ljust", ljust),
    ("rjust", rjust),
    ("center", center),
    ("zfill", zfill),
    ("isalpha", isalpha),
    ("isdigit", isdigit),
    ("isalnum", isalnum),
    ("isspace", isspace),
    ("isupper", isupper),
    ("islower", islower),
    ("istitle", istitle),
    ("isascii", isascii),
    ("swapcase", swapcase),
    ("capitalize", capitalize),
    ("title", title),
    ("expandtabs", expandtabs),
    ("translate", translate),
    ("decode", decode),
    ("hex", hex),
    ("split", split),
    ("strip", strip),
    ("lstrip", lstrip),
    ("rstrip", rstrip),
    ("startswith", startswith),
    ("endswith", endswith),
    ("find", find),
    ("replace", replace),
    ("join", join),
    ("upper", upper),
    ("lower", lower),
    ("count", count),
];

#[cfg(test)]
mod tests {
    fn run(src: &str) -> (String, String, i32) {
        let o = crate::run_source(src);
        (String::from_utf8(o.stdout).unwrap(), o.stderr, o.status)
    }

    #[test]
    fn decode_basic() {
        let (out, _, st) = run("b = b'caf\\xc3\\xa9'\nprint(b.decode())\nprint(b.decode('utf-8'))\nprint(b.decode('latin-1'))\nprint(b'abc'.decode('ascii'))\n");
        assert_eq!(st, 0);
        assert_eq!(out, "café\ncafé\ncafÃ©\nabc\n");
    }

    #[test]
    fn decode_errors_modes() {
        let (out, _, _) = run("b = b'a\\xffb'\nprint(b.decode('utf-8', 'replace'))\nprint(b.decode(errors='ignore'))\n");
        assert_eq!(out, "a\u{fffd}b\nab\n");
    }

    #[test]
    fn decode_strict_error() {
        let (_, err, st) = run("b'a\\xffb'.decode()\n");
        assert_ne!(st, 0);
        assert!(
            err.contains("UnicodeDecodeError: 'utf-8' codec can't decode byte 0xff in position 1: invalid start byte"),
            "{err}"
        );
        let (_, err, _) = run("b'\\x80'.decode('ascii')\n");
        assert!(
            err.contains("UnicodeDecodeError: 'ascii' codec can't decode byte 0x80 in position 0: ordinal not in range(128)"),
            "{err}"
        );
    }

    #[test]
    fn decode_unknown_encoding() {
        let (_, err, st) = run("b'a'.decode('nope')\n");
        assert_ne!(st, 0);
        assert!(err.contains("LookupError: unknown encoding: nope"), "{err}");
    }

    #[test]
    fn hex_split_strip() {
        let (out, _, _) = run("b = b'\\x01\\xab'\nprint(b.hex())\nprint(b'a,b,,c'.split(b','))\nprint(b'  a  b '.split())\nprint(b'a b c'.split(None, 1))\nprint(b'  x '.strip(), b'xxaxx'.strip(b'x'))\n");
        assert_eq!(out, "01ab\n[b'a', b'b', b'', b'c']\n[b'a', b'b']\n[b'a', b'b c']\nb'x' b'a'\n");
    }

    #[test]
    fn startswith_endswith_find_count() {
        let (out, _, _) = run("b = b'hello world'\nprint(b.startswith(b'hello'), b.endswith(b'world'), b.startswith((b'x', b'he')))\nprint(b.find(b'o'), b.find(b'o', 5), b.find(b'zz'), b.count(b'o'), b.count(b'l'))\n");
        assert_eq!(out, "True True True\n4 7 -1 2 3\n");
    }

    #[test]
    fn replace_join_case() {
        let (out, _, _) = run("print(b'aXbXc'.replace(b'X', b'-'))\nprint(b'aXbXc'.replace(b'X', b'-', 1))\nprint(b','.join([b'a', b'b', b'c']))\nprint(b'Ab'.upper(), b'Ab'.lower())\n");
        assert_eq!(out, "b'a-b-c'\nb'a-bXc'\nb'a,b,c'\nb'AB' b'ab'\n");
    }

    #[test]
    fn join_type_error() {
        let (_, err, st) = run("b','.join(['a'])\n");
        assert_ne!(st, 0);
        assert!(err.contains("TypeError: sequence item 0: expected a bytes-like object, str found"), "{err}");
    }
}
