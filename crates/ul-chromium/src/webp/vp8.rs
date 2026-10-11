//! O codificador VP8 com perdas do libwebp 1.5.0 (`src/enc/`), no caminho que o Skia do Chromium usa: método 3,
//! preset padrão, um passe, sem alvo de tamanho, sem filtro automático, uma partição, buffer de tokens.
//!
//! Os caminhos que esse modo nunca alcança (trellis, `RefineUsingDistortion`, os laços de busca por tamanho ou PSNR,
//! a coleta de estatísticas do filtro automático) ficam de fora; o resto segue o C linha a linha, inclusive a ordem
//! das decisões e as escolhas de desempate.

use super::bitwriter::Vp8BitWriter;
use super::dsp::{self, Matrix, BPS, C8DC8, C8HE8, C8TM8, C8VE8, DSP_SCAN, I16DC16, I16HE16, I16TM16, I16VE16, I4DC4, I4HD4, I4HE4, I4HU4, I4LD4, I4RD4, I4TM4, I4VE4, I4VL4, I4VR4, PRED_SIZE_ENC, U_OFF_ENC, V_OFF_ENC, YUV_SIZE_ENC, Y_OFF_ENC};
use super::tables::{AC_TABLE, AC_TABLE2, BMODES_PROBA, COEFFS_PROBA0, COEFFS_UPDATE_PROBA, DC_TABLE, ENC_BANDS, ENTROPY_COST, FIXED_COSTS_I4, LEVELS_FROM_DELTA, LEVEL_CODES, LEVEL_FIXED_COSTS};

const NUM_MB_SEGMENTS: usize = 4;
const NUM_TYPES: usize = 4;
const NUM_BANDS: usize = 8;
const NUM_CTX: usize = 3;
const NUM_PROBAS: usize = 11;
const MAX_VARIABLE_LEVEL: usize = 67;
const NUM_BMODES: usize = 10;
const NUM_PRED_MODES: usize = 4;
const MAX_COST: i64 = 0x7f_ffff_ffff_ffff;
const RD_DISTO_MULT: i64 = 256;
const MAX_LF_LEVELS: i32 = 64;
const ERROR_DIFFUSION_QUALITY: f32 = 98.0;

const I16_MODE_OFFSETS: [usize; 4] = [I16DC16, I16TM16, I16VE16, I16HE16];
const UV_MODE_OFFSETS: [usize; 4] = [C8DC8, C8TM8, C8VE8, C8HE8];
const I4_MODE_OFFSETS: [usize; NUM_BMODES] = [I4DC4, I4TM4, I4VE4, I4HE4, I4RD4, I4VR4, I4LD4, I4VL4, I4HD4, I4HU4];
const FIXED_COSTS_UV: [u16; 4] = [302, 984, 439, 642];
const FIXED_COSTS_I16: [u16; 4] = [663, 919, 872, 919];
const WEIGHT_Y: [u16; 16] = [38, 32, 20, 9, 32, 28, 17, 7, 20, 17, 10, 4, 9, 7, 4, 2];
const BIAS_MATRICES: [[u32; 2]; 3] = [[96, 110], [96, 108], [110, 115]];
const FREQ_SHARPENING: [u32; 16] = [0, 30, 60, 90, 30, 60, 90, 90, 60, 90, 90, 90, 90, 90, 90, 90];
const CAT3: &[u8] = &[173, 148, 140];
const CAT4: &[u8] = &[176, 155, 140, 135];
const CAT5: &[u8] = &[180, 157, 141, 134, 130];
const CAT6: &[u8] = &[254, 254, 243, 230, 196, 177, 153, 140, 133, 130, 129];
const TOP_LEFT_I4: [usize; 16] = [17, 21, 25, 29, 13, 17, 21, 25, 9, 13, 17, 21, 5, 9, 13, 17];
const SCAN_UV: [usize; 8] = [0, 4, 4 * BPS, 4 + 4 * BPS, 8, 12, 8 + 4 * BPS, 12 + 4 * BPS];

/// A configuração do `WebPConfig` que o codificador com perdas lê (o preset padrão do libwebp).
#[derive(Clone)]
pub struct Config {
    pub quality: f32,
    pub method: i32,
    pub sns_strength: i32,
    pub filter_strength: i32,
    pub filter_sharpness: i32,
    pub filter_type: i32,
    pub segments: i32,
    pub pass: i32,
}

impl Config {
    /// `WebPConfigPreset(WEBP_PRESET_DEFAULT, quality)` com o método 3 que o Skia pede.
    pub fn skia_lossy(quality: f32) -> Self {
        Config {
            quality,
            method: 3,
            sns_strength: 50,
            filter_strength: 60,
            filter_sharpness: 0,
            filter_type: 1,
            segments: 4,
            pass: 1,
        }
    }
}

/// O `WebPPicture` em YUV 4:2:0 (com o plano de alfa quando há transparência).
pub struct Picture {
    pub width: usize,
    pub height: usize,
    pub y: Vec<u8>,
    pub u: Vec<u8>,
    pub v: Vec<u8>,
    pub a: Option<Vec<u8>>,
}

impl Picture {
    pub fn y_stride(&self) -> usize {
        self.width
    }

    pub fn uv_stride(&self) -> usize {
        self.width.div_ceil(2)
    }
}

#[derive(Clone, Copy, Default)]
struct MbInfo {
    type_: u8,
    uv_mode: u8,
    skip: bool,
    segment: u8,
    alpha: i32,
}

#[derive(Clone, Default)]
struct SegmentInfo {
    y1: Matrix,
    y2: Matrix,
    uv: Matrix,
    alpha: i32,
    beta: i32,
    quant: i32,
    fstrength: i32,
    max_edge: i32,
    min_disto: i32,
    lambda_i16: i32,
    lambda_i4: i32,
    lambda_uv: i32,
    lambda_mode: i32,
    tlambda: i32,
}

type ProbaArray = [[u8; NUM_PROBAS]; NUM_CTX];
type StatsArray = [[u32; NUM_PROBAS]; NUM_CTX];
type CostArray = [[u16; MAX_VARIABLE_LEVEL + 1]; NUM_CTX];

/// `VP8EncProba`.
struct EncProba {
    segments: [u8; 3],
    skip_proba: u8,
    coeffs: [[ProbaArray; NUM_BANDS]; NUM_TYPES],
    stats: [[StatsArray; NUM_BANDS]; NUM_TYPES],
    level_cost: [[CostArray; NUM_BANDS]; NUM_TYPES],
    dirty: bool,
    use_skip_proba: bool,
    nb_skip: i32,
}

fn bit_cost(bit: bool, proba: u8) -> i32 {
    i32::from(if bit { ENTROPY_COST[255 - proba as usize] } else { ENTROPY_COST[proba as usize] })
}

fn level_cost(table: &[u16; MAX_VARIABLE_LEVEL + 1], level: i32) -> i32 {
    i32::from(LEVEL_FIXED_COSTS[level as usize]) + i32::from(table[(level as usize).min(MAX_VARIABLE_LEVEL)])
}

fn record_stats(bit: bool, stats: &mut u32) -> bool {
    let mut p = *stats;
    if p >= 0xfffe_0000 {
        p = ((p + 1) >> 1) & 0x7fff_7fff;
    }
    p += 0x0001_0000 + u32::from(bit);
    *stats = p;
    bit
}

impl EncProba {
    /// `VP8DefaultProbas`.
    fn new() -> Box<Self> {
        Box::new(EncProba {
            segments: [255; 3],
            skip_proba: 0,
            coeffs: COEFFS_PROBA0,
            stats: [[[[0; NUM_PROBAS]; NUM_CTX]; NUM_BANDS]; NUM_TYPES],
            level_cost: [[[[0; MAX_VARIABLE_LEVEL + 1]; NUM_CTX]; NUM_BANDS]; NUM_TYPES],
            dirty: true,
            use_skip_proba: false,
            nb_skip: 0,
        })
    }

    /// `remapped_costs_[ctype][n][ctx]`.
    fn costs(&self, ctype: usize, n: usize, ctx: usize) -> &[u16; MAX_VARIABLE_LEVEL + 1] {
        &self.level_cost[ctype][ENC_BANDS[n] as usize][ctx]
    }

    /// `VP8CalculateLevelCosts`.
    fn calculate_level_costs(&mut self) {
        if !self.dirty {
            return;
        }
        for ctype in 0..NUM_TYPES {
            for band in 0..NUM_BANDS {
                for ctx in 0..NUM_CTX {
                    let p = self.coeffs[ctype][band][ctx];
                    let cost0 = if ctx > 0 { bit_cost(true, p[0]) } else { 0 };
                    let cost_base = bit_cost(true, p[1]) + cost0;
                    let table = &mut self.level_cost[ctype][band][ctx];
                    table[0] = (bit_cost(false, p[1]) + cost0) as u16;
                    for v in 1..=MAX_VARIABLE_LEVEL {
                        table[v] = (cost_base + variable_level_cost(v, &p)) as u16;
                    }
                }
            }
        }
        self.dirty = false;
    }
}

/// `VariableLevelCost`.
fn variable_level_cost(level: usize, probas: &[u8; NUM_PROBAS]) -> i32 {
    let [mut pattern, mut bits] = LEVEL_CODES[level - 1];
    let mut cost = 0;
    let mut i = 2;
    while pattern != 0 {
        if pattern & 1 != 0 {
            cost += bit_cost(bits & 1 != 0, probas[i]);
        }
        bits >>= 1;
        pattern >>= 1;
        i += 1;
    }
    cost
}

/// `VP8ModeScore`.
#[derive(Clone)]
struct ModeScore {
    d: i64,
    sd: i64,
    h: i64,
    r: i64,
    score: i64,
    y_dc_levels: [i16; 16],
    y_ac_levels: [[i16; 16]; 16],
    uv_levels: [[i16; 16]; 8],
    mode_i16: i32,
    modes_i4: [u8; 16],
    mode_uv: i32,
    nz: u32,
    derr: [[i8; 3]; 2],
}

impl Default for ModeScore {
    fn default() -> Self {
        ModeScore {
            d: 0,
            sd: 0,
            h: 0,
            r: 0,
            score: MAX_COST,
            y_dc_levels: [0; 16],
            y_ac_levels: [[0; 16]; 16],
            uv_levels: [[0; 16]; 8],
            mode_i16: 0,
            modes_i4: [0; 16],
            mode_uv: 0,
            nz: 0,
            derr: [[0; 3]; 2],
        }
    }
}

impl ModeScore {
    /// `InitScore`.
    fn init(&mut self) {
        self.d = 0;
        self.sd = 0;
        self.r = 0;
        self.h = 0;
        self.nz = 0;
        self.score = MAX_COST;
    }

    /// `CopyScore`.
    fn copy_score(&mut self, src: &ModeScore) {
        self.d = src.d;
        self.sd = src.sd;
        self.r = src.r;
        self.h = src.h;
        self.nz = src.nz;
        self.score = src.score;
    }

    /// `AddScore`.
    fn add_score(&mut self, src: &ModeScore) {
        self.d += src.d;
        self.sd += src.sd;
        self.r += src.r;
        self.h += src.h;
        self.nz |= src.nz;
        self.score += src.score;
    }

    /// `SetRDScore`.
    fn set_rd_score(&mut self, lambda: i32) {
        self.score = (self.r + self.h) * i64::from(lambda) + RD_DISTO_MULT * (self.d + self.sd);
    }
}

/// `VP8Residual`.
struct Residual<'a> {
    first: usize,
    last: i32,
    coeffs: &'a [i16],
    coeff_type: usize,
}

impl<'a> Residual<'a> {
    /// `VP8InitResidual` com `VP8SetResidualCoeffs`.
    fn new(first: usize, coeff_type: usize, coeffs: &'a [i16]) -> Self {
        let last = (0..16).rev().find(|&n| coeffs[n] != 0).map_or(-1, |n| n as i32);
        Residual { first, last, coeffs, coeff_type }
    }

    /// `GetResidualCost_C`.
    fn cost(&self, proba: &EncProba, ctx0: usize) -> i32 {
        let mut n = self.first;
        let p0 = proba.coeffs[self.coeff_type][n][ctx0][0];
        let mut t = proba.costs(self.coeff_type, n, ctx0);
        let mut cost = if ctx0 == 0 { bit_cost(true, p0) } else { 0 };
        if self.last < 0 {
            return bit_cost(false, p0);
        }
        while (n as i32) < self.last {
            let v = i32::from(self.coeffs[n]).abs();
            let ctx = v.min(2) as usize;
            cost += level_cost(t, v);
            t = proba.costs(self.coeff_type, n + 1, ctx);
            n += 1;
        }
        let v = i32::from(self.coeffs[n]).abs();
        cost += level_cost(t, v);
        if n < 15 {
            let b = ENC_BANDS[n + 1] as usize;
            let ctx = if v == 1 { 1 } else { 2 };
            cost += bit_cost(false, proba.coeffs[self.coeff_type][b][ctx][0]);
        }
        cost
    }
}

/// O buffer de tokens (`VP8TBuffer`): cada token é `(bit << 15) | índice da probabilidade`, ou com
/// `FIXED_PROBA_BIT` a probabilidade constante. As páginas do C são lidas na mesma ordem em que foram escritas.
const FIXED_PROBA_BIT: u16 = 1 << 14;

fn token_id(t: usize, b: usize, ctx: usize) -> u16 {
    (NUM_PROBAS * (ctx + NUM_CTX * (b + NUM_BANDS * t))) as u16
}

struct TokenRecorder<'a> {
    tokens: &'a mut Vec<u16>,
    stats: &'a mut [[StatsArray; NUM_BANDS]; NUM_TYPES],
}

impl TokenRecorder<'_> {
    fn add(&mut self, bit: bool, proba_idx: u16) -> bool {
        self.add_counted(bit, proba_idx, proba_idx)
    }

    /// `AddToken` com a estatística num índice próprio: nas categorias 5 e 6 o libwebp grava o token na
    /// probabilidade 10 e conta a estatística na 9.
    fn add_counted(&mut self, bit: bool, proba_idx: u16, stats_idx: u16) -> bool {
        self.tokens.push((u16::from(bit) << 15) | proba_idx);
        let index = stats_idx as usize;
        let (p, rest) = (index % NUM_PROBAS, index / NUM_PROBAS);
        let (ctx, rest) = (rest % NUM_CTX, rest / NUM_CTX);
        let (b, t) = (rest % NUM_BANDS, rest / NUM_BANDS);
        record_stats(bit, &mut self.stats[t][b][ctx][p]);
        bit
    }

    fn add_constant(&mut self, bit: bool, proba: u16) {
        self.tokens.push((u16::from(bit) << 15) | FIXED_PROBA_BIT | proba);
    }

    /// `VP8RecordCoeffTokens`.
    fn record(&mut self, ctx: usize, res: &Residual) -> bool {
        let coeffs = res.coeffs;
        let t = res.coeff_type;
        let last = res.last;
        let mut n = res.first;
        let mut base_id = token_id(t, n, ctx);
        if !self.add(last >= 0, base_id) {
            return false;
        }
        while n < 16 {
            let c = i32::from(coeffs[n]);
            n += 1;
            let sign = c < 0;
            let v = c.unsigned_abs();
            if !self.add(v != 0, base_id + 1) {
                base_id = token_id(t, ENC_BANDS[n] as usize, 0);
                continue;
            }
            if !self.add(v > 1, base_id + 2) {
                base_id = token_id(t, ENC_BANDS[n] as usize, 1);
            } else {
                if !self.add(v > 4, base_id + 3) {
                    if self.add(v != 2, base_id + 4) {
                        self.add(v == 4, base_id + 5);
                    }
                } else if !self.add(v > 10, base_id + 6) {
                    if !self.add(v > 6, base_id + 7) {
                        self.add_constant(v == 6, 159);
                    } else {
                        self.add_constant(v >= 9, 165);
                        self.add_constant(v & 1 == 0, 145);
                    }
                } else {
                    let mut residue = v - 3;
                    let (mut mask, tab): (u32, &[u8]) = if residue < (8 << 1) {
                        self.add(false, base_id + 8);
                        self.add(false, base_id + 9);
                        residue -= 8;
                        (1 << 2, CAT3)
                    } else if residue < (8 << 2) {
                        self.add(false, base_id + 8);
                        self.add(true, base_id + 9);
                        residue -= 8 << 1;
                        (1 << 3, CAT4)
                    } else if residue < (8 << 3) {
                        self.add(true, base_id + 8);
                        self.add_counted(false, base_id + 10, base_id + 9);
                        residue -= 8 << 2;
                        (1 << 4, CAT5)
                    } else {
                        self.add(true, base_id + 8);
                        self.add_counted(true, base_id + 10, base_id + 9);
                        residue -= 8 << 3;
                        (1 << 10, CAT6)
                    };
                    let mut k = 0;
                    while mask != 0 {
                        self.add_constant(residue & mask != 0, u16::from(tab[k]));
                        k += 1;
                        mask >>= 1;
                    }
                }
                base_id = token_id(t, ENC_BANDS[n] as usize, 2);
            }
            self.add_constant(sign, 128);
            if n == 16 || !self.add(n as i32 <= last, base_id) {
                return true;
            }
        }
        true
    }
}

/// O estado do `VP8EncIterator` que não é do codificador.
struct Iter {
    x: usize,
    y: usize,
    yuv_in: Vec<u8>,
    yuv_out: Vec<u8>,
    yuv_out2: Vec<u8>,
    yuv_p: Vec<u8>,
    /// `y_left_[-1..16]`, `u_left_[-1..8]`, `v_left_[-1..8]`: o índice 0 é o `[-1]` do C.
    y_left: [u8; 17],
    u_left: [u8; 9],
    v_left: [u8; 9],
    /// A borda de cima vinda do `VP8IteratorImport` da análise (`tmp_32`); `None` usa as bordas do codificador.
    top_tmp: Option<[u8; 32]>,
    i4_boundary: [u8; 37],
    i4_top: usize,
    i4: usize,
    top_nz: [i32; 9],
    left_nz: [i32; 9],
    left_derr: [[i8; 2]; 2],
    count_down: i32,
}

/// `VP8Encoder`.
pub struct Encoder<'a> {
    config: &'a Config,
    pic: &'a Picture,
    filter_simple: bool,
    filter_level: i32,
    filter_sharpness: i32,
    num_segments: usize,
    update_map: bool,
    segment_size: i32,
    profile: i32,
    mb_w: usize,
    mb_h: usize,
    preds_w: usize,
    part0: Vp8BitWriter,
    part1: Vp8BitWriter,
    tokens: Vec<u16>,
    dqm: [SegmentInfo; NUM_MB_SEGMENTS],
    base_quant: i32,
    alpha: i32,
    uv_alpha: i32,
    dq_uv_dc: i32,
    dq_uv_ac: i32,
    proba: Box<EncProba>,
    max_i4_header_bits: i32,
    mb_info: Vec<MbInfo>,
    preds: Vec<u8>,
    nz: Vec<u32>,
    y_top: Vec<u8>,
    uv_top: Vec<u8>,
    top_derr: Option<Vec<[[i8; 2]; 2]>>,
}

const PREDS_ORIGIN_EXTRA: usize = 1;

impl<'a> Encoder<'a> {
    /// `InitVP8Encoder`.
    fn new(config: &'a Config, pic: &'a Picture) -> Self {
        let use_filter = config.filter_strength > 0;
        let mb_w = pic.width.div_ceil(16);
        let mb_h = pic.height.div_ceil(16);
        let preds_w = 4 * mb_w + 1;
        let preds_h = 4 * mb_h + 1;
        let limit = 100i64;
        let mut enc = Encoder {
            config,
            pic,
            filter_simple: true,
            filter_level: 0,
            filter_sharpness: 0,
            num_segments: config.segments as usize,
            update_map: config.segments > 1,
            segment_size: 0,
            profile: if use_filter { if config.filter_type == 1 { 0 } else { 1 } } else { 2 },
            mb_w,
            mb_h,
            preds_w,
            part0: Vp8BitWriter::new(),
            part1: Vp8BitWriter::new(),
            tokens: Vec::new(),
            dqm: Default::default(),
            base_quant: 0,
            alpha: 0,
            uv_alpha: 0,
            dq_uv_dc: 0,
            dq_uv_ac: 0,
            proba: EncProba::new(),
            max_i4_header_bits: (256 * 16 * 16 * (limit * limit) / (100 * 100)) as i32,
            mb_info: vec![MbInfo::default(); mb_w * mb_h],
            preds: vec![0; preds_w * preds_h + PREDS_ORIGIN_EXTRA],
            nz: vec![0; mb_w + 1],
            y_top: vec![0; mb_w * 16],
            uv_top: vec![0; mb_w * 16],
            top_derr: (config.quality <= ERROR_DIFFUSION_QUALITY || config.pass > 1).then(|| vec![[[0; 2]; 2]; mb_w]),
        };
        // `ResetBoundaryPredictions`: a linha de cima e a coluna da esquerda dos modos valem B_DC_PRED (0); o resto
        // do buffer também começa em zero, como o `memset` da alocação.
        enc.preds.fill(0);
        enc
    }

    /// O índice de `preds_` para o macrobloco (x, y): a origem do C fica em `1 + preds_w`.
    fn preds_index(&self, x: usize, y: usize) -> usize {
        1 + self.preds_w + y * 4 * self.preds_w + x * 4
    }

    // ----------------------------------------------------------------------------------------------------------
    // Iterador (`iterator_enc.c`).

    fn iter_new(&self) -> Iter {
        let mut it = Iter {
            x: 0,
            y: 0,
            yuv_in: vec![0; YUV_SIZE_ENC],
            yuv_out: vec![0; YUV_SIZE_ENC],
            yuv_out2: vec![0; YUV_SIZE_ENC],
            yuv_p: vec![0; PRED_SIZE_ENC],
            y_left: [0; 17],
            u_left: [0; 9],
            v_left: [0; 9],
            top_tmp: None,
            i4_boundary: [0; 37],
            i4_top: 0,
            i4: 0,
            top_nz: [0; 9],
            left_nz: [0; 9],
            left_derr: [[0; 2]; 2],
            count_down: 0,
        };
        self.iter_set_row(&mut it, 0);
        it.count_down = (self.mb_w * self.mb_h) as i32;
        it
    }

    /// `VP8IteratorReset` (dentro do `VP8IteratorInit`): também o `InitTop`.
    fn iter_init(&mut self) -> Iter {
        let it = self.iter_new();
        self.y_top.fill(127);
        self.uv_top.fill(127);
        self.nz[1..].fill(0);
        if let Some(top) = &mut self.top_derr {
            top.fill([[0; 2]; 2]);
        }
        it
    }

    fn init_left(&self, it: &mut Iter) {
        let corner = if it.y > 0 { 129 } else { 127 };
        it.y_left[0] = corner;
        it.u_left[0] = corner;
        it.v_left[0] = corner;
        it.y_left[1..].fill(129);
        it.u_left[1..].fill(129);
        it.v_left[1..].fill(129);
        it.left_nz[8] = 0;
        if self.top_derr.is_some() {
            it.left_derr = [[0; 2]; 2];
        }
    }

    fn iter_set_row(&self, it: &mut Iter, y: usize) {
        it.x = 0;
        it.y = y;
        it.top_tmp = None;
        self.init_left(it);
    }

    /// `VP8IteratorNext`.
    fn iter_next(&self, it: &mut Iter) -> bool {
        it.x += 1;
        if it.x == self.mb_w {
            let y = it.y + 1;
            self.iter_set_row(it, y);
        }
        it.count_down -= 1;
        it.count_down > 0
    }

    fn mb(&self, it: &Iter) -> &MbInfo {
        &self.mb_info[it.y * self.mb_w + it.x]
    }

    fn mb_mut(&mut self, it: &Iter) -> &mut MbInfo {
        let w = self.mb_w;
        &mut self.mb_info[it.y * w + it.x]
    }

    /// A borda de cima de luma (16 amostras, mais 4 do vizinho quando existe).
    fn y_top<'b>(&'b self, it: &'b Iter) -> &'b [u8] {
        match &it.top_tmp {
            Some(tmp) => &tmp[..16],
            None => &self.y_top[it.x * 16..],
        }
    }

    fn uv_top<'b>(&'b self, it: &'b Iter) -> &'b [u8] {
        match &it.top_tmp {
            Some(tmp) => &tmp[16..32],
            None => &self.uv_top[it.x * 16..it.x * 16 + 16],
        }
    }

    /// `VP8IteratorImport`. `with_borders` é o `tmp_32` da análise: as bordas vêm da imagem de origem.
    fn iter_import(&self, it: &mut Iter, with_borders: bool) {
        let pic = self.pic;
        let (x, y) = (it.x, it.y);
        let (ys, uvs) = (pic.y_stride(), pic.uv_stride());
        let y_at = (y * ys + x) * 16;
        let uv_at = (y * uvs + x) * 8;
        let w = (pic.width - x * 16).min(16);
        let h = (pic.height - y * 16).min(16);
        let uv_w = w.div_ceil(2);
        let uv_h = h.div_ceil(2);
        import_block(&pic.y, y_at, ys, &mut it.yuv_in, Y_OFF_ENC, w, h, 16);
        import_block(&pic.u, uv_at, uvs, &mut it.yuv_in, U_OFF_ENC, uv_w, uv_h, 8);
        import_block(&pic.v, uv_at, uvs, &mut it.yuv_in, V_OFF_ENC, uv_w, uv_h, 8);
        if !with_borders {
            return;
        }
        if x == 0 {
            self.init_left(it);
        } else {
            if y == 0 {
                it.y_left[0] = 127;
                it.u_left[0] = 127;
                it.v_left[0] = 127;
            } else {
                it.y_left[0] = pic.y[y_at - 1 - ys];
                it.u_left[0] = pic.u[uv_at - 1 - uvs];
                it.v_left[0] = pic.v[uv_at - 1 - uvs];
            }
            import_line(&pic.y, y_at - 1, ys, &mut it.y_left[1..], h, 16);
            import_line(&pic.u, uv_at - 1, uvs, &mut it.u_left[1..], uv_h, 8);
            import_line(&pic.v, uv_at - 1, uvs, &mut it.v_left[1..], uv_h, 8);
        }
        let mut tmp = [127u8; 32];
        if y > 0 {
            import_line(&pic.y, y_at - ys, 1, &mut tmp[..16], w, 16);
            import_line(&pic.u, uv_at - uvs, 1, &mut tmp[16..24], uv_w, 8);
            import_line(&pic.v, uv_at - uvs, 1, &mut tmp[24..32], uv_w, 8);
        }
        it.top_tmp = Some(tmp);
    }

    /// `VP8IteratorNzToBytes`.
    fn nz_to_bytes(&self, it: &mut Iter) {
        let tnz = self.nz[1 + it.x];
        let lnz = self.nz[it.x];
        let bit = |nz: u32, n: u32| ((nz >> n) & 1) as i32;
        for (k, n) in [12, 13, 14, 15, 18, 19, 22, 23, 24].into_iter().enumerate() {
            it.top_nz[k] = bit(tnz, n);
        }
        for (k, n) in [3, 7, 11, 15, 17, 19, 21, 23].into_iter().enumerate() {
            it.left_nz[k] = bit(lnz, n);
        }
    }

    /// `VP8IteratorBytesToNz`.
    fn bytes_to_nz(&mut self, it: &Iter) {
        let (t, l) = (&it.top_nz, &it.left_nz);
        let nz = (t[0] << 12) | (t[1] << 13) | (t[2] << 14) | (t[3] << 15) | (t[4] << 18) | (t[5] << 19) | (t[6] << 22) | (t[7] << 23) | (t[8] << 24)
            | (l[0] << 3) | (l[1] << 7) | (l[2] << 11) | (l[4] << 17) | (l[6] << 21);
        self.nz[1 + it.x] = nz as u32;
    }

    /// `VP8IteratorSaveBoundary`.
    fn save_boundary(&mut self, it: &mut Iter) {
        let (x, y) = (it.x, it.y);
        let out = &it.yuv_out;
        if x < self.mb_w - 1 {
            for i in 0..16 {
                it.y_left[1 + i] = out[Y_OFF_ENC + 15 + i * BPS];
            }
            for i in 0..8 {
                it.u_left[1 + i] = out[U_OFF_ENC + 7 + i * BPS];
                it.v_left[1 + i] = out[U_OFF_ENC + 15 + i * BPS];
            }
            it.y_left[0] = self.y_top[x * 16 + 15];
            it.u_left[0] = self.uv_top[x * 16 + 7];
            it.v_left[0] = self.uv_top[x * 16 + 8 + 7];
        }
        if y < self.mb_h - 1 {
            self.y_top[x * 16..x * 16 + 16].copy_from_slice(&out[Y_OFF_ENC + 15 * BPS..Y_OFF_ENC + 15 * BPS + 16]);
            self.uv_top[x * 16..x * 16 + 16].copy_from_slice(&out[U_OFF_ENC + 7 * BPS..U_OFF_ENC + 7 * BPS + 16]);
        }
    }

    fn set_intra16_mode(&mut self, it: &Iter, mode: u8) {
        let base = self.preds_index(it.x, it.y);
        for y in 0..4 {
            self.preds[base + y * self.preds_w..base + y * self.preds_w + 4].fill(mode);
        }
        self.mb_mut(it).type_ = 1;
    }

    fn set_intra4_mode(&mut self, it: &Iter, modes: &[u8; 16]) {
        let base = self.preds_index(it.x, it.y);
        for y in 0..4 {
            self.preds[base + y * self.preds_w..base + y * self.preds_w + 4].copy_from_slice(&modes[y * 4..y * 4 + 4]);
        }
        self.mb_mut(it).type_ = 0;
    }

    /// `VP8IteratorStartI4`.
    fn start_i4(&self, it: &mut Iter) {
        it.i4 = 0;
        it.i4_top = TOP_LEFT_I4[0];
        for i in 0..17 {
            it.i4_boundary[i] = it.y_left[16 - i];
        }
        let top: Vec<u8> = self.y_top(it).iter().take(20).copied().collect();
        it.i4_boundary[17..33].copy_from_slice(&top[..16]);
        if it.x < self.mb_w - 1 {
            it.i4_boundary[33..37].copy_from_slice(&top[16..20]);
        } else {
            let last = it.i4_boundary[17 + 15];
            it.i4_boundary[33..37].fill(last);
        }
        self.nz_to_bytes(it);
    }

    /// `VP8IteratorRotateI4`: `yuv_out` é o buffer com os blocos reconstruídos (o passo é `BPS`).
    fn rotate_i4(it: &mut Iter, yuv_out: &[u8], yuv_off: usize) -> bool {
        let blk = yuv_off + DSP_SCAN[it.i4];
        let top = it.i4_top;
        for i in 0..4 {
            it.i4_boundary[top - 4 + i] = yuv_out[blk + i + 3 * BPS];
        }
        if it.i4 & 3 != 3 {
            for i in 0..3 {
                it.i4_boundary[top + i] = yuv_out[blk + 3 + (2 - i) * BPS];
            }
        } else {
            for i in 0..4 {
                it.i4_boundary[top + i] = it.i4_boundary[top + i + 4];
            }
        }
        it.i4 += 1;
        if it.i4 == 16 {
            return false;
        }
        it.i4_top = TOP_LEFT_I4[it.i4];
        true
    }

    // ----------------------------------------------------------------------------------------------------------
    // Predições (`quant_enc.c`).

    fn make_luma16_preds(&self, it: &mut Iter) {
        let left = (it.x > 0).then(|| it.y_left);
        let top: Option<Vec<u8>> = (it.y > 0).then(|| self.y_top(it)[..16].to_vec());
        dsp::intra16_preds(&mut it.yuv_p, left.as_ref().map(|l| &l[..]), top.as_deref());
    }

    fn make_chroma8_preds(&self, it: &mut Iter) {
        let (u, v) = (it.u_left, it.v_left);
        let left = it.x > 0;
        let top: Option<Vec<u8>> = (it.y > 0).then(|| self.uv_top(it).to_vec());
        dsp::intra_chroma_preds(&mut it.yuv_p, left.then_some(&u[..]), left.then_some(&v[..]), top.as_deref());
    }

    // ----------------------------------------------------------------------------------------------------------
    // Análise (`analysis_enc.c`).

    /// `VP8EncAnalyze`.
    fn analyze(&mut self) {
        let do_segments = self.num_segments > 1 || self.config.method <= 1;
        if do_segments {
            let total_mb = (self.mb_w * self.mb_h) as i32;
            let mut alphas = [0i32; 256];
            let (mut alpha, mut uv_alpha) = (0, 0);
            let mut it = self.iter_new();
            loop {
                self.iter_import(&mut it, true);
                self.mb_analyze(&mut it, &mut alphas, &mut alpha, &mut uv_alpha);
                if !self.iter_next(&mut it) {
                    break;
                }
            }
            self.alpha = alpha / total_mb;
            self.uv_alpha = uv_alpha / total_mb;
            self.assign_segments(&alphas);
        } else {
            for mb in &mut self.mb_info {
                *mb = MbInfo { type_: 1, ..MbInfo::default() };
            }
            self.dqm[0].alpha = 0;
            self.dqm[0].beta = 0;
            self.alpha = 0;
            self.uv_alpha = 0;
        }
    }

    fn mb_analyze(&mut self, it: &mut Iter, alphas: &mut [i32; 256], alpha: &mut i32, uv_alpha: &mut i32) {
        self.set_intra16_mode(it, 0);
        self.mb_mut(it).skip = false;
        self.mb_mut(it).segment = 0;
        let best = if self.config.method <= 1 { self.fast_mb_analyze(it) } else { self.analyze_best_intra16(it) };
        let best_uv = self.analyze_best_uv(it);
        let best = (3 * best + best_uv + 2) >> 2;
        let best = (255 - best).clamp(0, 255);
        alphas[best as usize] += 1;
        self.mb_mut(it).alpha = best;
        *alpha += best;
        *uv_alpha += best_uv;
    }

    fn fast_mb_analyze(&mut self, it: &mut Iter) -> i32 {
        let q = self.config.quality as i32;
        let threshold = (8 + (17 - 8) * q / 100) as u32;
        let mut dc = [0u32; 16];
        for k in (0..16).step_by(4) {
            dc[k..k + 4].copy_from_slice(&dsp::mean16x4(&it.yuv_in, Y_OFF_ENC + k * BPS));
        }
        let (mut m, mut m2) = (0u32, 0u32);
        for &d in &dc {
            m = m.wrapping_add(d);
            m2 = m2.wrapping_add(d.wrapping_mul(d));
        }
        if threshold.wrapping_mul(m2) < m.wrapping_mul(m) {
            self.set_intra16_mode(it, 0);
        } else {
            self.set_intra4_mode(it, &[0; 16]);
        }
        0
    }

    fn analyze_best_intra16(&mut self, it: &mut Iter) -> i32 {
        let (mut best_alpha, mut best_mode) = (-1, 0);
        self.make_luma16_preds(it);
        for (mode, &off) in I16_MODE_OFFSETS.iter().enumerate().take(2) {
            let histo = dsp::collect_histogram(&it.yuv_in, Y_OFF_ENC, &it.yuv_p, off, 0, 16);
            let alpha = get_alpha(&histo);
            if alpha > best_alpha {
                best_alpha = alpha;
                best_mode = mode;
            }
        }
        self.set_intra16_mode(it, best_mode as u8);
        best_alpha
    }

    fn analyze_best_uv(&mut self, it: &mut Iter) -> i32 {
        let (mut best_alpha, mut smallest_alpha, mut best_mode) = (-1, 0, 0);
        self.make_chroma8_preds(it);
        for (mode, &off) in UV_MODE_OFFSETS.iter().enumerate().take(2) {
            let histo = dsp::collect_histogram(&it.yuv_in, U_OFF_ENC, &it.yuv_p, off, 16, 24);
            let alpha = get_alpha(&histo);
            if alpha > best_alpha {
                best_alpha = alpha;
            }
            if mode == 0 || alpha < smallest_alpha {
                smallest_alpha = alpha;
                best_mode = mode;
            }
        }
        self.mb_mut(it).uv_mode = best_mode as u8;
        best_alpha
    }

    /// `AssignSegments`.
    fn assign_segments(&mut self, alphas: &[i32; 256]) {
        let nb = self.num_segments.min(NUM_MB_SEGMENTS);
        let mut centers = [0i32; NUM_MB_SEGMENTS];
        let mut weighted_average = 0;
        let mut map = [0usize; 256];
        let mut n = 0;
        while n <= 255 && alphas[n] == 0 {
            n += 1;
        }
        let min_a = n as i32;
        let mut n = 255;
        while n > min_a as usize && alphas[n] == 0 {
            n -= 1;
        }
        let max_a = n as i32;
        let range_a = max_a - min_a;
        let mut n = 1;
        for center in centers.iter_mut().take(nb) {
            *center = min_a + (n * range_a) / (2 * nb as i32);
            n += 2;
        }
        for _ in 0..6 {
            let mut accum = [0i32; NUM_MB_SEGMENTS];
            let mut dist_accum = [0i32; NUM_MB_SEGMENTS];
            let mut n = 0;
            for a in min_a..=max_a {
                if alphas[a as usize] != 0 {
                    while n + 1 < nb && (a - centers[n + 1]).abs() < (a - centers[n]).abs() {
                        n += 1;
                    }
                    map[a as usize] = n;
                    dist_accum[n] += a * alphas[a as usize];
                    accum[n] += alphas[a as usize];
                }
            }
            let mut displaced = 0;
            weighted_average = 0;
            let mut total_weight = 0;
            for n in 0..nb {
                if accum[n] != 0 {
                    let new_center = (dist_accum[n] + accum[n] / 2) / accum[n];
                    displaced += (centers[n] - new_center).abs();
                    centers[n] = new_center;
                    weighted_average += new_center * accum[n];
                    total_weight += accum[n];
                }
            }
            weighted_average = (weighted_average + total_weight / 2) / total_weight;
            if displaced < 5 {
                break;
            }
        }
        for mb in &mut self.mb_info {
            let alpha = mb.alpha as usize;
            mb.segment = map[alpha] as u8;
            mb.alpha = centers[map[alpha]];
        }
        // `preprocessing & 1` (suavizar o mapa) vale zero no preset padrão.
        self.set_segment_alphas(&centers, weighted_average);
    }

    /// `SetSegmentAlphas`.
    fn set_segment_alphas(&mut self, centers: &[i32; NUM_MB_SEGMENTS], mid: i32) {
        let nb = self.num_segments;
        let (mut min, mut max) = (centers[0], centers[0]);
        if nb > 1 {
            for &c in centers.iter().take(nb) {
                min = min.min(c);
                max = max.max(c);
            }
        }
        if max == min {
            max = min + 1;
        }
        for n in 0..nb {
            let alpha = 255 * (centers[n] - mid) / (max - min);
            let beta = 255 * (centers[n] - min) / (max - min);
            self.dqm[n].alpha = alpha.clamp(-127, 127);
            self.dqm[n].beta = beta.clamp(0, 255);
        }
    }

    // ----------------------------------------------------------------------------------------------------------
    // Quantização (`quant_enc.c`).

    /// `VP8SetSegmentParams`.
    fn set_segment_params(&mut self, quality: f32) {
        let num_segments = self.num_segments;
        let amp = 0.9 * f64::from(self.config.sns_strength) / 100.0 / 128.0;
        let q = f64::from(quality) / 100.0;
        let c_base = quality_to_compression(q);
        for i in 0..num_segments {
            let expn = 1.0 - amp * f64::from(self.dqm[i].alpha);
            let c = c_base.powf(expn);
            let q = (127.0 * (1.0 - c)) as i32;
            self.dqm[i].quant = q.clamp(0, 127);
        }
        self.base_quant = self.dqm[0].quant;
        for i in num_segments..NUM_MB_SEGMENTS {
            self.dqm[i].quant = self.base_quant;
        }
        let sns = self.config.sns_strength;
        let mut dq_uv_ac = (self.uv_alpha - 64) * (6 - -4) / (100 - 30);
        dq_uv_ac = dq_uv_ac * sns / 100;
        dq_uv_ac = dq_uv_ac.clamp(-4, 6);
        let dq_uv_dc = (-4 * sns / 100).clamp(-15, 15);
        self.dq_uv_dc = dq_uv_dc;
        self.dq_uv_ac = dq_uv_ac;
        self.setup_filter_strength();
        if num_segments > 1 {
            self.simplify_segments();
        }
        self.setup_matrices();
    }

    /// `SetupFilterStrength`.
    fn setup_filter_strength(&mut self) {
        let level0 = 5 * self.config.filter_strength;
        for i in 0..NUM_MB_SEGMENTS {
            let m = &mut self.dqm[i];
            let qstep = i32::from(AC_TABLE[m.quant.clamp(0, 127) as usize]) >> 2;
            let base_strength = filter_strength_from_delta(self.filter_sharpness, qstep);
            let f = base_strength * level0 / (256 + m.beta);
            m.fstrength = if f < 2 { 0 } else { f.min(63) };
        }
        self.filter_level = self.dqm[0].fstrength;
        self.filter_simple = self.config.filter_type == 0;
        self.filter_sharpness = self.config.filter_sharpness;
    }

    /// `SimplifySegments`.
    fn simplify_segments(&mut self) {
        let mut map = [0u8, 1, 2, 3];
        let num_segments = self.num_segments.min(NUM_MB_SEGMENTS);
        let mut num_final = 1;
        for s1 in 1..num_segments {
            let mut found = None;
            for s2 in 0..num_final {
                if self.dqm[s1].quant == self.dqm[s2].quant && self.dqm[s1].fstrength == self.dqm[s2].fstrength {
                    found = Some(s2);
                    break;
                }
            }
            map[s1] = found.unwrap_or(num_final) as u8;
            if found.is_none() {
                if num_final != s1 {
                    self.dqm[num_final] = self.dqm[s1].clone();
                }
                num_final += 1;
            }
        }
        if num_final < num_segments {
            for mb in &mut self.mb_info {
                mb.segment = map[mb.segment as usize];
            }
            self.num_segments = num_final;
            for i in num_final..num_segments {
                self.dqm[i] = self.dqm[num_final - 1].clone();
            }
        }
    }

    /// `SetupMatrices`.
    fn setup_matrices(&mut self) {
        let tlambda_scale = if self.config.method >= 4 { self.config.sns_strength } else { 0 };
        for i in 0..self.num_segments {
            let (dq_uv_dc, dq_uv_ac) = (self.dq_uv_dc, self.dq_uv_ac);
            let m = &mut self.dqm[i];
            let q = m.quant;
            m.y1.q[0] = u16::from(DC_TABLE[q.clamp(0, 127) as usize]);
            m.y1.q[1] = AC_TABLE[q.clamp(0, 127) as usize];
            m.y2.q[0] = u16::from(DC_TABLE[q.clamp(0, 127) as usize]) * 2;
            m.y2.q[1] = AC_TABLE2[q.clamp(0, 127) as usize];
            m.uv.q[0] = u16::from(DC_TABLE[(q + dq_uv_dc).clamp(0, 117) as usize]);
            m.uv.q[1] = AC_TABLE[(q + dq_uv_ac).clamp(0, 127) as usize];
            let q_i4 = expand_matrix(&mut m.y1, 0);
            let q_i16 = expand_matrix(&mut m.y2, 1);
            let q_uv = expand_matrix(&mut m.uv, 2);
            m.lambda_i4 = ((3 * q_i4 * q_i4) >> 7).max(1);
            m.lambda_i16 = (3 * q_i16 * q_i16).max(1);
            m.lambda_uv = ((3 * q_uv * q_uv) >> 6).max(1);
            m.lambda_mode = ((q_i4 * q_i4) >> 7).max(1);
            m.tlambda = ((tlambda_scale * q_i4) >> 5).max(1);
            m.min_disto = 20 * i32::from(m.y1.q[0]);
            m.max_edge = 0;
        }
    }

    fn dqm_of(&self, it: &Iter) -> &SegmentInfo {
        &self.dqm[self.mb(it).segment as usize]
    }

    /// `ReconstructIntra16`: escreve a reconstrução em `out` (passo `BPS`) e devolve o padrão de não nulos.
    fn reconstruct_intra16(&self, it: &Iter, rd: &mut ModeScore, out: &mut [u8], mode: usize) -> u32 {
        let reference = &it.yuv_p;
        let ref_off = I16_MODE_OFFSETS[mode];
        let dqm = self.dqm_of(it);
        let mut nz = 0u32;
        let mut tmp = [[0i16; 16]; 16];
        let mut flat = [0i16; 256];
        for n in (0..16).step_by(2) {
            let mut pair = [0i16; 32];
            dsp::ftransform2(&it.yuv_in, Y_OFF_ENC + DSP_SCAN[n], reference, ref_off + DSP_SCAN[n], &mut pair);
            tmp[n].copy_from_slice(&pair[..16]);
            tmp[n + 1].copy_from_slice(&pair[16..]);
        }
        for (n, block) in tmp.iter().enumerate() {
            flat[n * 16..n * 16 + 16].copy_from_slice(block);
        }
        let mut dc_tmp = [0i16; 16];
        dsp::ftransform_wht(&flat, &mut dc_tmp);
        nz |= (dsp::quantize_block(&mut dc_tmp, &mut rd.y_dc_levels, &dqm.y2) as u32) << 24;
        for n in (0..16).step_by(2) {
            tmp[n][0] = 0;
            tmp[n + 1][0] = 0;
            let mut pair = [0i16; 32];
            pair[..16].copy_from_slice(&tmp[n]);
            pair[16..].copy_from_slice(&tmp[n + 1]);
            let mut levels = [0i16; 32];
            nz |= dsp::quantize_2blocks(&mut pair, &mut levels, &dqm.y1) << n;
            tmp[n].copy_from_slice(&pair[..16]);
            tmp[n + 1].copy_from_slice(&pair[16..]);
            rd.y_ac_levels[n].copy_from_slice(&levels[..16]);
            rd.y_ac_levels[n + 1].copy_from_slice(&levels[16..]);
        }
        // `VP8TransformWHT`: os DCs voltam para a posição 0 de cada bloco.
        transform_wht(&dc_tmp, &mut tmp);
        for n in (0..16).step_by(2) {
            let mut pair = [0i16; 32];
            pair[..16].copy_from_slice(&tmp[n]);
            pair[16..].copy_from_slice(&tmp[n + 1]);
            dsp::itransform(reference, ref_off + DSP_SCAN[n], &pair, out, Y_OFF_ENC + DSP_SCAN[n], true);
        }
        nz
    }

    /// `ReconstructIntra4`: a reconstrução sai num bloco 4x4 próprio (passo `BPS`).
    fn reconstruct_intra4(&self, it: &Iter, levels: &mut [i16; 16], src_off: usize, mode: usize, out: &mut [u8; 4 * BPS]) -> u32 {
        let ref_off = I4_MODE_OFFSETS[mode];
        let dqm = self.dqm_of(it);
        let mut tmp = [0i16; 16];
        dsp::ftransform(&it.yuv_in, src_off, &it.yuv_p, ref_off, &mut tmp);
        let nz = dsp::quantize_block(&mut tmp, levels, &dqm.y1);
        dsp::itransform(&it.yuv_p, ref_off, &tmp, out, 0, false);
        u32::from(nz)
    }

    /// `ReconstructUV`: a reconstrução sai em `out` (16x8, passo `BPS`, a partir de 0).
    fn reconstruct_uv(&self, it: &Iter, rd: &mut ModeScore, out: &mut [u8], mode: usize) -> u32 {
        let ref_off = UV_MODE_OFFSETS[mode];
        let dqm = self.dqm_of(it);
        let mut nz = 0u32;
        let mut tmp = [[0i16; 16]; 8];
        for n in (0..8).step_by(2) {
            let mut pair = [0i16; 32];
            dsp::ftransform2(&it.yuv_in, U_OFF_ENC + SCAN_UV[n], &it.yuv_p, ref_off + SCAN_UV[n], &mut pair);
            tmp[n].copy_from_slice(&pair[..16]);
            tmp[n + 1].copy_from_slice(&pair[16..]);
        }
        if let Some(top_derr) = &self.top_derr {
            correct_dc_values(&top_derr[it.x], &it.left_derr, &dqm.uv, &mut tmp, rd);
        }
        for n in (0..8).step_by(2) {
            let mut pair = [0i16; 32];
            pair[..16].copy_from_slice(&tmp[n]);
            pair[16..].copy_from_slice(&tmp[n + 1]);
            let mut levels = [0i16; 32];
            nz |= dsp::quantize_2blocks(&mut pair, &mut levels, &dqm.uv) << n;
            tmp[n].copy_from_slice(&pair[..16]);
            tmp[n + 1].copy_from_slice(&pair[16..]);
            rd.uv_levels[n].copy_from_slice(&levels[..16]);
            rd.uv_levels[n + 1].copy_from_slice(&levels[16..]);
        }
        for n in (0..8).step_by(2) {
            let mut pair = [0i16; 32];
            pair[..16].copy_from_slice(&tmp[n]);
            pair[16..].copy_from_slice(&tmp[n + 1]);
            dsp::itransform(&it.yuv_p, ref_off + SCAN_UV[n], &pair, out, SCAN_UV[n], true);
        }
        nz << 16
    }

    /// `VP8GetCostLuma16`.
    fn cost_luma16(&self, it: &mut Iter, rd: &ModeScore) -> i32 {
        self.nz_to_bytes(it);
        let mut r = Residual::new(0, 1, &rd.y_dc_levels).cost(&self.proba, (it.top_nz[8] + it.left_nz[8]) as usize);
        for y in 0..4 {
            for x in 0..4 {
                let ctx = (it.top_nz[x] + it.left_nz[y]) as usize;
                let res = Residual::new(1, 0, &rd.y_ac_levels[x + y * 4]);
                r += res.cost(&self.proba, ctx);
                let nz = i32::from(res.last >= 0);
                it.top_nz[x] = nz;
                it.left_nz[y] = nz;
            }
        }
        r
    }

    /// `VP8GetCostLuma4`.
    fn cost_luma4(&self, it: &Iter, levels: &[i16; 16]) -> i32 {
        let (x, y) = (it.i4 & 3, it.i4 >> 2);
        let ctx = (it.top_nz[x] + it.left_nz[y]) as usize;
        Residual::new(0, 3, levels).cost(&self.proba, ctx)
    }

    /// `VP8GetCostUV`.
    fn cost_uv(&self, it: &mut Iter, rd: &ModeScore) -> i32 {
        self.nz_to_bytes(it);
        let mut r = 0;
        for ch in [0, 2] {
            for y in 0..2 {
                for x in 0..2 {
                    let ctx = (it.top_nz[4 + ch + x] + it.left_nz[4 + ch + y]) as usize;
                    let res = Residual::new(0, 2, &rd.uv_levels[ch * 2 + x + y * 2]);
                    r += res.cost(&self.proba, ctx);
                    let nz = i32::from(res.last >= 0);
                    it.top_nz[4 + ch + x] = nz;
                    it.left_nz[4 + ch + y] = nz;
                }
            }
        }
        r
    }

    /// `PickBestIntra16`.
    fn pick_best_intra16(&mut self, it: &mut Iter, rd: &mut ModeScore) {
        let seg = self.mb(it).segment as usize;
        let lambda = self.dqm[seg].lambda_i16;
        let tlambda = self.dqm[seg].tlambda;
        let mut is_flat = is_flat_source16(&it.yuv_in, Y_OFF_ENC);
        let mut best: Option<ModeScore> = None;
        for mode in 0..NUM_PRED_MODES {
            let mut cur = ModeScore { mode_i16: mode as i32, ..ModeScore::default() };
            let mut scratch = std::mem::take(&mut it.yuv_out2);
            cur.nz = self.reconstruct_intra16(it, &mut cur, &mut scratch, mode);
            cur.d = i64::from(dsp::sse(&it.yuv_in, Y_OFF_ENC, &scratch, Y_OFF_ENC, 16, 16));
            cur.sd = if tlambda != 0 { i64::from(mult_8b(tlambda, dsp::disto16x16(&it.yuv_in, Y_OFF_ENC, &scratch, Y_OFF_ENC, &WEIGHT_Y))) } else { 0 };
            it.yuv_out2 = scratch;
            cur.h = i64::from(FIXED_COSTS_I16[mode]);
            cur.r = i64::from(self.cost_luma16(it, &cur));
            if is_flat {
                is_flat = is_flat_levels(&cur.y_ac_levels, 16, 0);
                if is_flat {
                    cur.d *= 2;
                    cur.sd *= 2;
                }
            }
            cur.set_rd_score(lambda);
            if mode == 0 || cur.score < best.as_ref().map_or(MAX_COST, |b| b.score) {
                best = Some(cur);
                std::mem::swap(&mut it.yuv_out, &mut it.yuv_out2);
            }
        }
        let best = best.unwrap_or_default();
        rd.copy_score(&best);
        rd.mode_i16 = best.mode_i16;
        rd.y_dc_levels = best.y_dc_levels;
        rd.y_ac_levels = best.y_ac_levels;
        rd.set_rd_score(self.dqm[seg].lambda_mode);
        self.set_intra16_mode(it, rd.mode_i16 as u8);
        if (rd.nz & 0x100_ffff) == 0x100_0000 && rd.d > i64::from(self.dqm[seg].min_disto) {
            let dqm = &mut self.dqm[seg];
            let (v0, v1, v2) = (i32::from(rd.y_dc_levels[1]).abs(), i32::from(rd.y_dc_levels[2]).abs(), i32::from(rd.y_dc_levels[4]).abs());
            let max_v = v0.max(v1).max(v2);
            if max_v > dqm.max_edge {
                dqm.max_edge = max_v;
            }
        }
    }

    /// `GetCostModeI4`.
    fn cost_mode_i4(&self, it: &Iter, modes: &[u8; 16]) -> &'static [u16; NUM_BMODES] {
        let pw = self.preds_w;
        let base = self.preds_index(it.x, it.y);
        let (x, y) = (it.i4 & 3, it.i4 >> 2);
        let left = if x == 0 { self.preds[base + y * pw - 1] } else { modes[it.i4 - 1] };
        let top = if y == 0 { self.preds[base - pw + x] } else { modes[it.i4 - 4] };
        &FIXED_COSTS_I4[top as usize][left as usize]
    }

    /// `PickBestIntra4`: devolve se o intra 4x4 venceu.
    fn pick_best_intra4(&mut self, it: &mut Iter, rd: &mut ModeScore) -> bool {
        let seg = self.mb(it).segment as usize;
        let (lambda, tlambda, lambda_mode) = (self.dqm[seg].lambda_i4, self.dqm[seg].tlambda, self.dqm[seg].lambda_mode);
        let mut total_header_bits = 0;
        let mut rd_best = ModeScore::default();
        if self.max_i4_header_bits == 0 {
            return false;
        }
        rd_best.init();
        rd_best.h = 211;
        rd_best.set_rd_score(lambda_mode);
        self.start_i4(it);
        loop {
            let src_off = Y_OFF_ENC + DSP_SCAN[it.i4];
            let mode_costs = self.cost_mode_i4(it, &rd.modes_i4);
            let mut rd_i4 = ModeScore::default();
            rd_i4.init();
            dsp::intra4_preds(&mut it.yuv_p, &it.i4_boundary, it.i4_top);
            let mut best_mode: i32 = -1;
            let mut best_block = [0u8; 4 * BPS];
            for mode in 0..NUM_BMODES {
                let mut rd_tmp = ModeScore::default();
                let mut levels = [0i16; 16];
                let mut block = [0u8; 4 * BPS];
                rd_tmp.nz = self.reconstruct_intra4(it, &mut levels, src_off, mode, &mut block) << it.i4;
                rd_tmp.d = i64::from(dsp::sse(&it.yuv_in, src_off, &block, 0, 4, 4));
                rd_tmp.sd = if tlambda != 0 { i64::from(mult_8b(tlambda, dsp::disto4x4(&it.yuv_in, src_off, &block, 0, &WEIGHT_Y))) } else { 0 };
                rd_tmp.h = i64::from(mode_costs[mode]);
                rd_tmp.r = if mode > 0 && is_flat_levels(std::slice::from_ref(&levels), 1, 3) { 140 } else { 0 };
                rd_tmp.set_rd_score(lambda);
                if best_mode >= 0 && rd_tmp.score >= rd_i4.score {
                    continue;
                }
                rd_tmp.r += i64::from(self.cost_luma4(it, &levels));
                rd_tmp.set_rd_score(lambda);
                if best_mode < 0 || rd_tmp.score < rd_i4.score {
                    rd_i4.copy_score(&rd_tmp);
                    best_mode = mode as i32;
                    best_block = block;
                    rd_best.y_ac_levels[it.i4] = levels;
                }
            }
            rd_i4.set_rd_score(lambda_mode);
            rd_best.add_score(&rd_i4);
            if rd_best.score >= rd.score {
                return false;
            }
            total_header_bits += rd_i4.h as i32;
            if total_header_bits > self.max_i4_header_bits {
                return false;
            }
            dsp::copy_block(&best_block, 0, &mut it.yuv_out2, Y_OFF_ENC + DSP_SCAN[it.i4], 4, 4);
            rd.modes_i4[it.i4] = best_mode as u8;
            let nz = i32::from(rd_i4.nz != 0);
            it.top_nz[it.i4 & 3] = nz;
            it.left_nz[it.i4 >> 2] = nz;
            let out2 = std::mem::take(&mut it.yuv_out2);
            let more = Self::rotate_i4(it, &out2, Y_OFF_ENC);
            it.yuv_out2 = out2;
            if !more {
                break;
            }
        }
        rd.copy_score(&rd_best);
        let modes = rd.modes_i4;
        self.set_intra4_mode(it, &modes);
        std::mem::swap(&mut it.yuv_out, &mut it.yuv_out2);
        rd.y_ac_levels = rd_best.y_ac_levels;
        true
    }

    /// `PickBestUV`.
    fn pick_best_uv(&mut self, it: &mut Iter, rd: &mut ModeScore) {
        let seg = self.mb(it).segment as usize;
        let lambda = self.dqm[seg].lambda_uv;
        let mut rd_best = ModeScore::default();
        rd.mode_uv = -1;
        rd_best.init();
        let mut best_out = [0u8; 8 * BPS];
        for mode in 0..NUM_PRED_MODES {
            let mut rd_uv = ModeScore::default();
            let mut out = [0u8; 8 * BPS];
            rd_uv.nz = self.reconstruct_uv(it, &mut rd_uv, &mut out, mode);
            rd_uv.d = i64::from(dsp::sse(&it.yuv_in, U_OFF_ENC, &out, 0, 16, 8));
            rd_uv.sd = 0;
            rd_uv.h = i64::from(FIXED_COSTS_UV[mode]);
            rd_uv.r = i64::from(self.cost_uv(it, &rd_uv));
            if mode > 0 && is_flat_levels(&rd_uv.uv_levels, 8, 2) {
                rd_uv.r += 140 * 8;
            }
            rd_uv.set_rd_score(lambda);
            if mode == 0 || rd_uv.score < rd_best.score {
                rd_best.copy_score(&rd_uv);
                rd.mode_uv = mode as i32;
                rd.uv_levels = rd_uv.uv_levels;
                if self.top_derr.is_some() {
                    rd.derr = rd_uv.derr;
                }
                best_out = out;
            }
        }
        self.mb_mut(it).uv_mode = rd.mode_uv as u8;
        rd.add_score(&rd_best);
        dsp::copy_block(&best_out, 0, &mut it.yuv_out, U_OFF_ENC, 16, 8);
        if let Some(top_derr) = &mut self.top_derr {
            for ch in 0..2 {
                let left1 = ((3 * i32::from(rd.derr[ch][2])) >> 2) as i8;
                it.left_derr[ch][0] = rd.derr[ch][0];
                it.left_derr[ch][1] = left1;
                top_derr[it.x][ch][0] = rd.derr[ch][1];
                top_derr[it.x][ch][1] = rd.derr[ch][2].wrapping_sub(left1);
            }
        }
    }

    /// `VP8Decimate` com `RD_OPT_BASIC` (método 3): devolve se o macrobloco foi pulado.
    fn decimate(&mut self, it: &mut Iter, rd: &mut ModeScore) -> bool {
        rd.init();
        self.make_luma16_preds(it);
        self.make_chroma8_preds(it);
        self.pick_best_intra16(it, rd);
        if self.config.method >= 2 {
            self.pick_best_intra4(it, rd);
        }
        self.pick_best_uv(it, rd);
        let skipped = rd.nz == 0;
        self.mb_mut(it).skip = skipped;
        skipped
    }

    // ----------------------------------------------------------------------------------------------------------
    // O laço de codificação com tokens (`frame_enc.c`).

    /// `RecordTokens`.
    fn record_tokens(&mut self, it: &mut Iter, rd: &ModeScore) {
        self.nz_to_bytes(it);
        let i16 = self.mb(it).type_ == 1;
        let mut tokens = std::mem::take(&mut self.tokens);
        {
            let mut rec = TokenRecorder { tokens: &mut tokens, stats: &mut self.proba.stats };
            let ac_type = if i16 {
                let ctx = (it.top_nz[8] + it.left_nz[8]) as usize;
                let nz = i32::from(rec.record(ctx, &Residual::new(0, 1, &rd.y_dc_levels)));
                it.top_nz[8] = nz;
                it.left_nz[8] = nz;
                (1, 0)
            } else {
                (0, 3)
            };
            for y in 0..4 {
                for x in 0..4 {
                    let ctx = (it.top_nz[x] + it.left_nz[y]) as usize;
                    let nz = i32::from(rec.record(ctx, &Residual::new(ac_type.0, ac_type.1, &rd.y_ac_levels[x + y * 4])));
                    it.top_nz[x] = nz;
                    it.left_nz[y] = nz;
                }
            }
            for ch in [0, 2] {
                for y in 0..2 {
                    for x in 0..2 {
                        let ctx = (it.top_nz[4 + ch + x] + it.left_nz[4 + ch + y]) as usize;
                        let nz = i32::from(rec.record(ctx, &Residual::new(0, 2, &rd.uv_levels[ch * 2 + x + y * 2])));
                        it.top_nz[4 + ch + x] = nz;
                        it.left_nz[4 + ch + y] = nz;
                    }
                }
            }
        }
        self.tokens = tokens;
        self.bytes_to_nz(it);
    }

    /// `SetSegmentProbas`.
    fn set_segment_probas(&mut self) {
        let mut p = [0i32; NUM_MB_SEGMENTS];
        for mb in &self.mb_info {
            p[mb.segment as usize] += 1;
        }
        if self.num_segments > 1 {
            let probas = &mut self.proba.segments;
            probas[0] = get_proba(p[0] + p[1], p[2] + p[3]);
            probas[1] = get_proba(p[0], p[1]);
            probas[2] = get_proba(p[2], p[3]);
            self.update_map = probas.iter().any(|&v| v != 255);
            if !self.update_map {
                for mb in &mut self.mb_info {
                    mb.segment = 0;
                }
            }
            let pr = self.proba.segments;
            self.segment_size = p[0] * (bit_cost(false, pr[0]) + bit_cost(false, pr[1]))
                + p[1] * (bit_cost(false, pr[0]) + bit_cost(true, pr[1]))
                + p[2] * (bit_cost(true, pr[0]) + bit_cost(false, pr[2]))
                + p[3] * (bit_cost(true, pr[0]) + bit_cost(true, pr[2]));
        } else {
            self.update_map = false;
            self.segment_size = 0;
        }
    }

    /// `FinalizeTokenProbas`.
    fn finalize_token_probas(&mut self) {
        let mut has_changed = false;
        let proba = &mut self.proba;
        for t in 0..NUM_TYPES {
            for b in 0..NUM_BANDS {
                for c in 0..NUM_CTX {
                    for p in 0..NUM_PROBAS {
                        let stats = proba.stats[t][b][c][p];
                        let nb = (stats & 0xffff) as i32;
                        let total = ((stats >> 16) & 0xffff) as i32;
                        let update_proba = COEFFS_UPDATE_PROBA[t][b][c][p];
                        let old_p = COEFFS_PROBA0[t][b][c][p];
                        let new_p = if nb != 0 { (255 - nb * 255 / total) as u8 } else { 255 };
                        let old_cost = branch_cost(nb, total, old_p) + bit_cost(false, update_proba);
                        let new_cost = branch_cost(nb, total, new_p) + bit_cost(true, update_proba) + 8 * 256;
                        if old_cost > new_cost {
                            proba.coeffs[t][b][c][p] = new_p;
                            has_changed |= new_p != old_p;
                        } else {
                            proba.coeffs[t][b][c][p] = old_p;
                        }
                    }
                }
            }
        }
        proba.dirty = has_changed;
    }

    /// `VP8EncTokenLoop` com um passe e sem busca.
    fn token_loop(&mut self) {
        let mut max_count = ((self.mb_w * self.mb_h) >> 3) as i32;
        if max_count < 96 {
            max_count = 96;
        }
        let q = self.config.quality.clamp(0.0, 100.0);
        loop {
            let mut it = self.iter_init();
            // `SetLoopParams`.
            self.set_segment_params(q);
            self.set_segment_probas();
            self.proba.calculate_level_costs();
            self.proba.nb_skip = 0;
            // Último passe: zera as estatísticas dos tokens.
            self.proba.stats = [[[[0; NUM_PROBAS]; NUM_CTX]; NUM_BANDS]; NUM_TYPES];
            self.tokens.clear();
            let mut size_p0: u64 = 0;
            let mut cnt = max_count;
            loop {
                self.iter_import(&mut it, false);
                cnt -= 1;
                if cnt < 0 {
                    self.finalize_token_probas();
                    self.proba.calculate_level_costs();
                    cnt = max_count;
                }
                let mut info = ModeScore::default();
                self.decimate(&mut it, &mut info);
                self.record_tokens(&mut it, &info);
                size_p0 += info.h as u64;
                self.save_boundary(&mut it);
                if !self.iter_next(&mut it) {
                    break;
                }
            }
            size_p0 += self.segment_size as u64;
            const PARTITION0_SIZE_LIMIT: u64 = ((524_287 - 2048) as u64) << 11;
            if self.max_i4_header_bits > 0 && size_p0 > PARTITION0_SIZE_LIMIT {
                self.max_i4_header_bits >>= 1;
                continue;
            }
            break;
        }
        self.finalize_token_probas();
        // `VP8EmitTokens`.
        let mut bw = Vp8BitWriter::new();
        for &token in &self.tokens {
            let bit = (token >> 15) & 1 != 0;
            if token & FIXED_PROBA_BIT != 0 {
                bw.put_bit(bit, i32::from(token & 0xff));
            } else {
                let index = (token & 0x3fff) as usize;
                let (p, rest) = (index % NUM_PROBAS, index / NUM_PROBAS);
                let (ctx, rest) = (rest % NUM_CTX, rest / NUM_CTX);
                let (b, t) = (rest % NUM_BANDS, rest / NUM_BANDS);
                bw.put_bit(bit, i32::from(self.proba.coeffs[t][b][ctx][p]));
            }
        }
        bw.finish();
        self.part1 = bw;
        self.adjust_filter_strength();
    }

    /// `VP8AdjustFilterStrength` sem estatísticas de filtro automático.
    fn adjust_filter_strength(&mut self) {
        if self.config.filter_strength > 0 {
            let mut max_level = 0;
            for s in 0..NUM_MB_SEGMENTS {
                let dqm = &mut self.dqm[s];
                let delta = (dqm.max_edge * i32::from(dqm.y2.q[1])) >> 3;
                let level = filter_strength_from_delta(self.filter_sharpness, delta);
                if level > dqm.fstrength {
                    dqm.fstrength = level;
                }
                max_level = max_level.max(dqm.fstrength);
            }
            self.filter_level = max_level;
        }
    }

    // ----------------------------------------------------------------------------------------------------------
    // Sintaxe (`syntax_enc.c`, `tree_enc.c`).

    fn put_segment_header(&self, bw: &mut Vp8BitWriter) {
        if bw.put_bit_uniform(self.num_segments > 1) {
            bw.put_bit_uniform(self.update_map);
            if bw.put_bit_uniform(true) {
                bw.put_bit_uniform(true);
                for s in 0..NUM_MB_SEGMENTS {
                    bw.put_signed_bits(self.dqm[s].quant, 7);
                }
                for s in 0..NUM_MB_SEGMENTS {
                    bw.put_signed_bits(self.dqm[s].fstrength, 6);
                }
            }
            if self.update_map {
                for s in 0..3 {
                    if bw.put_bit_uniform(self.proba.segments[s] != 255) {
                        bw.put_bits(u32::from(self.proba.segments[s]), 8);
                    }
                }
            }
        }
    }

    fn put_filter_header(&self, bw: &mut Vp8BitWriter) {
        bw.put_bit_uniform(self.filter_simple);
        bw.put_bits(self.filter_level as u32, 6);
        bw.put_bits(self.filter_sharpness as u32, 3);
        // `i4x4_lf_delta_` é sempre 0.
        bw.put_bit_uniform(false);
    }

    fn write_probas(&self, bw: &mut Vp8BitWriter) {
        for t in 0..NUM_TYPES {
            for b in 0..NUM_BANDS {
                for c in 0..NUM_CTX {
                    for p in 0..NUM_PROBAS {
                        let p0 = self.proba.coeffs[t][b][c][p];
                        let update = p0 != COEFFS_PROBA0[t][b][c][p];
                        if bw.put_bit(update, i32::from(COEFFS_UPDATE_PROBA[t][b][c][p])) {
                            bw.put_bits(u32::from(p0), 8);
                        }
                    }
                }
            }
        }
        if bw.put_bit_uniform(self.proba.use_skip_proba) {
            bw.put_bits(u32::from(self.proba.skip_proba), 8);
        }
    }

    /// `VP8CodeIntraModes`.
    fn code_intra_modes(&self, bw: &mut Vp8BitWriter) {
        let pw = self.preds_w;
        for y in 0..self.mb_h {
            for x in 0..self.mb_w {
                let mb = &self.mb_info[y * self.mb_w + x];
                let preds = self.preds_index(x, y);
                if self.update_map {
                    let s = i32::from(mb.segment);
                    let p = &self.proba.segments;
                    let right = bw.put_bit(s >= 2, i32::from(p[0]));
                    bw.put_bit(s & 1 != 0, i32::from(p[1 + usize::from(right)]));
                }
                if self.proba.use_skip_proba {
                    bw.put_bit(mb.skip, i32::from(self.proba.skip_proba));
                }
                if bw.put_bit(mb.type_ != 0, 145) {
                    let mode = self.preds[preds];
                    // TM_PRED = 1, H_PRED = 3, V_PRED = 2.
                    if bw.put_bit(mode == 1 || mode == 3, 156) {
                        bw.put_bit(mode == 1, 128);
                    } else {
                        bw.put_bit(mode == 2, 163);
                    }
                } else {
                    let mut top_row = preds - pw;
                    let mut row = preds;
                    for _ in 0..4 {
                        let mut left = self.preds[row - 1];
                        for x4 in 0..4 {
                            let probas = &BMODES_PROBA[self.preds[top_row + x4] as usize][left as usize];
                            left = put_i4_mode(bw, self.preds[row + x4], probas);
                        }
                        top_row = row;
                        row += pw;
                    }
                }
                let uv = mb.uv_mode;
                // DC_PRED = 0, V_PRED = 2, H_PRED = 3.
                if bw.put_bit(uv != 0, 142) && bw.put_bit(uv != 2, 114) {
                    bw.put_bit(uv != 3, 183);
                }
            }
        }
    }

    /// `GeneratePartition0`.
    fn generate_partition0(&mut self) {
        let mut bw = Vp8BitWriter::new();
        bw.put_bit_uniform(false);
        bw.put_bit_uniform(false);
        self.put_segment_header(&mut bw);
        self.put_filter_header(&mut bw);
        bw.put_bits(0, 2);
        bw.put_bits(self.base_quant as u32, 7);
        bw.put_signed_bits(0, 4);
        bw.put_signed_bits(0, 4);
        bw.put_signed_bits(0, 4);
        bw.put_signed_bits(self.dq_uv_dc, 4);
        bw.put_signed_bits(self.dq_uv_ac, 4);
        bw.put_bit_uniform(false);
        self.write_probas(&mut bw);
        self.code_intra_modes(&mut bw);
        bw.finish();
        self.part0 = bw;
    }

    /// `VP8EncWrite`: o conteúdo do chunk `VP8 ` (cabeçalho do quadro e as duas partições), sem o RIFF.
    fn write(&mut self) -> Vec<u8> {
        self.generate_partition0();
        let size0 = self.part0.bytes().len();
        let mut out = Vec::with_capacity(10 + size0 + self.part1.bytes().len());
        let bits = (self.profile as u32) << 1 | 1 << 4 | (size0 as u32) << 5;
        out.extend_from_slice(&bits.to_le_bytes()[..3]);
        out.extend_from_slice(&[0x9d, 0x01, 0x2a]);
        out.extend_from_slice(&(self.pic.width as u16).to_le_bytes());
        out.extend_from_slice(&(self.pic.height as u16).to_le_bytes());
        out.extend_from_slice(self.part0.bytes());
        out.extend_from_slice(self.part1.bytes());
        // O `PutVP8Header` conta o byte de alinhamento no tamanho do chunk: o enchimento faz parte do conteúdo.
        if out.len() & 1 != 0 {
            out.push(0);
        }
        out
    }
}

/// `put_i4_mode` (`PutI4Mode`). Modos: DC 0, TM 1, VE 2, HE 3, RD 4, VR 5, LD 6, VL 7, HD 8, HU 9.
fn put_i4_mode(bw: &mut Vp8BitWriter, mode: u8, prob: &[u8; 9]) -> u8 {
    let p = |k: usize| i32::from(prob[k]);
    if bw.put_bit(mode != 0, p(0)) && bw.put_bit(mode != 1, p(1)) && bw.put_bit(mode != 2, p(2)) {
        if !bw.put_bit(mode >= 6, p(3)) {
            if bw.put_bit(mode != 3, p(4)) {
                bw.put_bit(mode != 4, p(5));
            }
        } else if bw.put_bit(mode != 6, p(6)) && bw.put_bit(mode != 7, p(7)) {
            bw.put_bit(mode != 8, p(8));
        }
    }
    mode
}

fn import_block(src: &[u8], mut src_at: usize, src_stride: usize, dst: &mut [u8], dst_off: usize, w: usize, h: usize, size: usize) {
    let mut d = dst_off;
    for _ in 0..h {
        dst[d..d + w].copy_from_slice(&src[src_at..src_at + w]);
        if w < size {
            let last = dst[d + w - 1];
            dst[d + w..d + size].fill(last);
        }
        d += BPS;
        src_at += src_stride;
    }
    for _ in h..size {
        dst.copy_within(d - BPS..d - BPS + size, d);
        d += BPS;
    }
}

fn import_line(src: &[u8], mut at: usize, stride: usize, dst: &mut [u8], len: usize, total: usize) {
    for d in dst.iter_mut().take(len) {
        *d = src[at];
        at += stride;
    }
    let last = dst[len - 1];
    dst[len..total].fill(last);
}

/// `TransformWHT_C` do decodificador: `tmp[k][0]` recebe o DC de cada bloco.
fn transform_wht(input: &[i16; 16], out: &mut [[i16; 16]; 16]) {
    let mut tmp = [0i32; 16];
    for i in 0..4 {
        let at = |k: usize| i32::from(input[k + i]);
        let a0 = at(0) + at(12);
        let a1 = at(4) + at(8);
        let a2 = at(4) - at(8);
        let a3 = at(0) - at(12);
        tmp[i] = a0 + a1;
        tmp[8 + i] = a0 - a1;
        tmp[4 + i] = a3 + a2;
        tmp[12 + i] = a3 - a2;
    }
    for i in 0..4 {
        let dc = tmp[i * 4] + 3;
        let a0 = dc + tmp[3 + i * 4];
        let a1 = tmp[1 + i * 4] + tmp[2 + i * 4];
        let a2 = tmp[1 + i * 4] - tmp[2 + i * 4];
        let a3 = dc - tmp[3 + i * 4];
        // `out += 64` por linha, com `out[0]`, `out[16]`, `out[32]`, `out[48]`: os blocos 4i, 4i+1, 4i+2, 4i+3.
        out[i * 4][0] = ((a0 + a1) >> 3) as i16;
        out[i * 4 + 1][0] = ((a3 + a2) >> 3) as i16;
        out[i * 4 + 2][0] = ((a0 - a1) >> 3) as i16;
        out[i * 4 + 3][0] = ((a3 - a2) >> 3) as i16;
    }
}

/// `QuantizeSingle`.
fn quantize_single(v: &mut i16, mtx: &Matrix) -> i32 {
    let mut value = i32::from(*v);
    let sign = value < 0;
    if sign {
        value = -value;
    }
    if value > mtx.zthresh[0] as i32 {
        let qv = (((value as u32).wrapping_mul(u32::from(mtx.iq[0])).wrapping_add(mtx.bias[0]) >> 17) * u32::from(mtx.q[0])) as i32;
        let err = value - qv;
        *v = if sign { -qv } else { qv } as i16;
        return (if sign { -err } else { err }) >> 1;
    }
    *v = 0;
    (if sign { -value } else { value }) >> 1
}

/// `CorrectDCValues`.
fn correct_dc_values(top: &[[i8; 2]; 2], left: &[[i8; 2]; 2], mtx: &Matrix, tmp: &mut [[i16; 16]; 8], rd: &mut ModeScore) {
    const C1: i32 = 7;
    const C2: i32 = 8;
    const SHIFT: u32 = 3;
    for ch in 0..2 {
        let (top, left) = (top[ch], left[ch]);
        let c = &mut tmp[ch * 4..ch * 4 + 4];
        c[0][0] = (i32::from(c[0][0]) + ((C1 * i32::from(top[0]) + C2 * i32::from(left[0])) >> SHIFT)) as i16;
        let err0 = quantize_single(&mut c[0][0], mtx);
        c[1][0] = (i32::from(c[1][0]) + ((C1 * i32::from(top[1]) + C2 * err0) >> SHIFT)) as i16;
        let err1 = quantize_single(&mut c[1][0], mtx);
        c[2][0] = (i32::from(c[2][0]) + ((C1 * err0 + C2 * i32::from(left[1])) >> SHIFT)) as i16;
        let err2 = quantize_single(&mut c[2][0], mtx);
        c[3][0] = (i32::from(c[3][0]) + ((C1 * err1 + C2 * err2) >> SHIFT)) as i16;
        let err3 = quantize_single(&mut c[3][0], mtx);
        rd.derr[ch] = [err1 as i8, err2 as i8, err3 as i8];
    }
}

fn is_flat_source16(src: &[u8], off: usize) -> bool {
    let v = src[off];
    (0..16).all(|y| src[off + y * BPS..off + y * BPS + 16].iter().all(|&p| p == v))
}

/// `IsFlat`.
fn is_flat_levels(levels: &[[i16; 16]], num_blocks: usize, thresh: i32) -> bool {
    let mut score = 0;
    for block in levels.iter().take(num_blocks) {
        for &l in &block[1..] {
            score += i32::from(l != 0);
            if score > thresh {
                return false;
            }
        }
    }
    true
}

fn mult_8b(a: i32, b: i32) -> i32 {
    (a * b + 128) >> 8
}

fn get_alpha(histo: &dsp::Histogram) -> i32 {
    if histo.max_value > 1 {
        2 * 255 * histo.last_non_zero / histo.max_value
    } else {
        0
    }
}

fn quality_to_compression(c: f64) -> f64 {
    let linear_c = if c < 0.75 { c * (2.0 / 3.0) } else { 2.0 * c - 1.0 };
    linear_c.powf(1.0 / 3.0)
}

/// `ExpandMatrix`.
fn expand_matrix(m: &mut Matrix, type_: usize) -> i32 {
    for i in 0..2 {
        let bias = BIAS_MATRICES[type_][usize::from(i > 0)];
        m.iq[i] = ((1u32 << 17) / u32::from(m.q[i])) as u16;
        m.bias[i] = bias << (17 - 8);
        m.zthresh[i] = ((1u32 << 17) - 1 - m.bias[i]) / u32::from(m.iq[i]);
    }
    for i in 2..16 {
        m.q[i] = m.q[1];
        m.iq[i] = m.iq[1];
        m.bias[i] = m.bias[1];
        m.zthresh[i] = m.zthresh[1];
    }
    let mut sum = 0i32;
    for i in 0..16 {
        m.sharpen[i] = if type_ == 0 { ((FREQ_SHARPENING[i] * u32::from(m.q[i])) >> 11) as u16 } else { 0 };
        sum += i32::from(m.q[i]);
    }
    (sum + 8) >> 4
}

fn filter_strength_from_delta(sharpness: i32, delta: i32) -> i32 {
    let pos = delta.clamp(0, MAX_LF_LEVELS - 1) as usize;
    i32::from(LEVELS_FROM_DELTA[sharpness as usize][pos])
}

fn get_proba(a: i32, b: i32) -> u8 {
    let total = a + b;
    if total == 0 {
        255
    } else {
        ((255 * a + total / 2) / total) as u8
    }
}

fn branch_cost(nb: i32, total: i32, proba: u8) -> i32 {
    nb * bit_cost(true, proba) + (total - nb) * bit_cost(false, proba)
}

/// `WebPEncode` no caminho com perdas, depois da conversão para YUVA: o chunk `ALPH` (quando há alfa) e o `VP8 `.
pub fn encode(config: &Config, pic: &Picture) -> (Option<Vec<u8>>, Vec<u8>) {
    let mut enc = Encoder::new(config, pic);
    enc.analyze();
    // `VP8EncStartAlpha`/`VP8EncFinishAlpha`, sem threads.
    let alpha = pic.a.as_ref().map(|a| super::alpha::encode(a, pic.width, pic.height, config));
    enc.token_loop();
    (alpha, enc.write())
}
