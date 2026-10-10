//! `enc/histogram_enc.c` do libwebp 1.5.0: os histogramas do VP8L, as estimativas de custo e o agrupamento das
//! regiões em códigos de Huffman comuns (por faixas de entropia, estocástico e guloso).

use super::backward_refs::{distance_to_plane_code, PixOrCopy};
use super::dsp::{
    bits_entropy_unrefined, div_round, entropy_unrefined, extra_cost, prefix_encode, sub_sample_size, BitEntropy, Streaks, LOG_2_PRECISION_BITS,
    NON_TRIVIAL_SYM,
};

pub const NUM_LITERAL_CODES: usize = 256;
pub const NUM_LENGTH_CODES: usize = 24;
pub const NUM_DISTANCE_CODES: usize = 40;
const CODE_LENGTH_CODES: u64 = 19;
const NUM_PARTITIONS: usize = 4;
const BIN_SIZE: usize = NUM_PARTITIONS * NUM_PARTITIONS * NUM_PARTITIONS;
const MAX_HISTO_GREEDY: i64 = 100;

/// `VP8LHistogramNumCodes`.
pub fn num_codes(palette_code_bits: u32) -> usize {
    NUM_LITERAL_CODES + NUM_LENGTH_CODES + if palette_code_bits > 0 { 1 << palette_code_bits } else { 0 }
}

/// `VP8LHistogram`.
#[derive(Clone)]
pub struct Histogram {
    pub literal: Vec<u32>,
    pub red: [u32; NUM_LITERAL_CODES],
    pub blue: [u32; NUM_LITERAL_CODES],
    pub alpha: [u32; NUM_LITERAL_CODES],
    pub distance: [u32; NUM_DISTANCE_CODES],
    pub palette_code_bits: u32,
    trivial_symbol: u32,
    bit_cost: u64,
    literal_cost: u64,
    red_cost: u64,
    blue_cost: u64,
    is_used: [bool; 5],
}

impl Histogram {
    /// Um histograma zerado (`HistogramClear`).
    pub fn new(palette_code_bits: u32) -> Self {
        Histogram {
            literal: vec![0; num_codes(palette_code_bits)],
            red: [0; NUM_LITERAL_CODES],
            blue: [0; NUM_LITERAL_CODES],
            alpha: [0; NUM_LITERAL_CODES],
            distance: [0; NUM_DISTANCE_CODES],
            palette_code_bits,
            trivial_symbol: 0,
            bit_cost: 0,
            literal_cost: 0,
            red_cost: 0,
            blue_cost: 0,
            is_used: [false; 5],
        }
    }

    /// `VP8LHistogramAddSinglePixOrCopy`; com `plane_xsize` a distância passa pelo `DistanceToPlaneCode`.
    pub fn add(&mut self, v: &PixOrCopy, plane_xsize: Option<usize>) {
        match *v {
            PixOrCopy::Literal(argb) => {
                self.alpha[(argb >> 24) as usize] += 1;
                self.red[((argb >> 16) & 0xff) as usize] += 1;
                self.literal[((argb >> 8) & 0xff) as usize] += 1;
                self.blue[(argb & 0xff) as usize] += 1;
            }
            PixOrCopy::CacheIdx(idx) => self.literal[NUM_LITERAL_CODES + NUM_LENGTH_CODES + idx as usize] += 1,
            PixOrCopy::Copy { len, distance } => {
                self.literal[NUM_LITERAL_CODES + prefix_encode(len).0 as usize] += 1;
                let d = plane_xsize.map_or(distance, |xsize| distance_to_plane_code(xsize, distance));
                self.distance[prefix_encode(d).0 as usize] += 1;
            }
        }
    }

    /// `VP8LHistogramStoreRefs`.
    pub fn store_refs(&mut self, refs: &[PixOrCopy]) {
        for v in refs {
            self.add(v, None);
        }
    }

    /// `VP8LHistogramEstimateBits`.
    pub fn estimate_bits(&mut self) -> u64 {
        let mut used = [false; 5];
        let cost = population_cost(&self.literal, None, &mut used[0])
            + population_cost(&self.red, None, &mut used[1])
            + population_cost(&self.blue, None, &mut used[2])
            + population_cost(&self.alpha, None, &mut used[3])
            + population_cost(&self.distance, None, &mut used[4])
            + ((extra_cost(&self.literal[NUM_LITERAL_CODES..NUM_LITERAL_CODES + NUM_LENGTH_CODES], None) + extra_cost(&self.distance, None)) << LOG_2_PRECISION_BITS);
        self.is_used = used;
        cost
    }

    /// `UpdateHistogramCost`.
    fn update_cost(&mut self) {
        let mut used = [false; 5];
        let (mut alpha_sym, mut red_sym, mut blue_sym) = (0, 0, 0);
        let alpha_cost = population_cost(&self.alpha, Some(&mut alpha_sym), &mut used[3]);
        let distance_cost = population_cost(&self.distance, None, &mut used[4]) + (extra_cost(&self.distance, None) << LOG_2_PRECISION_BITS);
        self.literal_cost = population_cost(&self.literal, None, &mut used[0])
            + (extra_cost(&self.literal[NUM_LITERAL_CODES..NUM_LITERAL_CODES + NUM_LENGTH_CODES], None) << LOG_2_PRECISION_BITS);
        self.red_cost = population_cost(&self.red, Some(&mut red_sym), &mut used[1]);
        self.blue_cost = population_cost(&self.blue, Some(&mut blue_sym), &mut used[2]);
        self.is_used = used;
        self.bit_cost = self.literal_cost + self.red_cost + self.blue_cost + alpha_cost + distance_cost;
        self.trivial_symbol = if (alpha_sym | red_sym | blue_sym) == NON_TRIVIAL_SYM { NON_TRIVIAL_SYM } else { (alpha_sym << 24) | (red_sym << 16) | blue_sym };
    }

    /// Os cinco alfabetos na ordem do `is_used_`.
    fn alphabets(&self) -> [&[u32]; 5] {
        [&self.literal, &self.red, &self.blue, &self.alpha, &self.distance]
    }

    fn alphabets_mut(&mut self) -> [&mut [u32]; 5] {
        [&mut self.literal, &mut self.red, &mut self.blue, &mut self.alpha, &mut self.distance]
    }

    /// `HistogramAdd` com `out` igual a `self` (o `ADD_EQ` do `VP8LHistogramAdd`).
    fn add_from(&mut self, a: &Histogram) {
        let used = self.is_used;
        for (k, (dst, src)) in self.alphabets_mut().into_iter().zip(a.alphabets()).enumerate() {
            if a.is_used[k] {
                if used[k] {
                    for (d, s) in dst.iter_mut().zip(src) {
                        *d += s;
                    }
                } else {
                    dst.copy_from_slice(src);
                }
            }
        }
        for k in 0..5 {
            self.is_used[k] |= a.is_used[k];
        }
        if a.trivial_symbol != self.trivial_symbol {
            self.trivial_symbol = NON_TRIVIAL_SYM;
        }
    }

    /// `HistogramAdd` com `out` distinto das duas entradas (o `ADD` do `VP8LHistogramAdd`).
    fn sum(a: &Histogram, b: &Histogram, out: &mut Histogram) {
        for (k, ((dst, sa), sb)) in out.alphabets_mut().into_iter().zip(a.alphabets()).zip(b.alphabets()).enumerate() {
            match (a.is_used[k], b.is_used[k]) {
                (true, true) => {
                    for ((d, x), y) in dst.iter_mut().zip(sa).zip(sb) {
                        *d = x + y;
                    }
                }
                (true, false) => dst.copy_from_slice(sa),
                (false, true) => dst.copy_from_slice(sb),
                (false, false) => dst.fill(0),
            }
        }
        for k in 0..5 {
            out.is_used[k] = a.is_used[k] | b.is_used[k];
        }
        out.trivial_symbol = if a.trivial_symbol == b.trivial_symbol { a.trivial_symbol } else { NON_TRIVIAL_SYM };
    }
}

/// `BitsEntropyRefine`.
fn bits_entropy_refine(e: &BitEntropy) -> u64 {
    let mix: u64 = if e.nonzeros < 5 {
        if e.nonzeros <= 1 {
            return 0;
        }
        if e.nonzeros == 2 {
            return div_round((99 * (u64::from(e.sum) << LOG_2_PRECISION_BITS) + e.entropy) as i64, 100) as u64;
        }
        if e.nonzeros == 3 {
            950
        } else {
            700
        }
    } else {
        627
    };
    let min_limit = u64::from((2 * e.sum).wrapping_sub(e.max_val)) << LOG_2_PRECISION_BITS;
    let min_limit = div_round((mix * min_limit + (1000 - mix) * e.entropy) as i64, 1000) as u64;
    e.entropy.max(min_limit)
}

/// `VP8LBitsEntropy`.
pub fn bits_entropy(array: &[u32]) -> u64 {
    bits_entropy_refine(&bits_entropy_unrefined(array))
}

fn initial_huffman_cost() -> u64 {
    ((CODE_LENGTH_CODES * 3) << LOG_2_PRECISION_BITS) - div_round(91i64 << LOG_2_PRECISION_BITS, 10) as u64
}

/// `FinalHuffmanCost`.
fn final_huffman_cost(stats: &Streaks) -> u64 {
    let mut extra = (stats.counts[0] as u32).wrapping_mul(1600).wrapping_add(240u32.wrapping_mul(stats.streaks[0][1] as u32));
    extra = extra.wrapping_add((stats.counts[1] as u32).wrapping_mul(2640)).wrapping_add(720u32.wrapping_mul(stats.streaks[1][1] as u32));
    extra = extra.wrapping_add(1840u32.wrapping_mul(stats.streaks[0][0] as u32));
    extra = extra.wrapping_add(3360u32.wrapping_mul(stats.streaks[1][0] as u32));
    initial_huffman_cost() + (u64::from(extra) << (LOG_2_PRECISION_BITS - 10))
}

/// `PopulationCost`.
fn population_cost(population: &[u32], trivial_sym: Option<&mut u32>, is_used: &mut bool) -> u64 {
    let (e, stats) = entropy_unrefined(population, None);
    if let Some(t) = trivial_sym {
        *t = if e.nonzeros == 1 { e.nonzero_code } else { NON_TRIVIAL_SYM };
    }
    *is_used = stats.streaks[1][0] != 0 || stats.streaks[1][1] != 0;
    bits_entropy_refine(&e) + final_huffman_cost(&stats)
}

/// `GetCombinedEntropy`.
fn combined_entropy(x: &[u32], y: &[u32], x_used: bool, y_used: bool, trivial_at_end: bool) -> u64 {
    let len = x.len() as i32;
    if trivial_at_end {
        let stats = Streaks { counts: [1, 0], streaks: [[0, len - 1], [1, 0]] };
        return final_huffman_cost(&stats);
    }
    let (e, stats) = match (x_used, y_used) {
        (true, true) => entropy_unrefined(x, Some(y)),
        (true, false) => entropy_unrefined(x, None),
        (false, true) => entropy_unrefined(y, None),
        (false, false) => {
            let mut stats = Streaks { counts: [1, 0], ..Streaks::default() };
            stats.streaks[0][usize::from(len > 3)] = len;
            (BitEntropy::default(), stats)
        }
    };
    bits_entropy_refine(&e) + final_huffman_cost(&stats)
}

fn saturate_add(a: u64, b: &mut i64) {
    if *b < 0 || a as i64 <= i64::MAX - *b {
        *b += a as i64;
    } else {
        *b = i64::MAX;
    }
}

/// `GetCombinedHistogramEntropy`: `None` quando o custo alcança o limite.
fn combined_histogram_entropy(a: &Histogram, b: &Histogram, threshold_in: i64) -> Option<u64> {
    if threshold_in <= 0 {
        return None;
    }
    let threshold = threshold_in as u64;
    let lengths = NUM_LITERAL_CODES..NUM_LITERAL_CODES + NUM_LENGTH_CODES;
    let mut cost = combined_entropy(&a.literal, &b.literal, a.is_used[0], b.is_used[0], false);
    cost += extra_cost(&a.literal[lengths.clone()], Some(&b.literal[lengths])) << LOG_2_PRECISION_BITS;
    if cost >= threshold {
        return None;
    }
    let mut trivial_at_end = false;
    if a.trivial_symbol != NON_TRIVIAL_SYM && a.trivial_symbol == b.trivial_symbol {
        let edge = |c: u32| c == 0 || c == 0xff;
        let t = a.trivial_symbol;
        trivial_at_end = edge((t >> 24) & 0xff) && edge((t >> 16) & 0xff) && edge(t & 0xff);
    }
    for (k, (x, y)) in [(&a.red, &b.red), (&a.blue, &b.blue), (&a.alpha, &b.alpha)].into_iter().enumerate() {
        cost += combined_entropy(x, y, a.is_used[k + 1], b.is_used[k + 1], trivial_at_end);
        if cost >= threshold {
            return None;
        }
    }
    cost += combined_entropy(&a.distance, &b.distance, a.is_used[4], b.is_used[4], false);
    cost += extra_cost(&a.distance, Some(&b.distance)) << LOG_2_PRECISION_BITS;
    (cost < threshold).then_some(cost)
}

/// `HistogramAddEval`: soma em `out` se a combinação custar menos que o limite somado aos dois custos.
fn add_eval(a: &Histogram, b: &Histogram, out: &mut Histogram, mut threshold: i64) -> bool {
    saturate_add(a.bit_cost + b.bit_cost, &mut threshold);
    let Some(cost) = combined_histogram_entropy(a, b, threshold) else { return false };
    Histogram::sum(a, b, out);
    out.bit_cost = cost;
    out.palette_code_bits = a.palette_code_bits;
    true
}

/// `HistogramAddThresh`.
fn add_thresh(a: &Histogram, b: &Histogram, mut threshold: i64) -> Option<i64> {
    saturate_add(a.bit_cost, &mut threshold);
    combined_histogram_entropy(a, b, threshold).map(|cost| cost as i64 - a.bit_cost as i64)
}

/// O conjunto de histogramas: `None` é a vaga removida (`HistogramSetRemoveHistogram`).
struct Set {
    slots: Vec<Option<Box<Histogram>>>,
    /// `size`: a vaga depois da última ocupada.
    size: usize,
    num_used: usize,
}

impl Set {
    fn remove(&mut self, i: usize) {
        self.slots[i] = None;
        self.num_used -= 1;
        if i + 1 == self.size {
            while self.size >= 1 && self.slots[self.size - 1].is_none() {
                self.size -= 1;
            }
        }
    }

    fn get(&self, i: usize) -> &Histogram {
        self.slots[i].as_deref().expect("vaga de histograma removida")
    }

    /// `RemoveEmptyHistograms`.
    fn compact(&mut self) {
        let mut kept: Vec<Option<Box<Histogram>>> = self.slots.drain(..self.size).filter(Option::is_some).collect();
        let size = kept.len();
        kept.append(&mut self.slots);
        self.slots = kept;
        self.size = size;
    }
}

fn bin_id_for_entropy(min: u64, max: u64, val: u64) -> usize {
    let range = max - min;
    if range > 0 {
        ((NUM_PARTITIONS as f64 - 1e-6) * (val - min) as f64 / range as f64) as usize
    } else {
        0
    }
}

/// `HistogramAnalyzeEntropyBin` com `GetHistoBinIndex`.
fn analyze_entropy_bin(set: &Set, low_effort: bool) -> Vec<usize> {
    let (mut lit, mut red, mut blue) = ((u64::MAX, 0u64), (u64::MAX, 0u64), (u64::MAX, 0u64));
    for h in set.slots[..set.size].iter().flatten() {
        lit = (lit.0.min(h.literal_cost), lit.1.max(h.literal_cost));
        red = (red.0.min(h.red_cost), red.1.max(h.red_cost));
        blue = (blue.0.min(h.blue_cost), blue.1.max(h.blue_cost));
    }
    set.slots[..set.size]
        .iter()
        .map(|h| {
            h.as_ref().map_or(0, |h| {
                let mut bin = bin_id_for_entropy(lit.0, lit.1, h.literal_cost);
                if !low_effort {
                    bin = bin * NUM_PARTITIONS + bin_id_for_entropy(red.0, red.1, h.red_cost);
                    bin = bin * NUM_PARTITIONS + bin_id_for_entropy(blue.0, blue.1, h.blue_cost);
                }
                bin
            })
        })
        .collect()
}

/// `HistogramCombineEntropyBin`. O mapa de grupos do C só alimenta o `OptimizeHistogramSymbols`, cujo resultado o
/// `HistogramRemap` sobrescreve; aqui ele não é mantido.
fn combine_entropy_bin(set: &mut Set, cur_combo: &mut Box<Histogram>, bin_map: &[usize], num_bins: usize, combine_cost_factor: i32, low_effort: bool) {
    let mut first_of: Vec<Option<usize>> = vec![None; num_bins];
    let mut failures = vec![0u32; num_bins];
    for idx in 0..set.size {
        if set.slots[idx].is_none() {
            continue;
        }
        let bin = bin_map[idx];
        let Some(first) = first_of[bin] else {
            first_of[bin] = Some(idx);
            continue;
        };
        if low_effort {
            add_into(&mut set.slots, first, idx);
            set.remove(idx);
        } else {
            let bit_cost = set.get(idx).bit_cost;
            let thresh = -div_round(bit_cost as i64 * i64::from(combine_cost_factor), 100);
            if add_eval(set.get(first), set.get(idx), cur_combo, thresh) {
                let try_combine = cur_combo.trivial_symbol != NON_TRIVIAL_SYM
                    || (set.get(idx).trivial_symbol == NON_TRIVIAL_SYM && set.get(first).trivial_symbol == NON_TRIVIAL_SYM);
                if try_combine || failures[bin] >= 32 {
                    if let Some(slot) = set.slots[first].as_mut() {
                        std::mem::swap(cur_combo, slot);
                    }
                    set.remove(idx);
                } else {
                    failures[bin] += 1;
                }
            }
        }
    }
    if low_effort {
        for h in set.slots[..set.size].iter_mut().flatten() {
            h.update_cost();
        }
    }
}

fn my_rand(seed: &mut u32) -> u32 {
    *seed = ((u64::from(*seed) * 48271) % 2147483647) as u32;
    *seed
}

#[derive(Clone, Copy)]
struct Pair {
    idx1: usize,
    idx2: usize,
    cost_diff: i64,
    cost_combo: u64,
}

/// `HistoQueue`: a cabeça (`queue[0]`) é sempre o par de menor `cost_diff`.
struct Queue {
    pairs: Vec<Pair>,
    max_size: usize,
}

impl Queue {
    /// `HistoQueuePopPair`: o último par ocupa a posição `i`.
    fn pop(&mut self, i: usize) {
        self.pairs.swap_remove(i);
    }

    /// `HistoQueueUpdateHead`.
    fn update_head(&mut self, i: usize) {
        if self.pairs[i].cost_diff < self.pairs[0].cost_diff {
            self.pairs.swap(0, i);
        }
    }

    /// `HistoQueuePush`.
    fn push(&mut self, set: &Set, idx1: usize, idx2: usize, threshold: i64) -> i64 {
        if self.pairs.len() == self.max_size {
            return 0;
        }
        let (idx1, idx2) = if idx1 > idx2 { (idx2, idx1) } else { (idx1, idx2) };
        let mut pair = Pair { idx1, idx2, cost_diff: 0, cost_combo: 0 };
        if !update_pair(set.get(idx1), set.get(idx2), threshold, &mut pair) {
            return 0;
        }
        self.pairs.push(pair);
        let last = self.pairs.len() - 1;
        self.update_head(last);
        pair.cost_diff
    }
}

/// `HistoQueueUpdatePair`.
fn update_pair(h1: &Histogram, h2: &Histogram, mut threshold: i64, pair: &mut Pair) -> bool {
    let sum_cost = (h1.bit_cost + h2.bit_cost) as i64;
    saturate_add(sum_cost as u64, &mut threshold);
    let Some(combo) = combined_histogram_entropy(h1, h2, threshold) else { return false };
    pair.cost_combo = combo;
    pair.cost_diff = combo as i64 - sum_cost;
    true
}

/// Soma o histograma da vaga `src` no da vaga `dst` (`HistogramAdd(src, dst, dst)`).
fn add_into(slots: &mut [Option<Box<Histogram>>], dst: usize, src: usize) {
    let (a, b) = if dst < src {
        let (lo, hi) = slots.split_at_mut(src);
        (&mut lo[dst], &hi[0])
    } else {
        let (lo, hi) = slots.split_at_mut(dst);
        (&mut hi[0], &lo[src])
    };
    if let (Some(d), Some(s)) = (a.as_deref_mut(), b.as_deref()) {
        d.add_from(s);
    }
}

/// Junta `idx2` em `idx1` com o custo já calculado.
fn merge(set: &mut Set, idx1: usize, idx2: usize, cost_combo: u64) {
    add_into(&mut set.slots, idx1, idx2);
    if let Some(h1) = set.slots[idx1].as_deref_mut() {
        h1.bit_cost = cost_combo;
    }
    set.remove(idx2);
}

/// `HistogramCombineGreedy`.
fn combine_greedy(set: &mut Set) {
    let n = set.size;
    let mut queue = Queue { pairs: Vec::new(), max_size: n * n };
    for i in 0..n {
        if set.slots[i].is_none() {
            continue;
        }
        for j in i + 1..n {
            if set.slots[j].is_some() {
                queue.push(set, i, j, 0);
            }
        }
    }
    while !queue.pairs.is_empty() {
        let Pair { idx1, idx2, cost_combo, .. } = queue.pairs[0];
        merge(set, idx1, idx2, cost_combo);
        let mut i = 0;
        while i < queue.pairs.len() {
            let p = queue.pairs[i];
            if p.idx1 == idx1 || p.idx2 == idx1 || p.idx1 == idx2 || p.idx2 == idx2 {
                queue.pop(i);
            } else {
                queue.update_head(i);
                i += 1;
            }
        }
        for i in 0..set.size {
            if i != idx1 && set.slots[i].is_some() {
                queue.push(set, idx1, i, 0);
            }
        }
    }
}

/// `HistogramCombineStochastic`: devolve `do_greedy`.
fn combine_stochastic(set: &mut Set, min_cluster_size: usize) -> bool {
    if set.num_used < min_cluster_size {
        return true;
    }
    let mut seed = 1u32;
    let mut tries_with_no_success = 0usize;
    let outer_iters = set.num_used;
    let num_tries_no_success = outer_iters / 2;
    let mut queue = Queue { pairs: Vec::new(), max_size: 9 };
    let mut mappings: Vec<usize> = (0..set.size).filter(|&i| set.slots[i].is_some()).collect();
    let mut iter = 0;
    while iter < outer_iters && set.num_used >= min_cluster_size && {
        tries_with_no_success += 1;
        tries_with_no_success < num_tries_no_success
    } {
        iter += 1;
        let mut best_cost = queue.pairs.first().map_or(0, |p| p.cost_diff);
        let num_used = set.num_used;
        let rand_range = ((num_used - 1) * num_used) as u32;
        let num_tries = num_used / 2;
        let mut j = 0;
        while num_used >= 2 && j < num_tries {
            let tmp = my_rand(&mut seed) % rand_range;
            let idx1 = (tmp / (num_used as u32 - 1)) as usize;
            let mut idx2 = (tmp % (num_used as u32 - 1)) as usize;
            if idx2 >= idx1 {
                idx2 += 1;
            }
            let curr_cost = queue.push(set, mappings[idx1], mappings[idx2], best_cost);
            if curr_cost < 0 {
                best_cost = curr_cost;
                if queue.pairs.len() == queue.max_size {
                    break;
                }
            }
            j += 1;
        }
        if queue.pairs.is_empty() {
            continue;
        }
        let Pair { idx1: best1, idx2: best2, cost_combo, .. } = queue.pairs[0];
        if let Ok(pos) = mappings.binary_search(&best2) {
            mappings.remove(pos);
        }
        merge(set, best1, best2, cost_combo);
        let mut j = 0;
        while j < queue.pairs.len() {
            let mut p = queue.pairs[j];
            let is1 = p.idx1 == best1 || p.idx1 == best2;
            let is2 = p.idx2 == best1 || p.idx2 == best2;
            if is1 && is2 {
                queue.pop(j);
                continue;
            }
            let mut do_eval = false;
            if is1 {
                p.idx1 = best1;
                do_eval = true;
            } else if is2 {
                p.idx2 = best1;
                do_eval = true;
            }
            if p.idx1 > p.idx2 {
                std::mem::swap(&mut p.idx1, &mut p.idx2);
            }
            if do_eval && !update_pair(set.get(p.idx1), set.get(p.idx2), 0, &mut p) {
                queue.pairs[j] = p;
                queue.pop(j);
                continue;
            }
            queue.pairs[j] = p;
            queue.update_head(j);
            j += 1;
        }
        tries_with_no_success = 0;
    }
    set.num_used <= min_cluster_size
}

/// `GetCombineCostFactor`.
fn combine_cost_factor(histo_size: usize, quality: i32) -> i32 {
    let mut f = 16;
    if quality < 90 {
        if histo_size > 256 {
            f /= 2;
        }
        if histo_size > 512 {
            f /= 2;
        }
        if histo_size > 1024 {
            f /= 2;
        }
        if quality <= 50 {
            f /= 2;
        }
    }
    f
}

/// `VP8LGetHistoImageSymbols`: os histogramas finais e, para cada região, o índice do seu histograma.
pub fn histo_image_symbols(
    xsize: usize,
    ysize: usize,
    refs: &[PixOrCopy],
    quality: i32,
    low_effort: bool,
    histogram_bits: u32,
    cache_bits: u32,
) -> (Vec<Histogram>, Vec<u32>) {
    let histo_xsize = if histogram_bits > 0 { sub_sample_size(xsize, histogram_bits) } else { 1 };
    let histo_ysize = if histogram_bits > 0 { sub_sample_size(ysize, histogram_bits) } else { 1 };
    let raw_size = histo_xsize * histo_ysize;
    let num_bins = if low_effort { NUM_PARTITIONS } else { BIN_SIZE };

    // `HistogramBuild`.
    let mut orig: Vec<Option<Box<Histogram>>> = (0..raw_size).map(|_| Some(Box::new(Histogram::new(cache_bits)))).collect();
    let (mut x, mut y) = (0usize, 0usize);
    for v in refs {
        let ix = (y >> histogram_bits) * histo_xsize + (x >> histogram_bits);
        if let Some(h) = orig[ix].as_deref_mut() {
            h.add(v, None);
        }
        x += v.len();
        while x >= xsize {
            x -= xsize;
            y += 1;
        }
    }

    // `HistogramCopyAndAnalyze`.
    let mut set = Set { slots: Vec::with_capacity(raw_size), size: raw_size, num_used: raw_size };
    for h in orig.iter_mut() {
        let Some(hist) = h.as_deref_mut() else { continue };
        hist.update_cost();
        if hist.is_used.iter().any(|&u| u) {
            set.slots.push(Some(Box::new(hist.clone())));
        } else {
            *h = None;
            set.slots.push(None);
        }
    }
    let mut size = raw_size;
    for i in (0..raw_size).rev() {
        if set.slots[i].is_some() {
            break;
        }
        size = i;
    }
    set.size = size;
    set.num_used = set.slots.iter().filter(|s| s.is_some()).count();

    let entropy_combine = set.num_used > num_bins * 2 && quality < 100;
    if entropy_combine {
        let bin_map = analyze_entropy_bin(&set, low_effort);
        let mut cur_combo = Box::new(Histogram::new(cache_bits));
        combine_entropy_bin(&mut set, &mut cur_combo, &bin_map, num_bins, combine_cost_factor(raw_size, quality), low_effort);
    }
    if !low_effort || !entropy_combine {
        let q = i64::from(quality);
        let threshold_size = (1 + div_round(q * q * q * (MAX_HISTO_GREEDY - 1), 100 * 100 * 100)) as usize;
        if combine_stochastic(&mut set, threshold_size) {
            set.compact();
            combine_greedy(&mut set);
        }
    }
    set.compact();

    // `HistogramRemap`.
    let out_size = set.size;
    let mut symbols = vec![0u32; raw_size];
    if out_size > 1 {
        for i in 0..raw_size {
            let Some(h) = orig[i].as_deref() else {
                symbols[i] = symbols[i - 1];
                continue;
            };
            let (mut best_out, mut best_bits) = (0, i64::MAX);
            for k in 0..out_size {
                if let Some(cur) = add_thresh(set.get(k), h, best_bits) {
                    best_bits = cur;
                    best_out = k;
                }
            }
            symbols[i] = best_out as u32;
        }
    }
    let mut out: Vec<Histogram> = (0..out_size).map(|_| Histogram::new(cache_bits)).collect();
    for (i, h) in orig.iter().enumerate() {
        if let Some(h) = h {
            out[symbols[i] as usize].add_from(h);
        }
    }
    (out, symbols)
}
