//! O shaper khmer do HarfBuzz 10.2.0 (`hb-ot-shaper-khmer.cc`): sílabas pela máquina do Ragel,
//! coeng com Ro e vogais pré-base para o começo da sílaba antes das features básicas.

use crate::buffer::{scratch, tag, Buffer};
use crate::font::Font;
use crate::khmer_machine::{self as m, cat};
use crate::map::{Map, MapBuilder, Pause, F_GLOBAL, F_GLOBAL_MANUAL_JOINERS, F_MANUAL_JOINERS, F_NONE, F_PER_SYLLABLE};
use crate::props::next_syllable;

/// `khmer_syllable_type_t`.
mod syllable {
    pub const CONSONANT: u8 = 0;
    pub const BROKEN: u8 = 1;
    pub const NON_KHMER: u8 = 2;
}

/// `khmer_features`: as básicas até `cfar` e as de apresentação.
const FEATURES: [(&[u8; 4], u32); 9] = [
    (b"pref", F_MANUAL_JOINERS | F_PER_SYLLABLE),
    (b"blwf", F_MANUAL_JOINERS | F_PER_SYLLABLE),
    (b"abvf", F_MANUAL_JOINERS | F_PER_SYLLABLE),
    (b"pstf", F_MANUAL_JOINERS | F_PER_SYLLABLE),
    (b"cfar", F_MANUAL_JOINERS | F_PER_SYLLABLE),
    (b"pres", F_GLOBAL_MANUAL_JOINERS),
    (b"abvs", F_GLOBAL_MANUAL_JOINERS),
    (b"blws", F_GLOBAL_MANUAL_JOINERS),
    (b"psts", F_GLOBAL_MANUAL_JOINERS),
];
const PREF: usize = 0;
const BLWF: usize = 1;
const ABVF: usize = 2;
const PSTF: usize = 3;
const CFAR: usize = 4;
const BASIC_FEATURES: usize = 5;

/// `collect_features_khmer`.
pub fn collect_features(map: &mut MapBuilder) {
    map.add_gsub_pause(Some(Pause::KhmerSetupSyllables));
    map.add_gsub_pause(Some(Pause::KhmerReorder));
    map.enable_feature(tag(b"locl"), F_PER_SYLLABLE, 1);
    map.enable_feature(tag(b"ccmp"), F_PER_SYLLABLE, 1);
    for (t, flags) in &FEATURES[..BASIC_FEATURES] {
        map.add_feature(tag(t), *flags, 1);
    }
    map.add_gsub_pause(Some(Pause::SyllabicClearVar));
    for (t, flags) in &FEATURES[BASIC_FEATURES..] {
        map.add_feature(tag(t), *flags, 1);
    }
}

/// `override_features_khmer`.
pub fn override_features(map: &mut MapBuilder) {
    map.enable_feature(tag(b"clig"), F_NONE, 1);
    map.disable_feature(tag(b"liga"));
}

/// `khmer_shape_plan_t`.
pub struct KhmerPlan {
    mask_array: [u32; 9],
}

impl KhmerPlan {
    /// `data_create_khmer`.
    pub fn new(map: &Map) -> KhmerPlan {
        let mut mask_array = [0; 9];
        for (i, (t, flags)) in FEATURES.iter().enumerate() {
            mask_array[i] = if flags & F_GLOBAL != 0 { 0 } else { map.one_mask(tag(t)) };
        }
        KhmerPlan { mask_array }
    }
}

/// `setup_masks_khmer`: só a categoria, da mesma tabela do índico.
pub fn setup_masks(buffer: &mut Buffer) {
    for info in &mut buffer.info {
        info.shaper_cat = (crate::indic::categories(info.codepoint) & 0xFF) as u8;
    }
}

/// `decompose_khmer`: as vogais partidas começam pelo E pré-base.
pub fn decompose(ab: u32) -> Option<(u32, u32)> {
    match ab {
        0x17BE | 0x17BF | 0x17C0 | 0x17C4 | 0x17C5 => Some((0x17C1, ab)),
        _ => crate::unicode::decompose(ab),
    }
}

/// As pausas do shaper khmer.
pub fn pause(p: Pause, plan: &KhmerPlan, font: &Font, buffer: &mut Buffer) {
    match p {
        Pause::KhmerSetupSyllables => {
            find_syllables(buffer);
            let mut start = 0;
            while start < buffer.len() {
                let end = next_syllable(buffer, start);
                buffer.unsafe_to_break(start, end);
                start = end;
            }
        }
        Pause::KhmerReorder => reorder(plan, font, buffer),
        _ => {}
    }
}

/// `find_syllables_khmer`.
fn find_syllables(buffer: &mut Buffer) {
    let len = buffer.len() as isize;
    let (pe, eof) = (len, len);
    let mut cs = m::START;
    let mut ts: isize = 0;
    let mut te: isize = 0;
    let mut act = 0u32;
    let mut p: isize = 0;
    let mut serial: u8 = 1;

    let mut found = |buffer: &mut Buffer, ts: isize, te: isize, kind: u8| {
        for i in ts as usize..te as usize {
            buffer.info[i].syllable = (serial << 4) | kind;
        }
        serial += 1;
        if serial == 16 {
            serial = 1;
        }
        if kind == syllable::BROKEN {
            buffer.scratch_flags |= scratch::HAS_BROKEN_SYLLABLE;
        }
    };

    enum St {
        Resume,
        Trans(usize),
        TestEof,
    }
    let mut st = if p == pe { St::TestEof } else { St::Resume };
    loop {
        match st {
            St::Resume => {
                if m::FROM_STATE_ACTIONS[cs] == 7 {
                    ts = p;
                }
                let keys = cs << 1;
                let inds = usize::from(m::INDEX_OFFSETS[cs]);
                let slen = usize::from(m::KEY_SPANS[cs]);
                let c = buffer.info[p as usize].shaper_cat;
                let k = if slen > 0 && m::TRANS_KEYS[keys] <= c && c <= m::TRANS_KEYS[keys + 1] {
                    usize::from(c - m::TRANS_KEYS[keys])
                } else {
                    slen
                };
                st = St::Trans(usize::from(m::INDICIES[inds + k]));
            }
            St::Trans(trans) => {
                cs = usize::from(m::TRANS_TARGS[trans]);
                use syllable::*;
                match m::TRANS_ACTIONS[trans] {
                    2 => te = p + 1,
                    8 => {
                        te = p + 1;
                        found(buffer, ts, te, NON_KHMER);
                    }
                    a @ (10 | 11 | 12) => {
                        te = p;
                        p -= 1;
                        let kind = match a {
                            10 => CONSONANT,
                            11 => BROKEN,
                            _ => NON_KHMER,
                        };
                        found(buffer, ts, te, kind);
                    }
                    a @ (1 | 3) => {
                        p = te - 1;
                        found(buffer, ts, te, if a == 1 { CONSONANT } else { BROKEN });
                    }
                    5 => {
                        let kind = match act {
                            2 => Some(BROKEN),
                            3 => Some(NON_KHMER),
                            _ => None,
                        };
                        if let Some(kind) = kind {
                            p = te - 1;
                            found(buffer, ts, te, kind);
                        }
                    }
                    4 => {
                        te = p + 1;
                        act = 2;
                    }
                    9 => {
                        te = p + 1;
                        act = 3;
                    }
                    _ => {}
                }
                if m::TO_STATE_ACTIONS[cs] == 6 {
                    ts = 0;
                }
                p += 1;
                st = if p != pe { St::Resume } else { St::TestEof };
            }
            St::TestEof => {
                if p == eof && m::EOF_TRANS[cs] > 0 {
                    st = St::Trans(usize::from(m::EOF_TRANS[cs]) - 1);
                } else {
                    break;
                }
            }
        }
    }
}

/// `reorder_khmer`.
fn reorder(plan: &KhmerPlan, font: &Font, buffer: &mut Buffer) {
    crate::universal::insert_dotted_circles(font, buffer, syllable::BROKEN, cat::DOTTEDCIRCLE, None, None);
    let mut start = 0;
    while start < buffer.len() {
        let end = next_syllable(buffer, start);
        if matches!(buffer.info[start].syllable & 0x0F, syllable::CONSONANT | syllable::BROKEN) {
            reorder_consonant_syllable(plan, buffer, start, end);
        }
        start = end;
    }
}

/// `reorder_consonant_syllable` do khmer.
fn reorder_consonant_syllable(plan: &KhmerPlan, buffer: &mut Buffer, start: usize, end: usize) {
    let mask = plan.mask_array[BLWF] | plan.mask_array[ABVF] | plan.mask_array[PSTF];
    for info in &mut buffer.info[start + 1..end] {
        info.mask |= mask;
    }

    let mut num_coengs = 0;
    for i in start + 1..end {
        // Coeng com Ro vira pré-base: os dois vão para o começo da sílaba.
        if buffer.info[i].shaper_cat == cat::H && num_coengs <= 2 && i + 1 < end {
            num_coengs += 1;
            if buffer.info[i + 1].shaper_cat == cat::RA {
                buffer.info[i].mask |= plan.mask_array[PREF];
                buffer.info[i + 1].mask |= plan.mask_array[PREF];
                buffer.merge_clusters(start, i + 2);
                buffer.info[start..i + 2].rotate_right(2);
                if plan.mask_array[CFAR] != 0 {
                    for info in &mut buffer.info[i + 2..end] {
                        info.mask |= plan.mask_array[CFAR];
                    }
                }
                num_coengs = 2;
            }
        } else if buffer.info[i].shaper_cat == cat::VPRE {
            // A vogal pré-base vai para o começo.
            buffer.merge_clusters(start, i + 1);
            buffer.info[start..=i].rotate_right(1);
        }
    }
}
