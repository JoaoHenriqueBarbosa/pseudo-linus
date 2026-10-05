//! `float` (`Objects/floatobject.c`): `repr` curto e hash numérico.
//!
//! A formatação usa os dígitos mínimos de ida e volta do `{:e}` do Rust, que coincidem com o modo 0
//! do `_Py_dg_dtoa`; a fatia 19 traz o port do `dtoa.c` para os casos de desempate em que os dois
//! algoritmos possam divergir.

use super::int::{HASH_BITS, HASH_MODULUS};

/// `repr()` e `str()` de `float`.
pub fn float_repr(x: f64) -> String {
    format_float_short(x, true)
}

/// `float_repr_style == 'short'` do `Python/pystrtod.c`: menor sequência de dígitos que volta ao
/// mesmo `double`, em notação fixa quando o expoente decimal fica em `-4 <= exp < 16` e científica
/// (`1e-05`, `1e+16`) fora disso. `add_dot_0` acrescenta o `.0` dos inteiros, que o `repr` de
/// `float` usa e o de `complex` não.
pub fn format_float_short(x: f64, add_dot_0: bool) -> String {
    if x.is_nan() {
        return "nan".to_string();
    }
    if x.is_infinite() {
        return if x < 0.0 { "-inf" } else { "inf" }.to_string();
    }
    let sci = format!("{x:e}");
    let (mantissa, exp) = sci.split_once('e').unwrap_or((sci.as_str(), "0"));
    let exp: i64 = exp.parse().unwrap_or(0);
    let negative = mantissa.starts_with('-');
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let decpt = exp + 1;
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if decpt > -4 && decpt <= 16 {
        let len = digits.len() as i64;
        if decpt <= 0 {
            out.push_str("0.");
            out.extend(std::iter::repeat_n('0', (-decpt) as usize));
            out.push_str(&digits);
        } else if decpt >= len {
            out.push_str(&digits);
            out.extend(std::iter::repeat_n('0', (decpt - len) as usize));
            if add_dot_0 {
                out.push_str(".0");
            }
        } else {
            out.push_str(&digits[..decpt as usize]);
            out.push('.');
            out.push_str(&digits[decpt as usize..]);
        }
    } else {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        let e = decpt - 1;
        out.push_str(&format!("e{}{:02}", if e < 0 { '-' } else { '+' }, e.abs()));
    }
    out
}

/// `frexp` da libm: `x == m * 2**e` com `0.5 <= |m| < 1`.
fn frexp(x: f64) -> (f64, i32) {
    if x == 0.0 || !x.is_finite() {
        return (x, 0);
    }
    let bits = x.to_bits();
    let exp = ((bits >> 52) & 0x7ff) as i32;
    if exp == 0 {
        // Subnormal: normaliza antes.
        let (m, e) = frexp(x * f64::from_bits(0x4350_0000_0000_0000)); // 2**54
        return (m, e - 54);
    }
    let m = f64::from_bits((bits & !(0x7ff << 52)) | (1022 << 52));
    (m, exp - 1022)
}

/// `_Py_HashDouble`: o mesmo hash de `int` quando o valor é inteiro, para `hash(1.0) == hash(1)`.
///
/// NaN: desde o 3.10 o CPython usa a identidade do objeto. Aqui `float` é valor inline, sem
/// identidade; todo NaN recebe hash 0. Isso só muda a posição de um NaN dentro de um `set`.
pub fn float_hash(v: f64) -> i64 {
    if v.is_nan() {
        return 0;
    }
    if v.is_infinite() {
        return if v > 0.0 { 314_159 } else { -314_159 };
    }
    let (mut m, mut e) = frexp(v);
    let mut sign = 1i64;
    if m < 0.0 {
        sign = -1;
        m = -m;
    }
    let mut x: u64 = 0;
    while m != 0.0 {
        x = ((x << 28) & HASH_MODULUS) | x >> (HASH_BITS - 28);
        m *= 268_435_456.0; // 2**28
        e -= 28;
        let y = m as u64;
        m -= y as f64;
        x += y;
        if x >= HASH_MODULUS {
            x -= HASH_MODULUS;
        }
    }
    let bits = HASH_BITS as i32;
    let e = (if e >= 0 { e % bits } else { bits - 1 - ((-1 - e) % bits) }) as u32;
    x = ((x << e) & HASH_MODULUS) | x >> (HASH_BITS - e);
    let h = x as i64 * sign;
    if h == -1 { -2 } else { h }
}
