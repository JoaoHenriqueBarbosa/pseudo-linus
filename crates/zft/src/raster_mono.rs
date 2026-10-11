//! O rasterizador monocromático do FreeType 2.13.3 (`ftraster.c`, o "black rasterizer") e o
//! renderizador `raster1` (`ftrend1.c`) que o usa para `FT_RENDER_MODE_MONO`.
//!
//! O C trabalha num pool de `long` fixo na pilha (`FT_RENDER_POOL_SIZE` / 8 posições). Quando os
//! perfis de uma faixa não cabem nele, a faixa é dividida ao meio, e isso muda o recorte dos
//! perfis nas bordas das faixas; por isso o pool é reproduzido com o mesmo tamanho e a mesma
//! contabilidade (cabeçalho de perfil com 5 posições, viradas de y crescendo do fim para o começo).

use crate::calc::mul_div_no_round;
use crate::outline::{Outline, Vector};
use crate::raster::Bitmap;

pub const OUTLINE_IGNORE_DROPOUTS: u32 = 0x8;
pub const OUTLINE_SMART_DROPOUTS: u32 = 0x10;
pub const OUTLINE_INCLUDE_STUBS: u32 = 0x20;
pub const OUTLINE_HIGH_PRECISION: u32 = 0x100;
pub const OUTLINE_SINGLE_PASS: u32 = 0x200;

const MAX_BEZIER: usize = 32;
const PIXEL_BITS: i64 = 6;
/// `FT_MAX_BLACK_POOL` com o `FT_RENDER_POOL_SIZE` de 16384 do Debian.
const POOL_SIZE: usize = 16384 / 8;
/// `sizeof (TProfile)` até `x`, em posições de `long`.
const PROFILE_HEADER: usize = 5;

const FLOW_UP: u16 = 0x08;
const OVERSHOOT_TOP: u16 = 0x10;
const OVERSHOOT_BOTTOM: u16 = 0x20;
const DROPOUT: u16 = 0x40;

const TAG_ON: u8 = 1;
const TAG_CONIC: u8 = 0;
const TAG_CUBIC: u8 = 2;
const TAG_HAS_SCANMODE: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fail {
    Overflow,
    InvalidOutline,
    NegativeHeight,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Unknown,
    Ascending,
    Descending,
}

#[derive(Clone, Copy, Default)]
struct Profile {
    link: Option<usize>,
    next: Option<usize>,
    offset: i64,
    height: i64,
    start: i64,
    flags: u16,
    x_cur: i64,
    /// Posição de `x[0]` no pool.
    base: usize,
}

#[derive(Clone, Copy, Default)]
struct Pt {
    x: i64,
    y: i64,
}

type Splitter = fn(&mut [Pt], usize);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sweep {
    Vertical,
    Horizontal,
}

struct Worker<'a> {
    precision_bits: i64,
    precision: i64,
    precision_half: i64,
    precision_scale: i64,
    precision_step: i64,
    pool: Vec<i64>,
    top: usize,
    max_buff: usize,
    drop_out_control: u8,
    last_x: i64,
    last_y: i64,
    min_y: i64,
    max_y: i64,
    num_profs: usize,
    num_turns: usize,
    c_profile: Option<usize>,
    f_profile: Option<usize>,
    g_profile: Option<usize>,
    state: State,
    profiles: Vec<Profile>,
    outline: &'a Outline,
    flags: u32,
    b_top: i64,
    b_right: i64,
    b_pitch: i64,
    b_origin: i64,
    b_line: i64,
    buffer: &'a mut [u8],
    sweep: Sweep,
}

fn split_conic(base: &mut [Pt], o: usize) {
    let b = &mut base[o..];
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

fn split_cubic(base: &mut [Pt], o: usize) {
    let b = &mut base[o..];
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

/// `FMulDiv`: sem arredondamento, em `long`.
fn fmul_div(a: i64, b: i64, c: i64) -> i64 {
    if c == 0 {
        return 0;
    }
    a.wrapping_mul(b) / c
}

impl Worker<'_> {
    fn floor(&self, x: i64) -> i64 {
        x & -self.precision
    }

    fn ceiling(&self, x: i64) -> i64 {
        (x + self.precision - 1) & -self.precision
    }

    fn trunc(&self, x: i64) -> i64 {
        x >> self.precision_bits
    }

    fn frac(&self, x: i64) -> i64 {
        x & (self.precision - 1)
    }

    fn scaled(&self, x: i64) -> i64 {
        x * self.precision_scale - self.precision_half
    }

    fn is_bottom_overshoot(&self, x: i64) -> bool {
        self.ceiling(x) - x >= self.precision_half
    }

    fn is_top_overshoot(&self, x: i64) -> bool {
        x - self.floor(x) >= self.precision_half
    }

    fn smart(&self, p: i64, q: i64) -> i64 {
        self.floor((p + q + self.precision * 63 / 64) >> 1)
    }

    fn set_high_precision(&mut self, high: bool) {
        if high {
            self.precision_bits = 12;
            self.precision_step = 256;
        } else {
            self.precision_bits = 6;
            self.precision_step = 32;
        }
        self.precision = 1 << self.precision_bits;
        self.precision_half = self.precision >> 1;
        self.precision_scale = self.precision >> PIXEL_BITS;
    }

    fn turn(&self, n: isize) -> i64 {
        self.pool[(self.max_buff as isize + n) as usize]
    }

    fn insert_y_turns(&mut self, mut y: i64, top: i64) -> Result<(), Fail> {
        let mut n = self.num_turns as isize;
        let y_turns = self.max_buff as isize;
        let at = |n: isize| (y_turns + n) as usize;
        if n == 0 || top > self.pool[at(n)] {
            self.pool[at(n)] = top;
        }
        loop {
            let old = n;
            n -= 1;
            if old == 0 || y >= self.pool[at(n)] {
                break;
            }
        }
        if n < 0 || y > self.pool[at(n)] {
            self.max_buff -= 1;
            if self.max_buff <= self.top {
                return Err(Fail::Overflow);
            }
            loop {
                let y2 = self.pool[at(n)];
                self.pool[at(n)] = y;
                y = y2;
                let old = n;
                n -= 1;
                if old < 0 {
                    break;
                }
            }
            self.num_turns += 1;
        }
        Ok(())
    }

    fn new_profile(&mut self, s: State) -> Result<(), Fail> {
        let reuse = self.c_profile.filter(|&c| self.profiles[c].height == 0);
        let c = match reuse {
            Some(c) => c,
            None => {
                let base = self.top + PROFILE_HEADER;
                self.profiles.push(Profile { base, ..Profile::default() });
                self.top = base;
                if self.top >= self.max_buff {
                    return Err(Fail::Overflow);
                }
                self.profiles.len() - 1
            }
        };
        self.c_profile = Some(c);
        let mut flags = u16::from(self.drop_out_control);
        let e = match s {
            State::Ascending => {
                flags |= FLOW_UP;
                if self.is_bottom_overshoot(self.last_y) {
                    flags |= OVERSHOOT_BOTTOM;
                }
                self.ceiling(self.last_y)
            }
            State::Descending => {
                if self.is_top_overshoot(self.last_y) {
                    flags |= OVERSHOOT_TOP;
                }
                self.floor(self.last_y)
            }
            State::Unknown => return Err(Fail::InvalidOutline),
        };
        let e = e.min(self.max_y).max(self.min_y);
        let start = self.trunc(e) as i32 as i64;
        let p = &mut self.profiles[c];
        p.flags = flags;
        p.start = start;
        if self.last_y == e {
            self.push(self.last_x)?;
        }
        self.state = s;
        Ok(())
    }

    fn push(&mut self, v: i64) -> Result<(), Fail> {
        // As verificações de estouro vêm antes; aqui só o pool físico.
        *self.pool.get_mut(self.top).ok_or(Fail::Overflow)? = v;
        self.top += 1;
        Ok(())
    }

    fn end_profile(&mut self) -> Result<(), Fail> {
        let c = self.c_profile.expect("perfil corrente");
        let h = self.top as i64 - self.profiles[c].base as i64;
        if h < 0 {
            return Err(Fail::NegativeHeight);
        }
        if h > 0 {
            let over_top = self.is_top_overshoot(self.last_y);
            let over_bottom = self.is_bottom_overshoot(self.last_y);
            let p = &mut self.profiles[c];
            p.height = h;
            let (bottom, top);
            if p.flags & FLOW_UP != 0 {
                if over_top {
                    p.flags |= OVERSHOOT_TOP;
                }
                bottom = p.start;
                top = bottom + h;
                p.offset = 0;
                p.x_cur = self.pool[p.base];
            } else {
                if over_bottom {
                    p.flags |= OVERSHOOT_BOTTOM;
                }
                top = p.start + 1;
                bottom = top - h;
                p.start = bottom;
                p.offset = h - 1;
                p.x_cur = self.pool[p.base + h as usize - 1];
            }
            self.insert_y_turns(bottom, top)?;
            if self.g_profile.is_none() {
                self.g_profile = Some(c);
            }
            let p = &mut self.profiles[c];
            p.next = self.g_profile;
            // `p->link = (PProfile)ras.top`: o próximo perfil alocado.
            p.link = Some(c + 1);
            self.num_profs += 1;
        }
        Ok(())
    }

    fn finalize_profile_table(&mut self) {
        let mut n = self.num_profs;
        let mut p = self.f_profile.expect("primeiro perfil");
        loop {
            n -= 1;
            if n == 0 {
                break;
            }
            let q = self.profiles[p].link.expect("encadeado");
            if self.profiles[q].next == self.profiles[p].next {
                self.profiles[p].next = Some(q);
            }
            p = q;
        }
        self.profiles[p].link = None;
    }

    #[allow(clippy::too_many_arguments)]
    fn line_up(&mut self, x1: i64, y1: i64, x2: i64, y2: i64, miny: i64, maxy: i64) -> Result<(), Fail> {
        if y2 < miny || y1 > maxy {
            return Ok(());
        }
        let e2 = if y2 > maxy { maxy } else { self.floor(y2) };
        let mut e = if y1 < miny { miny } else { self.ceiling(y1) };
        if y1 == e {
            e += self.precision;
        }
        if e2 < e {
            return Ok(());
        }
        let mut size = self.trunc(e2 - e) as i32 as i64 + 1;
        if self.top as i64 + size >= self.max_buff as i64 {
            return Err(Fail::Overflow);
        }
        let mut dx = x2 - x1;
        let dy = y2 - y1;
        let mut x1 = x1;
        if dx == 0 {
            while size > 0 {
                self.push(x1)?;
                size -= 1;
            }
            return Ok(());
        }
        let mut ix = mul_div_no_round(e - y1, dx, dy);
        x1 += ix;
        self.push(x1)?;
        size -= 1;
        if size > 0 {
            let mut ax = dx.wrapping_mul(e - y1).wrapping_sub(dy.wrapping_mul(ix));
            ix = fmul_div(self.precision, dx, dy);
            let mut rx = dx.wrapping_mul(self.precision).wrapping_sub(dy.wrapping_mul(ix));
            dx = 1;
            if x2 < x1 {
                ax = -ax;
                rx = -rx;
                dx = -dx;
            }
            while size > 0 {
                x1 += ix;
                ax += rx;
                if ax >= dy {
                    ax -= dy;
                    x1 += dx;
                }
                self.push(x1)?;
                size -= 1;
            }
        }
        Ok(())
    }

    fn line_down(&mut self, x1: i64, y1: i64, x2: i64, y2: i64, miny: i64, maxy: i64) -> Result<(), Fail> {
        self.line_up(x1, -y1, x2, -y2, -maxy, -miny)
    }

    fn bezier_up(&mut self, degree: usize, arcs: &mut [Pt], arc0: usize, split: Splitter, miny: i64, maxy: i64) -> Result<(), Fail> {
        let mut arc = arc0 as isize;
        let y1 = arcs[arc0 + degree].y;
        let y2 = arcs[arc0].y;
        if y2 < miny || y1 > maxy {
            return Ok(());
        }
        let e2 = if y2 > maxy { maxy } else { self.floor(y2) };
        let mut e = if y1 < miny { miny } else { self.ceiling(y1) };
        if y1 == e {
            e += self.precision;
        }
        if e2 < e {
            return Ok(());
        }
        if self.top as i64 + self.trunc(e2 - e) + 1 >= self.max_buff as i64 {
            return Err(Fail::Overflow);
        }
        let d = degree as isize;
        loop {
            if arc < 0 || arc as usize + degree + 2 * degree >= arcs.len() {
                break;
            }
            let a = arc as usize;
            let y2 = arcs[a].y;
            let x2 = arcs[a].x;
            if y2 > e {
                let dy = y2 - arcs[a + degree].y;
                let dx = x2 - arcs[a + degree].x;
                if dy > self.precision_step || dx > self.precision_step || -dx > self.precision_step {
                    split(arcs, a);
                    arc += d;
                } else {
                    self.push(x2 - fmul_div(y2 - e, dx, dy))?;
                    e += self.precision;
                    arc -= d;
                }
            } else {
                if y2 == e {
                    self.push(x2)?;
                    e += self.precision;
                }
                arc -= d;
            }
            if e > e2 {
                break;
            }
        }
        Ok(())
    }

    fn bezier_down(&mut self, degree: usize, arcs: &mut [Pt], arc0: usize, split: Splitter, miny: i64, maxy: i64) -> Result<(), Fail> {
        for k in 0..=degree.min(3) {
            arcs[arc0 + k].y = -arcs[arc0 + k].y;
        }
        let r = self.bezier_up(degree, arcs, arc0, split, -maxy, -miny);
        arcs[arc0].y = -arcs[arc0].y;
        r
    }

    fn line_to(&mut self, x: i64, y: i64) -> Result<(), Fail> {
        if y != self.last_y {
            let s = if self.last_y < y { State::Ascending } else { State::Descending };
            if self.state != s {
                if self.state != State::Unknown {
                    self.end_profile()?;
                }
                self.new_profile(s)?;
            }
            let (lx, ly, mn, mx) = (self.last_x, self.last_y, self.min_y, self.max_y);
            if s == State::Ascending {
                self.line_up(lx, ly, x, y, mn, mx)?;
            } else {
                self.line_down(lx, ly, x, y, mn, mx)?;
            }
        }
        self.last_x = x;
        self.last_y = y;
        Ok(())
    }

    fn conic_to(&mut self, cx: i64, cy: i64, x: i64, y: i64) -> Result<(), Fail> {
        let mut arcs = [Pt::default(); 4 * MAX_BEZIER + 8];
        let mut arc: isize = 0;
        arcs[2] = Pt { x: self.last_x, y: self.last_y };
        arcs[1] = Pt { x: cx, y: cy };
        arcs[0] = Pt { x, y };
        loop {
            let a = arc as usize;
            if a + 4 >= arcs.len() {
                break;
            }
            let (y1, y2, y3, x3) = (arcs[a + 2].y, arcs[a + 1].y, arcs[a].y, arcs[a].x);
            let (ymin, ymax) = if y1 <= y3 { (y1, y3) } else { (y3, y1) };
            if y2 < self.floor(ymin) || y2 > self.ceiling(ymax) {
                split_conic(&mut arcs, a);
                arc += 2;
            } else if y1 == y3 {
                arc -= 2;
                self.last_x = x3;
                self.last_y = y3;
            } else {
                let s = if y1 < y3 { State::Ascending } else { State::Descending };
                if self.state != s {
                    if self.state != State::Unknown {
                        self.end_profile()?;
                    }
                    self.new_profile(s)?;
                }
                let (mn, mx) = (self.min_y, self.max_y);
                if s == State::Ascending {
                    self.bezier_up(2, &mut arcs, a, split_conic, mn, mx)?;
                } else {
                    self.bezier_down(2, &mut arcs, a, split_conic, mn, mx)?;
                }
                arc -= 2;
                self.last_x = x3;
                self.last_y = y3;
            }
            if arc < 0 {
                break;
            }
        }
        Ok(())
    }

    fn cubic_to(&mut self, cx1: i64, cy1: i64, cx2: i64, cy2: i64, x: i64, y: i64) -> Result<(), Fail> {
        let mut arcs = [Pt::default(); 6 * MAX_BEZIER + 12];
        let mut arc: isize = 0;
        arcs[3] = Pt { x: self.last_x, y: self.last_y };
        arcs[2] = Pt { x: cx1, y: cy1 };
        arcs[1] = Pt { x: cx2, y: cy2 };
        arcs[0] = Pt { x, y };
        loop {
            let a = arc as usize;
            if a + 6 >= arcs.len() {
                break;
            }
            let (y1, y2, y3, y4, x4) = (arcs[a + 3].y, arcs[a + 2].y, arcs[a + 1].y, arcs[a].y, arcs[a].x);
            let (ymin1, ymax1) = if y1 <= y4 { (y1, y4) } else { (y4, y1) };
            let (ymin2, ymax2) = if y2 <= y3 { (y2, y3) } else { (y3, y2) };
            if ymin2 < self.floor(ymin1) || ymax2 > self.ceiling(ymax1) {
                split_cubic(&mut arcs, a);
                arc += 3;
            } else if y1 == y4 {
                arc -= 3;
                self.last_x = x4;
                self.last_y = y4;
            } else {
                let s = if y1 < y4 { State::Ascending } else { State::Descending };
                if self.state != s {
                    if self.state != State::Unknown {
                        self.end_profile()?;
                    }
                    self.new_profile(s)?;
                }
                let (mn, mx) = (self.min_y, self.max_y);
                if s == State::Ascending {
                    self.bezier_up(3, &mut arcs, a, split_cubic, mn, mx)?;
                } else {
                    self.bezier_down(3, &mut arcs, a, split_cubic, mn, mx)?;
                }
                arc -= 3;
                self.last_x = x4;
                self.last_y = y4;
            }
            if arc < 0 {
                break;
            }
        }
        Ok(())
    }

    fn scaled_point(&self, p: Vector, flipped: bool) -> (i64, i64) {
        let (x, y) = (self.scaled(p.x), self.scaled(p.y));
        if flipped { (y, x) } else { (x, y) }
    }

    fn decompose_curve(&mut self, first: usize, last: usize, flipped: bool) -> Result<(), Fail> {
        let o = self.outline;
        let pts = &o.points;
        let tags = &o.tags;
        let mut limit = last as isize;
        let (mut sx, mut sy) = self.scaled_point(pts[first], flipped);
        let (lx, ly) = self.scaled_point(pts[last], flipped);
        let (mut cx, mut cy);
        let mut point = first as isize;
        if tags[first] & TAG_HAS_SCANMODE != 0 {
            self.drop_out_control = tags[first] >> 5;
        }
        let tag = tags[first] & 3;
        if tag == TAG_CUBIC {
            return Err(Fail::InvalidOutline);
        }
        if tag == TAG_CONIC {
            if tags[last] & 3 == TAG_ON {
                (sx, sy) = (lx, ly);
                limit -= 1;
            } else {
                sx = (sx + lx) / 2;
                sy = (sy + ly) / 2;
            }
            point -= 1;
        }
        self.last_x = sx;
        self.last_y = sy;
        while point < limit {
            point += 1;
            let p = point as usize;
            match tags[p] & 3 {
                TAG_ON => {
                    let (x, y) = self.scaled_point(pts[p], flipped);
                    self.line_to(x, y)?;
                }
                TAG_CONIC => {
                    (cx, cy) = self.scaled_point(pts[p], flipped);
                    loop {
                        if point < limit {
                            point += 1;
                            let p = point as usize;
                            let t = tags[p] & 3;
                            let (x, y) = self.scaled_point(pts[p], flipped);
                            if t == TAG_ON {
                                self.conic_to(cx, cy, x, y)?;
                                break;
                            }
                            if t != TAG_CONIC {
                                return Err(Fail::InvalidOutline);
                            }
                            self.conic_to(cx, cy, (cx + x) / 2, (cy + y) / 2)?;
                            (cx, cy) = (x, y);
                        } else {
                            self.conic_to(cx, cy, sx, sy)?;
                            return Ok(());
                        }
                    }
                }
                _ => {
                    if point + 1 > limit || tags[p + 1] & 3 != TAG_CUBIC {
                        return Err(Fail::InvalidOutline);
                    }
                    point += 2;
                    let (x1, y1) = self.scaled_point(pts[point as usize - 2], flipped);
                    let (x2, y2) = self.scaled_point(pts[point as usize - 1], flipped);
                    if point <= limit {
                        let (x3, y3) = self.scaled_point(pts[point as usize], flipped);
                        self.cubic_to(x1, y1, x2, y2, x3, y3)?;
                        continue;
                    }
                    self.cubic_to(x1, y1, x2, y2, sx, sy)?;
                    return Ok(());
                }
            }
        }
        self.line_to(sx, sy)
    }

    fn convert_glyph(&mut self, flipped: bool) -> Result<(), Fail> {
        self.f_profile = None;
        self.c_profile = None;
        self.profiles.clear();
        self.top = 0;
        self.max_buff = POOL_SIZE - 1;
        self.num_turns = 0;
        self.num_profs = 0;
        let mut first;
        let mut last: isize = -1;
        for i in 0..self.outline.contours.len() {
            self.state = State::Unknown;
            self.g_profile = None;
            first = (last + 1) as usize;
            last = self.outline.contours[i] as isize;
            self.decompose_curve(first, last as usize, flipped)?;
            let Some(g) = self.g_profile else { continue };
            if self.frac(self.last_y) == 0 && self.last_y >= self.min_y && self.last_y <= self.max_y {
                let c = self.c_profile.expect("perfil corrente");
                if self.profiles[g].flags & FLOW_UP == self.profiles[c].flags & FLOW_UP {
                    self.top -= 1;
                }
            }
            self.end_profile()?;
            if self.f_profile.is_none() {
                self.f_profile = self.g_profile;
            }
        }
        if self.f_profile.is_some() {
            self.finalize_profile_table();
        }
        Ok(())
    }

    fn ins_new(&mut self, list: &mut Option<usize>, profile: usize) {
        let x = self.profiles[profile].x_cur;
        let mut prev: Option<usize> = None;
        let mut current = *list;
        while let Some(c) = current {
            if self.profiles[c].x_cur >= x {
                break;
            }
            prev = Some(c);
            current = self.profiles[c].link;
        }
        self.profiles[profile].link = current;
        match prev {
            Some(p) => self.profiles[p].link = Some(profile),
            None => *list = Some(profile),
        }
    }

    fn set_link(&mut self, list: &mut Option<usize>, prev: Option<usize>, v: Option<usize>) {
        match prev {
            Some(p) => self.profiles[p].link = v,
            None => *list = v,
        }
    }

    fn increment(&mut self, list: &mut Option<usize>, flow: i64) {
        let mut prev: Option<usize> = None;
        let mut cur = *list;
        while let Some(c) = cur {
            let p = &mut self.profiles[c];
            p.height -= 1;
            if p.height != 0 {
                p.offset += flow;
                p.x_cur = self.pool[(p.base as i64 + p.offset) as usize];
                prev = Some(c);
                cur = p.link;
            } else {
                let l = p.link;
                self.set_link(list, prev, l);
                cur = l;
            }
        }
        let mut prev: Option<usize> = None;
        let Some(mut current) = *list else { return };
        while let Some(next) = self.profiles[current].link {
            if self.profiles[current].x_cur <= self.profiles[next].x_cur {
                prev = Some(current);
                current = next;
            } else {
                self.set_link(list, prev, Some(next));
                self.profiles[current].link = self.profiles[next].link;
                self.profiles[next].link = Some(current);
                prev = None;
                current = list.expect("lista não vazia");
            }
        }
    }

    fn sweep_init(&mut self, min: i64) {
        if self.sweep == Sweep::Vertical {
            self.b_line = self.b_origin - min * self.b_pitch;
        }
    }

    fn sweep_step(&mut self) {
        if self.sweep == Sweep::Vertical {
            self.b_line -= self.b_pitch;
        }
    }

    fn or_byte(&mut self, at: i64, v: i64) {
        if let Some(b) = usize::try_from(at).ok().and_then(|i| self.buffer.get_mut(i)) {
            *b |= v as u8;
        }
    }

    fn sweep_span(&mut self, y: i64, x1: i64, x2: i64) {
        match self.sweep {
            Sweep::Vertical => {
                let mut e1 = self.trunc(self.ceiling(x1)) as i32 as i64;
                let mut e2 = self.trunc(self.floor(x2)) as i32 as i64;
                if e2 >= 0 && e1 <= self.b_right {
                    e1 = e1.max(0);
                    e2 = e2.min(self.b_right);
                    let c1 = e1 >> 3;
                    let mut c2 = e2 >> 3;
                    let f1 = 0xFF >> (e1 & 7);
                    let f2 = !0x7Fi64 >> (e2 & 7);
                    let mut target = self.b_line + c1;
                    c2 -= c1;
                    if c2 > 0 {
                        self.or_byte(target, f1);
                        loop {
                            c2 -= 1;
                            if c2 <= 0 {
                                break;
                            }
                            target += 1;
                            if let Some(b) = usize::try_from(target).ok().and_then(|i| self.buffer.get_mut(i)) {
                                *b = 0xFF;
                            }
                        }
                        self.or_byte(target + 1, f2);
                    } else {
                        self.or_byte(target, f1 & f2);
                    }
                }
            }
            Sweep::Horizontal => {
                let e1 = self.ceiling(x1);
                let e2 = self.floor(x2);
                for (x, e) in [(x1, e1), (x2, e2)] {
                    if x == e {
                        let e = self.trunc(e);
                        if e >= 0 && e <= self.b_top {
                            let at = self.b_origin + (y >> 3) - e * self.b_pitch;
                            self.or_byte(at, 0x80 >> (y & 7));
                        }
                    }
                }
            }
        }
    }

    fn bit_set(&self, at: i64, f: i64) -> bool {
        usize::try_from(at).ok().and_then(|i| self.buffer.get(i)).is_some_and(|b| i64::from(*b) & f != 0)
    }

    fn sweep_drop(&mut self, y: i64, x1: i64, x2: i64) {
        let mut e1 = self.trunc(x1) as i32 as i64;
        let e2 = self.trunc(x2) as i32 as i64;
        let limit = if self.sweep == Sweep::Vertical { self.b_right } else { self.b_top };
        let at = |w: &Self, e: i64| match w.sweep {
            Sweep::Vertical => (w.b_line + (e >> 3), 0x80 >> (e & 7)),
            Sweep::Horizontal => (w.b_origin + (y >> 3) - e * w.b_pitch, 0x80 >> (y & 7)),
        };
        if e1 < 0 || e1 > limit {
            e1 = e2;
        } else if e2 >= 0 && e2 <= limit {
            let (a, f) = at(self, e2);
            if self.bit_set(a, f) {
                return;
            }
        }
        if e1 >= 0 && e1 <= limit {
            let (a, f) = at(self, e1);
            self.or_byte(a, f);
        }
    }

    fn draw_sweep(&mut self) {
        let mut waiting = self.f_profile;
        let mut draw_left: Option<usize> = None;
        let mut draw_right: Option<usize> = None;
        let min_y = self.turn(0) as i32 as i64;
        let max_y = self.turn(self.num_turns as isize) as i32 as i64 - 1;
        self.sweep_init(min_y);
        let mut y = min_y;
        while y <= max_y {
            let mut prev: Option<usize> = None;
            let mut q = waiting;
            while let Some(p) = q {
                let link = self.profiles[p].link;
                if self.profiles[p].start == y {
                    self.set_link(&mut waiting, prev, link);
                    if self.profiles[p].flags & FLOW_UP != 0 {
                        self.ins_new(&mut draw_left, p);
                    } else {
                        self.ins_new(&mut draw_right, p);
                    }
                } else {
                    prev = Some(p);
                }
                q = link;
            }
            self.max_buff += 1;
            let y_turn = self.turn(0) as i32 as i64;
            loop {
                let mut dropouts = 0;
                let (mut pl, mut pr) = (draw_left, draw_right);
                while let (Some(l), Some(r)) = (pl, pr) {
                    let mut x1 = self.profiles[l].x_cur;
                    let mut x2 = self.profiles[r].x_cur;
                    if x1 > x2 {
                        std::mem::swap(&mut x1, &mut x2);
                    }
                    if self.ceiling(x1) <= self.floor(x2) {
                        self.sweep_span(y, x1, x2);
                    } else {
                        let lp = self.profiles[l];
                        let rp = self.profiles[r];
                        let doc = lp.flags & 7;
                        let skip = doc & 2 != 0
                            || (doc & 1 != 0
                                && ((lp.height == 1
                                    && lp.next == Some(r)
                                    && !(lp.flags & OVERSHOOT_TOP != 0 && x2 - x1 >= self.precision_half))
                                    || (lp.offset == 0
                                        && rp.next == Some(l)
                                        && !(lp.flags & OVERSHOOT_BOTTOM != 0 && x2 - x1 >= self.precision_half))));
                        if !skip {
                            if doc & 4 != 0 {
                                x2 = self.smart(x1, x2);
                                x1 = if x1 > x2 { x2 + self.precision } else { x2 - self.precision };
                            } else {
                                x2 = self.floor(x2);
                                x1 = self.ceiling(x1);
                            }
                            self.profiles[l].x_cur = x2;
                            self.profiles[r].x_cur = x1;
                            self.profiles[l].flags |= DROPOUT;
                            dropouts += 1;
                        }
                    }
                    pl = self.profiles[l].link;
                    pr = self.profiles[r].link;
                }
                let (mut pl, mut pr) = (draw_left, draw_right);
                while dropouts > 0 {
                    let (Some(l), Some(r)) = (pl, pr) else { break };
                    if self.profiles[l].flags & DROPOUT != 0 {
                        let (a, b) = (self.profiles[l].x_cur, self.profiles[r].x_cur);
                        self.sweep_drop(y, a, b);
                        self.profiles[l].flags &= !DROPOUT;
                        dropouts -= 1;
                    }
                    pl = self.profiles[l].link;
                    pr = self.profiles[r].link;
                }
                self.sweep_step();
                self.increment(&mut draw_left, 1);
                self.increment(&mut draw_right, -1);
                y += 1;
                if y >= y_turn {
                    break;
                }
            }
        }
    }

    fn render_single_pass(&mut self, flipped: bool, mut y_min: i64, mut y_max: i64) -> Result<(), Fail> {
        let mut band_stack = [0i64; 32];
        let mut band_top: isize = 0;
        loop {
            self.min_y = y_min * self.precision;
            self.max_y = y_max * self.precision;
            match self.convert_glyph(flipped) {
                Err(Fail::Overflow) => {
                    if y_min == y_max || band_top as usize >= band_stack.len() {
                        return Err(Fail::Overflow);
                    }
                    let y_mid = (y_min + y_max) >> 1;
                    band_stack[band_top as usize] = y_min;
                    band_top += 1;
                    y_min = y_mid + 1;
                }
                Err(e) => return Err(e),
                Ok(()) => {
                    if self.f_profile.is_some() {
                        self.draw_sweep();
                    }
                    band_top -= 1;
                    if band_top < 0 {
                        break;
                    }
                    y_max = y_min - 1;
                    y_min = band_stack[band_top as usize];
                }
            }
        }
        Ok(())
    }

    fn render_glyph(&mut self) -> Result<(), Fail> {
        self.set_high_precision(self.flags & OUTLINE_HIGH_PRECISION != 0);
        self.drop_out_control = 0;
        if self.flags & OUTLINE_IGNORE_DROPOUTS != 0 {
            self.drop_out_control |= 2;
        }
        if self.flags & OUTLINE_SMART_DROPOUTS != 0 {
            self.drop_out_control |= 4;
        }
        if self.flags & OUTLINE_INCLUDE_STUBS == 0 {
            self.drop_out_control |= 1;
        }
        self.sweep = Sweep::Vertical;
        self.render_single_pass(false, 0, self.b_top)?;
        if self.flags & OUTLINE_SINGLE_PASS == 0 {
            self.sweep = Sweep::Horizontal;
            self.render_single_pass(true, 0, self.b_right)?;
        }
        Ok(())
    }
}

/// `ft_black_render` sobre um bitmap mono de cima para baixo, com o contorno já deslocado.
fn black_render(o: &Outline, rows: u32, width: u32, pitch: i32, buffer: &mut [u8]) -> Result<(), Fail> {
    if o.points.is_empty() || o.contours.is_empty() {
        return Ok(());
    }
    if o.contours.last().map(|c| c + 1) != Some(o.points.len()) {
        return Err(Fail::InvalidOutline);
    }
    if width == 0 || rows == 0 {
        return Ok(());
    }
    let b_top = i64::from(rows) - 1;
    let b_pitch = i64::from(pitch);
    let mut w = Worker {
        precision_bits: 6,
        precision: 64,
        precision_half: 32,
        precision_scale: 1,
        precision_step: 32,
        pool: vec![0; POOL_SIZE],
        top: 0,
        max_buff: POOL_SIZE - 1,
        drop_out_control: 0,
        last_x: 0,
        last_y: 0,
        min_y: 0,
        max_y: 0,
        num_profs: 0,
        num_turns: 0,
        c_profile: None,
        f_profile: None,
        g_profile: None,
        state: State::Unknown,
        profiles: Vec::new(),
        outline: o,
        flags: o.flags,
        b_top,
        b_right: i64::from(width) - 1,
        b_pitch,
        b_origin: if b_pitch > 0 { b_top * b_pitch } else { 0 },
        b_line: 0,
        buffer,
        sweep: Sweep::Vertical,
    };
    w.render_glyph()
}

/// `ft_glyphslot_preset_bitmap` no modo mono: caixa arredondada de modo assimétrico, para que o
/// centro de um pixel sempre entre.
pub fn preset(o: &Outline) -> (i64, i64, i64, i64) {
    let cb = o.cbox();
    let (mut x0, mut y0, mut x1, mut y1) = (cb.x_min >> 6, cb.y_min >> 6, cb.x_max >> 6, cb.y_max >> 6);
    let (rx0, ry0, rx1, ry1) = (cb.x_min & 63, cb.y_min & 63, cb.x_max & 63, cb.y_max & 63);
    x0 += (rx0 + 31) >> 6;
    x1 += (rx1 + 32) >> 6;
    if x0 == x1 {
        if ((rx0 + 31) & 63) - 31 + ((rx1 + 32) & 63) - 32 < 0 {
            x0 -= 1;
        } else {
            x1 += 1;
        }
    }
    y0 += (ry0 + 31) >> 6;
    y1 += (ry1 + 32) >> 6;
    if y0 == y1 {
        if ((ry0 + 31) & 63) - 31 + ((ry1 + 32) & 63) - 32 < 0 {
            y0 -= 1;
        } else {
            y1 += 1;
        }
    }
    (x0, y0, x1, y1)
}

/// `ft_raster1_render` com `FT_RENDER_MODE_MONO`: bitmap de 1 bit por pixel, linhas de `pitch`
/// bytes, o bit mais alto à esquerda.
pub fn render(o: &Outline) -> Option<Bitmap> {
    let (x0, y0, x1, y1) = preset(o);
    if x0 < -0x8000 || x1 > 0x7FFF || y0 < -0x8000 || y1 > 0x7FFF {
        return None;
    }
    let width = (x1 - x0) as u32;
    let rows = (y1 - y0) as u32;
    let pitch = ((i64::from(width) + 15) >> 4) << 1;
    let mut bm = Bitmap { left: x0 as i32, top: y1 as i32, width, rows, pitch: pitch as i32, buffer: Vec::new() };
    bm.buffer = vec![0u8; rows as usize * pitch as usize];
    let mut t = o.clone();
    let (dx, dy) = (-x0 * 64, (i64::from(rows) - y1) * 64);
    if dx != 0 || dy != 0 {
        t.translate(dx, dy);
    }
    black_render(&t, rows, width, pitch as i32, &mut bm.buffer).ok()?;
    Some(bm)
}
