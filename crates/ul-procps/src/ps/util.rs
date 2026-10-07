//! Peças de baixo nível do ps: locale, escapes de texto, `strtoul`, `strverscmp` e `wcwidth`.

use std::cmp::Ordering;

/// O locale do processo usa UTF-8? (o `nl_langinfo(CODESET)` depois do `setlocale(LC_ALL, "")`).
pub fn is_utf8() -> bool {
    let var = |n: &str| sysabi::sys::getenv(n).filter(|v| !v.is_empty());
    let loc = var("LC_ALL").or_else(|| var("LC_CTYPE")).or_else(|| var("LANG"));
    match loc {
        Some(l) => {
            let l = String::from_utf8_lossy(&l).to_ascii_lowercase();
            l.contains("utf-8") || l.contains("utf8")
        }
        None => false,
    }
}

/// Quantos bytes tem o caractere que começa com `b` (tabela `UTF_tab` da escape.c); -1 se inválido.
fn utf_len(b: u8) -> i32 {
    match b {
        0x00..=0x7f => 1,
        0x80..=0xc1 => -1,
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => -1,
    }
}

/// `escape_str` da libproc2: copia até o primeiro NUL, no máximo `bufsize - 1` bytes, e troca por `?`
/// o que não é imprimível (no UTF-8, só controles e sequências inválidas; no resto, tudo fora do
/// ASCII visível).
pub fn escape_str_lib(src: &[u8], bufsize: usize) -> Vec<u8> {
    if bufsize == 0 {
        return Vec::new();
    }
    let end = src.iter().position(|b| *b == 0).unwrap_or(src.len());
    let n = end.min(bufsize - 1);
    let mut s = src[..n].to_vec();
    if is_utf8() {
        let len = s.len();
        let mut i = 0;
        while i < len {
            let mut n = utf_len(s[i]);
            let mut bad = n < 0 || i + n as usize > len;
            if !bad && s[i] == 0xc2 && i + 1 < len && (0x80..=0x9f).contains(&s[i + 1]) {
                bad = true;
            }
            if !bad {
                for x in 1..n as usize {
                    if s[i + x] < 0x80 || s[i + x] > 0xbf {
                        bad = true;
                        break;
                    }
                }
            }
            if bad {
                s[i] = b'?';
                n = 1;
            } else if s[i] < 0x20 || s[i] == 0x7f {
                s[i] = b'?';
            }
            i += n as usize;
        }
    } else {
        for b in s.iter_mut() {
            if *b < 0x20 || *b == 0x7f {
                *b = b'.';
            } else if *b >= 0x80 {
                *b = b'?';
            }
        }
    }
    s
}

/// Decodifica um caractere UTF-8 como o `mbrtowc` do glibc: `Some((ponto de código, bytes))` ou
/// `None` se a sequência é inválida (sobrelonga, substituto, acima de U+10FFFF, truncada).
pub fn decode_utf8(s: &[u8]) -> Option<(u32, usize)> {
    let b0 = *s.first()?;
    let (len, mut cp) = match b0 {
        0x00..=0x7f => return Some((u32::from(b0), 1)),
        0xc2..=0xdf => (2, u32::from(b0 & 0x1f)),
        0xe0..=0xef => (3, u32::from(b0 & 0x0f)),
        0xf0..=0xf4 => (4, u32::from(b0 & 0x07)),
        _ => return None,
    };
    if s.len() < len {
        return None;
    }
    for b in &s[1..len] {
        if b & 0xc0 != 0x80 {
            return None;
        }
        cp = (cp << 6) | u32::from(b & 0x3f);
    }
    let min = match len {
        2 => 0x80,
        3 => 0x800,
        _ => 0x1_0000,
    };
    if cp < min || (0xd800..=0xdfff).contains(&cp) || cp > 0x10_ffff {
        return None;
    }
    Some((cp, len))
}

/// `iswprint` aproximado para o C.UTF-8.
fn wc_printable(cp: u32) -> bool {
    !(cp < 0xa0 || (0xd800..=0xdfff).contains(&cp) || cp == 0x2028 || cp == 0x2029 || cp == 0xfffe || cp == 0xffff)
}

/// `wcwidth` do C.UTF-8 sobre o ponto de código decodificado do UTF-8.
pub use ul_common::width::wcwidth_cp as wcwidth;

/// `escape_str` do output.c do ps: copia `src` (até o NUL) para `out` respeitando `bufsize` bytes e
/// `*maxcells` células de tela; controles e inválidos viram `?`. Devolve os bytes escritos e
/// desconta as células usadas de `maxcells`.
pub fn escape_str_out(out: &mut Vec<u8>, src: &[u8], bufsize: i32, maxcells: &mut i32) -> usize {
    if bufsize <= 0 || *maxcells <= 0 {
        return 0;
    }
    let end = src.iter().position(|b| *b == 0).unwrap_or(src.len());
    let src = &src[..end];
    let mut cells = 0i32;
    let mut bytes = 0i32;
    let mut i = 0usize;
    let utf8 = is_utf8();
    let mut bufsize = bufsize;
    if !utf8 && bufsize > *maxcells + 1 {
        bufsize = *maxcells + 1;
    }
    loop {
        if cells >= *maxcells || bytes + 1 >= bufsize || i >= src.len() {
            break;
        }
        if !utf8 {
            let c = src[i];
            i += 1;
            out.push(if (0x20..=0x7e).contains(&c) { c } else if c == 0x7f || c < 0x20 { b'.' } else { b'?' });
            cells += 1;
            bytes += 1;
            continue;
        }
        match decode_utf8(&src[i..]) {
            None => {
                out.push(b'?');
                i += 1;
                cells += 1;
                bytes += 1;
            }
            Some((cp, 1)) => {
                out.push(if (0x20..=0x7e).contains(&cp) { cp as u8 } else { b'?' });
                i += 1;
                cells += 1;
                bytes += 1;
            }
            Some((cp, len)) => {
                if !wc_printable(cp) {
                    out.push(b'?');
                    i += len;
                    cells += 1;
                    bytes += 1;
                } else {
                    let wlen = wcwidth(cp);
                    if wlen > *maxcells - cells || len as i32 >= bufsize - (bytes + 1) {
                        break;
                    }
                    out.extend_from_slice(&src[i..i + len]);
                    i += len;
                    bytes += len as i32;
                    if wlen > 0 {
                        cells += wlen;
                    }
                }
            }
        }
    }
    *maxcells -= cells;
    bytes as usize
}

/// `strtoul(s, &end, 0)`: espaço inicial, sinal, prefixo `0x` ou `0` (octal). Devolve o valor e
/// quantos bytes consumiu (0 se não leu número, como o glibc deixando `end == s`).
pub fn strtoul0(s: &[u8]) -> (u64, usize) {
    let c = ul_common::ctype::strtoull(s, 0);
    (c.value, c.used)
}

fn is_digit(c: u8) -> bool {
    c.is_ascii_digit()
}

/// `strverscmp` do glibc.
pub fn strverscmp(a: &[u8], b: &[u8]) -> Ordering {
    const S_N: usize = 0;
    const S_I: usize = 3;
    const S_F: usize = 6;
    const S_Z: usize = 9;
    const CMP: i8 = 2;
    const LEN: i8 = 3;
    const NEXT_STATE: [usize; 12] = [S_N, S_I, S_Z, S_N, S_I, S_I, S_N, S_F, S_F, S_N, S_F, S_Z];
    const RESULT_TYPE: [i8; 36] = [
        CMP, CMP, CMP, CMP, LEN, CMP, CMP, CMP, CMP, // S_N
        CMP, -1, -1, 1, LEN, LEN, 1, LEN, LEN, // S_I
        CMP, CMP, CMP, CMP, CMP, CMP, CMP, CMP, CMP, // S_F
        CMP, 1, 1, -1, CMP, CMP, -1, CMP, CMP, // S_Z
    ];
    let at = |s: &[u8], i: usize| -> u8 { s.get(i).copied().unwrap_or(0) };
    let mut i1 = 0usize;
    let mut i2 = 0usize;
    let mut c1 = at(a, i1);
    let mut c2 = at(b, i2);
    i1 += 1;
    i2 += 1;
    let mut state = S_N + usize::from(c1 == b'0') + usize::from(is_digit(c1));
    loop {
        let diff = i32::from(c1) - i32::from(c2);
        if diff != 0 {
            let cls2 = usize::from(c2 == b'0') + usize::from(is_digit(c2));
            let r = RESULT_TYPE[state * 3 + cls2];
            return match r {
                CMP => diff.cmp(&0),
                LEN => {
                    loop {
                        let d1 = is_digit(at(a, i1));
                        i1 += 1;
                        if !d1 {
                            break;
                        }
                        let d2 = is_digit(at(b, i2));
                        i2 += 1;
                        if !d2 {
                            return Ordering::Greater;
                        }
                    }
                    if is_digit(at(b, i2)) { Ordering::Less } else { diff.cmp(&0) }
                }
                x if x < 0 => Ordering::Less,
                _ => Ordering::Greater,
            };
        }
        if c1 == 0 {
            return Ordering::Equal;
        }
        state = NEXT_STATE[state];
        c1 = at(a, i1);
        c2 = at(b, i2);
        i1 += 1;
        i2 += 1;
        state += usize::from(c1 == b'0') + usize::from(is_digit(c1));
    }
}
