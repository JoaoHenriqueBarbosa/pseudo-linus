//! Um `printf` da glibc pra um formato com uma conversão só, do jeito que o hexdump usa: o texto
//! antes do `%` sai literal, a especificação segue a gramática da glibc (flags `-+ #0`, largura,
//! `.precisão`, modificadores `l`/`ll`, conversão) e o argumento é um só.
//!
//! Comportamentos da glibc 2.41 que importam aqui (conferidos no oráculo):
//!
//! - conversão desconhecida sai literal, do `%` até o caractere inválido, e o resto do formato
//!   continua como texto (`%5-3llx` sai como está);
//! - formato que acaba no meio da especificação (`%ll` no fim) faz o `printf` parar com EINVAL: o
//!   texto anterior já saiu, o resto não;
//! - `%c` e `%s` sempre completam a largura com espaço (o `0` não vale pra eles);
//! - inteiros: precisão é o mínimo de dígitos, `%.0d` com zero não imprime dígito, `#` no `o`
//!   garante o zero à esquerda e no `x`/`X` põe `0x`/`0X` se o valor não é zero; `+` e espaço só
//!   valem pros com sinal; `0` é ignorado com precisão ou com `-`;
//! - ponto flutuante: `%e %E %f %F %g %G` com arredondamento exato (metade pro par), `inf`, `nan`
//!   e `-nan` (o sinal do NaN aparece), expoente com pelo menos dois dígitos.

/// O argumento da conversão.
#[derive(Clone, Copy, Debug)]
pub enum Arg<'a> {
    /// Inteiro (os bits; `%d` com `ll` lê como `i64`, sem `l` como `i32`).
    Int(u64),
    Dbl(f64),
    /// Cadeia já cortada no NUL (o `%s` da glibc para no NUL).
    Str(&'a [u8]),
}

#[derive(Clone, Copy, Debug, Default)]
struct Flags {
    minus: bool,
    plus: bool,
    space: bool,
    alt: bool,
    zero: bool,
}

/// Formata `fmt` com `arg` e acrescenta em `out`.
pub fn cprintf(out: &mut Vec<u8>, fmt: &[u8], arg: Arg<'_>) {
    let mut i = 0;
    while i < fmt.len() {
        let b = fmt[i];
        if b != b'%' {
            out.push(b);
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        let mut f = Flags::default();
        while let Some(&c) = fmt.get(i) {
            match c {
                b'-' => f.minus = true,
                b'+' => f.plus = true,
                b' ' => f.space = true,
                b'#' => f.alt = true,
                b'0' => f.zero = true,
                _ => break,
            }
            i += 1;
        }
        let mut width = 0usize;
        while let Some(&d) = fmt.get(i).filter(|c| c.is_ascii_digit()) {
            width = width.saturating_mul(10).saturating_add(usize::from(d - b'0'));
            i += 1;
        }
        let mut prec = None;
        if fmt.get(i) == Some(&b'.') {
            i += 1;
            let mut p = 0usize;
            while let Some(&d) = fmt.get(i).filter(|c| c.is_ascii_digit()) {
                p = p.saturating_mul(10).saturating_add(usize::from(d - b'0'));
                i += 1;
            }
            prec = Some(p);
        }
        let mut longs = 0;
        while fmt.get(i) == Some(&b'l') {
            longs += 1;
            i += 1;
        }
        let Some(&conv) = fmt.get(i) else {
            // Especificação incompleta no fim: a glibc devolve -1 (EINVAL) e para aqui.
            return;
        };
        i += 1;
        match conv {
            b'%' => out.push(b'%'),
            b'd' | b'i' => {
                let v = match arg {
                    Arg::Int(v) if longs > 0 => v as i64,
                    Arg::Int(v) => i64::from(v as u32 as i32),
                    _ => 0,
                };
                fmt_signed(out, f, width, prec, v);
            }
            b'o' | b'u' | b'x' | b'X' => {
                let v = match arg {
                    Arg::Int(v) if longs > 0 => v,
                    Arg::Int(v) => u64::from(v as u32),
                    _ => 0,
                };
                fmt_unsigned(out, f, width, prec, conv, v);
            }
            b'c' => {
                let c = match arg {
                    Arg::Int(v) => v as u8,
                    _ => 0,
                };
                pad_str(out, f.minus, width, &[c]);
            }
            b's' => {
                let s = match arg {
                    Arg::Str(s) => s,
                    _ => b"(null)".as_slice(),
                };
                let s = match prec {
                    Some(p) if p < s.len() => &s[..p],
                    _ => s,
                };
                pad_str(out, f.minus, width, s);
            }
            b'e' | b'E' | b'f' | b'F' | b'g' | b'G' => {
                let v = match arg {
                    Arg::Dbl(v) => v,
                    _ => 0.0,
                };
                fmt_float(out, f, width, prec, conv, v);
            }
            _ => out.extend_from_slice(&fmt[start..i]),
        }
    }
}

fn pad_str(out: &mut Vec<u8>, left: bool, width: usize, s: &[u8]) {
    let pad = width.saturating_sub(s.len());
    if !left {
        out.extend(std::iter::repeat_n(b' ', pad));
    }
    out.extend_from_slice(s);
    if left {
        out.extend(std::iter::repeat_n(b' ', pad));
    }
}

/// Junta prefixo (sinal, `0x`), zeros de precisão já aplicados nos dígitos e o preenchimento da
/// largura (espaços, ou zeros quando vale o flag `0`).
fn emit_num(out: &mut Vec<u8>, f: Flags, width: usize, zero_ok: bool, prefix: &[u8], digits: &[u8]) {
    let len = prefix.len() + digits.len();
    let pad = width.saturating_sub(len);
    if f.minus {
        out.extend_from_slice(prefix);
        out.extend_from_slice(digits);
        out.extend(std::iter::repeat_n(b' ', pad));
    } else if f.zero && zero_ok {
        out.extend_from_slice(prefix);
        out.extend(std::iter::repeat_n(b'0', pad));
        out.extend_from_slice(digits);
    } else {
        out.extend(std::iter::repeat_n(b' ', pad));
        out.extend_from_slice(prefix);
        out.extend_from_slice(digits);
    }
}

fn apply_precision(mut digits: Vec<u8>, prec: Option<usize>, is_zero: bool) -> Vec<u8> {
    match prec {
        Some(0) if is_zero => Vec::new(),
        Some(p) if digits.len() < p => {
            let mut v = vec![b'0'; p - digits.len()];
            v.append(&mut digits);
            v
        }
        _ => digits,
    }
}

fn fmt_signed(out: &mut Vec<u8>, f: Flags, width: usize, prec: Option<usize>, v: i64) {
    let mag = v.unsigned_abs();
    let digits = apply_precision(mag.to_string().into_bytes(), prec, mag == 0);
    let prefix: &[u8] = if v < 0 {
        b"-"
    } else if f.plus {
        b"+"
    } else if f.space {
        b" "
    } else {
        b""
    };
    emit_num(out, f, width, prec.is_none(), prefix, &digits);
}

fn fmt_unsigned(out: &mut Vec<u8>, f: Flags, width: usize, prec: Option<usize>, conv: u8, v: u64) {
    let raw = match conv {
        b'o' => format!("{v:o}"),
        b'x' => format!("{v:x}"),
        b'X' => format!("{v:X}"),
        _ => v.to_string(),
    };
    let mut digits = apply_precision(raw.into_bytes(), prec, v == 0);
    let mut prefix: &[u8] = b"";
    if f.alt {
        match conv {
            b'o' if digits.first() != Some(&b'0') => digits.insert(0, b'0'),
            b'x' if v != 0 => prefix = b"0x",
            b'X' if v != 0 => prefix = b"0X",
            _ => {}
        }
    }
    emit_num(out, f, width, prec.is_none(), prefix, &digits);
}

/// Dígitos de `%e` (sem sinal): mantissa com `prec` casas e o expoente decimal.
fn exp_parts(v: f64, prec: usize) -> (String, i32) {
    // O `{:e}` do Rust é exato e arredonda metade pro par, como a glibc; só o expoente muda de
    // forma ("e5" no Rust, "e+05" no C).
    let s = format!("{:.*e}", prec, v);
    let (mant, exp) = s.split_once('e').unwrap_or((&s, "0"));
    (mant.to_string(), exp.parse().unwrap_or(0))
}

fn exp_suffix(upper: bool, exp: i32) -> String {
    let e = if upper { 'E' } else { 'e' };
    let sign = if exp < 0 { '-' } else { '+' };
    format!("{e}{sign}{:02}", exp.unsigned_abs())
}

fn strip_zeros(s: &mut String) {
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
}

fn fmt_float(out: &mut Vec<u8>, f: Flags, width: usize, prec: Option<usize>, conv: u8, v: f64) {
    let upper = conv.is_ascii_uppercase();
    let neg = v.is_sign_negative();
    let prefix: &[u8] = if neg {
        b"-"
    } else if f.plus {
        b"+"
    } else if f.space {
        b" "
    } else {
        b""
    };
    if !v.is_finite() {
        let body = match (v.is_nan(), upper) {
            (true, false) => "nan",
            (true, true) => "NAN",
            (false, false) => "inf",
            (false, true) => "INF",
        };
        emit_num(out, f, width, false, prefix, body.as_bytes());
        return;
    }
    let a = v.abs();
    let p = prec.unwrap_or(6);
    let body = match conv {
        b'f' | b'F' => {
            let mut s = format!("{:.*}", p, a);
            if f.alt && p == 0 {
                s.push('.');
            }
            s
        }
        b'e' | b'E' => {
            let (mut mant, exp) = exp_parts(a, p);
            if f.alt && p == 0 {
                mant.push('.');
            }
            mant + &exp_suffix(upper, exp)
        }
        _ => {
            let p = if p == 0 { 1 } else { p };
            let (_, x) = if a == 0.0 { (String::new(), 0) } else { exp_parts(a, p - 1) };
            if (x as i64) < p as i64 && x >= -4 {
                let fp = (p as i64 - 1 - x as i64) as usize;
                let mut s = format!("{:.*}", fp, a);
                if f.alt {
                    if !s.contains('.') {
                        s.push('.');
                    }
                } else {
                    strip_zeros(&mut s);
                }
                s
            } else {
                let (mut mant, exp) = exp_parts(a, p - 1);
                if f.alt {
                    if !mant.contains('.') {
                        mant.push('.');
                    }
                } else {
                    strip_zeros(&mut mant);
                }
                mant + &exp_suffix(upper, exp)
            }
        }
    };
    emit_num(out, f, width, true, prefix, body.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(fmt: &str, arg: Arg<'_>) -> String {
        let mut out = Vec::new();
        cprintf(&mut out, fmt.as_bytes(), arg);
        String::from_utf8_lossy(&out).into_owned()
    }

    // Saídas esperadas capturadas do `printf` do bash 5.2.37 (que repassa a especificação ao
    // printf(3) da glibc 2.41) no oráculo pseudo-linus-oracle:719900900623.
    #[test]
    fn integers_like_glibc() {
        assert_eq!(p("[%05s]", Arg::Str(b"ab")), "[   ab]");
        assert_eq!(p("[%5c]", Arg::Int(b'x' as u64)), "[    x]");
        assert_eq!(p("[%-5c]", Arg::Int(b'z' as u64)), "[z    ]");
        assert_eq!(p("[%+llu]", Arg::Int(5)), "[5]");
        assert_eq!(p("[%#llo]", Arg::Int(0)), "[0]");
        assert_eq!(p("[%#.0llo]", Arg::Int(0)), "[0]");
        assert_eq!(p("[%.0lld]", Arg::Int(0)), "[]");
        assert_eq!(p("[%#llx]", Arg::Int(0)), "[0]");
        assert_eq!(p("[%#.3llx]", Arg::Int(1)), "[0x001]");
        assert_eq!(p("[%08.3lld]", Arg::Int((-12i64) as u64)), "[    -012]");
        assert_eq!(p("[%-08lld]", Arg::Int((-12i64) as u64)), "[-12     ]");
        assert_eq!(p("[%+05lld]", Arg::Int(12)), "[+0012]");
        assert_eq!(p("[% 05lld]", Arg::Int(12)), "[ 0012]");
        assert_eq!(p("[%#5llo]", Arg::Int(8)), "[  010]");
        assert_eq!(p("[%#05llx]", Arg::Int(255)), "[0x0ff]");
        assert_eq!(p("[%#-6llX]", Arg::Int(255)), "[0XFF  ]");
        assert_eq!(p("[%#.3llo]", Arg::Int(8)), "[010]");
        assert_eq!(p("[%5.0lld]", Arg::Int(0)), "[     ]");
        assert_eq!(p("[%+.0lld]", Arg::Int(0)), "[+]");
        assert_eq!(p("[%5.1s]", Arg::Str(b"abc")), "[    a]");
    }

    // Saídas do hexdump do oráculo (`-e '1/4 "FMT|"'` e `1/8`), ver os casos `hexdump-float-*`.
    #[test]
    fn floats_like_glibc() {
        assert_eq!(p("%f", Arg::Dbl(1.0)), "1.000000");
        assert_eq!(p("%e", Arg::Dbl(0.1)), "1.000000e-01");
        assert_eq!(p("%g", Arg::Dbl(0.1)), "0.1");
        assert_eq!(p("%E", Arg::Dbl(f64::INFINITY)), "INF");
        assert_eq!(p("%G", Arg::Dbl(-f64::NAN)), "-NAN");
        assert_eq!(p("%08.2f", Arg::Dbl(f64::NEG_INFINITY)), "    -inf");
        assert_eq!(p("%#g", Arg::Dbl(2.0)), "2.00000");
        assert_eq!(p("%#.0f", Arg::Dbl(2.0)), "2.");
        assert_eq!(p("%#.0e", Arg::Dbl(0.1)), "1.e-01");
        assert_eq!(p("%+g", Arg::Dbl(0.0)), "+0");
        assert_eq!(p("% f", Arg::Dbl(1.0)), " 1.000000");
        assert_eq!(p("%.10g", Arg::Dbl(f64::from(0.1f32))), "0.1000000015");
        assert_eq!(p("%#.3g", Arg::Dbl(255.0)), "255.");
        assert_eq!(p("%#.3g", Arg::Dbl(10.0)), "10.0");
        assert_eq!(p("%010.4e", Arg::Dbl(-2.5)), "-2.5000e+00");
        assert_eq!(p("%.17g", Arg::Dbl(0.1)), "0.10000000000000001");
        assert_eq!(p("%g", Arg::Dbl(-0.0)), "-0");
        assert_eq!(p("%.2f", Arg::Dbl(0.125)), "0.12");
        assert_eq!(p("%.0f", Arg::Dbl(2.5)), "2");
        assert_eq!(p("%.0f", Arg::Dbl(3.5)), "4");
        assert_eq!(p("%g", Arg::Dbl(1e21)), "1e+21");
        assert_eq!(p("%g", Arg::Dbl(1e-5)), "1e-05");
        assert_eq!(p("%g", Arg::Dbl(0.0001)), "0.0001");
        assert_eq!(p("%g", Arg::Dbl(123456.0)), "123456");
        assert_eq!(p("%g", Arg::Dbl(1234567.0)), "1.23457e+06");
    }

    #[test]
    fn unknown_and_incomplete() {
        assert_eq!(p("ab%5-3llx", Arg::Int(1)), "ab%5-3llx");
        assert_eq!(p("ab%ll", Arg::Int(1)), "ab");
        assert_eq!(p("x%%y", Arg::Int(1)), "x%y");
    }
}
