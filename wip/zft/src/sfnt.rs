//! Leitura das tabelas SFNT que o FreeType usa para fontes TrueType (`sfobjs.c`, `ttload.c`,
//! `ttmtx.c`, `ttcmap.c`, `ttkern.c`).

use crate::Error;

pub(crate) fn u16_at(d: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*d.get(o)?, *d.get(o + 1)?]))
}

pub(crate) fn i16_at(d: &[u8], o: usize) -> Option<i16> {
    u16_at(d, o).map(|v| v as i16)
}

pub(crate) fn u32_at(d: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_be_bytes([*d.get(o)?, *d.get(o + 1)?, *d.get(o + 2)?, *d.get(o + 3)?]))
}

/// Um subtipo de cmap que o FreeType expõe como charmap.
#[derive(Clone, Debug)]
pub(crate) struct CharMap {
    pub platform: u16,
    pub encoding: u16,
    pub offset: usize,
    pub format: u16,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Os2 {
    pub version: u16,
    pub fs_selection: u16,
    pub typo_ascender: i16,
    pub typo_descender: i16,
    pub typo_line_gap: i16,
    pub win_ascent: u16,
    pub win_descent: u16,
    pub x_height: i16,
    pub cap_height: i16,
}

/// As tabelas já interpretadas de uma face.
pub(crate) struct Sfnt {
    pub tables: Vec<([u8; 4], usize, usize)>,
    pub units_per_em: u16,
    pub head_flags: u16,
    pub mac_style: u16,
    pub bbox: [i16; 4],
    pub index_to_loc: i16,
    pub num_glyphs: u16,
    pub max_size_of_instructions: u16,
    pub hhea_ascender: i16,
    pub hhea_descender: i16,
    pub hhea_line_gap: i16,
    pub advance_width_max: u16,
    pub num_hmetrics: u16,
    pub os2: Option<Os2>,
    pub is_fixed_pitch: bool,
    pub charmaps: Vec<CharMap>,
    pub unicode_cmap: Option<usize>,
}

impl Sfnt {
    pub fn table<'a>(&self, data: &'a [u8], tag: &[u8; 4]) -> Option<&'a [u8]> {
        let (off, len) = self.table_range(data, tag)?;
        Some(&data[off..off + len])
    }

    /// Onde a tabela está no arquivo, se ela cabe nele.
    pub fn table_range(&self, data: &[u8], tag: &[u8; 4]) -> Option<(usize, usize)> {
        let &(_, off, len) = self.tables.iter().find(|(t, _, _)| t == tag)?;
        data.get(off..off.checked_add(len)?)?;
        Some((off, len))
    }

    pub fn has(&self, tag: &[u8; 4]) -> bool {
        self.tables.iter().any(|(t, _, l)| t == tag && *l > 0)
    }

    pub fn parse(data: &[u8], index: usize) -> Result<Sfnt, Error> {
        let bad = || Error::UnknownFileFormat;
        let mut base = 0usize;
        let tag = u32_at(data, 0).ok_or(bad())?;
        if tag == u32::from_be_bytes(*b"ttcf") {
            let n = u32_at(data, 8).ok_or(bad())? as usize;
            if index >= n {
                return Err(Error::InvalidArgument);
            }
            base = u32_at(data, 12 + 4 * index).ok_or(bad())? as usize;
        } else if index != 0 {
            return Err(Error::InvalidArgument);
        }
        let ver = u32_at(data, base).ok_or(bad())?;
        if !matches!(ver, 0x0001_0000 | 0x7472_7565 | 0x4F54_544F) {
            return Err(bad());
        }
        let n = usize::from(u16_at(data, base + 4).ok_or(bad())?);
        let mut tables = Vec::with_capacity(n);
        for i in 0..n {
            let r = base + 12 + 16 * i;
            let t = data.get(r..r + 4).ok_or(bad())?;
            let off = u32_at(data, r + 8).ok_or(bad())? as usize;
            let len = u32_at(data, r + 12).ok_or(bad())? as usize;
            tables.push(([t[0], t[1], t[2], t[3]], off, len));
        }
        let mut s = Sfnt {
            tables,
            units_per_em: 0,
            head_flags: 0,
            mac_style: 0,
            bbox: [0; 4],
            index_to_loc: 0,
            num_glyphs: 0,
            max_size_of_instructions: 0,
            hhea_ascender: 0,
            hhea_descender: 0,
            hhea_line_gap: 0,
            advance_width_max: 0,
            num_hmetrics: 0,
            os2: None,
            is_fixed_pitch: false,
            charmaps: Vec::new(),
            unicode_cmap: None,
        };
        let head = s.table(data, b"head").ok_or(Error::TableMissing)?;
        if head.len() < 54 {
            return Err(Error::InvalidTable);
        }
        s.head_flags = u16_at(head, 16).unwrap_or(0);
        s.mac_style = u16_at(head, 44).unwrap_or(0);
        s.units_per_em = u16_at(head, 18).unwrap_or(0);
        for k in 0..4 {
            s.bbox[k] = i16_at(head, 36 + 2 * k).unwrap_or(0);
        }
        s.index_to_loc = i16_at(head, 50).unwrap_or(0);
        let maxp = s.table(data, b"maxp").ok_or(Error::TableMissing)?;
        s.num_glyphs = u16_at(maxp, 4).ok_or(Error::InvalidTable)?;
        if u32_at(maxp, 0) == Some(0x0001_0000) {
            s.max_size_of_instructions = u16_at(maxp, 26).unwrap_or(0);
        }
        let hhea = s.table(data, b"hhea").ok_or(Error::HorizHeaderMissing)?;
        s.hhea_ascender = i16_at(hhea, 4).unwrap_or(0);
        s.hhea_descender = i16_at(hhea, 6).unwrap_or(0);
        s.hhea_line_gap = i16_at(hhea, 8).unwrap_or(0);
        s.advance_width_max = u16_at(hhea, 10).unwrap_or(0);
        s.num_hmetrics = u16_at(hhea, 34).unwrap_or(0);
        if let Some(t) = s.table(data, b"OS/2") {
            if t.len() >= 78 {
                s.os2 = Some(Os2 {
                    version: u16_at(t, 0).unwrap_or(0),
                    fs_selection: u16_at(t, 62).unwrap_or(0),
                    typo_ascender: i16_at(t, 68).unwrap_or(0),
                    typo_descender: i16_at(t, 70).unwrap_or(0),
                    typo_line_gap: i16_at(t, 72).unwrap_or(0),
                    win_ascent: u16_at(t, 74).unwrap_or(0),
                    win_descent: u16_at(t, 76).unwrap_or(0),
                    x_height: if t.len() >= 90 { i16_at(t, 86).unwrap_or(0) } else { 0 },
                    cap_height: if t.len() >= 90 { i16_at(t, 88).unwrap_or(0) } else { 0 },
                });
            }
        }
        if let Some(t) = s.table(data, b"post") {
            s.is_fixed_pitch = u32_at(t, 12).unwrap_or(0) != 0;
        }
        s.read_cmaps(data);
        Ok(s)
    }

    fn read_cmaps(&mut self, data: &[u8]) {
        let Some(&(_, coff, clen)) = self.tables.iter().find(|(t, _, _)| t == b"cmap") else { return };
        let Some(cmap) = data.get(coff..coff + clen) else { return };
        let n = usize::from(u16_at(cmap, 2).unwrap_or(0));
        for i in 0..n {
            let r = 4 + 8 * i;
            let (Some(p), Some(e), Some(o)) = (u16_at(cmap, r), u16_at(cmap, r + 2), u32_at(cmap, r + 4)) else {
                break;
            };
            let o = o as usize;
            let Some(f) = u16_at(cmap, o) else { continue };
            if !matches!(f, 0 | 4 | 6 | 12 | 13) {
                continue;
            }
            self.charmaps.push(CharMap { platform: p, encoding: e, offset: coff + o, format: f });
        }
        let unicode = |c: &CharMap| match c.platform {
            0 | 2 => true,
            3 => matches!(c.encoding, 1 | 10),
            _ => false,
        };
        let ucs4 = |c: &CharMap| (c.platform == 3 && c.encoding == 10) || (c.platform == 0 && c.encoding == 4);
        self.unicode_cmap = (0..self.charmaps.len())
            .rev()
            .find(|&i| unicode(&self.charmaps[i]) && ucs4(&self.charmaps[i]))
            .or_else(|| (0..self.charmaps.len()).rev().find(|&i| unicode(&self.charmaps[i])));
    }

    /// `FT_Get_Char_Index` no charmap selecionado.
    pub fn char_index(&self, data: &[u8], code: u32) -> u32 {
        let Some(ci) = self.unicode_cmap else { return 0 };
        let cm = &self.charmaps[ci];
        let g = cmap_lookup(data, cm.offset, cm.format, code).unwrap_or(0);
        if g >= u32::from(self.num_glyphs) { 0 } else { g }
    }

    /// Todos os pares (código, glifo) do charmap Unicode com glifo válido, em ordem de código,
    /// como os devolveria uma sequência de `FT_Get_Next_Char`.
    pub fn mapped_chars(&self, data: &[u8]) -> Vec<(u32, u32)> {
        let Some(ci) = self.unicode_cmap else { return Vec::new() };
        let cm = &self.charmaps[ci];
        let (d, o) = (data, cm.offset);
        let mut codes: Vec<u32> = Vec::new();
        match cm.format {
            0 => codes.extend(0..256),
            4 => {
                let seg2 = usize::from(u16_at(d, o + 6).unwrap_or(0));
                for i in 0..seg2 / 2 {
                    let end = u32::from(u16_at(d, o + 14 + 2 * i).unwrap_or(0));
                    let start = u32::from(u16_at(d, o + 16 + seg2 + 2 * i).unwrap_or(0));
                    if start <= end {
                        codes.extend(start..=end);
                    }
                }
            }
            6 => {
                let first = u32::from(u16_at(d, o + 6).unwrap_or(0));
                let count = u32::from(u16_at(d, o + 8).unwrap_or(0));
                codes.extend(first..first + count);
            }
            12 | 13 => {
                let n = u32_at(d, o + 12).unwrap_or(0) as usize;
                for i in 0..n {
                    let r = o + 16 + 12 * i;
                    let (Some(start), Some(end)) = (u32_at(d, r), u32_at(d, r + 4)) else { break };
                    if start <= end && end <= 0x10FFFF {
                        codes.extend(start..=end);
                    }
                }
            }
            _ => {}
        }
        codes.sort_unstable();
        codes.dedup();
        codes
            .into_iter()
            .filter_map(|c| {
                let g = self.char_index(data, c);
                (g != 0).then_some((c, g))
            })
            .collect()
    }

    /// `tt_face_get_metrics`: avanço e bearing horizontais em unidades da fonte.
    pub fn hmetrics(&self, data: &[u8], gid: u32) -> (u16, i16) {
        let Some(t) = self.table(data, b"hmtx") else { return (0, 0) };
        let nh = u32::from(self.num_hmetrics);
        if nh == 0 {
            return (0, 0);
        }
        if gid < nh {
            let o = 4 * gid as usize;
            (u16_at(t, o).unwrap_or(0), i16_at(t, o + 2).unwrap_or(0))
        } else {
            let adv = u16_at(t, 4 * (nh as usize - 1)).unwrap_or(0);
            let o = 4 * nh as usize + 2 * (gid - nh) as usize;
            (adv, i16_at(t, o).unwrap_or(0))
        }
    }

    /// Faixa do glifo na tabela `glyf` (`tt_face_get_location`).
    pub fn location(&self, data: &[u8], gid: u32) -> Option<(usize, usize)> {
        let loca = self.table(data, b"loca")?;
        let g = gid as usize;
        let (a, b) = if self.index_to_loc == 0 {
            (usize::from(u16_at(loca, 2 * g)?) * 2, usize::from(u16_at(loca, 2 * g + 2).unwrap_or(0)) * 2)
        } else {
            (u32_at(loca, 4 * g)? as usize, u32_at(loca, 4 * g + 4).unwrap_or(0) as usize)
        };
        let glyf = self.tables.iter().find(|(t, _, _)| t == b"glyf")?;
        // Como o `tt_face_get_location`: fim além da tabela é truncado; fim antes do início vira 0.
        let b = b.min(glyf.2);
        if b <= a { return Some((glyf.1 + a.min(glyf.2), 0)) }
        Some((glyf.1 + a, b - a))
    }

    /// `tt_face_get_kerning` (formato 0 da tabela `kern`).
    pub fn kerning(&self, data: &[u8], left: u32, right: u32) -> i32 {
        let Some(t) = self.table(data, b"kern") else { return 0 };
        let n = usize::from(u16_at(t, 2).unwrap_or(0));
        let mut p = 4;
        let key = (left << 16) | right;
        let mut result = 0i32;
        for _ in 0..n {
            let (Some(len), Some(cov)) = (u16_at(t, p + 2), u16_at(t, p + 4)) else { break };
            // Só formato 0, horizontal, sem cross-stream nem mínimo.
            if cov & 0xFF07 == 0x0001 {
                let np = usize::from(u16_at(t, p + 6).unwrap_or(0));
                let (mut lo, mut hi) = (0usize, np);
                while lo < hi {
                    let mid = (lo + hi) / 2;
                    let r = p + 14 + 6 * mid;
                    let k = u32_at(t, r).unwrap_or(0);
                    if k == key {
                        let v = i32::from(i16_at(t, r + 4).unwrap_or(0));
                        if cov & 8 != 0 { result = v } else { result += v }
                        break;
                    }
                    if k < key { lo = mid + 1 } else { hi = mid }
                }
            }
            p += usize::from(len).max(6);
        }
        result
    }
}

fn cmap_lookup(d: &[u8], o: usize, format: u16, code: u32) -> Option<u32> {
    match format {
        0 => {
            if code < 256 {
                Some(u32::from(*d.get(o + 6 + code as usize)?))
            } else {
                Some(0)
            }
        }
        4 => {
            if code > 0xFFFF {
                return Some(0);
            }
            let seg2 = usize::from(u16_at(d, o + 6)?);
            let ends = o + 14;
            let starts = ends + seg2 + 2;
            let deltas = starts + seg2;
            let ranges = deltas + seg2;
            let (mut lo, mut hi) = (0usize, seg2 / 2);
            while lo < hi {
                let mid = (lo + hi) / 2;
                let end = u32::from(u16_at(d, ends + 2 * mid)?);
                let start = u32::from(u16_at(d, starts + 2 * mid)?);
                if code < start {
                    hi = mid;
                } else if code > end {
                    lo = mid + 1;
                } else {
                    let delta = u32::from(u16_at(d, deltas + 2 * mid)?);
                    let ro = usize::from(u16_at(d, ranges + 2 * mid)?);
                    if ro == 0xFFFF {
                        return Some(0);
                    }
                    if ro == 0 {
                        return Some((code + delta) & 0xFFFF);
                    }
                    let p = ranges + 2 * mid + ro + 2 * (code - start) as usize;
                    let g = u32::from(u16_at(d, p)?);
                    return Some(if g == 0 { 0 } else { (g + delta) & 0xFFFF });
                }
            }
            Some(0)
        }
        6 => {
            let first = u32::from(u16_at(d, o + 6)?);
            let count = u32::from(u16_at(d, o + 8)?);
            if code < first || code >= first + count {
                return Some(0);
            }
            Some(u32::from(u16_at(d, o + 10 + 2 * (code - first) as usize)?))
        }
        12 | 13 => {
            let n = u32_at(d, o + 12)? as usize;
            let (mut lo, mut hi) = (0usize, n);
            while lo < hi {
                let mid = (lo + hi) / 2;
                let r = o + 16 + 12 * mid;
                let start = u32_at(d, r)?;
                let end = u32_at(d, r + 4)?;
                if code < start {
                    hi = mid;
                } else if code > end {
                    lo = mid + 1;
                } else {
                    let g = u32_at(d, r + 8)?;
                    return Some(if format == 12 { g.wrapping_add(code - start) } else { g });
                }
            }
            Some(0)
        }
        _ => Some(0),
    }
}
