//! Rasterizador em tons de cinza do `ftgrays.c`, a decomposição do `FT_Outline_Decompose` e o
//! renderizador `smooth` (`ftsmooth.c`, `ft_glyphslot_preset_bitmap`).
//!
//! O original divide o glifo em faixas quando a memória de células enche; o resultado de cada
//! pixel não depende das faixas, então aqui cada linha guarda as suas células sem limite.

use crate::outline::{Outline, Vector, TAG_CUBIC, TAG_ON};

const PIXEL_BITS: i32 = 8;
const ONE_PIXEL: i64 = 1 << PIXEL_BITS;

fn trunc(x: i64) -> i32 {
    (x >> PIXEL_BITS) as i32
}

fn fract(x: i64) -> i32 {
    (x & (ONE_PIXEL - 1)) as i32
}

fn upscale(x: i64) -> i64 {
    x * (ONE_PIXEL >> 6)
}

#[derive(Clone, Copy)]
struct Cell {
    x: i32,
    cover: i32,
    area: i32,
}

/// Um intervalo de cobertura (`FT_Span`), na linha `y`.
#[derive(Clone, Copy, Debug)]
pub struct Span {
    pub x: i32,
    pub len: i32,
    pub coverage: u8,
}

struct Worker {
    min_ex: i32,
    max_ex: i32,
    min_ey: i32,
    max_ey: i32,
    rows: Vec<Vec<Cell>>,
    /// Célula corrente: linha e posição no vetor, ou `None` para a lixeira.
    cell: Option<(usize, usize)>,
    x: i64,
    y: i64,
}

impl Worker {
    fn set_cell(&mut self, ex: i32, ey: i32) {
        let idx = ey - self.min_ey;
        if idx < 0 || idx >= self.max_ey - self.min_ey || ex >= self.max_ex {
            self.cell = None;
            return;
        }
        let ex = ex.max(self.min_ex - 1);
        let row = &mut self.rows[idx as usize];
        let pos = match row.binary_search_by_key(&ex, |c| c.x) {
            Ok(p) => p,
            Err(p) => {
                row.insert(p, Cell { x: ex, cover: 0, area: 0 });
                p
            }
        };
        self.cell = Some((idx as usize, pos));
    }

    fn integrate(&mut self, a: i32, b: i32) {
        if let Some((r, p)) = self.cell {
            let c = &mut self.rows[r][p];
            c.cover = c.cover.wrapping_add(a);
            c.area = c.area.wrapping_add(a.wrapping_mul(b));
        }
    }

    fn render_line(&mut self, to_x: i64, to_y: i64) {
        let mut ey1 = trunc(self.y);
        let ey2 = trunc(to_y);
        if (ey1 >= self.max_ey && ey2 >= self.max_ey) || (ey1 < self.min_ey && ey2 < self.min_ey) {
            self.x = to_x;
            self.y = to_y;
            return;
        }
        let mut ex1 = trunc(self.x);
        let ex2 = trunc(to_x);
        let mut fx1 = fract(self.x);
        let mut fy1 = fract(self.y);
        let dx = to_x - self.x;
        let dy = to_y - self.y;
        if ex1 == ex2 && ey1 == ey2 {
        } else if dy == 0 {
            self.set_cell(ex2, ey2);
            self.x = to_x;
            self.y = to_y;
            return;
        } else if dx == 0 {
            if dy > 0 {
                loop {
                    self.integrate(ONE_PIXEL as i32 - fy1, fx1 * 2);
                    fy1 = 0;
                    ey1 += 1;
                    self.set_cell(ex1, ey1);
                    if ey1 == ey2 {
                        break;
                    }
                }
            } else {
                loop {
                    self.integrate(-fy1, fx1 * 2);
                    fy1 = ONE_PIXEL as i32;
                    ey1 -= 1;
                    self.set_cell(ex1, ey1);
                    if ey1 == ey2 {
                        break;
                    }
                }
            }
        } else {
            let mut prod = dx * i64::from(fy1) - dy * i64::from(fx1);
            let dx_r = if ex1 != ex2 { 0xFFFF_FFFFi64 / dx } else { 0 };
            let dy_r = if ey1 != ey2 { 0xFFFF_FFFFi64 / dy } else { 0 };
            let udiv = |a: i64, r: i64| ((a as u64).wrapping_mul(r as u64) >> 32) as i32;
            loop {
                let (fx2, fy2);
                if prod - dx * ONE_PIXEL > 0 && prod <= 0 {
                    // Sai pela esquerda.
                    fx2 = 0;
                    fy2 = udiv(-prod, -dx_r);
                    prod -= dy * ONE_PIXEL;
                    self.integrate(fy2 - fy1, fx1 + fx2);
                    fx1 = ONE_PIXEL as i32;
                    fy1 = fy2;
                    ex1 -= 1;
                } else if prod - dx * ONE_PIXEL + dy * ONE_PIXEL > 0 && prod - dx * ONE_PIXEL <= 0 {
                    // Sai por cima.
                    prod -= dx * ONE_PIXEL;
                    fx2 = udiv(-prod, dy_r);
                    fy2 = ONE_PIXEL as i32;
                    self.integrate(fy2 - fy1, fx1 + fx2);
                    fx1 = fx2;
                    fy1 = 0;
                    ey1 += 1;
                } else if prod + dy * ONE_PIXEL >= 0 && prod - dx * ONE_PIXEL + dy * ONE_PIXEL <= 0 {
                    // Sai pela direita.
                    prod += dy * ONE_PIXEL;
                    fx2 = ONE_PIXEL as i32;
                    fy2 = udiv(prod, dx_r);
                    self.integrate(fy2 - fy1, fx1 + fx2);
                    fx1 = 0;
                    fy1 = fy2;
                    ex1 += 1;
                } else {
                    // Sai por baixo.
                    fx2 = udiv(prod, -dy_r);
                    fy2 = 0;
                    prod += dx * ONE_PIXEL;
                    self.integrate(fy2 - fy1, fx1 + fx2);
                    fx1 = fx2;
                    fy1 = ONE_PIXEL as i32;
                    ey1 -= 1;
                }
                self.set_cell(ex1, ey1);
                if ex1 == ex2 && ey1 == ey2 {
                    break;
                }
            }
        }
        let fx2 = fract(to_x);
        let fy2 = fract(to_y);
        self.integrate(fy2 - fy1, fx1 + fx2);
        self.x = to_x;
        self.y = to_y;
    }

    fn render_conic(&mut self, control: Vector, to: Vector) {
        let p0 = Vector { x: self.x, y: self.y };
        let p1 = Vector { x: upscale(control.x), y: upscale(control.y) };
        let p2 = Vector { x: upscale(to.x), y: upscale(to.y) };
        let out = |y: i64| trunc(y);
        if (out(p0.y) >= self.max_ey && out(p1.y) >= self.max_ey && out(p2.y) >= self.max_ey)
            || (out(p0.y) < self.min_ey && out(p1.y) < self.min_ey && out(p2.y) < self.min_ey)
        {
            self.x = p2.x;
            self.y = p2.y;
            return;
        }
        let bx = p1.x - p0.x;
        let by = p1.y - p0.y;
        let ax = p2.x - p1.x - bx;
        let ay = p2.y - p1.y - by;
        let mut d = ax.abs().max(ay.abs());
        if d <= ONE_PIXEL / 4 {
            self.render_line(p2.x, p2.y);
            return;
        }
        let mut shift = 16;
        loop {
            d >>= 2;
            shift -= 1;
            if d <= ONE_PIXEL / 4 {
                break;
            }
        }
        let mut count = 0x10000u32 >> shift;
        let ls = |a: i64, b: i32| ((a as u64) << b) as i64;
        let mut rx = ls(ax, shift + shift);
        let mut ry = ls(ay, shift + shift);
        let mut qx = ls(bx, shift + 17).wrapping_add(rx);
        let mut qy = ls(by, shift + 17).wrapping_add(ry);
        rx = rx.wrapping_mul(2);
        ry = ry.wrapping_mul(2);
        let mut px = ls(p0.x, 32);
        let mut py = ls(p0.y, 32);
        loop {
            px = px.wrapping_add(qx);
            py = py.wrapping_add(qy);
            qx = qx.wrapping_add(rx);
            qy = qy.wrapping_add(ry);
            self.render_line(px >> 32, py >> 32);
            count -= 1;
            if count == 0 {
                break;
            }
        }
    }

    fn render_cubic(&mut self, c1: Vector, c2: Vector, to: Vector) {
        let mut stack = [Vector::default(); 16 * 3 + 1];
        stack[0] = Vector { x: upscale(to.x), y: upscale(to.y) };
        stack[1] = Vector { x: upscale(c2.x), y: upscale(c2.y) };
        stack[2] = Vector { x: upscale(c1.x), y: upscale(c1.y) };
        stack[3] = Vector { x: self.x, y: self.y };
        let all = |s: &[Vector], f: &dyn Fn(i32) -> bool| s[..4].iter().all(|v| f(trunc(v.y)));
        if all(&stack, &|t| t >= self.max_ey) || all(&stack, &|t| t < self.min_ey) {
            self.x = stack[0].x;
            self.y = stack[0].y;
            return;
        }
        let mut a = 0usize;
        loop {
            let s = &stack[a..];
            let half = ONE_PIXEL / 2;
            if (2 * s[0].x - 3 * s[1].x + s[3].x).abs() > half
                || (2 * s[0].y - 3 * s[1].y + s[3].y).abs() > half
                || (s[0].x - 3 * s[2].x + 2 * s[3].x).abs() > half
                || (s[0].y - 3 * s[2].y + 2 * s[3].y).abs() > half
            {
                split_cubic(&mut stack[a..a + 7]);
                a += 3;
                continue;
            }
            self.render_line(stack[a].x, stack[a].y);
            if a == 0 {
                return;
            }
            a -= 3;
        }
    }

    fn move_to(&mut self, to: Vector) {
        let x = upscale(to.x);
        let y = upscale(to.y);
        self.set_cell(trunc(x), trunc(y));
        self.x = x;
        self.y = y;
    }

    fn line_to(&mut self, to: Vector) {
        self.render_line(upscale(to.x), upscale(to.y));
    }
}

fn split_cubic(b: &mut [Vector]) {
    for axis in 0..2 {
        let g = |v: &Vector| if axis == 0 { v.x } else { v.y };
        let set = |v: &mut Vector, val: i64| if axis == 0 { v.x = val } else { v.y = val };
        let b3 = g(&b[3]);
        set(&mut b[6], b3);
        let mut a = g(&b[0]) + g(&b[1]);
        let bb = g(&b[1]) + g(&b[2]);
        let mut c = g(&b[2]) + b3;
        set(&mut b[5], c >> 1);
        c += bb;
        set(&mut b[4], c >> 2);
        set(&mut b[1], a >> 1);
        a += bb;
        set(&mut b[2], a >> 2);
        set(&mut b[3], (a + c) >> 3);
    }
}

/// `FT_Outline_Decompose` com `shift = 0` e `delta = 0`.
fn decompose(o: &Outline, w: &mut Worker) -> Result<(), ()> {
    let mut last: i64 = -1;
    for &end in &o.contours {
        let first = (last + 1) as usize;
        let lastu = end;
        last = end as i64;
        if lastu < first {
            return Err(());
        }
        let pts = &o.points;
        let tag = |i: usize| o.tags[i] & 3;
        let mut limit = lastu;
        let mut v_start = pts[first];
        let v_last = pts[lastu];
        let mut v_control;
        // `point` aponta para o ponto corrente; usa i64 para poder recuar um antes de `first`.
        let mut point = first as i64;
        let t0 = tag(first);
        if t0 == TAG_CUBIC {
            return Err(());
        }
        if t0 == 0 {
            if tag(lastu) == TAG_ON {
                v_start = v_last;
                limit -= 1;
            } else {
                v_start.x = (v_start.x + v_last.x) / 2;
                v_start.y = (v_start.y + v_last.y) / 2;
            }
            point -= 1;
        }
        w.move_to(v_start);
        let limit = limit as i64;
        let mut closed = false;
        while point < limit {
            point += 1;
            let p = point as usize;
            match tag(p) {
                1 => {
                    w.line_to(pts[p]);
                    continue;
                }
                0 => {
                    v_control = pts[p];
                    loop {
                        if point < limit {
                            point += 1;
                            let q = point as usize;
                            let vec = pts[q];
                            match tag(q) {
                                1 => {
                                    w.render_conic(v_control, vec);
                                    break;
                                }
                                0 => {}
                                _ => return Err(()),
                            }
                            let mid = Vector { x: (v_control.x + vec.x) / 2, y: (v_control.y + vec.y) / 2 };
                            w.render_conic(v_control, mid);
                            v_control = vec;
                            continue;
                        }
                        w.render_conic(v_control, v_start);
                        closed = true;
                        break;
                    }
                    if closed {
                        break;
                    }
                }
                _ => {
                    if point + 1 > limit || tag(p + 1) != TAG_CUBIC {
                        return Err(());
                    }
                    point += 2;
                    let (v1, v2) = (pts[point as usize - 2], pts[point as usize - 1]);
                    if point <= limit {
                        w.render_cubic(v1, v2, pts[point as usize]);
                        continue;
                    }
                    w.render_cubic(v1, v2, v_start);
                    closed = true;
                    break;
                }
            }
        }
        if !closed {
            w.line_to(v_start);
        }
    }
    Ok(())
}

fn fill_rule(area: i32, even_odd: bool) -> u8 {
    let fill: i32 = if even_odd { 0x100 } else { i32::MIN };
    let mut c = area >> (PIXEL_BITS * 2 + 1 - 8);
    if c & fill != 0 {
        c = !c;
    }
    if c > 255 && fill & i32::MIN != 0 {
        c = 255;
    }
    c as u8
}

/// Rasteriza o contorno na caixa `[x0, x1) x [y0, y1)` e entrega os intervalos de cada linha
/// (`gray_sweep_direct`), de baixo para cima.
pub fn raster_spans(o: &Outline, clip: (i32, i32, i32, i32), even_odd: bool, mut emit: impl FnMut(i32, &[Span])) {
    let (x0, y0, x1, y1) = clip;
    if o.points.is_empty() || o.contours.is_empty() || x0 >= x1 || y0 >= y1 {
        return;
    }
    let mut w = Worker {
        min_ex: x0,
        max_ex: x1,
        min_ey: y0,
        max_ey: y1,
        rows: vec![Vec::new(); (y1 - y0) as usize],
        cell: None,
        x: 0,
        y: 0,
    };
    if decompose(o, &mut w).is_err() {
        return;
    }
    let mut spans = Vec::new();
    for (i, row) in w.rows.iter().enumerate() {
        spans.clear();
        let mut x = x0;
        let mut cover: i32 = 0;
        for c in row {
            if cover != 0 && c.x > x {
                spans.push(Span { x, len: c.x - x, coverage: fill_rule(cover, even_odd) });
            }
            cover = cover.wrapping_add(c.cover.wrapping_mul((ONE_PIXEL * 2) as i32));
            let area = cover.wrapping_sub(c.area);
            if area != 0 && c.x >= x0 {
                spans.push(Span { x: c.x, len: 1, coverage: fill_rule(area, even_odd) });
            }
            x = c.x + 1;
        }
        if cover != 0 {
            spans.push(Span { x, len: x1 - x, coverage: fill_rule(cover, even_odd) });
        }
        if !spans.is_empty() {
            emit(y0 + i as i32, &spans);
        }
    }
}

/// `FT_Bitmap` em tons de cinza (`FT_PIXEL_MODE_GRAY`), de cima para baixo.
#[derive(Clone, Debug, Default)]
pub struct Bitmap {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub rows: u32,
    pub pitch: i32,
    pub buffer: Vec<u8>,
}

/// `ft_glyphslot_preset_bitmap` no modo normal: caixa em pixels e origem do bitmap.
pub fn preset(o: &Outline) -> (i64, i64, i64, i64) {
    let cb = o.cbox();
    let mut px0 = cb.x_min >> 6;
    let mut py0 = cb.y_min >> 6;
    let mut px1 = cb.x_max >> 6;
    let mut py1 = cb.y_max >> 6;
    px0 += (cb.x_min & 63) >> 6;
    py0 += (cb.y_min & 63) >> 6;
    px1 += ((cb.x_max & 63) + 63) >> 6;
    py1 += ((cb.y_max & 63) + 63) >> 6;
    (px0, py0, px1, py1)
}

/// `ft_smooth_render` no modo `FT_RENDER_MODE_NORMAL`. `overlap` liga a superamostragem 4x do
/// `ft_smooth_raster_overlap`, usada quando o contorno tem a marca `FT_OUTLINE_OVERLAP`.
pub fn render(o: &Outline, overlap: bool) -> Option<Bitmap> {
    let (x0, y0, x1, y1) = preset(o);
    if x0 < -0x8000 || x1 > 0x7FFF || y0 < -0x8000 || y1 > 0x7FFF {
        return None;
    }
    let width = (x1 - x0) as u32;
    let rows = (y1 - y0) as u32;
    let mut bm = Bitmap { left: x0 as i32, top: y1 as i32, width, rows, pitch: width as i32, buffer: Vec::new() };
    if rows == 0 || width == 0 {
        return Some(bm);
    }
    bm.buffer = vec![0u8; (rows * width) as usize];
    let mut t = o.clone();
    t.translate(-64 * x0, -64 * y0);
    let w = width as usize;
    if overlap {
        const SCALE: i64 = 4;
        if width as i64 * SCALE > 0x7FFF {
            return None;
        }
        for p in &mut t.points {
            p.x *= SCALE;
            p.y *= SCALE;
        }
        let clip = (0, 0, (width as i64 * SCALE) as i32, (rows as i64 * SCALE) as i32);
        raster_spans(&t, clip, false, |y, spans| {
            let row = rows as usize - 1 - (y / SCALE as i32) as usize;
            let line = &mut bm.buffer[row * w..row * w + w];
            for s in spans {
                let cover = (u32::from(s.coverage) + (SCALE * SCALE / 2) as u32) / (SCALE * SCALE) as u32;
                for k in 0..s.len {
                    let d = &mut line[((s.x + k) / SCALE as i32) as usize];
                    let sum = u32::from(*d) + cover;
                    *d = (sum - (sum >> 8)) as u8;
                }
            }
        });
    } else {
        raster_spans(&t, (0, 0, width as i32, rows as i32), false, |y, spans| {
            let row = rows as usize - 1 - y as usize;
            let line = &mut bm.buffer[row * w..row * w + w];
            for s in spans {
                line[s.x as usize..(s.x + s.len) as usize].fill(s.coverage);
            }
        });
    }
    Some(bm)
}
