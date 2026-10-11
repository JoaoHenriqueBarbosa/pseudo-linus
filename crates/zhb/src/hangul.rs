//! O shaper hangul do HarfBuzz 10.2.0 (`hb-ot-shaper-hangul.cc`): compõe sequências de jamo em
//! sílabas pré-compostas quando a fonte as tem, decompõe as sílabas que a fonte não tem, marca os
//! jamo soltos com `ljmo`, `vjmo` e `tjmo`, e põe as marcas de tom antes da sílaba.

use crate::buffer::{tag, Buffer, ClusterLevel, FLAG_DO_NOT_INSERT_DOTTED_CIRCLE};
use crate::font::Font;
use crate::map::{Map, MapBuilder, F_NONE};

const LJMO: u8 = 1;
const VJMO: u8 = 2;
const TJMO: u8 = 3;
const FEATURES: [&[u8; 4]; 3] = [b"ljmo", b"vjmo", b"tjmo"];

const L_BASE: u32 = 0x1100;
const V_BASE: u32 = 0x1161;
const T_BASE: u32 = 0x11A7;
const L_COUNT: u32 = 19;
const V_COUNT: u32 = 21;
const T_COUNT: u32 = 28;
const S_BASE: u32 = 0xAC00;
const N_COUNT: u32 = V_COUNT * T_COUNT;
const S_COUNT: u32 = L_COUNT * N_COUNT;

fn is_combining_l(u: u32) -> bool {
    (L_BASE..L_BASE + L_COUNT).contains(&u)
}

fn is_combining_v(u: u32) -> bool {
    (V_BASE..V_BASE + V_COUNT).contains(&u)
}

fn is_combining_t(u: u32) -> bool {
    (T_BASE + 1..T_BASE + T_COUNT).contains(&u)
}

fn is_combined_s(u: u32) -> bool {
    (S_BASE..S_BASE + S_COUNT).contains(&u)
}

fn is_l(u: u32) -> bool {
    (0x1100..=0x115F).contains(&u) || (0xA960..=0xA97C).contains(&u)
}

fn is_v(u: u32) -> bool {
    (0x1160..=0x11A7).contains(&u) || (0xD7B0..=0xD7C6).contains(&u)
}

fn is_t(u: u32) -> bool {
    (0x11A8..=0x11FF).contains(&u) || (0xD7CB..=0xD7FB).contains(&u)
}

fn is_tone(u: u32) -> bool {
    (0x302E..=0x302F).contains(&u)
}

/// `collect_features_hangul`.
pub fn collect_features(map: &mut MapBuilder) {
    for t in FEATURES {
        map.add_feature(tag(t), F_NONE, 1);
    }
}

/// `override_features_hangul`: o `calt` estragaria os jamo.
pub fn override_features(map: &mut MapBuilder) {
    map.disable_feature(tag(b"calt"));
}

/// `hangul_shape_plan_t`.
pub struct HangulPlan {
    mask_array: [u32; 4],
}

impl HangulPlan {
    /// `data_create_hangul`.
    pub fn new(map: &Map) -> HangulPlan {
        let mut mask_array = [0; 4];
        for (i, t) in FEATURES.iter().enumerate() {
            mask_array[i + 1] = map.one_mask(tag(t));
        }
        HangulPlan { mask_array }
    }
}

/// `setup_masks_hangul`.
pub fn setup_masks(plan: &HangulPlan, buffer: &mut Buffer) {
    for info in &mut buffer.info {
        info.mask |= plan.mask_array[usize::from(info.shaper_aux & 3)];
    }
}

fn has_glyph(font: &Font, u: u32) -> bool {
    font.nominal_glyph(u).is_some()
}

/// `is_zero_width_char`.
fn is_zero_width_char(font: &Font, u: u32) -> bool {
    font.nominal_glyph(u).is_some_and(|g| font.h_advance(g) == 0)
}

/// `preprocess_text_hangul`.
pub fn preprocess_text(buffer: &mut Buffer, font: &Font) {
    buffer.clear_output();
    // A extensão da última sílaba vista, no buffer de saída.
    let (mut start, mut end) = (0usize, 0usize);
    let count = buffer.len();
    buffer.idx = 0;
    while buffer.idx < count && buffer.successful {
        let u = buffer.cur(0).codepoint;

        if is_tone(u) {
            // A marca de tom vai para antes da sílaba; sem sílaba, ganha um círculo pontilhado.
            if start < end && end == buffer.out_len() {
                buffer.unsafe_to_break_from_outbuffer(start, buffer.idx);
                if !buffer.next_glyph() {
                    break;
                }
                if !is_zero_width_char(font, u) {
                    buffer.merge_out_clusters(start, end + 1);
                    buffer.out_info[start..=end].rotate_right(1);
                }
            } else if buffer.flags & FLAG_DO_NOT_INSERT_DOTTED_CIRCLE == 0 && has_glyph(font, 0x25CC) {
                let chars = if is_zero_width_char(font, u) { [0x25CC, u] } else { [u, 0x25CC] };
                buffer.replace_glyphs(1, &chars);
            } else {
                buffer.next_glyph();
            }
            start = buffer.out_len();
            end = start;
            continue;
        }

        start = buffer.out_len();

        if is_l(u) && buffer.idx + 1 < count {
            let l = u;
            let v = buffer.cur(1).codepoint;
            if is_v(v) {
                // Sequência L, V e talvez T: compõe se a fonte tem a sílaba.
                let mut t = 0;
                let mut tindex = 0;
                if buffer.idx + 2 < count {
                    t = buffer.cur(2).codepoint;
                    if is_t(t) {
                        tindex = t - T_BASE;
                    } else {
                        t = 0;
                    }
                }
                let idx = buffer.idx;
                buffer.unsafe_to_break(idx, idx + if t != 0 { 3 } else { 2 });

                if is_combining_l(l) && is_combining_v(v) && (t == 0 || is_combining_t(t)) {
                    let s = S_BASE + (l - L_BASE) * N_COUNT + (v - V_BASE) * T_COUNT + tindex;
                    if has_glyph(font, s) {
                        buffer.replace_glyphs(if t != 0 { 3 } else { 2 }, &[s]);
                        end = start + 1;
                        continue;
                    }
                }

                // Sem a sílaba: os jamo ficam e ganham as features de posição.
                let i = buffer.idx;
                buffer.info[i].shaper_aux = LJMO;
                buffer.next_glyph();
                let i = buffer.idx;
                buffer.info[i].shaper_aux = VJMO;
                buffer.next_glyph();
                if t != 0 {
                    let i = buffer.idx;
                    buffer.info[i].shaper_aux = TJMO;
                    buffer.next_glyph();
                    end = start + 3;
                } else {
                    end = start + 2;
                }
                if !buffer.successful {
                    break;
                }
                if buffer.cluster_level == ClusterLevel::MonotoneGraphemes {
                    buffer.merge_out_clusters(start, end);
                }
                continue;
            }
        } else if is_combined_s(u) {
            // Sílaba pré-composta: soma um T seguinte, ou decompõe se a fonte não a tem.
            let s = u;
            let has = has_glyph(font, s);
            let lindex = (s - S_BASE) / N_COUNT;
            let nindex = (s - S_BASE) % N_COUNT;
            let vindex = nindex / T_COUNT;
            let tindex = nindex % T_COUNT;

            if tindex == 0 && buffer.idx + 1 < count && is_combining_t(buffer.cur(1).codepoint) {
                let new_tindex = buffer.cur(1).codepoint - T_BASE;
                let new_s = s + new_tindex;
                if has_glyph(font, new_s) {
                    buffer.replace_glyphs(2, &[new_s]);
                    end = start + 1;
                    continue;
                }
                let idx = buffer.idx;
                buffer.unsafe_to_break(idx, idx + 2);
            }

            let followed_by_t = tindex == 0 && buffer.idx + 1 < count && is_t(buffer.cur(1).codepoint);
            if !has || followed_by_t {
                let decomposed = [L_BASE + lindex, V_BASE + vindex, T_BASE + tindex];
                if has_glyph(font, decomposed[0])
                    && has_glyph(font, decomposed[1])
                    && (tindex == 0 || has_glyph(font, decomposed[2]))
                {
                    let mut s_len = if tindex != 0 { 3 } else { 2 };
                    buffer.replace_glyphs(1, &decomposed[..s_len]);
                    if has && tindex == 0 {
                        buffer.next_glyph();
                        s_len += 1;
                    }
                    if !buffer.successful {
                        break;
                    }
                    end = start + s_len;
                    let mut i = start;
                    buffer.out_info[i].shaper_aux = LJMO;
                    i += 1;
                    buffer.out_info[i].shaper_aux = VJMO;
                    i += 1;
                    if i < end {
                        buffer.out_info[i].shaper_aux = TJMO;
                    }
                    if buffer.cluster_level == ClusterLevel::MonotoneGraphemes {
                        buffer.merge_out_clusters(start, end);
                    }
                    continue;
                } else if followed_by_t {
                    let idx = buffer.idx;
                    buffer.unsafe_to_break(idx, idx + 2);
                }
            }

            if has {
                end = start + 1;
            }
        }

        buffer.next_glyph();
    }
    buffer.sync();
}
