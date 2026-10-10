//! `enc/predictor_enc.c` do libwebp 1.5.0 no alcance do Skia: a imagem de resíduos do preditor no esforço baixo
//! (método 0, preditor 11 fixo) e no esforço maior (busca do melhor preditor por bloco em cada tamanho de
//! `min_bits` a `max_bits`, com a escolha do tamanho pela entropia), e o `VP8LOptimizeSampling`.

use super::dsp::{combined_shannon_entropy, div_round, fast_slog2, predict, sub_pixels, sub_sample_size, ARGB_BLACK, LOG_2_PRECISION_BITS};
use super::MAX_TRANSFORM_BITS;

const HISTO_SIZE: usize = 4 * 256;
const SPATIAL_PREDICTOR_BIAS: i64 = 15 << LOG_2_PRECISION_BITS;
pub const PRED_LOW_EFFORT: u32 = 11;
const NUM_PRED_MODES: usize = 14;
const MASK_ALPHA: u32 = 0xff00_0000;

/// `PredictionCostBias`.
pub fn prediction_cost_bias(counts: &[u32], weight_0: u64, exp_val: u64) -> i64 {
    let mut bits = (weight_0 * u64::from(counts[0])) << LOG_2_PRECISION_BITS;
    let mut exp_val = exp_val << LOG_2_PRECISION_BITS;
    for i in 1..16 {
        bits += div_round((exp_val * u64::from(counts[i] + counts[256 - i])) as i64, 100) as u64;
        exp_val = div_round((6 * exp_val) as i64, 10) as u64;
    }
    -div_round(bits as i64, 10)
}

/// `PredictionCostSpatialHistogram`.
fn prediction_cost_spatial(accumulated: &[u32], tile: &[u32], mode: u32, left_mode: u32, above_mode: u32) -> i64 {
    let mut retval = 0i64;
    for i in 0..4 {
        let (t, a) = (&tile[i * 256..i * 256 + 256], &accumulated[i * 256..i * 256 + 256]);
        retval += prediction_cost_bias(t, 1, 94);
        retval += combined_shannon_entropy(t, a) as i64;
    }
    if mode == left_mode {
        retval -= SPATIAL_PREDICTOR_BIAS;
    }
    if mode == above_mode {
        retval -= SPATIAL_PREDICTOR_BIAS;
    }
    retval
}

fn update_histo(histo: &mut [u32], argb: u32) {
    histo[(argb >> 24) as usize] += 1;
    histo[256 + ((argb >> 16) & 0xff) as usize] += 1;
    histo[512 + ((argb >> 8) & 0xff) as usize] += 1;
    histo[768 + (argb & 0xff) as usize] += 1;
}

/// `PredictBatch` (o `VP8LPredictorsSub`): `current` e `upper` são linhas inteiras com uma coluna a mais à direita.
fn predict_batch(mode: u32, x_start: usize, x_end: usize, y: usize, current: &[u32], upper: &[u32], out: &mut [u32]) {
    for (k, x) in (x_start..x_end).enumerate() {
        let pred = if x == 0 {
            if y == 0 {
                ARGB_BLACK
            } else {
                upper[0]
            }
        } else if y == 0 {
            current[x - 1]
        } else {
            predict(mode, current[x - 1], [upper[x - 1], upper[x], upper[x + 1]])
        };
        out[k] = sub_pixels(current[x], pred);
    }
}

/// `GetResidual` sem o quase sem perdas (`near_lossless` 100 em todos os caminhos do Skia).
fn residual(width: usize, upper: &mut [u32], current: &mut [u32], mode: u32, x_start: usize, x_end: usize, y: usize, exact: bool, out: &mut [u32]) {
    if exact {
        predict_batch(mode, x_start, x_end, y, current, upper, out);
        return;
    }
    for x in x_start..x_end {
        let pred = if y == 0 {
            if x == 0 {
                ARGB_BLACK
            } else {
                current[x - 1]
            }
        } else if x == 0 {
            upper[x]
        } else {
            predict(mode, current[x - 1], [upper[x - 1], upper[x], upper[x + 1]])
        };
        let mut res = sub_pixels(current[x], pred);
        if current[x] & MASK_ALPHA == 0 {
            res &= MASK_ALPHA;
            current[x] = pred & !MASK_ALPHA;
            if x == 0 && y != 0 {
                upper[width] = current[0];
            }
        }
        out[x - x_start] = res;
    }
}

/// `CopyImageWithPrediction` sem o quase sem perdas: troca cada pixel pelo resíduo do modo do seu bloco.
fn copy_image_with_prediction(width: usize, height: usize, bits: u32, modes: &[u32], argb: &mut [u32], low_effort: bool, exact: bool) {
    let tiles_per_row = sub_sample_size(width, bits);
    let mut upper = vec![0u32; width + 1];
    let mut current = vec![0u32; width + 1];
    for y in 0..height {
        std::mem::swap(&mut upper, &mut current);
        let n = width + usize::from(y + 1 < height);
        current[..n].copy_from_slice(&argb[y * width..y * width + n]);
        let row = &mut argb[y * width..(y + 1) * width];
        if low_effort {
            predict_batch(PRED_LOW_EFFORT, 0, width, y, &current, &upper, row);
        } else {
            let mut x = 0;
            while x < width {
                let mode = (modes[(y >> bits) * tiles_per_row + (x >> bits)] >> 8) & 0xff;
                let x_end = (x + (1 << bits)).min(width);
                residual(width, &mut upper, &mut current, mode, x, x_end, y, exact, &mut row[x..x_end]);
                x = x_end;
            }
        }
    }
}

/// `VP8LOptimizeSampling`: aumenta os bits enquanto a imagem amostrada se repetir em blocos.
pub fn optimize_sampling(image: &mut [u32], full_width: usize, full_height: usize, bits: u32, max_bits: u32) -> u32 {
    let mut width = sub_sample_size(full_width, bits);
    let mut height = sub_sample_size(full_height, bits);
    let mut best_bits = bits;
    while best_bits < max_bits {
        let new_square = 1usize << (best_bits + 1 - bits);
        let square = 1usize << (best_bits - bits);
        let mut is_good = true;
        let mut y = 0;
        while y + square < height {
            if image[y * width..(y + 1) * width] != image[(y + square) * width..(y + square + 1) * width] {
                is_good = false;
                break;
            }
            y += new_square;
        }
        if !is_good {
            break;
        }
        best_bits += 1;
    }
    if best_bits == bits {
        return bits;
    }
    while best_bits > bits {
        let square = 1usize << (best_bits - bits);
        let mut is_good = true;
        'rows: for y in 0..height {
            let mut x = 0;
            while x < width {
                for i in x + 1..(x + square).min(width) {
                    if image[y * width + i] != image[y * width + x] {
                        is_good = false;
                        break 'rows;
                    }
                }
                x += square;
            }
        }
        if is_good {
            break;
        }
        best_bits -= 1;
    }
    if best_bits == bits {
        return bits;
    }
    let old_width = width;
    let square = 1usize << (best_bits - bits);
    width = sub_sample_size(full_width, best_bits);
    height = sub_sample_size(full_height, best_bits);
    for y in 0..height {
        for x in 0..width {
            image[y * width + x] = image[square * (y * old_width + x)];
        }
    }
    best_bits
}

/// `ComputeResidualsForTile`: o histograma de resíduos de cada modo no bloco de `bits` (o nível 0), somado também
/// aos superblocos dos níveis 1 a `update_up_to`. `histos` guarda os níveis em sequência, `NUM_PRED_MODES`
/// histogramas cada.
fn residuals_for_tile(width: usize, height: usize, tile_x: usize, tile_y: usize, bits: u32, update_up_to: usize, argb: &[u32], exact: bool, histos: &mut [u32]) {
    let start_x = tile_x << bits;
    let start_y = tile_y << bits;
    let tile_size = 1usize << bits;
    let max_y = tile_size.min(height - start_y);
    let max_x = tile_size.min(width - start_x);
    let have_left = usize::from(start_x > 0);
    let context_start_x = start_x - have_left;
    let mut residuals = vec![0u32; max_x];
    let mut upper = vec![0u32; width + 1];
    let mut current = vec![0u32; width + 1];
    for mode in 0..NUM_PRED_MODES {
        if start_y > 0 {
            let n = max_x + have_left + 1;
            let from = (start_y - 1) * width + context_start_x;
            current[context_start_x..context_start_x + n].copy_from_slice(&argb[from..from + n]);
        }
        for relative_y in 0..max_y {
            let y = start_y + relative_y;
            std::mem::swap(&mut upper, &mut current);
            let n = max_x + have_left + usize::from(y + 1 < height);
            let from = y * width + context_start_x;
            current[context_start_x..context_start_x + n].copy_from_slice(&argb[from..from + n]);
            residual(width, &mut upper, &mut current, mode as u32, start_x, start_x + max_x, y, exact, &mut residuals);
            for level in 0..=update_up_to {
                let histo = histo_argb(histos, level, mode);
                for &r in &residuals {
                    update_histo(histo, r);
                }
            }
        }
    }
}

/// `GetHistoArgb`: o histograma do modo `mode` no nível de subamostragem `level`.
fn histo_argb(all: &mut [u32], level: usize, mode: usize) -> &mut [u32] {
    let at = (level * NUM_PRED_MODES + mode) * HISTO_SIZE;
    &mut all[at..at + HISTO_SIZE]
}

/// Os histogramas e modos de cada nível de subamostragem de `GetBestPredictorsAndSubSampling`.
struct Levels {
    all_argb: Vec<u32>,
    accumulated: Vec<u32>,
    pred_histos: Vec<u32>,
    modes: Vec<Vec<u32>>,
}

impl Levels {
    /// `GetBestPredictorForTile`: o modo de menor custo do superbloco (`tile_x`, `tile_y`) no nível `level`.
    fn best_predictor_for_tile(&mut self, level: usize, tile_x: usize, tile_y: usize, tiles_per_row: usize) {
        let modes = &mut self.modes[level];
        let left_mode = if tile_x > 0 { (modes[tile_y * tiles_per_row + tile_x - 1] >> 8) & 0xff } else { 0xff };
        let above_mode = if tile_y > 0 { (modes[(tile_y - 1) * tiles_per_row + tile_x] >> 8) & 0xff } else { 0xff };
        let accumulated = &mut self.accumulated[level * HISTO_SIZE..(level + 1) * HISTO_SIZE];
        let histo = |mode: usize| (level * NUM_PRED_MODES + mode) * HISTO_SIZE;
        let (mut best_diff, mut best_mode) = (i64::MAX, 0usize);
        for mode in 0..NUM_PRED_MODES {
            let cur = prediction_cost_spatial(accumulated, &self.all_argb[histo(mode)..histo(mode) + HISTO_SIZE], mode as u32, left_mode, above_mode);
            if cur < best_diff {
                best_diff = cur;
                best_mode = mode;
            }
        }
        for (a, h) in accumulated.iter_mut().zip(&self.all_argb[histo(best_mode)..histo(best_mode) + HISTO_SIZE]) {
            *a += h;
        }
        modes[tile_y * tiles_per_row + tile_x] = ARGB_BLACK | ((best_mode as u32) << 8);
        self.pred_histos[level * NUM_PRED_MODES + best_mode] += 1;
    }
}

/// `VP8LShannonEntropy`.
fn shannon_entropy(x: &[u32]) -> u64 {
    let (mut sum, mut retval) = (0u32, 0u64);
    for &v in x.iter().filter(|&&v| v != 0) {
        sum += v;
        retval += fast_slog2(v);
    }
    fast_slog2(sum) - retval
}

/// `GetBestPredictorsAndSubSampling`: escolhe o modo de cada bloco em todos os tamanhos de `min_bits` a `max_bits`,
/// percorrendo os blocos em zigue-zague para fechar os superblocos o quanto antes, e fica com o tamanho de menor
/// entropia. Devolve a imagem de modos (já pelo `VP8LOptimizeSampling`) e os bits.
fn best_predictors_and_sub_sampling(width: usize, height: usize, min_bits: u32, max_bits: u32, argb: &[u32], exact: bool) -> (Vec<u32>, u32) {
    let tiles_per_row = sub_sample_size(width, min_bits);
    let tiles_per_col = sub_sample_size(height, min_bits);
    let max_level = (max_bits - min_bits) as usize;
    let mut levels = Levels {
        all_argb: vec![0; (max_level + 1) * NUM_PRED_MODES * HISTO_SIZE],
        accumulated: vec![0; (max_level + 1) * HISTO_SIZE],
        pred_histos: vec![0; (max_level + 1) * NUM_PRED_MODES],
        modes: (0..=max_level).map(|level| vec![0; sub_sample_size(width, min_bits + level as u32) * sub_sample_size(height, min_bits + level as u32)]).collect(),
    };
    // Até o superbloco de lado 2^4 cada resíduo entra direto no histograma dele; acima disso, o histograma do nível
    // de baixo é somado quando ele fecha.
    let update_up_to = (4.min(max_bits).max(min_bits) - min_bits) as usize;
    let max_tile_size = 1usize << max_level;
    let (mut local_x, mut local_y, mut max_tile_x, mut max_tile_y, mut tile_x, mut tile_y) = (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
    while tile_y < tiles_per_col {
        residuals_for_tile(width, height, tile_x, tile_y, min_bits, update_up_to, argb, exact, &mut levels.all_argb);
        let mut level = 0;
        loop {
            levels.best_predictor_for_tile(level, tile_x >> level, tile_y >> level, sub_sample_size(width, min_bits + level as u32));
            if level == max_level {
                break;
            }
            level += 1;
            if level > update_up_to {
                let (below, above) = levels.all_argb.split_at_mut(level * NUM_PRED_MODES * HISTO_SIZE);
                for (a, b) in above[..NUM_PRED_MODES * HISTO_SIZE].iter_mut().zip(&below[(level - 1) * NUM_PRED_MODES * HISTO_SIZE..]) {
                    *a += b;
                }
            }
            let side = 1usize << level;
            if !((tile_x == tiles_per_row - 1 || (local_x + 1) % side == 0) && (tile_y == tiles_per_col - 1 || (local_y + 1) % side == 0)) {
                level -= 1;
                break;
            }
        }
        levels.all_argb[..HISTO_SIZE * NUM_PRED_MODES * (level + 1)].fill(0);
        if level == max_level {
            if tile_x == tiles_per_row - 1 {
                max_tile_x = 0;
                max_tile_y += 1;
            } else {
                max_tile_x += 1;
            }
            local_x = 0;
            local_y = 0;
        } else {
            let (mut coord_x, mut coord_y) = (local_x >> level, local_y >> level);
            if tile_x == tiles_per_row - 1 && coord_x % 2 == 0 {
                coord_y += 1;
            } else if coord_x % 2 == 0 {
                coord_x += 1;
            } else {
                coord_y += 1;
                coord_x -= 1;
            }
            local_x = coord_x << level;
            local_y = coord_y << level;
        }
        tile_x = max_tile_x * max_tile_size + local_x;
        tile_y = max_tile_y * max_tile_size + local_y;
    }
    let (mut best_cost, mut best_level) = (i64::MAX, 0usize);
    for level in 0..=max_level {
        let accumulated = &levels.accumulated[level * HISTO_SIZE..(level + 1) * HISTO_SIZE];
        let mut cost = shannon_entropy(&levels.pred_histos[level * NUM_PRED_MODES..(level + 1) * NUM_PRED_MODES]) as i64;
        for plane in 0..4 {
            cost += shannon_entropy(&accumulated[plane * 256..plane * 256 + 256]) as i64;
        }
        if cost < best_cost {
            best_cost = cost;
            best_level = level;
        }
    }
    let mut modes = std::mem::take(&mut levels.modes[best_level]);
    let best_bits = optimize_sampling(&mut modes, width, height, min_bits + best_level as u32, MAX_TRANSFORM_BITS);
    modes.truncate(sub_sample_size(width, best_bits) * sub_sample_size(height, best_bits));
    (modes, best_bits)
}

/// `VP8LResidualImage`: a imagem de modos (em `ARGB_BLACK | modo << 8`) e os bits do bloco; `argb` passa a ter os
/// resíduos.
pub fn residual_image(width: usize, height: usize, min_bits: u32, max_bits: u32, low_effort: bool, argb: &mut [u32], exact: bool) -> (Vec<u32>, u32) {
    if low_effort {
        let n = sub_sample_size(width, max_bits) * sub_sample_size(height, max_bits);
        let image = vec![ARGB_BLACK | (PRED_LOW_EFFORT << 8); n];
        copy_image_with_prediction(width, height, max_bits, &image, argb, true, exact);
        return (image, max_bits);
    }
    let (modes, best_bits) = best_predictors_and_sub_sampling(width, height, min_bits, max_bits, argb, exact);
    copy_image_with_prediction(width, height, best_bits, &modes, argb, false, exact);
    (modes, best_bits)
}
