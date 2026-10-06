//! Fontes TrueType do FreeType 2.13.3 (o `libfreetype6` do Debian 13), traduzidas sem `unsafe`,
//! para o `PIL._imagingft` do pseudo-linus.
//!
//! Cobre o caminho que o Pillow usa: `FT_New_Memory_Face`, `FT_Request_Size` nominal,
//! `FT_Load_Glyph` com o autohinter (fontes sem bytecode) e o rasterizador `smooth`.

mod autofit;
pub mod calc;
mod glyf;
pub mod outline;
pub mod raster;
mod sfnt;

use calc::{div_fix, mul_div, mul_fix, pix_ceil, pix_floor, pix_round};
pub use glyf::Loaded;
use outline::Outline;

/// Erros do `fterrdef.h` que o caminho portado pode produzir.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    UnknownFileFormat,
    InvalidArgument,
    InvalidTable,
    TableMissing,
    HorizHeaderMissing,
    InvalidGlyphIndex,
    InvalidOutline,
    InvalidComposite,
    TooManyHints,
    InvalidPixelSize,
    InvalidPpem,
    DivideByZero,
}

impl Error {
    /// O código numérico do FreeType (`FT_Err_*`).
    pub fn code(self) -> i32 {
        match self {
            Error::UnknownFileFormat => 0x02,
            Error::InvalidArgument => 0x06,
            Error::InvalidGlyphIndex => 0x10,
            Error::InvalidPixelSize => 0x17,
            Error::InvalidOutline => 0x14,
            Error::InvalidComposite => 0x15,
            Error::TooManyHints => 0x16,
            Error::InvalidTable => 0x08,
            Error::TableMissing => 0x8E,
            Error::HorizHeaderMissing => 0x8B,
            Error::InvalidPpem => 0x97,
            Error::DivideByZero => 0x99,
        }
    }

    /// A mensagem do `fterrdef.h`, que o Pillow põe no `OSError`.
    pub fn message(self) -> &'static str {
        match self {
            Error::UnknownFileFormat => "unknown file format",
            Error::InvalidArgument => "invalid argument",
            Error::InvalidTable => "broken table",
            Error::TableMissing => "table missing",
            Error::HorizHeaderMissing => "horizontal header (hhea) table missing",
            Error::InvalidGlyphIndex => "invalid glyph index",
            Error::InvalidOutline => "invalid outline",
            Error::InvalidComposite => "invalid composite glyph",
            Error::TooManyHints => "too many hints",
            Error::InvalidPixelSize => "invalid pixel size",
            Error::InvalidPpem => "invalid ppem value",
            Error::DivideByZero => "division by zero",
        }
    }
}

/// `FT_Size_Metrics`.
#[derive(Clone, Copy, Debug, Default)]
pub struct SizeMetrics {
    pub x_ppem: u16,
    pub y_ppem: u16,
    pub x_scale: i64,
    pub y_scale: i64,
    pub ascender: i64,
    pub descender: i64,
    pub height: i64,
    pub max_advance: i64,
}

pub const LOAD_NO_HINTING: u32 = 1 << 1;
pub const LOAD_NO_SCALE: u32 = 1 << 0;
pub const LOAD_FORCE_AUTOHINT: u32 = 1 << 5;
pub const LOAD_TARGET_MONO: u32 = 2 << 16;

/// Uma face aberta (`FT_Face` com o seu único `FT_Size`).
pub struct Face {
    data: Vec<u8>,
    sfnt: sfnt::Sfnt,
    pub units_per_em: i64,
    pub ascender: i64,
    pub descender: i64,
    pub height: i64,
    pub max_advance_width: i64,
    pub size: SizeMetrics,
    /// Os globais do autohinter (`face->autohint.data`), criados na primeira carga.
    autohint: Option<Box<autofit::Globals>>,
}

/// `FT_GlyphSlot` depois de um `FT_Load_Glyph`.
#[derive(Clone, Debug, Default)]
pub struct Slot {
    pub outline: Outline,
    pub hori_advance: i64,
    pub hori_bearing_x: i64,
    pub hori_bearing_y: i64,
    pub width: i64,
    pub height: i64,
    pub lsb_delta: i64,
    pub rsb_delta: i64,
    pub linear_hori_advance: i64,
}

impl Slot {
    /// `FT_Render_Glyph` com `FT_RENDER_MODE_NORMAL`.
    pub fn render(&self) -> Option<raster::Bitmap> {
        raster::render(&self.outline, self.outline.overlap)
    }
}

impl Face {
    /// `FT_New_Memory_Face`.
    pub fn new(data: Vec<u8>, index: usize) -> Result<Face, Error> {
        let s = sfnt::Sfnt::parse(&data, index)?;
        // `sfnt_load_face`: ascender, descender e altura.
        let (mut asc, mut desc, mut h);
        let os2 = s.os2;
        if let Some(o) = os2.filter(|o| o.fs_selection & 128 != 0) {
            asc = i64::from(o.typo_ascender);
            desc = i64::from(o.typo_descender);
            h = asc - desc + i64::from(o.typo_line_gap);
        } else {
            asc = i64::from(s.hhea_ascender);
            desc = i64::from(s.hhea_descender);
            h = asc - desc + i64::from(s.hhea_line_gap);
            if asc == 0 && desc == 0 {
                if let Some(o) = os2 {
                    if o.typo_ascender != 0 || o.typo_descender != 0 {
                        asc = i64::from(o.typo_ascender);
                        desc = i64::from(o.typo_descender);
                        h = asc - desc + i64::from(o.typo_line_gap);
                    } else {
                        asc = i64::from(o.win_ascent as i16);
                        desc = -i64::from(o.win_descent as i16);
                        h = asc - desc;
                    }
                }
            }
        }
        Ok(Face {
            units_per_em: i64::from(s.units_per_em),
            ascender: asc,
            descender: desc,
            height: i64::from(h as i16),
            max_advance_width: i64::from(s.advance_width_max as i16),
            data,
            sfnt: s,
            size: SizeMetrics::default(),
            autohint: None,
        })
    }

    /// `FT_STYLE_FLAG_ITALIC`, como o `sfnt_load_face` o deduz do OS/2 ou do `macStyle`.
    pub fn is_italic(&self) -> bool {
        match self.sfnt.os2 {
            Some(o) => o.fs_selection & (512 | 1) != 0,
            None => self.sfnt.mac_style & 2 != 0,
        }
    }

    pub(crate) fn has_unicode_cmap(&self) -> bool {
        self.sfnt.unicode_cmap.is_some()
    }

    pub(crate) fn mapped_chars(&self) -> Vec<(u32, u32)> {
        self.sfnt.mapped_chars(&self.data)
    }

    /// `FT_Get_Advance` com `FT_LOAD_NO_SCALE`: o avanço do `hmtx`.
    pub fn advance_unscaled(&self, gid: u32) -> i64 {
        i64::from(self.sfnt.hmetrics(&self.data, gid).0)
    }

    /// O `ft_glyphslot_load` escolhe o autohinter quando a fonte não traz bytecode.
    fn wants_autohint(&self, flags: u32) -> bool {
        if flags & (LOAD_NO_HINTING | LOAD_NO_SCALE) != 0 {
            return false;
        }
        flags & LOAD_FORCE_AUTOHINT != 0
            || (self.sfnt.has(b"loca")
                && self.sfnt.max_size_of_instructions == 0
                && !self.sfnt.has(b"fpgm")
                && !self.sfnt.has(b"prep"))
    }

    pub fn num_glyphs(&self) -> u32 {
        u32::from(self.sfnt.num_glyphs)
    }

    pub fn is_fixed_width(&self) -> bool {
        self.sfnt.is_fixed_pitch
    }

    pub fn has_kerning(&self) -> bool {
        self.sfnt.has(b"kern")
    }

    /// `FT_Get_Char_Index`.
    pub fn char_index(&self, code: u32) -> u32 {
        self.sfnt.char_index(&self.data, code)
    }

    /// `FT_Get_Kerning` com `FT_KERNING_DEFAULT`.
    pub fn kerning(&self, left: u32, right: u32) -> (i64, i64) {
        let k = i64::from(self.sfnt.kerning(&self.data, left, right));
        let mut x = mul_fix(k, self.size.x_scale);
        // Modo padrão: arredonda para o pixel, com o ajuste de tamanhos pequenos.
        if self.size.x_ppem < 25 {
            x = mul_div(x, i64::from(self.size.x_ppem), 25);
        }
        (pix_round(x), 0)
    }

    /// `FT_Request_Size` com `FT_SIZE_REQUEST_TYPE_NOMINAL` e resolução 0 (72 dpi).
    pub fn request_size(&mut self, width: i64, height: i64) -> Result<(), Error> {
        let upem = self.units_per_em;
        let m = &mut self.size;
        let (mut sw, mut sh) = (width, height);
        if height != 0 || width == 0 {
            if upem == 0 {
                return Err(Error::DivideByZero);
            }
            m.y_scale = div_fix(sh, upem);
        }
        if width != 0 {
            m.x_scale = div_fix(sw, upem);
        } else {
            m.x_scale = m.y_scale;
            sw = mul_div(sh, upem, upem);
        }
        if height == 0 {
            m.y_scale = m.x_scale;
            sh = mul_div(sw, upem, upem);
        }
        let sw = (sw + 32) >> 6;
        let sh = (sh + 32) >> 6;
        if sw > 0xFFFF || sh > 0xFFFF {
            return Err(Error::InvalidPixelSize);
        }
        m.x_ppem = sw as u16;
        m.y_ppem = sh as u16;
        // `ft_recompute_scaled_metrics` com `GRID_FIT_METRICS`.
        m.ascender = pix_ceil(mul_fix(self.ascender, m.y_scale));
        m.descender = pix_floor(mul_fix(self.descender, m.y_scale));
        m.height = pix_round(mul_fix(self.height, m.y_scale));
        m.max_advance = pix_round(mul_fix(self.max_advance_width, m.x_scale));
        // `tt_size_reset`: ppem que arredonda para zero é erro.
        if m.x_ppem < 1 || m.y_ppem < 1 {
            return Err(Error::InvalidPpem);
        }
        Ok(())
    }

    /// Escalas que o driver TrueType usa (`tt_size_reset`): com o bit 3 do `head`, baseadas no
    /// ppem inteiro.
    fn tt_scales(&self) -> (i64, i64) {
        if self.sfnt.head_flags & 8 != 0 {
            (
                div_fix(i64::from(self.size.x_ppem) << 6, self.units_per_em),
                div_fix(i64::from(self.size.y_ppem) << 6, self.units_per_em),
            )
        } else {
            (self.size.x_scale, self.size.y_scale)
        }
    }

    /// O glifo em unidades da fonte (`FT_LOAD_NO_SCALE`), como o autohinter o lê.
    pub fn load_unscaled(&self, gid: u32) -> Result<Loaded, Error> {
        glyf::load(&self.sfnt, &self.data, gid, None)
    }

    /// `FT_Load_Glyph`.
    pub fn load_glyph(&mut self, gid: u32, flags: u32) -> Result<Slot, Error> {
        if self.wants_autohint(flags) {
            return self.autohint_glyph(gid, flags);
        }
        // `tt_glyph_load`: sem hinting, o driver usa as métricas da camada base (escala exata).
        let scale = if flags & LOAD_NO_SCALE != 0 {
            None
        } else if flags & LOAD_NO_HINTING != 0 {
            Some((self.size.x_scale, self.size.y_scale))
        } else {
            Some(self.tt_scales())
        };
        let l = glyf::load(&self.sfnt, &self.data, gid, scale)?;
        let linear = if scale.is_some() { mul_div(l.linear, self.size.x_scale, 64) } else { l.linear };
        Ok(Slot {
            outline: l.outline,
            hori_advance: l.advance,
            hori_bearing_x: l.hori_bearing_x,
            hori_bearing_y: l.hori_bearing_y,
            width: l.width,
            height: l.height,
            lsb_delta: 0,
            rsb_delta: 0,
            linear_hori_advance: linear,
        })
    }
}
