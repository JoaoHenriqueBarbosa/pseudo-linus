//! Árvores de Huffman e saída em bits do deflate do Info-ZIP (trees.c). Porte fiel: a saída precisa
//! ser idêntica à do zip do Debian, que usa o `deflate.c`/`trees.c` próprios (não o zlib).

use super::consts::{ASCII, BINARY, MAX_MATCH, MIN_MATCH, STORE, UNKNOWN};
use super::deflate::DeflateIo;

const MAX_BITS: usize = 15;
const MAX_BL_BITS: i32 = 7;
const LENGTH_CODES: usize = 29;
const LITERALS: usize = 256;
const END_BLOCK: usize = 256;
const L_CODES: usize = LITERALS + 1 + LENGTH_CODES;
const D_CODES: usize = 30;
const BL_CODES: usize = 19;
const HEAP_SIZE: usize = 2 * L_CODES + 1;
const LIT_BUFSIZE: usize = 0x8000;
const DIST_BUFSIZE: usize = LIT_BUFSIZE;
const REP_3_6: usize = 16;
const REPZ_3_10: usize = 17;
const REPZ_11_138: usize = 18;
const BUF_SIZE: i32 = 16;

const EXTRA_LBITS: [i32; LENGTH_CODES] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const EXTRA_DBITS: [i32; D_CODES] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];
const EXTRA_BLBITS: [i32; BL_CODES] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 3, 7];
const BL_ORDER: [usize; BL_CODES] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

/// `ct_data`: o primeiro campo é a freqüência ou o código, o segundo o pai ou o comprimento (cada par
/// é uma união no C, e o programa depende da sobreposição).
#[derive(Clone, Copy, Default)]
struct Ct {
    fc: u16,
    dl: u16,
}

/// Estado do heap compartilhado entre as três árvores (`heap`, `depth`, `bl_count`, `opt_len`...).
struct HeapState {
    heap: [i32; HEAP_SIZE],
    heap_len: usize,
    heap_max: usize,
    depth: [u8; HEAP_SIZE],
    bl_count: [u16; MAX_BITS + 1],
    opt_len: u64,
    static_len: u64,
}

fn bi_reverse(mut code: u32, mut len: i32) -> u32 {
    let mut res: u32 = 0;
    loop {
        res |= code & 1;
        code >>= 1;
        res <<= 1;
        len -= 1;
        if len <= 0 {
            break;
        }
    }
    res >> 1
}

/// A saída em bits (`bi_buf`, `bi_valid`) acumulando os bytes num vetor que o chamador descarrega.
pub struct BitOut {
    bi_buf: u32,
    bi_valid: i32,
    pub out: Vec<u8>,
}

impl BitOut {
    fn new() -> BitOut {
        BitOut { bi_buf: 0, bi_valid: 0, out: Vec::new() }
    }

    fn put_short(&mut self, w: u32) {
        self.out.push((w & 0xff) as u8);
        self.out.push(((w >> 8) & 0xff) as u8);
    }

    fn send_bits(&mut self, value: i32, length: i32) {
        let v = value as u32;
        self.bi_buf |= v << self.bi_valid;
        self.bi_valid += length;
        if self.bi_valid > BUF_SIZE {
            let b = self.bi_buf;
            self.put_short(b);
            self.bi_valid -= BUF_SIZE;
            self.bi_buf = v >> (length - self.bi_valid);
        }
    }

    fn windup(&mut self) {
        if self.bi_valid > 8 {
            let b = self.bi_buf;
            self.put_short(b);
        } else if self.bi_valid > 0 {
            self.out.push((self.bi_buf & 0xff) as u8);
        }
        self.bi_buf = 0;
        self.bi_valid = 0;
    }
}

fn pqdownheap(hs: &mut HeapState, tree: &[Ct], mut k: usize) {
    let v = hs.heap[k];
    let mut j = k << 1;
    while j <= hs.heap_len {
        if j < hs.heap_len && smaller(hs, tree, hs.heap[j + 1], hs.heap[j]) {
            j += 1;
        }
        let htemp = hs.heap[j];
        if smaller(hs, tree, v, htemp) {
            break;
        }
        hs.heap[k] = htemp;
        k = j;
        j <<= 1;
    }
    hs.heap[k] = v;
}

fn smaller(hs: &HeapState, tree: &[Ct], n: i32, m: i32) -> bool {
    let (n, m) = (n as usize, m as usize);
    tree[n].fc < tree[m].fc || (tree[n].fc == tree[m].fc && hs.depth[n] <= hs.depth[m])
}

/// `gen_bitlen`: comprimentos ótimos dos códigos de uma árvore.
fn gen_bitlen(hs: &mut HeapState, tree: &mut [Ct], stree: Option<&[Ct]>, extra: &[i32], base: usize, max_code: i32, max_length: i32) {
    let mut overflow: i32 = 0;
    for b in hs.bl_count.iter_mut() {
        *b = 0;
    }
    let root = hs.heap[hs.heap_max] as usize;
    tree[root].dl = 0;
    let mut h = hs.heap_max + 1;
    while h < HEAP_SIZE {
        let n = hs.heap[h] as usize;
        let mut bits = tree[tree[n].dl as usize].dl as i32 + 1;
        if bits > max_length {
            bits = max_length;
            overflow += 1;
        }
        tree[n].dl = bits as u16;
        h += 1;
        if n as i32 > max_code {
            continue;
        }
        hs.bl_count[bits as usize] += 1;
        let mut xbits = 0;
        if n >= base {
            xbits = extra[n - base];
        }
        let f = tree[n].fc as u64;
        hs.opt_len = hs.opt_len.wrapping_add(f.wrapping_mul((bits + xbits) as u64));
        if let Some(st) = stree {
            hs.static_len = hs.static_len.wrapping_add(f.wrapping_mul((st[n].dl as i32 + xbits) as u64));
        }
    }
    if overflow == 0 {
        return;
    }
    loop {
        let mut bits = (max_length - 1) as usize;
        while hs.bl_count[bits] == 0 {
            bits -= 1;
        }
        hs.bl_count[bits] = hs.bl_count[bits].wrapping_sub(1);
        hs.bl_count[bits + 1] = hs.bl_count[bits + 1].wrapping_add(2);
        hs.bl_count[max_length as usize] = hs.bl_count[max_length as usize].wrapping_sub(1);
        overflow -= 2;
        if overflow <= 0 {
            break;
        }
    }
    let mut bits = max_length;
    while bits != 0 {
        let mut n = hs.bl_count[bits as usize];
        while n != 0 {
            h -= 1;
            let m = hs.heap[h] as usize;
            if m as i32 > max_code {
                continue;
            }
            if tree[m].dl as i32 != bits {
                let delta = (bits as i64) - (tree[m].dl as i64);
                hs.opt_len = hs.opt_len.wrapping_add((delta.wrapping_mul(tree[m].fc as i64)) as u64);
                tree[m].dl = bits as u16;
            }
            n -= 1;
        }
        bits -= 1;
    }
}

/// `gen_codes`: os códigos canônicos a partir dos comprimentos (`bl_count` já preenchido).
fn gen_codes(bl_count: &[u16; MAX_BITS + 1], tree: &mut [Ct], max_code: i32) {
    let mut next_code = [0u16; MAX_BITS + 1];
    let mut code: u16 = 0;
    for bits in 1..=MAX_BITS {
        code = ((code as u32 + bl_count[bits - 1] as u32) << 1) as u16;
        next_code[bits] = code;
    }
    let mut n = 0i32;
    while n <= max_code {
        let len = tree[n as usize].dl as usize;
        if len != 0 {
            let c = next_code[len];
            next_code[len] = c.wrapping_add(1);
            tree[n as usize].fc = bi_reverse(c as u32, len as i32) as u16;
        }
        n += 1;
    }
}

/// `build_tree`: monta uma árvore e devolve `max_code`.
fn build_tree(hs: &mut HeapState, tree: &mut [Ct], stree: Option<&[Ct]>, elems: usize, extra: &[i32], base: usize, max_length: i32) -> i32 {
    let mut max_code: i32 = -1;
    let mut node = elems;
    hs.heap_len = 0;
    hs.heap_max = HEAP_SIZE;
    for n in 0..elems {
        if tree[n].fc != 0 {
            hs.heap_len += 1;
            hs.heap[hs.heap_len] = n as i32;
            max_code = n as i32;
            hs.depth[n] = 0;
        } else {
            tree[n].dl = 0;
        }
    }
    while hs.heap_len < 2 {
        let new = if max_code < 2 {
            max_code += 1;
            max_code
        } else {
            0
        };
        hs.heap_len += 1;
        hs.heap[hs.heap_len] = new;
        tree[new as usize].fc = 1;
        hs.depth[new as usize] = 0;
        hs.opt_len = hs.opt_len.wrapping_sub(1);
        if let Some(st) = stree {
            hs.static_len = hs.static_len.wrapping_sub(st[new as usize].dl as u64);
        }
    }
    let mut n = hs.heap_len / 2;
    while n >= 1 {
        pqdownheap(hs, tree, n);
        n -= 1;
    }
    loop {
        // pqremove(tree, n)
        let nn = hs.heap[1];
        hs.heap[1] = hs.heap[hs.heap_len];
        hs.heap_len -= 1;
        pqdownheap(hs, tree, 1);
        let m = hs.heap[1];
        hs.heap_max -= 1;
        hs.heap[hs.heap_max] = nn;
        hs.heap_max -= 1;
        hs.heap[hs.heap_max] = m;
        let (nu, mu) = (nn as usize, m as usize);
        tree[node].fc = tree[nu].fc.wrapping_add(tree[mu].fc);
        hs.depth[node] = hs.depth[nu].max(hs.depth[mu]).wrapping_add(1);
        tree[nu].dl = node as u16;
        tree[mu].dl = node as u16;
        hs.heap[1] = node as i32;
        node += 1;
        pqdownheap(hs, tree, 1);
        if hs.heap_len < 2 {
            break;
        }
    }
    hs.heap_max -= 1;
    hs.heap[hs.heap_max] = hs.heap[1];
    gen_bitlen(hs, tree, stree, extra, base, max_code, max_length);
    gen_codes(&hs.bl_count, tree, max_code);
    max_code
}

/// Parâmetros de compressão de um arquivo que o `trees.c` consulta (`level`, `strstart`...).
pub struct Trees {
    dyn_ltree: Vec<Ct>,
    dyn_dtree: Vec<Ct>,
    static_ltree: Vec<Ct>,
    static_dtree: Vec<Ct>,
    bl_tree: Vec<Ct>,
    l_max_code: i32,
    d_max_code: i32,
    bl_max_code: i32,
    hs: HeapState,
    length_code: [u8; MAX_MATCH - MIN_MATCH + 1],
    dist_code: [u8; 512],
    base_length: [i32; LENGTH_CODES],
    base_dist: [i32; D_CODES],
    l_buf: Vec<u8>,
    d_buf: Vec<u16>,
    flag_buf: Vec<u8>,
    last_lit: usize,
    last_dist: usize,
    last_flags: usize,
    flags: u8,
    flag_bit: u8,
    cmpr_bytelen: u64,
    cmpr_len_bits: u64,
    pub file_type: u16,
    pub file_method: i32,
    initialized: bool,
    pub bits: BitOut,
}

impl Trees {
    pub fn new() -> Trees {
        Trees {
            dyn_ltree: vec![Ct::default(); HEAP_SIZE],
            dyn_dtree: vec![Ct::default(); 2 * D_CODES + 1],
            static_ltree: vec![Ct::default(); L_CODES + 2],
            static_dtree: vec![Ct::default(); D_CODES],
            bl_tree: vec![Ct::default(); 2 * BL_CODES + 1],
            l_max_code: 0,
            d_max_code: 0,
            bl_max_code: 0,
            hs: HeapState {
                heap: [0; HEAP_SIZE],
                heap_len: 0,
                heap_max: 0,
                depth: [0; HEAP_SIZE],
                bl_count: [0; MAX_BITS + 1],
                opt_len: 0,
                static_len: 0,
            },
            length_code: [0; MAX_MATCH - MIN_MATCH + 1],
            dist_code: [0; 512],
            base_length: [0; LENGTH_CODES],
            base_dist: [0; D_CODES],
            l_buf: vec![0; LIT_BUFSIZE],
            d_buf: vec![0; DIST_BUFSIZE],
            flag_buf: vec![0; LIT_BUFSIZE / 8],
            last_lit: 0,
            last_dist: 0,
            last_flags: 0,
            flags: 0,
            flag_bit: 1,
            cmpr_bytelen: 0,
            cmpr_len_bits: 0,
            file_type: UNKNOWN,
            file_method: 0,
            initialized: false,
            bits: BitOut::new(),
        }
    }

    /// `ct_init`: zera os contadores do arquivo e, na primeira chamada, monta as tabelas.
    pub fn ct_init(&mut self, attr: u16, method: i32) {
        self.file_type = attr;
        self.file_method = method;
        self.cmpr_len_bits = 0;
        self.cmpr_bytelen = 0;
        self.bits = BitOut::new();
        if self.initialized {
            return;
        }
        self.initialized = true;
        let mut length = 0usize;
        let mut code = 0usize;
        while code < LENGTH_CODES - 1 {
            self.base_length[code] = length as i32;
            for _ in 0..(1usize << EXTRA_LBITS[code]) {
                self.length_code[length] = code as u8;
                length += 1;
            }
            code += 1;
        }
        self.length_code[length - 1] = code as u8;
        let mut dist = 0usize;
        code = 0;
        while code < 16 {
            self.base_dist[code] = dist as i32;
            for _ in 0..(1usize << EXTRA_DBITS[code]) {
                self.dist_code[dist] = code as u8;
                dist += 1;
            }
            code += 1;
        }
        dist >>= 7;
        while code < D_CODES {
            self.base_dist[code] = (dist << 7) as i32;
            for _ in 0..(1usize << (EXTRA_DBITS[code] - 7)) {
                self.dist_code[256 + dist] = code as u8;
                dist += 1;
            }
            code += 1;
        }
        for b in self.hs.bl_count.iter_mut() {
            *b = 0;
        }
        let mut n = 0usize;
        while n <= 143 {
            self.static_ltree[n].dl = 8;
            self.hs.bl_count[8] += 1;
            n += 1;
        }
        while n <= 255 {
            self.static_ltree[n].dl = 9;
            self.hs.bl_count[9] += 1;
            n += 1;
        }
        while n <= 279 {
            self.static_ltree[n].dl = 7;
            self.hs.bl_count[7] += 1;
            n += 1;
        }
        while n <= 287 {
            self.static_ltree[n].dl = 8;
            self.hs.bl_count[8] += 1;
            n += 1;
        }
        let bc = self.hs.bl_count;
        gen_codes(&bc, &mut self.static_ltree, (L_CODES + 1) as i32);
        for n in 0..D_CODES {
            self.static_dtree[n].dl = 5;
            self.static_dtree[n].fc = bi_reverse(n as u32, 5) as u16;
        }
        self.init_block();
    }

    fn init_block(&mut self) {
        for n in 0..L_CODES {
            self.dyn_ltree[n].fc = 0;
        }
        for n in 0..D_CODES {
            self.dyn_dtree[n].fc = 0;
        }
        for n in 0..BL_CODES {
            self.bl_tree[n].fc = 0;
        }
        self.dyn_ltree[END_BLOCK].fc = 1;
        self.hs.opt_len = 0;
        self.hs.static_len = 0;
        self.last_lit = 0;
        self.last_dist = 0;
        self.last_flags = 0;
        self.flags = 0;
        self.flag_bit = 1;
    }

    fn d_code(&self, dist: usize) -> usize {
        if dist < 256 {
            self.dist_code[dist] as usize
        } else {
            self.dist_code[256 + (dist >> 7)] as usize
        }
    }

    /// `ct_tally`: registra um literal (`dist == 0`) ou uma correspondência; devolve se o bloco deve
    /// ser descarregado.
    pub fn tally(&mut self, dist: u32, lc: u32, level: i32, strstart: u32, block_start: i64) -> bool {
        self.l_buf[self.last_lit] = lc as u8;
        self.last_lit += 1;
        if dist == 0 {
            let f = &mut self.dyn_ltree[lc as usize].fc;
            *f = f.wrapping_add(1);
        } else {
            let d = (dist - 1) as usize;
            let li = self.length_code[lc as usize] as usize + LITERALS + 1;
            self.dyn_ltree[li].fc = self.dyn_ltree[li].fc.wrapping_add(1);
            let dc = self.d_code(d);
            self.dyn_dtree[dc].fc = self.dyn_dtree[dc].fc.wrapping_add(1);
            self.d_buf[self.last_dist] = d as u16;
            self.last_dist += 1;
            self.flags |= self.flag_bit;
        }
        self.flag_bit = self.flag_bit.wrapping_shl(1);
        if (self.last_lit & 7) == 0 {
            self.flag_buf[self.last_flags] = self.flags;
            self.last_flags += 1;
            self.flags = 0;
            self.flag_bit = 1;
        }
        if level > 2 && (self.last_lit & 0xfff) == 0 {
            let mut out_length = (self.last_lit as u64) * 8;
            let in_length = (strstart as i64 - block_start) as u64;
            for dcode in 0..D_CODES {
                out_length = out_length.wrapping_add((self.dyn_dtree[dcode].fc as u64) * (5 + EXTRA_DBITS[dcode] as u64));
            }
            out_length >>= 3;
            if self.last_dist < self.last_lit / 2 && out_length < in_length / 2 {
                return true;
            }
        }
        self.last_lit == LIT_BUFSIZE - 1 || self.last_dist == DIST_BUFSIZE
    }

    fn scan_tree(bl_tree: &mut [Ct], tree: &mut [Ct], max_code: i32) {
        let mut prevlen: i32 = -1;
        let mut nextlen: i32 = tree[0].dl as i32;
        let mut count: i32 = 0;
        let mut max_count = 7;
        let mut min_count = 4;
        if nextlen == 0 {
            max_count = 138;
            min_count = 3;
        }
        tree[(max_code + 1) as usize].dl = 0xFFFF;
        for n in 0..=max_code {
            let curlen = nextlen;
            nextlen = tree[(n + 1) as usize].dl as i32;
            count += 1;
            if count < max_count && curlen == nextlen {
                continue;
            } else if count < min_count {
                bl_tree[curlen as usize].fc = bl_tree[curlen as usize].fc.wrapping_add(count as u16);
            } else if curlen != 0 {
                if curlen != prevlen {
                    bl_tree[curlen as usize].fc = bl_tree[curlen as usize].fc.wrapping_add(1);
                }
                bl_tree[REP_3_6].fc = bl_tree[REP_3_6].fc.wrapping_add(1);
            } else if count <= 10 {
                bl_tree[REPZ_3_10].fc = bl_tree[REPZ_3_10].fc.wrapping_add(1);
            } else {
                bl_tree[REPZ_11_138].fc = bl_tree[REPZ_11_138].fc.wrapping_add(1);
            }
            count = 0;
            prevlen = curlen;
            if nextlen == 0 {
                max_count = 138;
                min_count = 3;
            } else if curlen == nextlen {
                max_count = 6;
                min_count = 3;
            } else {
                max_count = 7;
                min_count = 4;
            }
        }
    }

    fn send_code(bits: &mut BitOut, c: usize, tree: &[Ct]) {
        bits.send_bits(tree[c].fc as i32, tree[c].dl as i32);
    }

    fn send_tree(bits: &mut BitOut, bl_tree: &[Ct], tree: &[Ct], max_code: i32) {
        let mut prevlen: i32 = -1;
        let mut nextlen: i32 = tree[0].dl as i32;
        let mut count: i32 = 0;
        let mut max_count = 7;
        let mut min_count = 4;
        if nextlen == 0 {
            max_count = 138;
            min_count = 3;
        }
        for n in 0..=max_code {
            let curlen = nextlen;
            nextlen = tree[(n + 1) as usize].dl as i32;
            count += 1;
            if count < max_count && curlen == nextlen {
                continue;
            } else if count < min_count {
                loop {
                    Self::send_code(bits, curlen as usize, bl_tree);
                    count -= 1;
                    if count == 0 {
                        break;
                    }
                }
            } else if curlen != 0 {
                if curlen != prevlen {
                    Self::send_code(bits, curlen as usize, bl_tree);
                    count -= 1;
                }
                Self::send_code(bits, REP_3_6, bl_tree);
                bits.send_bits(count - 3, 2);
            } else if count <= 10 {
                Self::send_code(bits, REPZ_3_10, bl_tree);
                bits.send_bits(count - 3, 3);
            } else {
                Self::send_code(bits, REPZ_11_138, bl_tree);
                bits.send_bits(count - 11, 7);
            }
            count = 0;
            prevlen = curlen;
            if nextlen == 0 {
                max_count = 138;
                min_count = 3;
            } else if curlen == nextlen {
                max_count = 6;
                min_count = 3;
            } else {
                max_count = 7;
                min_count = 4;
            }
        }
    }

    fn build_bl_tree(&mut self) -> i32 {
        Self::scan_tree(&mut self.bl_tree, &mut self.dyn_ltree, self.l_max_code);
        Self::scan_tree(&mut self.bl_tree, &mut self.dyn_dtree, self.d_max_code);
        self.bl_max_code = build_tree(&mut self.hs, &mut self.bl_tree, None, BL_CODES, &EXTRA_BLBITS, 0, MAX_BL_BITS);
        let mut max_blindex = (BL_CODES - 1) as i32;
        while max_blindex >= 3 {
            if self.bl_tree[BL_ORDER[max_blindex as usize]].dl != 0 {
                break;
            }
            max_blindex -= 1;
        }
        self.hs.opt_len = self.hs.opt_len.wrapping_add((3 * (max_blindex + 1) + 5 + 5 + 4) as u64);
        max_blindex
    }

    fn send_all_trees(&mut self, lcodes: i32, dcodes: i32, blcodes: i32) {
        self.bits.send_bits(lcodes - 257, 5);
        self.bits.send_bits(dcodes - 1, 5);
        self.bits.send_bits(blcodes - 4, 4);
        for rank in 0..blcodes {
            let l = self.bl_tree[BL_ORDER[rank as usize]].dl as i32;
            self.bits.send_bits(l, 3);
        }
        Self::send_tree(&mut self.bits, &self.bl_tree, &self.dyn_ltree, lcodes - 1);
        Self::send_tree(&mut self.bits, &self.bl_tree, &self.dyn_dtree, dcodes - 1);
    }

    /// `compress_block`: emite os símbolos do bloco com as árvores dadas (dinâmicas ou estáticas).
    fn compress_block(&mut self, dynamic: bool) {
        let mut lx = 0usize;
        let mut dx = 0usize;
        let mut fx = 0usize;
        let mut flag: u8 = 0;
        if self.last_lit != 0 {
            loop {
                if (lx & 7) == 0 {
                    flag = self.flag_buf[fx];
                    fx += 1;
                }
                let mut lc = self.l_buf[lx] as i32;
                lx += 1;
                let (lt, dt): (&Vec<Ct>, &Vec<Ct>) = if dynamic { (&self.dyn_ltree, &self.dyn_dtree) } else { (&self.static_ltree, &self.static_dtree) };
                if (flag & 1) == 0 {
                    Self::send_code(&mut self.bits, lc as usize, lt);
                } else {
                    let code = self.length_code[lc as usize] as usize;
                    Self::send_code(&mut self.bits, code + LITERALS + 1, lt);
                    let extra = EXTRA_LBITS[code];
                    if extra != 0 {
                        lc -= self.base_length[code];
                        self.bits.send_bits(lc, extra);
                    }
                    let mut dist = self.d_buf[dx] as i32;
                    dx += 1;
                    let dcode = if dist < 256 { self.dist_code[dist as usize] as usize } else { self.dist_code[256 + (dist as usize >> 7)] as usize };
                    Self::send_code(&mut self.bits, dcode, dt);
                    let extra = EXTRA_DBITS[dcode];
                    if extra != 0 {
                        dist -= self.base_dist[dcode];
                        self.bits.send_bits(dist, extra);
                    }
                }
                flag >>= 1;
                if lx >= self.last_lit {
                    break;
                }
            }
        }
        let lt = if dynamic { &self.dyn_ltree } else { &self.static_ltree };
        Self::send_code(&mut self.bits, END_BLOCK, lt);
    }

    /// `set_file_type`: texto (ASCII) ou binário, a partir das freqüências dos literais.
    fn set_file_type(&mut self) {
        let mut mask: u32 = 0xf3ff_c07f;
        for n in 0..=31usize {
            if (mask & 1) != 0 && self.dyn_ltree[n].fc != 0 {
                self.file_type = BINARY;
                return;
            }
            mask >>= 1;
        }
        self.file_type = ASCII;
        if self.dyn_ltree[9].fc != 0 || self.dyn_ltree[10].fc != 0 || self.dyn_ltree[13].fc != 0 {
            return;
        }
        for n in 32..LITERALS {
            if self.dyn_ltree[n].fc != 0 {
                return;
            }
        }
        self.file_type = BINARY;
    }

    /// `flush_block`: escolhe a melhor codificação do bloco (armazenado, estático ou dinâmico),
    /// escreve na saída em bits e devolve o tamanho comprimido do arquivo até aqui. `buf` é o bloco
    /// de entrada, ou `None` se já saiu da janela. `io.write` recebe os bytes prontos (e os cifra).
    /// `io.seekable` só é consultado onde o C chama `seekable()`, porque o `fseeko` dele despeja a
    /// saída e isso muda a ordem do que aparece num pipe.
    pub fn flush_block(&mut self, buf: Option<&[u8]>, stored_len: u64, eof: bool, io: &mut dyn DeflateIo) -> u64 {
        self.flag_buf[self.last_flags] = self.flags;
        if self.file_type == UNKNOWN {
            self.set_file_type();
        }
        let mut dl = std::mem::take(&mut self.dyn_ltree);
        let mut dd = std::mem::take(&mut self.dyn_dtree);
        let sl = std::mem::take(&mut self.static_ltree);
        let sd = std::mem::take(&mut self.static_dtree);
        self.l_max_code = build_tree(&mut self.hs, &mut dl, Some(&sl), L_CODES, &EXTRA_LBITS, LITERALS + 1, MAX_BITS as i32);
        self.d_max_code = build_tree(&mut self.hs, &mut dd, Some(&sd), D_CODES, &EXTRA_DBITS, 0, MAX_BITS as i32);
        self.dyn_ltree = dl;
        self.dyn_dtree = dd;
        self.static_ltree = sl;
        self.static_dtree = sd;
        let max_blindex = self.build_bl_tree();
        let mut opt_lenb = (self.hs.opt_len.wrapping_add(3 + 7)) >> 3;
        let static_lenb = (self.hs.static_len.wrapping_add(3 + 7)) >> 3;
        if static_lenb <= opt_lenb {
            opt_lenb = static_lenb;
        }
        let eof_bit = eof as i32;
        if stored_len <= opt_lenb && eof && self.cmpr_bytelen == 0 && self.cmpr_len_bits == 0 && io.seekable() && !io.use_descriptors() {
            // A compressão falhou no primeiro e último bloco: o arquivo inteiro vira armazenado.
            let block = buf.expect("block vanished");
            self.copy_block(block, stored_len as usize, false, &mut |d| io.write(d));
            self.cmpr_bytelen = stored_len;
            self.file_method = STORE;
        } else if stored_len + 4 <= opt_lenb && buf.is_some() {
            self.bits.send_bits((0 << 1) + eof_bit, 3);
            self.cmpr_bytelen += ((self.cmpr_len_bits + 3 + 7) >> 3) + stored_len + 4;
            self.cmpr_len_bits = 0;
            let block = buf.unwrap();
            self.copy_block(block, stored_len as usize, true, &mut |d| io.write(d));
        } else if static_lenb == opt_lenb {
            self.bits.send_bits((1 << 1) + eof_bit, 3);
            self.compress_block(false);
            self.cmpr_len_bits += 3 + self.hs.static_len;
            self.cmpr_bytelen += self.cmpr_len_bits >> 3;
            self.cmpr_len_bits &= 7;
        } else {
            self.bits.send_bits((2 << 1) + eof_bit, 3);
            let (a, b, c) = (self.l_max_code + 1, self.d_max_code + 1, max_blindex + 1);
            self.send_all_trees(a, b, c);
            self.compress_block(true);
            self.cmpr_len_bits += 3 + self.hs.opt_len;
            self.cmpr_bytelen += self.cmpr_len_bits >> 3;
            self.cmpr_len_bits &= 7;
        }
        self.init_block();
        if eof {
            self.bits.windup();
            self.cmpr_len_bits += 7;
        }
        if !self.bits.out.is_empty() {
            let out = std::mem::take(&mut self.bits.out);
            io.write(&out);
        }
        self.cmpr_bytelen + (self.cmpr_len_bits >> 3)
    }

    /// `copy_block`: bloco armazenado, com o cabeçalho de tamanho se pedido.
    fn copy_block(&mut self, block: &[u8], len: usize, header: bool, write: &mut dyn FnMut(&[u8])) {
        self.bits.windup();
        if header {
            let l = len as u32;
            self.bits.put_short(l & 0xffff);
            self.bits.put_short(!l & 0xffff);
        }
        let out = std::mem::take(&mut self.bits.out);
        if !out.is_empty() {
            write(&out);
        }
        write(&block[..len]);
    }
}
