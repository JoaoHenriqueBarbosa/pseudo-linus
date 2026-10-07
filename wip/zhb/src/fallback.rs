//! Posicionamento sem GPOS (`hb-ot-shape-fallback.cc`) e a tabela `kern` do OpenType
//! (`hb-ot-kern-table.hh` com a máquina de `hb-kern.hh`).

use crate::buffer::{scratch, Buffer, Direction};
use crate::font::{Font, GlyphExtents};
use crate::gsubgpos::ApplyContext;
use crate::ot::{i16at, u16at, u32at, u8at};
use crate::unicode::{gc, space};

pub const CC_ATTACHED_BELOW_LEFT: u8 = 200;
pub const CC_ATTACHED_BELOW: u8 = 202;
pub const CC_ATTACHED_ABOVE: u8 = 214;
pub const CC_ATTACHED_ABOVE_RIGHT: u8 = 216;
pub const CC_BELOW_LEFT: u8 = 218;
pub const CC_BELOW: u8 = 220;
pub const CC_BELOW_RIGHT: u8 = 222;
pub const CC_ABOVE_LEFT: u8 = 228;
pub const CC_ABOVE: u8 = 230;
pub const CC_ABOVE_RIGHT: u8 = 232;
pub const CC_DOUBLE_BELOW: u8 = 233;
pub const CC_DOUBLE_ABOVE: u8 = 234;

/// `LookupFlag::IgnoreMarks`.
const IGNORE_MARKS: u32 = 0x0008;

fn recategorize_combining_class(u: u32, klass: u8) -> u8 {
    if klass >= 200 {
        return klass;
    }
    let mut klass = klass;
    if u & !0xFF == 0x0E00 {
        if klass == 0 {
            match u {
                0x0E31 | 0x0E34 | 0x0E35 | 0x0E36 | 0x0E37 | 0x0E47 | 0x0E4C | 0x0E4D | 0x0E4E => klass = CC_ABOVE_RIGHT,
                0x0EB1 | 0x0EB4 | 0x0EB5 | 0x0EB6 | 0x0EB7 | 0x0EBB | 0x0ECC | 0x0ECD => klass = CC_ABOVE,
                0x0EBC => klass = CC_BELOW,
                _ => {}
            }
        } else if u == 0x0E3A {
            klass = CC_BELOW_RIGHT;
        }
    }
    // As classes modificadas (`HB_MODIFIED_COMBINING_CLASS_CCC*`) pelo valor.
    match klass {
        22 | 15 | 16 | 17 | 23 | 18 | 19 | 20 | 21 | 24 | 25 => CC_BELOW,
        13 => CC_ATTACHED_ABOVE,
        10 => CC_ABOVE_RIGHT,
        11 | 14 => CC_ABOVE_LEFT,
        26 => CC_ABOVE,
        12 => klass,
        28 | 29 | 31 | 32 | 27 | 34 | 35 | 36 => CC_ABOVE,
        30 | 33 => CC_BELOW,
        3 => CC_BELOW_RIGHT,
        107 => CC_ABOVE_RIGHT,
        118 => CC_BELOW,
        122 => CC_ABOVE,
        129 => CC_BELOW,
        132 => CC_ABOVE,
        131 => CC_BELOW,
        _ => klass,
    }
}

/// `_hb_ot_shape_fallback_mark_position_recategorize_marks`.
pub fn recategorize_marks(buffer: &mut Buffer) {
    for info in &mut buffer.info {
        if info.general_category() == gc::NON_SPACING_MARK {
            let c = recategorize_combining_class(info.codepoint, info.modified_combining_class());
            info.set_modified_combining_class(c);
        }
    }
}

pub fn zero_mark_advances(buffer: &mut Buffer, start: usize, end: usize, adjust_offsets_when_zeroing: bool) {
    for i in start..end {
        if buffer.info[i].general_category() == gc::NON_SPACING_MARK {
            let p = &mut buffer.pos[i];
            if adjust_offsets_when_zeroing {
                p.x_offset -= p.x_advance;
                p.y_offset -= p.y_advance;
            }
            p.x_advance = 0;
            p.y_advance = 0;
        }
    }
}

fn position_mark(font: &Font, buffer: &mut Buffer, base: &mut GlyphExtents, i: usize, combining_class: u8) {
    let Some(mark) = font.glyph_extents(buffer.info[i].codepoint) else { return };
    let y_gap = font.y_scale / 16;
    let direction = buffer.props.direction;
    let pos = &mut buffer.pos[i];
    pos.x_offset = 0;
    pos.y_offset = 0;
    let centered = base.x_bearing + (base.width - mark.width) / 2 - mark.x_bearing;
    match combining_class {
        CC_DOUBLE_BELOW | CC_DOUBLE_ABOVE if direction == Direction::Ltr => {
            pos.x_offset += base.x_bearing + base.width - mark.width / 2 - mark.x_bearing;
        }
        CC_DOUBLE_BELOW | CC_DOUBLE_ABOVE if direction == Direction::Rtl => {
            pos.x_offset += base.x_bearing - mark.width / 2 - mark.x_bearing;
        }
        CC_ATTACHED_BELOW_LEFT | CC_BELOW_LEFT | CC_ABOVE_LEFT => {
            pos.x_offset += base.x_bearing - mark.x_bearing;
        }
        CC_ATTACHED_ABOVE_RIGHT | CC_BELOW_RIGHT | CC_ABOVE_RIGHT => {
            pos.x_offset += base.x_bearing + base.width - mark.width - mark.x_bearing;
        }
        _ => pos.x_offset += centered,
    }
    match combining_class {
        CC_DOUBLE_BELOW | CC_BELOW_LEFT | CC_BELOW | CC_BELOW_RIGHT | CC_ATTACHED_BELOW_LEFT | CC_ATTACHED_BELOW => {
            if !matches!(combining_class, CC_ATTACHED_BELOW_LEFT | CC_ATTACHED_BELOW) {
                base.height -= y_gap;
            }
            pos.y_offset = base.y_bearing + base.height - mark.y_bearing;
            if (y_gap > 0) == (pos.y_offset > 0) {
                base.height -= pos.y_offset;
                pos.y_offset = 0;
            }
            base.height += mark.height;
        }
        CC_DOUBLE_ABOVE | CC_ABOVE_LEFT | CC_ABOVE | CC_ABOVE_RIGHT | CC_ATTACHED_ABOVE | CC_ATTACHED_ABOVE_RIGHT => {
            if !matches!(combining_class, CC_ATTACHED_ABOVE | CC_ATTACHED_ABOVE_RIGHT) {
                base.y_bearing += y_gap;
                base.height -= y_gap;
            }
            pos.y_offset = base.y_bearing - (mark.y_bearing + mark.height);
            if (y_gap > 0) != (pos.y_offset > 0) {
                let correction = -pos.y_offset / 2;
                base.y_bearing += correction;
                base.height -= correction;
                pos.y_offset += correction;
            }
            base.y_bearing -= mark.height;
            base.height += mark.height;
        }
        _ => {}
    }
}

fn position_around_base(
    font: &Font,
    plan_direction: Direction,
    plan_script: u32,
    buffer: &mut Buffer,
    base: usize,
    end: usize,
    adjust_offsets_when_zeroing: bool,
) {
    let mut horiz_dir = Direction::Invalid;
    buffer.unsafe_to_break(base, end);
    let Some(mut base_extents) = font.glyph_extents(buffer.info[base].codepoint) else {
        zero_mark_advances(buffer, base + 1, end, adjust_offsets_when_zeroing);
        return;
    };
    base_extents.y_bearing += buffer.pos[base].y_offset;
    base_extents.x_bearing = 0;
    base_extents.width = font.h_advance(buffer.info[base].codepoint);
    let lig_id = buffer.info[base].lig_id();
    let num_lig_components = buffer.info[base].lig_num_comps() as i32;
    let forward = buffer.props.direction.is_forward();
    let (mut x_offset, mut y_offset) = (0i32, 0i32);
    if forward {
        x_offset -= buffer.pos[base].x_advance;
        y_offset -= buffer.pos[base].y_advance;
    }
    let mut component_extents = base_extents;
    let mut last_lig_component = -1i32;
    let mut last_combining_class = 255u32;
    let mut cluster_extents = base_extents;
    for i in base + 1..end {
        let this_cc = buffer.info[i].modified_combining_class();
        if this_cc != 0 {
            if num_lig_components > 1 {
                let this_lig_id = buffer.info[i].lig_id();
                let mut this_lig_component = buffer.info[i].lig_comp() as i32 - 1;
                if lig_id == 0 || lig_id != this_lig_id || this_lig_component >= num_lig_components {
                    this_lig_component = num_lig_components - 1;
                }
                if last_lig_component != this_lig_component {
                    last_lig_component = this_lig_component;
                    last_combining_class = 255;
                    component_extents = base_extents;
                    if horiz_dir == Direction::Invalid {
                        horiz_dir = if plan_direction.is_horizontal() {
                            plan_direction
                        } else {
                            crate::buffer::script_horizontal_direction(plan_script)
                        };
                    }
                    if horiz_dir == Direction::Ltr {
                        component_extents.x_bearing += (this_lig_component * component_extents.width) / num_lig_components;
                    } else {
                        component_extents.x_bearing +=
                            ((num_lig_components - 1 - this_lig_component) * component_extents.width) / num_lig_components;
                    }
                    component_extents.width /= num_lig_components;
                }
            }
            if last_combining_class != u32::from(this_cc) {
                last_combining_class = u32::from(this_cc);
                cluster_extents = component_extents;
            }
            position_mark(font, buffer, &mut cluster_extents, i, this_cc);
            let p = &mut buffer.pos[i];
            p.x_advance = 0;
            p.y_advance = 0;
            p.x_offset += x_offset;
            p.y_offset += y_offset;
        } else if forward {
            x_offset -= buffer.pos[i].x_advance;
            y_offset -= buffer.pos[i].y_advance;
        } else {
            x_offset += buffer.pos[i].x_advance;
            y_offset += buffer.pos[i].y_advance;
        }
    }
}

fn position_cluster(
    font: &Font,
    plan_direction: Direction,
    plan_script: u32,
    buffer: &mut Buffer,
    start: usize,
    end: usize,
    adjust: bool,
) {
    if end - start < 2 {
        return;
    }
    let mut i = start;
    while i < end {
        if !buffer.info[i].is_unicode_mark() {
            let mut j = i + 1;
            while j < end && buffer.info[j].is_unicode_mark() {
                j += 1;
            }
            position_around_base(font, plan_direction, plan_script, buffer, i, j, adjust);
            i = j - 1;
        }
        i += 1;
    }
}

/// `_hb_ot_shape_fallback_mark_position`.
pub fn mark_position(font: &Font, plan_direction: Direction, plan_script: u32, buffer: &mut Buffer, adjust: bool) {
    let mut start = 0;
    let count = buffer.len();
    for i in 1..count {
        if !buffer.info[i].is_unicode_mark() {
            position_cluster(font, plan_direction, plan_script, buffer, start, i, adjust);
            start = i;
        }
    }
    position_cluster(font, plan_direction, plan_script, buffer, start, count, adjust);
}

/// `hb_kern_machine_t::kern`.
fn kern_machine(
    font: &Font,
    buffer: &mut Buffer,
    kern_mask: u32,
    scale: bool,
    cross_stream: bool,
    get_kerning: &dyn Fn(u32, u32) -> i32,
) {
    buffer.unsafe_to_concat(0, buffer.len());
    let horizontal = buffer.props.direction.is_horizontal();
    let count = buffer.len();
    let mut c = ApplyContext::new(1, font, buffer);
    c.set_lookup_mask(kern_mask);
    c.lookup_props = IGNORE_MARKS;
    let mut idx = 0;
    while idx < count {
        if c.buffer.info[idx].mask & kern_mask == 0 {
            idx += 1;
            continue;
        }
        let mut skippy = c.iter(false);
        skippy.reset(&c, idx);
        let mut unsafe_to = 0;
        if !skippy.next(&c, Some(&mut unsafe_to)) {
            idx += 1;
            continue;
        }
        let i = idx;
        let j = skippy.idx;
        let mut kern = get_kerning(c.buffer.info[i].codepoint, c.buffer.info[j].codepoint);
        if kern != 0 {
            let pos = &mut c.buffer.pos;
            if horizontal {
                if scale {
                    kern = font.em_scale_x(kern as i16);
                }
                if cross_stream {
                    pos[j].y_offset = kern;
                    c.buffer.scratch_flags |= scratch::HAS_GPOS_ATTACHMENT;
                } else {
                    let kern1 = kern >> 1;
                    let kern2 = kern - kern1;
                    pos[i].x_advance += kern1;
                    pos[j].x_advance += kern2;
                    pos[j].x_offset += kern2;
                }
            } else {
                if scale {
                    kern = font.em_scale_y(kern as i16);
                }
                if cross_stream {
                    pos[j].x_offset = kern;
                    c.buffer.scratch_flags |= scratch::HAS_GPOS_ATTACHMENT;
                } else {
                    let kern1 = kern >> 1;
                    let kern2 = kern - kern1;
                    pos[i].y_advance += kern1;
                    pos[j].y_advance += kern2;
                    pos[j].y_offset += kern2;
                }
            }
            c.buffer.unsafe_to_break(i, j + 1);
        }
        idx = skippy.idx;
    }
}

/// `_hb_ot_shape_fallback_kern`: o kerning do `FT_Get_Kerning` pela fonte, já escalado.
pub fn fallback_kern(font: &Font, buffer: &mut Buffer, kern_mask: u32) {
    if !buffer.props.direction.is_horizontal() || font.kern.is_none() {
        // O hb-ft só tem kerning horizontal, e o `FT_Get_Kerning` sem `kern` dá zero.
        return;
    }
    let reverse = buffer.props.direction.is_backward();
    if reverse {
        buffer.reverse();
    }
    kern_machine(font, buffer, kern_mask, false, false, &|a, b| font.h_kerning(a, b));
    if reverse {
        buffer.reverse();
    }
}

/// Um subtable de formato 0 do `kern` OpenType: pares ordenados.
fn kern_format0(d: &[u8], left: u32, right: u32) -> i32 {
    let n = usize::from(u16at(d, 0));
    let key = (left << 16) | right;
    let (mut lo, mut hi) = (0usize, n);
    while lo < hi {
        let mid = (lo + hi) / 2;
        let r = 8 + mid * 6;
        if d.len() < r + 6 {
            return 0;
        }
        let k = (u32::from(u16at(d, r)) << 16) | u32::from(u16at(d, r + 2));
        if k == key {
            return i32::from(i16at(d, r + 4));
        }
        if k < key { lo = mid + 1 } else { hi = mid }
    }
    0
}

/// `ClassTable<HBUINT16>` dos `ObsoleteTypes`: primeiro glifo, quantidade e um valor por glifo;
/// fora da faixa vale 0.
fn obsolete_class(st: &[u8], table: usize, glyph: u32) -> usize {
    if st.len() < table + 4 {
        return 0;
    }
    let first = u32::from(u16at(st, table));
    let n = u32::from(u16at(st, table + 2));
    let i = glyph.wrapping_sub(first);
    if glyph < first || i >= n || st.len() < table + 4 + 2 * i as usize + 2 {
        return 0;
    }
    usize::from(u16at(st, table + 4 + 2 * i as usize))
}

/// `KerxSubTableFormat2::get_kerning` do `kern`: os valores das classes são deslocamentos em bytes
/// desde o início do subtable, somados e convertidos em índice no array de `FWORD`. Os campos vêm
/// depois do cabeçalho de `h` bytes (6 no OpenType, 8 na Apple).
fn kern_format2(st: &[u8], h: usize, left: u32, right: u32) -> i32 {
    if st.len() < h + 8 {
        return 0;
    }
    let left_table = usize::from(u16at(st, h + 2));
    let right_table = usize::from(u16at(st, h + 4));
    let array = usize::from(u16at(st, h + 6));
    let offset = obsolete_class(st, left_table, left) + obsolete_class(st, right_table, right);
    // `ObsoleteTypes::offsetToIndex`: (deslocamento menos a posição do array) / 2.
    let Some(rel) = offset.checked_sub(array) else { return 0 };
    let at = array + (rel / 2) * 2;
    if st.len() < at + 2 {
        return 0;
    }
    i32::from(i16at(st, at))
}

/// `KerxSubTableFormat3::get_kerning`: classes de um byte por glifo e índice na matriz de valores.
fn kern_format3(st: &[u8], h: usize, left: u32, right: u32) -> i32 {
    if st.len() < h + 6 {
        return 0;
    }
    let glyph_count = u32::from(u16at(st, h));
    let value_count = usize::from(u8at(st, h + 2));
    let left_count = usize::from(u8at(st, h + 3));
    let right_count = usize::from(u8at(st, h + 4));
    if left >= glyph_count || right >= glyph_count {
        return 0;
    }
    let values = h + 6;
    let left_class = values + 2 * value_count;
    let right_class = left_class + glyph_count as usize;
    let index = right_class + glyph_count as usize;
    let get = |p: usize| if p < st.len() { usize::from(st[p]) } else { 0 };
    let (l, r) = (get(left_class + left as usize), get(right_class + right as usize));
    if l >= left_count || r >= right_count {
        return 0;
    }
    let i = get(index + l * right_count + r);
    if i >= value_count || st.len() < values + 2 * i + 2 {
        return 0;
    }
    i32::from(i16at(st, values + 2 * i))
}

/// `hb_ot_layout_has_kerning`: `kern::has_data`, os primeiros 32 bits não nulos (versão 0 do
/// OpenType com subtables, ou a 1.0 da Apple).
pub fn has_kern_table(font: &Font) -> bool {
    font.kern.is_some_and(|k| k.len() >= 4 && u32at(k, 0) != 0)
}

/// Um subtable do `kern`, com os bits de cobertura já lidos conforme a variante.
pub struct KernSubtable<'a> {
    pub format: u8,
    pub horizontal: bool,
    pub cross_stream: bool,
    /// `Variation` da Apple: subtable de variação, que o HarfBuzz pula.
    pub variation: bool,
    /// Tamanho do cabeçalho: 6 no `KernOT`, 8 no `KernAAT`.
    pub header: usize,
    pub data: &'a [u8],
}

/// Os subtables de `KernOT` (versão de 16 bits igual a 0) ou de `KernAAT` (versão 1.0 de 32
/// bits); outras versões não têm subtables para o HarfBuzz.
pub fn kern_subtables(k: &[u8]) -> Vec<KernSubtable<'_>> {
    let mut out = Vec::new();
    if k.len() < 4 {
        return out;
    }
    let aat = match u16at(k, 0) {
        0 => false,
        1 if k.len() >= 8 => true,
        _ => return out,
    };
    let (count, mut off, header) = if aat { (u32at(k, 4) as usize, 8, 8) } else { (usize::from(u16at(k, 2)), 4, 6) };
    for _ in 0..count {
        if k.len() < off + header {
            break;
        }
        let (length, format, coverage) = if aat {
            (u32at(k, off) as usize, u8at(k, off + 5), u8at(k, off + 4))
        } else {
            (usize::from(u16at(k, off + 2)), u8at(k, off + 4), u8at(k, off + 5))
        };
        // O último subtable vai até o fim da tabela, como o `sanitize` do HarfBuzz admite.
        let end = if length < header { k.len() } else { off.saturating_add(length).min(k.len()) };
        out.push(if aat {
            KernSubtable {
                format,
                horizontal: coverage & 0x80 == 0,
                cross_stream: coverage & 0x40 != 0,
                variation: coverage & 0x20 != 0,
                header,
                data: &k[off..end],
            }
        } else {
            KernSubtable {
                format,
                horizontal: coverage & 0x01 != 0,
                cross_stream: coverage & 0x04 != 0,
                variation: false,
                header,
                data: &k[off..end],
            }
        });
        off = off.saturating_add(length.max(header));
    }
    out
}

/// `hb_ot_layout_kern`: o `KerxTable::apply` com o `kern` OpenType ou o da Apple.
pub fn apply_kern_table(font: &Font, buffer: &mut Buffer, kern_mask: u32) {
    let Some(k) = font.kern.filter(|_| has_kern_table(font)) else { return };
    buffer.unsafe_to_concat(0, buffer.len());
    let mut seen_cross_stream = false;
    for st in kern_subtables(k) {
        if st.variation {
            continue;
        }
        let (h, cross, data) = (st.header, st.cross_stream, st.data);
        if buffer.props.direction.is_horizontal() == st.horizontal {
            if !seen_cross_stream && cross {
                seen_cross_stream = true;
                let fwd = buffer.props.direction.is_forward();
                for p in &mut buffer.pos {
                    p.attach_type = crate::gpos::ATTACH_TYPE_CURSIVE;
                    p.attach_chain = if fwd { -1 } else { 1 };
                }
            }
            let reverse = buffer.props.direction.is_backward();
            if reverse {
                buffer.reverse();
            }
            if kern_mask != 0 {
                match st.format {
                    0 => {
                        let body = &data[h.min(data.len())..];
                        kern_machine(font, buffer, kern_mask, true, cross, &|a, b| kern_format0(body, a, b));
                    }
                    2 if st.horizontal => kern_machine(font, buffer, kern_mask, true, cross, &|a, b| kern_format2(data, h, a, b)),
                    3 => kern_machine(font, buffer, kern_mask, true, cross, &|a, b| kern_format3(data, h, a, b)),
                    _ => {}
                }
            }
            if reverse {
                buffer.reverse();
            }
        }
    }
}

/// `_hb_ot_shape_fallback_spaces`.
pub fn fallback_spaces(font: &Font, buffer: &mut Buffer) {
    let horizontal = buffer.props.direction.is_horizontal();
    let invisible = buffer.invisible;
    for i in 0..buffer.len() {
        let info = buffer.info[i];
        if !info.is_unicode_space() || info.ligated() {
            continue;
        }
        let pos = &mut buffer.pos[i];
        if invisible != 0 && info.codepoint == invisible {
            if horizontal {
                pos.x_advance = font.x_scale / 4;
            } else {
                pos.y_advance = -font.y_scale / 4;
            }
        }
        let t = info.unicode_space_fallback_type();
        match t {
            space::SPACE_EM | space::SPACE_EM_2 | space::SPACE_EM_3 | space::SPACE_EM_4 | space::SPACE_EM_5
            | space::SPACE_EM_6 | space::SPACE_EM_16 => {
                let n = i32::from(t);
                if horizontal {
                    pos.x_advance = (font.x_scale + n / 2) / n;
                } else {
                    pos.y_advance = -(font.y_scale + n / 2) / n;
                }
            }
            space::SPACE_4_EM_18 => {
                if horizontal {
                    pos.x_advance = (i64::from(font.x_scale) * 4 / 18) as i32;
                } else {
                    pos.y_advance = (-i64::from(font.y_scale) * 4 / 18) as i32;
                }
            }
            space::SPACE_FIGURE => {
                if let Some(g) = (b'0'..=b'9').find_map(|u| font.nominal_glyph(u32::from(u))) {
                    if horizontal {
                        pos.x_advance = font.h_advance(g);
                    } else {
                        pos.y_advance = font.v_advance(g);
                    }
                }
            }
            space::SPACE_PUNCTUATION => {
                if let Some(g) = font.nominal_glyph(u32::from(b'.')).or_else(|| font.nominal_glyph(u32::from(b','))) {
                    if horizontal {
                        pos.x_advance = font.h_advance(g);
                    } else {
                        pos.y_advance = font.v_advance(g);
                    }
                }
            }
            space::SPACE_NARROW => {
                if horizontal {
                    pos.x_advance /= 2;
                } else {
                    pos.y_advance /= 2;
                }
            }
            _ => {}
        }
    }
}
