//! As estruturas comuns do OpenType Layout (`hb-ot-layout-common.hh` e `GDEF.hh`), lidas
//! direto dos bytes. Deslocamento fora da tabela vale como o objeto nulo do HarfBuzz: tudo zero.

pub const NOT_FOUND_INDEX: u32 = 0xFFFF;
pub const NOT_COVERED: u32 = u32::MAX;

pub fn u8at(d: &[u8], o: usize) -> u8 {
    d.get(o).copied().unwrap_or(0)
}

pub fn u16at(d: &[u8], o: usize) -> u16 {
    match d.get(o..o + 2) {
        Some(b) => u16::from_be_bytes([b[0], b[1]]),
        None => 0,
    }
}

pub fn i16at(d: &[u8], o: usize) -> i16 {
    u16at(d, o) as i16
}

pub fn u24at(d: &[u8], o: usize) -> u32 {
    match d.get(o..o + 3) {
        Some(b) => u32::from_be_bytes([0, b[0], b[1], b[2]]),
        None => 0,
    }
}

pub fn u32at(d: &[u8], o: usize) -> u32 {
    match d.get(o..o + 4) {
        Some(b) => u32::from_be_bytes([b[0], b[1], b[2], b[3]]),
        None => 0,
    }
}

/// A subtabela num deslocamento de 16 bits a partir de `base`; `None` é o nulo.
pub fn sub(d: &[u8], base: usize, at: usize) -> Option<&[u8]> {
    let off = usize::from(u16at(d, base + at));
    if off == 0 {
        return None;
    }
    d.get(base + off..)
}

pub fn sub32(d: &[u8], base: usize, at: usize) -> Option<&[u8]> {
    let off = u32at(d, base + at) as usize;
    if off == 0 {
        return None;
    }
    d.get(base.checked_add(off)?..)
}

/// Os `lookupIndex` de uma tabela `Feature`.
fn feature_table_lookups(f: &[u8]) -> Vec<u32> {
    let n = usize::from(u16at(f, 2));
    if f.len() < 4 + n * 2 {
        return Vec::new();
    }
    (0..n).map(|k| u32::from(u16at(f, 4 + k * 2))).collect()
}

/// `ConditionSet::evaluate`: todas as condições valem (conjunto vazio vale). Só o formato 1
/// (`ConditionFormat1`, faixa de um eixo) é avaliado; os demais valem falso, como no HarfBuzz.
fn condition_set_evaluate(set: &[u8], coords: &[i32]) -> bool {
    let n = usize::from(u16at(set, 0));
    (0..n).all(|k| {
        let Some(c) = sub32(set, 0, 2 + k * 4) else { return false };
        if u16at(c, 0) != 1 {
            return false;
        }
        let axis = usize::from(u16at(c, 2));
        let coord = coords.get(axis).copied().unwrap_or(0);
        i32::from(i16at(c, 4)) <= coord && coord <= i32::from(i16at(c, 6))
    })
}

/// `FeatureTableSubstitution::find_substitute`: busca binária pelo índice do feature.
fn find_substitute(subst: &[u8], feature_index: u32) -> Option<&[u8]> {
    if u16at(subst, 0) != 1 {
        return None;
    }
    let n = usize::from(u16at(subst, 4)).min(subst.len().saturating_sub(6) / 6);
    let (mut lo, mut hi) = (0usize, n);
    while lo < hi {
        let mid = (lo + hi) / 2;
        let idx = u32::from(u16at(subst, 6 + mid * 6));
        match idx.cmp(&feature_index) {
            std::cmp::Ordering::Less => lo = mid + 1,
            std::cmp::Ordering::Greater => hi = mid,
            std::cmp::Ordering::Equal => return sub32(subst, 0, 6 + mid * 6 + 2),
        }
    }
    None
}

/// `Coverage::get_coverage`.
pub fn coverage(d: Option<&[u8]>, g: u32) -> u32 {
    let Some(d) = d else { return NOT_COVERED };
    match u16at(d, 0) {
        1 => {
            let n = usize::from(u16at(d, 2));
            if d.len() < 4 + n * 2 {
                return NOT_COVERED;
            }
            let (mut lo, mut hi) = (0usize, n);
            while lo < hi {
                let mid = (lo + hi) / 2;
                let v = u32::from(u16at(d, 4 + mid * 2));
                if g < v {
                    hi = mid;
                } else if g > v {
                    lo = mid + 1;
                } else {
                    return mid as u32;
                }
            }
            NOT_COVERED
        }
        2 => {
            let n = usize::from(u16at(d, 2));
            if d.len() < 4 + n * 6 {
                return NOT_COVERED;
            }
            let (mut lo, mut hi) = (0usize, n);
            while lo < hi {
                let mid = (lo + hi) / 2;
                let r = 4 + mid * 6;
                let (s, e) = (u32::from(u16at(d, r)), u32::from(u16at(d, r + 2)));
                if g < s {
                    hi = mid;
                } else if g > e {
                    lo = mid + 1;
                } else {
                    return u32::from(u16at(d, r + 4)) + (g - s);
                }
            }
            NOT_COVERED
        }
        _ => NOT_COVERED,
    }
}

/// `ClassDef::get_class`.
pub fn class(d: Option<&[u8]>, g: u32) -> u32 {
    let Some(d) = d else { return 0 };
    match u16at(d, 0) {
        1 => {
            let start = u32::from(u16at(d, 2));
            let n = u32::from(u16at(d, 4));
            if g >= start && g - start < n && d.len() >= 6 + n as usize * 2 {
                u32::from(u16at(d, 6 + (g - start) as usize * 2))
            } else {
                0
            }
        }
        2 => {
            let n = usize::from(u16at(d, 2));
            if d.len() < 4 + n * 6 {
                return 0;
            }
            let (mut lo, mut hi) = (0usize, n);
            while lo < hi {
                let mid = (lo + hi) / 2;
                let r = 4 + mid * 6;
                let (s, e) = (u32::from(u16at(d, r)), u32::from(u16at(d, r + 2)));
                if g < s {
                    hi = mid;
                } else if g > e {
                    lo = mid + 1;
                } else {
                    return u32::from(u16at(d, r + 4));
                }
            }
            0
        }
        _ => 0,
    }
}

/// `RecordArrayOf::find_index`: busca binária pela tag, como o `bfind` do C.
fn find_record(d: &[u8], at: usize, tag: u32) -> Option<u32> {
    let n = usize::from(u16at(d, at));
    if d.len() < at + 2 + n * 6 {
        return None;
    }
    let (mut lo, mut hi) = (0usize, n);
    while lo < hi {
        let mid = (lo + hi) / 2;
        let t = u32at(d, at + 2 + mid * 6);
        if tag < t {
            hi = mid;
        } else if tag > t {
            lo = mid + 1;
        } else {
            return Some(mid as u32);
        }
    }
    None
}

/// `LangSys`.
#[derive(Clone, Copy, Default)]
pub struct LangSys<'a> {
    d: &'a [u8],
}

impl<'a> LangSys<'a> {
    pub fn required_feature_index(&self) -> u32 {
        if self.d.is_empty() { NOT_FOUND_INDEX } else { u32::from(u16at(self.d, 2)) }
    }
    pub fn has_required_feature(&self) -> bool {
        self.required_feature_index() != 0xFFFF
    }
    pub fn feature_count(&self) -> usize {
        let n = usize::from(u16at(self.d, 4));
        if self.d.len() < 6 + n * 2 { 0 } else { n }
    }
    pub fn feature_index(&self, i: usize) -> u32 {
        if i < self.feature_count() { u32::from(u16at(self.d, 6 + i * 2)) } else { NOT_FOUND_INDEX }
    }
}

/// `Script`.
#[derive(Clone, Copy, Default)]
pub struct Script<'a> {
    d: &'a [u8],
}

impl<'a> Script<'a> {
    pub fn find_lang_sys_index(&self, tag: u32) -> Option<u32> {
        if self.d.is_empty() {
            return None;
        }
        find_record(self.d, 2, tag)
    }
    pub fn lang_sys_count(&self) -> usize {
        usize::from(u16at(self.d, 2))
    }
    /// `get_lang_sys`: o índice nulo devolve o `defaultLangSys`.
    pub fn lang_sys(&self, index: u32) -> LangSys<'a> {
        if self.d.is_empty() {
            return LangSys::default();
        }
        if index == NOT_FOUND_INDEX {
            return LangSys { d: sub(self.d, 0, 0).unwrap_or(&[]) };
        }
        let i = index as usize;
        if i >= self.lang_sys_count() {
            return LangSys::default();
        }
        LangSys { d: sub(self.d, 0, 4 + i * 6 + 4).unwrap_or(&[]) }
    }
}

/// `Lookup`.
#[derive(Clone, Copy)]
pub struct Lookup<'a> {
    pub d: &'a [u8],
}

impl<'a> Lookup<'a> {
    pub fn lookup_type(&self) -> u16 {
        u16at(self.d, 0)
    }
    pub fn flag(&self) -> u16 {
        u16at(self.d, 2)
    }
    pub fn subtable_count(&self) -> usize {
        usize::from(u16at(self.d, 4))
    }
    pub fn subtable(&self, i: usize) -> Option<&'a [u8]> {
        sub(self.d, 0, 6 + i * 2)
    }
    /// `Lookup::get_props`: a flag com o `markFilteringSet` nos 16 bits altos.
    pub fn props(&self) -> u32 {
        let mut flag = u32::from(self.flag());
        if flag & 0x10 != 0 {
            let set = u16at(self.d, 6 + self.subtable_count() * 2);
            flag |= u32::from(set) << 16;
        }
        flag
    }
}

/// `GSUBGPOS`: as listas comuns de GSUB e GPOS.
#[derive(Clone, Copy, Default)]
pub struct GsubGpos<'a> {
    pub d: &'a [u8],
}

impl<'a> GsubGpos<'a> {
    pub fn new(d: Option<&'a [u8]>) -> GsubGpos<'a> {
        match d {
            Some(d) if u16at(d, 0) == 1 && d.len() >= 10 => GsubGpos { d },
            _ => GsubGpos::default(),
        }
    }
    pub fn has_data(&self) -> bool {
        !self.d.is_empty()
    }
    fn script_list(&self) -> &'a [u8] {
        sub(self.d, 0, 4).unwrap_or(&[])
    }
    fn feature_list(&self) -> &'a [u8] {
        sub(self.d, 0, 6).unwrap_or(&[])
    }
    fn lookup_list(&self) -> &'a [u8] {
        sub(self.d, 0, 8).unwrap_or(&[])
    }
    pub fn find_script_index(&self, tag: u32) -> Option<u32> {
        let l = self.script_list();
        if l.is_empty() { None } else { find_record(l, 0, tag) }
    }
    pub fn script(&self, index: u32) -> Script<'a> {
        let l = self.script_list();
        let i = index as usize;
        if index == NOT_FOUND_INDEX || i >= usize::from(u16at(l, 0)) {
            return Script::default();
        }
        Script { d: sub(l, 0, 2 + i * 6 + 4).unwrap_or(&[]) }
    }
    pub fn feature_count(&self) -> usize {
        usize::from(u16at(self.feature_list(), 0))
    }
    pub fn feature_tag(&self, i: u32) -> u32 {
        if i == NOT_FOUND_INDEX || i as usize >= self.feature_count() {
            return 0;
        }
        u32at(self.feature_list(), 2 + i as usize * 6)
    }
    /// Os índices de lookup de uma feature (`Feature::lookupIndex`).
    pub fn feature_lookups(&self, i: u32) -> Vec<u32> {
        if i == NOT_FOUND_INDEX || i as usize >= self.feature_count() {
            return Vec::new();
        }
        let l = self.feature_list();
        let Some(f) = sub(l, 0, 2 + i as usize * 6 + 4) else { return Vec::new() };
        feature_table_lookups(f)
    }
    /// `FeatureVariations` da tabela 1.1; vazio nas demais.
    fn feature_variations(&self) -> &'a [u8] {
        if u16at(self.d, 2) < 1 {
            return &[];
        }
        match sub32(self.d, 0, 10) {
            Some(fv) if u16at(fv, 0) == 1 => fv,
            _ => &[],
        }
    }
    /// `FeatureVariations::find_index`: o primeiro registro cujo `ConditionSet` vale nas
    /// coordenadas normalizadas (F2DOT14); eixo sem coordenada vale 0, como no `hb_font_t` sem
    /// variações. `NOT_FOUND_INDEX` se nenhum vale.
    pub fn find_variations_index(&self, coords: &[i32]) -> u32 {
        let fv = self.feature_variations();
        let count = u32at(fv, 4) as usize;
        for i in 0..count.min(fv.len().saturating_sub(8) / 8) {
            let set = sub32(fv, 0, 8 + i * 8).unwrap_or(&[]);
            if condition_set_evaluate(set, coords) {
                return i as u32;
            }
        }
        NOT_FOUND_INDEX
    }
    /// `hb_ot_layout_feature_with_variations_get_lookups`: a tabela do feature trocada pela do
    /// `FeatureTableSubstitution` do registro de variações, se ele substitui este feature.
    pub fn feature_lookups_with_variations(&self, i: u32, variations_index: u32) -> Vec<u32> {
        if variations_index != NOT_FOUND_INDEX && i != NOT_FOUND_INDEX {
            let fv = self.feature_variations();
            if (variations_index as usize) < u32at(fv, 4) as usize {
                let subst = sub32(fv, 0, 8 + variations_index as usize * 8 + 4).unwrap_or(&[]);
                if let Some(f) = find_substitute(subst, i) {
                    return feature_table_lookups(f);
                }
            }
        }
        self.feature_lookups(i)
    }
    pub fn lookup_count(&self) -> usize {
        usize::from(u16at(self.lookup_list(), 0))
    }
    pub fn lookup(&self, i: usize) -> Option<Lookup<'a>> {
        let l = self.lookup_list();
        if i >= self.lookup_count() {
            return None;
        }
        sub(l, 0, 2 + i * 2).map(|d| Lookup { d })
    }
}

/// `GDEF`.
#[derive(Clone, Copy, Default)]
pub struct Gdef<'a> {
    pub d: &'a [u8],
}

pub const CLASS_BASE: u32 = 1;
pub const CLASS_LIGATURE: u32 = 2;
pub const CLASS_MARK: u32 = 3;

pub const GLYPH_PROPS_BASE_GLYPH: u16 = 0x02;
pub const GLYPH_PROPS_LIGATURE: u16 = 0x04;
pub const GLYPH_PROPS_MARK: u16 = 0x08;
pub const GLYPH_PROPS_SUBSTITUTED: u16 = 0x10;
pub const GLYPH_PROPS_LIGATED: u16 = 0x20;
pub const GLYPH_PROPS_MULTIPLIED: u16 = 0x40;
pub const GLYPH_PROPS_PRESERVE: u16 = GLYPH_PROPS_SUBSTITUTED | GLYPH_PROPS_LIGATED | GLYPH_PROPS_MULTIPLIED;

impl<'a> Gdef<'a> {
    /// O acelerador: GDEF de versão desconhecida ou da lista de bloqueio vira nulo.
    pub fn new(d: Option<&'a [u8]>, gsub_len: usize, gpos_len: usize) -> Gdef<'a> {
        let Some(d) = d else { return Gdef::default() };
        if u16at(d, 0) != 1 || d.len() < 12 {
            return Gdef::default();
        }
        if is_blocklisted(d.len(), gsub_len, gpos_len) {
            return Gdef::default();
        }
        Gdef { d }
    }
    pub fn has_glyph_classes(&self) -> bool {
        !self.d.is_empty() && u16at(self.d, 4) != 0
    }
    pub fn glyph_class(&self, g: u32) -> u32 {
        if self.d.is_empty() {
            return 0;
        }
        class(sub(self.d, 0, 4), g)
    }
    pub fn mark_attachment_type(&self, g: u32) -> u32 {
        if self.d.is_empty() {
            return 0;
        }
        class(sub(self.d, 0, 10), g)
    }
    /// `GDEF::get_glyph_props`.
    pub fn glyph_props(&self, g: u32) -> u16 {
        match self.glyph_class(g) {
            CLASS_BASE => GLYPH_PROPS_BASE_GLYPH,
            CLASS_LIGATURE => GLYPH_PROPS_LIGATURE,
            CLASS_MARK => GLYPH_PROPS_MARK | ((self.mark_attachment_type(g) as u16) << 8),
            _ => 0,
        }
    }
    /// `MarkGlyphSets::covers`.
    pub fn mark_set_covers(&self, set: u32, g: u32) -> bool {
        if self.d.is_empty() || u16at(self.d, 2) < 2 {
            return false;
        }
        let Some(m) = sub(self.d, 0, 12) else { return false };
        if u16at(m, 0) != 1 || set >= u32::from(u16at(m, 2)) {
            return false;
        }
        let Some(c) = sub32(m, 0, 4 + set as usize * 4) else { return false };
        coverage(Some(c), g) != NOT_COVERED
    }
}

/// `GDEF::is_blocklisted`.
fn is_blocklisted(gdef: usize, gsub: usize, gpos: usize) -> bool {
    const LIST: &[(usize, usize, usize)] = &[
        (442, 2874, 42038), (430, 2874, 40662), (442, 2874, 39116), (430, 2874, 39374),
        (490, 3046, 41638), (478, 3046, 41902), (898, 12554, 46470), (910, 12566, 47732),
        (928, 23298, 59332), (940, 23310, 60732), (964, 23836, 60072), (976, 23832, 61456),
        (994, 24474, 60336), (1006, 24470, 61740), (1006, 24576, 61346), (1018, 24572, 62828),
        (1006, 24576, 61352), (1018, 24572, 62834), (832, 7324, 47162), (844, 7302, 45474),
        (180, 13054, 7254), (192, 12638, 7254), (192, 12690, 7254), (188, 248, 3852),
        (188, 264, 3426), (1058, 47032, 11818), (1046, 47030, 12600), (1058, 71796, 16770),
        (1046, 71790, 17862), (1046, 71788, 17112), (1058, 71794, 17514), (1330, 109904, 57938),
        (1330, 109904, 58972), (1004, 59092, 14836), (588, 5078, 14418), (588, 5078, 14238),
        (894, 17162, 33960), (894, 17154, 34472), (816, 7868, 17052), (816, 7868, 17138),
    ];
    LIST.contains(&(gdef, gsub, gpos))
}
