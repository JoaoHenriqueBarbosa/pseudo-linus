//! Módulo `struct` do CPython 3.13 (arquivo `pystruct.rs`, porque `struct` é palavra reservada em
//! Rust): `pack`, `unpack`, `pack_into`, `unpack_from`, `iter_unpack` (devolve lista) e `calcsize`.
//!
//! Ordem de bytes e alinhamento: `@` (nativo, little endian e alinhado, `l`/`L` com 8 bytes), `=`
//! (nativo sem alinhamento, tamanhos padrão), `<`, `>` e `!`. Códigos: `x c b B ? h H i I l L q Q n
//! N e f d s p P`, com contagens. Limitações: sem inteiros arbitrários, então `Q`, `N` e `P` acima
//! de `i64::MAX` levantam `OverflowError` no `unpack`. Fica de fora a classe `Struct`.

use std::rc::Rc;

use crate::modules::ModuleBuilder;
use crate::native_util::{bind, exactly, no_kwargs, want_int};
use crate::object::{Kw, ModuleObj, Value};
use crate::vm::{exc, type_error, PyException, PyResult, Vm};

fn struct_error(msg: impl Into<String>) -> PyException {
    exc("struct.error", msg)
}

#[derive(Clone, Copy, Debug)]
struct Mode {
    /// Tamanhos nativos (`l` com 8 bytes, `n`/`N`/`P` válidos).
    native: bool,
    /// Alinha cada item ao próprio tamanho (só no `@`).
    align: bool,
    big: bool,
}

#[derive(Clone, Copy, Debug)]
struct Item {
    code: char,
    count: usize,
}

fn code_size(code: char, native: bool) -> Option<usize> {
    match code {
        'x' | 'c' | 'b' | 'B' | '?' | 's' | 'p' => Some(1),
        'h' | 'H' | 'e' => Some(2),
        'i' | 'I' | 'f' => Some(4),
        'l' | 'L' => Some(if native { 8 } else { 4 }),
        'q' | 'Q' | 'd' => Some(8),
        'n' | 'N' | 'P' if native => Some(8),
        _ => None,
    }
}

fn is_signed(code: char) -> bool {
    matches!(code, 'b' | 'h' | 'i' | 'l' | 'q' | 'n')
}

fn is_int_code(code: char) -> bool {
    matches!(code, 'b' | 'B' | 'h' | 'H' | 'i' | 'I' | 'l' | 'L' | 'q' | 'Q' | 'n' | 'N' | 'P')
}

fn parse_format(fmt: &str) -> PyResult<(Mode, Vec<Item>)> {
    let chars: Vec<char> = fmt.chars().collect();
    let mut i = 0;
    let mut mode = Mode { native: true, align: true, big: false };
    match chars.first() {
        Some('@') => i = 1,
        Some('=') => {
            mode = Mode { native: false, align: false, big: false };
            i = 1;
        }
        Some('<') => {
            mode = Mode { native: false, align: false, big: false };
            i = 1;
        }
        Some('>') | Some('!') => {
            mode = Mode { native: false, align: false, big: true };
            i = 1;
        }
        _ => {}
    }
    let mut items = Vec::new();
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        if c.is_whitespace() {
            continue;
        }
        let mut count: Option<usize> = None;
        let mut ch = c;
        if ch.is_ascii_digit() {
            let mut n: usize = 0;
            loop {
                let d = ch.to_digit(10).unwrap() as usize;
                n = n.checked_mul(10).and_then(|x| x.checked_add(d)).ok_or_else(|| struct_error("total struct size too long"))?;
                if i < chars.len() && chars[i].is_ascii_digit() {
                    ch = chars[i];
                    i += 1;
                } else {
                    break;
                }
            }
            count = Some(n);
            if i >= chars.len() {
                return Err(struct_error("repeat count given without format specifier"));
            }
            ch = chars[i];
            i += 1;
        }
        if code_size(ch, mode.native).is_none() {
            return Err(struct_error("bad char in struct format"));
        }
        items.push(Item { code: ch, count: count.unwrap_or(1) });
    }
    Ok((mode, items))
}

fn align_up(n: usize, a: usize) -> usize {
    if a <= 1 {
        n
    } else {
        n.div_ceil(a) * a
    }
}

fn calc_size(mode: Mode, items: &[Item]) -> PyResult<usize> {
    let too_long = || struct_error("total struct size too long");
    let mut off: usize = 0;
    for it in items {
        match it.code {
            'x' | 's' | 'p' => off = off.checked_add(it.count).ok_or_else(too_long)?,
            code => {
                let size = code_size(code, mode.native).unwrap_or(1);
                if mode.align {
                    off = align_up(off, size);
                }
                off = it.count.checked_mul(size).and_then(|b| off.checked_add(b)).ok_or_else(too_long)?;
            }
        }
    }
    Ok(off)
}

fn fmt_of(v: &Value) -> PyResult<String> {
    match v {
        Value::Str(s) => Ok(s.as_str().to_string()),
        Value::Bytes(b) => Ok(b.iter().map(|&c| c as char).collect()),
        other => Err(type_error(format!(
            "Struct() argument 1 must be a str or bytes object, not {}",
            other.type_name()
        ))),
    }
}

fn to_half(x: f64) -> Option<u16> {
    let bits = x.to_bits();
    let sign = ((bits >> 63) as u16) << 15;
    if x.is_nan() {
        return Some(sign | 0x7e00);
    }
    if x.is_infinite() {
        return Some(sign | 0x7c00);
    }
    let a = x.abs();
    if a == 0.0 {
        return Some(sign);
    }
    let exp = ((bits >> 52) & 0x7ff) as i32 - 1023;
    if exp >= -14 {
        let mant52 = bits & ((1u64 << 52) - 1);
        let rem = mant52 & ((1u64 << 42) - 1);
        let half = 1u64 << 41;
        let mut m10 = mant52 >> 42;
        if rem > half || (rem == half && m10 & 1 == 1) {
            m10 += 1;
        }
        let mut he = exp + 15;
        if m10 == 1024 {
            m10 = 0;
            he += 1;
        }
        if he >= 31 {
            return None;
        }
        Some(sign | ((he as u16) << 10) | m10 as u16)
    } else {
        let r = a * 16_777_216.0;
        let fl = r.floor();
        let diff = r - fl;
        let mut n = fl as u32;
        if diff > 0.5 || (diff == 0.5 && n % 2 == 1) {
            n += 1;
        }
        Some(sign | n as u16)
    }
}

fn from_half(h: u16) -> f64 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let e = i32::from((h >> 10) & 0x1f);
    let m = f64::from(h & 0x3ff);
    match e {
        0 => sign * m * 2f64.powi(-24),
        31 => {
            if m == 0.0 {
                sign * f64::INFINITY
            } else {
                f64::NAN
            }
        }
        _ => sign * (1.0 + m / 1024.0) * 2f64.powi(e - 15),
    }
}

fn put_bytes(out: &mut Vec<u8>, le: &[u8], big: bool) {
    if big {
        out.extend(le.iter().rev());
    } else {
        out.extend_from_slice(le);
    }
}

fn want_pack_int(v: &Value) -> PyResult<i128> {
    match v {
        Value::Int(i) => Ok(i128::from(*i)),
        Value::Bool(b) => Ok(i128::from(*b)),
        _ => Err(struct_error("required argument is not an integer")),
    }
}

fn want_pack_float(v: &Value) -> PyResult<f64> {
    match v {
        Value::Float(x) => Ok(*x),
        Value::Int(i) => Ok(*i as f64),
        Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        _ => Err(struct_error("required argument is not a float")),
    }
}

fn pack_int(out: &mut Vec<u8>, code: char, v: i128, size: usize, big: bool) -> PyResult<()> {
    let bits = (size * 8) as u32;
    let (lo, hi): (i128, i128) = if is_signed(code) {
        (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1)
    } else {
        (0, (1i128 << bits) - 1)
    };
    if v < lo || v > hi {
        if size < 8 {
            return Err(struct_error(format!("'{code}' format requires {lo} <= number <= {hi}")));
        }
        return Err(struct_error("argument out of range"));
    }
    let le = (v as u128).to_le_bytes();
    put_bytes(out, &le[..size], big);
    Ok(())
}

/// `struct.pack(fmt, *values)`.
pub fn pack_values(fmt: &str, vals: &[Value]) -> PyResult<Vec<u8>> {
    let (mode, items) = parse_format(fmt)?;
    let need: usize = items
        .iter()
        .map(|it| match it.code {
            'x' => 0,
            's' | 'p' => 1,
            _ => it.count,
        })
        .sum();
    if vals.len() != need {
        return Err(struct_error(format!("pack expected {need} items for packing (got {})", vals.len())));
    }
    let mut out: Vec<u8> = Vec::new();
    let mut vi = 0;
    for it in &items {
        match it.code {
            'x' => out.resize(out.len() + it.count, 0),
            's' | 'p' => {
                let Some(data) = vals[vi].bytes_like() else {
                    return Err(struct_error(format!("argument for '{}' must be a bytes object", it.code)));
                };
                vi += 1;
                let start = out.len();
                if it.code == 's' {
                    let n = data.len().min(it.count);
                    out.extend_from_slice(&data[..n]);
                } else if it.count > 0 {
                    let n = data.len().min(it.count - 1).min(255);
                    out.push(n as u8);
                    out.extend_from_slice(&data[..n]);
                }
                out.resize(start + it.count, 0);
            }
            code => {
                let size = code_size(code, mode.native).unwrap_or(1);
                if mode.align {
                    let target = align_up(out.len(), size);
                    out.resize(target, 0);
                }
                for _ in 0..it.count {
                    let v = &vals[vi];
                    vi += 1;
                    match code {
                        'c' => match v {
                            Value::Bytes(_) | Value::ByteArray(_) if v.bytes_like().is_some_and(|b| b.len() == 1) => {
                                out.push(v.bytes_like().map_or(0, |b| b[0]))
                            }
                            _ => return Err(struct_error("char format requires a bytes object of length 1")),
                        },
                        '?' => out.push(u8::from(v.is_true())),
                        'f' => {
                            let x = want_pack_float(v)?;
                            let r = x as f32;
                            if r.is_infinite() && x.is_finite() {
                                return Err(exc("OverflowError", "float too large to pack with f format"));
                            }
                            put_bytes(&mut out, &r.to_bits().to_le_bytes(), mode.big);
                        }
                        'd' => {
                            let x = want_pack_float(v)?;
                            put_bytes(&mut out, &x.to_bits().to_le_bytes(), mode.big);
                        }
                        'e' => {
                            let x = want_pack_float(v)?;
                            let Some(h) = to_half(x) else {
                                return Err(exc("OverflowError", "float too large to pack with e format"));
                            };
                            put_bytes(&mut out, &h.to_le_bytes(), mode.big);
                        }
                        c if is_int_code(c) => {
                            let n = want_pack_int(v)?;
                            pack_int(&mut out, c, n, size, mode.big)?;
                        }
                        _ => return Err(struct_error("bad char in struct format")),
                    }
                }
            }
        }
    }
    Ok(out)
}

fn read_uint(bytes: &[u8], big: bool) -> u128 {
    let mut v: u128 = 0;
    if big {
        for &b in bytes {
            v = (v << 8) | u128::from(b);
        }
    } else {
        for &b in bytes.iter().rev() {
            v = (v << 8) | u128::from(b);
        }
    }
    v
}

fn unpack_items(mode: Mode, items: &[Item], data: &[u8]) -> PyResult<Vec<Value>> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    for it in items {
        match it.code {
            'x' => pos += it.count,
            's' => {
                out.push(Value::bytes(data[pos..pos + it.count].to_vec()));
                pos += it.count;
            }
            'p' => {
                if it.count == 0 {
                    out.push(Value::bytes(Vec::new()));
                } else {
                    let n = (data[pos] as usize).min(it.count - 1);
                    out.push(Value::bytes(data[pos + 1..pos + 1 + n].to_vec()));
                }
                pos += it.count;
            }
            code => {
                let size = code_size(code, mode.native).unwrap_or(1);
                if mode.align {
                    pos = align_up(pos, size);
                }
                for _ in 0..it.count {
                    let chunk = &data[pos..pos + size];
                    pos += size;
                    let v = match code {
                        'c' => Value::bytes(vec![chunk[0]]),
                        '?' => Value::Bool(chunk[0] != 0),
                        'f' => {
                            let bits = read_uint(chunk, mode.big) as u32;
                            Value::Float(f64::from(f32::from_bits(bits)))
                        }
                        'd' => Value::Float(f64::from_bits(read_uint(chunk, mode.big) as u64)),
                        'e' => Value::Float(from_half(read_uint(chunk, mode.big) as u16)),
                        c => {
                            let raw = read_uint(chunk, mode.big);
                            let n: i128 = if is_signed(c) && raw >> (size * 8 - 1) & 1 == 1 {
                                raw as i128 - (1i128 << (size * 8))
                            } else {
                                raw as i128
                            };
                            crate::bigint::norm(num_bigint::BigInt::from(n))
                        }
                    };
                    out.push(v);
                }
            }
        }
    }
    Ok(out)
}

fn want_buffer(v: &Value) -> PyResult<Rc<[u8]>> {
    v.bytes_like()
        .ok_or_else(|| type_error(format!("a bytes-like object is required, not '{}'", v.type_name())))
}

/// `pack_into(format, buffer, offset, v1, v2, ...)`: grava no `bytearray` a partir de `offset`.
fn pack_into(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("pack_into", &kw)?;
    if args.len() < 3 {
        return Err(type_error(format!("pack_into expected at least 3 arguments, got {}", args.len())));
    }
    let fmt = fmt_of(&args[0])?;
    let Value::ByteArray(buf) = &args[1] else {
        return Err(type_error(format!(
            "argument must be read-write bytes-like object, not {}",
            args[1].type_name()
        )));
    };
    let orig = want_int(&args[2])?;
    let packed = pack_values(&fmt, &args[3..])?;
    let len = buf.borrow().len() as i64;
    let off = if orig < 0 { orig + len } else { orig };
    if off < 0 || off > len {
        return Err(struct_error(format!("offset {orig} out of range for {len}-byte buffer")));
    }
    let off = off as usize;
    if buf.borrow().len() - off < packed.len() {
        return Err(struct_error(format!(
            "pack_into requires a buffer of at least {} bytes for packing {} bytes at offset {off} (actual buffer size is {len})",
            packed.len() + off,
            packed.len()
        )));
    }
    buf.borrow_mut()[off..off + packed.len()].copy_from_slice(&packed);
    Ok(Value::None)
}

fn pack(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("pack", &kw)?;
    if args.is_empty() {
        return Err(type_error("pack expected at least 1 argument, got 0"));
    }
    let fmt = fmt_of(&args[0])?;
    Ok(Value::bytes(pack_values(&fmt, &args[1..])?))
}

fn unpack(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("unpack", &kw)?;
    exactly("unpack", &args, 2)?;
    let fmt = fmt_of(&args[0])?;
    let data = want_buffer(&args[1])?;
    let (mode, items) = parse_format(&fmt)?;
    let size = calc_size(mode, &items)?;
    if data.len() != size {
        return Err(struct_error(format!("unpack requires a buffer of {size} bytes")));
    }
    Ok(Value::tuple(unpack_items(mode, &items, &data)?))
}

fn unpack_from(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind("unpack_from", args, kw, &["format", "buffer", "offset"], 2)?;
    let fmt = fmt_of(a[0].as_ref().unwrap())?;
    let data = want_buffer(a[1].as_ref().unwrap())?;
    let orig = match &a[2] {
        Some(v) => want_int(v)?,
        None => 0,
    };
    let (mode, items) = parse_format(&fmt)?;
    let size = calc_size(mode, &items)?;
    let len = data.len() as i64;
    let off = if orig < 0 { orig + len } else { orig };
    if off < 0 || off > len {
        return Err(struct_error(format!("offset {orig} out of range for {len}-byte buffer")));
    }
    let off = off as usize;
    if data.len() - off < size {
        return Err(struct_error(format!(
            "unpack_from requires a buffer of at least {} bytes for unpacking {size} bytes at offset {off} (actual buffer size is {})",
            size + off,
            data.len()
        )));
    }
    Ok(Value::tuple(unpack_items(mode, &items, &data[off..off + size])?))
}

fn iter_unpack(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("iter_unpack", &kw)?;
    exactly("iter_unpack", &args, 2)?;
    let fmt = fmt_of(&args[0])?;
    let data = want_buffer(&args[1])?;
    let (mode, items) = parse_format(&fmt)?;
    let size = calc_size(mode, &items)?;
    if size == 0 {
        return Err(struct_error("cannot iteratively unpack with a struct of length 0"));
    }
    if data.len() % size != 0 {
        return Err(struct_error(format!("iterative unpacking requires a buffer of a multiple of {size} bytes")));
    }
    let mut out = Vec::new();
    for chunk in data.chunks(size) {
        out.push(Value::tuple(unpack_items(mode, &items, chunk)?));
    }
    Ok(Value::list(out))
}

fn calcsize(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("calcsize", &kw)?;
    exactly("calcsize", &args, 1)?;
    let fmt = fmt_of(&args[0])?;
    let (mode, items) = parse_format(&fmt)?;
    Ok(Value::Int(calc_size(mode, &items)? as i64))
}

pub fn build(_vm: &mut Vm) -> Rc<ModuleObj> {
    ModuleBuilder::new("_struct")
        .func("pack", pack)
        .func("unpack", unpack)
        .func("pack_into", pack_into)
        .func("unpack_from", unpack_from)
        .func("iter_unpack", iter_unpack)
        .func("calcsize", calcsize)
        .value("error", Value::Builtin("struct.error"))
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

    fn s(x: &str) -> Value {
        Value::str(x)
    }

    fn b(x: &[u8]) -> Value {
        Value::bytes(x.to_vec())
    }

    fn r(f: NativeFnPtr, args: Vec<Value>) -> String {
        repr(&call(f, args).unwrap())
    }

    fn err(f: NativeFnPtr, args: Vec<Value>) -> String {
        let e = call(f, args).unwrap_err();
        format!("{}: {}", e.kind, e.msg)
    }

    #[test]
    fn pack_integers() {
        assert_eq!(r(pack, vec![s("<I"), Value::Int(1)]), "b'\\x01\\x00\\x00\\x00'");
        assert_eq!(r(pack, vec![s(">H"), Value::Int(258)]), "b'\\x01\\x02'");
        assert_eq!(r(pack, vec![s("<bh"), Value::Int(-1), Value::Int(-2)]), "b'\\xff\\xfe\\xff'");
        assert_eq!(r(pack, vec![s("@bi"), Value::Int(1), Value::Int(2)]), "b'\\x01\\x00\\x00\\x00\\x02\\x00\\x00\\x00'");
        assert_eq!(r(pack, vec![s("<q"), Value::Int(-1)]), "b'\\xff\\xff\\xff\\xff\\xff\\xff\\xff\\xff'");
        assert_eq!(r(pack, vec![s("?"), Value::Int(3)]), "b'\\x01'");
        assert_eq!(r(pack, vec![s("3x")]), "b'\\x00\\x00\\x00'");
    }

    #[test]
    fn pack_strings_and_floats() {
        assert_eq!(r(pack, vec![s("4s"), b(b"ab")]), "b'ab\\x00\\x00'");
        assert_eq!(r(pack, vec![s("2s"), b(b"abcd")]), "b'ab'");
        assert_eq!(r(pack, vec![s("5p"), b(b"abc")]), "b'\\x03abc\\x00'");
        assert_eq!(r(pack, vec![s("c"), b(b"x")]), "b'x'");
        assert_eq!(r(pack, vec![s("<f"), Value::Float(1.0)]), "b'\\x00\\x00\\x80?'");
        assert_eq!(r(pack, vec![s("<e"), Value::Float(1.0)]), "b'\\x00<'");
        assert_eq!(r(pack, vec![s(">e"), Value::Float(1.5)]), "b'>\\x00'");
        assert_eq!(r(pack, vec![s(">d"), Value::Float(1.0)]), "b'?\\xf0\\x00\\x00\\x00\\x00\\x00\\x00'");
    }

    #[test]
    fn unpack_values() {
        assert_eq!(r(unpack, vec![s("<I"), b(b"\x01\x00\x00\x00")]), "(1,)");
        assert_eq!(r(unpack, vec![s(">HH"), b(b"\x00\x01\x00\x02")]), "(1, 2)");
        assert_eq!(r(unpack, vec![s("<b"), b(b"\xff")]), "(-1,)");
        assert_eq!(r(unpack, vec![s("<d"), b(b"\x00\x00\x00\x00\x00\x00\xf8?")]), "(1.5,)");
        assert_eq!(r(unpack, vec![s("<e"), b(b"\x00<")]), "(1.0,)");
        assert_eq!(r(unpack, vec![s("5p"), b(b"\x03abc\x00")]), "(b'abc',)");
        assert_eq!(r(unpack, vec![s("?c2s"), b(b"\x01xyz")]), "(True, b'x', b'yz')");
        assert_eq!(r(unpack, vec![s("@bi"), b(b"\x01\x00\x00\x00\x02\x00\x00\x00")]), "(1, 2)");
        let off = vec![s("<H"), b(b"\x00\x01\x00\x02\x00"), Value::Int(2)];
        let mut vm = Vm::new();
        let v = unpack_from(&mut vm, off, Vec::new()).unwrap();
        assert_eq!(repr(&v), "(512,)");
        assert_eq!(r(iter_unpack, vec![s("<H"), b(b"\x01\x00\x02\x00")]), "[(1,), (2,)]");
    }

    #[test]
    fn sizes() {
        assert_eq!(r(calcsize, vec![s("@bi")]), "8");
        assert_eq!(r(calcsize, vec![s("<bi")]), "5");
        assert_eq!(r(calcsize, vec![s("i")]), "4");
        assert_eq!(r(calcsize, vec![s("2s")]), "2");
        assert_eq!(r(calcsize, vec![s("!hhl")]), "8");
        assert_eq!(r(calcsize, vec![s("@l")]), "8");
        assert_eq!(r(calcsize, vec![s("<l")]), "4");
        assert_eq!(r(calcsize, vec![s("q")]), "8");
        assert_eq!(r(calcsize, vec![s("")]), "0");
        assert_eq!(r(calcsize, vec![s("3x")]), "3");
        assert_eq!(r(calcsize, vec![s("<3h")]), "6");
    }

    #[test]
    fn errors() {
        assert_eq!(err(pack, vec![s("<i")]), "struct.error: pack expected 1 items for packing (got 0)");
        assert_eq!(err(pack, vec![s("b"), Value::Int(200)]), "struct.error: 'b' format requires -128 <= number <= 127");
        assert_eq!(err(pack, vec![s("<H"), Value::Int(-1)]), "struct.error: 'H' format requires 0 <= number <= 65535");
        assert_eq!(err(pack, vec![s("i"), s("x")]), "struct.error: required argument is not an integer");
        assert_eq!(err(pack, vec![s("f"), s("x")]), "struct.error: required argument is not a float");
        assert_eq!(err(unpack, vec![s("<I"), b(b"\x00")]), "struct.error: unpack requires a buffer of 4 bytes");
        assert_eq!(err(calcsize, vec![s("z")]), "struct.error: bad char in struct format");
        assert_eq!(err(calcsize, vec![s("<n")]), "struct.error: bad char in struct format");
        assert_eq!(err(calcsize, vec![s("3")]), "struct.error: repeat count given without format specifier");
        assert_eq!(err(pack, vec![s("c"), b(b"xy")]), "struct.error: char format requires a bytes object of length 1");
        assert_eq!(err(pack, vec![s("<e"), Value::Float(1e10)]), "OverflowError: float too large to pack with e format");
    }

    #[test]
    fn roundtrip() {
        let packed = call(pack, vec![s("<hiqd"), Value::Int(-3), Value::Int(70000), Value::Int(-5_000_000_000), Value::Float(2.5)]).unwrap();
        let un = call(unpack, vec![s("<hiqd"), packed]).unwrap();
        assert_eq!(repr(&un), "(-3, 70000, -5000000000, 2.5)");
    }
}
