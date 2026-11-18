//! O CRC-32 no aarch64, na forma em que o `crc32fast` 1.5.5 o implementa. Com a extensão PMULL (parte
//! da `aes`), a partir de 64 bytes: dobra por multiplicação sem vai-um de 4, 8 ou 16 fluxos de 128
//! bits, recolhidos em árvore. Sem ela, ou abaixo disso: a instrução `crc32` da extensão CRC, em três
//! cadeias independentes enquanto a entrada é longa (uma cadeia só espera a latência de cada `crc32x`),
//! juntadas por tabelas que deslocam um estado por um tamanho fixo.
//!
//! Os kernels são funções `#[target_feature]` seguras, só com intrinsics seguras: as cargas saem de
//! `u64::from_le_bytes` montadas com `vcombine_u64`. O único `unsafe` é a chamada de cada kernel em
//! [`crc32_update`], depois de a CPU confirmar as extensões.

use std::arch::aarch64 as arch;
use std::sync::atomic::AtomicU8;

use arch::uint64x2_t;

use super::{cpu_level, crc32_braid, fold_keys, multmodp, x2nmodp};

/// Os kernels, do mais simples ao mais largo; [`crc32_update`] usa o maior que a CPU executa.
const BRAID: u8 = 1;
const CRC: u8 = 2;
const PMULL: u8 = 3;

static LEVEL: AtomicU8 = AtomicU8::new(0);

fn detect() -> u8 {
    if !std::arch::is_aarch64_feature_detected!("crc") {
        BRAID
    } else if std::arch::is_aarch64_feature_detected!("aes") {
        PMULL
    } else {
        CRC
    }
}

// As constantes de dobra, pela distância em bits.
const FOLD_128: [u64; 2] = fold_keys(128);
const FOLD_256: [u64; 2] = fold_keys(256);
const FOLD_512: [u64; 2] = fold_keys(512);
const FOLD_1024: [u64; 2] = fold_keys(1024);
const FOLD_2048: [u64; 2] = fold_keys(2048);

/// Abaixo disso, as cadeias de `crc32` fecham antes da dobra.
const MIN_PMULL_BYTES: usize = 64;

/// O CRC-32 de `data` continuando de `crc`, pelo kernel `level` ou, se a CPU não o executa, pelo
/// maior abaixo dele que ela executa.
#[allow(unsafe_code)]
#[inline]
fn run(level: u8, crc: u32, data: &[u8]) -> u32 {
    match level.min(cpu_level(&LEVEL, detect)) {
        // SAFETY: `detect` só chega a cada nível depois de a CPU confirmar as extensões que o
        // `#[target_feature]` do kernel habilita (`crc` para os dois, `aes` também para o PMULL).
        PMULL if data.len() >= MIN_PMULL_BYTES => unsafe { crc32_pmull(crc, data) },
        PMULL | CRC => unsafe { crc32_chains(crc, data) },
        _ => crc32_braid(crc, data),
    }
}

/// Continua um CRC-32 a partir de `value` (o CRC já finalizado dos bytes anteriores, 0 no começo).
#[inline]
pub fn crc32_update(value: u32, data: &[u8]) -> u32 {
    run(PMULL, value, data)
}

/// Bytes por cadeia nas voltas de três cadeias: os blocos longos enquanto a entrada dá, depois os
/// curtos. Abaixo de `3 * SHORT` = 1,5 KiB, uma cadeia só.
const LONG: usize = 32768;
const SHORT: usize = 512;

/// `shift_table`: `[k][v]` é o byte `v` na posição `k` do estado, deslocado `len` bytes zero adiante.
/// Juntar duas cadeias é deslocar o estado da primeira pelo tamanho da segunda e somar.
const fn shift_table(len: u64) -> [[u32; 256]; 4] {
    let x = x2nmodp(len, 3);
    let mut table = [[0u32; 256]; 4];
    let mut k = 0;
    while k < 4 {
        let mut v = 0;
        while v < 256 {
            table[k][v] = multmodp(x, (v as u32) << (8 * k));
            v += 1;
        }
        k += 1;
    }
    table
}

static LONG_SHIFT: [[u32; 256]; 4] = shift_table(LONG as u64);
static SHORT_SHIFT: [[u32; 256]; 4] = shift_table(SHORT as u64);

/// O estado cru `state` deslocado pelo tamanho para o qual `table` foi feita.
#[inline(always)]
fn shift(table: &[[u32; 256]; 4], state: u32) -> u32 {
    (0..4).fold(0, |acc, k| acc ^ table[k][((state >> (8 * k)) & 0xff) as usize])
}

/// A palavra `i` de 8 bytes de `b`, em little-endian, a ordem que o `crc32x` espera.
#[inline(always)]
fn word(b: &[u8], i: usize) -> u64 {
    u64::from_le_bytes(b[i * 8..i * 8 + 8].try_into().unwrap_or([0; 8]))
}

/// As cadeias de `crc32`: a parte alinhada em 8 bytes vai por palavras, em três cadeias enquanto
/// houver três blocos; o começo e o fim desalinhados, por [`crc_tail`].
#[target_feature(enable = "crc")]
fn crc32_chains(crc: u32, data: &[u8]) -> u32 {
    let head = data.as_ptr().align_offset(8).min(data.len());
    let (pre, rest) = data.split_at(head);
    let (mut words, post) = rest.split_at(rest.len() / 8 * 8);
    let mut c0 = crc_tail(!crc, pre);
    for (n, table) in [(LONG, &LONG_SHIFT), (SHORT, &SHORT_SHIFT)] {
        while words.len() >= 3 * n {
            let (a, rest) = words.split_at(n);
            let (b, rest) = rest.split_at(n);
            let (c, rest) = rest.split_at(n);
            words = rest;
            // A cadeia de A leva o estado corrente; B e C partem do zero, e juntá-las a A é linear.
            let (mut c1, mut c2) = (0, 0);
            let (a, b, c) = (a.as_chunks::<32>().0, b.as_chunks::<32>().0, c.as_chunks::<32>().0);
            for ((qa, qb), qc) in a.iter().zip(b).zip(c) {
                for k in 0..4 {
                    c0 = arch::__crc32d(c0, word(qa, k));
                    c1 = arch::__crc32d(c1, word(qb, k));
                    c2 = arch::__crc32d(c2, word(qc, k));
                }
            }
            c0 = shift(table, c0) ^ c1;
            c0 = shift(table, c0) ^ c2;
        }
    }
    // O que sobra numa cadeia só, desenrolada em oito palavras por volta (como no `crc32fast`).
    let (blocks, rest) = words.as_chunks::<64>();
    let c = blocks.iter().fold(c0, |c, b| (0..8).fold(c, |c, k| arch::__crc32d(c, word(b, k))));
    let c = rest.as_chunks::<8>().0.iter().fold(c, |c, w| arch::__crc32d(c, u64::from_le_bytes(*w)));
    !crc_tail(c, post)
}

/// O estado cru `c` sobre os últimos 0 a 15 bytes: no máximo um `crc32x`, um `crc32w`, um `crc32h` e
/// um `crc32b`, em vez de um `crc32b` por byte.
#[target_feature(enable = "crc")]
#[inline]
fn crc_tail(mut c: u32, mut data: &[u8]) -> u32 {
    if data.is_empty() {
        return c;
    }
    if data.len() >= 8 {
        c = arch::__crc32d(c, word(data, 0));
        data = &data[8..];
    }
    if data.len() >= 4 {
        c = arch::__crc32w(c, u32::from_le_bytes([data[0], data[1], data[2], data[3]]));
        data = &data[4..];
    }
    if data.len() >= 2 {
        c = arch::__crc32h(c, u16::from_le_bytes([data[0], data[1]]));
        data = &data[2..];
    }
    if let Some(&b) = data.first() {
        c = arch::__crc32b(c, b);
    }
    c
}

/// A dobra com PMULL, com tantos fluxos quantos pagam a montagem e o recolhimento no tamanho de
/// `data` (os limites são os de um Apple M4, medidos pelo `crc32fast`).
#[target_feature(enable = "crc,aes")]
fn crc32_pmull(crc: u32, data: &[u8]) -> u32 {
    match data.len() {
        0..=255 => fold_streams::<4>(crc, data, FOLD_512),
        256..=4095 => fold_streams::<8>(crc, data, FOLD_1024),
        _ => fold_streams::<16>(crc, data, FOLD_2048),
    }
}

/// `N` fluxos de 16 bytes avançam `16 * N` bytes por volta (`k`) até sobrar menos que isso; então se
/// recolhem em árvore, a metade de baixo dobrada sobre a de cima, e cada nível absorve a entrada que
/// cobre, se houver. Sobram 16 bytes cujo CRC a partir do zero, seguido dos últimos 0 a 15 bytes, é o
/// CRC de tudo. `data` tem pelo menos `16 * N` bytes, e `N` é 4, 8 ou 16.
#[target_feature(enable = "crc,aes")]
#[inline]
fn fold_streams<const N: usize>(crc: u32, mut data: &[u8], k: [u64; 2]) -> u32 {
    let mut v: [uint64x2_t; N] = [arch::vdupq_n_u64(0); N];
    for x in v.iter_mut() {
        *x = load(&mut data);
    }
    // O estado (invertido, a forma interna) entra nos 4 primeiros bytes.
    v[0] = arch::veorq_u64(v[0], arch::vsetq_lane_u64::<0>(u64::from(!crc), arch::vdupq_n_u64(0)));

    let kv = keys(k);
    while data.len() >= 16 * N {
        for x in v.iter_mut() {
            *x = fold(*x, load(&mut data), kv);
        }
    }

    // Os níveis escritos um a um: num laço, os fluxos deixam de ficar nos registradores.
    macro_rules! halve {
        ($h:expr, $keys:expr) => {
            if N >= 2 * $h {
                let k = keys($keys);
                for i in 0..$h {
                    v[i] = fold(v[i], v[i + $h], k);
                }
                if data.len() >= 16 * $h {
                    for x in v[..$h].iter_mut() {
                        *x = fold(*x, load(&mut data), k);
                    }
                }
            }
        };
    }
    halve!(8, FOLD_1024);
    halve!(4, FOLD_512);
    halve!(2, FOLD_256);
    halve!(1, FOLD_128);

    let c = arch::__crc32d(0, arch::vgetq_lane_u64::<0>(v[0]));
    let c = arch::__crc32d(c, arch::vgetq_lane_u64::<1>(v[0]));
    !crc_tail(c, data)
}

/// As duas constantes de dobra num vetor, a baixa na pista baixa.
#[target_feature(enable = "neon,aes")]
#[inline]
fn keys(k: [u64; 2]) -> uint64x2_t {
    arch::vreinterpretq_u64_p128(u128::from(k[0]) | u128::from(k[1]) << 64)
}

/// Dobra `a` adiante pela distância para a qual `k` foi feita e soma a `b`: `a.lo * k.lo + a.hi * k.hi
/// + b`, sem vai-um.
#[target_feature(enable = "aes")]
#[inline]
fn fold(a: uint64x2_t, b: uint64x2_t, k: uint64x2_t) -> uint64x2_t {
    let lo = arch::vmull_p64(arch::vgetq_lane_u64::<0>(a), arch::vgetq_lane_u64::<0>(k));
    let hi = arch::vmull_high_p64(arch::vreinterpretq_p64_u64(a), arch::vreinterpretq_p64_u64(k));
    arch::veorq_u64(b, arch::veorq_u64(arch::vreinterpretq_u64_p128(lo), arch::vreinterpretq_u64_p128(hi)))
}

/// Tira os próximos 16 bytes de `data` num vetor, as pistas em little-endian. Montado de um `u128`, o
/// LLVM carrega com um `ldr q` só; montado de duas palavras, ele carrega a de cima numa pista à parte.
#[target_feature(enable = "neon,aes")]
#[inline]
fn load(data: &mut &[u8]) -> uint64x2_t {
    let (head, tail) = data.split_at(16);
    *data = tail;
    arch::vreinterpretq_u64_p128(u128::from_le_bytes(head.try_into().unwrap_or([0; 16])))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O deslocamento por tabela contra o byte a byte sobre zeros (o teste do `crc32fast`).
    #[test]
    fn shift_table_appends_zeros() {
        for len in [1u64, 7, 512, 32768] {
            let table = shift_table(len);
            let zeros = vec![0u8; len as usize];
            for state in [0u32, 1, 0x8000_0000, 0xdead_beef, 0xffff_ffff, 0x1234_5678] {
                let expected = !crc32_braid(!state, &zeros);
                assert_eq!(shift(&table, state), expected, "len {len} state {state:#010x}");
            }
        }
    }

    /// Cada kernel que a CPU executa, contra o trançado, em volta de onde as estratégias trocam e
    /// em todo comprimento até depois da primeira volta de três cadeias.
    #[test]
    fn kernels_match_braid() {
        let mut data = vec![0u8; 3 * LONG + 8200];
        let mut s: u32 = 0x1234_5678;
        for b in data.iter_mut() {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *b = (s >> 24) as u8;
        }
        let lens = (0..=1700).chain([4095, 4096, 4097, 8192, 3 * LONG - 1, 3 * LONG, 3 * LONG + 8199]);
        let lens: Vec<usize> = lens.collect();
        for level in CRC..=cpu_level(&LEVEL, detect) {
            for &len in &lens {
                for offset in [0usize, 1, 3, 7, 8] {
                    let Some(slice) = data.get(offset..offset + len) else { continue };
                    for init in [0, 0xdead_beef] {
                        assert_eq!(run(level, init, slice), crc32_braid(init, slice), "nível {level} len {len} offset {offset}");
                    }
                }
            }
        }
    }
}
