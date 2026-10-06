//! Módulo `math` do CPython 3.13: constantes e funções puras sobre `float` e `int`.
//!
//! Limitação conhecida: `int` é `i64` (inteiro arbitrário ainda não existe), então `factorial`,
//! `comb`, `perm`, `lcm`, `floor`, `ceil` e `trunc` levantam `OverflowError` quando o resultado
//! não cabe em 64 bits. Ficam de fora: `nextafter`, `ulp`, `erf`, `gamma`, `lgamma`, `remainder`,
//! `sumprod`, `cbrt` de inteiros enormes.

use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{Signed, ToPrimitive, Zero};
use std::rc::Rc;

use crate::modules::ModuleBuilder;
use crate::native_util::{bind, exactly, no_kwargs};
use crate::object::{Kw, ModuleObj, Value};
use crate::vm::{exc, iterate, py_binary, type_error, PyException, PyResult, Vm};

fn domain() -> PyException {
    exc("ValueError", "math domain error")
}

fn range_err() -> PyException {
    exc("OverflowError", "math range error")
}

fn overflow64() -> PyException {
    exc("OverflowError", "integer result outside the 64-bit range (arbitrary int is pending)")
}

fn to_f(v: &Value) -> PyResult<f64> {
    match v {
        Value::Float(x) => Ok(*x),
        Value::Int(i) => Ok(*i as f64),
        Value::Big(n) => crate::bigint::to_f64(n),
        Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        other => Err(type_error(format!("must be real number, not {}", other.type_name()))),
    }
}

fn one(fname: &str, args: &[Value], kw: &Kw) -> PyResult<f64> {
    no_kwargs(fname, kw)?;
    exactly(fname, args, 1)?;
    to_f(&args[0])
}

fn two(fname: &str, args: &[Value], kw: &Kw) -> PyResult<(f64, f64)> {
    no_kwargs(fname, kw)?;
    exactly(fname, args, 2)?;
    Ok((to_f(&args[0])?, to_f(&args[1])?))
}

/// Funções de um `float` que devolvem `float`.
macro_rules! unary {
    ($fname:ident, $py:literal, $body:expr) => {
        fn $fname(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
            let x = one($py, &args, &kw)?;
            let f: fn(f64) -> PyResult<f64> = $body;
            f(x).map(Value::Float)
        }
    };
}

/// Predicados de um `float`.
macro_rules! predicate {
    ($fname:ident, $py:literal, $body:expr) => {
        fn $fname(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
            let x = one($py, &args, &kw)?;
            let f: fn(f64) -> bool = $body;
            Ok(Value::Bool(f(x)))
        }
    };
}

/// Resultado que estourou: `inf` vindo de entrada finita.
fn checked_range(x: f64, r: f64) -> PyResult<f64> {
    if r.is_infinite() && x.is_finite() {
        Err(range_err())
    } else {
        Ok(r)
    }
}

unary!(sqrt, "sqrt", |x| if x < 0.0 { Err(domain()) } else { Ok(x.sqrt()) });
unary!(exp, "exp", |x| checked_range(x, x.exp()));
unary!(expm1, "expm1", |x| checked_range(x, x.exp_m1()));
unary!(exp2, "exp2", |x| checked_range(x, x.exp2()));
unary!(cbrt, "cbrt", |x| Ok(x.cbrt()));
unary!(erf, "erf", |x| Ok(libm::erf(x)));
unary!(erfc, "erfc", |x| Ok(libm::erfc(x)));
unary!(gamma, "gamma", |x| {
    if x.is_nan() || x == f64::INFINITY {
        return Ok(x);
    }
    if x == f64::NEG_INFINITY || (x <= 0.0 && x == x.floor()) {
        return Err(domain());
    }
    checked_range(x, libm::tgamma(x))
});
unary!(lgamma, "lgamma", |x| {
    if x.is_nan() {
        return Ok(x);
    }
    if x.is_infinite() {
        return Ok(f64::INFINITY);
    }
    if x <= 0.0 && x == x.floor() {
        return Err(domain());
    }
    Ok(libm::lgamma(x))
});

/// `sumprod(p, q)`: inteiros exatos; com `float`, produtos e soma sem perda (erro do produto via fma).
fn sumprod(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("sumprod", &kw)?;
    exactly("sumprod", &args, 2)?;
    let p = iterate(&args[0])?;
    let q = iterate(&args[1])?;
    if p.len() != q.len() {
        return Err(exc("ValueError", "Inputs are not the same length"));
    }
    let all_int = p.iter().chain(q.iter()).all(|v| matches!(v, Value::Int(_) | Value::Big(_) | Value::Bool(_)));
    if all_int {
        let mut acc = Value::Int(0);
        for (a, b) in p.iter().zip(q.iter()) {
            acc = py_binary("+", &acc, &py_binary("*", a, b)?)?;
        }
        return Ok(acc);
    }
    let numeric = |v: &Value| matches!(v, Value::Int(_) | Value::Big(_) | Value::Bool(_) | Value::Float(_));
    if p.iter().chain(q.iter()).all(numeric) {
        let mut terms = Vec::with_capacity(p.len() * 2);
        for (a, b) in p.iter().zip(q.iter()) {
            let (x, y) = (to_f(a)?, to_f(b)?);
            let prod = x * y;
            terms.push(Value::Float(prod));
            if prod.is_finite() {
                terms.push(Value::Float(x.mul_add(y, -prod)));
            }
        }
        return fsum(vm, vec![Value::list(terms)], Kw::default());
    }
    let mut acc = Value::Int(0);
    for (a, b) in p.iter().zip(q.iter()) {
        acc = py_binary("+", &acc, &py_binary("*", a, b)?)?;
    }
    Ok(acc)
}
fn log2(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    if let ([Value::Big(n)], true) = (args.as_slice(), kw.is_empty()) {
        return big_log(n, 2).map(Value::Float);
    }
    let x = one("log2", &args, &kw)?;
    if x <= 0.0 { Err(domain()) } else { Ok(Value::Float(x.log2())) }
}

fn log10(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    if let ([Value::Big(n)], true) = (args.as_slice(), kw.is_empty()) {
        return big_log(n, 10).map(Value::Float);
    }
    let x = one("log10", &args, &kw)?;
    if x <= 0.0 { Err(domain()) } else { Ok(Value::Float(x.log10())) }
}
unary!(log1p, "log1p", |x| if x <= -1.0 { Err(domain()) } else { Ok(x.ln_1p()) });
unary!(sin, "sin", |x| if x.is_infinite() { Err(domain()) } else { Ok(x.sin()) });
unary!(cos, "cos", |x| if x.is_infinite() { Err(domain()) } else { Ok(x.cos()) });
unary!(tan, "tan", |x| if x.is_infinite() { Err(domain()) } else { Ok(x.tan()) });
unary!(asin, "asin", |x| if x.abs() > 1.0 { Err(domain()) } else { Ok(x.asin()) });
unary!(acos, "acos", |x| if x.abs() > 1.0 { Err(domain()) } else { Ok(x.acos()) });
unary!(atan, "atan", |x| Ok(x.atan()));
unary!(sinh, "sinh", |x| checked_range(x, x.sinh()));
unary!(cosh, "cosh", |x| checked_range(x, x.cosh()));
unary!(tanh, "tanh", |x| Ok(x.tanh()));
unary!(asinh, "asinh", |x| Ok(x.asinh()));
unary!(acosh, "acosh", |x| if x < 1.0 { Err(domain()) } else { Ok(x.acosh()) });
unary!(atanh, "atanh", |x| if x.abs() >= 1.0 { Err(domain()) } else { Ok(x.atanh()) });
unary!(fabs, "fabs", |x| Ok(x.abs()));
unary!(degrees, "degrees", |x| Ok(x.to_degrees()));
unary!(radians, "radians", |x| Ok(x.to_radians()));

predicate!(isnan, "isnan", |x| x.is_nan());
predicate!(isinf, "isinf", |x| x.is_infinite());
predicate!(isfinite, "isfinite", |x| x.is_finite());

/// `ln` de um `int` grande, mesmo quando não cabe em `double`.
fn big_ln(n: &BigInt) -> PyResult<f64> {
    big_log(n, 0)
}

/// Logaritmo de `int` grande na base 2, 10 ou `e` (0), separando o expoente binário da mantissa.
fn big_log(n: &BigInt, base: u32) -> PyResult<f64> {
    if !n.is_positive() {
        return Err(domain());
    }
    let shift = n.bits().saturating_sub(60);
    let mant = (n >> shift as usize).to_f64().unwrap_or(1.0);
    let e = shift as f64;
    Ok(match base {
        2 => mant.log2() + e,
        10 => mant.log10() + e * std::f64::consts::LOG10_2,
        _ => mant.ln() + e * std::f64::consts::LN_2,
    })
}

fn log(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("log", &kw)?;
    if args.is_empty() {
        return Err(type_error("log expected at least 1 argument, got 0"));
    }
    if args.len() > 2 {
        return Err(type_error(format!("log expected at most 2 arguments, got {}", args.len())));
    }
    let ln = |v: &Value| -> PyResult<f64> {
        if let Value::Big(n) = v {
            return big_ln(n);
        }
        let x = to_f(v)?;
        if x <= 0.0 {
            Err(domain())
        } else {
            Ok(x.ln())
        }
    };
    let num = ln(&args[0])?;
    if args.len() == 1 {
        return Ok(Value::Float(num));
    }
    let den = ln(&args[1])?;
    if den == 0.0 {
        return Err(exc("ZeroDivisionError", "float division by zero"));
    }
    Ok(Value::Float(num / den))
}

fn pow(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let (x, y) = two("pow", &args, &kw)?;
    if x.is_finite() && y.is_finite() {
        if x == 0.0 && y < 0.0 {
            return Err(domain());
        }
        let r = x.powf(y);
        if r.is_nan() {
            return Err(domain());
        }
        if r.is_infinite() {
            return Err(range_err());
        }
        return Ok(Value::Float(r));
    }
    Ok(Value::Float(x.powf(y)))
}

fn atan2(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let (y, x) = two("atan2", &args, &kw)?;
    Ok(Value::Float(y.atan2(x)))
}

fn copysign(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let (x, y) = two("copysign", &args, &kw)?;
    Ok(Value::Float(x.copysign(y)))
}

fn fmod(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let (x, y) = two("fmod", &args, &kw)?;
    if y.is_infinite() && x.is_finite() {
        return Ok(Value::Float(x));
    }
    let r = x % y;
    if r.is_nan() && !x.is_nan() && !y.is_nan() {
        return Err(domain());
    }
    Ok(Value::Float(r))
}

fn hypot_of(xs: &[f64]) -> f64 {
    if xs.iter().any(|x| x.is_infinite()) {
        return f64::INFINITY;
    }
    if xs.iter().any(|x| x.is_nan()) {
        return f64::NAN;
    }
    let m = xs.iter().fold(0.0f64, |a, x| a.max(x.abs()));
    if m == 0.0 {
        return 0.0;
    }
    let s: f64 = xs
        .iter()
        .map(|x| {
            let r = x / m;
            r * r
        })
        .sum();
    s.sqrt() * m
}

fn hypot(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("hypot", &kw)?;
    let xs = args.iter().map(to_f).collect::<PyResult<Vec<f64>>>()?;
    let r = hypot_of(&xs);
    if r.is_infinite() && xs.iter().all(|x| x.is_finite()) {
        return Err(range_err());
    }
    Ok(Value::Float(r))
}

fn dist(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("dist", &kw)?;
    exactly("dist", &args, 2)?;
    let p = iterate(&args[0])?.iter().map(to_f).collect::<PyResult<Vec<f64>>>()?;
    let q = iterate(&args[1])?.iter().map(to_f).collect::<PyResult<Vec<f64>>>()?;
    if p.len() != q.len() {
        return Err(exc("ValueError", "both points must have the same number of dimensions"));
    }
    let diffs: Vec<f64> = p.iter().zip(q.iter()).map(|(a, b)| a - b).collect();
    let r = hypot_of(&diffs);
    if r.is_infinite() && diffs.iter().all(|x| x.is_finite()) {
        return Err(range_err());
    }
    Ok(Value::Float(r))
}

fn float_to_int(x: f64) -> PyResult<Value> {
    if x.is_nan() {
        return Err(exc("ValueError", "cannot convert float NaN to integer"));
    }
    if x.is_infinite() {
        return Err(exc("OverflowError", "cannot convert float infinity to integer"));
    }
    if x >= 9_223_372_036_854_775_808.0 || x < -9_223_372_036_854_775_808.0 {
        return Ok(crate::bigint::norm(crate::bigint::float_to_big(x).unwrap_or_default()));
    }
    Ok(Value::Int(x as i64))
}

fn rounding(vm: &mut Vm, fname: &str, special: &str, args: &[Value], kw: &Kw, f: fn(f64) -> f64) -> PyResult<Value> {
    no_kwargs(fname, kw)?;
    exactly(fname, args, 1)?;
    match &args[0] {
        Value::Int(i) => Ok(Value::Int(*i)),
        Value::Big(_) => Ok(args[0].clone()),
        Value::Bool(b) => Ok(Value::Int(i64::from(*b))),
        Value::Float(x) => float_to_int(f(*x)),
        other => {
            if let Some(r) = vm.call_dunder(other, special, Vec::new()) {
                return r;
            }
            // Sem `__floor__`/`__ceil__`, o CPython converte por `__float__`; `trunc` exige `__trunc__`.
            if special != "__trunc__" {
                if let Some(Ok(Value::Float(x))) = vm.call_dunder(other, "__float__", Vec::new()) {
                    return float_to_int(f(x));
                }
            }
            Err(type_error(format!("type {} doesn't define {} method", other.type_name(), special)))
        }
    }
}

fn floor(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    rounding(vm, "floor", "__floor__", &args, &kw, f64::floor)
}

fn ceil(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    rounding(vm, "ceil", "__ceil__", &args, &kw, f64::ceil)
}

fn trunc(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    rounding(vm, "trunc", "__trunc__", &args, &kw, f64::trunc)
}

fn isclose(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("isclose", args, kw, &["a", "b", "rel_tol", "abs_tol"], 2)?;
    let a = to_f(s[0].as_ref().unwrap())?;
    let b = to_f(s[1].as_ref().unwrap())?;
    let rel = match &s[2] {
        Some(v) => to_f(v)?,
        None => 1e-9,
    };
    let abs = match &s[3] {
        Some(v) => to_f(v)?,
        None => 0.0,
    };
    if rel < 0.0 || abs < 0.0 {
        return Err(exc("ValueError", "tolerances must be non-negative"));
    }
    if a == b {
        return Ok(Value::Bool(true));
    }
    if a.is_infinite() || b.is_infinite() {
        return Ok(Value::Bool(false));
    }
    let diff = (b - a).abs();
    Ok(Value::Bool(diff <= (rel * b).abs() || diff <= (rel * a).abs() || diff <= abs))
}

/// Argumento inteiro (`int`, `int` grande ou `bool`) como `BigInt`.
fn want_big(v: &Value) -> PyResult<BigInt> {
    crate::bigint::as_big(v)
        .ok_or_else(|| type_error(format!("'{}' object cannot be interpreted as an integer", v.type_name())))
}

fn gcd(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("gcd", &kw)?;
    let mut g = BigInt::zero();
    for a in &args {
        g = g.gcd(&want_big(a)?);
    }
    Ok(crate::bigint::norm(g))
}

fn lcm(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("lcm", &kw)?;
    let mut l = BigInt::from(1);
    for a in &args {
        l = l.lcm(&want_big(a)?);
    }
    Ok(crate::bigint::norm(l))
}

/// Produto `lo * (lo+1) * ... * hi` por divisão e conquista (como o CPython, evita multiplicar um
/// número enorme por um pequeno a cada passo).
fn product_range(lo: u64, hi: u64) -> BigInt {
    if lo > hi {
        return BigInt::from(1);
    }
    if hi - lo < 16 {
        return (lo..=hi).fold(BigInt::from(1), |acc, i| acc * i);
    }
    let mid = lo + (hi - lo) / 2;
    product_range(lo, mid) * product_range(mid + 1, hi)
}

fn count_arg(v: &Value, what: &str) -> PyResult<u64> {
    let n = want_big(v)?;
    if n.is_negative() {
        return Err(exc("ValueError", what));
    }
    n.to_u64().ok_or_else(|| exc("OverflowError", format!("factorial() argument should not exceed {}", i64::MAX)))
}

fn factorial(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("factorial", &kw)?;
    exactly("factorial", &args, 1)?;
    let n = want_big(&args[0])?;
    if n.is_negative() {
        return Err(exc("ValueError", "factorial() not defined for negative values"));
    }
    let n = n
        .to_u64()
        .ok_or_else(|| exc("OverflowError", format!("factorial() argument should not exceed {}", i64::MAX)))?;
    Ok(crate::bigint::norm(product_range(2, n)))
}

fn comb(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("comb", &kw)?;
    exactly("comb", &args, 2)?;
    let n = want_big(&args[0])?;
    let k = want_big(&args[1])?;
    if n.is_negative() {
        return Err(exc("ValueError", "n must be a non-negative integer"));
    }
    if k.is_negative() {
        return Err(exc("ValueError", "k must be a non-negative integer"));
    }
    if k > n {
        return Ok(Value::Int(0));
    }
    let k = k.clone().min(&n - &k);
    let k = k.to_u64().ok_or_else(|| exc("OverflowError", "min(n - k, k) must not exceed 9223372036854775807"))?;
    let mut r = BigInt::from(1);
    for i in 1..=k {
        r = r * (&n - k + i) / i;
    }
    Ok(crate::bigint::norm(r))
}

fn perm(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("perm", args, kw, &["n", "k"], 1)?;
    let n = want_big(s[0].as_ref().unwrap())?;
    let k = match &s[1] {
        None | Some(Value::None) => n.clone(),
        Some(v) => want_big(v)?,
    };
    if n.is_negative() {
        return Err(exc("ValueError", "n must be a non-negative integer"));
    }
    if k.is_negative() {
        return Err(exc("ValueError", "k must be a non-negative integer"));
    }
    if k > n {
        return Ok(Value::Int(0));
    }
    let k = k.to_u64().ok_or_else(|| exc("OverflowError", "k must not exceed 9223372036854775807"))?;
    let mut r = BigInt::from(1);
    for i in 0..k {
        r *= &n - i;
    }
    Ok(crate::bigint::norm(r))
}

fn prod(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("prod", args, kw, &["iterable", "start"], 1)?;
    let mut acc = s[1].clone().unwrap_or(Value::Int(1));
    for x in iterate(s[0].as_ref().unwrap())? {
        acc = py_binary("*", &acc, &x)?;
    }
    Ok(acc)
}

fn fsum(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("fsum", &kw)?;
    exactly("fsum", &args, 1)?;
    let mut partials: Vec<f64> = Vec::new();
    let (mut pinf, mut ninf, mut nan) = (false, false, false);
    for v in iterate(&args[0])? {
        let mut x = to_f(&v)?;
        if x.is_nan() {
            nan = true;
            continue;
        }
        if x.is_infinite() {
            if x > 0.0 {
                pinf = true;
            } else {
                ninf = true;
            }
            continue;
        }
        let mut i = 0;
        for j in 0..partials.len() {
            let mut y = partials[j];
            if x.abs() < y.abs() {
                std::mem::swap(&mut x, &mut y);
            }
            let hi = x + y;
            let yr = hi - x;
            let lo = y - yr;
            if lo != 0.0 {
                partials[i] = lo;
                i += 1;
            }
            x = hi;
        }
        partials.truncate(i);
        partials.push(x);
    }
    if nan {
        return Ok(Value::Float(f64::NAN));
    }
    if pinf && ninf {
        return Err(exc("ValueError", "-inf + inf in fsum"));
    }
    if pinf {
        return Ok(Value::Float(f64::INFINITY));
    }
    if ninf {
        return Ok(Value::Float(f64::NEG_INFINITY));
    }
    let mut hi = 0.0f64;
    let mut n = partials.len();
    if n > 0 {
        n -= 1;
        hi = partials[n];
        let mut lo = 0.0f64;
        while n > 0 {
            let x = hi;
            n -= 1;
            let y = partials[n];
            hi = x + y;
            let yr = hi - x;
            lo = y - yr;
            if lo != 0.0 {
                break;
            }
        }
        if n > 0 && ((lo < 0.0 && partials[n - 1] < 0.0) || (lo > 0.0 && partials[n - 1] > 0.0)) {
            let y = lo * 2.0;
            let x = hi + y;
            let yr = x - hi;
            if y == yr {
                hi = x;
            }
        }
    }
    Ok(Value::Float(hi))
}

fn isqrt(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("isqrt", &kw)?;
    exactly("isqrt", &args, 1)?;
    let n = want_big(&args[0])?;
    if n.is_negative() {
        return Err(exc("ValueError", "isqrt() argument must be nonnegative"));
    }
    Ok(crate::bigint::norm(n.sqrt()))
}

fn modf(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let x = one("modf", &args, &kw)?;
    if x.is_infinite() {
        return Ok(Value::tuple(vec![Value::Float(0.0f64.copysign(x)), Value::Float(x)]));
    }
    let i = x.trunc();
    let f = (x - i).copysign(x);
    Ok(Value::tuple(vec![Value::Float(f), Value::Float(i)]))
}

fn frexp_of(x: f64) -> (f64, i32) {
    if x == 0.0 || !x.is_finite() {
        return (x, 0);
    }
    let bits = x.to_bits();
    let e = ((bits >> 52) & 0x7ff) as i32;
    if e == 0 {
        let (m, e2) = frexp_of(x * 2f64.powi(54));
        return (m, e2 - 54);
    }
    let m = f64::from_bits((bits & !(0x7ffu64 << 52)) | (1022u64 << 52));
    (m, e - 1022)
}

fn frexp(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let x = one("frexp", &args, &kw)?;
    let (m, e) = frexp_of(x);
    Ok(Value::tuple(vec![Value::Float(m), Value::Int(i64::from(e))]))
}

fn ldexp(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("ldexp", &kw)?;
    exactly("ldexp", &args, 2)?;
    let x = to_f(&args[0])?;
    let i = match &args[1] {
        Value::Int(i) => *i,
        Value::Bool(b) => i64::from(*b),
        _ => return Err(type_error("Expected an int as second argument to ldexp.")),
    };
    if x == 0.0 || !x.is_finite() {
        return Ok(Value::Float(x));
    }
    let mut e = i.clamp(-2200, 2200) as i32;
    let mut r = x;
    while e > 1000 {
        r *= 2f64.powi(1000);
        e -= 1000;
    }
    while e < -1000 {
        r *= 2f64.powi(-1000);
        e += 1000;
    }
    r *= 2f64.powi(e);
    if r.is_infinite() {
        return Err(range_err());
    }
    Ok(Value::Float(r))
}

pub fn build(_vm: &mut Vm) -> Rc<ModuleObj> {
    ModuleBuilder::new("math")
        .value("pi", Value::Float(std::f64::consts::PI))
        .value("e", Value::Float(std::f64::consts::E))
        .value("tau", Value::Float(std::f64::consts::TAU))
        .value("inf", Value::Float(f64::INFINITY))
        .value("nan", Value::Float(f64::NAN))
        .func("sqrt", sqrt)
        .func("pow", pow)
        .func("exp", exp)
        .func("expm1", expm1)
        .func("exp2", exp2)
        .func("cbrt", cbrt)
        .func("erf", erf)
        .func("erfc", erfc)
        .func("gamma", gamma)
        .func("lgamma", lgamma)
        .func("sumprod", sumprod)
        .func("log", log)
        .func("log2", log2)
        .func("log10", log10)
        .func("log1p", log1p)
        .func("sin", sin)
        .func("cos", cos)
        .func("tan", tan)
        .func("asin", asin)
        .func("acos", acos)
        .func("atan", atan)
        .func("atan2", atan2)
        .func("sinh", sinh)
        .func("cosh", cosh)
        .func("tanh", tanh)
        .func("asinh", asinh)
        .func("acosh", acosh)
        .func("atanh", atanh)
        .func("floor", floor)
        .func("ceil", ceil)
        .func("trunc", trunc)
        .func("fabs", fabs)
        .func("fmod", fmod)
        .func("copysign", copysign)
        .func("hypot", hypot)
        .func("degrees", degrees)
        .func("radians", radians)
        .func("isnan", isnan)
        .func("isinf", isinf)
        .func("isfinite", isfinite)
        .func("isclose", isclose)
        .func("gcd", gcd)
        .func("lcm", lcm)
        .func("factorial", factorial)
        .func("comb", comb)
        .func("perm", perm)
        .func("prod", prod)
        .func("fsum", fsum)
        .func("isqrt", isqrt)
        .func("modf", modf)
        .func("frexp", frexp)
        .func("ldexp", ldexp)
        .func("dist", dist)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::repr;
    use crate::object::NativeFnPtr;

    fn call(f: NativeFnPtr, args: Vec<Value>) -> PyResult<Value> {
        let mut vm = Vm::new();
        f(&mut vm, args, Vec::new())
    }

    fn r(f: NativeFnPtr, args: Vec<Value>) -> String {
        repr(&call(f, args).unwrap())
    }

    fn err(f: NativeFnPtr, args: Vec<Value>) -> String {
        let e = call(f, args).unwrap_err();
        format!("{}: {}", e.kind, e.msg)
    }

    fn fl(x: f64) -> Value {
        Value::Float(x)
    }

    fn int(i: i64) -> Value {
        Value::Int(i)
    }

    #[test]
    fn basic_functions() {
        assert_eq!(r(sqrt, vec![int(16)]), "4.0");
        assert_eq!(r(exp, vec![int(0)]), "1.0");
        assert_eq!(r(log2, vec![int(8)]), "3.0");
        assert_eq!(r(log10, vec![int(1000)]), "3.0");
        assert_eq!(r(atan2, vec![int(1), int(1)]), "0.7853981633974483");
        assert_eq!(r(degrees, vec![fl(std::f64::consts::PI)]), "180.0");
        assert_eq!(r(hypot, vec![int(3), int(4)]), "5.0");
        assert_eq!(r(fabs, vec![int(-3)]), "3.0");
        assert_eq!(r(copysign, vec![int(3), fl(-0.0)]), "-3.0");
        assert_eq!(r(fmod, vec![int(7), int(3)]), "1.0");
        assert_eq!(r(isnan, vec![fl(f64::NAN)]), "True");
        assert_eq!(r(isinf, vec![fl(f64::NEG_INFINITY)]), "True");
        assert_eq!(r(isfinite, vec![int(1)]), "True");
        assert_eq!(r(pow, vec![int(2), int(10)]), "1024.0");
    }

    #[test]
    fn integer_results() {
        assert_eq!(r(floor, vec![fl(-2.5)]), "-3");
        assert_eq!(r(ceil, vec![fl(2.1)]), "3");
        assert_eq!(r(trunc, vec![fl(-2.7)]), "-2");
        assert_eq!(r(floor, vec![int(5)]), "5");
        assert_eq!(r(gcd, vec![int(12), int(18)]), "6");
        assert_eq!(r(gcd, vec![]), "0");
        assert_eq!(r(lcm, vec![int(4), int(6)]), "12");
        assert_eq!(r(factorial, vec![int(5)]), "120");
        assert_eq!(r(factorial, vec![int(0)]), "1");
        assert_eq!(r(comb, vec![int(5), int(2)]), "10");
        assert_eq!(r(perm, vec![int(5), int(2)]), "20");
        assert_eq!(r(isqrt, vec![int(17)]), "4");
        assert_eq!(r(isqrt, vec![int(16)]), "4");
    }

    #[test]
    fn tuples_and_sums() {
        assert_eq!(r(modf, vec![fl(3.5)]), "(0.5, 3.0)");
        assert_eq!(r(frexp, vec![fl(8.0)]), "(0.5, 4)");
        assert_eq!(r(ldexp, vec![fl(0.5), int(4)]), "8.0");
        let tenth = Value::list((0..10).map(|_| fl(0.1)).collect());
        assert_eq!(r(fsum, vec![tenth]), "1.0");
        let l = Value::list(vec![int(2), int(3), int(4)]);
        assert_eq!(r(prod, vec![l]), "24");
        assert_eq!(r(isclose, vec![fl(1.0), fl(1.0000000001)]), "True");
        assert_eq!(r(isclose, vec![fl(1.0), fl(1.1)]), "False");
        let p = Value::tuple(vec![int(0), int(0)]);
        let q = Value::tuple(vec![int(3), int(4)]);
        assert_eq!(r(dist, vec![p, q]), "5.0");
    }

    #[test]
    fn errors() {
        assert_eq!(err(sqrt, vec![int(-1)]), "ValueError: math domain error");
        assert_eq!(err(log, vec![int(0)]), "ValueError: math domain error");
        assert_eq!(err(exp, vec![int(1000)]), "OverflowError: math range error");
        assert_eq!(err(factorial, vec![int(-1)]), "ValueError: factorial() not defined for negative values");
        assert_eq!(err(isqrt, vec![int(-1)]), "ValueError: isqrt() argument must be nonnegative");
        assert_eq!(err(floor, vec![fl(f64::NAN)]), "ValueError: cannot convert float NaN to integer");
        assert_eq!(err(floor, vec![fl(f64::INFINITY)]), "OverflowError: cannot convert float infinity to integer");
        assert_eq!(err(comb, vec![int(-1), int(2)]), "ValueError: n must be a non-negative integer");
        assert_eq!(err(sqrt, vec![Value::str("x")]), "TypeError: must be real number, not str");
    }

    #[test]
    fn module_has_constants() {
        let mut vm = Vm::new();
        let m = build(&mut vm);
        let attrs = m.attrs.borrow();
        assert_eq!(repr(&attrs["pi"]), "3.141592653589793");
        assert_eq!(repr(&attrs["e"]), "2.718281828459045");
        assert_eq!(repr(&attrs["tau"]), "6.283185307179586");
        assert_eq!(repr(&attrs["inf"]), "inf");
        assert_eq!(repr(&attrs["nan"]), "nan");
    }
}
