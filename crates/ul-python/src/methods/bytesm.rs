//! Métodos de `bytes` (`Objects/bytesobject.c`).
//!
//! Só `bytes` imutável existe no núcleo (sem `bytearray`/`memoryview`), então "bytes-like" aqui é
//! `Value::Bytes`. Maiúsculas e minúsculas são só ASCII, como no CPython.

use std::rc::Rc;

use crate::native_util::{bind, want_int};
use crate::object::{Kw, NativeFnPtr, Value};
use crate::vm::{exc, iterate, type_error, PyResult, Vm};

fn this(args: &[Value]) -> PyResult<Rc<[u8]>> {
    match args.first() {
        Some(Value::Bytes(b)) => Ok(b.clone()),
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
    match v {
        Value::Bytes(b) => Ok(b.clone()),
        other => Err(type_error(format!("a bytes-like object is required, not '{}'", other.type_name()))),
    }
}

/// Argumento `sub` de `find`/`count`: bytes ou um inteiro de 0 a 255.
fn sub_arg(v: &Value) -> PyResult<Vec<u8>> {
    match v {
        Value::Bytes(b) => Ok(b.to_vec()),
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
}

fn codec_of(name: &str) -> Option<Codec> {
    let norm = name.trim().to_ascii_lowercase().replace('_', "-");
    match norm.as_str() {
        "utf-8" | "utf8" | "u8" | "utf" => Some(Codec::Utf8),
        "ascii" | "us-ascii" | "646" => Some(Codec::Ascii),
        "latin-1" | "latin1" | "iso-8859-1" | "iso8859-1" | "8859" | "cp819" | "latin" | "l1" => Some(Codec::Latin1),
        _ => None,
    }
}

fn decode_error(codec: &str, bad_start: usize, bad_len: usize, byte: u8, reason: &str) -> crate::vm::PyException {
    let msg = if bad_len == 1 {
        format!("'{codec}' codec can't decode byte 0x{byte:02x} in position {bad_start}: {reason}")
    } else {
        format!("'{codec}' codec can't decode bytes in position {bad_start}-{}: {reason}", bad_start + bad_len - 1)
    };
    exc("UnicodeDecodeError", msg)
}

fn decode_utf8(data: &[u8], errors: &str) -> PyResult<String> {
    let mut out = String::new();
    let mut pos = 0usize;
    while pos < data.len() {
        match std::str::from_utf8(&data[pos..]) {
            Ok(s) => {
                out.push_str(s);
                break;
            }
            Err(e) => {
                let valid = e.valid_up_to();
                out.push_str(std::str::from_utf8(&data[pos..pos + valid]).unwrap_or(""));
                let bad_start = pos + valid;
                let first = data[bad_start];
                let (bad_len, reason) = match e.error_len() {
                    Some(n) => {
                        let r = if (0x80..=0xc1).contains(&first) || first >= 0xf5 {
                            "invalid start byte"
                        } else {
                            "invalid continuation byte"
                        };
                        (n, r)
                    }
                    None => (data.len() - bad_start, "unexpected end of data"),
                };
                match errors {
                    "ignore" => {}
                    "replace" => out.push('\u{fffd}'),
                    _ => return Err(decode_error("utf-8", bad_start, bad_len, first, reason)),
                }
                pos = bad_start + bad_len;
            }
        }
    }
    Ok(out)
}

fn decode_ascii(data: &[u8], errors: &str) -> PyResult<String> {
    let mut out = String::with_capacity(data.len());
    for (i, &b) in data.iter().enumerate() {
        if b < 0x80 {
            out.push(b as char);
            continue;
        }
        match errors {
            "ignore" => {}
            "replace" => out.push('\u{fffd}'),
            _ => return Err(decode_error("ascii", i, 1, b, "ordinal not in range(128)")),
        }
    }
    Ok(out)
}

fn decode(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let data = this(&args)?;
    let slots = bind("decode", args[1..].to_vec(), kw, &["encoding", "errors"], 0)?;
    let text_arg = |slot: &Option<Value>, what: &str, default: &str| -> PyResult<String> {
        match slot {
            None => Ok(default.to_string()),
            Some(Value::Str(s)) => Ok(s.as_str().to_string()),
            Some(other) => {
                Err(type_error(format!("decode() argument '{what}' must be str, not {}", other.type_name())))
            }
        }
    };
    let encoding = text_arg(&slots[0], "encoding", "utf-8")?;
    let errors = text_arg(&slots[1], "errors", "strict")?;
    let Some(codec) = codec_of(&encoding) else {
        return Err(exc("LookupError", format!("unknown encoding: {encoding}")));
    };
    let text = match codec {
        Codec::Utf8 => decode_utf8(&data, &errors)?,
        Codec::Ascii => decode_ascii(&data, &errors)?,
        Codec::Latin1 => data.iter().map(|&b| b as char).collect(),
    };
    Ok(Value::str(text))
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

fn split(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
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

fn strip(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    strip_impl("strip", args, kw, true, true)
}

fn lstrip(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    strip_impl("lstrip", args, kw, true, false)
}

fn rstrip(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
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
        Value::Bytes(b) => vec![b.clone()],
        Value::Tuple(items) => {
            let mut v = Vec::new();
            for it in items.iter() {
                match it {
                    Value::Bytes(b) => v.push(b.clone()),
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

fn replace(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
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

fn join(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("join", &kw)?;
    argc("join", &args[1..], 1, 1)?;
    let sep = this(&args)?;
    let items = iterate(&args[1])?;
    let mut out: Vec<u8> = Vec::new();
    for (i, it) in items.iter().enumerate() {
        match it {
            Value::Bytes(b) => {
                if i > 0 {
                    out.extend_from_slice(&sep);
                }
                out.extend_from_slice(b);
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

fn upper(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("upper", &kw)?;
    argc("upper", &args[1..], 0, 0)?;
    Ok(Value::bytes(this(&args)?.to_ascii_uppercase()))
}

fn lower(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("lower", &kw)?;
    argc("lower", &args[1..], 0, 0)?;
    Ok(Value::bytes(this(&args)?.to_ascii_lowercase()))
}

pub const TABLE: &[(&str, NativeFnPtr)] = &[
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
