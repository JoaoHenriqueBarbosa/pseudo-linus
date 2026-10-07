//! Normalização Unicode do shaper (`hb-ot-shape-normalize.cc`): decompõe o que a fonte não
//! cobre, reordena as marcas pela classe de combinação e recompõe o que a fonte cobre.

use crate::buffer::{scratch, Buffer, GlyphInfo};
use crate::font::Font;
use crate::props::set_unicode_props;
use crate::unicode;

/// `HB_OT_SHAPE_MAX_COMBINING_MARKS`.
const MAX_COMBINING_MARKS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    None,
    Decomposed,
    ComposedDiacritics,
    ComposedDiacriticsNoShortCircuit,
    Auto,
}

/// Os ganchos de um shaper sobre a normalização.
pub struct Hooks<'h> {
    pub decompose: Option<&'h dyn Fn(u32) -> Option<(u32, u32)>>,
    pub compose: Option<&'h dyn Fn(u32, u32) -> Option<u32>>,
    pub reorder_marks: Option<&'h dyn Fn(&mut Buffer, usize, usize)>,
}

/// `hb_unicode_funcs_t::decompose`: começa com `a = ab, b = 0`.
fn decompose_unicode(ab: u32) -> Option<(u32, u32)> {
    unicode::decompose(ab)
}

/// `hb_unicode_funcs_t::compose`: falha com qualquer um dos dois nulo.
fn compose_unicode(a: u32, b: u32) -> Option<u32> {
    if a == 0 || b == 0 {
        return None;
    }
    unicode::compose(a, b)
}

struct Ctx<'c, 'f> {
    font: &'c Font<'f>,
    decompose: &'c dyn Fn(u32) -> Option<(u32, u32)>,
    compose: &'c dyn Fn(u32, u32) -> Option<u32>,
}

fn info_cc(i: &GlyphInfo) -> u8 {
    i.modified_combining_class()
}

/// `font->get_nominal_glyph (u, &glyph)`: o glifo vira 0 quando falta.
fn set_glyph(info: &mut GlyphInfo, font: &Font) {
    info.glyph_index = font.nominal_glyph(info.codepoint).unwrap_or(0);
}

fn output_char(buffer: &mut Buffer, unichar: u32, glyph: u32) {
    buffer.cur_mut(0).glyph_index = glyph;
    buffer.output_glyph(unichar);
    let mut f = buffer.scratch_flags;
    set_unicode_props(buffer.prev_mut(), &mut f);
    buffer.scratch_flags = f;
}

fn next_char(buffer: &mut Buffer, glyph: u32) {
    buffer.cur_mut(0).glyph_index = glyph;
    buffer.next_glyph();
}

fn decompose(c: &Ctx, buffer: &mut Buffer, shortest: bool, ab: u32) -> u32 {
    let Some((a, b)) = (c.decompose)(ab) else { return 0 };
    let mut b_glyph = 0;
    if b != 0 {
        match c.font.nominal_glyph(b) {
            Some(g) => b_glyph = g,
            None => return 0,
        }
    }
    let a_glyph = c.font.nominal_glyph(a);
    if shortest {
        if let Some(ag) = a_glyph {
            output_char(buffer, a, ag);
            if b != 0 {
                output_char(buffer, b, b_glyph);
                return 2;
            }
            return 1;
        }
    }
    let ret = decompose(c, buffer, shortest, a);
    if ret != 0 {
        if b != 0 {
            output_char(buffer, b, b_glyph);
            return ret + 1;
        }
        return ret;
    }
    if let Some(ag) = a_glyph {
        output_char(buffer, a, ag);
        if b != 0 {
            output_char(buffer, b, b_glyph);
            return 2;
        }
        return 1;
    }
    0
}

fn decompose_current_character(c: &Ctx, buffer: &mut Buffer, shortest: bool) {
    let u = buffer.cur(0).codepoint;
    let nominal = c.font.nominal_glyph(u);
    let glyph = nominal.unwrap_or(buffer.not_found);
    if shortest && nominal.is_some() {
        next_char(buffer, glyph);
        return;
    }
    if decompose(c, buffer, shortest, u) != 0 {
        buffer.skip_glyph();
        return;
    }
    if !shortest && nominal.is_some() {
        next_char(buffer, glyph);
        return;
    }
    if buffer.cur(0).is_unicode_space() {
        let space_type = unicode::space_fallback_type(u);
        if space_type != unicode::space::NOT_SPACE {
            let space_glyph = c.font.nominal_glyph(0x20).unwrap_or(buffer.invisible);
            if space_glyph != 0 {
                buffer.cur_mut(0).set_unicode_space_fallback_type(space_type);
                next_char(buffer, space_glyph);
                buffer.scratch_flags |= scratch::HAS_SPACE_FALLBACK;
                return;
            }
        }
    }
    if u == 0x2011 {
        if let Some(other) = c.font.nominal_glyph(0x2010) {
            next_char(buffer, other);
            return;
        }
    }
    next_char(buffer, glyph);
}

fn handle_variation_selector_cluster(c: &Ctx, buffer: &mut Buffer, end: usize) {
    while buffer.idx + 1 < end && buffer.successful {
        if unicode::is_variation_selector(buffer.cur(1).codepoint) {
            let (u, vs) = (buffer.cur(0).codepoint, buffer.cur(1).codepoint);
            if let Some(g) = c.font.variation_glyph(u, vs) {
                buffer.cur_mut(0).glyph_index = g;
                buffer.replace_glyphs(2, &[u]);
            } else {
                set_glyph(buffer.cur_mut(0), c.font);
                buffer.next_glyph();
                buffer.scratch_flags |= scratch::HAS_VARIATION_SELECTOR_FALLBACK;
                buffer.cur_mut(0).set_variation_selector(true);
                if buffer.not_found_variation_selector.is_some() {
                    buffer.cur_mut(0).clear_default_ignorable();
                }
                set_glyph(buffer.cur_mut(0), c.font);
                buffer.next_glyph();
            }
            while buffer.idx < end && buffer.successful && unicode::is_variation_selector(buffer.cur(0).codepoint) {
                set_glyph(buffer.cur_mut(0), c.font);
                buffer.next_glyph();
            }
        } else {
            set_glyph(buffer.cur_mut(0), c.font);
            buffer.next_glyph();
        }
    }
    if buffer.idx < end {
        set_glyph(buffer.cur_mut(0), c.font);
        buffer.next_glyph();
    }
}

fn decompose_multi_char_cluster(c: &Ctx, buffer: &mut Buffer, end: usize, short_circuit: bool) {
    if buffer.successful && (buffer.idx..end).any(|i| unicode::is_variation_selector(buffer.info[i].codepoint)) {
        handle_variation_selector_cluster(c, buffer, end);
        return;
    }
    while buffer.idx < end && buffer.successful {
        decompose_current_character(c, buffer, short_circuit);
    }
}

/// `_hb_ot_shape_normalize`. `mode` já vem resolvido do `AUTO`.
pub fn normalize(buffer: &mut Buffer, font: &Font, mode: Mode, hooks: &Hooks) {
    if buffer.is_empty() {
        return;
    }
    let mode = if mode == Mode::Auto { Mode::ComposedDiacritics } else { mode };
    let c = Ctx {
        font,
        decompose: hooks.decompose.unwrap_or(&decompose_unicode),
        compose: hooks.compose.unwrap_or(&compose_unicode),
    };
    let always_short_circuit = mode == Mode::None;
    let might_short_circuit =
        always_short_circuit || (mode != Mode::Decomposed && mode != Mode::ComposedDiacriticsNoShortCircuit);

    // Primeira volta: decompor.
    let mut all_simple = true;
    buffer.clear_output();
    let count = buffer.len();
    buffer.idx = 0;
    loop {
        let mut end = buffer.idx + 1;
        while end < count && !buffer.info[end].is_unicode_mark() {
            end += 1;
        }
        if end < count {
            end -= 1;
        }
        if might_short_circuit {
            // `get_nominal_glyphs`: para no primeiro que falta.
            let mut done = 0;
            while buffer.idx + done < end {
                let i = buffer.idx + done;
                match font.nominal_glyph(buffer.info[i].codepoint) {
                    Some(g) => buffer.info[i].glyph_index = g,
                    None => break,
                }
                done += 1;
            }
            if !buffer.next_glyphs(done) {
                break;
            }
        }
        while buffer.idx < end && buffer.successful {
            decompose_current_character(&c, buffer, might_short_circuit);
        }
        if buffer.idx == count || !buffer.successful {
            break;
        }
        all_simple = false;
        let mut end = buffer.idx + 1;
        while end < count && buffer.info[end].is_unicode_mark() {
            end += 1;
        }
        decompose_multi_char_cluster(&c, buffer, end, always_short_circuit);
        if !(buffer.idx < count && buffer.successful) {
            break;
        }
    }
    buffer.sync();

    // Segunda volta: reordenar as marcas.
    if !all_simple {
        let count = buffer.len();
        let mut i = 0;
        while i < count {
            if info_cc(&buffer.info[i]) == 0 {
                i += 1;
                continue;
            }
            let mut end = i + 1;
            while end < count && info_cc(&buffer.info[end]) != 0 {
                end += 1;
            }
            if end - i <= MAX_COMBINING_MARKS {
                buffer.sort(i, end, |a, b| info_cc(a).cmp(&info_cc(b)));
                if let Some(r) = hooks.reorder_marks {
                    r(buffer, i, end);
                }
            }
            i = end + 1;
        }
    }
    if buffer.scratch_flags & scratch::HAS_CGJ != 0 {
        let count = buffer.len();
        for i in 1..count.saturating_sub(1) {
            let info = &buffer.info;
            if info[i].codepoint == 0x034F && (info_cc(&info[i + 1]) == 0 || info_cc(&info[i - 1]) <= info_cc(&info[i + 1])) {
                buffer.info[i].unhide();
            }
        }
    }

    // Terceira volta: recompor.
    if !all_simple
        && buffer.successful
        && matches!(mode, Mode::ComposedDiacritics | Mode::ComposedDiacriticsNoShortCircuit)
    {
        buffer.clear_output();
        let count = buffer.len();
        let mut starter = 0;
        buffer.next_glyph();
        while buffer.idx < count {
            if buffer.cur(0).is_unicode_mark()
                && (starter == buffer.out_len() - 1 || info_cc(buffer.prev()) < info_cc(buffer.cur(0)))
            {
                if let Some(composed) = (c.compose)(buffer.out_info[starter].codepoint, buffer.cur(0).codepoint) {
                    if let Some(glyph) = font.nominal_glyph(composed) {
                        if !buffer.next_glyph() {
                            break;
                        }
                        let out_len = buffer.out_len();
                        buffer.merge_out_clusters(starter, out_len);
                        buffer.out_info.pop();
                        buffer.out_info[starter].codepoint = composed;
                        buffer.out_info[starter].glyph_index = glyph;
                        let mut f = buffer.scratch_flags;
                        set_unicode_props(&mut buffer.out_info[starter], &mut f);
                        buffer.scratch_flags = f;
                        continue;
                    }
                }
            }
            if !buffer.next_glyph() {
                break;
            }
            if info_cc(buffer.prev()) == 0 {
                starter = buffer.out_len() - 1;
            }
        }
        buffer.sync();
    }
}
