//! Port do layout do libraqm 0.10.2 (`raqm.c`) no recorte que o Pillow usa: uma face, um
//! conjunto de load flags e uma língua para o texto todo. Itemiza por bidi (fribidi) e por
//! script, e faz o shaping de cada run com o texto inteiro como contexto.

use crate::bidi;
use crate::buffer::{Buffer, Direction, FLAG_BOT, FLAG_EOT, SCRIPT_COMMON, SCRIPT_INHERITED, SCRIPT_INVALID};
use crate::font::Font;
use crate::shape::{shape, Feature, Plan};
use crate::unicode;

/// `raqm_direction_t`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParDirection {
    Default,
    Rtl,
    Ltr,
    Ttb,
}

/// `raqm_glyph_t`, com o cluster em índice de codepoint (o Pillow passa UTF-32).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Glyph {
    pub index: u32,
    pub x_advance: i32,
    pub y_advance: i32,
    pub x_offset: i32,
    pub y_offset: i32,
    pub cluster: u32,
}

/// O resultado de `raqm_layout` + `raqm_get_glyphs`.
pub struct Layout {
    pub glyphs: Vec<Glyph>,
    /// `raqm_get_par_resolved_direction`.
    pub resolved: ParDirection,
}

/// Os pares de pontuação que o raqm faz herdar o script do que abriu o par.
const PAIRED_CHARS: [u32; 34] = [
    0x0028, 0x0029, 0x003c, 0x003e, 0x005b, 0x005d, 0x007b, 0x007d, 0x00ab, 0x00bb, 0x2018, 0x2019,
    0x201c, 0x201d, 0x2039, 0x203a, 0x3008, 0x3009, 0x300a, 0x300b, 0x300c, 0x300d, 0x300e, 0x300f,
    0x3010, 0x3011, 0x3014, 0x3015, 0x3016, 0x3017, 0x3018, 0x3019, 0x301a, 0x301b,
];

/// `_raqm_unicode_script`: marca sem espaçamento herda o script da base.
fn unicode_script(u: u32) -> u32 {
    if unicode::general_category(u) == unicode::gc::NON_SPACING_MARK {
        return SCRIPT_INHERITED;
    }
    unicode::script(u)
}

/// `_raqm_resolve_scripts`.
fn resolve_scripts(text: &[u32]) -> Vec<u32> {
    let mut script: Vec<u32> = text.iter().map(|&c| unicode_script(c)).collect();
    let mut last_script_index: i64 = -1;
    let mut last_set_index: i64 = -1;
    let mut last_script = SCRIPT_INVALID;
    // A pilha do raqm é indexada a partir de 1; o topo vazio devolve `HB_SCRIPT_INVALID`.
    let mut stack: Vec<(u32, usize)> = Vec::new();
    for i in 0..text.len() {
        if script[i] == SCRIPT_COMMON && last_script_index != -1 {
            match PAIRED_CHARS.binary_search(&text[i]) {
                Ok(pair_index) if pair_index & 1 == 0 => {
                    script[i] = last_script;
                    last_set_index = i as i64;
                    if stack.len() < text.len() {
                        stack.push((script[i], pair_index));
                    }
                }
                Ok(pair_index) => {
                    while stack.last().is_some_and(|&(_, p)| p != pair_index & !1) {
                        stack.pop();
                    }
                    if let Some(&(s, _)) = stack.last() {
                        script[i] = s;
                        last_script = s;
                    } else {
                        script[i] = last_script;
                    }
                    last_set_index = i as i64;
                }
                Err(_) => {
                    script[i] = last_script;
                    last_set_index = i as i64;
                }
            }
        } else if script[i] == SCRIPT_INHERITED && last_script_index != -1 {
            script[i] = last_script;
            last_set_index = i as i64;
        } else {
            for j in (last_set_index + 1) as usize..i {
                script[j] = script[i];
            }
            last_script = script[i];
            last_script_index = i as i64;
            last_set_index = i as i64;
        }
    }
    for i in (0..text.len().saturating_sub(1)).rev() {
        if script[i] == SCRIPT_INHERITED || script[i] == SCRIPT_COMMON {
            script[i] = script[i + 1];
        }
    }
    script
}

struct BidiRun {
    pos: usize,
    len: usize,
    level: i32,
}

/// `_raqm_reorder_runs`: L1 sobre o fim da linha e L2.
fn reorder_runs(types: &[u32], base_dir: u32, levels: &mut [i32]) -> Vec<BidiRun> {
    let len = levels.len();
    if len == 0 {
        return Vec::new();
    }
    for i in (0..len).rev() {
        if types[i] & (bidi::MASK_EXPLICIT | bidi::MASK_BN | bidi::MASK_WS) == 0 {
            break;
        }
        levels[i] = i32::from(base_dir & bidi::MASK_RTL != 0);
    }
    let max_level = levels.iter().copied().max().unwrap_or(0).max(0);
    let mut runs = Vec::new();
    let mut start = 0;
    while start < len {
        let mut end = start;
        while end < len && levels[start] == levels[end] {
            end += 1;
        }
        runs.push(BidiRun { pos: start, len: end - start, level: levels[start] });
        start = end;
    }
    let count = runs.len() as i64;
    let mut level = max_level;
    while level > 0 {
        let mut i = count - 1;
        while i >= 0 {
            if runs[i as usize].level >= level {
                let end = i;
                i -= 1;
                while i >= 0 && runs[i as usize].level >= level {
                    i -= 1;
                }
                runs[(i + 1) as usize..=end as usize].reverse();
            }
            i -= 1;
        }
        level -= 1;
    }
    runs
}

struct Run {
    pos: usize,
    len: usize,
    direction: Direction,
    script: u32,
}

/// `raqm_layout` seguido de `raqm_get_glyphs`.
pub fn layout(font: &Font, text: &[u32], dir: ParDirection, language: Option<&str>, features: &[Feature]) -> Layout {
    if text.is_empty() {
        return Layout { glyphs: Vec::new(), resolved: ParDirection::Default };
    }
    let script = resolve_scripts(text);

    let (bidi_runs, resolved) = if dir == ParDirection::Ttb {
        (vec![BidiRun { pos: 0, len: text.len(), level: 0 }], ParDirection::Ttb)
    } else {
        let mut par = match dir {
            ParDirection::Rtl => bidi::PAR_RTL,
            ParDirection::Ltr => bidi::PAR_LTR,
            _ => bidi::PAR_ON,
        };
        let types = bidi::bidi_types(text);
        let brackets = bidi::bracket_types(text, &types);
        let mut levels = vec![0; text.len()];
        bidi::par_embedding_levels(&types, &brackets, &mut par, &mut levels);
        let resolved = if par == bidi::PAR_RTL { ParDirection::Rtl } else { ParDirection::Ltr };
        (reorder_runs(&types, par, &mut levels), resolved)
    };

    let hb_dir = |level: i32| {
        if dir == ParDirection::Ttb {
            Direction::Ttb
        } else if level & 1 != 0 {
            Direction::Rtl
        } else {
            Direction::Ltr
        }
    };

    let mut runs: Vec<Run> = Vec::new();
    for br in &bidi_runs {
        let direction = hb_dir(br.level);
        if direction.is_backward() {
            let mut cur = Run { pos: br.pos + br.len - 1, len: 0, direction, script: script[br.pos + br.len - 1] };
            for j in (0..br.len).rev() {
                let p = br.pos + j;
                if script[cur.pos] != script[p] {
                    runs.push(cur);
                    cur = Run { pos: p, len: 1, direction, script: script[p] };
                } else {
                    cur.len += 1;
                    cur.pos = p;
                }
            }
            runs.push(cur);
        } else {
            let mut cur = Run { pos: br.pos, len: 0, direction, script: script[br.pos] };
            for j in 0..br.len {
                let p = br.pos + j;
                if script[cur.pos] != script[p] {
                    runs.push(cur);
                    cur = Run { pos: p, len: 1, direction, script: script[p] };
                } else {
                    cur.len += 1;
                }
            }
            runs.push(cur);
        }
    }

    let mut glyphs = Vec::new();
    for run in &runs {
        let mut buffer = Buffer::new();
        buffer.add_codepoints(text, run.pos, run.len);
        buffer.props.script = run.script;
        buffer.props.language = language.map(str::to_string);
        buffer.props.direction = run.direction;
        buffer.flags = FLAG_BOT | FLAG_EOT;
        let plan = Plan::new(font, &buffer.props, features);
        shape(&plan, font, &mut buffer);
        for (i, p) in buffer.info.iter().zip(&buffer.pos) {
            glyphs.push(Glyph {
                index: i.codepoint,
                x_advance: p.x_advance,
                y_advance: p.y_advance,
                x_offset: p.x_offset,
                y_offset: p.y_offset,
                cluster: i.cluster,
            });
        }
    }
    Layout { glyphs, resolved }
}
