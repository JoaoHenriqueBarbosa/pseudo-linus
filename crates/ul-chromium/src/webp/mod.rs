//! O codificador WebP do libwebp 1.5.0 no caminho que o `SkWebpEncoder` do Chromium usa no `toDataURL` e no `toBlob`:
//! com perdas (VP8, método 3) ou sem perdas (VP8L, método 0, qualidade 75), e o contêiner do `WebPMux` com o perfil
//! ICC do sRGB quando o canvas tem contexto.

mod alpha;
mod bitwriter;
mod dsp;
mod picture;
mod tables;
mod vp8;
mod vp8l;

/// O que o Skia entrega ao libwebp.
pub struct Options<'a> {
    /// A qualidade de 0 a 100 (o `fQuality` do Skia).
    pub quality: f32,
    pub lossless: bool,
    pub icc: Option<&'a [u8]>,
}

fn chunk(out: &mut Vec<u8>, tag: &[u8; 4], payload: &[u8]) {
    out.extend_from_slice(tag);
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    if payload.len() & 1 != 0 {
        out.push(0);
    }
}

/// Codifica RGBA não pré-multiplicado em linhas contíguas.
pub fn encode(rgba: &[u8], width: usize, height: usize, options: &Options) -> Vec<u8> {
    let (alpha, image, image_tag, has_alpha): (Option<Vec<u8>>, Vec<u8>, &[u8; 4], bool) = if options.lossless {
        let (stream, has_alpha) = vp8l::encode_image(rgba, width, height);
        (None, stream, b"VP8L", has_alpha)
    } else {
        let config = vp8::Config::skia_lossy(options.quality);
        let mut pic = picture::import_rgba(rgba, width, height);
        picture::cleanup_transparent_area(&mut pic);
        let (alpha, vp8) = vp8::encode(&config, &pic);
        let has_alpha = alpha.is_some();
        (alpha, vp8, b"VP8 ", has_alpha)
    };
    let mut body = Vec::new();
    if options.icc.is_some() || has_alpha && !options.lossless {
        let flags = if options.icc.is_some() { 0x20u32 } else { 0 } | if has_alpha { 0x10 } else { 0 };
        let mut vp8x = Vec::with_capacity(10);
        vp8x.extend_from_slice(&flags.to_le_bytes());
        vp8x.extend_from_slice(&((width - 1) as u32).to_le_bytes()[..3]);
        vp8x.extend_from_slice(&((height - 1) as u32).to_le_bytes()[..3]);
        chunk(&mut body, b"VP8X", &vp8x);
        if let Some(icc) = options.icc {
            chunk(&mut body, b"ICCP", icc);
        }
        if let Some(alpha) = &alpha {
            chunk(&mut body, b"ALPH", alpha);
        }
    }
    chunk(&mut body, image_tag, &image);
    let mut out = Vec::with_capacity(body.len() + 12);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(body.len() as u32 + 4).to_le_bytes());
    out.extend_from_slice(b"WEBP");
    out.extend_from_slice(&body);
    out
}
