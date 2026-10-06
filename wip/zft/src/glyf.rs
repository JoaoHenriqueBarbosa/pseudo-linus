//! Carregador de glifos TrueType sem hinting (`ttgload.c`): contornos simples e compostos, pontos
//! fantasmas e as métricas do `compute_glyph_metrics`.

use crate::calc::{hypot, mul_fix};
use crate::outline::{Matrix, Outline, Vector};
use crate::sfnt::{i16_at, u16_at, Sfnt};
use crate::Error;

const ARGS_ARE_WORDS: u16 = 0x0001;
const ARGS_ARE_XY_VALUES: u16 = 0x0002;
const WE_HAVE_A_SCALE: u16 = 0x0008;
const MORE_COMPONENTS: u16 = 0x0020;
const WE_HAVE_AN_XY_SCALE: u16 = 0x0040;
const WE_HAVE_A_2X2: u16 = 0x0080;
const USE_MY_METRICS: u16 = 0x0200;
const SCALED_COMPONENT_OFFSET: u16 = 0x0800;

/// Glifo carregado: contorno já com a origem em `pp1`, e as métricas da fatia.
#[derive(Clone, Debug, Default)]
pub struct Loaded {
    pub outline: Outline,
    /// Avanço horizontal (`pp2.x - pp1.x`), escalado ou em unidades da fonte.
    pub advance: i64,
    /// Avanço em unidades da fonte (`linearHoriAdvance` antes da escala da camada base).
    pub linear: i64,
    pub hori_bearing_x: i64,
    pub hori_bearing_y: i64,
    pub width: i64,
    pub height: i64,
    pub vert_bearing_x: i64,
    pub vert_bearing_y: i64,
    pub vert_advance: i64,
}

struct Loader<'a> {
    sfnt: &'a Sfnt,
    data: &'a [u8],
    scale: Option<(i64, i64)>,
    out: Outline,
    pp1: Vector,
    pp2: Vector,
    pp3: Vector,
    pp4: Vector,
    linear: i64,
    vadvance: i64,
    bbox: [i64; 4],
    stack: Vec<u32>,
}

struct SubGlyph {
    flags: u16,
    index: u32,
    arg1: i64,
    arg2: i64,
    m: Matrix,
}

impl Loader<'_> {
    fn xs(&self) -> i64 {
        self.scale.map_or(0x10000, |s| s.0)
    }

    fn ys(&self) -> i64 {
        self.scale.map_or(0x10000, |s| s.1)
    }

    /// `tt_get_metrics` + `tt_loader_set_pp`.
    fn set_metrics(&mut self, gid: u32) {
        let (aw, lsb) = self.sfnt.hmetrics(self.data, gid);
        // Sem `vmtx`: o `tt_face_get_metrics` vertical emula com o OS/2 ou o hhea.
        let (ascender, descender) = match &self.sfnt.os2 {
            Some(o) => (i64::from(o.typo_ascender), i64::from(o.typo_descender)),
            None => (i64::from(self.sfnt.hhea_ascender), i64::from(self.sfnt.hhea_descender)),
        };
        let ah = ascender - descender;
        let tsb = ascender - self.bbox[3];
        self.linear = i64::from(aw);
        self.vadvance = ah;
        self.pp1 = Vector { x: self.bbox[0] - i64::from(lsb), y: 0 };
        self.pp2 = Vector { x: self.pp1.x + i64::from(aw), y: 0 };
        self.pp3 = Vector { x: 0, y: self.bbox[3] + tsb };
        self.pp4 = Vector { x: 0, y: self.pp3.y - ah };
    }

    fn scale_phantoms(&mut self) {
        if self.scale.is_some() {
            let (xs, ys) = (self.xs(), self.ys());
            self.pp1.x = mul_fix(self.pp1.x, xs);
            self.pp2.x = mul_fix(self.pp2.x, xs);
            self.pp3.x = mul_fix(self.pp3.x, xs);
            self.pp3.y = mul_fix(self.pp3.y, ys);
            self.pp4.x = mul_fix(self.pp4.x, xs);
            self.pp4.y = mul_fix(self.pp4.y, ys);
        }
    }

    fn load(&mut self, gid: u32, depth: usize) -> Result<(), Error> {
        if gid >= u32::from(self.sfnt.num_glyphs) {
            return Err(Error::InvalidGlyphIndex);
        }
        let (off, len) = self.sfnt.location(self.data, gid).unwrap_or((0, 0));
        let g = self.data.get(off..off + len).ok_or(Error::InvalidOutline)?;
        let mut n_contours = 0i32;
        if len > 0 {
            if len < 10 {
                return Err(Error::InvalidOutline);
            }
            n_contours = i32::from(i16_at(g, 0).unwrap_or(0));
            for k in 0..4 {
                self.bbox[k] = i64::from(i16_at(g, 2 + 2 * k).unwrap_or(0));
            }
        }
        if len == 0 || n_contours == 0 {
            self.bbox = [0; 4];
        }
        self.set_metrics(gid);
        if len == 0 || n_contours == 0 {
            self.scale_phantoms();
            return Ok(());
        }
        if n_contours > 0 {
            self.simple(&g[10..], n_contours as usize)
        } else {
            self.composite(&g[10..], gid, depth)
        }
    }

    fn simple(&mut self, g: &[u8], nc: usize) -> Result<(), Error> {
        let bad = Error::InvalidOutline;
        if nc >= 0xFFF || 2 * nc + 2 > g.len() {
            return Err(bad);
        }
        let base = self.out.points.len();
        let mut last: i64 = -1;
        let mut ends = Vec::with_capacity(nc);
        for i in 0..nc {
            let e = i64::from(u16_at(g, 2 * i).ok_or(bad)?);
            if e <= last {
                return Err(bad);
            }
            last = e;
            ends.push(e as usize);
        }
        let n = (last + 1) as usize;
        let mut p = 2 * nc;
        let n_ins = usize::from(u16_at(g, p).ok_or(bad)?);
        p += 2;
        if p + n_ins > g.len() {
            return Err(Error::TooManyHints);
        }
        p += n_ins;
        let mut flags = Vec::with_capacity(n);
        while flags.len() < n {
            let c = *g.get(p).ok_or(bad)?;
            p += 1;
            flags.push(c);
            if c & 8 != 0 {
                let count = usize::from(*g.get(p).ok_or(bad)?);
                p += 1;
                if flags.len() + count > n {
                    return Err(bad);
                }
                flags.extend(std::iter::repeat_n(c, count));
            }
        }
        // `OVERLAP_SIMPLE` no primeiro ponto do glifo.
        if flags.first().is_some_and(|f| f & 0x40 != 0) {
            self.out.overlap = true;
        }
        let mut pts = vec![Vector::default(); n];
        let mut x = 0i64;
        for (i, &f) in flags.iter().enumerate() {
            let mut d = 0i64;
            if f & 2 != 0 {
                d = i64::from(*g.get(p).ok_or(bad)?);
                p += 1;
                if f & 16 == 0 {
                    d = -d;
                }
            } else if f & 16 == 0 {
                d = i64::from(i16_at(g, p).ok_or(bad)?);
                p += 2;
            }
            x += d;
            pts[i].x = x;
        }
        let mut y = 0i64;
        for (i, &f) in flags.iter().enumerate() {
            let mut d = 0i64;
            if f & 4 != 0 {
                d = i64::from(*g.get(p).ok_or(bad)?);
                p += 1;
                if f & 32 == 0 {
                    d = -d;
                }
            } else if f & 32 == 0 {
                d = i64::from(i16_at(g, p).ok_or(bad)?);
                p += 2;
            }
            y += d;
            pts[i].y = y;
        }
        // `TT_Process_Simple_Glyph`: os fantasmas entram junto e são escalados com os pontos.
        pts.extend([self.pp1, self.pp2, self.pp3, self.pp4]);
        if let Some((xs, ys)) = self.scale {
            for v in &mut pts {
                v.x = mul_fix(v.x, xs);
                v.y = mul_fix(v.y, ys);
            }
        }
        self.pp1 = pts[n];
        self.pp2 = pts[n + 1];
        self.pp3 = pts[n + 2];
        self.pp4 = pts[n + 3];
        pts.truncate(n);
        self.out.points.extend(pts);
        self.out.tags.extend(flags.iter().map(|f| f & 1));
        self.out.contours.extend(ends.iter().map(|e| e + base));
        Ok(())
    }

    fn composite(&mut self, g: &[u8], gid: u32, depth: usize) -> Result<(), Error> {
        let bad = Error::InvalidComposite;
        if self.stack.contains(&gid) || depth > 64 {
            return Err(bad);
        }
        let mut subs = Vec::new();
        let mut p = 0usize;
        loop {
            let flags = u16_at(g, p).ok_or(bad)?;
            let index = u32::from(u16_at(g, p + 2).ok_or(bad)?);
            p += 4;
            if index >= u32::from(self.sfnt.num_glyphs) {
                return Err(bad);
            }
            let (arg1, arg2);
            let words = flags & ARGS_ARE_WORDS != 0;
            if flags & ARGS_ARE_XY_VALUES != 0 {
                if words {
                    arg1 = i64::from(i16_at(g, p).ok_or(bad)?);
                    arg2 = i64::from(i16_at(g, p + 2).ok_or(bad)?);
                    p += 4;
                } else {
                    arg1 = i64::from(*g.get(p).ok_or(bad)? as i8);
                    arg2 = i64::from(*g.get(p + 1).ok_or(bad)? as i8);
                    p += 2;
                }
            } else if words {
                arg1 = i64::from(u16_at(g, p).ok_or(bad)?);
                arg2 = i64::from(u16_at(g, p + 2).ok_or(bad)?);
                p += 4;
            } else {
                arg1 = i64::from(*g.get(p).ok_or(bad)?);
                arg2 = i64::from(*g.get(p + 1).ok_or(bad)?);
                p += 2;
            }
            let rd = |o: usize| i16_at(g, o).map(|v| i64::from(v) * 4).ok_or(bad);
            let mut m = Matrix { xx: 0x10000, xy: 0, yx: 0, yy: 0x10000 };
            if flags & WE_HAVE_A_SCALE != 0 {
                m.xx = rd(p)?;
                m.yy = m.xx;
                p += 2;
            } else if flags & WE_HAVE_AN_XY_SCALE != 0 {
                m.xx = rd(p)?;
                m.yy = rd(p + 2)?;
                p += 4;
            } else if flags & WE_HAVE_A_2X2 != 0 {
                m.xx = rd(p)?;
                m.yx = rd(p + 2)?;
                m.xy = rd(p + 4)?;
                m.yy = rd(p + 6)?;
                p += 8;
            }
            subs.push(SubGlyph { flags, index, arg1, arg2, m });
            if flags & MORE_COMPONENTS == 0 {
                break;
            }
        }
        self.scale_phantoms();
        // `OVERLAP_COMPOUND` no primeiro componente do glifo de topo.
        if depth == 0 && subs[0].flags & 0x0400 != 0 {
            self.out.overlap = true;
        }
        self.stack.push(gid);
        let start_point = self.out.points.len();
        for sg in &subs {
            let saved = (self.pp1, self.pp2, self.pp3, self.pp4, self.linear, self.vadvance);
            let num_base = self.out.points.len();
            self.load(sg.index, depth + 1)?;
            if sg.flags & USE_MY_METRICS == 0 {
                (self.pp1, self.pp2, self.pp3, self.pp4, self.linear, self.vadvance) = saved;
            }
            if self.out.points.len() == num_base {
                continue;
            }
            self.process_component(sg, start_point, num_base)?;
        }
        self.stack.pop();
        Ok(())
    }

    /// `TT_Process_Composite_Component`.
    fn process_component(&mut self, sg: &SubGlyph, start_point: usize, num_base: usize) -> Result<(), Error> {
        let have_scale = sg.flags & (WE_HAVE_A_SCALE | WE_HAVE_AN_XY_SCALE | WE_HAVE_A_2X2) != 0;
        if have_scale {
            for v in &mut self.out.points[num_base..] {
                *v = crate::outline::transform_vector(*v, &sg.m);
            }
        }
        let (mut x, mut y);
        if sg.flags & ARGS_ARE_XY_VALUES == 0 {
            let k = sg.arg1 as usize + start_point;
            let l = sg.arg2 as usize + num_base;
            if k >= num_base || l >= self.out.points.len() {
                return Err(Error::InvalidComposite);
            }
            x = self.out.points[k].x - self.out.points[l].x;
            y = self.out.points[k].y - self.out.points[l].y;
        } else {
            x = sg.arg1;
            y = sg.arg2;
            if x == 0 && y == 0 {
                return Ok(());
            }
            if have_scale && sg.flags & SCALED_COMPONENT_OFFSET != 0 {
                x = mul_fix(x, hypot(sg.m.xx, sg.m.xy));
                y = mul_fix(y, hypot(sg.m.yy, sg.m.yx));
            }
            if let Some((xs, ys)) = self.scale {
                x = mul_fix(x, xs);
                y = mul_fix(y, ys);
            }
        }
        if x != 0 || y != 0 {
            for v in &mut self.out.points[num_base..] {
                v.x += x;
                v.y += y;
            }
        }
        Ok(())
    }
}

/// `TT_Load_Glyph` sem hinting. `scale` é `(x_scale, y_scale)`, ou `None` para `FT_LOAD_NO_SCALE`.
pub(crate) fn load(sfnt: &Sfnt, data: &[u8], gid: u32, scale: Option<(i64, i64)>) -> Result<Loaded, Error> {
    let mut l = Loader {
        sfnt,
        data,
        scale,
        out: Outline::default(),
        pp1: Vector::default(),
        pp2: Vector::default(),
        pp3: Vector::default(),
        pp4: Vector::default(),
        linear: 0,
        vadvance: 0,
        bbox: [0; 4],
        stack: Vec::new(),
    };
    l.load(gid, 0)?;
    let mut out = std::mem::take(&mut l.out);
    if l.pp1.x != 0 {
        out.translate(-l.pp1.x, 0);
    }
    // `compute_glyph_metrics`.
    let b = out.cbox();
    let y_scale = l.ys();
    let height_fu = i64::from(crate::calc::div_fix(b.y_max - b.y_min, y_scale) as i16);
    let (ascender, descender) = match &sfnt.os2 {
        Some(o) => (i64::from(o.typo_ascender), i64::from(o.typo_descender)),
        None => (i64::from(sfnt.hhea_ascender), i64::from(sfnt.hhea_descender)),
    };
    let adv_fu = ascender - descender;
    let top = (adv_fu - height_fu) / 2;
    let top = mul_fix(top, y_scale);
    let vadv = mul_fix(adv_fu, y_scale);
    let advance = l.pp2.x - l.pp1.x;
    Ok(Loaded {
        hori_bearing_x: b.x_min,
        hori_bearing_y: b.y_max,
        width: b.x_max - b.x_min,
        height: b.y_max - b.y_min,
        // `metrics->vertBearingX = metrics->horiBearingX - metrics->horiAdvance / 2`.
        vert_bearing_x: b.x_min - advance / 2,
        vert_bearing_y: top,
        vert_advance: vadv,
        advance,
        linear: l.linear,
        outline: out,
    })
}
