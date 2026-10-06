//! `ImagingReduce` da libImaging (`Reduce.c` do Pillow 11.1.0): reduz a imagem por fatores
//! inteiros tirando a média de cada bloco `xscale` × `yscale`. É o que o `Image.reduce` e o
//! `thumbnail`/`resize` com `reducing_gap` usam antes do resample.
//!
//! As variantes otimizadas do original (1x2, 2x1, 2x2, 4x4) dividem com deslocamento exato; as
//! demais multiplicam por `2^32 / (256·n)` calculado em `float` e deslocam 24 bits. Os dois
//! caminhos estão reproduzidos para dar os mesmos bytes. A última coluna e a última linha
//! parciais (`ImagingReduceCorners`) usam a média do pedaço que existe.
//!
//! Portado da libImaging (MIT-CMU: Copyright © 2019 Alexander Karpinsky e Secret Labs AB).

use super::image::{Image, PixType};

/// `division_UINT32(divider, 8)`.
fn multiplier(divider: i32) -> u32 {
    let max_dividend = (256u32).wrapping_mul(divider as u32);
    let max_int = (1u32 << 30) as f32 * 4.0;
    (max_int / max_dividend as f32) as u32
}

/// `ROUND_UP` do original.
fn round_up(f: f64) -> i32 {
    if f >= 0.0 { (f + 0.5) as i32 } else { (f - 0.5) as i32 }
}

fn byte(im: &Image, x: i32, y: i32, c: usize) -> u32 {
    u32::from(im.data[im.offset(x, y) + c])
}

/// Média de um bloco de 8 bits por canal, com a divisão da variante que o original escolheria.
fn mean_u8(sum: u32, n: i32, shift: Option<u32>) -> u8 {
    let amend = (n / 2) as u32;
    match shift {
        Some(s) => ((sum + amend) >> s) as u8,
        None => ((sum + amend).wrapping_mul(multiplier(n)) >> 24) as u8,
    }
}

/// Soma de um bloco de `float`/`int32` na ordem do original: linhas aos pares, colunas aos pares,
/// cada grupo de quatro somado no tipo do pixel antes de entrar no acumulador `double`.
fn sum_32(im: &Image, x0: i32, y0: i32, xs: i32, ys: i32) -> f64 {
    let float = im.kind == PixType::Float32;
    let word = |x: i32, y: i32| {
        let o = im.offset(x, y);
        [im.data[o], im.data[o + 1], im.data[o + 2], im.data[o + 3]]
    };
    let fv = |x: i32, y: i32| f32::from_le_bytes(word(x, y));
    let iv = |x: i32, y: i32| i32::from_le_bytes(word(x, y));
    let mut ss = 0f64;
    let mut yy = y0;
    while yy < y0 + ys - 1 {
        let mut xx = x0;
        while xx < x0 + xs - 1 {
            ss += if float {
                f64::from(fv(xx, yy) + fv(xx + 1, yy) + fv(xx, yy + 1) + fv(xx + 1, yy + 1))
            } else {
                f64::from(iv(xx, yy).wrapping_add(iv(xx + 1, yy)).wrapping_add(iv(xx, yy + 1)).wrapping_add(iv(xx + 1, yy + 1)))
            };
            xx += 2;
        }
        if xs & 1 != 0 {
            ss += if float { f64::from(fv(xx, yy) + fv(xx, yy + 1)) } else { f64::from(iv(xx, yy).wrapping_add(iv(xx, yy + 1))) };
        }
        yy += 2;
    }
    if ys & 1 != 0 {
        let mut xx = x0;
        while xx < x0 + xs - 1 {
            ss += if float { f64::from(fv(xx, yy) + fv(xx + 1, yy)) } else { f64::from(iv(xx, yy).wrapping_add(iv(xx + 1, yy))) };
            xx += 2;
        }
        if xs & 1 != 0 {
            ss += if float { f64::from(fv(xx, yy)) } else { f64::from(iv(xx, yy)) };
        }
    }
    ss
}

/// Soma simples de um pedaço de canto em `float`/`int32` (o `ImagingReduceCorners_32bpc`).
fn sum_32_flat(im: &Image, x0: i32, y0: i32, x1: i32, y1: i32) -> f64 {
    let mut ss = 0f64;
    for yy in y0..y1 {
        for xx in x0..x1 {
            let o = im.offset(xx, yy);
            let b = [im.data[o], im.data[o + 1], im.data[o + 2], im.data[o + 3]];
            ss += if im.kind == PixType::Float32 { f64::from(f32::from_le_bytes(b)) } else { f64::from(i32::from_le_bytes(b)) };
        }
    }
    ss
}

fn put_32(out: &mut Image, x: i32, y: i32, v: f64) {
    let o = out.offset(x, y);
    let b = if out.kind == PixType::Float32 { (v as f32).to_le_bytes() } else { round_up(v).to_le_bytes() };
    out.data[o..o + 4].copy_from_slice(&b);
}

/// `ImagingReduce(imIn, xscale, yscale, box)`, com `box` = `(left, top, width, height)`.
pub fn reduce(im: &Image, xscale: i32, yscale: i32, b: [i32; 4]) -> Result<Image, String> {
    if im.mode == "P" || im.mode == "1" || im.kind == PixType::Special {
        return Err("image has wrong mode".into());
    }
    let mut out = Image::new(&im.mode, (b[2] + xscale - 1) / xscale, (b[3] + yscale - 1) / yscale).expect("modo já validado");
    let (nx, ny) = (b[2] / xscale, b[3] / yscale);
    let (rx, ry) = (b[2] % xscale, b[3] % yscale);
    if im.kind == PixType::Uint8 {
        let shift = match (xscale, yscale) {
            (1, 2) | (2, 1) => Some(1),
            (2, 2) => Some(2),
            (4, 4) => Some(4),
            _ => None,
        };
        let chans: &[usize] = if im.is8() {
            &[0]
        } else {
            match im.bands {
                2 => &[0, 3],
                3 => &[0, 1, 2],
                _ => &[0, 1, 2, 3],
            }
        };
        let block = |out: &mut Image, x: i32, y: i32, x0: i32, y0: i32, w: i32, h: i32, chans: &[usize], shift: Option<u32>| {
            let mut px = [0u8; 4];
            for &c in chans {
                let mut s = 0u32;
                for yy in y0..y0 + h {
                    for xx in x0..x0 + w {
                        s += byte(im, xx, yy, c);
                    }
                }
                px[c] = mean_u8(s, w * h, shift);
            }
            let o = out.offset(x, y);
            if out.is8() {
                out.data[o] = px[0];
            } else {
                out.data[o..o + 4].copy_from_slice(&px);
            }
        };
        for y in 0..ny {
            for x in 0..nx {
                block(&mut out, x, y, b[0] + x * xscale, b[1] + y * yscale, xscale, yscale, chans, shift);
            }
        }
        // Os cantos calculam as quatro bandas no armazenamento de 32 bits.
        let all: &[usize] = if im.is8() { &[0] } else { &[0, 1, 2, 3] };
        if rx != 0 {
            for y in 0..ny {
                block(&mut out, nx, y, b[0] + nx * xscale, b[1] + y * yscale, rx, yscale, all, None);
            }
        }
        if ry != 0 {
            for x in 0..nx {
                block(&mut out, x, ny, b[0] + x * xscale, b[1] + ny * yscale, xscale, ry, all, None);
            }
        }
        if rx != 0 && ry != 0 {
            block(&mut out, nx, ny, b[0] + nx * xscale, b[1] + ny * yscale, rx, ry, all, None);
        }
    } else {
        let m = 1.0 / f64::from(yscale * xscale);
        for y in 0..ny {
            for x in 0..nx {
                let ss = sum_32(im, b[0] + x * xscale, b[1] + y * yscale, xscale, yscale);
                put_32(&mut out, x, y, ss * m);
            }
        }
        if rx != 0 {
            let m = 1.0 / f64::from(rx * yscale);
            for y in 0..ny {
                let (x0, y0) = (b[0] + nx * xscale, b[1] + y * yscale);
                let ss = sum_32_flat(im, x0, y0, b[0] + b[2], y0 + yscale);
                put_32(&mut out, nx, y, ss * m);
            }
        }
        if ry != 0 {
            let m = 1.0 / f64::from(xscale * ry);
            for x in 0..nx {
                let (x0, y0) = (b[0] + x * xscale, b[1] + ny * yscale);
                let ss = sum_32_flat(im, x0, y0, x0 + xscale, b[1] + b[3]);
                put_32(&mut out, x, ny, ss * m);
            }
        }
        if rx != 0 && ry != 0 {
            let m = 1.0 / f64::from(rx * ry);
            let ss = sum_32_flat(im, b[0] + nx * xscale, b[1] + ny * yscale, b[0] + b[2], b[1] + b[3]);
            put_32(&mut out, nx, ny, ss * m);
        }
    }
    Ok(out)
}
