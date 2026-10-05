//! Pedaços da libc que o porte do libmagic usa com a semântica exata do C: `ctype` por byte no
//! locale C.UTF-8 (byte acima de 0x7f nunca é classe nenhuma), `strtol`/`strtoul`/`strtoull` com
//! base 0 (prefixo `0x` e `0`), `strtod` e cadeias terminadas em NUL.

/// `isspace`: espaço, `\t`, `\n`, `\v`, `\f`, `\r`.
pub fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// `isprint` por byte (no C.UTF-8 só o ASCII imprimível conta).
pub fn is_print(c: u8) -> bool {
    (0x20..=0x7e).contains(&c)
}

pub fn is_digit(c: u8) -> bool {
    c.is_ascii_digit()
}

pub fn is_alpha(c: u8) -> bool {
    c.is_ascii_alphabetic()
}

pub fn is_alnum(c: u8) -> bool {
    c.is_ascii_alphanumeric()
}

pub fn is_upper(c: u8) -> bool {
    c.is_ascii_uppercase()
}

pub fn is_lower(c: u8) -> bool {
    c.is_ascii_lowercase()
}

/// A cadeia C que começa em `s`: até o primeiro NUL (ou o fim).
pub fn cstr(s: &[u8]) -> &[u8] {
    match s.iter().position(|&b| b == 0) {
        Some(n) => &s[..n],
        None => s,
    }
}

/// Byte na posição `i`, ou NUL depois do fim (o terminador implícito das cadeias C).
pub fn at(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

fn digit_value(c: u8) -> Option<u32> {
    match c {
        b'0'..=b'9' => Some(u32::from(c - b'0')),
        b'a'..=b'z' => Some(u32::from(c - b'a') + 10),
        b'A'..=b'Z' => Some(u32::from(c - b'A') + 10),
        _ => None,
    }
}

/// Resultado de uma conversão `strto*`: valor, bytes consumidos (0 = nada convertido, como
/// `endptr == nptr`) e se houve estouro (`ERANGE`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Conv<T> {
    pub value: T,
    pub used: usize,
    pub overflow: bool,
}

/// Núcleo comum: espaços, sinal, prefixo da base e dígitos. Devolve (negativo, magnitude
/// saturada em `u64::MAX`, consumido, estourou).
fn scan_integer(s: &[u8], base: u32) -> (bool, u64, usize, bool) {
    let mut i = 0;
    while is_space(at(s, i)) {
        i += 1;
    }
    let mut neg = false;
    if at(s, i) == b'-' || at(s, i) == b'+' {
        neg = at(s, i) == b'-';
        i += 1;
    }
    let mut base = base;
    if (base == 0 || base == 16) && at(s, i) == b'0' && (at(s, i + 1) | 0x20) == b'x' {
        // `0x` só vale como prefixo se vier um dígito hexadecimal depois.
        if digit_value(at(s, i + 2)).is_some_and(|d| d < 16) {
            i += 2;
            base = 16;
        } else if base == 0 {
            base = 8;
        }
    } else if base == 0 {
        base = if at(s, i) == b'0' { 8 } else { 10 };
    }
    let start = i;
    let mut value: u64 = 0;
    let mut overflow = false;
    while let Some(d) = digit_value(at(s, i)) {
        if d >= base {
            break;
        }
        match value
            .checked_mul(u64::from(base))
            .and_then(|v| v.checked_add(u64::from(d)))
        {
            Some(v) => value = v,
            None => {
                overflow = true;
                value = u64::MAX;
            }
        }
        i += 1;
    }
    if i == start {
        return (false, 0, 0, false);
    }
    (neg, value, i, overflow)
}

/// `strtoull(s, &end, base)`: sinal negativo inverte (com volta) a magnitude.
pub fn strtoull(s: &[u8], base: u32) -> Conv<u64> {
    let (neg, mag, used, overflow) = scan_integer(s, base);
    if overflow {
        return Conv {
            value: u64::MAX,
            used,
            overflow,
        };
    }
    let value = if neg { mag.wrapping_neg() } else { mag };
    Conv {
        value,
        used,
        overflow,
    }
}

/// `strtoul` (unsigned long de 64 bits no x86-64).
pub fn strtoul(s: &[u8], base: u32) -> Conv<u64> {
    strtoull(s, base)
}

/// `strtol` (long de 64 bits), com saturação em `i64::MIN`/`i64::MAX`.
pub fn strtol(s: &[u8], base: u32) -> Conv<i64> {
    let (neg, mag, used, overflow) = scan_integer(s, base);
    if neg {
        if overflow || mag > (i64::MAX as u64) + 1 {
            return Conv {
                value: i64::MIN,
                used,
                overflow: true,
            };
        }
        Conv {
            value: (mag as i64).wrapping_neg(),
            used,
            overflow: false,
        }
    } else {
        if overflow || mag > i64::MAX as u64 {
            return Conv {
                value: i64::MAX,
                used,
                overflow: true,
            };
        }
        Conv {
            value: mag as i64,
            used,
            overflow: false,
        }
    }
}

/// `strtod` no locale C: decimal com expoente, `inf`/`infinity`/`nan` e hexadecimal (`0x1p3`).
pub fn strtod(s: &[u8]) -> Conv<f64> {
    let mut i = 0;
    while is_space(at(s, i)) {
        i += 1;
    }
    let sign_start = i;
    let mut neg = false;
    if at(s, i) == b'-' || at(s, i) == b'+' {
        neg = at(s, i) == b'-';
        i += 1;
    }
    let lower = |k: usize| at(s, k).to_ascii_lowercase();
    // inf / infinity / nan
    if lower(i) == b'i' && lower(i + 1) == b'n' && lower(i + 2) == b'f' {
        let mut used = i + 3;
        if (0..5).all(|k| lower(i + 3 + k) == b"inity"[k]) {
            used = i + 8;
        }
        let v = if neg {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
        return Conv {
            value: v,
            used,
            overflow: false,
        };
    }
    if lower(i) == b'n' && lower(i + 1) == b'a' && lower(i + 2) == b'n' {
        let mut used = i + 3;
        if at(s, used) == b'(' {
            let mut k = used + 1;
            while at(s, k).is_ascii_alphanumeric() || at(s, k) == b'_' {
                k += 1;
            }
            if at(s, k) == b')' {
                used = k + 1;
            }
        }
        let v = if neg { -f64::NAN } else { f64::NAN };
        return Conv {
            value: v,
            used,
            overflow: false,
        };
    }
    if at(s, i) == b'0'
        && lower(i + 1) == b'x'
        && let Some((v, used)) = scan_hex_float(s, i + 2)
    {
        return Conv {
            value: if neg { -v } else { v },
            used,
            overflow: v.is_infinite(),
        };
    }
    let digits_start = i;
    let mut saw_digit = false;
    while is_digit(at(s, i)) {
        i += 1;
        saw_digit = true;
    }
    if at(s, i) == b'.' {
        i += 1;
        while is_digit(at(s, i)) {
            i += 1;
            saw_digit = true;
        }
    }
    if !saw_digit {
        return Conv {
            value: 0.0,
            used: 0,
            overflow: false,
        };
    }
    let mut end = i;
    if lower(i) == b'e' {
        let mut k = i + 1;
        if at(s, k) == b'-' || at(s, k) == b'+' {
            k += 1;
        }
        if is_digit(at(s, k)) {
            while is_digit(at(s, k)) {
                k += 1;
            }
            end = k;
        }
    }
    let text = String::from_utf8_lossy(&s[digits_start..end]).into_owned();
    let text = if text.starts_with('.') {
        format!("0{text}")
    } else {
        text
    };
    let v: f64 = text.parse().unwrap_or(0.0);
    let _ = sign_start;
    Conv {
        value: if neg { -v } else { v },
        used: end,
        overflow: v.is_infinite(),
    }
}

fn scan_hex_float(s: &[u8], mut i: usize) -> Option<(f64, usize)> {
    let mut mant: f64 = 0.0;
    let mut exp: i32 = 0;
    let mut saw = false;
    while let Some(d) = digit_value(at(s, i)).filter(|d| *d < 16) {
        mant = mant * 16.0 + f64::from(d);
        i += 1;
        saw = true;
    }
    if at(s, i) == b'.' {
        i += 1;
        while let Some(d) = digit_value(at(s, i)).filter(|d| *d < 16) {
            mant = mant * 16.0 + f64::from(d);
            exp -= 4;
            i += 1;
            saw = true;
        }
    }
    if !saw {
        return None;
    }
    if (at(s, i) | 0x20) == b'p' {
        let c = strtol(&s[i + 1..], 10);
        if c.used > 0 && !at(s, i + 1).is_ascii_whitespace() {
            exp = exp.saturating_add(c.value.clamp(-100_000, 100_000) as i32);
            i += 1 + c.used;
        }
    }
    Some((mant * 2f64.powi(exp), i))
}

/// `strtof`: o `strtod` arredondado pra `float`.
pub fn strtof(s: &[u8]) -> Conv<f32> {
    let c = strtod(s);
    Conv {
        value: c.value as f32,
        used: c.used,
        overflow: c.overflow || (c.value.is_finite() && (c.value as f32).is_infinite()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strtol_bases_and_prefixes() {
        assert_eq!(
            strtol(b"0x1f rest", 0),
            Conv {
                value: 31,
                used: 4,
                overflow: false
            }
        );
        assert_eq!(strtol(b"017", 0).value, 15);
        assert_eq!(
            strtol(b"-12x", 0),
            Conv {
                value: -12,
                used: 3,
                overflow: false
            }
        );
        assert_eq!(strtol(b"  +7", 0).value, 7);
        assert_eq!(strtol(b"x", 0).used, 0);
        // "0x" sem dígito hexadecimal: converte só o zero.
        assert_eq!(
            strtol(b"0xg", 0),
            Conv {
                value: 0,
                used: 1,
                overflow: false
            }
        );
        assert!(strtol(b"99999999999999999999", 10).overflow);
    }

    #[test]
    fn strtoull_wraps_negative() {
        assert_eq!(strtoull(b"-1", 0).value, u64::MAX);
        assert_eq!(strtoull(b"0xffffffffffffffff", 0).value, u64::MAX);
        assert!(strtoull(b"0x1ffffffffffffffff", 0).overflow);
    }

    #[test]
    fn strtod_forms() {
        assert_eq!(strtod(b"1.5e2x").value, 150.0);
        assert_eq!(strtod(b"1.5e2x").used, 5);
        assert_eq!(strtod(b".5").value, 0.5);
        assert_eq!(strtod(b"1e").used, 1);
        assert!(strtod(b"inf").value.is_infinite());
        assert!(strtod(b"nan").value.is_nan());
        assert_eq!(strtod(b"0x1p4").value, 16.0);
        assert_eq!(strtod(b"abc").used, 0);
    }
}
