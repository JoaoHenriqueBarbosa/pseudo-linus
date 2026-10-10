//! `dsp/lossless_enc.c`, `dsp/lossless.c` e `dsp/lossless_common.h` do libwebp 1.5.0: logaritmos em ponto fixo,
//! entropias, códigos de prefixo, operações de pixel e os 14 preditores do VP8L.

use super::tables::{LOG2_TABLE, SLOG2_TABLE};

pub const LOG_2_PRECISION_BITS: u32 = 23;
const APPROX_LOG_WITH_CORRECTION_MAX: u32 = 65536;
const APPROX_LOG_MAX: u32 = 4096;
const LOG_2_RECIPROCAL_FIXED_DOUBLE: f64 = 12102203.161561485379934310913085937500;
const LOG_2_RECIPROCAL_FIXED: u64 = 12102203;
pub const ARGB_BLACK: u32 = 0xff00_0000;

pub fn bits_log2_floor(n: u32) -> u32 {
    31 - n.leading_zeros()
}

pub fn sub_sample_size(size: usize, bits: u32) -> usize {
    (size + (1 << bits) - 1) >> bits
}

/// `DivRound`.
pub fn div_round(a: i64, b: i64) -> i64 {
    if (a < 0) == (b < 0) {
        (a + b / 2) / b
    } else {
        (a - b / 2) / b
    }
}

/// `VP8LFastLog2`.
pub fn fast_log2(v: u32) -> u32 {
    if v < 256 {
        return LOG2_TABLE[v as usize];
    }
    if v < APPROX_LOG_WITH_CORRECTION_MAX {
        let log_cnt = bits_log2_floor(v) - 7;
        let y = 1u32 << log_cnt;
        let mut log_2 = LOG2_TABLE[(v >> log_cnt) as usize] + (log_cnt << LOG_2_PRECISION_BITS);
        if v >= APPROX_LOG_MAX {
            let correction = LOG_2_RECIPROCAL_FIXED * u64::from(v & (y - 1));
            log_2 += div_round(correction as i64, i64::from(v)) as u32;
        }
        log_2
    } else {
        (LOG_2_RECIPROCAL_FIXED_DOUBLE * f64::from(v).ln() + 0.5) as u32
    }
}

/// `VP8LFastSLog2`: `v * log2(v)` em ponto fixo.
pub fn fast_slog2(v: u32) -> u64 {
    if v < 256 {
        return SLOG2_TABLE[v as usize];
    }
    if v < APPROX_LOG_WITH_CORRECTION_MAX {
        let log_cnt = u64::from(bits_log2_floor(v) - 7);
        let y = 1u32 << log_cnt;
        let correction = LOG_2_RECIPROCAL_FIXED * u64::from(v & (y - 1));
        u64::from(v) * (u64::from(LOG2_TABLE[(v >> log_cnt) as usize]) + (log_cnt << LOG_2_PRECISION_BITS)) + correction
    } else {
        (LOG_2_RECIPROCAL_FIXED_DOUBLE * f64::from(v) * f64::from(v).ln() + 0.5) as u64
    }
}

/// `CombinedShannonEntropy_C` (dois alfabetos de 256).
pub fn combined_shannon_entropy(x: &[u32], y: &[u32]) -> u64 {
    let (mut retval, mut sum_x, mut sum_xy) = (0u64, 0u32, 0u32);
    for (&xi, &yi) in x.iter().zip(y) {
        if xi != 0 {
            let xy = xi + yi;
            sum_x += xi;
            retval += fast_slog2(xi);
            sum_xy += xy;
            retval += fast_slog2(xy);
        } else if yi != 0 {
            sum_xy += yi;
            retval += fast_slog2(yi);
        }
    }
    (fast_slog2(sum_x) + fast_slog2(sum_xy)).wrapping_sub(retval)
}

pub const NON_TRIVIAL_SYM: u32 = 0xffff_ffff;

/// `VP8LBitEntropy`.
#[derive(Clone, Copy)]
pub struct BitEntropy {
    pub entropy: u64,
    pub sum: u32,
    pub nonzeros: i32,
    pub max_val: u32,
    pub nonzero_code: u32,
}

impl Default for BitEntropy {
    fn default() -> Self {
        BitEntropy { entropy: 0, sum: 0, nonzeros: 0, max_val: 0, nonzero_code: NON_TRIVIAL_SYM }
    }
}

/// `VP8LStreaks`.
#[derive(Clone, Copy, Default)]
pub struct Streaks {
    pub counts: [i32; 2],
    pub streaks: [[i32; 2]; 2],
}

/// `VP8LBitsEntropyUnrefined`.
pub fn bits_entropy_unrefined(array: &[u32]) -> BitEntropy {
    let mut e = BitEntropy::default();
    for (i, &v) in array.iter().enumerate() {
        if v != 0 {
            e.sum += v;
            e.nonzero_code = i as u32;
            e.nonzeros += 1;
            e.entropy += fast_slog2(v);
            e.max_val = e.max_val.max(v);
        }
    }
    e.entropy = fast_slog2(e.sum).wrapping_sub(e.entropy);
    e
}

fn entropy_helper(val: u32, i: usize, val_prev: &mut u32, i_prev: &mut usize, e: &mut BitEntropy, stats: &mut Streaks) {
    let streak = (i - *i_prev) as i32;
    if *val_prev != 0 {
        e.sum = e.sum.wrapping_add(val_prev.wrapping_mul(streak as u32));
        e.nonzeros += streak;
        e.nonzero_code = *i_prev as u32;
        e.entropy += fast_slog2(*val_prev) * streak as u64;
        e.max_val = e.max_val.max(*val_prev);
    }
    let nz = usize::from(*val_prev != 0);
    stats.counts[nz] += i32::from(streak > 3);
    stats.streaks[nz][usize::from(streak > 3)] += streak;
    *val_prev = val;
    *i_prev = i;
}

/// `GetEntropyUnrefined_C` e `GetCombinedEntropyUnrefined_C` (com `y`).
pub fn entropy_unrefined(x: &[u32], y: Option<&[u32]>) -> (BitEntropy, Streaks) {
    let at = |i: usize| x[i] + y.map_or(0, |y| y[i]);
    let mut e = BitEntropy::default();
    let mut stats = Streaks::default();
    let (mut i_prev, mut prev) = (0usize, at(0));
    for i in 1..x.len() {
        let v = at(i);
        if v != prev {
            entropy_helper(v, i, &mut prev, &mut i_prev, &mut e, &mut stats);
        }
    }
    entropy_helper(0, x.len(), &mut prev, &mut i_prev, &mut e, &mut stats);
    e.entropy = fast_slog2(e.sum).wrapping_sub(e.entropy);
    (e, stats)
}

/// `ExtraCost_C` e `ExtraCostCombined_C` (com `y`).
pub fn extra_cost(x: &[u32], y: Option<&[u32]>) -> u64 {
    let at = |i: usize| u64::from(x[i]) + y.map_or(0, |y| u64::from(y[i]));
    let mut cost = at(4) + at(5);
    for i in 2..x.len() / 2 - 1 {
        cost += i as u64 * (at(2 * i + 2) + at(2 * i + 3));
    }
    cost as u32 as u64
}

/// `VP8LPrefixEncode`: (código, bits extras, valor dos bits extras).
pub fn prefix_encode(distance: u32) -> (u32, u32, u32) {
    if distance < 5 {
        return (distance.saturating_sub(1), 0, 0);
    }
    let d = distance - 1;
    let highest_bit = bits_log2_floor(d);
    let second = (d >> (highest_bit - 1)) & 1;
    let extra_bits = highest_bit - 1;
    (2 * highest_bit + second, extra_bits, d & ((1 << extra_bits) - 1))
}

/// `VP8LSubPixels`.
pub fn sub_pixels(a: u32, b: u32) -> u32 {
    let ag = 0x00ff00ffu32.wrapping_add(a & 0xff00ff00).wrapping_sub(b & 0xff00ff00);
    let rb = 0xff00ff00u32.wrapping_add(a & 0x00ff00ff).wrapping_sub(b & 0x00ff00ff);
    (ag & 0xff00ff00) | (rb & 0x00ff00ff)
}

fn average2(a0: u32, a1: u32) -> u32 {
    (((a0 ^ a1) & 0xfefefefe) >> 1) + (a0 & a1)
}

fn clip255(a: u32) -> u32 {
    if a < 256 {
        a
    } else {
        !a >> 24
    }
}

fn channels(c: u32) -> [i32; 4] {
    [(c >> 24) as i32, ((c >> 16) & 0xff) as i32, ((c >> 8) & 0xff) as i32, (c & 0xff) as i32]
}

fn pack(v: [u32; 4]) -> u32 {
    (v[0] << 24) | (v[1] << 16) | (v[2] << 8) | v[3]
}

fn clamped_add_subtract_full(c0: u32, c1: u32, c2: u32) -> u32 {
    let (a, b, c) = (channels(c0), channels(c1), channels(c2));
    pack(std::array::from_fn(|k| clip255((a[k] + b[k] - c[k]) as u32)))
}

fn clamped_add_subtract_half(c0: u32, c1: u32, c2: u32) -> u32 {
    let (a, c) = (channels(average2(c0, c1)), channels(c2));
    pack(std::array::from_fn(|k| clip255((a[k] + (a[k] - c[k]) / 2) as u32)))
}

fn select(a: u32, b: u32, c: u32) -> u32 {
    let (a4, b4, c4) = (channels(a), channels(b), channels(c));
    let pa_minus_pb: i32 = (0..4).map(|k| (b4[k] - c4[k]).abs() - (a4[k] - c4[k]).abs()).sum();
    if pa_minus_pb <= 0 {
        a
    } else {
        b
    }
}

/// `VP8LPredictor{mode}_C`: `top` traz `[top[-1], top[0], top[1]]`.
pub fn predict(mode: u32, left: u32, top: [u32; 3]) -> u32 {
    let [tl, t, tr] = top;
    match mode {
        0 => ARGB_BLACK,
        1 => left,
        2 => t,
        3 => tr,
        4 => tl,
        5 => average2(average2(left, tr), t),
        6 => average2(left, tl),
        7 => average2(left, t),
        8 => average2(tl, t),
        9 => average2(t, tr),
        10 => average2(average2(left, tl), average2(t, tr)),
        11 => select(t, left, tl),
        12 => clamped_add_subtract_full(left, t, tl),
        13 => clamped_add_subtract_half(left, t, tl),
        _ => ARGB_BLACK,
    }
}

/// `VP8LSubtractGreenFromBlueAndRed_C`.
pub fn subtract_green(data: &mut [u32]) {
    for p in data {
        let argb = *p;
        let green = (argb >> 8) & 0xff;
        let r = ((argb >> 16) & 0xff).wrapping_sub(green) & 0xff;
        let b = (argb & 0xff).wrapping_sub(green) & 0xff;
        *p = (argb & 0xff00ff00) | (r << 16) | b;
    }
}

/// `VP8LBundleColorMap_C`.
pub fn bundle_color_map(row: &[u8], xbits: u32, dst: &mut [u32]) {
    if xbits > 0 {
        let bit_depth = 1 << (3 - xbits);
        let mask = (1 << xbits) - 1;
        let mut code = 0xff00_0000u32;
        for (x, &v) in row.iter().enumerate() {
            let xsub = x & mask;
            if xsub == 0 {
                code = 0xff00_0000;
            }
            code |= u32::from(v) << (8 + bit_depth * xsub);
            dst[x >> xbits] = code;
        }
    } else {
        for (d, &v) in dst.iter_mut().zip(row) {
            *d = 0xff00_0000 | (u32::from(v) << 8);
        }
    }
}
