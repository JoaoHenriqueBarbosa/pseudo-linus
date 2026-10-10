//! Texto do canvas 2D: a forma (fonte escolhida e glifos posicionados) pelo mesmo contexto de fontes do documento,
//! e as medidas do `measureText` como o Blink as calcula, conferidas contra o oráculo.

use std::cell::RefCell;

use parley::{FontContext, FontFamily, FontStyle, FontWeight, Layout, LayoutContext, PositionedLayoutItem, StyleProperty};
use skrifa::instance::{LocationRef, Size};
use skrifa::metrics::GlyphMetrics;
use skrifa::outline::{DrawSettings, Engine, HintingInstance, HintingOptions, OutlinePen, SmoothMode, Target};
use skrifa::raw::TableProvider;
use skrifa::{FontRef, GlyphId, MetadataProvider};

thread_local! {
    /// O contexto de fontes do canvas, montado uma vez por processo (registrar as fontes da VFS é caro).
    static CONTEXTS: RefCell<Option<(FontContext, LayoutContext<()>)>> = const { RefCell::new(None) };
}

/// A fonte de um texto do canvas, já interpretada pelo prelúdio (a lista de famílias em sintaxe CSS).
pub struct CanvasFont<'a> {
    pub family: &'a str,
    pub weight: f32,
    pub style: &'a str,
    pub size: f32,
}

/// Um glifo posicionado: os bytes e o índice da fonte, o id, a origem na linha de base e o tamanho.
pub struct PlacedGlyph {
    pub font: parley::FontData,
    pub id: u32,
    pub x: f32,
    pub size: f32,
}

/// A forma de um texto: os glifos, a largura (soma dos avanços), o ascendente e descendente da fonte primária e a
/// caixa em do OS/2 (o `sTypoAscender` e o `sTypoDescender` normalizados para o tamanho).
pub struct Shaped {
    pub glyphs: Vec<PlacedGlyph>,
    pub width: f32,
    pub ascent: f32,
    pub descent: f32,
    pub em_ascent: f64,
    pub em_descent: f64,
}

/// A caixa em de uma fonte no tamanho pedido, em 1/64 de pixel como o LayoutUnit do Blink.
fn em_box(data: &parley::FontData, size: f32) -> (f64, f64) {
    let Some(os2) = FontRef::from_index(data.data.as_ref(), data.index).ok().and_then(|f| f.os2().ok()) else { return (0.0, 0.0) };
    let (ascent, descent) = (f64::from(os2.s_typo_ascender()), f64::from(os2.s_typo_descender()).abs());
    if ascent + descent == 0.0 {
        return (0.0, 0.0);
    }
    let snap = |v: f64| (v * 64.0).round() / 64.0;
    let size = f64::from(size);
    (snap(size * ascent / (ascent + descent)), snap(size * descent / (ascent + descent)))
}

/// Quanto a linha de base alfabética desce em relação ao `y` pedido, para cada `textBaseline` (medido no oráculo):
/// `top` e `bottom` são a caixa em, `middle` o meio dela, `hanging` 80% do ascendente arredondado (em f32) e
/// `ideographic` o descendente arredondado.
pub fn baseline_shift(shaped: &Shaped, baseline: &str) -> f64 {
    let (ascent, descent) = (f64::from(shaped.ascent.round()), f64::from(shaped.descent.round()));
    match baseline {
        "top" => shaped.em_ascent,
        "hanging" => f64::from((ascent * 0.8) as f32),
        "middle" => (shaped.em_ascent - shaped.em_descent) / 2.0,
        "ideographic" => -descent,
        "bottom" => -shaped.em_descent,
        _ => 0.0,
    }
}

/// Escolhe as fontes e posiciona os glifos de `text` numa linha só, como o Blink faz para o canvas.
pub fn shape(text: &str, font: &CanvasFont<'_>) -> Shaped {
    let shaped = shape_line(text, font);
    if !text.is_empty() {
        return shaped;
    }
    // Texto vazio: as métricas vêm da fonte primária mesmo sem glifo; um espaço a escolhe.
    let blank = shape_line(" ", font);
    Shaped { ascent: blank.ascent, descent: blank.descent, em_ascent: blank.em_ascent, em_descent: blank.em_descent, ..shaped }
}

fn shape_line(text: &str, font: &CanvasFont<'_>) -> Shaped {
    CONTEXTS.with(|cell| {
        let mut cell = cell.borrow_mut();
        let (fcx, lcx) = cell.get_or_insert_with(|| (crate::fonts::context(), LayoutContext::new()));
        let mut builder = lcx.ranged_builder(fcx, text, 1.0, false);
        builder.push_default(StyleProperty::FontFamily(FontFamily::from(font.family)));
        builder.push_default(StyleProperty::FontSize(font.size));
        builder.push_default(StyleProperty::FontWeight(FontWeight::new(font.weight)));
        let style = match font.style {
            "italic" => FontStyle::Italic,
            "oblique" => FontStyle::Oblique(None),
            _ => FontStyle::Normal,
        };
        builder.push_default(StyleProperty::FontStyle(style));
        let mut layout: Layout<()> = builder.build(text);
        layout.break_all_lines(None);
        let mut shaped = Shaped { glyphs: Vec::new(), width: 0.0, ascent: 0.0, descent: 0.0, em_ascent: 0.0, em_descent: 0.0 };
        let mut primary = true;
        for line in layout.lines() {
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(run) = item else { continue };
                let (data, size) = (run.run().font().clone(), run.run().font_size());
                if primary {
                    let metrics = run.run().metrics();
                    (shaped.ascent, shaped.descent) = (metrics.ascent, metrics.descent);
                    (shaped.em_ascent, shaped.em_descent) = em_box(&data, size);
                    primary = false;
                }
                for glyph in run.glyphs() {
                    shaped.glyphs.push(PlacedGlyph { font: data.clone(), id: glyph.id, x: shaped.width + glyph.x, size });
                    shaped.width += glyph.advance;
                }
            }
        }
        shaped
    })
}

/// Acumula a caixa de controle de um contorno (o `FT_Outline_Get_CBox` que o Skia usa nas métricas do glifo).
#[derive(Default)]
struct ControlBox(Option<[f32; 4]>);

impl ControlBox {
    fn add(&mut self, x: f32, y: f32) {
        let b = self.0.get_or_insert([x, x, y, y]);
        *b = [b[0].min(x), b[1].max(x), b[2].min(y), b[3].max(y)];
    }
}

impl OutlinePen for ControlBox {
    fn move_to(&mut self, x: f32, y: f32) {
        self.add(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.add(x, y);
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.add(cx0, cy0);
        self.add(x, y);
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.add(cx0, cy0);
        self.add(cx1, cy1);
        self.add(x, y);
    }
    fn close(&mut self) {}
}

/// O hinting do Chromium do Debian: o fontconfig pede `hintslight`, que o FreeType atende com o autohinter leve.
fn hinting(font: &FontRef<'_>, size: f32) -> Option<HintingInstance> {
    let options = HintingOptions { engine: Engine::Auto(None), target: Target::Smooth { mode: SmoothMode::Light, symmetric_rendering: true, preserve_linear_metrics: true } };
    HintingInstance::new(&font.outline_glyphs(), Size::new(size), LocationRef::default(), options).ok()
}

/// Desenha o contorno de um glifo (em pixels, y para cima) na caneta. Com `scale`, o glifo sai com o hinting do
/// tamanho no dispositivo (`size * scale`) e a caneta recebe as coordenadas do dispositivo; sem, o contorno puro.
pub fn draw_glyph(glyph: &PlacedGlyph, scale: Option<f32>, pen: &mut impl OutlinePen) -> Option<()> {
    let font = FontRef::from_index(glyph.font.data.as_ref(), glyph.font.index).ok()?;
    let outline = font.outline_glyphs().get(GlyphId::new(glyph.id))?;
    match scale {
        Some(scale) => {
            let instance = hinting(&font, glyph.size * scale)?;
            outline.draw(DrawSettings::hinted(&instance, false), pen).ok()?;
        }
        None => {
            outline.draw(DrawSettings::unhinted(Size::new(glyph.size), LocationRef::default()), pen).ok()?;
        }
    }
    Some(())
}

/// A caixa de tinta de um glifo em pixels (y para cima): a horizontal do contorno sem hinting (o hinting leve não
/// mexe no eixo x), a vertical do contorno com hinting; arredondadas para fora como o Skia entrega ao Blink.
fn ink_box(glyph: &PlacedGlyph) -> Option<[f32; 4]> {
    let font = FontRef::from_index(glyph.font.data.as_ref(), glyph.font.index).ok()?;
    let bounds = GlyphMetrics::new(&font, Size::new(glyph.size), LocationRef::default()).bounds(GlyphId::new(glyph.id))?;
    let outline = font.outline_glyphs().get(GlyphId::new(glyph.id))?;
    let mut hinted = ControlBox::default();
    let instance = hinting(&font, glyph.size)?;
    outline.draw(DrawSettings::hinted(&instance, false), &mut hinted).ok()?;
    let [_, _, y0, y1] = hinted.0?;
    Some([glyph.x + bounds.x_min.floor(), glyph.x + bounds.x_max.ceil(), y0.floor(), y1.ceil()])
}

/// O `measureText`: [largura, actualBoundingBoxLeft, actualBoundingBoxRight, ascendente e descendente de tinta,
/// ascendente e descendente da fonte, deslocamento da linha de base de `baseline`], relativos à linha de base
/// alfabética e ao início do texto.
pub fn measure(text: &str, font: &CanvasFont<'_>, baseline: &str) -> [f64; 8] {
    let shaped = shape(text, font);
    let (mut left, mut right, mut ascent, mut descent) = (0f32, 0f32, 0f32, 0f32);
    let mut first = true;
    for glyph in &shaped.glyphs {
        let Some([x0, x1, y0, y1]) = ink_box(glyph) else { continue };
        if first {
            (left, right, ascent, descent) = (-x0, x1, y1, -y0);
            first = false;
        } else {
            (left, right, ascent, descent) = (left.max(-x0), right.max(x1), ascent.max(y1), descent.max(-y0));
        }
    }
    let shift = baseline_shift(&shaped, baseline);
    [f64::from(shaped.width), f64::from(left), f64::from(right), f64::from(ascent), f64::from(descent), f64::from(shaped.ascent.round()), f64::from(shaped.descent.round()), shift].map(|v| v + 0.0)
}
