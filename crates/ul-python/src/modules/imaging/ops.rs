//! Operações avulsas da libImaging do Pillow 11.1.0: `ImagingCrop` (`Crop.c`), `ImagingGetBBox` e
//! `ImagingGetExtrema` (`GetBBox.c`), bandas (`Bands.c`), histograma (`Histo.c`), as transposições
//! e a transformação afim de vizinho mais próximo que o `resize(NEAREST)` usa (`Geometry.c`).
//!
//! Portado da libImaging (MIT-CMU: Copyright © 1997-2011 Secret Labs AB, © 1995-2011 Fredrik Lundh
//! e colaboradores).

use super::image::{Image, PixType};
use super::paste::paste;

/// Imagem nova do mesmo modo, com a paleta copiada (`ImagingCopyPalette`).
fn like(im: &Image, xsize: i32, ysize: i32) -> Image {
    let mut out = Image::new(&im.mode, xsize, ysize).expect("modo já validado");
    out.palette = im.palette.clone();
    out
}

/// `ImagingCrop`: o que cai fora da imagem fica zerado.
pub fn crop(im: &Image, sx0: i32, sy0: i32, sx1: i32, sy1: i32) -> Image {
    let mut out = like(im, (sx1 - sx0).max(0), (sy1 - sy0).max(0));
    let _ = paste(&mut out, im, None, -sx0, -sy0, im.xsize - sx0, im.ysize - sy0);
    out
}

/// `ImagingGetBBox`.
pub fn getbbox(im: &Image, alpha_only: bool) -> Option<[i32; 4]> {
    let mask: u32 = if im.is8() {
        0xff
    } else if im.bands == 3 {
        0x00ff_ffff
    } else if alpha_only && matches!(im.mode.as_str(), "RGBa" | "RGBA" | "La" | "LA" | "PA") {
        0xff00_0000
    } else {
        0xffff_ffff
    };
    // No armazenamento de 8 bits o `image8[y][x]` lê um byte por coluna, também nos `I;16` (onde
    // o laço até `xsize` só vê a primeira metade da linha, como no original).
    let width = im.xsize;
    let hit = |x: i32, y: i32| -> bool {
        if im.is8() {
            u32::from(im.data[(y * im.linesize + x) as usize]) & mask != 0
        } else {
            let o = im.offset(x, y);
            u32::from_le_bytes([im.data[o], im.data[o + 1], im.data[o + 2], im.data[o + 3]]) & mask != 0
        }
    };
    let mut b = [width, -1, 0, 0];
    'top: for y in 0..im.ysize {
        for x in 0..width {
            if hit(x, y) {
                b[0] = x;
                b[1] = y;
                break 'top;
            }
        }
    }
    if b[1] < 0 {
        return None;
    }
    'bottom: for y in (b[1]..im.ysize).rev() {
        for x in 0..width {
            if hit(x, y) {
                if x < b[0] {
                    b[0] = x;
                }
                b[3] = y + 1;
                break 'bottom;
            }
        }
    }
    for y in b[1]..b[3] {
        for x in 0..b[0] {
            if hit(x, y) {
                b[0] = x;
                break;
            }
        }
        let mut x = width - 1;
        while x >= b[2] {
            if hit(x, y) {
                b[2] = x + 1;
                break;
            }
            x -= 1;
        }
    }
    Some(b)
}

/// Resultado de `ImagingGetExtrema`.
pub enum Extrema {
    U8(u8, u8),
    I32(i32, i32),
    F32(f32, f32),
    U16(u16, u16),
}

/// `ImagingGetExtrema`: `Ok(None)` na imagem vazia.
pub fn getextrema(im: &Image) -> Result<Option<Extrema>, String> {
    if im.bands != 1 {
        return Err("image has wrong mode".into());
    }
    if im.xsize == 0 || im.ysize == 0 {
        return Ok(None);
    }
    let word = |o: usize| [im.data[o], im.data[o + 1], im.data[o + 2], im.data[o + 3]];
    Ok(Some(match im.kind {
        PixType::Uint8 => {
            let (mut lo, mut hi) = (im.data[0], im.data[0]);
            for y in 0..im.ysize {
                for &v in &im.line(y)[..im.xsize as usize] {
                    if lo > v {
                        lo = v;
                    } else if hi < v {
                        hi = v;
                    }
                }
                if lo == 0 && hi == 255 {
                    break;
                }
            }
            Extrema::U8(lo, hi)
        }
        PixType::Int32 => {
            let (mut lo, mut hi) = (i32::from_le_bytes(word(0)), i32::from_le_bytes(word(0)));
            for y in 0..im.ysize {
                for x in 0..im.xsize {
                    let v = i32::from_le_bytes(word(im.offset(x, y)));
                    if lo > v {
                        lo = v;
                    } else if hi < v {
                        hi = v;
                    }
                }
            }
            Extrema::I32(lo, hi)
        }
        PixType::Float32 => {
            let (mut lo, mut hi) = (f32::from_le_bytes(word(0)), f32::from_le_bytes(word(0)));
            for y in 0..im.ysize {
                for x in 0..im.xsize {
                    let v = f32::from_le_bytes(word(im.offset(x, y)));
                    if lo > v {
                        lo = v;
                    } else if hi < v {
                        hi = v;
                    }
                }
            }
            Extrema::F32(lo, hi)
        }
        PixType::Special if im.mode == "I;16" => {
            let v0 = u16::from_le_bytes([im.data[0], im.data[1]]);
            let (mut lo, mut hi) = (v0, v0);
            for y in 0..im.ysize {
                for x in 0..im.xsize {
                    let o = im.offset(x, y);
                    let v = u16::from_le_bytes([im.data[o], im.data[o + 1]]);
                    if lo > v {
                        lo = v;
                    } else if hi < v {
                        hi = v;
                    }
                }
            }
            Extrema::U16(lo, hi)
        }
        PixType::Special => return Err("image has wrong mode".into()),
    }))
}

/// `ImagingGetBand`.
pub fn getband(im: &Image, band: i32) -> Result<Image, String> {
    if im.kind != PixType::Uint8 {
        return Err("image has wrong mode".into());
    }
    if band < 0 || band >= im.bands {
        return Err("band index out of range".into());
    }
    if im.bands == 1 {
        return Ok(im.clone());
    }
    let band = if im.bands == 2 && band == 1 { 3 } else { band } as usize;
    let mut out = Image::new("L", im.xsize, im.ysize).expect("L");
    for y in 0..im.ysize {
        for x in 0..im.xsize {
            let o = out.offset(x, y);
            out.data[o] = im.data[im.offset(x, y) + band];
        }
    }
    Ok(out)
}

/// `ImagingPutBand`.
pub fn putband(out: &mut Image, inp: &Image, band: i32) -> Result<(), String> {
    if inp.bands != 1 {
        return Err("image has wrong mode".into());
    }
    if band < 0 || band >= out.bands {
        return Err("band index out of range".into());
    }
    if inp.kind != out.kind || inp.xsize != out.xsize || inp.ysize != out.ysize {
        return Err("images do not match".into());
    }
    if out.bands == 1 {
        if inp.mode != out.mode {
            return Err("images do not match".into());
        }
        out.data.copy_from_slice(&inp.data);
        return Ok(());
    }
    let band = if out.bands == 2 && band == 1 { 3 } else { band } as usize;
    for y in 0..inp.ysize {
        for x in 0..inp.xsize {
            let o = out.offset(x, y) + band;
            out.data[o] = inp.data[inp.offset(x, y)];
        }
    }
    Ok(())
}

/// `ImagingFillBand`.
pub fn fillband(out: &mut Image, band: i32, color: i32) -> Result<(), String> {
    if out.kind != PixType::Uint8 {
        return Err("image has wrong mode".into());
    }
    if band < 0 || band >= out.bands {
        return Err("band index out of range".into());
    }
    let band = if out.bands == 2 && band == 1 { 3 } else { band } as usize;
    let c = color.clamp(0, 255) as u8;
    for y in 0..out.ysize {
        for x in 0..out.xsize {
            let o = out.offset(x, y) + band;
            out.data[o] = c;
        }
    }
    Ok(())
}

/// Os limites do histograma de imagens `I` e `F`.
pub enum MinMax {
    I(i32, i32),
    F(f32, f32),
}

/// `ImagingGetHistogram`: devolve `bands * 256` contagens.
pub fn histogram(im: &Image, mask: Option<&Image>, minmax: Option<MinMax>) -> Result<Vec<i64>, String> {
    if let Some(m) = mask {
        if im.xsize != m.xsize || im.ysize != m.ysize {
            return Err("images do not match".into());
        }
        if m.mode != "1" && m.mode != "L" {
            return Err("bad transparency mask".into());
        }
    }
    let mut h = vec![0i64; 1024];
    let on = |x: i32, y: i32| mask.is_none_or(|m| m.data[m.offset(x, y)] != 0);
    if im.is8() {
        // `image8[y][x]`: nos `I;16` são os primeiros `xsize` bytes da linha, como no original.
        for y in 0..im.ysize {
            for x in 0..im.xsize {
                if on(x, y) {
                    h[usize::from(im.data[(y * im.linesize + x) as usize])] += 1;
                }
            }
        }
    } else if mask.is_some() && im.kind != PixType::Uint8 {
        return Err("image has wrong mode".into());
    } else {
        match im.kind {
            PixType::Uint8 => {
                for y in 0..im.ysize {
                    for x in 0..im.xsize {
                        if on(x, y) {
                            let o = im.offset(x, y);
                            for k in 0..4 {
                                h[k * 256 + usize::from(im.data[o + k])] += 1;
                            }
                        }
                    }
                }
            }
            PixType::Int32 | PixType::Float32 => {
                let Some(mm) = minmax else {
                    return Err("min/max not given".into());
                };
                for y in 0..im.ysize {
                    for x in 0..im.xsize {
                        let o = im.offset(x, y);
                        let b = [im.data[o], im.data[o + 1], im.data[o + 2], im.data[o + 3]];
                        let i = match mm {
                            MinMax::I(lo, hi) if lo < hi => {
                                let scale = 255.0f32 / (hi - lo) as f32;
                                (i32::from_le_bytes(b).wrapping_sub(lo) as f32 * scale) as i32
                            }
                            MinMax::F(lo, hi) if lo < hi => {
                                let scale = 255.0f32 / (hi - lo);
                                ((f32::from_le_bytes(b) - lo) * scale) as i32
                            }
                            _ => continue,
                        };
                        if (0..256).contains(&i) {
                            h[i as usize] += 1;
                        }
                    }
                }
            }
            // Os `I;16` são armazenamento de 8 bits e caem no ramo de cima.
            PixType::Special => {}
        }
    }
    h.truncate(im.bands as usize * 256);
    Ok(h)
}

/// `_transpose(op)`: 0 espelha na horizontal, 1 na vertical, 2 gira 90°, 3 gira 180°, 4 gira 270°,
/// 5 transpõe e 6 faz a transversa.
pub fn transpose(im: &Image, op: i32) -> Result<Image, String> {
    let (w, h) = (im.xsize, im.ysize);
    let mut out = match op {
        0 | 1 | 3 => like(im, w, h),
        2 | 4 | 5 | 6 => like(im, h, w),
        _ => return Err("No such transpose operation".into()),
    };
    let p = im.pixelsize as usize;
    for y in 0..h {
        for x in 0..w {
            let (dx, dy) = match op {
                0 => (w - 1 - x, y),
                1 => (x, h - 1 - y),
                2 => (y, w - 1 - x),
                3 => (w - 1 - x, h - 1 - y),
                4 => (h - 1 - y, x),
                5 => (y, x),
                _ => (h - 1 - y, w - 1 - x),
            };
            let (s, d) = (im.offset(x, y), out.offset(dx, dy));
            out.data[d..d + p].copy_from_slice(&im.data[s..s + p]);
        }
    }
    Ok(out)
}

/// `COORD(v)`.
fn coord(v: f64) -> i32 {
    if v < 0.0 { -1 } else { v as i32 }
}

/// `resize` com `NEAREST`: a transformação afim `ImagingScaleAffine` (escala pura, `fill` ligado),
/// ou o motor genérico nos modos especiais (`I;16`).
pub fn resize_nearest(im: &Image, xsize: i32, ysize: i32, b: [f32; 4]) -> Image {
    let a0 = f64::from(b[2] - b[0]) / f64::from(xsize);
    let a4 = f64::from(b[3] - b[1]) / f64::from(ysize);
    let (a2, a5) = (f64::from(b[0]), f64::from(b[1]));
    let mut out = like(im, xsize, ysize);
    let p = im.pixelsize as usize;
    if im.kind == PixType::Special {
        for y in 0..ysize {
            for x in 0..xsize {
                let xi = coord(a0 * (f64::from(x) + 0.5) + a2);
                let yi = coord(a4 * (f64::from(y) + 0.5) + a5);
                if xi >= 0 && xi < im.xsize && yi >= 0 && yi < im.ysize {
                    let (s, d) = (im.offset(xi, yi), out.offset(x, y));
                    out.data[d..d + p].copy_from_slice(&im.data[s..s + p]);
                }
            }
        }
        return out;
    }
    let mut xintab = vec![0i32; xsize as usize];
    let (mut xmin, mut xmax) = (xsize, 0);
    let mut xo = a2 + a0 * 0.5;
    for x in 0..xsize {
        let xin = coord(xo);
        if xin >= 0 && xin < im.xsize {
            xmax = x + 1;
            if x < xmin {
                xmin = x;
            }
            xintab[x as usize] = xin;
        }
        xo += a0;
    }
    let mut yo = a5 + a4 * 0.5;
    for y in 0..ysize {
        let yi = coord(yo);
        if yi >= 0 && yi < im.ysize {
            for x in xmin..xmax {
                let (s, d) = (im.offset(xintab[x as usize], yi), out.offset(x, y));
                out.data[d..d + p].copy_from_slice(&im.data[s..s + p]);
            }
        }
        yo += a4;
    }
    out
}

/// A tabela do `ImagingPoint`, no tipo que o `_point` montou para o caso.
pub enum PointTable {
    /// 8 bits por entrada: `256 · bandas` entradas, ou 65536 para `I` → `L`.
    U8(Vec<u8>),
    /// 4 bytes por entrada (`I` ou `F`), lidos de 8 bits para 32 bits.
    Word(Vec<[u8; 4]>),
}

/// `ImagingPoint` (`Point.c`): transformação por tabela.
pub fn point(im: &Image, mode: &str, table: &PointTable) -> Result<Image, String> {
    let mismatch = || "point operation not supported for this mode".to_string();
    if im.kind != PixType::Uint8 {
        if im.kind != PixType::Int32 || mode != "L" {
            return Err(mismatch());
        }
    } else if !im.is8() && im.mode != mode {
        return Err(mismatch());
    }
    let mut out = Image::new(mode, im.xsize, im.ysize).ok_or_else(|| "unrecognized image mode".to_string())?;
    out.palette = im.palette.clone();
    for y in 0..im.ysize {
        for x in 0..im.xsize {
            let (s, d) = (im.offset(x, y), out.offset(x, y));
            match table {
                PointTable::U8(t) if im.kind == PixType::Uint8 => {
                    if im.is8() && out.is8() {
                        out.data[d] = t[usize::from(im.data[s])];
                    } else if im.is8() {
                        // `im_point_8_32`: `L` para várias bandas lê palavras de 4 bytes da tabela.
                        let v = 4 * usize::from(im.data[s]);
                        out.data[d..d + 4].copy_from_slice(&t[v..v + 4]);
                    } else {
                        let chans: &[usize] = match im.bands {
                            2 => &[0, 3],
                            3 => &[0, 1, 2],
                            _ => &[0, 1, 2, 3],
                        };
                        for (k, &c) in chans.iter().enumerate() {
                            // Com duas bandas o alfa usa a segunda fatia da tabela (`in[3] + 256`).
                            out.data[d + c] = t[usize::from(im.data[s + c]) + 256 * k];
                        }
                    }
                }
                PointTable::U8(t) => {
                    let v = i32::from_le_bytes([im.data[s], im.data[s + 1], im.data[s + 2], im.data[s + 3]]);
                    out.data[d] = t[v.clamp(0, 65535) as usize];
                }
                PointTable::Word(t) => {
                    out.data[d..d + 4].copy_from_slice(&t[usize::from(im.data[s])]);
                }
            }
        }
    }
    Ok(out)
}

/// `ImagingPointTransform`: `v · scale + offset` em `I`, `I;16` e `F`.
pub fn point_transform(im: &Image, scale: f64, offset: f64) -> Result<Image, String> {
    if !matches!(im.mode.as_str(), "I" | "I;16" | "F") {
        return Err("image has wrong mode".into());
    }
    let mut out = Image::new(&im.mode, im.xsize, im.ysize).expect("modo já validado");
    for y in 0..im.ysize {
        for x in 0..im.xsize {
            let (s, d) = (im.offset(x, y), out.offset(x, y));
            match im.kind {
                PixType::Int32 => {
                    let v = i32::from_le_bytes([im.data[s], im.data[s + 1], im.data[s + 2], im.data[s + 3]]);
                    // Conversão `double` → `INT32` do C; o Rust satura onde o C seria indefinido.
                    out.data[d..d + 4].copy_from_slice(&((f64::from(v) * scale + offset) as i32).to_le_bytes());
                }
                PixType::Float32 => {
                    let v = f32::from_le_bytes([im.data[s], im.data[s + 1], im.data[s + 2], im.data[s + 3]]);
                    let r = (f64::from(v) * scale + offset) as f32;
                    out.data[d..d + 4].copy_from_slice(&r.to_le_bytes());
                }
                _ => {
                    let v = u16::from_le_bytes([im.data[s], im.data[s + 1]]);
                    let r = (f64::from(v) * scale + offset) as u16;
                    out.data[d..d + 2].copy_from_slice(&r.to_le_bytes());
                }
            }
        }
    }
    Ok(out)
}
