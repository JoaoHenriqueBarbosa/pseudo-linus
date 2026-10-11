//! O contexto de aplicação de lookups (`hb-ot-layout-gsubgpos.hh`): iterador que pula glifos,
//! casamento de entrada, contexto e encadeamento, recursão e o laço de aplicação de
//! `hb-ot-layout.cc`.

use crate::buffer::{Buffer, Direction, GlyphInfo};
use crate::font::Font;
use crate::ot::{self, GLYPH_PROPS_LIGATED, GLYPH_PROPS_MARK, GLYPH_PROPS_MULTIPLIED, GLYPH_PROPS_PRESERVE, GLYPH_PROPS_SUBSTITUTED, Gdef, GsubGpos, Lookup, NOT_COVERED, u16at};
use crate::props::allocate_lig_id;
use crate::unicode::gc;

pub const MAX_NESTING_LEVEL: u32 = 64;
pub const MAX_CONTEXT_LENGTH: usize = 64;

pub mod lookup_flag {
    pub const RIGHT_TO_LEFT: u32 = 0x0001;
    pub const IGNORE_BASE_GLYPHS: u32 = 0x0002;
    pub const IGNORE_LIGATURES: u32 = 0x0004;
    pub const IGNORE_MARKS: u32 = 0x0008;
    pub const IGNORE_FLAGS: u32 = 0x000E;
    pub const USE_MARK_FILTERING_SET: u32 = 0x0010;
    pub const MARK_ATTACHMENT_TYPE: u32 = 0xFF00;
}

/// A função de casamento de um iterador (`match_glyph`, `match_class`, `match_coverage`).
#[derive(Clone, Copy)]
pub enum MatchFn<'a> {
    Always,
    Glyph,
    Class(Option<&'a [u8]>),
    /// Os valores são deslocamentos de `Coverage` a partir desta base.
    Coverage(&'a [u8]),
}

impl MatchFn<'_> {
    pub fn matches(&self, info: &GlyphInfo, value: u32) -> bool {
        match *self {
            MatchFn::Always => true,
            MatchFn::Glyph => info.codepoint == value,
            MatchFn::Class(cd) => ot::class(cd, info.codepoint) == value,
            MatchFn::Coverage(base) => {
                let off = value as usize;
                let cov = if off == 0 { None } else { base.get(off..) };
                ot::coverage(cov, info.codepoint) != NOT_COVERED
            }
        }
    }
}

/// Uma lista de valores de 16 bits dentro de uma tabela.
#[derive(Clone, Copy)]
pub struct Values<'a> {
    pub d: &'a [u8],
    pub off: usize,
}

impl Values<'_> {
    pub fn get(&self, i: usize) -> u32 {
        u32::from(u16at(self.d, self.off + i * 2))
    }
}

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum MayMatch {
    No,
    Yes,
    Maybe,
}

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum MaySkip {
    No,
    Yes,
    Maybe,
}

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum IterMatch {
    Match,
    NotMatch,
    Skip,
}

/// `skipping_iterator_t` com o seu `matcher_t`.
pub struct Skippy<'a> {
    pub idx: usize,
    end: usize,
    lookup_props: u32,
    ignore_zwnj: bool,
    ignore_zwj: bool,
    ignore_hidden: bool,
    mask: u32,
    per_syllable: bool,
    syllable: u8,
    func: Option<MatchFn<'a>>,
    data: Option<Values<'a>>,
    k: usize,
}

impl<'a> Skippy<'a> {
    pub fn set_lookup_props(&mut self, p: u32) {
        self.lookup_props = p;
    }

    pub fn set_match(&mut self, func: Option<MatchFn<'a>>, data: Option<Values<'a>>) {
        self.func = func;
        self.data = data;
        self.k = 0;
    }

    pub fn reset(&mut self, c: &ApplyContext, start: usize) {
        self.idx = start;
        self.end = c.buffer.len();
        let s = if start == c.buffer.idx { c.buffer.cur(0).syllable } else { 0 };
        self.syllable = if self.per_syllable { s } else { 0 };
    }

    pub fn reset_fast(&mut self, start: usize) {
        self.idx = start;
    }

    fn glyph_data(&self) -> u32 {
        self.data.map_or(0, |v| v.get(self.k))
    }

    pub fn reject(&mut self) {
        if self.data.is_some() {
            self.k = self.k.wrapping_sub(1);
        }
    }

    fn may_match(&self, info: &GlyphInfo) -> MayMatch {
        if info.mask & self.mask == 0 || (self.syllable != 0 && self.syllable != info.syllable) {
            return MayMatch::No;
        }
        match self.func {
            Some(f) => {
                if f.matches(info, self.glyph_data()) {
                    MayMatch::Yes
                } else {
                    MayMatch::No
                }
            }
            None => MayMatch::Maybe,
        }
    }

    pub fn may_skip(&self, c: &ApplyContext, info: &GlyphInfo) -> MaySkip {
        if !c.check_glyph_property(info, self.lookup_props) {
            return MaySkip::Yes;
        }
        if info.is_default_ignorable()
            && (self.ignore_zwnj || !info.is_zwnj())
            && (self.ignore_zwj || !info.is_zwj())
            && (self.ignore_hidden || !info.is_hidden())
        {
            return MaySkip::Maybe;
        }
        MaySkip::No
    }

    pub fn match_info(&self, c: &ApplyContext, info: &GlyphInfo) -> IterMatch {
        let skip = self.may_skip(c, info);
        if skip == MaySkip::Yes {
            return IterMatch::Skip;
        }
        let m = self.may_match(info);
        if m == MayMatch::Yes || (m == MayMatch::Maybe && skip == MaySkip::No) {
            return IterMatch::Match;
        }
        if skip == MaySkip::No {
            return IterMatch::NotMatch;
        }
        IterMatch::Skip
    }

    pub fn next(&mut self, c: &ApplyContext, unsafe_to: Option<&mut usize>) -> bool {
        let stop = self.end as isize - 1;
        while (self.idx as isize) < stop {
            self.idx += 1;
            match self.match_info(c, &c.buffer.info[self.idx]) {
                IterMatch::Match => {
                    self.k += 1;
                    return true;
                }
                IterMatch::NotMatch => {
                    if let Some(u) = unsafe_to {
                        *u = self.idx + 1;
                    }
                    return false;
                }
                IterMatch::Skip => {}
            }
        }
        if let Some(u) = unsafe_to {
            *u = self.end;
        }
        false
    }

    pub fn prev(&mut self, c: &ApplyContext, unsafe_from: Option<&mut usize>) -> bool {
        while self.idx > 0 {
            self.idx -= 1;
            match self.match_info(c, c.out_at(self.idx)) {
                IterMatch::Match => {
                    self.k += 1;
                    return true;
                }
                IterMatch::NotMatch => {
                    if let Some(u) = unsafe_from {
                        *u = self.idx.max(1) - 1;
                    }
                    return false;
                }
                IterMatch::Skip => {}
            }
        }
        if let Some(u) = unsafe_from {
            *u = 0;
        }
        false
    }
}

/// `hb_ot_apply_context_t`.
pub struct ApplyContext<'a, 'b> {
    pub table_index: usize,
    pub table: GsubGpos<'a>,
    pub gdef: Gdef<'a>,
    pub font: &'b Font<'a>,
    pub buffer: &'b mut Buffer,
    pub direction: Direction,
    pub lookup_mask: u32,
    pub lookup_index: u32,
    pub lookup_props: u32,
    pub nesting_level_left: u32,
    pub has_glyph_classes: bool,
    pub auto_zwnj: bool,
    pub auto_zwj: bool,
    pub per_syllable: bool,
    pub random: bool,
    pub new_syllables: Option<u8>,
    pub last_base: i32,
    pub last_base_until: u32,
}

impl<'a, 'b> ApplyContext<'a, 'b> {
    pub fn new(table_index: usize, font: &'b Font<'a>, buffer: &'b mut Buffer) -> ApplyContext<'a, 'b> {
        let table = if table_index == 0 { font.gsub } else { font.gpos };
        let direction = buffer.props.direction;
        ApplyContext {
            table_index,
            table,
            gdef: font.gdef,
            font,
            buffer,
            direction,
            lookup_mask: 1,
            lookup_index: u32::MAX,
            lookup_props: 0,
            nesting_level_left: MAX_NESTING_LEVEL,
            has_glyph_classes: font.gdef.has_glyph_classes(),
            auto_zwnj: true,
            auto_zwj: true,
            per_syllable: false,
            random: false,
            new_syllables: None,
            last_base: -1,
            last_base_until: 0,
        }
    }

    /// `out_info[i]`; sem saída o C aponta `out_info` para o próprio `info`.
    pub fn out_at(&self, i: usize) -> &GlyphInfo {
        if self.buffer.have_output { &self.buffer.out_info[i] } else { &self.buffer.info[i] }
    }

    pub fn set_lookup_mask(&mut self, mask: u32) {
        self.lookup_mask = mask;
        self.last_base = -1;
        self.last_base_until = 0;
    }

    /// Um iterador recém-iniciado (`iter_input` com `false`, `iter_context` com `true`).
    pub fn iter(&self, context_match: bool) -> Skippy<'a> {
        Skippy {
            idx: 0,
            end: self.buffer.len(),
            lookup_props: self.lookup_props,
            ignore_zwnj: self.table_index == 1 || (context_match && self.auto_zwnj),
            ignore_zwj: context_match || self.auto_zwj,
            ignore_hidden: self.table_index == 1,
            mask: if context_match { u32::MAX } else { self.lookup_mask },
            per_syllable: self.table_index == 0 && self.per_syllable,
            syllable: 0,
            func: None,
            data: None,
            k: 0,
        }
    }

    pub fn random_number(&mut self) -> u32 {
        // Em `uint32_t` como no C: o produto estoura antes do módulo, então não é o minstd exato.
        self.buffer.random_state = self.buffer.random_state.wrapping_mul(48271) % 2147483647;
        self.buffer.random_state
    }

    fn match_properties_mark(&self, glyph: u32, glyph_props: u32, match_props: u32) -> bool {
        if match_props & lookup_flag::USE_MARK_FILTERING_SET != 0 {
            return self.gdef.mark_set_covers(match_props >> 16, glyph);
        }
        if match_props & lookup_flag::MARK_ATTACHMENT_TYPE != 0 {
            return (match_props & lookup_flag::MARK_ATTACHMENT_TYPE) == (glyph_props & lookup_flag::MARK_ATTACHMENT_TYPE);
        }
        true
    }

    /// `check_glyph_property`.
    pub fn check_glyph_property(&self, info: &GlyphInfo, match_props: u32) -> bool {
        let glyph_props = u32::from(info.glyph_props);
        if glyph_props & match_props & lookup_flag::IGNORE_FLAGS != 0 {
            return false;
        }
        if glyph_props & u32::from(GLYPH_PROPS_MARK) != 0 {
            return self.match_properties_mark(info.codepoint, glyph_props, match_props);
        }
        true
    }

    /// `_set_glyph_class`.
    fn set_glyph_class(&mut self, glyph: u32, class_guess: u16, ligature: bool, component: bool) {
        if let Some(s) = self.new_syllables {
            self.buffer.cur_mut(0).syllable = s;
        }
        let mut props = self.buffer.cur(0).glyph_props;
        props |= GLYPH_PROPS_SUBSTITUTED;
        if ligature {
            props |= GLYPH_PROPS_LIGATED;
            props &= !GLYPH_PROPS_MULTIPLIED;
        }
        if component {
            props |= GLYPH_PROPS_MULTIPLIED;
        }
        let v = if self.has_glyph_classes {
            (props & GLYPH_PROPS_PRESERVE) | self.gdef.glyph_props(glyph)
        } else if class_guess != 0 {
            (props & GLYPH_PROPS_PRESERVE) | class_guess
        } else {
            props
        };
        self.buffer.cur_mut(0).glyph_props = v;
    }

    pub fn replace_glyph(&mut self, g: u32) {
        self.set_glyph_class(g, 0, false, false);
        self.buffer.replace_glyph(g);
    }

    pub fn replace_glyph_inplace(&mut self, g: u32) {
        self.set_glyph_class(g, 0, false, false);
        self.buffer.cur_mut(0).codepoint = g;
    }

    pub fn replace_glyph_with_ligature(&mut self, g: u32, class_guess: u16) {
        self.set_glyph_class(g, class_guess, true, false);
        self.buffer.replace_glyph(g);
    }

    pub fn output_glyph_for_component(&mut self, g: u32, class_guess: u16) {
        self.set_glyph_class(g, class_guess, false, true);
        self.buffer.output_glyph(g);
    }

    /// `hb_ot_apply_context_t::recurse` com o `dispatch_recurse_func` da tabela.
    pub fn recurse(&mut self, lookup_index: u32) -> bool {
        self.buffer.max_ops -= 1;
        if self.nesting_level_left == 0 || self.buffer.max_ops < 0 {
            self.buffer.shaping_failed = true;
            return false;
        }
        self.nesting_level_left -= 1;
        let Some(l) = self.table.lookup(lookup_index as usize) else {
            self.nesting_level_left += 1;
            return false;
        };
        let (saved_props, saved_index) = (self.lookup_props, self.lookup_index);
        self.lookup_index = lookup_index;
        self.lookup_props = l.props();
        let ret = apply_lookup_subtables(self, l);
        self.lookup_index = saved_index;
        self.lookup_props = saved_props;
        self.nesting_level_left += 1;
        ret
    }
}

/// O subtipo efetivo de uma subtabela, atravessando `Extension` (GSUB 7, GPOS 9).
pub fn resolve_subtable<'a>(table_index: usize, ltype: u16, d: &'a [u8]) -> (u16, &'a [u8]) {
    let ext = if table_index == 0 { 7 } else { 9 };
    if ltype == ext {
        if u16at(d, 0) != 1 {
            return (0, &[]);
        }
        let t = u16at(d, 2);
        if t == ext {
            return (0, &[]);
        }
        return match ot::sub32(d, 0, 4) {
            Some(s) => (t, s),
            None => (0, &[]),
        };
    }
    (ltype, d)
}

/// `hb_ot_layout_lookup_accelerator_t::apply`: a primeira subtabela que aplica vence.
pub fn apply_lookup_subtables<'a>(c: &mut ApplyContext<'a, '_>, l: Lookup<'a>) -> bool {
    for i in 0..l.subtable_count() {
        let Some(d) = l.subtable(i) else { continue };
        let (t, d) = resolve_subtable(c.table_index, l.lookup_type(), d);
        let applied = if c.table_index == 0 { crate::gsub::apply_subtable(c, t, d) } else { crate::gpos::apply_subtable(c, t, d) };
        if applied {
            return true;
        }
    }
    false
}

pub fn lookup_is_reverse(l: &Lookup) -> bool {
    let t = l.lookup_type();
    if t == 7 {
        return l.subtable(0).is_some_and(|d| resolve_subtable(0, 7, d).0 == 8);
    }
    t == 8
}

/// `apply_string` com `apply_forward` e `apply_backward`.
pub fn apply_string<'a>(c: &mut ApplyContext<'a, '_>, l: Lookup<'a>) -> bool {
    if c.buffer.is_empty() || c.lookup_mask == 0 {
        return false;
    }
    c.lookup_props = l.props();
    let mut ret = false;
    let inplace = c.table_index == 1;
    if c.table_index == 1 || !lookup_is_reverse(&l) {
        if !inplace {
            c.buffer.clear_output();
        }
        c.buffer.idx = 0;
        while c.buffer.idx < c.buffer.len() && c.buffer.successful {
            let cur = *c.buffer.cur(0);
            let mut applied = false;
            if cur.mask & c.lookup_mask != 0 && c.check_glyph_property(&cur, c.lookup_props) {
                applied = apply_lookup_subtables(c, l);
            }
            if applied {
                ret = true;
            } else {
                c.buffer.next_glyph();
            }
        }
        if !inplace {
            c.buffer.sync();
        }
    } else {
        let mut i = c.buffer.len() as isize - 1;
        c.buffer.idx = i as usize;
        loop {
            let cur = *c.buffer.cur(0);
            if cur.mask & c.lookup_mask != 0 && c.check_glyph_property(&cur, c.lookup_props) {
                ret |= apply_lookup_subtables(c, l);
            }
            i = c.buffer.idx as isize - 1;
            if i < 0 {
                break;
            }
            c.buffer.idx = i as usize;
        }
        c.buffer.idx = 0;
    }
    ret
}

/// `match_input`. `input` são os valores do segundo componente em diante.
pub fn match_input<'a>(
    c: &ApplyContext<'a, '_>,
    count: usize,
    input: Values<'a>,
    func: MatchFn<'a>,
    end_position: &mut usize,
    positions: &mut [usize],
    total_component_count: Option<&mut u32>,
) -> bool {
    if count > MAX_CONTEXT_LENGTH {
        return false;
    }
    let buffer = &*c.buffer;
    let mut it = c.iter(false);
    it.reset(c, buffer.idx);
    it.set_match(Some(func), Some(input));
    let mut total = 0u32;
    let first_lig_id = buffer.cur(0).lig_id();
    let first_lig_comp = buffer.cur(0).lig_comp();
    // 0: não verificado, 1: não pode pular, 2: pode pular.
    let mut ligbase = 0u8;
    for i in 1..count {
        let mut unsafe_to = 0;
        if !it.next(c, Some(&mut unsafe_to)) {
            *end_position = unsafe_to;
            return false;
        }
        positions[i] = it.idx;
        let this = &buffer.info[it.idx];
        let (this_lig_id, this_lig_comp) = (this.lig_id(), this.lig_comp());
        if first_lig_id != 0 && first_lig_comp != 0 {
            if first_lig_id != this_lig_id || first_lig_comp != this_lig_comp {
                if ligbase == 0 {
                    let mut found = false;
                    let mut j = buffer.out_len();
                    while j > 0 && c.out_at(j - 1).lig_id() == first_lig_id {
                        if c.out_at(j - 1).lig_comp() == 0 {
                            j -= 1;
                            found = true;
                            break;
                        }
                        j -= 1;
                    }
                    ligbase = if found && it.may_skip(c, c.out_at(j)) == MaySkip::Yes { 2 } else { 1 };
                }
                if ligbase == 1 {
                    return false;
                }
            }
        } else if this_lig_id != 0 && this_lig_comp != 0 && this_lig_id != first_lig_id {
            return false;
        }
        total += this.lig_num_comps();
    }
    *end_position = it.idx + 1;
    if let Some(t) = total_component_count {
        total += buffer.cur(0).lig_num_comps();
        *t = total;
    }
    positions[0] = buffer.idx;
    true
}

/// `ligate_input`.
pub fn ligate_input(c: &mut ApplyContext, count: usize, positions: &[usize], match_end: usize, lig_glyph: u32, total_component_count: u32) {
    let idx = c.buffer.idx;
    c.buffer.merge_clusters(idx, match_end);
    let mut is_base_ligature = c.buffer.info[positions[0]].is_base_glyph();
    let mut is_mark_ligature = c.buffer.info[positions[0]].is_mark();
    for &p in &positions[1..count] {
        if !c.buffer.info[p].is_mark() {
            is_base_ligature = false;
            is_mark_ligature = false;
            break;
        }
    }
    let is_ligature = !is_base_ligature && !is_mark_ligature;
    let klass = if is_ligature { ot::GLYPH_PROPS_LIGATURE } else { 0 };
    let lig_id = if is_ligature { allocate_lig_id(c.buffer) } else { 0 };
    let mut last_lig_id = c.buffer.cur(0).lig_id();
    let mut last_num_components = c.buffer.cur(0).lig_num_comps();
    let mut components_so_far = last_num_components;
    if is_ligature {
        let cur = c.buffer.cur_mut(0);
        cur.set_lig_props_for_ligature(lig_id, total_component_count);
        if cur.general_category() == gc::NON_SPACING_MARK {
            cur.set_general_category(gc::OTHER_LETTER);
        }
    }
    c.replace_glyph_with_ligature(lig_glyph, klass);
    for &p in &positions[1..count] {
        while c.buffer.idx < p && c.buffer.successful {
            if is_ligature {
                let mut this_comp = c.buffer.cur(0).lig_comp();
                if this_comp == 0 {
                    this_comp = last_num_components;
                }
                let new = components_so_far - last_num_components + this_comp.min(last_num_components);
                c.buffer.cur_mut(0).set_lig_props_for_mark(lig_id, new);
            }
            c.buffer.next_glyph();
        }
        last_lig_id = c.buffer.cur(0).lig_id();
        last_num_components = c.buffer.cur(0).lig_num_comps();
        components_so_far += last_num_components;
        c.buffer.idx += 1;
    }
    if !is_mark_ligature && last_lig_id != 0 {
        for i in c.buffer.idx..c.buffer.len() {
            if last_lig_id != c.buffer.info[i].lig_id() {
                break;
            }
            let this_comp = c.buffer.info[i].lig_comp();
            if this_comp == 0 {
                break;
            }
            let new = components_so_far - last_num_components + this_comp.min(last_num_components);
            c.buffer.info[i].set_lig_props_for_mark(lig_id, new);
        }
    }
}

/// `match_backtrack`.
pub fn match_backtrack<'a>(c: &ApplyContext<'a, '_>, count: usize, values: Values<'a>, func: MatchFn<'a>, match_start: &mut usize) -> bool {
    let mut it = c.iter(true);
    it.reset(c, c.buffer.backtrack_len());
    it.set_match(Some(func), Some(values));
    for _ in 0..count {
        let mut unsafe_from = 0;
        if !it.prev(c, Some(&mut unsafe_from)) {
            *match_start = unsafe_from;
            return false;
        }
    }
    *match_start = it.idx;
    true
}

/// `match_lookahead`.
pub fn match_lookahead<'a>(c: &ApplyContext<'a, '_>, count: usize, values: Values<'a>, func: MatchFn<'a>, start_index: usize, end_index: &mut usize) -> bool {
    let mut it = c.iter(true);
    it.reset(c, start_index - 1);
    it.set_match(Some(func), Some(values));
    for _ in 0..count {
        let mut unsafe_to = 0;
        if !it.next(c, Some(&mut unsafe_to)) {
            *end_index = unsafe_to;
            return false;
        }
    }
    *end_index = it.idx + 1;
    true
}

/// `apply_lookup`: aplica os `LookupRecord` sobre as posições casadas.
pub fn apply_lookup(c: &mut ApplyContext, count: usize, positions: &mut Vec<usize>, records: Values, record_count: usize, match_end: usize) {
    let mut count = count;
    positions.truncate(count);
    let mut end: isize;
    {
        let bl = c.buffer.backtrack_len() as isize;
        end = bl + match_end as isize - c.buffer.idx as isize;
        let delta = bl - c.buffer.idx as isize;
        for p in positions.iter_mut() {
            *p = (*p as isize + delta) as usize;
        }
    }
    for i in 0..record_count {
        if !c.buffer.successful {
            break;
        }
        let idx = records.get(i * 2) as usize;
        let lookup_index = records.get(i * 2 + 1);
        if idx >= count {
            continue;
        }
        let orig_len = c.buffer.backtrack_len() + c.buffer.lookahead_len();
        if positions[idx] >= orig_len {
            continue;
        }
        if !c.buffer.move_to(positions[idx]) {
            break;
        }
        if c.buffer.max_ops <= 0 {
            break;
        }
        if !c.recurse(lookup_index) {
            continue;
        }
        let new_len = c.buffer.backtrack_len() + c.buffer.lookahead_len();
        let mut delta = new_len as isize - orig_len as isize;
        if delta == 0 {
            continue;
        }
        end += delta;
        if end < positions[idx] as isize {
            delta += positions[idx] as isize - end;
            end = positions[idx] as isize;
        }
        let mut next = idx + 1;
        if delta > 0 {
            if delta as usize + count > MAX_CONTEXT_LENGTH {
                break;
            }
        } else {
            delta = delta.max(next as isize - count as isize);
            next = (next as isize - delta) as usize;
        }
        // memmove (positions + next + delta, positions + next, count - next)
        let tail: Vec<usize> = positions[next..count].to_vec();
        let new_count = (count as isize + delta) as usize;
        positions.resize(new_count.max(count), 0);
        let dst = (next as isize + delta) as usize;
        positions[dst..dst + tail.len()].copy_from_slice(&tail);
        positions.truncate(new_count);
        next = dst;
        count = new_count;
        for j in idx + 1..next {
            positions[j] = positions[j - 1] + 1;
        }
        while next < count {
            positions[next] = (positions[next] as isize + delta) as usize;
            next += 1;
        }
    }
    c.buffer.move_to(end.max(0) as usize);
}

/// `context_apply_lookup`.
fn context_apply_lookup<'a>(c: &mut ApplyContext<'a, '_>, input_count: usize, input: Values<'a>, func: MatchFn<'a>, records: Values<'a>, record_count: usize) -> bool {
    if input_count > MAX_CONTEXT_LENGTH {
        return false;
    }
    let mut positions = vec![0usize; input_count.max(1)];
    let mut match_end = 0;
    if match_input(c, input_count, input, func, &mut match_end, &mut positions, None) {
        let idx = c.buffer.idx;
        c.buffer.unsafe_to_break(idx, match_end);
        apply_lookup(c, input_count, &mut positions, records, record_count, match_end);
        true
    } else {
        let idx = c.buffer.idx;
        c.buffer.unsafe_to_concat(idx, match_end);
        false
    }
}

/// `chain_context_apply_lookup`.
#[allow(clippy::too_many_arguments)]
fn chain_context_apply_lookup<'a>(
    c: &mut ApplyContext<'a, '_>,
    backtrack_count: usize,
    backtrack: Values<'a>,
    input_count: usize,
    input: Values<'a>,
    lookahead_count: usize,
    lookahead: Values<'a>,
    records: Values<'a>,
    record_count: usize,
    funcs: [MatchFn<'a>; 3],
) -> bool {
    if input_count > MAX_CONTEXT_LENGTH {
        return false;
    }
    let mut positions = vec![0usize; input_count.max(1)];
    let mut start_index = c.buffer.out_len();
    let mut end_index = c.buffer.idx;
    let mut match_end = 0;
    let ok = match_input(c, input_count, input, funcs[1], &mut match_end, &mut positions, None) && {
        end_index = match_end;
        end_index != 0
    } && match_lookahead(c, lookahead_count, lookahead, funcs[2], match_end, &mut end_index);
    if !ok {
        let idx = c.buffer.idx;
        c.buffer.unsafe_to_concat(idx, end_index);
        return false;
    }
    if !match_backtrack(c, backtrack_count, backtrack, funcs[0], &mut start_index) {
        c.buffer.unsafe_to_concat_from_outbuffer(start_index, end_index);
        return false;
    }
    c.buffer.unsafe_to_break_from_outbuffer(start_index, end_index);
    apply_lookup(c, input_count, &mut positions, records, record_count, match_end);
    true
}

/// `Rule::apply` (contexto simples e por classes).
fn rule_apply<'a>(c: &mut ApplyContext<'a, '_>, r: &'a [u8], func: MatchFn<'a>) -> bool {
    let input_count = usize::from(u16at(r, 0));
    let lookup_count = usize::from(u16at(r, 2));
    let input = Values { d: r, off: 4 };
    let records = Values { d: r, off: 4 + input_count.saturating_sub(1) * 2 };
    if r.len() < 4 + input_count.saturating_sub(1) * 2 + lookup_count * 4 {
        return false;
    }
    context_apply_lookup(c, input_count, input, func, records, lookup_count)
}

fn rule_set_apply<'a>(c: &mut ApplyContext<'a, '_>, set: Option<&'a [u8]>, func: MatchFn<'a>) -> bool {
    let Some(set) = set else { return false };
    let n = usize::from(u16at(set, 0));
    for i in 0..n {
        let Some(r) = ot::sub(set, 0, 2 + i * 2) else { continue };
        if rule_apply(c, r, func) {
            return true;
        }
    }
    false
}

/// `Context::dispatch` (GSUB 5, GPOS 7).
pub fn context_apply<'a>(c: &mut ApplyContext<'a, '_>, d: &'a [u8]) -> bool {
    let g = c.buffer.cur(0).codepoint;
    match u16at(d, 0) {
        1 => {
            let index = ot::coverage(ot::sub(d, 0, 2), g);
            if index == NOT_COVERED || index as usize >= usize::from(u16at(d, 4)) {
                return false;
            }
            rule_set_apply(c, ot::sub(d, 0, 6 + index as usize * 2), MatchFn::Glyph)
        }
        2 => {
            if ot::coverage(ot::sub(d, 0, 2), g) == NOT_COVERED {
                return false;
            }
            let cd = ot::sub(d, 0, 4);
            let index = ot::class(cd, g) as usize;
            if index >= usize::from(u16at(d, 6)) {
                return false;
            }
            rule_set_apply(c, ot::sub(d, 0, 8 + index * 2), MatchFn::Class(cd))
        }
        3 => {
            let glyph_count = usize::from(u16at(d, 2));
            let lookup_count = usize::from(u16at(d, 4));
            if glyph_count == 0 || d.len() < 6 + glyph_count * 2 + lookup_count * 4 {
                return false;
            }
            if ot::coverage(ot::sub(d, 0, 6), g) == NOT_COVERED {
                return false;
            }
            let input = Values { d, off: 8 };
            let records = Values { d, off: 6 + glyph_count * 2 };
            context_apply_lookup(c, glyph_count, input, MatchFn::Coverage(d), records, lookup_count)
        }
        _ => false,
    }
}

/// `ChainRule::apply`.
fn chain_rule_apply<'a>(c: &mut ApplyContext<'a, '_>, r: &'a [u8], funcs: [MatchFn<'a>; 3]) -> bool {
    let bt = usize::from(u16at(r, 0));
    let mut o = 2 + bt * 2;
    let input_count = usize::from(u16at(r, o));
    let input_off = o + 2;
    o = input_off + input_count.saturating_sub(1) * 2;
    let la = usize::from(u16at(r, o));
    let la_off = o + 2;
    o = la_off + la * 2;
    let lookup_count = usize::from(u16at(r, o));
    let rec_off = o + 2;
    if r.len() < rec_off + lookup_count * 4 {
        return false;
    }
    chain_context_apply_lookup(
        c,
        bt,
        Values { d: r, off: 2 },
        input_count,
        Values { d: r, off: input_off },
        la,
        Values { d: r, off: la_off },
        Values { d: r, off: rec_off },
        lookup_count,
        funcs,
    )
}

fn chain_rule_set_apply<'a>(c: &mut ApplyContext<'a, '_>, set: Option<&'a [u8]>, funcs: [MatchFn<'a>; 3]) -> bool {
    let Some(set) = set else { return false };
    let n = usize::from(u16at(set, 0));
    for i in 0..n {
        let Some(r) = ot::sub(set, 0, 2 + i * 2) else { continue };
        if chain_rule_apply(c, r, funcs) {
            return true;
        }
    }
    false
}

/// `ChainContext::dispatch` (GSUB 6, GPOS 8).
pub fn chain_context_apply<'a>(c: &mut ApplyContext<'a, '_>, d: &'a [u8]) -> bool {
    let g = c.buffer.cur(0).codepoint;
    match u16at(d, 0) {
        1 => {
            let index = ot::coverage(ot::sub(d, 0, 2), g);
            if index == NOT_COVERED || index as usize >= usize::from(u16at(d, 4)) {
                return false;
            }
            chain_rule_set_apply(c, ot::sub(d, 0, 6 + index as usize * 2), [MatchFn::Glyph; 3])
        }
        2 => {
            if ot::coverage(ot::sub(d, 0, 2), g) == NOT_COVERED {
                return false;
            }
            let (bcd, icd, lcd) = (ot::sub(d, 0, 4), ot::sub(d, 0, 6), ot::sub(d, 0, 8));
            let index = ot::class(icd, g) as usize;
            if index >= usize::from(u16at(d, 10)) {
                return false;
            }
            chain_rule_set_apply(c, ot::sub(d, 0, 12 + index * 2), [MatchFn::Class(bcd), MatchFn::Class(icd), MatchFn::Class(lcd)])
        }
        3 => {
            let bt = usize::from(u16at(d, 2));
            let mut o = 4 + bt * 2;
            let input_count = usize::from(u16at(d, o));
            let input_off = o + 2;
            o = input_off + input_count * 2;
            let la = usize::from(u16at(d, o));
            let la_off = o + 2;
            o = la_off + la * 2;
            let lookup_count = usize::from(u16at(d, o));
            let rec_off = o + 2;
            if input_count == 0 || d.len() < rec_off + lookup_count * 4 {
                return false;
            }
            if ot::coverage(ot::sub(d, 0, input_off), g) == NOT_COVERED {
                return false;
            }
            chain_context_apply_lookup(
                c,
                bt,
                Values { d, off: 4 },
                input_count,
                Values { d, off: input_off + 2 },
                la,
                Values { d, off: la_off },
                Values { d, off: rec_off },
                lookup_count,
                [MatchFn::Coverage(d); 3],
            )
        }
        _ => false,
    }
}
