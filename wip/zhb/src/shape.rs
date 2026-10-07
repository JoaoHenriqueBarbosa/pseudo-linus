//! O shaper OpenType (`hb-ot-shape.cc`): o plano (features, máscaras e a escolha entre GPOS,
//! `kern` e o kerning de fallback) e o pipeline de `hb_ot_shape_internal`.

use crate::buffer::{
    scratch, tag, Buffer, ClusterLevel, ContentType, Direction, GlyphInfo, SegmentProperties, FLAG_BOT,
    FLAG_DO_NOT_INSERT_DOTTED_CIRCLE, FLAG_PRESERVE_DEFAULT_IGNORABLES, FLAG_PRODUCE_SAFE_TO_INSERT_TATWEEL,
    FLAG_PRODUCE_UNSAFE_TO_CONCAT, FLAG_REMOVE_DEFAULT_IGNORABLES, GLYPH_FLAG_DEFINED, GLYPH_FLAG_SAFE_TO_INSERT_TATWEEL,
    GLYPH_FLAG_UNSAFE_TO_BREAK, GLYPH_FLAG_UNSAFE_TO_CONCAT,
};
use crate::arabic::ArabicPlan;
use crate::fallback;
use crate::font::Font;
use crate::gsubgpos::{apply_string, ApplyContext};
use crate::map::{Map, MapBuilder, F_GLOBAL, F_GLOBAL_HAS_FALLBACK, F_GLOBAL_MANUAL_JOINERS, F_GLOBAL_SEARCH, F_HAS_FALLBACK, F_NONE, F_RANDOM};
use crate::normalize::{self, Hooks, Mode};
use crate::ot::{GLYPH_PROPS_BASE_GLYPH, GLYPH_PROPS_MARK};
use crate::props::set_unicode_props;
use crate::unicode::{self, gc};

/// `HB_OT_MAP_MAX_VALUE`.
const MAP_MAX_VALUE: u32 = 255;

/// Um `hb_feature_t` do usuário; `start`/`end` em clusters, `u32::MAX` como fim global.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Feature {
    pub tag: u32,
    pub value: u32,
    pub start: u32,
    pub end: u32,
}

impl Feature {
    pub const GLOBAL_START: u32 = 0;
    pub const GLOBAL_END: u32 = u32::MAX;

    fn is_global(&self) -> bool {
        self.start == Self::GLOBAL_START && self.end == Self::GLOBAL_END
    }
}

/// `hb_ot_shape_zero_width_marks_type_t`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZeroWidthMarks {
    None,
    ByGdefEarly,
    ByGdefLate,
}

/// Qual `hb_ot_shaper_t` o plano usa.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShaperKind {
    Default,
    Arabic,
    Hebrew,
}

/// `hb_ot_shaper_t`: as propriedades que o pipeline consulta.
#[derive(Clone, Copy, Debug)]
pub struct Shaper {
    pub kind: ShaperKind,
    pub normalization: Mode,
    pub zero_width_marks: ZeroWidthMarks,
    pub fallback_position: bool,
    pub gpos_tag: u32,
}

/// `_hb_ot_shaper_default`.
pub const SHAPER_DEFAULT: Shaper = Shaper {
    kind: ShaperKind::Default,
    normalization: Mode::Auto,
    zero_width_marks: ZeroWidthMarks::ByGdefLate,
    fallback_position: true,
    gpos_tag: 0,
};

/// `_hb_ot_shaper_arabic`.
pub const SHAPER_ARABIC: Shaper = Shaper { kind: ShaperKind::Arabic, ..SHAPER_DEFAULT };

/// `_hb_ot_shaper_hebrew`.
pub const SHAPER_HEBREW: Shaper = Shaper { kind: ShaperKind::Hebrew, gpos_tag: tag(b"hebr"), ..SHAPER_DEFAULT };

/// `hb_ot_shaper_categorize`. Dos scripts complexos, o árabe e o hebraico foram traduzidos.
fn categorize(script: u32, direction: Direction, gsub_script: u32) -> Shaper {
    if script == tag(b"Hebr") {
        return SHAPER_HEBREW;
    }
    if (script == tag(b"Arab") || script == tag(b"Syrc"))
        && (gsub_script != tag(b"DFLT") || script == tag(b"Arab"))
        && direction.is_horizontal()
    {
        return SHAPER_ARABIC;
    }
    SHAPER_DEFAULT
}

/// `hb_ot_shape_plan_t`.
pub struct Plan {
    pub props: SegmentProperties,
    pub shaper: Shaper,
    pub map: Map,
    /// Os dados do shaper árabe (`plan->data`).
    pub arabic: Option<ArabicPlan>,
    pub frac_mask: u32,
    pub numr_mask: u32,
    pub dnom_mask: u32,
    pub rtlm_mask: u32,
    pub kern_mask: u32,
    pub has_frac: bool,
    pub has_vert: bool,
    pub has_gpos_mark: bool,
    pub zero_marks: bool,
    pub fallback_glyph_classes: bool,
    pub fallback_mark_positioning: bool,
    pub adjust_mark_positioning_when_zeroing: bool,
    pub apply_gpos: bool,
    pub apply_kern: bool,
    pub apply_fallback_kern: bool,
    pub user_features: Vec<Feature>,
}

const COMMON_FEATURES: [(&[u8; 4], u32); 7] = [
    (b"abvm", F_GLOBAL),
    (b"blwm", F_GLOBAL),
    (b"ccmp", F_GLOBAL),
    (b"locl", F_GLOBAL),
    (b"mark", F_GLOBAL_MANUAL_JOINERS),
    (b"mkmk", F_GLOBAL_MANUAL_JOINERS),
    (b"rlig", F_GLOBAL),
];

const HORIZONTAL_FEATURES: [(&[u8; 4], u32); 7] = [
    (b"calt", F_GLOBAL),
    (b"clig", F_GLOBAL),
    (b"curs", F_GLOBAL),
    (b"dist", F_GLOBAL),
    (b"kern", F_GLOBAL_HAS_FALLBACK),
    (b"liga", F_GLOBAL),
    (b"rclt", F_GLOBAL),
];

/// `hb_ot_layout_has_machine_kerning` e `hb_ot_layout_has_cross_kerning` da tabela `kern`.
fn kern_table_traits(font: &Font) -> (bool, bool) {
    let Some(k) = font.kern.filter(|_| fallback::has_kern_table(font)) else { return (false, false) };
    let count = usize::from(crate::ot::u16at(k, 2));
    let (mut machine, mut cross) = (false, false);
    let mut off = 4;
    for _ in 0..count {
        if k.len() < off + 6 {
            break;
        }
        let length = usize::from(crate::ot::u16at(k, off + 2));
        let format = k[off + 4];
        let coverage = k[off + 5];
        machine |= format == 1;
        cross |= coverage & 0x04 != 0;
        off += length.max(6);
    }
    (machine, cross)
}

impl Plan {
    /// `hb_ot_shape_plan_t::init0`: planner, coleta de features e compilação.
    pub fn new(font: &Font, props: &SegmentProperties, user_features: &[Feature]) -> Plan {
        let mut map = MapBuilder::new(font, props);
        let shaper = categorize(props.script, props.direction, map.chosen_script(0));
        let script_zero_marks = shaper.zero_width_marks != ZeroWidthMarks::None;
        let script_fallback_mark_positioning = shaper.fallback_position;

        // `hb_ot_shape_collect_features`.
        map.is_simple = true;
        map.enable_feature(tag(b"rvrn"), F_NONE, 1);
        map.add_gsub_pause(None);
        match props.direction {
            Direction::Ltr => {
                map.enable_feature(tag(b"ltra"), F_NONE, 1);
                map.enable_feature(tag(b"ltrm"), F_NONE, 1);
            }
            Direction::Rtl => {
                map.enable_feature(tag(b"rtla"), F_NONE, 1);
                map.add_feature(tag(b"rtlm"), F_NONE, 1);
            }
            _ => {}
        }
        map.add_feature(tag(b"frac"), F_NONE, 1);
        map.add_feature(tag(b"numr"), F_NONE, 1);
        map.add_feature(tag(b"dnom"), F_NONE, 1);
        map.enable_feature(tag(b"rand"), F_RANDOM, MAP_MAX_VALUE);
        map.enable_feature(tag(b"trak"), F_HAS_FALLBACK, 1);
        map.enable_feature(tag(b"Harf"), F_NONE, 1);
        map.enable_feature(tag(b"HARF"), F_NONE, 1);
        if shaper.kind == ShaperKind::Arabic {
            map.is_simple = false;
            crate::arabic::collect_features(&mut map, props);
        }
        map.enable_feature(tag(b"Buzz"), F_NONE, 1);
        map.enable_feature(tag(b"BUZZ"), F_NONE, 1);
        for (t, f) in COMMON_FEATURES {
            map.add_feature(tag(t), f, 1);
        }
        if props.direction.is_horizontal() {
            for (t, f) in HORIZONTAL_FEATURES {
                map.add_feature(tag(t), f, 1);
            }
        } else {
            map.enable_feature(tag(b"vert"), F_GLOBAL_SEARCH, 1);
        }
        if !user_features.is_empty() {
            map.is_simple = false;
        }
        for f in user_features {
            map.add_feature(f.tag, if f.is_global() { F_GLOBAL } else { F_NONE }, f.value);
        }

        // `hb_ot_shape_planner_t::compile`.
        let map = map.compile();
        let frac_mask = map.one_mask(tag(b"frac"));
        let numr_mask = map.one_mask(tag(b"numr"));
        let dnom_mask = map.one_mask(tag(b"dnom"));
        let has_frac = frac_mask != 0 || (numr_mask != 0 && dnom_mask != 0);
        let rtlm_mask = map.one_mask(tag(b"rtlm"));
        let has_vert = map.one_mask(tag(b"vert")) != 0;
        let kern_tag = if props.direction.is_horizontal() { tag(b"kern") } else { tag(b"vkrn") };
        let kern_mask = map.mask(kern_tag).0;
        let has_gpos_kern = map.feature_index(1, kern_tag) != crate::map::NO_FEATURE_INDEX;
        let disable_gpos = shaper.gpos_tag != 0 && shaper.gpos_tag != map.chosen_script[1];
        let fallback_glyph_classes = !font.gdef.has_glyph_classes();
        let has_gpos = !disable_gpos && font.gpos.has_data();
        let apply_gpos = has_gpos;
        let mut apply_kern = false;
        if !has_gpos_kern || !apply_gpos {
            apply_kern = fallback::has_kern_table(font);
        }
        let apply_fallback_kern = !(apply_gpos || apply_kern);
        let (machine_kerning, cross_kerning) = kern_table_traits(font);
        let zero_marks = script_zero_marks && (!apply_kern || !machine_kerning);
        let has_gpos_mark = map.one_mask(tag(b"mark")) != 0;
        let adjust_mark_positioning_when_zeroing = !apply_gpos && (!apply_kern || !cross_kerning);
        let fallback_mark_positioning = adjust_mark_positioning_when_zeroing && script_fallback_mark_positioning;
        let arabic = (shaper.kind == ShaperKind::Arabic).then(|| ArabicPlan::new(&map, props, font));
        Plan {
            props: props.clone(),
            shaper,
            map,
            arabic,
            frac_mask,
            numr_mask,
            dnom_mask,
            rtlm_mask,
            kern_mask,
            has_frac,
            has_vert,
            has_gpos_mark,
            zero_marks,
            fallback_glyph_classes,
            fallback_mark_positioning,
            adjust_mark_positioning_when_zeroing,
            apply_gpos,
            apply_kern,
            apply_fallback_kern,
            user_features: user_features.to_vec(),
        }
    }

    /// `hb_ot_map_t::apply` com o GSUB (0) ou o GPOS (1).
    fn apply_map(&self, table_index: usize, font: &Font, buffer: &mut Buffer) {
        let table = if table_index == 0 { font.gsub } else { font.gpos };
        let mut c = ApplyContext::new(table_index, font, buffer);
        let mut i = 0;
        for stage in &self.map.stages[table_index] {
            while i < stage.last_lookup {
                let lookup = self.map.lookups[table_index][i];
                i += 1;
                let Some(l) = table.lookup(usize::from(lookup.index)) else { continue };
                c.lookup_index = u32::from(lookup.index);
                c.set_lookup_mask(lookup.mask);
                c.auto_zwj = lookup.auto_zwj;
                c.auto_zwnj = lookup.auto_zwnj;
                c.random = lookup.random;
                c.per_syllable = lookup.per_syllable;
                apply_string(&mut c, l);
            }
            if let (Some(p), Some(arabic)) = (stage.pause, &self.arabic) {
                crate::arabic::pause(p, arabic, font, &mut *c.buffer);
            }
        }
    }

    fn position(&self, font: &Font, buffer: &mut Buffer) {
        if self.apply_gpos {
            self.apply_map(1, font, buffer);
        }
        if self.apply_kern {
            fallback::apply_kern_table(font, buffer, self.kern_mask);
        } else if self.apply_fallback_kern {
            fallback::fallback_kern(font, buffer, self.kern_mask);
        }
    }
}

fn is_regional_indicator(u: u32) -> bool {
    (0x1F1E6..=0x1F1FF).contains(&u)
}

fn grapheme_group(_a: &GlyphInfo, b: &GlyphInfo) -> bool {
    b.is_continuation()
}

/// `hb_set_unicode_props`.
fn set_unicode_props_all(buffer: &mut Buffer) {
    let count = buffer.len();
    let mut flags = buffer.scratch_flags;
    let info = &mut buffer.info;
    let mut i = 0;
    while i < count {
        set_unicode_props(&mut info[i], &mut flags);
        let g = info[i].general_category();
        let u = info[i].codepoint;
        let letters = gc::flag(gc::LOWERCASE_LETTER)
            | gc::flag(gc::UPPERCASE_LETTER)
            | gc::flag(gc::TITLECASE_LETTER)
            | gc::flag(gc::OTHER_LETTER)
            | gc::flag(gc::SPACE_SEPARATOR);
        if gc::flag(g) & letters != 0 {
            i += 1;
            continue;
        }
        if g == gc::MODIFIER_SYMBOL && (0x1F3FB..=0x1F3FF).contains(&u) {
            info[i].set_continuation();
        } else if i > 0 && is_regional_indicator(u) {
            if is_regional_indicator(info[i - 1].codepoint) && !info[i - 1].is_continuation() {
                info[i].set_continuation();
            }
        } else if info[i].is_zwj() {
            info[i].set_continuation();
            if i + 1 < count && unicode::is_extended_pictographic(info[i + 1].codepoint) {
                i += 1;
                set_unicode_props(&mut info[i], &mut flags);
                info[i].set_continuation();
            }
        } else if (0xFF9E..=0xFF9F).contains(&u) || (0xE0020..=0xE007F).contains(&u) {
            info[i].set_continuation();
        }
        i += 1;
    }
    buffer.scratch_flags = flags;
}

/// `hb_insert_dotted_circle`.
fn insert_dotted_circle(buffer: &mut Buffer, font: &Font) {
    if buffer.flags & FLAG_DO_NOT_INSERT_DOTTED_CIRCLE != 0 {
        return;
    }
    if buffer.flags & FLAG_BOT == 0 || buffer.context_len[0] != 0 || !buffer.info[0].is_unicode_mark() {
        return;
    }
    if font.nominal_glyph(0x25CC).is_none() {
        return;
    }
    let mut dotted = GlyphInfo { codepoint: 0x25CC, ..Default::default() };
    let mut flags = buffer.scratch_flags;
    set_unicode_props(&mut dotted, &mut flags);
    buffer.scratch_flags = flags;
    buffer.clear_output();
    buffer.idx = 0;
    dotted.cluster = buffer.cur(0).cluster;
    dotted.mask = buffer.cur(0).mask;
    buffer.output_info(dotted);
    buffer.sync();
}

/// `hb_form_clusters`.
fn form_clusters(buffer: &mut Buffer) {
    if buffer.scratch_flags & scratch::HAS_NON_ASCII == 0 {
        return;
    }
    let mut start = 0;
    while start < buffer.len() {
        let end = buffer.group_end(start, grapheme_group);
        if buffer.cluster_level == ClusterLevel::MonotoneGraphemes {
            buffer.merge_clusters(start, end);
        } else {
            buffer.unsafe_to_break(start, end);
        }
        start = end;
    }
}

/// `hb_ensure_native_direction`.
fn ensure_native_direction(buffer: &mut Buffer) {
    let direction = buffer.props.direction;
    let mut horiz_dir = crate::buffer::script_horizontal_direction(buffer.props.script);
    if horiz_dir == Direction::Rtl && direction == Direction::Ltr {
        let (mut found_number, mut found_letter, mut found_ri) = (false, false, false);
        for info in &buffer.info {
            let g = info.general_category();
            if g == gc::DECIMAL_NUMBER {
                found_number = true;
            } else if gc::is_letter(g) {
                found_letter = true;
                break;
            } else if is_regional_indicator(info.codepoint) {
                found_ri = true;
            }
        }
        if (found_number || found_ri) && !found_letter {
            horiz_dir = Direction::Ltr;
        }
    }
    if (direction.is_horizontal() && direction != horiz_dir && horiz_dir != Direction::Invalid)
        || (direction.is_vertical() && direction != Direction::Ttb)
    {
        let merge = buffer.cluster_level == ClusterLevel::MonotoneCharacters;
        buffer.reverse_groups(grapheme_group, merge);
        buffer.props.direction = buffer.props.direction.reverse();
    }
}

/// `hb_vert_char_for`.
fn vert_char_for(u: u32) -> u32 {
    match u {
        0x2013 => 0xFE32,
        0x2014 => 0xFE31,
        0x2025 => 0xFE30,
        0x2026 => 0xFE19,
        0x3001 => 0xFE11,
        0x3002 => 0xFE12,
        0x3008 => 0xFE3F,
        0x3009 => 0xFE40,
        0x300A => 0xFE3D,
        0x300B => 0xFE3E,
        0x300C => 0xFE41,
        0x300D => 0xFE42,
        0x300E => 0xFE43,
        0x300F => 0xFE44,
        0x3010 => 0xFE3B,
        0x3011 => 0xFE3C,
        0x3014 => 0xFE39,
        0x3015 => 0xFE3A,
        0x3016 => 0xFE17,
        0x3017 => 0xFE18,
        0xFE4F => 0xFE34,
        0xFF01 => 0xFE15,
        0xFF08 => 0xFE35,
        0xFF09 => 0xFE36,
        0xFF0C => 0xFE10,
        0xFF1A => 0xFE13,
        0xFF1B => 0xFE14,
        0xFF1F => 0xFE16,
        0xFF3B => 0xFE47,
        0xFF3D => 0xFE48,
        0xFF3F => 0xFE33,
        0xFF5B => 0xFE37,
        0xFF5D => 0xFE38,
        _ => u,
    }
}

/// `hb_ot_rotate_chars`.
fn rotate_chars(plan: &Plan, font: &Font, buffer: &mut Buffer, target_direction: Direction) {
    if target_direction.is_backward() {
        for info in &mut buffer.info {
            let m = unicode::mirroring(info.codepoint);
            if m != info.codepoint && font.nominal_glyph(m).is_some() {
                info.codepoint = m;
            } else {
                info.mask |= plan.rtlm_mask;
            }
        }
    }
    if target_direction.is_vertical() && !plan.has_vert {
        for info in &mut buffer.info {
            let v = vert_char_for(info.codepoint);
            if v != info.codepoint && font.nominal_glyph(v).is_some() {
                info.codepoint = v;
            }
        }
    }
}

/// `hb_ot_shape_setup_masks_fraction`.
fn setup_masks_fraction(plan: &Plan, buffer: &mut Buffer) {
    if buffer.scratch_flags & scratch::HAS_NON_ASCII == 0 || !plan.has_frac {
        return;
    }
    let (pre_mask, post_mask) = if buffer.props.direction.is_forward() {
        (plan.numr_mask | plan.frac_mask, plan.frac_mask | plan.dnom_mask)
    } else {
        (plan.frac_mask | plan.dnom_mask, plan.numr_mask | plan.frac_mask)
    };
    let count = buffer.len();
    let mut i = 0;
    while i < count {
        if buffer.info[i].codepoint == 0x2044 {
            let (mut start, mut end) = (i, i + 1);
            while start > 0 && buffer.info[start - 1].general_category() == gc::DECIMAL_NUMBER {
                start -= 1;
            }
            while end < count && buffer.info[end].general_category() == gc::DECIMAL_NUMBER {
                end += 1;
            }
            if start == i || end == i + 1 {
                if start == i {
                    buffer.unsafe_to_concat(start, start + 1);
                }
                if end == i + 1 {
                    buffer.unsafe_to_concat(end - 1, end);
                }
                i += 1;
                continue;
            }
            buffer.unsafe_to_break(start, end);
            for j in start..i {
                buffer.info[j].mask |= pre_mask;
            }
            buffer.info[i].mask |= plan.frac_mask;
            for j in i + 1..end {
                buffer.info[j].mask |= post_mask;
            }
            i = end;
            continue;
        }
        i += 1;
    }
}

/// `hb_ot_shape_setup_masks`.
fn setup_masks(plan: &Plan, buffer: &mut Buffer) {
    setup_masks_fraction(plan, buffer);
    if let Some(arabic) = &plan.arabic {
        crate::arabic::setup_masks(arabic, buffer, plan.props.script);
    }
    for f in &plan.user_features {
        if !f.is_global() {
            let (mask, shift) = plan.map.mask(f.tag);
            buffer.set_masks(f.value << shift, mask, f.start, f.end);
        }
    }
}

/// `hb_ot_zero_width_default_ignorables`.
fn zero_width_default_ignorables(buffer: &mut Buffer) {
    if buffer.scratch_flags & scratch::HAS_DEFAULT_IGNORABLES == 0
        || buffer.flags & (FLAG_PRESERVE_DEFAULT_IGNORABLES | FLAG_REMOVE_DEFAULT_IGNORABLES) != 0
    {
        return;
    }
    for i in 0..buffer.len() {
        if buffer.info[i].is_default_ignorable() {
            let p = &mut buffer.pos[i];
            p.x_advance = 0;
            p.y_advance = 0;
            p.x_offset = 0;
            p.y_offset = 0;
        }
    }
}

/// `hb_ot_deal_with_variation_selectors`.
fn deal_with_variation_selectors(buffer: &mut Buffer) {
    let Some(nf) = buffer.not_found_variation_selector else { return };
    if buffer.scratch_flags & scratch::HAS_VARIATION_SELECTOR_FALLBACK == 0 {
        return;
    }
    for i in 0..buffer.len() {
        if buffer.info[i].is_variation_selector() {
            buffer.info[i].codepoint = nf;
            let p = &mut buffer.pos[i];
            p.x_advance = 0;
            p.y_advance = 0;
            p.x_offset = 0;
            p.y_offset = 0;
            buffer.info[i].set_variation_selector(false);
        }
    }
}

/// `hb_ot_hide_default_ignorables`.
fn hide_default_ignorables(buffer: &mut Buffer, font: &Font) {
    if buffer.scratch_flags & scratch::HAS_DEFAULT_IGNORABLES == 0 || buffer.flags & FLAG_PRESERVE_DEFAULT_IGNORABLES != 0 {
        return;
    }
    let invisible = if buffer.invisible != 0 { Some(buffer.invisible) } else { font.nominal_glyph(0x20) };
    match invisible {
        Some(inv) if buffer.flags & FLAG_REMOVE_DEFAULT_IGNORABLES == 0 => {
            for info in &mut buffer.info {
                if info.is_default_ignorable() {
                    info.codepoint = inv;
                }
            }
        }
        _ => buffer.delete_glyphs_inplace(|i| i.is_default_ignorable()),
    }
}

/// `hb_synthesize_glyph_classes`.
fn synthesize_glyph_classes(buffer: &mut Buffer) {
    for info in &mut buffer.info {
        info.glyph_props = if info.general_category() != gc::NON_SPACING_MARK || info.is_default_ignorable() {
            GLYPH_PROPS_BASE_GLYPH
        } else {
            GLYPH_PROPS_MARK
        };
    }
}

fn zero_mark_widths_by_gdef(buffer: &mut Buffer, adjust_offsets: bool) {
    for i in 0..buffer.len() {
        if buffer.info[i].is_mark() {
            let p = &mut buffer.pos[i];
            if adjust_offsets {
                p.x_offset -= p.x_advance;
                p.y_offset -= p.y_advance;
            }
            p.x_advance = 0;
            p.y_advance = 0;
        }
    }
}

fn substitute_pre(plan: &Plan, font: &Font, buffer: &mut Buffer, target_direction: Direction) {
    // `hb_ot_substitute_default`.
    rotate_chars(plan, font, buffer, target_direction);
    let arabic_reorder = |b: &mut Buffer, s: usize, e: usize| crate::arabic::reorder_marks(b, s, e);
    let hebrew_reorder = |b: &mut Buffer, s: usize, e: usize| crate::hebrew::reorder_marks(b, s, e);
    let has_gpos_mark = plan.has_gpos_mark;
    let hebrew_compose = move |a: u32, b: u32| crate::hebrew::compose(a, b, has_gpos_mark);
    let (reorder_marks, compose): (Option<&dyn Fn(&mut Buffer, usize, usize)>, Option<&dyn Fn(u32, u32) -> Option<u32>>) =
        match plan.shaper.kind {
            ShaperKind::Arabic => (Some(&arabic_reorder), None),
            ShaperKind::Hebrew => (Some(&hebrew_reorder), Some(&hebrew_compose)),
            ShaperKind::Default => (None, None),
        };
    let hooks = Hooks { decompose: None, compose, reorder_marks };
    normalize::normalize(buffer, font, plan.shaper.normalization, &hooks);
    setup_masks(plan, buffer);
    if plan.fallback_mark_positioning {
        fallback::recategorize_marks(buffer);
    }
    for info in &mut buffer.info {
        info.codepoint = info.glyph_index;
    }
    buffer.content_type = ContentType::Glyphs;

    // `hb_ot_substitute_plan`.
    for info in &mut buffer.info {
        info.glyph_props = font.gdef.glyph_props(info.codepoint);
        info.clear_lig_props();
    }
    if plan.fallback_glyph_classes {
        synthesize_glyph_classes(buffer);
    }
    plan.apply_map(0, font, buffer);
}

fn position(plan: &Plan, font: &Font, buffer: &mut Buffer) {
    buffer.clear_positions();
    // `hb_ot_position_default`.
    let count = buffer.len();
    if buffer.props.direction.is_horizontal() {
        for i in 0..count {
            buffer.pos[i].x_advance = font.h_advance(buffer.info[i].codepoint);
        }
    } else {
        for i in 0..count {
            let g = buffer.info[i].codepoint;
            buffer.pos[i].y_advance = font.v_advance(g);
            let (ox, oy) = font.v_origin(g);
            buffer.pos[i].x_offset -= ox;
            buffer.pos[i].y_offset -= oy;
        }
    }
    if buffer.scratch_flags & scratch::HAS_SPACE_FALLBACK != 0 {
        fallback::fallback_spaces(font, buffer);
    }

    // `hb_ot_position_plan`.
    let adjust = plan.adjust_mark_positioning_when_zeroing && buffer.props.direction.is_forward();
    crate::gpos::position_start(buffer);
    if plan.zero_marks && plan.shaper.zero_width_marks == ZeroWidthMarks::ByGdefEarly {
        zero_mark_widths_by_gdef(buffer, adjust);
    }
    plan.position(font, buffer);
    if plan.zero_marks && plan.shaper.zero_width_marks == ZeroWidthMarks::ByGdefLate {
        zero_mark_widths_by_gdef(buffer, adjust);
    }
    zero_width_default_ignorables(buffer);
    crate::gpos::position_finish_offsets(buffer);
    if plan.fallback_mark_positioning {
        fallback::mark_position(font, plan.props.direction, plan.props.script, buffer, adjust);
    }
    if buffer.props.direction.is_backward() {
        buffer.reverse();
    }
}

/// `hb_propagate_flags`.
fn propagate_flags(buffer: &mut Buffer) {
    if buffer.scratch_flags & scratch::HAS_GLYPH_FLAGS == 0 {
        return;
    }
    let flip_tatweel = buffer.flags & FLAG_PRODUCE_SAFE_TO_INSERT_TATWEEL != 0;
    let clear_concat = buffer.flags & FLAG_PRODUCE_UNSAFE_TO_CONCAT == 0;
    let mut start = 0;
    while start < buffer.len() {
        let end = buffer.cluster_end(start);
        let mut mask = 0;
        for i in start..end {
            mask |= buffer.info[i].mask & GLYPH_FLAG_DEFINED;
        }
        if flip_tatweel {
            if mask & GLYPH_FLAG_UNSAFE_TO_BREAK != 0 {
                mask &= !GLYPH_FLAG_SAFE_TO_INSERT_TATWEEL;
            }
            if mask & GLYPH_FLAG_SAFE_TO_INSERT_TATWEEL != 0 {
                mask |= GLYPH_FLAG_UNSAFE_TO_BREAK | GLYPH_FLAG_UNSAFE_TO_CONCAT;
            }
        }
        if clear_concat {
            mask &= !GLYPH_FLAG_UNSAFE_TO_CONCAT;
        }
        for i in start..end {
            buffer.info[i].mask = mask;
        }
        start = end;
    }
}

/// `hb_shape_full` com o shaper `ot`: o buffer sai com glifos e posições.
pub fn shape(plan: &Plan, font: &Font, buffer: &mut Buffer) {
    if buffer.is_empty() {
        return;
    }
    buffer.enter();
    let target_direction = buffer.props.direction;
    buffer.reset_masks(plan.map.global_mask);
    set_unicode_props_all(buffer);
    insert_dotted_circle(buffer, font);
    form_clusters(buffer);
    ensure_native_direction(buffer);
    substitute_pre(plan, font, buffer, target_direction);
    position(plan, font, buffer);
    deal_with_variation_selectors(buffer);
    hide_default_ignorables(buffer, font);
    if plan.shaper.kind == ShaperKind::Arabic {
        crate::arabic::postprocess_glyphs(buffer, font);
    }
    propagate_flags(buffer);
    buffer.props.direction = target_direction;
    buffer.leave();
}
