//! O stroker do FreeType 2.13.3 (`ftstroke.c`), com o `FT_Glyph_Stroke` que o Pillow usa para o
//! `stroke_width`: cada contorno vira as duas bordas deslocadas pelo raio, com junções e pontas
//! arredondadas.

use crate::calc::trig::{angle_diff, atan2, cos, from_polar, length, sin, tan, unit, ANGLE_PI, ANGLE_PI2};
use crate::calc::{div_fix, mul_div, mul_fix};
use crate::outline::{Outline, Vector};
use crate::Error;

const SMALL_CONIC_THRESHOLD: i64 = ANGLE_PI / 6;
const SMALL_CUBIC_THRESHOLD: i64 = ANGLE_PI / 8;
const ARC_CUBIC_ANGLE: i64 = ANGLE_PI / 2;

const TAG_ON: u8 = 1;
const TAG_CUBIC: u8 = 2;
const TAG_BEGIN: u8 = 4;
const TAG_END: u8 = 8;
const TAG_BEGIN_END: u8 = TAG_BEGIN | TAG_END;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineCap {
    Butt,
    Round,
    Square,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineJoin {
    Round,
    Bevel,
    MiterVariable,
    MiterFixed,
}

fn is_small(x: i64) -> bool {
    x > -2 && x < 2
}

fn v(x: i64, y: i64) -> Vector {
    Vector { x, y }
}

fn polar(len: i64, angle: i64) -> Vector {
    let (x, y) = from_polar(len, angle);
    v(x, y)
}

fn side_to_rotate(s: usize) -> i64 {
    ANGLE_PI2 - s as i64 * ANGLE_PI
}

fn angle_mean(a1: i64, a2: i64) -> i64 {
    a1 + angle_diff(a1, a2) / 2
}

fn conic_split(b: &mut [Vector]) {
    b[4].x = b[2].x;
    let a = b[0].x + b[1].x;
    let c = b[1].x + b[2].x;
    b[3].x = c >> 1;
    b[2].x = (a + c) >> 2;
    b[1].x = a >> 1;
    b[4].y = b[2].y;
    let a = b[0].y + b[1].y;
    let c = b[1].y + b[2].y;
    b[3].y = c >> 1;
    b[2].y = (a + c) >> 2;
    b[1].y = a >> 1;
}

fn cubic_split(b: &mut [Vector]) {
    b[6].x = b[3].x;
    let mut a = b[0].x + b[1].x;
    let bb = b[1].x + b[2].x;
    let mut c = b[2].x + b[3].x;
    b[5].x = c >> 1;
    c += bb;
    b[4].x = c >> 2;
    b[1].x = a >> 1;
    a += bb;
    b[2].x = a >> 2;
    b[3].x = (a + c) >> 3;
    b[6].y = b[3].y;
    let mut a = b[0].y + b[1].y;
    let bb = b[1].y + b[2].y;
    let mut c = b[2].y + b[3].y;
    b[5].y = c >> 1;
    c += bb;
    b[4].y = c >> 2;
    b[1].y = a >> 1;
    a += bb;
    b[2].y = a >> 2;
    b[3].y = (a + c) >> 3;
}

fn close(d: Vector) -> bool {
    is_small(d.x) && is_small(d.y)
}

fn diff(a: Vector, b: Vector) -> Vector {
    v(a.x - b.x, a.y - b.y)
}

/// `ft_conic_is_small_enough`; `angle_in`/`angle_out` só mudam quando há direção.
fn conic_is_small_enough(b: &[Vector], angle_in: &mut i64, angle_out: &mut i64) -> bool {
    let d1 = diff(b[1], b[2]);
    let d2 = diff(b[0], b[1]);
    match (close(d1), close(d2)) {
        (true, true) => {}
        (true, false) => {
            *angle_in = atan2(d2.x, d2.y);
            *angle_out = *angle_in;
        }
        (false, true) => {
            *angle_in = atan2(d1.x, d1.y);
            *angle_out = *angle_in;
        }
        (false, false) => {
            *angle_in = atan2(d1.x, d1.y);
            *angle_out = atan2(d2.x, d2.y);
        }
    }
    angle_diff(*angle_in, *angle_out).abs() < SMALL_CONIC_THRESHOLD
}

/// `ft_cubic_is_small_enough`.
fn cubic_is_small_enough(b: &[Vector], ai: &mut i64, am: &mut i64, ao: &mut i64) -> bool {
    let d1 = diff(b[2], b[3]);
    let d2 = diff(b[1], b[2]);
    let d3 = diff(b[0], b[1]);
    let at = |d: Vector| atan2(d.x, d.y);
    match (close(d1), close(d2), close(d3)) {
        (true, true, true) => {}
        (true, true, false) => {
            *ai = at(d3);
            *am = *ai;
            *ao = *ai;
        }
        (true, false, true) => {
            *ai = at(d2);
            *am = *ai;
            *ao = *ai;
        }
        (true, false, false) => {
            *ai = at(d2);
            *am = *ai;
            *ao = at(d3);
        }
        (false, true, true) => {
            *ai = at(d1);
            *am = *ai;
            *ao = *ai;
        }
        (false, true, false) => {
            *ai = at(d1);
            *ao = at(d3);
            *am = angle_mean(*ai, *ao);
        }
        (false, false, true) => {
            *ai = at(d1);
            *am = at(d2);
            *ao = *am;
        }
        (false, false, false) => {
            *ai = at(d1);
            *am = at(d2);
            *ao = at(d3);
        }
    }
    let t1 = angle_diff(*ai, *am).abs();
    let t2 = angle_diff(*am, *ao).abs();
    t1 < SMALL_CUBIC_THRESHOLD && t2 < SMALL_CUBIC_THRESHOLD
}

#[derive(Clone, Default)]
struct Border {
    points: Vec<Vector>,
    tags: Vec<u8>,
    movable: bool,
    start: i64,
    valid: bool,
}

impl Border {
    fn reset(&mut self) {
        self.points.clear();
        self.tags.clear();
        self.start = -1;
        self.valid = false;
    }

    fn close(&mut self, reverse: bool) {
        let start = self.start as usize;
        let count = self.points.len();
        if count <= start + 1 {
            self.points.truncate(start);
            self.tags.truncate(start);
        } else {
            let count = count - 1;
            self.points[start] = self.points[count];
            self.tags[start] = self.tags[count];
            self.points.truncate(count);
            self.tags.truncate(count);
            if reverse && count > start + 1 {
                self.points[start + 1..count].reverse();
                self.tags[start + 1..count].reverse();
            }
            self.tags[start] |= TAG_BEGIN;
            self.tags[count - 1] |= TAG_END;
        }
        self.start = -1;
        self.movable = false;
    }

    fn line_to(&mut self, to: Vector, movable: bool) {
        if self.movable {
            let n = self.points.len();
            self.points[n - 1] = to;
        } else {
            if self.points.len() as i64 > self.start {
                if let Some(&last) = self.points.last() {
                    if is_small(last.x - to.x) && is_small(last.y - to.y) {
                        return;
                    }
                }
            }
            self.points.push(to);
            self.tags.push(TAG_ON);
        }
        self.movable = movable;
    }

    fn conic_to(&mut self, control: Vector, to: Vector) {
        self.points.extend([control, to]);
        self.tags.extend([0, TAG_ON]);
        self.movable = false;
    }

    fn cubic_to(&mut self, c1: Vector, c2: Vector, to: Vector) {
        self.points.extend([c1, c2, to]);
        self.tags.extend([TAG_CUBIC, TAG_CUBIC, TAG_ON]);
        self.movable = false;
    }

    fn arc_to(&mut self, center: Vector, radius: i64, angle_start: i64, angle_diff: i64) {
        let mut arcs = 1i64;
        while angle_diff > ARC_CUBIC_ANGLE * arcs || -angle_diff > ARC_CUBIC_ANGLE * arcs {
            arcs += 1;
        }
        let mut coef = tan(angle_diff / (4 * arcs));
        coef += coef / 3;
        let mut a0 = polar(radius, angle_start);
        let mut a1 = v(mul_fix(-a0.y, coef), mul_fix(a0.x, coef));
        a0.x += center.x;
        a0.y += center.y;
        a1.x += a0.x;
        a1.y += a0.y;
        for i in 1..=arcs {
            let mut a3 = polar(radius, angle_start + i * angle_diff / arcs);
            let mut a2 = v(mul_fix(a3.y, coef), mul_fix(-a3.x, coef));
            a3.x += center.x;
            a3.y += center.y;
            a2.x += a3.x;
            a2.y += a3.y;
            self.cubic_to(a1, a2, a3);
            a1 = v(a3.x - a2.x + a3.x, a3.y - a2.y + a3.y);
        }
    }

    fn move_to(&mut self, to: Vector) {
        if self.start >= 0 {
            self.close(false);
        }
        self.start = self.points.len() as i64;
        self.movable = false;
        self.line_to(to, false);
    }

    /// `ft_stroke_border_get_counts`.
    fn counts(&mut self) -> (usize, usize) {
        let mut contours = 0;
        let mut in_contour = false;
        for &t in &self.tags {
            if t & TAG_BEGIN != 0 {
                if in_contour {
                    return (0, 0);
                }
                in_contour = true;
            } else if !in_contour {
                return (0, 0);
            }
            if t & TAG_END != 0 {
                in_contour = false;
                contours += 1;
            }
        }
        if in_contour {
            return (0, 0);
        }
        self.valid = true;
        (self.points.len(), contours)
    }

    fn export(&self, o: &mut Outline) {
        let base = o.points.len();
        o.points.extend_from_slice(&self.points);
        for (i, &t) in self.tags.iter().enumerate() {
            o.tags.push(if t & TAG_ON != 0 {
                1
            } else if t & TAG_CUBIC != 0 {
                2
            } else {
                0
            });
            if t & TAG_END != 0 {
                o.contours.push(base + i);
            }
        }
    }
}

/// `FT_StrokerRec`.
pub struct Stroker {
    angle_in: i64,
    angle_out: i64,
    center: Vector,
    line_length: i64,
    first_point: bool,
    subpath_open: bool,
    subpath_angle: i64,
    subpath_start: Vector,
    subpath_line_length: i64,
    handle_wide_strokes: bool,
    line_cap: LineCap,
    line_join: LineJoin,
    line_join_saved: LineJoin,
    miter_limit: i64,
    radius: i64,
    borders: [Border; 2],
}

impl Stroker {
    /// `FT_Stroker_New` + `FT_Stroker_Set`.
    pub fn new(radius: i64, line_cap: LineCap, line_join: LineJoin, miter_limit: i64) -> Stroker {
        let mut s = Stroker {
            angle_in: 0,
            angle_out: 0,
            center: Vector::default(),
            line_length: 0,
            first_point: false,
            subpath_open: false,
            subpath_angle: 0,
            subpath_start: Vector::default(),
            subpath_line_length: 0,
            handle_wide_strokes: false,
            line_cap,
            line_join,
            line_join_saved: line_join,
            miter_limit: miter_limit.max(0x10000),
            radius,
            borders: [Border { start: -1, ..Border::default() }, Border { start: -1, ..Border::default() }],
        };
        s.rewind();
        s
    }

    fn rewind(&mut self) {
        self.borders[0].reset();
        self.borders[1].reset();
    }

    fn arcto(&mut self, side: usize) {
        let rotate = side_to_rotate(side);
        let mut total = angle_diff(self.angle_in, self.angle_out);
        if total == ANGLE_PI {
            total = -rotate * 2;
        }
        let (c, r, a) = (self.center, self.radius, self.angle_in + rotate);
        let b = &mut self.borders[side];
        b.arc_to(c, r, a, total);
        b.movable = false;
    }

    fn cap(&mut self, angle: i64, side: usize) {
        if self.line_cap == LineCap::Round {
            self.angle_in = angle;
            self.angle_out = angle + ANGLE_PI;
            self.arcto(side);
        } else {
            let mut middle = polar(self.radius, angle);
            let mut delta = if side != 0 { v(middle.y, -middle.x) } else { v(-middle.y, middle.x) };
            if self.line_cap == LineCap::Square {
                middle.x += self.center.x;
                middle.y += self.center.y;
            } else {
                middle = self.center;
            }
            delta.x += middle.x;
            delta.y += middle.y;
            let b = &mut self.borders[side];
            b.line_to(delta, false);
            delta = v(middle.x - delta.x + middle.x, middle.y - delta.y + middle.y);
            b.line_to(delta, false);
        }
    }

    fn inside(&mut self, side: usize, line_length: i64) {
        let rotate = side_to_rotate(side);
        let theta = angle_diff(self.angle_in, self.angle_out) / 2;
        let mut sigma = (0, 0);
        let intersect = if !self.borders[side].movable || line_length == 0 || !(-0x59C000..=0x59C000).contains(&theta) {
            false
        } else {
            sigma = unit(theta);
            let min_length = mul_div(self.radius, sigma.1, sigma.0).abs();
            min_length != 0 && self.line_length >= min_length && line_length >= min_length
        };
        let delta;
        if !intersect {
            let d = polar(self.radius, self.angle_out + rotate);
            delta = v(d.x + self.center.x, d.y + self.center.y);
            self.borders[side].movable = false;
        } else {
            let phi = self.angle_in + theta + rotate;
            let len = div_fix(self.radius, sigma.0);
            let d = polar(len, phi);
            delta = v(d.x + self.center.x, d.y + self.center.y);
        }
        self.borders[side].line_to(delta, false);
    }

    fn outside(&mut self, side: usize, line_length: i64) {
        if self.line_join == LineJoin::Round {
            self.arcto(side);
            return;
        }
        let radius = self.radius;
        let rotate = side_to_rotate(side);
        let mut bevel = self.line_join == LineJoin::Bevel;
        let fixed_bevel = self.line_join != LineJoin::MiterVariable;
        let (mut theta, mut phi, mut sigma) = (0, 0, (0, 0));
        if !bevel {
            theta = angle_diff(self.angle_in, self.angle_out) / 2;
            if theta == ANGLE_PI2 {
                theta = -rotate;
            }
            phi = self.angle_in + theta + rotate;
            sigma = from_polar(self.miter_limit, theta);
            if sigma.0 < 0x10000 && (fixed_bevel || theta.abs() > 57) {
                bevel = true;
            }
        }
        let center = self.center;
        let (angle_out, miter_limit) = (self.angle_out, self.miter_limit);
        let b = &mut self.borders[side];
        let at_out = |r: i64| {
            let d = polar(r, angle_out + rotate);
            v(d.x + center.x, d.y + center.y)
        };
        if bevel {
            if fixed_bevel {
                b.movable = false;
                b.line_to(at_out(radius), false);
            } else {
                let mut middle = polar(mul_fix(radius, miter_limit), phi);
                let coef = div_fix(0x10000 - sigma.0, sigma.1);
                let mut delta = v(mul_fix(middle.y, coef), mul_fix(-middle.x, coef));
                middle.x += center.x;
                middle.y += center.y;
                delta.x += middle.x;
                delta.y += middle.y;
                b.line_to(delta, false);
                delta = v(middle.x - delta.x + middle.x, middle.y - delta.y + middle.y);
                b.line_to(delta, false);
                if line_length == 0 {
                    b.line_to(at_out(radius), false);
                }
            }
        } else {
            let len = mul_div(radius, miter_limit, sigma.0);
            let d = polar(len, phi);
            b.line_to(v(d.x + center.x, d.y + center.y), false);
            if line_length == 0 {
                b.line_to(at_out(radius), false);
            }
        }
    }

    fn process_corner(&mut self, line_length: i64) {
        let turn = angle_diff(self.angle_in, self.angle_out);
        if turn == 0 {
            return;
        }
        let inside_side = usize::from(turn < 0);
        self.inside(inside_side, line_length);
        self.outside(1 - inside_side, line_length);
    }

    fn subpath_start(&mut self, start_angle: i64, line_length: i64) {
        let d = polar(self.radius, start_angle + ANGLE_PI2);
        let c = self.center;
        self.borders[0].move_to(v(c.x + d.x, c.y + d.y));
        self.borders[1].move_to(v(c.x - d.x, c.y - d.y));
        self.subpath_angle = start_angle;
        self.first_point = false;
        self.subpath_line_length = line_length;
    }

    /// `FT_Stroker_LineTo`.
    pub fn line_to(&mut self, to: Vector) {
        let delta = diff(to, self.center);
        if delta.x == 0 && delta.y == 0 {
            return;
        }
        let line_length = length(delta.x, delta.y);
        let angle = atan2(delta.x, delta.y);
        let mut delta = polar(self.radius, angle + ANGLE_PI2);
        if self.first_point {
            self.subpath_start(angle, line_length);
        } else {
            self.angle_out = angle;
            self.process_corner(line_length);
        }
        for side in 0..2 {
            self.borders[side].line_to(v(to.x + delta.x, to.y + delta.y), true);
            delta = v(-delta.x, -delta.y);
        }
        self.angle_in = angle;
        self.center = to;
        self.line_length = line_length;
    }

    /// O laço comum dos traços largos: devolve `true` quando a borda já foi tratada.
    fn wide_stroke(&mut self, side: usize, alpha0: i64, from: Vector, to_pt: Vector, end: Vector) -> Option<Vector> {
        if !self.handle_wide_strokes {
            return None;
        }
        let b = &mut self.borders[side];
        let start = *b.points.last()?;
        let alpha1 = atan2(end.x - start.x, end.y - start.y);
        if angle_diff(alpha0, alpha1).abs() <= ANGLE_PI / 2 {
            return None;
        }
        let beta = atan2(from.x - start.x, from.y - start.y);
        let gamma = atan2(to_pt.x - end.x, to_pt.y - end.y);
        let bvec = diff(end, start);
        let blen = length(bvec.x, bvec.y);
        let sin_a = sin(alpha1 - gamma).abs();
        let sin_b = sin(beta - gamma).abs();
        let alen = mul_div(blen, sin_a, sin_b);
        let d = polar(alen, beta);
        b.movable = false;
        b.line_to(v(d.x + start.x, d.y + start.y), false);
        b.line_to(end, false);
        Some(start)
    }

    /// `FT_Stroker_ConicTo`.
    pub fn conic_to(&mut self, control: Vector, to: Vector) {
        if close(diff(self.center, control)) && close(diff(control, to)) {
            self.center = to;
            return;
        }
        let mut stack = [Vector::default(); 34];
        let limit = 30isize;
        let mut arc: isize = 0;
        let mut first_arc = true;
        stack[0] = to;
        stack[1] = control;
        stack[2] = self.center;
        while arc >= 0 {
            let a = arc as usize;
            let (mut angle_in, mut angle_out) = (self.angle_in, self.angle_in);
            if arc < limit && !conic_is_small_enough(&stack[a..], &mut angle_in, &mut angle_out) {
                if self.first_point {
                    self.angle_in = angle_in;
                }
                conic_split(&mut stack[a..]);
                arc += 2;
                continue;
            }
            if first_arc {
                first_arc = false;
                if self.first_point {
                    self.subpath_start(angle_in, 0);
                } else {
                    self.angle_out = angle_in;
                    self.process_corner(0);
                }
            } else if angle_diff(self.angle_in, angle_in).abs() > SMALL_CONIC_THRESHOLD / 4 {
                self.center = stack[a + 2];
                self.angle_out = angle_in;
                self.line_join = LineJoin::Round;
                self.process_corner(0);
                self.line_join = self.line_join_saved;
            }
            let theta = angle_diff(angle_in, angle_out) / 2;
            let phi = angle_in + theta;
            let len = div_fix(self.radius, cos(theta));
            let alpha0 = if self.handle_wide_strokes {
                atan2(stack[a].x - stack[a + 2].x, stack[a].y - stack[a + 2].y)
            } else {
                0
            };
            for side in 0..2 {
                let rotate = side_to_rotate(side);
                let c = polar(len, phi + rotate);
                let ctrl = v(c.x + stack[a + 1].x, c.y + stack[a + 1].y);
                let e = polar(self.radius, angle_out + rotate);
                let end = v(e.x + stack[a].x, e.y + stack[a].y);
                if let Some(start) = self.wide_stroke(side, alpha0, stack[a + 2], stack[a], end) {
                    let b = &mut self.borders[side];
                    b.conic_to(ctrl, start);
                    b.line_to(end, false);
                    continue;
                }
                self.borders[side].conic_to(ctrl, end);
            }
            arc -= 2;
            self.angle_in = angle_out;
        }
        self.center = to;
        self.line_length = 0;
    }

    /// `FT_Stroker_CubicTo`.
    pub fn cubic_to(&mut self, c1: Vector, c2: Vector, to: Vector) {
        if close(diff(self.center, c1)) && close(diff(c1, c2)) && close(diff(c2, to)) {
            self.center = to;
            return;
        }
        let mut stack = [Vector::default(); 37];
        let limit = 32isize;
        let mut arc: isize = 0;
        let mut first_arc = true;
        stack[0] = to;
        stack[1] = c2;
        stack[2] = c1;
        stack[3] = self.center;
        while arc >= 0 {
            let a = arc as usize;
            let (mut ai, mut am, mut ao) = (self.angle_in, self.angle_in, self.angle_in);
            if arc < limit && !cubic_is_small_enough(&stack[a..], &mut ai, &mut am, &mut ao) {
                if self.first_point {
                    self.angle_in = ai;
                }
                cubic_split(&mut stack[a..]);
                arc += 3;
                continue;
            }
            if first_arc {
                first_arc = false;
                if self.first_point {
                    self.subpath_start(ai, 0);
                } else {
                    self.angle_out = ai;
                    self.process_corner(0);
                }
            } else if angle_diff(self.angle_in, ai).abs() > SMALL_CUBIC_THRESHOLD / 4 {
                self.center = stack[a + 3];
                self.angle_out = ai;
                self.line_join = LineJoin::Round;
                self.process_corner(0);
                self.line_join = self.line_join_saved;
            }
            let theta1 = angle_diff(ai, am) / 2;
            let theta2 = angle_diff(am, ao) / 2;
            let phi1 = angle_mean(ai, am);
            let phi2 = angle_mean(am, ao);
            let len1 = div_fix(self.radius, cos(theta1));
            let len2 = div_fix(self.radius, cos(theta2));
            let alpha0 = if self.handle_wide_strokes {
                atan2(stack[a].x - stack[a + 3].x, stack[a].y - stack[a + 3].y)
            } else {
                0
            };
            for side in 0..2 {
                let rotate = side_to_rotate(side);
                let p1 = polar(len1, phi1 + rotate);
                let ctrl1 = v(p1.x + stack[a + 2].x, p1.y + stack[a + 2].y);
                let p2 = polar(len2, phi2 + rotate);
                let ctrl2 = v(p2.x + stack[a + 1].x, p2.y + stack[a + 1].y);
                let e = polar(self.radius, ao + rotate);
                let end = v(e.x + stack[a].x, e.y + stack[a].y);
                if let Some(start) = self.wide_stroke(side, alpha0, stack[a + 3], stack[a], end) {
                    let b = &mut self.borders[side];
                    b.cubic_to(ctrl2, ctrl1, start);
                    b.line_to(end, false);
                    continue;
                }
                self.borders[side].cubic_to(ctrl1, ctrl2, end);
            }
            arc -= 3;
            self.angle_in = ao;
        }
        self.center = to;
        self.line_length = 0;
    }

    /// `FT_Stroker_BeginSubPath`.
    pub fn begin_subpath(&mut self, to: Vector, open: bool) {
        self.first_point = true;
        self.center = to;
        self.subpath_open = open;
        self.handle_wide_strokes = self.line_join != LineJoin::Round || (open && self.line_cap == LineCap::Butt);
        self.subpath_start = to;
        self.angle_in = 0;
    }

    fn add_reverse_left(&mut self, open: bool) {
        let [right, left] = &mut self.borders;
        let start = left.start as usize;
        if left.points.len() > start {
            for i in (start..left.points.len()).rev() {
                let mut t = left.tags[i];
                if open {
                    t &= !TAG_BEGIN_END;
                } else {
                    let tt = t & TAG_BEGIN_END;
                    if tt == TAG_BEGIN || tt == TAG_END {
                        t ^= TAG_BEGIN_END;
                    }
                }
                right.points.push(left.points[i]);
                right.tags.push(t);
            }
            left.points.truncate(start);
            left.tags.truncate(start);
            right.movable = false;
            left.movable = false;
        }
    }

    /// `FT_Stroker_EndSubPath`.
    pub fn end_subpath(&mut self) {
        if self.subpath_open {
            self.cap(self.angle_in, 0);
            self.add_reverse_left(true);
            self.center = self.subpath_start;
            self.cap(self.subpath_angle + ANGLE_PI, 0);
            self.borders[0].close(false);
        } else {
            if !close(diff(self.center, self.subpath_start)) {
                self.line_to(self.subpath_start);
            }
            self.angle_out = self.subpath_angle;
            self.process_corner(self.subpath_line_length);
            self.borders[0].close(false);
            self.borders[1].close(true);
        }
    }

    /// `FT_Stroker_ParseOutline`.
    pub fn parse_outline(&mut self, o: &Outline, opened: bool) -> Result<(), Error> {
        self.rewind();
        let mut last: isize = -1;
        for &c in &o.contours {
            let first = (last + 1) as usize;
            last = c as isize;
            if last as usize <= first {
                continue;
            }
            let last_u = last as usize;
            let mut limit = last;
            let mut v_start = o.points[first];
            let v_last = o.points[last_u];
            let mut point = first as isize;
            let tag = o.tags[first] & 3;
            if tag == 2 {
                return Err(Error::InvalidOutline);
            }
            if tag == 0 {
                if o.tags[last_u] & 3 == 1 {
                    v_start = v_last;
                    limit -= 1;
                } else {
                    v_start = v((v_start.x + v_last.x) / 2, (v_start.y + v_last.y) / 2);
                }
                point -= 1;
            }
            self.begin_subpath(v_start, opened);
            'contour: while point < limit {
                point += 1;
                let p = point as usize;
                match o.tags[p] & 3 {
                    1 => self.line_to(o.points[p]),
                    0 => {
                        let mut control = o.points[p];
                        loop {
                            if point < limit {
                                point += 1;
                                let q = point as usize;
                                let vec = o.points[q];
                                match o.tags[q] & 3 {
                                    1 => {
                                        self.conic_to(control, vec);
                                        continue 'contour;
                                    }
                                    0 => {}
                                    _ => return Err(Error::InvalidOutline),
                                }
                                self.conic_to(control, v((control.x + vec.x) / 2, (control.y + vec.y) / 2));
                                control = vec;
                            } else {
                                self.conic_to(control, v_start);
                                break 'contour;
                            }
                        }
                    }
                    _ => {
                        if point + 1 > limit || o.tags[p + 1] & 3 != 2 {
                            return Err(Error::InvalidOutline);
                        }
                        point += 2;
                        let (v1, v2) = (o.points[point as usize - 2], o.points[point as usize - 1]);
                        if point <= limit {
                            self.cubic_to(v1, v2, o.points[point as usize]);
                            continue;
                        }
                        self.cubic_to(v1, v2, v_start);
                        break 'contour;
                    }
                }
            }
            if !self.first_point {
                self.end_subpath();
            }
        }
        Ok(())
    }

    /// `FT_Glyph_Stroke`: o contorno inteiro substituído pelas duas bordas.
    pub fn stroke(&mut self, o: &Outline) -> Result<Outline, Error> {
        self.parse_outline(o, false)?;
        // `FT_Stroker_GetCounts`: a segunda borda só é contada se a primeira for válida.
        let (p0, _) = self.borders[0].counts();
        let mut total = p0;
        if self.borders[0].valid {
            total += self.borders[1].counts().0;
        }
        if total > 0xFFFF {
            return Err(Error::InvalidArgument);
        }
        let mut out = Outline::default();
        for b in &self.borders {
            if b.valid {
                b.export(&mut out);
            }
        }
        Ok(out)
    }
}
