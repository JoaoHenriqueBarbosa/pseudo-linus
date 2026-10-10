//! `enc/backward_references_enc.c` e `enc/backward_references_cost_enc.c` do libwebp 1.5.0: a cadeia de hash, as
//! referências para trás (LZ77 padrão, RLE e caixa), o cache de cores e o caminho de menor custo (`TraceBackwards`).

use super::dsp::{fast_log2, div_round, prefix_encode, LOG_2_PRECISION_BITS};
use super::histogram::{num_codes, Histogram, NUM_DISTANCE_CODES, NUM_LENGTH_CODES, NUM_LITERAL_CODES};

const HASH_BITS: u32 = 18;
const HASH_SIZE: usize = 1 << HASH_BITS;
const MAX_LENGTH_BITS: u32 = 12;
const WINDOW_SIZE_BITS: u32 = 20;
pub const MAX_LENGTH: usize = (1 << MAX_LENGTH_BITS) - 1;
const WINDOW_SIZE: usize = (1 << WINDOW_SIZE_BITS) - 120;
const MIN_LENGTH: usize = 4;
pub const MAX_COLOR_CACHE_BITS: u32 = 10;
pub const LZ77_STANDARD: u32 = 1;
pub const LZ77_RLE: u32 = 2;
pub const LZ77_BOX: u32 = 4;

/// `PixOrCopy`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixOrCopy {
    Literal(u32),
    CacheIdx(u32),
    Copy { len: u32, distance: u32 },
}

impl PixOrCopy {
    pub fn len(&self) -> usize {
        match *self {
            PixOrCopy::Copy { len, .. } => len as usize,
            _ => 1,
        }
    }
}

const PLANE_TO_CODE_LUT: [u8; 128] = [
    96, 73, 55, 39, 23, 13, 5, 1, 255, 255, 255, 255, 255, 255, 255, 255, 101, 78, 58, 42, 26, 16, 8, 2, 0, 3, 9, 17, 27, 43, 59, 79, 102, 86, 62, 46, 32,
    20, 10, 6, 4, 7, 11, 21, 33, 47, 63, 87, 105, 90, 70, 52, 37, 28, 18, 14, 12, 15, 19, 29, 38, 53, 71, 91, 110, 99, 82, 66, 48, 35, 30, 24, 22, 25,
    31, 36, 49, 67, 83, 100, 115, 108, 94, 76, 64, 50, 44, 40, 34, 41, 45, 51, 65, 77, 95, 109, 118, 113, 103, 92, 80, 68, 60, 56, 54, 57, 61, 69, 81,
    93, 104, 114, 119, 116, 111, 106, 97, 88, 84, 74, 72, 75, 85, 89, 98, 107, 112, 117,
];

/// `VP8LDistanceToPlaneCode`.
pub fn distance_to_plane_code(xsize: usize, dist: u32) -> u32 {
    let dist = dist as usize;
    let yoffset = dist / xsize;
    let xoffset = dist - yoffset * xsize;
    if xoffset <= 8 && yoffset < 8 {
        u32::from(PLANE_TO_CODE_LUT[yoffset * 16 + 8 - xoffset]) + 1
    } else if xoffset + 8 > xsize && yoffset < 7 {
        u32::from(PLANE_TO_CODE_LUT[(yoffset + 1) * 16 + 8 + (xsize - xoffset)]) + 1
    } else {
        (dist + 120) as u32
    }
}

/// `VP8LVectorMismatch`.
fn vector_mismatch(a: &[u32], b: &[u32], length: usize) -> usize {
    a.iter().zip(b).take(length).take_while(|(x, y)| x == y).count()
}

/// `FindMatchLength` com as duas sequências no mesmo vetor.
fn find_match_length(argb: &[u32], a: usize, b: usize, best_len: usize, max_limit: usize) -> usize {
    if argb[a + best_len] != argb[b + best_len] {
        return 0;
    }
    vector_mismatch(&argb[a..], &argb[b..], max_limit)
}

/// `VP8LHashPix`.
pub fn hash_pix(argb: u32, shift: u32) -> usize {
    (argb.wrapping_mul(0x1e35a7bd) >> shift) as usize
}

/// `VP8LColorCache`.
pub struct ColorCache {
    colors: Vec<u32>,
    shift: u32,
}

impl ColorCache {
    pub fn new(bits: u32) -> Self {
        ColorCache { colors: vec![0; 1 << bits], shift: 32 - bits }
    }

    pub fn index(&self, argb: u32) -> usize {
        hash_pix(argb, self.shift)
    }

    pub fn insert(&mut self, argb: u32) {
        let key = self.index(argb);
        self.colors[key] = argb;
    }

    /// `VP8LColorCacheContains`.
    pub fn contains(&self, argb: u32) -> Option<usize> {
        let key = self.index(argb);
        (self.colors[key] == argb).then_some(key)
    }
}

/// `VP8LHashChain`: para cada posição, `(distância << 12) | comprimento` da melhor cópia.
pub struct HashChain {
    offset_length: Vec<u32>,
}

impl HashChain {
    pub fn offset(&self, pos: usize) -> usize {
        (self.offset_length[pos] >> MAX_LENGTH_BITS) as usize
    }

    pub fn length(&self, pos: usize) -> usize {
        (self.offset_length[pos] & ((1 << MAX_LENGTH_BITS) - 1)) as usize
    }

    /// `VP8LHashChainFill`.
    pub fn fill(quality: i32, argb: &[u32], xsize: usize, ysize: usize, low_effort: bool) -> Self {
        let size = xsize * ysize;
        let mut out = vec![0u32; size];
        if size <= 2 {
            return HashChain { offset_length: out };
        }
        let iter_max = 8 + (quality * quality) / 128;
        let max_window = if quality > 75 {
            WINDOW_SIZE
        } else if quality > 50 {
            xsize << 8
        } else if quality > 25 {
            xsize << 6
        } else {
            xsize << 4
        };
        let window_size = max_window.min(WINDOW_SIZE);
        let pair_hash = |a: u32, b: u32| (b.wrapping_mul(0xc6a4a793).wrapping_add(a.wrapping_mul(0x5bd1e996)) >> (32 - HASH_BITS)) as usize;
        let mut hash_to_first = vec![-1i32; HASH_SIZE];
        let mut chain = vec![-1i32; size];
        let mut argb_comp = argb[0] == argb[1];
        let mut pos = 0usize;
        while pos < size - 2 {
            let argb_comp_next = argb[pos + 1] == argb[pos + 2];
            if argb_comp && argb_comp_next {
                let first = argb[pos];
                let mut len = 1usize;
                while pos + len + 2 < size && argb[pos + len + 2] == argb[pos] {
                    len += 1;
                }
                if len > MAX_LENGTH {
                    chain[pos..pos + len - MAX_LENGTH].fill(-1);
                    pos += len - MAX_LENGTH;
                    len = MAX_LENGTH;
                }
                while len > 0 {
                    let h = pair_hash(first, len as u32);
                    len -= 1;
                    chain[pos] = hash_to_first[h];
                    hash_to_first[h] = pos as i32;
                    pos += 1;
                }
                argb_comp = false;
            } else {
                let h = pair_hash(argb[pos], argb[pos + 1]);
                chain[pos] = hash_to_first[h];
                hash_to_first[h] = pos as i32;
                pos += 1;
                argb_comp = argb_comp_next;
            }
        }
        chain[pos] = hash_to_first[pair_hash(argb[pos], argb[pos + 1])];

        let mut base_position = size - 2;
        while base_position > 0 {
            let max_len = (size - 1 - base_position).min(MAX_LENGTH);
            let mut iter = iter_max;
            let mut best_length = 0usize;
            let mut best_distance = 0usize;
            let min_pos = if base_position > window_size { (base_position - window_size) as i64 } else { 0 };
            let length_max = max_len.min(256);
            let mut p = i64::from(chain[base_position]);
            if !low_effort {
                if base_position >= xsize {
                    let curr = find_match_length(argb, base_position - xsize, base_position, best_length, max_len);
                    if curr > best_length {
                        best_length = curr;
                        best_distance = xsize;
                    }
                    iter -= 1;
                }
                let curr = find_match_length(argb, base_position - 1, base_position, best_length, max_len);
                if curr > best_length {
                    best_length = curr;
                    best_distance = 1;
                }
                iter -= 1;
                if best_length == MAX_LENGTH {
                    p = min_pos - 1;
                }
            }
            let mut best_argb = argb[base_position + best_length];
            while p >= min_pos && {
                iter -= 1;
                iter != 0
            } {
                let pu = p as usize;
                if argb[pu + best_length] == best_argb {
                    let curr = vector_mismatch(&argb[pu..], &argb[base_position..], max_len);
                    if best_length < curr {
                        best_length = curr;
                        best_distance = base_position - pu;
                        best_argb = argb[base_position + best_length];
                        if best_length >= length_max {
                            break;
                        }
                    }
                }
                p = i64::from(chain[pu]);
            }
            let mut max_base_position = base_position;
            loop {
                out[base_position] = ((best_distance as u32) << MAX_LENGTH_BITS) | best_length as u32;
                base_position -= 1;
                if best_distance == 0 || base_position == 0 {
                    break;
                }
                if base_position < best_distance || argb[base_position - best_distance] != argb[base_position] {
                    break;
                }
                if best_length == MAX_LENGTH && best_distance != 1 && base_position + MAX_LENGTH < max_base_position {
                    break;
                }
                if best_length < MAX_LENGTH {
                    best_length += 1;
                    max_base_position = base_position;
                }
            }
        }
        out[0] = 0;
        out[size - 1] = 0;
        HashChain { offset_length: out }
    }
}

/// `BackwardReferencesRle` sem cache de cores (o único uso no codificador).
fn backward_refs_rle(xsize: usize, ysize: usize, argb: &[u32]) -> Vec<PixOrCopy> {
    let pix_count = xsize * ysize;
    let mut refs = vec![PixOrCopy::Literal(argb[0])];
    let mut i = 1;
    while i < pix_count {
        let max_len = (pix_count - i).min(MAX_LENGTH);
        let rle_len = find_match_length(argb, i, i - 1, 0, max_len);
        let prev_row_len = if i < xsize { 0 } else { find_match_length(argb, i, i - xsize, 0, max_len) };
        if rle_len >= prev_row_len && rle_len >= MIN_LENGTH {
            refs.push(PixOrCopy::Copy { len: rle_len as u32, distance: 1 });
            i += rle_len;
        } else if prev_row_len >= MIN_LENGTH {
            refs.push(PixOrCopy::Copy { len: prev_row_len as u32, distance: xsize as u32 });
            i += prev_row_len;
        } else {
            refs.push(PixOrCopy::Literal(argb[i]));
            i += 1;
        }
    }
    refs
}

/// `BackwardReferencesLz77` sem cache de cores.
fn backward_refs_lz77(xsize: usize, ysize: usize, argb: &[u32], chain: &HashChain) -> Vec<PixOrCopy> {
    let pix_count = xsize * ysize;
    let mut refs = Vec::new();
    let mut i_last_check: i64 = -1;
    let mut i = 0;
    while i < pix_count {
        let offset = chain.offset(i);
        let mut len = chain.length(i);
        if len >= MIN_LENGTH {
            let len_ini = len;
            let mut max_reach = 0;
            let j_max = if i + len_ini >= pix_count { pix_count - 1 } else { i + len_ini };
            i_last_check = i_last_check.max(i as i64);
            for j in (i_last_check + 1) as usize..=j_max {
                let len_j = chain.length(j);
                let reach = j + if len_j >= MIN_LENGTH { len_j } else { 1 };
                if reach > max_reach {
                    len = j - i;
                    max_reach = reach;
                    if max_reach >= pix_count {
                        break;
                    }
                }
            }
        } else {
            len = 1;
        }
        if len == 1 {
            refs.push(PixOrCopy::Literal(argb[i]));
        } else {
            refs.push(PixOrCopy::Copy { len: len as u32, distance: offset as u32 });
        }
        i += len;
    }
    refs
}

/// `BackwardReferencesLz77Box`: devolve a cadeia da caixa (usada depois pelo `TraceBackwards`) e as referências.
fn backward_refs_lz77_box(xsize: usize, ysize: usize, argb: &[u32], best: &HashChain) -> (HashChain, Vec<PixOrCopy>) {
    const WINDOW_OFFSETS_SIZE_MAX: usize = 32;
    let pix_count = xsize * ysize;
    let mut counts = vec![0u16; pix_count];
    counts[pix_count - 1] = 1;
    for i in (0..pix_count - 1).rev() {
        counts[i] = if argb[i] == argb[i + 1] { counts[i + 1] + u16::from(usize::from(counts[i + 1]) != MAX_LENGTH) } else { 1 };
    }
    let mut window_offsets = [0usize; WINDOW_OFFSETS_SIZE_MAX];
    for y in 0..=6i64 {
        for x in -6..=6i64 {
            let offset = y * xsize as i64 + x;
            if offset <= 0 {
                continue;
            }
            let plane_code = distance_to_plane_code(xsize, offset as u32) as usize - 1;
            if plane_code < WINDOW_OFFSETS_SIZE_MAX {
                window_offsets[plane_code] = offset as usize;
            }
        }
    }
    let offsets: Vec<usize> = window_offsets.iter().copied().filter(|&o| o != 0).collect();
    let offsets_new: Vec<usize> = offsets.iter().copied().filter(|&o| !offsets.iter().any(|&p| o == p + 1)).collect();
    let mut out = vec![0u32; pix_count];
    let (mut best_offset_prev, mut best_length_prev) = (0usize, -1i64);
    for i in 1..pix_count {
        let mut best_length = best.length(i);
        let mut best_offset = 0;
        let mut do_compute = true;
        if best_length >= MAX_LENGTH {
            best_offset = best.offset(i);
            if offsets.contains(&best_offset) {
                do_compute = false;
            }
        }
        if do_compute {
            let use_prev = best_length_prev > 1 && best_length_prev < MAX_LENGTH as i64;
            let list = if use_prev { &offsets_new } else { &offsets };
            best_length = if use_prev { (best_length_prev - 1) as usize } else { 0 };
            best_offset = if use_prev { best_offset_prev } else { 0 };
            for &off in list {
                if off > i || argb[i - off] != argb[i] {
                    continue;
                }
                let (mut j, mut j_offset, mut curr_length) = (i, i - off, 0usize);
                loop {
                    let cjo = usize::from(counts[j_offset]);
                    let cj = usize::from(counts[j]);
                    if cjo != cj {
                        curr_length += cjo.min(cj);
                        break;
                    }
                    curr_length += cjo;
                    j_offset += cjo;
                    j += cjo;
                    if !(curr_length <= MAX_LENGTH && j < pix_count && argb[j_offset] == argb[j]) {
                        break;
                    }
                }
                if best_length < curr_length {
                    best_offset = off;
                    if curr_length >= MAX_LENGTH {
                        best_length = MAX_LENGTH;
                        break;
                    }
                    best_length = curr_length;
                }
            }
        }
        if best_length <= MIN_LENGTH {
            out[i] = 0;
            best_offset_prev = 0;
            best_length_prev = 0;
        } else {
            out[i] = ((best_offset as u32) << MAX_LENGTH_BITS) | best_length as u32;
            best_offset_prev = best_offset;
            best_length_prev = best_length as i64;
        }
    }
    out[0] = 0;
    let chain = HashChain { offset_length: out };
    let refs = backward_refs_lz77(xsize, ysize, argb, &chain);
    (chain, refs)
}

/// `BackwardReferences2DLocality`.
fn to_2d_locality(xsize: usize, refs: &mut [PixOrCopy]) {
    for v in refs {
        if let PixOrCopy::Copy { distance, .. } = v {
            *distance = distance_to_plane_code(xsize, *distance);
        }
    }
}

/// `CalculateBestCacheSize`.
fn best_cache_size(argb: &[u32], quality: i32, refs: &[PixOrCopy], cache_bits_max: u32) -> u32 {
    let cache_bits_max = if quality <= 25 { 0 } else { cache_bits_max };
    if cache_bits_max == 0 {
        return 0;
    }
    let mut histos: Vec<Histogram> = (0..=cache_bits_max).map(Histogram::new).collect();
    let mut caches: Vec<Vec<u32>> = (0..=cache_bits_max).map(|i| vec![0u32; 1 << i]).collect();
    let mut p = 0usize;
    for v in refs {
        if let PixOrCopy::Literal(_) = v {
            let pix = argb[p];
            p += 1;
            let (a, r, g, b) = ((pix >> 24) as usize, ((pix >> 16) & 0xff) as usize, ((pix >> 8) & 0xff) as usize, (pix & 0xff) as usize);
            let mut key = hash_pix(pix, 32 - cache_bits_max);
            let count = |h: &mut Histogram| {
                h.blue[b] += 1;
                h.literal[g] += 1;
                h.red[r] += 1;
                h.alpha[a] += 1;
            };
            count(&mut histos[0]);
            for i in (1..=cache_bits_max as usize).rev() {
                if caches[i][key] == pix {
                    histos[i].literal[NUM_LITERAL_CODES + NUM_LENGTH_CODES + key] += 1;
                } else {
                    caches[i][key] = pix;
                    count(&mut histos[i]);
                }
                key >>= 1;
            }
        } else {
            let mut len = v.len();
            let mut argb_prev = argb[p] ^ 0xffff_ffff;
            let code = prefix_encode(len as u32).0 as usize;
            for h in &mut histos {
                h.literal[NUM_LITERAL_CODES + code] += 1;
            }
            loop {
                if argb[p] != argb_prev {
                    let mut key = hash_pix(argb[p], 32 - cache_bits_max);
                    for i in (1..=cache_bits_max as usize).rev() {
                        caches[i][key] = argb[p];
                        key >>= 1;
                    }
                    argb_prev = argb[p];
                }
                p += 1;
                len -= 1;
                if len == 0 {
                    break;
                }
            }
        }
    }
    let mut best = (0u32, u64::MAX);
    for (i, h) in histos.iter_mut().enumerate() {
        let entropy = h.estimate_bits();
        if i == 0 || entropy < best.1 {
            best = (i as u32, entropy);
        }
    }
    best.0
}

/// `BackwardRefsWithLocalCache`.
fn with_local_cache(argb: &[u32], cache_bits: u32, refs: &mut [PixOrCopy]) {
    let mut cache = ColorCache::new(cache_bits);
    let mut pixel_index = 0;
    for v in refs {
        match *v {
            PixOrCopy::Literal(lit) => {
                if let Some(ix) = cache.contains(lit) {
                    *v = PixOrCopy::CacheIdx(ix as u32);
                } else {
                    cache.insert(lit);
                }
                pixel_index += 1;
            }
            _ => {
                for _ in 0..v.len() {
                    cache.insert(argb[pixel_index]);
                    pixel_index += 1;
                }
            }
        }
    }
}

fn estimate(refs: &[PixOrCopy], cache_bits: u32) -> u64 {
    let mut h = Histogram::new(cache_bits);
    h.store_refs(refs);
    h.estimate_bits()
}

/// `VP8LGetBackwardReferences` sem a variante sem cache (`do_no_cache` é zero nos modos que o Skia alcança):
/// as referências e os bits do cache de cores.
pub fn get_backward_references(
    width: usize,
    height: usize,
    argb: &[u32],
    quality: i32,
    low_effort: bool,
    lz77_types: u32,
    cache_bits_max: u32,
    chain: &HashChain,
) -> (Vec<PixOrCopy>, u32) {
    if low_effort {
        let mut refs = backward_refs_lz77(width, height, argb, chain);
        to_2d_locality(width, &mut refs);
        return (refs, 0);
    }
    let mut best: Option<(Vec<PixOrCopy>, u64, u32, u32)> = None;
    let mut box_chain: Option<HashChain> = None;
    for lz77_type in [LZ77_STANDARD, LZ77_RLE, LZ77_BOX] {
        if lz77_types & lz77_type == 0 {
            continue;
        }
        let mut refs = match lz77_type {
            LZ77_RLE => backward_refs_rle(width, height, argb),
            LZ77_STANDARD => backward_refs_lz77(width, height, argb, chain),
            _ => {
                let (c, refs) = backward_refs_lz77_box(width, height, argb, chain);
                box_chain = Some(c);
                refs
            }
        };
        let cache_bits = best_cache_size(argb, quality, &refs, cache_bits_max);
        if cache_bits > 0 {
            with_local_cache(argb, cache_bits, &mut refs);
        }
        let bit_cost = estimate(&refs, cache_bits);
        if best.as_ref().is_none_or(|b| bit_cost < b.1) {
            best = Some((refs, bit_cost, lz77_type, cache_bits));
        }
    }
    let Some((mut refs, bit_cost, lz77_type, cache_bits)) = best else { return (Vec::new(), 0) };
    if (lz77_type == LZ77_STANDARD || lz77_type == LZ77_BOX) && quality >= 25 {
        let chain_tmp = if lz77_type == LZ77_STANDARD { chain } else { box_chain.as_ref().unwrap_or(chain) };
        let traced = trace_backwards(width, height, argb, cache_bits, chain_tmp, &refs);
        if estimate(&traced, cache_bits) < bit_cost {
            refs = traced;
        }
    }
    to_2d_locality(width, &mut refs);
    (refs, cache_bits)
}

// ---------------------------------------------------------------------------------------------------------------
// `backward_references_cost_enc.c`.

/// `CostModel`.
struct CostModel {
    alpha: [u32; 256],
    red: [u32; 256],
    blue: [u32; 256],
    distance: [u32; NUM_DISTANCE_CODES],
    literal: Vec<u32>,
}

/// `ConvertPopulationCountTableToBitEstimates`.
fn bit_estimates(counts: &[u32], out: &mut [u32]) {
    let sum = counts.iter().fold(0u32, |s, &c| s.wrapping_add(c));
    let nonzeros = counts.iter().filter(|&&c| c > 0).count();
    if nonzeros <= 1 {
        out.fill(0);
    } else {
        let logsum = fast_log2(sum);
        for (o, &c) in out.iter_mut().zip(counts) {
            *o = logsum.wrapping_sub(fast_log2(c));
        }
    }
}

impl CostModel {
    /// `CostModelBuild`.
    fn build(xsize: usize, cache_bits: u32, refs: &[PixOrCopy]) -> Self {
        let mut h = Histogram::new(cache_bits);
        for v in refs {
            h.add(v, Some(xsize));
        }
        let mut m = CostModel { alpha: [0; 256], red: [0; 256], blue: [0; 256], distance: [0; NUM_DISTANCE_CODES], literal: vec![0; num_codes(cache_bits)] };
        bit_estimates(&h.literal, &mut m.literal);
        bit_estimates(&h.red, &mut m.red);
        bit_estimates(&h.blue, &mut m.blue);
        bit_estimates(&h.alpha, &mut m.alpha);
        bit_estimates(&h.distance, &mut m.distance);
        m
    }

    fn literal_cost(&self, v: u32) -> i64 {
        i64::from(self.alpha[(v >> 24) as usize]) + i64::from(self.red[((v >> 16) & 0xff) as usize]) + i64::from(self.literal[((v >> 8) & 0xff) as usize])
            + i64::from(self.blue[(v & 0xff) as usize])
    }

    fn cache_cost(&self, idx: usize) -> i64 {
        i64::from(self.literal[NUM_LITERAL_CODES + NUM_LENGTH_CODES + idx])
    }

    fn length_cost(&self, length: u32) -> i64 {
        let (code, extra_bits, _) = prefix_encode(length);
        i64::from(self.literal[NUM_LITERAL_CODES + code as usize]) + (i64::from(extra_bits) << LOG_2_PRECISION_BITS)
    }

    fn distance_cost(&self, distance: u32) -> i64 {
        let (code, extra_bits, _) = prefix_encode(distance);
        i64::from(self.distance[code as usize]) + (i64::from(extra_bits) << LOG_2_PRECISION_BITS)
    }
}

const COST_CACHE_INTERVAL_SIZE_MAX: usize = 500;

/// `CostInterval`, numa arena com ligações por índice.
#[derive(Clone, Copy)]
struct Interval {
    cost: i64,
    start: usize,
    end: usize,
    index: usize,
    previous: Option<usize>,
    next: Option<usize>,
}

/// `CostCacheInterval`.
struct CacheInterval {
    cost: i64,
    start: usize,
    end: usize,
}

/// `CostManager`.
struct CostManager {
    head: Option<usize>,
    count: usize,
    cache_intervals: Vec<CacheInterval>,
    cost_cache: Vec<i64>,
    costs: Vec<i64>,
    dist_array: Vec<u16>,
    arena: Vec<Interval>,
    free: Vec<usize>,
}

impl CostManager {
    /// `CostManagerInit`.
    fn new(pix_count: usize, model: &CostModel) -> Self {
        let cost_cache: Vec<i64> = (0..pix_count.min(MAX_LENGTH)).map(|i| model.length_cost(i as u32)).collect();
        let mut cache_intervals = vec![CacheInterval { cost: cost_cache[0], start: 0, end: 1 }];
        for (i, &c) in cost_cache.iter().enumerate().skip(1) {
            if let Some(cur) = cache_intervals.last_mut() {
                if c != cur.cost {
                    cache_intervals.push(CacheInterval { cost: c, start: i, end: i + 1 });
                } else {
                    cur.end = i + 1;
                }
            }
        }
        CostManager {
            head: None,
            count: 0,
            cache_intervals,
            cost_cache,
            costs: vec![i64::MAX; pix_count],
            dist_array: vec![0; pix_count],
            arena: Vec::new(),
            free: Vec::new(),
        }
    }

    /// `UpdateCost`.
    fn update_cost(&mut self, i: usize, position: usize, cost: i64) {
        let k = i - position;
        if self.costs[i] > cost {
            self.costs[i] = cost;
            self.dist_array[i] = (k + 1) as u16;
        }
    }

    fn update_cost_per_interval(&mut self, start: usize, end: usize, position: usize, cost: i64) {
        for i in start..end {
            self.update_cost(i, position, cost);
        }
    }

    /// `ConnectIntervals`.
    fn connect(&mut self, prev: Option<usize>, next: Option<usize>) {
        match prev {
            Some(p) => self.arena[p].next = next,
            None => self.head = next,
        }
        if let Some(n) = next {
            self.arena[n].previous = prev;
        }
    }

    /// `PopInterval`.
    fn pop(&mut self, interval: usize) {
        let Interval { previous, next, .. } = self.arena[interval];
        self.connect(previous, next);
        self.free.push(interval);
        self.count -= 1;
    }

    /// `UpdateCostAtIndex`.
    fn update_cost_at_index(&mut self, i: usize, do_clean: bool) {
        let mut current = self.head;
        while let Some(c) = current {
            let iv = self.arena[c];
            if iv.start > i {
                break;
            }
            if iv.end <= i {
                if do_clean {
                    self.pop(c);
                }
            } else {
                self.update_cost(i, iv.index, iv.cost);
            }
            current = iv.next;
        }
    }

    /// `PositionOrphanInterval`.
    fn position_orphan(&mut self, current: usize, previous: Option<usize>) {
        let start = self.arena[current].start;
        let mut previous = previous.or(self.head);
        while let Some(p) = previous {
            if start < self.arena[p].start {
                previous = self.arena[p].previous;
            } else {
                break;
            }
        }
        while let Some(p) = previous {
            match self.arena[p].next {
                Some(n) if self.arena[n].start < start => previous = Some(n),
                _ => break,
            }
        }
        let after = match previous {
            Some(p) => self.arena[p].next,
            None => self.head,
        };
        self.connect(Some(current), after);
        self.connect(previous, Some(current));
    }

    /// `InsertInterval`.
    fn insert(&mut self, interval_in: Option<usize>, cost: i64, position: usize, start: usize, end: usize) {
        if start >= end {
            return;
        }
        if self.count >= COST_CACHE_INTERVAL_SIZE_MAX {
            self.update_cost_per_interval(start, end, position, cost);
            return;
        }
        let iv = Interval { cost, start, end, index: position, previous: None, next: None };
        let id = match self.free.pop() {
            Some(id) => {
                self.arena[id] = iv;
                id
            }
            None => {
                self.arena.push(iv);
                self.arena.len() - 1
            }
        };
        self.position_orphan(id, interval_in);
        self.count += 1;
    }

    /// `PushInterval`.
    fn push(&mut self, distance_cost: i64, position: usize, len: usize) {
        const SKIP_DISTANCE: usize = 10;
        if len < SKIP_DISTANCE {
            for j in position..position + len {
                let k = j - position;
                let cost_tmp = distance_cost + self.cost_cache[k];
                if self.costs[j] > cost_tmp {
                    self.costs[j] = cost_tmp;
                    self.dist_array[j] = (k + 1) as u16;
                }
            }
            return;
        }
        let mut interval = self.head;
        let mut i = 0;
        while i < self.cache_intervals.len() && self.cache_intervals[i].start < len {
            let ci = &self.cache_intervals[i];
            let mut start = position + ci.start;
            let end = position + ci.end.min(len);
            let cost = distance_cost + ci.cost;
            while let Some(cur) = interval {
                if self.arena[cur].start >= end {
                    break;
                }
                let next = self.arena[cur].next;
                let iv = self.arena[cur];
                if start >= iv.end {
                    interval = next;
                    continue;
                }
                if cost >= iv.cost {
                    let start_new = iv.end;
                    self.insert(Some(cur), cost, position, start, iv.start);
                    start = start_new;
                    if start >= end {
                        break;
                    }
                    interval = next;
                    continue;
                }
                if start <= iv.start {
                    if iv.end <= end {
                        self.pop(cur);
                    } else {
                        self.arena[cur].start = end;
                        break;
                    }
                } else if end < iv.end {
                    let end_original = iv.end;
                    self.arena[cur].end = start;
                    self.insert(Some(cur), iv.cost, iv.index, end, end_original);
                    interval = self.arena[cur].next;
                    break;
                } else {
                    self.arena[cur].end = start;
                }
                interval = next;
            }
            self.insert(interval, cost, position, start, end);
            i += 1;
        }
    }
}

/// `AddSingleLiteralWithCostModel`.
fn add_literal_with_cost(argb: &[u32], cache: &mut Option<ColorCache>, model: &CostModel, idx: usize, prev_cost: i64, m: &mut CostManager) {
    let color = argb[idx];
    let ix = cache.as_ref().and_then(|c| c.contains(color));
    let cost_val = match ix {
        Some(ix) => prev_cost + div_round(model.cache_cost(ix) * 68, 100),
        None => {
            if let Some(c) = cache.as_mut() {
                c.insert(color);
            }
            prev_cost + div_round(model.literal_cost(color) * 82, 100)
        }
    };
    if m.costs[idx] > cost_val {
        m.costs[idx] = cost_val;
        m.dist_array[idx] = 1;
    }
}

/// `BackwardReferencesHashChainDistanceOnly`: o comprimento do último passo do melhor caminho em cada posição.
fn distance_only(xsize: usize, ysize: usize, argb: &[u32], cache_bits: u32, chain: &HashChain, refs: &[PixOrCopy]) -> Vec<u16> {
    let pix_count = xsize * ysize;
    let mut cache = (cache_bits > 0).then(|| ColorCache::new(cache_bits));
    let model = CostModel::build(xsize, cache_bits, refs);
    let mut m = CostManager::new(pix_count, &model);
    let (mut offset_prev, mut len_prev): (i64, i64) = (-1, -1);
    let mut offset_cost: i64 = -1;
    let mut first_offset_is_constant = -1;
    let mut reach = 0usize;
    m.dist_array[0] = 0;
    add_literal_with_cost(argb, &mut cache, &model, 0, 0, &mut m);
    for i in 1..pix_count {
        let prev_cost = m.costs[i - 1];
        let (offset, len) = (chain.offset(i), chain.length(i));
        add_literal_with_cost(argb, &mut cache, &model, i, prev_cost, &mut m);
        if len >= 2 {
            if offset as i64 != offset_prev {
                let code = distance_to_plane_code(xsize, offset as u32);
                offset_cost = model.distance_cost(code);
                first_offset_is_constant = 1;
                m.push(prev_cost + offset_cost, i, len);
            } else {
                // Como no C, o -1 inicial também conta como verdadeiro.
                if first_offset_is_constant != 0 {
                    reach = (i as i64 - 1 + len_prev - 1) as usize;
                    first_offset_is_constant = 0;
                }
                if i + len - 1 > reach {
                    let (mut offset_j, mut len_j) = (0usize, 0usize);
                    let mut j = i;
                    while j <= reach {
                        offset_j = chain.offset(j + 1);
                        len_j = chain.length(j + 1);
                        if offset_j != offset {
                            offset_j = chain.offset(j);
                            len_j = chain.length(j);
                            break;
                        }
                        j += 1;
                    }
                    let _ = offset_j;
                    m.update_cost_at_index(j - 1, false);
                    m.update_cost_at_index(j, false);
                    m.push(m.costs[j - 1] + offset_cost, j, len_j);
                    reach = j + len_j - 1;
                }
            }
        }
        m.update_cost_at_index(i, true);
        offset_prev = offset as i64;
        len_prev = len as i64;
    }
    m.dist_array
}

/// `VP8LBackwardReferencesTraceBackwards`.
fn trace_backwards(xsize: usize, ysize: usize, argb: &[u32], cache_bits: u32, chain: &HashChain, refs_src: &[PixOrCopy]) -> Vec<PixOrCopy> {
    let dist_array = distance_only(xsize, ysize, argb, cache_bits, chain, refs_src);
    // `TraceBackwards`.
    let mut path = Vec::new();
    let mut cur = dist_array.len() as i64 - 1;
    while cur >= 0 {
        let k = dist_array[cur as usize];
        path.push(k);
        cur -= i64::from(k);
    }
    path.reverse();
    // `BackwardReferencesHashChainFollowChosenPath`.
    let mut cache = (cache_bits > 0).then(|| ColorCache::new(cache_bits));
    let mut refs = Vec::with_capacity(path.len());
    let mut i = 0;
    for &len in &path {
        let len = usize::from(len);
        if len != 1 {
            refs.push(PixOrCopy::Copy { len: len as u32, distance: chain.offset(i) as u32 });
            if let Some(c) = cache.as_mut() {
                for k in 0..len {
                    c.insert(argb[i + k]);
                }
            }
            i += len;
        } else {
            let v = match cache.as_mut() {
                Some(c) => match c.contains(argb[i]) {
                    Some(idx) => PixOrCopy::CacheIdx(idx as u32),
                    None => {
                        c.insert(argb[i]);
                        PixOrCopy::Literal(argb[i])
                    }
                },
                None => PixOrCopy::Literal(argb[i]),
            };
            refs.push(v);
            i += 1;
        }
    }
    refs
}
