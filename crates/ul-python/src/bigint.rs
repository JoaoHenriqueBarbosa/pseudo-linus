//! `int` de precisão arbitrária (`Objects/longobject.c`): o que não cabe em `i64` vive em
//! `Value::Big`, sempre normalizado (um valor que cabe em `i64` é `Value::Int`).

use std::cmp::Ordering;
use std::rc::Rc;

use num_bigint::{BigInt, Sign};
use num_integer::Integer;
use num_traits::{Signed, ToPrimitive, Zero};

use crate::ast::Operator;
use crate::object::Value;
use crate::vm::{exc, PyException};

/// Mensagem do `OverflowError` interno que as contas de `i64` levantam quando o resultado não cabe.
/// O chamador a reconhece com [`is_overflow`] e refaz a conta em precisão arbitrária.
pub const OVERFLOW_MSG: &str = "integer result outside the 64-bit range (arbitrary int is pending)";

/// O `int` normalizado: `Int` se cabe em `i64`, senão `Big`.
pub fn norm(b: BigInt) -> Value {
    match b.to_i64() {
        Some(i) => Value::Int(i),
        None => Value::Big(Rc::new(b)),
    }
}

/// `bool`, `int` e `int` grande como `BigInt`.
pub fn as_big(v: &Value) -> Option<BigInt> {
    match v {
        Value::Bool(b) => Some(BigInt::from(i64::from(*b))),
        Value::Int(i) => Some(BigInt::from(*i)),
        Value::Big(b) => Some((**b).clone()),
        _ => None,
    }
}

/// `v` é `bool`, `int` ou `int` grande.
pub fn is_int(v: &Value) -> bool {
    matches!(v, Value::Bool(_) | Value::Int(_) | Value::Big(_))
}

/// O erro é o `OverflowError` interno das contas de `i64`.
pub fn is_overflow(e: &PyException) -> bool {
    e.kind == "OverflowError" && e.msg == OVERFLOW_MSG
}

fn too_large() -> PyException {
    exc("OverflowError", "int too large to convert to float")
}

/// `float(int)`: arredonda para o par mais próximo; `OverflowError` se não cabe em `double`.
pub fn to_f64(b: &BigInt) -> Result<f64, PyException> {
    match b.to_f64() {
        Some(x) if x.is_finite() => Ok(x),
        _ => Err(too_large()),
    }
}

/// `x * 2**e` sem passar por infinito/zero nos expoentes intermediários.
fn ldexp(mut x: f64, mut e: i64) -> f64 {
    while e > 1000 {
        x *= 2f64.powi(1000);
        e -= 1000;
    }
    while e < -1000 {
        x *= 2f64.powi(-1000);
        e += 1000;
    }
    x * 2f64.powi(e as i32)
}

/// Divisão verdadeira `a / b` exata (arredondada uma vez só), como o `long_true_divide`.
fn true_divide(a: &BigInt, b: &BigInt) -> Result<f64, PyException> {
    if b.is_zero() {
        return Err(exc("ZeroDivisionError", "division by zero"));
    }
    if a.is_zero() {
        return Ok(0.0);
    }
    let negative = (a.sign() == Sign::Minus) != (b.sign() == Sign::Minus);
    let (a, b) = (a.abs(), b.abs());
    // Quociente com pelo menos 55 bits, mais um bit "pegajoso" se sobrou resto.
    let shift = 56 + b.bits() as i64 - a.bits() as i64;
    let (num, den) = if shift >= 0 { (&a << shift as usize, b.clone()) } else { (a.clone(), &b << (-shift) as usize) };
    let (q, r) = num.div_rem(&den);
    let q = if r.is_zero() { q } else { (q << 1usize) | BigInt::from(1) };
    let extra = i64::from(!r.is_zero());
    let value = q.to_f64().unwrap_or(f64::INFINITY);
    let out = ldexp(value, -shift - extra);
    if out.is_infinite() {
        return Err(exc("OverflowError", "integer division result too large for a float"));
    }
    Ok(if negative { -out } else { out })
}

fn shift_count(b: &BigInt) -> Result<usize, PyException> {
    if b.is_negative() {
        return Err(exc("ValueError", "negative shift count"));
    }
    b.to_usize().ok_or_else(|| exc("OverflowError", "too many digits in integer"))
}

/// Aritmética de `int` de precisão arbitrária; `Err(None)` é "tipo não suportado".
pub fn binary(op: Operator, a: &BigInt, b: &BigInt) -> Result<Value, Option<PyException>> {
    use Operator as O;
    let some = Some;
    Ok(match op {
        O::Add => norm(a + b),
        O::Sub => norm(a - b),
        O::Mult => norm(a * b),
        O::Div => Value::Float(true_divide(a, b).map_err(some)?),
        O::FloorDiv => {
            if b.is_zero() {
                return Err(Some(exc("ZeroDivisionError", "integer division or modulo by zero")));
            }
            norm(a.div_floor(b))
        }
        O::Mod => {
            if b.is_zero() {
                return Err(Some(exc("ZeroDivisionError", "integer modulo by zero")));
            }
            norm(a.mod_floor(b))
        }
        O::Pow => {
            if b.is_negative() {
                let (x, y) = (to_f64(a).map_err(some)?, to_f64(b).map_err(some)?);
                if x == 0.0 {
                    return Err(Some(exc("ZeroDivisionError", "zero to a negative power")));
                }
                return Ok(Value::Float(x.powf(y)));
            }
            if a.is_zero() || *a == BigInt::from(1) {
                return Ok(norm(a.clone()));
            }
            if *a == BigInt::from(-1) {
                return Ok(Value::Int(if b.is_even() { 1 } else { -1 }));
            }
            let e = b.to_u32().ok_or_else(|| Some(exc("MemoryError", "")))?;
            norm(num_traits::pow::Pow::pow(a, e))
        }
        O::LShift => norm(a << shift_count(b).map_err(some)?),
        O::RShift => {
            let n = match shift_count(b) {
                Ok(n) => n,
                Err(e) if e.kind == "OverflowError" => return Ok(Value::Int(if a.is_negative() { -1 } else { 0 })),
                Err(e) => return Err(Some(e)),
            };
            norm(a >> n)
        }
        O::BitAnd => norm(a & b),
        O::BitOr => norm(a | b),
        O::BitXor => norm(a ^ b),
        O::MatMult => return Err(None),
    })
}

/// `hash(int)`: módulo de `2**61 - 1` com o sinal do número (`long_hash`).
pub fn hash(b: &BigInt) -> i64 {
    let modulus = BigInt::from(crate::object::HASH_MODULUS);
    let m = (b.abs() % modulus).to_i64().unwrap_or(0);
    let h = if b.is_negative() { -m } else { m };
    if h == -1 { -2 } else { h }
}

/// Ordem entre `int` grande e `float` (exata, sem arredondar o inteiro).
pub fn cmp_float(b: &BigInt, x: f64) -> Option<Ordering> {
    if x.is_nan() {
        return None;
    }
    if x.is_infinite() {
        return Some(if x > 0.0 { Ordering::Less } else { Ordering::Greater });
    }
    let t = x.trunc();
    let ti = float_to_big(t)?;
    Some(match b.cmp(&ti) {
        Ordering::Equal => {
            let frac = x - t;
            if frac > 0.0 {
                Ordering::Less
            } else if frac < 0.0 {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        }
        other => other,
    })
}

/// `int(float)` sem perda: o inteiro exato do `double` (que já é inteiro).
pub fn float_to_big(x: f64) -> Option<BigInt> {
    use num_traits::FromPrimitive;
    BigInt::from_f64(x.trunc())
}

/// `int(texto, base)` já sem espaços, sinal ou sublinhados de grupo tratados por quem chama.
pub fn parse(digits: &str, base: u32) -> Option<BigInt> {
    BigInt::parse_bytes(digits.as_bytes(), base)
}

/// Dígitos de `b` na `base` (2 a 36), minúsculos, sem prefixo; com `-` se negativo.
pub fn to_radix(b: &BigInt, base: u32) -> String {
    b.to_str_radix(base)
}
