//! O shaper índico do HarfBuzz 10.2.0 (`hb-ot-shaper-indic.cc`): devanágari, bengali, gurmukhi,
//! guzerate, oriá, tâmil, télugo, canarês e malaiala com as tags antigas e as `*2` (as `*3` vão para
//! o USE). Sílabas pela máquina do Ragel, reordenação inicial antes das features básicas e final
//! antes das de apresentação.

use crate::buffer::{scratch, tag, Buffer, GlyphInfo, SegmentProperties};
use crate::font::Font;
use crate::indic_machine::{self as m, cat};
use crate::map::{Map, MapBuilder, Pause, F_GLOBAL, F_GLOBAL_MANUAL_JOINERS, F_MANUAL_JOINERS, F_PER_SYLLABLE};
use crate::props::next_syllable;
use crate::would::would_substitute;

/// `indic_position_t`.
#[allow(dead_code)]
pub mod pos {
    pub const START: u8 = 0;
    pub const RA_TO_BECOME_REPH: u8 = 1;
    pub const PRE_M: u8 = 2;
    pub const PRE_C: u8 = 3;
    pub const BASE_C: u8 = 4;
    pub const AFTER_MAIN: u8 = 5;
    pub const ABOVE_C: u8 = 6;
    pub const BEFORE_SUB: u8 = 7;
    pub const BELOW_C: u8 = 8;
    pub const AFTER_SUB: u8 = 9;
    pub const BEFORE_POST: u8 = 10;
    pub const POST_C: u8 = 11;
    pub const AFTER_POST: u8 = 12;
    pub const SMVD: u8 = 13;
    pub const END: u8 = 14;
}

/// `indic_syllable_type_t`.
mod syllable {
    pub const CONSONANT: u8 = 0;
    pub const VOWEL: u8 = 1;
    pub const STANDALONE: u8 = 2;
    pub const SYMBOL: u8 = 3;
    pub const BROKEN: u8 = 4;
    pub const NON_INDIC: u8 = 5;
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RephMode {
    Implicit,
    Explicit,
    LogRepha,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BlwfMode {
    PreAndPost,
    PostOnly,
}

/// `indic_config_t`.
#[derive(Clone, Copy)]
struct Config {
    has_old_spec: bool,
    virama: u32,
    reph_pos: u8,
    reph_mode: RephMode,
    blwf_mode: BlwfMode,
}

fn config(script: u32) -> Config {
    use BlwfMode::*;
    use RephMode::*;
    let c = |virama, reph_pos, reph_mode, blwf_mode| Config { has_old_spec: true, virama, reph_pos, reph_mode, blwf_mode };
    match &script.to_be_bytes() {
        b"Deva" => c(0x094D, pos::BEFORE_POST, Implicit, PreAndPost),
        b"Beng" => c(0x09CD, pos::AFTER_SUB, Implicit, PreAndPost),
        b"Guru" => c(0x0A4D, pos::BEFORE_SUB, Implicit, PreAndPost),
        b"Gujr" => c(0x0ACD, pos::BEFORE_POST, Implicit, PreAndPost),
        b"Orya" => c(0x0B4D, pos::AFTER_MAIN, Implicit, PreAndPost),
        b"Taml" => c(0x0BCD, pos::AFTER_POST, Implicit, PreAndPost),
        b"Telu" => c(0x0C4D, pos::AFTER_POST, Explicit, PostOnly),
        b"Knda" => c(0x0CCD, pos::AFTER_POST, Implicit, PostOnly),
        b"Mlym" => c(0x0D4D, pos::AFTER_MAIN, LogRepha, PreAndPost),
        _ => Config { has_old_spec: false, virama: 0, reph_pos: pos::BEFORE_POST, reph_mode: Implicit, blwf_mode: PreAndPost },
    }
}

/// `indic_features`, na ordem: as básicas até `init` (exclusive) e as de apresentação.
const FEATURES: [(&[u8; 4], u32); 17] = [
    (b"nukt", F_GLOBAL_MANUAL_JOINERS | F_PER_SYLLABLE),
    (b"akhn", F_GLOBAL_MANUAL_JOINERS | F_PER_SYLLABLE),
    (b"rphf", F_MANUAL_JOINERS | F_PER_SYLLABLE),
    (b"rkrf", F_GLOBAL_MANUAL_JOINERS | F_PER_SYLLABLE),
    (b"pref", F_MANUAL_JOINERS | F_PER_SYLLABLE),
    (b"blwf", F_MANUAL_JOINERS | F_PER_SYLLABLE),
    (b"abvf", F_MANUAL_JOINERS | F_PER_SYLLABLE),
    (b"half", F_MANUAL_JOINERS | F_PER_SYLLABLE),
    (b"pstf", F_MANUAL_JOINERS | F_PER_SYLLABLE),
    (b"vatu", F_GLOBAL_MANUAL_JOINERS | F_PER_SYLLABLE),
    (b"cjct", F_GLOBAL_MANUAL_JOINERS | F_PER_SYLLABLE),
    (b"init", F_MANUAL_JOINERS | F_PER_SYLLABLE),
    (b"pres", F_GLOBAL_MANUAL_JOINERS | F_PER_SYLLABLE),
    (b"abvs", F_GLOBAL_MANUAL_JOINERS | F_PER_SYLLABLE),
    (b"blws", F_GLOBAL_MANUAL_JOINERS | F_PER_SYLLABLE),
    (b"psts", F_GLOBAL_MANUAL_JOINERS | F_PER_SYLLABLE),
    (b"haln", F_GLOBAL_MANUAL_JOINERS | F_PER_SYLLABLE),
];
const RPHF: usize = 2;
const PREF: usize = 4;
const BLWF: usize = 5;
const ABVF: usize = 6;
const HALF: usize = 7;
const PSTF: usize = 8;
const INIT: usize = 11;
const BASIC_FEATURES: usize = INIT;

/// `collect_features_indic`.
pub fn collect_features(map: &mut MapBuilder) {
    map.add_gsub_pause(Some(Pause::IndicSetupSyllables));
    map.enable_feature(tag(b"locl"), F_PER_SYLLABLE, 1);
    map.enable_feature(tag(b"ccmp"), F_PER_SYLLABLE, 1);
    map.add_gsub_pause(Some(Pause::IndicInitialReordering));
    for (t, flags) in &FEATURES[..BASIC_FEATURES] {
        map.add_feature(tag(t), *flags, 1);
        map.add_gsub_pause(None);
    }
    map.add_gsub_pause(Some(Pause::IndicFinalReordering));
    for (t, flags) in &FEATURES[BASIC_FEATURES..] {
        map.add_feature(tag(t), *flags, 1);
    }
}

/// `override_features_indic`.
pub fn override_features(map: &mut MapBuilder) {
    map.disable_feature(tag(b"liga"));
    map.add_gsub_pause(Some(Pause::SyllabicClearVar));
}

/// `hb_indic_would_substitute_feature_t`: os lookups do estágio do GSUB de uma feature.
struct WouldSubstitute {
    lookups: Vec<u32>,
    zero_context: bool,
}

impl WouldSubstitute {
    fn new(map: &Map, t: &[u8; 4], zero_context: bool) -> WouldSubstitute {
        let stage = map.feature_stage(0, tag(t)) as usize;
        let lookups = map.stage_lookups(0, stage).iter().map(|l| u32::from(l.index)).collect();
        WouldSubstitute { lookups, zero_context }
    }

    fn would_substitute(&self, font: &Font, glyphs: &[u32]) -> bool {
        self.lookups.iter().any(|&l| would_substitute(&font.gsub, l, glyphs, self.zero_context))
    }
}

/// `indic_shape_plan_t`.
pub struct IndicPlan {
    config: Config,
    is_old_spec: bool,
    /// `load_virama_glyph`: zero quando a fonte não tem o virama.
    virama_glyph: u32,
    rphf: WouldSubstitute,
    pref: WouldSubstitute,
    blwf: WouldSubstitute,
    pstf: WouldSubstitute,
    vatu: WouldSubstitute,
    mask_array: [u32; 17],
}

impl IndicPlan {
    /// `data_create_indic`.
    pub fn new(map: &Map, props: &SegmentProperties, font: &Font) -> IndicPlan {
        let config = config(props.script);
        let is_old_spec = config.has_old_spec && map.chosen_script[0] & 0xFF != u32::from(b'2');
        let zero_context = !is_old_spec && props.script != tag(b"Mlym");
        let mut mask_array = [0; 17];
        for (i, (t, flags)) in FEATURES.iter().enumerate() {
            mask_array[i] = if flags & F_GLOBAL != 0 { 0 } else { map.one_mask(tag(t)) };
        }
        let virama_glyph = if config.virama == 0 { 0 } else { font.nominal_glyph(config.virama).unwrap_or(0) };
        IndicPlan {
            config,
            is_old_spec,
            virama_glyph,
            rphf: WouldSubstitute::new(map, b"rphf", zero_context),
            pref: WouldSubstitute::new(map, b"pref", zero_context),
            blwf: WouldSubstitute::new(map, b"blwf", zero_context),
            pstf: WouldSubstitute::new(map, b"pstf", zero_context),
            vatu: WouldSubstitute::new(map, b"vatu", zero_context),
            mask_array,
        }
    }
}

/// `setup_masks_indic`: categoria e posição de cada caractere.
pub fn setup_masks(buffer: &mut Buffer) {
    for info in &mut buffer.info {
        let t = categories(info.codepoint);
        info.shaper_cat = (t & 0xFF) as u8;
        info.shaper_aux = (t >> 8) as u8;
    }
}

/// `hb_indic_get_categories`.
fn categories(u: u32) -> u16 {
    use crate::indic_table::{DEFAULT, RANGES, SINGLES, TABLE};
    if let Some(&(_, v)) = SINGLES.iter().find(|&&(c, _)| c == u) {
        return v;
    }
    for &(a, b, off) in RANGES {
        if (a..=b).contains(&u) {
            return TABLE[(u - a) as usize + off];
        }
    }
    DEFAULT
}

/// `decompose_indic`: o Rra e o Rha do bengali, o Rra do devanágari e o Au do tâmil ficam inteiros.
pub fn decompose(ab: u32) -> Option<(u32, u32)> {
    match ab {
        0x0931 | 0x09DC | 0x09DD | 0x0B94 => None,
        _ => crate::unicode::decompose(ab),
    }
}

/// `compose_indic`.
pub fn compose(a: u32, b: u32) -> Option<u32> {
    if crate::unicode::gc::is_mark(crate::unicode::general_category(a)) {
        return None;
    }
    if a == 0x09AF && b == 0x09BC {
        return Some(0x09DF);
    }
    crate::normalize::compose_unicode(a, b)
}

/// As pausas do shaper índico.
pub fn pause(p: Pause, plan: &IndicPlan, font: &Font, buffer: &mut Buffer) {
    match p {
        Pause::IndicSetupSyllables => setup_syllables(buffer),
        Pause::IndicInitialReordering => initial_reordering(plan, font, buffer),
        Pause::IndicFinalReordering => final_reordering(plan, buffer),
        _ => {}
    }
}

/// `FLAG_UNSAFE`.
const fn flag(x: u8) -> u32 {
    if x < 32 { 1 << x } else { 0 }
}

const CONSONANT_FLAGS: u32 = flag(cat::C) | flag(cat::CS) | flag(cat::RA) | flag(cat::CM) | flag(cat::V) | flag(cat::PLACEHOLDER) | flag(cat::DOTTEDCIRCLE);
const JOINER_FLAGS: u32 = flag(cat::ZWJ) | flag(cat::ZWNJ);
const MATRA_FLAGS: u32 = flag(cat::M) | flag(cat::MPST);

fn is_one_of(info: &GlyphInfo, flags: u32) -> bool {
    !info.ligated() && flag(info.shaper_cat) & flags != 0
}

fn is_consonant(info: &GlyphInfo) -> bool {
    is_one_of(info, CONSONANT_FLAGS)
}

fn is_joiner(info: &GlyphInfo) -> bool {
    is_one_of(info, JOINER_FLAGS)
}

fn is_halant(info: &GlyphInfo) -> bool {
    is_one_of(info, flag(cat::H))
}

/// `find_syllables_indic`: a máquina do Ragel sobre todos os glifos.
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
                if m::FROM_STATE_ACTIONS[cs] == 10 {
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
                    11 => {
                        te = p + 1;
                        found(buffer, ts, te, NON_INDIC);
                    }
                    a @ (14 | 15 | 18 | 20 | 16 | 17) => {
                        te = p;
                        p -= 1;
                        let kind = match a {
                            14 => CONSONANT,
                            15 => VOWEL,
                            18 => STANDALONE,
                            20 => SYMBOL,
                            16 => BROKEN,
                            _ => NON_INDIC,
                        };
                        found(buffer, ts, te, kind);
                    }
                    a @ (1 | 3 | 7 | 8 | 4) => {
                        p = te - 1;
                        let kind = match a {
                            1 => CONSONANT,
                            3 => VOWEL,
                            7 => STANDALONE,
                            8 => SYMBOL,
                            _ => BROKEN,
                        };
                        found(buffer, ts, te, kind);
                    }
                    6 => {
                        let kind = match act {
                            1 => Some(CONSONANT),
                            5 | 7 => Some(NON_INDIC),
                            6 => Some(BROKEN),
                            _ => None,
                        };
                        if let Some(kind) = kind {
                            p = te - 1;
                            found(buffer, ts, te, kind);
                        }
                    }
                    a @ (19 | 13 | 5 | 12) => {
                        te = p + 1;
                        act = match a {
                            19 => 1,
                            13 => 5,
                            5 => 6,
                            _ => 7,
                        };
                    }
                    _ => {}
                }
                if m::TO_STATE_ACTIONS[cs] == 9 {
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

/// `setup_syllables_indic`.
fn setup_syllables(buffer: &mut Buffer) {
    find_syllables(buffer);
    let mut start = 0;
    while start < buffer.len() {
        let end = next_syllable(buffer, start);
        buffer.unsafe_to_break(start, end);
        start = end;
    }
}

/// `consonant_position_from_face`.
fn consonant_position_from_face(plan: &IndicPlan, consonant: u32, virama: u32, font: &Font) -> u8 {
    let glyphs = [virama, consonant, virama];
    let (a, b) = (&glyphs[..2], &glyphs[1..]);
    if plan.blwf.would_substitute(font, a)
        || plan.blwf.would_substitute(font, b)
        || plan.vatu.would_substitute(font, a)
        || plan.vatu.would_substitute(font, b)
    {
        return pos::BELOW_C;
    }
    if plan.pstf.would_substitute(font, a) || plan.pstf.would_substitute(font, b) {
        return pos::POST_C;
    }
    if plan.pref.would_substitute(font, a) || plan.pref.would_substitute(font, b) {
        return pos::POST_C;
    }
    pos::BASE_C
}

/// `update_consonant_positions_indic`.
fn update_consonant_positions(plan: &IndicPlan, font: &Font, buffer: &mut Buffer) {
    let virama = plan.virama_glyph;
    if virama == 0 {
        return;
    }
    for i in 0..buffer.len() {
        if buffer.info[i].shaper_aux == pos::BASE_C {
            let consonant = buffer.info[i].codepoint;
            buffer.info[i].shaper_aux = consonant_position_from_face(plan, consonant, virama, font);
        }
    }
}

/// `initial_reordering_indic`.
fn initial_reordering(plan: &IndicPlan, font: &Font, buffer: &mut Buffer) {
    update_consonant_positions(plan, font, buffer);
    crate::universal::insert_dotted_circles(font, buffer, syllable::BROKEN, cat::DOTTEDCIRCLE, Some(cat::REPHA), Some(pos::END));
    let mut start = 0;
    while start < buffer.len() {
        let end = next_syllable(buffer, start);
        match buffer.info[start].syllable & 0x0F {
            // As vogais se parecem com consoantes, e o círculo pontilhado já entrou nas partidas.
            syllable::VOWEL | syllable::CONSONANT | syllable::BROKEN | syllable::STANDALONE => {
                initial_reordering_consonant_syllable(plan, font, buffer, start, end);
            }
            _ => {}
        }
        start = end;
    }
}

/// `initial_reordering_consonant_syllable`.
fn initial_reordering_consonant_syllable(plan: &IndicPlan, font: &Font, buffer: &mut Buffer, start: usize, end: usize) {
    let script = buffer.props.script;
    let config = plan.config;

    // Ra, halant e ZWJ do canarês viram Ra, ZWJ e halant.
    if script == tag(b"Knda")
        && start + 3 <= end
        && is_one_of(&buffer.info[start], flag(cat::RA))
        && is_one_of(&buffer.info[start + 1], flag(cat::H))
        && is_one_of(&buffer.info[start + 2], flag(cat::ZWJ))
    {
        buffer.merge_clusters(start + 1, start + 3);
        buffer.info.swap(start + 1, start + 2);
    }

    // 1. A consoante de base.
    let mut base = end;
    let mut has_reph = false;
    {
        let info = &buffer.info;
        let mut limit = start;
        if plan.mask_array[RPHF] != 0
            && start + 3 <= end
            && ((config.reph_mode == RephMode::Implicit && !is_joiner(&info[start + 2]))
                || (config.reph_mode == RephMode::Explicit && info[start + 2].shaper_cat == cat::ZWJ))
        {
            let explicit = config.reph_mode == RephMode::Explicit;
            let glyphs = [info[start].codepoint, info[start + 1].codepoint, if explicit { info[start + 2].codepoint } else { 0 }];
            if plan.rphf.would_substitute(font, &glyphs[..2]) || (explicit && plan.rphf.would_substitute(font, &glyphs)) {
                limit += 2;
                while limit < end && is_joiner(&info[limit]) {
                    limit += 1;
                }
                base = start;
                has_reph = true;
            }
        } else if config.reph_mode == RephMode::LogRepha && info[start].shaper_cat == cat::REPHA {
            limit += 1;
            while limit < end && is_joiner(&info[limit]) {
                limit += 1;
            }
            base = start;
            has_reph = true;
        }

        let mut i = end;
        let mut seen_below = false;
        loop {
            i -= 1;
            if is_consonant(&info[i]) {
                let p = info[i].shaper_aux;
                if p != pos::BELOW_C && (p != pos::POST_C || seen_below) {
                    base = i;
                    break;
                }
                if p == pos::BELOW_C {
                    seen_below = true;
                }
                base = i;
            } else if start < i && info[i].shaper_cat == cat::ZWJ && info[i - 1].shaper_cat == cat::H {
                break;
            }
            if i <= limit {
                break;
            }
        }

        // Um Ra sozinho com halant não é reph.
        if has_reph && base == start && limit - base <= 2 {
            has_reph = false;
        }
    }

    // 2. Posições: antes da base vira pré-base, a base, e o reph.
    {
        let info = &mut buffer.info;
        for item in &mut info[start..base] {
            item.shaper_aux = item.shaper_aux.min(pos::PRE_C);
        }
        if base < end {
            info[base].shaper_aux = pos::BASE_C;
        }
        if has_reph {
            info[start].shaper_aux = pos::RA_TO_BECOME_REPH;
        }

        // Na especificação antiga, o halant depois da base vai para depois da última consoante.
        if plan.is_old_spec {
            let disallow_double_halants = script == tag(b"Knda");
            for i in base + 1..end {
                if info[i].shaper_cat == cat::H {
                    let mut j = end - 1;
                    while j > i {
                        if is_consonant(&info[j]) || (disallow_double_halants && info[j].shaper_cat == cat::H) {
                            break;
                        }
                        j -= 1;
                    }
                    if info[j].shaper_cat != cat::H && j > i {
                        info[i..=j].rotate_left(1);
                    }
                    break;
                }
            }
        }

        // As marcas herdam a posição do que vem antes.
        let mut last_pos = pos::START;
        for i in start..end {
            let c = info[i].shaper_cat;
            if flag(c) & (JOINER_FLAGS | flag(cat::N) | flag(cat::RS) | flag(cat::CM) | flag(cat::H)) != 0 {
                info[i].shaper_aux = last_pos;
                if c == cat::H && info[i].shaper_aux == pos::PRE_M {
                    for j in (start + 1..=i).rev() {
                        if info[j - 1].shaper_aux != pos::PRE_M {
                            info[i].shaper_aux = info[j - 1].shaper_aux;
                            break;
                        }
                    }
                }
            } else if info[i].shaper_aux != pos::SMVD {
                if c == cat::MPST && i > start && info[i - 1].shaper_cat == cat::SM {
                    info[i - 1].shaper_aux = info[i].shaper_aux;
                }
                last_pos = info[i].shaper_aux;
            }
        }

        // O que fica entre consoantes depois da base acompanha a consoante seguinte.
        let mut last = base;
        for i in base + 1..end {
            if is_consonant(&info[i]) {
                for j in last + 1..i {
                    if info[j].shaper_aux < pos::SMVD {
                        info[j].shaper_aux = info[i].shaper_aux;
                    }
                }
                last = i;
            } else if flag(info[i].shaper_cat) & MATRA_FLAGS != 0 {
                last = i;
            }
        }
    }

    // 3. Ordena pela posição e junta os clusters do que se moveu.
    {
        let syllable = buffer.info[start].syllable;
        for i in start..end {
            buffer.info[i].syllable = (i - start) as u8;
        }
        buffer.info[start..end].sort_by_key(|i| i.shaper_aux);

        let mut first_left_matra = end;
        let mut last_left_matra = end;
        base = end;
        for i in start..end {
            let p = buffer.info[i].shaper_aux;
            if p == pos::BASE_C {
                base = i;
                break;
            } else if p == pos::PRE_M {
                if first_left_matra == end {
                    first_left_matra = i;
                }
                last_left_matra = i;
            }
        }
        // Matras à esquerda com seus modificadores voltam à ordem lógica.
        if first_left_matra < last_left_matra {
            buffer.reverse_range(first_left_matra, last_left_matra + 1);
            let mut i = first_left_matra;
            for j in i..=last_left_matra {
                if flag(buffer.info[j].shaper_cat) & MATRA_FLAGS != 0 {
                    buffer.reverse_range(i, j + 1);
                    i = j + 1;
                }
            }
        }

        if plan.is_old_spec || end - start > 127 {
            buffer.merge_clusters(base, end);
        } else {
            for i in base..end {
                if buffer.info[i].syllable != 255 {
                    let mut min = i;
                    let mut max = i;
                    let mut j = start + usize::from(buffer.info[i].syllable);
                    while j != i {
                        min = min.min(j);
                        max = max.max(j);
                        let next = start + usize::from(buffer.info[j].syllable);
                        buffer.info[j].syllable = 255;
                        j = next;
                    }
                    buffer.merge_clusters(base.max(min), max + 1);
                }
            }
        }
        for i in start..end {
            buffer.info[i].syllable = syllable;
        }
    }

    // 4. As máscaras das features de cada lado da base.
    let info = &mut buffer.info;
    {
        let mut i = start;
        while i < end && info[i].shaper_aux == pos::RA_TO_BECOME_REPH {
            info[i].mask |= plan.mask_array[RPHF];
            i += 1;
        }
        let mut mask = plan.mask_array[HALF];
        if !plan.is_old_spec && config.blwf_mode == BlwfMode::PreAndPost {
            mask |= plan.mask_array[BLWF];
        }
        for item in &mut info[start..base] {
            item.mask |= mask;
        }
        let mask = plan.mask_array[BLWF] | plan.mask_array[ABVF] | plan.mask_array[PSTF];
        for item in info.iter_mut().take(end).skip(base + 1) {
            item.mask |= mask;
        }
    }

    // Ra e halant antes da base, na especificação antiga do devanágari, pegam o blwf.
    if plan.is_old_spec && script == tag(b"Deva") {
        let mut i = start;
        while i + 1 < base {
            if info[i].shaper_cat == cat::RA && info[i + 1].shaper_cat == cat::H && (i + 2 == base || info[i + 2].shaper_cat != cat::ZWJ) {
                info[i].mask |= plan.mask_array[BLWF];
                info[i + 1].mask |= plan.mask_array[BLWF];
            }
            i += 1;
        }
    }

    let pref_len = 2;
    if plan.mask_array[PREF] != 0 && base + pref_len < end {
        for i in base + 1..end + 1 - pref_len {
            let glyphs = [info[i].codepoint, info[i + 1].codepoint];
            if plan.pref.would_substitute(font, &glyphs) {
                info[i].mask |= plan.mask_array[PREF];
                info[i + 1].mask |= plan.mask_array[PREF];
                break;
            }
        }
    }

    // O ZWNJ tira o half da consoante antes dele.
    for i in start + 1..end {
        if is_joiner(&info[i]) {
            let non_joiner = info[i].shaper_cat == cat::ZWNJ;
            let mut j = i;
            loop {
                j -= 1;
                if non_joiner {
                    info[j].mask &= !plan.mask_array[HALF];
                }
                if !(j > start && !is_consonant(&info[j])) {
                    break;
                }
            }
        }
    }
}

/// `final_reordering_indic`.
fn final_reordering(plan: &IndicPlan, buffer: &mut Buffer) {
    let mut start = 0;
    while start < buffer.len() {
        let end = next_syllable(buffer, start);
        final_reordering_syllable(plan, buffer, start, end);
        start = end;
    }
}

/// `final_reordering_syllable_indic`.
fn final_reordering_syllable(plan: &IndicPlan, buffer: &mut Buffer, start: usize, end: usize) {
    let script = buffer.props.script;
    let mlym_or_taml = script == tag(b"Mlym") || script == tag(b"Taml");

    // O virama que uma ligadura multiplicou volta a ser halant.
    if plan.virama_glyph != 0 {
        for info in &mut buffer.info[start..end] {
            if info.codepoint == plan.virama_glyph && info.ligated() && info.multiplied() {
                info.shaper_cat = cat::H;
                info.clear_ligated_and_multiplied();
            }
        }
    }

    // 1. Acha a base de novo, já que as features podem ter mudado tudo.
    let mut try_pref = plan.mask_array[PREF] != 0;
    let mut base = start;
    {
        let info = &mut buffer.info;
        while base < end {
            if info[base].shaper_aux >= pos::BASE_C {
                if try_pref && base + 1 < end {
                    for i in base + 1..end {
                        if info[i].mask & plan.mask_array[PREF] != 0 {
                            if !(info[i].substituted() && info[i].ligated_and_didnt_multiply()) {
                                base = i;
                                while base < end && is_halant(&info[base]) {
                                    base += 1;
                                }
                                if base < end {
                                    info[base].shaper_aux = pos::BASE_C;
                                }
                                try_pref = false;
                            }
                            break;
                        }
                    }
                    if base == end {
                        break;
                    }
                }
                // No malaiala, o halant e a consoante abaixo depois da base deixam a base no fim.
                if script == tag(b"Mlym") {
                    let mut i = base + 1;
                    while i < end {
                        while i < end && is_joiner(&info[i]) {
                            i += 1;
                        }
                        if i == end || !is_halant(&info[i]) {
                            break;
                        }
                        i += 1;
                        while i < end && is_joiner(&info[i]) {
                            i += 1;
                        }
                        if i < end && is_consonant(&info[i]) && info[i].shaper_aux == pos::BELOW_C {
                            base = i;
                            info[base].shaper_aux = pos::BASE_C;
                        }
                        i += 1;
                    }
                }
                if start < base && info[base].shaper_aux > pos::BASE_C {
                    base -= 1;
                }
                break;
            }
            base += 1;
        }
        if base == end && start < base && is_one_of(&info[base - 1], flag(cat::ZWJ)) {
            base -= 1;
        }
        if base < end {
            while start < base && is_one_of(&info[base], flag(cat::N) | flag(cat::H)) {
                base -= 1;
            }
        }
    }

    // 2. A matra pré-base vai para antes da base, depois do último halant.
    if start + 1 < end && start < base {
        let mut new_pos = if base == end { base - 2 } else { base - 1 };
        if !mlym_or_taml {
            let info = &buffer.info;
            loop {
                while new_pos > start && !is_one_of(&info[new_pos], MATRA_FLAGS | flag(cat::H)) {
                    new_pos -= 1;
                }
                if is_halant(&info[new_pos]) && info[new_pos].shaper_aux != pos::PRE_M {
                    if new_pos + 1 < end && info[new_pos + 1].shaper_cat == cat::ZWJ && new_pos > start {
                        new_pos -= 1;
                        continue;
                    }
                } else {
                    new_pos = start;
                }
                break;
            }
        }
        if start < new_pos && buffer.info[new_pos].shaper_aux != pos::PRE_M {
            let mut i = new_pos;
            while i > start {
                if buffer.info[i - 1].shaper_aux == pos::PRE_M {
                    let old_pos = i - 1;
                    if old_pos < base && base <= new_pos {
                        base -= 1;
                    }
                    buffer.info[old_pos..=new_pos].rotate_left(1);
                    buffer.merge_clusters(new_pos, end.min(base + 1));
                    new_pos -= 1;
                }
                i -= 1;
            }
        } else {
            for i in start..base {
                if buffer.info[i].shaper_aux == pos::PRE_M {
                    buffer.merge_clusters(i, end.min(base + 1));
                    break;
                }
            }
        }
    }

    // 3. O reph vai para a posição do script.
    if start + 1 < end
        && buffer.info[start].shaper_aux == pos::RA_TO_BECOME_REPH
        && ((buffer.info[start].shaper_cat == cat::REPHA) ^ buffer.info[start].ligated_and_didnt_multiply())
    {
        let info = &buffer.info;
        let reph_pos = plan.config.reph_pos;
        let after_halant = || -> Option<usize> {
            let mut n = start + 1;
            while n < base && !is_halant(&info[n]) {
                n += 1;
            }
            if n < base && is_halant(&info[n]) {
                if n + 1 < base && is_joiner(&info[n + 1]) {
                    n += 1;
                }
                return Some(n);
            }
            None
        };
        let new_reph_pos = 'found: {
            if reph_pos != pos::AFTER_POST {
                if let Some(n) = after_halant() {
                    break 'found n;
                }
                if reph_pos == pos::AFTER_MAIN {
                    let mut n = base;
                    while n + 1 < end && info[n + 1].shaper_aux <= pos::AFTER_MAIN {
                        n += 1;
                    }
                    if n < end {
                        break 'found n;
                    }
                }
                if reph_pos == pos::AFTER_SUB {
                    let mut n = base;
                    let stop = flag(pos::POST_C) | flag(pos::AFTER_POST) | flag(pos::SMVD);
                    while n + 1 < end && flag(info[n + 1].shaper_aux) & stop == 0 {
                        n += 1;
                    }
                    if n < end {
                        break 'found n;
                    }
                }
            }
            if let Some(n) = after_halant() {
                break 'found n;
            }
            let mut n = end - 1;
            while n > start && info[n].shaper_aux == pos::SMVD {
                n -= 1;
            }
            if is_halant(&info[n]) {
                let mut i = base + 1;
                while i < n {
                    if flag(info[i].shaper_cat) & MATRA_FLAGS != 0 {
                        n -= 1;
                    }
                    i += 1;
                }
            }
            n
        };
        buffer.merge_clusters(start, new_reph_pos + 1);
        buffer.info[start..=new_reph_pos].rotate_left(1);
        if start < base && base <= new_reph_pos {
            base -= 1;
        }
    }

    // 4. O Ra pré-base reordenado vai para antes da base.
    if try_pref && base + 1 < end {
        for i in base + 1..end {
            if buffer.info[i].mask & plan.mask_array[PREF] != 0 {
                if buffer.info[i].ligated_and_didnt_multiply() {
                    let mut new_pos = base;
                    if !mlym_or_taml {
                        while new_pos > start && !is_one_of(&buffer.info[new_pos - 1], MATRA_FLAGS | flag(cat::H)) {
                            new_pos -= 1;
                        }
                    }
                    if new_pos > start && is_halant(&buffer.info[new_pos - 1]) && new_pos < end && is_joiner(&buffer.info[new_pos]) {
                        new_pos += 1;
                    }
                    let old_pos = i;
                    buffer.merge_clusters(new_pos, old_pos + 1);
                    buffer.info[new_pos..=old_pos].rotate_right(1);
                }
                break;
            }
        }
    }

    // 5. A matra pré-base no começo da palavra pega o init.
    if buffer.info[start].shaper_aux == pos::PRE_M {
        let format_to_mark = start > 0 && (1..=12).contains(&buffer.info[start - 1].general_category());
        if !format_to_mark {
            buffer.info[start].mask |= plan.mask_array[INIT];
        } else {
            buffer.unsafe_to_break(start - 1, start + 1);
        }
    }
}
