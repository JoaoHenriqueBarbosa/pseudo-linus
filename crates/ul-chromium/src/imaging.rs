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
