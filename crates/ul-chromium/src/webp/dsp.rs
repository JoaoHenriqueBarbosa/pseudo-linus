//! `src/dsp/enc.c` do libwebp 1.5.0: transformadas, predições intra, métricas e quantização do codificador VP8.
//! Os blocos vivem em buffers com passo `BPS` (32), como no C; cada função recebe o buffer e o deslocamento.

pub const BPS: usize = 32;

pub const MAX_COEFF_THRESH: usize = 31;
pub const MAX_LEVEL: i32 = 2047;
pub const QFIX: u32 = 17;

/// `VP8DspScan`: o deslocamento de cada bloco 4x4 (16 de luma, 4 de U e 4 de V) no buffer de passo `BPS`.
pub const DSP_SCAN: [usize; 24] = [
    0, 4, 8, 12, 4 * BPS, 4 + 4 * BPS, 8 + 4 * BPS, 12 + 4 * BPS, 8 * BPS, 4 + 8 * BPS, 8 + 8 * BPS, 12 + 8 * BPS, 12 * BPS, 4 + 12 * BPS,
    8 + 12 * BPS, 12 + 12 * BPS, 0, 4, 4 * BPS, 4 + 4 * BPS, 8, 12, 8 + 4 * BPS, 12 + 4 * BPS,
];

// Deslocamentos das predições no buffer `yuv_p` (`vp8i_enc.h`).
pub const I16DC16: usize = 0;
pub const I16TM16: usize = I16DC16 + 16;
pub const I16VE16: usize = 16 * BPS;
pub const I16HE16: usize = I16VE16 + 16;
pub const C8DC8: usize = 2 * 16 * BPS;
pub const C8TM8: usize = C8DC8 + 16;
pub const C8VE8: usize = 2 * 16 * BPS + 8 * BPS;
pub const C8HE8: usize = C8VE8 + 16;
pub const I4DC4: usize = 3 * 16 * BPS;
pub const I4TM4: usize = I4DC4 + 4;
pub const I4VE4: usize = I4DC4 + 8;
pub const I4HE4: usize = I4DC4 + 12;
pub const I4RD4: usize = I4DC4 + 16;
pub const I4VR4: usize = I4DC4 + 20;
pub const I4LD4: usize = I4DC4 + 24;
pub const I4VL4: usize = I4DC4 + 28;
pub const I4HD4: usize = 3 * 16 * BPS + 4 * BPS;
pub const I4HU4: usize = I4HD4 + 4;

pub const YUV_SIZE_ENC: usize = BPS * 16;
pub const PRED_SIZE_ENC: usize = 32 * BPS + 16 * BPS + 8 * BPS;
pub const Y_OFF_ENC: usize = 0;
pub const U_OFF_ENC: usize = 16;
pub const V_OFF_ENC: usize = 16 + 8;

/// `VP8Matrix`.
#[derive(Clone, Default)]
pub struct Matrix {
    pub q: [u16; 16],
    pub iq: [u16; 16],
    pub bias: [u32; 16],
    pub zthresh: [u32; 16],
    pub sharpen: [u16; 16],
}

/// `VP8Histogram`.
#[derive(Clone, Copy, Default)]
pub struct Histogram {
    pub max_value: i32,
    pub last_non_zero: i32,
}

pub fn clip_8b(v: i32) -> u8 {
    if v & !0xff == 0 {
        v as u8
    } else if v < 0 {
        0
    } else {
        255
    }
}

fn mul1(a: i32) -> i32 {
    ((a * 20091) >> 16) + a
}

fn mul2(a: i32) -> i32 {
    (a * 35468) >> 16
}

/// `VP8SetHistogramData`.
pub fn set_histogram_data(distribution: &[i32; MAX_COEFF_THRESH + 1]) -> Histogram {
    let (mut max_value, mut last_non_zero) = (0, 1);
    for (k, &value) in distribution.iter().enumerate() {
        if value > 0 {
            max_value = max_value.max(value);
            last_non_zero = k as i32;
        }
    }
    Histogram { max_value, last_non_zero }
}

/// `CollectHistogram_C`.
pub fn collect_histogram(src: &[u8], src_off: usize, pred: &[u8], pred_off: usize, start: usize, end: usize) -> Histogram {
    let mut distribution = [0i32; MAX_COEFF_THRESH + 1];
    for j in start..end {
        let mut out = [0i16; 16];
        ftransform(src, src_off + DSP_SCAN[j], pred, pred_off + DSP_SCAN[j], &mut out);
        for &c in &out {
            let v = (i32::from(c).abs() >> 3).min(MAX_COEFF_THRESH as i32);
            distribution[v as usize] += 1;
        }
    }
    set_histogram_data(&distribution)
}

fn itransform_one(reference: &[u8], ref_off: usize, input: &[i16], dst: &mut [u8], dst_off: usize) {
    let mut c = [0i32; 16];
    for i in 0..4 {
        let at = |k: usize| i32::from(input[i + k]);
        let a = at(0) + at(8);
        let b = at(0) - at(8);
        let cc = mul2(at(4)) - mul1(at(12));
        let d = mul1(at(4)) + mul2(at(12));
        c[i * 4] = a + d;
        c[i * 4 + 1] = b + cc;
        c[i * 4 + 2] = b - cc;
        c[i * 4 + 3] = a - d;
    }
    for i in 0..4 {
        let dc = c[i] + 4;
        let a = dc + c[i + 8];
        let b = dc - c[i + 8];
        let cc = mul2(c[i + 4]) - mul1(c[i + 12]);
        let d = mul1(c[i + 4]) + mul2(c[i + 12]);
        let mut store = |x: usize, v: i32| {
            dst[dst_off + x + i * BPS] = clip_8b(i32::from(reference[ref_off + x + i * BPS]) + (v >> 3));
        };
        store(0, a + d);
        store(1, b + cc);
        store(2, b - cc);
        store(3, a - d);
    }
}

/// `ITransform_C`: `reference` e `dst` podem ser buffers diferentes.
pub fn itransform(reference: &[u8], ref_off: usize, input: &[i16], dst: &mut [u8], dst_off: usize, do_two: bool) {
    itransform_one(reference, ref_off, input, dst, dst_off);
    if do_two {
        itransform_one(reference, ref_off + 4, &input[16..], dst, dst_off + 4);
    }
}

/// `FTransform_C`.
pub fn ftransform(src: &[u8], src_off: usize, reference: &[u8], ref_off: usize, out: &mut [i16]) {
    let mut tmp = [0i32; 16];
    for i in 0..4 {
        let s = |k: usize| i32::from(src[src_off + i * BPS + k]);
        let r = |k: usize| i32::from(reference[ref_off + i * BPS + k]);
        let (d0, d1, d2, d3) = (s(0) - r(0), s(1) - r(1), s(2) - r(2), s(3) - r(3));
        let (a0, a1, a2, a3) = (d0 + d3, d1 + d2, d1 - d2, d0 - d3);
        tmp[i * 4] = (a0 + a1) * 8;
        tmp[1 + i * 4] = (a2 * 2217 + a3 * 5352 + 1812) >> 9;
        tmp[2 + i * 4] = (a0 - a1) * 8;
        tmp[3 + i * 4] = (a3 * 2217 - a2 * 5352 + 937) >> 9;
    }
    for i in 0..4 {
        let a0 = tmp[i] + tmp[12 + i];
        let a1 = tmp[4 + i] + tmp[8 + i];
        let a2 = tmp[4 + i] - tmp[8 + i];
        let a3 = tmp[i] - tmp[12 + i];
        out[i] = ((a0 + a1 + 7) >> 4) as i16;
        out[4 + i] = (((a2 * 2217 + a3 * 5352 + 12000) >> 16) + i32::from(a3 != 0)) as i16;
        out[8 + i] = ((a0 - a1 + 7) >> 4) as i16;
        out[12 + i] = ((a3 * 2217 - a2 * 5352 + 51000) >> 16) as i16;
    }
}

/// `FTransform2_C`.
pub fn ftransform2(src: &[u8], src_off: usize, reference: &[u8], ref_off: usize, out: &mut [i16]) {
    ftransform(src, src_off, reference, ref_off, &mut out[..16]);
    ftransform(src, src_off + 4, reference, ref_off + 4, &mut out[16..32]);
}

/// `FTransformWHT_C`: `input` são os 16 blocos de 16 coeficientes, lidos de 16 em 16.
pub fn ftransform_wht(input: &[i16], out: &mut [i16; 16]) {
    let mut tmp = [0i32; 16];
    for i in 0..4 {
        let at = |k: usize| i32::from(input[i * 64 + k * 16]);
        let a0 = at(0) + at(2);
        let a1 = at(1) + at(3);
        let a2 = at(1) - at(3);
        let a3 = at(0) - at(2);
        tmp[i * 4] = a0 + a1;
        tmp[1 + i * 4] = a3 + a2;
        tmp[2 + i * 4] = a3 - a2;
        tmp[3 + i * 4] = a0 - a1;
    }
    for i in 0..4 {
        let a0 = tmp[i] + tmp[8 + i];
        let a1 = tmp[4 + i] + tmp[12 + i];
        let a2 = tmp[4 + i] - tmp[12 + i];
        let a3 = tmp[i] - tmp[8 + i];
        out[i] = ((a0 + a1) >> 1) as i16;
        out[4 + i] = ((a3 + a2) >> 1) as i16;
        out[8 + i] = ((a3 - a2) >> 1) as i16;
        out[12 + i] = ((a0 - a1) >> 1) as i16;
    }
}

fn fill(dst: &mut [u8], off: usize, value: u8, size: usize) {
    for j in 0..size {
        dst[off + j * BPS..off + j * BPS + size].fill(value);
    }
}

/// Uma borda de predição: `None` quando não existe (primeira linha ou coluna).
pub type Edge<'a> = Option<&'a [u8]>;

fn vertical_pred(dst: &mut [u8], off: usize, top: Edge, size: usize) {
    match top {
        Some(top) => {
            for j in 0..size {
                dst[off + j * BPS..off + j * BPS + size].copy_from_slice(&top[..size]);
            }
        }
        None => fill(dst, off, 127, size),
    }
}

fn horizontal_pred(dst: &mut [u8], off: usize, left: Edge, size: usize) {
    match left {
        Some(left) => {
            for j in 0..size {
                dst[off + j * BPS..off + j * BPS + size].fill(left[j]);
            }
        }
        None => fill(dst, off, 129, size),
    }
}

/// `TrueMotion`: `left` começa no canto (`left[0]` é o `left[-1]` do C).
fn true_motion(dst: &mut [u8], off: usize, left: Edge, top: Edge, size: usize) {
    match (left, top) {
        (Some(left), Some(top)) => {
            let corner = i32::from(left[0]);
            for y in 0..size {
                for x in 0..size {
                    dst[off + y * BPS + x] = clip_8b(i32::from(left[1 + y]) + i32::from(top[x]) - corner);
                }
            }
        }
        (Some(left), None) => horizontal_pred(dst, off, Some(&left[1..]), size),
        (None, Some(top)) => vertical_pred(dst, off, Some(top), size),
        (None, None) => fill(dst, off, 129, size),
    }
}

fn dc_mode(dst: &mut [u8], off: usize, left: Edge, top: Edge, size: usize, round: i32, shift: u32) {
    let sum = |edge: &[u8]| edge[..size].iter().map(|&v| i32::from(v)).sum::<i32>();
    let dc = match (top, left) {
        (Some(top), Some(left)) => (sum(top) + sum(left) + round) >> shift,
        (Some(top), None) => (2 * sum(top) + round) >> shift,
        (None, Some(left)) => (2 * sum(left) + round) >> shift,
        (None, None) => 0x80,
    };
    fill(dst, off, dc as u8, size);
}

/// `IntraChromaPreds_C`. `left` é a borda esquerda com o canto: `[canto_u, u0..u7, ?, ...]` no layout do iterador
/// (`u_left` com o canto em `-1`, e o `v_left` 16 bytes depois). Aqui recebemos as duas bordas já separadas, cada uma
/// com o canto na posição 0.
pub fn intra_chroma_preds(dst: &mut [u8], left_u: Edge, left_v: Edge, top: Edge) {
    for (k, left) in [left_u, left_v].into_iter().enumerate() {
        let off = k * 8;
        let top = top.map(|t| &t[k * 8..]);
        dc_mode(dst, C8DC8 + off, left.map(|l| &l[1..]), top, 8, 8, 4);
        vertical_pred(dst, C8VE8 + off, top, 8);
        horizontal_pred(dst, C8HE8 + off, left.map(|l| &l[1..]), 8);
        true_motion(dst, C8TM8 + off, left, top, 8);
    }
}

/// `Intra16Preds_C`. `left` traz o canto na posição 0.
pub fn intra16_preds(dst: &mut [u8], left: Edge, top: Edge) {
    dc_mode(dst, I16DC16, left.map(|l| &l[1..]), top, 16, 16, 5);
    vertical_pred(dst, I16VE16, top, 16);
    horizontal_pred(dst, I16HE16, left.map(|l| &l[1..]), 16);
    true_motion(dst, I16TM16, left, top, 16);
}

fn avg3(a: i32, b: i32, c: i32) -> u8 {
    ((a + 2 * b + c + 2) >> 2) as u8
}

fn avg2(a: i32, b: i32) -> u8 {
    ((a + b + 1) >> 1) as u8
}

/// `Intra4Preds_C`. `top` é a borda do `i4_boundary_` a partir de `top[-5]`: `t(k)` lê o `top[k]` do C.
pub fn intra4_preds(dst: &mut [u8], boundary: &[u8], top_at: usize) {
    let t = |k: isize| i32::from(boundary[(top_at as isize + k) as usize]);
    let (x, i, j, k, l) = (t(-1), t(-2), t(-3), t(-4), t(-5));
    let (a, b, c, d, e, f, g, h) = (t(0), t(1), t(2), t(3), t(4), t(5), t(6), t(7));
    let mut set = |base: usize, xx: usize, yy: usize, v: u8| dst[base + xx + yy * BPS] = v;
    // DC4
    let dc = (4 + (0..4).map(|n| t(n) + t(-5 + n)).sum::<i32>()) >> 3;
    for yy in 0..4 {
        for xx in 0..4 {
            set(I4DC4, xx, yy, dc as u8);
        }
    }
    // TM4
    for yy in 0..4 {
        for xx in 0..4 {
            set(I4TM4, xx, yy, clip_8b(t(-2 - yy as isize) + t(xx as isize) - x));
        }
    }
    // VE4
    let ve = [avg3(x, a, b), avg3(a, b, c), avg3(b, c, d), avg3(c, d, e)];
    for yy in 0..4 {
        for (xx, &v) in ve.iter().enumerate() {
            set(I4VE4, xx, yy, v);
        }
    }
    // HE4
    let he = [avg3(x, i, j), avg3(i, j, k), avg3(j, k, l), avg3(k, l, l)];
    for (yy, &v) in he.iter().enumerate() {
        for xx in 0..4 {
            set(I4HE4, xx, yy, v);
        }
    }
    // RD4
    {
        let p = I4RD4;
        set(p, 0, 3, avg3(j, k, l));
        let v = avg3(i, j, k);
        set(p, 0, 2, v);
        set(p, 1, 3, v);
        let v = avg3(x, i, j);
        set(p, 0, 1, v);
        set(p, 1, 2, v);
        set(p, 2, 3, v);
        let v = avg3(a, x, i);
        for n in 0..4 {
            set(p, n, n, v);
        }
        let v = avg3(b, a, x);
        set(p, 1, 0, v);
        set(p, 2, 1, v);
        set(p, 3, 2, v);
        let v = avg3(c, b, a);
        set(p, 2, 0, v);
        set(p, 3, 1, v);
        set(p, 3, 0, avg3(d, c, b));
    }
    // VR4
    {
        let p = I4VR4;
        let v = avg2(x, a);
        set(p, 0, 0, v);
        set(p, 1, 2, v);
        let v = avg2(a, b);
        set(p, 1, 0, v);
        set(p, 2, 2, v);
        let v = avg2(b, c);
        set(p, 2, 0, v);
        set(p, 3, 2, v);
        set(p, 3, 0, avg2(c, d));
        set(p, 0, 3, avg3(k, j, i));
        set(p, 0, 2, avg3(j, i, x));
        let v = avg3(i, x, a);
        set(p, 0, 1, v);
        set(p, 1, 3, v);
        let v = avg3(x, a, b);
        set(p, 1, 1, v);
        set(p, 2, 3, v);
        let v = avg3(a, b, c);
        set(p, 2, 1, v);
        set(p, 3, 3, v);
        set(p, 3, 1, avg3(b, c, d));
    }
    // LD4
    {
        let p = I4LD4;
        set(p, 0, 0, avg3(a, b, c));
        let v = avg3(b, c, d);
        set(p, 1, 0, v);
        set(p, 0, 1, v);
        let v = avg3(c, d, e);
        set(p, 2, 0, v);
        set(p, 1, 1, v);
        set(p, 0, 2, v);
        let v = avg3(d, e, f);
        set(p, 3, 0, v);
        set(p, 2, 1, v);
        set(p, 1, 2, v);
        set(p, 0, 3, v);
        let v = avg3(e, f, g);
        set(p, 3, 1, v);
        set(p, 2, 2, v);
        set(p, 1, 3, v);
        let v = avg3(f, g, h);
        set(p, 3, 2, v);
        set(p, 2, 3, v);
        set(p, 3, 3, avg3(g, h, h));
    }
    // VL4
    {
        let p = I4VL4;
        set(p, 0, 0, avg2(a, b));
        let v = avg2(b, c);
        set(p, 1, 0, v);
        set(p, 0, 2, v);
        let v = avg2(c, d);
        set(p, 2, 0, v);
        set(p, 1, 2, v);
        let v = avg2(d, e);
        set(p, 3, 0, v);
        set(p, 2, 2, v);
        set(p, 0, 1, avg3(a, b, c));
        let v = avg3(b, c, d);
        set(p, 1, 1, v);
        set(p, 0, 3, v);
        let v = avg3(c, d, e);
        set(p, 2, 1, v);
        set(p, 1, 3, v);
        let v = avg3(d, e, f);
        set(p, 3, 1, v);
        set(p, 2, 3, v);
        set(p, 3, 2, avg3(e, f, g));
        set(p, 3, 3, avg3(f, g, h));
    }
    // HD4
    {
        let p = I4HD4;
        let v = avg2(i, x);
        set(p, 0, 0, v);
        set(p, 2, 1, v);
        let v = avg2(j, i);
        set(p, 0, 1, v);
        set(p, 2, 2, v);
        let v = avg2(k, j);
        set(p, 0, 2, v);
        set(p, 2, 3, v);
        set(p, 0, 3, avg2(l, k));
        set(p, 3, 0, avg3(a, b, c));
        set(p, 2, 0, avg3(x, a, b));
        let v = avg3(i, x, a);
        set(p, 1, 0, v);
        set(p, 3, 1, v);
        let v = avg3(j, i, x);
        set(p, 1, 1, v);
        set(p, 3, 2, v);
        let v = avg3(k, j, i);
        set(p, 1, 2, v);
        set(p, 3, 3, v);
        set(p, 1, 3, avg3(l, k, j));
    }
    // HU4
    {
        let p = I4HU4;
        set(p, 0, 0, avg2(i, j));
        let v = avg2(j, k);
        set(p, 2, 0, v);
        set(p, 0, 1, v);
        let v = avg2(k, l);
        set(p, 2, 1, v);
        set(p, 0, 2, v);
        set(p, 1, 0, avg3(i, j, k));
        let v = avg3(j, k, l);
        set(p, 3, 0, v);
        set(p, 1, 1, v);
        let v = avg3(k, l, l);
        set(p, 3, 1, v);
        set(p, 1, 2, v);
        for (xx, yy) in [(3, 2), (2, 2), (0, 3), (1, 3), (2, 3), (3, 3)] {
            set(p, xx, yy, l as u8);
        }
    }
}

/// `GetSSE`.
pub fn sse(a: &[u8], a_off: usize, b: &[u8], b_off: usize, w: usize, h: usize) -> i32 {
    let mut count = 0;
    for y in 0..h {
        for x in 0..w {
            let diff = i32::from(a[a_off + y * BPS + x]) - i32::from(b[b_off + y * BPS + x]);
            count += diff * diff;
        }
    }
    count
}

/// `Mean16x4_C`.
pub fn mean16x4(reference: &[u8], off: usize) -> [u32; 4] {
    let mut dc = [0u32; 4];
    for (k, d) in dc.iter_mut().enumerate() {
        for y in 0..4 {
            for x in 0..4 {
                *d += u32::from(reference[off + k * 4 + x + y * BPS]);
            }
        }
    }
    dc
}

fn ttransform(input: &[u8], off: usize, w: &[u16; 16]) -> i32 {
    let mut tmp = [0i32; 16];
    for i in 0..4 {
        let p = |k: usize| i32::from(input[off + i * BPS + k]);
        let a0 = p(0) + p(2);
        let a1 = p(1) + p(3);
        let a2 = p(1) - p(3);
        let a3 = p(0) - p(2);
        tmp[i * 4] = a0 + a1;
        tmp[1 + i * 4] = a3 + a2;
        tmp[2 + i * 4] = a3 - a2;
        tmp[3 + i * 4] = a0 - a1;
    }
    let mut sum = 0;
    for i in 0..4 {
        let a0 = tmp[i] + tmp[8 + i];
        let a1 = tmp[4 + i] + tmp[12 + i];
        let a2 = tmp[4 + i] - tmp[12 + i];
        let a3 = tmp[i] - tmp[8 + i];
        sum += i32::from(w[i]) * (a0 + a1).abs();
        sum += i32::from(w[4 + i]) * (a3 + a2).abs();
        sum += i32::from(w[8 + i]) * (a3 - a2).abs();
        sum += i32::from(w[12 + i]) * (a0 - a1).abs();
    }
    sum
}

/// `Disto4x4_C`.
pub fn disto4x4(a: &[u8], a_off: usize, b: &[u8], b_off: usize, w: &[u16; 16]) -> i32 {
    (ttransform(b, b_off, w) - ttransform(a, a_off, w)).abs() >> 5
}

/// `Disto16x16_C`.
pub fn disto16x16(a: &[u8], a_off: usize, b: &[u8], b_off: usize, w: &[u16; 16]) -> i32 {
    let mut d = 0;
    for y in (0..16 * BPS).step_by(4 * BPS) {
        for x in (0..16).step_by(4) {
            d += disto4x4(a, a_off + x + y, b, b_off + x + y, w);
        }
    }
    d
}

pub const ZIGZAG: [usize; 16] = [0, 1, 4, 8, 5, 2, 3, 6, 9, 12, 13, 10, 7, 11, 14, 15];

/// `QuantizeBlock_C` (também o `VP8EncQuantizeBlockWHT`).
pub fn quantize_block(input: &mut [i16], out: &mut [i16], mtx: &Matrix) -> bool {
    let mut last = -1i32;
    for n in 0..16 {
        let j = ZIGZAG[n];
        let sign = input[j] < 0;
        let coeff = (i32::from(input[j]).unsigned_abs()) + u32::from(mtx.sharpen[j]);
        if coeff > mtx.zthresh[j] {
            let q = u32::from(mtx.q[j]);
            let iq = u32::from(mtx.iq[j]);
            let b = mtx.bias[j];
            let mut level = ((coeff.wrapping_mul(iq).wrapping_add(b)) >> QFIX) as i32;
            if level > MAX_LEVEL {
                level = MAX_LEVEL;
            }
            if sign {
                level = -level;
            }
            input[j] = (level * q as i32) as i16;
            out[n] = level as i16;
            if level != 0 {
                last = n as i32;
            }
        } else {
            out[n] = 0;
            input[j] = 0;
        }
    }
    last >= 0
}

/// `Quantize2Blocks_C`.
pub fn quantize_2blocks(input: &mut [i16], out: &mut [i16], mtx: &Matrix) -> u32 {
    let nz0 = quantize_block(&mut input[..16], &mut out[..16], mtx) as u32;
    let nz1 = quantize_block(&mut input[16..32], &mut out[16..32], mtx) as u32;
    nz0 | (nz1 << 1)
}

/// `Copy`: bloco `w`x`h` com passo `BPS`.
pub fn copy_block(src: &[u8], src_off: usize, dst: &mut [u8], dst_off: usize, w: usize, h: usize) {
    for y in 0..h {
        dst[dst_off + y * BPS..dst_off + y * BPS + w].copy_from_slice(&src[src_off + y * BPS..src_off + y * BPS + w]);
    }
}
