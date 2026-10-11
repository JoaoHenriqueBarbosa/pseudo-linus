//! A pintura da página: o Blitz resolve o layout e o `blitz-paint` desenha no rasterizador em CPU (vello_cpu).
//! Serve o `--screenshot` e, depois, o `Page.captureScreenshot`; o resultado é o que o Chromium mostra no viewport
//! (fundo branco, sem barras de rolagem no modo headless).

use anyrender::ImageRenderer as _;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_traits::shell::{ColorScheme, Viewport};

use crate::page::Page;

impl Page {
    /// O viewport da página pintado em RGBA de 8 bits (alfa reto, sobre o fundo branco do Chromium).
    pub fn paint_rgba(&self, width: u32, height: u32) -> Vec<u8> {
        self.bridge.flush_styles();
        {
            let mut doc = self.bridge.doc.borrow_mut();
            doc.set_viewport(Viewport::new(width, height, 1.0, ColorScheme::Light));
            doc.resolve(0.0);
        }
        // O layout pede as imagens e folhas que faltam; elas chegam e o layout é refeito antes de pintar.
        self.bridge.serve_resources();
        let mut doc = self.bridge.doc.borrow_mut();
        doc.resolve(0.0);
        let mut renderer = VelloCpuImageRenderer::new(width, height);
        let mut buffer = Vec::new();
        renderer.render_to_vec(|scene| blitz_paint::paint_scene(scene, &mut doc, 1.0, width, height, 0, 0), &mut buffer);
        over_white(&mut buffer);
        buffer
    }

    /// O viewport da página como PNG.
    pub fn screenshot_png(&self, width: u32, height: u32) -> Vec<u8> {
        crate::imaging::encode_png(width, height, &self.paint_rgba(width, height))
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
