//! A pintura da página: o Blitz resolve o layout e o `blitz-paint` desenha no rasterizador em CPU (vello_cpu).
//! Serve o `--screenshot` e o `Page.captureScreenshot`; o resultado é o que o Chromium mostra no viewport (fundo
//! branco; a barra de rolagem clássica quando o documento passa do viewport, salvo `--hide-scrollbars`).

use anyrender::ImageRenderer as _;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_traits::shell::Viewport;

use crate::bridge::Bridge;
use crate::page::Page;

/// A largura da barra de rolagem vertical do Chromium no Linux, em pixels CSS.
pub const SCROLLBAR: f64 = 15.0;

/// As medidas do documento para o `Page.getLayoutMetrics`, em pixels CSS.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    /// A janela (o viewport visual) e o viewport do layout, que perde a barra de rolagem quando ela aparece.
    pub window: (f64, f64),
    pub client: (f64, f64),
    /// O tamanho do conteúdo: o documento inteiro, nunca menor que o viewport.
    pub content: (f64, f64),
    pub scrollbar: bool,
    /// A escala do dispositivo (`Emulation.setDeviceMetricsOverride`).
    pub scale: f64,
}

/// O retângulo de um recorte, em pixels CSS do documento, e a escala dele.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Clip {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub scale: f64,
}

/// O que um screenshot pede além da imagem inteira do viewport.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Capture {
    pub clip: Option<Clip>,
    /// `captureBeyondViewport`: o documento inteiro, sem barra de rolagem.
    pub beyond: bool,
    /// `--hide-scrollbars`.
    pub hide_scrollbars: bool,
}

/// As medidas do documento corrente.
pub fn metrics(bridge: &Bridge) -> Metrics {
    bridge.flush_styles();
    let mut doc = bridge.doc.borrow_mut();
    doc.resolve(0.0);
    let viewport = doc.viewport();
    let scale = f64::from(viewport.hidpi_scale.max(0.1));
    let window = (f64::from(viewport.window_size.0) / scale, f64::from(viewport.window_size.1) / scale);
    let root = doc.root_element();
    // O layout do Blitz usa a largura da janela inteira; o que passa dela é o que o conteúdo ocupa de verdade.
    let tall = f64::from(root.scroll_height()).max(window.1);
    let scrollbar = tall > window.1;
    let client = (if scrollbar { window.0 - SCROLLBAR } else { window.0 }, window.1);
    // O layout do Blitz usa a largura da janela inteira: só o que passa dela é conteúdo mais largo que o viewport.
    let wide = f64::from(root.scroll_width());
    let content = (if wide > window.0 { wide } else { client.0 }, tall);
    Metrics { window, client, content, scrollbar, scale }
}

/// Pinta o documento num viewport de `css_size` pixels CSS na escala `scale` e devolve o RGBA (alfa reto, sobre o branco).
fn render(bridge: &Bridge, css_size: (f64, f64), scale: f64) -> (u32, u32, Vec<u8>) {
    let (width, height) = ((css_size.0 * scale).round().max(1.0) as u32, (css_size.1 * scale).round().max(1.0) as u32);
    {
        let mut doc = bridge.doc.borrow_mut();
        let scheme = doc.viewport().color_scheme;
        doc.set_viewport(Viewport::new(width, height, scale as f32, scheme));
        doc.resolve(0.0);
    }
    // O layout pede as imagens e folhas que faltam; elas chegam e o layout é refeito antes de pintar.
    bridge.serve_resources();
    let mut doc = bridge.doc.borrow_mut();
    doc.resolve(0.0);
    let mut renderer = VelloCpuImageRenderer::new(width, height);
    let mut buffer = Vec::new();
    renderer.render_to_vec(|scene| blitz_paint::paint_scene(scene, &mut doc, scale, width, height, 0, 0), &mut buffer);
    over_white(&mut buffer);
    (width, height, buffer)
}

/// O screenshot do documento: (largura, altura, RGBA) em pixels físicos. Sem recorte, o viewport inteiro (ou o documento
/// inteiro com `beyond`); com recorte, só o retângulo, na escala dele.
pub fn capture(bridge: &Bridge, capture: &Capture) -> (u32, u32, Vec<u8>) {
    let measured = metrics(bridge);
    let (saved_size, scheme) = {
        let doc = bridge.doc.borrow();
        (doc.viewport().window_size, doc.viewport().color_scheme)
    };
    let clip_scale = capture.clip.map_or(1.0, |c| c.scale.max(0.01));
    let scale = measured.scale * clip_scale;
    // O documento inteiro pinta num viewport do tamanho dele (sem barra); senão, na janela.
    let size = if capture.beyond { (if measured.scrollbar { measured.client.0 } else { measured.window.0 }, measured.content.1) } else { measured.window };
    let (width, height, mut rgba) = render(bridge, size, scale);
    if measured.scrollbar && !capture.beyond && !capture.hide_scrollbars {
        draw_scrollbar(&mut rgba, width, height, scale, measured.content.1 / measured.window.1);
    }
    {
        let mut doc = bridge.doc.borrow_mut();
        doc.set_viewport(Viewport::new(saved_size.0, saved_size.1, measured.scale as f32, scheme));
        doc.resolve(0.0);
    }
    let Some(clip) = capture.clip else { return (width, height, rgba) };
    // O recorte em pixels da imagem; o que cai fora do documento fica branco.
    let (cx, cy) = ((clip.x * scale).round() as i64, (clip.y * scale).round() as i64);
    let (cw, ch) = (((clip.width * scale).round() as i64).max(1), ((clip.height * scale).round() as i64).max(1));
    let mut out = vec![255u8; (cw * ch * 4) as usize];
    for row in 0..ch {
        let source_y = cy + row;
        if !(0..i64::from(height)).contains(&source_y) {
            continue;
        }
        let from = (cx.max(0)).min(i64::from(width));
        let to = (cx + cw).clamp(0, i64::from(width));
        if from >= to {
            continue;
        }
        let source = ((source_y * i64::from(width) + from) * 4) as usize..((source_y * i64::from(width) + to) * 4) as usize;
        let target = ((row * cw + (from - cx)) * 4) as usize;
        out[target..target + source.len()].copy_from_slice(&rgba[source]);
    }
    (cw as u32, ch as u32, out)
}

/// A barra de rolagem vertical do Chromium no Linux, medida no oráculo (780x437, documento de 1274 de altura): a trilha
/// de 15 px em `rgb(252,252,252)`, o polegar cinza `rgb(139,139,139)` de 10 px de largura com as pontas arredondadas e as
/// duas setas de 5 px. `ratio` é a altura do documento sobre a da janela (o polegar mede a razão inversa da trilha).
fn draw_scrollbar(rgba: &mut [u8], width: u32, height: u32, scale: f64, ratio: f64) {
    let s = |v: f64| (v * scale).round() as i64;
    let (w, h) = (i64::from(width), i64::from(height));
    let left = w - s(SCROLLBAR);
    let mut set = |x: i64, y: i64, value: u8| {
        if (0..w).contains(&x) && (0..h).contains(&y) {
            let at = ((y * w + x) * 4) as usize;
            rgba[at..at + 3].fill(value);
        }
    };
    for y in 0..h {
        for x in left..w {
            set(x, y, 252);
        }
    }
    let track = (h - 2 * s(SCROLLBAR)).max(1);
    let thumb = ((track as f64 / ratio).round() as i64).max(s(20.0)).min(track);
    let top = s(SCROLLBAR) + s(2.0);
    for y in top..(top + thumb - s(4.0)) {
        let rounding = (s(3.0) - (y - top).min(top + thumb - s(4.0) - 1 - y)).max(0);
        for x in (left + s(3.0) + rounding / 2)..(left + s(12.0) - rounding / 2) {
            set(x, y, 139);
        }
    }
    // As setas: triângulos de 5 px no centro dos botões das pontas.
    let center = left + s(7.5);
    for row in 0..s(4.0).max(1) {
        for x in (center - row)..=(center + row) {
            set(x, s(5.0) + row, 139);
            set(x, h - s(5.0) - row, 139);
        }
    }
}

impl Page {
    /// O viewport da página pintado em RGBA de 8 bits (alfa reto, sobre o fundo branco do Chromium).
    pub fn paint_rgba(&self, width: u32, height: u32, hide_scrollbars: bool) -> Vec<u8> {
        self.bridge.doc.borrow_mut().set_viewport(Viewport::new(width, height, 1.0, blitz_traits::shell::ColorScheme::Light));
        capture(&self.bridge, &Capture { hide_scrollbars, ..Capture::default() }).2
    }

    /// O viewport da página como PNG.
    pub fn screenshot_png(&self, width: u32, height: u32, hide_scrollbars: bool) -> Vec<u8> {
        crate::imaging::encode_png(width, height, &self.paint_rgba(width, height, hide_scrollbars))
    }
}

/// Compõe o RGBA (alfa pré-multiplicado, como o rasterizador entrega) sobre o branco e deixa o alfa opaco.
fn over_white(rgba: &mut [u8]) {
    for pixel in rgba.chunks_exact_mut(4) {
        let missing = 255 - pixel[3];
        for channel in &mut pixel[..3] {
            *channel = channel.saturating_add(missing);
        }
        pixel[3] = 255;
    }
}
