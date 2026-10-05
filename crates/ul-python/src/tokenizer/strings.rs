//! Decodificação dos literais de string e de bytes, como o `Parser/string_parser.c` do CPython 3.13.
//!
//! Para `str`, o C não decodifica o fonte direto: `decode_unicode_with_escapes` primeiro reescreve
//! cada caractere não ASCII como `\UXXXXXXXX` (e uma barra seguida de não ASCII como `\`) e só
//! então chama o codec `unicode_escape`. Por isso as posições das mensagens de erro ("can't decode
//! bytes in position A-B") contam bytes desse buffer reescrito, e este módulo monta o mesmo buffer.
//! Para `bytes`, o caminho é o `_PyBytes_DecodeEscape2`.
//!
//! Só o primeiro escape inválido vira `SyntaxWarning`, como no C.

use std::fmt;

/// Prefixos que mudam a decodificação.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StringFlags {
    /// `b`/`B`.
    pub bytes: bool,
    /// `r`/`R`: nenhum escape é interpretado.
    pub raw: bool,
    /// Pedaço de f-string (FSTRING_MIDDLE): `\{` e `\}` já foram avisados pelo tokenizer.
    pub fstring: bool,
}

/// Valor do literal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    /// Pontos de código. Um `str` do Python pode conter surrogates soltos (`'\ud800'`), que `String`
    /// não representa, então o valor fica como `u32`.
    Str(Vec<u32>),
    Bytes(Vec<u8>),
}

impl Value {
    /// O `str` como `String`, trocando surrogates soltos por U+FFFD; bytes viram Latin-1.
    pub fn to_string_lossy(&self) -> String {
        match self {
            Value::Str(cps) => cps.iter().map(|&n| char::from_u32(n).unwrap_or('\u{FFFD}')).collect(),
            Value::Bytes(b) => b.iter().map(|&x| char::from(x)).collect(),
        }
    }
}

/// Literal decodificado e o `SyntaxWarning` do primeiro escape inválido, se houver.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decoded {
    pub value: Value,
    pub warning: Option<String>,
}

/// Erro de decodificação, com a mensagem exata do `SyntaxError` (já com o "(unicode error) " ou o
/// "(value error) " que o parser acrescenta).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodeError {
    pub msg: String,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.msg)
    }
}

/// Decodifica o corpo de um literal (o texto entre as aspas).
pub fn decode_string(body: &str, flags: StringFlags) -> Result<Decoded, DecodeError> {
    if flags.bytes {
        if !body.is_ascii() {
            return Err(DecodeError { msg: "bytes can only contain ASCII literal characters".to_string() });
        }
        if flags.raw {
            return Ok(Decoded { value: Value::Bytes(body.as_bytes().to_vec()), warning: None });
        }
        let buf = body.as_bytes();
        let (bytes, first_invalid) = decode_bytes_escape(buf)?;
        let warning = first_invalid.and_then(|i| escape_warning(buf, i, false));
        return Ok(Decoded { value: Value::Bytes(bytes), warning });
    }
    if flags.raw {
        return Ok(Decoded { value: Value::Str(body.chars().map(u32::from).collect()), warning: None });
    }
    let buf = ascii_buffer(body);
    let (cps, first_invalid) = decode_unicode_escape(&buf)?;
    let warning = first_invalid.and_then(|i| escape_warning(&buf, i, flags.fstring));
    Ok(Decoded { value: Value::Str(cps), warning })
}

/// Decodifica o texto completo de um token STRING (prefixo, aspas e corpo).
pub fn parse_literal(text: &str) -> Result<Decoded, DecodeError> {
    let mut flags = StringFlags::default();
    let mut prefix_len = 0;
    for c in text.chars() {
        match c {
            'b' | 'B' => flags.bytes = true,
            'r' | 'R' => flags.raw = true,
            'f' | 'F' => flags.fstring = true,
            'u' | 'U' => {}
            _ => break,
        }
        prefix_len += 1;
    }
    let rest = &text[prefix_len..];
    let quote = rest.chars().next().unwrap_or('\'');
    let triple: String = [quote; 3].iter().collect();
    let quote_len = if rest.len() >= 6 && rest.starts_with(&triple) && rest.ends_with(&triple) { 3 } else { 1 };
    let body = rest.get(quote_len..rest.len().saturating_sub(quote_len)).unwrap_or("");
    decode_string(body, flags)
}

/// O buffer que `decode_unicode_with_escapes` entrega ao codec: não ASCII vira `\U%08x` e uma barra
/// no fim ou antes de não ASCII vira `\`.
fn ascii_buffer(body: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len());
    let mut it = body.chars().peekable();
    while let Some(mut c) = it.next() {
        if c == '\\' {
            out.push(b'\\');
            match it.peek() {
                None => {
                    out.extend_from_slice(b"u005c");
                    break;
                }
                Some(&n) => {
                    if !n.is_ascii() {
                        out.extend_from_slice(b"u005c");
                    }
                    c = n;
                    it.next();
                }
            }
        }
        if c.is_ascii() {
            out.push(c as u8);
        } else {
            out.extend_from_slice(format!("\\U{:08x}", c as u32).as_bytes());
        }
    }
    out
}

/// Mensagem do `UnicodeDecodeError` que o parser embrulha em `SyntaxError`.
fn unicode_error(buf: &[u8], start: usize, end: usize, reason: &str) -> DecodeError {
    let detail = if end - start == 1 {
        format!("can't decode byte 0x{:02x} in position {start}: {reason}", buf[start])
    } else {
        format!("can't decode bytes in position {start}-{}: {reason}", end - 1)
    };
    DecodeError { msg: format!("(unicode error) 'unicodeescape' codec {detail}") }
}

/// `_PyUnicode_DecodeUnicodeEscapeInternal` sobre o buffer ASCII; devolve também o índice do
/// primeiro escape inválido.
fn decode_unicode_escape(buf: &[u8]) -> Result<(Vec<u32>, Option<usize>), DecodeError> {
    let end = buf.len();
    let mut out = Vec::with_capacity(end);
    let mut first_invalid = None;
    let mut s = 0;
    while s < end {
        if buf[s] != b'\\' {
            out.push(u32::from(buf[s]));
            s += 1;
            continue;
        }
        let start = s;
        s += 1;
        if s >= end {
            return Err(unicode_error(buf, start, s, "\\ at end of string"));
        }
        let c = buf[s];
        s += 1;
        let simple = match c {
            b'\n' => Some(None),
            b'\\' => Some(Some(b'\\')),
            b'\'' => Some(Some(b'\'')),
            b'"' => Some(Some(b'"')),
            b'b' => Some(Some(0x08)),
            b'f' => Some(Some(0x0c)),
            b't' => Some(Some(b'\t')),
            b'n' => Some(Some(b'\n')),
            b'r' => Some(Some(b'\r')),
            b'v' => Some(Some(0x0b)),
            b'a' => Some(Some(0x07)),
            _ => None,
        };
        if let Some(ch) = simple {
            out.extend(ch.map(u32::from));
            continue;
        }
        match c {
            b'0'..=b'7' => {
                let mut ch = u32::from(c - b'0');
                for _ in 0..2 {
                    match buf.get(s) {
                        Some(&d @ b'0'..=b'7') => {
                            ch = (ch << 3) + u32::from(d - b'0');
                            s += 1;
                        }
                        _ => break,
                    }
                }
                if ch > 0o377 && first_invalid.is_none() {
                    first_invalid = Some(s - 3);
                }
                out.push(ch);
            }
            b'x' | b'u' | b'U' => {
                let (count, reason) = match c {
                    b'x' => (2, "truncated \\xXX escape"),
                    b'u' => (4, "truncated \\uXXXX escape"),
                    _ => (8, "truncated \\UXXXXXXXX escape"),
                };
                let mut ch = 0u32;
                for _ in 0..count {
                    let Some(d) = buf.get(s).and_then(|&b| char::from(b).to_digit(16)) else {
                        return Err(unicode_error(buf, start, s, reason));
                    };
                    ch = (ch << 4) + d;
                    s += 1;
                }
                if ch > 0x10FFFF {
                    return Err(unicode_error(buf, start, s, "illegal Unicode character"));
                }
                out.push(ch);
            }
            b'N' => {
                let malformed = "malformed \\N character escape";
                if buf.get(s) != Some(&b'{') {
                    return Err(unicode_error(buf, start, s, malformed));
                }
                s += 1;
                let name_start = s;
                while s < end && buf[s] != b'}' {
                    s += 1;
                }
                if s >= end || s == name_start {
                    return Err(unicode_error(buf, start, s, malformed));
                }
                let name = &buf[name_start..s];
                s += 1;
                let Some(ch) = lookup_name(name) else {
                    return Err(unicode_error(buf, start, s, "unknown Unicode character name"));
                };
                out.push(ch);
            }
            _ => {
                if first_invalid.is_none() {
                    first_invalid = Some(s - 1);
                }
                out.push(u32::from(b'\\'));
                out.push(u32::from(c));
            }
        }
    }
    Ok((out, first_invalid))
}

/// `_PyBytes_DecodeEscape2` com `errors` estrito.
fn decode_bytes_escape(buf: &[u8]) -> Result<(Vec<u8>, Option<usize>), DecodeError> {
    let end = buf.len();
    let mut out = Vec::with_capacity(end);
    let mut first_invalid = None;
    let mut s = 0;
    while s < end {
        if buf[s] != b'\\' {
            out.push(buf[s]);
            s += 1;
            continue;
        }
        s += 1;
        if s >= end {
            return Err(DecodeError { msg: "(value error) Trailing \\ in string".to_string() });
        }
        let c = buf[s];
        s += 1;
        match c {
            b'\n' => {}
            b'\\' | b'\'' | b'"' => out.push(c),
            b'b' => out.push(0x08),
            b'f' => out.push(0x0c),
            b't' => out.push(b'\t'),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b'v' => out.push(0x0b),
            b'a' => out.push(0x07),
            b'0'..=b'7' => {
                let mut ch = u32::from(c - b'0');
                for _ in 0..2 {
                    match buf.get(s) {
                        Some(&d @ b'0'..=b'7') => {
                            ch = (ch << 3) + u32::from(d - b'0');
                            s += 1;
                        }
                        _ => break,
                    }
                }
                if ch > 0o377 && first_invalid.is_none() {
                    first_invalid = Some(s - 3);
                }
                // O C grava num `char`: fica o byte baixo.
                out.push((ch & 0xFF) as u8);
            }
            b'x' => {
                let hex = |i: usize| buf.get(i).and_then(|&b| char::from(b).to_digit(16));
                if s + 1 < end
                    && let (Some(d1), Some(d2)) = (hex(s), hex(s + 1)) {
                        out.push(((d1 << 4) + d2) as u8);
                        s += 2;
                        continue;
                    }
                return Err(DecodeError { msg: format!("(value error) invalid \\x escape at position {}", s - 2) });
            }
            _ => {
                if first_invalid.is_none() {
                    first_invalid = Some(s - 1);
                }
                out.push(b'\\');
                out.push(c);
            }
        }
    }
    Ok((out, first_invalid))
}

/// `warn_invalid_escape_sequence` do `string_parser.c`.
fn escape_warning(buf: &[u8], i: usize, fstring: bool) -> Option<String> {
    let c = buf[i];
    if fstring && (c == b'{' || c == b'}') {
        return None;
    }
    if (b'4'..=b'7').contains(&c) {
        let digits = String::from_utf8_lossy(&buf[i..(i + 3).min(buf.len())]).into_owned();
        Some(format!("invalid octal escape sequence '\\{digits}'"))
    } else {
        Some(format!("invalid escape sequence '\\{}'", char::from(c)))
    }
}

/// Nomes aceitos por `\N{...}` além dos gerados (letras latinas, dígitos e ideogramas CJK). A busca
/// não diferencia maiúsculas, como a do `unicodedata`; inclui os aliases mais comuns.
const NAMES: &[(&str, u32)] = &[
    ("NULL", 0x00),
    ("CHARACTER TABULATION", 0x09),
    ("LINE FEED", 0x0A),
    ("LINE TABULATION", 0x0B),
    ("FORM FEED", 0x0C),
    ("CARRIAGE RETURN", 0x0D),
    ("ESCAPE", 0x1B),
    ("SPACE", 0x20),
    ("EXCLAMATION MARK", 0x21),
    ("QUOTATION MARK", 0x22),
    ("NUMBER SIGN", 0x23),
    ("DOLLAR SIGN", 0x24),
    ("PERCENT SIGN", 0x25),
    ("AMPERSAND", 0x26),
    ("APOSTROPHE", 0x27),
    ("LEFT PARENTHESIS", 0x28),
    ("RIGHT PARENTHESIS", 0x29),
    ("ASTERISK", 0x2A),
    ("PLUS SIGN", 0x2B),
    ("COMMA", 0x2C),
    ("HYPHEN-MINUS", 0x2D),
    ("FULL STOP", 0x2E),
    ("SOLIDUS", 0x2F),
    ("COLON", 0x3A),
    ("SEMICOLON", 0x3B),
    ("LESS-THAN SIGN", 0x3C),
    ("EQUALS SIGN", 0x3D),
    ("GREATER-THAN SIGN", 0x3E),
    ("QUESTION MARK", 0x3F),
    ("COMMERCIAL AT", 0x40),
    ("LEFT SQUARE BRACKET", 0x5B),
    ("REVERSE SOLIDUS", 0x5C),
    ("RIGHT SQUARE BRACKET", 0x5D),
    ("CIRCUMFLEX ACCENT", 0x5E),
    ("LOW LINE", 0x5F),
    ("GRAVE ACCENT", 0x60),
    ("LEFT CURLY BRACKET", 0x7B),
    ("VERTICAL LINE", 0x7C),
    ("RIGHT CURLY BRACKET", 0x7D),
    ("TILDE", 0x7E),
    ("DELETE", 0x7F),
    ("NO-BREAK SPACE", 0xA0),
    ("SECTION SIGN", 0xA7),
    ("COPYRIGHT SIGN", 0xA9),
    ("REGISTERED SIGN", 0xAE),
    ("DEGREE SIGN", 0xB0),
    ("PLUS-MINUS SIGN", 0xB1),
    ("MICRO SIGN", 0xB5),
    ("PILCROW SIGN", 0xB6),
    ("MIDDLE DOT", 0xB7),
    ("MULTIPLICATION SIGN", 0xD7),
    ("LATIN SMALL LETTER SHARP S", 0xDF),
    ("LATIN SMALL LETTER A WITH GRAVE", 0xE0),
    ("LATIN SMALL LETTER A WITH ACUTE", 0xE1),
    ("LATIN SMALL LETTER A WITH CIRCUMFLEX", 0xE2),
    ("LATIN SMALL LETTER A WITH TILDE", 0xE3),
    ("LATIN SMALL LETTER A WITH DIAERESIS", 0xE4),
    ("LATIN SMALL LETTER C WITH CEDILLA", 0xE7),
    ("LATIN SMALL LETTER E WITH ACUTE", 0xE9),
    ("LATIN SMALL LETTER E WITH CIRCUMFLEX", 0xEA),
    ("LATIN SMALL LETTER I WITH ACUTE", 0xED),
    ("LATIN SMALL LETTER N WITH TILDE", 0xF1),
    ("LATIN SMALL LETTER O WITH ACUTE", 0xF3),
    ("LATIN SMALL LETTER O WITH CIRCUMFLEX", 0xF4),
    ("LATIN SMALL LETTER O WITH TILDE", 0xF5),
    ("LATIN SMALL LETTER O WITH DIAERESIS", 0xF6),
    ("DIVISION SIGN", 0xF7),
    ("LATIN SMALL LETTER U WITH ACUTE", 0xFA),
    ("LATIN SMALL LETTER U WITH DIAERESIS", 0xFC),
    ("LATIN CAPITAL LETTER A WITH ACUTE", 0xC1),
    ("LATIN CAPITAL LETTER C WITH CEDILLA", 0xC7),
    ("LATIN CAPITAL LETTER E WITH ACUTE", 0xC9),
    ("GREEK CAPITAL LETTER DELTA", 0x394),
    ("GREEK CAPITAL LETTER SIGMA", 0x3A3),
    ("GREEK CAPITAL LETTER OMEGA", 0x3A9),
    ("GREEK SMALL LETTER ALPHA", 0x3B1),
    ("GREEK SMALL LETTER BETA", 0x3B2),
    ("GREEK SMALL LETTER GAMMA", 0x3B3),
    ("GREEK SMALL LETTER DELTA", 0x3B4),
    ("GREEK SMALL LETTER EPSILON", 0x3B5),
    ("GREEK SMALL LETTER THETA", 0x3B8),
    ("GREEK SMALL LETTER LAMDA", 0x3BB),
    ("GREEK SMALL LETTER MU", 0x3BC),
    ("GREEK SMALL LETTER PI", 0x3C0),
    ("GREEK SMALL LETTER SIGMA", 0x3C3),
    ("GREEK SMALL LETTER OMEGA", 0x3C9),
    ("ZERO WIDTH SPACE", 0x200B),
    ("ZERO WIDTH NON-JOINER", 0x200C),
    ("ZERO WIDTH JOINER", 0x200D),
    ("EN DASH", 0x2013),
    ("EM DASH", 0x2014),
    ("LEFT SINGLE QUOTATION MARK", 0x2018),
    ("RIGHT SINGLE QUOTATION MARK", 0x2019),
    ("LEFT DOUBLE QUOTATION MARK", 0x201C),
    ("RIGHT DOUBLE QUOTATION MARK", 0x201D),
    ("DAGGER", 0x2020),
    ("BULLET", 0x2022),
    ("HORIZONTAL ELLIPSIS", 0x2026),
    ("EURO SIGN", 0x20AC),
    ("TRADE MARK SIGN", 0x2122),
    ("LEFTWARDS ARROW", 0x2190),
    ("UPWARDS ARROW", 0x2191),
    ("RIGHTWARDS ARROW", 0x2192),
    ("DOWNWARDS ARROW", 0x2193),
    ("INFINITY", 0x221E),
    ("NOT EQUAL TO", 0x2260),
    ("LESS-THAN OR EQUAL TO", 0x2264),
    ("GREATER-THAN OR EQUAL TO", 0x2265),
    ("BLACK STAR", 0x2605),
    ("WHITE STAR", 0x2606),
    ("SNOWMAN", 0x2603),
    ("BLACK HEART SUIT", 0x2665),
    ("CHECK MARK", 0x2713),
    ("HEAVY CHECK MARK", 0x2714),
    ("BYTE ORDER MARK", 0xFEFF),
    ("ZERO WIDTH NO-BREAK SPACE", 0xFEFF),
    ("REPLACEMENT CHARACTER", 0xFFFD),
    ("SNAKE", 0x1F40D),
    ("THUMBS UP SIGN", 0x1F44D),
    ("PILE OF POO", 0x1F4A9),
    ("GRINNING FACE", 0x1F600),
];

const DIGIT_NAMES: [&str; 10] = ["ZERO", "ONE", "TWO", "THREE", "FOUR", "FIVE", "SIX", "SEVEN", "EIGHT", "NINE"];

/// `getcode` do `unicodedata`, restrito a `NAMES` e aos nomes gerados por regra.
fn lookup_name(name: &[u8]) -> Option<u32> {
    let name = String::from_utf8_lossy(name).to_ascii_uppercase();
    if let Some(&(_, cp)) = NAMES.iter().find(|(n, _)| *n == name) {
        return Some(cp);
    }
    for (prefix, base) in [("LATIN CAPITAL LETTER ", b'A'), ("LATIN SMALL LETTER ", b'a')] {
        if let Some(rest) = name.strip_prefix(prefix)
            && rest.len() == 1 && rest.as_bytes()[0].is_ascii_uppercase() {
                return Some(u32::from(base + (rest.as_bytes()[0] - b'A')));
            }
    }
    if let Some(rest) = name.strip_prefix("DIGIT ") {
        return DIGIT_NAMES.iter().position(|d| *d == rest).map(|i| 0x30 + i as u32);
    }
    if let Some(hex) = name.strip_prefix("CJK UNIFIED IDEOGRAPH-")
        && (4..=5).contains(&hex.len()) && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            let cp = u32::from_str_radix(hex, 16).ok()?;
            let ranges = [(0x3400, 0x4DBF), (0x4E00, 0x9FFF), (0x20000, 0x2A6DF), (0x2A700, 0x2B739)];
            if ranges.iter().any(|&(a, b)| (a..=b).contains(&cp)) {
                return Some(cp);
            }
        }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(body: &str) -> Decoded {
        decode_string(body, StringFlags::default()).unwrap()
    }

    fn cps(text: &str) -> Value {
        Value::Str(text.chars().map(u32::from).collect())
    }

    fn err(body: &str, flags: StringFlags) -> String {
        decode_string(body, flags).unwrap_err().msg
    }

    #[test]
    fn simple_and_numeric_escapes() {
        let d = s("a\\n\\t\\x41\\u00e9\\U0001F600\\101\\0\\\nz\\'\\\"\\\\");
        assert_eq!(d.value, cps("a\n\tA\u{e9}\u{1F600}A\0z'\"\\"));
        assert_eq!(d.warning, None);
        assert_eq!(s("\\ud800").value, Value::Str(vec![0xD800]));
        assert_eq!(s("\u{e9}\\n").value, cps("\u{e9}\n"));
    }

    #[test]
    fn named_escapes() {
        assert_eq!(s("\\N{BULLET}\\N{bullet}\\N{LATIN SMALL LETTER Q}\\N{DIGIT SEVEN}").value, cps("\u{2022}\u{2022}q7"));
        assert_eq!(s("\\N{CJK UNIFIED IDEOGRAPH-4E2D}").value, cps("\u{4E2D}"));
        let flags = StringFlags::default();
        assert_eq!(
            err("\\N{NOPE}", flags),
            "(unicode error) 'unicodeescape' codec can't decode bytes in position 0-7: unknown Unicode character name"
        );
        assert_eq!(
            err("\\N{}", flags),
            "(unicode error) 'unicodeescape' codec can't decode bytes in position 0-2: malformed \\N character escape"
        );
        assert_eq!(
            err("\\N", flags),
            "(unicode error) 'unicodeescape' codec can't decode bytes in position 0-1: malformed \\N character escape"
        );
    }

    #[test]
    fn truncated_escapes_count_rewritten_bytes() {
        let flags = StringFlags::default();
        assert_eq!(
            err("\\x4", flags),
            "(unicode error) 'unicodeescape' codec can't decode bytes in position 0-2: truncated \\xXX escape"
        );
        // O `é` vira `\U000000e9` (10 bytes) antes do codec.
        assert_eq!(
            err("\u{e9}\\xg", flags),
            "(unicode error) 'unicodeescape' codec can't decode bytes in position 10-11: truncated \\xXX escape"
        );
        assert_eq!(
            err("\\U00110000", flags),
            "(unicode error) 'unicodeescape' codec can't decode bytes in position 0-9: illegal Unicode character"
        );
    }

    #[test]
    fn invalid_escape_warnings() {
        let d = s("\\d\\q");
        assert_eq!(d.value, cps("\\d\\q"));
        assert_eq!(d.warning.as_deref(), Some("invalid escape sequence '\\d'"));
        let d = s("\\777");
        assert_eq!(d.value, Value::Str(vec![0o777]));
        assert_eq!(d.warning.as_deref(), Some("invalid octal escape sequence '\\777'"));
        assert_eq!(s("\\8").warning.as_deref(), Some("invalid escape sequence '\\8'"));
        let f = StringFlags { fstring: true, ..StringFlags::default() };
        assert_eq!(decode_string("\\{", f).unwrap().warning, None);
        let r = StringFlags { raw: true, ..StringFlags::default() };
        let d = decode_string("\\d", r).unwrap();
        assert_eq!((d.value, d.warning), (cps("\\d"), None));
    }

    #[test]
    fn bytes_literals() {
        let b = StringFlags { bytes: true, ..StringFlags::default() };
        let d = decode_string("\\xff\\101\\n\\u", b).unwrap();
        assert_eq!(d.value, Value::Bytes(vec![0xFF, b'A', b'\n', b'\\', b'u']));
        assert_eq!(d.warning.as_deref(), Some("invalid escape sequence '\\u'"));
        assert_eq!(err("ab\\x4g", b), "(value error) invalid \\x escape at position 2");
        assert_eq!(err("\u{e9}", b), "bytes can only contain ASCII literal characters");
        assert_eq!(decode_string("\\777", b).unwrap().value, Value::Bytes(vec![0xFF]));
    }

    #[test]
    fn whole_token_text() {
        assert_eq!(parse_literal("rb'\\x'").unwrap().value, Value::Bytes(b"\\x".to_vec()));
        assert_eq!(parse_literal("'''a\\nb'''").unwrap().value, cps("a\nb"));
        assert_eq!(parse_literal("U\"\"").unwrap().value, cps(""));
        assert_eq!(parse_literal("''").unwrap().value, cps(""));
    }
}
