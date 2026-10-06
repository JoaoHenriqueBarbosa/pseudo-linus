//! `aflatin.c`: o sistema de escrita latino do autohinter (métricas globais, segmentos, arestas,
//! zonas azuis e o ajuste das arestas na grade).

use super::hints::*;
use super::{blue_stringset, get_cluster, Scaler, StyleClass, RENDER_MODE_LCD, RENDER_MODE_LCD_V, RENDER_MODE_LIGHT, RENDER_MODE_MONO};
use super::tables::{SCRIPTS, STYLES, STYLE_NONE_DFLT};
use crate::calc::{div_fix, mul_div, mul_fix, pix_round};
use crate::outline::{Outline, TAG_ON};
use crate::Face;

const MAX_WIDTHS: usize = 16;

const BLUE_ACTIVE: u32 = 1 << 0;
const BLUE_TOP: u32 = 1 << 1;
const BLUE_SUB_TOP: u32 = 1 << 2;
const BLUE_NEUTRAL: u32 = 1 << 3;
const BLUE_ADJUSTMENT: u32 = 1 << 4;

const PROP_TOP: u32 = 1 << 0;
const PROP_SUB_TOP: u32 = 1 << 1;
const PROP_NEUTRAL: u32 = 1 << 2;
const PROP_X_HEIGHT: u32 = 1 << 3;
const PROP_LONG: u32 = 1 << 4;

const HINTS_HORZ_SNAP: u32 = 1 << 0;
const HINTS_VERT_SNAP: u32 = 1 << 1;
const HINTS_STEM_ADJUST: u32 = 1 << 2;
const HINTS_MONO: u32 = 1 << 3;

const INT_MIN: i64 = i32::MIN as i64;
const INT_MAX: i64 = i32::MAX as i64;

/// `(FT_Short)x`.
pub(crate) fn s16(x: i64) -> i64 {
    i64::from(x as i16)
}

/// `AF_LATIN_CONSTANT`: constantes pensadas para 2048 unidades por em.
pub(crate) fn latin_constant(upem: i64, c: i64) -> i64 {
    c * upem / 2048
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Width {
    pub org: i64,
    pub cur: i64,
    pub fit: i64,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct LatinBlue {
    pub r: Width,
    pub shoot: Width,
    pub ascender: i64,
    pub descender: i64,
    pub flags: u32,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct LatinAxis {
    pub scale: i64,
    pub delta: i64,
    pub widths: Vec<Width>,
    pub edge_distance_threshold: i64,
    pub standard_width: i64,
    pub extra_light: bool,
    pub blues: Vec<LatinBlue>,
    pub org_scale: i64,
    pub org_delta: i64,
}

/// `AF_LatinMetricsRec`.
pub(crate) struct LatinMetrics {
    pub style: usize,
    pub units_per_em: i64,
    pub axis: [LatinAxis; 2],
    pub digits_have_same_width: bool,
    pub scaler: Scaler,
    x_ppem: u16,
}

impl LatinMetrics {
    fn style_class(&self) -> &'static StyleClass {
        &STYLES[self.style]
    }

    /// `af_latin_metrics_init`. `None` quando a fonte não tem nenhuma zona azul do estilo: aí os
    /// glifos dele passam para `none_dflt`, como no C.
    pub(crate) fn new(face: &Face, style: usize, gstyles: &mut [u16]) -> Option<LatinMetrics> {
        let mut m = LatinMetrics {
            style,
            units_per_em: face.units_per_em,
            axis: [LatinAxis::default(), LatinAxis::default()],
            digits_have_same_width: false,
            scaler: Scaler::default(),
            x_ppem: 0,
        };
        if face.has_unicode_cmap() {
            m.init_widths(face);
            if !m.init_blues(face) {
                for s in gstyles.iter_mut() {
                    if *s & 0x3FFF == style as u16 {
                        *s = STYLE_NONE_DFLT as u16;
                    }
                }
                return None;
            }
            m.check_digits(face);
        }
        Some(m)
    }

    /// `af_latin_metrics_init_widths`.
    fn init_widths(&mut self, face: &Face) {
        init_widths(face, self.style, self.units_per_em, &mut self.axis);
    }
}

/// `af_latin_metrics_init_widths` (o `af_cjk_metrics_init_widths` é idêntico): as larguras de
/// haste do glifo padrão do script.
pub(crate) fn init_widths(face: &Face, style: usize, upem: i64, axes: &mut [LatinAxis; 2]) {
    {
        let script = &SCRIPTS[STYLES[style].script];
        let s = script.standard_chars.as_bytes();
        let mut p = 0;
        let mut glyph_index = 0;
        while p < s.len() {
            while p < s.len() && s[p] == b' ' {
                p += 1;
            }
            if p >= s.len() {
                break;
            }
            let (g, n) = get_cluster(face, s, &mut p);
            if n > 1 {
                continue;
            }
            glyph_index = g;
            if glyph_index != 0 {
                break;
            }
        }
        'exit: {
            if glyph_index == 0 {
                break 'exit;
            }
            let Ok(l) = face.load_unscaled(glyph_index) else { break 'exit };
            if l.outline.points.is_empty() {
                break 'exit;
            }
            let mut hints = GlyphHints { x_scale: 0x10000, y_scale: 0x10000, ..Default::default() };
            hints.reload(&l.outline, face.units_per_em);
            for dim in 0..2 {
                compute_segments(&mut hints, dim, upem);
                link_segments(&mut hints, &[], dim, upem);
                let segs = &hints.axis[dim].segments;
                let mut widths = Vec::new();
                for (i, seg) in segs.iter().enumerate() {
                    if let Some(link) = seg.link {
                        if segs[link].link == Some(i) && link > i {
                            let dist = (seg.pos - segs[link].pos).abs();
                            if widths.len() < MAX_WIDTHS {
                                widths.push(dist);
                            }
                        }
                    }
                }
                let n = if widths.is_empty() { 0 } else { sort_and_quantize_widths(&mut widths, upem / 100) };
                axes[dim].widths = widths[..n].iter().map(|&org| Width { org, cur: 0, fit: 0 }).collect();
            }
        }
        for axis in axes.iter_mut() {
            let stdw = axis.widths.first().map_or(latin_constant(upem, 50), |w| w.org);
            axis.edge_distance_threshold = stdw / 5;
            axis.standard_width = stdw;
            axis.extra_light = false;
        }
    }
}

impl LatinMetrics {
    /// Um glifo de uma string azul: o extremo e se ele é redondo. Atualiza ascendente e
    /// descendente da string pelo caminho, como o C. `None` quando o glifo não conta.
    fn blue_glyph(&self, face: &Face, gid: u32, props: u32, asc: &mut i64, desc: &mut i64) -> Option<(i64, bool)> {
        let upem = self.units_per_em;
        let flat_threshold = upem / 14;
        let is_top = props & PROP_TOP != 0;
        let is_sub_top = props & PROP_SUB_TOP != 0;
        let y_offset = 0;
        let l = face.load_unscaled(gid).ok()?;
        let outline = &l.outline;
        if outline.points.len() <= 2 {
            return None;
        }
        let points = &outline.points;
        let on = |i: usize| outline.tags[i] & 3 == TAG_ON;
        let mut best_point: i64 = -1;
        let mut best_contour_first: i64 = -1;
        let mut best_contour_last: i64 = -1;
        let mut best_y: i64 = 0;
        let mut last: i64 = -1;
        for &c in &outline.contours {
            let first = last + 1;
            last = c as i64;
            if last <= first {
                continue;
            }
            for pp in first..=last {
                let y = points[pp as usize].y;
                if is_top || is_sub_top {
                    if best_point < 0 || y > best_y {
                        best_point = pp;
                        best_y = y;
                        *asc = (*asc).max(best_y + y_offset);
                    } else {
                        *desc = (*desc).min(y + y_offset);
                    }
                } else if best_point < 0 || y < best_y {
                    best_point = pp;
                    best_y = y;
                    *desc = (*desc).min(best_y + y_offset);
                } else {
                    *asc = (*asc).max(y + y_offset);
                }
            }
            if best_point > best_contour_last {
                best_contour_first = first;
                best_contour_last = last;
            }
        }
        let mut round = false;
        if best_point >= 0 {
            let x = |i: i64| points[i as usize].x;
            let y = |i: i64| points[i as usize].y;
            let ont = |i: i64| on(i as usize);
            let best_x = x(best_point);
            let mut best_segment_first = best_point;
            let mut best_segment_last = best_point;
            let (mut best_on_point_first, mut best_on_point_last) =
                if ont(best_point) { (best_point, best_point) } else { (-1, -1) };
            let mut prev = best_point;
            let mut next = prev;
            let mut dist;
            loop {
                if prev > best_contour_first {
                    prev -= 1;
                } else {
                    prev = best_contour_last;
                }
                dist = (y(prev) - best_y).abs();
                if dist > 5 && (x(prev) - best_x).abs() <= 20 * dist {
                    break;
                }
                best_segment_first = prev;
                if ont(prev) {
                    best_on_point_first = prev;
                    if best_on_point_last < 0 {
                        best_on_point_last = prev;
                    }
                }
                if prev == best_point {
                    break;
                }
            }
            loop {
                if next < best_contour_last {
                    next += 1;
                } else {
                    next = best_contour_first;
                }
                dist = (y(next) - best_y).abs();
                if dist > 5 && (x(next) - best_x).abs() <= 20 * dist {
                    break;
                }
                best_segment_last = next;
                if ont(next) {
                    best_on_point_last = next;
                    if best_on_point_first < 0 {
                        best_on_point_first = next;
                    }
                }
                if next == best_point {
                    break;
                }
            }
            if props & PROP_LONG != 0 {
                let length_threshold = upem / 25;
                dist = (x(best_segment_last) - x(best_segment_first)).abs();
                if dist < length_threshold
                    && best_segment_last - best_segment_first + 2 <= best_contour_last - best_contour_first
                {
                    let height_threshold = upem / 4;
                    prev = best_point;
                    loop {
                        if prev > best_contour_first {
                            prev -= 1;
                        } else {
                            prev = best_contour_last;
                        }
                        if x(prev) != best_x || prev == best_point {
                            break;
                        }
                    }
                    if prev == best_point {
                        return None;
                    }
                    let left2right = x(prev) < x(best_point);
                    let mut first = best_segment_last;
                    let mut last = first;
                    let mut hit = false;
                    let (mut p_first, mut p_last): (i64, i64) = (0, 0);
                    'scan: loop {
                        'body: {
                            if !hit {
                                first = last;
                                if ont(first) {
                                    p_first = first;
                                    p_last = first;
                                } else {
                                    p_first = -1;
                                    p_last = -1;
                                }
                                hit = true;
                            }
                            if last < best_contour_last {
                                last += 1;
                            } else {
                                last = best_contour_first;
                            }
                            if (best_y - y(first)).abs() > height_threshold {
                                hit = false;
                                break 'body;
                            }
                            dist = (y(last) - y(first)).abs();
                            if dist > 5 && (x(last) - x(first)).abs() <= 20 * dist {
                                hit = false;
                                break 'body;
                            }
                            if ont(last) {
                                p_last = last;
                                if p_first < 0 {
                                    p_first = last;
                                }
                            }
                            let l2r = x(first) < x(last);
                            let d = (x(last) - x(first)).abs();
                            if l2r == left2right && d >= length_threshold {
                                loop {
                                    if last < best_contour_last {
                                        last += 1;
                                    } else {
                                        last = best_contour_first;
                                    }
                                    let d = (y(last) - y(first)).abs();
                                    // Como no C: `next` e `dist` vêm de fora deste laço.
                                    if d > 5 && (x(next) - x(first)).abs() <= 20 * dist {
                                        if last > best_contour_first {
                                            last -= 1;
                                        } else {
                                            last = best_contour_last;
                                        }
                                        break;
                                    }
                                    p_last = last;
                                    if ont(last) {
                                        p_last = last;
                                        if p_first < 0 {
                                            p_first = last;
                                        }
                                    }
                                    if last == best_segment_first {
                                        break;
                                    }
                                }
                                best_y = y(first);
                                best_segment_first = first;
                                best_segment_last = last;
                                best_on_point_first = p_first;
                                best_on_point_last = p_last;
                                break 'scan;
                            }
                        }
                        if last == best_segment_first {
                            break;
                        }
                    }
                }
            }
            best_y += y_offset;
            round = if best_on_point_first >= 0
                && best_on_point_last >= 0
                && (x(best_on_point_last) - x(best_on_point_first)).abs() > flat_threshold
            {
                false
            } else {
                !ont(best_segment_first) || !ont(best_segment_last)
            };
            if round && props & PROP_NEUTRAL != 0 {
                return None;
            }
        }
        Some((best_y, round))
    }

    /// `af_latin_metrics_init_blues`. Falso quando nenhuma zona foi achada.
    fn init_blues(&mut self, face: &Face) -> bool {
        let bss = self.style_class().blue_stringset;
        let mut blues = Vec::new();
        for bs in blue_stringset(bss) {
            let props = bs.properties;
            let is_top = props & PROP_TOP != 0;
            let is_sub_top = props & PROP_SUB_TOP != 0;
            let s = bs.text.as_bytes();
            let mut flats = Vec::new();
            let mut rounds = Vec::new();
            let (mut ascender, mut descender) = (0i64, 0i64);
            let mut p = 0;
            while p < s.len() {
                while p < s.len() && s[p] == b' ' {
                    p += 1;
                }
                if p >= s.len() {
                    break;
                }
                let (gid, n) = get_cluster(face, s, &mut p);
                if n == 0 {
                    continue;
                }
                let mut best_y_extremum = if is_top { INT_MIN } else { INT_MAX };
                let mut best_round = false;
                if gid != 0 {
                    if let Some((best_y, round)) = self.blue_glyph(face, gid, props, &mut ascender, &mut descender) {
                        if is_top {
                            if best_y > best_y_extremum {
                                best_y_extremum = best_y;
                                best_round = round;
                            }
                        } else if best_y < best_y_extremum {
                            best_y_extremum = best_y;
                            best_round = round;
                        }
                    }
                }
                if !(best_y_extremum == INT_MIN || best_y_extremum == INT_MAX) {
                    if best_round {
                        rounds.push(best_y_extremum);
                    } else {
                        flats.push(best_y_extremum);
                    }
                }
            }
            if flats.is_empty() && rounds.is_empty() {
                continue;
            }
            sort_pos(&mut rounds);
            sort_pos(&mut flats);
            let mut blue = LatinBlue::default();
            let (r, sh) = if flats.is_empty() {
                (rounds[rounds.len() / 2], rounds[rounds.len() / 2])
            } else if rounds.is_empty() {
                (flats[flats.len() / 2], flats[flats.len() / 2])
            } else {
                (flats[flats.len() / 2], rounds[rounds.len() / 2])
            };
            blue.r.org = r;
            blue.shoot.org = sh;
            if sh != r {
                let over_ref = sh > r;
                if (is_top || is_sub_top) ^ over_ref {
                    blue.r.org = (sh + r) / 2;
                    blue.shoot.org = blue.r.org;
                }
            }
            blue.ascender = ascender;
            blue.descender = descender;
            if is_top {
                blue.flags |= BLUE_TOP;
            }
            if is_sub_top {
                blue.flags |= BLUE_SUB_TOP;
            }
            if props & PROP_NEUTRAL != 0 {
                blue.flags |= BLUE_NEUTRAL;
            }
            if props & PROP_X_HEIGHT != 0 {
                blue.flags |= BLUE_ADJUSTMENT;
            }
            blues.push(blue);
        }
        if blues.is_empty() {
            return false;
        }
        // `af_latin_sort_blue` e o ajuste das zonas sobrepostas.
        let key = |b: &LatinBlue| if b.flags & (BLUE_TOP | BLUE_SUB_TOP) != 0 { b.r.org } else { b.shoot.org };
        let mut sorted: Vec<usize> = (0..blues.len()).collect();
        for i in 1..sorted.len() {
            let mut j = i;
            while j > 0 {
                let a = key(&blues[sorted[j - 1]]);
                let b = key(&blues[sorted[j]]);
                if b >= a {
                    break;
                }
                sorted.swap(j, j - 1);
                j -= 1;
            }
        }
        for i in 0..sorted.len() - 1 {
            let (ia, ib) = (sorted[i], sorted[i + 1]);
            let top = |b: &LatinBlue| b.flags & (BLUE_TOP | BLUE_SUB_TOP) != 0;
            let bv = if top(&blues[ib]) { blues[ib].shoot.org } else { blues[ib].r.org };
            let a = &mut blues[ia];
            let av = if top(a) { &mut a.shoot.org } else { &mut a.r.org };
            if *av > bv {
                *av = bv;
            }
        }
        self.axis[DIMENSION_VERT].blues = blues;
        true
    }

    /// `af_latin_metrics_check_digits`.
    fn check_digits(&mut self, face: &Face) {
        let s = b"0 1 2 3 4 5 6 7 8 9";
        let mut p = 0;
        let mut started = false;
        let mut same_width = true;
        let mut old_advance = 0;
        while p < s.len() {
            let (gid, n) = get_cluster(face, s, &mut p);
            if n > 1 || gid == 0 {
                continue;
            }
            let advance = face.advance_unscaled(gid);
            if started {
                if advance != old_advance {
                    same_width = false;
                    break;
                }
            } else {
                old_advance = advance;
                started = true;
            }
        }
        self.digits_have_same_width = same_width;
    }

    /// `af_latin_metrics_scale_dim`.
    fn scale_dim(&mut self, scaler: &Scaler, dim: usize, increase_x_height: u32) {
        let (mut scale, delta) =
            if dim == DIMENSION_HORZ { (scaler.x_scale, scaler.x_delta) } else { (scaler.y_scale, scaler.y_delta) };
        let ppem = u32::from(self.x_ppem);
        let upem = self.units_per_em;
        {
            let vert = &self.axis[DIMENSION_VERT];
            if let Some(blue) = vert.blues.iter().find(|b| b.flags & BLUE_ADJUSTMENT != 0) {
                let scaled = mul_fix(blue.shoot.org, scale);
                let limit = increase_x_height;
                let mut threshold = 40;
                if limit != 0 && ppem <= limit && ppem >= 6 {
                    threshold = 52;
                }
                let fitted = (scaled + threshold) & !63;
                if scaled != fitted && dim == DIMENSION_VERT {
                    let new_scale = mul_div(scale, fitted, scaled);
                    let mut max_height = upem;
                    for b in &vert.blues {
                        max_height = max_height.max(b.ascender);
                        max_height = max_height.max(-b.descender);
                    }
                    let dist = mul_fix(max_height, new_scale - scale);
                    if -128 < dist && dist < 128 {
                        scale = new_scale;
                    }
                }
            }
        }
        let axis = &mut self.axis[dim];
        axis.org_scale = if dim == DIMENSION_HORZ { scaler.x_scale } else { scaler.y_scale };
        axis.org_delta = delta;
        axis.scale = scale;
        axis.delta = delta;
        if dim == DIMENSION_HORZ {
            self.scaler.x_scale = scale;
            self.scaler.x_delta = delta;
        } else {
            self.scaler.y_scale = scale;
            self.scaler.y_delta = delta;
        }
        for w in &mut axis.widths {
            w.cur = mul_fix(w.org, scale);
            w.fit = w.cur;
        }
        axis.extra_light = mul_fix(axis.standard_width, scale) < 32 + 8;
        if dim == DIMENSION_VERT {
            for blue in &mut axis.blues {
                blue.r.cur = mul_fix(blue.r.org, scale) + delta;
                blue.r.fit = blue.r.cur;
                blue.shoot.cur = mul_fix(blue.shoot.org, scale) + delta;
                blue.shoot.fit = blue.shoot.cur;
                blue.flags &= !BLUE_ACTIVE;
                let dist = mul_fix(blue.r.org - blue.shoot.org, scale);
                if dist <= 48 && dist >= -48 {
                    let mut delta2 = dist.abs();
                    delta2 = if delta2 < 32 {
                        0
                    } else if delta2 < 48 {
                        32
                    } else {
                        64
                    };
                    if dist < 0 {
                        delta2 = -delta2;
                    }
                    blue.r.fit = pix_round(blue.r.cur);
                    blue.shoot.fit = blue.r.fit - delta2;
                    blue.flags |= BLUE_ACTIVE;
                }
            }
            for nn in 0..axis.blues.len() {
                let blue = axis.blues[nn];
                if blue.flags & BLUE_SUB_TOP == 0 || blue.flags & BLUE_ACTIVE == 0 {
                    continue;
                }
                for i in 0..axis.blues.len() {
                    let b = &axis.blues[i];
                    if b.flags & BLUE_SUB_TOP != 0 || b.flags & BLUE_ACTIVE == 0 {
                        continue;
                    }
                    if b.r.fit <= blue.shoot.fit && b.shoot.fit >= blue.r.fit {
                        axis.blues[nn].flags &= !BLUE_ACTIVE;
                        break;
                    }
                }
            }
        }
    }

    /// `af_latin_metrics_scale`.
    pub(crate) fn scale(&mut self, scaler: &Scaler, x_ppem: u16, increase_x_height: u32) {
        self.x_ppem = x_ppem;
        self.scaler.render_mode = scaler.render_mode;
        self.scaler.flags = scaler.flags;
        self.scale_dim(scaler, DIMENSION_HORZ, increase_x_height);
        self.scale_dim(scaler, DIMENSION_VERT, increase_x_height);
    }

    /// `af_latin_hints_init`.
    pub(crate) fn hints_init(&self, hints: &mut GlyphHints, italic: bool) {
        hints.scaler_flags = self.scaler.flags;
        hints.x_scale = self.axis[DIMENSION_HORZ].scale;
        hints.x_delta = self.axis[DIMENSION_HORZ].delta;
        hints.y_scale = self.axis[DIMENSION_VERT].scale;
        hints.y_delta = self.axis[DIMENSION_VERT].delta;
        let mode = self.scaler.render_mode;
        let mut other = 0;
        if mode == RENDER_MODE_MONO || mode == RENDER_MODE_LCD {
            other |= HINTS_HORZ_SNAP;
        }
        if mode == RENDER_MODE_MONO || mode == RENDER_MODE_LCD_V {
            other |= HINTS_VERT_SNAP;
        }
        if mode != RENDER_MODE_LIGHT && mode != RENDER_MODE_LCD {
            other |= HINTS_STEM_ADJUST;
        }
        if mode == RENDER_MODE_MONO {
            other |= HINTS_MONO;
        }
        if mode == RENDER_MODE_LIGHT || mode == RENDER_MODE_LCD || italic {
            hints.scaler_flags |= SCALER_FLAG_NO_HORIZONTAL;
        }
        hints.other_flags = other;
    }

    /// `af_latin_hints_apply`.
    pub(crate) fn apply(&self, hints: &mut GlyphHints, outline: &mut Outline, nonbase: bool, upem: i64) {
        hints.reload(outline, upem);
        let do_h = hints.scaler_flags & SCALER_FLAG_NO_HORIZONTAL == 0;
        let do_v = hints.scaler_flags & SCALER_FLAG_NO_VERTICAL == 0;
        if do_h {
            self.detect_features(hints, DIMENSION_HORZ);
        }
        if do_v {
            self.detect_features(hints, DIMENSION_VERT);
            if !nonbase {
                self.compute_blue_edges(hints);
            }
        }
        for dim in 0..2 {
            if (dim == DIMENSION_HORZ && do_h) || (dim == DIMENSION_VERT && do_v) {
                self.hint_edges(hints, dim);
                hints.align_edge_points(dim);
                hints.align_strong_points(dim);
                hints.align_weak_points(dim);
            }
        }
        hints.save(outline);
    }

    /// `af_latin_hints_detect_features`.
    fn detect_features(&self, hints: &mut GlyphHints, dim: usize) {
        let upem = self.units_per_em;
        compute_segments(hints, dim, upem);
        let widths: Vec<i64> = self.axis[dim].widths.iter().map(|w| w.org).collect();
        link_segments(hints, &widths, dim, upem);
        let ttb = dim == DIMENSION_VERT && SCRIPTS[self.style_class().script].top_to_bottom;
        compute_edges(hints, dim, self.axis[dim].edge_distance_threshold, ttb);
    }

    /// `af_latin_hints_compute_blue_edges`.
    fn compute_blue_edges(&self, hints: &mut GlyphHints) {
        let axis = &mut hints.axis[DIMENSION_VERT];
        let latin = &self.axis[DIMENSION_VERT];
        let scale = latin.scale;
        let major = axis.major_dir;
        for edge in &mut axis.edges {
            let mut best_blue = None;
            let mut best_blue_is_neutral = false;
            let mut best_dist = mul_fix(self.units_per_em / 40, scale).min(64 / 2);
            for (bb, blue) in latin.blues.iter().enumerate() {
                if blue.flags & BLUE_ACTIVE == 0 {
                    continue;
                }
                let is_top_blue = blue.flags & (BLUE_TOP | BLUE_SUB_TOP) != 0;
                let is_neutral_blue = blue.flags & BLUE_NEUTRAL != 0;
                let is_major_dir = edge.dir == major;
                if is_top_blue ^ is_major_dir || is_neutral_blue {
                    let dist = mul_fix((edge.fpos - blue.r.org).abs(), scale);
                    if dist < best_dist {
                        best_dist = dist;
                        best_blue = Some(BlueRef { blue: bb, shoot: false });
                        best_blue_is_neutral = is_neutral_blue;
                    }
                    if edge.flags & EDGE_ROUND != 0 && dist != 0 && !is_neutral_blue {
                        let is_under_ref = edge.fpos < blue.r.org;
                        if is_top_blue ^ is_under_ref {
                            let dist = mul_fix((edge.fpos - blue.shoot.org).abs(), scale);
                            if dist < best_dist {
                                best_dist = dist;
                                best_blue = Some(BlueRef { blue: bb, shoot: true });
                                best_blue_is_neutral = is_neutral_blue;
                            }
                        }
                    }
                }
            }
            if best_blue.is_some() {
                edge.blue_edge = best_blue;
                if best_blue_is_neutral {
                    edge.flags |= EDGE_NEUTRAL;
                }
            }
        }
    }

    fn blue_fit(&self, b: BlueRef) -> i64 {
        let blue = &self.axis[DIMENSION_VERT].blues[b.blue];
        if b.shoot { blue.shoot.fit } else { blue.r.fit }
    }

    /// `af_latin_compute_stem_width`.
    fn compute_stem_width(&self, hints: &GlyphHints, dim: usize, width: i64, base_delta: i64, base_flags: u32, stem_flags: u32) -> i64 {
        let axis = &self.axis[dim];
        let mut dist = width;
        let mut sign = false;
        let vertical = dim == DIMENSION_VERT;
        if hints.other_flags & HINTS_STEM_ADJUST == 0 || axis.extra_light {
            return width;
        }
        if dist < 0 {
            dist = -width;
            sign = true;
        }
        let snap = if vertical { hints.other_flags & HINTS_VERT_SNAP != 0 } else { hints.other_flags & HINTS_HORZ_SNAP != 0 };
        'done: {
            if !snap {
                if stem_flags & EDGE_SERIF != 0 && vertical && dist < 3 * 64 {
                    break 'done;
                } else if base_flags & EDGE_ROUND != 0 {
                    if dist < 80 {
                        dist = 64;
                    }
                } else if dist < 56 {
                    dist = 56;
                }
                if !axis.widths.is_empty() {
                    let delta = (dist - axis.widths[0].cur).abs();
                    if delta < 40 {
                        dist = axis.widths[0].cur;
                        if dist < 48 {
                            dist = 48;
                        }
                        break 'done;
                    }
                    if dist < 3 * 64 {
                        let delta = dist & 63;
                        dist &= -64;
                        if delta < 10 {
                            dist += delta;
                        } else if delta < 32 {
                            dist += 10;
                        } else if delta < 54 {
                            dist += 54;
                        } else {
                            dist += delta;
                        }
                    } else {
                        let mut bdelta = 0;
                        if (width > 0 && base_delta > 0) || (width < 0 && base_delta < 0) {
                            let ppem = i64::from(self.x_ppem);
                            if ppem < 10 {
                                bdelta = base_delta;
                            } else if ppem < 30 {
                                bdelta = (base_delta * (30 - ppem)) / 20;
                            }
                            if bdelta < 0 {
                                bdelta = -bdelta;
                            }
                        }
                        dist = (dist - bdelta + 32) & !63;
                    }
                }
            } else {
                let org_dist = dist;
                dist = snap_width(&axis.widths, dist);
                if vertical {
                    dist = if dist >= 64 { (dist + 16) & !63 } else { 64 };
                } else if hints.other_flags & HINTS_MONO != 0 {
                    dist = if dist < 64 { 64 } else { (dist + 32) & !63 };
                } else if dist < 48 {
                    dist = (dist + 64) >> 1;
                } else if dist < 128 {
                    dist = (dist + 22) & !63;
                    let delta = (dist - org_dist).abs();
                    if delta >= 16 {
                        dist = org_dist;
                        if dist < 48 {
                            dist = (dist + 64) >> 1;
                        }
                    }
                } else {
                    dist = (dist + 32) & !63;
                }
            }
        }
        if sign { -dist } else { dist }
    }

    /// `af_latin_align_linked_edge`.
    fn align_linked_edge(&self, hints: &mut GlyphHints, dim: usize, base: usize, stem: usize) {
        let edges = &hints.axis[dim].edges;
        let (b, s) = (edges[base], edges[stem]);
        let dist = s.opos - b.opos;
        let base_delta = b.pos - b.opos;
        let fitted = self.compute_stem_width(hints, dim, dist, base_delta, b.flags, s.flags);
        hints.axis[dim].edges[stem].pos = b.pos + fitted;
    }

    /// `af_latin_hint_edges`.
    fn hint_edges(&self, hints: &mut GlyphHints, dim: usize) {
        let n = hints.axis[dim].edges.len();
        let mut anchor: Option<usize> = None;
        let mut has_serifs = 0;
        let ttb = dim == DIMENSION_VERT && SCRIPTS[self.style_class().script].top_to_bottom;
        macro_rules! e {
            ($i:expr) => {
                hints.axis[dim].edges[$i]
            };
        }
        if dim == DIMENSION_VERT {
            for edge in 0..n {
                if e!(edge).flags & EDGE_DONE != 0 {
                    continue;
                }
                let mut edge1 = None;
                let mut edge2 = e!(edge).link;
                if let Some(e2) = edge2 {
                    if e!(edge).blue_edge.is_some() && e!(e2).blue_edge.is_some() {
                        let neutral = e!(edge).flags & EDGE_NEUTRAL != 0;
                        let neutral2 = e!(e2).flags & EDGE_NEUTRAL != 0;
                        if neutral2 {
                            e!(e2).blue_edge = None;
                            e!(e2).flags &= !EDGE_NEUTRAL;
                        } else if neutral {
                            e!(edge).blue_edge = None;
                            e!(edge).flags &= !EDGE_NEUTRAL;
                        }
                    }
                }
                let mut blue = e!(edge).blue_edge;
                if blue.is_some() {
                    edge1 = Some(edge);
                } else if let Some(e2) = edge2.filter(|&e2| e!(e2).blue_edge.is_some()) {
                    blue = e!(e2).blue_edge;
                    edge1 = Some(e2);
                    edge2 = Some(edge);
                }
                let (Some(edge1), Some(blue)) = (edge1, blue) else { continue };
                e!(edge1).pos = self.blue_fit(blue);
                e!(edge1).flags |= EDGE_DONE;
                if let Some(e2) = edge2 {
                    if e!(e2).blue_edge.is_none() {
                        self.align_linked_edge(hints, dim, edge1, e2);
                        e!(e2).flags |= EDGE_DONE;
                    }
                }
                if anchor.is_none() {
                    anchor = Some(edge);
                }
            }
        }
        for edge in 0..n {
            if e!(edge).flags & EDGE_DONE != 0 {
                continue;
            }
            let Some(edge2) = e!(edge).link else {
                has_serifs += 1;
                continue;
            };
            if e!(edge2).blue_edge.is_some() {
                self.align_linked_edge(hints, dim, edge2, edge);
                e!(edge).flags |= EDGE_DONE;
                continue;
            }
            match anchor {
                None => {
                    let org_len = e!(edge2).opos - e!(edge).opos;
                    let cur_len = self.compute_stem_width(hints, dim, org_len, 0, e!(edge).flags, e!(edge2).flags);
                    let (u_off, d_off) = if cur_len <= 64 { (32, 32) } else { (38, 26) };
                    if cur_len < 96 {
                        let org_center = e!(edge).opos + (org_len >> 1);
                        let mut cur_pos1 = pix_round(org_center);
                        let error1 = (org_center - (cur_pos1 - u_off)).abs();
                        let error2 = (org_center - (cur_pos1 + d_off)).abs();
                        if error1 < error2 {
                            cur_pos1 -= u_off;
                        } else {
                            cur_pos1 += d_off;
                        }
                        e!(edge).pos = cur_pos1 - cur_len / 2;
                        e!(edge2).pos = e!(edge).pos + cur_len;
                    } else {
                        e!(edge).pos = pix_round(e!(edge).opos);
                    }
                    anchor = Some(edge);
                    e!(edge).flags |= EDGE_DONE;
                    self.align_linked_edge(hints, dim, edge, edge2);
                }
                Some(a) => {
                    let org_pos = e!(a).pos + (e!(edge).opos - e!(a).opos);
                    let org_len = e!(edge2).opos - e!(edge).opos;
                    let org_center = org_pos + (org_len >> 1);
                    let cur_len = self.compute_stem_width(hints, dim, org_len, 0, e!(edge).flags, e!(edge2).flags);
                    if e!(edge2).flags & EDGE_DONE != 0 {
                        e!(edge).pos = e!(edge2).pos - cur_len;
                    } else if cur_len < 96 {
                        let mut cur_pos1 = pix_round(org_center);
                        let (u_off, d_off) = if cur_len <= 64 { (32, 32) } else { (38, 26) };
                        let delta1 = (org_center - (cur_pos1 - u_off)).abs();
                        let delta2 = (org_center - (cur_pos1 + d_off)).abs();
                        if delta1 < delta2 {
                            cur_pos1 -= u_off;
                        } else {
                            cur_pos1 += d_off;
                        }
                        e!(edge).pos = cur_pos1 - cur_len / 2;
                        e!(edge2).pos = cur_pos1 + cur_len / 2;
                    } else {
                        let cur_pos1 = pix_round(org_pos);
                        let delta1 = (cur_pos1 + (cur_len >> 1) - org_center).abs();
                        let cur_pos2 = pix_round(org_pos + org_len) - cur_len;
                        let delta2 = (cur_pos2 + (cur_len >> 1) - org_center).abs();
                        e!(edge).pos = if delta1 < delta2 { cur_pos1 } else { cur_pos2 };
                        e!(edge2).pos = e!(edge).pos + cur_len;
                    }
                    e!(edge).flags |= EDGE_DONE;
                    e!(edge2).flags |= EDGE_DONE;
                    if edge > 0 && (if ttb { e!(edge).pos > e!(edge - 1).pos } else { e!(edge).pos < e!(edge - 1).pos }) {
                        if let Some(l) = e!(edge).link {
                            if (e!(l).pos - e!(edge - 1).pos).abs() > 16 {
                                e!(edge).pos = e!(edge - 1).pos;
                            }
                        }
                    }
                }
            }
        }
        if dim == DIMENSION_HORZ && (n == 6 || n == 12) {
            let (edge1, edge2, edge3) = if n == 6 { (0, 2, 4) } else { (1, 5, 9) };
            let dist1 = e!(edge2).opos - e!(edge1).opos;
            let dist2 = e!(edge3).opos - e!(edge2).opos;
            let span = (dist1 - dist2).abs();
            if span < 8 {
                let delta = e!(edge3).pos - (2 * e!(edge2).pos - e!(edge1).pos);
                e!(edge3).pos -= delta;
                if let Some(l) = e!(edge3).link {
                    e!(l).pos -= delta;
                }
                if n == 12 {
                    e!(8).pos -= delta;
                    e!(11).pos -= delta;
                }
                e!(edge3).flags |= EDGE_DONE;
                if let Some(l) = e!(edge3).link {
                    e!(l).flags |= EDGE_DONE;
                }
            }
        }
        if has_serifs != 0 || anchor.is_none() {
            for edge in 0..n {
                if e!(edge).flags & EDGE_DONE != 0 {
                    continue;
                }
                let mut delta = 1000;
                if let Some(s) = e!(edge).serif {
                    delta = (e!(s).opos - e!(edge).opos).abs();
                }
                if delta < 64 + 16 {
                    // `af_latin_align_serif_edge( hints, edge->serif, edge )`.
                    let s = e!(edge).serif.unwrap_or(edge);
                    e!(edge).pos = e!(s).pos + (e!(edge).opos - e!(s).opos);
                } else if anchor.is_none() {
                    e!(edge).pos = pix_round(e!(edge).opos);
                    anchor = Some(edge);
                } else {
                    let a = anchor.unwrap_or(0);
                    let before = (0..edge).rev().find(|&b| e!(b).flags & EDGE_DONE != 0);
                    let after = (edge + 1..n).find(|&b| e!(b).flags & EDGE_DONE != 0);
                    if let (Some(before), Some(after)) = (before, after) {
                        if e!(after).opos == e!(before).opos {
                            e!(edge).pos = e!(before).pos;
                        } else {
                            e!(edge).pos = e!(before).pos
                                + mul_div(
                                    e!(edge).opos - e!(before).opos,
                                    e!(after).pos - e!(before).pos,
                                    e!(after).opos - e!(before).opos,
                                );
                        }
                    } else {
                        e!(edge).pos = e!(a).pos + ((e!(edge).opos - e!(a).opos + 16) & !31);
                    }
                }
                e!(edge).flags |= EDGE_DONE;
                if edge > 0 && (if ttb { e!(edge).pos > e!(edge - 1).pos } else { e!(edge).pos < e!(edge - 1).pos }) {
                    if let Some(l) = e!(edge).link {
                        if (e!(l).pos - e!(edge - 1).pos).abs() > 16 {
                            e!(edge).pos = e!(edge - 1).pos;
                        }
                    }
                }
                // O C lê `edge[-1]` aqui mesmo na primeira aresta; nesse caso a condição fica falsa.
                if edge > 0
                    && edge + 1 < n
                    && e!(edge + 1).flags & EDGE_DONE != 0
                    && (if ttb { e!(edge).pos < e!(edge + 1).pos } else { e!(edge).pos > e!(edge + 1).pos })
                {
                    if let Some(l) = e!(edge).link {
                        if (e!(l).pos - e!(edge - 1).pos).abs() > 16 {
                            e!(edge).pos = e!(edge + 1).pos;
                        }
                    }
                }
            }
        }
    }
}

/// `af_latin_snap_width`.
fn snap_width(widths: &[Width], width: i64) -> i64 {
    let mut best = 64 + 32 + 2;
    let mut reference = width;
    for w in widths {
        let dist = (width - w.cur).abs();
        if dist < best {
            best = dist;
            reference = w.cur;
        }
    }
    let scaled = pix_round(reference);
    if width >= reference {
        if width < scaled + 48 {
            return reference;
        }
    } else if width > scaled - 48 {
        return reference;
    }
    width
}

/// `af_latin_hints_compute_segments`.
pub(crate) fn compute_segments(hints: &mut GlyphHints, dim: usize, upem: i64) {
    let flat_threshold = upem / 14;
    let seg0 = Segment { score: 32000, flags: EDGE_NORMAL, ..Default::default() };
    let GlyphHints { points: pts, contours, axis, .. } = hints;
    let axis = &mut axis[dim];
    let major_dir = axis.major_dir.abs();
    let mut segment_dir = major_dir;
    let segs = &mut axis.segments;
    segs.clear();
    for p in pts.iter_mut() {
        if dim == DIMENSION_HORZ {
            p.u = p.fx;
            p.v = p.fy;
        } else {
            p.u = p.fy;
            p.v = p.fx;
        }
    }
    for &c in contours.iter() {
        let mut point = c;
        let mut last = pts[point].prev;
        let mut on_edge = false;
        let (mut min_pos, mut max_pos, mut min_coord, mut max_coord) = (32000i64, -32000i64, 32000i64, -32000i64);
        let (mut min_flags, mut max_flags) = (0u32, 0u32);
        let (mut min_on_coord, mut max_on_coord) = (32000i64, -32000i64);
        let mut prev_segment: Option<usize> = None;
        let (mut prev_min_pos, mut prev_max_pos, mut prev_min_coord, mut prev_max_coord) =
            (min_pos, max_pos, min_coord, max_coord);
        let (mut prev_min_flags, mut prev_max_flags) = (min_flags, max_flags);
        let (mut prev_min_on_coord, mut prev_max_on_coord) = (min_on_coord, max_on_coord);
        let mut segment: Option<usize> = None;
        if pts[last].out_dir.abs() == major_dir && pts[point].out_dir.abs() == major_dir {
            last = point;
            loop {
                point = pts[point].prev;
                if pts[point].out_dir.abs() != major_dir {
                    point = pts[point].next;
                    break;
                }
                if point == last {
                    break;
                }
            }
        }
        last = point;
        let mut passed = false;
        loop {
            if on_edge {
                let p = pts[point];
                let u = p.u;
                min_pos = min_pos.min(u);
                max_pos = max_pos.max(u);
                let v = p.v;
                if v < min_coord {
                    min_coord = v;
                    min_flags = p.flags;
                }
                if v > max_coord {
                    max_coord = v;
                    max_flags = p.flags;
                }
                if p.flags & FLAG_CONTROL == 0 {
                    min_on_coord = min_on_coord.min(v);
                    max_on_coord = max_on_coord.max(v);
                }
                if p.out_dir != segment_dir || point == last {
                    let si = segment.unwrap_or(0);
                    let round = (min_flags | max_flags) & FLAG_CONTROL != 0 && (max_on_coord - min_on_coord) < flat_threshold;
                    if prev_segment.is_none_or(|ps| segs[si].first != segs[ps].last) {
                        let s = &mut segs[si];
                        s.last = point;
                        s.pos = s16((min_pos + max_pos) >> 1);
                        s.delta = s16((max_pos - min_pos) >> 1);
                        if round {
                            s.flags |= EDGE_ROUND;
                        }
                        s.min_coord = s16(min_coord);
                        s.max_coord = s16(max_coord);
                        s.height = s16(s.max_coord - s.min_coord);
                        prev_segment = Some(si);
                        prev_min_pos = min_pos;
                        prev_max_pos = max_pos;
                        prev_min_coord = min_coord;
                        prev_max_coord = max_coord;
                        prev_min_flags = min_flags;
                        prev_max_flags = max_flags;
                        prev_min_on_coord = min_on_coord;
                        prev_max_on_coord = max_on_coord;
                    } else {
                        let ps = prev_segment.unwrap_or(0);
                        if pts[segs[ps].last].in_dir == p.in_dir {
                            if prev_min_pos < min_pos {
                                min_pos = prev_min_pos;
                            }
                            if prev_max_pos > max_pos {
                                max_pos = prev_max_pos;
                            }
                            if prev_min_coord < min_coord {
                                min_coord = prev_min_coord;
                                min_flags = prev_min_flags;
                            }
                            if prev_max_coord > max_coord {
                                max_coord = prev_max_coord;
                                max_flags = prev_max_flags;
                            }
                            if prev_min_on_coord < min_on_coord {
                                min_on_coord = prev_min_on_coord;
                            }
                            if prev_max_on_coord > max_on_coord {
                                max_on_coord = prev_max_on_coord;
                            }
                            let round =
                                (min_flags | max_flags) & FLAG_CONTROL != 0 && (max_on_coord - min_on_coord) < flat_threshold;
                            let s = &mut segs[ps];
                            s.last = point;
                            s.pos = s16((min_pos + max_pos) >> 1);
                            s.delta = s16((max_pos - min_pos) >> 1);
                            if round {
                                s.flags |= EDGE_ROUND;
                            } else {
                                s.flags &= !EDGE_ROUND;
                            }
                            s.min_coord = s16(min_coord);
                            s.max_coord = s16(max_coord);
                            s.height = s16(s.max_coord - s.min_coord);
                        } else if (prev_max_coord - prev_min_coord).abs() > (max_coord - min_coord).abs() {
                            if min_pos < prev_min_pos {
                                prev_min_pos = min_pos;
                            }
                            if max_pos > prev_max_pos {
                                prev_max_pos = max_pos;
                            }
                            let s = &mut segs[ps];
                            s.last = point;
                            s.pos = s16((prev_min_pos + prev_max_pos) >> 1);
                            s.delta = s16((prev_max_pos - prev_min_pos) >> 1);
                        } else {
                            if prev_min_pos < min_pos {
                                min_pos = prev_min_pos;
                            }
                            if prev_max_pos > max_pos {
                                max_pos = prev_max_pos;
                            }
                            let s = &mut segs[si];
                            s.last = point;
                            s.pos = s16((min_pos + max_pos) >> 1);
                            s.delta = s16((max_pos - min_pos) >> 1);
                            if round {
                                s.flags |= EDGE_ROUND;
                            }
                            s.min_coord = s16(min_coord);
                            s.max_coord = s16(max_coord);
                            s.height = s16(s.max_coord - s.min_coord);
                            segs[ps] = segs[si];
                            prev_min_pos = min_pos;
                            prev_max_pos = max_pos;
                            prev_min_coord = min_coord;
                            prev_max_coord = max_coord;
                            prev_min_flags = min_flags;
                            prev_max_flags = max_flags;
                            prev_min_on_coord = min_on_coord;
                            prev_max_on_coord = max_on_coord;
                        }
                        segs.pop();
                    }
                    on_edge = false;
                    segment = None;
                }
            }
            if point == last {
                if passed {
                    break;
                }
                passed = true;
            }
            let p = pts[point];
            if !on_edge && (p.out_dir.abs() == major_dir || point == p.prev) {
                if segs.len() > 1000 {
                    segs.clear();
                    return;
                }
                segment_dir = p.out_dir;
                segs.push(seg0);
                let si = segs.len() - 1;
                segs[si].dir = segment_dir;
                segs[si].first = point;
                segs[si].last = point;
                if prev_segment.is_some() {
                    prev_segment = si.checked_sub(1);
                }
                min_pos = p.u;
                max_pos = p.u;
                min_coord = p.v;
                max_coord = p.v;
                min_flags = p.flags;
                max_flags = p.flags;
                if p.flags & FLAG_CONTROL != 0 {
                    min_on_coord = 32000;
                    max_on_coord = -32000;
                } else {
                    min_on_coord = p.v;
                    max_on_coord = p.v;
                }
                on_edge = true;
                segment = Some(si);
                if point == p.prev {
                    let s = &mut segs[si];
                    s.pos = s16(min_pos);
                    if p.flags & FLAG_CONTROL != 0 {
                        s.flags |= EDGE_ROUND;
                    }
                    s.min_coord = s16(p.v);
                    s.max_coord = s16(p.v);
                    s.height = 0;
                    on_edge = false;
                    segment = None;
                }
            }
            point = pts[point].next;
        }
    }
    for s in segs.iter_mut() {
        let first_v = pts[s.first].v;
        let last_v = pts[s.last].v;
        if first_v < last_v {
            let p = pts[pts[s.first].prev].v;
            if p < first_v {
                s.height = s16(s.height + ((first_v - p) >> 1));
            }
            let p = pts[pts[s.last].next].v;
            if p > last_v {
                s.height = s16(s.height + ((p - last_v) >> 1));
            }
        } else {
            let p = pts[pts[s.first].prev].v;
            if p > first_v {
                s.height = s16(s.height + ((p - first_v) >> 1));
            }
            let p = pts[pts[s.last].next].v;
            if p < last_v {
                s.height = s16(s.height + ((last_v - p) >> 1));
            }
        }
    }
}

/// `af_latin_hints_link_segments`.
pub(crate) fn link_segments(hints: &mut GlyphHints, widths: &[i64], dim: usize, upem: i64) {
    let axis = &mut hints.axis[dim];
    let major = axis.major_dir;
    let segs = &mut axis.segments;
    let max_width = widths.last().copied().unwrap_or(0);
    let mut len_threshold = latin_constant(upem, 8);
    if len_threshold == 0 {
        len_threshold = 1;
    }
    let len_score = latin_constant(upem, 6000);
    let dist_score = 3000;
    let n = segs.len();
    for s1 in 0..n {
        if segs[s1].dir != major {
            continue;
        }
        for s2 in 0..n {
            let pos1 = segs[s1].pos;
            let pos2 = segs[s2].pos;
            if i32::from(segs[s1].dir) + i32::from(segs[s2].dir) == 0 && pos2 > pos1 {
                let min = segs[s1].min_coord.max(segs[s2].min_coord);
                let max = segs[s1].max_coord.min(segs[s2].max_coord);
                let len = max - min;
                if len >= len_threshold {
                    let dist = pos2 - pos1;
                    let dist_demerit = if max_width != 0 {
                        let delta = (dist << 10) / max_width - (1 << 10);
                        if delta > 10000 {
                            32000
                        } else if delta > 0 {
                            delta * delta / dist_score
                        } else {
                            0
                        }
                    } else {
                        dist
                    };
                    let score = dist_demerit + len_score / len;
                    if score < segs[s1].score {
                        segs[s1].score = score;
                        segs[s1].link = Some(s2);
                    }
                    if score < segs[s2].score {
                        segs[s2].score = score;
                        segs[s2].link = Some(s1);
                    }
                }
            }
        }
    }
    for s1 in 0..n {
        if let Some(s2) = segs[s1].link {
            if segs[s2].link != Some(s1) {
                segs[s1].link = None;
                segs[s1].serif = segs[s2].link;
            }
        }
    }
}

/// `af_latin_hints_compute_edges`.
pub(crate) fn compute_edges(hints: &mut GlyphHints, dim: usize, laxis_edt: i64, top_to_bottom: bool) {
    let scale = if dim == DIMENSION_HORZ { hints.x_scale } else { hints.y_scale };
    let segment_length_threshold = if dim == DIMENSION_HORZ { div_fix(64, hints.y_scale) } else { 0 };
    let segment_width_threshold = div_fix(32, scale);
    let edt = mul_fix(laxis_edt, scale).min(64 / 4);
    let edge_distance_threshold = div_fix(edt, scale);
    let axis = &mut hints.axis[dim];
    axis.edges.clear();
    let n = axis.segments.len();
    for si in 0..n {
        let seg = axis.segments[si];
        if seg.height < segment_length_threshold || seg.delta > segment_width_threshold || seg.dir == DIR_NONE {
            continue;
        }
        if seg.serif.is_some() && 2 * seg.height < 3 * segment_length_threshold {
            continue;
        }
        let found = axis
            .edges
            .iter()
            .position(|e| (seg.pos - e.fpos).abs() < edge_distance_threshold && e.dir == seg.dir);
        match found {
            None => {
                let e = axis.new_edge(seg.pos, seg.dir, top_to_bottom);
                let edge = &mut axis.edges[e];
                edge.first = si;
                edge.last = si;
                edge.dir = seg.dir;
                edge.fpos = s16(seg.pos);
                edge.opos = mul_fix(seg.pos, scale);
                edge.pos = edge.opos;
                axis.segments[si].edge_next = si;
            }
            Some(f) => {
                let (first, last) = (axis.edges[f].first, axis.edges[f].last);
                axis.segments[si].edge_next = first;
                axis.segments[last].edge_next = si;
                axis.edges[f].last = si;
            }
        }
    }
    for si in 0..n {
        let seg = axis.segments[si];
        if seg.dir != DIR_NONE {
            continue;
        }
        let found = axis.edges.iter().position(|e| (seg.pos - e.fpos).abs() < edge_distance_threshold);
        if let Some(f) = found {
            let (first, last) = (axis.edges[f].first, axis.edges[f].last);
            axis.segments[si].edge_next = first;
            axis.segments[last].edge_next = si;
            axis.edges[f].last = si;
        }
    }
    let ne = axis.edges.len();
    for e in 0..ne {
        let first = axis.edges[e].first;
        let mut s = first;
        loop {
            axis.segments[s].edge = Some(e);
            s = axis.segments[s].edge_next;
            if s == first {
                break;
            }
        }
    }
    for e in 0..ne {
        let (mut is_round, mut is_straight) = (0, 0);
        let first = axis.edges[e].first;
        let mut s = first;
        loop {
            let seg = axis.segments[s];
            if seg.flags & EDGE_ROUND != 0 {
                is_round += 1;
            } else {
                is_straight += 1;
            }
            let is_serif = seg.serif.is_some_and(|sr| axis.segments[sr].edge.is_some_and(|se| se != e));
            if seg.link.is_some_and(|l| axis.segments[l].edge.is_some()) || is_serif {
                let mut edge2 = axis.edges[e].link;
                let mut seg2 = seg.link;
                if is_serif {
                    seg2 = seg.serif;
                    edge2 = axis.edges[e].serif;
                }
                let seg2 = seg2.unwrap_or(s);
                let edge2 = match edge2 {
                    Some(e2) => {
                        let edge_delta = (axis.edges[e].fpos - axis.edges[e2].fpos).abs();
                        let seg_delta = (seg.pos - axis.segments[seg2].pos).abs();
                        if seg_delta < edge_delta { axis.segments[seg2].edge } else { Some(e2) }
                    }
                    None => axis.segments[seg2].edge,
                };
                if is_serif {
                    axis.edges[e].serif = edge2;
                    if let Some(e2) = edge2 {
                        axis.edges[e2].flags |= EDGE_SERIF;
                    }
                } else {
                    axis.edges[e].link = edge2;
                }
            }
            s = seg.edge_next;
            if s == first {
                break;
            }
        }
        let edge = &mut axis.edges[e];
        edge.flags = EDGE_NORMAL;
        if is_round > 0 && is_round >= is_straight {
            edge.flags |= EDGE_ROUND;
        }
        if edge.serif.is_some() && edge.link.is_some() {
            edge.serif = None;
        }
    }
}
