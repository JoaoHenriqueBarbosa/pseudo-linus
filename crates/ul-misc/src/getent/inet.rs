//! Conversões de endereço da glibc 2.41 que o `getent` usa: `inet_pton`, `inet_ntop`, `inet_aton`,
//! `inet_network`, `ether_aton` e `ether_ntoa`. Portadas de `resolv/inet_pton.c`, `inet_ntop.c`,
//! `inet_addr.c`, `inet_net.c` e `inet/ether_aton_r.c`, trabalhando em bytes (as entradas vêm de
//! arquivos e de argv sem garantia de UTF-8).

/// `isspace` do locale C.
pub fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

fn hex_val(b: u8) -> Option<u32> {
    match b {
        b'0'..=b'9' => Some(u32::from(b - b'0')),
        b'a'..=b'f' => Some(u32::from(b - b'a') + 10),
        b'A'..=b'F' => Some(u32::from(b - b'A') + 10),
        _ => None,
    }
}

/// `inet_pton (AF_INET, ...)`: quatro decimais sem zero à esquerda, cada um até 255.
pub fn pton4(src: &[u8]) -> Option<[u8; 4]> {
    let mut tmp = [0u8; 4];
    let mut idx = 0usize;
    let mut saw_digit = false;
    let mut octets = 0;
    for &ch in src {
        if ch.is_ascii_digit() {
            let new = u32::from(tmp[idx]) * 10 + u32::from(ch - b'0');
            if saw_digit && tmp[idx] == 0 {
                return None;
            }
            if new > 255 {
                return None;
            }
            tmp[idx] = new as u8;
            if !saw_digit {
                octets += 1;
                if octets > 4 {
                    return None;
                }
                saw_digit = true;
            }
        } else if ch == b'.' && saw_digit {
            if octets == 4 {
                return None;
            }
            idx += 1;
            tmp[idx] = 0;
            saw_digit = false;
        } else {
            return None;
        }
    }
    if octets < 4 { None } else { Some(tmp) }
}

/// `inet_pton (AF_INET6, ...)`.
pub fn pton6(src: &[u8]) -> Option<[u8; 16]> {
    let mut tmp = [0u8; 16];
    let mut tp = 0usize;
    let mut colonp: Option<usize> = None;
    let mut i = 0usize;
    // `::` no começo pede tratamento especial.
    if src.first() == Some(&b':') {
        i += 1;
        if src.get(i) != Some(&b':') {
            return None;
        }
    }
    let mut curtok = i;
    let mut xdigits_seen = 0;
    let mut val: u32 = 0;
    while i < src.len() {
        let ch = src[i];
        i += 1;
        if let Some(d) = hex_val(ch) {
            if xdigits_seen == 4 {
                return None;
            }
            val = (val << 4) | d;
            if val > 0xffff {
                return None;
            }
            xdigits_seen += 1;
            continue;
        }
        if ch == b':' {
            curtok = i;
            if xdigits_seen == 0 {
                if colonp.is_some() {
                    return None;
                }
                colonp = Some(tp);
                continue;
            } else if i >= src.len() {
                return None;
            }
            if tp + 2 > 16 {
                return None;
            }
            tmp[tp] = (val >> 8) as u8;
            tmp[tp + 1] = val as u8;
            tp += 2;
            xdigits_seen = 0;
            val = 0;
            continue;
        }
        if ch == b'.' && tp + 4 <= 16 {
            if let Some(v4) = pton4(&src[curtok..]) {
                tmp[tp..tp + 4].copy_from_slice(&v4);
                tp += 4;
                xdigits_seen = 0;
                break;
            }
        }
        return None;
    }
    if xdigits_seen > 0 {
        if tp + 2 > 16 {
            return None;
        }
        tmp[tp] = (val >> 8) as u8;
        tmp[tp + 1] = val as u8;
        tp += 2;
    }
    if let Some(cp) = colonp {
        // `::` vira zeros; um campo de largura zero é erro.
        if tp == 16 {
            return None;
        }
        let n = tp - cp;
        // O destino fica depois da origem e pode sobrepor: copia de trás pra frente (memmove).
        for k in (0..n).rev() {
            tmp[16 - n + k] = tmp[cp + k];
        }
        for b in tmp.iter_mut().take(16 - n).skip(cp) {
            *b = 0;
        }
        tp = 16;
    }
    if tp != 16 {
        return None;
    }
    Some(tmp)
}

/// `inet_ntop (AF_INET, ...)`.
pub fn ntop4(a: &[u8; 4]) -> String {
    format!("{}.{}.{}.{}", a[0], a[1], a[2], a[3])
}

/// `inet_ntop (AF_INET6, ...)`: a maior sequência de zeros (a primeira em empate, com pelo menos dois
/// grupos) vira `::`, e `::a.b.c.d` / `::ffff:a.b.c.d` saem com o final em decimal.
pub fn ntop6(a: &[u8; 16]) -> String {
    let mut words = [0u16; 8];
    for (i, w) in words.iter_mut().enumerate() {
        *w = (u16::from(a[2 * i]) << 8) | u16::from(a[2 * i + 1]);
    }
    let (mut best_base, mut best_len): (i32, i32) = (-1, 0);
    let (mut cur_base, mut cur_len): (i32, i32) = (-1, 0);
    for (i, w) in words.iter().enumerate() {
        if *w == 0 {
            if cur_base == -1 {
                cur_base = i as i32;
                cur_len = 1;
            } else {
                cur_len += 1;
            }
        } else if cur_base != -1 {
            if best_base == -1 || cur_len > best_len {
                best_base = cur_base;
                best_len = cur_len;
            }
            cur_base = -1;
        }
    }
    if cur_base != -1 && (best_base == -1 || cur_len > best_len) {
        best_base = cur_base;
        best_len = cur_len;
    }
    if best_base != -1 && best_len < 2 {
        best_base = -1;
    }
    let mut out = String::new();
    let mut i: i32 = 0;
    while i < 8 {
        if best_base != -1 && i >= best_base && i < best_base + best_len {
            if i == best_base {
                out.push(':');
            }
            i += 1;
            continue;
        }
        if i != 0 {
            out.push(':');
        }
        if i == 6 && best_base == 0 && (best_len == 6 || (best_len == 5 && words[5] == 0xffff)) {
            out.push_str(&ntop4(&[a[12], a[13], a[14], a[15]]));
            return out;
        }
        out.push_str(&format!("{:x}", words[i as usize]));
        i += 1;
    }
    if best_base != -1 && best_base + best_len == 8 {
        out.push(':');
    }
    out
}

/// `inet_aton` da glibc: `a`, `a.b`, `a.b.c` ou `a.b.c.d`, cada número em decimal, octal (`0`) ou
/// hexa (`0x`). Devolve o endereço em ordem de host. Com `exact`, qualquer sobra depois do número
/// (inclusive espaço) invalida (o `__inet_aton_exact` do getaddrinfo); sem ele, um espaço e o resto
/// da string são ignorados, como no `inet_aton`.
pub fn inet_aton(s: &[u8], exact: bool) -> Option<u32> {
    let get = |i: usize| -> u8 { s.get(i).copied().unwrap_or(0) };
    let mut i = 0usize;
    let mut parts: Vec<u32> = Vec::new();
    let mut val: u32;
    let mut c = get(i);
    loop {
        if !c.is_ascii_digit() {
            return None;
        }
        // `strtoul(cp, &endp, 0)`: o `0x` só vale com um dígito hexa depois; estouro (`ERANGE`) ou
        // valor acima de 32 bits invalidam.
        let base: u64 = if c == b'0' && (get(i + 1) | 0x20) == b'x' && get(i + 2).is_ascii_hexdigit() {
            i += 2;
            16
        } else if c == b'0' {
            8
        } else {
            10
        };
        let mut ul: u64 = 0;
        let mut overflow = false;
        loop {
            let d = match get(i) {
                d @ b'0'..=b'9' => u64::from(d - b'0'),
                d @ (b'a'..=b'f' | b'A'..=b'F') => u64::from((d | 0x20) - b'a' + 10),
                _ => break,
            };
            if d >= base {
                break;
            }
            match ul.checked_mul(base).and_then(|v| v.checked_add(d)) {
                Some(v) => ul = v,
                None => overflow = true,
            }
            i += 1;
        }
        if overflow || ul > 0xffff_ffff {
            return None;
        }
        val = ul as u32;
        c = get(i);
        if c == b'.' {
            if parts.len() >= 3 || val > 0xff {
                return None;
            }
            parts.push(val);
            i += 1;
            c = get(i);
        } else {
            break;
        }
    }
    if c != 0 && (!c.is_ascii() || !is_space(c)) {
        return None;
    }
    if exact && c != 0 {
        return None;
    }
    let max: [u32; 4] = [0xffff_ffff, 0x00ff_ffff, 0x0000_ffff, 0x0000_00ff];
    if val > max[parts.len()] {
        return None;
    }
    let mut addr = val;
    for (k, p) in parts.iter().enumerate() {
        addr |= p << (24 - 8 * k as u32);
    }
    Some(addr)
}

/// `inet_network` da glibc: cada número vale 8 bits; devolve `0xffffffff` (INADDR_NONE) se inválido.
pub fn inet_network(s: &[u8]) -> u32 {
    const NONE: u32 = 0xffff_ffff;
    let get = |i: usize| -> u8 { s.get(i).copied().unwrap_or(0) };
    let mut i = 0usize;
    let mut parts: Vec<u32> = Vec::new();
    loop {
        let mut val: u32 = 0;
        let mut base = 10u32;
        let mut digit = false;
        if get(i) == b'0' {
            digit = true;
            base = 8;
            i += 1;
        }
        if get(i) == b'x' || get(i) == b'X' {
            base = 16;
            i += 1;
        }
        let mut c = get(i);
        while c != 0 {
            if c.is_ascii_digit() {
                if base == 8 && (c == b'8' || c == b'9') {
                    return NONE;
                }
                val = val.wrapping_mul(base).wrapping_add(u32::from(c - b'0'));
                i += 1;
                digit = true;
            } else if base == 16 && c.is_ascii_hexdigit() {
                val = (val << 4).wrapping_add(hex_val(c).unwrap_or(0));
                i += 1;
                digit = true;
            } else {
                break;
            }
            c = get(i);
        }
        if !digit {
            return NONE;
        }
        if parts.len() >= 4 || val > 0xff {
            return NONE;
        }
        if c == b'.' {
            parts.push(val);
            i += 1;
            continue;
        }
        if get(i) != 0 && !is_space(get(i)) {
            return NONE;
        }
        parts.push(val);
        break;
    }
    if parts.len() > 4 {
        return NONE;
    }
    let mut out = 0u32;
    for p in &parts {
        out = (out << 8) | (p & 0xff);
    }
    out
}

/// `ether_aton`: `x:x:x:x:x:x` com um ou dois dígitos hexa por byte.
pub fn ether_aton(asc: &[u8]) -> Option<[u8; 6]> {
    let get = |i: usize| -> u8 { asc.get(i).copied().unwrap_or(0) };
    let mut out = [0u8; 6];
    let mut i = 0usize;
    for cnt in 0..6 {
        let mut ch = get(i).to_ascii_lowercase();
        i += 1;
        let digit = |c: u8| -> Option<u32> {
            match c {
                b'0'..=b'9' => Some(u32::from(c - b'0')),
                b'a'..=b'f' => Some(u32::from(c - b'a') + 10),
                _ => None,
            }
        };
        let mut number = digit(ch)?;
        ch = get(i).to_ascii_lowercase();
        if (cnt < 5 && ch != b':') || (cnt == 5 && ch != 0 && !is_space(ch)) {
            i += 1;
            let d = digit(ch)?;
            number = (number << 4) + d;
            let after = get(i);
            if cnt < 5 && after != b':' {
                return None;
            }
        }
        out[cnt] = number as u8;
        i += 1;
    }
    Some(out)
}

/// `ether_ntoa`: `%x:%x:%x:%x:%x:%x`, sem zeros à esquerda.
pub fn ether_ntoa(a: &[u8; 6]) -> String {
    format!("{:x}:{:x}:{:x}:{:x}:{:x}:{:x}", a[0], a[1], a[2], a[3], a[4], a[5])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pton_and_ntop() {
        assert_eq!(pton4(b"127.0.0.1"), Some([127, 0, 0, 1]));
        assert_eq!(pton4(b"08.1.1.1"), None);
        assert_eq!(pton4(b"1.2.3"), None);
        assert_eq!(pton6(b"::1").map(|a| ntop6(&a)), Some("::1".to_string()));
        assert_eq!(pton6(b"::ffff:1.2.3.4").map(|a| ntop6(&a)), Some("::ffff:1.2.3.4".to_string()));
        assert_eq!(pton6(b"fe00::0").map(|a| ntop6(&a)), Some("fe00::".to_string()));
        assert_eq!(pton6(b"1:2:3:4:5:6:7:8").map(|a| ntop6(&a)), Some("1:2:3:4:5:6:7:8".to_string()));
        assert_eq!(pton6(b"::").map(|a| ntop6(&a)), Some("::".to_string()));
        assert_eq!(pton6(b"1::2::3"), None);
        assert_eq!(pton6(b":1"), None);
        assert_eq!(ntop6(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4]), "::1.2.3.4");
    }

    #[test]
    fn aton_forms() {
        assert_eq!(inet_aton(b"10.1", true), Some(0x0a00_0001));
        assert_eq!(inet_aton(b"1234", true), Some(1234));
        assert_eq!(inet_aton(b"0x7f.1", true), Some(0x7f00_0001));
        assert_eq!(inet_aton(b"08.1", true), None);
        assert_eq!(inet_aton(b"1.2.3.4 x", false), Some(0x0102_0304));
        assert_eq!(inet_aton(b"1.2.3.4 x", true), None);
        assert_eq!(inet_aton(b"1.2.3.4.5", true), None);
        assert_eq!(inet_network(b"127.0.0.0"), 0x7f00_0000);
        assert_eq!(inet_network(b"10.0.0.0"), 0x0a00_0000);
        assert_eq!(inet_network(b"x"), 0xffff_ffff);
    }

    #[test]
    fn ethers() {
        assert_eq!(ether_aton(b"0:11:22:33:44:55"), Some([0, 0x11, 0x22, 0x33, 0x44, 0x55]));
        assert_eq!(ether_aton(b"00:11:22:33:44"), None);
        assert_eq!(ether_ntoa(&[0, 0x11, 0x22, 0x33, 0x44, 0x55]), "0:11:22:33:44:55");
    }
}
