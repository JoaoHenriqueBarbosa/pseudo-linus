//! Os acessores de `unicode_props`, `glyph_props` e `lig_props` do `hb-ot-layout.hh`.

use crate::buffer::{Buffer, GlyphInfo, scratch};
use crate::ot::{GLYPH_PROPS_BASE_GLYPH, GLYPH_PROPS_LIGATED, GLYPH_PROPS_LIGATURE, GLYPH_PROPS_MARK, GLYPH_PROPS_MULTIPLIED, GLYPH_PROPS_SUBSTITUTED};
use crate::unicode::{self, gc};

pub const UPROPS_MASK_GEN_CAT: u16 = 0x001F;
pub const UPROPS_MASK_IGNORABLE: u16 = 0x0020;
pub const UPROPS_MASK_HIDDEN: u16 = 0x0040;
pub const UPROPS_MASK_CONTINUATION: u16 = 0x0080;
pub const UPROPS_MASK_CF_ZWJ: u16 = 0x0100;
pub const UPROPS_MASK_CF_ZWNJ: u16 = 0x0200;
pub const UPROPS_MASK_CF_VS: u16 = 0x0400;

const IS_LIG_BASE: u8 = 0x10;

/// `_hb_glyph_info_set_unicode_props`.
pub fn set_unicode_props(info: &mut GlyphInfo, scratch_flags: &mut u32) {
    let u = info.codepoint;
    let gen_cat = unicode::general_category(u);
    let mut props = u16::from(gen_cat);
    if u >= 0x80 {
        *scratch_flags |= scratch::HAS_NON_ASCII;
        if unicode::is_default_ignorable(u) {
            *scratch_flags |= scratch::HAS_DEFAULT_IGNORABLES;
            props |= UPROPS_MASK_IGNORABLE;
            if u == 0x200C {
                props |= UPROPS_MASK_CF_ZWNJ;
            } else if u == 0x200D {
                props |= UPROPS_MASK_CF_ZWJ;
            } else if (0x180B..=0x180D).contains(&u) || u == 0x180F || (0xE0020..=0xE007F).contains(&u) {
                props |= UPROPS_MASK_HIDDEN;
            } else if u == 0x034F {
                *scratch_flags |= scratch::HAS_CGJ;
                props |= UPROPS_MASK_HIDDEN;
            }
        }
        if gc::is_mark(gen_cat) {
            props |= UPROPS_MASK_CONTINUATION;
            props |= u16::from(unicode::modified_combining_class(u)) << 8;
        }
    }
    info.unicode_props = props;
}

impl GlyphInfo {
    pub fn general_category(&self) -> u8 {
        (self.unicode_props & UPROPS_MASK_GEN_CAT) as u8
    }
    pub fn set_general_category(&mut self, g: u8) {
        self.unicode_props = u16::from(g) | (self.unicode_props & (0xFF & !UPROPS_MASK_GEN_CAT));
    }
    pub fn is_unicode_mark(&self) -> bool {
        gc::is_mark(self.general_category())
    }
    pub fn set_modified_combining_class(&mut self, c: u8) {
        if !self.is_unicode_mark() {
            return;
        }
        self.unicode_props = (u16::from(c) << 8) | (self.unicode_props & 0xFF);
    }
    pub fn modified_combining_class(&self) -> u8 {
        if self.is_unicode_mark() { (self.unicode_props >> 8) as u8 } else { 0 }
    }
    pub fn is_unicode_space(&self) -> bool {
        self.general_category() == gc::SPACE_SEPARATOR
    }
    pub fn set_unicode_space_fallback_type(&mut self, s: u8) {
        if !self.is_unicode_space() {
            return;
        }
        self.unicode_props = (u16::from(s) << 8) | (self.unicode_props & 0xFF);
    }
    pub fn unicode_space_fallback_type(&self) -> u8 {
        if self.is_unicode_space() { (self.unicode_props >> 8) as u8 } else { unicode::space::NOT_SPACE }
    }
    pub fn is_variation_selector(&self) -> bool {
        self.general_category() == gc::FORMAT && self.unicode_props & UPROPS_MASK_CF_VS != 0
    }
    pub fn set_variation_selector(&mut self, customize: bool) {
        if customize {
            self.set_general_category(gc::FORMAT);
            self.unicode_props |= UPROPS_MASK_CF_VS;
        } else {
            self.set_general_category(gc::NON_SPACING_MARK);
        }
    }
    pub fn is_default_ignorable(&self) -> bool {
        self.unicode_props & UPROPS_MASK_IGNORABLE != 0 && !self.substituted()
    }
    pub fn clear_default_ignorable(&mut self) {
        self.unicode_props &= !UPROPS_MASK_IGNORABLE;
    }
    pub fn is_hidden(&self) -> bool {
        self.unicode_props & UPROPS_MASK_HIDDEN != 0
    }
    pub fn unhide(&mut self) {
        self.unicode_props &= !UPROPS_MASK_HIDDEN;
    }
    pub fn set_continuation(&mut self) {
        self.unicode_props |= UPROPS_MASK_CONTINUATION;
    }
    pub fn reset_continuation(&mut self) {
        self.unicode_props &= !UPROPS_MASK_CONTINUATION;
    }
    pub fn is_continuation(&self) -> bool {
        self.unicode_props & UPROPS_MASK_CONTINUATION != 0
    }
    pub fn is_unicode_format(&self) -> bool {
        self.general_category() == gc::FORMAT
    }
    pub fn is_zwnj(&self) -> bool {
        self.is_unicode_format() && self.unicode_props & UPROPS_MASK_CF_ZWNJ != 0
    }
    pub fn is_zwj(&self) -> bool {
        self.is_unicode_format() && self.unicode_props & UPROPS_MASK_CF_ZWJ != 0
    }
    pub fn is_joiner(&self) -> bool {
        self.is_unicode_format() && self.unicode_props & (UPROPS_MASK_CF_ZWNJ | UPROPS_MASK_CF_ZWJ) != 0
    }
    pub fn flip_joiners(&mut self) {
        if !self.is_unicode_format() {
            return;
        }
        self.unicode_props ^= UPROPS_MASK_CF_ZWNJ | UPROPS_MASK_CF_ZWJ;
    }

    pub fn clear_lig_props(&mut self) {
        self.lig_props = 0;
    }
    pub fn set_lig_props_for_ligature(&mut self, lig_id: u32, num_comps: u32) {
        self.lig_props = ((lig_id << 5) as u8) | IS_LIG_BASE | (num_comps & 0x0F) as u8;
    }
    pub fn set_lig_props_for_mark(&mut self, lig_id: u32, comp: u32) {
        self.lig_props = ((lig_id << 5) as u8) | (comp & 0x0F) as u8;
    }
    pub fn set_lig_props_for_component(&mut self, comp: u32) {
        self.set_lig_props_for_mark(0, comp);
    }
    pub fn lig_id(&self) -> u32 {
        u32::from(self.lig_props >> 5)
    }
    fn ligated_internal(&self) -> bool {
        self.lig_props & IS_LIG_BASE != 0
    }
    pub fn lig_comp(&self) -> u32 {
        if self.ligated_internal() { 0 } else { u32::from(self.lig_props & 0x0F) }
    }
    pub fn lig_num_comps(&self) -> u32 {
        if self.glyph_props & GLYPH_PROPS_LIGATURE != 0 && self.ligated_internal() {
            u32::from(self.lig_props & 0x0F)
        } else {
            1
        }
    }

    pub fn is_base_glyph(&self) -> bool {
        self.glyph_props & GLYPH_PROPS_BASE_GLYPH != 0
    }
    pub fn is_ligature(&self) -> bool {
        self.glyph_props & GLYPH_PROPS_LIGATURE != 0
    }
    pub fn is_mark(&self) -> bool {
        self.glyph_props & GLYPH_PROPS_MARK != 0
    }
    pub fn substituted(&self) -> bool {
        self.glyph_props & GLYPH_PROPS_SUBSTITUTED != 0
    }
    pub fn ligated(&self) -> bool {
        self.glyph_props & GLYPH_PROPS_LIGATED != 0
    }
    pub fn multiplied(&self) -> bool {
        self.glyph_props & GLYPH_PROPS_MULTIPLIED != 0
    }
    pub fn ligated_and_didnt_multiply(&self) -> bool {
        self.ligated() && !self.multiplied()
    }
    pub fn clear_ligated_and_multiplied(&mut self) {
        self.glyph_props &= !(GLYPH_PROPS_LIGATED | GLYPH_PROPS_MULTIPLIED);
    }
    pub fn clear_substituted(&mut self) {
        self.glyph_props &= !GLYPH_PROPS_SUBSTITUTED;
    }
}

/// `_hb_allocate_lig_id`.
pub fn allocate_lig_id(buffer: &mut Buffer) -> u32 {
    loop {
        let id = u32::from(buffer.next_serial() & 0x07);
        if id != 0 {
            return id;
        }
    }
}

/// `_hb_next_syllable`.
pub fn next_syllable(buffer: &Buffer, mut start: usize) -> usize {
    let s = buffer.info[start].syllable;
    start += 1;
    while start < buffer.len() && buffer.info[start].syllable == s {
        start += 1;
    }
    start
}
