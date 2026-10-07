//! Métodos de `int`, `bool` e `float` (`Objects/longobject.c`, `Objects/floatobject.c`).
//!
//! As três classes dividem uma tabela só (o `lookup` não separa por tipo), então um método que o
//! CPython não tem num dos tipos levanta o `AttributeError` com o texto do CPython. `bool` herda
//! os métodos de `int`.

use crate::native_util::{bind, want_int};
use crate::object::{Kw, NativeFnPtr, Value};
use crate::vm::{exc, type_error, PyResult, Vm};

fn nokw(fname: &str, kw: &Kw) -> PyResult<()> {
    if kw.is_empty() {
        Ok(())
    } else {
        Err(type_error(format!("{fname}() takes no keyword arguments")))
    }
}

fn noargs(fname: &str, rest: &[Value]) -> PyResult<()> {
    if rest.is_empty() {
        Ok(())
    } else {
        Err(type_error(format!("{fname}() takes no arguments ({} given)", rest.len())))
    }
}

fn bit_length(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("bit_length", &kw)?;
    noargs("bit_length", &args[1..])?;
    let Some(i) = crate::bigint::as_big(&args[0]) else { return Err(crate::object::no_attribute(&args[0].type_name(), "bit_length")) };
    Ok(Value::Int(i.bits() as i64))
}

fn bit_count(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("bit_count", &kw)?;
    noargs("bit_count", &args[1..])?;
    let Some(i) = crate::bigint::as_big(&args[0]) else { return Err(crate::object::no_attribute(&args[0].type_name(), "bit_count")) };
    Ok(Value::Int(i.magnitude().iter_u64_digits().map(|d| i64::from(d.count_ones())).sum()))
}

fn to_bytes(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    use num_traits::Signed;
    let Some(v) = crate::bigint::as_big(&args[0]) else { return Err(crate::object::no_attribute(&args[0].type_name(), "to_bytes")) };
    if args.len() > 3 {
        return Err(type_error(format!(
            "to_bytes() takes at most 2 positional arguments ({} given)",
            args.len() - 1
        )));
    }
    let slots = bind("to_bytes", args[1..].to_vec(), kw, &["length", "byteorder", "signed"], 0)?;
    let length = match &slots[0] {
        None => 1,
        Some(l) => want_int(l)?,
    };
    let big = match &slots[1] {
        None => true,
        Some(Value::Str(s)) => match s.as_str() {
            "big" => true,
            "little" => false,
            _ => return Err(exc("ValueError", "byteorder must be either 'little' or 'big'")),
        },
        Some(other) => {
            return Err(type_error(format!("to_bytes() argument 'byteorder' must be str, not {}", other.type_name())))
        }
    };
    let signed = slots[2].as_ref().is_some_and(Value::is_true);
    if length < 0 {
        return Err(exc("ValueError", "length argument must be non-negative"));
    }
    if v.is_negative() && !signed {
        return Err(exc("OverflowError", "can't convert negative int to unsigned"));
    }
    let length = length as usize;
    // Complemento de dois em `length` bytes; cabe?
    let bits = length * 8;
    let modulus = num_bigint::BigInt::from(1) << bits;
    let fits = if signed {
        bits > 0 && v >= -(&modulus >> 1usize) && v < (&modulus >> 1usize)
    } else {
        v < modulus
    };
    if !fits {
        return Err(exc("OverflowError", "int too big to convert"));
    }
    let wrapped = if v.is_negative() { &v + &modulus } else { v };
    let (_, mut mag) = wrapped.to_bytes_be();
    if wrapped.sign() == num_bigint::Sign::NoSign {
        mag.clear();
    }
    let mut out = vec![0u8; length - mag.len()];
    out.extend(mag);
    if !big {
        out.reverse();
    }
    Ok(Value::bytes(out))
}

fn conjugate(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("conjugate", &kw)?;
    noargs("conjugate", &args[1..])?;
    match &args[0] {
        Value::Float(x) => Ok(Value::Float(*x)),
        other => match crate::bigint::as_big(other) {
            Some(i) => Ok(crate::bigint::norm(i)),
            None => Err(crate::object::no_attribute(other.type_name(), "conjugate")),
        },
    }
}

fn is_integer(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("is_integer", &kw)?;
    noargs("is_integer", &args[1..])?;
    match &args[0] {
        Value::Float(x) => Ok(Value::Bool(x.is_finite() && x.fract() == 0.0)),
        other => match crate::bigint::as_big(other) {
            Some(_) => Ok(Value::Bool(true)),
            None => Err(crate::object::no_attribute(other.type_name(), "is_integer")),
        },
    }
}

/// `float.hex()`: `0x1.8000000000000p+0`.
fn float_hex(x: f64) -> String {
    if x.is_nan() {
        return "nan".to_string();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf".to_string() } else { "-inf".to_string() };
    }
    let bits = x.to_bits();
    let sign = if bits >> 63 == 1 { "-" } else { "" };
    let exp = ((bits >> 52) & 0x7ff) as i64;
    let frac = bits & ((1u64 << 52) - 1);
    if exp == 0 && frac == 0 {
        return format!("{sign}0x0.0p+0");
    }
    if exp == 0 {
        return format!("{sign}0x0.{frac:013x}p-1022");
    }
    let e = exp - 1023;
    let esign = if e < 0 { "-" } else { "+" };
    format!("{sign}0x1.{frac:013x}p{esign}{}", e.abs())
}

fn hex(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("hex", &kw)?;
    noargs("hex", &args[1..])?;
    match &args[0] {
        Value::Float(x) => Ok(Value::str(float_hex(*x))),
        other => Err(crate::object::no_attribute(other.type_name(), "hex")),
    }
}

fn as_integer_ratio(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    nokw("as_integer_ratio", &kw)?;
    noargs("as_integer_ratio", &args[1..])?;
    let x = match &args[0] {
        Value::Float(x) => *x,
        other => {
            return match crate::bigint::as_big(other) {
                Some(i) => Ok(Value::tuple(vec![crate::bigint::norm(i), Value::Int(1)])),
                None => Err(crate::object::no_attribute(other.type_name(), "as_integer_ratio")),
            }
        }
    };
    if x.is_nan() {
        return Err(exc("ValueError", "cannot convert NaN to integer ratio"));
    }
    if x.is_infinite() {
        return Err(exc("OverflowError", "cannot convert Infinity to integer ratio"));
    }
    let bits = x.to_bits();
    let neg = bits >> 63 == 1;
    let exp_bits = ((bits >> 52) & 0x7ff) as i32;
    let frac = bits & ((1u64 << 52) - 1);
    let (mut m, mut e) = if exp_bits == 0 { (frac, -1074i32) } else { (frac | (1u64 << 52), exp_bits - 1075) };
    if m == 0 {
        return Ok(Value::tuple(vec![Value::Int(0), Value::Int(1)]));
    }
    let tz = m.trailing_zeros() as i32;
    m >>= tz;
    e += tz;
    let (num, den): (num_bigint::BigInt, num_bigint::BigInt) = if e >= 0 {
        (num_bigint::BigInt::from(m) << e as usize, num_bigint::BigInt::from(1))
    } else {
        (num_bigint::BigInt::from(m), num_bigint::BigInt::from(1) << (-e) as usize)
    };
    let num = if neg { -num } else { num };
    Ok(Value::tuple(vec![crate::bigint::norm(num), crate::bigint::norm(den)]))
}

fn dunder_round(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    if !matches!(args[0], Value::Float(_)) && crate::bigint::as_big(&args[0]).is_none() {
        return Err(crate::object::no_attribute(&args[0].type_name(), "__round__"));
    }
    crate::builtins::b_round(vm, args, kw)
}

/// `__trunc__`, `__floor__` e `__ceil__`: o inteiro na direção pedida.
fn to_int(vm: &mut Vm, args: Vec<Value>, kw: Kw, name: &str, f: fn(f64) -> f64) -> PyResult<Value> {
    nokw(name, &kw)?;
    noargs(name, &args[1..])?;
    match &args[0] {
        Value::Float(x) => crate::builtins::b_int(vm, vec![Value::Float(f(*x))], Vec::new()),
        other => match crate::bigint::as_big(other) {
            Some(i) => Ok(crate::bigint::norm(i)),
            None => Err(crate::object::no_attribute(other.type_name(), name)),
        },
    }
}

fn dunder_trunc(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    to_int(vm, args, kw, "__trunc__", f64::trunc)
}

fn dunder_floor(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    to_int(vm, args, kw, "__floor__", f64::floor)
}

fn dunder_ceil(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    to_int(vm, args, kw, "__ceil__", f64::ceil)
}

pub const TABLE: &[(&str, NativeFnPtr)] = &[
    ("__round__", dunder_round),
    ("__trunc__", dunder_trunc),
    ("__floor__", dunder_floor),
    ("__ceil__", dunder_ceil),
    ("bit_length", bit_length),
    ("bit_count", bit_count),
    ("to_bytes", to_bytes),
    ("conjugate", conjugate),
    ("is_integer", is_integer),
    ("hex", hex),
    ("as_integer_ratio", as_integer_ratio),
];

#[cfg(test)]
mod tests {
    fn run(src: &str) -> (String, String, i32) {
        let o = crate::run_source(src);
        (String::from_utf8(o.stdout).unwrap(), o.stderr, o.status)
    }

    #[test]
    fn bit_length() {
        let (out, _, st) = run("a = 255\nb = 0\nc = -5\nd = 1024\nprint(a.bit_length(), b.bit_length(), c.bit_length(), d.bit_length())\n");
        assert_eq!(st, 0);
        assert_eq!(out, "8 0 3 11\n");
    }

    #[test]
    fn to_bytes() {
        let (out, _, st) = run("a = 258\nprint(a.to_bytes(2, 'big'))\nprint(a.to_bytes(2, 'little'))\nb = -1\nprint(b.to_bytes(2, 'big', signed=True))\nprint(a.to_bytes(length=3, byteorder='big'))\nc = 5\nprint(c.to_bytes())\n");
        assert_eq!(st, 0);
        assert_eq!(out, "b'\\x01\\x02'\nb'\\x02\\x01'\nb'\\xff\\xff'\nb'\\x00\\x01\\x02'\nb'\\x05'\n");
    }

    #[test]
    fn to_bytes_overflow() {
        let (_, err, st) = run("a = 256\na.to_bytes(1, 'big')\n");
        assert_ne!(st, 0);
        assert!(err.contains("OverflowError: int too big to convert"), "{err}");
        let (_, err, _) = run("a = -1\na.to_bytes(2, 'big')\n");
        assert!(err.contains("OverflowError: can't convert negative int to unsigned"), "{err}");
    }

    #[test]
    fn int_misc() {
        let (out, _, _) = run("a = 7\nprint(a.conjugate(), a.is_integer())\nt = True\nprint(t.bit_length(), t.conjugate())\n");
        assert_eq!(out, "7 True\n1 1\n");
    }

    #[test]
    fn float_methods() {
        let (out, _, st) = run("x = 2.5\ny = 2.0\nprint(x.is_integer(), y.is_integer())\nprint(x.as_integer_ratio())\nprint(x.hex(), y.hex())\nz = 0.0\nprint(z.hex())\nprint(x.conjugate())\n");
        assert_eq!(st, 0);
        assert_eq!(
            out,
            "False True\n(5, 2)\n0x1.4000000000000p+1 0x1.0000000000000p+1\n0x0.0p+0\n2.5\n"
        );
    }

    #[test]
    fn wrong_receiver() {
        let (_, err, st) = run("x = 2.5\nx.bit_length()\n");
        assert_ne!(st, 0);
        assert!(err.contains("AttributeError: 'float' object has no attribute 'bit_length'"), "{err}");
    }
}
