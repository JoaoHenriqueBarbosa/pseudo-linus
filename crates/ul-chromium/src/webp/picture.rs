//! A conversão RGBA para YUVA 4:2:0 do libwebp 1.5.0 (`enc/picture_csp_enc.c`, `ImportYUVAFromRGBA` sem pontilhado
//! nem conversão iterativa), com a média dos tons de cinza em gama comprimida, e a limpeza das áreas transparentes
//! (`enc/picture_tools_enc.c`, `WebPCleanupTransparentArea`).

use super::vp8::Picture;
use std::sync::OnceLock;

const GAMMA_FIX: u32 = 12;
const GAMMA_TAB_FIX: u32 = 7;
const GAMMA_TAB_SIZE: usize = 1 << (GAMMA_FIX - GAMMA_TAB_FIX);
const GAMMA: f64 = 0.80;
const GAMMA_SCALE: i32 = (1 << GAMMA_FIX) - 1;
const GAMMA_TAB_SCALE: i32 = 1 << GAMMA_TAB_FIX;
const GAMMA_TAB_ROUNDER: i32 = GAMMA_TAB_SCALE >> 1;
const ALPHA_FIX: u32 = 19;
const YUV_FIX: u32 = 16;
const YUV_HALF: i32 = 1 << (YUV_FIX - 1);

struct GammaTables {
    to_linear: [u32; 256],
    to_gamma: [i32; GAMMA_TAB_SIZE + 1],
}

/// `InitGammaTables`.
fn gamma_tables() -> &'static GammaTables {
    static TABLES: OnceLock<GammaTables> = OnceLock::new();
    TABLES.get_or_init(|| {
        let scale = f64::from(1 << GAMMA_TAB_FIX) / f64::from(GAMMA_SCALE);
        let norm = 1.0 / 255.0;
        let mut t = GammaTables { to_linear: [0; 256], to_gamma: [0; GAMMA_TAB_SIZE + 1] };
        for (v, e) in t.to_linear.iter_mut().enumerate() {
            *e = u32::from(((norm * v as f64).powf(GAMMA) * f64::from(GAMMA_SCALE) + 0.5) as u16);
        }
        for (v, e) in t.to_gamma.iter_mut().enumerate() {
            *e = (255.0 * (scale * v as f64).powf(1.0 / GAMMA) + 0.5) as i32;
        }
        t
    })
}

/// `LinearToGamma`.
fn linear_to_gamma(base_value: u32, shift: u32) -> i32 {
    let t = gamma_tables();
    let v = (base_value << shift) as i32;
    let tab_pos = (v >> (GAMMA_TAB_FIX + 2)) as usize;
    let x = v & ((GAMMA_TAB_SCALE << 2) - 1);
    let y = t.to_gamma[tab_pos + 1] * x + t.to_gamma[tab_pos] * ((GAMMA_TAB_SCALE << 2) - x);
    (y + GAMMA_TAB_ROUNDER) >> GAMMA_TAB_FIX
}

fn to_linear(v: u8) -> u32 {
    gamma_tables().to_linear[v as usize]
}

/// `kInvAlpha[a] = (1 << 19) / a`, com `kInvAlpha[0] = 0`.
fn inv_alpha(a: u32) -> u32 {
    if a == 0 {
        0
    } else {
        (1 << ALPHA_FIX) / a
    }
}

fn rgb_to_y(r: i32, g: i32, b: i32) -> u8 {
    ((16839 * r + 33059 * g + 6420 * b + YUV_HALF + (16 << YUV_FIX)) >> YUV_FIX) as u8
}

fn clip_uv(uv: i32) -> u8 {
    let uv = (uv + (YUV_HALF << 2) + (128 << (YUV_FIX + 2))) >> (YUV_FIX + 2);
    if uv & !0xff == 0 {
        uv as u8
    } else if uv < 0 {
        0
    } else {
        255
    }
}

/// Os quatro canais de um pixel; `at(x, dy)` lê a coluna `x` na linha `dy` (0 ou 1) do par.
struct Rows<'a> {
    rgba: &'a [u8],
    row: usize,
    stride: usize,
}

impl Rows<'_> {
    fn at(&self, x: usize, dy: usize, c: usize) -> u8 {
        self.rgba[self.row + dy * self.stride + x * 4 + c]
    }
}

/// `AccumulateRGB`/`AccumulateRGBA` com `WebPConvertRGBA32ToUV` de um par de linhas (`stride` 0 na última ímpar).
fn accumulate_uv(rows: &Rows, width: usize, with_alpha: bool, u: &mut [u8], v: &mut [u8]) {
    for (i, (du, dv)) in u.iter_mut().zip(v.iter_mut()).enumerate() {
        let x = 2 * i;
        let pair = x + 1 < width;
        let cols: &[usize] = if pair { &[x, x + 1] } else { &[x, x] };
        let alpha: u32 = if pair {
            cols.iter().map(|&c| u32::from(rows.at(c, 0, 3)) + u32::from(rows.at(c, 1, 3))).sum()
        } else {
            2 * (u32::from(rows.at(x, 0, 3)) + u32::from(rows.at(x, 1, 3)))
        };
        let channel = |c: usize| -> i32 {
            if !with_alpha || alpha == 4 * 0xff || alpha == 0 {
                if pair {
                    let sum: u32 = cols.iter().map(|&x| to_linear(rows.at(x, 0, c)) + to_linear(rows.at(x, 1, c))).sum();
                    linear_to_gamma(sum, 0)
                } else {
                    linear_to_gamma(to_linear(rows.at(x, 0, c)) + to_linear(rows.at(x, 1, c)), 1)
                }
            } else {
                // `LinearToGammaWeighted`: na coluna ímpar o C lê o mesmo pixel duas vezes (passo 0).
                let sum: u32 = cols.iter().map(|&x| (0..2).map(|dy| u32::from(rows.at(x, dy, 3)) * to_linear(rows.at(x, dy, c))).sum::<u32>()).sum();
                linear_to_gamma(sum.wrapping_mul(inv_alpha(alpha)) >> (ALPHA_FIX - 2), 0)
            }
        };
        let (r, g, b) = (channel(0), channel(1), channel(2));
        *du = clip_uv(-9719 * r - 19081 * g + 28800 * b);
        *dv = clip_uv(28800 * r - 24116 * g - 4684 * b);
    }
}

/// `ImportYUVAFromRGBA` com RGBA não pré-multiplicado em linhas contíguas.
pub fn import_rgba(rgba: &[u8], width: usize, height: usize) -> Picture {
    let stride = width * 4;
    let has_alpha = rgba.chunks_exact(4).any(|p| p[3] != 0xff);
    let uv_w = width.div_ceil(2);
    let uv_h = height.div_ceil(2);
    let mut pic = Picture {
        width,
        height,
        y: vec![0; width * height],
        u: vec![0; uv_w * uv_h],
        v: vec![0; uv_w * uv_h],
        a: has_alpha.then(|| rgba.chunks_exact(4).map(|p| p[3]).collect()),
    };
    for (y, row) in pic.y.chunks_exact_mut(width).enumerate() {
        for (x, d) in row.iter_mut().enumerate() {
            let p = &rgba[y * stride + x * 4..];
            *d = rgb_to_y(i32::from(p[0]), i32::from(p[1]), i32::from(p[2]));
        }
    }
    for uy in 0..uv_h {
        let row = 2 * uy * stride;
        let last_odd = 2 * uy + 1 == height;
        let rows = Rows { rgba, row, stride: if last_odd { 0 } else { stride } };
        // `rows_have_alpha`: o par de linhas tem algum alfa diferente de 0xff (`!WebPExtractAlpha`).
        let lines = if last_odd { 1 } else { 2 };
        let rows_have_alpha = has_alpha && rgba[row..row + lines * stride].chunks_exact(4).any(|p| p[3] != 0xff);
        let (u, v) = (&mut pic.u[uy * uv_w..(uy + 1) * uv_w], &mut pic.v[uy * uv_w..(uy + 1) * uv_w]);
        accumulate_uv(&rows, width, rows_have_alpha, u, v);
    }
    pic
}

const SIZE: usize = 8;
const SIZE2: usize = SIZE / 2;

/// `SmoothenBlock`: devolve se o bloco é todo transparente.
fn smoothen_block(a: &[u8], y: &mut [u8], stride: usize, at: usize, width: usize, height: usize) -> bool {
    let (mut sum, mut count) = (0u32, 0usize);
    for j in 0..height {
        for i in 0..width {
            let k = at + j * stride + i;
            if a[k] != 0 {
                count += 1;
                sum += u32::from(y[k]);
            }
        }
    }
    if count > 0 && count < width * height {
        let avg = (sum / count as u32) as u8;
        for j in 0..height {
            for i in 0..width {
                let k = at + j * stride + i;
                if a[k] == 0 {
                    y[k] = avg;
                }
            }
        }
    }
    count == 0
}

fn flatten(plane: &mut [u8], at: usize, value: u8, stride: usize, size: usize) {
    for j in 0..size {
        plane[at + j * stride..at + j * stride + size].fill(value);
    }
}

/// `WebPCleanupTransparentArea` no caminho YUVA.
pub fn cleanup_transparent_area(pic: &mut Picture) {
    let Some(a) = pic.a.take() else { return };
    let (width, height) = (pic.width, pic.height);
    let (ys, uvs) = (pic.y_stride(), pic.uv_stride());
    let mut values = [0u8; 3];
    let mut y = 0;
    while y + SIZE <= height {
        let mut need_reset = true;
        let mut x = 0;
        while x + SIZE <= width {
            if smoothen_block(&a, &mut pic.y, ys, y * ys + x, SIZE, SIZE) {
                let uv_at = (y / 2) * uvs + x / 2;
                if need_reset {
                    values = [pic.y[y * ys + x], pic.u[uv_at], pic.v[uv_at]];
                    need_reset = false;
                }
                flatten(&mut pic.y, y * ys + x, values[0], ys, SIZE);
                flatten(&mut pic.u, uv_at, values[1], uvs, SIZE2);
                flatten(&mut pic.v, uv_at, values[2], uvs, SIZE2);
            } else {
                need_reset = true;
            }
            x += SIZE;
        }
        if x < width {
            smoothen_block(&a, &mut pic.y, ys, y * ys + x, width - x, SIZE);
        }
        y += SIZE;
    }
    if y < height {
        let sub_height = height - y;
        let mut x = 0;
        while x + SIZE <= width {
            smoothen_block(&a, &mut pic.y, ys, y * ys + x, SIZE, sub_height);
            x += SIZE;
        }
        if x < width {
            smoothen_block(&a, &mut pic.y, ys, y * ys + x, width - x, sub_height);
        }
    }
    pic.a = Some(a);
}
