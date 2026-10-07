//! `hb-ot-shaper-use.cc`: o Universal Shaping Engine, com a máquina de sílabas
//! (`hb-ot-shaper-use-machine.rl`), a inserção de círculos pontilhados do
//! `hb-ot-shaper-syllabic.cc` e as restrições de vogais do `hb-ot-shaper-vowel-constraints.cc`.

use crate::arabic::ArabicPlan;
use crate::buffer::{scratch, tag, Buffer, GlyphInfo, FLAG_DO_NOT_INSERT_DOTTED_CIRCLE};
use crate::font::Font;
use crate::map::{Map, MapBuilder, Pause, F_MANUAL_ZWJ, F_NONE, F_PER_SYLLABLE};
use crate::props::next_syllable;
use crate::unicode;
use crate::use_machine as m;
use crate::use_table::CATEGORIES;

/// As categorias USE (`use_syllable_machine_ex_*`) que o código consulta.
pub mod cat {
    pub const O: u8 = 0;
    pub const B: u8 = 1;
    pub const CGJ: u8 = 6;
    pub const H: u8 = 12;
    pub const ZWNJ: u8 = 14;
    pub const R: u8 = 18;
    pub const VPRE: u8 = 22;
    pub const VMPRE: u8 = 23;
    pub const FABV: u8 = 24;
    pub const FBLW: u8 = 25;
    pub const FPST: u8 = 26;
    pub const MABV: u8 = 27;
    pub const MBLW: u8 = 28;
    pub const MPST: u8 = 29;
    pub const MPRE: u8 = 30;
    pub const VABV: u8 = 33;
    pub const VBLW: u8 = 34;
    pub const VPST: u8 = 35;
    pub const VMABV: u8 = 37;
    pub const VMBLW: u8 = 38;
    pub const VMPST: u8 = 39;
    pub const IS: u8 = 44;
    pub const FMABV: u8 = 45;
    pub const FMBLW: u8 = 46;
    pub const FMPST: u8 = 47;
    pub const HVM: u8 = 53;
}

/// `use_syllable_type_t`.
mod syllable {
    pub const VIRAMA_TERMINATED: u8 = 0;
    pub const SAKOT_TERMINATED: u8 = 1;
    pub const STANDARD: u8 = 2;
    pub const NUMBER_JOINER_TERMINATED: u8 = 3;
    pub const NUMERAL: u8 = 4;
    pub const SYMBOL: u8 = 5;
    pub const HIEROGLYPH: u8 = 6;
    pub const BROKEN: u8 = 7;
    pub const NON: u8 = 8;
}

/// Os scripts que o `hb_ot_shaper_categorize` manda para o USE (se a fonte não escolheu `DFLT`
/// nem `latn`).
pub const SCRIPTS: &[&[u8; 4]] = &[
    b"Tibt", b"Mong", b"Sinh", b"Buhd", b"Hano", b"Tglg", b"Tagb", b"Limb", b"Tale", b"Bugi", b"Khar", b"Sylo",
    b"Tfng", b"Bali", b"Nkoo", b"Phag", b"Cham", b"Kali", b"Lepc", b"Rjng", b"Saur", b"Sund", b"Egyp", b"Java",
    b"Kthi", b"Mtei", b"Lana", b"Tavt", b"Batk", b"Brah", b"Mand", b"Cakm", b"Plrd", b"Shrd", b"Takr", b"Dupl",
    b"Gran", b"Khoj", b"Sind", b"Mahj", b"Mani", b"Modi", b"Hmng", b"Phlp", b"Sidd", b"Tirh", b"Ahom", b"Mult",
    b"Adlm", b"Bhks", b"Marc", b"Newa", b"Gonm", b"Soyo", b"Zanb", b"Dogr", b"Gong", b"Rohg", b"Maka", b"Medf",
    b"Sogo", b"Sogd", b"Elym", b"Nand", b"Hmnp", b"Wcho", b"Chrs", b"Diak", b"Kits", b"Yezi", b"Cpmn", b"Ougr",
    b"Tnsa", b"Toto", b"Vith", b"Kawi", b"Nagm", b"Gara", b"Gukh", b"Krai", b"Onao", b"Sunu", b"Todr", b"Tutg",
];

pub fn is_use_script(script: u32) -> bool {
    SCRIPTS.iter().any(|s| tag(s) == script)
}

/// `has_arabic_joining`.
pub fn has_arabic_joining(script: u32) -> bool {
    [b"Adlm", b"Arab", b"Chrs", b"Rohg", b"Mand", b"Mani", b"Mong", b"Nkoo", b"Ougr", b"Phag", b"Phlp", b"Sogd", b"Syrc"]
        .iter()
        .any(|s| tag(s) == script)
}

/// `hb_use_get_category`.
pub fn category(u: u32) -> u8 {
    match CATEGORIES.binary_search_by(|&(s, e, _)| {
        if e < u {
            std::cmp::Ordering::Less
        } else if s > u {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    }) {
        Ok(i) => CATEGORIES[i].2,
        Err(_) => cat::O,
    }
}

const BASIC_FEATURES: [&[u8; 4]; 7] = [b"rkrf", b"abvf", b"blwf", b"half", b"pstf", b"vatu", b"cjct"];
const TOPOGRAPHICAL_FEATURES: [&[u8; 4]; 4] = [b"isol", b"init", b"medi", b"fina"];
const OTHER_FEATURES: [&[u8; 4]; 5] = [b"abvs", b"blws", b"haln", b"pres", b"psts"];

/// `collect_features_use`.
pub fn collect_features(map: &mut MapBuilder) {
    map.add_gsub_pause(Some(Pause::UseSetupSyllables));
    map.enable_feature(tag(b"locl"), F_PER_SYLLABLE, 1);
    map.enable_feature(tag(b"ccmp"), F_PER_SYLLABLE, 1);
    map.enable_feature(tag(b"nukt"), F_PER_SYLLABLE, 1);
    map.enable_feature(tag(b"akhn"), F_MANUAL_ZWJ | F_PER_SYLLABLE, 1);
    map.add_gsub_pause(Some(Pause::ClearSubstitutionFlags));
    map.add_feature(tag(b"rphf"), F_MANUAL_ZWJ | F_PER_SYLLABLE, 1);
    map.add_gsub_pause(Some(Pause::UseRecordRphf));
    map.add_gsub_pause(Some(Pause::ClearSubstitutionFlags));
    map.enable_feature(tag(b"pref"), F_MANUAL_ZWJ | F_PER_SYLLABLE, 1);
    map.add_gsub_pause(Some(Pause::UseRecordPref));
    for f in BASIC_FEATURES {
        map.enable_feature(tag(f), F_MANUAL_ZWJ | F_PER_SYLLABLE, 1);
    }
    map.add_gsub_pause(Some(Pause::UseReorder));
    map.add_gsub_pause(Some(Pause::SyllabicClearVar));
    for f in TOPOGRAPHICAL_FEATURES {
        map.add_feature(tag(f), F_NONE, 1);
    }
    map.add_gsub_pause(None);
    for f in OTHER_FEATURES {
        map.enable_feature(tag(f), F_MANUAL_ZWJ, 1);
    }
}

/// `use_shape_plan_t`.
pub struct UsePlan {
    rphf_mask: u32,
    arabic: Option<ArabicPlan>,
    /// As máscaras de isol, init, medi e fina (zero onde coincidem com a global).
    topographical: [u32; 4],
}

impl UsePlan {
    /// `data_create_use`.
    pub fn new(map: &Map, props: &crate::buffer::SegmentProperties, font: &Font) -> UsePlan {
        let mut topographical = [0; 4];
        for (i, f) in TOPOGRAPHICAL_FEATURES.iter().enumerate() {
            let mask = map.one_mask(tag(f));
            topographical[i] = if mask == map.global_mask { 0 } else { mask };
        }
        UsePlan {
            rphf_mask: map.one_mask(tag(b"rphf")),
            arabic: has_arabic_joining(props.script).then(|| ArabicPlan::new(map, props, font)),
            topographical,
        }
    }
}

/// `setup_masks_use`.
pub fn setup_masks(plan: &UsePlan, buffer: &mut Buffer, script: u32) {
    if let Some(arabic) = &plan.arabic {
        crate::arabic::setup_masks(arabic, buffer, script);
    }
    for info in &mut buffer.info {
        info.shaper_cat = category(info.codepoint);
    }
}

/// `compose_use`: não recompõe matras partidas.
pub fn compose(a: u32, b: u32) -> Option<u32> {
    if unicode::gc::is_mark(unicode::general_category(a)) {
        return None;
    }
    crate::normalize::compose_unicode(a, b)
}

/// As pausas do USE.
pub fn pause(p: Pause, plan: &UsePlan, font: &Font, buffer: &mut Buffer) {
    match p {
        Pause::UseSetupSyllables => setup_syllables(plan, buffer),
        Pause::ClearSubstitutionFlags => {
            for info in &mut buffer.info {
                info.clear_substituted();
            }
        }
        Pause::UseRecordRphf => record_rphf(plan, buffer),
        Pause::UseRecordPref => record_pref(buffer),
        Pause::UseReorder => reorder(font, buffer),
        // `hb_syllabic_clear_var` só libera a variável no C.
        Pause::SyllabicClearVar => {}
        _ => {}
    }
}

/// `find_syllables_use`: a máquina do Ragel sobre os glifos filtrados (sem CGJ, e sem ZWNJ
/// seguido de marca).
fn find_syllables(buffer: &mut Buffer) {
    let len = buffer.len();
    let not_cgj = |i: &GlyphInfo| i.shaper_cat != cat::CGJ;
    let mut idx: Vec<usize> = Vec::new();
    for i in 0..len {
        let info = &buffer.info[i];
        if !not_cgj(info) {
            continue;
        }
        if info.shaper_cat == cat::ZWNJ {
            if let Some(j) = (i + 1..len).find(|&j| not_cgj(&buffer.info[j])) {
                if buffer.info[j].is_unicode_mark() {
                    continue;
                }
            }
        }
        idx.push(i);
    }
    let n = idx.len() as isize;
    // A posição filtrada `p` cobre os glifos originais a partir de `orig(p)`; o fim é `len`.
    let orig = |p: isize| -> usize { if p >= n { len } else { idx[p as usize] } };
    let (pe, eof) = (n, n);
    let mut cs = 1usize;
    let mut ts: Option<isize> = None;
    let mut te: isize = 0;
    let mut act = 0u32;
    let mut p: isize = 0;
    let mut serial: u8 = 1;

    let mut found = |buffer: &mut Buffer, ts: Option<isize>, te: isize, kind: u8| {
        let (s, e) = (orig(ts.unwrap_or(0)), orig(te));
        for i in s..e {
            buffer.info[i].syllable = (serial << 4) | kind;
        }
        serial += 1;
        if serial == 16 {
            serial = 1;
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
                if p < 0 || p >= pe {
                    break;
                }
                if m::FROM_STATE_ACTIONS[cs] == 3 {
                    ts = Some(p);
                }
                let keys = cs << 1;
                let inds = usize::from(m::INDEX_OFFSETS[cs]);
                let slen = usize::from(m::KEY_SPANS[cs]);
                let c = buffer.info[idx[p as usize]].shaper_cat;
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
                    7 => te = p + 1,
                    16 => {
                        te = p + 1;
                        found(buffer, ts, te, VIRAMA_TERMINATED);
                    }
                    14 => {
                        te = p + 1;
                        found(buffer, ts, te, SAKOT_TERMINATED);
                    }
                    12 => {
                        te = p + 1;
                        found(buffer, ts, te, STANDARD);
                    }
                    20 => {
                        te = p + 1;
                        found(buffer, ts, te, NUMBER_JOINER_TERMINATED);
                    }
                    18 => {
                        te = p + 1;
                        found(buffer, ts, te, NUMERAL);
                    }
                    10 => {
                        te = p + 1;
                        found(buffer, ts, te, SYMBOL);
                    }
                    25 => {
                        te = p + 1;
                        found(buffer, ts, te, HIEROGLYPH);
                    }
                    5 => {
                        te = p + 1;
                        found(buffer, ts, te, BROKEN);
                        buffer.scratch_flags |= scratch::HAS_BROKEN_SYLLABLE;
                    }
                    4 => {
                        te = p + 1;
                        found(buffer, ts, te, NON);
                    }
                    15 | 13 | 11 | 19 | 17 | 9 | 24 | 21 | 23 => {
                        te = p;
                        p -= 1;
                        let kind = match m::TRANS_ACTIONS[trans] {
                            15 => VIRAMA_TERMINATED,
                            13 => SAKOT_TERMINATED,
                            11 => STANDARD,
                            19 => NUMBER_JOINER_TERMINATED,
                            17 => NUMERAL,
                            9 => SYMBOL,
                            24 => HIEROGLYPH,
                            21 => BROKEN,
                            _ => NON,
                        };
                        found(buffer, ts, te, kind);
                        if kind == BROKEN {
                            buffer.scratch_flags |= scratch::HAS_BROKEN_SYLLABLE;
                        }
                    }
                    1 => {
                        p = te - 1;
                        found(buffer, ts, te, SYMBOL);
                    }
                    22 => match act {
                        8 => {
                            p = te - 1;
                            found(buffer, ts, te, NON);
                        }
                        9 => {
                            p = te - 1;
                            found(buffer, ts, te, BROKEN);
                            buffer.scratch_flags |= scratch::HAS_BROKEN_SYLLABLE;
                        }
                        _ => {}
                    },
                    6 => {
                        te = p + 1;
                        act = 8;
                    }
                    8 => {
                        te = p + 1;
                        act = 9;
                    }
                    _ => {}
                }
                if m::TO_STATE_ACTIONS[cs] == 2 {
                    ts = None;
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

/// `setup_rphf_mask`.
fn setup_rphf_mask(plan: &UsePlan, buffer: &mut Buffer) {
    let mask = plan.rphf_mask;
    if mask == 0 {
        return;
    }
    let mut start = 0;
    while start < buffer.len() {
        let end = next_syllable(buffer, start);
        let limit = if buffer.info[start].shaper_cat == cat::R { 1 } else { 3.min(end - start) };
        for i in start..start + limit {
            buffer.info[i].mask |= mask;
        }
        start = end;
    }
}

/// `setup_topographical_masks`.
fn setup_topographical_masks(plan: &UsePlan, buffer: &mut Buffer) {
    if plan.arabic.is_some() {
        return;
    }
    let masks = plan.topographical;
    let all = masks.iter().fold(0, |a, m| a | m);
    if all == 0 {
        return;
    }
    let other = !all;
    const ISOL: usize = 0;
    const INIT: usize = 1;
    const MEDI: usize = 2;
    const FINA: usize = 3;
    let mut last_start = 0;
    let mut last_form: Option<usize> = None;
    let mut start = 0;
    while start < buffer.len() {
        let end = next_syllable(buffer, start);
        match buffer.info[start].syllable & 0x0F {
            syllable::HIEROGLYPH | syllable::NON => last_form = None,
            syllable::VIRAMA_TERMINATED
            | syllable::SAKOT_TERMINATED
            | syllable::STANDARD
            | syllable::NUMBER_JOINER_TERMINATED
            | syllable::NUMERAL
            | syllable::SYMBOL
            | syllable::BROKEN => {
                let join = last_form == Some(FINA) || last_form == Some(ISOL);
                if join {
                    let f = if last_form == Some(FINA) { MEDI } else { INIT };
                    for i in last_start..start {
                        buffer.info[i].mask = (buffer.info[i].mask & other) | masks[f];
                    }
                }
                let f = if join { FINA } else { ISOL };
                last_form = Some(f);
                for i in start..end {
                    buffer.info[i].mask = (buffer.info[i].mask & other) | masks[f];
                }
            }
            _ => {}
        }
        last_start = start;
        start = end;
    }
}

/// `setup_syllables_use`.
fn setup_syllables(plan: &UsePlan, buffer: &mut Buffer) {
    find_syllables(buffer);
    let mut start = 0;
    while start < buffer.len() {
        let end = next_syllable(buffer, start);
        buffer.unsafe_to_break(start, end);
        start = end;
    }
    setup_rphf_mask(plan, buffer);
    setup_topographical_masks(plan, buffer);
}

/// `record_rphf_use`: o repha substituído vira R.
fn record_rphf(plan: &UsePlan, buffer: &mut Buffer) {
    let mask = plan.rphf_mask;
    if mask == 0 {
        return;
    }
    let mut start = 0;
    while start < buffer.len() {
        let end = next_syllable(buffer, start);
        let mut i = start;
        while i < end && buffer.info[i].mask & mask != 0 {
            if buffer.info[i].substituted() {
                buffer.info[i].shaper_cat = cat::R;
                break;
            }
            i += 1;
        }
        start = end;
    }
}

/// `record_pref_use`: o pref substituído se comporta como VPre.
fn record_pref(buffer: &mut Buffer) {
    let mut start = 0;
    while start < buffer.len() {
        let end = next_syllable(buffer, start);
        for i in start..end {
            if buffer.info[i].substituted() {
                buffer.info[i].shaper_cat = cat::VPRE;
                break;
            }
        }
        start = end;
    }
}

fn is_halant(info: &GlyphInfo) -> bool {
    matches!(info.shaper_cat, cat::H | cat::HVM | cat::IS) && !info.ligated()
}

const fn flag64(c: u8) -> u64 {
    1u64 << c
}

const POST_BASE_FLAGS64: u64 = flag64(cat::FABV)
    | flag64(cat::FBLW)
    | flag64(cat::FPST)
    | flag64(cat::FMABV)
    | flag64(cat::FMBLW)
    | flag64(cat::FMPST)
    | flag64(cat::MABV)
    | flag64(cat::MBLW)
    | flag64(cat::MPST)
    | flag64(cat::MPRE)
    | flag64(cat::VABV)
    | flag64(cat::VBLW)
    | flag64(cat::VPST)
    | flag64(cat::VPRE)
    | flag64(cat::VMABV)
    | flag64(cat::VMBLW)
    | flag64(cat::VMPST)
    | flag64(cat::VMPRE);

/// `FLAG64_UNSAFE`.
fn flag64_unsafe(c: u8) -> u64 {
    if c < 64 { 1u64 << c } else { 0 }
}

/// `reorder_syllable_use`.
fn reorder_syllable(buffer: &mut Buffer, start: usize, end: usize) {
    use syllable::*;
    let kind = buffer.info[start].syllable & 0x0F;
    if !matches!(kind, VIRAMA_TERMINATED | SAKOT_TERMINATED | STANDARD | SYMBOL | BROKEN) {
        return;
    }
    // O repha anda para o fim, antes do primeiro glifo pós-base.
    if buffer.info[start].shaper_cat == cat::R && end - start > 1 {
        for mut i in start + 1..end {
            let post_base = flag64_unsafe(buffer.info[i].shaper_cat) & POST_BASE_FLAGS64 != 0 || is_halant(&buffer.info[i]);
            if post_base || i == end - 1 {
                if post_base {
                    i -= 1;
                }
                buffer.merge_clusters(start, i + 1);
                let t = buffer.info[start];
                buffer.info.copy_within(start + 1..i + 1, start);
                buffer.info[i] = t;
                break;
            }
        }
    }
    // As pré-base voltam para o começo ou para depois do último halant.
    let mut j = start;
    for i in start..end {
        let c = buffer.info[i].shaper_cat;
        if is_halant(&buffer.info[i]) {
            j = i + 1;
        } else if (c == cat::VPRE || c == cat::VMPRE) && buffer.info[i].lig_comp() == 0 && j < i {
            buffer.merge_clusters(j, i + 1);
            let t = buffer.info[i];
            buffer.info.copy_within(j..i, j + 1);
            buffer.info[j] = t;
        }
    }
}

/// `hb_syllabic_insert_dotted_circles`.
pub fn insert_dotted_circles(font: &Font, buffer: &mut Buffer, broken: u8, dotted_cat: u8, repha_cat: Option<u8>, position: Option<u8>) -> bool {
    if buffer.flags & FLAG_DO_NOT_INSERT_DOTTED_CIRCLE != 0 {
        return false;
    }
    if buffer.scratch_flags & scratch::HAS_BROKEN_SYLLABLE == 0 {
        return false;
    }
    let Some(glyph) = font.nominal_glyph(0x25CC) else { return false };
    let mut dotted = GlyphInfo { codepoint: glyph, shaper_cat: dotted_cat, ..GlyphInfo::default() };
    if let Some(pos) = position {
        dotted.shaper_aux = pos;
    }
    buffer.clear_output();
    buffer.idx = 0;
    let mut last_syllable = 0;
    while buffer.idx < buffer.len() {
        let s = buffer.cur(0).syllable;
        if last_syllable != s && s & 0x0F == broken {
            last_syllable = s;
            let mut g = dotted;
            g.cluster = buffer.cur(0).cluster;
            g.mask = buffer.cur(0).mask;
            g.syllable = s;
            if let Some(r) = repha_cat {
                while buffer.idx < buffer.len() && last_syllable == buffer.cur(0).syllable && buffer.cur(0).shaper_cat == r {
                    buffer.next_glyph();
                }
            }
            buffer.output_info(g);
        } else {
            buffer.next_glyph();
        }
    }
    buffer.sync();
    true
}

/// `reorder_use`.
fn reorder(font: &Font, buffer: &mut Buffer) {
    insert_dotted_circles(font, buffer, syllable::BROKEN, cat::B, Some(cat::R), None);
    let mut start = 0;
    while start < buffer.len() {
        let end = next_syllable(buffer, start);
        reorder_syllable(buffer, start, end);
        start = end;
    }
}

/// `_hb_preprocess_text_vowel_constraints` para os scripts do USE: um círculo pontilhado entre
/// sequências de vogais que imitam outra vogal.
pub fn preprocess_text(buffer: &mut Buffer) {
    if buffer.flags & FLAG_DO_NOT_INSERT_DOTTED_CIRCLE != 0 {
        return;
    }
    let script = buffer.props.script;
    let rules: &[(&[u32], &[u32])] = if script == tag(b"Brah") {
        &[(&[0x11005], &[0x11038]), (&[0x1100B], &[0x1103E]), (&[0x1100F], &[0x11042])]
    } else if script == tag(b"Khoj") {
        &[
            (&[0x11200], &[0x1122C, 0x11231, 0x11233]),
            (&[0x11206], &[0x1122C]),
            (&[0x1122C], &[0x11230, 0x11231]),
            (&[0x11240], &[0x1122E]),
        ]
    } else if script == tag(b"Sind") {
        &[(&[0x112B0], &[0x112E0, 0x112E5, 0x112E6, 0x112E7, 0x112E8])]
    } else if script == tag(b"Tirh") {
        &[(&[0x11481], &[0x114B0]), (&[0x1148B, 0x1148D], &[0x114BA]), (&[0x114AA], &[0x114B5, 0x114B6])]
    } else if script == tag(b"Modi") {
        &[(&[0x11600, 0x11601], &[0x11639, 0x1163A])]
    } else if script == tag(b"Takr") {
        &[(&[0x11680], &[0x116AD, 0x116B4, 0x116B5]), (&[0x11686], &[0x116B2])]
    } else {
        &[]
    };
    buffer.clear_output();
    if !rules.is_empty() {
        let count = buffer.len();
        buffer.idx = 0;
        while buffer.idx + 1 < count {
            let (a, b) = (buffer.cur(0).codepoint, buffer.cur(1).codepoint);
            let matched = rules.iter().any(|(first, second)| first.contains(&a) && second.contains(&b));
            buffer.next_glyph();
            if matched {
                buffer.output_glyph(0x25CC);
                buffer.prev_mut().reset_continuation();
                buffer.next_glyph();
            }
        }
    }
    buffer.sync();
}
