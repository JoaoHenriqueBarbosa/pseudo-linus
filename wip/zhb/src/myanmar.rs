//! O shaper myanmar do HarfBuzz 10.2.0 (`hb-ot-shaper-myanmar.cc`): sílabas pela máquina do
//! Ragel e reordenação inicial (kinzi, medial Ra e vogal pré-base) antes das features básicas.

use crate::buffer::{scratch, tag, Buffer, GlyphInfo};
use crate::font::Font;
use crate::indic::pos;
use crate::map::{MapBuilder, Pause, F_MANUAL_ZWJ, F_PER_SYLLABLE};
use crate::myanmar_machine::{self as m, cat};
use crate::props::next_syllable;

/// `myanmar_syllable_type_t`.
mod syllable {
    pub const CONSONANT: u8 = 0;
    pub const BROKEN: u8 = 1;
    pub const NON_MYANMAR: u8 = 2;
}

/// `collect_features_myanmar`.
pub fn collect_features(map: &mut MapBuilder) {
    map.add_gsub_pause(Some(Pause::MyanmarSetupSyllables));
    map.enable_feature(tag(b"locl"), F_PER_SYLLABLE, 1);
    map.enable_feature(tag(b"ccmp"), F_PER_SYLLABLE, 1);
    map.add_gsub_pause(Some(Pause::MyanmarReorder));
    for t in [b"rphf", b"pref", b"blwf", b"pstf"] {
        map.enable_feature(tag(t), F_MANUAL_ZWJ | F_PER_SYLLABLE, 1);
        map.add_gsub_pause(None);
    }
    map.add_gsub_pause(Some(Pause::SyllabicClearVar));
    for t in [b"pres", b"abvs", b"blws", b"psts"] {
        map.enable_feature(tag(t), F_MANUAL_ZWJ, 1);
    }
}

/// `setup_masks_myanmar`: só a categoria, da mesma tabela do índico.
pub fn setup_masks(buffer: &mut Buffer) {
    for info in &mut buffer.info {
        info.shaper_cat = (crate::indic::categories(info.codepoint) & 0xFF) as u8;
    }
}

/// As pausas do shaper myanmar.
pub fn pause(p: Pause, font: &Font, buffer: &mut Buffer) {
    match p {
        Pause::MyanmarSetupSyllables => {
            find_syllables(buffer);
            let mut start = 0;
            while start < buffer.len() {
                let end = next_syllable(buffer, start);
                buffer.unsafe_to_break(start, end);
                start = end;
            }
        }
        Pause::MyanmarReorder => {
            crate::universal::insert_dotted_circles(font, buffer, syllable::BROKEN, cat::DOTTEDCIRCLE, None, None);
            let mut start = 0;
            while start < buffer.len() {
                let end = next_syllable(buffer, start);
                if matches!(buffer.info[start].syllable & 0x0F, syllable::CONSONANT | syllable::BROKEN) {
                    initial_reordering_consonant_syllable(buffer, start, end);
                }
                start = end;
            }
        }
        _ => {}
    }
}

/// `find_syllables_myanmar`.
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
                if m::FROM_STATE_ACTIONS[cs] == 2 {
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
                    a @ (8 | 4 | 10 | 3) => {
                        te = p + 1;
                        let kind = match a {
                            8 => CONSONANT,
                            10 => BROKEN,
                            _ => NON_MYANMAR,
                        };
                        found(buffer, ts, te, kind);
                    }
                    a @ (7 | 9 | 12) => {
                        te = p;
                        p -= 1;
                        let kind = match a {
                            7 => CONSONANT,
                            9 => BROKEN,
                            _ => NON_MYANMAR,
                        };
                        found(buffer, ts, te, kind);
                    }
                    11 => {
                        let kind = match act {
                            2 => Some(NON_MYANMAR),
                            3 => Some(BROKEN),
                            _ => None,
                        };
                        if let Some(kind) = kind {
                            p = te - 1;
                            found(buffer, ts, te, kind);
                        }
                    }
                    6 => {
                        te = p + 1;
                        act = 2;
                    }
                    5 => {
                        te = p + 1;
                        act = 3;
                    }
                    _ => {}
                }
                if m::TO_STATE_ACTIONS[cs] == 1 {
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

/// `FLAG_UNSAFE`.
const fn flag(x: u8) -> u32 {
    if x < 32 { 1 << x } else { 0 }
}

/// `is_consonant_myanmar`.
fn is_consonant(info: &GlyphInfo) -> bool {
    let flags = flag(cat::C) | flag(cat::CS) | flag(cat::RA) | flag(cat::IV) | flag(cat::GB) | flag(cat::DOTTEDCIRCLE);
    !info.ligated() && flag(info.shaper_cat) & flags != 0
}

/// `initial_reordering_consonant_syllable` do myanmar.
fn initial_reordering_consonant_syllable(buffer: &mut Buffer, start: usize, end: usize) {
    let info = &mut buffer.info;

    // A base, depois de um kinzi (Ra, asat e virama) no começo.
    let mut base = end;
    let mut has_reph = false;
    let mut limit = start;
    if start + 3 <= end && info[start].shaper_cat == cat::RA && info[start + 1].shaper_cat == cat::AS && info[start + 2].shaper_cat == cat::H {
        limit += 3;
        base = start;
        has_reph = true;
    }
    if !has_reph {
        base = limit;
    }
    if let Some(i) = (limit..end).find(|&i| is_consonant(&info[i])) {
        base = i;
    }

    // As posições.
    let mut i = start;
    while i < start + if has_reph { 3 } else { 0 } {
        info[i].shaper_aux = pos::AFTER_MAIN;
        i += 1;
    }
    while i < base {
        info[i].shaper_aux = pos::PRE_C;
        i += 1;
    }
    if i < end {
        info[i].shaper_aux = pos::BASE_C;
        i += 1;
    }
    let mut p = pos::AFTER_MAIN;
    while i < end {
        let c = info[i].shaper_cat;
        if c == cat::MR {
            info[i].shaper_aux = pos::PRE_C;
        } else if c == cat::VPRE {
            info[i].shaper_aux = pos::PRE_M;
        } else if c == cat::VS {
            info[i].shaper_aux = info[i - 1].shaper_aux;
        } else if p == pos::AFTER_MAIN && c == cat::VBLW {
            p = pos::BELOW_C;
            info[i].shaper_aux = p;
        } else if p == pos::BELOW_C && c == cat::A {
            info[i].shaper_aux = pos::BEFORE_SUB;
        } else if p == pos::BELOW_C && c == cat::VBLW {
            info[i].shaper_aux = p;
        } else if p == pos::BELOW_C {
            p = pos::AFTER_SUB;
            info[i].shaper_aux = p;
        } else {
            info[i].shaper_aux = p;
        }
        i += 1;
    }

    // `hb_buffer_t::sort`: inserção que junta os clusters do que se moveu.
    buffer.sort(start, end, |a, b| a.shaper_aux.cmp(&b.shaper_aux));

    // As vogais pré-base voltam à ordem lógica.
    let mut first_left_matra = end;
    let mut last_left_matra = end;
    for i in start..end {
        if buffer.info[i].shaper_aux == pos::PRE_M {
            if first_left_matra == end {
                first_left_matra = i;
            }
            last_left_matra = i;
        }
    }
    if first_left_matra < last_left_matra {
        buffer.reverse_range(first_left_matra, last_left_matra + 1);
        let mut i = first_left_matra;
        for j in i..=last_left_matra {
            if buffer.info[j].shaper_cat == cat::VPRE {
                buffer.reverse_range(i, j + 1);
                i = j + 1;
            }
        }
    }
}
