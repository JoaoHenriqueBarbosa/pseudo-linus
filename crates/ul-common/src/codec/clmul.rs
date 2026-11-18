//! O CRC-32 por dobra com multiplicação sem vai-um (PCLMULQDQ), do artigo da Intel *Fast CRC
//! Computation for Generic Polynomials Using PCLMULQDQ Instruction* (Gopal et al., 2009), a partir do
//! `crc32fast` 1.5.5: quatro registradores de 128 bits, e as variantes largas com VPCLMULQDQ, oito
//! fluxos em registradores de 256 bits (AVX2) e dezesseis em registradores de 512 bits (AVX-512).
//!
//! Onde este porte se afasta do `crc32fast`, cada mudança medida contra ele:
//!
//! - os fluxos se recolhem em árvore no fim, como no kernel do aarch64 dele, e não um a um;
//! - a redução final vai de 96 bits a 32 por Barrett com `⌊x^95 / P⌋`, três multiplicações em vez de
//!   quatro;
//! - o bloco seguinte do laço é carregado uma volta antes, o que tira um XOR do caminho de cada fluxo;
//! - os laços pedem a entrada ao cache 2 KiB adiante;
//! - até 16 bytes, um slicing desenrolado para cada tamanho faz tudo numa rodada de consultas.
//!
//! Os kernels são funções `#[target_feature]` seguras, só com intrinsics seguras: as cargas saem de
//! `u64::from_le_bytes` montadas com `_mm*_set_epi64*`, que o LLVM junta numa carga só. O único
//! `unsafe` é a chamada de cada kernel em [`crc32_update`], depois de a CPU confirmar as features.

use std::arch::x86_64 as arch;
use std::sync::atomic::AtomicU8;

use arch::{__m128i, __m256i, __m512i};

use super::{CRC32_POLY, CRC32_TABLE, cpu_level, crc32_braid, fold_keys};

// As constantes de dobra, pela distância: 128 bits (um registrador de 128 sobre o seguinte), 512 (a
// volta de quatro deles), 1024 (a volta dos oito fluxos do AVX2) e 2048 (a dos dezesseis do AVX-512);
// as intermediárias recolhem os fluxos no fim.
const FOLD_128: [u64; 2] = fold_keys(128);
const FOLD_256: [u64; 2] = fold_keys(256);
const FOLD_512: [u64; 2] = fold_keys(512);
const FOLD_1024: [u64; 2] = fold_keys(1024);
const FOLD_2048: [u64; 2] = fold_keys(2048);
/// O polinômio com o termo x^32, refletido em 33 bits.
const POLY_33: u64 = ((CRC32_POLY as u64) << 1) | 1;

/// O quociente de Barrett `⌊x^95 / P⌋`, de grau 63, refletido em 64 bits: com ele, a redução de 96
/// bits a 32 sai de duas multiplicações sem máscara entre elas (o artigo vai de 64 bits, com
/// `⌊x^64 / P⌋`, e precisa de mais uma dobra antes).
const BARRETT_95: u64 = {
    let poly = (1u128 << 32) | CRC32_POLY.reverse_bits() as u128;
    let mut rest = 1u128 << 95;
    let mut quotient = 0u128;
    let mut i = 95;
    while i >= 32 {
        if rest & (1 << i) != 0 {
            rest ^= poly << (i - 32);
            quotient |= 1 << (i - 32);
        }
        i -= 1;
    }
    (quotient as u64).reverse_bits()
};

/// Abaixo disso, montar e recolher os oito fluxos do AVX2 custa mais do que rende.
const MIN_AVX2_BYTES: usize = 256;
/// Abaixo disso, montar e recolher os dezesseis fluxos do AVX-512 custa mais do que rende (o
/// `crc32fast` só entra com 2 KiB porque recolhe os dezesseis um a um).
const MIN_AVX512_BYTES: usize = 512;
/// A que distância à frente os laços pedem a entrada ao cache (medido de 512 a 4096: 2 KiB é o que
/// mais ganha do L2 para fora).
const PREFETCH: usize = 2048;

/// Os kernels, do mais simples ao mais largo; [`crc32_update`] usa o maior que a CPU executa.
const BRAID: u8 = 1;
const SSE: u8 = 2;
const AVX2: u8 = 3;
const AVX512: u8 = 4;

static LEVEL: AtomicU8 = AtomicU8::new(0);

fn detect() -> u8 {
    let sse = is_x86_feature_detected!("pclmulqdq") && is_x86_feature_detected!("sse4.1") && is_x86_feature_detected!("ssse3");
    let wide = sse && is_x86_feature_detected!("vpclmulqdq") && is_x86_feature_detected!("avx") && is_x86_feature_detected!("avx2");
    if wide && is_x86_feature_detected!("avx512f") {
        AVX512
    } else if wide {
        AVX2
    } else if sse {
        SSE
    } else {
        BRAID
    }
}

/// O CRC-32 de `data` continuando de `crc`, pelo kernel `level` ou, se a CPU não o executa, pelo
/// maior abaixo dele que ela executa.
#[allow(unsafe_code)]
#[inline]
fn run(level: u8, crc: u32, data: &[u8]) -> u32 {
    // Até um registrador: nenhum kernel ajuda.
    if data.len() <= 16 {
        return crc32_short(crc, data);
    }
    match level.min(cpu_level(&LEVEL, detect)) {
        // SAFETY: `detect` só chega a cada nível depois de a CPU confirmar todas as features que o
        // `#[target_feature]` do kernel habilita.
        AVX512 => unsafe { avx512(crc, data) },
        AVX2 => unsafe { avx2(crc, data) },
        SSE => unsafe { sse(crc, data) },
        _ => crc32_braid(crc, data),
    }
}

/// Continua um CRC-32 a partir de `value` (o CRC já finalizado dos bytes anteriores, 0 no começo).
#[inline]
pub fn crc32_update(value: u32, data: &[u8]) -> u32 {
    run(AVX512, value, data)
}

/// As dezesseis tabelas do slicing: `[k][n]` é o CRC cru do byte `n` seguido de `k` bytes zero.
static CRC32_SLICES: [[u32; 256]; 16] = {
    let mut tables = [[0u32; 256]; 16];
    tables[0] = CRC32_TABLE;
    let mut k = 1;
    while k < 16 {
        let mut n = 0;
        while n < 256 {
            let prev = tables[k - 1][n];
            tables[k][n] = CRC32_TABLE[(prev & 0xff) as usize] ^ (prev >> 8);
            n += 1;
        }
        k += 1;
    }
    tables
};

/// O CRC-32 de até 16 bytes, continuando de `crc`: o slicing faz tudo numa rodada de consultas, antes
/// de a dobra pagar a redução final (que custa o mesmo para 16 bytes ou para 31).
#[inline]
fn crc32_short(crc: u32, data: &[u8]) -> u32 {
    if data.is_empty() { crc } else { crc32_1_to_16(crc, data) }
}

/// De 1 a 16 bytes numa rodada só de consultas, desenrolada para cada tamanho (o `crc32fast` faz 8,
/// depois 4, depois byte a byte, uma rodada encadeada em cada).
#[inline(never)]
fn crc32_1_to_16(crc: u32, data: &[u8]) -> u32 {
    let c = !crc;
    !match data.len() {
        1 => slice::<1>(c, data),
        2 => slice::<2>(c, data),
        3 => slice::<3>(c, data),
        4 => slice::<4>(c, data),
        5 => slice::<5>(c, data),
        6 => slice::<6>(c, data),
        7 => slice::<7>(c, data),
        8 => slice::<8>(c, data),
        9 => slice::<9>(c, data),
        10 => slice::<10>(c, data),
        11 => slice::<11>(c, data),
        12 => slice::<12>(c, data),
        13 => slice::<13>(c, data),
        14 => slice::<14>(c, data),
        15 => slice::<15>(c, data),
        _ => slice::<16>(c, data),
    }
}

/// Os `N` primeiros bytes de `data` no registrador cru `c`: o byte `i` passa pela tabela `N - 1 - i`.
/// Só os quatro primeiros se misturam a `c` antes da consulta, e só eles ficam no caminho do CRC; os
/// de 4 em diante não dependem dele e se juntam antes. Com menos de 4 bytes, o que o registrador tinha
/// além deles desce `8 * N` bits.
#[inline(always)]
fn slice<const N: usize>(c: u32, data: &[u8]) -> u32 {
    let k = N.min(4);
    let free = (k..N).fold(0, |acc, i| acc ^ CRC32_SLICES[N - 1 - i][data[i] as usize]);
    let t = |i: usize| CRC32_SLICES[N - 1 - i][((u32::from(data[i]) ^ (c >> (8 * i))) & 0xff) as usize];
    let dep = match k {
        1 => t(0),
        2 => t(0) ^ t(1),
        3 => t(0) ^ t(1) ^ t(2),
        _ => (t(0) ^ t(1)) ^ (t(2) ^ t(3)),
    };
    c.checked_shr(8 * N as u32).unwrap_or(0) ^ free ^ dep
}

/// O par de constantes num registrador, a parte baixa na pista baixa.
#[target_feature(enable = "sse2")]
#[inline]
fn keys(k: [u64; 2]) -> __m128i {
    arch::_mm_set_epi64x(k[1] as i64, k[0] as i64)
}

/// O último bloco parcial (1 a 15 bytes) em duas palavras little-endian completadas com zero, por
/// cargas que se sobrepõem: montar nos registradores evita a cópia por um buffer na pilha, cujas
/// escritas estreitas a carga de 16 bytes seguinte não consegue aproveitar.
#[inline]
fn partial_words(data: &[u8]) -> (u64, u64) {
    let n = data.len();
    let v: u128 = if n >= 8 {
        let lo = u64::from_le_bytes(data[..8].try_into().unwrap_or([0; 8]));
        let hi = u64::from_le_bytes(data[n - 8..].try_into().unwrap_or([0; 8]));
        u128::from(lo) | u128::from(hi) << (8 * (n - 8))
    } else if n >= 4 {
        let lo = u32::from_le_bytes(data[..4].try_into().unwrap_or([0; 4]));
        let hi = u32::from_le_bytes(data[n - 4..].try_into().unwrap_or([0; 4]));
        u128::from(lo) | u128::from(hi) << (8 * (n - 4))
    } else {
        u128::from(data[0]) | u128::from(data[n / 2]) << (8 * (n / 2)) | u128::from(data[n - 1]) << (8 * (n - 1))
    };
    (v as u64, (v >> 64) as u64)
}

/// Pede ao cache as `LINES` linhas de 64 bytes que o laço vai ler daqui a [`PREFETCH`] bytes: o
/// prefetch do hardware sozinho deixa a dobra esperando a memória a partir do L2. O ponteiro pode
/// passar do fim de `data`; um prefetch nunca falha.
#[target_feature(enable = "sse")]
#[inline]
fn prefetch<const LINES: usize>(data: &[u8]) {
    for line in 0..LINES {
        arch::_mm_prefetch::<{ arch::_MM_HINT_T0 }>(data.as_ptr().wrapping_add(PREFETCH + 64 * line).cast());
    }
}

/// A palavra `i` de 64 bits de `b`, em little-endian.
#[inline(always)]
fn word(b: &[u8], i: usize) -> i64 {
    i64::from_le_bytes(b[i * 8..i * 8 + 8].try_into().unwrap_or([0; 8]))
}

/// Tira os próximos 16 bytes de `data` num registrador.
#[target_feature(enable = "sse2")]
#[inline]
fn get(data: &mut &[u8]) -> __m128i {
    let (head, tail) = data.split_at(16);
    *data = tail;
    arch::_mm_set_epi64x(word(head, 1), word(head, 0))
}

#[target_feature(enable = "avx")]
#[inline]
fn get256(data: &mut &[u8]) -> __m256i {
    let (head, tail) = data.split_at(32);
    *data = tail;
    arch::_mm256_set_epi64x(word(head, 3), word(head, 2), word(head, 1), word(head, 0))
}

#[target_feature(enable = "avx512f")]
#[inline]
fn get512(data: &mut &[u8]) -> __m512i {
    let (head, tail) = data.split_at(64);
    *data = tail;
    let w = |i| word(head, i);
    arch::_mm512_set_epi64(w(7), w(6), w(5), w(4), w(3), w(2), w(1), w(0))
}

/// Dobra `a` sobre `b`: as duas metades de `a` multiplicadas pelas constantes de `keys`, somadas a `b`.
#[target_feature(enable = "pclmulqdq,sse2")]
#[inline]
fn reduce128(a: __m128i, b: __m128i, keys: __m128i) -> __m128i {
    let t1 = arch::_mm_clmulepi64_si128(a, keys, 0x00);
    let t2 = arch::_mm_clmulepi64_si128(a, keys, 0x11);
    arch::_mm_xor_si128(arch::_mm_xor_si128(b, t1), t2)
}

#[target_feature(enable = "avx2,vpclmulqdq")]
#[inline]
fn reduce256(a: __m256i, b: __m256i, keys: __m256i) -> __m256i {
    let t1 = arch::_mm256_clmulepi64_epi128(a, keys, 0x00);
    let t2 = arch::_mm256_clmulepi64_epi128(a, keys, 0x11);
    arch::_mm256_xor_si256(arch::_mm256_xor_si256(b, t1), t2)
}

#[target_feature(enable = "avx512f,vpclmulqdq")]
#[inline]
fn reduce512(a: __m512i, b: __m512i, keys: __m512i) -> __m512i {
    let t1 = arch::_mm512_clmulepi64_epi128(a, keys, 0x00);
    let t2 = arch::_mm512_clmulepi64_epi128(a, keys, 0x11);
    arch::_mm512_xor_si512(arch::_mm512_xor_si512(b, t1), t2)
}

/// O fim comum dos kernels: dobra os blocos inteiros de 16 bytes que sobraram, encaixa o último
/// pedaço (menos de 16 bytes) deslocando o acumulador, e reduz os 128 bits a 32 por Barrett.
#[target_feature(enable = "pclmulqdq,sse2,sse4.1,ssse3")]
fn reduce_128_to_crc(mut x: __m128i, mut data: &[u8]) -> u32 {
    let k3k4 = keys(FOLD_128);
    while data.len() >= 16 {
        x = reduce128(x, get(&mut data), k3k4);
    }

    // Os `n` bytes finais: os `n` últimos do acumulador saem (`overflow`) e voltam dobrados 128 bits
    // adiante, enquanto `x` desce para abrir espaço para os bytes novos. As máscaras do shuffle saem
    // do próprio `n`, sem tabela.
    let n = data.len();
    if n > 0 {
        let seq = arch::_mm_setr_epi8(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15);
        let shl = arch::_mm_add_epi8(seq, arch::_mm_set1_epi8(n as i8 - 16));
        let shr = arch::_mm_xor_si128(shl, arch::_mm_set1_epi8(-128));
        let overflow = arch::_mm_shuffle_epi8(x, shl);
        x = arch::_mm_shuffle_epi8(x, shr);
        let (lo, hi) = partial_words(data);
        let part = arch::_mm_set_epi64x(hi as i64, lo as i64);
        x = arch::_mm_xor_si128(x, arch::_mm_shuffle_epi8(part, shl));
        x = reduce128(overflow, x, k3k4);
    }

    // De 128 para 96 bits: a pista baixa (os termos de grau alto, refletidos) multiplicada por
    // x^96 mod P, mais a pista alta. Fica `W`, de grau menor que 96, com `W ≡ V · x^32`.
    let w = arch::_mm_xor_si128(arch::_mm_clmulepi64_si128(x, k3k4, 0x10), arch::_mm_srli_si128(x, 8));

    // Barrett de 96 para 32 bits: q = ⌊⌊W / x^32⌋ · ⌊x^95 / P⌋ / x^63⌋ é a pista baixa do primeiro
    // produto, e o CRC é W + q · P nos 32 bits de baixo (a terceira palavra, refletida). Três
    // multiplicações no caminho, contra quatro (e três máscaras) na redução do artigo.
    let mu_p = arch::_mm_set_epi64x(POLY_33 as i64, BARRETT_95 as i64);
    let q = arch::_mm_clmulepi64_si128(w, mu_p, 0x00);
    let t = arch::_mm_clmulepi64_si128(q, mu_p, 0x10);
    !(arch::_mm_extract_epi32(arch::_mm_xor_si128(w, t), 2) as u32)
}

// Os três kernels terminam do mesmo jeito, que é o do kernel do aarch64 do `crc32fast` (o do x86 dele
// recolhe os fluxos um a um, uma dobra encadeada em cada): os fluxos se recolhem em árvore, a metade de
// baixo dobrada sobre a de cima, e cada nível ainda absorve a entrada que ele cobre, se houver. Com
// oito fluxos, são três dobras no caminho em vez de oito.

/// PCLMULQDQ de 128 bits, dobrando quatro registradores (64 bytes por volta).
#[target_feature(enable = "pclmulqdq,sse2,sse4.1,ssse3")]
fn sse(crc: u32, mut data: &[u8]) -> u32 {
    if data.len() <= 16 {
        return crc32_short(crc, data);
    }
    let init = arch::_mm_cvtsi32_si128(!crc as i32);
    if data.len() < 32 {
        return reduce_128_to_crc(arch::_mm_xor_si128(get(&mut data), init), data);
    }
    // De 32 a 63 bytes, dois fluxos; de 64 em diante, quatro, recolhidos em dois.
    let mut pair = if data.len() < 64 {
        [arch::_mm_xor_si128(get(&mut data), init), get(&mut data)]
    } else {
        let mut v = [arch::_mm_xor_si128(get(&mut data), init), get(&mut data), get(&mut data), get(&mut data)];
        if data.len() >= 64 {
            // O bloco seguinte é carregado uma volta antes: chegando ao laço como valor já pronto, ele
            // entra no XOR com o primeiro produto, e só o segundo fica no caminho de cada fluxo (montado
            // dentro da volta, o LLVM soma os dois produtos primeiro e põe um XOR a mais nesse caminho).
            let k = keys(FOLD_512);
            let mut next = [get(&mut data), get(&mut data), get(&mut data), get(&mut data)];
            loop {
                for (r, b) in v.iter_mut().zip(next) {
                    *r = reduce128(*r, b, k);
                }
                prefetch::<1>(data);
                if data.len() < 64 {
                    break;
                }
                next = [get(&mut data), get(&mut data), get(&mut data), get(&mut data)];
            }
        }
        let k = keys(FOLD_256);
        [reduce128(v[0], v[2], k), reduce128(v[1], v[3], k)]
    };
    let k = keys(FOLD_256);
    if data.len() >= 32 {
        for r in &mut pair {
            *r = reduce128(*r, get(&mut data), k);
        }
    }
    reduce_128_to_crc(reduce128(pair[0], pair[1], keys(FOLD_128)), data)
}

/// Os dois últimos fluxos, num registrador YMM: absorve 32 bytes se houver, recolhe em um e fecha.
#[target_feature(enable = "pclmulqdq,sse2,sse4.1,ssse3,avx,avx2,vpclmulqdq")]
fn finish256(mut y: __m256i, mut data: &[u8]) -> u32 {
    if data.len() >= 32 {
        y = reduce256(y, get256(&mut data), arch::_mm256_broadcastsi128_si256(keys(FOLD_256)));
    }
    let x = reduce128(arch::_mm256_castsi256_si128(y), arch::_mm256_extracti128_si256(y, 1), keys(FOLD_128));
    reduce_128_to_crc(x, data)
}

/// VPCLMULQDQ de 256 bits: oito fluxos em quatro registradores YMM (128 bytes por volta).
#[target_feature(enable = "pclmulqdq,sse2,sse4.1,ssse3,avx,avx2,vpclmulqdq")]
fn avx2(crc: u32, mut data: &[u8]) -> u32 {
    if data.len() < MIN_AVX2_BYTES {
        return sse(crc, data);
    }
    let init = arch::_mm256_castsi128_si256(arch::_mm_cvtsi32_si128(!crc as i32));
    let mut v = [arch::_mm256_xor_si256(get256(&mut data), init), get256(&mut data), get256(&mut data), get256(&mut data)];
    if data.len() >= 128 {
        let k = arch::_mm256_broadcastsi128_si256(keys(FOLD_1024));
        let mut next = [get256(&mut data), get256(&mut data), get256(&mut data), get256(&mut data)];
        loop {
            for (r, b) in v.iter_mut().zip(next) {
                *r = reduce256(*r, b, k);
            }
            prefetch::<2>(data);
            if data.len() < 128 {
                break;
            }
            next = [get256(&mut data), get256(&mut data), get256(&mut data), get256(&mut data)];
        }
    }
    let k = arch::_mm256_broadcastsi128_si256(keys(FOLD_512));
    let mut half = [reduce256(v[0], v[2], k), reduce256(v[1], v[3], k)];
    if data.len() >= 64 {
        for r in &mut half {
            *r = reduce256(*r, get256(&mut data), k);
        }
    }
    finish256(reduce256(half[0], half[1], arch::_mm256_broadcastsi128_si256(keys(FOLD_256))), data)
}

/// VPCLMULQDQ de 512 bits: dezesseis fluxos em quatro registradores ZMM (256 bytes por volta).
#[target_feature(enable = "pclmulqdq,sse2,sse4.1,ssse3,avx,avx2,vpclmulqdq,avx512f")]
fn avx512(crc: u32, mut data: &[u8]) -> u32 {
    if data.len() < MIN_AVX512_BYTES {
        return avx2(crc, data);
    }
    let init = arch::_mm512_castsi128_si512(arch::_mm_cvtsi32_si128(!crc as i32));
    let mut v = [arch::_mm512_xor_si512(get512(&mut data), init), get512(&mut data), get512(&mut data), get512(&mut data)];
    if data.len() >= 256 {
        let k = arch::_mm512_broadcast_i32x4(keys(FOLD_2048));
        let mut next = [get512(&mut data), get512(&mut data), get512(&mut data), get512(&mut data)];
        loop {
            for (r, b) in v.iter_mut().zip(next) {
                *r = reduce512(*r, b, k);
            }
            prefetch::<4>(data);
            if data.len() < 256 {
                break;
            }
            next = [get512(&mut data), get512(&mut data), get512(&mut data), get512(&mut data)];
        }
    }
    let k = arch::_mm512_broadcast_i32x4(keys(FOLD_1024));
    let mut half = [reduce512(v[0], v[2], k), reduce512(v[1], v[3], k)];
    if data.len() >= 128 {
        for r in &mut half {
            *r = reduce512(*r, get512(&mut data), k);
        }
    }
    let k = arch::_mm512_broadcast_i32x4(keys(FOLD_512));
    let mut u = reduce512(half[0], half[1], k);
    if data.len() >= 64 {
        u = reduce512(u, get512(&mut data), k);
    }
    let y = reduce256(
        arch::_mm512_castsi512_si256(u),
        arch::_mm512_extracti64x4_epi64(u, 1),
        arch::_mm256_broadcastsi128_si256(keys(FOLD_256)),
    );
    finish256(y, data)
}

#[cfg(test)]
mod tests {
    use super::super::crc32_step;
    use super::*;

    /// As constantes da redução final, conferidas num modelo das instruções contra o `zlib.crc32`.
    #[test]
    fn barrett_constants() {
        assert_eq!(POLY_33, 0x1_db71_0641);
        assert_eq!(BARRETT_95, 0xb4e5_b025_f701_1641);
    }

    /// O caminho curto contra o byte a byte, em todo tamanho que ele cobre.
    #[test]
    fn short_matches_bytewise() {
        let data: Vec<u8> = (0..16u8).map(|i| i.wrapping_mul(37) ^ 0xa5).collect();
        for len in 0..=16 {
            for init in [0, 0xdead_beef] {
                let slow = !data[..len].iter().fold(!init, |c, &b| crc32_step(c, b));
                assert_eq!(crc32_short(init, &data[..len]), slow, "len {len}");
            }
        }
    }

    /// Cada kernel que a CPU executa, contra o trançado, nos tamanhos e deslocamentos em que as
    /// estratégias trocam (os do teste do `crc32fast`).
    #[test]
    fn kernels_match_braid() {
        let mut data = vec![0u8; 8200];
        let mut s: u32 = 0x1234_5678;
        for b in data.iter_mut() {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *b = (s >> 24) as u8;
        }
        let lens = [
            0usize, 1, 15, 16, 17, 63, 64, 127, 128, 129, 255, 256, 257, 511, 512, 513, 1023, 1024, 1025, 2047, 2048, 2049, 2175, 2176, 2303,
            2304, 4096, 8192, 8199,
        ];
        // Os tamanhos de troca do `crc32fast` e, em volta das entradas mínimas dos kernels, todo
        // comprimento, para cada resto que a árvore absorve em cada nível.
        let sweep = (0..=700).chain(1900..=2700);
        let lens: Vec<usize> = lens.into_iter().chain(sweep).collect();
        for level in SSE..=cpu_level(&LEVEL, detect) {
            for &len in &lens {
                for offset in [0usize, 1, 3, 7, 8, 15] {
                    let Some(slice) = data.get(offset..offset + len) else { continue };
                    for init in [0, 0xdead_beef] {
                        assert_eq!(run(level, init, slice), crc32_braid(init, slice), "nível {level} len {len} offset {offset}");
                    }
                }
            }
        }
    }
}
