//! As funções Unicode do HarfBuzz (`hb-ucd.cc` e `hb-unicode.hh`), sobre as tabelas geradas em
//! `ucd_table.rs`.

use crate::ucd_table::*;

/// `hb_unicode_general_category_t`, na ordem do `hb-unicode.h`.
pub mod gc {
    pub const CONTROL: u8 = 0;
    pub const FORMAT: u8 = 1;
    pub const UNASSIGNED: u8 = 2;
    pub const PRIVATE_USE: u8 = 3;
    pub const SURROGATE: u8 = 4;
    pub const LOWERCASE_LETTER: u8 = 5;
    pub const MODIFIER_LETTER: u8 = 6;
    pub const OTHER_LETTER: u8 = 7;
    pub const TITLECASE_LETTER: u8 = 8;
    pub const UPPERCASE_LETTER: u8 = 9;
    pub const SPACING_MARK: u8 = 10;
    pub const ENCLOSING_MARK: u8 = 11;
    pub const NON_SPACING_MARK: u8 = 12;
    pub const DECIMAL_NUMBER: u8 = 13;
    pub const LETTER_NUMBER: u8 = 14;
    pub const OTHER_NUMBER: u8 = 15;
    pub const CONNECT_PUNCTUATION: u8 = 16;
    pub const DASH_PUNCTUATION: u8 = 17;
    pub const CLOSE_PUNCTUATION: u8 = 18;
    pub const FINAL_PUNCTUATION: u8 = 19;
    pub const INITIAL_PUNCTUATION: u8 = 20;
    pub const OTHER_PUNCTUATION: u8 = 21;
    pub const OPEN_PUNCTUATION: u8 = 22;
    pub const CURRENCY_SYMBOL: u8 = 23;
    pub const MODIFIER_SYMBOL: u8 = 24;
    pub const MATH_SYMBOL: u8 = 25;
    pub const OTHER_SYMBOL: u8 = 26;
    pub const LINE_SEPARATOR: u8 = 27;
    pub const PARAGRAPH_SEPARATOR: u8 = 28;
    pub const SPACE_SEPARATOR: u8 = 29;

    pub const fn flag(g: u8) -> u32 {
        1 << g
    }

    pub fn is_mark(g: u8) -> bool {
        flag(g) & (flag(SPACING_MARK) | flag(ENCLOSING_MARK) | flag(NON_SPACING_MARK)) != 0
    }

    pub fn is_letter(g: u8) -> bool {
        flag(g)
            & (flag(LOWERCASE_LETTER)
                | flag(MODIFIER_LETTER)
                | flag(OTHER_LETTER)
                | flag(TITLECASE_LETTER)
                | flag(UPPERCASE_LETTER))
            != 0
    }
}

fn b4(a: &[u8], i: usize) -> usize {
    usize::from((a[i >> 1] >> ((i & 1) << 2)) & 15)
}

fn u8_(i: usize) -> usize {
    usize::from(HB_UCD_U8[i])
}

fn u16_(i: usize) -> usize {
    usize::from(HB_UCD_U16[i])
}

/// `_hb_ucd_gc`.
pub fn general_category(u: u32) -> u8 {
    if u >= 1114110 {
        return gc::UNASSIGNED;
    }
    let u = u as usize;
    let a = u8_(u >> 1 >> 3 >> 4 >> 4);
    let b = u8_(272 + (a << 4) + ((u >> 1 >> 3 >> 4) & 15));
    let c = u16_((b << 4) + ((u >> 1 >> 3) & 15));
    let d = u8_(816 + (c << 3) + ((u >> 1) & 7));
    HB_UCD_U8[6472 + (d << 1) + (u & 1)]
}

/// `_hb_ucd_ccc`.
pub fn combining_class(u: u32) -> u8 {
    if u >= 125259 {
        return 0;
    }
    let u = u as usize;
    let a = u8_(6854 + (u >> 2 >> 2 >> 2 >> 3));
    let b = u8_(7100 + (a << 3) + ((u >> 2 >> 2 >> 2) & 7));
    let c = u8_(7460 + (b << 2) + ((u >> 2 >> 2) & 3));
    let d = u8_(7936 + (c << 2) + ((u >> 2) & 3));
    HB_UCD_U8[8504 + (d << 2) + (u & 3)]
}

/// `hb_ucd_mirroring` (`_hb_ucd_bmg`).
pub fn mirroring(u: u32) -> u32 {
    if u >= 65380 {
        return u;
    }
    let i = u as usize;
    let a = b4(&HB_UCD_U8[9004..], i >> 2 >> 3 >> 3);
    let b = u8_(9132 + (a << 3) + ((i >> 2 >> 3) & 7));
    let c = u8_(9252 + (b << 3) + ((i >> 2) & 7));
    (i64::from(u) + i64::from(HB_UCD_I16[(c << 2) + (i & 3)])) as u32
}

/// `hb_ucd_script`: a tag ISO 15924 (`hb_script_t`).
pub fn script(u: u32) -> u32 {
    let idx = if u >= 918000 {
        2
    } else {
        let i = u as usize;
        let a = u8_(9588 + (i >> 3 >> 3 >> 4));
        let b = u16_(2624 + (a << 4) + ((i >> 3 >> 3) & 15));
        let c = u16_(3744 + (b << 3) + ((i >> 3) & 7));
        u8_(10486 + (c << 3) + (i & 7))
    };
    HB_UCD_SC_MAP[idx]
}

fn dm(u: u32) -> usize {
    if u >= 195102 {
        return 0;
    }
    let i = u as usize;
    let a = u8_(16334 + (i >> 4 >> 5));
    let b = u8_(16716 + (a << 5) + ((i >> 4) & 31));
    u16_(6976 + (b << 4) + (i & 15))
}

const SBASE: u32 = 0xAC00;
const LBASE: u32 = 0x1100;
const VBASE: u32 = 0x1161;
const TBASE: u32 = 0x11A7;
const SCOUNT: u32 = 11172;
const LCOUNT: u32 = 19;
const VCOUNT: u32 = 21;
const TCOUNT: u32 = 28;
const NCOUNT: u32 = VCOUNT * TCOUNT;

/// `hb_ucd_compose`.
pub fn compose(a: u32, b: u32) -> Option<u32> {
    if (SBASE..SBASE + SCOUNT).contains(&a) && b > TBASE && b < TBASE + TCOUNT && (a - SBASE) % TCOUNT == 0 {
        return Some(a + (b - TBASE));
    }
    if (LBASE..LBASE + LCOUNT).contains(&a) && (VBASE..VBASE + VCOUNT).contains(&b) {
        return Some(SBASE + (a - LBASE) * NCOUNT + (b - VBASE) * TCOUNT);
    }
    let u = if a & 0xFFFF_F800 == 0 && b & 0xFFFF_FF80 == 0x0300 {
        let k = ((a & 0x7FF) << 21) | ((b & 0x7F) << 14);
        let i = HB_UCD_DM2_U32_MAP.binary_search_by(|v| (v & !0x3FFF).cmp(&k)).ok()?;
        HB_UCD_DM2_U32_MAP[i] & 0x3FFF
    } else {
        let k = (u64::from(a) << 42) | (u64::from(b) << 21);
        let i = HB_UCD_DM2_U64_MAP.binary_search_by(|v| (v & !0x1F_FFFF).cmp(&k)).ok()?;
        (HB_UCD_DM2_U64_MAP[i] & 0x1F_FFFF) as u32
    };
    (u != 0).then_some(u)
}

/// `hb_ucd_decompose`: `(a, b)`, com `b = 0` nas decomposições de um só caractere.
pub fn decompose(ab: u32) -> Option<(u32, u32)> {
    let si = ab.wrapping_sub(SBASE);
    if si < SCOUNT {
        return Some(if si % TCOUNT != 0 {
            (SBASE + (si / TCOUNT) * TCOUNT, TBASE + si % TCOUNT)
        } else {
            (LBASE + si / NCOUNT, VBASE + (si % NCOUNT) / TCOUNT)
        });
    }
    let mut i = dm(ab);
    if i == 0 {
        return None;
    }
    i -= 1;
    let (p0, p2) = (HB_UCD_DM1_P0_MAP.len(), HB_UCD_DM1_P2_MAP.len());
    if i < p0 {
        return Some((u32::from(HB_UCD_DM1_P0_MAP[i]), 0));
    }
    if i < p0 + p2 {
        return Some((0x20000 | u32::from(HB_UCD_DM1_P2_MAP[i - p0]), 0));
    }
    i -= p0 + p2;
    if i < HB_UCD_DM2_U32_MAP.len() {
        let v = HB_UCD_DM2_U32_MAP[i];
        return Some((v >> 21, ((v >> 14) & 0x7F) | 0x0300));
    }
    i -= HB_UCD_DM2_U32_MAP.len();
    let v = HB_UCD_DM2_U64_MAP[i];
    Some(((v >> 42) as u32, ((v >> 21) & 0x1F_FFFF) as u32))
}

/// `_hb_modified_combining_class`.
fn modified_ccc_table(c: u8) -> u8 {
    match c {
        10 => 22,
        11 => 15,
        12 => 16,
        13 => 17,
        14 => 23,
        15 => 18,
        16 => 19,
        17 => 20,
        18 => 21,
        19 => 14,
        20 => 24,
        21 => 12,
        22 => 25,
        23 => 13,
        24 => 10,
        25 => 11,
        26 => 26,
        27 => 28,
        28 => 29,
        29 => 30,
        30 => 31,
        31 => 32,
        32 => 33,
        33 => 27,
        34 => 34,
        35 => 35,
        36 => 36,
        84 => 4,
        91 => 5,
        103 => 3,
        130 => 132,
        132 => 131,
        c => c,
    }
}

/// `hb_unicode_funcs_t::modified_combining_class`.
pub fn modified_combining_class(u: u32) -> u8 {
    match u {
        0x1A60 | 0x0FC6 => 254,
        0x0F39 => 127,
        _ => modified_ccc_table(combining_class(u)),
    }
}

pub fn is_variation_selector(u: u32) -> bool {
    (0xFE00..=0xFE0F).contains(&u) || (0xE0100..=0xE01EF).contains(&u)
}

/// `hb_unicode_funcs_t::is_default_ignorable`.
pub fn is_default_ignorable(ch: u32) -> bool {
    let plane = ch >> 16;
    if plane == 0 {
        match ch >> 8 {
            0x00 => ch == 0x00AD,
            0x03 => ch == 0x034F,
            0x06 => ch == 0x061C,
            0x17 => (0x17B4..=0x17B5).contains(&ch),
            0x18 => (0x180B..=0x180E).contains(&ch),
            0x20 => (0x200B..=0x200F).contains(&ch) || (0x202A..=0x202E).contains(&ch) || (0x2060..=0x206F).contains(&ch),
            0xFE => (0xFE00..=0xFE0F).contains(&ch) || ch == 0xFEFF,
            0xFF => (0xFFF0..=0xFFF8).contains(&ch),
            _ => false,
        }
    } else {
        match plane {
            0x01 => (0x1D173..=0x1D17A).contains(&ch),
            0x0E => (0xE0000..=0xE0FFF).contains(&ch),
            _ => false,
        }
    }
}

/// `hb_unicode_funcs_t::space_t`.
pub mod space {
    pub const NOT_SPACE: u8 = 0;
    pub const SPACE_EM: u8 = 1;
    pub const SPACE_EM_2: u8 = 2;
    pub const SPACE_EM_3: u8 = 3;
    pub const SPACE_EM_4: u8 = 4;
    pub const SPACE_EM_5: u8 = 5;
    pub const SPACE_EM_6: u8 = 6;
    pub const SPACE_EM_16: u8 = 16;
    pub const SPACE_4_EM_18: u8 = 17;
    pub const SPACE: u8 = 18;
    pub const SPACE_FIGURE: u8 = 19;
    pub const SPACE_PUNCTUATION: u8 = 20;
    pub const SPACE_NARROW: u8 = 21;
}

pub fn space_fallback_type(u: u32) -> u8 {
    use space::*;
    match u {
        0x0020 | 0x00A0 => SPACE,
        0x2000 => SPACE_EM_2,
        0x2001 => SPACE_EM,
        0x2002 => SPACE_EM_2,
        0x2003 => SPACE_EM,
        0x2004 => SPACE_EM_3,
        0x2005 => SPACE_EM_4,
        0x2006 => SPACE_EM_6,
        0x2007 => SPACE_FIGURE,
        0x2008 => SPACE_PUNCTUATION,
        0x2009 => SPACE_EM_5,
        0x200A => SPACE_EM_16,
        0x202F => SPACE_NARROW,
        0x205F => SPACE_4_EM_18,
        0x3000 => SPACE_EM,
        _ => NOT_SPACE,
    }
}

/// `_hb_emoji_is_Extended_Pictographic`.
pub fn is_extended_pictographic(u: u32) -> bool {
    if u >= 131070 {
        return false;
    }
    let i = u as usize;
    let e = &HB_EMOJI_U8;
    let a = b4(e, i >> 5 >> 2 >> 3);
    let b = usize::from(e[64 + (a << 3) + ((i >> 5 >> 2) & 7)]);
    let c = usize::from(e[144 + (b << 2) + ((i >> 5) & 3)]);
    let bit = (c << 5) + (i & 31);
    (e[264 + (bit >> 3)] >> (bit & 7)) & 1 != 0
}
