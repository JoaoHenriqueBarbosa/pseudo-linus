//! Mapa de features para lookups (`hb-ot-map.cc`), com a escolha de script e de langsys do
//! `hb-ot-layout.cc` e as tags de script do `hb-ot-tag.cc`.

use crate::buffer::{tag, SegmentProperties, GLYPH_FLAG_DEFINED, SCRIPT_INVALID};
use crate::font::Font;
use crate::ot::{GsubGpos, NOT_FOUND_INDEX};

pub const F_NONE: u32 = 0x0000;
pub const F_GLOBAL: u32 = 0x0001;
pub const F_HAS_FALLBACK: u32 = 0x0002;
pub const F_MANUAL_ZWNJ: u32 = 0x0004;
pub const F_MANUAL_ZWJ: u32 = 0x0008;
pub const F_MANUAL_JOINERS: u32 = F_MANUAL_ZWNJ | F_MANUAL_ZWJ;
pub const F_GLOBAL_MANUAL_JOINERS: u32 = F_GLOBAL | F_MANUAL_JOINERS;
pub const F_GLOBAL_HAS_FALLBACK: u32 = F_GLOBAL | F_HAS_FALLBACK;
pub const F_GLOBAL_SEARCH: u32 = 0x0010;
pub const F_RANDOM: u32 = 0x0020;
pub const F_PER_SYLLABLE: u32 = 0x0040;

/// `HB_OT_MAP_MAX_BITS`.
const MAX_BITS: u32 = 8;
/// `HB_OT_LAYOUT_NO_FEATURE_INDEX`.
pub const NO_FEATURE_INDEX: u32 = NOT_FOUND_INDEX;

const TAG_DEFAULT_SCRIPT: u32 = tag(b"DFLT");
const TAG_DEFAULT_LANGUAGE: u32 = tag(b"dflt");
const TAG_LATIN_SCRIPT: u32 = tag(b"latn");

/// As pausas entre estágios que os shapers complexos registram.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pause {
    /// `record_stch` do shaper árabe.
    ArabicRecordStch,
    /// `deallocate_buffer_var` do shaper árabe: só libera a variável no C.
    ArabicDeallocate,
    /// `arabic_fallback_shape`.
    ArabicFallback,
}

#[derive(Clone, Copy, Debug)]
pub struct FeatureMap {
    pub tag: u32,
    pub index: [u32; 2],
    pub stage: [u32; 2],
    pub shift: u32,
    pub mask: u32,
    pub one_mask: u32,
    pub needs_fallback: bool,
    pub auto_zwnj: bool,
    pub auto_zwj: bool,
    pub random: bool,
    pub per_syllable: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct LookupMap {
    pub index: u16,
    pub auto_zwnj: bool,
    pub auto_zwj: bool,
    pub random: bool,
    pub per_syllable: bool,
    pub mask: u32,
    pub feature_tag: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct StageMap {
    pub last_lookup: usize,
    pub pause: Option<Pause>,
}

/// `hb_ot_map_t`.
#[derive(Clone, Debug, Default)]
pub struct Map {
    pub chosen_script: [u32; 2],
    pub found_script: [bool; 2],
    pub global_mask: u32,
    /// Ordenado por tag.
    pub features: Vec<FeatureMap>,
    pub lookups: [Vec<LookupMap>; 2],
    pub stages: [Vec<StageMap>; 2],
}

impl Map {
    fn find(&self, t: u32) -> Option<&FeatureMap> {
        self.features.binary_search_by(|f| f.tag.cmp(&t)).ok().map(|i| &self.features[i])
    }

    pub fn mask(&self, t: u32) -> (u32, u32) {
        self.find(t).map_or((0, 0), |f| (f.mask, f.shift))
    }

    pub fn needs_fallback(&self, t: u32) -> bool {
        self.find(t).is_some_and(|f| f.needs_fallback)
    }

    pub fn one_mask(&self, t: u32) -> u32 {
        self.find(t).map_or(0, |f| f.one_mask)
    }

    pub fn feature_index(&self, table_index: usize, t: u32) -> u32 {
        self.find(t).map_or(NO_FEATURE_INDEX, |f| f.index[table_index])
    }

    pub fn feature_stage(&self, table_index: usize, t: u32) -> u32 {
        self.find(t).map_or(u32::MAX, |f| f.stage[table_index])
    }

    /// `get_stage_lookups`.
    pub fn stage_lookups(&self, table_index: usize, stage: usize) -> &[LookupMap] {
        let stages = &self.stages[table_index];
        if stage > stages.len() {
            return &[];
        }
        let start = if stage > 0 { stages[stage - 1].last_lookup } else { 0 };
        let end = if stage < stages.len() { stages[stage].last_lookup } else { self.lookups[table_index].len() };
        &self.lookups[table_index][start..end]
    }
}

/// `hb_ot_old_tag_from_script`.
fn old_tag_from_script(script: u32) -> u32 {
    match script {
        SCRIPT_INVALID => TAG_DEFAULT_SCRIPT,
        s if s == tag(b"Zmth") => tag(b"math"),
        s if s == tag(b"Hira") => tag(b"kana"),
        s if s == tag(b"Laoo") => tag(b"lao "),
        s if s == tag(b"Yiii") => tag(b"yi  "),
        s if s == tag(b"Nkoo") => tag(b"nko "),
        s if s == tag(b"Vaii") => tag(b"vai "),
        s => s | 0x2000_0000,
    }
}

/// `hb_ot_new_tag_from_script`.
fn new_tag_from_script(script: u32) -> u32 {
    let pairs: [(&[u8; 4], &[u8; 4]); 10] = [
        (b"Beng", b"bng2"),
        (b"Deva", b"dev2"),
        (b"Gujr", b"gjr2"),
        (b"Guru", b"gur2"),
        (b"Knda", b"knd2"),
        (b"Mlym", b"mlm2"),
        (b"Orya", b"ory2"),
        (b"Taml", b"tml2"),
        (b"Telu", b"tel2"),
        (b"Mymr", b"mym2"),
    ];
    pairs.iter().find(|(s, _)| tag(s) == script).map_or(TAG_DEFAULT_SCRIPT, |(_, t)| tag(t))
}

/// `hb_ot_all_tags_from_script`, com as três posições do `HB_OT_MAX_TAGS_PER_SCRIPT`.
pub fn tags_from_script(script: u32) -> Vec<u32> {
    let mut tags = Vec::new();
    let new_tag = new_tag_from_script(script);
    if new_tag != TAG_DEFAULT_SCRIPT {
        if new_tag != tag(b"mym2") {
            tags.push((new_tag & !0xFF) | u32::from(b'3'));
        }
        tags.push(new_tag);
    }
    let old_tag = old_tag_from_script(script);
    if old_tag != TAG_DEFAULT_SCRIPT {
        tags.push(old_tag);
    }
    tags
}

/// `hb_ot_layout_table_select_script`.
fn select_script(g: &GsubGpos, tags: &[u32]) -> (bool, u32, u32) {
    for &t in tags {
        if let Some(i) = g.find_script_index(t) {
            return (true, i, t);
        }
    }
    for t in [TAG_DEFAULT_SCRIPT, TAG_DEFAULT_LANGUAGE, TAG_LATIN_SCRIPT] {
        if let Some(i) = g.find_script_index(t) {
            return (false, i, t);
        }
    }
    (false, NOT_FOUND_INDEX, 0)
}

/// `hb_ot_layout_script_select_language2`.
fn select_language(g: &GsubGpos, script_index: u32, tags: &[u32]) -> u32 {
    let s = g.script(script_index);
    for &t in tags {
        if let Some(i) = s.find_lang_sys_index(t) {
            return i;
        }
    }
    s.find_lang_sys_index(TAG_DEFAULT_LANGUAGE).unwrap_or(NOT_FOUND_INDEX)
}

#[derive(Clone, Copy, Debug)]
struct FeatureInfo {
    tag: u32,
    seq: usize,
    max_value: u32,
    flags: u32,
    default_value: u32,
    stage: [u32; 2],
}

#[derive(Clone, Copy, Debug)]
struct StageInfo {
    index: u32,
    pause: Option<Pause>,
}

/// `hb_ot_map_builder_t`.
pub struct MapBuilder<'f, 'a> {
    font: &'f Font<'a>,
    pub props: SegmentProperties,
    chosen_script: [u32; 2],
    found_script: [bool; 2],
    script_index: [u32; 2],
    language_index: [u32; 2],
    feature_infos: Vec<FeatureInfo>,
    stages: [Vec<StageInfo>; 2],
    current_stage: [u32; 2],
    pub is_simple: bool,
}

impl<'f, 'a> MapBuilder<'f, 'a> {
    fn table(&self, table_index: usize) -> &GsubGpos<'a> {
        if table_index == 0 { &self.font.gsub } else { &self.font.gpos }
    }

    pub fn new(font: &'f Font<'a>, props: &SegmentProperties) -> MapBuilder<'f, 'a> {
        let script_tags = tags_from_script(props.script);
        // Os tags de idioma do `hb-ot-tag.cc` dependem de `props.language`; sem idioma (o
        // locale C do Pillow) a lista é vazia e vale o langsys padrão.
        let language_tags: Vec<u32> = Vec::new();
        let mut b = MapBuilder {
            font,
            props: props.clone(),
            chosen_script: [0; 2],
            found_script: [false; 2],
            script_index: [NOT_FOUND_INDEX; 2],
            language_index: [NOT_FOUND_INDEX; 2],
            feature_infos: Vec::new(),
            stages: [Vec::new(), Vec::new()],
            current_stage: [0; 2],
            is_simple: false,
        };
        for ti in 0..2 {
            let g = b.table(ti);
            let (found, si, chosen) = select_script(g, &script_tags);
            let li = select_language(g, si, &language_tags);
            b.found_script[ti] = found;
            b.script_index[ti] = si;
            b.chosen_script[ti] = chosen;
            b.language_index[ti] = li;
        }
        b
    }

    pub fn chosen_script(&self, table_index: usize) -> u32 {
        self.chosen_script[table_index]
    }

    pub fn add_feature(&mut self, t: u32, flags: u32, value: u32) {
        if t == 0 {
            return;
        }
        self.feature_infos.push(FeatureInfo {
            tag: t,
            seq: self.feature_infos.len() + 1,
            max_value: value,
            flags,
            default_value: if flags & F_GLOBAL != 0 { value } else { 0 },
            stage: self.current_stage,
        });
    }

    pub fn enable_feature(&mut self, t: u32, flags: u32, value: u32) {
        self.add_feature(t, F_GLOBAL | flags, value);
    }

    pub fn disable_feature(&mut self, t: u32) {
        self.add_feature(t, F_GLOBAL, 0);
    }

    /// `hb_ot_layout_language_find_feature` nas duas tabelas.
    pub fn has_feature(&self, t: u32) -> bool {
        (0..2).any(|ti| {
            let g = self.table(ti);
            let l = g.script(self.script_index[ti]).lang_sys(self.language_index[ti]);
            (0..l.feature_count()).any(|i| g.feature_tag(l.feature_index(i)) == t)
        })
    }

    fn add_pause(&mut self, ti: usize, pause: Option<Pause>) {
        self.stages[ti].push(StageInfo { index: self.current_stage[ti], pause });
        self.current_stage[ti] += 1;
    }

    pub fn add_gsub_pause(&mut self, pause: Option<Pause>) {
        self.add_pause(0, pause);
    }

    pub fn add_gpos_pause(&mut self, pause: Option<Pause>) {
        self.add_pause(1, pause);
    }

    #[allow(clippy::too_many_arguments)]
    fn add_lookups(
        &self,
        m: &mut Map,
        ti: usize,
        feature_index: u32,
        mask: u32,
        auto_zwnj: bool,
        auto_zwj: bool,
        random: bool,
        per_syllable: bool,
        feature_tag: u32,
    ) {
        let g = self.table(ti);
        let count = g.lookup_count() as u32;
        // `FeatureVariations` sem coordenadas: vale a lista do próprio feature.
        for li in g.feature_lookups(feature_index) {
            if li >= count {
                continue;
            }
            m.lookups[ti].push(LookupMap {
                index: li as u16,
                auto_zwnj,
                auto_zwj,
                random,
                per_syllable,
                mask,
                feature_tag,
            });
        }
    }

    /// `hb_ot_map_builder_t::compile`.
    pub fn compile(mut self) -> Map {
        let global_bit_shift = 31u32;
        let global_bit_mask = 1u32 << global_bit_shift;
        let mut m = Map { global_mask: global_bit_mask, ..Default::default() };
        let mut required_index = [NO_FEATURE_INDEX; 2];
        let mut required_tag = [0u32; 2];
        let mut required_stage = [0u32; 2];
        for ti in 0..2 {
            m.chosen_script[ti] = self.chosen_script[ti];
            m.found_script[ti] = self.found_script[ti];
            let g = self.table(ti);
            let l = g.script(self.script_index[ti]).lang_sys(self.language_index[ti]);
            required_index[ti] = l.required_feature_index();
            required_tag[ti] = if required_index[ti] == NO_FEATURE_INDEX { 0 } else { g.feature_tag(required_index[ti]) };
        }

        if !self.feature_infos.is_empty() {
            if !self.is_simple {
                self.feature_infos.sort_by(|a, b| a.tag.cmp(&b.tag).then(a.seq.cmp(&b.seq)));
            }
            let f = &mut self.feature_infos;
            let mut j = 0;
            for i in 1..f.len() {
                if f[i].tag != f[j].tag {
                    j += 1;
                    f[j] = f[i];
                } else {
                    let fi = f[i];
                    if fi.flags & F_GLOBAL != 0 {
                        f[j].flags |= F_GLOBAL;
                        f[j].max_value = fi.max_value;
                        f[j].default_value = fi.default_value;
                    } else {
                        if f[j].flags & F_GLOBAL != 0 {
                            f[j].flags ^= F_GLOBAL;
                        }
                        f[j].max_value = f[j].max_value.max(fi.max_value);
                    }
                    f[j].flags |= fi.flags & F_HAS_FALLBACK;
                    f[j].stage[0] = f[j].stage[0].min(fi.stage[0]);
                    f[j].stage[1] = f[j].stage[1].min(fi.stage[1]);
                }
            }
            f.truncate(j + 1);
        }

        // `hb_ot_layout_collect_features_map`: de trás para a frente, o primeiro índice vence.
        let mut feature_indices: [Vec<(u32, u32)>; 2] = [Vec::new(), Vec::new()];
        for ti in 0..2 {
            let g = self.table(ti);
            let l = g.script(self.script_index[ti]).lang_sys(self.language_index[ti]);
            for i in (0..l.feature_count()).rev() {
                let fi = l.feature_index(i);
                let t = g.feature_tag(fi);
                match feature_indices[ti].iter_mut().find(|(k, _)| *k == t) {
                    Some(e) => e.1 = fi,
                    None => feature_indices[ti].push((t, fi)),
                }
            }
        }

        let mut next_bit = GLYPH_FLAG_DEFINED.count_ones() + 1;
        for info in self.feature_infos.clone() {
            let bits_needed = if info.flags & F_GLOBAL != 0 && info.max_value == 1 {
                0
            } else {
                MAX_BITS.min(32 - info.max_value.leading_zeros())
            };
            if info.max_value == 0 || next_bit + bits_needed >= global_bit_shift {
                continue;
            }
            let mut found = false;
            let mut feature_index = [NO_FEATURE_INDEX; 2];
            for ti in 0..2 {
                if required_tag[ti] == info.tag {
                    required_stage[ti] = info.stage[ti];
                }
                if let Some(&(_, idx)) = feature_indices[ti].iter().find(|(k, _)| *k == info.tag) {
                    feature_index[ti] = idx;
                    found = true;
                }
            }
            if !found && info.flags & F_GLOBAL_SEARCH != 0 {
                for ti in 0..2 {
                    let g = self.table(ti);
                    match (0..g.feature_count() as u32).find(|&i| g.feature_tag(i) == info.tag) {
                        Some(i) => {
                            feature_index[ti] = i;
                            found = true;
                        }
                        None => feature_index[ti] = NO_FEATURE_INDEX,
                    }
                }
            }
            if !found && info.flags & F_HAS_FALLBACK == 0 {
                continue;
            }
            let (shift, mask) = if info.flags & F_GLOBAL != 0 && info.max_value == 1 {
                (global_bit_shift, global_bit_mask)
            } else {
                let shift = next_bit;
                let mask = (1u32 << (next_bit + bits_needed)) - (1u32 << next_bit);
                next_bit += bits_needed;
                m.global_mask |= (info.default_value << shift) & mask;
                (shift, mask)
            };
            m.features.push(FeatureMap {
                tag: info.tag,
                index: feature_index,
                stage: info.stage,
                shift,
                mask,
                one_mask: (1u32 << shift) & mask,
                needs_fallback: !found,
                auto_zwnj: info.flags & F_MANUAL_ZWNJ == 0,
                auto_zwj: info.flags & F_MANUAL_ZWJ == 0,
                random: info.flags & F_RANDOM != 0,
                per_syllable: info.flags & F_PER_SYLLABLE != 0,
            });
        }
        // A busca por tag pede a lista ordenada (o C só ordena no caminho simples porque no
        // outro ela já sai ordenada).
        m.features.sort_by(|a, b| a.tag.cmp(&b.tag));

        self.add_gsub_pause(None);
        self.add_gpos_pause(None);

        for ti in 0..2 {
            let mut stage_index = 0;
            let mut last_num_lookups = 0;
            for stage in 0..self.current_stage[ti] {
                if required_index[ti] != NO_FEATURE_INDEX && required_stage[ti] == stage {
                    self.add_lookups(&mut m, ti, required_index[ti], global_bit_mask, true, true, false, false, tag(b"    "));
                }
                for k in 0..m.features.len() {
                    let f = m.features[k];
                    if f.stage[ti] == stage {
                        self.add_lookups(&mut m, ti, f.index[ti], f.mask, f.auto_zwnj, f.auto_zwj, f.random, f.per_syllable, f.tag);
                    }
                }
                let lookups = &mut m.lookups[ti];
                if last_num_lookups + 1 < lookups.len() {
                    lookups[last_num_lookups..].sort_by_key(|l| l.index);
                    let mut j = last_num_lookups;
                    for i in j + 1..lookups.len() {
                        if lookups[i].index != lookups[j].index {
                            j += 1;
                            lookups[j] = lookups[i];
                        } else {
                            let li = lookups[i];
                            lookups[j].mask |= li.mask;
                            lookups[j].auto_zwnj &= li.auto_zwnj;
                            lookups[j].auto_zwj &= li.auto_zwj;
                        }
                    }
                    lookups.truncate(j + 1);
                }
                last_num_lookups = lookups.len();
                if stage_index < self.stages[ti].len() && self.stages[ti][stage_index].index == stage {
                    m.stages[ti].push(StageMap { last_lookup: last_num_lookups, pause: self.stages[ti][stage_index].pause });
                    stage_index += 1;
                }
            }
        }
        m
    }
}
