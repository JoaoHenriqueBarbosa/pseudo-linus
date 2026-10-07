//! Módulo `hashlib` do CPython 3.13, com os algoritmos escritos à mão em Rust puro (sem dependências).
//!
//! Cobre `md5`, `sha1`, `sha224`, `sha256`, `sha384`, `sha512`, `blake2b`, `new(name)` e
//! `pbkdf2_hmac`. O objeto de hash guarda os bytes recebidos e calcula o resumo sob demanda, então
//! `update`, `copy`, `digest` e `hexdigest` são baratos de escrever e sempre coerentes. Ficam de
//! fora: `sha3_*`, `shake_*`, `blake2s`, `scrypt`, `file_digest`, `algorithms_available`, e as
//! opções `key`/`salt`/`person` do `blake2b`.
//!
//! As constantes do SHA-2 (e o IV do BLAKE2b) são derivadas na primeira chamada das raízes
//! quadradas e cúbicas dos primeiros primos, com aritmética inteira exata, em vez de digitadas.

use std::cell::RefCell;
use std::cmp::Ordering;
use std::rc::Rc;
use std::sync::OnceLock;

use crate::modules::ModuleBuilder;
use crate::native_util::{bind, want_int, want_str};
use crate::object::{ExtObject, Kw, ModuleObj, Value};
use crate::vm::{exc, type_error, PyResult, Vm};

// ---------------------------------------------------------------------------
// Constantes derivadas (parte fracionária de raízes de primos)
// ---------------------------------------------------------------------------

fn primes(count: usize) -> Vec<u64> {
    let mut out: Vec<u64> = Vec::new();
    let mut n = 2u64;
    while out.len() < count {
        if out.iter().all(|p| n % p != 0) {
            out.push(n);
        }
        n += 1;
    }
    out
}

fn big_mul(a: &[u32], b: &[u32]) -> Vec<u32> {
    let mut out = vec![0u32; a.len() + b.len()];
    for (i, &x) in a.iter().enumerate() {
        let mut carry = 0u64;
        for (j, &y) in b.iter().enumerate() {
            let cur = u64::from(out[i + j]) + u64::from(x) * u64::from(y) + carry;
            out[i + j] = cur as u32;
            carry = cur >> 32;
        }
        out[i + b.len()] = carry as u32;
    }
    out
}

fn big_cmp(a: &[u32], b: &[u32]) -> Ordering {
    let trim = |x: &[u32]| {
        let mut n = x.len();
        while n > 0 && x[n - 1] == 0 {
            n -= 1;
        }
        n
    };
    let (la, lb) = (trim(a), trim(b));
    if la != lb {
        return la.cmp(&lb);
    }
    for i in (0..la).rev() {
        if a[i] != b[i] {
            return a[i].cmp(&b[i]);
        }
    }
    Ordering::Equal
}

fn limbs(x: u128) -> Vec<u32> {
    vec![x as u32, (x >> 32) as u32, (x >> 64) as u32, (x >> 96) as u32]
}

/// Maior `x` com `x^n <= p * 2^(64 n)`: a raiz `n`-ésima de `p` com 64 bits fracionários.
fn iroot(p: u64, n: u32) -> u128 {
    let mut target = vec![0u32; (2 * n) as usize];
    target.push(p as u32);
    let power = |x: u128| -> Vec<u32> {
        let l = limbs(x);
        let mut acc = l.clone();
        for _ in 1..n {
            acc = big_mul(&acc, &l);
        }
        acc
    };
    let guess = ((p as f64).powf(1.0 / f64::from(n)) * 18_446_744_073_709_551_616.0) as u128;
    let margin: u128 = 1 << 24;
    let mut lo = guess.saturating_sub(margin);
    let mut hi = guess + margin;
    // Invariante: lo^n <= alvo < hi^n.
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if big_cmp(&power(mid), &target) != Ordering::Greater {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}

fn frac_root(p: u64, n: u32) -> u64 {
    (iroot(p, n) & u128::from(u64::MAX)) as u64
}

/// Parte fracionária das raízes quadradas dos 16 primeiros primos (64 bits).
fn sqrt_consts() -> &'static [u64; 16] {
    static CELL: OnceLock<[u64; 16]> = OnceLock::new();
    CELL.get_or_init(|| {
        let ps = primes(16);
        let mut out = [0u64; 16];
        for (i, p) in ps.iter().enumerate() {
            out[i] = frac_root(*p, 2);
        }
        out
    })
}

/// Parte fracionária das raízes cúbicas dos 80 primeiros primos (64 bits).
fn cbrt_consts() -> &'static [u64; 80] {
    static CELL: OnceLock<[u64; 80]> = OnceLock::new();
    CELL.get_or_init(|| {
        let ps = primes(80);
        let mut out = [0u64; 80];
        for (i, p) in ps.iter().enumerate() {
            out[i] = frac_root(*p, 3);
        }
        out
    })
}

// ---------------------------------------------------------------------------
// Algoritmos
// ---------------------------------------------------------------------------

fn pad_message(data: &[u8], block: usize, len_bytes: usize, big_endian: bool) -> Vec<u8> {
    let mut m = data.to_vec();
    m.push(0x80);
    while m.len() % block != block - len_bytes {
        m.push(0);
    }
    let bits = (data.len() as u128) * 8;
    if big_endian {
        let b = bits.to_be_bytes();
        m.extend_from_slice(&b[16 - len_bytes..]);
    } else {
        let b = bits.to_le_bytes();
        m.extend_from_slice(&b[..len_bytes]);
    }
    m
}

pub fn md5(data: &[u8]) -> Vec<u8> {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14,
        20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6,
        10, 15, 21,
    ];
    let k: Vec<u32> = (0..64).map(|i| (((i as f64) + 1.0).sin().abs() * 4_294_967_296.0) as u32).collect();
    let (mut a0, mut b0, mut c0, mut d0) = (0x6745_2301u32, 0xefcd_ab89u32, 0x98ba_dcfeu32, 0x1032_5476u32);
    let msg = pad_message(data, 64, 8, false);
    for chunk in msg.chunks(64) {
        let mut m = [0u32; 16];
        for (i, w) in m.iter_mut().enumerate() {
            *w = u32::from_le_bytes([chunk[4 * i], chunk[4 * i + 1], chunk[4 * i + 2], chunk[4 * i + 3]]);
        }
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64usize {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let f = f.wrapping_add(a).wrapping_add(k[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(S[i]));
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }
    let mut out = Vec::with_capacity(16);
    for w in [a0, b0, c0, d0] {
        out.extend_from_slice(&w.to_le_bytes());
    }
    out
}

pub fn sha1(data: &[u8]) -> Vec<u8> {
    let mut h: [u32; 5] = [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476, 0xC3D2_E1F0];
    let msg = pad_message(data, 64, 8, true);
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([chunk[4 * i], chunk[4 * i + 1], chunk[4 * i + 2], chunk[4 * i + 3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i / 20 {
                0 => ((b & c) | (!b & d), 0x5A82_7999u32),
                1 => (b ^ c ^ d, 0x6ED9_EBA1u32),
                2 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDCu32),
                _ => (b ^ c ^ d, 0xCA62_C1D6u32),
            };
            let temp = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }
    h.iter().flat_map(|w| w.to_be_bytes()).collect()
}

fn sha256_core(data: &[u8], init: [u32; 8], out_len: usize) -> Vec<u8> {
    let kc = cbrt_consts();
    let mut h = init;
    let msg = pad_message(data, 64, 8, true);
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([chunk[4 * i], chunk[4 * i + 1], chunk[4 * i + 2], chunk[4 * i + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let k = (kc[i] >> 32) as u32;
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(k).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v[7] = v[6];
            v[6] = v[5];
            v[5] = v[4];
            v[4] = v[3].wrapping_add(t1);
            v[3] = v[2];
            v[2] = v[1];
            v[1] = v[0];
            v[0] = t1.wrapping_add(t2);
        }
        for i in 0..8 {
            h[i] = h[i].wrapping_add(v[i]);
        }
    }
    let mut out: Vec<u8> = h.iter().flat_map(|w| w.to_be_bytes()).collect();
    out.truncate(out_len);
    out
}

pub fn sha256(data: &[u8]) -> Vec<u8> {
    let s = sqrt_consts();
    let mut init = [0u32; 8];
    for i in 0..8 {
        init[i] = (s[i] >> 32) as u32;
    }
    sha256_core(data, init, 32)
}

pub fn sha224(data: &[u8]) -> Vec<u8> {
    let s = sqrt_consts();
    let mut init = [0u32; 8];
    for i in 0..8 {
        init[i] = (s[8 + i] & 0xffff_ffff) as u32;
    }
    sha256_core(data, init, 28)
}

fn sha512_core(data: &[u8], init: [u64; 8], out_len: usize) -> Vec<u8> {
    let kc = cbrt_consts();
    let mut h = init;
    let msg = pad_message(data, 128, 16, true);
    for chunk in msg.chunks(128) {
        let mut w = [0u64; 80];
        for i in 0..16 {
            let mut b = [0u8; 8];
            b.copy_from_slice(&chunk[8 * i..8 * i + 8]);
            w[i] = u64::from_be_bytes(b);
        }
        for i in 16..80 {
            let s0 = w[i - 15].rotate_right(1) ^ w[i - 15].rotate_right(8) ^ (w[i - 15] >> 7);
            let s1 = w[i - 2].rotate_right(19) ^ w[i - 2].rotate_right(61) ^ (w[i - 2] >> 6);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..80 {
            let s1 = v[4].rotate_right(14) ^ v[4].rotate_right(18) ^ v[4].rotate_right(41);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(kc[i]).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(28) ^ v[0].rotate_right(34) ^ v[0].rotate_right(39);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v[7] = v[6];
            v[6] = v[5];
            v[5] = v[4];
            v[4] = v[3].wrapping_add(t1);
            v[3] = v[2];
            v[2] = v[1];
            v[1] = v[0];
            v[0] = t1.wrapping_add(t2);
        }
        for i in 0..8 {
            h[i] = h[i].wrapping_add(v[i]);
        }
    }
    let mut out: Vec<u8> = h.iter().flat_map(|w| w.to_be_bytes()).collect();
    out.truncate(out_len);
    out
}

pub fn sha512(data: &[u8]) -> Vec<u8> {
    let s = sqrt_consts();
    let mut init = [0u64; 8];
    init.copy_from_slice(&s[..8]);
    sha512_core(data, init, 64)
}

pub fn sha384(data: &[u8]) -> Vec<u8> {
    let s = sqrt_consts();
    let mut init = [0u64; 8];
    init.copy_from_slice(&s[8..]);
    sha512_core(data, init, 48)
}

const BLAKE2B_SIGMA: [[usize; 16]; 10] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
    [11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4],
    [7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8],
    [9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13],
    [2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9],
    [12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11],
    [13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10],
    [6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5],
    [10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0],
];

fn blake2b_g(v: &mut [u64; 16], a: usize, b: usize, c: usize, d: usize, x: u64, y: u64) {
    v[a] = v[a].wrapping_add(v[b]).wrapping_add(x);
    v[d] = (v[d] ^ v[a]).rotate_right(32);
    v[c] = v[c].wrapping_add(v[d]);
    v[b] = (v[b] ^ v[c]).rotate_right(24);
    v[a] = v[a].wrapping_add(v[b]).wrapping_add(y);
    v[d] = (v[d] ^ v[a]).rotate_right(16);
    v[c] = v[c].wrapping_add(v[d]);
    v[b] = (v[b] ^ v[c]).rotate_right(63);
}

fn blake2b_compress(h: &mut [u64; 8], block: &[u8], t: u128, last: bool) {
    let iv = &sqrt_consts()[..8];
    let mut m = [0u64; 16];
    for (i, w) in m.iter_mut().enumerate() {
        let mut b = [0u8; 8];
        b.copy_from_slice(&block[8 * i..8 * i + 8]);
        *w = u64::from_le_bytes(b);
    }
    let mut v = [0u64; 16];
    v[..8].copy_from_slice(&h[..]);
    v[8..].copy_from_slice(iv);
    v[12] ^= t as u64;
    v[13] ^= (t >> 64) as u64;
    if last {
        v[14] = !v[14];
    }
    for r in 0..12 {
        let s = &BLAKE2B_SIGMA[r % 10];
        blake2b_g(&mut v, 0, 4, 8, 12, m[s[0]], m[s[1]]);
        blake2b_g(&mut v, 1, 5, 9, 13, m[s[2]], m[s[3]]);
        blake2b_g(&mut v, 2, 6, 10, 14, m[s[4]], m[s[5]]);
        blake2b_g(&mut v, 3, 7, 11, 15, m[s[6]], m[s[7]]);
        blake2b_g(&mut v, 0, 5, 10, 15, m[s[8]], m[s[9]]);
        blake2b_g(&mut v, 1, 6, 11, 12, m[s[10]], m[s[11]]);
        blake2b_g(&mut v, 2, 7, 8, 13, m[s[12]], m[s[13]]);
        blake2b_g(&mut v, 3, 4, 9, 14, m[s[14]], m[s[15]]);
    }
    for i in 0..8 {
        h[i] ^= v[i] ^ v[i + 8];
    }
}

pub fn blake2b(data: &[u8], out_len: usize) -> Vec<u8> {
    let mut h = [0u64; 8];
    h.copy_from_slice(&sqrt_consts()[..8]);
    h[0] ^= 0x0101_0000 ^ (out_len as u64);
    let n = data.len();
    if n == 0 {
        blake2b_compress(&mut h, &[0u8; 128], 0, true);
    } else {
        let mut off = 0usize;
        while n - off > 128 {
            blake2b_compress(&mut h, &data[off..off + 128], (off + 128) as u128, false);
            off += 128;
        }
        let mut block = [0u8; 128];
        block[..n - off].copy_from_slice(&data[off..]);
        blake2b_compress(&mut h, &block, n as u128, true);
    }
    let mut out: Vec<u8> = h.iter().flat_map(|w| w.to_le_bytes()).collect();
    out.truncate(out_len);
    out
}

// ---------------------------------------------------------------------------
// Algoritmo nomeado, HMAC e PBKDF2
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Algo {
    Md5,
    Sha1,
    Sha224,
    Sha256,
    Sha384,
    Sha512,
    Blake2b(usize),
    /// SHA-3 com o tamanho do resumo em bytes (28, 32, 48 ou 64).
    Sha3(usize),
}

/// Keccak-f[1600]. As constantes de rodada saem do LFSR do padrão e as rotações da caminhada
/// `(x, y) -> (y, 2x + 3y)`, em vez de digitadas.
fn keccak_f(a: &mut [u64; 25]) {
    let mut lfsr: u8 = 1;
    for _ in 0..24 {
        let mut c = [0u64; 5];
        for x in 0..5 {
            c[x] = a[x] ^ a[x + 5] ^ a[x + 10] ^ a[x + 15] ^ a[x + 20];
        }
        for x in 0..5 {
            let d = c[(x + 4) % 5] ^ c[(x + 1) % 5].rotate_left(1);
            for y in 0..5 {
                a[x + 5 * y] ^= d;
            }
        }
        let mut b = [0u64; 25];
        b[0] = a[0];
        let (mut x, mut y) = (1usize, 0usize);
        for t in 0..24u32 {
            let (nx, ny) = (y, (2 * x + 3 * y) % 5);
            b[nx + 5 * ny] = a[x + 5 * y].rotate_left(((t + 1) * (t + 2) / 2) % 64);
            x = nx;
            y = ny;
        }
        for y in 0..5 {
            for x in 0..5 {
                a[x + 5 * y] = b[x + 5 * y] ^ (!b[(x + 1) % 5 + 5 * y] & b[(x + 2) % 5 + 5 * y]);
            }
        }
        for j in 0..7 {
            let bit = lfsr & 1 != 0;
            lfsr = if lfsr & 0x80 != 0 { (lfsr << 1) ^ 0x71 } else { lfsr << 1 };
            if bit {
                a[0] ^= 1u64 << ((1u32 << j) - 1);
            }
        }
    }
}

/// SHA-3 (FIPS 202) com resumo de `out_len` bytes.
pub fn sha3(data: &[u8], out_len: usize) -> Vec<u8> {
    let rate = 200 - 2 * out_len;
    let mut msg = data.to_vec();
    msg.push(0x06);
    while msg.len() % rate != 0 {
        msg.push(0);
    }
    let last = msg.len() - 1;
    msg[last] |= 0x80;
    let mut state = [0u64; 25];
    for block in msg.chunks(rate) {
        for (i, lane) in block.chunks(8).enumerate() {
            let mut w = [0u8; 8];
            w.copy_from_slice(lane);
            state[i] ^= u64::from_le_bytes(w);
        }
        keccak_f(&mut state);
    }
    let mut out = Vec::with_capacity(out_len);
    for lane in state.iter() {
        out.extend_from_slice(&lane.to_le_bytes());
    }
    out.truncate(out_len);
    out
}

impl Algo {
    pub fn from_name(name: &str) -> Option<Algo> {
        Some(match name.to_ascii_lowercase().replace('-', "_").as_str() {
            "md5" => Algo::Md5,
            "sha1" => Algo::Sha1,
            "sha224" => Algo::Sha224,
            "sha256" => Algo::Sha256,
            "sha384" => Algo::Sha384,
            "sha512" => Algo::Sha512,
            "blake2b" => Algo::Blake2b(64),
            "sha3_224" => Algo::Sha3(28),
            "sha3_256" => Algo::Sha3(32),
            "sha3_384" => Algo::Sha3(48),
            "sha3_512" => Algo::Sha3(64),
            _ => return None,
        })
    }

    pub fn name(&self) -> &'static str {
        match self {
            Algo::Md5 => "md5",
            Algo::Sha1 => "sha1",
            Algo::Sha224 => "sha224",
            Algo::Sha256 => "sha256",
            Algo::Sha384 => "sha384",
            Algo::Sha512 => "sha512",
            Algo::Blake2b(_) => "blake2b",
            Algo::Sha3(28) => "sha3_224",
            Algo::Sha3(32) => "sha3_256",
            Algo::Sha3(48) => "sha3_384",
            Algo::Sha3(_) => "sha3_512",
        }
    }

    pub fn digest_size(&self) -> usize {
        match self {
            Algo::Md5 => 16,
            Algo::Sha1 => 20,
            Algo::Sha224 => 28,
            Algo::Sha256 => 32,
            Algo::Sha384 => 48,
            Algo::Sha512 => 64,
            Algo::Blake2b(n) | Algo::Sha3(n) => *n,
        }
    }

    pub fn block_size(&self) -> usize {
        match self {
            Algo::Md5 | Algo::Sha1 | Algo::Sha224 | Algo::Sha256 => 64,
            Algo::Sha384 | Algo::Sha512 | Algo::Blake2b(_) => 128,
            Algo::Sha3(n) => 200 - 2 * n,
        }
    }

    pub fn digest(&self, data: &[u8]) -> Vec<u8> {
        match self {
            Algo::Md5 => md5(data),
            Algo::Sha1 => sha1(data),
            Algo::Sha224 => sha224(data),
            Algo::Sha256 => sha256(data),
            Algo::Sha384 => sha384(data),
            Algo::Sha512 => sha512(data),
            Algo::Blake2b(n) => blake2b(data, *n),
            Algo::Sha3(n) => sha3(data, *n),
        }
    }
}

/// HMAC (RFC 2104) sobre o algoritmo dado.
pub fn hmac(algo: Algo, key: &[u8], msg: &[u8]) -> Vec<u8> {
    let bs = algo.block_size();
    let mut k = if key.len() > bs { algo.digest(key) } else { key.to_vec() };
    k.resize(bs, 0);
    let mut inner: Vec<u8> = k.iter().map(|b| b ^ 0x36).collect();
    inner.extend_from_slice(msg);
    let ih = algo.digest(&inner);
    let mut outer: Vec<u8> = k.iter().map(|b| b ^ 0x5c).collect();
    outer.extend_from_slice(&ih);
    algo.digest(&outer)
}

/// PBKDF2 (RFC 8018) com HMAC.
pub fn pbkdf2(algo: Algo, password: &[u8], salt: &[u8], iterations: u64, dklen: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(dklen);
    let mut block: u32 = 1;
    while out.len() < dklen {
        let mut s = salt.to_vec();
        s.extend_from_slice(&block.to_be_bytes());
        let mut u = hmac(algo, password, &s);
        let mut t = u.clone();
        for _ in 1..iterations {
            u = hmac(algo, password, &u);
            for (a, b) in t.iter_mut().zip(u.iter()) {
                *a ^= b;
            }
        }
        out.extend_from_slice(&t);
        block += 1;
    }
    out.truncate(dklen);
    out
}

// ---------------------------------------------------------------------------
// Objeto Python
// ---------------------------------------------------------------------------

fn hex_of(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(DIGITS[(b >> 4) as usize] as char);
        s.push(DIGITS[(b & 15) as usize] as char);
    }
    s
}

fn want_hash_input(v: &Value) -> PyResult<Vec<u8>> {
    match v {
        Value::Bytes(_) | Value::ByteArray(_) | Value::Instance(_) if v.bytes_like().is_some() => {
            Ok(v.bytes_like().map(|b| b.to_vec()).unwrap_or_default())
        }
        Value::Str(_) => Err(type_error("Strings must be encoded before hashing")),
        _ => Err(type_error("object supporting the buffer API required")),
    }
}

struct HashObj {
    algo: Algo,
    data: RefCell<Vec<u8>>,
}

impl ExtObject for HashObj {
    fn type_name(&self) -> &'static str {
        "HASH"
    }

    fn repr(&self) -> String {
        format!("<{} _hashlib.HASH object @ {:#x}>", self.algo.name(), crate::object::py_addr(self as *const Self as usize))
    }

    fn methods(&self) -> &'static [&'static str] {
        &["update", "digest", "hexdigest", "copy"]
    }

    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        match name {
            "name" => Some(Ok(Value::str(self.algo.name()))),
            "digest_size" => Some(Ok(Value::Int(self.algo.digest_size() as i64))),
            "block_size" => Some(Ok(Value::Int(self.algo.block_size() as i64))),
            _ => None,
        }
    }

    fn call_method(&self, _vm: &mut Vm, name: &str, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        match name {
            "update" => {
                let s = bind("update", args, kw, &["obj"], 1)?;
                let data = want_hash_input(s[0].as_ref().unwrap())?;
                self.data.borrow_mut().extend_from_slice(&data);
                Ok(Value::None)
            }
            "digest" => {
                crate::native_util::no_kwargs("digest", &kw)?;
                crate::native_util::exactly("digest", &args, 0)?;
                Ok(Value::bytes(self.algo.digest(&self.data.borrow())))
            }
            "hexdigest" => {
                crate::native_util::no_kwargs("hexdigest", &kw)?;
                crate::native_util::exactly("hexdigest", &args, 0)?;
                Ok(Value::str(hex_of(&self.algo.digest(&self.data.borrow()))))
            }
            "copy" => {
                crate::native_util::no_kwargs("copy", &kw)?;
                crate::native_util::exactly("copy", &args, 0)?;
                Ok(make_hash(self.algo, self.data.borrow().clone()))
            }
            _ => Err(exc("AttributeError", format!("'HASH' object has no attribute '{name}'"))),
        }
    }
}

fn make_hash(algo: Algo, data: Vec<u8>) -> Value {
    Value::Ext(Rc::new(HashObj { algo, data: RefCell::new(data) }))
}

fn new_hash(fname: &str, algo: Algo, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind(fname, args, kw, &["data", "usedforsecurity", "string"], 0)?;
    let init = match (&s[0], &s[2]) {
        (Some(v), _) | (None, Some(v)) => want_hash_input(v)?,
        (None, None) => Vec::new(),
    };
    Ok(make_hash(algo, init))
}

macro_rules! ctor {
    ($f:ident, $py:literal, $algo:expr) => {
        fn $f(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
            new_hash($py, $algo, args, kw)
        }
    };
}

ctor!(md5_new, "md5", Algo::Md5);
ctor!(sha1_new, "sha1", Algo::Sha1);
ctor!(sha224_new, "sha224", Algo::Sha224);
ctor!(sha256_new, "sha256", Algo::Sha256);
ctor!(sha384_new, "sha384", Algo::Sha384);
ctor!(sha512_new, "sha512", Algo::Sha512);
ctor!(sha3_224_new, "sha3_224", Algo::Sha3(28));
ctor!(sha3_256_new, "sha3_256", Algo::Sha3(32));
ctor!(sha3_384_new, "sha3_384", Algo::Sha3(48));
ctor!(sha3_512_new, "sha3_512", Algo::Sha3(64));

fn blake2b_new(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("blake2b", args, kw, &["data", "digest_size"], 0)?;
    let size = match &s[1] {
        None => 64,
        Some(v) => want_int(v)?,
    };
    if !(1..=64).contains(&size) {
        return Err(exc("ValueError", format!("digest_size for blake2b must be between 1 and 64 bytes, got {size}")));
    }
    let init = match &s[0] {
        Some(v) => want_hash_input(v)?,
        None => Vec::new(),
    };
    Ok(make_hash(Algo::Blake2b(size as usize), init))
}

fn new(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("new", args, kw, &["name", "data"], 1)?;
    let name = want_str("new", s[0].as_ref().unwrap())?;
    let Some(algo) = Algo::from_name(name) else {
        return Err(exc("ValueError", format!("unsupported hash type {name}")));
    };
    let init = match &s[1] {
        Some(v) => want_hash_input(v)?,
        None => Vec::new(),
    };
    Ok(make_hash(algo, init))
}

fn pbkdf2_hmac(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("pbkdf2_hmac", args, kw, &["hash_name", "password", "salt", "iterations", "dklen"], 4)?;
    let name = want_str("pbkdf2_hmac", s[0].as_ref().unwrap())?;
    let Some(algo) = Algo::from_name(name) else {
        return Err(exc("ValueError", format!("unsupported hash type {name}")));
    };
    let password = want_hash_input(s[1].as_ref().unwrap())?;
    let salt = want_hash_input(s[2].as_ref().unwrap())?;
    let iterations = want_int(s[3].as_ref().unwrap())?;
    if iterations < 1 {
        return Err(exc("ValueError", "iteration value must be greater than 0."));
    }
    let dklen = match &s[4] {
        None | Some(Value::None) => algo.digest_size() as i64,
        Some(v) => want_int(v)?,
    };
    if dklen < 1 {
        return Err(exc("ValueError", "key length must be greater than 0."));
    }
    Ok(Value::bytes(pbkdf2(algo, &password, &salt, iterations as u64, dklen as usize)))
}

fn names_set(names: &[&str]) -> Value {
    let mut set = crate::object::Set::new();
    for n in names {
        let _ = set.add(Value::str(*n));
    }
    Value::set(set)
}

pub fn build(_vm: &mut Vm) -> Rc<ModuleObj> {
    ModuleBuilder::new("hashlib")
        .func("md5", md5_new)
        .func("sha1", sha1_new)
        .func("sha224", sha224_new)
        .func("sha256", sha256_new)
        .func("sha384", sha384_new)
        .func("sha512", sha512_new)
        .func("sha3_224", sha3_224_new)
        .func("sha3_256", sha3_256_new)
        .func("sha3_384", sha3_384_new)
        .func("sha3_512", sha3_512_new)
        .value("algorithms_guaranteed", names_set(&["blake2b", "md5", "sha1", "sha224", "sha256", "sha384", "sha3_224", "sha3_256", "sha3_384", "sha3_512", "sha512"]))
        .value("algorithms_available", names_set(&["blake2b", "md5", "sha1", "sha224", "sha256", "sha384", "sha3_224", "sha3_256", "sha3_384", "sha3_512", "sha512"]))
        .func("blake2b", blake2b_new)
        .func("new", new)
        .func("pbkdf2_hmac", pbkdf2_hmac)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::repr;

    fn hx(v: Vec<u8>) -> String {
        hex_of(&v)
    }

    #[test]
    fn md5_vectors() {
        assert_eq!(hx(md5(b"")), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(hx(md5(b"abc")), "900150983cd24fb0d6963f7d28e17f72");
    }

    #[test]
    fn sha1_vectors() {
        assert_eq!(hx(sha1(b"abc")), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(hx(sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    }

    #[test]
    fn sha2_vectors() {
        assert_eq!(hx(sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(hx(sha256(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(hx(sha224(b"abc")), "23097d223405d8228642a477bda255b32aadbce4bda0b3f7e36c9da7");
        assert_eq!(
            hx(sha384(b"abc")),
            "cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7"
        );
        assert_eq!(
            hx(sha512(b"abc")),
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        );
    }

    #[test]
    fn blake2b_prefixes() {
        assert!(hx(blake2b(b"", 64)).starts_with("786a02f742015903"));
        assert!(hx(blake2b(b"abc", 64)).starts_with("ba80a53f981c4d0d"));
        assert_eq!(blake2b(b"abc", 32).len(), 32);
    }

    #[test]
    fn pbkdf2_rfc6070() {
        let dk = pbkdf2(Algo::Sha1, b"password", b"salt", 1, 20);
        assert_eq!(hx(dk), "0c60c80f961f0e71f3a9b524af6012062fe037a6");
    }

    #[test]
    fn python_object() {
        let mut vm = Vm::new();
        let h = md5_new(&mut vm, vec![Value::bytes(b"a".to_vec())], Vec::new()).unwrap();
        let Value::Ext(obj) = &h else { panic!("esperava um objeto Ext") };
        obj.call_method(&mut vm, "update", vec![Value::bytes(b"bc".to_vec())], Vec::new()).unwrap();
        let hexd = obj.call_method(&mut vm, "hexdigest", Vec::new(), Vec::new()).unwrap();
        assert_eq!(repr(&hexd), "'900150983cd24fb0d6963f7d28e17f72'");
        let copy = obj.call_method(&mut vm, "copy", Vec::new(), Vec::new()).unwrap();
        let Value::Ext(c) = &copy else { panic!("esperava um objeto Ext") };
        c.call_method(&mut vm, "update", vec![Value::bytes(b"d".to_vec())], Vec::new()).unwrap();
        let d1 = obj.call_method(&mut vm, "digest", Vec::new(), Vec::new()).unwrap();
        let d2 = c.call_method(&mut vm, "digest", Vec::new(), Vec::new()).unwrap();
        assert_ne!(repr(&d1), repr(&d2));
        let size = obj.getattr(&mut vm, "digest_size").unwrap().unwrap();
        assert_eq!(repr(&size), "16");
        let name = obj.getattr(&mut vm, "name").unwrap().unwrap();
        assert_eq!(repr(&name), "'md5'");
        let e = md5_new(&mut vm, vec![Value::str("x")], Vec::new()).unwrap_err();
        assert_eq!(e.msg, "Strings must be encoded before hashing");
        let e = new(&mut vm, vec![Value::str("nope")], Vec::new()).unwrap_err();
        assert_eq!(e.msg, "unsupported hash type nope");
    }
}
