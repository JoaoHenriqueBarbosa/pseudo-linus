//! Codificação do documento, na ordem do algoritmo de sniffing da especificação: BOM, `charset` do `Content-Type`,
//! pré-varredura de `<meta>` nos primeiros 1024 bytes e, sem nada disso, windows-1252 (o padrão do Chromium em inglês).

use encoding_rs::{Encoding, UTF_8, UTF_16BE, UTF_16LE, WINDOWS_1252};

/// Decodifica `bytes`; `content_type` é o valor do cabeçalho `Content-Type`, quando houve.
///
/// Sem declaração nenhuma o Chromium roda o detector estatístico CED, que acerta UTF-8 na maioria dos textos mas
/// erra em amostras minúsculas (um `á` isolado num comentário vira windows-1252). Aqui a regra é UTF-8 válido com
/// não-ASCII vira UTF-8, o resto windows-1252; portar o CED fica como divergência conhecida (`known-divergences.txt`).
pub fn decode(bytes: &[u8], content_type: Option<&str>) -> String {
    let guess = || if bytes.is_ascii() || std::str::from_utf8(bytes).is_err() { WINDOWS_1252 } else { UTF_8 };
    let encoding = bom(bytes).or_else(|| content_type.and_then(charset_of)).or_else(|| prescan(bytes)).unwrap_or_else(guess);
    // `decode` já descarta o BOM que casa com a codificação escolhida.
    encoding.decode(bytes).0.into_owned()
}

/// O rótulo do charset declarado no `Content-Type` (`utf-8` quando não há): o que o XHR usa no `responseText`.
pub fn label(content_type: Option<&str>) -> String {
    content_type.and_then(charset_of).unwrap_or(UTF_8).name().to_ascii_lowercase()
}

fn bom(bytes: &[u8]) -> Option<&'static Encoding> {
    Encoding::for_bom(bytes).map(|(encoding, _)| encoding)
}

/// O `charset=` de um valor de `Content-Type` ou de um `content` de `<meta http-equiv>`.
fn charset_of(value: &str) -> Option<&'static Encoding> {
    let lower = value.to_ascii_lowercase();
    let after = &value[lower.find("charset")? + "charset".len()..];
    let after = after.trim_start().strip_prefix('=')?.trim_start();
    let (quote, rest) = match after.chars().next() {
        Some(q @ ('"' | '\'')) => (Some(q), &after[1..]),
        _ => (None, after),
    };
    let end = rest.find(|c: char| Some(c) == quote || (quote.is_none() && (c.is_ascii_whitespace() || c == ';' || c == '>'))).unwrap_or(rest.len());
    let encoding = Encoding::for_label(rest[..end].trim().as_bytes())?;
    // Uma `<meta>` que declara UTF-16 já foi lida como ASCII, então vale como UTF-8.
    Some(if encoding == UTF_16LE || encoding == UTF_16BE { UTF_8 } else { encoding })
}

fn prescan(bytes: &[u8]) -> Option<&'static Encoding> {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]).into_owned();
    let lower = head.to_ascii_lowercase();
    let mut from = 0;
    while let Some(start) = lower[from..].find("<meta") {
        let start = from + start;
        let end = lower[start..].find('>').map_or(lower.len(), |e| start + e);
        if let Some(encoding) = charset_of(&head[start..end]) {
            return Some(encoding);
        }
        from = start + 5;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guesses_without_declaration() {
        assert_eq!(decode(b"caf\xe9", None), "café");
        assert_eq!(decode("é".as_bytes(), None), "é");
    }

    #[test]
    fn header_meta_and_bom() {
        assert_eq!(decode("é".as_bytes(), Some("text/html; charset=utf-8")), "é");
        assert_eq!(decode(b"<meta charset=\"utf-8\">\xc3\xa9", None), "<meta charset=\"utf-8\">é");
        assert_eq!(decode(b"<meta http-equiv=content-type content='text/html; charset=iso-8859-1'>\xe9", None), "<meta http-equiv=content-type content='text/html; charset=iso-8859-1'>é");
        assert_eq!(decode(b"\xef\xbb\xbf\xc3\xa9", None), "é");
    }
}
