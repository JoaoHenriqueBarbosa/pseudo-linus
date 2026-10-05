//! `parse_size` do `lib/strutils.c` do util-linux (domínio público): número com base automática
//! (`0x` hexadecimal, `0` octal) e sufixo opcional `K M G T P E Z Y` (potência de 1024), `KiB`
//! (1024) ou `KB` (1000), com parte fracionária antes do sufixo (`1.5K`). Negativo é EINVAL;
//! estouro é ERANGE.

use sysabi::Errno;

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')
}

fn at(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

/// `strtoumax(s + i, &end, 0)`: valor, fim e se estourou.
fn strtoumax(s: &[u8], i: usize) -> (u64, usize, bool) {
    let mut j = i;
    while is_space(at(s, j)) {
        j += 1;
    }
    let neg = match at(s, j) {
        b'+' => {
            j += 1;
            false
        }
        b'-' => {
            j += 1;
            true
        }
        _ => false,
    };
    let (base, mut k) = if at(s, j) == b'0'
        && matches!(at(s, j + 1), b'x' | b'X')
        && at(s, j + 2).is_ascii_hexdigit()
    {
        (16, j + 2)
    } else if at(s, j) == b'0' {
        (8, j)
    } else {
        (10, j)
    };
    let start = k;
    let mut v: u64 = 0;
    let mut overflow = false;
    while let Some(d) = (at(s, k) as char).to_digit(base) {
        match v
            .checked_mul(u64::from(base))
            .and_then(|x| x.checked_add(u64::from(d)))
        {
            Some(x) => v = x,
            None => overflow = true,
        }
        k += 1;
    }
    if k == start {
        return (0, i, false);
    }
    if overflow {
        return (u64::MAX, k, true);
    }
    (if neg { v.wrapping_neg() } else { v }, k, false)
}

fn scale(x: &mut u64, base: u64, power: u32) -> Result<(), Errno> {
    for _ in 0..power {
        *x = x.checked_mul(base).ok_or(Errno::ERANGE)?;
    }
    Ok(())
}

/// O `strtosize` do util-linux.
pub fn parse_size(s: &[u8]) -> Result<u64, Errno> {
    // O C para no NUL.
    let s = match s.iter().position(|&b| b == 0) {
        Some(n) => &s[..n],
        None => s,
    };
    if s.is_empty() {
        return Err(Errno::EINVAL);
    }
    let mut p = 0;
    while is_space(at(s, p)) {
        p += 1;
    }
    if at(s, p) == b'-' {
        return Err(Errno::EINVAL);
    }
    let (mut x, end, overflow) = strtoumax(s, 0);
    if end == 0 {
        return Err(Errno::EINVAL);
    }
    if overflow {
        return Err(Errno::ERANGE);
    }
    if end >= s.len() {
        return Ok(x);
    }
    let mut p = end;
    let mut frac: u64 = 0;
    let mut frac_zeros = 0u32;
    let mut base: u64 = 1024;
    loop {
        if at(s, p + 1) == b'i' && matches!(at(s, p + 2), b'B' | b'b') && at(s, p + 3) == 0 {
            base = 1024;
        } else if matches!(at(s, p + 1), b'B' | b'b') && at(s, p + 2) == 0 {
            base = 1000;
        } else if at(s, p + 1) != 0 {
            // Ponto decimal (o do C.UTF-8 é `.`).
            if frac == 0 && at(s, p) == b'.' {
                let mut q = p + 1;
                while at(s, q) == b'0' {
                    frac_zeros += 1;
                    q += 1;
                }
                let fend;
                if at(s, q).is_ascii_digit() {
                    let (f, e, ov) = strtoumax(s, q);
                    if e == q {
                        return Err(Errno::EINVAL);
                    }
                    if ov {
                        return Err(Errno::ERANGE);
                    }
                    frac = f;
                    fend = e;
                } else {
                    fend = q;
                }
                if frac != 0 && fend >= s.len() {
                    return Err(Errno::EINVAL);
                }
                p = fend;
                continue;
            }
            return Err(Errno::EINVAL);
        }
        break;
    }
    const SUF: &[u8] = b"KMGTPEZY";
    const SUF2: &[u8] = b"kmgtpezy";
    let c = at(s, p);
    let pwr = match SUF
        .iter()
        .position(|&b| b == c && c != 0)
        .or_else(|| SUF2.iter().position(|&b| b == c && c != 0))
    {
        Some(i) => i as u32 + 1,
        None => return Err(Errno::EINVAL),
    };
    let rc = scale(&mut x, base, pwr);
    if frac != 0 && pwr != 0 {
        let mut frac_base: u64 = 1;
        let _ = scale(&mut frac_base, base, pwr);
        let mut frac_div: u64 = 10;
        let mut frac_poz: u64 = 1;
        while frac_div < frac {
            if frac_div <= u64::MAX / 10 {
                frac_div *= 10;
            } else {
                frac /= 10;
            }
        }
        for _ in 0..frac_zeros {
            if frac_div <= u64::MAX / 10 {
                frac_div *= 10;
            } else {
                frac /= 10;
            }
        }
        loop {
            let seg = frac % 10;
            let seg_div = frac_div / frac_poz;
            frac /= 10;
            frac_poz = frac_poz.wrapping_mul(10);
            if seg != 0 && seg_div / seg != 0 {
                x = x.wrapping_add(frac_base / (seg_div / seg));
            }
            if frac == 0 {
                break;
            }
        }
    }
    rc.map(|()| x)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Valores conferidos com `hexdump -n` no oráculo (ver os casos `hexdump-size-*`).
    #[test]
    fn sizes() {
        assert_eq!(parse_size(b"5"), Ok(5));
        assert_eq!(parse_size(b"0x10"), Ok(16));
        assert_eq!(parse_size(b"010"), Ok(8));
        assert_eq!(parse_size(b"1K"), Ok(1024));
        assert_eq!(parse_size(b"1KiB"), Ok(1024));
        assert_eq!(parse_size(b"1KB"), Ok(1000));
        assert_eq!(parse_size(b"1.5K"), Ok(1536));
        assert_eq!(parse_size(b"1.K"), Ok(1024));
        assert_eq!(parse_size(b"0.5MB"), Ok(500_000));
        assert_eq!(parse_size(b"0.5MiB"), Ok(524_288));
        assert_eq!(parse_size(b"2k"), Ok(2048));
        assert_eq!(parse_size(b"abc"), Err(Errno::EINVAL));
        assert_eq!(parse_size(b"-1"), Err(Errno::EINVAL));
        assert_eq!(parse_size(b"5x"), Err(Errno::EINVAL));
        assert_eq!(parse_size(b""), Err(Errno::EINVAL));
        assert_eq!(parse_size(b"1R"), Err(Errno::EINVAL));
        assert_eq!(parse_size(b"1.5"), Err(Errno::EINVAL));
        assert_eq!(parse_size(b"99999999999999999999999"), Err(Errno::ERANGE));
        assert_eq!(parse_size(b"1Y"), Err(Errno::ERANGE));
        assert_eq!(parse_size(b" +7"), Ok(7));
    }
}
