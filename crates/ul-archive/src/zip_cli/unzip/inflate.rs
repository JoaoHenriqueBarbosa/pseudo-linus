//! O inflate do próprio unzip (inflate.c, compilado sem zlib no Debian), pra deflate e deflate64:
//! a janela de 64 KiB despejada a cada volta, as tabelas de Huffman em vários níveis do
//! `huft_build` e o comportamento exato do C em dados corrompidos (o que já foi despejado fica,
//! o resto da janela some, e a leitura além do fim só vira erro quando falta bit de verdade).

use super::Uz;

/// Janela (`WSIZE`): 64 KiB, por causa do deflate64.
pub const WSIZE: usize = 0x10000;
/// Código inválido numa tabela (`INVALID_CODE`).
const INVALID_CODE: u8 = 99;
/// Maior comprimento de código e maior número de códigos (`BMAX`, `N_MAX`).
const BMAX: usize = 16;
const N_MAX: usize = 288;
/// Com `PKZIP_BUG_WORKAROUND`, como no Debian.
const MAXLITLENS: usize = 288;
const MAXDISTS: usize = 32;
/// Bits das tabelas de base de literais/comprimentos e de distâncias (`lbits`, `dbits`).
const LBITS: u32 = 9;
const DBITS: u32 = 6;

/// Ordem dos comprimentos do código de comprimentos (`border`).
const BORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
/// Comprimentos de cópia e bits extras dos códigos 257..287 (o 285 muda no deflate64).
const CPLENS64: [u16; 31] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 3, 0, 0];
const CPLENS32: [u16; 31] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258, 0, 0];
const CPLEXT64: [u8; 31] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 16, INVALID_CODE, INVALID_CODE];
const CPLEXT32: [u8; 31] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0, INVALID_CODE, INVALID_CODE];
/// Distâncias e bits extras dos códigos 0..31.
const CPDIST: [u16; 32] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577, 32769, 49153];
const CPDEXT64: [u8; 32] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13, 14, 14];
const CPDEXT32: [u8; 32] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13, INVALID_CODE, INVALID_CODE];

/// `mask_bits[n]`.
fn mask(n: u32) -> u64 {
    (1u64 << n) - 1
}

/// Uma entrada de tabela (`struct huft`): bits extras ou operação em `e`, bits a descartar em
/// `b`, e o valor ou o índice da subtabela em `v`.
#[derive(Clone, Copy, Default)]
pub struct Huft {
    pub e: u8,
    pub b: u8,
    pub v: u32,
}

/// As tabelas de um código: todas no mesmo vetor, a de base começando em `root`.
#[derive(Clone, Default)]
pub struct Tables {
    pub h: Vec<Huft>,
    pub root: usize,
}

/// Monta as tabelas de decodificação a partir dos comprimentos (`huft_build`). Devolve 0, 1 se o
/// código é incompleto (as tabelas saem mesmo assim) ou 2 se é inválido; `m` entra com o máximo
/// de bits da tabela de base e sai com o real. O código 256 (fim de bloco) termina numa borda de
/// tabela, pra que não se peça bit além dele.
pub fn huft_build(b: &[u32], s: u32, d: &[u16], e: &[u8], m: &mut u32) -> (i32, Option<Tables>) {
    let n = b.len();
    let el = if n > 256 { b[256] as i32 } else { BMAX as i32 };
    let mut c = [0u32; BMAX + 1];
    for &len in b {
        c[len as usize] += 1;
    }
    if c[0] as usize == n {
        *m = 0;
        return (0, None);
    }
    let mut j = 1usize;
    while j <= BMAX && c[j] == 0 {
        j += 1;
    }
    let k = j as i32;
    if (*m as usize) < j {
        *m = j as u32;
    }
    let mut i = BMAX;
    while i > 0 && c[i] == 0 {
        i -= 1;
    }
    let g = i as i32;
    if *m as usize > i {
        *m = i as u32;
    }
    let mut y: i64 = 1 << j;
    while j < i {
        y -= i64::from(c[j]);
        if y < 0 {
            return (2, None);
        }
        j += 1;
        y <<= 1;
    }
    y -= i64::from(c[i]);
    if y < 0 {
        return (2, None);
    }
    c[i] += y as u32;
    // Onde começa cada comprimento na lista de valores.
    let mut x = [0u32; BMAX + 1];
    let mut acc = 0u32;
    for len in 2..=i {
        acc += c[len - 1];
        x[len] = acc;
    }
    let mut v = [0u32; N_MAX];
    for (idx, &len) in b.iter().enumerate() {
        if len != 0 {
            v[x[len as usize] as usize] = idx as u32;
            x[len as usize] += 1;
        }
    }
    let nv = x[g as usize] as usize;
    build_tables(&c, &v, nv, s, d, e, k, g, el, m, y)
}

/// A segunda metade do `huft_build`: percorre os códigos em ordem de comprimento, abrindo
/// subtabelas quando o código passa dos bits já decodificados e preenchendo as entradas
/// repetidas de cada código.
#[allow(clippy::too_many_arguments)]
fn build_tables(c: &[u32; BMAX + 1], v: &[u32], nv: usize, s: u32, d: &[u16], e: &[u8], kmin: i32, g: i32, el: i32, m: &mut u32, y: i64) -> (i32, Option<Tables>) {
    let mut t = Tables::default();
    let mut x = [0u32; BMAX + 1];
    // `l[-1..BMAX-1]`: bits de cada nível de tabela, com o nível -1 valendo 0.
    let mut lx = [0i32; BMAX + 1];
    let l = |lx: &[i32; BMAX + 1], h: i32| lx[(h + 1) as usize];
    let mut u = [0usize; BMAX];
    let mut i: u32 = 0;
    let mut p = 0usize;
    let mut h: i32 = -1;
    let mut w: i32 = 0;
    let mut q = 0usize;
    let mut z: u32 = 0;
    let mut r = Huft::default();
    for k in kmin..=g {
        let mut a = c[k as usize];
        while a != 0 {
            a -= 1;
            while k > w + l(&lx, h) {
                w += l(&lx, h);
                h += 1;
                z = ((g - w) as u32).min(*m);
                let mut j = (k - w) as u32;
                let mut f: u32 = 1 << j;
                if f > a + 1 {
                    f -= a + 1;
                    let mut xp = k as usize;
                    loop {
                        j += 1;
                        if j >= z {
                            break;
                        }
                        f <<= 1;
                        xp += 1;
                        if f <= c[xp] {
                            break;
                        }
                        f -= c[xp];
                    }
                }
                if w + j as i32 > el && w < el {
                    j = (el - w) as u32;
                }
                z = 1 << j;
                lx[(h + 1) as usize] = j as i32;
                q = t.h.len();
                t.h.resize(q + z as usize, Huft::default());
                if h == 0 {
                    t.root = q;
                }
                u[h as usize] = q;
                if h > 0 {
                    x[h as usize] = i;
                    r.b = l(&lx, h - 1) as u8;
                    r.e = (32 + j) as u8;
                    r.v = q as u32;
                    let jj = ((i & ((1 << w) - 1)) >> (w - l(&lx, h - 1))) as usize;
                    t.h[u[h as usize - 1] + jj] = r;
                }
            }
            r.b = (k - w) as u8;
            if p >= nv {
                r.e = INVALID_CODE;
            } else if v[p] < s {
                r.e = if v[p] < 256 { 32 } else { 31 };
                r.v = v[p];
                p += 1;
            } else {
                let idx = (v[p] - s) as usize;
                r.e = e[idx];
                r.v = u32::from(d[idx]);
                p += 1;
            }
            let f = 1u32 << (k - w);
            let mut jj = i >> w;
            while jj < z {
                t.h[q + jj as usize] = r;
                jj += f;
            }
            let mut bit = 1u32 << (k - 1);
            while i & bit != 0 {
                i ^= bit;
                bit >>= 1;
            }
            i ^= bit;
            while (i & ((1 << w) - 1)) != x[h as usize] {
                h -= 1;
                w -= l(&lx, h);
            }
        }
    }
    *m = l(&lx, 0) as u32;
    (i32::from(y != 0 && g != 1), Some(t))
}

/// As tabelas dos blocos de Huffman fixo, montadas uma vez por processo (`G.fixed_tl32`...).
#[derive(Clone)]
pub struct Fixed {
    tl: Tables,
    bl: u32,
    td: Option<Tables>,
    bd: u32,
}

/// O estado de um inflate em andamento: a janela, a posição nela e o buffer de bits.
struct Inf {
    slide: Vec<u8>,
    wp: usize,
    bb: u64,
    bk: u32,
    defl64: bool,
}

impl Inf {
    fn cplens(&self) -> &'static [u16] {
        if self.defl64 { &CPLENS64 } else { &CPLENS32 }
    }
    fn cplext(&self) -> &'static [u8] {
        if self.defl64 { &CPLEXT64 } else { &CPLEXT32 }
    }
    fn cpdext(&self) -> &'static [u8] {
        if self.defl64 { &CPDEXT64 } else { &CPDEXT32 }
    }
}

/// O que sai de um laço de decodificação: um código de retorno pro chamador (1 e 2 são dados
/// inválidos, 3 falta de memória, os outros vêm do `flush`).
type Ret = Result<(), i32>;

impl Uz {
    /// Descomprime o membro deflate (ou deflate64) inteiro (`inflate`), despejando a janela pelo
    /// `flush`. Devolve 0 ou o código de erro.
    pub fn inflate(&mut self, defl64: bool) -> i32 {
        let mut slide = std::mem::take(&mut self.x.slide);
        slide.resize(WSIZE, 0);
        let mut s = Inf { slide, wp: 0, bb: 0, bk: 0, defl64 };
        let r = self.inflate_blocks(&mut s);
        self.x.slide = s.slide;
        r
    }

    fn inflate_blocks(&mut self, s: &mut Inf) -> i32 {
        loop {
            match self.inflate_block(s) {
                Ok(true) => break,
                Ok(false) => {}
                Err(r) => return r,
            }
        }
        self.flush_window(s, s.wp)
    }

    /// O inflate de um bloco comprimido do campo extra, todo em memória (`G.mem_mode`): a entrada
    /// é o próprio bloco, a saída cabe em `tgtsize` bytes (`memflush`), e o buffer do zip volta
    /// como estava.
    pub fn inflate_in_memory(&mut self, data: &[u8], tgtsize: usize, defl64: bool) -> (i32, Vec<u8>) {
        let saved = (std::mem::replace(&mut self.zin.buf, data.to_vec()), self.zin.inptr, self.zin.incnt, self.csize);
        self.zin.inptr = 0;
        self.zin.incnt = data.len() as i64;
        self.csize = data.len() as i64;
        self.x.mem = Some((Vec::new(), tgtsize));
        let r = self.inflate(defl64);
        let (out, _) = self.x.mem.take().unwrap_or_default();
        (self.zin.buf, self.zin.inptr, self.zin.incnt, self.csize) = saved;
        (r, out)
    }

    /// Despeja os `n` primeiros bytes da janela (`FLUSH`): no arquivo pelo `flush`, ou, no modo
    /// memória, no buffer de saída (`memflush`, que recusa passar do tamanho previsto).
    fn flush_window(&mut self, s: &mut Inf, n: usize) -> i32 {
        if let Some((out, room)) = &mut self.x.mem {
            if n > *room {
                return super::PK_DISK;
            }
            out.extend_from_slice(&s.slide[..n]);
            *room -= n;
            return 0;
        }
        let slide = std::mem::take(&mut s.slide);
        let r = self.flush(&slide[..n]);
        s.slide = slide;
        r
    }

    /// `NEEDBITS(n)`: garante `n` bits em `b`. No fim dos dados segue com o que tem enquanto a
    /// contagem não ficou negativa; só aí é erro (`CHECK_EOF` sem o ajuste de tabela).
    fn needbits(&mut self, b: &mut u64, k: &mut u32, n: u32) -> Ret {
        while (*k as i32) < n as i32 {
            match self.next_byte() {
                Some(c) => {
                    // A contagem só fica negativa depois do fim dos dados, quando não vem mais byte.
                    *b |= u64::from(c).wrapping_shl(*k);
                    *k = k.wrapping_add(8);
                }
                None if (*k as i32) >= 0 => break,
                None => return Err(1),
            }
        }
        Ok(())
    }

    /// Um bloco (`inflate_block`): `Ok(true)` se foi o último.
    fn inflate_block(&mut self, s: &mut Inf) -> Result<bool, i32> {
        let (mut b, mut k) = (s.bb, s.bk);
        self.needbits(&mut b, &mut k, 1)?;
        let last = b & 1 != 0;
        dump(&mut b, &mut k, 1);
        self.needbits(&mut b, &mut k, 2)?;
        let t = b & 3;
        dump(&mut b, &mut k, 2);
        s.bb = b;
        s.bk = k;
        match t {
            2 => self.inflate_dynamic(s)?,
            0 => self.inflate_stored(s)?,
            1 => self.inflate_fixed(s)?,
            _ => return Err(2),
        }
        Ok(last)
    }

    /// Bloco guardado sem compressão (`inflate_stored`).
    fn inflate_stored(&mut self, s: &mut Inf) -> Ret {
        let (mut b, mut k, mut w) = (s.bb, s.bk, s.wp);
        let skip = k & 7;
        dump(&mut b, &mut k, skip);
        self.needbits(&mut b, &mut k, 16)?;
        let mut n = (b & 0xffff) as u32;
        dump(&mut b, &mut k, 16);
        self.needbits(&mut b, &mut k, 16)?;
        if n != (!b & 0xffff) as u32 {
            return Err(1);
        }
        dump(&mut b, &mut k, 16);
        while n > 0 {
            n -= 1;
            self.needbits(&mut b, &mut k, 8)?;
            s.slide[w] = b as u8;
            w += 1;
            if w == WSIZE {
                self.flush_full(s, w)?;
                w = 0;
            }
            dump(&mut b, &mut k, 8);
        }
        s.wp = w;
        s.bb = b;
        s.bk = k;
        Ok(())
    }

    /// Despeja a janela cheia; um erro do `flush` encerra o inflate com ele.
    fn flush_full(&mut self, s: &mut Inf, w: usize) -> Ret {
        match self.flush_window(s, w) {
            0 => Ok(()),
            r => Err(r),
        }
    }

    /// Bloco de Huffman fixo (`inflate_fixed`), com as tabelas montadas na primeira vez.
    fn inflate_fixed(&mut self, s: &mut Inf) -> Ret {
        let slot = usize::from(s.defl64);
        if self.x.fixed[slot].is_none() {
            let mut l = [0u32; 288];
            for (i, len) in l.iter_mut().enumerate() {
                *len = match i {
                    0..=143 => 8,
                    144..=255 => 9,
                    256..=279 => 7,
                    _ => 8,
                };
            }
            let mut bl = 7;
            let (r, tl) = huft_build(&l, 257, s.cplens(), s.cplext(), &mut bl);
            if r != 0 {
                return Err(r);
            }
            let ld = [5u32; MAXDISTS];
            let mut bd = 5;
            let (r, td) = huft_build(&ld, 0, &CPDIST, s.cpdext(), &mut bd);
            if r > 1 {
                return Err(r);
            }
            self.x.fixed[slot] = Some(Fixed { tl: tl.unwrap_or_default(), bl, td, bd });
        }
        let f = self.x.fixed[slot].clone().unwrap_or_else(|| unreachable!());
        self.inflate_codes(s, &f.tl, f.td.as_ref(), f.bl, f.bd)
    }

    /// Bloco de Huffman dinâmico (`inflate_dynamic`): lê os comprimentos pelo código de
    /// comprimentos e monta as duas tabelas.
    fn inflate_dynamic(&mut self, s: &mut Inf) -> Ret {
        let (mut b, mut k) = (s.bb, s.bk);
        self.needbits(&mut b, &mut k, 5)?;
        let nl = 257 + (b & 0x1f) as usize;
        dump(&mut b, &mut k, 5);
        self.needbits(&mut b, &mut k, 5)?;
        let nd = 1 + (b & 0x1f) as usize;
        dump(&mut b, &mut k, 5);
        self.needbits(&mut b, &mut k, 4)?;
        let nb = 4 + (b & 0xf) as usize;
        dump(&mut b, &mut k, 4);
        if nl > MAXLITLENS || nd > MAXDISTS {
            return Err(1);
        }
        let mut ll = [0u32; MAXLITLENS + MAXDISTS];
        for &pos in &BORDER[..nb] {
            self.needbits(&mut b, &mut k, 3)?;
            ll[pos] = (b & 7) as u32;
            dump(&mut b, &mut k, 3);
        }
        for &pos in &BORDER[nb..] {
            ll[pos] = 0;
        }
        let mut bl = 7;
        let (mut r, tl) = huft_build(&ll[..19], 19, &[], &[], &mut bl);
        if bl == 0 {
            r = 1;
        }
        if r != 0 {
            return Err(r);
        }
        let tl = tl.unwrap_or_default();
        let n = nl + nd;
        let m = mask(bl);
        let (mut i, mut last) = (0usize, 0u32);
        while i < n {
            self.needbits(&mut b, &mut k, bl)?;
            let th = tl.h[tl.root + (b & m) as usize];
            dump(&mut b, &mut k, u32::from(th.b));
            let j = th.v;
            let (count, value) = match j {
                0..=15 => {
                    ll[i] = j;
                    i += 1;
                    last = j;
                    continue;
                }
                16 => {
                    self.needbits(&mut b, &mut k, 2)?;
                    let c = 3 + (b & 3) as usize;
                    dump(&mut b, &mut k, 2);
                    (c, last)
                }
                17 => {
                    self.needbits(&mut b, &mut k, 3)?;
                    let c = 3 + (b & 7) as usize;
                    dump(&mut b, &mut k, 3);
                    (c, 0)
                }
                _ => {
                    self.needbits(&mut b, &mut k, 7)?;
                    let c = 11 + (b & 0x7f) as usize;
                    dump(&mut b, &mut k, 7);
                    (c, 0)
                }
            };
            if i + count > n {
                return Err(1);
            }
            ll[i..i + count].fill(value);
            i += count;
            if j != 16 {
                last = 0;
            }
        }
        s.bb = b;
        s.bk = k;
        let mut bl = LBITS;
        let (mut r, tl) = huft_build(&ll[..nl], 257, s.cplens(), s.cplext(), &mut bl);
        if bl == 0 {
            r = 1;
        }
        if r != 0 {
            if r == 1 && self.o.qflag == 0 {
                self.info(super::MSG_STDERR, "(incomplete l-tree)  ");
            }
            return Err(r);
        }
        let mut bd = DBITS;
        let (mut r, td) = huft_build(&ll[nl..nl + nd], 0, &CPDIST, s.cpdext(), &mut bd);
        // `PKZIP_BUG_WORKAROUND`: código de distâncias incompleto passa.
        if r == 1 {
            r = 0;
        }
        if bd == 0 && nl > 257 {
            r = 1;
        }
        if r != 0 {
            if r == 1 && self.o.qflag == 0 {
                self.info(super::MSG_STDERR, "(incomplete d-tree)  ");
            }
            return Err(r);
        }
        self.inflate_codes(s, &tl.unwrap_or_default(), td.as_ref(), bl, bd)
    }

    /// Decodifica os códigos de um bloco até o fim de bloco (`inflate_codes`): literais vão pra
    /// janela, pares comprimento/distância copiam dela. Um código inválido devolve 1 sem salvar o
    /// estado, como no C.
    fn inflate_codes(&mut self, s: &mut Inf, tl: &Tables, td: Option<&Tables>, bl: u32, bd: u32) -> Ret {
        let (mut b, mut k, mut w) = (s.bb, s.bk, s.wp);
        let (ml, md) = (mask(bl), mask(bd));
        loop {
            self.needbits(&mut b, &mut k, bl)?;
            let mut t = tl.h[tl.root + (b & ml) as usize];
            loop {
                dump(&mut b, &mut k, u32::from(t.b));
                let e = u32::from(t.e);
                if e == 32 {
                    s.slide[w] = t.v as u8;
                    w += 1;
                    if w == WSIZE {
                        self.flush_full(s, w)?;
                        w = 0;
                    }
                    break;
                }
                if e < 31 {
                    self.needbits(&mut b, &mut k, e)?;
                    let mut n = t.v as usize + (b & mask(e)) as usize;
                    dump(&mut b, &mut k, e);
                    self.needbits(&mut b, &mut k, bd)?;
                    let td = td.unwrap_or_else(|| unreachable!("comprimento sem tabela de distâncias"));
                    let mut t2 = td.h[td.root + (b & md) as usize];
                    let mut e2;
                    loop {
                        dump(&mut b, &mut k, u32::from(t2.b));
                        e2 = u32::from(t2.e);
                        if e2 < 32 {
                            break;
                        }
                        if t2.e == INVALID_CODE {
                            return Err(1);
                        }
                        e2 &= 31;
                        self.needbits(&mut b, &mut k, e2)?;
                        t2 = td.h[t2.v as usize + (b & mask(e2)) as usize];
                    }
                    self.needbits(&mut b, &mut k, e2)?;
                    let mut d = (w as u32).wrapping_sub(t2.v).wrapping_sub((b & mask(e2)) as u32) as usize;
                    dump(&mut b, &mut k, e2);
                    loop {
                        d &= WSIZE - 1;
                        let mut cnt = WSIZE - d.max(w);
                        if cnt > n {
                            cnt = n;
                        }
                        n -= cnt;
                        for _ in 0..cnt {
                            s.slide[w] = s.slide[d];
                            w += 1;
                            d += 1;
                        }
                        if w == WSIZE {
                            self.flush_full(s, w)?;
                            w = 0;
                        }
                        if n == 0 {
                            break;
                        }
                    }
                    break;
                }
                if e == 31 {
                    s.wp = w;
                    s.bb = b;
                    s.bk = k;
                    return Ok(());
                }
                if t.e == INVALID_CODE {
                    return Err(1);
                }
                let e = e & 31;
                self.needbits(&mut b, &mut k, e)?;
                t = tl.h[t.v as usize + (b & mask(e)) as usize];
            }
        }
    }
}

/// `DUMPBITS(n)`; a contagem pode passar abaixo de zero depois do fim dos dados.
fn dump(b: &mut u64, k: &mut u32, n: u32) {
    *b >>= n;
    *k = k.wrapping_sub(n);
}
