//! O `printf` da glibc pras descrições das regras: um formato com no máximo uma conversão (o
//! `check_format` do apprentice garante) e um argumento já convertido pro tipo que o C passaria.
//! Cobre bandeiras `-+ #0`, largura, precisão, modificadores de tamanho e as conversões
//! `d i u o x X c s e E f F g G %`.

use super::cutil::is_digit;

/// O argumento do `file_printf`, com o tipo que o C empilha no varargs.
#[derive(Clone, Copy, Debug)]
pub enum Arg<'a> {
    /// `int` ou `unsigned int` (os tipos de 8, 16 e 32 bits já promovidos): os 32 bits.
    Int(u32),
    /// `long long` ou `unsigned long long`.
    Long(u64),
    Str(&'a [u8]),
    Double(f64),
    None,
}

#[derive(Default, Clone, Copy)]
struct Spec {
    left: bool,
    plus: bool,
    space: bool,
    alt: bool,
    zero: bool,
    width: usize,
    prec: Option<usize>,
    /// 0 = int, -1 = h, -2 = hh, 1 = l, 2 = ll (e q, j, z, t, L).
    len: i8,
    conv: u8,
}

/// Formata `fmt` com um argumento.
pub fn format(fmt: &[u8], arg: Arg<'_>) -> Vec<u8> {
    let mut out = Vec::with_capacity(fmt.len() + 16);
    let mut i = 0usize;
    while i < fmt.len() {
        let c = fmt[i];
        if c != b'%' {
            out.push(c);
            i += 1;
            continue;
        }
        i += 1;
        if i < fmt.len() && fmt[i] == b'%' {
            out.push(b'%');
            i += 1;
            continue;
        }
        let mut sp = Spec::default();
        while i < fmt.len() {
            match fmt[i] {
                b'-' => sp.left = true,
                b'+' => sp.plus = true,
                b' ' => sp.space = true,
                b'#' => sp.alt = true,
                b'0' => sp.zero = true,
                b'\'' => {}
                _ => break,
            }
            i += 1;
        }
        while i < fmt.len() && is_digit(fmt[i]) {
            sp.width = sp
                .width
                .saturating_mul(10)
                .saturating_add(usize::from(fmt[i] - b'0'));
            i += 1;
        }
        if i < fmt.len() && fmt[i] == b'.' {
            i += 1;
            let mut p = 0usize;
            while i < fmt.len() && is_digit(fmt[i]) {
                p = p
                    .saturating_mul(10)
                    .saturating_add(usize::from(fmt[i] - b'0'));
                i += 1;
            }
            sp.prec = Some(p);
        }
        loop {
            match fmt.get(i) {
                Some(b'h') => sp.len = if sp.len == -1 { -2 } else { -1 },
                Some(b'l') => sp.len = if sp.len == 1 { 2 } else { 1 },
                Some(b'q' | b'j' | b'L') => sp.len = 2,
                Some(b'z' | b't') => sp.len = 1,
                _ => break,
            }
            i += 1;
        }
        let Some(&conv) = fmt.get(i) else {
            // Formato truncado: a glibc imprime o `%` e o que veio.
            out.push(b'%');
            break;
        };
        i += 1;
        sp.conv = conv;
        format_one(&mut out, &sp, arg);
    }
    out
}

fn pad(out: &mut Vec<u8>, body: &[u8], sp: &Spec, zero_ok: bool) {
    let n = body.len();
    if n >= sp.width {
        out.extend_from_slice(body);
        return;
    }
    let fill = sp.width - n;
    if sp.left {
        out.extend_from_slice(body);
        out.extend(std::iter::repeat_n(b' ', fill));
    } else if zero_ok && sp.zero {
        // Zeros entram depois do sinal e do prefixo `0x`.
        let mut split = 0;
        if body
            .first()
            .is_some_and(|c| matches!(c, b'-' | b'+' | b' '))
        {
            split = 1;
        }
        if body.len() >= split + 2 && body[split] == b'0' && matches!(body[split + 1], b'x' | b'X')
        {
            split += 2;
        }
        out.extend_from_slice(&body[..split]);
        out.extend(std::iter::repeat_n(b'0', fill));
        out.extend_from_slice(&body[split..]);
    } else {
        out.extend(std::iter::repeat_n(b' ', fill));
        out.extend_from_slice(body);
    }
}

/// O valor inteiro como o `va_arg` leria com o modificador do formato.
fn int_value(arg: Arg<'_>, len: i8, signed: bool) -> (bool, u64) {
    let raw: u64 = match arg {
        Arg::Int(v) => u64::from(v),
        Arg::Long(v) => v,
        Arg::Double(d) => d as i64 as u64,
        Arg::Str(_) | Arg::None => 0,
    };
    // `%d` lendo um `long long` pega os 32 bits de baixo (x86-64); `%lld` lendo um `int` não
    // acontece (o `check_format` do apprentice exige `ll` só nos tipos de 64 bits).
    let bits = match len {
        -2 => 8,
        -1 => 16,
        0 => 32,
        _ => {
            if matches!(arg, Arg::Long(_)) {
                64
            } else {
                32
            }
        }
    };
    let masked = if bits == 64 {
        raw
    } else {
        raw & ((1u64 << bits) - 1)
    };
    if signed {
        let v: i64 = match bits {
            8 => masked as u8 as i8 as i64,
            16 => masked as u16 as i16 as i64,
            32 => masked as u32 as i32 as i64,
            _ => masked as i64,
        };
        (v < 0, v.unsigned_abs())
    } else {
        (false, masked)
    }
}

fn format_one(out: &mut Vec<u8>, sp: &Spec, arg: Arg<'_>) {
    match sp.conv {
        b'd' | b'i' => {
            let (neg, mag) = int_value(arg, sp.len, true);
            let mut digits = mag.to_string().into_bytes();
            apply_int_precision(&mut digits, sp.prec, mag == 0);
            let mut body = Vec::new();
            if neg {
                body.push(b'-');
            } else if sp.plus {
                body.push(b'+');
            } else if sp.space {
                body.push(b' ');
            }
            body.extend_from_slice(&digits);
            pad(out, &body, sp, sp.prec.is_none());
        }
        b'u' | b'o' | b'x' | b'X' => {
            let (_, v) = int_value(arg, sp.len, false);
            let mut digits = match sp.conv {
                b'u' => v.to_string().into_bytes(),
                b'o' => format!("{v:o}").into_bytes(),
                b'x' => format!("{v:x}").into_bytes(),
                _ => format!("{v:X}").into_bytes(),
            };
            apply_int_precision(&mut digits, sp.prec, v == 0);
            let mut body = Vec::new();
            if sp.alt {
                match sp.conv {
                    b'o' => {
                        if digits.first() != Some(&b'0') {
                            body.push(b'0');
                        }
                    }
                    b'x' if v != 0 => body.extend_from_slice(b"0x"),
                    b'X' if v != 0 => body.extend_from_slice(b"0X"),
                    _ => {}
                }
            }
            body.extend_from_slice(&digits);
            pad(out, &body, sp, sp.prec.is_none());
        }
        b'c' => {
            let (_, v) = int_value(arg, 0, false);
            pad(out, &[v as u8], sp, false);
        }
        b's' => {
            let s: &[u8] = match arg {
                Arg::Str(s) => super::cutil::cstr(s),
                _ => b"(null)",
            };
            let s = match sp.prec {
                Some(p) if p < s.len() => &s[..p],
                _ => s,
            };
            pad(out, s, sp, false);
        }
        b'e' | b'E' | b'f' | b'F' | b'g' | b'G' => {
            let d = match arg {
                Arg::Double(d) => d,
                Arg::Int(v) => f64::from(v),
                Arg::Long(v) => v as f64,
                _ => 0.0,
            };
            let body = format_double(d, sp);
            let finite = d.is_finite();
            pad(out, &body, sp, finite);
        }
        other => {
            // Conversão desconhecida: a glibc imprime o texto do formato como veio.
            out.push(b'%');
            out.push(other);
        }
    }
}

fn apply_int_precision(digits: &mut Vec<u8>, prec: Option<usize>, zero: bool) {
    if let Some(p) = prec {
        if p == 0 && zero {
            digits.clear();
            return;
        }
        if digits.len() < p {
            let mut z = vec![b'0'; p - digits.len()];
            z.extend_from_slice(digits);
            *digits = z;
        }
    }
}

/// `%e %f %g` com as regras do C (expoente com sinal e pelo menos dois dígitos, `%g` sem zeros
/// à direita a não ser com `#`).
fn format_double(d: f64, sp: &Spec) -> Vec<u8> {
    let upper = sp.conv.is_ascii_uppercase();
    let mut body = Vec::new();
    let neg = d.is_sign_negative() && !d.is_nan();
    if neg {
        body.push(b'-');
    } else if sp.plus {
        body.push(b'+');
    } else if sp.space {
        body.push(b' ');
    }
    let a = d.abs();
    if !d.is_finite() {
        let t = if d.is_nan() { "nan" } else { "inf" };
        body.extend_from_slice(
            if upper {
                t.to_uppercase()
            } else {
                t.to_string()
            }
            .as_bytes(),
        );
        return body;
    }
    let prec = sp.prec.unwrap_or(6);
    let text = match sp.conv.to_ascii_lowercase() {
        b'f' => {
            let mut t = format!("{a:.prec$}");
            if sp.alt && prec == 0 {
                t.push('.');
            }
            t
        }
        b'e' => exp_format(a, prec, sp.alt),
        _ => {
            let p = if prec == 0 { 1 } else { prec };
            // Expoente que o %e daria com precisão p-1.
            let x = if a == 0.0 {
                0
            } else {
                let e = exp_format(a, p - 1, false);
                e.rsplit('e')
                    .next()
                    .and_then(|s| s.parse::<i32>().ok())
                    .unwrap_or(0)
            };
            let mut t = if p as i32 > x && x >= -4 {
                let fp = (p as i32 - 1 - x) as usize;
                format!("{a:.fp$}")
            } else {
                exp_format(a, p - 1, false)
            };
            if !sp.alt {
                t = strip_g_zeros(&t);
            }
            t
        }
    };
    let text = if upper { text.to_uppercase() } else { text };
    body.extend_from_slice(text.as_bytes());
    body
}

fn exp_format(a: f64, prec: usize, alt: bool) -> String {
    let s = format!("{a:.prec$e}");
    let (mant, exp) = s.split_once('e').unwrap_or((&s, "0"));
    let e: i32 = exp.parse().unwrap_or(0);
    let mut m = mant.to_string();
    if alt && prec == 0 {
        m.push('.');
    }
    format!("{m}e{}{:02}", if e < 0 { '-' } else { '+' }, e.abs())
}

fn strip_g_zeros(t: &str) -> String {
    let (mant, exp) = match t.find('e') {
        Some(i) => (&t[..i], &t[i..]),
        None => (t, ""),
    };
    let mant = if mant.contains('.') {
        mant.trim_end_matches('0').trim_end_matches('.')
    } else {
        mant
    };
    format!("{mant}{exp}")
}

/// Classe do argumento que uma conversão consome (pra o `fmtcheck`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Int(i8),
    Str,
    Double,
}

fn first_conv(fmt: &[u8]) -> Option<Option<Kind>> {
    let mut i = 0;
    let mut found: Option<Kind> = None;
    let mut count = 0;
    while i < fmt.len() {
        if fmt[i] != b'%' {
            i += 1;
            continue;
        }
        i += 1;
        if fmt.get(i) == Some(&b'%') {
            i += 1;
            continue;
        }
        while i < fmt.len() && b"-+ #0'".contains(&fmt[i]) {
            i += 1;
        }
        if fmt.get(i) == Some(&b'*') {
            return None;
        }
        while i < fmt.len() && (is_digit(fmt[i]) || fmt[i] == b'.') {
            i += 1;
        }
        let mut len: i8 = 0;
        loop {
            match fmt.get(i) {
                Some(b'h') => len = if len == -1 { -2 } else { -1 },
                Some(b'l') => len = if len == 1 { 2 } else { 1 },
                Some(b'q' | b'j' | b'L') => len = 2,
                Some(b'z' | b't') => len = 1,
                _ => break,
            }
            i += 1;
        }
        let k = match fmt.get(i)? {
            b'd' | b'i' | b'u' | b'o' | b'x' | b'X' | b'c' => {
                Kind::Int(if len < 0 { 0 } else { len })
            }
            b's' => Kind::Str,
            b'e' | b'E' | b'f' | b'F' | b'g' | b'G' | b'a' | b'A' => Kind::Double,
            _ => return None,
        };
        i += 1;
        count += 1;
        if count > 1 {
            return None;
        }
        found = Some(k);
    }
    Some(found)
}

/// `fmtcheck(desc, def)`: devolve `desc` se ele consome os mesmos argumentos que `def`,
/// senão `def`.
pub fn fmtcheck<'a>(desc: &'a [u8], def: &'a [u8]) -> &'a [u8] {
    if !desc.contains(&b'%') {
        return desc;
    }
    let a = first_conv(desc);
    let b = first_conv(def);
    match (a, b) {
        (Some(x), Some(y)) if x == y => desc,
        (Some(None), _) => desc,
        _ => def,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(fmt: &str, a: Arg<'_>) -> String {
        String::from_utf8(format(fmt.as_bytes(), a)).unwrap()
    }

    #[test]
    fn integers() {
        assert_eq!(f("%d x", Arg::Int(42)), "42 x");
        assert_eq!(f("%d", Arg::Int(-1i32 as u32)), "-1");
        assert_eq!(f("%u", Arg::Int(-1i32 as u32)), "4294967295");
        assert_eq!(f("%x", Arg::Int(-1i32 as u32)), "ffffffff");
        assert_eq!(f("%#x", Arg::Int(0)), "0");
        assert_eq!(f("%#x", Arg::Int(255)), "0xff");
        assert_eq!(f("%#o", Arg::Int(8)), "010");
        assert_eq!(f("%04X", Arg::Int(0xab)), "00AB");
        assert_eq!(f("%-4d|", Arg::Int(7)), "7   |");
        assert_eq!(f("%.3d", Arg::Int(7)), "007");
        assert_eq!(f("%lld", Arg::Long(-5i64 as u64)), "-5");
        assert_eq!(f("%llx", Arg::Long(u64::MAX)), "ffffffffffffffff");
        assert_eq!(f("%c", Arg::Int(u32::from(b'A'))), "A");
        assert_eq!(f("100%%", Arg::None), "100%");
    }

    #[test]
    fn strings_and_doubles() {
        assert_eq!(f("[%s]", Arg::Str(b"abc\0def")), "[abc]");
        assert_eq!(f("%.2s", Arg::Str(b"abc")), "ab");
        assert_eq!(f("%5s|", Arg::Str(b"ab")), "   ab|");
        assert_eq!(f("%g", Arg::Double(0.5)), "0.5");
        assert_eq!(f("%g", Arg::Double(100000.0)), "100000");
        assert_eq!(f("%g", Arg::Double(1000000.0)), "1e+06");
        assert_eq!(f("%g", Arg::Double(0.0001)), "0.0001");
        assert_eq!(f("%g", Arg::Double(0.00001)), "1e-05");
        assert_eq!(f("%.2f", Arg::Double(3.14159)), "3.14");
        assert_eq!(f("%e", Arg::Double(1500.0)), "1.500000e+03");
        assert_eq!(f("%g", Arg::Double(f64::INFINITY)), "inf");
    }

    #[test]
    fn fmtcheck_compat() {
        assert_eq!(fmtcheck(b"v%d", b"%d"), b"v%d");
        assert_eq!(fmtcheck(b"v%s", b"%d"), b"%d");
        assert_eq!(fmtcheck(b"none", b"%s"), b"none");
        assert_eq!(fmtcheck(b"%x", b"%u"), b"%x");
    }
}
