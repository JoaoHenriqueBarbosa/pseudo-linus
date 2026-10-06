//! `ImagingPaste` e `ImagingFill2` da libImaging (`Paste.c` do Pillow 11.1.0): colar uma imagem
//! ou preencher com uma cor, opcionalmente através de uma máscara `1`, `L`, `LA`/`RGBA` ou `RGBa`.
//! É o que o `Image.paste` e o texto do `ImageDraw` (`draw_bitmap`) usam.
//!
//! Portado da libImaging (MIT-CMU: Copyright © 1997-2006 Secret Labs AB, © 1996-1997 Fredrik Lundh).

use super::image::{Image, blend};

fn preblend(mask: u32, in1: u32, in2: u32) -> u8 {
    // `MULDIV255(in1, 255 - mask) + in2`, em `unsigned int` e truncado para `UINT8`.
    let t = in1 * (255 - mask) + 128;
    ((((t >> 8) + t) >> 8) + in2) as u8
}

/// A região a copiar depois do recorte: `(dx, dy, sx, sy, xsize, ysize)`; `None` se vazia.
fn clip(out: &Image, mut dx: i32, mut dy: i32, mut xsize: i32, mut ysize: i32) -> Option<(i32, i32, i32, i32, i32, i32)> {
    let (mut sx, mut sy) = (0, 0);
    if dx < 0 {
        xsize += dx;
        sx = -dx;
        dx = 0;
    }
    if dx + xsize > out.xsize {
        xsize = out.xsize - dx;
    }
    if dy < 0 {
        ysize += dy;
        sy = -dy;
        dy = 0;
    }
    if dy + ysize > out.ysize {
        ysize = out.ysize - dy;
    }
    if xsize <= 0 || ysize <= 0 {
        return None;
    }
    Some((dx, dy, sx, sy, xsize, ysize))
}

/// `ImagingPaste(out, in, mask, dx0, dy0, dx1, dy1)`.
pub fn paste(out: &mut Image, inp: &Image, mask: Option<&Image>, dx0: i32, dy0: i32, dx1: i32, dy1: i32) -> Result<(), String> {
    let ps = out.pixelsize;
    let (xsize, ysize) = (dx1 - dx0, dy1 - dy0);
    if xsize != inp.xsize || ysize != inp.ysize || ps != inp.pixelsize {
        return Err("images do not match".into());
    }
    if let Some(m) = mask {
        if xsize != m.xsize || ysize != m.ysize {
            return Err("images do not match".into());
        }
    }
    let Some((dx, dy, sx, sy, xs, ys)) = clip(out, dx0, dy0, xsize, ysize) else {
        return Ok(());
    };
    let p = ps as usize;
    match mask {
        None => {
            for y in 0..ys {
                let o = out.offset(dx, y + dy);
                let i = inp.offset(sx, y + sy);
                let n = (xs * ps) as usize;
                out.data[o..o + n].copy_from_slice(&inp.data[i..i + n]);
            }
        }
        Some(m) if m.mode == "1" => {
            for y in 0..ys {
                for x in 0..xs {
                    if m.data[m.offset(sx + x, sy + y)] != 0 {
                        let o = out.offset(dx + x, dy + y);
                        let i = inp.offset(sx + x, sy + y);
                        out.data[o..o + p].copy_from_slice(&inp.data[i..i + p]);
                    }
                }
            }
        }
        Some(m) if m.mode == "L" => {
            for y in 0..ys {
                for x in 0..xs {
                    let a = u32::from(m.data[m.offset(sx + x, sy + y)]);
                    let o = out.offset(dx + x, dy + y);
                    let i = inp.offset(sx + x, sy + y);
                    // No armazenamento de 8 bits só o primeiro byte; no de 32, os quatro.
                    let n = if out.is8() { 1 } else { 4 };
                    for k in 0..n {
                        out.data[o + k] = blend(a, u32::from(out.data[o + k]), u32::from(inp.data[i + k]));
                    }
                }
            }
        }
        Some(m) if m.mode == "LA" || m.mode == "RGBA" || m.mode == "RGBa" => {
            let pre = m.mode == "RGBa";
            for y in 0..ys {
                for x in 0..xs {
                    let a = u32::from(m.data[m.offset(sx + x, sy + y) + 3]);
                    let o = out.offset(dx + x, dy + y);
                    let i = inp.offset(sx + x, sy + y);
                    let n = if out.is8() { 1 } else { 4 };
                    for k in 0..n {
                        let (o1, i1) = (u32::from(out.data[o + k]), u32::from(inp.data[i + k]));
                        out.data[o + k] = if pre { preblend(a, o1, i1) } else { blend(a, o1, i1) };
                    }
                }
            }
        }
        Some(_) => return Err("bad transparency mask".into()),
    }
    Ok(())
}

/// `ImagingFill2(out, ink, mask, dx0, dy0, dx1, dy1)`: `ink` são os 4 bytes do `getink`.
pub fn fill2(out: &mut Image, ink: [u8; 4], mask: Option<&Image>, dx0: i32, dy0: i32, dx1: i32, dy1: i32) -> Result<(), String> {
    let ps = out.pixelsize;
    let (xsize, ysize) = (dx1 - dx0, dy1 - dy0);
    if let Some(m) = mask {
        if xsize != m.xsize || ysize != m.ysize {
            return Err("images do not match".into());
        }
    }
    let Some((dx, dy, sx, sy, xs, ys)) = clip(out, dx0, dy0, xsize, ysize) else {
        return Ok(());
    };
    let p = ps as usize;
    let mut ink32 = [0u8; 4];
    ink32[..p.min(4)].copy_from_slice(&ink[..p.min(4)]);
    match mask {
        None => {
            if out.is8() || ink32 == [0; 4] {
                for y in 0..ys {
                    let o = out.offset(dx, y + dy);
                    let n = (xs * ps) as usize;
                    out.data[o..o + n].fill(ink[0]);
                }
            } else {
                for y in 0..ys {
                    for x in 0..xs {
                        let o = out.offset(dx + x, dy + y);
                        out.data[o..o + 4].copy_from_slice(&ink32);
                    }
                }
            }
        }
        Some(m) if m.mode == "1" => {
            for y in 0..ys {
                for x in 0..xs {
                    if m.data[m.offset(sx + x, sy + y)] != 0 {
                        let o = out.offset(dx + x, dy + y);
                        if out.is8() {
                            out.data[o] = ink[0];
                        } else {
                            out.data[o..o + 4].copy_from_slice(&ink32);
                        }
                    }
                }
            }
        }
        Some(m) if m.mode == "L" => {
            if out.is8() {
                let i16 = out.is_i16();
                for y in 0..ys {
                    for x in 0..xs {
                        let a = u32::from(m.data[m.offset(sx + x, sy + y)]);
                        let o = out.offset(dx + x, dy + y);
                        out.data[o] = blend(a, u32::from(out.data[o]), u32::from(ink[0]));
                        if i16 {
                            out.data[o + 1] = blend(a, u32::from(out.data[o + 1]), u32::from(ink[1]));
                        }
                    }
                }
            } else {
                let alpha_channel = matches!(out.mode.as_str(), "RGBa" | "RGBA" | "La" | "LA" | "PA");
                for y in 0..ys {
                    for x in 0..xs {
                        let m0 = i32::from(m.data[m.offset(sx + x, sy + y)]);
                        let o = out.offset(dx + x, dy + y);
                        for k in 0..p {
                            let mut cm = m0;
                            if alpha_channel && k != 3 && cm != 0 {
                                let a = i32::from(out.data[o + 3]);
                                cm = 255 - (255 - cm) * (1 - (255 - a) / 255);
                            }
                            out.data[o + k] = blend(cm as u32 & 0xff, u32::from(out.data[o + k]), u32::from(ink[k]));
                        }
                    }
                }
            }
        }
        Some(m) if m.mode == "LA" || m.mode == "RGBA" || m.mode == "RGBa" => {
            let pre = m.mode == "RGBa";
            for y in 0..ys {
                for x in 0..xs {
                    let a = u32::from(m.data[m.offset(sx + x, sy + y) + 3]);
                    let o = out.offset(dx + x, dy + y);
                    let n = if out.is8() { 1 } else { p };
                    for k in 0..n {
                        let o1 = u32::from(out.data[o + k]);
                        let i1 = u32::from(ink[k]);
                        out.data[o + k] = if pre { preblend(a, o1, i1) } else { blend(a, o1, i1) };
                    }
                }
            }
        }
        Some(_) => return Err("bad transparency mask".into()),
    }
    Ok(())
}
