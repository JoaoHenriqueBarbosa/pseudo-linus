//! Passo de saída do decodificador: a suavização entre blocos do progressivo
//! (`decompress_smooth_data` do `jdcoefct.c`), a IDCT, o aumento de amostragem do croma
//! (`jdsample.c`, com o modo "fancy" padrão) e a conversão de cor (`jdcolor.c`).
//!
//! O original trabalha por grupos de linhas com linhas de contexto (`jdmainct.c`); aqui cada
//! componente vira um plano inteiro e o contexto é a linha vizinha com as bordas repetidas, que é
//! o que os ponteiros do `make_funny_pointers` e do `set_bottom_pointers` fazem.

use crate::decode::{Component, Frame};
use crate::idct::idct_islow;
use crate::ColorSpace;

const SCALEBITS: i32 = 16;
const ONE_HALF: i64 = 1 << (SCALEBITS - 1);

fn fix(x: f64) -> i64 {
    (x * f64::from(1 << SCALEBITS) + 0.5) as i64
}

/// `sample_range_limit[x]` para os valores que a conversão de cor produz.
fn clamp(x: i64) -> u8 {
    x.clamp(0, 255) as u8
}

struct YccTables {
    cr_r: [i64; 256],
    cb_b: [i64; 256],
    cr_g: [i64; 256],
    cb_g: [i64; 256],
}

impl YccTables {
    fn new() -> YccTables {
        let mut t = YccTables { cr_r: [0; 256], cb_b: [0; 256], cr_g: [0; 256], cb_g: [0; 256] };
        for i in 0..256 {
            let x = i as i64 - 128;
            t.cr_r[i] = (fix(1.40200) * x + ONE_HALF) >> SCALEBITS;
            t.cb_b[i] = (fix(1.77200) * x + ONE_HALF) >> SCALEBITS;
            t.cr_g[i] = -fix(0.71414) * x;
            t.cb_g[i] = -fix(0.34414) * x + ONE_HALF;
        }
        t
    }

    fn rgb(&self, y: u8, cb: u8, cr: u8) -> [u8; 3] {
        let (y, cb, cr) = (i64::from(y), usize::from(cb), usize::from(cr));
        [
            clamp(y + self.cr_r[cr]),
            clamp(y + ((self.cb_g[cb] + self.cr_g[cr]) >> SCALEBITS)),
            clamp(y + self.cb_b[cb]),
        ]
    }
}

const Q01: usize = 1;
const Q10: usize = 8;
const Q20: usize = 16;
const Q11: usize = 9;
const Q02: usize = 2;
const Q03: usize = 3;
const Q12: usize = 10;
const Q21: usize = 17;
const Q30: usize = 24;

/// `smoothing_ok`: devolve o `coef_bits_latch` de cada componente quando a suavização é útil.
fn smoothing_latch(f: &Frame) -> Option<Vec<[i32; 10]>> {
    if !f.progressive {
        return None;
    }
    let mut useful = false;
    let mut latch = Vec::with_capacity(f.comps.len());
    for (ci, c) in f.comps.iter().enumerate() {
        let q = c.quant?;
        if [0, Q01, Q10, Q20, Q11, Q02, Q03, Q12, Q21, Q30].iter().any(|&k| q[k] == 0) {
            return None;
        }
        let bits = &f.coef_bits[ci];
        if bits[0] < 0 {
            return None;
        }
        let mut l = [0i32; 10];
        l[0] = bits[0];
        for k in 1..10 {
            l[k] = bits[k];
            if bits[k] != 0 {
                useful = true;
            }
        }
        latch.push(l);
    }
    useful.then_some(latch)
}

/// Uma previsão de coeficiente AC (ou DC) do `decompress_smooth_data`.
fn predict(q00: i64, qk: i64, num_dc: i64, al: i32, limit: bool) -> i16 {
    let num = q00 * num_dc;
    let mut pred;
    if num >= 0 {
        pred = ((qk << 7) + num) / (qk << 8);
        if limit && al > 0 && pred >= (1 << al) {
            pred = (1 << al) - 1;
        }
    } else {
        pred = ((qk << 7) - num) / (qk << 8);
        if limit && al > 0 && pred >= (1 << al) {
            pred = (1 << al) - 1;
        }
        pred = -pred;
    }
    pred as i32 as i16
}

/// IDCT de todos os blocos úteis de uma componente, com a suavização quando ela se aplica.
fn component_plane(f: &Frame, ci: usize, latch: Option<&[i32; 10]>) -> Vec<u8> {
    let c = &f.comps[ci];
    let quant = c.quant.unwrap_or([1; 64]);
    let stride = c.width_in_blocks * 8;
    let mut plane = vec![0u8; stride * c.height_in_blocks * 8];
    let block = |bx: usize, by: usize| &c.coefs[by * c.blocks_w + bx];
    let Some(bits) = latch else {
        for by in 0..c.height_in_blocks {
            for bx in 0..c.width_in_blocks {
                idct_islow(block(bx, by), &quant, &mut plane, by * 8 * stride + bx * 8, stride);
            }
        }
        return plane;
    };
    let total_imcu = f.height.div_ceil(f.max_v * 8);
    let last_imcu = total_imcu - 1;
    let change_dc = bits[1..10].iter().all(|&b| b == -1);
    let q = |k: usize| i64::from(quant[k]);
    let q00 = q(0);
    let last_col = c.width_in_blocks - 1;
    for row in 0..c.height_in_blocks {
        let imcu = row / c.v_samp;
        let brow = row % c.v_samp;
        let block_rows = if imcu < last_imcu {
            c.v_samp
        } else {
            match c.height_in_blocks % c.v_samp {
                0 => c.v_samp,
                n => n,
            }
        };
        let prev = if brow > 0 || imcu > 0 { row - 1 } else { row };
        let prev_prev = if brow > 1 || imcu > 1 { row - 2 } else { prev };
        let next = if brow + 1 < block_rows || imcu < last_imcu { row + 1 } else { row };
        let next_next = if brow + 2 < block_rows || imcu + 1 < last_imcu { row + 2 } else { next };
        let dc = |r: usize, col: usize| i64::from(block(col, r)[0]);
        let rows = [prev_prev, prev, row, next, next_next];
        // DC01..DC25 em cinco linhas de cinco colunas (a coluna 2 é a do bloco atual).
        let mut d = [[0i64; 5]; 5];
        for (i, &r) in rows.iter().enumerate() {
            d[i] = [dc(r, 0); 5];
        }
        for bx in 0..=last_col {
            let mut ws = *block(bx, row);
            if bx == 0 && bx < last_col {
                for (i, &r) in rows.iter().enumerate() {
                    d[i][3] = dc(r, 1);
                }
            }
            if bx + 1 < last_col {
                for (i, &r) in rows.iter().enumerate() {
                    d[i][4] = dc(r, bx + 2);
                }
            }
            let dcv = |n: usize| d[(n - 1) / 5][(n - 1) % 5];
            let (dc01, dc02, dc03, dc04, dc05) = (dcv(1), dcv(2), dcv(3), dcv(4), dcv(5));
            let (dc06, dc07, dc08, dc09, dc10) = (dcv(6), dcv(7), dcv(8), dcv(9), dcv(10));
            let (dc11, dc12, dc13, dc14, dc15) = (dcv(11), dcv(12), dcv(13), dcv(14), dcv(15));
            let (dc16, dc17, dc18, dc19, dc20) = (dcv(16), dcv(17), dcv(18), dcv(19), dcv(20));
            let (dc21, dc22, dc23, dc24, dc25) = (dcv(21), dcv(22), dcv(23), dcv(24), dcv(25));
            if bits[1] != 0 && ws[1] == 0 {
                let n = if change_dc {
                    -dc01 - dc02 + dc04 + dc05 - 3 * dc06 + 13 * dc07 - 13 * dc09 + 3 * dc10 - 3 * dc11 + 38 * dc12
                        - 38 * dc14
                        + 3 * dc15
                        - 3 * dc16
                        + 13 * dc17
                        - 13 * dc19
                        + 3 * dc20
                        - dc21
                        - dc22
                        + dc24
                        + dc25
                } else {
                    -7 * dc11 + 50 * dc12 - 50 * dc14 + 7 * dc15
                };
                ws[1] = predict(q00, q(Q01), n, bits[1], true);
            }
            if bits[2] != 0 && ws[8] == 0 {
                let n = if change_dc {
                    -dc01 - 3 * dc02 - 3 * dc03 - 3 * dc04 - dc05 - dc06 + 13 * dc07 + 38 * dc08 + 13 * dc09 - dc10
                        + dc16
                        - 13 * dc17
                        - 38 * dc18
                        - 13 * dc19
                        + dc20
                        + dc21
                        + 3 * dc22
                        + 3 * dc23
                        + 3 * dc24
                        + dc25
                } else {
                    -7 * dc03 + 50 * dc08 - 50 * dc18 + 7 * dc23
                };
                ws[8] = predict(q00, q(Q10), n, bits[2], true);
            }
            if bits[3] != 0 && ws[16] == 0 {
                let n = if change_dc {
                    dc03 + 2 * dc07 + 7 * dc08 + 2 * dc09 - 5 * dc12 - 14 * dc13 - 5 * dc14 + 2 * dc17 + 7 * dc18
                        + 2 * dc19
                        + dc23
                } else {
                    -dc03 + 13 * dc08 - 24 * dc13 + 13 * dc18 - dc23
                };
                ws[16] = predict(q00, q(Q20), n, bits[3], true);
            }
            if bits[4] != 0 && ws[9] == 0 {
                let n = if change_dc {
                    -dc01 + dc05 + 9 * dc07 - 9 * dc09 - 9 * dc17 + 9 * dc19 + dc21 - dc25
                } else {
                    dc10 + dc16 - 10 * dc17 + 10 * dc19 - dc02 - dc20 + dc22 - dc24 + dc04 - dc06 + 10 * dc07
                        - 10 * dc09
                };
                ws[9] = predict(q00, q(Q11), n, bits[4], true);
            }
            if bits[5] != 0 && ws[2] == 0 {
                let n = if change_dc {
                    2 * dc07 - 5 * dc08 + 2 * dc09 + dc11 + 7 * dc12 - 14 * dc13 + 7 * dc14 + dc15 + 2 * dc17
                        - 5 * dc18
                        + 2 * dc19
                } else {
                    -dc11 + 13 * dc12 - 24 * dc13 + 13 * dc14 - dc15
                };
                ws[2] = predict(q00, q(Q02), n, bits[5], true);
            }
            if change_dc {
                if bits[6] != 0 && ws[3] == 0 {
                    ws[3] = predict(q00, q(Q03), dc07 - dc09 + 2 * dc12 - 2 * dc14 + dc17 - dc19, bits[6], true);
                }
                if bits[7] != 0 && ws[10] == 0 {
                    ws[10] = predict(q00, q(Q12), dc07 - 3 * dc08 + dc09 - dc17 + 3 * dc18 - dc19, bits[7], true);
                }
                if bits[8] != 0 && ws[17] == 0 {
                    ws[17] = predict(q00, q(Q21), dc07 - dc09 - 3 * dc12 + 3 * dc14 + dc17 - dc19, bits[8], true);
                }
                if bits[9] != 0 && ws[24] == 0 {
                    ws[24] = predict(q00, q(Q30), dc07 + 2 * dc08 + dc09 - dc17 - 2 * dc18 - dc19, bits[9], true);
                }
                let n = -2 * dc01 - 6 * dc02 - 8 * dc03 - 6 * dc04 - 2 * dc05 - 6 * dc06 + 6 * dc07 + 42 * dc08
                    + 6 * dc09
                    - 6 * dc10
                    - 8 * dc11
                    + 42 * dc12
                    + 152 * dc13
                    + 42 * dc14
                    - 8 * dc15
                    - 6 * dc16
                    + 6 * dc17
                    + 42 * dc18
                    + 6 * dc19
                    - 6 * dc20
                    - 2 * dc21
                    - 6 * dc22
                    - 8 * dc23
                    - 6 * dc24
                    - 2 * dc25;
                ws[0] = predict(q00, q00, n, 0, false);
            }
            idct_islow(&ws, &quant, &mut plane, row * 8 * stride + bx * 8, stride);
            for r in d.iter_mut() {
                r.copy_within(1..5, 0);
            }
        }
    }
    plane
}

/// Como uma componente chega ao tamanho de saída (`jinit_upsampler`).
#[derive(Clone, Copy)]
enum Up {
    Full,
    H2V1Fancy,
    H2V1,
    H1V2Fancy,
    H2V2Fancy,
    H2V2,
    Int(usize, usize),
}

fn up_method(f: &Frame, c: &Component, fancy: bool) -> Result<Up, crate::Error> {
    let (hi, ho, vi, vo) = (c.h_samp, f.max_h, c.v_samp, f.max_v);
    Ok(if hi == ho && vi == vo {
        Up::Full
    } else if hi * 2 == ho && vi == vo {
        if fancy && c.downsampled_width > 2 { Up::H2V1Fancy } else { Up::H2V1 }
    } else if hi == ho && vi * 2 == vo && fancy {
        Up::H1V2Fancy
    } else if hi * 2 == ho && vi * 2 == vo {
        if fancy && c.downsampled_width > 2 { Up::H2V2Fancy } else { Up::H2V2 }
    } else if ho % hi == 0 && vo % vi == 0 {
        Up::Int(ho / hi, vo / vi)
    } else {
        return Err(crate::Error::FractionalSampling);
    })
}

/// A linha `y` de saída da componente, com `width` amostras.
fn upsample_row(c: &Component, plane: &[u8], m: Up, y: usize, width: usize, out: &mut Vec<u8>) {
    let stride = c.width_in_blocks * 8;
    let dw = c.downsampled_width;
    let last_row = c.downsampled_height - 1;
    let row = |r: usize| &plane[r * stride..r * stride + stride];
    out.clear();
    match m {
        Up::Full => out.extend_from_slice(&row(y)[..width]),
        Up::H2V1 | Up::H2V2 => {
            let r = row(if matches!(m, Up::H2V2) { y / 2 } else { y });
            out.extend((0..width).map(|x| r[x / 2]));
        }
        Up::Int(h, v) => {
            let r = row(y / v);
            out.extend((0..width).map(|x| r[x / h]));
        }
        Up::H2V1Fancy => {
            let r = row(y);
            let v = |i: usize| i32::from(r[i]);
            out.push(r[0]);
            out.push(((v(0) * 3 + v(1) + 2) >> 2) as u8);
            for i in 1..dw - 1 {
                let inv = v(i) * 3;
                out.push(((inv + v(i - 1) + 1) >> 2) as u8);
                out.push(((inv + v(i + 1) + 2) >> 2) as u8);
            }
            out.push(((v(dw - 1) * 3 + v(dw - 2) + 1) >> 2) as u8);
            out.push(r[dw - 1]);
            out.truncate(width);
        }
        Up::H1V2Fancy | Up::H2V2Fancy => {
            let inrow = y / 2;
            let (near, far, bias) = if y % 2 == 0 {
                (inrow, inrow.saturating_sub(1), 1)
            } else {
                (inrow, (inrow + 1).min(last_row), 2)
            };
            let (r0, r1) = (row(near), row(far));
            if matches!(m, Up::H1V2Fancy) {
                out.extend((0..width).map(|x| ((i32::from(r0[x]) * 3 + i32::from(r1[x]) + bias) >> 2) as u8));
            } else {
                let cs = |i: usize| i32::from(r0[i]) * 3 + i32::from(r1[i]);
                let mut this = cs(0);
                let mut next = cs(1);
                out.push(((this * 4 + 8) >> 4) as u8);
                out.push(((this * 3 + next + 7) >> 4) as u8);
                let mut last = this;
                this = next;
                for i in 2..dw {
                    next = cs(i);
                    out.push(((this * 3 + last + 8) >> 4) as u8);
                    out.push(((this * 3 + next + 7) >> 4) as u8);
                    last = this;
                    this = next;
                }
                out.push(((this * 3 + last + 8) >> 4) as u8);
                out.push(((this * 4 + 7) >> 4) as u8);
                out.truncate(width);
            }
        }
    }
}

/// Monta a imagem de saída: `out` amostras intercaladas por pixel, `out_components` por pixel.
pub fn output(f: &Frame, jcs: ColorSpace, ocs: ColorSpace, fancy: bool) -> Result<(Vec<u8>, usize), crate::Error> {
    let n = f.comps.len();
    let expect = match jcs {
        ColorSpace::Grayscale => Some(1),
        ColorSpace::Rgb | ColorSpace::YCbCr => Some(3),
        ColorSpace::Cmyk | ColorSpace::Ycck => Some(4),
        ColorSpace::Unknown => None,
    };
    if expect.is_some_and(|e| e != n) {
        return Err(crate::Error::BadColorSpace);
    }
    use ColorSpace::*;
    let (out_comps, needed): (usize, Vec<bool>) = match (ocs, jcs) {
        (Grayscale, Grayscale | YCbCr) => (1, (0..n).map(|i| i == 0).collect()),
        (Grayscale, Rgb) | (Rgb, YCbCr | Grayscale | Rgb) => (if ocs == Grayscale { 1 } else { 3 }, vec![true; n]),
        (Cmyk, Ycck | Cmyk) => (4, vec![true; n]),
        (o, j) if o == j => (n, vec![true; n]),
        _ => return Err(crate::Error::ConversionNotImplemented),
    };
    // `do_block_smoothing` é verdadeiro por padrão e o Pillow não o desliga.
    let latch = smoothing_latch(f);
    let planes: Vec<Option<Vec<u8>>> = (0..n)
        .map(|ci| needed[ci].then(|| component_plane(f, ci, latch.as_ref().map(|l| &l[ci]))))
        .collect();
    let methods = f.comps.iter().map(|c| up_method(f, c, fancy)).collect::<Result<Vec<_>, _>>()?;
    let (w, h) = (f.width, f.height);
    let mut out = vec![0u8; w * h * out_comps];
    let ycc = YccTables::new();
    let mut rows: Vec<Vec<u8>> = vec![Vec::new(); n];
    for y in 0..h {
        for ci in 0..n {
            if let Some(p) = &planes[ci] {
                upsample_row(&f.comps[ci], p, methods[ci], y, w, &mut rows[ci]);
            }
        }
        let o = &mut out[y * w * out_comps..(y + 1) * w * out_comps];
        match (ocs, jcs) {
            (Grayscale, Grayscale | YCbCr) => o.copy_from_slice(&rows[0]),
            (Grayscale, Rgb) => {
                for x in 0..w {
                    let (r, g, b) = (i64::from(rows[0][x]), i64::from(rows[1][x]), i64::from(rows[2][x]));
                    o[x] = ((fix(0.29900) * r + fix(0.58700) * g + fix(0.11400) * b + ONE_HALF) >> SCALEBITS) as u8;
                }
            }
            (Rgb, YCbCr) => {
                for x in 0..w {
                    o[x * 3..x * 3 + 3].copy_from_slice(&ycc.rgb(rows[0][x], rows[1][x], rows[2][x]));
                }
            }
            (Rgb, Grayscale) => {
                for x in 0..w {
                    o[x * 3..x * 3 + 3].fill(rows[0][x]);
                }
            }
            (Cmyk, Ycck) => {
                for x in 0..w {
                    let [r, g, b] = ycc.rgb_unclamped(rows[0][x], rows[1][x], rows[2][x]);
                    o[x * 4] = clamp(255 - r);
                    o[x * 4 + 1] = clamp(255 - g);
                    o[x * 4 + 2] = clamp(255 - b);
                    o[x * 4 + 3] = rows[3][x];
                }
            }
            _ => {
                for x in 0..w {
                    for ci in 0..n {
                        o[x * n + ci] = rows[ci][x];
                    }
                }
            }
        }
    }
    Ok((out, out_comps))
}

impl YccTables {
    fn rgb_unclamped(&self, y: u8, cb: u8, cr: u8) -> [i64; 3] {
        let (y, cb, cr) = (i64::from(y), usize::from(cb), usize::from(cr));
        [y + self.cr_r[cr], y + ((self.cb_g[cb] + self.cr_g[cr]) >> SCALEBITS), y + self.cb_b[cb]]
    }
}
