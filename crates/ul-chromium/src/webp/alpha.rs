//! O chunk `ALPH` do libwebp 1.5.0 (`enc/alpha_enc.c`, `utils/filters_utils.c`, `dsp/filters.c`) com a configuração
//! do Skia: qualidade de alfa 100 (sem redução de níveis), filtro rápido, compressão sem perdas no esforço do método.

use super::vp8::Config;
use super::vp8l;

const FILTER_NONE: usize = 0;
const FILTER_HORIZONTAL: usize = 1;
const FILTER_VERTICAL: usize = 2;
const FILTER_GRADIENT: usize = 3;
const FILTER_LAST: usize = 4;
const SMAX: usize = 16;
const NO_COMPRESSION: u8 = 0;
const LOSSLESS_COMPRESSION: u8 = 1;

fn gradient_predictor(a: u8, b: u8, c: u8) -> u8 {
    (i32::from(a) + i32::from(b) - i32::from(c)).clamp(0, 255) as u8
}

/// `WebPFilters[filter]` (`HorizontalFilter_C`, `VerticalFilter_C`, `GradientFilter_C`) com passo igual à largura.
fn apply_filter(filter: usize, data: &[u8], width: usize, height: usize) -> Vec<u8> {
    let mut out = vec![0u8; data.len()];
    out[0] = data[0];
    for x in 1..width {
        out[x] = data[x].wrapping_sub(data[x - 1]);
    }
    for row in 1..height {
        let (at, prev) = (row * width, (row - 1) * width);
        for x in 0..width {
            let pred = match (filter, x) {
                (FILTER_VERTICAL, _) | (_, 0) => data[prev + x],
                (FILTER_HORIZONTAL, _) => data[at + x - 1],
                _ => gradient_predictor(data[at + x - 1], data[prev + x], data[prev + x - 1]),
            };
            out[at + x] = data[at + x].wrapping_sub(pred);
        }
    }
    out
}

/// `WebPEstimateBestFilter`.
fn estimate_best_filter(data: &[u8], width: usize, height: usize) -> usize {
    let mut bins = [[false; SMAX]; FILTER_LAST];
    let sdiff = |a: i32, b: i32| ((a - b).unsigned_abs() >> 4) as usize;
    let mut j = 2;
    while j + 1 < height {
        let p = j * width;
        let mut mean = i32::from(data[p]);
        let mut i = 2;
        while i + 1 < width {
            let at = |k: usize| i32::from(data[k]);
            let v = at(p + i);
            let grad = i32::from(gradient_predictor(data[p + i - 1], data[p + i - width], data[p + i - width - 1]));
            bins[FILTER_NONE][sdiff(v, mean)] = true;
            bins[FILTER_HORIZONTAL][sdiff(v, at(p + i - 1))] = true;
            bins[FILTER_VERTICAL][sdiff(v, at(p + i - width))] = true;
            bins[FILTER_GRADIENT][sdiff(v, grad)] = true;
            mean = (3 * mean + v + 2) >> 2;
            i += 2;
        }
        j += 2;
    }
    let mut best = (FILTER_NONE, i32::MAX);
    for (filter, bins) in bins.iter().enumerate() {
        let score = bins.iter().enumerate().filter(|&(_, &b)| b).map(|(i, _)| i as i32).sum();
        if score < best.1 {
            best = (filter, score);
        }
    }
    best.0
}

/// `GetFilterMap` com `WEBP_FILTER_FAST`.
fn filter_map(alpha: &[u8], width: usize, height: usize, effort: i32) -> u32 {
    let mut seen = [false; 256];
    for &a in alpha {
        seen[a as usize] = true;
    }
    let num_colors = seen.iter().filter(|&&s| s).count();
    let filter = if num_colors <= 16 { FILTER_NONE } else { estimate_best_filter(alpha, width, height) };
    let mut map = 1 << filter;
    if effort > 3 || num_colors > 192 {
        map |= 1 << FILTER_NONE;
    }
    map
}

/// `EncodeAlphaInternal` sem redução de níveis.
fn encode_with_filter(alpha: &[u8], width: usize, height: usize, filter: usize, effort: i32) -> Vec<u8> {
    let filtered = if filter == FILTER_NONE { alpha.to_vec() } else { apply_filter(filter, alpha, width, height) };
    // `WebPDispatchAlphaToGreen`: o alfa vai para o verde de pixels ARGB com A, R e B zerados.
    let argb: Vec<u32> = filtered.iter().map(|&a| u32::from(a) << 8).collect();
    let quality = 8.0 * effort as f32;
    let stream = vp8l::encode_stream(&argb, width, height, quality, effort, true);
    let (method, payload) = if stream.len() > alpha.len() { (NO_COMPRESSION, &filtered) } else { (LOSSLESS_COMPRESSION, &stream) };
    let mut out = Vec::with_capacity(1 + payload.len());
    out.push(method | (filter as u8) << 2);
    out.extend_from_slice(payload);
    out
}

/// `EncodeAlpha` com `ApplyFiltersAndEncode`: o conteúdo do chunk `ALPH`.
pub fn encode(alpha: &[u8], width: usize, height: usize, config: &Config) -> Vec<u8> {
    let effort = config.method;
    let mut try_map = filter_map(alpha, width, height, effort);
    let mut best: Option<Vec<u8>> = None;
    let mut filter = FILTER_NONE;
    while try_map != 0 {
        if try_map & 1 != 0 {
            let trial = encode_with_filter(alpha, width, height, filter, effort);
            if best.as_ref().is_none_or(|b| trial.len() < b.len()) {
                best = Some(trial);
            }
        }
        filter += 1;
        try_map >>= 1;
    }
    best.unwrap_or_default()
}
