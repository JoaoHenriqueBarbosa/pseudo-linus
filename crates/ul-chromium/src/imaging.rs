//! Decodificação das imagens da página: o formato sai dos bytes (como no Chromium, o `Content-Type` não decide), e o
//! resultado é a imagem em RGBA de 8 bits com alfa não premultiplicado.

use image::ImageReader;
use std::io::Cursor;

/// Uma imagem decodificada. `rgba` fica vazio no SVG, que ainda não é rasterizado.
pub struct Decoded {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Decodifica `bytes`; `None` quando o formato não é reconhecido ou o arquivo está corrompido (o `error` da imagem).
/// SVG não tem pixels aqui, só o tamanho intrínseco.
pub fn decode(bytes: &[u8]) -> Option<Decoded> {
    if let Some((width, height)) = svg_size(bytes) {
        return Some(Decoded { width, height, rgba: Vec::new() });
    }
    let reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format().ok()?;
    let image = reader.decode().ok()?.into_rgba8();
    Some(Decoded { width: image.width(), height: image.height(), rgba: image.into_raw() })
}

/// O tamanho intrínseco de um SVG como imagem, medido no oráculo: `width`/`height` absolutos valem; o que falta sai
/// da proporção do `viewBox` quando o outro existe, senão 300x150 (porcentagem conta como ausente; 1em = 16px).
fn svg_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let text = std::str::from_utf8(bytes).ok()?;
    let start = text.find("<svg")?;
    if text[..start].contains(|c: char| !c.is_whitespace()) && !text[..start].trim_start().starts_with("<?xml") && !text[..start].contains("<!") {
        return None;
    }
    let tag = &text[start..start + text[start..].find('>')?];
    let attr = |name: &str| -> Option<&str> {
        let mut rest = tag;
        while let Some(i) = rest.find(name) {
            let before = rest[..i].chars().last();
            let after = rest[i + name.len()..].trim_start();
            if before.is_some_and(char::is_whitespace) && after.starts_with('=') {
                let value = after[1..].trim_start();
                let quote = value.chars().next()?;
                return value[1..].split(quote).next();
            }
            rest = &rest[i + name.len()..];
        }
        None
    };
    let length = |v: Option<&str>| -> Option<f64> {
        let v = v?.trim();
        let split = v.find(|c: char| c.is_ascii_alphabetic() || c == '%').unwrap_or(v.len());
        let number: f64 = v[..split].trim().parse().ok()?;
        let scale = match &v[split..] {
            "" | "px" => 1.0,
            "em" => 16.0,
            "ex" => 8.0,
            "in" => 96.0,
            "cm" => 96.0 / 2.54,
            "mm" => 96.0 / 25.4,
            "pt" => 96.0 / 72.0,
            "pc" => 16.0,
            _ => return None,
        };
        Some(number * scale)
    };
    let ratio = attr("viewBox").and_then(|v| {
        let n: Vec<f64> = v.split(|c: char| c == ',' || c.is_whitespace()).filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).collect();
        (n.len() == 4 && n[2] > 0.0 && n[3] > 0.0).then(|| n[2] / n[3])
    });
    let (w, h) = (length(attr("width")), length(attr("height")));
    let (width, height) = match (w, h, ratio) {
        (Some(w), Some(h), _) => (w, h),
        (Some(w), None, Some(r)) => (w, w / r),
        (None, Some(h), Some(r)) => (h * r, h),
        (w, h, _) => (w.unwrap_or(300.0), h.unwrap_or(150.0)),
    };
    Some((width.round() as u32, height.round() as u32))
}

/// O `toDataURL`: `mime;base64,dados`. PNG por padrão; `image/jpeg` com a qualidade pedida (0.92 sem ela). O tipo
/// que o Chromium não codifica vira PNG.
/// `icc` diz se o canvas leva o perfil sRGB no WebP (o JPEG leva sempre).
pub fn encode(mime: &str, quality: f64, icc: bool, width: u32, height: u32, rgba: &[u8]) -> String {
    let (kind, bytes) = match mime.to_ascii_lowercase().as_str() {
        "image/jpeg" => {
            let quality = if (0.0..=1.0).contains(&quality) { quality } else { 0.92 };
            ("image/jpeg", encode_jpeg(width, height, rgba, quality))
        }
        // Qualidade 1 é sem perdas; fora de [0, 1], 0,8. O ICCP é o perfil do APP2 sem o cabeçalho `ICC_PROFILE`.
        "image/webp" => {
            let quality = if (0.0..=1.0).contains(&quality) { quality } else { 0.8 };
            let options = crate::webp::Options { quality: (quality * 100.0) as f32, lossless: quality == 1.0, icc: icc.then(|| &JPEG_SRGB_ICC[18..]) };
            ("image/webp", crate::webp::encode(rgba, width as usize, height as usize, &options))
        }
        _ => ("image/png", encode_png(width, height, rgba)),
    };
    format!("{kind};base64,{}", ul_common::codec::base64_string(&bytes))
}

/// O segmento APP2 com o perfil ICC sRGB que o Skia grava em todo JPEG do canvas (tirado do oráculo).
const JPEG_SRGB_ICC: &[u8] = include_bytes!("../data/jpeg-srgb-icc.bin");

/// O JPEG do canvas: o alfa é composto sobre preto, como o Skia faz antes do libjpeg-turbo, com o perfil sRGB depois
/// do JFIF e Huffman otimizado.
fn encode_jpeg(width: u32, height: u32, rgba: &[u8], quality: f64) -> Vec<u8> {
    let rgb: Vec<u8> = rgba.chunks_exact(4).flat_map(|p| [0, 1, 2].map(|c| ((u32::from(p[c]) * u32::from(p[3]) + 127) / 255) as u8)).collect();
    let mut options = zjpeg::EncodeOptions::new(width as usize, height as usize, zjpeg::InputSpace::Rgb);
    options.quality = Some((quality * 100.0).round() as i32);
    options.optimize = true;
    options.extra = JPEG_SRGB_ICC.to_vec();
    zjpeg::encode(&options, &rgb).unwrap_or_default()
}

/// O tamanho do buffer de IDAT do `StreamWriter` do `png` crate: o fluxo sai em IDATs desse tamanho.
const PNG_CHUNK: usize = 4096;

/// Comprime `input` no fluxo zlib corrente até o `flush` pedido.
fn deflate_until(compressor: &mut miniz_oxide::deflate::core::CompressorOxide, mut input: &[u8], flush: miniz_oxide::MZFlush) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        let result = miniz_oxide::deflate::stream::deflate(compressor, input, &mut buffer, flush);
        input = &input[result.bytes_consumed..];
        out.extend_from_slice(&buffer[..result.bytes_written]);
        let done = matches!(result.status, Ok(miniz_oxide::MZStatus::StreamEnd)) || result.status.is_err() || (input.is_empty() && result.bytes_written < buffer.len());
        if done {
            return out;
        }
    }
}

fn png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut chunk = kind.to_vec();
    chunk.extend_from_slice(data);
    out.extend_from_slice(&chunk);
    out.extend_from_slice(&ul_common::codec::crc32_update(0, &chunk).to_be_bytes());
}

/// O PNG do canvas como o Chromium do oráculo o escreve (o codificador Rust do Skia, sobre o `png` crate com o
/// `StreamWriter` e compressão rápida): RGBA de 8 bits, filtro Up em todas as linhas, zlib nível 1 do miniz_oxide com
/// flush de sincronia no fim dos dados (IDATs de 4 KiB) e o bloco final num IDAT próprio. Medido em
/// `testbench/oracle/measured/canvas-encode.html`. O zlib vai direto ao miniz_oxide porque, no workspace, o flate2
/// que o `png` crate usaria compila com o backend zlib-rs (que o `ul-net` liga) e comprime diferente.
fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let stride = width as usize * 4;
    let mut raw = Vec::with_capacity((stride + 1) * height as usize);
    let mut previous = vec![0u8; stride];
    for row in rgba.chunks_exact(stride.max(1)).take(height as usize) {
        raw.push(2);
        raw.extend(row.iter().zip(&previous).map(|(a, b)| a.wrapping_sub(*b)));
        previous.copy_from_slice(row);
    }
    let mut compressor = miniz_oxide::deflate::core::CompressorOxide::default();
    compressor.set_format_and_level(miniz_oxide::DataFormat::Zlib, 1);
    let body = deflate_until(&mut compressor, &raw, miniz_oxide::MZFlush::Sync);
    let tail = deflate_until(&mut compressor, &[], miniz_oxide::MZFlush::Finish);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    png_chunk(&mut out, b"IHDR", &ihdr);
    for piece in body.chunks(PNG_CHUNK) {
        png_chunk(&mut out, b"IDAT", piece);
    }
    png_chunk(&mut out, b"IDAT", &tail);
    png_chunk(&mut out, b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_dimensions_and_pixels() {
        // PNG 2x1 RGBA: vermelho opaco e azul meio transparente.
        let mut png = Vec::new();
        let img = image::RgbaImage::from_raw(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 128]).unwrap();
        img.write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let decoded = decode(&png).unwrap();
        assert_eq!((decoded.width, decoded.height), (2, 1));
        assert_eq!(decoded.rgba, vec![255, 0, 0, 255, 0, 0, 255, 128]);
        assert!(decode(b"not an image").is_none());
    }
}
