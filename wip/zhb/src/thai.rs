//! `hb-ot-shaper-thai.cc`: o SARA AM decomposto e reordenado (tailandês e laosiano) e, em fontes
//! tailandesas sem GSUB próprio, a forma de apresentação por PUA.

use crate::buffer::{Buffer, ClusterLevel};
use crate::font::Font;
use crate::shape::Plan;
use crate::unicode::gc;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Consonant {
    Nc,
    Ac,
    Rc,
    Dc,
    Not,
}

fn consonant_type(u: u32) -> Consonant {
    match u {
        0x0E1B | 0x0E1D | 0x0E1F => Consonant::Ac,
        0x0E0D | 0x0E10 => Consonant::Rc,
        0x0E0E | 0x0E0F => Consonant::Dc,
        0x0E01..=0x0E2E => Consonant::Nc,
        _ => Consonant::Not,
    }
}

/// AV, BV e T (índices das colunas das máquinas), ou `None` para o que não é marca.
fn mark_type(u: u32) -> Option<usize> {
    match u {
        0x0E31 | 0x0E34..=0x0E37 | 0x0E47 | 0x0E4D..=0x0E4E => Some(0),
        0x0E38..=0x0E3A => Some(1),
        0x0E48..=0x0E4C => Some(2),
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Nop,
    Sd,
    Sl,
    Sdl,
    Rd,
}

const SD_MAPPINGS: &[(u32, u32, u32)] = &[
    (0x0E48, 0xF70A, 0xF88B),
    (0x0E49, 0xF70B, 0xF88E),
    (0x0E4A, 0xF70C, 0xF891),
    (0x0E4B, 0xF70D, 0xF894),
    (0x0E4C, 0xF70E, 0xF897),
    (0x0E38, 0xF718, 0xF89B),
    (0x0E39, 0xF719, 0xF89C),
    (0x0E3A, 0xF71A, 0xF89D),
];
const SDL_MAPPINGS: &[(u32, u32, u32)] = &[
    (0x0E48, 0xF705, 0xF88C),
    (0x0E49, 0xF706, 0xF88F),
    (0x0E4A, 0xF707, 0xF892),
    (0x0E4B, 0xF708, 0xF895),
    (0x0E4C, 0xF709, 0xF898),
];
const SL_MAPPINGS: &[(u32, u32, u32)] = &[
    (0x0E48, 0xF713, 0xF88A),
    (0x0E49, 0xF714, 0xF88D),
    (0x0E4A, 0xF715, 0xF890),
    (0x0E4B, 0xF716, 0xF893),
    (0x0E4C, 0xF717, 0xF896),
    (0x0E31, 0xF710, 0xF884),
    (0x0E34, 0xF701, 0xF885),
    (0x0E35, 0xF702, 0xF886),
    (0x0E36, 0xF703, 0xF887),
    (0x0E37, 0xF704, 0xF888),
    (0x0E47, 0xF712, 0xF889),
    (0x0E4D, 0xF711, 0xF899),
];
const RD_MAPPINGS: &[(u32, u32, u32)] = &[(0x0E0D, 0xF70F, 0xF89A), (0x0E10, 0xF700, 0xF89E)];

/// `thai_pua_shape`: a forma PUA do Windows, senão a do Mac, se a fonte tiver o glifo.
fn pua_shape(u: u32, action: Action, font: &Font) -> u32 {
    let table = match action {
        Action::Nop => return u,
        Action::Sd => SD_MAPPINGS,
        Action::Sdl => SDL_MAPPINGS,
        Action::Sl => SL_MAPPINGS,
        Action::Rd => RD_MAPPINGS,
    };
    if let Some(&(_, win, mac)) = table.iter().find(|m| m.0 == u) {
        if font.nominal_glyph(win).is_some() {
            return win;
        }
        if font.nominal_glyph(mac).is_some() {
            return mac;
        }
    }
    u
}

/// Estados de cima: T0..T3; de baixo: B0..B2.
fn above_start(c: Consonant) -> usize {
    match c {
        Consonant::Ac => 1,
        Consonant::Not => 3,
        _ => 0,
    }
}

fn below_start(c: Consonant) -> usize {
    match c {
        Consonant::Rc => 1,
        Consonant::Dc | Consonant::Not => 2,
        _ => 0,
    }
}

use Action::{Nop, Rd, Sd, Sdl, Sl};

const ABOVE_MACHINE: [[(Action, usize); 3]; 4] = [
    [(Nop, 3), (Nop, 0), (Sd, 3)],
    [(Sl, 2), (Nop, 1), (Sdl, 2)],
    [(Nop, 3), (Nop, 2), (Sl, 3)],
    [(Nop, 3), (Nop, 3), (Nop, 3)],
];

const BELOW_MACHINE: [[(Action, usize); 3]; 3] = [
    [(Nop, 0), (Nop, 2), (Nop, 0)],
    [(Nop, 1), (Rd, 2), (Nop, 1)],
    [(Nop, 2), (Sd, 2), (Nop, 2)],
];

/// `do_thai_pua_shaping`.
fn pua_shaping(buffer: &mut Buffer, font: &Font) {
    let mut above = above_start(Consonant::Not);
    let mut below = below_start(Consonant::Not);
    let mut base = 0;
    for i in 0..buffer.len() {
        let Some(mt) = mark_type(buffer.info[i].codepoint) else {
            let ct = consonant_type(buffer.info[i].codepoint);
            above = above_start(ct);
            below = below_start(ct);
            base = i;
            continue;
        };
        let (a_action, a_next) = ABOVE_MACHINE[above][mt];
        let (b_action, b_next) = BELOW_MACHINE[below][mt];
        above = a_next;
        below = b_next;
        let action = if a_action != Nop { a_action } else { b_action };
        buffer.unsafe_to_break(base, i);
        if action == Rd {
            buffer.info[base].codepoint = pua_shape(buffer.info[base].codepoint, action, font);
        } else {
            buffer.info[i].codepoint = pua_shape(buffer.info[i].codepoint, action, font);
        }
    }
}

fn is_sara_am(x: u32) -> bool {
    x & !0x0080 == 0x0E33
}

fn is_above_base_mark(x: u32) -> bool {
    matches!(x & !0x0080, 0x0E34..=0x0E37 | 0x0E47..=0x0E4E | 0x0E31 | 0x0E3B)
}

/// `preprocess_text_thai`.
pub fn preprocess_text(plan: &Plan, buffer: &mut Buffer, font: &Font) {
    buffer.clear_output();
    let count = buffer.len();
    buffer.idx = 0;
    while buffer.idx < count {
        let u = buffer.cur(0).codepoint;
        if !is_sara_am(u) {
            if !buffer.next_glyph() {
                break;
            }
            continue;
        }
        // NIKHAHIT seguido de SARA AA.
        buffer.output_glyph(u - 0x0E33 + 0x0E4D);
        buffer.prev_mut().set_continuation();
        if !buffer.replace_glyph(u - 1) {
            break;
        }
        let end = buffer.out_len();
        buffer.out_info[end - 2].set_general_category(gc::NON_SPACING_MARK);
        let mut start = end - 2;
        while start > 0 && is_above_base_mark(buffer.out_info[start - 1].codepoint) {
            start -= 1;
        }
        if start + 2 < end {
            buffer.merge_out_clusters(start, end);
            let t = buffer.out_info[end - 2];
            buffer.out_info.copy_within(start..end - 2, start + 1);
            buffer.out_info[start] = t;
        } else if start != 0 && buffer.cluster_level == ClusterLevel::MonotoneGraphemes {
            buffer.merge_out_clusters(start - 1, end);
        }
    }
    buffer.sync();
    if plan.props.script == crate::buffer::tag(b"Thai") && !plan.map.found_script[0] {
        pua_shaping(buffer, font);
    }
}
