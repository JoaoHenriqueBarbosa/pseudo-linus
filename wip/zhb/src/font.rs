//! `hb_font_t` com as funções do `hb-ft.cc` sobre uma face do zft, como o raqm cria com
//! `hb_ft_font_create_referenced` (load flags `FT_LOAD_DEFAULT | FT_LOAD_NO_HINTING`).

use std::cell::RefCell;

use crate::ot::{Gdef, GsubGpos};

/// As tabelas de layout copiadas da face, para que as visões vivam fora do `RefCell`.
pub struct Tables {
    pub gsub: Option<Vec<u8>>,
    pub gpos: Option<Vec<u8>>,
    pub gdef: Option<Vec<u8>>,
    pub kern: Option<Vec<u8>>,
}

impl Tables {
    pub fn load(face: &zft::Face) -> Tables {
        let t = |tag: &[u8; 4]| face.table(tag).map(<[u8]>::to_vec);
        Tables { gsub: t(b"GSUB"), gpos: t(b"GPOS"), gdef: t(b"GDEF"), kern: t(b"kern") }
    }
}

/// O `roundf` do HarfBuzz (`hb-algs.hh` o redefine): `floorf (x + .5f)`, que leva -0,5 a 0.
pub fn hb_roundf(v: f32) -> i32 {
    (v + 0.5).floor() as i32
}

/// `hb_glyph_extents_t`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GlyphExtents {
    pub x_bearing: i32,
    pub y_bearing: i32,
    pub width: i32,
    pub height: i32,
}

pub struct Font<'a> {
    face: &'a RefCell<zft::Face>,
    pub gsub: GsubGpos<'a>,
    pub gpos: GsubGpos<'a>,
    pub gdef: Gdef<'a>,
    pub kern: Option<&'a [u8]>,
    pub upem: i32,
    pub x_scale: i32,
    pub y_scale: i32,
    pub x_mult: i64,
    pub y_mult: i64,
    pub x_multf: f32,
    pub y_multf: f32,
    pub x_ppem: u32,
    pub y_ppem: u32,
    face_x_scale: i64,
    face_y_scale: i64,
}

const LOAD_FLAGS: u32 = zft::LOAD_NO_HINTING;

impl<'a> Font<'a> {
    /// `hb_ft_font_create`: escala e ppem tirados do tamanho atual da face.
    pub fn new(face: &'a RefCell<zft::Face>, tables: &'a Tables) -> Font<'a> {
        let f = face.borrow();
        let upem = f.units_per_em as i32;
        let m = f.size;
        let x_scale = ((m.x_scale as u64 * f.units_per_em as u64 + (1 << 15)) >> 16) as i32;
        let y_scale = ((m.y_scale as u64 * f.units_per_em as u64 + (1 << 15)) >> 16) as i32;
        let gsub_len = tables.gsub.as_ref().map_or(0, Vec::len);
        let gpos_len = tables.gpos.as_ref().map_or(0, Vec::len);
        let mult = |s: i32| -> i64 {
            if s < 0 { -((-(s as i64)) << 16) / i64::from(upem) } else { (i64::from(s) << 16) / i64::from(upem) }
        };
        Font {
            face,
            gsub: GsubGpos::new(tables.gsub.as_deref()),
            gpos: GsubGpos::new(tables.gpos.as_deref()),
            gdef: Gdef::new(tables.gdef.as_deref(), gsub_len, gpos_len),
            kern: tables.kern.as_deref(),
            upem,
            x_scale,
            y_scale,
            x_mult: mult(x_scale),
            y_mult: mult(y_scale),
            x_multf: x_scale as f32 / upem as f32,
            y_multf: y_scale as f32 / upem as f32,
            x_ppem: u32::from(m.x_ppem),
            y_ppem: u32::from(m.y_ppem),
            face_x_scale: m.x_scale,
            face_y_scale: m.y_scale,
        }
    }

    pub fn em_scale_x(&self, v: i16) -> i32 {
        ((i64::from(v) * self.x_mult + 32768) >> 16) as i32
    }
    pub fn em_scale_y(&self, v: i16) -> i32 {
        ((i64::from(v) * self.y_mult + 32768) >> 16) as i32
    }
    pub fn em_fscale_x(&self, v: i16) -> f32 {
        f32::from(v) * self.x_multf
    }
    pub fn em_fscale_y(&self, v: i16) -> f32 {
        f32::from(v) * self.y_multf
    }
    pub fn em_scalef_x(&self, v: f32) -> i32 {
        hb_roundf(v * self.x_multf)
    }
    pub fn em_scalef_y(&self, v: f32) -> i32 {
        hb_roundf(v * self.y_multf)
    }

    /// `hb_ft_get_nominal_glyph`.
    pub fn nominal_glyph(&self, u: u32) -> Option<u32> {
        let g = self.face.borrow().char_index(u);
        (g != 0).then_some(g)
    }

    /// `hb_ft_get_variation_glyph` (`FT_Face_GetCharVariantIndex`).
    pub fn variation_glyph(&self, u: u32, vs: u32) -> Option<u32> {
        let g = self.face.borrow().char_variant_index(u, vs);
        (g != 0).then_some(g)
    }

    /// `hb_ft_get_glyph_h_advances`: `FT_Get_Advance` no caminho rápido, 16.16 para 26.6.
    pub fn h_advance(&self, g: u32) -> i32 {
        let f = self.face.borrow();
        // `FT_Get_Advance` recusa glifo fora da face, e o hb-ft devolve 0.
        if g >= f.num_glyphs() {
            return 0;
        }
        let adv = f.advance_unscaled(g);
        let v = zft::calc::mul_div(adv, self.face_x_scale, 64).abs();
        let x_mult: i64 = if self.x_scale < 0 { -1 } else { 1 };
        ((v * x_mult + (1 << 9)) as i32) >> 10
    }

    /// `hb_ft_get_glyph_v_advance`: o avanço vertical do FreeType cresce para baixo, daí a
    /// negação.
    pub fn v_advance(&self, g: u32) -> i32 {
        let f = self.face.borrow();
        let Some(adv) = f.advance_vertical_unscaled(g) else { return 0 };
        let v = zft::calc::mul_div(adv, self.face_y_scale, 64);
        let y_mult: i64 = if self.y_scale < 0 { -1 } else { 1 };
        let v = (v * y_mult) as i32;
        (-v + (1 << 9)) >> 10
    }

    /// `hb_ft_get_glyph_v_origin`; glifo que não carrega dá a origem zero.
    pub fn v_origin(&self, g: u32) -> (i32, i32) {
        let mut f = self.face.borrow_mut();
        let Ok(slot) = f.load_glyph(g, LOAD_FLAGS) else { return (0, 0) };
        let x_mult: i64 = if self.x_scale < 0 { -1 } else { 1 };
        let y_mult: i64 = if self.y_scale < 0 { -1 } else { 1 };
        let x = slot.hori_bearing_x - slot.vert_bearing_x;
        let y = slot.hori_bearing_y + slot.vert_bearing_y;
        ((x * x_mult) as i32, (y * y_mult) as i32)
    }

    /// `hb_ft_get_glyph_contour_point`.
    pub fn contour_point(&self, g: u32, point: u32) -> Option<(i32, i32)> {
        let mut f = self.face.borrow_mut();
        let slot = f.load_glyph(g, LOAD_FLAGS).ok()?;
        let p = slot.outline.points.get(point as usize)?;
        Some((p.x as i32, p.y as i32))
    }

    /// `hb_ft_get_glyph_extents`.
    pub fn glyph_extents(&self, g: u32) -> Option<GlyphExtents> {
        let mut f = self.face.borrow_mut();
        let slot = f.load_glyph(g, LOAD_FLAGS).ok()?;
        let x_mult: i64 = if self.x_scale < 0 { -1 } else { 1 };
        let y_mult: i64 = if self.y_scale < 0 { -1 } else { 1 };
        Some(GlyphExtents {
            x_bearing: (slot.hori_bearing_x * x_mult) as i32,
            y_bearing: (slot.hori_bearing_y * y_mult) as i32,
            width: (slot.width * x_mult) as i32,
            height: (-slot.height * y_mult) as i32,
        })
    }

    /// `hb_ft_get_glyph_h_kerning`.
    pub fn h_kerning(&self, left: u32, right: u32) -> i32 {
        let f = self.face.borrow();
        if self.x_ppem != 0 { f.kerning(left, right).0 as i32 } else { f.kerning_unfitted(left, right).0 as i32 }
    }
}
