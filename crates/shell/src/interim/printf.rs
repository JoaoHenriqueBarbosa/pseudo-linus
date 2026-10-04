//! PROVISÓRIO: printf mínimo pra desenvolvimento do núcleo, até chegar o módulo de sala limpa
//! (`src/printf.rs`). Não é entregue.

pub struct Tm {
    pub year: i64,
    pub mon: u32,
    pub mday: u32,
    pub hour: u32,
    pub min: u32,
    pub sec: u32,
    pub wday: u32,
    pub yday: u32,
    pub gmtoff: i64,
    pub zone: Vec<u8>,
    pub isdst: bool,
}

pub trait PrintfEnv {
    fn now(&self) -> i64;
    fn shell_start(&self) -> i64;
    fn localtime(&self, t: i64) -> Option<Tm>;
}

pub struct PrintfOutput {
    pub out: Vec<u8>,
    pub errors: Vec<Vec<u8>>,
    pub status: i32,
}

pub fn strftime(fmt: &[u8], tm: &Tm) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < fmt.len() {
        if fmt[i] != b'%' || i + 1 >= fmt.len() {
            out.push(fmt[i]);
            i += 1;
            continue;
        }
        let c = fmt[i + 1];
        i += 2;
        let s = match c {
            b'Y' => tm.year.to_string(),
            b'm' => format!("{:02}", tm.mon),
            b'd' => format!("{:02}", tm.mday),
            b'H' => format!("{:02}", tm.hour),
            b'M' => format!("{:02}", tm.min),
            b'S' => format!("{:02}", tm.sec),
            b'F' => format!("{}-{:02}-{:02}", tm.year, tm.mon, tm.mday),
            b'T' => format!("{:02}:{:02}:{:02}", tm.hour, tm.min, tm.sec),
            b'Z' => String::from_utf8_lossy(&tm.zone).into_owned(),
            b's' => String::new(),
            b'%' => "%".into(),
            other => format!("%{}", other as char),
        };
        out.extend_from_slice(s.as_bytes());
    }
    out
}

fn num_arg(a: &[u8], errors: &mut Vec<Vec<u8>>, status: &mut i32) -> i64 {
    if a.is_empty() {
        return 0;
    }
    if (a[0] == b'\'' || a[0] == b'"') && a.len() > 1 {
        let s = String::from_utf8_lossy(&a[1..]);
        return s.chars().next().map(|c| c as i64).unwrap_or(0);
    }
    let s = String::from_utf8_lossy(a);
    let t = s.trim_start();
    let (neg, body) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let (base, digits) = if let Some(h) = body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
        (16, h)
    } else if body.len() > 1 && body.starts_with('0') {
        (8, &body[1..])
    } else {
        (10, body)
    };
    let mut v: i64 = 0;
    let mut n = 0;
    for c in digits.chars() {
        match c.to_digit(base) {
            Some(d) => {
                v = v.wrapping_mul(base as i64).wrapping_add(d as i64);
                n += 1;
            }
            None => break,
        }
    }
    if n < digits.len() || (n == 0 && base == 10) {
        let mut e = a.to_vec();
        e.extend_from_slice(b": invalid number");
        errors.push(e);
        *status = 1;
    }
    if neg { -v } else { v }
}

pub fn printf(format: &[u8], args: &[Vec<u8>], _env: &dyn PrintfEnv) -> PrintfOutput {
    let mut out = Vec::new();
    let mut errors = Vec::new();
    let mut status = 0;
    let mut ai = 0;
    loop {
        let start_ai = ai;
        let mut i = 0;
        while i < format.len() {
            let c = format[i];
            if c == b'\\' && i + 1 < format.len() {
                let n = format[i + 1];
                let (max, digit): (usize, fn(u8) -> bool) = match n {
                    b'0'..=b'7' => (3, |d| (b'0'..=b'7').contains(&d)),
                    b'x' => (2, |d| d.is_ascii_hexdigit()),
                    b'u' => (4, |d| d.is_ascii_hexdigit()),
                    b'U' => (8, |d| d.is_ascii_hexdigit()),
                    _ => (0, |_| false),
                };
                let mut j = if matches!(n, b'0'..=b'7') { i + 1 } else { i + 2 };
                let lim = j + max;
                while j < format.len() && j < lim && digit(format[j]) {
                    j += 1;
                }
                let j = j.max(i + 2).min(format.len());
                out.extend(crate::quote::decode_ansi_c(&format[i..j], true));
                i = j;
                continue;
            }
            if c != b'%' {
                out.push(c);
                i += 1;
                continue;
            }
            i += 1;
            if i < format.len() && format[i] == b'%' {
                out.push(b'%');
                i += 1;
                continue;
            }
            let mut flags = String::new();
            while i < format.len() && b"-+ #0".contains(&format[i]) {
                flags.push(format[i] as char);
                i += 1;
            }
            let mut width = String::new();
            while i < format.len() && (format[i].is_ascii_digit() || format[i] == b'*') {
                if format[i] == b'*' {
                    let a = args.get(ai).cloned().unwrap_or_default();
                    ai += 1;
                    width = num_arg(&a, &mut errors, &mut status).to_string();
                } else {
                    width.push(format[i] as char);
                }
                i += 1;
            }
            let mut prec: Option<usize> = None;
            if i < format.len() && format[i] == b'.' {
                i += 1;
                let mut p = String::new();
                while i < format.len() && (format[i].is_ascii_digit() || format[i] == b'*') {
                    if format[i] == b'*' {
                        let a = args.get(ai).cloned().unwrap_or_default();
                        ai += 1;
                        p = num_arg(&a, &mut errors, &mut status).to_string();
                    } else {
                        p.push(format[i] as char);
                    }
                    i += 1;
                }
                prec = Some(p.parse().unwrap_or(0));
            }
            if i >= format.len() {
                break;
            }
            let conv = format[i];
            i += 1;
            let arg = args.get(ai).cloned();
            ai += 1;
            let w: usize = width.parse().unwrap_or(0);
            let left = flags.contains('-');
            let zero = flags.contains('0') && !left;
            let pad = |s: Vec<u8>, numeric: bool| -> Vec<u8> {
                if s.len() >= w {
                    return s;
                }
                let fill = w - s.len();
                if left {
                    let mut v = s;
                    v.extend(std::iter::repeat_n(b' ', fill));
                    v
                } else if zero && numeric {
                    let (sign, digits) = if s.first() == Some(&b'-') { (vec![b'-'], s[1..].to_vec()) } else { (Vec::new(), s) };
                    let mut v = sign;
                    v.extend(std::iter::repeat_n(b'0', fill));
                    v.extend(digits);
                    v
                } else {
                    let mut v: Vec<u8> = std::iter::repeat_n(b' ', fill).collect();
                    v.extend(s);
                    v
                }
            };
            let a = arg.unwrap_or_default();
            let piece = match conv {
                b's' => {
                    let mut s = a.clone();
                    if let Some(p) = prec {
                        s.truncate(p);
                    }
                    pad(s, false)
                }
                b'b' => {
                    let (v, stop) = crate::quote::decode_printf_b(&a);
                    out.extend(pad(v, false));
                    if stop {
                        return PrintfOutput { out, errors, status };
                    }
                    continue;
                }
                b'q' => pad(crate::quote::printf_q(&a, true), false),
                b'c' => pad(a.iter().take(1).copied().collect(), false),
                b'd' | b'i' => {
                    let n = num_arg(&a, &mut errors, &mut status);
                    let mut s = n.to_string();
                    if flags.contains('+') && n >= 0 {
                        s.insert(0, '+');
                    }
                    pad(s.into_bytes(), true)
                }
                b'u' => pad((num_arg(&a, &mut errors, &mut status) as u64).to_string().into_bytes(), true),
                b'x' => pad(format!("{:x}", num_arg(&a, &mut errors, &mut status)).into_bytes(), true),
                b'X' => pad(format!("{:X}", num_arg(&a, &mut errors, &mut status)).into_bytes(), true),
                b'o' => pad(format!("{:o}", num_arg(&a, &mut errors, &mut status)).into_bytes(), true),
                b'f' | b'F' | b'e' | b'E' | b'g' | b'G' => {
                    let f: f64 = String::from_utf8_lossy(&a).trim().parse().unwrap_or(0.0);
                    let p = prec.unwrap_or(6);
                    let s = match conv {
                        b'f' | b'F' => format!("{f:.p$}"),
                        b'e' | b'E' => {
                            let s = format!("{f:.p$e}");
                            let (m, e) = s.split_once('e').unwrap_or((&s, "0"));
                            let ev: i32 = e.parse().unwrap_or(0);
                            format!("{m}e{}{:02}", if ev < 0 { '-' } else { '+' }, ev.abs())
                        }
                        _ => format!("{f}"),
                    };
                    pad(s.into_bytes(), true)
                }
                other => {
                    errors.push(format!("`{}': invalid format character", other as char).into_bytes());
                    status = 1;
                    return PrintfOutput { out, errors, status };
                }
            };
            out.extend(piece);
        }
        if ai >= args.len() || ai == start_ai {
            break;
        }
    }
    PrintfOutput { out, errors, status }
}
