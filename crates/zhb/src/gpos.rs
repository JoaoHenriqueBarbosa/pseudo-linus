//! As subtabelas do GPOS (`OT/Layout/GPOS/*.hh`) e o acabamento das posições.

use crate::buffer::{Buffer, Direction, GlyphPosition, scratch};
use crate::gsubgpos::{self, ApplyContext, IterMatch, lookup_flag};
use crate::ot::{self, NOT_COVERED, i16at, u16at};

pub const ATTACH_TYPE_MARK: u8 = 0x01;
pub const ATTACH_TYPE_CURSIVE: u8 = 0x02;

pub fn apply_subtable<'a>(c: &mut ApplyContext<'a, '_>, t: u16, d: &'a [u8]) -> bool {
    match t {
        1 => single(c, d),
        2 => pair(c, d),
        3 => cursive(c, d),
        4 => mark_base(c, d),
        5 => mark_lig(c, d),
        6 => mark_mark(c, d),
        7 => gsubgpos::context_apply(c, d),
        8 => gsubgpos::chain_context_apply(c, d),
        _ => false,
    }
}

fn value_len(format: u16) -> usize {
    format.count_ones() as usize
}

/// `Device::get_x_delta`/`get_y_delta` com o `HintingDevice`.
fn device_delta(d: Option<&[u8]>, ppem: u32, scale: i32) -> i32 {
    let Some(d) = d else { return 0 };
    let f = u32::from(u16at(d, 4));
    if !(1..=3).contains(&f) || ppem == 0 {
        return 0;
    }
    let start = u32::from(u16at(d, 0));
    let end = u32::from(u16at(d, 2));
    if ppem < start || ppem > end {
        return 0;
    }
    let s = ppem - start;
    let word = u32::from(u16at(d, 6 + (s >> (4 - f)) as usize * 2));
    let bits = word >> (16 - (((s & ((1 << (4 - f)) - 1)) + 1) << f));
    let mask = 0xFFFFu32 >> (16 - (1 << f));
    let mut delta = (bits & mask) as i32;
    if delta as u32 >= (mask + 1) >> 1 {
        delta -= (mask + 1) as i32;
    }
    if delta == 0 {
        return 0;
    }
    (i64::from(delta) * i64::from(scale) / i64::from(ppem)) as i32
}

/// `ValueFormat::apply_value`. `base` é o início da subtabela, `at` o primeiro valor.
fn apply_value(c: &ApplyContext, format: u16, base: &[u8], at: usize, pos: &mut GlyphPosition) -> bool {
    let mut ret = false;
    if format == 0 {
        return ret;
    }
    let font = c.font;
    let horizontal = c.direction.is_horizontal();
    let mut o = at;
    let mut short = |o: &mut usize, ret: &mut bool| -> i16 {
        let v = i16at(base, *o);
        *ret |= v != 0;
        *o += 2;
        v
    };
    if format & 0x1 != 0 {
        pos.x_offset += font.em_scale_x(short(&mut o, &mut ret));
    }
    if format & 0x2 != 0 {
        pos.y_offset += font.em_scale_y(short(&mut o, &mut ret));
    }
    if format & 0x4 != 0 {
        if horizontal {
            pos.x_advance += font.em_scale_x(short(&mut o, &mut ret));
        } else {
            o += 2;
        }
    }
    if format & 0x8 != 0 {
        if !horizontal {
            pos.y_advance -= font.em_scale_y(short(&mut o, &mut ret));
        } else {
            o += 2;
        }
    }
    if format & 0xF0 == 0 {
        return ret;
    }
    let use_x = font.x_ppem != 0;
    let use_y = font.y_ppem != 0;
    if !use_x && !use_y {
        return ret;
    }
    let mut device = |o: &mut usize, ret: &mut bool| -> Option<&[u8]> {
        let off = usize::from(u16at(base, *o));
        *ret |= off != 0;
        *o += 2;
        if off == 0 { None } else { base.get(off..) }
    };
    if format & 0x10 != 0 {
        let d = device(&mut o, &mut ret);
        if use_x {
            pos.x_offset += device_delta(d, font.x_ppem, font.x_scale);
        }
    }
    if format & 0x20 != 0 {
        let d = device(&mut o, &mut ret);
        if use_y {
            pos.y_offset += device_delta(d, font.y_ppem, font.y_scale);
        }
    }
    if format & 0x40 != 0 {
        let d = device(&mut o, &mut ret);
        if horizontal && use_x {
            pos.x_advance += device_delta(d, font.x_ppem, font.x_scale);
        }
    }
    if format & 0x80 != 0 {
        let d = device(&mut o, &mut ret);
        if !horizontal && use_y {
            pos.y_advance -= device_delta(d, font.y_ppem, font.y_scale);
        }
    }
    ret
}

/// `Anchor::get_anchor`.
fn anchor(c: &ApplyContext, a: Option<&[u8]>, glyph: u32) -> (f32, f32) {
    let Some(a) = a else { return (0.0, 0.0) };
    let font = c.font;
    let (x, y) = (i16at(a, 2), i16at(a, 4));
    match u16at(a, 0) {
        1 => (font.em_fscale_x(x), font.em_fscale_y(y)),
        2 => {
            let point = u32::from(u16at(a, 6));
            let cp = if font.x_ppem != 0 || font.y_ppem != 0 { font.contour_point(glyph, point) } else { None };
            let fx = match cp {
                Some((cx, _)) if font.x_ppem != 0 => cx as f32,
                _ => font.em_fscale_x(x),
            };
            let fy = match cp {
                Some((_, cy)) if font.y_ppem != 0 => cy as f32,
                _ => font.em_fscale_y(y),
            };
            (fx, fy)
        }
        3 => {
            let mut fx = font.em_fscale_x(x);
            let mut fy = font.em_fscale_y(y);
            if font.x_ppem != 0 {
                fx += device_delta(ot::sub(a, 0, 6), font.x_ppem, font.x_scale) as f32;
            }
            if font.y_ppem != 0 {
                fy += device_delta(ot::sub(a, 0, 8), font.y_ppem, font.y_scale) as f32;
            }
            (fx, fy)
        }
        _ => (0.0, 0.0),
    }
}

fn roundf(v: f32) -> i32 {
    crate::font::hb_roundf(v)
}

fn single(c: &mut ApplyContext, d: &[u8]) -> bool {
    let index = ot::coverage(ot::sub(d, 0, 2), c.buffer.cur(0).codepoint);
    if index == NOT_COVERED {
        return false;
    }
    let format = u16at(d, 4);
    let at = match u16at(d, 0) {
        1 => 6,
        2 => {
            if index >= u32::from(u16at(d, 6)) {
                return false;
            }
            8 + index as usize * value_len(format) * 2
        }
        _ => return false,
    };
    let idx = c.buffer.idx;
    let mut p = c.buffer.pos[idx];
    apply_value(c, format, d, at, &mut p);
    c.buffer.pos[idx] = p;
    c.buffer.idx += 1;
    true
}

fn next_glyph_for_pair(c: &mut ApplyContext) -> Option<usize> {
    let mut it = c.iter(false);
    it.reset_fast(c.buffer.idx);
    let mut unsafe_to = 0;
    if !it.next(c, Some(&mut unsafe_to)) {
        let idx = c.buffer.idx;
        c.buffer.unsafe_to_concat(idx, unsafe_to);
        return None;
    }
    Some(it.idx)
}

fn pair(c: &mut ApplyContext, d: &[u8]) -> bool {
    let index = ot::coverage(ot::sub(d, 0, 2), c.buffer.cur(0).codepoint);
    if index == NOT_COVERED {
        return false;
    }
    match u16at(d, 0) {
        1 => {
            let (vf1, vf2) = (u16at(d, 4), u16at(d, 6));
            if index >= u32::from(u16at(d, 8)) {
                return false;
            }
            let Some(j) = next_glyph_for_pair(c) else { return false };
            let Some(set) = ot::sub(d, 0, 10 + index as usize * 2) else {
                let idx = c.buffer.idx;
                c.buffer.unsafe_to_concat(idx, j + 1);
                return false;
            };
            pair_set_apply(c, set, vf1, vf2, j)
        }
        2 => {
            let Some(j) = next_glyph_for_pair(c) else { return false };
            let (vf1, vf2) = (u16at(d, 4), u16at(d, 6));
            let klass1 = ot::class(ot::sub(d, 0, 8), c.buffer.cur(0).codepoint);
            let klass2 = ot::class(ot::sub(d, 0, 10), c.buffer.info[j].codepoint);
            let (c1, c2) = (u32::from(u16at(d, 12)), u32::from(u16at(d, 14)));
            let idx = c.buffer.idx;
            if klass1 >= c1 || klass2 >= c2 {
                c.buffer.unsafe_to_concat(idx, j + 1);
                return false;
            }
            let (len1, len2) = (value_len(vf1), value_len(vf2));
            let at = 16 + (len1 + len2) * 2 * (klass1 * c2 + klass2) as usize;
            let mut p1 = c.buffer.pos[idx];
            let mut p2 = c.buffer.pos[j];
            let applied_first = len1 > 0 && apply_value(c, vf1, d, at, &mut p1);
            let applied_second = len2 > 0 && apply_value(c, vf2, d, at + len1 * 2, &mut p2);
            c.buffer.pos[idx] = p1;
            c.buffer.pos[j] = p2;
            let mut j = j;
            if applied_first || applied_second {
                c.buffer.unsafe_to_break(idx, j + 1);
            } else {
                c.buffer.unsafe_to_concat(idx, j + 1);
            }
            if len2 > 0 {
                j += 1;
                c.buffer.unsafe_to_break(idx, j + 1);
            }
            c.buffer.idx = j;
            true
        }
        _ => false,
    }
}

/// `PairSet::apply`.
fn pair_set_apply(c: &mut ApplyContext, set: &[u8], vf1: u16, vf2: u16, pos: usize) -> bool {
    let (len1, len2) = (value_len(vf1), value_len(vf2));
    let rec = 2 + (len1 + len2) * 2;
    let n = usize::from(u16at(set, 0));
    let g = c.buffer.info[pos].codepoint;
    let (mut lo, mut hi) = (0usize, n);
    let mut found = None;
    if set.len() >= 2 + n * rec {
        while lo < hi {
            let mid = (lo + hi) / 2;
            let r = 2 + mid * rec;
            let v = u32::from(u16at(set, r));
            if g < v {
                hi = mid;
            } else if g > v {
                lo = mid + 1;
            } else {
                found = Some(r);
                break;
            }
        }
    }
    let idx = c.buffer.idx;
    let Some(r) = found else {
        c.buffer.unsafe_to_concat(idx, pos + 1);
        return false;
    };
    let mut p1 = c.buffer.pos[idx];
    let mut p2 = c.buffer.pos[pos];
    let applied_first = len1 > 0 && apply_value(c, vf1, set, r + 2, &mut p1);
    let applied_second = len2 > 0 && apply_value(c, vf2, set, r + 2 + len1 * 2, &mut p2);
    c.buffer.pos[idx] = p1;
    c.buffer.pos[pos] = p2;
    if applied_first || applied_second {
        c.buffer.unsafe_to_break(idx, pos + 1);
    }
    let mut pos = pos;
    if len2 > 0 {
        pos += 1;
    }
    c.buffer.unsafe_to_break(idx, pos + 1);
    c.buffer.idx = pos;
    true
}

fn entry_exit<'a>(d: &'a [u8], index: u32) -> (Option<&'a [u8]>, Option<&'a [u8]>) {
    let n = u32::from(u16at(d, 4));
    if index >= n {
        return (None, None);
    }
    let r = 6 + index as usize * 4;
    (ot::sub(d, 0, r), ot::sub(d, 0, r + 2))
}

fn reverse_cursive_minor_offset(pos: &mut [GlyphPosition], i: usize, direction: Direction, new_parent: usize) {
    let (chain, ty) = (pos[i].attach_chain, pos[i].attach_type);
    if chain == 0 || ty & ATTACH_TYPE_CURSIVE == 0 {
        return;
    }
    pos[i].attach_chain = 0;
    let j = (i as isize + chain as isize) as usize;
    if j == new_parent {
        return;
    }
    reverse_cursive_minor_offset(pos, j, direction, new_parent);
    if direction.is_horizontal() {
        pos[j].y_offset = -pos[i].y_offset;
    } else {
        pos[j].x_offset = -pos[i].x_offset;
    }
    pos[j].attach_chain = -chain;
    pos[j].attach_type = ty;
}

fn cursive(c: &mut ApplyContext, d: &[u8]) -> bool {
    if u16at(d, 0) != 1 {
        return false;
    }
    let cov = ot::sub(d, 0, 2);
    let (this_entry, _) = entry_exit(d, ot::coverage(cov, c.buffer.cur(0).codepoint));
    if this_entry.is_none() {
        return false;
    }
    let mut it = c.iter(false);
    it.reset_fast(c.buffer.idx);
    let mut unsafe_from = 0;
    if !it.prev(c, Some(&mut unsafe_from)) {
        let idx = c.buffer.idx;
        c.buffer.unsafe_to_concat_from_outbuffer(unsafe_from, idx + 1);
        return false;
    }
    let i = it.idx;
    let j = c.buffer.idx;
    let (_, prev_exit) = entry_exit(d, ot::coverage(cov, c.buffer.info[i].codepoint));
    if prev_exit.is_none() {
        c.buffer.unsafe_to_concat_from_outbuffer(i, j + 1);
        return false;
    }
    c.buffer.unsafe_to_break(i, j + 1);
    let (exit_x, exit_y) = anchor(c, prev_exit, c.buffer.info[i].codepoint);
    let (entry_x, entry_y) = anchor(c, this_entry, c.buffer.info[j].codepoint);
    let direction = c.direction;
    let lookup_props = c.lookup_props;
    let pos = &mut c.buffer.pos;
    match direction {
        Direction::Ltr => {
            pos[i].x_advance = roundf(exit_x) + pos[i].x_offset;
            let d = roundf(entry_x) + pos[j].x_offset;
            pos[j].x_advance -= d;
            pos[j].x_offset -= d;
        }
        Direction::Rtl => {
            let d = roundf(exit_x) + pos[i].x_offset;
            pos[i].x_advance -= d;
            pos[i].x_offset -= d;
            pos[j].x_advance = roundf(entry_x) + pos[j].x_offset;
        }
        Direction::Ttb => {
            pos[i].y_advance = roundf(exit_y) + pos[i].y_offset;
            let d = roundf(entry_y) + pos[j].y_offset;
            pos[j].y_advance -= d;
            pos[j].y_offset -= d;
        }
        Direction::Btt => {
            let d = roundf(exit_y) + pos[i].y_offset;
            pos[i].y_advance -= d;
            pos[i].y_offset -= d;
            pos[j].y_advance = roundf(entry_y);
        }
        Direction::Invalid => {}
    }
    let (mut child, mut parent) = (i, j);
    let mut x_offset = roundf(entry_x - exit_x);
    let mut y_offset = roundf(entry_y - exit_y);
    if lookup_props & lookup_flag::RIGHT_TO_LEFT == 0 {
        std::mem::swap(&mut child, &mut parent);
        x_offset = -x_offset;
        y_offset = -y_offset;
    }
    reverse_cursive_minor_offset(pos, child, direction, parent);
    pos[child].attach_type = ATTACH_TYPE_CURSIVE;
    pos[child].attach_chain = (parent as isize - child as isize) as i16;
    if direction.is_horizontal() {
        pos[child].y_offset = y_offset;
    } else {
        pos[child].x_offset = x_offset;
    }
    if pos[parent].attach_chain == -pos[child].attach_chain {
        pos[parent].attach_chain = 0;
        if direction.is_horizontal() {
            pos[parent].y_offset = 0;
        } else {
            pos[parent].x_offset = 0;
        }
    }
    c.buffer.scratch_flags |= scratch::HAS_GPOS_ATTACHMENT;
    c.buffer.idx += 1;
    true
}

/// `MarkArray::apply`.
fn mark_array_apply(c: &mut ApplyContext, marks: &[u8], mark_index: u32, glyph_index: u32, matrix: Option<&[u8]>, class_count: u32, glyph_pos: usize) -> bool {
    // Índice fora do array devolve o registro nulo: classe 0, âncora nula.
    let (mark_class, mark_anchor) = if mark_index < u32::from(u16at(marks, 0)) {
        let r = 2 + mark_index as usize * 4;
        (u32::from(u16at(marks, r)), ot::sub(marks, 0, r + 2))
    } else {
        (0, None)
    };
    // `AnchorMatrix::get_anchor`.
    let Some(m) = matrix else { return false };
    let rows = u32::from(u16at(m, 0));
    if glyph_index >= rows || mark_class >= class_count {
        return false;
    }
    let Some(glyph_anchor) = ot::sub(m, 0, 2 + (glyph_index * class_count + mark_class) as usize * 2) else { return false };
    let idx = c.buffer.idx;
    c.buffer.unsafe_to_break(glyph_pos, idx + 1);
    let (mark_x, mark_y) = anchor(c, mark_anchor, c.buffer.cur(0).codepoint);
    let (base_x, base_y) = anchor(c, Some(glyph_anchor), c.buffer.info[glyph_pos].codepoint);
    let o = &mut c.buffer.pos[idx];
    o.x_offset = roundf(base_x - mark_x);
    o.y_offset = roundf(base_y - mark_y);
    o.attach_type = ATTACH_TYPE_MARK;
    o.attach_chain = (glyph_pos as isize - idx as isize) as i16;
    c.buffer.scratch_flags |= scratch::HAS_GPOS_ATTACHMENT;
    c.buffer.idx += 1;
    true
}

/// `MarkBasePosFormat1::accept`.
fn accept(b: &Buffer, idx: usize) -> bool {
    let i = &b.info;
    !i[idx].multiplied()
        || i[idx].lig_comp() == 0
        || idx == 0
        || i[idx - 1].is_mark()
        || !i[idx - 1].multiplied()
        || i[idx].lig_id() != i[idx - 1].lig_id()
        || i[idx].lig_comp() != i[idx - 1].lig_comp() + 1
}

/// A busca para trás por base (ou ligadura) com o cache `last_base`.
fn find_base(c: &mut ApplyContext, base_cov: Option<&[u8]>, check_accept: bool) -> bool {
    let mut it = c.iter(false);
    it.set_lookup_props(lookup_flag::IGNORE_MARKS);
    let idx = c.buffer.idx;
    if c.last_base_until as usize > idx {
        c.last_base_until = 0;
        c.last_base = -1;
    }
    let mut j = idx;
    while j > c.last_base_until as usize {
        let mut m = it.match_info(c, &c.buffer.info[j - 1]);
        if m == IterMatch::Match && check_accept && !accept(c.buffer, j - 1) && ot::coverage(base_cov, c.buffer.info[j - 1].codepoint) == NOT_COVERED {
            m = IterMatch::Skip;
        }
        if m == IterMatch::Match {
            c.last_base = j as i32 - 1;
            break;
        }
        j -= 1;
    }
    c.last_base_until = idx as u32;
    true
}

fn mark_base(c: &mut ApplyContext, d: &[u8]) -> bool {
    if u16at(d, 0) != 1 {
        return false;
    }
    let mark_index = ot::coverage(ot::sub(d, 0, 2), c.buffer.cur(0).codepoint);
    if mark_index == NOT_COVERED {
        return false;
    }
    let base_cov = ot::sub(d, 0, 4);
    find_base(c, base_cov, true);
    let idx = c.buffer.idx;
    if c.last_base == -1 {
        c.buffer.unsafe_to_concat_from_outbuffer(0, idx + 1);
        return false;
    }
    let b = c.last_base as usize;
    let base_index = ot::coverage(base_cov, c.buffer.info[b].codepoint);
    if base_index == NOT_COVERED {
        c.buffer.unsafe_to_concat_from_outbuffer(b, idx + 1);
        return false;
    }
    let class_count = u32::from(u16at(d, 6));
    let Some(marks) = ot::sub(d, 0, 8) else { return false };
    mark_array_apply(c, marks, mark_index, base_index, ot::sub(d, 0, 10), class_count, b)
}

fn mark_lig(c: &mut ApplyContext, d: &[u8]) -> bool {
    if u16at(d, 0) != 1 {
        return false;
    }
    let mark_index = ot::coverage(ot::sub(d, 0, 2), c.buffer.cur(0).codepoint);
    if mark_index == NOT_COVERED {
        return false;
    }
    find_base(c, None, false);
    let idx = c.buffer.idx;
    if c.last_base == -1 {
        c.buffer.unsafe_to_concat_from_outbuffer(0, idx + 1);
        return false;
    }
    let b = c.last_base as usize;
    let lig_index = ot::coverage(ot::sub(d, 0, 4), c.buffer.info[b].codepoint);
    if lig_index == NOT_COVERED {
        c.buffer.unsafe_to_concat_from_outbuffer(b, idx + 1);
        return false;
    }
    let Some(lig_array) = ot::sub(d, 0, 10) else { return false };
    if lig_index >= u32::from(u16at(lig_array, 0)) {
        return false;
    }
    let lig_attach = ot::sub(lig_array, 0, 2 + lig_index as usize * 2);
    let comp_count = lig_attach.map_or(0, |a| u32::from(u16at(a, 0)));
    if comp_count == 0 {
        c.buffer.unsafe_to_concat_from_outbuffer(b, idx + 1);
        return false;
    }
    let lig_id = c.buffer.info[b].lig_id();
    let mark_id = c.buffer.cur(0).lig_id();
    let mark_comp = c.buffer.cur(0).lig_comp();
    let comp_index = if lig_id != 0 && lig_id == mark_id && mark_comp > 0 { comp_count.min(mark_comp) - 1 } else { comp_count - 1 };
    let class_count = u32::from(u16at(d, 6));
    let Some(marks) = ot::sub(d, 0, 8) else { return false };
    mark_array_apply(c, marks, mark_index, comp_index, lig_attach, class_count, b)
}

fn mark_mark(c: &mut ApplyContext, d: &[u8]) -> bool {
    if u16at(d, 0) != 1 {
        return false;
    }
    let mark1_index = ot::coverage(ot::sub(d, 0, 2), c.buffer.cur(0).codepoint);
    if mark1_index == NOT_COVERED {
        return false;
    }
    let mut it = c.iter(false);
    it.reset_fast(c.buffer.idx);
    it.set_lookup_props(c.lookup_props & !lookup_flag::IGNORE_FLAGS);
    let mut unsafe_from = 0;
    let idx = c.buffer.idx;
    if !it.prev(c, Some(&mut unsafe_from)) {
        c.buffer.unsafe_to_concat_from_outbuffer(unsafe_from, idx + 1);
        return false;
    }
    let j = it.idx;
    if !c.buffer.info[j].is_mark() {
        c.buffer.unsafe_to_concat_from_outbuffer(j, idx + 1);
        return false;
    }
    let id1 = c.buffer.cur(0).lig_id();
    let id2 = c.buffer.info[j].lig_id();
    let comp1 = c.buffer.cur(0).lig_comp();
    let comp2 = c.buffer.info[j].lig_comp();
    let good = if id1 == id2 { id1 == 0 || comp1 == comp2 } else { (id1 > 0 && comp1 == 0) || (id2 > 0 && comp2 == 0) };
    if !good {
        c.buffer.unsafe_to_concat_from_outbuffer(j, idx + 1);
        return false;
    }
    let mark2_index = ot::coverage(ot::sub(d, 0, 4), c.buffer.info[j].codepoint);
    if mark2_index == NOT_COVERED {
        c.buffer.unsafe_to_concat_from_outbuffer(j, idx + 1);
        return false;
    }
    let class_count = u32::from(u16at(d, 6));
    let Some(marks) = ot::sub(d, 0, 8) else { return false };
    mark_array_apply(c, marks, mark1_index, mark2_index, ot::sub(d, 0, 10), class_count, j)
}

/// `GPOS::position_start`.
pub fn position_start(buffer: &mut Buffer) {
    for p in &mut buffer.pos {
        p.attach_chain = 0;
        p.attach_type = 0;
    }
}

fn propagate_attachment_offsets(pos: &mut [GlyphPosition], i: usize, direction: Direction, nesting: u32) {
    let (chain, ty) = (pos[i].attach_chain, pos[i].attach_type);
    if chain == 0 {
        return;
    }
    pos[i].attach_chain = 0;
    let j = (i as isize + chain as isize) as usize;
    if j >= pos.len() || nesting == 0 {
        return;
    }
    propagate_attachment_offsets(pos, j, direction, nesting - 1);
    if ty & ATTACH_TYPE_CURSIVE != 0 {
        if direction.is_horizontal() {
            pos[i].y_offset += pos[j].y_offset;
        } else {
            pos[i].x_offset += pos[j].x_offset;
        }
    } else {
        pos[i].x_offset += pos[j].x_offset;
        pos[i].y_offset += pos[j].y_offset;
        if direction.is_forward() {
            for k in j..i {
                pos[i].x_offset -= pos[k].x_advance;
                pos[i].y_offset -= pos[k].y_advance;
            }
        } else {
            for k in j + 1..i + 1 {
                pos[i].x_offset += pos[k].x_advance;
                pos[i].y_offset += pos[k].y_advance;
            }
        }
    }
}

/// `GPOS::position_finish_offsets`.
pub fn position_finish_offsets(buffer: &mut Buffer) {
    let direction = buffer.props.direction;
    if buffer.scratch_flags & scratch::HAS_GPOS_ATTACHMENT != 0 {
        for i in 0..buffer.pos.len() {
            propagate_attachment_offsets(&mut buffer.pos, i, direction, gsubgpos::MAX_NESTING_LEVEL);
        }
    }
}
