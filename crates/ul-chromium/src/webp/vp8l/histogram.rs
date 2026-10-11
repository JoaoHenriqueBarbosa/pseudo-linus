//! `enc/histogram_enc.c` do libwebp 1.6.0 (o do Chromium do oráculo): os histogramas do VP8L, as estimativas de
//! custo e o agrupamento das regiões em códigos de Huffman comuns (por faixas de entropia, estocástico e guloso).
//! O 1.6.0 guarda o custo de cada alfabeto e o símbolo trivial por alfabeto, e o conjunto fica compacto: remover um
//! histograma põe o último no lugar dele.

use super::backward_refs::{distance_to_plane_code, PixOrCopy};
use super::dsp::{bits_entropy_unrefined, div_round, entropy_unrefined, extra_cost, prefix_encode, sub_sample_size, BitEntropy, Streaks, LOG_2_PRECISION_BITS};

pub const NUM_LITERAL_CODES: usize = 256;
pub const NUM_LENGTH_CODES: usize = 24;
pub const NUM_DISTANCE_CODES: usize = 40;
const CODE_LENGTH_CODES: u64 = 19;
const NUM_PARTITIONS: usize = 4;
const BIN_SIZE: usize = NUM_PARTITIONS * NUM_PARTITIONS * NUM_PARTITIONS;
const MAX_HISTO_GREEDY: i64 = 100;
/// `VP8L_NON_TRIVIAL_SYM` do 1.6.0, em 16 bits.
const NON_TRIVIAL_SYM: u16 = 0xffff;
/// Os índices dos alfabetos (`HistogramIndex`).
const LITERAL: usize = 0;
const RED: usize = 1;
const BLUE: usize = 2;
const ALPHA: usize = 3;

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
    trivial_symbol: [u16; 5],
    bit_cost: u64,
    costs: [u64; 5],
    is_used: [bool; 5],
    bin_id: usize,
}

impl Histogram {
    /// Um histograma zerado com as estatísticas do `HistogramStatsClear` (todo alfabeto usado, nenhum trivial).
    pub fn new(palette_code_bits: u32) -> Self {
        Histogram {
            literal: vec![0; num_codes(palette_code_bits)],
            red: [0; NUM_LITERAL_CODES],
            blue: [0; NUM_LITERAL_CODES],
            alpha: [0; NUM_LITERAL_CODES],
            distance: [0; NUM_DISTANCE_CODES],
            palette_code_bits,
            trivial_symbol: [NON_TRIVIAL_SYM; 5],
            bit_cost: 0,
            costs: [0; 5],
            is_used: [true; 5],
            bin_id: 0,
        }
    }

    /// O histograma do `VP8LHistogramSetClear`: tudo zerado pelo `memset`, inclusive o `is_used` e o símbolo
    /// trivial (que fica 0, não o não trivial).
    fn cleared(palette_code_bits: u32) -> Self {
        Histogram { trivial_symbol: [0; 5], is_used: [false; 5], ..Histogram::new(palette_code_bits) }
    }

    /// `HistogramAddSinglePixOrCopy`; com `plane_xsize` a distância passa pelo `DistanceToPlaneCode`.
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

    /// `VP8LHistogramStoreRefs` sem modificador de distância.
    pub fn store_refs(&mut self, refs: &[PixOrCopy]) {
        for v in refs {
            self.add(v, None);
        }
    }

    /// Os cinco alfabetos na ordem do `HistogramIndex` (`GetPopulationInfo`).
    fn alphabets(&self) -> [&[u32]; 5] {
        [&self.literal, &self.red, &self.blue, &self.alpha, &self.distance]
    }

    fn alphabets_mut(&mut self) -> [&mut [u32]; 5] {
        [&mut self.literal, &mut self.red, &mut self.blue, &mut self.alpha, &mut self.distance]
    }

    /// `VP8LHistogramEstimateBits`.
    pub fn estimate_bits(&self) -> u64 {
        let cost: u64 = self.alphabets().iter().map(|p| population_cost(p).0).sum();
        cost + ((extra_cost(&self.literal[NUM_LITERAL_CODES..NUM_LITERAL_CODES + NUM_LENGTH_CODES], None) + extra_cost(&self.distance, None)) << LOG_2_PRECISION_BITS)
    }

    /// `ComputeHistogramCost`: o custo, o símbolo trivial e o uso de cada alfabeto, sem o custo extra de comprimento e
    /// distância (constante para o agrupamento).
    fn compute_cost(&mut self) {
        for i in 0..5 {
            let (cost, trivial, used) = population_cost(self.alphabets()[i]);
            self.costs[i] = cost;
            self.trivial_symbol[i] = trivial;
            self.is_used[i] = used;
        }
        self.bit_cost = self.costs.iter().sum();
    }

    /// `HistogramAdd(a, self, self)`.
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
        self.merge_stats(a);
    }

    /// `HistogramAdd(a, b, out)` com `out` distinto das duas entradas.
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
        out.trivial_symbol = b.trivial_symbol;
        out.is_used = b.is_used;
        out.merge_stats(a);
    }

    /// O fim do `HistogramAdd`: o símbolo trivial que as duas partes têm em comum e o uso de cada alfabeto.
    fn merge_stats(&mut self, a: &Histogram) {
        for i in 0..5 {
            if a.trivial_symbol[i] != self.trivial_symbol[i] {
                self.trivial_symbol[i] = NON_TRIVIAL_SYM;
            }
            self.is_used[i] |= a.is_used[i];
        }
    }

    /// `UpdateHistogramCost`.
    fn update_cost(&mut self, bit_cost: u64, costs: [u64; 5]) {
        self.bit_cost = bit_cost;
        self.costs = costs;
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

/// `PopulationCost`: o custo, o símbolo trivial (o único presente, se houver um só) e se o alfabeto é usado (alguma
/// sequência de não zeros).
fn population_cost(population: &[u32]) -> (u64, u16, bool) {
    let (e, stats) = entropy_unrefined(population, None);
    let trivial = if e.nonzeros == 1 { e.nonzero_code as u16 } else { NON_TRIVIAL_SYM };
    let used = stats.streaks[1][0] != 0 || stats.streaks[1][1] != 0;
    (bits_entropy_refine(&e) + final_huffman_cost(&stats), trivial, used)
}

/// `GetCombinedEntropy`: com o mesmo símbolo trivial nos dois, ou um deles sem uso, vale o custo já guardado.
fn combined_entropy(h1: &Histogram, h2: &Histogram, index: usize) -> u64 {
    let (u1, u2) = (h1.is_used[index], h2.is_used[index]);
    let trivial = h1.trivial_symbol[index] != NON_TRIVIAL_SYM && h1.trivial_symbol[index] == h2.trivial_symbol[index];
    if trivial || !u1 || !u2 {
        return if u1 { h1.costs[index] } else { h2.costs[index] };
    }
    let (e, stats) = entropy_unrefined(h1.alphabets()[index], Some(h2.alphabets()[index]));
    bits_entropy_refine(&e) + final_huffman_cost(&stats)
}

fn saturate_add(a: u64, b: &mut i64) {
    if *b < 0 || a as i64 <= i64::MAX - *b {
        *b += a as i64;
    } else {
        *b = i64::MAX;
    }
}

/// `GetCombinedHistogramEntropy`: o custo da soma e o de cada alfabeto; `None` quando o custo alcança o limite.
fn combined_histogram_entropy(a: &Histogram, b: &Histogram, threshold_in: i64) -> Option<(u64, [u64; 5])> {
    if threshold_in <= 0 {
        return None;
    }
    let threshold = threshold_in as u64;
    let (mut cost, mut costs) = (0u64, [0u64; 5]);
    for i in 0..5 {
        costs[i] = combined_entropy(a, b, i);
        cost += costs[i];
        if cost >= threshold {
            return None;
        }
    }
    Some((cost, costs))
}

/// `HistogramAddEval`: soma em `out` se a combinação custar menos que o limite somado aos dois custos.
fn add_eval(a: &Histogram, b: &Histogram, out: &mut Histogram, mut threshold: i64) -> bool {
    saturate_add(a.bit_cost + b.bit_cost, &mut threshold);
    let Some((cost, costs)) = combined_histogram_entropy(a, b, threshold) else { return false };
    Histogram::sum(a, b, out);
    out.update_cost(cost, costs);
    true
}

/// `HistogramAddThresh`.
fn add_thresh(a: &Histogram, b: &Histogram, mut threshold: i64) -> Option<i64> {
    saturate_add(a.bit_cost, &mut threshold);
    combined_histogram_entropy(a, b, threshold).map(|(cost, _)| cost as i64 - a.bit_cost as i64)
}

/// `HistogramSetRemoveHistogram`: o último histograma ocupa o lugar do removido.
fn remove(set: &mut Vec<Box<Histogram>>, i: usize) {
    set.swap_remove(i);
}

/// Soma o histograma `src` no `dst` do conjunto (`HistogramAdd(src, dst, dst)`).
fn add_into(set: &mut [Box<Histogram>], dst: usize, src: usize) {
    let (d, s) = if dst < src {
        let (lo, hi) = set.split_at_mut(src);
        (&mut lo[dst], &hi[0])
    } else {
        let (lo, hi) = set.split_at_mut(dst);
        (&mut hi[0], &lo[src])
    };
    d.add_from(s);
}

fn bin_id_for_entropy(min: u64, max: u64, val: u64) -> usize {
    let range = max - min;
    if range > 0 {
        ((NUM_PARTITIONS as f64 - 1e-6) * (val - min) as f64 / range as f64) as usize
    } else {
        0
    }
}

/// `HistogramAnalyzeEntropyBin` com `GetHistoBinIndex`: a faixa de cada histograma pelos custos de literal,
/// vermelho e azul.
fn analyze_entropy_bin(set: &mut [Box<Histogram>], low_effort: bool) {
    let (mut lit, mut red, mut blue) = ((u64::MAX, 0u64), (u64::MAX, 0u64), (u64::MAX, 0u64));
    for h in set.iter() {
        lit = (lit.0.min(h.costs[LITERAL]), lit.1.max(h.costs[LITERAL]));
        red = (red.0.min(h.costs[RED]), red.1.max(h.costs[RED]));
        blue = (blue.0.min(h.costs[BLUE]), blue.1.max(h.costs[BLUE]));
    }
    for h in set.iter_mut() {
        let mut bin = bin_id_for_entropy(lit.0, lit.1, h.costs[LITERAL]);
        if !low_effort {
            bin = bin * NUM_PARTITIONS + bin_id_for_entropy(red.0, red.1, h.costs[RED]);
            bin = bin * NUM_PARTITIONS + bin_id_for_entropy(blue.0, blue.1, h.costs[BLUE]);
        }
        h.bin_id = bin;
    }
}

/// O símbolo de vermelho, azul ou alfa não é trivial.
fn any_non_trivial(h: &Histogram) -> bool {
    [RED, BLUE, ALPHA].iter().any(|&i| h.trivial_symbol[i] == NON_TRIVIAL_SYM)
}

/// `HistogramCombineEntropyBin`.
fn combine_entropy_bin(set: &mut Vec<Box<Histogram>>, cur_combo: &mut Box<Histogram>, num_bins: usize, combine_cost_factor: i32, low_effort: bool) {
    let mut first_of: Vec<Option<usize>> = vec![None; num_bins];
    let mut failures = vec![0u32; num_bins];
    let mut idx = 0;
    while idx < set.len() {
        let bin = set[idx].bin_id;
        let Some(first) = first_of[bin] else {
            first_of[bin] = Some(idx);
            idx += 1;
            continue;
        };
        if low_effort {
            add_into(set, first, idx);
            remove(set, idx);
            continue;
        }
        let thresh = -div_round(set[idx].bit_cost as i64 * i64::from(combine_cost_factor), 100);
        if add_eval(&set[first], &set[idx], cur_combo, thresh) {
            // Junta só quando a soma é trivial, ou quando os dois já não eram; depois de 32 recusas na faixa, junta
            // de qualquer jeito para o cabeçalho não crescer.
            let try_combine = ![RED, BLUE, ALPHA].iter().any(|&i| cur_combo.trivial_symbol[i] == NON_TRIVIAL_SYM)
                || (any_non_trivial(&set[idx]) && any_non_trivial(&set[first]));
            if try_combine || failures[bin] >= 32 {
                std::mem::swap(cur_combo, &mut set[first]);
                remove(set, idx);
            } else {
                failures[bin] += 1;
                idx += 1;
            }
        } else {
            idx += 1;
        }
    }
    if low_effort {
        for h in set.iter_mut() {
            h.compute_cost();
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
    costs: [u64; 5],
}

impl Pair {
    /// `HistoQueueFixPair`: troca `bad` por `good` e mantém `idx1 <= idx2`.
    fn fix(&mut self, bad: usize, good: usize) {
        if self.idx1 == bad {
            self.idx1 = good;
        }
        if self.idx2 == bad {
            self.idx2 = good;
        }
        if self.idx1 > self.idx2 {
            std::mem::swap(&mut self.idx1, &mut self.idx2);
        }
    }
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
    fn push(&mut self, set: &[Box<Histogram>], idx1: usize, idx2: usize, threshold: i64) -> i64 {
        if self.pairs.len() == self.max_size {
            return 0;
        }
        let (idx1, idx2) = if idx1 > idx2 { (idx2, idx1) } else { (idx1, idx2) };
        let mut pair = Pair { idx1, idx2, cost_diff: 0, cost_combo: 0, costs: [0; 5] };
        if !update_pair(&set[idx1], &set[idx2], threshold, &mut pair) {
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
    let Some((combo, costs)) = combined_histogram_entropy(h1, h2, threshold) else { return false };
    pair.cost_combo = combo;
    pair.costs = costs;
    pair.cost_diff = combo as i64 - sum_cost;
    true
}

/// Junta `idx2` em `idx1` com o custo do par e tira `idx2` do conjunto.
fn merge(set: &mut Vec<Box<Histogram>>, pair: &Pair) {
    add_into(set, pair.idx1, pair.idx2);
    set[pair.idx1].update_cost(pair.cost_combo, pair.costs);
    remove(set, pair.idx2);
}

/// `HistogramCombineGreedy`.
fn combine_greedy(set: &mut Vec<Box<Histogram>>) {
    let n = set.len();
    let mut queue = Queue { pairs: Vec::new(), max_size: n * n };
    for i in 0..n {
        for j in i + 1..n {
            queue.push(set, i, j, 0);
        }
    }
    while !queue.pairs.is_empty() {
        let best = queue.pairs[0];
        let (idx1, idx2) = (best.idx1, best.idx2);
        merge(set, &best);
        let mut i = 0;
        while i < queue.pairs.len() {
            let p = &mut queue.pairs[i];
            if p.idx1 == idx1 || p.idx2 == idx1 || p.idx1 == idx2 || p.idx2 == idx2 {
                queue.pop(i);
            } else {
                p.fix(set.len(), idx2);
                queue.update_head(i);
                i += 1;
            }
        }
        for i in 0..set.len() {
            if i != idx1 {
                queue.push(set, idx1, i, 0);
            }
        }
    }
}

/// `HistogramCombineStochastic`: devolve `do_greedy`.
fn combine_stochastic(set: &mut Vec<Box<Histogram>>, min_cluster_size: usize) -> bool {
    if set.len() < min_cluster_size {
        return true;
    }
    let mut seed = 1u32;
    let mut tries_with_no_success = 0usize;
    let outer_iters = set.len();
    let num_tries_no_success = outer_iters / 2;
    let mut queue = Queue { pairs: Vec::new(), max_size: 9 };
    let mut iter = 0;
    while iter < outer_iters && set.len() >= min_cluster_size && {
        tries_with_no_success += 1;
        tries_with_no_success < num_tries_no_success
    } {
        iter += 1;
        let mut best_cost = queue.pairs.first().map_or(0, |p| p.cost_diff);
        let size = set.len();
        let rand_range = ((size - 1) * size) as u32;
        let num_tries = size / 2;
        let mut j = 0;
        while size >= 2 && j < num_tries {
            let tmp = my_rand(&mut seed) % rand_range;
            let idx1 = (tmp / (size as u32 - 1)) as usize;
            let mut idx2 = (tmp % (size as u32 - 1)) as usize;
            if idx2 >= idx1 {
                idx2 += 1;
            }
            let curr_cost = queue.push(set, idx1, idx2, best_cost);
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
        let best = queue.pairs[0];
        let (best1, best2) = (best.idx1, best.idx2);
        merge(set, &best);
        let mut j = 0;
        while j < queue.pairs.len() {
            let mut p = queue.pairs[j];
            let is1 = p.idx1 == best1 || p.idx1 == best2;
            let is2 = p.idx2 == best1 || p.idx2 == best2;
            if is1 && is2 {
                queue.pop(j);
                continue;
            }
            if is1 || is2 {
                p.fix(best2, best1);
                if !update_pair(&set[p.idx1], &set[p.idx2], 0, &mut p) {
                    queue.pop(j);
                    continue;
                }
            }
            p.fix(set.len(), best2);
            queue.pairs[j] = p;
            queue.update_head(j);
            j += 1;
        }
        tries_with_no_success = 0;
    }
    set.len() <= min_cluster_size
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

    // `HistogramBuild` (sobre o conjunto limpo pelo `VP8LHistogramSetClear`).
    let mut orig: Vec<Option<Histogram>> = (0..raw_size).map(|_| Some(Histogram::cleared(cache_bits))).collect();
    let (mut x, mut y) = (0usize, 0usize);
    for v in refs {
        let ix = (y >> histogram_bits) * histo_xsize + (x >> histogram_bits);
        if let Some(h) = orig[ix].as_mut() {
            h.add(v, None);
        }
        x += v.len();
        while x >= xsize {
            x -= xsize;
            y += 1;
        }
    }

    // `HistogramCopyAndAnalyze`: o histograma sem nada (região toda coberta por cópias de outras) sai.
    let mut set: Vec<Box<Histogram>> = Vec::with_capacity(raw_size);
    for slot in orig.iter_mut() {
        let Some(h) = slot.as_mut() else { continue };
        h.compute_cost();
        if h.is_used.iter().any(|&u| u) {
            set.push(Box::new(h.clone()));
        } else {
            *slot = None;
        }
    }

    let entropy_combine = set.len() > num_bins * 2 && quality < 100;
    if entropy_combine {
        analyze_entropy_bin(&mut set, low_effort);
        let mut cur_combo = Box::new(Histogram::new(cache_bits));
        combine_entropy_bin(&mut set, &mut cur_combo, num_bins, combine_cost_factor(raw_size, quality), low_effort);
    }
    if !low_effort || !entropy_combine {
        let q = i64::from(quality);
        let threshold_size = (1 + div_round(q * q * q * (MAX_HISTO_GREEDY - 1), 100 * 100 * 100)) as usize;
        if combine_stochastic(&mut set, threshold_size) {
            combine_greedy(&mut set);
        }
    }

    // `HistogramRemap`.
    let out_size = set.len();
    let mut symbols = vec![0u32; raw_size];
    if out_size > 1 {
        for i in 0..raw_size {
            let Some(h) = orig[i].as_ref() else {
                symbols[i] = symbols[i - 1];
                continue;
            };
            let (mut best_out, mut best_bits) = (0, i64::MAX);
            for (k, out) in set.iter().enumerate() {
                if let Some(cur) = add_thresh(out, h, best_bits) {
                    best_bits = cur;
                    best_out = k;
                }
            }
            symbols[i] = best_out as u32;
        }
    }
    let mut out: Vec<Histogram> = (0..out_size).map(|_| Histogram::cleared(cache_bits)).collect();
    for (i, h) in orig.iter().enumerate() {
        if let Some(h) = h {
            out[symbols[i] as usize].add_from(h);
        }
    }
    (out, symbols)
}
