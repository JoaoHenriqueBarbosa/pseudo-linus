//! O autohinter (`autofit`) do FreeType 2.13.3, sem HarfBuzz, como no `libfreetype6` do Debian:
//! `afglobal.c` (cobertura de estilos), `afloader.c` (o laço de carga) e o sistema de escrita
//! latino. Os glifos sem estilo caem no `hani_dflt` (o Debian liga `AF_CONFIG_OPTION_CJK`).

mod cjk;
mod hints;
mod latin;
#[allow(dead_code)]
mod tables;

use crate::calc::{mul_fix, pix_ceil, pix_floor, pix_round};
use crate::outline::Outline;
use crate::{Error, Face};
use cjk::CjkMetrics;
use hints::GlyphHints;
use latin::LatinMetrics;
use tables::{BLUE_STRINGSETS, COVERAGE_DEFAULT, SCRIPTS, STYLES, WS_CJK, WS_INDIC, WS_LATIN};

pub(crate) struct ScriptClass {
    #[allow(dead_code)]
    pub name: &'static str,
    pub ranges: &'static [(u32, u32)],
    pub nonbase: &'static [(u32, u32)],
    pub top_to_bottom: bool,
    pub standard_chars: &'static str,
}

pub(crate) struct StyleClass {
    #[allow(dead_code)]
    pub name: &'static str,
    pub writing_system: u32,
    pub script: usize,
    pub blue_stringset: usize,
    pub coverage: u32,
}

pub(crate) struct BlueString {
    pub text: &'static str,
    pub properties: u32,
}

const STYLE_MASK: u16 = 0x3FFF;
const STYLE_UNASSIGNED: u16 = STYLE_MASK;
const DIGIT: u16 = 0x8000;
const NONBASE: u16 = 0x4000;
/// `AF_STYLE_FALLBACK` com `AF_CONFIG_OPTION_CJK`.
const STYLE_FALLBACK: u16 = 86;

/// `FT_LOAD_TARGET_MODE`: o modo de renderização embutido nas flags de carga (o `NORMAL` é 0).
pub(crate) const RENDER_MODE_LIGHT: u32 = 1;
pub(crate) const RENDER_MODE_MONO: u32 = 2;
pub(crate) const RENDER_MODE_LCD: u32 = 3;
pub(crate) const RENDER_MODE_LCD_V: u32 = 4;

/// `AF_ScalerRec`.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Scaler {
    pub x_scale: i64,
    pub y_scale: i64,
    pub x_delta: i64,
    pub y_delta: i64,
    pub render_mode: u32,
    pub flags: u32,
}

/// As métricas de um estilo, por sistema de escrita.
pub(crate) enum StyleMetrics {
    Dummy { scaler: Scaler },
    Latin(Box<LatinMetrics>),
    Cjk(Box<CjkMetrics>),
}

/// `AF_FaceGlobalsRec`.
pub(crate) struct Globals {
    pub glyph_styles: Vec<u16>,
    metrics: Vec<Option<StyleMetrics>>,
    pub increase_x_height: u32,
}

/// Lê um caractere UTF-8 de `s` a partir de `*p` (`GET_UTF8_CHAR`).
fn next_utf8(s: &[u8], p: &mut usize) -> u32 {
    let b = s[*p];
    *p += 1;
    let (mut ch, n) = if b < 0x80 {
        (u32::from(b), 0)
    } else if b < 0xE0 {
        (u32::from(b & 0x1F), 1)
    } else if b < 0xF0 {
        (u32::from(b & 0x0F), 2)
    } else {
        (u32::from(b & 0x07), 3)
    };
    for _ in 0..n {
        ch = (ch << 6) | u32::from(s.get(*p).copied().unwrap_or(0) & 0x3F);
        *p += 1;
    }
    ch
}

/// `af_shaper_get_cluster` sem HarfBuzz: um token de um caractere vira um glifo; tokens mais
/// longos não contam. Devolve `(glifo, contagem)`.
pub(crate) fn get_cluster(face: &Face, s: &[u8], p: &mut usize) -> (u32, u32) {
    while *p < s.len() && s[*p] == b' ' {
        *p += 1;
    }
    let ch = next_utf8(s, p);
    let mut dummy = 0;
    while *p < s.len() && s[*p] != b' ' {
        dummy = next_utf8(s, p);
    }
    if dummy != 0 {
        (0, 0)
    } else {
        (face.char_index(ch), 1)
    }
}

impl Globals {
    /// `af_face_globals_new` e `af_face_globals_compute_style_coverage`.
    pub(crate) fn new(face: &Face) -> Globals {
        let count = face.num_glyphs() as usize;
        let mut gstyles = vec![STYLE_UNASSIGNED; count];
        let chars = face.mapped_chars();
        if face.has_unicode_cmap() {
            // `FT_Get_Char_Index` no início da faixa e `FT_Get_Next_Char` dali em diante.
            let in_range = |first: u32, last: u32| {
                let start = chars.partition_point(|&(c, _)| c < first);
                chars[start..].iter().take_while(move |&&(c, _)| c <= last).map(|&(_, g)| g as usize)
            };
            for (ss, style) in STYLES.iter().enumerate() {
                if style.coverage != COVERAGE_DEFAULT {
                    continue;
                }
                let script = &SCRIPTS[style.script];
                for &(first, last) in script.ranges {
                    for g in in_range(first, last) {
                        if g < count && gstyles[g] & STYLE_MASK == STYLE_UNASSIGNED {
                            gstyles[g] = ss as u16;
                        }
                    }
                }
                for &(first, last) in script.nonbase {
                    for g in in_range(first, last) {
                        if g < count && gstyles[g] & STYLE_MASK == ss as u16 {
                            gstyles[g] |= NONBASE;
                        }
                    }
                }
            }
            for c in 0x30..=0x39 {
                let g = face.char_index(c) as usize;
                if g != 0 && g < count {
                    gstyles[g] |= DIGIT;
                }
            }
        }
        for s in gstyles.iter_mut() {
            if *s & STYLE_MASK == STYLE_UNASSIGNED {
                *s = (*s & !STYLE_MASK) | STYLE_FALLBACK;
            }
        }
        Globals { glyph_styles: gstyles, metrics: (0..STYLES.len()).map(|_| None).collect(), increase_x_height: 0 }
    }

    fn is_digit(&self, gid: u32) -> bool {
        self.glyph_styles.get(gid as usize).is_some_and(|s| s & DIGIT != 0)
    }

    /// `af_face_globals_get_metrics`: o estilo do glifo, com as métricas criadas na primeira vez.
    fn get_metrics(&mut self, face: &Face, gid: u32) -> Result<usize, Error> {
        if gid as usize >= self.glyph_styles.len() {
            return Err(Error::InvalidArgument);
        }
        loop {
            let style = usize::from(self.glyph_styles[gid as usize] & STYLE_UNASSIGNED);
            if self.metrics[style].is_some() {
                return Ok(style);
            }
            let ws = STYLES[style].writing_system;
            let m = if ws == WS_LATIN {
                match LatinMetrics::new(face, style, &mut self.glyph_styles) {
                    Some(m) => StyleMetrics::Latin(Box::new(m)),
                    // Sem zonas azuis: os glifos do estilo foram para `none_dflt`; tenta de novo.
                    None => continue,
                }
            } else if ws == WS_CJK || ws == WS_INDIC {
                StyleMetrics::Cjk(Box::new(CjkMetrics::new(face, style, ws == WS_CJK)))
            } else {
                StyleMetrics::Dummy { scaler: Scaler::default() }
            };
            self.metrics[style] = Some(m);
            return Ok(style);
        }
    }
}

impl Face {
    /// `af_loader_load_glyph`: carrega o glifo pelo autohinter. `flags` é o `load_flags` do
    /// `FT_Load_Glyph`.
    pub(crate) fn autohint_glyph(&mut self, gid: u32, flags: u32) -> Result<crate::Slot, Error> {
        let mut globals = match self.autohint.take() {
            Some(g) => g,
            None => Box::new(Globals::new(self)),
        };
        let r = self.autohint_with(&mut globals, gid, flags);
        self.autohint = Some(globals);
        r
    }

    fn autohint_with(&self, globals: &mut Globals, gid: u32, flags: u32) -> Result<crate::Slot, Error> {
        let mode = (flags >> 16) & 15;
        let scaler = Scaler {
            x_scale: self.size.x_scale,
            y_scale: self.size.y_scale,
            x_delta: 0,
            y_delta: 0,
            render_mode: mode,
            flags: 0,
        };
        let style = globals.get_metrics(self, gid)?;
        let increase = globals.increase_x_height;
        let mut hints = GlyphHints::default();
        let nonbase = globals.glyph_styles[gid as usize] & NONBASE != 0;
        let metrics = globals.metrics[style].as_mut().ok_or(Error::InvalidArgument)?;
        let metrics_scaler = match metrics {
            StyleMetrics::Dummy { scaler: s } => {
                *s = scaler;
                hints.x_scale = s.x_scale;
                hints.y_scale = s.y_scale;
                hints.x_delta = s.x_delta;
                hints.y_delta = s.y_delta;
                *s
            }
            StyleMetrics::Latin(m) => {
                m.scale(&scaler, self.size.x_ppem, increase);
                m.hints_init(&mut hints, self.is_italic());
                m.scaler
            }
            StyleMetrics::Cjk(m) => {
                m.scale(&scaler);
                m.hints_init(&mut hints);
                m.scaler
            }
        };

        let l = self.load_unscaled(gid)?;
        let mut outline: Outline = l.outline;
        let (mut pp1x, mut pp2x) = (hints.x_delta, mul_fix(l.advance, hints.x_scale) + hints.x_delta);
        let (mut lsb_delta, mut rsb_delta) = (0, 0);
        if !outline.points.is_empty() {
            match metrics {
                StyleMetrics::Dummy { .. } => {
                    hints.reload(&outline, self.units_per_em);
                    hints.save(&mut outline);
                }
                StyleMetrics::Latin(m) => m.apply(&mut hints, &mut outline, nonbase, self.units_per_em),
                StyleMetrics::Cjk(m) => m.apply(&mut hints, &mut outline),
            }
            let axis = &hints.axis[0];
            let do_advance = hints.scaler_flags & hints::SCALER_FLAG_NO_ADVANCE == 0;
            if mode != RENDER_MODE_LIGHT && axis.edges.len() > 1 && do_advance {
                let edge1 = &axis.edges[0];
                let edge2 = &axis.edges[axis.edges.len() - 1];
                let old_rsb = pp2x - edge2.opos;
                let old_lsb = edge1.opos;
                let new_lsb = edge1.pos;
                let mut pp1x_uh = new_lsb - old_lsb;
                let mut pp2x_uh = edge2.pos + old_rsb;
                if old_lsb < 24 {
                    pp1x_uh -= 8;
                }
                if old_rsb < 24 {
                    pp2x_uh += 8;
                }
                pp1x = pix_round(pp1x_uh);
                pp2x = pix_round(pp2x_uh);
                if pp1x >= new_lsb && old_lsb > 0 {
                    pp1x -= 64;
                }
                if pp2x <= edge2.pos && old_rsb > 0 {
                    pp2x += 64;
                }
                lsb_delta = pp1x - pp1x_uh;
                rsb_delta = pp2x - pp2x_uh;
            } else {
                let (a, b) = (pp1x, pp2x);
                pp1x = pix_round(a);
                pp2x = pix_round(b);
                lsb_delta = pp1x - a;
                rsb_delta = pp2x - b;
            }
        }

        // Hint_Metrics.
        if pp1x != 0 {
            outline.translate(-pp1x, 0);
        }
        let b = outline.cbox();
        let (x_min, y_min, x_max, y_max) = (pix_floor(b.x_min), pix_floor(b.y_min), pix_ceil(b.x_max), pix_ceil(b.y_max));
        let digits_same = match metrics {
            StyleMetrics::Latin(m) => m.digits_have_same_width,
            StyleMetrics::Cjk(m) => m.digits_have_same_width,
            StyleMetrics::Dummy { .. } => false,
        };
        let mut advance = l.advance;
        if mode != RENDER_MODE_LIGHT && (self.is_fixed_width() || (globals.is_digit(gid) && digits_same)) {
            advance = mul_fix(advance, metrics_scaler.x_scale);
            lsb_delta = 0;
            rsb_delta = 0;
        } else if advance != 0 {
            advance = pp2x - pp1x;
        }
        Ok(crate::Slot {
            outline,
            hori_advance: pix_round(advance),
            hori_bearing_x: x_min,
            hori_bearing_y: y_max,
            width: x_max - x_min,
            height: y_max - y_min,
            lsb_delta,
            rsb_delta,
            linear_hori_advance: crate::calc::mul_div(l.linear, self.size.x_scale, 64),
        })
    }
}

pub(crate) fn blue_stringset(start: usize) -> impl Iterator<Item = &'static BlueString> {
    BLUE_STRINGSETS[start..].iter().take_while(|b| !b.text.is_empty())
}
