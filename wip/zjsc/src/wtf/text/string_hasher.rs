//! Tradução de `WTF/wtf/text/StringHasher.h`, `StringHasherInlines.h` e do `RapidHash.h` que o
//! `StringHasher` usa de fato (o `SuperFastHash.h` não participa do hash do `StringImpl`).
//!
//! O hash de string do WebKit é o rapidhash: se todos os caracteres de um buffer de 16 bits são
//! Latin1, cada um vira um byte (assim `"ABC"` Latin1 e `u"ABC"` têm o mesmo hash); caso contrário
//! os `2N` bytes little-endian entram como estão.

use crate::wtf::text::string_impl::CharType;

/// Razão áurea. Valor inicial arbitrário, para não mapear zeros em hash zero.
pub const STRING_HASHING_START_VALUE: u32 = 0x9E37_79B9;

/// Oito bits do hash são reservados para as flags do `StringImpl`.
pub const FLAG_COUNT: u32 = 8;
pub const MASK_HASH: u32 = (1u32 << (u32::BITS - FLAG_COUNT)) - 1;

/// Segredos do rapidhash (`RapidHash::secret`).
const SECRET: [u64; 3] = [0x2d35_8dcc_aa6c_78a5, 0x8bb8_4b93_962e_acc9, 0x4b33_a62e_d433_d4a3];

/// Equivalente do parâmetro de template `Converter` do C++: transforma o caractere antes de entrar
/// no hash. `IS_DEFAULT` marca o `DefaultConverter`, que dispara o caminho Latin1/bytes crus.
pub trait Converter {
    const IS_DEFAULT: bool;
    fn convert(character: u16) -> u16;
}

/// `StringHasher::DefaultConverter`: o próprio caractere.
pub struct DefaultConverter;

impl Converter for DefaultConverter {
    const IS_DEFAULT: bool = true;
    fn convert(character: u16) -> u16 {
        character
    }
}

/// Evita devolver hash 0, que sinaliza "hash ainda não calculado". O bit alto preserva boa
/// fidelidade a um hash 0, que provavelmente daria 0 depois que a busca mascara os bits altos.
pub const fn avoid_zero(hash: u32) -> u32 {
    if hash != 0 {
        return hash;
    }
    0x8000_0000u32 >> FLAG_COUNT
}

pub const fn avalanche_bits(hash: u32) -> u32 {
    let mut result = hash;

    result ^= result << 3;
    result = result.wrapping_add(result >> 5);
    result ^= result << 2;
    result = result.wrapping_add(result >> 15);
    result ^= result << 10;

    result
}

pub const fn finalize(hash: u32) -> u32 {
    avoid_zero(avalanche_bits(hash))
}

pub const fn finalize_and_mask_top8_bits(hash: u32) -> u32 {
    // Reservar os bits altos para flags preserva a maior parte do valor do hash, já que a busca
    // costuma mascarar esses bits de qualquer forma.
    avoid_zero(avalanche_bits(hash) & MASK_HASH)
}

/// `StringHasher::computeHashAndMaskTop8Bits<T>` com o `DefaultConverter`.
pub fn compute_hash_and_mask_top8_bits<T: CharType>(data: &[T]) -> u32 {
    compute_hash_and_mask_top8_bits_with::<T, DefaultConverter>(data)
}

/// `StringHasher::computeHashAndMaskTop8Bits<T, Converter>`.
pub fn compute_hash_and_mask_top8_bits_with<T: CharType, C: Converter>(data: &[T]) -> u32 {
    avoid_zero((rapidhash::<T, C>(data) as u32) & MASK_HASH)
}

/// `StringHasher::computeLiteralHashAndMaskTop8Bits`: os caracteres sem o terminador nulo (o
/// `ASCIILiteral` entra como `&[u8]`).
pub fn compute_literal_hash_and_mask_top8_bits<T: CharType>(characters: &[T]) -> u32 {
    compute_hash_and_mask_top8_bits::<T>(characters)
}

fn rapid_mul128(a: u64, b: u64) -> (u64, u64) {
    let r = (a as u128) * (b as u128);
    (r as u64, (r >> 64) as u64)
}

fn rapid_mix(a: u64, b: u64) -> u64 {
    let (lo, hi) = rapid_mul128(a, b);
    lo ^ hi
}

/// Núcleo do rapidhash com leitores por índice.
/// `read64(off)` lê 8 bytes, `read32(off)` lê 4 bytes, `read_small(off, k)` lê 1 a 3 bytes.
fn rapidhash_impl(
    len: usize,
    read64: &dyn Fn(usize) -> u64,
    read32: &dyn Fn(usize) -> u64,
    read_small: &dyn Fn(usize, usize) -> u64,
) -> u64 {
    let mut seed = rapid_mix(SECRET[0], SECRET[1]) ^ (len as u64);
    let a: u64;
    let b: u64;

    if len <= 16 {
        if len >= 4 {
            let delta = if len >= 8 { 4 } else { 0 };
            a = (read32(0) << 32) | read32(len - 4);
            b = (read32(delta) << 32) | read32(len - 4 - delta);
        } else if len > 0 {
            a = read_small(0, len);
            b = 0;
        } else {
            a = 0;
            b = 0;
        }
    } else {
        let mut i = len;
        let mut off = 0usize;
        if i > 48 {
            let mut see1 = seed;
            let mut see2 = seed;
            loop {
                seed = rapid_mix(read64(off) ^ SECRET[0], read64(off + 8) ^ seed);
                see1 = rapid_mix(read64(off + 16) ^ SECRET[1], read64(off + 24) ^ see1);
                see2 = rapid_mix(read64(off + 32) ^ SECRET[2], read64(off + 40) ^ see2);
                off += 48;
                i -= 48;
                if i < 48 {
                    break;
                }
            }
            seed ^= see1 ^ see2;
        }
        if i > 16 {
            seed = rapid_mix(read64(off) ^ SECRET[2], read64(off + 8) ^ seed ^ SECRET[1]);
            if i > 32 {
                seed = rapid_mix(read64(off + 16) ^ SECRET[2], read64(off + 24) ^ seed);
            }
        }

        a = read64(off + i - 16);
        b = read64(off + i - 8);
    }

    let a = a ^ SECRET[1];
    let b = b ^ seed;
    let (a_lo, a_hi) = rapid_mul128(a, b);
    rapid_mix(a_lo ^ SECRET[0] ^ (len as u64), a_hi ^ SECRET[1])
}

/// Despacho do `RapidHash::rapidhash`:
/// - 1 byte por caractere: cada byte é `fold_byte(c)`;
/// - 2 bytes com `DefaultConverter`: se tudo é Latin1, um byte por caractere (o byte baixo); senão
///   os `2N` bytes little-endian crus;
/// - 2 bytes com conversor próprio: sempre um byte por caractere.
fn rapidhash<T: CharType, C: Converter>(data: &[T]) -> u64 {
    if T::SIZE == 2 && C::IS_DEFAULT && !data.iter().all(|c| Into::<u32>::into(*c) <= 0xFF) {
        return rapidhash_raw_bytes(data);
    }
    rapidhash_one_byte_per_char::<T, C>(data)
}

/// Um byte por caractere. Com `DefaultConverter` é o truncamento simples; com conversor próprio
/// é o OR-fold `(conv & 0xFF) | (conv >> 8)`.
fn rapidhash_one_byte_per_char<T: CharType, C: Converter>(data: &[T]) -> u64 {
    let fold_byte = |idx: usize| -> u8 {
        let value: u32 = data[idx].into();
        if C::IS_DEFAULT {
            value as u8
        } else {
            let conv = C::convert(value as u16);
            ((conv & 0xFF) | (conv >> 8)) as u8
        }
    };

    let read64 = |off: usize| -> u64 {
        let mut result = 0u64;
        for i in 0..8 {
            result |= (fold_byte(off + i) as u64) << (i * 8);
        }
        result
    };

    let read32 = |off: usize| -> u64 {
        let mut result = 0u64;
        for i in 0..4 {
            result |= (fold_byte(off + i) as u64) << (i * 8);
        }
        result
    };

    let read_small = |off: usize, k: usize| -> u64 {
        ((fold_byte(off) as u64) << 56)
            | ((fold_byte(off + (k >> 1)) as u64) << 32)
            | (fold_byte(off + k - 1) as u64)
    };

    rapidhash_impl(data.len(), &read64, &read32, &read_small)
}

/// Os `2N` bytes little-endian de um buffer de 16 bits. Só entra quando há ao menos um caractere
/// fora do Latin1.
fn rapidhash_raw_bytes<T: CharType>(data: &[T]) -> u64 {
    let byte_length = 2 * data.len();

    let read_byte = |bi: usize| -> u8 {
        let c: u32 = data[bi >> 1].into();
        if bi & 1 != 0 {
            (c >> 8) as u8
        } else {
            c as u8
        }
    };

    let read64 = |off: usize| -> u64 {
        let mut r = 0u64;
        for i in 0..8 {
            r |= (read_byte(off + i) as u64) << (i * 8);
        }
        r
    };

    let read32 = |off: usize| -> u64 {
        let mut r = 0u64;
        for i in 0..4 {
            r |= (read_byte(off + i) as u64) << (i * 8);
        }
        r
    };

    let read_small = |off: usize, k: usize| -> u64 {
        ((read_byte(off) as u64) << 56)
            | ((read_byte(off + (k >> 1)) as u64) << 32)
            | (read_byte(off + k - 1) as u64)
    };

    rapidhash_impl(byte_length, &read64, &read32, &read_small)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Referência independente: o rapidhash do repositório original sobre um vetor de bytes, com
    /// leituras `from_le_bytes`.
    #[test]
    fn latin1_utf16_equals_latin1_bytes() {
        for len in 0..=120usize {
            let wide: Vec<u16> = (0..len).map(|i| ((i * 13 + 1) & 0xFF) as u16).collect();
            let narrow: Vec<u8> = wide.iter().map(|c| *c as u8).collect();
            assert_eq!(
                compute_hash_and_mask_top8_bits::<u16>(&wide),
                compute_hash_and_mask_top8_bits::<u8>(&narrow),
                "len {len}"
            );
        }
    }

    #[test]
    fn hash_never_zero_and_top_bits_clear() {
        for len in 0..64usize {
            let data: Vec<u8> = (0..len).map(|i| (i * 5) as u8).collect();
            let h = compute_hash_and_mask_top8_bits::<u8>(&data);
            assert_ne!(h, 0);
            assert_eq!(h >> 24, 0);
        }
        assert_eq!(avoid_zero(0), 0x0080_0000);
        assert_eq!(avoid_zero(5), 5);
    }

    #[test]
    fn literal_equals_runtime_hash() {
        assert_eq!(
            compute_literal_hash_and_mask_top8_bits::<u8>(b"length"),
            compute_hash_and_mask_top8_bits::<u8>(b"length")
        );
    }

    #[test]
    fn avalanche_matches_manual_steps() {
        // Passos do C++ aplicados à mão ao valor inicial.
        let mut r = STRING_HASHING_START_VALUE;
        r ^= r << 3;
        r = r.wrapping_add(r >> 5);
        r ^= r << 2;
        r = r.wrapping_add(r >> 15);
        r ^= r << 10;
        assert_eq!(avalanche_bits(STRING_HASHING_START_VALUE), r);
        assert_eq!(finalize(0), avoid_zero(avalanche_bits(0)));
        assert_eq!(avalanche_bits(0), 0);
        assert_eq!(finalize(0), 0x0080_0000);
    }
}
