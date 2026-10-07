//! O shaper árabe do HarfBuzz 10.2.0 (`hb-ot-shaper-arabic.cc` e
//! `hb-ot-shaper-arabic-fallback.hh`): máquina de junção, features por posição na palavra,
//! `stch`, reordenação das marcas modificadoras e o fallback por formas de apresentação para
//! fontes sem GSUB árabe.

use crate::arabic_table::{
    JOINING, LIGATURE_3_TABLE, LIGATURE_MARK_TABLE, LIGATURE_TABLE, SHAPING_TABLE, SHAPING_TABLE_FIRST,
    SHAPING_TABLE_LAST,
};
use crate::buffer::{scratch, tag, Buffer, Direction, SegmentProperties};
use crate::font::Font;
use crate::gsubgpos::{apply_string, ApplyContext};
use crate::map::{Map, MapBuilder, Pause, F_HAS_FALLBACK, F_MANUAL_ZWJ, F_NONE};
use crate::ot::{GsubGpos, Lookup};
use crate::unicode::{self, gc};

const SCRIPT_ARABIC: u32 = tag(b"Arab");
const SCRIPT_MONGOLIAN: u32 = tag(b"Mong");

/// `HB_BUFFER_SCRATCH_FLAG_ARABIC_HAS_STCH`.
const HAS_STCH: u32 = scratch::SHAPER0;

const JOINING_TYPE_U: u8 = 0;
const JOINING_TYPE_R: u8 = 2;
const JOINING_TYPE_T: u8 = 7;
const JOINING_TYPE_X: u8 = 8;

const ISOL: u8 = 0;
const FINA: u8 = 1;
const FIN2: u8 = 2;
const FIN3: u8 = 3;
const MEDI: u8 = 4;
const MED2: u8 = 5;
const INIT: u8 = 6;
const NONE: u8 = 7;
const NUM_FEATURES: usize = 7;
const STCH_FIXED: u8 = 8;
const STCH_REPEATING: u8 = 9;

const ARABIC_FEATURES: [&[u8; 4]; NUM_FEATURES] = [b"isol", b"fina", b"fin2", b"fin3", b"medi", b"med2", b"init"];

fn feature_is_syriac(t: &[u8; 4]) -> bool {
    matches!(t[3], b'2' | b'3')
}

/// `arabic_state_table`: (ação no anterior, ação no atual, próximo estado), colunas
/// U, L, R, D, ALAPH, DALATH_RISH.
const STATE_TABLE: [[(u8, u8, u8); 6]; 7] = [
    [(NONE, NONE, 0), (NONE, ISOL, 2), (NONE, ISOL, 1), (NONE, ISOL, 2), (NONE, ISOL, 1), (NONE, ISOL, 6)],
    [(NONE, NONE, 0), (NONE, ISOL, 2), (NONE, ISOL, 1), (NONE, ISOL, 2), (NONE, FIN2, 5), (NONE, ISOL, 6)],
    [(NONE, NONE, 0), (NONE, ISOL, 2), (INIT, FINA, 1), (INIT, FINA, 3), (INIT, FINA, 4), (INIT, FINA, 6)],
    [(NONE, NONE, 0), (NONE, ISOL, 2), (MEDI, FINA, 1), (MEDI, FINA, 3), (MEDI, FINA, 4), (MEDI, FINA, 6)],
    [(NONE, NONE, 0), (NONE, ISOL, 2), (MED2, ISOL, 1), (MED2, ISOL, 2), (MED2, FIN2, 5), (MED2, ISOL, 6)],
    [(NONE, NONE, 0), (NONE, ISOL, 2), (ISOL, ISOL, 1), (ISOL, ISOL, 2), (ISOL, FIN2, 5), (ISOL, ISOL, 6)],
    [(NONE, NONE, 0), (NONE, ISOL, 2), (NONE, ISOL, 1), (NONE, ISOL, 2), (NONE, FIN3, 5), (NONE, ISOL, 6)],
];

fn joining_type_raw(u: u32) -> u8 {
    let i = JOINING.partition_point(|r| r.1 < u);
    match JOINING.get(i) {
        Some(r) if r.0 <= u => r.2,
        _ => JOINING_TYPE_X,
    }
}

/// `get_joining_type`.
fn joining_type(u: u32, gen_cat: u8) -> u8 {
    let j = joining_type_raw(u);
    if j != JOINING_TYPE_X {
        return j;
    }
    if gc::flag(gen_cat) & (gc::flag(gc::NON_SPACING_MARK) | gc::flag(gc::ENCLOSING_MARK) | gc::flag(gc::FORMAT)) != 0 {
        JOINING_TYPE_T
    } else {
        JOINING_TYPE_U
    }
}

/// `HB_ARABIC_GENERAL_CATEGORY_IS_WORD`.
fn general_category_is_word(g: u8) -> bool {
    let word = [
        gc::UNASSIGNED,
        gc::PRIVATE_USE,
        gc::MODIFIER_LETTER,
        gc::OTHER_LETTER,
        gc::SPACING_MARK,
        gc::ENCLOSING_MARK,
        gc::NON_SPACING_MARK,
        gc::DECIMAL_NUMBER,
        gc::LETTER_NUMBER,
        gc::OTHER_NUMBER,
        gc::CURRENCY_SYMBOL,
        gc::MODIFIER_SYMBOL,
        gc::MATH_SYMBOL,
        gc::OTHER_SYMBOL,
    ];
    word.iter().any(|&w| w == g)
}

/// `collect_features_arabic`.
pub fn collect_features(map: &mut MapBuilder, props: &SegmentProperties) {
    map.enable_feature(tag(b"stch"), F_NONE, 1);
    map.add_gsub_pause(Some(Pause::ArabicRecordStch));
    map.enable_feature(tag(b"ccmp"), F_MANUAL_ZWJ, 1);
    map.enable_feature(tag(b"locl"), F_MANUAL_ZWJ, 1);
    map.add_gsub_pause(None);
    for f in ARABIC_FEATURES {
        let has_fallback = props.script == SCRIPT_ARABIC && !feature_is_syriac(f);
        map.add_feature(tag(f), F_MANUAL_ZWJ | if has_fallback { F_HAS_FALLBACK } else { F_NONE }, 1);
        map.add_gsub_pause(None);
    }
    map.add_gsub_pause(Some(Pause::ArabicDeallocate));
    map.enable_feature(tag(b"rlig"), F_MANUAL_ZWJ | F_HAS_FALLBACK, 1);
    if props.script == SCRIPT_ARABIC {
        map.add_gsub_pause(Some(Pause::ArabicFallback));
    }
    map.enable_feature(tag(b"calt"), F_MANUAL_ZWJ, 1);
    if !map.has_feature(tag(b"rclt")) {
        map.add_gsub_pause(None);
    }
    map.enable_feature(tag(b"liga"), F_MANUAL_ZWJ, 1);
    map.enable_feature(tag(b"clig"), F_MANUAL_ZWJ, 1);
    map.enable_feature(tag(b"mset"), F_MANUAL_ZWJ, 1);
}

/// Um lookup de GSUB sintetizado pelo fallback, com a máscara da feature que o liga.
struct FallbackLookup {
    mask: u32,
    data: Vec<u8>,
}

/// `arabic_shape_plan_t`.
pub struct ArabicPlan {
    mask_array: [u32; NUM_FEATURES + 1],
    do_fallback: bool,
    has_stch: bool,
    fallback: Vec<FallbackLookup>,
}

impl ArabicPlan {
    /// `data_create_arabic`; o plano de fallback, que o C cria na primeira vez que precisa,
    /// sai já pronto aqui.
    pub fn new(map: &Map, props: &SegmentProperties, font: &Font) -> ArabicPlan {
        let mut do_fallback = props.script == SCRIPT_ARABIC;
        let has_stch = map.one_mask(tag(b"stch")) != 0;
        let mut mask_array = [0; NUM_FEATURES + 1];
        for (i, f) in ARABIC_FEATURES.iter().enumerate() {
            mask_array[i] = map.one_mask(tag(f));
            do_fallback = do_fallback && (feature_is_syriac(f) || map.needs_fallback(tag(f)));
        }
        let fallback = if do_fallback { fallback_plan(map, font) } else { Vec::new() };
        ArabicPlan { mask_array, do_fallback, has_stch, fallback }
    }
}

/// `arabic_joining`.
fn arabic_joining(buffer: &mut Buffer) {
    let count = buffer.len();
    let mut prev: Option<usize> = None;
    let mut state = 0usize;

    for i in 0..buffer.context_len[0] {
        let u = buffer.context[0][i];
        let t = joining_type(u, unicode::general_category(u));
        if t == JOINING_TYPE_T {
            continue;
        }
        state = usize::from(STATE_TABLE[state][usize::from(t)].2);
        break;
    }

    for i in 0..count {
        let t = joining_type(buffer.info[i].codepoint, buffer.info[i].general_category());
        if t == JOINING_TYPE_T {
            buffer.info[i].shaper_aux = NONE;
            continue;
        }
        let entry = STATE_TABLE[state][usize::from(t)];
        match prev {
            Some(p) if entry.0 != NONE => {
                buffer.info[p].shaper_aux = entry.0;
                buffer.safe_to_insert_tatweel(p, i + 1);
            }
            None => {
                if t >= JOINING_TYPE_R {
                    buffer.unsafe_to_concat_from_outbuffer(0, i + 1);
                }
            }
            Some(p) => {
                if t >= JOINING_TYPE_R || (2..=5).contains(&state) {
                    buffer.unsafe_to_concat(p, i + 1);
                }
            }
        }
        buffer.info[i].shaper_aux = entry.1;
        prev = Some(i);
        state = usize::from(entry.2);
    }

    for i in 0..buffer.context_len[1] {
        let u = buffer.context[1][i];
        let t = joining_type(u, unicode::general_category(u));
        if t == JOINING_TYPE_T {
            continue;
        }
        let entry = STATE_TABLE[state][usize::from(t)];
        if entry.0 != NONE && prev.is_some() {
            let p = prev.unwrap_or(0);
            buffer.info[p].shaper_aux = entry.0;
            let len = buffer.len();
            buffer.safe_to_insert_tatweel(p, len);
        } else if (2..=5).contains(&state) {
            // O C passa `prev` mesmo quando ele é `UINT_MAX`, o que só acontece com o buffer vazio.
            let len = buffer.len();
            buffer.unsafe_to_concat(prev.unwrap_or(usize::MAX), len);
        }
        break;
    }
}

/// `mongolian_variation_selectors`.
fn mongolian_variation_selectors(buffer: &mut Buffer) {
    for i in 1..buffer.len() {
        let u = buffer.info[i].codepoint;
        if (0x180B..=0x180D).contains(&u) || u == 0x180F {
            buffer.info[i].shaper_aux = buffer.info[i - 1].shaper_aux;
        }
    }
}

/// `setup_masks_arabic_plan`.
pub fn setup_masks(plan: &ArabicPlan, buffer: &mut Buffer, script: u32) {
    arabic_joining(buffer);
    if script == SCRIPT_MONGOLIAN {
        mongolian_variation_selectors(buffer);
    }
    for info in &mut buffer.info {
        info.mask |= plan.mask_array.get(usize::from(info.shaper_aux)).copied().unwrap_or(0);
    }
}

/// As pausas do shaper árabe dentro do GSUB.
pub fn pause(p: Pause, plan: &ArabicPlan, font: &Font, buffer: &mut Buffer) {
    match p {
        Pause::ArabicRecordStch => record_stch(plan, buffer),
        Pause::ArabicDeallocate => {}
        Pause::ArabicFallback => fallback_shape(plan, font, buffer),
        _ => {}
    }
}

/// `record_stch`.
fn record_stch(plan: &ArabicPlan, buffer: &mut Buffer) {
    if !plan.has_stch {
        return;
    }
    let mut found = false;
    for info in &mut buffer.info {
        if info.multiplied() {
            info.shaper_aux = if info.lig_comp() % 2 != 0 { STCH_REPEATING } else { STCH_FIXED };
            found = true;
        }
    }
    if found {
        buffer.scratch_flags |= HAS_STCH;
    }
}

fn is_stch(a: u8) -> bool {
    (STCH_FIXED..=STCH_REPEATING).contains(&a)
}

/// `apply_stch` (o `postprocess_glyphs` do shaper).
pub fn postprocess_glyphs(buffer: &mut Buffer, font: &Font) {
    if buffer.scratch_flags & HAS_STCH == 0 {
        return;
    }
    let rtl = buffer.props.direction == Direction::Rtl;
    if !rtl {
        buffer.reverse();
    }
    let sign: i32 = if font.x_scale < 0 { -1 } else { 1 };
    let mut extra_glyphs_needed = 0usize;
    for cut in [false, true] {
        let count = buffer.len();
        let mut out_info = Vec::new();
        let mut out_pos = Vec::new();
        let mut i = count;
        while i > 0 {
            if !is_stch(buffer.info[i - 1].shaper_aux) {
                if cut {
                    out_info.push(buffer.info[i - 1]);
                    out_pos.push(buffer.pos[i - 1]);
                }
                i -= 1;
                continue;
            }
            let (mut w_total, mut w_fixed, mut w_repeating) = (0i32, 0i32, 0i32);
            let (mut n_fixed, mut n_repeating) = (0i32, 0i32);
            let end = i;
            while i > 0 && is_stch(buffer.info[i - 1].shaper_aux) {
                i -= 1;
                let width = font.h_advance(buffer.info[i].codepoint);
                if buffer.info[i].shaper_aux == STCH_FIXED {
                    w_fixed += width;
                    n_fixed += 1;
                } else {
                    w_repeating += width;
                    n_repeating += 1;
                }
            }
            let _ = n_fixed;
            let start = i;
            let mut context = i;
            while context > 0
                && !is_stch(buffer.info[context - 1].shaper_aux)
                && (buffer.info[context - 1].is_default_ignorable()
                    || general_category_is_word(buffer.info[context - 1].general_category()))
            {
                context -= 1;
                w_total += buffer.pos[context].x_advance;
            }

            let mut n_copies = 0i32;
            let mut w_remaining = w_total - w_fixed;
            if sign * w_remaining > sign * w_repeating && sign * w_repeating > 0 {
                n_copies = (sign * w_remaining) / (sign * w_repeating) - 1;
            }
            let mut extra_repeat_overlap = 0i32;
            let shortfall = sign * w_remaining - sign * w_repeating * (n_copies + 1);
            if shortfall > 0 && n_repeating > 0 {
                n_copies += 1;
                let excess = (n_copies + 1) * sign * w_repeating - sign * w_remaining;
                if excess > 0 {
                    extra_repeat_overlap = excess / (n_copies * n_repeating);
                    w_remaining = 0;
                }
            }

            if !cut {
                extra_glyphs_needed += (n_copies * n_repeating) as usize;
            } else {
                buffer.unsafe_to_break(context, end);
                let mut x_offset = w_remaining / 2;
                for k in (start + 1..=end).rev() {
                    let width = font.h_advance(buffer.info[k - 1].codepoint);
                    let mut repeat = 1;
                    if buffer.info[k - 1].shaper_aux == STCH_REPEATING {
                        repeat += n_copies;
                    }
                    buffer.pos[k - 1].x_advance = 0;
                    for n in 0..repeat {
                        if rtl {
                            x_offset -= width;
                            if n > 0 {
                                x_offset += extra_repeat_overlap;
                            }
                        }
                        buffer.pos[k - 1].x_offset = x_offset;
                        out_info.push(buffer.info[k - 1]);
                        out_pos.push(buffer.pos[k - 1]);
                        if !rtl {
                            x_offset += width;
                            if n > 0 {
                                x_offset -= extra_repeat_overlap;
                            }
                        }
                    }
                }
            }
            // O `i++` do C desfeito pelo `i--` do laço: retoma em `start`.
            i = start;
        }
        if !cut {
            if !buffer.ensure(count + extra_glyphs_needed) {
                break;
            }
        } else {
            out_info.reverse();
            out_pos.reverse();
            buffer.info = out_info;
            buffer.pos = out_pos;
        }
    }
    if !rtl {
        buffer.reverse();
    }
}

/// As marcas modificadoras do UTR #53.
const MODIFIER_COMBINING_MARKS: [u32; 14] = [
    0x0654, 0x0655, 0x0658, 0x06DC, 0x06E3, 0x06E7, 0x06E8, 0x08CA, 0x08CB, 0x08CD, 0x08CE, 0x08CF, 0x08D3, 0x08F3,
];

/// `HB_MODIFIED_COMBINING_CLASS_CCC22` e `CCC26`.
const MCC_CCC22: u8 = 25;
const MCC_CCC26: u8 = 26;

/// `reorder_marks_arabic`.
pub fn reorder_marks(buffer: &mut Buffer, mut start: usize, end: usize) {
    let mut i = start;
    for cc in [220u8, 230] {
        while i < end && buffer.info[i].modified_combining_class() < cc {
            i += 1;
        }
        if i == end {
            break;
        }
        if buffer.info[i].modified_combining_class() > cc {
            continue;
        }
        let mut j = i;
        while j < end
            && buffer.info[j].modified_combining_class() == cc
            && MODIFIER_COMBINING_MARKS.contains(&buffer.info[j].codepoint)
        {
            j += 1;
        }
        if i == j {
            continue;
        }
        buffer.merge_clusters(start, j);
        buffer.info[start..j].rotate_right(j - i);
        let new_start = start + j - i;
        let new_cc = if cc == 220 { MCC_CCC22 } else { MCC_CCC26 };
        while start < new_start {
            buffer.info[start].set_modified_combining_class(new_cc);
            start += 1;
        }
        i = j;
    }
}

/// `IgnoreMarks`.
const LOOKUP_FLAG_IGNORE_MARKS: u16 = 0x0008;

fn put16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_be_bytes());
}

/// Um `Lookup` de GSUB com uma subtabela.
fn lookup_bytes(kind: u16, flag: u16, subtable: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    put16(&mut v, kind);
    put16(&mut v, flag);
    put16(&mut v, 1);
    put16(&mut v, 8);
    put16(&mut v, 0);
    v.extend_from_slice(subtable);
    v
}

fn coverage_bytes(glyphs: &[u16]) -> Vec<u8> {
    let mut v = Vec::new();
    put16(&mut v, 1);
    put16(&mut v, glyphs.len() as u16);
    for &g in glyphs {
        put16(&mut v, g);
    }
    v
}

/// `arabic_fallback_synthesize_lookup_single`.
fn synthesize_single(font: &Font, feature_index: usize) -> Option<Vec<u8>> {
    let mut pairs: Vec<(u16, u16)> = Vec::new();
    for u in SHAPING_TABLE_FIRST..=SHAPING_TABLE_LAST {
        let s = u32::from(SHAPING_TABLE[(u - SHAPING_TABLE_FIRST) as usize][feature_index]);
        if s == 0 {
            continue;
        }
        let (Some(ug), Some(sg)) = (font.nominal_glyph(u), font.nominal_glyph(s)) else { continue };
        if ug == sg || ug > 0xFFFF || sg > 0xFFFF {
            continue;
        }
        pairs.push((ug as u16, sg as u16));
    }
    if pairs.is_empty() {
        return None;
    }
    pairs.sort_by_key(|p| p.0);
    let glyphs: Vec<u16> = pairs.iter().map(|p| p.0).collect();
    let mut st = Vec::new();
    put16(&mut st, 2);
    put16(&mut st, (6 + 2 * pairs.len()) as u16);
    put16(&mut st, pairs.len() as u16);
    for p in &pairs {
        put16(&mut st, p.1);
    }
    st.extend_from_slice(&coverage_bytes(&glyphs));
    Some(lookup_bytes(1, LOOKUP_FLAG_IGNORE_MARKS, &st))
}

/// `arabic_fallback_synthesize_lookup_ligature`. Um componente que falha depois de outro já
/// anotado deixa o anotado na lista, como no C, e desalinha os seguintes.
fn synthesize_ligature<const N: usize>(font: &Font, table: &[(u16, &[([u16; N], u16)])], flags: u16) -> Option<Vec<u8>> {
    let mut firsts: Vec<(u16, usize)> = Vec::new();
    for (idx, (first_u, _)) in table.iter().enumerate() {
        if let Some(g) = font.nominal_glyph(u32::from(*first_u)) {
            firsts.push((g as u16, idx));
        }
    }
    firsts.sort_by_key(|f| f.0);
    let mut per_first = vec![0usize; firsts.len()];
    let mut ligatures: Vec<u16> = Vec::new();
    let mut components: Vec<u16> = Vec::new();
    for (i, &(_, idx)) in firsts.iter().enumerate() {
        for (comps, lig_u) in table[idx].1 {
            let Some(lig_g) = font.nominal_glyph(u32::from(*lig_u)) else { continue };
            let mut matched = true;
            for &cu in comps {
                match font.nominal_glyph(u32::from(cu)).filter(|_| cu != 0) {
                    Some(cg) => components.push(cg as u16),
                    None => {
                        matched = false;
                        break;
                    }
                }
            }
            if !matched {
                continue;
            }
            ligatures.push(lig_g as u16);
            per_first[i] += 1;
        }
    }
    if ligatures.is_empty() {
        return None;
    }

    // LigatureSubstFormat1: cobertura no fim, depois dos conjuntos.
    let n = firsts.len();
    let mut sets: Vec<Vec<u8>> = Vec::new();
    let (mut li, mut ci) = (0, 0);
    for &count in &per_first {
        let mut set = Vec::new();
        put16(&mut set, count as u16);
        let mut ligs: Vec<Vec<u8>> = Vec::new();
        for _ in 0..count {
            let mut l = Vec::new();
            put16(&mut l, ligatures[li]);
            put16(&mut l, (N + 1) as u16);
            for k in 0..N {
                put16(&mut l, components.get(ci + k).copied().unwrap_or(0));
            }
            ci += N;
            li += 1;
            ligs.push(l);
        }
        let mut off = 2 + 2 * ligs.len();
        for l in &ligs {
            put16(&mut set, off as u16);
            off += l.len();
        }
        for l in ligs {
            set.extend_from_slice(&l);
        }
        sets.push(set);
    }
    let mut st = Vec::new();
    put16(&mut st, 1);
    let header = 6 + 2 * n;
    let sets_len: usize = sets.iter().map(Vec::len).sum();
    put16(&mut st, (header + sets_len) as u16);
    put16(&mut st, n as u16);
    let mut off = header;
    for s in &sets {
        put16(&mut st, off as u16);
        off += s.len();
    }
    for s in sets {
        st.extend_from_slice(&s);
    }
    let glyphs: Vec<u16> = firsts.iter().map(|f| f.0).collect();
    st.extend_from_slice(&coverage_bytes(&glyphs));
    Some(lookup_bytes(4, flags, &st))
}

/// `arabic_fallback_plan_init_unicode` (o caminho win1256 só existe no Windows).
fn fallback_plan(map: &Map, font: &Font) -> Vec<FallbackLookup> {
    const FEATURES: [&[u8; 4]; 7] = [b"init", b"medi", b"fina", b"isol", b"rlig", b"rlig", b"rlig"];
    let mut out = Vec::new();
    for (i, f) in FEATURES.iter().enumerate() {
        let mask = map.one_mask(tag(f));
        if mask == 0 {
            continue;
        }
        let data = match i {
            0..=3 => synthesize_single(font, i),
            4 => synthesize_ligature(font, &LIGATURE_3_TABLE, LOOKUP_FLAG_IGNORE_MARKS),
            5 => synthesize_ligature(font, &LIGATURE_TABLE, LOOKUP_FLAG_IGNORE_MARKS),
            _ => synthesize_ligature(font, &LIGATURE_MARK_TABLE, 0),
        };
        if let Some(data) = data {
            out.push(FallbackLookup { mask, data });
        }
    }
    out
}

/// `arabic_fallback_shape` com `arabic_fallback_plan_shape`.
fn fallback_shape(plan: &ArabicPlan, font: &Font, buffer: &mut Buffer) {
    if !plan.do_fallback {
        return;
    }
    let mut c = ApplyContext::new(0, font, buffer);
    c.table = GsubGpos::new(None);
    for l in &plan.fallback {
        c.set_lookup_mask(l.mask);
        apply_string(&mut c, Lookup { d: &l.data });
    }
}
