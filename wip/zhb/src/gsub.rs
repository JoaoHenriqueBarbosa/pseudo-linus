//! As subtabelas do GSUB (`OT/Layout/GSUB/*.hh`).

use crate::gsubgpos::{self, ApplyContext, MatchFn, Values, MAX_CONTEXT_LENGTH, MAX_NESTING_LEVEL};
use crate::ot::{self, GLYPH_PROPS_BASE_GLYPH, NOT_COVERED, u16at};

const MAP_MAX_VALUE: u32 = 255;

pub fn apply_subtable<'a>(c: &mut ApplyContext<'a, '_>, t: u16, d: &'a [u8]) -> bool {
    match t {
        1 => single(c, d),
        2 => multiple(c, d),
        3 => alternate(c, d),
        4 => ligature(c, d),
        5 => gsubgpos::context_apply(c, d),
        6 => gsubgpos::chain_context_apply(c, d),
        8 => reverse_chain(c, d),
        _ => false,
    }
}

fn cov(c: &ApplyContext, d: &[u8]) -> u32 {
    ot::coverage(ot::sub(d, 0, 2), c.buffer.cur(0).codepoint)
}

fn single(c: &mut ApplyContext, d: &[u8]) -> bool {
    let index = cov(c, d);
    if index == NOT_COVERED {
        return false;
    }
    match u16at(d, 0) {
        1 => {
            let g = (c.buffer.cur(0).codepoint.wrapping_add(u32::from(u16at(d, 4)))) & 0xFFFF;
            c.replace_glyph(g);
            true
        }
        2 => {
            let n = u32::from(u16at(d, 4));
            if index >= n || d.len() < 6 + n as usize * 2 {
                return false;
            }
            c.replace_glyph(u32::from(u16at(d, 6 + index as usize * 2)));
            true
        }
        _ => false,
    }
}

fn multiple(c: &mut ApplyContext, d: &[u8]) -> bool {
    if u16at(d, 0) != 1 {
        return false;
    }
    let index = cov(c, d);
    if index == NOT_COVERED || index >= u32::from(u16at(d, 4)) {
        return false;
    }
    let Some(seq) = ot::sub(d, 0, 6 + index as usize * 2) else { return false };
    let count = usize::from(u16at(seq, 0));
    if seq.len() < 2 + count * 2 {
        return false;
    }
    if count == 1 {
        c.replace_glyph(u32::from(u16at(seq, 2)));
        return true;
    }
    if count == 0 {
        c.buffer.delete_glyph();
        return true;
    }
    let klass = if c.buffer.cur(0).is_ligature() { GLYPH_PROPS_BASE_GLYPH } else { 0 };
    let lig_id = c.buffer.cur(0).lig_id();
    for i in 0..count {
        if lig_id == 0 {
            c.buffer.cur_mut(0).set_lig_props_for_component(i as u32);
        }
        c.output_glyph_for_component(u32::from(u16at(seq, 2 + i * 2)), klass);
    }
    c.buffer.skip_glyph();
    true
}

fn alternate(c: &mut ApplyContext, d: &[u8]) -> bool {
    if u16at(d, 0) != 1 {
        return false;
    }
    let index = cov(c, d);
    if index == NOT_COVERED || index >= u32::from(u16at(d, 4)) {
        return false;
    }
    let Some(set) = ot::sub(d, 0, 6 + index as usize * 2) else { return false };
    let count = u32::from(u16at(set, 0));
    if count == 0 || set.len() < 2 + count as usize * 2 {
        return false;
    }
    let glyph_mask = c.buffer.cur(0).mask;
    let lookup_mask = c.lookup_mask;
    let shift = lookup_mask.trailing_zeros();
    let mut alt_index = (lookup_mask & glyph_mask) >> shift;
    if alt_index == MAP_MAX_VALUE && c.random {
        let n = c.buffer.len();
        c.buffer.unsafe_to_break(0, n);
        alt_index = c.random_number() % count + 1;
    }
    if alt_index > count || alt_index == 0 {
        return false;
    }
    c.replace_glyph(u32::from(u16at(set, 2 + (alt_index as usize - 1) * 2)));
    true
}

fn ligature<'a>(c: &mut ApplyContext<'a, '_>, d: &'a [u8]) -> bool {
    if u16at(d, 0) != 1 {
        return false;
    }
    let index = cov(c, d);
    if index == NOT_COVERED || index >= u32::from(u16at(d, 4)) {
        return false;
    }
    let Some(set) = ot::sub(d, 0, 6 + index as usize * 2) else { return false };
    let n = usize::from(u16at(set, 0));
    for i in 0..n {
        let Some(lig) = ot::sub(set, 0, 2 + i * 2) else { continue };
        if ligature_apply(c, lig) {
            return true;
        }
    }
    false
}

/// `Ligature::apply`.
fn ligature_apply<'a>(c: &mut ApplyContext<'a, '_>, lig: &'a [u8]) -> bool {
    let lig_glyph = u32::from(u16at(lig, 0));
    let count = usize::from(u16at(lig, 2));
    if count == 0 || lig.len() < 4 + (count - 1) * 2 {
        return false;
    }
    if count == 1 {
        c.replace_glyph(lig_glyph);
        return true;
    }
    if count > MAX_CONTEXT_LENGTH {
        return false;
    }
    let mut positions = vec![0usize; count];
    let mut match_end = 0;
    let mut total = 0;
    if !gsubgpos::match_input(c, count, Values { d: lig, off: 4 }, MatchFn::Glyph, &mut match_end, &mut positions, Some(&mut total)) {
        let idx = c.buffer.idx;
        c.buffer.unsafe_to_concat(idx, match_end);
        return false;
    }
    gsubgpos::ligate_input(c, count, &positions, match_end, lig_glyph, total);
    true
}

fn reverse_chain<'a>(c: &mut ApplyContext<'a, '_>, d: &'a [u8]) -> bool {
    if u16at(d, 0) != 1 {
        return false;
    }
    let index = cov(c, d);
    if index == NOT_COVERED {
        return false;
    }
    if c.nesting_level_left != MAX_NESTING_LEVEL {
        return false;
    }
    let bt = usize::from(u16at(d, 4));
    let la_at = 6 + bt * 2;
    let la = usize::from(u16at(d, la_at));
    let sub_at = la_at + 2 + la * 2;
    let n = u32::from(u16at(d, sub_at));
    if index >= n || d.len() < sub_at + 2 + n as usize * 2 {
        return false;
    }
    let mut start_index = 0;
    let mut end_index = 0;
    let idx = c.buffer.idx;
    if gsubgpos::match_backtrack(c, bt, Values { d, off: 6 }, MatchFn::Coverage(d), &mut start_index)
        && gsubgpos::match_lookahead(c, la, Values { d, off: la_at + 2 }, MatchFn::Coverage(d), idx + 1, &mut end_index)
    {
        c.buffer.unsafe_to_break_from_outbuffer(start_index, end_index);
        c.replace_glyph_inplace(u32::from(u16at(d, sub_at + 2 + index as usize * 2)));
        true
    } else {
        c.buffer.unsafe_to_concat_from_outbuffer(start_index, end_index);
        false
    }
}
