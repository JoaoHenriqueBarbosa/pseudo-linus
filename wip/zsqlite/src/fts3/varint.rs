//! A parte de varints de `fts3.c`: `sqlite3Fts3PutVarint`, `GetVarint`, `GetVarintU`,
//! `GetVarintBounded`, `GetVarint32` e `VarintLen`.
//!
//! O formato NÃO é o do SQLite: aqui são grupos de 7 bits, o menos significativo primeiro, com o
//! bit alto indicando continuação, e o máximo é 10 bytes ([`FTS3_VARINT_MAX`]). Esta parte do
//! `fts3.c` mora aqui porque `aux.rs`, `expr.rs` e as fatias seguintes a usam.
//!
//! O C lê os bytes além do fim do buffer porque todo buffer de doclist tem
//! `FTS3_BUFFER_PADDING` bytes de folga; aqui o fim da fatia lê como zero (`util::at`).

use crate::util::at;

use super::int::FTS3_VARINT_MAX;

/// `sqlite3Fts3PutVarint`: grava `v` em `p[0..]` e devolve o número de bytes (de 1 a
/// [`FTS3_VARINT_MAX`]). `p` precisa ter espaço para eles.
pub fn fts3_put_varint(p: &mut [u8], v: i64) -> i32 {
    let mut vu = v as u64;
    let mut n = 0usize;
    loop {
        p[n] = ((vu & 0x7f) | 0x80) as u8;
        n += 1;
        vu >>= 7;
        if vu == 0 {
            break;
        }
    }
    p[n - 1] &= 0x7f;
    debug_assert!(n <= FTS3_VARINT_MAX);
    n as i32
}

/// `sqlite3Fts3GetVarintU`: lê um varint de 64 bits sem sinal de `p[0..]`. Devolve o número de
/// bytes lidos e o valor.
pub fn fts3_get_varint_u(p: &[u8]) -> (i32, u64) {
    /* Os quatro primeiros bytes cabem num u32 (as macros GETVARINT_INIT e GETVARINT_STEP). */
    let mut a: u32 = at(p, 0) as u32;
    if (a & 0x80) == 0 {
        return (1, a as u64);
    }
    a = (a & 0x7F) | ((at(p, 1) as u32) << 7);
    if (a & 0x4000) == 0 {
        return (2, a as u64);
    }
    a = (a & 0x3FFF) | ((at(p, 2) as u32) << 14);
    if (a & 0x20_0000) == 0 {
        return (3, a as u64);
    }
    a = (a & 0x1F_FFFF) | ((at(p, 3) as u32) << 21);
    if (a & 0x1000_0000) == 0 {
        return (4, a as u64);
    }
    let mut b: u64 = (a & 0x0FFF_FFFF) as u64;
    let mut pos = 4usize;
    let mut shift = 28u32;
    while shift <= 63 {
        let c = at(p, pos) as u64;
        pos += 1;
        b = b.wrapping_add((c & 0x7F) << shift);
        if (c & 0x80) == 0 {
            break;
        }
        shift += 7;
    }
    (pos as i32, b)
}

/// `sqlite3Fts3GetVarint`: o mesmo que [`fts3_get_varint_u`] com o valor com sinal.
pub fn fts3_get_varint(p: &[u8]) -> (i32, i64) {
    let (n, v) = fts3_get_varint_u(p);
    (n, v as i64)
}

/// `sqlite3Fts3GetVarintBounded`: como [`fts3_get_varint`], mas sem ler além do fim da fatia
/// (os bytes que faltam valem zero). O C recebe `pBuf` e `pEnd`; aqui `p` termina em `pEnd`.
pub fn fts3_get_varint_bounded(p: &[u8]) -> (i32, i64) {
    let mut b: u64 = 0;
    let mut pos = 0usize;
    let mut shift = 0u32;
    while shift <= 63 {
        let c = if pos < p.len() { p[pos] as u64 } else { 0 };
        pos += 1;
        b = b.wrapping_add((c & 0x7F) << shift);
        if (c & 0x80) == 0 {
            break;
        }
        shift += 7;
    }
    (pos as i32, b as i64)
}

/// `sqlite3Fts3GetVarint32` e a macro `fts3GetVarint32`: lê um varint truncado a um inteiro de
/// 32 bits não negativo. Um primeiro byte sem o bit alto é o caso rápido da macro (1 byte); o
/// resto é o corpo da função.
pub fn fts3_get_varint32(p: &[u8]) -> (i32, i32) {
    let mut a: u32 = at(p, 0) as u32;
    if (a & 0x80) == 0 {
        return (1, a as i32);
    }
    a = (a & 0x7F) | ((at(p, 1) as u32) << 7);
    if (a & 0x4000) == 0 {
        return (2, a as i32);
    }
    a = (a & 0x3FFF) | ((at(p, 2) as u32) << 14);
    if (a & 0x20_0000) == 0 {
        return (3, a as i32);
    }
    a = (a & 0x1F_FFFF) | ((at(p, 3) as u32) << 21);
    if (a & 0x1000_0000) == 0 {
        return (4, a as i32);
    }
    a &= 0x0FFF_FFFF;
    let v = a | (((at(p, 4) & 0x07) as u32) << 28);
    debug_assert!(v & 0x8000_0000 == 0);
    (5, v as i32)
}

/// `sqlite3Fts3VarintLen`: o número de bytes que `v` ocupa como varint.
pub fn fts3_varint_len(v: u64) -> i32 {
    let mut v = v;
    let mut i = 0;
    loop {
        i += 1;
        v >>= 7;
        if v == 0 {
            break;
        }
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let mut buf = [0u8; 16];
        for &v in &[0i64, 1, 127, 128, 16383, 16384, 0x0FFF_FFFF, 0x1000_0000, 0x7FFF_FFFF, i64::MAX, -1] {
            let n = fts3_put_varint(&mut buf, v);
            assert_eq!(n, fts3_varint_len(v as u64));
            let (m, w) = fts3_get_varint(&buf);
            assert_eq!((m, w), (n, v));
            let (m2, w2) = fts3_get_varint_bounded(&buf[..n as usize]);
            assert_eq!((m2, w2), (n, v));
        }
        let n = fts3_put_varint(&mut buf, 0x7FFF_FFFF);
        assert_eq!(fts3_get_varint32(&buf), (n, 0x7FFF_FFFF));
        assert_eq!(fts3_put_varint(&mut buf, -1), 10);
    }
}
