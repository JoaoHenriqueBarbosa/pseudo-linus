//! `ImagingResample` da libImaging (`Resample.c` do Pillow 11.1.0): redimensionamento em duas
//! passadas separáveis (horizontal e vertical) com os filtros box, bilinear, hamming, bicubic e
//! lanczos. Os coeficientes são calculados em `double` e, nas imagens de 8 bits, convertidos para
//! ponto fixo com 22 bits de fração, como no original, para dar os mesmos bytes.
//!
//! Portado da libImaging (MIT-CMU: Copyright © 1997-2011 Secret Labs AB, © 1995-2011 Fredrik Lundh
//! e colaboradores, © 2010 Jeffrey A. Clark e colaboradores).

use super::image::{Image, PixType};
use std::f64::consts::PI;

const PRECISION_BITS: i32 = 32 - 8 - 2;

pub const NEAREST: i32 = 0;
pub const LANCZOS: i32 = 1;
pub const BILINEAR: i32 = 2;
pub const BICUBIC: i32 = 3;
pub const BOX: i32 = 4;
pub const HAMMING: i32 = 5;

struct Filter {
    f: fn(f64) -> f64,
    support: f64,
}

fn box_filter(x: f64) -> f64 {
    if x > -0.5 && x <= 0.5 { 1.0 } else { 0.0 }
}

fn bilinear_filter(x: f64) -> f64 {
    let x = x.abs();
    if x < 1.0 { 1.0 - x } else { 0.0 }
}

fn hamming_filter(x: f64) -> f64 {
    let x = x.abs();
    if x == 0.0 {
        return 1.0;
    }
    if x >= 1.0 {
        return 0.0;
    }
    let x = x * PI;
    // `0.54f + 0.46f * cos(x)`: as constantes são `float` promovidas a `double`.
    x.sin() / x * (f64::from(0.54f32) + f64::from(0.46f32) * x.cos())
}

fn bicubic_filter(x: f64) -> f64 {
    let a = -0.5;
    let x = x.abs();
    if x < 1.0 {
        return ((a + 2.0) * x - (a + 3.0)) * x * x + 1.0;
    }
    if x < 2.0 {
        return (((x - 5.0) * x + 8.0) * x - 4.0) * a;
    }
    0.0
}

fn sinc_filter(x: f64) -> f64 {
    if x == 0.0 {
        return 1.0;
    }
    let x = x * PI;
    x.sin() / x
}

fn lanczos_filter(x: f64) -> f64 {
    if (-3.0..3.0).contains(&x) { sinc_filter(x) * sinc_filter(x / 3.0) } else { 0.0 }
}

/// `clip8_lookups[in >> PRECISION_BITS]`: a tabela cobre de -640 a 639 e satura em 0 e 255.
fn clip8(v: i32) -> u8 {
    (v >> PRECISION_BITS).clamp(0, 255) as u8
}

/// `ROUND_UP(f)`.
fn round_up(f: f64) -> i32 {
    if f >= 0.0 { (f + 0.5) as i32 } else { (f - 0.5) as i32 }
}

/// `precompute_coeffs`: `(ksize, bounds, coeficientes)`.
fn precompute_coeffs(in_size: i32, in0: f32, in1: f32, out_size: i32, fp: &Filter) -> (usize, Vec<i32>, Vec<f64>) {
    let scale = f64::from(in1 - in0) / f64::from(out_size);
    let filterscale = scale.max(1.0);
    let support = fp.support * filterscale;
    let ksize = (support.ceil() as usize) * 2 + 1;
    let n = out_size as usize;
    let mut kk = vec![0f64; n * ksize];
    let mut bounds = vec![0i32; n * 2];
    for xx in 0..n {
        let center = f64::from(in0) + (xx as f64 + 0.5) * scale;
        let ss = 1.0 / filterscale;
        let xmin = ((center - support + 0.5) as i32).max(0);
        let xmax = ((center + support + 0.5) as i32).min(in_size) - xmin;
        let k = &mut kk[xx * ksize..(xx + 1) * ksize];
        let mut ww = 0.0;
        for x in 0..xmax.max(0) as usize {
            let w = (fp.f)((x as f64 + f64::from(xmin) - center + 0.5) * ss);
            k[x] = w;
            ww += w;
        }
        if ww != 0.0 {
            for v in k.iter_mut().take(xmax.max(0) as usize) {
                *v /= ww;
            }
        }
        bounds[xx * 2] = xmin;
        bounds[xx * 2 + 1] = xmax;
    }
    (ksize, bounds, kk)
}

/// `normalize_coeffs_8bpc`.
fn normalize_8bpc(kk: &[f64]) -> Vec<i32> {
    let one = f64::from(1 << PRECISION_BITS);
    kk.iter().map(|&v| if v < 0.0 { (-0.5 + v * one) as i32 } else { (0.5 + v * one) as i32 }).collect()
}

/// As bandas que a passada de 8 bits calcula no armazenamento de 32 bits; o resto vira 0.
fn channels(im: &Image) -> &'static [usize] {
    match im.bands {
        2 => &[0, 3],
        3 => &[0, 1, 2],
        _ => &[0, 1, 2, 3],
    }
}

/// Uma passada. `horizontal`: o pixel de saída `(xx, yy)` lê a linha `yy + offset` nas colunas dos
/// `bounds[xx]`; senão lê a coluna `xx` nas linhas dos `bounds[yy]`.
fn pass(out: &mut Image, inp: &Image, offset: i32, ksize: usize, bounds: &[i32], kk: &[f64], horizontal: bool) {
    let src = |xx: i32, yy: i32, i: i32| if horizontal { (i, yy + offset) } else { (xx, i) };
    let pick = |xx: i32, yy: i32| {
        let j = if horizontal { xx } else { yy } as usize;
        (bounds[j * 2], bounds[j * 2 + 1], j * ksize)
    };
    match inp.kind {
        PixType::Uint8 => {
            let k8 = normalize_8bpc(kk);
            let chans: &[usize] = if inp.is8() { &[0] } else { channels(inp) };
            for yy in 0..out.ysize {
                for xx in 0..out.xsize {
                    let (min, max, kb) = pick(xx, yy);
                    let mut px = [0u8; 4];
                    for &c in chans {
                        let mut ss: i32 = 1 << (PRECISION_BITS - 1);
                        for i in 0..max {
                            let (sx, sy) = src(xx, yy, i + min);
                            let v = i32::from(inp.data[inp.offset(sx, sy) + c]);
                            ss = ss.wrapping_add(v.wrapping_mul(k8[kb + i as usize]));
                        }
                        px[c] = clip8(ss);
                    }
                    let o = out.offset(xx, yy);
                    if out.is8() {
                        out.data[o] = px[0];
                    } else {
                        out.data[o..o + 4].copy_from_slice(&px);
                    }
                }
            }
        }
        PixType::Special => {
            let big = inp.mode == "I;16B";
            let (lo, hi) = if big { (1, 0) } else { (0, 1) };
            for yy in 0..out.ysize {
                for xx in 0..out.xsize {
                    let (min, max, kb) = pick(xx, yy);
                    let mut ss = 0.0;
                    for i in 0..max {
                        let (sx, sy) = src(xx, yy, i + min);
                        let o = inp.offset(sx, sy);
                        let v = i32::from(inp.data[o + lo]) + (i32::from(inp.data[o + hi]) << 8);
                        ss += f64::from(v) * kk[kb + i as usize];
                    }
                    let s = round_up(ss);
                    let o = out.offset(xx, yy);
                    // `CLIP8(ss_int % 256)` e `CLIP8(ss_int >> 8)`, com o resto do C (sinal do dividendo).
                    out.data[o + lo] = (s % 256).clamp(0, 255) as u8;
                    out.data[o + hi] = (s >> 8).clamp(0, 255) as u8;
                }
            }
        }
        PixType::Int32 | PixType::Float32 => {
            let float = inp.kind == PixType::Float32;
            for yy in 0..out.ysize {
                for xx in 0..out.xsize {
                    let (min, max, kb) = pick(xx, yy);
                    let mut ss = 0.0;
                    for i in 0..max {
                        let (sx, sy) = src(xx, yy, i + min);
                        let o = inp.offset(sx, sy);
                        let b = [inp.data[o], inp.data[o + 1], inp.data[o + 2], inp.data[o + 3]];
                        let v = if float { f64::from(f32::from_le_bytes(b)) } else { f64::from(i32::from_le_bytes(b)) };
                        ss += v * kk[kb + i as usize];
                    }
                    let o = out.offset(xx, yy);
                    let b = if float { (ss as f32).to_le_bytes() } else { round_up(ss).to_le_bytes() };
                    out.data[o..o + 4].copy_from_slice(&b);
                }
            }
        }
    }
}

fn blank_like(im: &Image, xsize: i32, ysize: i32) -> Image {
    let mut out = Image::new(&im.mode, xsize, ysize).expect("modo já validado");
    out.palette = im.palette.clone();
    out
}

/// `ImagingResample(imIn, xsize, ysize, filter, box)`.
pub fn resample(inp: &Image, xsize: i32, ysize: i32, filter: i32, b: [f32; 4]) -> Result<Image, String> {
    if inp.mode == "P" || inp.mode == "1" || (inp.kind == PixType::Special && !inp.is_i16()) {
        return Err("image has wrong mode".into());
    }
    let fp = match filter {
        BOX => Filter { f: box_filter, support: 0.5 },
        BILINEAR => Filter { f: bilinear_filter, support: 1.0 },
        HAMMING => Filter { f: hamming_filter, support: 1.0 },
        BICUBIC => Filter { f: bicubic_filter, support: 2.0 },
        LANCZOS => Filter { f: lanczos_filter, support: 3.0 },
        _ => return Err("unsupported resampling filter".into()),
    };
    let need_h = xsize != inp.xsize || b[0] != 0.0 || b[2] != xsize as f32;
    let need_v = ysize != inp.ysize || b[1] != 0.0 || b[3] != ysize as f32;
    let (ks_h, bounds_h, kk_h) = precompute_coeffs(inp.xsize, b[0], b[2], xsize, &fp);
    let (ks_v, mut bounds_v, kk_v) = precompute_coeffs(inp.ysize, b[1], b[3], ysize, &fp);
    let first = bounds_v[0];
    let n = ysize as usize;
    let last = bounds_v[n * 2 - 2] + bounds_v[n * 2 - 1];
    let mut cur: Option<Image> = None;
    if need_h {
        for i in 0..n {
            bounds_v[i * 2] -= first;
        }
        let mut tmp = blank_like(inp, xsize, last - first);
        pass(&mut tmp, inp, first, ks_h, &bounds_h, &kk_h, true);
        cur = Some(tmp);
    }
    if need_v {
        let src = cur.as_ref().unwrap_or(inp);
        let mut out = blank_like(src, src.xsize, ysize);
        pass(&mut out, src, 0, ks_v, &bounds_v, &kk_v, false);
        cur = Some(out);
    }
    Ok(cur.unwrap_or_else(|| inp.clone()))
}
