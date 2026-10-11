//! `afhints.c`: pontos, segmentos e arestas de um glifo, e o alinhamento final dos pontos.
//! As ligações entre pontos, segmentos e arestas são índices nos vetores do glifo.

use crate::calc::{div_fix, msb, mul_fix};
use crate::outline::{Outline, TAG_CUBIC, TAG_ON};

pub(crate) const DIR_NONE: i8 = 4;
pub(crate) const DIR_RIGHT: i8 = 1;
pub(crate) const DIR_LEFT: i8 = -1;
pub(crate) const DIR_UP: i8 = 2;
pub(crate) const DIR_DOWN: i8 = -2;

pub(crate) const FLAG_CONIC: u32 = 1 << 0;
pub(crate) const FLAG_CUBIC: u32 = 1 << 1;
pub(crate) const FLAG_CONTROL: u32 = FLAG_CONIC | FLAG_CUBIC;
pub(crate) const FLAG_TOUCH_X: u32 = 1 << 2;
pub(crate) const FLAG_TOUCH_Y: u32 = 1 << 3;
pub(crate) const FLAG_WEAK_INTERPOLATION: u32 = 1 << 4;
pub(crate) const FLAG_NEAR: u32 = 1 << 5;

pub(crate) const EDGE_NORMAL: u32 = 0;
pub(crate) const EDGE_ROUND: u32 = 1 << 0;
pub(crate) const EDGE_SERIF: u32 = 1 << 1;
pub(crate) const EDGE_DONE: u32 = 1 << 2;
pub(crate) const EDGE_NEUTRAL: u32 = 1 << 3;

pub(crate) const SCALER_FLAG_NO_HORIZONTAL: u32 = 1;
pub(crate) const SCALER_FLAG_NO_VERTICAL: u32 = 2;
pub(crate) const SCALER_FLAG_NO_ADVANCE: u32 = 4;

pub(crate) const DIMENSION_HORZ: usize = 0;
pub(crate) const DIMENSION_VERT: usize = 1;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Point {
    pub flags: u32,
    pub in_dir: i8,
    pub out_dir: i8,
    pub ox: i64,
    pub oy: i64,
    pub fx: i64,
    pub fy: i64,
    pub x: i64,
    pub y: i64,
    pub u: i64,
    pub v: i64,
    pub next: usize,
    pub prev: usize,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Segment {
    pub flags: u32,
    pub dir: i8,
    pub pos: i64,
    pub delta: i64,
    pub min_coord: i64,
    pub max_coord: i64,
    pub height: i64,
    pub score: i64,
    pub len: i64,
    pub link: Option<usize>,
    pub serif: Option<usize>,
    pub edge_next: usize,
    pub edge: Option<usize>,
    pub first: usize,
    pub last: usize,
}

/// Uma zona azul referida por uma aresta: índice no eixo vertical e se é a `shoot`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BlueRef {
    pub blue: usize,
    pub shoot: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Edge {
    pub fpos: i64,
    pub opos: i64,
    pub pos: i64,
    pub flags: u32,
    pub dir: i8,
    pub scale: i64,
    pub blue_edge: Option<BlueRef>,
    pub link: Option<usize>,
    pub serif: Option<usize>,
    pub first: usize,
    pub last: usize,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct AxisHints {
    pub segments: Vec<Segment>,
    pub edges: Vec<Edge>,
    pub major_dir: i8,
}

impl AxisHints {
    /// `af_axis_hints_new_edge`: insere ordenado por `fpos` e devolve o índice.
    pub(crate) fn new_edge(&mut self, fpos: i64, dir: i8, top_to_bottom: bool) -> usize {
        let mut i = self.edges.len();
        while i > 0 {
            let f = self.edges[i - 1].fpos;
            if if top_to_bottom { f > fpos } else { f < fpos } {
                break;
            }
            if f == fpos && dir == self.major_dir {
                break;
            }
            i -= 1;
        }
        self.edges.insert(i, Edge::default());
        i
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct GlyphHints {
    pub points: Vec<Point>,
    pub contours: Vec<usize>,
    pub axis: [AxisHints; 2],
    pub x_scale: i64,
    pub y_scale: i64,
    pub x_delta: i64,
    pub y_delta: i64,
    pub scaler_flags: u32,
    pub other_flags: u32,
}

/// `af_sort_pos`.
pub(crate) fn sort_pos(table: &mut [i64]) {
    for i in 1..table.len() {
        let mut j = i;
        while j > 0 && table[j] < table[j - 1] {
            table.swap(j, j - 1);
            j -= 1;
        }
    }
}

/// `af_sort_and_quantize_widths` sobre os valores `org`; devolve a nova contagem.
pub(crate) fn sort_and_quantize_widths(table: &mut [i64], threshold: i64) -> usize {
    let count = table.len();
    if count == 1 {
        return 1;
    }
    sort_pos(table);
    let mut cur_idx = 0;
    let mut cur_val = table[cur_idx];
    let mut i = 1;
    while i < count {
        if table[i] - cur_val > threshold || i == count - 1 {
            let mut sum = 0;
            if table[i] - cur_val <= threshold && i == count - 1 {
                i += 1;
            }
            let mut j = cur_idx;
            while j < i {
                sum += table[j];
                table[j] = 0;
                j += 1;
            }
            table[cur_idx] = sum / j as i64;
            if i + 1 < count {
                cur_idx = i + 1;
                cur_val = table[cur_idx];
            }
        }
        i += 1;
    }
    let mut cur = 1;
    for i in 1..count {
        if table[i] != 0 {
            table[cur] = table[i];
            cur += 1;
        }
    }
    cur
}

/// `af_direction_compute`.
pub(crate) fn direction_compute(dx: i64, dy: i64) -> i8 {
    let (dir, ll, ss) = if dy >= dx {
        if dy >= -dx { (DIR_UP, dy, dx) } else { (DIR_LEFT, -dx, dy) }
    } else if dy >= -dx {
        (DIR_RIGHT, dx, dy)
    } else {
        (DIR_DOWN, -dy, dx)
    };
    if ll <= 14 * ss.abs() { DIR_NONE } else { dir }
}

/// `FT_HYPOT`: aproximação alfa-max-beta-min.
fn ft_hypot(x: i64, y: i64) -> i64 {
    let (x, y) = (x.abs(), y.abs());
    if x > y { x + ((3 * y) >> 3) } else { y + ((3 * x) >> 3) }
}

/// `ft_corner_is_flat`.
fn corner_is_flat(in_x: i64, in_y: i64, out_x: i64, out_y: i64) -> bool {
    let d_in = ft_hypot(in_x, in_y);
    let d_out = ft_hypot(out_x, out_y);
    let d_hypot = ft_hypot(in_x + out_x, in_y + out_y);
    (d_in + d_out - d_hypot) < (d_hypot >> 4)
}

/// `FT_Outline_Get_Orientation`: verdadeiro quando a orientação é a PostScript.
fn is_postscript(outline: &Outline) -> bool {
    if outline.points.is_empty() {
        return false;
    }
    let b = outline.cbox();
    if b.x_min == b.x_max || b.y_min == b.y_max {
        return false;
    }
    if b.x_min < -0x1000000 || b.y_min < -0x1000000 || b.x_max > 0x1000000 || b.y_max > 0x1000000 {
        return false;
    }
    let xshift = (msb((b.x_max.abs() | b.x_min.abs()) as u32) - 14).max(0);
    let yshift = (msb((b.y_max - b.y_min) as u32) - 14).max(0);
    let p = &outline.points;
    let mut area: i64 = 0;
    let mut first = 0;
    for &last in &outline.contours {
        let (mut px, mut py) = (p[last].x >> xshift, p[last].y >> yshift);
        for q in &p[first..=last] {
            let (cx, cy) = (q.x >> xshift, q.y >> yshift);
            area = area.wrapping_add((cy - py).wrapping_mul(cx + px));
            px = cx;
            py = cy;
        }
        first = last + 1;
    }
    area > 0
}

impl GlyphHints {
    /// `af_glyph_hints_reload`: monta os pontos a partir do contorno em unidades da fonte.
    pub(crate) fn reload(&mut self, outline: &Outline, units_per_em: i64) {
        let (x_scale, y_scale, x_delta, y_delta) = (self.x_scale, self.y_scale, self.x_delta, self.y_delta);
        self.axis[0].segments.clear();
        self.axis[0].edges.clear();
        self.axis[1].segments.clear();
        self.axis[1].edges.clear();
        let n = outline.points.len();
        self.points = vec![Point::default(); n];
        self.contours.clear();
        self.axis[DIMENSION_HORZ].major_dir = DIR_UP;
        self.axis[DIMENSION_VERT].major_dir = DIR_LEFT;
        if is_postscript(outline) {
            self.axis[DIMENSION_HORZ].major_dir = DIR_DOWN;
            self.axis[DIMENSION_VERT].major_dir = DIR_RIGHT;
        }
        if n == 0 {
            return;
        }
        let near_limit = 20 * units_per_em / 2048;
        let pts = &mut self.points;
        {
            let mut endpoint = outline.contours[0];
            let mut end = endpoint;
            let mut prev = end;
            let mut contour_index = 0;
            for i in 0..n {
                let vec = outline.points[i];
                let p = &mut pts[i];
                p.in_dir = DIR_NONE;
                p.out_dir = DIR_NONE;
                p.fx = i64::from(vec.x as i16);
                p.fy = i64::from(vec.y as i16);
                p.ox = mul_fix(vec.x, x_scale) + x_delta;
                p.x = p.ox;
                p.oy = mul_fix(vec.y, y_scale) + y_delta;
                p.y = p.oy;
                pts[end].fx = i64::from(outline.points[endpoint].x as i16);
                pts[end].fy = i64::from(outline.points[endpoint].y as i16);
                pts[i].flags = match outline.tags[i] & 3 {
                    0 => FLAG_CONIC,
                    TAG_CUBIC => FLAG_CUBIC,
                    _ => 0,
                };
                let out_x = pts[i].fx - pts[prev].fx;
                let out_y = pts[i].fy - pts[prev].fy;
                if out_x.abs() + out_y.abs() < near_limit {
                    pts[prev].flags |= FLAG_NEAR;
                }
                pts[i].prev = prev;
                pts[prev].next = i;
                prev = i;
                if i == end {
                    contour_index += 1;
                    if contour_index < outline.contours.len() {
                        endpoint = outline.contours[contour_index];
                        end = endpoint;
                        prev = end;
                    }
                }
            }
        }
        let mut idx = 0;
        for &e in &outline.contours {
            self.contours.push(idx);
            idx = e + 1;
        }
        let near_limit2 = 2 * near_limit - 1;
        for &c in &self.contours {
            let first0 = c;
            let mut point = first0;
            let mut prev = pts[first0].prev;
            while prev != first0 {
                let out_x = pts[point].fx - pts[prev].fx;
                let out_y = pts[point].fy - pts[prev].fy;
                if out_x.abs() + out_y.abs() >= near_limit2 {
                    break;
                }
                point = prev;
                prev = pts[prev].prev;
            }
            let first = point;
            let mut curr = first;
            pts[curr].u = first as i64 - curr as i64;
            pts[first].v = -pts[curr].u;
            let (mut out_x, mut out_y) = (0i64, 0i64);
            let mut next = first;
            loop {
                let point = next;
                next = pts[point].next;
                out_x += pts[next].fx - pts[point].fx;
                out_y += pts[next].fy - pts[point].fy;
                if out_x.abs() + out_y.abs() < near_limit {
                    pts[next].flags |= FLAG_WEAK_INTERPOLATION;
                    if next == first {
                        break;
                    }
                    continue;
                }
                pts[curr].u = next as i64 - curr as i64;
                pts[next].v = -pts[curr].u;
                let out_dir = direction_compute(out_x, out_y);
                pts[curr].out_dir = out_dir;
                curr = pts[curr].next;
                while curr != next {
                    pts[curr].in_dir = out_dir;
                    pts[curr].out_dir = out_dir;
                    curr = pts[curr].next;
                }
                pts[next].in_dir = out_dir;
                pts[curr].u = first as i64 - curr as i64;
                pts[first].v = -pts[curr].u;
                out_x = 0;
                out_y = 0;
                if next == first {
                    break;
                }
            }
        }
        let at = |i: usize, off: i64| (i as i64 + off) as usize;
        for i in 0..n {
            if pts[i].flags & FLAG_WEAK_INTERPOLATION != 0 {
                continue;
            }
            if pts[i].in_dir == DIR_NONE && pts[i].out_dir == DIR_NONE {
                let next_u = at(i, pts[i].u);
                let prev_v = at(i, pts[i].v);
                let in_x = pts[i].fx - pts[prev_v].fx;
                let in_y = pts[i].fy - pts[prev_v].fy;
                let out_x = pts[next_u].fx - pts[i].fx;
                let out_y = pts[next_u].fy - pts[i].fy;
                if (in_x ^ out_x) >= 0 && (in_y ^ out_y) >= 0 {
                    pts[i].flags |= FLAG_WEAK_INTERPOLATION;
                    pts[prev_v].u = next_u as i64 - prev_v as i64;
                    pts[next_u].v = -pts[prev_v].u;
                }
            }
        }
        for i in 0..n {
            let p = pts[i];
            if p.flags & FLAG_WEAK_INTERPOLATION != 0 {
                continue;
            }
            let weak = if p.flags & FLAG_CONTROL != 0 {
                true
            } else if p.out_dir == p.in_dir {
                if p.out_dir != DIR_NONE {
                    true
                } else {
                    let next_u = at(i, p.u);
                    let prev_v = at(i, p.v);
                    if corner_is_flat(
                        p.fx - pts[prev_v].fx,
                        p.fy - pts[prev_v].fy,
                        pts[next_u].fx - p.fx,
                        pts[next_u].fy - p.fy,
                    ) {
                        pts[prev_v].u = next_u as i64 - prev_v as i64;
                        pts[next_u].v = -pts[prev_v].u;
                        true
                    } else {
                        false
                    }
                }
            } else {
                p.in_dir == -p.out_dir
            };
            if weak {
                pts[i].flags |= FLAG_WEAK_INTERPOLATION;
            }
        }
    }

    /// `af_glyph_hints_save`.
    pub(crate) fn save(&self, outline: &mut Outline) {
        for (i, p) in self.points.iter().enumerate() {
            outline.points[i].x = p.x;
            outline.points[i].y = p.y;
            outline.tags[i] = if p.flags & FLAG_CONIC != 0 {
                0
            } else if p.flags & FLAG_CUBIC != 0 {
                TAG_CUBIC
            } else {
                TAG_ON
            };
        }
    }

    /// `af_glyph_hints_align_edge_points`.
    pub(crate) fn align_edge_points(&mut self, dim: usize) {
        let axis = &self.axis[dim];
        for seg in &axis.segments {
            let Some(e) = seg.edge else { continue };
            let pos = axis.edges[e].pos;
            let mut point = seg.first;
            loop {
                let p = &mut self.points[point];
                if dim == DIMENSION_HORZ {
                    p.x = pos;
                    p.flags |= FLAG_TOUCH_X;
                } else {
                    p.y = pos;
                    p.flags |= FLAG_TOUCH_Y;
                }
                if point == seg.last {
                    break;
                }
                point = p.next;
            }
        }
    }

    /// `af_glyph_hints_align_strong_points`.
    pub(crate) fn align_strong_points(&mut self, dim: usize) {
        let touch_flag = if dim == DIMENSION_HORZ { FLAG_TOUCH_X } else { FLAG_TOUCH_Y };
        let edges = &mut self.axis[dim].edges;
        if edges.is_empty() {
            return;
        }
        let ne = edges.len();
        for point in self.points.iter_mut() {
            if point.flags & touch_flag != 0 || point.flags & FLAG_WEAK_INTERPOLATION != 0 {
                continue;
            }
            let (mut u, ou) = if dim == DIMENSION_VERT { (point.fy, point.oy) } else { (point.fx, point.ox) };
            let fu = u;
            'store: {
                let edge = &edges[0];
                if edge.fpos - u >= 0 {
                    u = edge.pos - (edge.opos - ou);
                    break 'store;
                }
                let edge = &edges[ne - 1];
                if u - edge.fpos >= 0 {
                    u = edge.pos + (ou - edge.opos);
                    break 'store;
                }
                let mut min = 0usize;
                let mut max = ne;
                if max <= 8 {
                    let mut nn = 0;
                    while nn < max {
                        if edges[nn].fpos >= u {
                            break;
                        }
                        nn += 1;
                    }
                    if edges[nn].fpos == u {
                        u = edges[nn].pos;
                        break 'store;
                    }
                    min = nn;
                } else {
                    while min < max {
                        let mid = (max + min) >> 1;
                        let fpos = edges[mid].fpos;
                        if u < fpos {
                            max = mid;
                        } else if u > fpos {
                            min = mid + 1;
                        } else {
                            u = edges[mid].pos;
                            break 'store;
                        }
                    }
                }
                let (before, after) = (min - 1, min);
                if edges[before].scale == 0 {
                    edges[before].scale =
                        div_fix(edges[after].pos - edges[before].pos, edges[after].fpos - edges[before].fpos);
                }
                u = edges[before].pos + mul_fix(fu - edges[before].fpos, edges[before].scale);
            }
            if dim == DIMENSION_HORZ {
                point.x = u;
            } else {
                point.y = u;
            }
            point.flags |= touch_flag;
        }
    }

    /// `af_glyph_hints_align_weak_points`.
    pub(crate) fn align_weak_points(&mut self, dim: usize) {
        let touch_flag = if dim == DIMENSION_HORZ { FLAG_TOUCH_X } else { FLAG_TOUCH_Y };
        let pts = &mut self.points;
        for p in pts.iter_mut() {
            if dim == DIMENSION_HORZ {
                p.u = p.x;
                p.v = p.ox;
            } else {
                p.u = p.y;
                p.v = p.oy;
            }
        }
        for &c in &self.contours {
            let mut point = c;
            let end_point = pts[point].prev;
            let first_point = point;
            loop {
                if point > end_point {
                    break;
                }
                if pts[point].flags & touch_flag != 0 {
                    break;
                }
                point += 1;
            }
            if point > end_point {
                continue;
            }
            let first_touched = point;
            let mut last_touched;
            loop {
                while point < end_point && pts[point + 1].flags & touch_flag != 0 {
                    point += 1;
                }
                last_touched = point;
                point += 1;
                let mut end = false;
                loop {
                    if point > end_point {
                        end = true;
                        break;
                    }
                    if pts[point].flags & touch_flag != 0 {
                        break;
                    }
                    point += 1;
                }
                if end {
                    break;
                }
                iup_interp(pts, last_touched + 1, point - 1, last_touched, point);
            }
            if last_touched == first_touched {
                iup_shift(pts, first_point, end_point, first_touched);
            } else {
                if last_touched < end_point {
                    iup_interp(pts, last_touched + 1, end_point, last_touched, first_touched);
                }
                if first_touched > 0 && first_touched > first_point {
                    iup_interp(pts, first_point, first_touched - 1, last_touched, first_touched);
                }
            }
        }
        for p in pts.iter_mut() {
            if dim == DIMENSION_HORZ {
                p.x = p.u;
            } else {
                p.y = p.u;
            }
        }
    }
}

fn iup_shift(pts: &mut [Point], p1: usize, p2: usize, r: usize) {
    let delta = pts[r].u - pts[r].v;
    if delta == 0 {
        return;
    }
    for p in p1..r {
        pts[p].u = pts[p].v + delta;
    }
    for p in r + 1..=p2 {
        pts[p].u = pts[p].v + delta;
    }
}

fn iup_interp(pts: &mut [Point], p1: usize, p2: usize, mut ref1: usize, mut ref2: usize) {
    if p1 > p2 {
        return;
    }
    if pts[ref1].v > pts[ref2].v {
        std::mem::swap(&mut ref1, &mut ref2);
    }
    let (v1, v2, u1, u2) = (pts[ref1].v, pts[ref2].v, pts[ref1].u, pts[ref2].u);
    let (d1, d2) = (u1 - v1, u2 - v2);
    if u1 == u2 || v1 == v2 {
        for p in &mut pts[p1..=p2] {
            let mut u = p.v;
            if u <= v1 {
                u += d1;
            } else if u >= v2 {
                u += d2;
            } else {
                u = u1;
            }
            p.u = u;
        }
    } else {
        let scale = div_fix(u2 - u1, v2 - v1);
        for p in &mut pts[p1..=p2] {
            let mut u = p.v;
            if u <= v1 {
                u += d1;
            } else if u >= v2 {
                u += d2;
            } else {
                u = u1 + mul_fix(u - v1, scale);
            }
            p.u = u;
        }
    }
}
