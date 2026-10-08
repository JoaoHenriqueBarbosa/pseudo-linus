//! `_tls_crypto`: as primitivas quentes do TLS 1.3 do `_ssl.py`, em Rust: ChaCha20-Poly1305 (RFC 8439),
//! AES-GCM de chave 128 e 256 (NIST SP 800-38D, tabela de 4 bits para o GHASH), X25519 (RFC 7748) e HMAC.
//! É módulo de apoio: só código embutido o importa.

use std::rc::Rc;

use ul_common::hash::{hmac, Algo};

use crate::modules::ModuleBuilder;
use crate::native_util::{exactly, no_kwargs, value_error, want_str};
use crate::object::{Kw, ModuleObj, Value};
use crate::vm::{type_error, PyResult, Vm};

// ---------------------------------------------------------------------------
// ChaCha20 e Poly1305 (RFC 8439)
// ---------------------------------------------------------------------------

fn le32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

fn quarter(x: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    x[a] = x[a].wrapping_add(x[b]);
    x[d] = (x[d] ^ x[a]).rotate_left(16);
    x[c] = x[c].wrapping_add(x[d]);
    x[b] = (x[b] ^ x[c]).rotate_left(12);
    x[a] = x[a].wrapping_add(x[b]);
    x[d] = (x[d] ^ x[a]).rotate_left(8);
    x[c] = x[c].wrapping_add(x[d]);
    x[b] = (x[b] ^ x[c]).rotate_left(7);
}

fn chacha_block(key: &[u8; 32], counter: u32, nonce: &[u8; 12]) -> [u8; 64] {
    let mut state = [0u32; 16];
    state[..4].copy_from_slice(&[0x6170_7865, 0x3320_646e, 0x7962_2d32, 0x6b20_6574]);
    for i in 0..8 {
        state[4 + i] = le32(&key[4 * i..]);
    }
    state[12] = counter;
    for i in 0..3 {
        state[13 + i] = le32(&nonce[4 * i..]);
    }
    let mut x = state;
    for _ in 0..10 {
        quarter(&mut x, 0, 4, 8, 12);
        quarter(&mut x, 1, 5, 9, 13);
        quarter(&mut x, 2, 6, 10, 14);
        quarter(&mut x, 3, 7, 11, 15);
        quarter(&mut x, 0, 5, 10, 15);
        quarter(&mut x, 1, 6, 11, 12);
        quarter(&mut x, 2, 7, 8, 13);
        quarter(&mut x, 3, 4, 9, 14);
    }
    let mut out = [0u8; 64];
    for i in 0..16 {
        out[4 * i..4 * i + 4].copy_from_slice(&x[i].wrapping_add(state[i]).to_le_bytes());
    }
    out
}

fn chacha_xor(key: &[u8; 32], nonce: &[u8; 12], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    for (i, chunk) in data.chunks(64).enumerate() {
        let stream = chacha_block(key, 1 + i as u32, nonce);
        out.extend(chunk.iter().zip(stream.iter()).map(|(a, b)| a ^ b));
    }
    out
}

/// Poly1305 com limbs de 26 bits (a mesma divisão do poly1305-donna de 32 bits).
fn poly1305(key: &[u8; 32], msg: &[u8]) -> [u8; 16] {
    const M: u64 = 0x3ff_ffff;
    let (t0, t1, t2, t3) = (le32(&key[0..]), le32(&key[4..]), le32(&key[8..]), le32(&key[12..]));
    let r = [
        u64::from(t0 & 0x3ff_ffff),
        u64::from(((t0 >> 26) | (t1 << 6)) & 0x3ff_ff03),
        u64::from(((t1 >> 20) | (t2 << 12)) & 0x3ff_c0ff),
        u64::from(((t2 >> 14) | (t3 << 18)) & 0x3f0_3fff),
        u64::from((t3 >> 8) & 0x00f_ffff),
    ];
    let s = [r[1] * 5, r[2] * 5, r[3] * 5, r[4] * 5];
    let mut h = [0u64; 5];
    for chunk in msg.chunks(16) {
        let mut block = [0u8; 17];
        block[..chunk.len()].copy_from_slice(chunk);
        let hibit = if chunk.len() == 16 {
            1u64 << 24
        } else {
            block[chunk.len()] = 1;
            0
        };
        let (m0, m1, m2, m3) = (
            u64::from(le32(&block[0..])),
            u64::from(le32(&block[4..])),
            u64::from(le32(&block[8..])),
            u64::from(le32(&block[12..])),
        );
        h[0] += m0 & M;
        h[1] += ((m0 >> 26) | (m1 << 6)) & M;
        h[2] += ((m1 >> 20) | (m2 << 12)) & M;
        h[3] += ((m2 >> 14) | (m3 << 18)) & M;
        h[4] += (m3 >> 8) | hibit;
        let mut d = [
            h[0] * r[0] + h[1] * s[3] + h[2] * s[2] + h[3] * s[1] + h[4] * s[0],
            h[0] * r[1] + h[1] * r[0] + h[2] * s[3] + h[3] * s[2] + h[4] * s[1],
            h[0] * r[2] + h[1] * r[1] + h[2] * r[0] + h[3] * s[3] + h[4] * s[2],
            h[0] * r[3] + h[1] * r[2] + h[2] * r[1] + h[3] * r[0] + h[4] * s[3],
            h[0] * r[4] + h[1] * r[3] + h[2] * r[2] + h[3] * r[1] + h[4] * r[0],
        ];
        let mut c = 0;
        for (hi, di) in h.iter_mut().zip(d.iter_mut()) {
            *di += c;
            c = *di >> 26;
            *hi = *di & M;
        }
        h[0] += c * 5;
        c = h[0] >> 26;
        h[0] &= M;
        h[1] += c;
    }
    // Redução final: h mod (2^130 - 5), escolhendo entre h e h + 5 - 2^130 sem desvio.
    for i in 0..4 {
        let c = h[i] >> 26;
        h[i] &= M;
        h[i + 1] += c;
    }
    let c = h[4] >> 26;
    h[4] &= M;
    h[0] += c * 5;
    let c = h[0] >> 26;
    h[0] &= M;
    h[1] += c;
    let mut g = [0u64; 5];
    let mut c = 5;
    for i in 0..4 {
        g[i] = h[i] + c;
        c = g[i] >> 26;
        g[i] &= M;
    }
    g[4] = (h[4] + c).wrapping_sub(1 << 26);
    let mask = (g[4] >> 63).wrapping_sub(1);
    for i in 0..5 {
        h[i] = (h[i] & !mask) | (g[i] & mask);
    }
    let acc = u128::from(h[0]) | (u128::from(h[1]) << 26) | (u128::from(h[2]) << 52) | (u128::from(h[3]) << 78) | (u128::from(h[4]) << 104);
    let pad = u128::from_le_bytes(key[16..32].try_into().unwrap_or([0; 16]));
    acc.wrapping_add(pad).to_le_bytes()
}

fn chacha_tag(key: &[u8; 32], nonce: &[u8; 12], aad: &[u8], ct: &[u8]) -> [u8; 16] {
    let block = chacha_block(key, 0, nonce);
    let mut otk = [0u8; 32];
    otk.copy_from_slice(&block[..32]);
    let mut mac = Vec::with_capacity(aad.len() + ct.len() + 48);
    mac.extend_from_slice(aad);
    mac.resize(mac.len().div_ceil(16) * 16, 0);
    mac.extend_from_slice(ct);
    mac.resize(mac.len().div_ceil(16) * 16, 0);
    mac.extend_from_slice(&(aad.len() as u64).to_le_bytes());
    mac.extend_from_slice(&(ct.len() as u64).to_le_bytes());
    poly1305(&otk, &mac)
}

fn chacha_seal(key: &[u8; 32], nonce: &[u8; 12], aad: &[u8], plain: &[u8]) -> Vec<u8> {
    let mut out = chacha_xor(key, nonce, plain);
    let tag = chacha_tag(key, nonce, aad, &out);
    out.extend_from_slice(&tag);
    out
}

fn chacha_open(key: &[u8; 32], nonce: &[u8; 12], aad: &[u8], data: &[u8]) -> Option<Vec<u8>> {
    let split = data.len().checked_sub(16)?;
    let (ct, tag) = data.split_at(split);
    if !ct_eq(&chacha_tag(key, nonce, aad, ct), tag) {
        return None;
    }
    Some(chacha_xor(key, nonce, ct))
}

/// Comparação sem curto-circuito.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

// ---------------------------------------------------------------------------
// AES (FIPS 197) e GCM (SP 800-38D)
// ---------------------------------------------------------------------------

const fn xtime(b: u8) -> u8 {
    (b << 1) ^ if b & 0x80 != 0 { 0x1b } else { 0 }
}

/// A S-box pelo inverso multiplicativo em GF(2^8) mais a transformação afim, como na FIPS 197.
const fn build_sbox() -> [u8; 256] {
    let mut sbox = [0u8; 256];
    let mut p: u8 = 1;
    let mut q: u8 = 1;
    loop {
        p = p ^ (p << 1) ^ if p & 0x80 != 0 { 0x1b } else { 0 };
        q ^= q << 1;
        q ^= q << 2;
        q ^= q << 4;
        if q & 0x80 != 0 {
            q ^= 0x09;
        }
        let x = q ^ q.rotate_left(1) ^ q.rotate_left(2) ^ q.rotate_left(3) ^ q.rotate_left(4);
        sbox[p as usize] = x ^ 0x63;
        if p == 1 {
            break;
        }
    }
    sbox[0] = 0x63;
    sbox
}

const SBOX: [u8; 256] = build_sbox();

/// `Te0[x]` = coluna `[2s, s, s, 3s]` (big-endian) de `s = SBOX[x]`; as outras três são rotações.
const fn build_te0() -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let s = SBOX[i];
        let s2 = xtime(s);
        let s3 = s2 ^ s;
        t[i] = u32::from_be_bytes([s2, s, s, s3]);
        i += 1;
    }
    t
}

const TE0: [u32; 256] = build_te0();

fn sub_word(w: u32) -> u32 {
    let b = w.to_be_bytes();
    u32::from_be_bytes([SBOX[b[0] as usize], SBOX[b[1] as usize], SBOX[b[2] as usize], SBOX[b[3] as usize]])
}

struct Aes {
    round_keys: Vec<u32>,
    rounds: usize,
}

impl Aes {
    /// `key` de 16 ou 32 bytes (o chamador já validou).
    fn new(key: &[u8]) -> Aes {
        let nk = key.len() / 4;
        let rounds = nk + 6;
        let mut w: Vec<u32> = key.chunks(4).map(|c| u32::from_be_bytes([c[0], c[1], c[2], c[3]])).collect();
        let mut rcon: u8 = 1;
        for i in nk..4 * (rounds + 1) {
            let mut t = w[i - 1];
            if i % nk == 0 {
                t = sub_word(t.rotate_left(8)) ^ (u32::from(rcon) << 24);
                rcon = xtime(rcon);
            } else if nk > 6 && i % nk == 4 {
                t = sub_word(t);
            }
            w.push(w[i - nk] ^ t);
        }
        Aes { round_keys: w, rounds }
    }

    fn encrypt(&self, block: &[u8; 16]) -> [u8; 16] {
        let rk = &self.round_keys;
        let mut s = [0u32; 4];
        for (c, word) in s.iter_mut().enumerate() {
            *word = u32::from_be_bytes([block[4 * c], block[4 * c + 1], block[4 * c + 2], block[4 * c + 3]]) ^ rk[c];
        }
        for round in 1..self.rounds {
            let mut t = [0u32; 4];
            for c in 0..4 {
                t[c] = TE0[(s[c] >> 24) as usize]
                    ^ TE0[((s[(c + 1) % 4] >> 16) & 0xff) as usize].rotate_right(8)
                    ^ TE0[((s[(c + 2) % 4] >> 8) & 0xff) as usize].rotate_right(16)
                    ^ TE0[(s[(c + 3) % 4] & 0xff) as usize].rotate_right(24)
                    ^ rk[4 * round + c];
            }
            s = t;
        }
        let mut out = [0u8; 16];
        for c in 0..4 {
            let w = u32::from_be_bytes([
                SBOX[(s[c] >> 24) as usize],
                SBOX[((s[(c + 1) % 4] >> 16) & 0xff) as usize],
                SBOX[((s[(c + 2) % 4] >> 8) & 0xff) as usize],
                SBOX[(s[(c + 3) % 4] & 0xff) as usize],
            ]) ^ rk[4 * self.rounds + c];
            out[4 * c..4 * c + 4].copy_from_slice(&w.to_be_bytes());
        }
        out
    }
}

const GCM_R: u128 = 0xe1 << 120;

fn gcm_shift(v: u128) -> u128 {
    (v >> 1) ^ if v & 1 != 0 { GCM_R } else { 0 }
}

/// Multiplicação em GF(2^128) pelo método de Shoup: tabela de 4 bits de `n * h` e a de redução.
struct Ghash {
    table: [u128; 16],
    red: [u128; 16],
}

impl Ghash {
    fn new(h: u128) -> Ghash {
        let b4 = gcm_shift(h);
        let b2 = gcm_shift(b4);
        let b1 = gcm_shift(b2);
        let base = [(8usize, h), (4, b4), (2, b2), (1, b1)];
        let mut table = [0u128; 16];
        for (n, slot) in table.iter_mut().enumerate() {
            *slot = base.iter().filter(|entry| n & entry.0 != 0).fold(0, |acc, entry| acc ^ entry.1);
        }
        let mut red = [0u128; 16];
        for (r, slot) in red.iter_mut().enumerate() {
            *slot = (0..4).fold(r as u128, |t, _| gcm_shift(t));
        }
        Ghash { table, red }
    }

    fn mul(&self, x: u128) -> u128 {
        let mut z = 0u128;
        for shift in (0..128).step_by(4) {
            z = (z >> 4) ^ self.red[(z & 0xf) as usize] ^ self.table[((x >> shift) & 0xf) as usize];
        }
        z
    }

    fn digest(&self, aad: &[u8], ct: &[u8]) -> u128 {
        let mut y = 0u128;
        for part in [aad, ct] {
            for chunk in part.chunks(16) {
                let mut block = [0u8; 16];
                block[..chunk.len()].copy_from_slice(chunk);
                y = self.mul(y ^ u128::from_be_bytes(block));
            }
        }
        self.mul(y ^ ((((aad.len() as u128) * 8) << 64) | ((ct.len() as u128) * 8)))
    }
}

fn gcm_counter_block(nonce: &[u8; 12], counter: u32) -> [u8; 16] {
    let mut block = [0u8; 16];
    block[..12].copy_from_slice(nonce);
    block[12..].copy_from_slice(&counter.to_be_bytes());
    block
}

fn gcm_ctr(aes: &Aes, nonce: &[u8; 12], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    for (i, chunk) in data.chunks(16).enumerate() {
        let stream = aes.encrypt(&gcm_counter_block(nonce, 2 + i as u32));
        out.extend(chunk.iter().zip(stream.iter()).map(|(a, b)| a ^ b));
    }
    out
}

fn gcm_tag(aes: &Aes, ghash: &Ghash, nonce: &[u8; 12], aad: &[u8], ct: &[u8]) -> [u8; 16] {
    let mask = u128::from_be_bytes(aes.encrypt(&gcm_counter_block(nonce, 1)));
    (mask ^ ghash.digest(aad, ct)).to_be_bytes()
}

fn gcm_setup(key: &[u8]) -> (Aes, Ghash) {
    let aes = Aes::new(key);
    let ghash = Ghash::new(u128::from_be_bytes(aes.encrypt(&[0; 16])));
    (aes, ghash)
}

fn gcm_seal(key: &[u8], nonce: &[u8; 12], aad: &[u8], plain: &[u8]) -> Vec<u8> {
    let (aes, ghash) = gcm_setup(key);
    let mut out = gcm_ctr(&aes, nonce, plain);
    let tag = gcm_tag(&aes, &ghash, nonce, aad, &out);
    out.extend_from_slice(&tag);
    out
}

fn gcm_open(key: &[u8], nonce: &[u8; 12], aad: &[u8], data: &[u8]) -> Option<Vec<u8>> {
    let split = data.len().checked_sub(16)?;
    let (ct, tag) = data.split_at(split);
    let (aes, ghash) = gcm_setup(key);
    if !ct_eq(&gcm_tag(&aes, &ghash, nonce, aad, ct), tag) {
        return None;
    }
    Some(gcm_ctr(&aes, nonce, ct))
}

// ---------------------------------------------------------------------------
// X25519 (RFC 7748), corpo de 2^255 - 19 em cinco limbs de 51 bits
// ---------------------------------------------------------------------------

const MASK51: u64 = (1 << 51) - 1;

#[derive(Clone, Copy)]
struct Fe([u64; 5]);

impl Fe {
    const ONE: Fe = Fe([1, 0, 0, 0, 0]);
    const ZERO: Fe = Fe([0; 5]);

    fn from_bytes(b: &[u8; 32]) -> Fe {
        let w = |i: usize| u64::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3], b[i + 4], b[i + 5], b[i + 6], b[i + 7]]);
        let (w0, w1, w2, w3) = (w(0), w(8), w(16), w(24));
        Fe([
            w0 & MASK51,
            ((w0 >> 51) | (w1 << 13)) & MASK51,
            ((w1 >> 38) | (w2 << 26)) & MASK51,
            ((w2 >> 25) | (w3 << 39)) & MASK51,
            (w3 >> 12) & MASK51,
        ])
    }

    fn carry(mut t: [u128; 5]) -> Fe {
        let m = u128::from(MASK51);
        let mut c = 0u128;
        for limb in t.iter_mut() {
            *limb += c;
            c = *limb >> 51;
            *limb &= m;
        }
        t[0] += c * 19;
        let c = t[0] >> 51;
        t[0] &= m;
        t[1] += c;
        Fe([t[0] as u64, t[1] as u64, t[2] as u64, t[3] as u64, t[4] as u64])
    }

    fn add(self, o: Fe) -> Fe {
        let mut r = [0u64; 5];
        for i in 0..5 {
            r[i] = self.0[i] + o.0[i];
        }
        Fe(r)
    }

    fn sub(self, o: Fe) -> Fe {
        // a + 2p - b: os limbs de 2p garantem que nenhuma subtração fica negativa.
        const TWO_P: [u64; 5] = [0xf_ffff_ffff_ffda, 0xf_ffff_ffff_fffe, 0xf_ffff_ffff_fffe, 0xf_ffff_ffff_fffe, 0xf_ffff_ffff_fffe];
        let mut r = [0u128; 5];
        for i in 0..5 {
            r[i] = u128::from(self.0[i] + TWO_P[i] - o.0[i]);
        }
        Fe::carry(r)
    }

    fn mul(self, o: Fe) -> Fe {
        let a = self.0.map(u128::from);
        let b = o.0.map(u128::from);
        Fe::carry([
            a[0] * b[0] + 19 * (a[1] * b[4] + a[2] * b[3] + a[3] * b[2] + a[4] * b[1]),
            a[0] * b[1] + a[1] * b[0] + 19 * (a[2] * b[4] + a[3] * b[3] + a[4] * b[2]),
            a[0] * b[2] + a[1] * b[1] + a[2] * b[0] + 19 * (a[3] * b[4] + a[4] * b[3]),
            a[0] * b[3] + a[1] * b[2] + a[2] * b[1] + a[3] * b[0] + 19 * (a[4] * b[4]),
            a[0] * b[4] + a[1] * b[3] + a[2] * b[2] + a[3] * b[1] + a[4] * b[0],
        ])
    }

    fn mul_small(self, k: u64) -> Fe {
        Fe::carry(self.0.map(|l| u128::from(l) * u128::from(k)))
    }

    /// `self^(p-2)`: o inverso, por quadrados e produtos sobre os bits de `p - 2 = 2^255 - 21`.
    fn invert(self) -> Fe {
        let mut r = Fe::ONE;
        for i in (0..255).rev() {
            r = r.mul(r);
            let bit = if i >= 5 { 1 } else { (0b01011 >> i) & 1 };
            if bit == 1 {
                r = r.mul(self);
            }
        }
        r
    }

    fn to_bytes(self) -> [u8; 32] {
        let mut t = self.0;
        for _ in 0..2 {
            for i in 0..4 {
                t[i + 1] += t[i] >> 51;
                t[i] &= MASK51;
            }
            t[0] += 19 * (t[4] >> 51);
            t[4] &= MASK51;
        }
        // Subtrai p se t >= p: q vale 1 exatamente quando t + 19 transborda 2^255.
        let mut q = (t[0] + 19) >> 51;
        for limb in &t[1..] {
            q = (limb + q) >> 51;
        }
        t[0] += 19 * q;
        for i in 0..4 {
            t[i + 1] += t[i] >> 51;
            t[i] &= MASK51;
        }
        t[4] &= MASK51;
        let words = [
            t[0] | (t[1] << 51),
            (t[1] >> 13) | (t[2] << 38),
            (t[2] >> 26) | (t[3] << 25),
            (t[3] >> 39) | (t[4] << 12),
        ];
        let mut out = [0u8; 32];
        for (i, w) in words.iter().enumerate() {
            out[8 * i..8 * i + 8].copy_from_slice(&w.to_le_bytes());
        }
        out
    }

    fn cswap(a: &mut Fe, b: &mut Fe, swap: u64) {
        let mask = 0u64.wrapping_sub(swap);
        for i in 0..5 {
            let x = mask & (a.0[i] ^ b.0[i]);
            a.0[i] ^= x;
            b.0[i] ^= x;
        }
    }
}

fn x25519(scalar: &[u8; 32], point: &[u8; 32]) -> [u8; 32] {
    let mut k = *scalar;
    k[0] &= 248;
    k[31] &= 127;
    k[31] |= 64;
    let x1 = Fe::from_bytes(point);
    let (mut x2, mut z2, mut x3, mut z3) = (Fe::ONE, Fe::ZERO, x1, Fe::ONE);
    let mut swap = 0u64;
    for t in (0..255).rev() {
        let kt = u64::from((k[t / 8] >> (t % 8)) & 1);
        swap ^= kt;
        Fe::cswap(&mut x2, &mut x3, swap);
        Fe::cswap(&mut z2, &mut z3, swap);
        swap = kt;
        let a = x2.add(z2);
        let aa = a.mul(a);
        let b = x2.sub(z2);
        let bb = b.mul(b);
        let e = aa.sub(bb);
        let c = x3.add(z3);
        let d = x3.sub(z3);
        let da = d.mul(a);
        let cb = c.mul(b);
        let s = da.add(cb);
        x3 = s.mul(s);
        let df = da.sub(cb);
        z3 = x1.mul(df.mul(df));
        x2 = aa.mul(bb);
        z2 = e.mul(aa.add(e.mul_small(121_665)));
    }
    Fe::cswap(&mut x2, &mut x3, swap);
    Fe::cswap(&mut z2, &mut z3, swap);
    x2.mul(z2.invert()).to_bytes()
}

// ---------------------------------------------------------------------------
// Ponte para o Python
// ---------------------------------------------------------------------------

fn want_bytes(fname: &str, what: &str, v: &Value) -> PyResult<Rc<[u8]>> {
    v.bytes_like().ok_or_else(|| type_error(format!("{fname}() {what} must be a bytes-like object, not {}", v.type_name())))
}

fn fixed<const N: usize>(fname: &str, what: &str, v: &Value) -> PyResult<[u8; N]> {
    let b = want_bytes(fname, what, v)?;
    <[u8; N]>::try_from(&*b).map_err(|_| value_error(format!("{fname}(): {what} must be {N} bytes")))
}

/// Os quatro argumentos (`key`, `nonce`, `aad`, `data`) das funções AEAD.
fn aead_args(fname: &str, args: &[Value], kw: &Kw) -> PyResult<(Rc<[u8]>, [u8; 12], Rc<[u8]>, Rc<[u8]>)> {
    no_kwargs(fname, kw)?;
    exactly(fname, args, 4)?;
    Ok((
        want_bytes(fname, "key", &args[0])?,
        fixed::<12>(fname, "nonce", &args[1])?,
        want_bytes(fname, "aad", &args[2])?,
        want_bytes(fname, "data", &args[3])?,
    ))
}

fn aes_key<'a>(fname: &str, key: &'a [u8]) -> PyResult<&'a [u8]> {
    if key.len() == 16 || key.len() == 32 {
        Ok(key)
    } else {
        Err(value_error(format!("{fname}(): key must be 16 or 32 bytes")))
    }
}

fn chacha_key(fname: &str, key: &[u8]) -> PyResult<[u8; 32]> {
    <[u8; 32]>::try_from(key).map_err(|_| value_error(format!("{fname}(): key must be 32 bytes")))
}

fn open_result(plain: Option<Vec<u8>>) -> Value {
    plain.map_or(Value::None, Value::bytes)
}

fn chacha_seal_py(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let (key, nonce, aad, data) = aead_args("chacha20_poly1305_seal", &args, &kw)?;
    Ok(Value::bytes(chacha_seal(&chacha_key("chacha20_poly1305_seal", &key)?, &nonce, &aad, &data)))
}

fn chacha_open_py(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let (key, nonce, aad, data) = aead_args("chacha20_poly1305_open", &args, &kw)?;
    Ok(open_result(chacha_open(&chacha_key("chacha20_poly1305_open", &key)?, &nonce, &aad, &data)))
}

fn gcm_seal_py(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let (key, nonce, aad, data) = aead_args("aes_gcm_seal", &args, &kw)?;
    Ok(Value::bytes(gcm_seal(aes_key("aes_gcm_seal", &key)?, &nonce, &aad, &data)))
}

fn gcm_open_py(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let (key, nonce, aad, data) = aead_args("aes_gcm_open", &args, &kw)?;
    Ok(open_result(gcm_open(aes_key("aes_gcm_open", &key)?, &nonce, &aad, &data)))
}

fn x25519_py(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("x25519", &kw)?;
    exactly("x25519", &args, 2)?;
    let scalar = fixed::<32>("x25519", "scalar", &args[0])?;
    let point = fixed::<32>("x25519", "point", &args[1])?;
    Ok(Value::bytes(x25519(&scalar, &point).to_vec()))
}

fn hmac_py(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("hmac", &kw)?;
    exactly("hmac", &args, 3)?;
    let name = want_str("hmac", &args[0])?;
    let Some(algo) = Algo::from_name(name) else {
        return Err(value_error(format!("unsupported hash type {name}")));
    };
    let key = want_bytes("hmac", "key", &args[1])?;
    let data = want_bytes("hmac", "data", &args[2])?;
    Ok(Value::bytes(hmac(algo, &key, &data)))
}

pub fn build(_vm: &mut Vm) -> Rc<ModuleObj> {
    ModuleBuilder::new("_tls_crypto")
        .func("chacha20_poly1305_seal", chacha_seal_py)
        .func("chacha20_poly1305_open", chacha_open_py)
        .func("aes_gcm_seal", gcm_seal_py)
        .func("aes_gcm_open", gcm_open_py)
        .func("x25519", x25519_py)
        .func("hmac", hmac_py)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unhex(s: &str) -> Vec<u8> {
        let s: String = s.split_whitespace().collect();
        (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect()
    }

    fn arr<const N: usize>(s: &str) -> [u8; N] {
        unhex(s).try_into().unwrap()
    }

    #[test]
    fn poly1305_rfc8439_2_5_2() {
        let key = arr::<32>("85d6be7857556d337f4452fe42d506a80103808afb0db2fd4abff6af4149f51b");
        let tag = poly1305(&key, b"Cryptographic Forum Research Group");
        assert_eq!(tag.to_vec(), unhex("a8061dc1305136c6c22b8baf0c0127a9"));
    }

    #[test]
    fn chacha20_block_rfc8439_2_3_2() {
        let key = arr::<32>("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f");
        let nonce = arr::<12>("000000090000004a00000000");
        let block = chacha_block(&key, 1, &nonce);
        assert_eq!(
            block.to_vec(),
            unhex(
                "10f1e7e4d13b5915500fdd1fa32071c4c7d1f4c733c068030422aa9ac3d46c4e
                 d2826446079faa0914c2d705d98b02a2b5129cd1de164eb9cbd083e8a2503c4e"
            )
        );
    }

    #[test]
    fn chacha20_poly1305_rfc8439_2_8_2() {
        let key = arr::<32>("808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f");
        let nonce = arr::<12>("070000004041424344454647");
        let aad = unhex("50515253c0c1c2c3c4c5c6c7");
        let plain = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
        let sealed = chacha_seal(&key, &nonce, &aad, plain);
        let expected = unhex(
            "d31a8d34648e60db7b86afbc53ef7ec2a4aded51296e08fea9e2b5a736ee62d6
             3dbea45e8ca9671282fafb69da92728b1a71de0a9e060b2905d6a5b67ecd3b36
             92ddbd7f2d778b8c9803aee328091b58fab324e4fad675945585808b4831d7bc
             3ff4def08e4b7a9de576d26586cec64b6116
             1ae10b594f09e26a7e902ecbd0600691",
        );
        assert_eq!(sealed, expected);
        assert_eq!(chacha_open(&key, &nonce, &aad, &sealed).as_deref(), Some(&plain[..]));
        let mut bad = sealed;
        bad[3] ^= 1;
        assert_eq!(chacha_open(&key, &nonce, &aad, &bad), None);
        assert_eq!(chacha_open(&key, &nonce, &aad, &[0u8; 15]), None);
    }

    #[test]
    fn aes_fips197_appendix_c() {
        let pt = arr::<16>("00112233445566778899aabbccddeeff");
        let k128 = unhex("000102030405060708090a0b0c0d0e0f");
        assert_eq!(Aes::new(&k128).encrypt(&pt).to_vec(), unhex("69c4e0d86a7b0430d8cdb78070b4c55a"));
        let k256 = unhex("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f");
        assert_eq!(Aes::new(&k256).encrypt(&pt).to_vec(), unhex("8ea2b7ca516745bfeafc49904b496089"));
    }

    #[test]
    fn gcm_nist_test_cases() {
        let zero_nonce = [0u8; 12];
        // Casos 1 e 2 (chave de 128 bits zerada).
        let k = [0u8; 16];
        assert_eq!(gcm_seal(&k, &zero_nonce, b"", b""), unhex("58e2fccefa7e3061367f1d57a4e7455a"));
        assert_eq!(
            gcm_seal(&k, &zero_nonce, b"", &[0u8; 16]),
            unhex("0388dace60b6a392f328c2b971b2fe78ab6e47d42cec13bdf53a67b21257bddf")
        );
        // Caso 13 (chave de 256 bits zerada).
        assert_eq!(
            gcm_seal(&[0u8; 32], &zero_nonce, b"", &[0u8; 16]),
            unhex("cea7403d4d606b6e074ec5d3baf39d18d0d1c8a799996bf0265b98b5d48ab919")
        );
        // Caso 4 (com dados adicionais e texto que não é múltiplo de 16).
        let key = unhex("feffe9928665731c6d6a8f9467308308");
        let nonce = arr::<12>("cafebabefacedbaddecaf888");
        let aad = unhex("feedfacedeadbeeffeedfacedeadbeefabaddad2");
        let plain = unhex(
            "d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a72
             1c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b39",
        );
        let expected = unhex(
            "42831ec2217774244b7221b784d0d49ce3aa212f2c02a4e035c17e2329aca12e
             21d514b25466931c7d8f6a5aac84aa051ba30b396a0aac973d58e091
             5bc94fbc3221a5db94fae95ae7121a47",
        );
        let sealed = gcm_seal(&key, &nonce, &aad, &plain);
        assert_eq!(sealed, expected);
        assert_eq!(gcm_open(&key, &nonce, &aad, &sealed), Some(plain));
        let mut bad = sealed;
        bad[0] ^= 0x80;
        assert_eq!(gcm_open(&key, &nonce, &aad, &bad), None);
    }

    #[test]
    fn gcm_256_roundtrip_and_tamper() {
        let key = [7u8; 32];
        let nonce = [9u8; 12];
        let plain: Vec<u8> = (0..1000u32).map(|i| (i * 31) as u8).collect();
        let sealed = gcm_seal(&key, &nonce, b"header", &plain);
        assert_eq!(sealed.len(), plain.len() + 16);
        assert_eq!(gcm_open(&key, &nonce, b"header", &sealed), Some(plain));
        assert_eq!(gcm_open(&key, &nonce, b"other", &sealed), None);
    }

    #[test]
    fn x25519_rfc7748_5_2() {
        let scalar = arr::<32>("a546e36bf0527c9d3b16154b82465edd62144c0ac1fc5a18506a2244ba449ac4");
        let point = arr::<32>("e6db6867583030db3594c1a424b15f7c726624ec26b3353b10a903a6d0ab1c4c");
        assert_eq!(
            x25519(&scalar, &point).to_vec(),
            unhex("c3da55379de9c6908e94ea4df28d084f32eccf03491c71f754b4075577a28552")
        );
    }

    #[test]
    fn x25519_rfc7748_6_1_diffie_hellman() {
        let mut base = [0u8; 32];
        base[0] = 9;
        let alice = arr::<32>("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a");
        let bob = arr::<32>("5dab087e624a8a4b79e17f8b83800ee66f3bb1292618b6fd1c2f8b27ff88e0eb");
        let alice_pub = x25519(&alice, &base);
        let bob_pub = x25519(&bob, &base);
        assert_eq!(alice_pub.to_vec(), unhex("8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a"));
        assert_eq!(bob_pub.to_vec(), unhex("de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f"));
        let shared = unhex("4a5d9d5ba4ce2de1728e3bf480350f25e07e21c947d19e3376f09b3c1e161742");
        assert_eq!(x25519(&alice, &bob_pub).to_vec(), shared);
        assert_eq!(x25519(&bob, &alice_pub).to_vec(), shared);
    }
}
