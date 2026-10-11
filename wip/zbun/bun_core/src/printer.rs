//! `bun_core::printer`: as constantes e os escapes `\uHHHH` que o `js_printer` (e o `quote_for_json`) usam.

use crate::fmt::hex4_upper;

pub const FIRST_ASCII: u32 = 0x20;
pub const LAST_ASCII: u32 = 0x7E;
pub const FIRST_HIGH_SURROGATE: u32 = 0xD800;
pub const LAST_LOW_SURROGATE: u32 = 0xDFFF;

/// Unidade BMP (`c <= 0xFFFF`, surrogate solto incluído) como os 6 bytes `\uHHHH`, hexadecimal maiúsculo.
#[inline]
pub const fn bmp_escape(c: u32) -> [u8; 6] {
    let h = hex4_upper(c as u16);
    [b'\\', b'u', h[0], h[1], h[2], h[3]]
}

/// Ponto de código suplementar (`c > 0xFFFF`) como o par de surrogates `\uHHHH\uHHHH`, hexadecimal maiúsculo.
#[inline]
pub const fn surrogate_pair_escape(c: u32) -> [u8; 12] {
    let [lead, trail] = crate::strings::encode_surrogate_pair(c);
    let l = hex4_upper(lead);
    let h = hex4_upper(trail);
    [b'\\', b'u', l[0], l[1], l[2], l[3], b'\\', b'u', h[0], h[1], h[2], h[3]]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_are_uppercase_hex() {
        assert_eq!(&bmp_escape(0x2028), b"\\u2028");
        assert_eq!(&bmp_escape(0xD83D), b"\\uD83D");
        assert_eq!(&surrogate_pair_escape(0x1F600), b"\\uD83D\\uDE00");
    }

    #[test]
    fn needs_escape_follows_the_quote_char() {
        use crate::strings::index_of_needs_escape_for_java_script_string as find;
        assert_eq!(find(b"", b'"'), None);
        assert_eq!(find(b"abc", b'"'), None);
        assert_eq!(find(b"ab'c", b'"'), None);
        assert_eq!(find(b"ab\"c", b'"'), Some(2));
        assert_eq!(find(b"a$b", b'"'), None);
        assert_eq!(find(b"a$b", b'`'), Some(1));
        assert_eq!(find(b"a\x07", b'\''), Some(1));
        assert_eq!(find(b"a\x7f", b'\''), Some(1));
        assert_eq!(find("é".as_bytes(), b'\''), Some(0));
    }

    #[test]
    fn identifier_key_check() {
        use crate::strings::contains_non_bmp_code_point_or_is_invalid_identifier as bad;
        assert!(bad(b""));
        assert!(bad(b"1a"));
        assert!(bad(b"a-b"));
        assert!(bad("a😀".as_bytes()));
        assert!(!bad(b"$_a1"));
        assert!(!bad("é1".as_bytes()));
    }

    #[test]
    fn wtf8_to_utf16le() {
        use crate::strings::write_wtf8_as_utf16le as conv;
        let units = |bytes: &[u8]| {
            let first = bytes.iter().position(|b| *b >= 0x80).unwrap_or(bytes.len());
            let mut out = Vec::new();
            conv(bytes, first, &mut out);
            out.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>()
        };
        assert_eq!(units(b"ab"), [0x61, 0x62]);
        assert_eq!(units("aé😀".as_bytes()), [0x61, 0xE9, 0xD83D, 0xDE00]);
        assert_eq!(units(b"a\xFFb"), [0x61, 0xFFFD, 0x62]);
        assert_eq!(units(b"a\xC3"), [0x61, 0xFFFD]);
        // Surrogate solto (WTF-8: ED A0 BD) passa como a unidade.
        assert_eq!(units(b"\xED\xA0\xBD"), [0xD83D]);
    }
}
