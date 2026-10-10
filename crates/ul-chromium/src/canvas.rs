//! A superfície de um canvas 2D: os comandos de desenho do prelúdio viram uma cena do vello_cpu (caminhos, texto e
//! imagens com antisserrilhado, gradientes, padrões, composição, recorte e sombra), rasterizada só quando a página lê
//! os pixels. O bitmap guardado é RGBA premultiplicado, como o N32 do Skia; a leitura desfaz a premultiplicação com
//! o arredondamento do Skia.

use std::f64::consts::{PI, TAU};
use std::sync::Arc;

use skrifa::outline::OutlinePen;
use vello_cpu::color::{AlphaColor, PremulRgba8, Srgb};
use vello_cpu::filter_effects::{EdgeMode, Filter, FilterPrimitive};
use vello_cpu::kurbo::{self, Affine, BezPath, Cap, Join, ParamCurveNearest, Point, Rect, Shape, Stroke, StrokeOpts, SvgArc, Vec2};
use vello_cpu::peniko::{BlendMode, Compose, Extend, Fill, Gradient, ImageAlphaType, ImageBrush, ImageQuality, ImageSampler, InterpolationAlphaSpace, Mix};
use vello_cpu::{ImageSource, Mask, PaintType, PixelMetadata, Pixmap, RenderContext, Resources};
use zjsc::runtime::dom_bridge::DomArg;

use crate::canvas_text::{baseline_shift, draw_glyph, shape, CanvasFont};

/// O maior lado que o rasterizador aceita; canvas maior fica sem bitmap (o Chromium também recusa a superfície).
const MAX_SIDE: u32 = u16::MAX as u32;

/// A distância que conta como infinita no recorte de padrões que não se repetem num eixo.
const FAR: f64 = 1e7;

/// O erro máximo, em pixels do dispositivo, ao achatar curvas em retas.
const DEVICE_TOLERANCE: f64 = 0.02;

/// O caminho achatado em retas com `DEVICE_TOLERANCE`.
fn flat(path: BezPath) -> BezPath {
    let mut out = BezPath::new();
    kurbo::flatten(path, DEVICE_TOLERANCE, |el| out.push(el));
    out
}

pub struct Surface {
    width: u32,
    height: u32,
    /// O bitmap já rasterizado.
    pixels: Pixmap,
    /// A cena com o que foi desenhado depois do último rasterizar; começa pelo bitmap.
    scene: Option<RenderContext>,
}

fn num(args: &[DomArg], i: usize) -> f64 {
    match args.get(i) {
        Some(DomArg::Num(n)) => *n,
        Some(DomArg::Bool(b)) => f64::from(u8::from(*b)),
        _ => 0.0,
    }
}

fn text(args: &[DomArg], i: usize) -> &str {
    match args.get(i) {
        Some(DomArg::Str(s)) => s,
        _ => "",
    }
}

fn list(args: &[DomArg], i: usize) -> &[DomArg] {
    match args.get(i) {
        Some(DomArg::List(l)) => l,
        _ => &[],
    }
}

fn affine(args: &[DomArg]) -> Affine {
    if args.len() < 6 {
        return Affine::IDENTITY;
    }
    Affine::new([num(args, 0), num(args, 1), num(args, 2), num(args, 3), num(args, 4), num(args, 5)])
}

/// A cor do prelúdio ([r, g, b, a] com a em 0..1) com a opacidade global, quantizada em 8 bits como no Skia.
fn color(args: &[DomArg], alpha: f64) -> AlphaColor<Srgb> {
    let a = (num(args, 3) * alpha * 255.0).round().clamp(0.0, 255.0) as u8;
    AlphaColor::from_rgba8(num(args, 0) as u8, num(args, 1) as u8, num(args, 2) as u8, a)
}

/// Monta um caminho em coordenadas do dispositivo a partir das operações do prelúdio. `X` troca a transformação
/// corrente (o caminho do contexto guarda a de cada chamada, como manda a especificação); Path2D chega com a
/// transformação do momento do desenho.
fn build(ops: &[DomArg], mut t: Affine, out: &mut BezPath) {
    let mut current: Option<Point> = None;
    let mut start: Option<Point> = None;
    let mut reopen = false;
    let to = |t: Affine, x: f64, y: f64| t * Point::new(x, y);
    for op in ops {
        let DomArg::List(op) = op else { continue };
        let kind = text(op, 0);
        if reopen && !matches!(kind, "M" | "X" | "Z" | "R" | "P") {
            if let Some(p) = start {
                out.move_to(p);
            }
            reopen = false;
        }
        match kind {
            "X" => t = affine(&op[1..]),
            "M" => {
                let p = to(t, num(op, 1), num(op, 2));
                out.move_to(p);
                (current, start, reopen) = (Some(p), Some(p), false);
            }
            "L" => {
                let p = to(t, num(op, 1), num(op, 2));
                out.line_to(p);
                current = Some(p);
            }
            "Q" => {
                let p = to(t, num(op, 3), num(op, 4));
                out.quad_to(to(t, num(op, 1), num(op, 2)), p);
                current = Some(p);
            }
            "C" => {
                let p = to(t, num(op, 5), num(op, 6));
                out.curve_to(to(t, num(op, 1), num(op, 2)), to(t, num(op, 3), num(op, 4)), p);
                current = Some(p);
            }
            "Z" => {
                if current.is_some() {
                    out.close_path();
                    current = start;
                    reopen = true;
                }
            }
            "A" => {
                let (center, radii, rotation) = (Point::new(num(op, 1), num(op, 2)), Vec2::new(num(op, 3), num(op, 4)), num(op, 5));
                let sweep = arc_sweep(num(op, 6), num(op, 7), num(op, 8) != 0.0);
                let arc = kurbo::Arc { center, radii, start_angle: num(op, 6), sweep_angle: sweep, x_rotation: rotation };
                append_arc(&arc, t, out);
                current = Some(t * arc_point(&arc, num(op, 6) + sweep));
            }
            "T" => {
                let inverse = t.inverse();
                let p0 = inverse * current.unwrap_or_default();
                let (p1, p2, r) = (Point::new(num(op, 1), num(op, 2)), Point::new(num(op, 3), num(op, 4)), num(op, 5));
                current = Some(arc_to(p0, p1, p2, r, t, out));
            }
            "SA" => {
                let from = Point::new(num(op, 1), num(op, 2));
                let svg = SvgArc { from, to: Point::new(num(op, 8), num(op, 9)), radii: Vec2::new(num(op, 3).abs(), num(op, 4).abs()), x_rotation: num(op, 5).to_radians(), large_arc: num(op, 6) != 0.0, sweep: num(op, 7) != 0.0 };
                match kurbo::Arc::from_svg_arc(&svg) {
                    Some(arc) => append_arc(&arc, t, out),
                    None => out.line_to(t * svg.to),
                }
                current = Some(t * svg.to);
            }
            "R" => {
                round_rect(op, t, out);
                let p = to(t, num(op, 1), num(op, 2));
                out.move_to(p);
                (current, start, reopen) = (Some(p), Some(p), false);
            }
            "P" => {
                let m = list(op, 2);
                let inner = if m.is_empty() { Affine::IDENTITY } else { affine(m) };
                build(list(op, 1), t * inner, out);
            }
            _ => {}
        }
    }
}

/// O ângulo varrido de `arc`/`ellipse`, normalizado como no Skia: uma volta inteira no máximo.
fn arc_sweep(start: f64, end: f64, ccw: bool) -> f64 {
    let sweep = end - start;
    if !ccw {
        if sweep >= TAU { TAU } else { sweep.rem_euclid(TAU) }
    } else if sweep <= -TAU {
        -TAU
    } else {
        let s = sweep.rem_euclid(TAU);
        if s > 0.0 { s - TAU } else { s }
    }
}

fn arc_point(arc: &kurbo::Arc, angle: f64) -> Point {
    let (s, c) = angle.sin_cos();
    let (rs, rc) = arc.x_rotation.sin_cos();
    let (x, y) = (arc.radii.x * c, arc.radii.y * s);
    Point::new(arc.center.x + x * rc - y * rs, arc.center.y + x * rs + y * rc)
}

fn append_arc(arc: &kurbo::Arc, t: Affine, out: &mut BezPath) {
    for el in arc.append_iter(0.01) {
        out.push(t * el);
    }
}

/// O `arcTo` da especificação: reta até o ponto de tangência e o arco até o outro, ou só a reta nos casos degenerados.
fn arc_to(p0: Point, p1: Point, p2: Point, r: f64, t: Affine, out: &mut BezPath) -> Point {
    let (v1, v2) = (p0 - p1, p2 - p1);
    let cross = v1.x * v2.y - v1.y * v2.x;
    if p0 == p1 || p1 == p2 || r == 0.0 || cross.abs() < 1e-12 {
        out.line_to(t * p1);
        return t * p1;
    }
    let (n1, n2) = (v1.normalize(), v2.normalize());
    let angle = n1.dot(n2).clamp(-1.0, 1.0).acos();
    let distance = r / (angle / 2.0).tan();
    let (t1, t2) = (p1 + n1 * distance, p1 + n2 * distance);
    let center = p1 + (n1 + n2).normalize() * (r / (angle / 2.0).sin());
    let a1 = (t1 - center).atan2();
    let mut sweep = (t2 - center).atan2() - a1;
    if sweep > PI {
        sweep -= TAU;
    } else if sweep < -PI {
        sweep += TAU;
    }
    out.line_to(t * t1);
    append_arc(&kurbo::Arc { center, radii: Vec2::new(r, r), start_angle: a1, sweep_angle: sweep, x_rotation: 0.0 }, t, out);
    t * t2
}

/// O `roundRect` da especificação: raios normalizados para os quatro cantos, reduzidos juntos se não couberem, e os
/// lados com sinal (largura ou altura negativa espelha o retângulo).
fn round_rect(op: &[DomArg], t: Affine, out: &mut BezPath) {
    let (x, y, w, h) = (num(op, 1), num(op, 2), num(op, 3), num(op, 4));
    let radii: Vec<Vec2> = list(op, 5)
        .iter()
        .map(|r| match r {
            DomArg::List(xy) => Vec2::new(num(xy, 0), num(xy, 1)),
            DomArg::Num(n) => Vec2::new(*n, *n),
            _ => Vec2::ZERO,
        })
        .collect();
    let [mut tl, mut tr, mut br, mut bl] = match radii.as_slice() {
        [a] => [*a; 4],
        [a, b] => [*a, *b, *a, *b],
        [a, b, c] => [*a, *b, *c, *b],
        [a, b, c, d, ..] => [*a, *b, *c, *d],
        [] => [Vec2::ZERO; 4],
    };
    let scale = [w.abs() / (tl.x + tr.x), w.abs() / (bl.x + br.x), h.abs() / (tl.y + bl.y), h.abs() / (tr.y + br.y)].into_iter().filter(|s| s.is_finite()).fold(1.0f64, f64::min);
    if scale < 1.0 {
        for r in [&mut tl, &mut tr, &mut br, &mut bl] {
            *r = *r * scale;
        }
    }
    if w < 0.0 {
        (tl, tr, br, bl) = (tr, tl, bl, br);
    }
    if h < 0.0 {
        (tl, tr, br, bl) = (bl, br, tr, tl);
    }
    let (x0, x1, y0, y1) = (x.min(x + w), x.max(x + w), y.min(y + h), y.max(y + h));
    let corner = |out: &mut BezPath, cx: f64, cy: f64, r: Vec2, from: f64| append_arc(&kurbo::Arc { center: Point::new(cx, cy), radii: r, start_angle: from, sweep_angle: PI / 2.0, x_rotation: 0.0 }, t, out);
    out.move_to(t * Point::new(x0 + tl.x, y0));
    out.line_to(t * Point::new(x1 - tr.x, y0));
    corner(out, x1 - tr.x, y0 + tr.y, tr, -PI / 2.0);
    out.line_to(t * Point::new(x1, y1 - br.y));
    corner(out, x1 - br.x, y1 - br.y, br, 0.0);
    out.line_to(t * Point::new(x0 + bl.x, y1));
    corner(out, x0 + bl.x, y1 - bl.y, bl, PI / 2.0);
    out.line_to(t * Point::new(x0, y0 + tl.y));
    corner(out, x0 + tl.x, y0 + tl.y, tl, PI);
    out.close_path();
}

/// Um caminho do prelúdio: [operações, transformação inicial] em coordenadas do dispositivo.
fn device_path(arg: &[DomArg]) -> BezPath {
    let mut path = BezPath::new();
    build(list(arg, 0), affine(list(arg, 1)), &mut path);
    path
}

fn fill_rule(name: &str) -> Fill {
    if name == "evenodd" { Fill::EvenOdd } else { Fill::NonZero }
}

/// `globalCompositeOperation` → modo do vello; `None` é o `source-over` comum, que dispensa camada.
fn blend(name: &str) -> Option<BlendMode> {
    let compose = |c| Some(BlendMode::new(Mix::Normal, c));
    let mix = |m| Some(BlendMode::new(m, Compose::SrcOver));
    match name {
        "source-in" => compose(Compose::SrcIn),
        "source-out" => compose(Compose::SrcOut),
        "source-atop" => compose(Compose::SrcAtop),
        "destination-over" => compose(Compose::DestOver),
        "destination-in" => compose(Compose::DestIn),
        "destination-out" => compose(Compose::DestOut),
        "destination-atop" => compose(Compose::DestAtop),
        "lighter" => compose(Compose::Plus),
        "copy" => compose(Compose::Copy),
        "xor" => compose(Compose::Xor),
        "multiply" => mix(Mix::Multiply),
        "screen" => mix(Mix::Screen),
        "overlay" => mix(Mix::Overlay),
        "darken" => mix(Mix::Darken),
        "lighten" => mix(Mix::Lighten),
        "color-dodge" => mix(Mix::ColorDodge),
        "color-burn" => mix(Mix::ColorBurn),
        "hard-light" => mix(Mix::HardLight),
        "soft-light" => mix(Mix::SoftLight),
        "difference" => mix(Mix::Difference),
        "exclusion" => mix(Mix::Exclusion),
        "hue" => mix(Mix::Hue),
        "saturation" => mix(Mix::Saturation),
        "color" => mix(Mix::Color),
        "luminosity" => mix(Mix::Luminosity),
        _ => None,
    }
}

/// Um canal premultiplicado pelo alfa com o arredondamento do `SkMulDiv255Round`.
fn premultiply(c: u8, a: u8) -> u8 {
    let prod = u32::from(c) * u32::from(a) + 128;
    ((prod + (prod >> 8)) >> 8) as u8
}

/// Converte RGBA não premultiplicado no bitmap premultiplicado.
pub fn premultiplied(width: u32, height: u32, rgba: &[u8]) -> Option<Pixmap> {
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE || rgba.len() < (width * height * 4) as usize {
        return None;
    }
    let mut data = rgba[..(width * height * 4) as usize].to_vec();
    for px in data.chunks_exact_mut(4) {
        let a = px[3];
        for c in &mut px[..3] {
            *c = premultiply(*c, a);
        }
    }
    Some(Pixmap::from_parts(data, width as u16, height as u16, PixelMetadata { may_have_transparency: true, alpha_type: ImageAlphaType::AlphaPremultiplied }))
}

/// Desfaz a premultiplicação como o `SkUnPreMultiply` (tabela de escalas de 24 bits).
fn unpremultiply(px: PremulRgba8) -> [u8; 4] {
    let a = u32::from(px.a);
    if a == 0 {
        return [0, 0, 0, 0];
    }
    let scale = ((255u32 << 24) + a / 2) / a;
    let un = |c: u8| ((scale * u32::from(c) + (1 << 23)) >> 24).min(255) as u8;
    [un(px.r), un(px.g), un(px.b), px.a]
}

/// O traço do prelúdio: [largura, cap, join, miterLimit, traços, deslocamento].
fn stroke_style(args: &[DomArg]) -> Stroke {
    let cap = match text(args, 1) {
        "round" => Cap::Round,
        "square" => Cap::Square,
        _ => Cap::Butt,
    };
    let join = match text(args, 2) {
        "round" => Join::Round,
        "bevel" => Join::Bevel,
        _ => Join::Miter,
    };
    let dashes: Vec<f64> = list(args, 4).iter().filter_map(|d| if let DomArg::Num(n) = d { Some(*n) } else { None }).collect();
    let mut stroke = Stroke::new(num(args, 0)).with_caps(cap).with_join(join).with_miter_limit(num(args, 3));
    if !dashes.is_empty() {
        stroke = stroke.with_dashes(num(args, 5), dashes);
    }
    stroke
}

/// Uma caneta que leva o contorno de um glifo (em pixels, y para cima) ao caminho, já posicionado.
struct GlyphPen<'a> {
    out: &'a mut BezPath,
    at: Affine,
}

impl OutlinePen for GlyphPen<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.out.move_to(self.at * Point::new(f64::from(x), f64::from(-y)));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.out.line_to(self.at * Point::new(f64::from(x), f64::from(-y)));
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.out.quad_to(self.at * Point::new(f64::from(cx0), f64::from(-cy0)), self.at * Point::new(f64::from(x), f64::from(-y)));
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let p = |x: f32, y: f32| self.at * Point::new(f64::from(x), f64::from(-y));
        let (a, b, c) = (p(cx0, cy0), p(cx1, cy1), p(x, y));
        self.out.curve_to(a, b, c);
    }
    fn close(&mut self) {
        self.out.close_path();
    }
}

/// O texto do canvas como caminho em coordenadas do usuário: alinhamento, linha de base e `maxWidth` (que comprime
/// na horizontal) como no Blink. Argumentos: [texto, família, peso, estilo, tamanho, x, y, maxWidth, align,
/// baseline, direção]. Com a transformação só de translação e escala uniforme, os glifos saem como o Skia os
/// desenha: com o hinting leve do tamanho no dispositivo e a linha de base no pixel inteiro.
fn text_path(args: &[DomArg], ctm: Affine) -> BezPath {
    let font = CanvasFont { family: text(args, 1), weight: num(args, 2) as f32, style: text(args, 3), size: num(args, 4) as f32 };
    let shaped = shape(text(args, 0), &font);
    let width = f64::from(shaped.width);
    let rtl = text(args, 10) == "rtl";
    let offset = match (text(args, 8), rtl) {
        ("center", _) => -width / 2.0,
        ("right", _) | ("end", false) | ("start", true) => -width,
        _ => 0.0,
    };
    let squeeze = match args.get(7) {
        Some(DomArg::Num(max)) if *max < width && width > 0.0 => max.max(0.0) / width,
        _ => 1.0,
    };
    let [a, b, c, d, _, f] = ctm.as_coeffs();
    let hinted = (b == 0.0 && c == 0.0 && a == d && a > 0.0).then_some(a);
    let mut baseline = num(args, 6) + baseline_shift(&shaped, text(args, 9));
    if let Some(scale) = hinted {
        baseline = ((f + scale * baseline).round() - f) / scale;
    }
    let origin = Affine::translate((num(args, 5), baseline)) * Affine::scale_non_uniform(squeeze, 1.0) * Affine::translate((offset, 0.0));
    let unscale = Affine::scale(1.0 / hinted.unwrap_or(1.0));
    let mut path = BezPath::new();
    for glyph in &shaped.glyphs {
        let mut pen = GlyphPen { out: &mut path, at: origin * Affine::translate((f64::from(glyph.x), 0.0)) * unscale };
        draw_glyph(glyph, hinted.map(|s| s as f32), &mut pen);
    }
    path
}

/// O `SkPath::contains` do Skia: dentro pela regra de preenchimento, e o ponto sobre o contorno também conta.
fn contains(path: &BezPath, point: Point, rule: Fill) -> bool {
    let on_edge = path.segments().any(|seg| seg.nearest(point, 1e-12).distance_sq <= 1e-18);
    let winding = path.winding(point);
    on_edge || if rule == Fill::EvenOdd { winding % 2 != 0 } else { winding != 0 }
}

/// O `isPointInPath` (`stroke` vazio) e o `isPointInStroke` (`stroke` = [largura, cap, join, miterLimit, traços,
/// deslocamento]). `path` é o caminho do prelúdio; `ctm`, a transformação atual; (x, y) em pixels do canvas, sem
/// transformação, como medido no oráculo (`testbench/oracle/measured/canvas-hit.html`).
pub fn hit(path: &[DomArg], rule: &str, stroke: &[DomArg], ctm: &[DomArg], x: f64, y: f64) -> bool {
    if !x.is_finite() || !y.is_finite() {
        return false;
    }
    let device = device_path(path);
    let point = Point::new(x, y);
    if stroke.is_empty() {
        return contains(&device, point, fill_rule(rule));
    }
    let ctm = affine(ctm);
    if ctm.determinant() == 0.0 {
        return false;
    }
    let inverse = ctm.inverse();
    let style = stroke_style(stroke);
    let outline = kurbo::stroke(inverse * device, &style, &StrokeOpts::default(), 1e-4);
    contains(&outline, inverse * point, Fill::NonZero)
}

impl Surface {
    pub fn new(width: u32, height: u32) -> Surface {
        Surface { width, height, pixels: Pixmap::new(clamp_side(width), clamp_side(height)), scene: None }
    }

    /// O resize do canvas: bitmap novo, transparente.
    pub fn resize(&mut self, width: u32, height: u32) {
        *self = Surface::new(width, height);
    }

    fn drawable(&self) -> bool {
        self.width > 0 && self.height > 0 && self.width <= MAX_SIDE && self.height <= MAX_SIDE
    }

    /// A cena pendente; uma cena nova começa pelo bitmap atual.
    fn scene(&mut self) -> &mut RenderContext {
        let (w, h) = (self.width as u16, self.height as u16);
        let base = &self.pixels;
        self.scene.get_or_insert_with(|| {
            let mut ctx = RenderContext::new(w, h);
            if base.data().iter().any(|p| p.a != 0) {
                let brush = ImageBrush { image: ImageSource::Pixmap(Arc::new(base.clone())), sampler: ImageSampler::default().with_quality(ImageQuality::Low) };
                ctx.set_paint(brush);
                ctx.fill_rect(&Rect::new(0.0, 0.0, f64::from(w), f64::from(h)));
            }
            ctx
        })
    }

    /// Rasteriza o que está pendente.
    pub fn flush(&mut self) -> &Pixmap {
        if let Some(scene) = self.scene.take() {
            let mut pixels = Pixmap::new(self.width as u16, self.height as u16);
            let mut scene = scene;
            scene.flush();
            scene.render(&mut pixels, &mut Resources::default());
            self.pixels = pixels;
        }
        &self.pixels
    }

    /// Um retângulo do bitmap em RGBA não premultiplicado; o que cai fora do canvas sai transparente.
    pub fn read(&mut self, x: i64, y: i64, w: u32, h: u32) -> Vec<u8> {
        let mut out = vec![0u8; (w as usize) * (h as usize) * 4];
        if !self.drawable() {
            return out;
        }
        let cells = self.region(x, y, w, h);
        let data = self.flush().data();
        for (canvas, at) in cells {
            out[at..at + 4].copy_from_slice(&unpremultiply(data[canvas]));
        }
        out
    }

    /// O `putImageData`: os pixels entram crus, sem composição, recorte nem transformação.
    pub fn write(&mut self, x: i64, y: i64, w: u32, h: u32, rgba: &[u8]) {
        if !self.drawable() {
            return;
        }
        let cells = self.region(x, y, w, h);
        self.flush();
        let data = self.pixels.data_mut();
        for (canvas, at) in cells {
            let Some(src) = rgba.get(at..at + 4) else { continue };
            data[canvas] = PremulRgba8 { r: premultiply(src[0], src[3]), g: premultiply(src[1], src[3]), b: premultiply(src[2], src[3]), a: src[3] };
        }
    }

    /// As células de um retângulo `w`x`h` em (`x`, `y`) que caem no canvas: (índice do pixel no canvas, deslocamento
    /// do byte no buffer RGBA do retângulo).
    fn region(&self, x: i64, y: i64, w: u32, h: u32) -> Vec<(usize, usize)> {
        let (width, height) = (i64::from(self.width), i64::from(self.height));
        let rows = (0..i64::from(h)).filter(|row| (0..height).contains(&(y + row)));
        rows.flat_map(|row| (0..i64::from(w)).filter(move |col| (0..width).contains(&(x + col))).map(move |col| (((y + row) * width + x + col) as usize, ((row * i64::from(w) + col) * 4) as usize))).collect()
    }

    /// Um comando de desenho. `common` = [CTM, globalAlpha, composição, recortes, sombra]; `cmd` = [tipo, ...];
    /// `source` é a imagem do `drawImage` ou do padrão, já em bitmap premultiplicado.
    pub fn draw(&mut self, common: &[DomArg], cmd: &[DomArg], source: Option<Arc<Pixmap>>) {
        if !self.drawable() {
            return;
        }
        let ctm = affine(list(common, 0));
        let alpha = num(common, 1);
        if ctm.determinant() == 0.0 || !ctm.determinant().is_finite() || alpha <= 0.0 {
            return;
        }
        let mode = blend(text(common, 2));
        let clips = list(common, 3);
        let shadow = list(common, 4);
        let kind = text(cmd, 0);
        // clearRect: apaga o retângulo dentro do recorte, sem composição, opacidade nem sombra, e sem antisserrilhado:
        // alinhado aos eixos, o Skia arredonda cada borda com floor(x + 0,5); girado, vale o centro do pixel.
        if kind == "clear" {
            let rect = ctm.transform_rect_bbox(Rect::new(num(cmd, 1), num(cmd, 2), num(cmd, 1) + num(cmd, 3), num(cmd, 2) + num(cmd, 4)));
            let [_, b, c, _, _, _] = ctm.as_coeffs();
            let (w, h) = (self.width, self.height);
            let scene = self.scene();
            push_clip_and_mode(scene, clips, Some(BlendMode::new(Mix::Normal, Compose::DestOut)), w, h);
            scene.set_transform(Affine::IDENTITY);
            scene.set_paint(AlphaColor::<Srgb>::BLACK);
            scene.set_fill_rule(Fill::NonZero);
            if b == 0.0 && c == 0.0 {
                let snap = |v: f64| (v + 0.5).floor();
                scene.fill_rect(&Rect::new(snap(rect.x0), snap(rect.y0), snap(rect.x1), snap(rect.y1)));
            } else {
                scene.set_aliasing_threshold(Some(128));
                scene.fill_path(&flat(ctm * Rect::new(num(cmd, 1), num(cmd, 2), num(cmd, 1) + num(cmd, 3), num(cmd, 2) + num(cmd, 4)).to_path(0.1)));
                scene.set_aliasing_threshold(None);
            }
            pop_layers(scene, clips, true);
            return;
        }
        // O que desenhar, já em coordenadas do dispositivo: o traço se expande no espaço do usuário (a largura e os
        // traços acompanham a transformação) e tudo se achata em retas finas, para a cobertura das curvas ser a da
        // curva e não a das cordas do achatamento grosso do vello.
        let tolerance = DEVICE_TOLERANCE / ctm.determinant().abs().sqrt().max(1e-9);
        let stroked = |user: BezPath, style: &Stroke| -> Option<BezPath> {
            (style.width > 0.0).then(|| ctm * kurbo::stroke(user, style, &StrokeOpts::default(), tolerance))
        };
        let (path, rule, image) = match kind {
            "fill" => (device_path(list(cmd, 1)), fill_rule(text(cmd, 2)), None),
            "stroke" => {
                let Some(path) = stroked(ctm.inverse() * device_path(list(cmd, 1)), &stroke_style(list(cmd, 2))) else { return };
                (path, Fill::NonZero, None)
            }
            "fillText" => (ctm * text_path(list(cmd, 1), ctm), Fill::NonZero, None),
            "strokeText" => {
                let Some(path) = stroked(text_path(list(cmd, 1), ctm), &stroke_style(list(cmd, 2))) else { return };
                (path, Fill::NonZero, None)
            }
            "image" => {
                let Some(image) = source.as_ref() else { return };
                let (iw, ih) = (f64::from(image.width()), f64::from(image.height()));
                let (mut sx, mut sy, mut sw, mut sh) = (num(cmd, 1), num(cmd, 2), num(cmd, 3), num(cmd, 4));
                let (mut dx, mut dy, mut dw, mut dh) = (num(cmd, 5), num(cmd, 6), num(cmd, 7), num(cmd, 8));
                // Retângulos com lado negativo se normalizam; a origem fora da imagem encolhe o destino na proporção.
                if sw < 0.0 { (sx, sw) = (sx + sw, -sw); }
                if sh < 0.0 { (sy, sh) = (sy + sh, -sh); }
                if dw < 0.0 { (dx, dw) = (dx + dw, -dw); }
                if dh < 0.0 { (dy, dh) = (dy + dh, -dh); }
                if sw == 0.0 || sh == 0.0 || dw == 0.0 || dh == 0.0 {
                    return;
                }
                let (kx, ky) = (dw / sw, dh / sh);
                let clipped = Rect::new(sx.max(0.0), sy.max(0.0), (sx + sw).min(iw), (sy + sh).min(ih));
                if clipped.width() <= 0.0 || clipped.height() <= 0.0 {
                    return;
                }
                let dest = Rect::new(dx + (clipped.x0 - sx) * kx, dy + (clipped.y0 - sy) * ky, dx + (clipped.x1 - sx) * kx, dy + (clipped.y1 - sy) * ky);
                let map = Affine::translate((dx, dy)) * Affine::scale_non_uniform(kx, ky) * Affine::translate((-sx, -sy));
                (ctm * dest.to_path(0.1), Fill::NonZero, Some(ctm * map))
            }
            _ => return,
        };
        let path = flat(path);
        let smoothing = num(common, 5) != 0.0;
        let quality = if !smoothing {
            ImageQuality::Low
        } else if text(common, 6) == "low" {
            ImageQuality::Medium
        } else {
            ImageQuality::High
        };
        let (w, h) = (self.width, self.height);
        let scene = self.scene();
        let shadowed = shadow_filter(shadow);
        if let Some(filter) = &shadowed {
            scene.set_transform(Affine::IDENTITY);
            scene.push_layer(None, None, None, None, Some(filter.clone()));
        }
        push_clip_and_mode(scene, clips, mode, w, h);
        scene.set_transform(Affine::IDENTITY);
        scene.set_fill_rule(rule);
        match image {
            Some(map) => {
                if let Some(image) = source {
                    scene.set_paint(ImageBrush { image: ImageSource::Pixmap(image), sampler: ImageSampler::default().with_quality(quality).with_alpha(alpha as f32) });
                    scene.set_paint_transform(map);
                    scene.fill_path(&path);
                }
            }
            None => {
                if let Some(pushed) = set_paint(scene, list(cmd, 3), alpha, source, quality, ctm) {
                    scene.set_fill_rule(rule);
                    scene.fill_path(&path);
                    if pushed {
                        scene.pop_layer();
                    }
                }
            }
        }
        pop_layers(scene, clips, mode.is_some());
        if shadowed.is_some() {
            scene.pop_layer();
        }
    }

    /// Os bytes RGBA não premultiplicados do canvas inteiro (para codificar).
    pub fn rgba(&mut self) -> (u32, u32, Vec<u8>) {
        let (w, h) = (self.width, self.height);
        (w, h, self.read(0, 0, w, h))
    }

    /// O bitmap rasterizado, para servir de imagem a outro desenho.
    pub fn snapshot(&mut self) -> Option<Arc<Pixmap>> {
        if !self.drawable() {
            return None;
        }
        Some(Arc::new(self.flush().clone()))
    }
}

fn clamp_side(side: u32) -> u16 {
    if side == 0 || side > MAX_SIDE { 1 } else { side as u16 }
}

/// A sombra (`shadowColor`, `shadowBlur`, `shadowOffsetX/Y`), quando visível: o desvio padrão é metade do blur.
fn shadow_filter(shadow: &[DomArg]) -> Option<Filter> {
    let color = color(list(shadow, 0), 1.0);
    let (blur, dx, dy) = (num(shadow, 1), num(shadow, 2), num(shadow, 3));
    if color.components[3] == 0.0 || (blur == 0.0 && dx == 0.0 && dy == 0.0) {
        return None;
    }
    Some(Filter::from_primitive(FilterPrimitive::DropShadow { dx: dx as f32, dy: dy as f32, std_deviation: (blur / 2.0) as f32, color, edge_mode: EdgeMode::None }))
}

/// O recorte (um caminho direto; vários viram máscara da interseção) e a composição numa camada só, para a
/// composição valer contra o canvas e não contra uma camada vazia.
fn push_clip_and_mode(scene: &mut RenderContext, clips: &[DomArg], mode: Option<BlendMode>, width: u32, height: u32) {
    if clips.is_empty() && mode.is_none() {
        return;
    }
    scene.set_transform(Affine::IDENTITY);
    let paths: Vec<(BezPath, Fill)> = clips.iter().filter_map(|c| if let DomArg::List(c) = c { Some((flat(device_path(c)), fill_rule(text(c, 2)))) } else { None }).collect();
    match paths.as_slice() {
        [] => scene.push_layer(None, mode, None, None, None),
        [(path, rule)] => {
            scene.set_fill_rule(*rule);
            scene.push_layer(Some(path), mode, None, None, None);
        }
        _ => {
            let mut mask_ctx = RenderContext::new(width as u16, height as u16);
            for (path, rule) in &paths {
                mask_ctx.set_fill_rule(*rule);
                mask_ctx.push_clip_layer(path);
            }
            mask_ctx.set_paint(AlphaColor::<Srgb>::BLACK);
            mask_ctx.fill_rect(&Rect::new(0.0, 0.0, f64::from(width), f64::from(height)));
            for _ in &paths {
                mask_ctx.pop_layer();
            }
            mask_ctx.flush();
            let mut pixmap = Pixmap::new(width as u16, height as u16);
            mask_ctx.render(&mut pixmap, &mut Resources::default());
            scene.push_layer(None, mode, None, Some(Mask::new_alpha(&pixmap)), None);
        }
    }
}

fn pop_layers(scene: &mut RenderContext, clips: &[DomArg], had_mode: bool) {
    if !clips.is_empty() || had_mode {
        scene.pop_layer();
    }
}

/// A pintura do prelúdio: ['color', r, g, b, a], ['linear', x0, y0, x1, y1, paradas], ['radial', x0, y0, r0, x1,
/// y1, r1, paradas], ['conic', ângulo, x, y, paradas] ou ['pattern', repetição, matriz]. `None` quando nada é
/// pintado (gradiente sem paradas ou degenerado); `Some(true)` quando abriu a camada de recorte do padrão que não se
/// repete em algum eixo, que o chamador fecha.
fn set_paint(scene: &mut RenderContext, paint: &[DomArg], alpha: f64, source: Option<Arc<Pixmap>>, quality: ImageQuality, ctm: Affine) -> Option<bool> {
    let stops = |i: usize| -> Vec<(f32, AlphaColor<Srgb>)> { list(paint, i).iter().filter_map(|s| if let DomArg::List(s) = s { Some((num(s, 0) as f32, color(&s[1..], alpha))) } else { None }).collect() };
    let gradient = |g: Gradient, stops: Vec<(f32, AlphaColor<Srgb>)>| -> PaintType {
        let mut g = g.with_stops(stops.as_slice()).with_extend(Extend::Pad);
        g.interpolation_alpha_space = InterpolationAlphaSpace::Unpremultiplied;
        PaintType::Gradient(g)
    };
    let mut clip: Option<BezPath> = None;
    let brush: PaintType = match text(paint, 0) {
        "linear" => {
            let s = stops(5);
            let (p0, p1) = (Point::new(num(paint, 1), num(paint, 2)), Point::new(num(paint, 3), num(paint, 4)));
            if s.is_empty() || p0 == p1 {
                return None;
            }
            if s.len() == 1 { PaintType::Solid(s[0].1) } else { gradient(Gradient::new_linear(p0, p1), s) }
        }
        "radial" => {
            let s = stops(7);
            let (c0, r0, c1, r1) = (Point::new(num(paint, 1), num(paint, 2)), num(paint, 3), Point::new(num(paint, 4), num(paint, 5)), num(paint, 6));
            if s.is_empty() || (c0 == c1 && r0 == r1) {
                return None;
            }
            if s.len() == 1 { PaintType::Solid(s[0].1) } else { gradient(Gradient::new_two_point_radial(c0, r0 as f32, c1, r1 as f32), s) }
        }
        "conic" => {
            let s = stops(4);
            if s.is_empty() {
                return None;
            }
            let start = num(paint, 1) as f32;
            if s.len() == 1 { PaintType::Solid(s[0].1) } else { gradient(Gradient::new_sweep(Point::new(num(paint, 2), num(paint, 3)), start, start + std::f32::consts::TAU), s) }
        }
        "pattern" => {
            let Some(image) = source else { return None };
            let (iw, ih) = (f64::from(image.width()), f64::from(image.height()));
            let repetition = text(paint, 1);
            let (rx, ry) = (matches!(repetition, "repeat" | "repeat-x" | ""), matches!(repetition, "repeat" | "repeat-y" | ""));
            let m = list(paint, 2);
            let matrix = ctm * if m.is_empty() { Affine::IDENTITY } else { affine(m) };
            if !(rx && ry) {
                let area = Rect::new(if rx { -FAR } else { 0.0 }, if ry { -FAR } else { 0.0 }, if rx { FAR } else { iw }, if ry { FAR } else { ih });
                clip = Some(flat(matrix * area.to_path(0.1)));
            }
            scene.set_paint_transform(matrix);
            let extend = |r: bool| if r { Extend::Repeat } else { Extend::Pad };
            PaintType::Image(ImageBrush { image: ImageSource::Pixmap(image), sampler: ImageSampler::default().with_x_extend(extend(rx)).with_y_extend(extend(ry)).with_quality(quality).with_alpha(alpha as f32) })
        }
        _ => PaintType::Solid(color(&paint[1.min(paint.len())..], alpha)),
    };
    let pushed = clip.is_some();
    scene.set_transform(Affine::IDENTITY);
    if let Some(clip) = &clip {
        scene.push_clip_layer(clip);
    }
    if !matches!(brush, PaintType::Image(_)) {
        scene.set_paint_transform(ctm);
    }
    scene.set_paint(brush);
    Some(pushed)
}
