//! Primitivas de desenho da libImaging (`Draw.c` do Pillow 11.1.0): pontos, linhas de Bresenham,
//! linhas largas, retângulos, polígonos pela varredura de arestas, elipses pelo algoritmo de
//! quartos e os recortes de arco, corda e fatia. A aritmética segue a do C: as arestas e as
//! interseções da varredura são `float` (f32), as linhas largas e os recortes são `double`.
//!
//! Portado da libImaging (MIT-CMU: Copyright © 1996-2006 Fredrik Lundh, © 1997-2006 Secret Labs AB).

use super::image::{Image, blend};

#[derive(Clone, Copy, Default)]
pub struct Edge {
    pub d: i32,
    pub x0: i32,
    pub y0: i32,
    pub xmin: i32,
    pub ymin: i32,
    pub xmax: i32,
    pub ymax: i32,
    pub dx: f32,
}

/// Qual família de primitivas o `DRAWINIT` escolhe.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    D8,
    D32,
    D32Rgba,
}

struct Draw {
    kind: Kind,
    ink: i32,
}

/// `DRAWINIT`: a família pelo armazenamento e a tinta lida dos 4 bytes do `ink_`.
fn drawinit(im: &Image, ink: i32, op: bool) -> Draw {
    if im.is8() {
        let b = ink.to_le_bytes();
        let ink = if im.is_i16() { i32::from(u16::from_le_bytes([b[0], b[1]])) } else { i32::from(b[0]) };
        Draw { kind: Kind::D8, ink }
    } else {
        Draw { kind: if op { Kind::D32Rgba } else { Kind::D32 }, ink }
    }
}

fn round_up(f: f64) -> i32 {
    if f >= 0.0 { (f + 0.5).floor() as i32 } else { -((f.abs() + 0.5).floor() as i32) }
}

fn round_down(f: f64) -> i32 {
    if f >= 0.0 { (f - 0.5).ceil() as i32 } else { -((f.abs() - 0.5).ceil() as i32) }
}

/// `ROUND_UP` aplicado a um `float`: a soma com `0.5F` acontece em f32.
fn round_up_f(f: f32) -> i32 {
    if f >= 0.0 { f64::from(f + 0.5).floor() as i32 } else { -(f64::from(f.abs() + 0.5).floor() as i32) }
}

fn round_down_f(f: f32) -> i32 {
    if f >= 0.0 { f64::from(f - 0.5).ceil() as i32 } else { -(f64::from(f.abs() - 0.5).ceil() as i32) }
}

impl Draw {
    fn point(&self, im: &mut Image, x: i32, y: i32) {
        if x < 0 || x >= im.xsize || y < 0 || y >= im.ysize {
            return;
        }
        let o = im.offset(x, y);
        match self.kind {
            Kind::D8 => {
                if im.is_i16() {
                    im.data[o] = self.ink as u8;
                    im.data[o + 1] = (self.ink >> 8) as u8;
                } else {
                    im.data[o] = self.ink as u8;
                }
            }
            Kind::D32 => im.data[o..o + 4].copy_from_slice(&self.ink.to_le_bytes()),
            Kind::D32Rgba => {
                let i = self.ink.to_le_bytes();
                let a = u32::from(i[3]);
                for k in 0..3 {
                    im.data[o + k] = blend(a, u32::from(im.data[o + k]), u32::from(i[k]));
                }
            }
        }
    }

    fn hline(&self, im: &mut Image, mut x0: i32, y0: i32, mut x1: i32) {
        if y0 < 0 || y0 >= im.ysize {
            return;
        }
        if x0 < 0 {
            x0 = 0;
        } else if x0 >= im.xsize {
            return;
        }
        if x1 < 0 {
            return;
        } else if x1 >= im.xsize {
            x1 = im.xsize - 1;
        }
        if x0 > x1 {
            return;
        }
        match self.kind {
            Kind::D8 => {
                let pw = if im.is_i16() { 2 } else { 1 };
                let o = (y0 * im.linesize + x0 * pw) as usize;
                let n = ((x1 - x0 + 1) * pw) as usize;
                im.data[o..o + n].fill(self.ink as u8);
            }
            _ => {
                for x in x0..=x1 {
                    self.point(im, x, y0);
                }
            }
        }
    }

    fn line(&self, im: &mut Image, mut x0: i32, mut y0: i32, x1: i32, y1: i32) {
        let mut dx = x1 - x0;
        let xs = if dx < 0 {
            dx = -dx;
            -1
        } else {
            1
        };
        let mut dy = y1 - y0;
        let ys = if dy < 0 {
            dy = -dy;
            -1
        } else {
            1
        };
        if dx == 0 {
            for _ in 0..dy {
                self.point(im, x0, y0);
                y0 += ys;
            }
        } else if dy == 0 {
            for _ in 0..dx {
                self.point(im, x0, y0);
                x0 += xs;
            }
        } else if dx > dy {
            let n = dx;
            dy += dy;
            let mut e = dy - dx;
            dx += dx;
            for _ in 0..n {
                self.point(im, x0, y0);
                if e >= 0 {
                    y0 += ys;
                    e -= dx;
                }
                e += dy;
                x0 += xs;
            }
        } else {
            let n = dy;
            dx += dx;
            let mut e = dx - dy;
            dy += dy;
            for _ in 0..n {
                self.point(im, x0, y0);
                if e >= 0 {
                    x0 += xs;
                    e -= dy;
                }
                e += dx;
                y0 += ys;
            }
        }
    }

    fn polygon(&self, im: &mut Image, e: &[Edge]) {
        polygon_generic(self, im, e, self.kind == Kind::D32Rgba);
    }
}

fn draw_horizontal_lines(d: &Draw, im: &mut Image, e: &[Edge], x_pos: &mut i32, y: i32) {
    for ed in e {
        if ed.ymin == y && ed.ymin == ed.ymax {
            let mut xmin = ed.xmin;
            if *x_pos != -1 && *x_pos < xmin {
                continue;
            }
            let xmax = ed.xmax;
            if *x_pos > xmin {
                xmin = *x_pos;
                if xmax < xmin {
                    continue;
                }
            }
            d.hline(im, xmin, ed.ymin, xmax);
            *x_pos = xmax + 1;
        }
    }
}

fn edge_x(e: &Edge, y: i32) -> f32 {
    (y - e.y0) as f32 * e.dx + e.x0 as f32
}

fn polygon_generic(d: &Draw, im: &mut Image, e: &[Edge], has_alpha: bool) {
    if e.is_empty() {
        return;
    }
    let mut ymin = im.ysize - 1;
    let mut ymax = 0;
    let mut table: Vec<usize> = Vec::new();
    for (i, ed) in e.iter().enumerate() {
        if ymin > ed.ymin {
            ymin = ed.ymin;
        }
        if ymax < ed.ymax {
            ymax = ed.ymax;
        }
        if ed.ymin == ed.ymax {
            if !has_alpha {
                d.hline(im, ed.xmin, ed.ymin, ed.xmax);
            }
            continue;
        }
        table.push(i);
    }
    if ymin < 0 {
        ymin = 0;
    }
    if ymax > im.ysize {
        ymax = im.ysize;
    }
    let mut xx = vec![0f32; table.len() * 2];
    while ymin <= ymax {
        let mut j = 0usize;
        for i in 0..table.len() {
            let cur = &e[table[i]];
            if ymin >= cur.ymin && ymin <= cur.ymax {
                xx[j] = edge_x(cur, ymin);
                j += 1;
                if ymin == cur.ymax && ymin < ymax {
                    xx[j] = xx[j - 1];
                    j += 1;
                } else if cur.dx != 0.0 && j % 2 == 1 && xx[j - 1].round() == xx[j - 1] {
                    for k in 0..i {
                        let other = &e[table[k]];
                        if (cur.dx > 0.0 && other.dx <= 0.0) || (cur.dx < 0.0 && other.dx >= 0.0) {
                            continue;
                        }
                        if xx[j - 1] == edge_x(other, ymin) {
                            let offset = if ymin == ymax { -1 } else { 1 };
                            let adj = f64::from(edge_x(cur, ymin + offset));
                            let adj_other = f64::from(edge_x(other, ymin + offset));
                            xx[k] = if ymin == cur.ymax {
                                if cur.dx > 0.0 { adj.max(adj_other) + 1.0 } else { adj.min(adj_other) - 1.0 }
                            } else if cur.dx > 0.0 {
                                adj.min(adj_other)
                            } else {
                                adj.max(adj_other) + 1.0
                            } as f32;
                            break;
                        }
                    }
                }
            }
        }
        // qsort não é estável, mas os empates são valores iguais: a ordem entre eles não importa.
        xx[..j].sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        if has_alpha {
            let mut x_pos = if j == 0 { -1 } else { 0 };
            let mut i = 1;
            while i < j {
                let x_end = round_down_f(xx[i]);
                if x_end < x_pos {
                    i += 2;
                    continue;
                }
                draw_horizontal_lines(d, im, e, &mut x_pos, ymin);
                if x_end < x_pos {
                    i += 2;
                    continue;
                }
                let mut x_start = round_up_f(xx[i - 1]);
                if x_pos > x_start {
                    x_start = x_pos;
                    if x_end < x_start {
                        i += 2;
                        continue;
                    }
                }
                d.hline(im, x_start, ymin, x_end);
                x_pos = x_end + 1;
                i += 2;
            }
            draw_horizontal_lines(d, im, e, &mut x_pos, ymin);
        } else {
            let mut i = 1;
            while i < j {
                d.hline(im, round_up_f(xx[i - 1]), ymin, round_down_f(xx[i]));
                i += 2;
            }
        }
        ymin += 1;
    }
}

pub fn add_edge(x0: i32, y0: i32, x1: i32, y1: i32) -> Edge {
    let mut e = Edge::default();
    if x0 <= x1 {
        e.xmin = x0;
        e.xmax = x1;
    } else {
        e.xmin = x1;
        e.xmax = x0;
    }
    if y0 <= y1 {
        e.ymin = y0;
        e.ymax = y1;
    } else {
        e.ymin = y1;
        e.ymax = y0;
    }
    if y0 == y1 {
        e.d = 0;
        e.dx = 0.0;
    } else {
        e.dx = (x1 - x0) as f32 / (y1 - y0) as f32;
        e.d = if y0 == e.ymin { 1 } else { -1 };
    }
    e.x0 = x0;
    e.y0 = y0;
    e
}

pub fn point(im: &mut Image, x0: i32, y0: i32, ink: i32, op: bool) {
    let d = drawinit(im, ink, op);
    d.point(im, x0, y0);
}

pub fn line(im: &mut Image, x0: i32, y0: i32, x1: i32, y1: i32, ink: i32, op: bool) {
    let d = drawinit(im, ink, op);
    d.line(im, x0, y0, x1, y1);
}

pub fn wide_line(im: &mut Image, x0: i32, y0: i32, x1: i32, y1: i32, ink: i32, width: i32, op: bool) {
    let d = drawinit(im, ink, op);
    let dx = x1 - x0;
    let dy = y1 - y0;
    if dx == 0 && dy == 0 {
        d.point(im, x0, y0);
        return;
    }
    let big = f64::from(dx).hypot(f64::from(dy));
    let small = f64::from(width - 1) / 2.0;
    let ratio_max = f64::from(round_up(small)) / big;
    let ratio_min = f64::from(round_down(small)) / big;
    let dxmin = round_down(ratio_min * f64::from(dy));
    let dxmax = round_down(ratio_max * f64::from(dy));
    let dymin = round_down(ratio_min * f64::from(dx));
    let dymax = round_down(ratio_max * f64::from(dx));
    let v = [
        (x0 - dxmin, y0 + dymax),
        (x1 - dxmin, y1 + dymax),
        (x1 + dxmax, y1 - dymin),
        (x0 + dxmax, y0 - dymin),
    ];
    let e = [
        add_edge(v[0].0, v[0].1, v[1].0, v[1].1),
        add_edge(v[1].0, v[1].1, v[2].0, v[2].1),
        add_edge(v[2].0, v[2].1, v[3].0, v[3].1),
        add_edge(v[3].0, v[3].1, v[0].0, v[0].1),
    ];
    d.polygon(im, &e);
}

#[allow(clippy::too_many_arguments)]
pub fn rectangle(im: &mut Image, x0: i32, mut y0: i32, x1: i32, mut y1: i32, ink: i32, fill: bool, mut width: i32, op: bool) {
    let d = drawinit(im, ink, op);
    if y0 > y1 {
        std::mem::swap(&mut y0, &mut y1);
    }
    if fill {
        if y0 < 0 {
            y0 = 0;
        } else if y0 >= im.ysize {
            return;
        }
        if y1 < 0 {
            return;
        } else if y1 > im.ysize {
            y1 = im.ysize;
        }
        for y in y0..=y1 {
            d.hline(im, x0, y, x1);
        }
    } else {
        if width == 0 {
            width = 1;
        }
        for i in 0..width {
            d.hline(im, x0, y0 + i, x1);
            d.hline(im, x0, y1 - i, x1);
            d.line(im, x1 - i, y0 + width, x1 - i, y1 - width + 1);
            d.line(im, x0 + i, y0 + width, x0 + i, y1 - width + 1);
        }
    }
}

pub fn polygon(im: &mut Image, xy: &[i32], ink: i32, fill: bool, width: i32, op: bool) {
    let count = xy.len() / 2;
    if count == 0 {
        return;
    }
    let d = drawinit(im, ink, op);
    if fill {
        let mut e: Vec<Edge> = Vec::with_capacity(count);
        let mut i = 0;
        while i + 1 < count {
            let (x0, y0, x1, y1) = (xy[i * 2], xy[i * 2 + 1], xy[i * 2 + 2], xy[i * 2 + 3]);
            if y0 == y1 && i != 0 && y0 == xy[i * 2 - 1] {
                let n = e.len();
                if x1 > x0 && x0 > xy[i * 2 - 2] {
                    e[n - 1].xmax = x1;
                    i += 1;
                    continue;
                } else if x1 < x0 && x0 < xy[i * 2 - 2] {
                    e[n - 1].xmin = x1;
                    i += 1;
                    continue;
                }
            }
            e.push(add_edge(x0, y0, x1, y1));
            i += 1;
        }
        if xy[i * 2] != xy[0] || xy[i * 2 + 1] != xy[1] {
            e.push(add_edge(xy[i * 2], xy[i * 2 + 1], xy[0], xy[1]));
        }
        d.polygon(im, &e);
    } else if width == 1 {
        let mut i = 0;
        while i + 1 < count {
            d.line(im, xy[i * 2], xy[i * 2 + 1], xy[i * 2 + 2], xy[i * 2 + 3]);
            i += 1;
        }
        d.line(im, xy[i * 2], xy[i * 2 + 1], xy[0], xy[1]);
    } else {
        let mut i = 0;
        while i + 1 < count {
            wide_line(im, xy[i * 2], xy[i * 2 + 1], xy[i * 2 + 2], xy[i * 2 + 3], ink, width, op);
            i += 1;
        }
        wide_line(im, xy[i * 2], xy[i * 2 + 1], xy[0], xy[1], ink, width, op);
    }
}

/// `ImagingDrawOutline`: o polígono das arestas já montadas.
pub fn outline(im: &mut Image, e: &[Edge], ink: i32, op: bool) {
    let d = drawinit(im, ink, op);
    d.polygon(im, e);
}

// ---- elipses ----

#[derive(Clone, Copy, Default)]
struct Quarter {
    cx: i32,
    cy: i32,
    ex: i32,
    ey: i32,
    a2: i64,
    b2: i64,
    a2b2: i64,
    finished: bool,
}

impl Quarter {
    fn new(a: i32, b: i32) -> Quarter {
        if a < 0 || b < 0 {
            return Quarter { finished: true, ..Quarter::default() };
        }
        // `a * a` é feito em int32 no original antes de virar int64.
        let a2 = i64::from(a.wrapping_mul(a));
        let b2 = i64::from(b.wrapping_mul(b));
        Quarter { cx: a, cy: b % 2, ex: a % 2, ey: b, a2, b2, a2b2: a2.wrapping_mul(b2), finished: false }
    }

    fn delta(&self, x: i64, y: i64) -> i64 {
        (self.a2 * y * y + self.b2 * x * x - self.a2b2).abs()
    }

    fn next(&mut self) -> Option<(i32, i32)> {
        if self.finished {
            return None;
        }
        let ret = (self.cx, self.cy);
        if self.cx == self.ex && self.cy == self.ey {
            self.finished = true;
        } else {
            let mut nx = self.cx;
            let mut ny = self.cy + 2;
            let mut nd = self.delta(i64::from(nx), i64::from(ny));
            if nx > 1 {
                let d = self.delta(i64::from(self.cx - 2), i64::from(self.cy + 2));
                if nd > d {
                    nx = self.cx - 2;
                    ny = self.cy + 2;
                    nd = d;
                }
                let d = self.delta(i64::from(self.cx - 2), i64::from(self.cy));
                if nd > d {
                    nx = self.cx - 2;
                    ny = self.cy;
                }
            }
            self.cx = nx;
            self.cy = ny;
        }
        Some(ret)
    }
}

#[derive(Clone, Copy, Default)]
struct Ellipse {
    st_o: Quarter,
    st_i: Quarter,
    py: i32,
    pl: i32,
    pr: i32,
    cy: [i32; 4],
    cl: [i32; 4],
    cr: [i32; 4],
    bufcnt: usize,
    finished: bool,
    leftmost: i32,
}

impl Ellipse {
    fn new(a: i32, b: i32, w: i32) -> Ellipse {
        let mut s = Ellipse { leftmost: a % 2, st_o: Quarter::new(a, b), ..Ellipse::default() };
        let first = if w < 1 { None } else { s.st_o.next() };
        match first {
            None => s.finished = true,
            Some((x, y)) => {
                s.pr = x;
                s.py = y;
                s.st_i = Quarter::new(a - 2 * (w - 1), b - 2 * (w - 1));
                s.pl = s.leftmost;
            }
        }
        s
    }

    fn next(&mut self) -> Option<(i32, i32, i32)> {
        if self.bufcnt == 0 {
            if self.finished {
                return None;
            }
            let y = self.py;
            let mut l = self.pl;
            let r = self.pr;
            let got = loop {
                match self.st_o.next() {
                    Some((_, cy)) if cy <= y => {}
                    other => break other,
                }
            };
            match got {
                None => self.finished = true,
                Some((cx, cy)) => {
                    self.pr = cx;
                    self.py = cy;
                }
            }
            let inner = loop {
                match self.st_i.next() {
                    Some((cx, cy)) if cy <= y => l = cx,
                    other => break other,
                }
            };
            self.pl = match inner {
                None => self.leftmost,
                Some((cx, _)) => cx,
            };
            let push = |s: &mut Ellipse, cl: i32, cy: i32, cr: i32| {
                s.cl[s.bufcnt] = cl;
                s.cy[s.bufcnt] = cy;
                s.cr[s.bufcnt] = cr;
                s.bufcnt += 1;
            };
            if (l > 0 || l < r) && y > 0 {
                push(self, if l == 0 { 2 } else { l }, y, r);
            }
            if y > 0 {
                push(self, -r, y, -l);
            }
            if l > 0 || l < r {
                push(self, if l == 0 { 2 } else { l }, -y, r);
            }
            push(self, -r, -y, -l);
        }
        self.bufcnt -= 1;
        let k = self.bufcnt;
        Some((self.cl[k], self.cy[k], self.cr[k]))
    }
}

#[allow(clippy::too_many_arguments)]
pub fn ellipse(im: &mut Image, x0: i32, y0: i32, x1: i32, y1: i32, ink: i32, fill: bool, mut width: i32, op: bool) {
    let d = drawinit(im, ink, op);
    let a = x1 - x0;
    let b = y1 - y0;
    if a < 0 || b < 0 {
        return;
    }
    if fill {
        width = a + b;
    }
    let mut st = Ellipse::new(a, b, width);
    while let Some((xl, y, xr)) = st.next() {
        d.hline(im, x0 + (xl + a) / 2, y0 + (y + b) / 2, x0 + (xr + a) / 2);
    }
}

// ---- recortes (arco, corda, fatia) ----

#[derive(Clone, Copy, PartialEq)]
enum Ct {
    And,
    Or,
    Clip,
}

#[derive(Clone)]
struct Node {
    t: Ct,
    a: f64,
    b: f64,
    c: f64,
    l: Option<usize>,
    r: Option<usize>,
}

impl Node {
    fn clip(a: f64, b: f64, c: f64) -> Node {
        Node { t: Ct::Clip, a, b, c, l: None, r: None }
    }

    fn comb(t: Ct, l: usize, r: usize) -> Node {
        Node { t, a: 0.0, b: 0.0, c: 0.0, l: Some(l), r: Some(r) }
    }
}

struct ClipTree {
    nodes: Vec<Node>,
    root: Option<usize>,
}

impl ClipTree {
    fn add(&mut self, n: Node) -> usize {
        self.nodes.push(n);
        self.nodes.len() - 1
    }

    fn transpose(&mut self) {
        for n in &mut self.nodes {
            if n.t == Ct::Clip {
                std::mem::swap(&mut n.a, &mut n.b);
            }
        }
    }

    /// Eventos `(x, tipo)`: 1 abre, -1 fecha.
    fn do_clip(&self, node: Option<usize>, mut x0: i32, y: i32, mut x1: i32) -> Vec<(i32, i8)> {
        let Some(n) = node else {
            return vec![(x0, 1), (x1, -1)];
        };
        let nd = &self.nodes[n];
        match nd.t {
            Ct::Clip => {
                let eps = 1e-9;
                let (a, b, c) = (nd.a, nd.b, nd.c);
                let yf = f64::from(y);
                if a.abs() < eps {
                    if b * yf + c < -eps {
                        x0 = 1;
                        x1 = 0;
                    }
                } else {
                    let ix = -(b * yf + c) / a;
                    if a * f64::from(x0) + b * yf + c < eps {
                        x0 = f64::from(x0).max(ix).round() as i32;
                    }
                    if a * f64::from(x1) + b * yf + c < eps {
                        x1 = f64::from(x1).min(ix).round() as i32;
                    }
                }
                if x0 <= x1 { vec![(x0, 1), (x1, -1)] } else { Vec::new() }
            }
            Ct::Or | Ct::And => {
                let l1 = self.do_clip(nd.l, x0, y, x1);
                let l2 = self.do_clip(nd.r, x0, y, x1);
                let (mut i1, mut i2) = (0, 0);
                let (mut k1, mut k2) = (0i32, 0i32);
                let mut out: Vec<(i32, i8)> = Vec::new();
                while i1 < l1.len() || i2 < l2.len() {
                    let take1 = i2 >= l2.len()
                        || (i1 < l1.len() && (l1[i1].0 < l2[i2].0 || (l1[i1].0 == l2[i2].0 && l1[i1].1 > l2[i2].1)));
                    let t = if take1 {
                        let t = l1[i1];
                        k1 += i32::from(t.1);
                        i1 += 1;
                        t
                    } else {
                        let t = l2[i2];
                        k2 += i32::from(t.1);
                        i2 += 1;
                        t
                    };
                    let tail = out.last().map(|e| e.1);
                    let keep = if nd.t == Ct::Or {
                        (t.1 == 1 && (tail.is_none() || tail == Some(-1))) || (t.1 == -1 && k1 == 0 && k2 == 0)
                    } else {
                        (t.1 == 1 && (tail.is_none() || tail == Some(-1)) && k1 > 0 && k2 > 0)
                            || (t.1 == -1 && tail == Some(1) && (k1 == 0 || k2 == 0))
                    };
                    if keep {
                        out.push(t);
                    }
                }
                out
            }
        }
    }
}

/// Ângulos em `float` como no original: `0 <= al < 360` e `al <= ar <= al + 360`.
fn normalize_angles(al: &mut f32, ar: &mut f32) {
    if *ar - *al >= 360.0 {
        *al = 0.0;
        *ar = 360.0;
    } else {
        let l = f64::from(*al);
        let nl = (if l < 0.0 { 360.0 - (-l % 360.0) } else { l }) % 360.0;
        *al = nl as f32;
        // `*ar - *al` e `*al - *ar` são subtrações de float no original.
        let span = if *ar < *al { 360.0 - f64::from(*al - *ar) % 360.0 } else { f64::from(*ar - *al) };
        *ar = (f64::from(*al) + span % 360.0) as f32;
    }
}

struct ClipEllipse {
    st: Ellipse,
    tree: ClipTree,
    pending: Vec<(i32, i8)>,
    pos: usize,
    y: i32,
}

type ClipInit = fn(i32, i32, i32, f32, f32) -> ClipEllipse;

fn deg(v: f32) -> f64 {
    f64::from(v) * std::f64::consts::PI / 180.0
}

fn arc_init(a: i32, b: i32, w: i32, al: f32, ar: f32) -> ClipEllipse {
    if a < b {
        let mut s = arc_init(b, a, w, 90.0 - ar, 90.0 - al);
        s.st = Ellipse::new(a, b, w);
        s.tree.transpose();
        return s;
    }
    let st = Ellipse::new(a, b, w);
    let mut tree = ClipTree { nodes: Vec::new(), root: None };
    let (mut al, mut ar) = (al, ar);
    normalize_angles(&mut al, &mut ar);
    if ar != al + 360.0 {
        let (af, bf) = (f64::from(a), f64::from(b));
        let lc = tree.add(Node::clip(
            -af * deg(al).sin(),
            bf * deg(al).cos(),
            f64::from(a * a - b * b) * (f64::from(al) * std::f64::consts::PI / 90.0).sin() / 2.0,
        ));
        let rc = tree.add(Node::clip(
            af * deg(ar).sin(),
            -bf * deg(ar).cos(),
            f64::from(b * b - a * a) * (f64::from(ar) * std::f64::consts::PI / 90.0).sin() / 2.0,
        ));
        let (alf, arf) = (f64::from(al), f64::from(ar));
        if alf % 180.0 == 0.0 || arf % 180.0 == 0.0 {
            let t = if ar - al < 180.0 { Ct::And } else { Ct::Or };
            tree.root = Some(tree.add(Node::comb(t, lc, rc)));
        } else if ((alf / 180.0) as i32 + (arf / 180.0) as i32) % 2 == 1 {
            let ll = tree.add(Node::clip(0.0, if (alf / 180.0) as i32 % 2 == 0 { 1.0 } else { -1.0 }, 0.0));
            let l = tree.add(Node::comb(Ct::And, ll, lc));
            let rl = tree.add(Node::clip(0.0, if (arf / 180.0) as i32 % 2 == 0 { 1.0 } else { -1.0 }, 0.0));
            let r = tree.add(Node::comb(Ct::And, rl, rc));
            tree.root = Some(tree.add(Node::comb(Ct::Or, l, r)));
        } else {
            let t = if ar - al < 180.0 { Ct::And } else { Ct::Or };
            let l = tree.add(Node::comb(t, lc, rc));
            let r = tree.add(Node::clip(0.0, if ar < 180.0 || ar > 540.0 { 1.0 } else { -1.0 }, 0.0));
            tree.root = Some(tree.add(Node::comb(t, l, r)));
        }
    }
    ClipEllipse { st, tree, pending: Vec::new(), pos: 0, y: 0 }
}

fn chord_line_init(a: i32, b: i32, w: i32, al: f32, ar: f32) -> ClipEllipse {
    let st = Ellipse::new(a, b, a + b + 1);
    let mut tree = ClipTree { nodes: Vec::new(), root: None };
    let (af, bf) = (f64::from(a), f64::from(b));
    let (xl, xr) = (af * deg(al).cos(), af * deg(ar).cos());
    let (yl, yr) = (bf * deg(al).sin(), bf * deg(ar).sin());
    let la = yr - yl;
    let lb = xl - xr;
    let lcc = -(la * xl + lb * yl);
    let l = tree.add(Node::clip(la, lb, lcc));
    let r = tree.add(Node::clip(-la, -lb, 2.0 * f64::from(w) * (la.powf(2.0) + lb.powf(2.0)).sqrt() - lcc));
    tree.root = Some(tree.add(Node::comb(Ct::And, l, r)));
    ClipEllipse { st, tree, pending: Vec::new(), pos: 0, y: 0 }
}

fn pie_side_init(a: i32, b: i32, w: i32, al: f32, _ar: f32) -> ClipEllipse {
    let st = Ellipse::new(a, b, a + b + 1);
    let mut tree = ClipTree { nodes: Vec::new(), root: None };
    let xl = f64::from(a) * deg(al).cos();
    let yl = f64::from(b) * deg(al).sin();
    let a1 = -yl;
    let b1 = xl;
    let c1 = f64::from(w) * (a1 * a1 + b1 * b1).sqrt();
    let n1 = tree.add(Node::clip(a1, b1, c1));
    let n2 = tree.add(Node::clip(-a1, -b1, c1));
    let l = tree.add(Node::comb(Ct::And, n1, n2));
    let r = tree.add(Node::clip(b1, -a1, 0.0));
    tree.root = Some(tree.add(Node::comb(Ct::And, l, r)));
    ClipEllipse { st, tree, pending: Vec::new(), pos: 0, y: 0 }
}

fn chord_init(a: i32, b: i32, w: i32, al: f32, ar: f32) -> ClipEllipse {
    let st = Ellipse::new(a, b, w);
    let mut tree = ClipTree { nodes: Vec::new(), root: None };
    let (af, bf) = (f64::from(a), f64::from(b));
    let (xl, xr) = (af * deg(al).cos(), af * deg(ar).cos());
    let (yl, yr) = (bf * deg(al).sin(), bf * deg(ar).sin());
    let ca = yr - yl;
    let cb = xl - xr;
    tree.root = Some(tree.add(Node::clip(ca, cb, -(ca * xl + cb * yl))));
    ClipEllipse { st, tree, pending: Vec::new(), pos: 0, y: 0 }
}

fn pie_init(a: i32, b: i32, w: i32, al: f32, ar: f32) -> ClipEllipse {
    let st = Ellipse::new(a, b, w);
    let mut tree = ClipTree { nodes: Vec::new(), root: None };
    let (af, bf) = (f64::from(a), f64::from(b));
    let (xl, xr) = (af * deg(al).cos(), af * deg(ar).cos());
    let (yl, yr) = (bf * deg(al).sin(), bf * deg(ar).sin());
    let lc = tree.add(Node::clip(-yl, xl, 0.0));
    let rc = tree.add(Node::clip(yr, -xr, 0.0));
    let t = if ar - al < 180.0 { Ct::And } else { Ct::Or };
    let mut root = tree.add(Node::comb(t, lc, rc));
    if ar - al < 90.0 {
        let spike = tree.add(Node::clip((xl + xr) / 2.0, (yl + yr) / 2.0, 0.0));
        root = tree.add(Node::comb(Ct::And, root, spike));
    }
    tree.root = Some(root);
    ClipEllipse { st, tree, pending: Vec::new(), pos: 0, y: 0 }
}

impl ClipEllipse {
    fn next(&mut self) -> Option<(i32, i32, i32)> {
        while self.pos >= self.pending.len() {
            let (x0, y, x1) = self.st.next()?;
            self.pending = self.tree.do_clip(self.tree.root, x0, y, x1);
            self.pos = 0;
            self.y = y;
        }
        let a = self.pending[self.pos].0;
        let b = self.pending[self.pos + 1].0;
        self.pos += 2;
        Some((a, self.y, b))
    }
}

#[allow(clippy::too_many_arguments)]
fn clip_ellipse_new(
    im: &mut Image,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    start: f32,
    end: f32,
    ink: i32,
    width: i32,
    op: bool,
    init: ClipInit,
) {
    let d = drawinit(im, ink, op);
    let a = x1 - x0;
    let b = y1 - y0;
    if a < 0 || b < 0 {
        return;
    }
    let mut st = init(a, b, width, start, end);
    while let Some((xl, y, xr)) = st.next() {
        d.hline(im, x0 + (xl + a) / 2, y0 + (y + b) / 2, x0 + (xr + a) / 2);
    }
}

#[allow(clippy::too_many_arguments)]
pub fn arc(im: &mut Image, x0: i32, y0: i32, x1: i32, y1: i32, mut start: f32, mut end: f32, ink: i32, width: i32, op: bool) {
    normalize_angles(&mut start, &mut end);
    if start + 360.0 == end {
        return ellipse(im, x0, y0, x1, y1, ink, false, width, op);
    }
    if start == end {
        return;
    }
    clip_ellipse_new(im, x0, y0, x1, y1, start, end, ink, width, op, arc_init);
}

#[allow(clippy::too_many_arguments)]
pub fn chord(im: &mut Image, x0: i32, y0: i32, x1: i32, y1: i32, mut start: f32, mut end: f32, ink: i32, fill: bool, width: i32, op: bool) {
    normalize_angles(&mut start, &mut end);
    if start + 360.0 == end {
        return ellipse(im, x0, y0, x1, y1, ink, fill, width, op);
    }
    if start == end {
        return;
    }
    if fill {
        clip_ellipse_new(im, x0, y0, x1, y1, start, end, ink, x1 - x0 + y1 - y0 + 1, op, chord_init);
    } else {
        clip_ellipse_new(im, x0, y0, x1, y1, start, end, ink, width, op, chord_line_init);
        clip_ellipse_new(im, x0, y0, x1, y1, start, end, ink, width, op, chord_init);
    }
}

#[allow(clippy::too_many_arguments)]
pub fn pieslice(im: &mut Image, x0: i32, y0: i32, x1: i32, y1: i32, mut start: f32, mut end: f32, ink: i32, fill: bool, width: i32, op: bool) {
    normalize_angles(&mut start, &mut end);
    if start + 360.0 == end {
        return ellipse(im, x0, y0, x1, y1, ink, fill, width, op);
    }
    if start == end {
        return;
    }
    if fill {
        clip_ellipse_new(im, x0, y0, x1, y1, start, end, ink, x1 + y1 - x0 - y0, op, pie_init);
    } else {
        clip_ellipse_new(im, x0, y0, x1, y1, start, 0.0, ink, width, op, pie_side_init);
        clip_ellipse_new(im, x0, y0, x1, y1, end, 0.0, ink, width, op, pie_side_init);
        let xc = (f64::from(x0 + x1 - width) / 2.0).round() as i32;
        let yc = (f64::from(y0 + y1 - width) / 2.0).round() as i32;
        ellipse(im, xc, yc, xc + width - 1, yc + width - 1, ink, true, 0, op);
        clip_ellipse_new(im, x0, y0, x1, y1, start, end, ink, width, op, pie_init);
    }
}
