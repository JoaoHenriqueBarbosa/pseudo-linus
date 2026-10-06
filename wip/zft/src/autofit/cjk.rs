//! `afcjk.c`: o sistema de escrita CJK do autohinter. No Debian ele também é o estilo de reserva
//! (`hani_dflt`) dos glifos que nenhum script cobre, como o `.notdef`.

use super::hints::*;
use super::latin::{compute_segments, init_widths, latin_constant, LatinAxis, LatinBlue, Width};
use super::tables::{SCRIPTS, STYLES};
use super::{blue_stringset, get_cluster, Scaler, RENDER_MODE_LCD, RENDER_MODE_LCD_V, RENDER_MODE_LIGHT, RENDER_MODE_MONO};
use crate::calc::{div_fix, mul_div, mul_fix, pix_floor, pix_round};
use crate::outline::Outline;
use crate::Face;

const BLUE_ACTIVE: u32 = 1 << 0;
const BLUE_TOP: u32 = 1 << 1;

const PROP_TOP: u32 = 1 << 0;
const PROP_HORIZ: u32 = 1 << 1;

const HINTS_HORZ_SNAP: u32 = 1 << 0;
const HINTS_VERT_SNAP: u32 = 1 << 1;
const HINTS_STEM_ADJUST: u32 = 1 << 2;
const HINTS_MONO: u32 = 1 << 3;

const LIGHT_MODE_MAX_HORZ_GAP: i64 = 9;
const LIGHT_MODE_MAX_VERT_GAP: i64 = 15;
const LIGHT_MODE_MAX_DELTA_ABS: i64 = 14;

/// `AF_CJKMetricsRec`.
pub(crate) struct CjkMetrics {
    pub units_per_em: i64,
    pub axis: [LatinAxis; 2],
    pub digits_have_same_width: bool,
    pub scaler: Scaler,
}

impl CjkMetrics {
    /// `af_cjk_metrics_init`; com `blues` falso é o `af_indic_metrics_init`, que é o mesmo sem as
    /// zonas azuis.
    pub(crate) fn new(face: &Face, style: usize, blues: bool) -> CjkMetrics {
        let mut m = CjkMetrics {
            units_per_em: face.units_per_em,
            axis: [LatinAxis::default(), LatinAxis::default()],
            digits_have_same_width: false,
            scaler: Scaler::default(),
        };
        if face.has_unicode_cmap() {
            init_widths(face, style, m.units_per_em, &mut m.axis);
            if blues {
                m.init_blues(face, style);
            }
            m.check_digits(face);
        }
        m
    }

    /// `af_cjk_metrics_init_blues`.
    fn init_blues(&mut self, face: &Face, style: usize) {
        let _ = SCRIPTS;
        for bs in blue_stringset(STYLES[style].blue_stringset) {
            let props = bs.properties;
            let horiz = props & PROP_HORIZ != 0;
            let top = props & PROP_TOP != 0;
            let s = bs.text.as_bytes();
            let mut fills = Vec::new();
            let mut flats = Vec::new();
            let mut fill = true;
            let mut p = 0;
            while p < s.len() {
                while p < s.len() && s[p] == b' ' {
                    p += 1;
                }
                if p >= s.len() {
                    break;
                }
                if s[p] == b'|' {
                    fill = false;
                    p += 1;
                    continue;
                }
                let (gid, n) = get_cluster(face, s, &mut p);
                if n > 1 || gid == 0 {
                    continue;
                }
                let Ok(l) = face.load_unscaled(gid) else { continue };
                let outline = &l.outline;
                if outline.points.len() <= 2 {
                    continue;
                }
                let mut best_point: i64 = -1;
                let mut best_pos = 0;
                let mut last: i64 = -1;
                for &c in &outline.contours {
                    let first = last + 1;
                    last = c as i64;
                    if last <= first {
                        continue;
                    }
                    for pp in first..=last {
                        let pt = outline.points[pp as usize];
                        let v = if horiz { pt.x } else { pt.y };
                        if best_point < 0 || if top { v > best_pos } else { v < best_pos } {
                            best_point = pp;
                            best_pos = v;
                        }
                    }
                }
                if fill {
                    fills.push(best_pos);
                } else {
                    flats.push(best_pos);
                }
            }
            if flats.is_empty() && fills.is_empty() {
                continue;
            }
            sort_pos(&mut fills);
            sort_pos(&mut flats);
            let (r, sh) = if flats.is_empty() {
                (fills[fills.len() / 2], fills[fills.len() / 2])
            } else if fills.is_empty() {
                (flats[flats.len() / 2], flats[flats.len() / 2])
            } else {
                (fills[fills.len() / 2], flats[flats.len() / 2])
            };
            let mut blue = LatinBlue::default();
            blue.r.org = r;
            blue.shoot.org = sh;
            if sh != r {
                let under_ref = sh < r;
                if top ^ under_ref {
                    blue.r.org = (sh + r) / 2;
                    blue.shoot.org = blue.r.org;
                }
            }
            if top {
                blue.flags |= BLUE_TOP;
            }
            let dim = if horiz { DIMENSION_HORZ } else { DIMENSION_VERT };
            self.axis[dim].blues.push(blue);
        }
    }

    /// `af_cjk_metrics_check_digits`.
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

    /// `af_cjk_metrics_scale`. As larguras de haste não são escaladas aqui (o C também não o faz:
    /// o `cur` delas fica zero).
    pub(crate) fn scale(&mut self, scaler: &Scaler) {
        self.scaler = *scaler;
        for dim in 0..2 {
            let (scale, delta) =
                if dim == DIMENSION_HORZ { (scaler.x_scale, scaler.x_delta) } else { (scaler.y_scale, scaler.y_delta) };
            let axis = &mut self.axis[dim];
            axis.org_scale = scale;
            axis.org_delta = delta;
            axis.scale = scale;
            axis.delta = delta;
            for blue in &mut axis.blues {
                blue.r.cur = mul_fix(blue.r.org, scale) + delta;
                blue.r.fit = blue.r.cur;
                blue.shoot.cur = mul_fix(blue.shoot.org, scale) + delta;
                blue.shoot.fit = blue.shoot.cur;
                blue.flags &= !BLUE_ACTIVE;
                let dist = mul_fix(blue.r.org - blue.shoot.org, scale);
                if dist <= 48 && dist >= -48 {
                    blue.r.fit = pix_round(blue.r.cur);
                    let delta1 = div_fix(blue.r.fit, scale) - blue.shoot.org;
                    let mut delta2 = mul_fix(delta1.abs(), scale);
                    delta2 = if delta2 < 32 { 0 } else { pix_round(delta2) };
                    if delta1 < 0 {
                        delta2 = -delta2;
                    }
                    blue.shoot.fit = blue.r.fit - delta2;
                    blue.flags |= BLUE_ACTIVE;
                }
            }
        }
    }

    /// `af_cjk_hints_init`.
    pub(crate) fn hints_init(&self, hints: &mut GlyphHints) {
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
        hints.scaler_flags |= SCALER_FLAG_NO_ADVANCE;
        hints.other_flags = other;
    }

    /// `af_cjk_hints_apply`.
    pub(crate) fn apply(&self, hints: &mut GlyphHints, outline: &mut Outline) {
        let upem = self.units_per_em;
        hints.reload(outline, upem);
        let do_h = hints.scaler_flags & SCALER_FLAG_NO_HORIZONTAL == 0;
        let do_v = hints.scaler_flags & SCALER_FLAG_NO_VERTICAL == 0;
        if do_h {
            self.detect_features(hints, DIMENSION_HORZ);
            self.compute_blue_edges(hints, DIMENSION_HORZ);
        }
        if do_v {
            self.detect_features(hints, DIMENSION_VERT);
            self.compute_blue_edges(hints, DIMENSION_VERT);
        }
        for dim in 0..2 {
            if (dim == DIMENSION_HORZ && do_h) || (dim == DIMENSION_VERT && do_v) {
                self.hint_edges(hints, dim);
                align_edge_points(hints, dim);
                hints.align_strong_points(dim);
                hints.align_weak_points(dim);
            }
        }
        hints.save(outline);
    }

    /// `af_cjk_hints_detect_features`.
    fn detect_features(&self, hints: &mut GlyphHints, dim: usize) {
        let upem = self.units_per_em;
        // `af_cjk_hints_compute_segments`: o latino, e depois a marca de redondo própria do CJK.
        compute_segments(hints, dim, upem);
        let GlyphHints { points, axis, .. } = hints;
        for seg in &mut axis[dim].segments {
            let mut pt = seg.first;
            let last = seg.last;
            let mut f0 = points[pt].flags & FLAG_CONTROL;
            seg.flags &= !EDGE_ROUND;
            while pt != last {
                pt = points[pt].next;
                let f1 = points[pt].flags & FLAG_CONTROL;
                if f0 == 0 && f1 == 0 {
                    break;
                }
                if pt == last {
                    seg.flags |= EDGE_ROUND;
                }
                f0 = f1;
            }
        }
        self.link_segments(hints, dim);
        self.compute_edges(hints, dim);
    }

    /// `af_cjk_hints_link_segments`.
    fn link_segments(&self, hints: &mut GlyphHints, dim: usize) {
        let len_threshold = latin_constant(self.units_per_em, 8);
        let dist_threshold = div_fix(64 * 3, if dim == DIMENSION_HORZ { hints.x_scale } else { hints.y_scale });
        let axis = &mut hints.axis[dim];
        let major_dir = axis.major_dir;
        let segs = &mut axis.segments;
        let n = segs.len();
        for s1 in 0..n {
            if segs[s1].dir != major_dir {
                continue;
            }
            for s2 in 0..n {
                if s2 == s1 || i32::from(segs[s1].dir) + i32::from(segs[s2].dir) != 0 {
                    continue;
                }
                let dist = segs[s2].pos - segs[s1].pos;
                if dist < 0 {
                    continue;
                }
                let min = segs[s1].min_coord.max(segs[s2].min_coord);
                let max = segs[s1].max_coord.min(segs[s2].max_coord);
                let len = max - min;
                if len >= len_threshold {
                    if dist * 8 < segs[s1].score * 9 && (dist * 8 < segs[s1].score * 7 || segs[s1].len < len) {
                        segs[s1].score = dist;
                        segs[s1].len = len;
                        segs[s1].link = Some(s2);
                    }
                    if dist * 8 < segs[s2].score * 9 && (dist * 8 < segs[s2].score * 7 || segs[s2].len < len) {
                        segs[s2].score = dist;
                        segs[s2].len = len;
                        segs[s2].link = Some(s1);
                    }
                }
            }
        }
        for s1 in 0..n {
            let Some(link1) = segs[s1].link else { continue };
            if segs[link1].link != Some(s1) || segs[link1].pos <= segs[s1].pos {
                continue;
            }
            if segs[s1].score >= dist_threshold {
                continue;
            }
            for s2 in 0..n {
                if segs[s2].pos > segs[s1].pos || s1 == s2 {
                    continue;
                }
                let Some(link2) = segs[s2].link else { continue };
                if segs[link2].link != Some(s2) || segs[link2].pos < segs[link1].pos {
                    continue;
                }
                if segs[s1].pos == segs[s2].pos && segs[link1].pos == segs[link2].pos {
                    continue;
                }
                if segs[s2].score <= segs[s1].score || segs[s1].score * 4 <= segs[s2].score {
                    continue;
                }
                if segs[s1].len >= segs[s2].len * 3 {
                    for s in 0..n {
                        let link = segs[s].link;
                        if link == Some(s2) {
                            segs[s].link = None;
                            segs[s].serif = Some(link1);
                        } else if link == Some(link2) {
                            segs[s].link = None;
                            segs[s].serif = Some(s1);
                        }
                    }
                } else {
                    segs[s1].link = None;
                    segs[link1].link = None;
                    break;
                }
            }
        }
        for s1 in 0..n {
            if let Some(s2) = segs[s1].link {
                if segs[s2].link != Some(s1) {
                    segs[s1].link = None;
                    if segs[s2].score < dist_threshold || segs[s1].score < segs[s2].score * 4 {
                        segs[s1].serif = segs[s2].link;
                    }
                }
            }
        }
    }

    /// `af_cjk_hints_compute_edges`.
    fn compute_edges(&self, hints: &mut GlyphHints, dim: usize) {
        let scale = if dim == DIMENSION_HORZ { hints.x_scale } else { hints.y_scale };
        let laxis = &self.axis[dim];
        let edge_distance_threshold = if mul_fix(laxis.edge_distance_threshold, scale) > 64 / 4 {
            div_fix(64 / 4, scale)
        } else {
            laxis.edge_distance_threshold
        };
        let axis = &mut hints.axis[dim];
        axis.edges.clear();
        let n = axis.segments.len();
        let dist_seg = |a: &Segment, b: &Segment| (a.pos - b.pos).abs();
        for si in 0..n {
            let seg = axis.segments[si];
            let mut found = None;
            let mut best = 0xFFFF;
            for (ee, edge) in axis.edges.iter().enumerate() {
                if edge.dir != seg.dir {
                    continue;
                }
                let dist = (seg.pos - edge.fpos).abs();
                if dist < edge_distance_threshold && dist < best {
                    if let Some(link) = seg.link {
                        let mut s1 = edge.first;
                        let mut dist2 = 0;
                        loop {
                            if let Some(link1) = axis.segments[s1].link {
                                dist2 = dist_seg(&axis.segments[link], &axis.segments[link1]);
                                if dist2 >= edge_distance_threshold {
                                    break;
                                }
                            }
                            s1 = axis.segments[s1].edge_next;
                            if s1 == edge.first {
                                break;
                            }
                        }
                        if dist2 >= edge_distance_threshold {
                            continue;
                        }
                    }
                    best = dist;
                    found = Some(ee);
                }
            }
            match found {
                None => {
                    let e = axis.new_edge(seg.pos, seg.dir, false);
                    let edge = &mut axis.edges[e];
                    edge.first = si;
                    edge.last = si;
                    edge.dir = seg.dir;
                    edge.fpos = seg.pos;
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
                let is_serif = seg.serif.is_some_and(|sr| axis.segments[sr].edge != Some(e));
                if seg.link.is_some() || is_serif {
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
                            let seg_delta = dist_seg(&seg, &axis.segments[seg2]);
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

    /// `af_cjk_hints_compute_blue_edges`.
    fn compute_blue_edges(&self, hints: &mut GlyphHints, dim: usize) {
        let axis = &mut hints.axis[dim];
        let cjk = &self.axis[dim];
        let scale = cjk.scale;
        let best_dist0 = mul_fix(self.units_per_em / 40, scale).min(64 / 2);
        let major = axis.major_dir;
        for edge in &mut axis.edges {
            let mut best_blue = None;
            let mut best_dist = best_dist0;
            for (bb, blue) in cjk.blues.iter().enumerate() {
                if blue.flags & BLUE_ACTIVE == 0 {
                    continue;
                }
                let is_top_right_blue = blue.flags & BLUE_TOP != 0;
                let is_major_dir = edge.dir == major;
                if is_top_right_blue ^ is_major_dir {
                    let shoot = (edge.fpos - blue.r.org).abs() > (edge.fpos - blue.shoot.org).abs();
                    let org = if shoot { blue.shoot.org } else { blue.r.org };
                    let dist = mul_fix((edge.fpos - org).abs(), scale);
                    if dist < best_dist {
                        best_dist = dist;
                        best_blue = Some(BlueRef { blue: bb, shoot });
                    }
                }
            }
            if best_blue.is_some() {
                edge.blue_edge = best_blue;
            }
        }
    }

    fn blue_fit(&self, dim: usize, b: BlueRef) -> i64 {
        let blue = &self.axis[dim].blues[b.blue];
        if b.shoot { blue.shoot.fit } else { blue.r.fit }
    }

    /// `af_cjk_compute_stem_width`.
    fn compute_stem_width(&self, hints: &GlyphHints, dim: usize, width: i64) -> i64 {
        let axis = &self.axis[dim];
        let mut dist = width;
        let mut sign = false;
        let vertical = dim == DIMENSION_VERT;
        if hints.other_flags & HINTS_STEM_ADJUST == 0 {
            return width;
        }
        if dist < 0 {
            dist = -width;
            sign = true;
        }
        let snap = if vertical { hints.other_flags & HINTS_VERT_SNAP != 0 } else { hints.other_flags & HINTS_HORZ_SNAP != 0 };
        'done: {
            if !snap {
                if let Some(w0) = axis.widths.first() {
                    if (dist - w0.cur).abs() < 40 {
                        dist = w0.cur.max(48);
                        break 'done;
                    }
                }
                if dist < 54 {
                    dist += (54 - dist) / 2;
                } else if dist < 3 * 64 {
                    let delta = dist & 63;
                    dist &= -64;
                    if delta < 10 {
                        dist += delta;
                    } else if delta < 22 {
                        dist += 10;
                    } else if delta < 42 {
                        dist += delta;
                    } else if delta < 54 {
                        dist += 54;
                    } else {
                        dist += delta;
                    }
                }
            } else {
                dist = snap_width(&axis.widths, dist);
                if vertical {
                    dist = if dist >= 64 { (dist + 16) & !63 } else { 64 };
                } else if hints.other_flags & HINTS_MONO != 0 {
                    dist = if dist < 64 { 64 } else { (dist + 32) & !63 };
                } else if dist < 48 {
                    dist = (dist + 64) >> 1;
                } else if dist < 128 {
                    dist = (dist + 22) & !63;
                } else {
                    dist = (dist + 32) & !63;
                }
            }
        }
        if sign { -dist } else { dist }
    }

    /// `af_cjk_align_linked_edge`.
    fn align_linked_edge(&self, hints: &mut GlyphHints, dim: usize, base: usize, stem: usize) {
        let edges = &hints.axis[dim].edges;
        let dist = edges[stem].opos - edges[base].opos;
        let fitted = self.compute_stem_width(hints, dim, dist);
        let base_pos = hints.axis[dim].edges[base].pos;
        hints.axis[dim].edges[stem].pos = base_pos + fitted;
    }

    /// `af_hint_normal_stem`.
    fn hint_normal_stem(&self, hints: &mut GlyphHints, edge: usize, edge2: usize, anchor: i64, dim: usize) -> i64 {
        let stem_adjust = hints.other_flags & HINTS_STEM_ADJUST != 0;
        let (e1, e2) = (hints.axis[dim].edges[edge], hints.axis[dim].edges[edge2]);
        let mut threshold = 64;
        if !stem_adjust {
            threshold = if e1.flags & EDGE_ROUND != 0 && e2.flags & EDGE_ROUND != 0 {
                if dim == DIMENSION_VERT { 64 - LIGHT_MODE_MAX_HORZ_GAP } else { 64 - LIGHT_MODE_MAX_VERT_GAP }
            } else if dim == DIMENSION_VERT {
                64 - LIGHT_MODE_MAX_HORZ_GAP / 3
            } else {
                64 - LIGHT_MODE_MAX_VERT_GAP / 3
            };
        }
        let org_len = e2.opos - e1.opos;
        let cur_len = self.compute_stem_width(hints, dim, org_len);
        let org_center = (e1.opos + e2.opos) / 2 + anchor;
        let mut cur_pos1 = org_center - cur_len / 2;
        let cur_pos2 = cur_pos1 + cur_len;
        let mut d_off1 = cur_pos1 - pix_floor(cur_pos1);
        let mut d_off2 = cur_pos2 - pix_floor(cur_pos2);
        let mut u_off1 = 64 - d_off1;
        let mut u_off2 = 64 - d_off2;
        let mut delta = 0;
        'exit: {
            if d_off1 == 0 || d_off2 == 0 {
                break 'exit;
            }
            if cur_len <= threshold {
                if d_off2 < cur_len {
                    delta = if u_off1 <= d_off2 { u_off1 } else { -d_off2 };
                }
                break 'exit;
            }
            if threshold < 64
                && (d_off1 >= threshold || u_off1 >= threshold || d_off2 >= threshold || u_off2 >= threshold)
            {
                break 'exit;
            }
            let mut offset = cur_len & 63;
            if offset < 32 {
                if u_off1 <= offset || d_off2 <= offset {
                    break 'exit;
                }
            } else {
                offset = 64 - threshold;
            }
            d_off1 = threshold - u_off1;
            u_off1 -= offset;
            u_off2 = threshold - d_off2;
            d_off2 -= offset;
            if d_off1 <= u_off1 {
                u_off1 = -d_off1;
            }
            if d_off2 <= u_off2 {
                u_off2 = -d_off2;
            }
            delta = if u_off1.abs() <= u_off2.abs() { u_off1 } else { u_off2 };
        }
        if !stem_adjust {
            delta = delta.clamp(-LIGHT_MODE_MAX_DELTA_ABS, LIGHT_MODE_MAX_DELTA_ABS);
        }
        cur_pos1 += delta;
        let edges = &mut hints.axis[dim].edges;
        if e1.opos < e2.opos {
            edges[edge].pos = cur_pos1;
            edges[edge2].pos = cur_pos1 + cur_len;
        } else {
            edges[edge].pos = cur_pos1 + cur_len;
            edges[edge2].pos = cur_pos1;
        }
        delta
    }

    /// `af_cjk_hint_edges`.
    fn hint_edges(&self, hints: &mut GlyphHints, dim: usize) {
        let n = hints.axis[dim].edges.len();
        let mut anchor: Option<usize> = None;
        let mut delta = 0;
        let mut skipped = 0;
        let mut has_last_stem = false;
        let mut last_stem_pos = 0;
        macro_rules! e {
            ($i:expr) => {
                hints.axis[dim].edges[$i]
            };
        }
        for edge in 0..n {
            if e!(edge).flags & EDGE_DONE != 0 {
                continue;
            }
            let mut blue = e!(edge).blue_edge;
            let mut edge1 = None;
            let mut edge2 = e!(edge).link;
            if blue.is_some() {
                edge1 = Some(edge);
            } else if let Some(e2) = edge2.filter(|&e2| e!(e2).blue_edge.is_some()) {
                blue = e!(e2).blue_edge;
                edge1 = Some(e2);
                edge2 = Some(edge);
            }
            let (Some(edge1), Some(blue)) = (edge1, blue) else { continue };
            e!(edge1).pos = self.blue_fit(dim, blue);
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
        for edge in 0..n {
            if e!(edge).flags & EDGE_DONE != 0 {
                continue;
            }
            let Some(edge2) = e!(edge).link else {
                skipped += 1;
                continue;
            };
            if has_last_stem && (e!(edge).pos < last_stem_pos + 64 || e!(edge2).pos < last_stem_pos + 64) {
                skipped += 1;
                continue;
            }
            if e!(edge2).blue_edge.is_some() {
                self.align_linked_edge(hints, dim, edge2, edge);
                e!(edge).flags |= EDGE_DONE;
                continue;
            }
            if edge2 < edge {
                self.align_linked_edge(hints, dim, edge2, edge);
                e!(edge).flags |= EDGE_DONE;
                has_last_stem = true;
                last_stem_pos = e!(edge).pos;
                continue;
            }
            if dim != DIMENSION_VERT && anchor.is_none() {
                delta = self.hint_normal_stem(hints, edge, edge2, 0, DIMENSION_HORZ);
            } else {
                self.hint_normal_stem(hints, edge, edge2, delta, dim);
            }
            anchor = Some(edge);
            e!(edge).flags |= EDGE_DONE;
            e!(edge2).flags |= EDGE_DONE;
            has_last_stem = true;
            last_stem_pos = e!(edge2).pos;
        }
        if dim == DIMENSION_HORZ && (n == 6 || n == 12) {
            let (edge1, edge2, edge3) = if n == 6 { (0, 2, 4) } else { (1, 5, 9) };
            let dist1 = e!(edge2).opos - e!(edge1).opos;
            let dist2 = e!(edge3).opos - e!(edge2).opos;
            let span = (dist1 - dist2).abs();
            if e!(edge1).link == Some(edge1 + 1)
                && e!(edge2).link == Some(edge2 + 1)
                && e!(edge3).link == Some(edge3 + 1)
                && span < 8
            {
                delta = e!(edge3).pos - (2 * e!(edge2).pos - e!(edge1).pos);
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
        if skipped == 0 {
            return;
        }
        for edge in 0..n {
            if e!(edge).flags & EDGE_DONE != 0 {
                continue;
            }
            if let Some(s) = e!(edge).serif {
                e!(edge).pos = e!(s).pos + (e!(edge).opos - e!(s).opos);
                e!(edge).flags |= EDGE_DONE;
                skipped -= 1;
            }
        }
        if skipped == 0 {
            return;
        }
        for edge in 0..n {
            if e!(edge).flags & EDGE_DONE != 0 {
                continue;
            }
            let before = (0..edge).rev().find(|&b| e!(b).flags & EDGE_DONE != 0);
            let after = (edge + 1..n).find(|&a| e!(a).flags & EDGE_DONE != 0);
            match (before, after) {
                (None, Some(a)) => e!(edge).pos = e!(a).pos + (e!(edge).opos - e!(a).opos),
                (Some(b), None) => e!(edge).pos = e!(b).pos + (e!(edge).opos - e!(b).opos),
                (Some(b), Some(a)) => {
                    if e!(a).fpos == e!(b).fpos {
                        e!(edge).pos = e!(b).pos;
                    } else {
                        e!(edge).pos = e!(b).pos
                            + mul_div(e!(edge).fpos - e!(b).fpos, e!(a).pos - e!(b).pos, e!(a).fpos - e!(b).fpos);
                    }
                }
                (None, None) => {}
            }
        }
    }
}

/// `af_cjk_snap_width`.
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

/// `af_cjk_align_edge_points`.
fn align_edge_points(hints: &mut GlyphHints, dim: usize) {
    let snapping = if dim == DIMENSION_HORZ {
        hints.other_flags & HINTS_HORZ_SNAP != 0
    } else {
        hints.other_flags & HINTS_VERT_SNAP != 0
    };
    let GlyphHints { points, axis, .. } = hints;
    let axis = &axis[dim];
    for edge in &axis.edges {
        let delta = edge.pos - edge.opos;
        let mut s = edge.first;
        loop {
            let seg = &axis.segments[s];
            let mut point = seg.first;
            loop {
                let p = &mut points[point];
                if dim == DIMENSION_HORZ {
                    p.x = if snapping { edge.pos } else { p.x + delta };
                    p.flags |= FLAG_TOUCH_X;
                } else {
                    p.y = if snapping { edge.pos } else { p.y + delta };
                    p.flags |= FLAG_TOUCH_Y;
                }
                if point == seg.last {
                    break;
                }
                point = p.next;
            }
            s = seg.edge_next;
            if s == edge.first {
                break;
            }
        }
    }
}
