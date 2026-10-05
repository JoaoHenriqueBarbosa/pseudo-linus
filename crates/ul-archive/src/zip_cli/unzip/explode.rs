//! O explode do unzip (explode.c), pro método 6 (implode do PKZIP 1.x): árvores de Shannon-Fano
//! lidas como listas de comprimentos, códigos com os bits invertidos, janela de 4 ou 8 KiB dentro
//! da janela de 64 KiB do inflate, e o comportamento do C em dados corrompidos (a leitura além do
//! fim acrescenta bits ligados e a conta dos bytes usados aparece no aviso de tamanho).

use super::inflate::{huft_build, Huft, Tables, WSIZE};
use super::Uz;

/// Código inválido numa tabela (`INVALID_CODE`).
const INVALID_CODE: u8 = 99;

const fn ramp(start: u16, step: u16) -> [u16; 64] {
    let mut a = [0u16; 64];
    let mut i = 0;
    while i < 64 {
        a[i] = start + step * i as u16;
        i += 1;
    }
    a
}

/// Comprimentos de cópia sem árvore de literais (mínimo 2) e com ela (mínimo 3).
const CPLEN2: [u16; 64] = ramp(2, 1);
const CPLEN3: [u16; 64] = ramp(3, 1);
/// Bits extras dos comprimentos: só o último código lê mais 8 bits.
const EXTRA: [u8; 64] = {
    let mut a = [0u8; 64];
    a[63] = 8;
    a
};
/// Bases das distâncias da janela de 4 KiB e da de 8 KiB.
const CPDIST4: [u16; 64] = ramp(1, 64);
const CPDIST8: [u16; 64] = ramp(1, 128);

/// `mask_bits[n]`.
fn mask(n: u32) -> u64 {
    (1u64 << n) - 1
}

fn dump(b: &mut u64, k: &mut u32, n: u32) {
    *b >>= n;
    *k -= n;
}

impl Uz {
    /// `NEXTBYTE` como `unsigned`: o EOF (-1) vira todos os bits ligados.
    fn next_byte_u32(&mut self) -> u32 {
        self.next_byte().map_or(u32::MAX, u32::from)
    }

    /// `NEEDBITS` do explode, sem conferência de fim: `(ulg)EOF` acrescenta bits ligados.
    fn ex_needbits(&mut self, b: &mut u64, k: &mut u32, n: u32) {
        while *k < n {
            let c = self.next_byte().map_or(u64::MAX, u64::from);
            *b |= c << *k;
            *k += 8;
        }
    }

    /// `DECODEHUFT`: os códigos do implode vêm com os bits invertidos.
    fn ex_decode(&mut self, t: &Tables, bits: u32, b: &mut u64, k: &mut u32) -> Result<Huft, i32> {
        self.ex_needbits(b, k, bits);
        let mut h = t.h[t.root + (!*b & mask(bits)) as usize];
        loop {
            dump(b, k, u32::from(h.b));
            if h.e <= 32 {
                return Ok(h);
            }
            if h.e == INVALID_CODE {
                return Err(1);
            }
            let e = u32::from(h.e & 31);
            self.ex_needbits(b, k, e);
            h = t.h[h.v as usize + (!*b & mask(e)) as usize];
        }
    }

    /// `get_tree`: a lista de comprimentos em pares (comprimento, repetições) de 4 bits cada,
    /// precedida da quantidade de pares menos um. 4 se a lista não fecha em `n` códigos.
    fn get_tree(&mut self, l: &mut [u32; 256], n: usize) -> i32 {
        let mut i = self.next_byte_u32().wrapping_add(1);
        let mut k = 0usize;
        loop {
            let j0 = self.next_byte_u32();
            let bits = (j0 & 0xf) + 1;
            let j = ((j0 & 0xf0) >> 4) + 1;
            if k + j as usize > n {
                return 4;
            }
            for _ in 0..j {
                l[k] = bits;
                k += 1;
            }
            i = i.wrapping_sub(1);
            if i == 0 {
                break;
            }
        }
        if k != n { 4 } else { 0 }
    }

    /// `explode`: lê as árvores (literais se o bit 2 da GPF está ligado, comprimentos e
    /// distâncias), escolhe a janela de 8 KiB pelo bit 1 e descomprime. Devolve 0, o código do
    /// `huft_build` ou do `get_tree`, o erro do `flush`, ou 5 quando os bytes usados não batem com o
    /// `csize` (`x.used_csize`).
    pub fn explode(&mut self) -> i32 {
        let mut bl = 7u32;
        let mut bd = if self.csize + self.zin.incnt > 200_000 { 8 } else { 7 };
        let mut l = [0u32; 256];
        let gpf = self.lrec.general_purpose_bit_flag;
        let mut lit: Option<(Tables, u32)> = None;
        let cplen = if gpf & 4 != 0 {
            let mut bb = 9u32;
            let r = self.get_tree(&mut l, 256);
            if r != 0 {
                return r;
            }
            let (r, tb) = huft_build(&l, 256, &[], &[], &mut bb);
            if r != 0 {
                return r;
            }
            lit = Some((tb.unwrap_or_default(), bb));
            &CPLEN3
        } else {
            &CPLEN2
        };
        let r = self.get_tree(&mut l, 64);
        if r != 0 {
            return r;
        }
        let (r, tl) = huft_build(&l[..64], 0, cplen, &EXTRA, &mut bl);
        if r != 0 {
            return r;
        }
        let r = self.get_tree(&mut l, 64);
        if r != 0 {
            return r;
        }
        let (bdl, cpdist) = if gpf & 2 != 0 { (7, &CPDIST8) } else { (6, &CPDIST4) };
        let (r, td) = huft_build(&l[..64], 0, cpdist, &EXTRA, &mut bd);
        if r != 0 {
            return r;
        }
        let (tl, td) = (tl.unwrap_or_default(), td.unwrap_or_default());
        let mut slide = std::mem::take(&mut self.x.slide);
        slide.resize(WSIZE, 0);
        let lit = lit.as_ref().map(|(t, bb)| (t, *bb));
        let r = self.explode_codes(&mut slide, lit, &tl, &td, bl, bd, bdl);
        self.x.slide = slide;
        r
    }

    /// `explode_lit` e `explode_nolit`: um bit diz literal ou cópia; o literal vem pela árvore ou
    /// em 8 bits crus; a cópia lê os bits baixos da distância crus, os altos e o comprimento pelas
    /// árvores. Até o primeiro despejo da janela, cópia de antes do começo vira zeros.
    #[allow(clippy::too_many_arguments)]
    fn explode_codes(&mut self, slide: &mut [u8], lit: Option<(&Tables, u32)>, tl: &Tables, td: &Tables, bl: u32, bd: u32, bdl: u32) -> i32 {
        let (mut b, mut k, mut w) = (0u64, 0u32, 0usize);
        let mut unflushed = true;
        let mut s = self.lrec.ucsize;
        while s > 0 {
            self.ex_needbits(&mut b, &mut k, 1);
            if b & 1 != 0 {
                dump(&mut b, &mut k, 1);
                s -= 1;
                let byte = match lit {
                    Some((tb, bb)) => match self.ex_decode(tb, bb, &mut b, &mut k) {
                        Ok(h) => h.v as u8,
                        Err(r) => return r,
                    },
                    None => {
                        self.ex_needbits(&mut b, &mut k, 8);
                        let c = b as u8;
                        dump(&mut b, &mut k, 8);
                        c
                    }
                };
                slide[w] = byte;
                w += 1;
                if w == WSIZE {
                    let r = self.flush(&slide[..w]);
                    if r != 0 {
                        return r;
                    }
                    w = 0;
                    unflushed = false;
                }
                continue;
            }
            dump(&mut b, &mut k, 1);
            self.ex_needbits(&mut b, &mut k, bdl);
            let low = (b & mask(bdl)) as u32;
            dump(&mut b, &mut k, bdl);
            let h = match self.ex_decode(td, bd, &mut b, &mut k) {
                Ok(h) => h,
                Err(r) => return r,
            };
            let mut d = (w as u32).wrapping_sub(low).wrapping_sub(h.v) as usize;
            let h = match self.ex_decode(tl, bl, &mut b, &mut k) {
                Ok(h) => h,
                Err(r) => return r,
            };
            let mut n = h.v as usize;
            if h.e != 0 {
                self.ex_needbits(&mut b, &mut k, 8);
                n += (b & 0xff) as usize;
                dump(&mut b, &mut k, 8);
            }
            s = s.saturating_sub(n as u64);
            loop {
                d &= WSIZE - 1;
                let e = (WSIZE - d.max(w)).min(n);
                n -= e;
                if unflushed && w <= d {
                    slide[w..w + e].fill(0);
                    w += e;
                    d += e;
                } else {
                    for _ in 0..e {
                        slide[w] = slide[d];
                        w += 1;
                        d += 1;
                    }
                }
                if w == WSIZE {
                    let r = self.flush(&slide[..w]);
                    if r != 0 {
                        return r;
                    }
                    w = 0;
                    unflushed = false;
                }
                if n == 0 {
                    break;
                }
            }
        }
        let r = self.flush(&slide[..w]);
        if r != 0 {
            return r;
        }
        // Devia ter lido `csize` bytes, mas às vezes lê um a mais: o `k >> 3` compensa.
        let left = self.csize + self.zin.incnt + i64::from(k >> 3);
        if left != 0 {
            self.x.used_csize = self.lrec.csize as i64 - left;
            return 5;
        }
        0
    }
}
