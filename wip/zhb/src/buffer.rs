//! `hb_buffer_t` do HarfBuzz 10.2.0 (`hb-buffer.hh` e `hb-buffer.cc`).
//!
//! O C alterna entre escrever no próprio `info` e num `out_info` emprestado do `pos`; aqui a saída
//! vive sempre num vetor separado, o que dá o mesmo resultado observável. As uniões `var1`/`var2`
//! viram campos explícitos.

use crate::unicode;

pub const GLYPH_FLAG_UNSAFE_TO_BREAK: u32 = 0x1;
pub const GLYPH_FLAG_UNSAFE_TO_CONCAT: u32 = 0x2;
pub const GLYPH_FLAG_SAFE_TO_INSERT_TATWEEL: u32 = 0x4;
pub const GLYPH_FLAG_DEFINED: u32 = 0x7;

pub const FLAG_BOT: u32 = 0x1;
pub const FLAG_EOT: u32 = 0x2;
pub const FLAG_PRESERVE_DEFAULT_IGNORABLES: u32 = 0x4;
pub const FLAG_REMOVE_DEFAULT_IGNORABLES: u32 = 0x8;
pub const FLAG_DO_NOT_INSERT_DOTTED_CIRCLE: u32 = 0x10;
pub const FLAG_VERIFY: u32 = 0x20;
pub const FLAG_PRODUCE_UNSAFE_TO_CONCAT: u32 = 0x40;
pub const FLAG_PRODUCE_SAFE_TO_INSERT_TATWEEL: u32 = 0x80;

pub mod scratch {
    pub const HAS_NON_ASCII: u32 = 0x1;
    pub const HAS_DEFAULT_IGNORABLES: u32 = 0x2;
    pub const HAS_SPACE_FALLBACK: u32 = 0x4;
    pub const HAS_GPOS_ATTACHMENT: u32 = 0x8;
    pub const HAS_CGJ: u32 = 0x10;
    pub const HAS_GLYPH_FLAGS: u32 = 0x20;
    pub const HAS_BROKEN_SYLLABLE: u32 = 0x40;
    pub const HAS_VARIATION_SELECTOR_FALLBACK: u32 = 0x80;
    pub const SHAPER0: u32 = 0x0100_0000;
    pub const SHAPER1: u32 = 0x0200_0000;
    pub const SHAPER2: u32 = 0x0400_0000;
    pub const SHAPER3: u32 = 0x0800_0000;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClusterLevel {
    MonotoneGraphemes,
    MonotoneCharacters,
    Characters,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentType {
    Invalid,
    Unicode,
    Glyphs,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Invalid,
    Ltr,
    Rtl,
    Ttb,
    Btt,
}

impl Direction {
    pub fn is_horizontal(self) -> bool {
        matches!(self, Direction::Ltr | Direction::Rtl)
    }
    pub fn is_vertical(self) -> bool {
        matches!(self, Direction::Ttb | Direction::Btt)
    }
    pub fn is_backward(self) -> bool {
        matches!(self, Direction::Rtl | Direction::Btt)
    }
    pub fn is_forward(self) -> bool {
        matches!(self, Direction::Ltr | Direction::Ttb)
    }
    pub fn reverse(self) -> Direction {
        match self {
            Direction::Ltr => Direction::Rtl,
            Direction::Rtl => Direction::Ltr,
            Direction::Ttb => Direction::Btt,
            Direction::Btt => Direction::Ttb,
            Direction::Invalid => Direction::Invalid,
        }
    }
}

pub const fn tag(s: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*s)
}

pub const SCRIPT_COMMON: u32 = tag(b"Zyyy");
pub const SCRIPT_INHERITED: u32 = tag(b"Zinh");
pub const SCRIPT_UNKNOWN: u32 = tag(b"Zzzz");
pub const SCRIPT_INVALID: u32 = 0;

/// `hb_segment_properties_t`; a língua é a tag BCP 47 já normalizada (minúsculas).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SegmentProperties {
    pub direction: Direction,
    pub script: u32,
    pub language: Option<String>,
}

impl Default for SegmentProperties {
    fn default() -> Self {
        SegmentProperties { direction: Direction::Invalid, script: SCRIPT_INVALID, language: None }
    }
}

/// `hb_glyph_info_t` com as variáveis de `var1`/`var2` como campos.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GlyphInfo {
    pub codepoint: u32,
    pub mask: u32,
    pub cluster: u32,
    /// `unicode_props` (var2.u16[0]).
    pub unicode_props: u16,
    /// `glyph_props` (var1.u16[0]).
    pub glyph_props: u16,
    /// `lig_props` (var1.u8[2]).
    pub lig_props: u8,
    /// `syllable` (var1.u8[3]).
    pub syllable: u8,
    /// `ot_shaper_var_u8_category` e `ot_shaper_var_u8_auxiliary` dos shapers complexos.
    pub shaper_cat: u8,
    pub shaper_aux: u8,
    /// `glyph_index` guardado pela normalização.
    pub glyph_index: u32,
}

/// `hb_glyph_position_t` com `var` (`attach_chain`, `attach_type`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GlyphPosition {
    pub x_advance: i32,
    pub y_advance: i32,
    pub x_offset: i32,
    pub y_offset: i32,
    pub attach_chain: i16,
    pub attach_type: u8,
}

const MAX_LEN_FACTOR: usize = 64;
const MAX_LEN_MIN: usize = 16384;
const MAX_LEN_DEFAULT: usize = 0x3FFF_FFFF;
const MAX_OPS_FACTOR: i64 = 1024;
const MAX_OPS_MIN: i64 = 16384;
const MAX_OPS_DEFAULT: i64 = 0x1FFF_FFFF;
const CONTEXT_LENGTH: usize = 5;

pub struct Buffer {
    pub flags: u32,
    pub cluster_level: ClusterLevel,
    pub replacement: u32,
    pub invisible: u32,
    pub not_found: u32,
    pub not_found_variation_selector: Option<u32>,
    pub content_type: ContentType,
    pub props: SegmentProperties,
    pub successful: bool,
    pub shaping_failed: bool,
    pub have_output: bool,
    pub have_positions: bool,
    pub idx: usize,
    pub info: Vec<GlyphInfo>,
    pub out_info: Vec<GlyphInfo>,
    pub pos: Vec<GlyphPosition>,
    pub context: [[u32; CONTEXT_LENGTH]; 2],
    pub context_len: [usize; 2],
    pub serial: u8,
    pub random_state: u32,
    pub scratch_flags: u32,
    pub max_len: usize,
    pub max_ops: i64,
}

impl Default for Buffer {
    fn default() -> Self {
        Buffer::new()
    }
}

impl Buffer {
    /// `hb_buffer_create`.
    pub fn new() -> Buffer {
        Buffer {
            flags: 0,
            cluster_level: ClusterLevel::MonotoneGraphemes,
            replacement: 0xFFFD,
            invisible: 0,
            not_found: 0,
            not_found_variation_selector: None,
            content_type: ContentType::Invalid,
            props: SegmentProperties::default(),
            successful: true,
            shaping_failed: false,
            have_output: false,
            have_positions: false,
            idx: 0,
            info: Vec::new(),
            out_info: Vec::new(),
            pos: Vec::new(),
            context: [[0; CONTEXT_LENGTH]; 2],
            context_len: [0; 2],
            serial: 0,
            random_state: 1,
            scratch_flags: 0,
            max_len: MAX_LEN_DEFAULT,
            max_ops: MAX_OPS_DEFAULT,
        }
    }

    pub fn len(&self) -> usize {
        self.info.len()
    }

    pub fn is_empty(&self) -> bool {
        self.info.is_empty()
    }

    pub fn out_len(&self) -> usize {
        self.out_info.len()
    }

    /// `hb_buffer_t::clear`.
    pub fn clear(&mut self) {
        self.content_type = ContentType::Invalid;
        self.props = SegmentProperties::default();
        self.successful = true;
        self.shaping_failed = false;
        self.have_output = false;
        self.have_positions = false;
        self.idx = 0;
        self.info.clear();
        self.out_info.clear();
        self.pos.clear();
        self.context = [[0; CONTEXT_LENGTH]; 2];
        self.context_len = [0; 2];
        self.serial = 0;
        self.random_state = 1;
        self.scratch_flags = 0;
    }

    /// `hb_buffer_t::enter`.
    pub fn enter(&mut self) {
        self.serial = 0;
        self.shaping_failed = false;
        self.scratch_flags = 0;
        let len = self.len();
        if let Some(m) = len.checked_mul(MAX_LEN_FACTOR) {
            self.max_len = m.max(MAX_LEN_MIN);
        }
        if let Some(m) = (len as i64).checked_mul(MAX_OPS_FACTOR) {
            self.max_ops = m.max(MAX_OPS_MIN);
        }
    }

    /// `hb_buffer_t::leave`.
    pub fn leave(&mut self) {
        self.max_len = MAX_LEN_DEFAULT;
        self.max_ops = MAX_OPS_DEFAULT;
        self.serial = 0;
    }

    pub fn next_serial(&mut self) -> u8 {
        self.serial = self.serial.wrapping_add(1);
        if self.serial == 0 {
            self.serial = 1;
        }
        self.serial
    }

    pub fn backtrack_len(&self) -> usize {
        if self.have_output {
            self.out_len()
        } else {
            self.idx
        }
    }

    pub fn lookahead_len(&self) -> usize {
        self.len() - self.idx
    }

    fn ensure(&mut self, size: usize) -> bool {
        if size >= self.max_len {
            self.successful = false;
        }
        self.successful
    }

    /// `hb_buffer_t::add`.
    pub fn add(&mut self, codepoint: u32, cluster: u32) {
        if !self.ensure(self.len() + 1) {
            return;
        }
        self.info.push(GlyphInfo { codepoint, cluster, ..Default::default() });
    }

    pub fn add_info(&mut self, g: GlyphInfo) {
        if !self.ensure(self.len() + 1) {
            return;
        }
        self.info.push(g);
    }

    /// `hb_buffer_add_utf` sobre UTF-32 já decodificado (o raqm entrega codepoints).
    pub fn add_codepoints(&mut self, text: &[u32], item_offset: usize, item_length: usize) {
        if !self.ensure_unicode() {
            return;
        }
        let replacement = self.replacement;
        let fix = |c: u32| if c > 0x10FFFF || (0xD800..=0xDFFF).contains(&c) { replacement } else { c };
        if self.is_empty() && item_offset > 0 {
            self.clear_context(0);
            let mut i = item_offset;
            while i > 0 && self.context_len[0] < CONTEXT_LENGTH {
                i -= 1;
                let n = self.context_len[0];
                self.context[0][n] = fix(text[i]);
                self.context_len[0] += 1;
            }
        }
        for (k, &c) in text[item_offset..item_offset + item_length].iter().enumerate() {
            self.add(fix(c), (item_offset + k) as u32);
        }
        self.clear_context(1);
        let mut i = item_offset + item_length;
        while i < text.len() && self.context_len[1] < CONTEXT_LENGTH {
            let n = self.context_len[1];
            self.context[1][n] = fix(text[i]);
            self.context_len[1] += 1;
            i += 1;
        }
    }

    pub fn clear_context(&mut self, side: usize) {
        self.context_len[side] = 0;
    }

    pub fn ensure_unicode(&mut self) -> bool {
        if self.content_type != ContentType::Unicode {
            if self.content_type != ContentType::Invalid {
                return false;
            }
            self.content_type = ContentType::Unicode;
        }
        true
    }

    pub fn ensure_glyphs(&mut self) -> bool {
        if self.content_type != ContentType::Glyphs {
            if self.content_type != ContentType::Invalid {
                return false;
            }
            self.content_type = ContentType::Glyphs;
        }
        true
    }

    /// `hb_buffer_t::clear_output`.
    pub fn clear_output(&mut self) {
        self.have_output = true;
        self.have_positions = false;
        self.idx = 0;
        self.out_info.clear();
    }

    /// `hb_buffer_t::clear_positions`.
    pub fn clear_positions(&mut self) {
        self.have_output = false;
        self.have_positions = true;
        self.out_info.clear();
        self.pos.clear();
        self.pos.resize(self.len(), GlyphPosition::default());
    }

    /// `hb_buffer_t::sync`.
    pub fn sync(&mut self) -> bool {
        debug_assert!(self.have_output && self.idx <= self.len());
        let ok = self.successful && self.next_glyphs(self.len() - self.idx);
        if ok {
            self.info = std::mem::take(&mut self.out_info);
        }
        self.have_output = false;
        self.out_info.clear();
        self.idx = 0;
        ok
    }

    /// `hb_buffer_t::sync_so_far`.
    pub fn sync_so_far(&mut self) -> isize {
        let had_output = self.have_output;
        let out_i = self.out_len();
        let i = self.idx;
        let old_idx = self.idx;
        self.idx = if self.sync() { out_i } else { i };
        if had_output {
            self.have_output = true;
            self.out_info = self.info[..self.idx].to_vec();
        }
        self.idx as isize - old_idx as isize
    }

    pub fn cur(&self, i: usize) -> &GlyphInfo {
        &self.info[self.idx + i]
    }

    pub fn cur_mut(&mut self, i: usize) -> &mut GlyphInfo {
        let j = self.idx + i;
        &mut self.info[j]
    }

    pub fn cur_pos_mut(&mut self, i: usize) -> &mut GlyphPosition {
        let j = self.idx + i;
        &mut self.pos[j]
    }

    pub fn prev(&self) -> &GlyphInfo {
        &self.out_info[self.out_len().saturating_sub(1)]
    }

    pub fn prev_mut(&mut self) -> &mut GlyphInfo {
        let j = self.out_len().saturating_sub(1);
        &mut self.out_info[j]
    }

    /// `hb_buffer_t::replace_glyphs`.
    pub fn replace_glyphs(&mut self, num_in: usize, glyphs: &[u32]) -> bool {
        if !self.ensure(self.out_len() + glyphs.len()) {
            return false;
        }
        self.merge_clusters(self.idx, self.idx + num_in);
        let orig = if self.idx < self.len() { *self.cur(0) } else { *self.prev() };
        for &g in glyphs {
            self.out_info.push(GlyphInfo { codepoint: g, ..orig });
        }
        self.idx += num_in;
        true
    }

    pub fn replace_glyph(&mut self, g: u32) -> bool {
        self.replace_glyphs(1, &[g])
    }

    pub fn output_glyph(&mut self, g: u32) -> bool {
        self.replace_glyphs(0, &[g])
    }

    pub fn output_info(&mut self, g: GlyphInfo) -> bool {
        if !self.ensure(self.out_len() + 1) {
            return false;
        }
        self.out_info.push(g);
        true
    }

    pub fn copy_glyph(&mut self) -> bool {
        let g = *self.cur(0);
        self.output_info(g)
    }

    /// `hb_buffer_t::next_glyph`.
    pub fn next_glyph(&mut self) -> bool {
        if self.have_output {
            if !self.ensure(self.out_len() + 1) {
                return false;
            }
            let g = self.info[self.idx];
            self.out_info.push(g);
        }
        self.idx += 1;
        true
    }

    /// `hb_buffer_t::next_glyphs`.
    pub fn next_glyphs(&mut self, n: usize) -> bool {
        if self.have_output {
            if !self.ensure(self.out_len() + n) {
                return false;
            }
            let (a, b) = (self.idx, self.idx + n);
            self.out_info.extend_from_slice(&self.info[a..b]);
        }
        self.idx += n;
        true
    }

    pub fn skip_glyph(&mut self) {
        self.idx += 1;
    }

    /// `hb_buffer_t::move_to`.
    pub fn move_to(&mut self, i: usize) -> bool {
        if !self.have_output {
            debug_assert!(i <= self.len());
            self.idx = i;
            return true;
        }
        if !self.successful {
            return false;
        }
        let out_len = self.out_len();
        if out_len < i {
            let count = i - out_len;
            if !self.ensure(out_len + count) {
                return false;
            }
            let (a, b) = (self.idx, self.idx + count);
            self.out_info.extend_from_slice(&self.info[a..b]);
            self.idx += count;
        } else if out_len > i {
            let count = out_len - i;
            // `shift_forward`: abre espaço antes de `idx` quando falta.
            if self.idx < count {
                let extra = count - self.idx;
                if !self.ensure(self.len() + extra) {
                    return false;
                }
                let at = self.idx;
                self.info.splice(at..at, std::iter::repeat_n(GlyphInfo::default(), extra));
                self.idx += extra;
            }
            self.idx -= count;
            let moved: Vec<GlyphInfo> = self.out_info.drain(i..).collect();
            let at = self.idx;
            self.info[at..at + count].copy_from_slice(&moved);
        }
        true
    }

    pub fn reset_masks(&mut self, mask: u32) {
        for g in &mut self.info {
            g.mask = mask;
        }
    }

    pub fn add_masks(&mut self, mask: u32) {
        for g in &mut self.info {
            g.mask |= mask;
        }
    }

    /// `hb_buffer_t::set_masks`.
    pub fn set_masks(&mut self, value: u32, mask: u32, cluster_start: u32, cluster_end: u32) {
        if mask == 0 {
            return;
        }
        let value = value & mask;
        for g in &mut self.info {
            if cluster_start <= g.cluster && g.cluster < cluster_end {
                g.mask = (g.mask & !mask) | value;
            }
        }
    }

    pub fn set_cluster(inf: &mut GlyphInfo, cluster: u32, mask: u32) {
        if inf.cluster != cluster {
            inf.mask = (inf.mask & !GLYPH_FLAG_DEFINED) | (mask & GLYPH_FLAG_DEFINED);
        }
        inf.cluster = cluster;
    }

    pub fn merge_clusters(&mut self, start: usize, end: usize) {
        if end.saturating_sub(start) < 2 {
            return;
        }
        self.merge_clusters_impl(start, end);
    }

    /// `hb_buffer_t::merge_clusters_impl`.
    fn merge_clusters_impl(&mut self, mut start: usize, mut end: usize) {
        if self.cluster_level == ClusterLevel::Characters {
            self.unsafe_to_break(start, end);
            return;
        }
        let len = self.len();
        let mut cluster = self.info[start].cluster;
        for i in start + 1..end {
            cluster = cluster.min(self.info[i].cluster);
        }
        if cluster != self.info[end - 1].cluster {
            while end < len && self.info[end - 1].cluster == self.info[end].cluster {
                end += 1;
            }
        }
        if cluster != self.info[start].cluster {
            while self.idx < start && self.info[start - 1].cluster == self.info[start].cluster {
                start -= 1;
            }
        }
        if self.idx == start && self.info[start].cluster != cluster {
            let c = self.info[start].cluster;
            let mut i = self.out_len();
            while i > 0 && self.out_info[i - 1].cluster == c {
                Self::set_cluster(&mut self.out_info[i - 1], cluster, 0);
                i -= 1;
            }
        }
        for g in &mut self.info[start..end] {
            Self::set_cluster(g, cluster, 0);
        }
    }

    /// `hb_buffer_t::merge_out_clusters`.
    pub fn merge_out_clusters(&mut self, mut start: usize, mut end: usize) {
        if self.cluster_level == ClusterLevel::Characters || end.saturating_sub(start) < 2 {
            return;
        }
        let out_len = self.out_len();
        let mut cluster = self.out_info[start].cluster;
        for i in start + 1..end {
            cluster = cluster.min(self.out_info[i].cluster);
        }
        while start > 0 && self.out_info[start - 1].cluster == self.out_info[start].cluster {
            start -= 1;
        }
        while end < out_len && self.out_info[end - 1].cluster == self.out_info[end].cluster {
            end += 1;
        }
        if end == out_len {
            let c = self.out_info[end - 1].cluster;
            let mut i = self.idx;
            while i < self.len() && self.info[i].cluster == c {
                Self::set_cluster(&mut self.info[i], cluster, 0);
                i += 1;
            }
        }
        for g in &mut self.out_info[start..end] {
            Self::set_cluster(g, cluster, 0);
        }
    }

    /// `hb_buffer_t::delete_glyph`.
    pub fn delete_glyph(&mut self) {
        let cluster = self.info[self.idx].cluster;
        let out_len = self.out_len();
        let same_next = self.idx + 1 < self.len() && cluster == self.info[self.idx + 1].cluster;
        let same_prev = out_len > 0 && cluster == self.out_info[out_len - 1].cluster;
        if !same_next && !same_prev {
            if out_len > 0 {
                if cluster < self.out_info[out_len - 1].cluster {
                    let mask = self.info[self.idx].mask;
                    let old = self.out_info[out_len - 1].cluster;
                    let mut i = out_len;
                    while i > 0 && self.out_info[i - 1].cluster == old {
                        Self::set_cluster(&mut self.out_info[i - 1], cluster, mask);
                        i -= 1;
                    }
                }
            } else if self.idx + 1 < self.len() {
                self.merge_clusters(self.idx, self.idx + 2);
            }
        }
        self.skip_glyph();
    }

    /// `hb_buffer_t::delete_glyphs_inplace`.
    pub fn delete_glyphs_inplace(&mut self, filter: impl Fn(&GlyphInfo) -> bool) {
        let mut j = 0;
        let count = self.len();
        let has_pos = self.pos.len() >= count;
        for i in 0..count {
            if filter(&self.info[i]) {
                let cluster = self.info[i].cluster;
                if i + 1 < count && cluster == self.info[i + 1].cluster {
                    continue;
                }
                if j > 0 {
                    if cluster < self.info[j - 1].cluster {
                        let mask = self.info[i].mask;
                        let old = self.info[j - 1].cluster;
                        let mut k = j;
                        while k > 0 && self.info[k - 1].cluster == old {
                            Self::set_cluster(&mut self.info[k - 1], cluster, mask);
                            k -= 1;
                        }
                    }
                    continue;
                }
                if i + 1 < count {
                    self.merge_clusters(i, i + 2);
                }
                continue;
            }
            if j != i {
                self.info[j] = self.info[i];
                if has_pos {
                    self.pos[j] = self.pos[i];
                }
            }
            j += 1;
        }
        self.info.truncate(j);
        if has_pos {
            self.pos.truncate(j);
        }
    }

    pub fn reverse_range(&mut self, start: usize, end: usize) {
        if end <= start {
            return;
        }
        self.info[start..end].reverse();
        if self.have_positions {
            self.pos[start..end].reverse();
        }
    }

    pub fn reverse(&mut self) {
        let n = self.len();
        self.reverse_range(0, n);
    }

    /// `hb_buffer_t::reverse_groups`.
    pub fn reverse_groups(&mut self, group: impl Fn(&GlyphInfo, &GlyphInfo) -> bool, merge: bool) {
        let len = self.len();
        if len == 0 {
            return;
        }
        let mut start = 0;
        let mut i = 1;
        while i < len {
            if !group(&self.info[i - 1], &self.info[i]) {
                if merge {
                    self.merge_clusters(start, i);
                }
                self.reverse_range(start, i);
                start = i;
            }
            i += 1;
        }
        if merge {
            self.merge_clusters(start, i);
        }
        self.reverse_range(start, i);
        self.reverse();
    }

    pub fn reverse_clusters(&mut self) {
        self.reverse_groups(|a, b| a.cluster == b.cluster, false);
    }

    pub fn group_end(&self, mut start: usize, group: impl Fn(&GlyphInfo, &GlyphInfo) -> bool) -> usize {
        start += 1;
        while start < self.len() && group(&self.info[start - 1], &self.info[start]) {
            start += 1;
        }
        start
    }

    pub fn cluster_end(&self, start: usize) -> usize {
        self.group_end(start, |a, b| a.cluster == b.cluster)
    }

    fn infos_find_min_cluster(&self, infos: &[GlyphInfo], start: usize, end: usize, cluster: u32) -> u32 {
        if start == end {
            return cluster;
        }
        if self.cluster_level == ClusterLevel::Characters {
            return infos[start..end].iter().fold(cluster, |c, g| c.min(g.cluster));
        }
        cluster.min(infos[start].cluster.min(infos[end - 1].cluster))
    }

    fn infos_set_glyph_flags(
        level: ClusterLevel,
        scratch_flags: &mut u32,
        infos: &mut [GlyphInfo],
        start: usize,
        end: usize,
        cluster: u32,
        mask: u32,
    ) {
        if start == end {
            return;
        }
        let first = infos[start].cluster;
        let last = infos[end - 1].cluster;
        if level == ClusterLevel::Characters || (cluster != first && cluster != last) {
            for g in &mut infos[start..end] {
                if cluster != g.cluster {
                    *scratch_flags |= scratch::HAS_GLYPH_FLAGS;
                    g.mask |= mask;
                }
            }
            return;
        }
        if cluster == first {
            let mut i = end;
            while start < i && infos[i - 1].cluster != first {
                *scratch_flags |= scratch::HAS_GLYPH_FLAGS;
                infos[i - 1].mask |= mask;
                i -= 1;
            }
        } else {
            let mut i = start;
            while i < end && infos[i].cluster != last {
                *scratch_flags |= scratch::HAS_GLYPH_FLAGS;
                infos[i].mask |= mask;
                i += 1;
            }
        }
    }

    /// `hb_buffer_t::_set_glyph_flags`.
    fn set_glyph_flags(&mut self, mask: u32, start: usize, end: usize, interior: bool, from_out: bool) {
        let end = end.min(self.len());
        if interior && !from_out && end.saturating_sub(start) < 2 {
            return;
        }
        self.scratch_flags |= scratch::HAS_GLYPH_FLAGS;
        let level = self.cluster_level;
        if !from_out || !self.have_output {
            if !interior {
                for g in &mut self.info[start..end] {
                    g.mask |= mask;
                }
            } else {
                let cluster = self.infos_find_min_cluster(&self.info, start, end, u32::MAX);
                Self::infos_set_glyph_flags(level, &mut self.scratch_flags, &mut self.info, start, end, cluster, mask);
            }
        } else {
            let out_len = self.out_len();
            let idx = self.idx;
            if !interior {
                for g in &mut self.out_info[start..out_len] {
                    g.mask |= mask;
                }
                for g in &mut self.info[idx..end] {
                    g.mask |= mask;
                }
            } else {
                let c = self.infos_find_min_cluster(&self.info, idx, end, u32::MAX);
                let c = self.infos_find_min_cluster(&self.out_info, start, out_len, c);
                Self::infos_set_glyph_flags(level, &mut self.scratch_flags, &mut self.out_info, start, out_len, c, mask);
                Self::infos_set_glyph_flags(level, &mut self.scratch_flags, &mut self.info, idx, end, c, mask);
            }
        }
    }

    pub fn unsafe_to_break(&mut self, start: usize, end: usize) {
        self.set_glyph_flags(GLYPH_FLAG_UNSAFE_TO_BREAK | GLYPH_FLAG_UNSAFE_TO_CONCAT, start, end, true, false);
    }

    pub fn safe_to_insert_tatweel(&mut self, start: usize, end: usize) {
        if self.flags & FLAG_PRODUCE_SAFE_TO_INSERT_TATWEEL == 0 {
            self.unsafe_to_break(start, end);
            return;
        }
        self.set_glyph_flags(GLYPH_FLAG_SAFE_TO_INSERT_TATWEEL, start, end, true, false);
    }

    pub fn unsafe_to_concat(&mut self, start: usize, end: usize) {
        if self.flags & FLAG_PRODUCE_UNSAFE_TO_CONCAT == 0 {
            return;
        }
        self.set_glyph_flags(GLYPH_FLAG_UNSAFE_TO_CONCAT, start, end, false, false);
    }

    pub fn unsafe_to_break_from_outbuffer(&mut self, start: usize, end: usize) {
        self.set_glyph_flags(GLYPH_FLAG_UNSAFE_TO_BREAK | GLYPH_FLAG_UNSAFE_TO_CONCAT, start, end, true, true);
    }

    pub fn unsafe_to_concat_from_outbuffer(&mut self, start: usize, end: usize) {
        if self.flags & FLAG_PRODUCE_UNSAFE_TO_CONCAT == 0 {
            return;
        }
        self.set_glyph_flags(GLYPH_FLAG_UNSAFE_TO_CONCAT, start, end, false, true);
    }

    pub fn clear_glyph_flags(&mut self, mask: u32) {
        for g in &mut self.info {
            g.mask = (g.mask & !GLYPH_FLAG_DEFINED) | (mask & GLYPH_FLAG_DEFINED);
        }
    }

    /// `hb_buffer_t::sort`: inserção estável, como o C.
    pub fn sort(&mut self, start: usize, end: usize, cmp: impl Fn(&GlyphInfo, &GlyphInfo) -> std::cmp::Ordering) {
        debug_assert!(!self.have_positions);
        for i in start + 1..end {
            let mut j = i;
            while j > start && cmp(&self.info[j - 1], &self.info[i]) == std::cmp::Ordering::Greater {
                j -= 1;
            }
            if i == j {
                continue;
            }
            self.merge_clusters(j, i + 1);
            let t = self.info[i];
            self.info.copy_within(j..i, j + 1);
            self.info[j] = t;
        }
    }

    /// `hb_buffer_t::guess_segment_properties`.
    pub fn guess_segment_properties(&mut self) {
        if self.props.script == SCRIPT_INVALID {
            for g in &self.info {
                let s = unicode::script(g.codepoint);
                if s != SCRIPT_COMMON && s != SCRIPT_INHERITED && s != SCRIPT_UNKNOWN {
                    self.props.script = s;
                    break;
                }
            }
        }
        if self.props.direction == Direction::Invalid {
            self.props.direction = script_horizontal_direction(self.props.script);
            if self.props.direction == Direction::Invalid {
                self.props.direction = Direction::Ltr;
            }
        }
    }
}

/// `hb_script_get_horizontal_direction`.
pub fn script_horizontal_direction(script: u32) -> Direction {
    const RTL: &[&[u8; 4]] = &[
        b"Arab", b"Hebr", b"Syrc", b"Thaa", b"Cprt", b"Khar", b"Phnx", b"Nkoo", b"Lydi", b"Avst", b"Armi",
        b"Phli", b"Prti", b"Sarb", b"Orkh", b"Samr", b"Mand", b"Merc", b"Mero", b"Mani", b"Mend", b"Nbat",
        b"Narb", b"Palm", b"Phlp", b"Hatr", b"Adlm", b"Rohg", b"Sogo", b"Sogd", b"Elym", b"Chrs",
        b"Yezi", b"Ougr", b"Gara",
    ];
    const BIDI: &[&[u8; 4]] = &[b"Hung", b"Ital", b"Runr", b"Tfng"];
    if RTL.iter().any(|t| tag(t) == script) {
        Direction::Rtl
    } else if BIDI.iter().any(|t| tag(t) == script) {
        Direction::Invalid
    } else {
        Direction::Ltr
    }
}
