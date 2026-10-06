//! Conversões de modo da libImaging (`Convert.c` do Pillow 11.1.0): a tabela de conversores linha a
//! linha, o pontilhado de Floyd-Steinberg para `1` (`tobilevel`) e a saída de paleta
//! (`frompalette`).
//!
//! Portado da libImaging (MIT-CMU: Copyright © 1997-2006 Secret Labs AB, © 1995-1997 Fredrik Lundh).

use super::image::{Image, Palette};

/// `L(rgb)`: luminância em milésimos (ITU-R 601-2).
fn lum(p: &[u8]) -> i32 {
    i32::from(p[0]) * 299 + i32::from(p[1]) * 587 + i32::from(p[2]) * 114
}

/// `L24(rgb) >> 16`.
fn l24(p: &[u8]) -> u8 {
    ((u32::from(p[0]) * 19595 + u32::from(p[1]) * 38470 + u32::from(p[2]) * 7471 + 0x8000) >> 16) as u8
}

fn muldiv255(a: u32, b: u32) -> u32 {
    let t = a * b + 128;
    ((t >> 8) + t) >> 8
}

type Shuffler = fn(&mut [u8], &[u8], usize);

fn bit2l(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        o[x] = if i[x] != 0 { 255 } else { 0 };
    }
}

fn bit2rgb(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        let v = if i[x] != 0 { 255 } else { 0 };
        o[x * 4..x * 4 + 4].copy_from_slice(&[v, v, v, 255]);
    }
}

fn bit2i(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        let v: i32 = if i[x] != 0 { 255 } else { 0 };
        o[x * 4..x * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
}

fn bit2f(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        let v: f32 = if i[x] != 0 { 255.0 } else { 0.0 };
        o[x * 4..x * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
}

fn l2bit(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        o[x] = if i[x] >= 128 { 255 } else { 0 };
    }
}

fn l2rgb(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        let v = i[x];
        o[x * 4..x * 4 + 4].copy_from_slice(&[v, v, v, 255]);
    }
}

fn l2i(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        o[x * 4..x * 4 + 4].copy_from_slice(&i32::from(i[x]).to_le_bytes());
    }
}

fn l2f(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        o[x * 4..x * 4 + 4].copy_from_slice(&f32::from(i[x]).to_le_bytes());
    }
}

fn la2l(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        o[x] = i[x * 4];
    }
}

fn la2rgb(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        let v = i[x * 4];
        o[x * 4..x * 4 + 4].copy_from_slice(&[v, v, v, i[x * 4 + 3]]);
    }
}

fn la2la_premul(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        let a = u32::from(i[x * 4 + 3]);
        let p = muldiv255(u32::from(i[x * 4]), a) as u8;
        o[x * 4..x * 4 + 4].copy_from_slice(&[p, p, p, a as u8]);
    }
}

fn la_unpremul(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        let a = u32::from(i[x * 4 + 3]);
        let p = if a == 255 || a == 0 { i[x * 4] } else { ((255 * u32::from(i[x * 4])) / a).min(255) as u8 };
        o[x * 4..x * 4 + 4].copy_from_slice(&[p, p, p, a as u8]);
    }
}

fn rgb2bit(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        o[x] = if lum(&i[x * 4..]) >= 128_000 { 255 } else { 0 };
    }
}

fn rgb2l(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        o[x] = l24(&i[x * 4..]);
    }
}

fn rgb2la(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        let v = l24(&i[x * 4..]);
        o[x * 4..x * 4 + 4].copy_from_slice(&[v, v, v, 255]);
    }
}

fn rgba2la(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        let v = l24(&i[x * 4..]);
        o[x * 4..x * 4 + 4].copy_from_slice(&[v, v, v, i[x * 4 + 3]]);
    }
}

fn rgb2i(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        o[x * 4..x * 4 + 4].copy_from_slice(&i32::from(l24(&i[x * 4..])).to_le_bytes());
    }
}

fn rgb2f(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        let v = lum(&i[x * 4..]) as f32 / 1000.0;
        o[x * 4..x * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
}

fn rgb2rgba(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        o[x * 4..x * 4 + 3].copy_from_slice(&i[x * 4..x * 4 + 3]);
        o[x * 4 + 3] = 255;
    }
}

fn rgba_premul(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        let a = u32::from(i[x * 4 + 3]);
        for k in 0..3 {
            o[x * 4 + k] = muldiv255(u32::from(i[x * 4 + k]), a) as u8;
        }
        o[x * 4 + 3] = a as u8;
    }
}

fn rgba_unpremul(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        let a = u32::from(i[x * 4 + 3]);
        for k in 0..3 {
            let v = u32::from(i[x * 4 + k]);
            o[x * 4 + k] = if a == 255 || a == 0 { v as u8 } else { ((255 * v) / a).min(255) as u8 };
        }
        o[x * 4 + 3] = a as u8;
    }
}

fn rgba2rgb_unpremul(o: &mut [u8], i: &[u8], n: usize) {
    rgba_unpremul(o, i, n);
    for x in 0..n {
        o[x * 4 + 3] = 255;
    }
}

fn rgb2cmyk(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        let c = 255 - i[x * 4];
        let m = 255 - i[x * 4 + 1];
        let y = 255 - i[x * 4 + 2];
        o[x * 4..x * 4 + 4].copy_from_slice(&[c, m, y, 0]);
    }
}

fn cmyk2rgb(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        let k = u32::from(i[x * 4 + 3]);
        let mut px = [0u8; 4];
        for c in 0..3 {
            let v = u32::from(i[x * 4 + c]);
            // `MULDIV255(255 - v, 255 - k)` como no `cmyk2rgb`.
            px[c] = muldiv255(255 - v, 255 - k) as u8;
        }
        px[3] = 255;
        o[x * 4..x * 4 + 4].copy_from_slice(&px);
    }
}

fn i2l(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        let v = i32::from_le_bytes([i[x * 4], i[x * 4 + 1], i[x * 4 + 2], i[x * 4 + 3]]);
        o[x] = v.clamp(0, 255) as u8;
    }
}

fn i2f(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        let v = i32::from_le_bytes([i[x * 4], i[x * 4 + 1], i[x * 4 + 2], i[x * 4 + 3]]);
        o[x * 4..x * 4 + 4].copy_from_slice(&(v as f32).to_le_bytes());
    }
}

fn i2rgb(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        let v = i32::from_le_bytes([i[x * 4], i[x * 4 + 1], i[x * 4 + 2], i[x * 4 + 3]]).clamp(0, 255) as u8;
        o[x * 4..x * 4 + 4].copy_from_slice(&[v, v, v, 255]);
    }
}

fn f2l(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        let v = f32::from_le_bytes([i[x * 4], i[x * 4 + 1], i[x * 4 + 2], i[x * 4 + 3]]);
        o[x] = if v <= 0.0 { 0 } else if v >= 255.0 { 255 } else { v as u8 };
    }
}

fn f2i(o: &mut [u8], i: &[u8], n: usize) {
    for x in 0..n {
        let v = f32::from_le_bytes([i[x * 4], i[x * 4 + 1], i[x * 4 + 2], i[x * 4 + 3]]);
        o[x * 4..x * 4 + 4].copy_from_slice(&(v as i32).to_le_bytes());
    }
}

fn shuffler(from: &str, to: &str) -> Option<Shuffler> {
    Some(match (from, to) {
        ("1", "L") => bit2l,
        ("1", "I") => bit2i,
        ("1", "F") => bit2f,
        ("1", "RGB" | "RGBA" | "RGBX") => bit2rgb,
        ("L", "1") => l2bit,
        ("L", "LA" | "RGB" | "RGBA" | "RGBX") => l2rgb,
        ("L", "I") => l2i,
        ("L", "F") => l2f,
        ("LA", "L") => la2l,
        ("LA", "La") => la2la_premul,
        ("LA", "RGB" | "RGBA" | "RGBX") => la2rgb,
        ("La", "LA") => la_unpremul,
        ("I", "L") => i2l,
        ("I", "F") => i2f,
        ("I", "RGB" | "RGBA" | "RGBX") => i2rgb,
        ("F", "L") => f2l,
        ("F", "I") => f2i,
        ("RGB" | "RGBA" | "RGBX", "1") => rgb2bit,
        ("RGB" | "RGBA" | "RGBX", "L") => rgb2l,
        ("RGB" | "RGBX", "LA") | ("RGB", "La") => rgb2la,
        ("RGBA", "LA") => rgba2la,
        ("RGB" | "RGBA" | "RGBX", "I") => rgb2i,
        ("RGB" | "RGBA" | "RGBX", "F") => rgb2f,
        ("RGB", "RGBA" | "RGBa" | "RGBX") | ("RGBA" | "RGBX", "RGB") | ("RGBA", "RGBX") => rgb2rgba,
        ("RGBA", "RGBa") => rgba_premul,
        ("RGBa", "RGBA") => rgba_unpremul,
        ("RGBa", "RGB") => rgba2rgb_unpremul,
        ("RGB" | "RGBA" | "RGBX", "CMYK") => rgb2cmyk,
        ("CMYK", "RGB" | "RGBA" | "RGBX") => cmyk2rgb,
        _ => return None,
    })
}

/// `tobilevel`: L ou RGB para `1` com difusão de erro.
fn tobilevel(im: &Image) -> Result<Image, String> {
    if im.mode != "L" && im.mode != "RGB" {
        return Err("conversion not supported".into());
    }
    let mut out = Image::new("1", im.xsize, im.ysize).expect("modo 1");
    let w = im.xsize as usize;
    let mut errors = vec![0i32; w + 1];
    for y in 0..im.ysize {
        let inp = im.line(y).to_vec();
        let o = out.line_mut(y);
        let (mut l, mut l0, mut l1) = (0i32, 0i32, 0i32);
        let mut x = 0;
        while x < w {
            let base = if im.bands == 1 { i32::from(inp[x]) } else { lum(&inp[x * 4..]) / 1000 };
            l = (base + (l + errors[x + 1]) / 16).clamp(0, 255);
            o[x] = if l > 128 { 255 } else { 0 };
            l -= i32::from(o[x]);
            let l2 = l;
            let d2 = l + l;
            l += d2;
            errors[x] = l + l0;
            l += d2;
            l0 = l + l1;
            l1 = l2;
            l += d2;
            x += 1;
        }
        errors[x] = l0;
    }
    Ok(out)
}

fn pal_px(p: &Palette, idx: u8) -> &[u8] {
    &p.colors[usize::from(idx) * 4..usize::from(idx) * 4 + 4]
}

/// `frompalette`: P/PA para os outros modos pela paleta.
fn frompalette(im: &Image, mode: &str) -> Result<Image, String> {
    let Some(pal) = im.palette.as_ref() else {
        return Err("no palette".into());
    };
    let alpha = im.mode == "PA";
    let supported = ["1", "L", "LA", "P", "PA", "I", "F", "RGB", "RGBA", "RGBX", "CMYK"];
    if !supported.contains(&mode) {
        return Err("conversion not supported".into());
    }
    let mut out = Image::new(mode, im.xsize, im.ysize).ok_or("unrecognized image mode")?;
    if mode == "P" || mode == "PA" {
        out.palette = Some(pal.clone());
    }
    let w = im.xsize as usize;
    for y in 0..im.ysize {
        let inp = im.line(y).to_vec();
        let o = out.line_mut(y);
        for x in 0..w {
            let (idx, a_in) = if alpha { (inp[x * 4], inp[x * 4 + 3]) } else { (inp[x], 0) };
            let c = pal_px(pal, idx);
            match mode {
                "1" => o[x] = if lum(c) >= 128_000 { 255 } else { 0 },
                "L" => o[x] = l24(c),
                "LA" => {
                    let v = l24(c);
                    o[x * 4..x * 4 + 4].copy_from_slice(&[v, v, v, if alpha { a_in } else { c[3] }]);
                }
                "P" => o[x] = idx,
                "PA" => {
                    let a = if pal.mode == "RGB" { 255 } else { c[3] };
                    o[x * 4..x * 4 + 4].copy_from_slice(&[idx, idx, idx, a]);
                }
                "I" => o[x * 4..x * 4 + 4].copy_from_slice(&i32::from(l24(c)).to_le_bytes()),
                "F" => o[x * 4..x * 4 + 4].copy_from_slice(&(lum(c) as f32 / 1000.0).to_le_bytes()),
                "RGB" => o[x * 4..x * 4 + 4].copy_from_slice(&[c[0], c[1], c[2], 255]),
                "RGBA" | "RGBX" => {
                    let a = if alpha { a_in } else { c[3] };
                    o[x * 4..x * 4 + 4].copy_from_slice(&[c[0], c[1], c[2], a]);
                }
                _ => {
                    let rgb = [c[0], c[1], c[2], 255];
                    rgb2cmyk(&mut o[x * 4..x * 4 + 4], &rgb, 1);
                }
            }
        }
    }
    Ok(out)
}

/// `ImagingConvert` sem paleta de destino (o caminho para `P` vem em [`topalette_gray`] e no
/// quantizador).
pub fn convert(im: &Image, mode: Option<&str>, dither: bool) -> Result<Image, String> {
    let mode = match mode {
        None => match &im.palette {
            Some(p) => p.mode.clone(),
            None => return Err("wrong mode".into()),
        },
        Some(m) => {
            if m == im.mode {
                return Ok(im.clone());
            }
            m.to_string()
        }
    };
    if im.mode == "P" || im.mode == "PA" {
        return frompalette(im, &mode);
    }
    if mode == "P" || mode == "PA" {
        return topalette(im, &mode, dither);
    }
    if dither && mode == "1" {
        return tobilevel(im);
    }
    let Some(f) = shuffler(&im.mode, &mode) else {
        let from: String = im.mode.chars().take(10).collect();
        let to: String = mode.chars().take(10).collect();
        return Err(format!("conversion from {from} to {to} not supported"));
    };
    let mut out = Image::new(&mode, im.xsize, im.ysize).ok_or("unrecognized image mode")?;
    for y in 0..im.ysize {
        let s = (y * im.linesize) as usize;
        let inp = &im.data[s..s + im.linesize as usize];
        f(out.line_mut(y), inp, im.xsize as usize);
    }
    Ok(out)
}

/// `topalette` sem paleta de entrada: L vira P com a paleta de cinza; RGB vira P com a paleta
/// "browser" (`ImagingPaletteNewBrowser`), com ou sem Floyd-Steinberg.
fn topalette(im: &Image, mode: &str, dither: bool) -> Result<Image, String> {
    if im.mode != "L" && !im.mode.starts_with("RGB") {
        return Err("conversion not supported".into());
    }
    let alpha = mode == "PA";
    let mut out = Image::new(mode, im.xsize, im.ysize).ok_or("unrecognized image mode")?;
    let w = im.xsize as usize;
    if im.bands == 1 {
        let mut pal = Palette::new("RGB");
        pal.size = 256;
        for y in 0..im.ysize {
            let inp = im.line(y).to_vec();
            let o = out.line_mut(y);
            for x in 0..w {
                if alpha {
                    o[x * 4..x * 4 + 4].copy_from_slice(&[inp[x], inp[x], inp[x], 255]);
                } else {
                    o[x] = inp[x];
                }
            }
        }
        out.palette = Some(pal);
        return Ok(out);
    }
    let pal = browser_palette();
    let mut cache = PaletteCache::new();
    if dither {
        // Floyd-Steinberg por canal; `e` anda de 3 em 3 e o fim da linha grava b0, b1, b2 nas
        // três posições, como o original.
        let mut errors = vec![0i32; (w + 1) * 3];
        for y in 0..im.ysize {
            let inp = im.line(y).to_vec();
            let o = out.line_mut(y);
            let (mut r, mut r0, mut r1) = (0i32, 0i32, 0i32);
            let (mut g, mut g0, mut g1) = (0i32, 0i32, 0i32);
            let (mut b, mut b0, mut b1, mut b2) = (0i32, 0i32, 0i32, 0i32);
            let mut e = 0usize;
            for x in 0..w {
                let px = &inp[x * 4..x * 4 + 3];
                r = (i32::from(px[0]) + (r + errors[e + 3]) / 16).clamp(0, 255);
                g = (i32::from(px[1]) + (g + errors[e + 4]) / 16).clamp(0, 255);
                b = (i32::from(px[2]) + (b + errors[e + 5]) / 16).clamp(0, 255);
                let idx = cache.get(&pal, r, g, b);
                if alpha {
                    o[x * 4..x * 4 + 4].copy_from_slice(&[idx, idx, idx, 255]);
                } else {
                    o[x] = idx;
                }
                let c = &pal.colors[usize::from(idx) * 4..];
                r -= i32::from(c[0]);
                g -= i32::from(c[1]);
                b -= i32::from(c[2]);
                let r2 = r;
                let mut d2 = r + r;
                r += d2;
                errors[e] = r + r0;
                r += d2;
                r0 = r + r1;
                r1 = r2;
                r += d2;
                let g2 = g;
                d2 = g + g;
                g += d2;
                errors[e + 1] = g + g0;
                g += d2;
                g0 = g + g1;
                g1 = g2;
                g += d2;
                b2 = b;
                d2 = b + b;
                b += d2;
                errors[e + 2] = b + b0;
                b += d2;
                b0 = b + b1;
                b1 = b2;
                b += d2;
                e += 3;
            }
            errors[e] = b0;
            errors[e + 1] = b1;
            errors[e + 2] = b2;
        }
    } else {
        for y in 0..im.ysize {
            let inp = im.line(y).to_vec();
            let o = out.line_mut(y);
            for x in 0..w {
                let px = &inp[x * 4..x * 4 + 3];
                let idx = cache.get(&pal, i32::from(px[0]), i32::from(px[1]), i32::from(px[2]));
                if alpha {
                    o[x * 4..x * 4 + 4].copy_from_slice(&[idx, idx, idx, 255]);
                } else {
                    o[x] = idx;
                }
            }
        }
    }
    out.palette = Some(pal);
    Ok(out)
}

/// `ImagingPaletteNewBrowser`: as 10 primeiras cores ficam pretas e o cubo 6x6x6 (passo 51, com
/// o vermelho variando mais rápido) ocupa os índices 10 a 225; o tamanho é 226.
pub fn browser_palette() -> Palette {
    let mut p = Palette::new("RGB");
    let mut i = 10usize;
    for b in (0..256).step_by(51) {
        for g in (0..256).step_by(51) {
            for r in (0..256).step_by(51) {
                p.colors[i * 4] = r as u8;
                p.colors[i * 4 + 1] = g as u8;
                p.colors[i * 4 + 2] = b as u8;
                i += 1;
            }
        }
    }
    p.size = i;
    p
}

/// O cache de cores da paleta (`ImagingPaletteCachePrepare`/`Update`): 64x64x64 células de 4
/// valores por canal; cada consulta numa caixa ainda vazia preenche as 8x8x8 células da caixa de
/// 32 valores com a cor mais próxima de cada célula (seleção de Heckbert, varredura de Thomas).
pub struct PaletteCache {
    cache: Vec<i16>,
}

const BOX: i32 = 8;
const STEP: i32 = 4;

impl PaletteCache {
    pub fn new() -> PaletteCache {
        PaletteCache { cache: vec![0x100; 64 * 64 * 64] }
    }

    fn slot(r: i32, g: i32, b: i32) -> usize {
        ((r >> 2) + (g >> 2) * 64 + (b >> 2) * 64 * 64) as usize
    }

    pub fn get(&mut self, pal: &Palette, r: i32, g: i32, b: i32) -> u8 {
        let s = Self::slot(r, g, b);
        if self.cache[s] == 0x100 {
            self.update(pal, r, g, b);
        }
        self.cache[s] as u8
    }

    fn update(&mut self, pal: &Palette, r: i32, g: i32, b: i32) {
        let dist = |a: i32, b: i32| ((a - b) * (a - b)) as u32;
        let (r0, g0, b0) = (r & 0xe0, g & 0xe0, b & 0xe0);
        let (r1, g1, b1) = (r0 + 0x1f, g0 + 0x1f, b0 + 0x1f);
        let (rc, gc, bc) = ((r0 + r1) / 2, (g0 + g1) / 2, (b0 + b1) / 2);
        let size = pal.size.min(256);
        let mut dmin = [0u32; 256];
        let mut dmax = u32::MAX;
        for i in 0..size {
            let pr = i32::from(pal.colors[i * 4]);
            let pg = i32::from(pal.colors[i * 4 + 1]);
            let pb = i32::from(pal.colors[i * 4 + 2]);
            let mut tmin = if pr < r0 { dist(pr, r0) } else if pr > r1 { dist(pr, r1) } else { 0 };
            let mut tmax = if pr <= rc { dist(pr, r1) } else { dist(pr, r0) };
            tmin += if pg < g0 { dist(pg, g0) } else if pg > g1 { dist(pg, g1) } else { 0 };
            tmax += if pg <= gc { dist(pg, g1) } else { dist(pg, g0) };
            tmin += if pb < b0 { dist(pb, b0) } else if pb > b1 { dist(pb, b1) } else { 0 };
            tmax += if pb <= bc { dist(pb, b1) } else { dist(pb, b0) };
            dmin[i] = tmin;
            if tmax < dmax {
                dmax = tmax;
            }
        }
        let vol = (BOX * BOX * BOX) as usize;
        let mut d = vec![u32::MAX; vol];
        let mut c = vec![0u8; vol];
        for i in 0..size {
            if dmin[i] > dmax {
                continue;
            }
            let mut ri = r0 - i32::from(pal.colors[i * 4]);
            let mut gi = g0 - i32::from(pal.colors[i * 4 + 1]);
            let mut bi = b0 - i32::from(pal.colors[i * 4 + 2]);
            let mut rd = ri * ri + gi * gi + bi * bi;
            ri = ri * (2 * STEP) + STEP * STEP;
            gi = gi * (2 * STEP) + STEP * STEP;
            bi = bi * (2 * STEP) + STEP * STEP;
            let mut rx = ri;
            let mut j = 0usize;
            for _ in 0..BOX {
                let mut gd = rd;
                let mut gx = gi;
                for _ in 0..BOX {
                    let mut bd = gd;
                    let mut bx = bi;
                    for _ in 0..BOX {
                        if (bd as u32) < d[j] {
                            d[j] = bd as u32;
                            c[j] = i as u8;
                        }
                        bd += bx;
                        bx += 2 * STEP * STEP;
                        j += 1;
                    }
                    gd += gx;
                    gx += 2 * STEP * STEP;
                }
                rd += rx;
                rx += 2 * STEP * STEP;
            }
        }
        // A caixa é percorrida em r, g, b (o laço de dentro é o azul), na mesma ordem do `c`.
        let mut j = 0usize;
        let mut rr = r0;
        while rr < r1 {
            let mut gg = g0;
            while gg < g1 {
                let mut bb = b0;
                while bb < b1 {
                    self.cache[Self::slot(rr, gg, bb)] = i16::from(c[j]);
                    j += 1;
                    bb += 4;
                }
                gg += 4;
            }
            rr += 4;
        }
    }
}
