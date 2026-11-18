//! Codificações que os programas repetiam: base64, CRC-32 e hexadecimal minúsculo.
//!
//! O base64 aqui é o do `binascii.a2b_base64` do CPython (a decodificação tolera lixo e quebra de
//! linha fora do modo estrito); os chamadores que só codificam não enxergam a diferença.

/// Alfabeto base64 padrão (RFC 4648, seção 4).
pub const BASE64_STANDARD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Alfabeto base64 seguro para URL (RFC 4648, seção 5): `-` e `_` no lugar de `+` e `/`.
pub const BASE64_URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Codifica em base64 com o alfabeto dado, com preenchimento `=` se `pad`.
pub fn base64_encode(data: &[u8], alphabet: &[u8; 64], pad: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = u32::from(*chunk.get(1).unwrap_or(&0));
        let b2 = u32::from(*chunk.get(2).unwrap_or(&0));
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(alphabet[((n >> 18) & 63) as usize]);
        out.push(alphabet[((n >> 12) & 63) as usize]);
        if chunk.len() > 1 {
            out.push(alphabet[((n >> 6) & 63) as usize]);
        } else if pad {
            out.push(b'=');
        }
        if chunk.len() > 2 {
            out.push(alphabet[(n & 63) as usize]);
        } else if pad {
            out.push(b'=');
        }
    }
    out
}

/// Base64 padrão com preenchimento, como texto (o `Authorization: Basic`, as chaves do yaml).
pub fn base64_string(data: &[u8]) -> String {
    base64_encode(data, BASE64_STANDARD, true).into_iter().map(char::from).collect()
}

fn base64_value(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Decodifica base64 (alfabeto padrão) como o `binascii.a2b_base64`. Fora do modo `strict` os
/// caracteres fora do alfabeto são descartados; no modo estrito viram erro. O erro é a mensagem do
/// `binascii.Error` do CPython 3.13.
pub fn base64_decode(data: &[u8], strict: bool) -> Result<Vec<u8>, String> {
    let mut out: Vec<u8> = Vec::new();
    let mut quad: usize = 0;
    let mut left: u8 = 0;
    let mut pads: usize = 0;
    let mut padding_started = false;
    for (idx, &c) in data.iter().enumerate() {
        if c == b'=' {
            padding_started = true;
            if quad >= 2 {
                pads += 1;
                if quad + pads >= 4 {
                    if strict && idx + 1 < data.len() {
                        return Err("Excess data after padding".to_string());
                    }
                    quad = 0;
                    break;
                }
            } else if strict && quad == 0 {
                return Err("Leading padding not allowed".to_string());
            }
            continue;
        }
        let Some(v) = base64_value(c) else {
            if strict {
                return Err("Only base64 data is allowed".to_string());
            }
            continue;
        };
        if strict && padding_started {
            return Err("Excess data after padding".to_string());
        }
        pads = 0;
        match quad {
            0 => {
                quad = 1;
                left = v;
            }
            1 => {
                quad = 2;
                out.push((left << 2) | (v >> 4));
                left = v & 0xf;
            }
            2 => {
                quad = 3;
                out.push((left << 4) | (v >> 2));
                left = v & 0x3;
            }
            _ => {
                quad = 0;
                out.push((left << 6) | v);
                left = 0;
            }
        }
    }
    if quad != 0 {
        if quad == 1 {
            return Err(format!(
                "Invalid base64-encoded string: number of data characters ({}) cannot be 1 more than a multiple of 4",
                (out.len() / 3) * 4 + 1
            ));
        }
        return Err("Incorrect padding".to_string());
    }
    Ok(out)
}

/// Hexadecimal em minúsculas, dois dígitos por byte.
pub fn hex_lower(data: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(data.len() * 2);
    for b in data {
        s.push(DIGITS[(b >> 4) as usize] as char);
        s.push(DIGITS[(b & 15) as usize] as char);
    }
    s
}

/// A tabela de um CRC refletido de 32 bits com o polinômio `poly` (0xEDB88320 no CRC-32 de zlib, 0x82F63B78 no
/// CRC-32C de Castagnoli).
const fn build_crc_table(poly: u32) -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut n = 0;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { poly ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        table[n] = c;
        n += 1;
    }
    table
}

/// A tabela do CRC-32C (polinômio refletido 0x82F63B78), o que o leveldb, o iSCSI e o ext4 usam.
static CRC32C_TABLE: [u32; 256] = build_crc_table(0x82F6_3B78);

/// Continua um CRC-32C a partir de `value` (o CRC já finalizado dos bytes anteriores, 0 no começo).
pub fn crc32c_update(value: u32, data: &[u8]) -> u32 {
    let mut crc = !value;
    for &byte in data {
        crc = CRC32C_TABLE[((crc ^ u32::from(byte)) & 0xff) as usize] ^ (crc >> 8);
    }
    !crc
}

/// O CRC-32C de `data`.
pub fn crc32c(data: &[u8]) -> u32 {
    crc32c_update(0, data)
}

/// O CRC "mascarado" do leveldb: guardar o CRC de dados que contêm CRCs deixa de dar o valor trivial. Gira 15 bits
/// e soma uma constante (`crc32c::Mask` do leveldb).
pub fn crc32c_mask(crc: u32) -> u32 {
    ((crc >> 15) | (crc << 17)).wrapping_add(0xa282_ead8)
}

/// Desfaz [`crc32c_mask`].
pub fn crc32c_unmask(masked: u32) -> u32 {
    let rotated = masked.wrapping_sub(0xa282_ead8);
    (rotated >> 17) | (rotated << 15)
}

/// O polinômio refletido do CRC-32 (`POLY` do `crc32.c` do zlib): o de zlib, gzip, zip, xz, lzip e PNG.
const CRC32_POLY: u32 = 0xEDB8_8320;

/// A tabela do CRC-32, a `crc_table` do zlib.
pub static CRC32_TABLE: [u32; 256] = build_crc_table(CRC32_POLY);

/// Um passo do CRC-32 sobre o registrador cru (sem a inversão inicial e final): é o `CRC32(c, b)` do
/// `crypt.c` do zip, que a cifra tradicional do PKZIP usa nas chaves.
pub fn crc32_step(crc: u32, byte: u8) -> u32 {
    CRC32_TABLE[((crc ^ u32::from(byte)) & 0xff) as usize] ^ (crc >> 8)
}

// O CRC-32 trançado ("braided") do `crc32.c` do zlib 1.3.1, com os parâmetros que ele escolhe para
// palavras de 64 bits: N = 5 CRCs independentes, cada um sobre uma palavra de W = 8 bytes por volta.
// As cinco cadeias não dependem uma da outra, então o processador sobrepõe as consultas às tabelas;
// no fim, as cinco se combinam de volta num CRC só, palavra a palavra.

/// Quantas tranças correm lado a lado (`N`).
const BRAID_N: usize = 5;
/// Bytes por palavra de cada trança (`W`).
const BRAID_W: usize = 8;

/// `multmodp`: o produto de `a` e `b` módulo o polinômio, com os bits refletidos (o bit 31 é x^0).
const fn multmodp(a: u32, mut b: u32) -> u32 {
    let mut m = 1u32 << 31;
    let mut p = 0;
    while m != 0 {
        if a & m != 0 {
            p ^= b;
        }
        m >>= 1;
        b = if b & 1 != 0 { (b >> 1) ^ CRC32_POLY } else { b >> 1 };
    }
    p
}

/// `x2n_table`: `[k]` é x^(2^k) módulo o polinômio.
const X2N_TABLE: [u32; 32] = {
    let mut table = [0u32; 32];
    let mut p = 1u32 << 30;
    table[0] = p;
    let mut n = 1;
    while n < 32 {
        p = multmodp(p, p);
        table[n] = p;
        n += 1;
    }
    table
};

/// `x2nmodp`: x^(n * 2^k) módulo o polinômio.
const fn x2nmodp(mut n: u64, mut k: usize) -> u32 {
    let mut p = 1u32 << 31;
    while n != 0 {
        if n & 1 != 0 {
            p = multmodp(X2N_TABLE[k & 31], p);
        }
        n >>= 1;
        k += 1;
    }
    p
}

/// `crc_braid_table` (o `braid` do zlib): `[k][i]` leva o byte `i`, na posição `k` da palavra, até a
/// mesma posição da palavra seguinte da sua trança, `N * W` bytes adiante.
static CRC32_BRAID: [[u32; 256]; BRAID_W] = {
    let mut table = [[0u32; 256]; BRAID_W];
    let mut k = 0;
    while k < BRAID_W {
        let p = x2nmodp(((BRAID_N * BRAID_W + 3 - k) << 3) as u64, 0);
        let mut i = 1;
        while i < 256 {
            table[k][i] = multmodp((i as u32) << 24, p);
            i += 1;
        }
        k += 1;
    }
    table
};

/// `crc_word`: passa os `W` bytes de uma palavra pelo CRC, um de cada vez.
fn crc32_word(mut word: u64) -> u32 {
    for _ in 0..BRAID_W {
        word = (word >> 8) ^ u64::from(CRC32_TABLE[(word & 0xff) as usize]);
    }
    word as u32
}

/// As `N` palavras de um bloco, cada uma já misturada ao CRC da sua trança.
fn braid_words(block: &[u8; BRAID_N * BRAID_W], crcs: &[u32; BRAID_N]) -> [u64; BRAID_N] {
    let (words, _) = block.as_chunks::<BRAID_W>();
    std::array::from_fn(|j| u64::from(crcs[j]) ^ u64::from_le_bytes(words[j]))
}

/// O CRC-32 sem instrução especial, para a CPU que não tem nenhuma das que os kernels de
/// [`crc32_update`] usam (e para o resto que não enche um registrador): o `crc32_z` do zlib. Continua
/// a partir de `value`, o CRC já finalizado dos bytes anteriores (0 no começo).
pub fn crc32_braid(value: u32, data: &[u8]) -> u32 {
    let mut crc = !value;
    let (blocks, rest) = data.as_chunks::<{ BRAID_N * BRAID_W }>();
    if let Some((last, body)) = blocks.split_last() {
        let mut crcs = [crc, 0, 0, 0, 0];
        for block in body {
            let words = braid_words(block, &crcs);
            for (c, word) in crcs.iter_mut().zip(words) {
                *c = CRC32_BRAID
                    .iter()
                    .enumerate()
                    .fold(0, |acc, (k, t)| acc ^ t[((word >> (k * 8)) & 0xff) as usize]);
            }
        }
        // O último bloco não vai adiante pela trança: cada palavra entra no CRC combinado.
        crc = braid_words(last, &crcs).iter().fold(0, |comb, &word| crc32_word(word ^ u64::from(comb)));
    }
    for &b in rest {
        crc = crc32_step(crc, b);
    }
    !crc
}

/// As constantes que dobram um acumulador de 128 bits `bits` bits adiante com duas multiplicações sem
/// vai-um, a parte baixa primeiro: `x^(bits + 32)` e `x^(bits - 32)` módulo o polinômio, refletidas,
/// em 33 bits (o `fold_keys` do `crc32fast`).
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
const fn fold_keys(bits: u64) -> [u64; 2] {
    [(x2nmodp((bits + 32) / 8, 3) as u64) << 1, (x2nmodp((bits - 32) / 8, 3) as u64) << 1]
}

// `crc32_update` (continua um CRC-32 a partir de `value`, o CRC já finalizado dos bytes anteriores, 0
// no começo) é o kernel mais rápido que a CPU executa: a dobra por multiplicação sem vai-um
// (PCLMULQDQ e VPCLMULQDQ) no x86_64, a instrução `crc32` no aarch64 e, sem nenhuma delas, o
// trançado do zlib.
#[cfg(target_arch = "aarch64")]
mod armcrc;
#[cfg(target_arch = "x86_64")]
mod clmul;
#[cfg(target_arch = "aarch64")]
pub use armcrc::crc32_update;
#[cfg(target_arch = "x86_64")]
pub use clmul::crc32_update;
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
pub use crc32_braid as crc32_update;

/// O kernel mais rápido que a CPU executa, numerado de 1 (o trançado) para cima: `detect` roda na
/// primeira consulta e o resultado fica em `cache` (0 é "ainda não detectado").
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[inline]
fn cpu_level(cache: &std::sync::atomic::AtomicU8, detect: fn() -> u8) -> u8 {
    match cache.load(std::sync::atomic::Ordering::Relaxed) {
        0 => detect_level(cache, detect),
        level => level,
    }
}

/// A primeira consulta de [`cpu_level`], fora da linha: assim quem a chama não paga, em toda chamada,
/// os registradores que a detecção usa.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[cold]
#[inline(never)]
fn detect_level(cache: &std::sync::atomic::AtomicU8, detect: fn() -> u8) -> u8 {
    let level = detect();
    cache.store(level, std::sync::atomic::Ordering::Relaxed);
    level
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_encode_vectors() {
        assert_eq!(base64_encode(b"", BASE64_STANDARD, true), b"");
        assert_eq!(base64_encode(b"f", BASE64_STANDARD, true), b"Zg==");
        assert_eq!(base64_encode(b"fo", BASE64_STANDARD, true), b"Zm8=");
        assert_eq!(base64_encode(b"foo", BASE64_STANDARD, true), b"Zm9v");
        assert_eq!(base64_encode(b"foobar", BASE64_STANDARD, true), b"Zm9vYmFy");
        assert_eq!(base64_encode(b"f", BASE64_STANDARD, false), b"Zg");
        assert_eq!(base64_encode(b"\xfb\xff", BASE64_STANDARD, true), b"+/8=");
        assert_eq!(base64_encode(b"\xfb\xff", BASE64_URL, true), b"-_8=");
        assert_eq!(base64_string(b"user:pass"), "dXNlcjpwYXNz");
    }

    #[test]
    fn base64_decode_modes() {
        assert_eq!(base64_decode(b"", false).unwrap(), Vec::<u8>::new());
        assert_eq!(base64_decode(b"Zm9v", true).unwrap(), b"foo".to_vec());
        assert_eq!(base64_decode(b"aGVsbG8=\n", false).unwrap(), b"hello".to_vec());
        // Sem o modo estrito, o que está fora do alfabeto é descartado.
        assert_eq!(base64_decode(b"aGVs\nbG8=", false).unwrap(), b"hello".to_vec());
        assert_eq!(base64_decode(b"Zm9v!", true).unwrap_err(), "Only base64 data is allowed");
        assert_eq!(base64_decode(b"aGVsbG8", false).unwrap_err(), "Incorrect padding");
        assert_eq!(base64_decode(b"=Zm9v", true).unwrap_err(), "Leading padding not allowed");
        assert_eq!(base64_decode(b"Zg==Zg==", true).unwrap_err(), "Excess data after padding");
        assert!(base64_decode(b"Z", false).unwrap_err().starts_with("Invalid base64-encoded string"));
    }

    #[test]
    fn base64_roundtrip_all_lengths() {
        let data: Vec<u8> = (0..=255u8).collect();
        for n in 0..data.len() {
            let enc = base64_encode(&data[..n], BASE64_STANDARD, true);
            assert_eq!(base64_decode(&enc, true).unwrap(), data[..n].to_vec());
        }
    }

    #[test]
    fn hex_lower_digits() {
        assert_eq!(hex_lower(b"\x00\xffAb"), "00ff4162");
        assert_eq!(hex_lower(b""), "");
    }

    #[test]
    fn crc32_known_values() {
        assert_eq!(crc32_update(0, b"123456789"), 0xcbf4_3926);
        assert_eq!(crc32_update(0, b""), 0);
        assert_eq!(crc32_update(0, b"a"), 0xe8b7_be43);
        let (a, b) = b"123456789".split_at(4);
        assert_eq!(crc32_update(crc32_update(0, a), b), 0xcbf4_3926);
    }

    #[test]
    fn crc32_braid_matches_bytewise() {
        let data: Vec<u8> = (0..1000u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8).collect();
        // Em volta de um, dois e muitos blocos de N * W = 40 bytes, e o resto byte a byte.
        for len in [0, 1, 7, 8, 9, 39, 40, 41, 79, 80, 81, 119, 120, 999, 1000] {
            let slow = !data[..len].iter().fold(!0u32, |crc, &b| crc32_step(crc, b));
            assert_eq!(crc32_braid(0, &data[..len]), slow, "len {len}");
            // Retomar do meio dá o mesmo valor, com o corte caindo dentro ou fora de um bloco.
            for cut in [0, len.min(1), len / 3, len / 2, len] {
                let (a, b) = data[..len].split_at(cut);
                assert_eq!(crc32_braid(crc32_braid(0, a), b), slow, "len {len} cut {cut}");
            }
        }
    }

    /// A `crc_braid_table` do `crc32.h` do zlib 1.3.1 (little-endian, N = 5, W = 8): as pontas da
    /// primeira e da última linha, e o CRC-32 da tabela inteira em little-endian, tirado do `crc32.h`
    /// com o `zlib.crc32` do Python.
    #[test]
    fn crc32_braid_table_matches_zlib() {
        assert_eq!(CRC32_BRAID[0][..4], [0x0000_0000, 0xaf44_9247, 0x85f8_22cf, 0x2abc_b088]);
        assert_eq!(CRC32_BRAID[7][..4], [0x0000_0000, 0x36f2_90f3, 0x6de5_21e6, 0x5b17_b115]);
        assert_eq!(CRC32_BRAID[7][255], 0xf437_7108);
        let bytes: Vec<u8> = CRC32_BRAID.iter().flatten().flat_map(|v| v.to_le_bytes()).collect();
        assert_eq!(crc32_braid(0, &bytes), 0xf3b6_93bc);
    }

    /// As do artigo da Intel, que o `crc32fast` traz literais no kernel do x86.
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    #[test]
    fn fold_keys_match_the_paper() {
        assert_eq!(fold_keys(128), [0x1_7519_97d0, 0x0_ccaa_009e]);
        assert_eq!(fold_keys(512), [0x1_5444_2bd4, 0x1_c6e4_1596]);
        assert_eq!(fold_keys(1024), [0x1_e88e_f372, 0x1_4a7f_e880]);
        assert_eq!(fold_keys(2048), [0x1_1542_778a, 0x1_322d_1430]);
    }

    #[test]
    fn crc32_table_and_step() {
        assert_eq!(CRC32_TABLE[0], 0);
        assert_eq!(CRC32_TABLE[1], 0x7707_3096);
        assert_eq!(CRC32_TABLE[255], 0x2d02_ef8d);
        // O passo cru, encadeado com as inversões, é o crc32_update.
        assert_eq!(!crc32_step(crc32_step(!0, b'a'), b'b'), crc32_update(0, b"ab"));
    }

    /// Os vetores do RFC 3720 (apêndice B.4) e do `crc32c_test.cc` do leveldb.
    #[test]
    fn crc32c_known_values() {
        assert_eq!(crc32c(b"123456789"), 0xe306_9283);
        assert_eq!(crc32c(&[0u8; 32]), 0x8a91_36aa);
        assert_eq!(crc32c(&[0xffu8; 32]), 0x62a8_ab43);
        let ascending: Vec<u8> = (0..32).collect();
        assert_eq!(crc32c(&ascending), 0x46dd_794e);
        let (a, b) = b"123456789".split_at(4);
        assert_eq!(crc32c_update(crc32c(a), b), 0xe306_9283);
    }

    #[test]
    fn crc32c_mask_round_trips() {
        for crc in [0u32, 1, 0xe306_9283, 0xffff_ffff] {
            assert_eq!(crc32c_unmask(crc32c_mask(crc)), crc);
            assert_ne!(crc32c_mask(crc), crc);
        }
    }
}
