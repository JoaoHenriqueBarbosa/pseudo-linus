//! Sequências de escape do GNU sed (manual, seção "Escapes"): `\a \f \n \r \t \v`, `\cX`, `\dNNN`,
//! `\oNNN`, `\xHH`. O efeito depende do contexto, como medido no sed 4.9:
//!
//! - regex: o caractere produzido entra cru no padrão (`\x5e` vira a âncora `^`, `\x5c` sozinho é
//!   "Trailing backslash"); os demais `\X` ficam pro parser da regex, inclusive dentro de colchetes
//!   (`[\t]` casa tab, `[\n]` casa newline).
//! - substituição: `&` e `\` produzidos ficam literais (`\x26` dá `&`); os demais `\X` ficam pro
//!   parser da substituição.
//! - texto de `a`, `i`, `c`: `\X` desconhecido vira `X`.
//! - `y`: igual ao texto (`\b` vira `b`, `\\` vira `\`).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Context {
    Regex,
    Replacement,
    Text,
    Translit,
}

/// Valor de `\dNNN`, `\oNNN`, `\xHH`: dígitos enquanto a base elevada cabe em 255 (3 decimais, 3
/// octais, 2 hexadecimais). Devolve (byte, dígitos consumidos).
fn number(s: &[u8], base: u32) -> (u8, usize) {
    let mut n: u32 = 0;
    let mut max: u32 = 1;
    let mut k = 0;
    while k < s.len() && max <= 255 {
        let d = match s[k] {
            c @ b'0'..=b'9' => (c - b'0') as u32,
            c @ b'a'..=b'f' => (c - b'a' + 10) as u32,
            c @ b'A'..=b'F' => (c - b'A' + 10) as u32,
            _ => break,
        };
        if d >= base {
            break;
        }
        n = n * base + d;
        k += 1;
        max *= base;
    }
    ((n & 0xff) as u8, k)
}

/// Aplica os escapes de `text` no contexto dado.
pub fn convert(text: &[u8], ctx: Context) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        let c = text[i];
        if c != b'\\' || i + 1 >= text.len() {
            if c == b'\\' && matches!(ctx, Context::Text | Context::Translit) {
                // Barra no fim: some.
                i += 1;
                continue;
            }
            out.push(c);
            i += 1;
            continue;
        }
        let e = text[i + 1];
        let simple = match e {
            b'a' => Some(0x07),
            b'f' => Some(0x0c),
            b'n' => Some(b'\n'),
            b'r' => Some(b'\r'),
            b't' => Some(b'\t'),
            b'v' => Some(0x0b),
            _ => None,
        };
        let produced: Option<(u8, usize)> = if let Some(ch) = simple {
            Some((ch, 2))
        } else {
            match e {
                b'd' | b'o' | b'x' => {
                    let base = match e {
                        b'd' => 10,
                        b'o' => 8,
                        _ => 16,
                    };
                    let (v, k) = number(&text[i + 2..], base);
                    if k == 0 { None } else { Some((v, 2 + k)) }
                }
                b'c' if i + 2 < text.len() => {
                    let x = text[i + 2].to_ascii_uppercase() ^ 0x40;
                    // `\c\\` é Control-\ e consome as duas barras.
                    let len = if text[i + 2] == b'\\' && text.get(i + 3) == Some(&b'\\') { 4 } else { 3 };
                    Some((x, len))
                }
                _ => None,
            }
        };
        match produced {
            Some((ch, len)) => {
                if ctx == Context::Replacement && (ch == b'&' || ch == b'\\') {
                    out.push(b'\\');
                }
                out.push(ch);
                i += len;
            }
            None => {
                match ctx {
                    Context::Regex | Context::Replacement => {
                        out.push(b'\\');
                        out.push(e);
                    }
                    Context::Text | Context::Translit => out.push(e),
                }
                i += 2;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contexts() {
        assert_eq!(convert(br"a\tb\n", Context::Regex), b"a\tb\n");
        assert_eq!(convert(br"\x41\d066\o103", Context::Regex), b"ABC");
        assert_eq!(convert(br"\.\(", Context::Regex), br"\.\(");
        assert_eq!(convert(br"\x5e", Context::Regex), b"^");
        assert_eq!(convert(br"\x26\x5c", Context::Replacement), br"\&\\");
        assert_eq!(convert(br"\1\&\U", Context::Replacement), br"\1\&\U");
        assert_eq!(convert(br"\dx", Context::Regex), br"\dx");
        assert_eq!(convert(br"\x414243", Context::Text), b"A4243");
        assert_eq!(convert(br"\cz", Context::Text), b"\x1a");
        assert_eq!(convert(br"a\b\\c", Context::Translit), b"ab\\c");
        assert_eq!(convert(br"foo\", Context::Text), b"foo");
    }
}
