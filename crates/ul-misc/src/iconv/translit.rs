//! Transliteração do `//TRANSLIT`, que a glibc tira do `LC_CTYPE` do locale corrente.
//!
//! - Locale `C`/`POSIX`: a tabela embutida do `locale/C-translit.h` (aqui o subconjunto de
//!   símbolos, ligaduras, aspas, traços e espaços; letras acentuadas não estão nela).
//! - Locale `C.UTF-8`: a tabela do arquivo de locale, que é o `translit_combining` (letra com
//!   diacrítico vira a letra base, as marcas combinantes somem); os símbolos do `C-translit` não
//!   entram.
//!
//! Nos dois, o que não tem entrada vira o `default_missing` do locale, que é `?`.

/// Substituto que falta na tabela.
pub const DEFAULT_MISSING: &str = "?";

/// As letras base de U+00C0 a U+017F; `.` é "sem entrada".
const COMBINING_LATIN: &[u8; 192] = b"AAAAAA.CEEEEIIII\
.NOOOOO.OUUUUY..\
aaaaaa.ceeeeiiii\
.nooooo.ouuuuy.y\
AaAaAaCcCcCcCcDd\
DdEeEeEeEeEeGgGg\
GgGgHhHhIiIiIiIi\
Ii..JjKk.LlLlLl.\
.LlNnNnNn...OoOo\
Oo..RrRrRrSsSsSs\
SsTtTtTtUuUuUuUu\
UuUuWwYyYZzZzZz.";

static LETTERS: [&str; 128] = {
    let mut t = [""; 128];
    let alphabet: &[&str] = &[
        "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R",
        "S", "T", "U", "V", "W", "X", "Y", "Z", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j",
        "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z",
    ];
    let mut i = 0;
    while i < 26 {
        t[b'A' as usize + i] = alphabet[i];
        t[b'a' as usize + i] = alphabet[26 + i];
        i += 1;
    }
    t
};

/// Tabela do `C.UTF-8` (`translit_combining`).
fn combining(c: u32) -> Option<&'static str> {
    if (0x0300..0x0370).contains(&c) {
        return Some("");
    }
    if (0xC0..0x180).contains(&c) {
        let b = COMBINING_LATIN[(c - 0xC0) as usize];
        if b != b'.' {
            return Some(LETTERS[usize::from(b)]);
        }
    }
    None
}

/// Tabela embutida do locale `C` (`C-translit.h`), ordenada por ponto de código.
const C_TRANSLIT: &[(u32, &str)] = &[
    (0x00A0, " "),
    (0x00A9, "(C)"),
    (0x00AB, "<<"),
    (0x00AD, "-"),
    (0x00AE, "(R)"),
    (0x00B5, "u"),
    (0x00B8, ","),
    (0x00BB, ">>"),
    (0x00BC, " 1/4 "),
    (0x00BD, " 1/2 "),
    (0x00BE, " 3/4 "),
    (0x00C6, "AE"),
    (0x00D7, "x"),
    (0x00DF, "ss"),
    (0x00E6, "ae"),
    (0x0110, "D"),
    (0x0111, "d"),
    (0x0126, "H"),
    (0x0127, "h"),
    (0x0131, "i"),
    (0x0132, "IJ"),
    (0x0133, "ij"),
    (0x0138, "q"),
    (0x013F, "L."),
    (0x0140, "l."),
    (0x0141, "L"),
    (0x0142, "l"),
    (0x0149, "'n"),
    (0x014A, "N"),
    (0x014B, "n"),
    (0x0152, "OE"),
    (0x0153, "oe"),
    (0x0166, "T"),
    (0x0167, "t"),
    (0x017F, "s"),
    (0x2002, " "),
    (0x2003, " "),
    (0x2004, " "),
    (0x2005, " "),
    (0x2006, " "),
    (0x2008, " "),
    (0x2009, " "),
    (0x200A, " "),
    (0x200B, ""),
    (0x2010, "-"),
    (0x2011, "-"),
    (0x2012, "-"),
    (0x2013, "-"),
    (0x2014, "-"),
    (0x2015, "-"),
    (0x2018, "'"),
    (0x2019, "'"),
    (0x201A, ","),
    (0x201B, "'"),
    (0x201C, "\""),
    (0x201D, "\""),
    (0x201E, ",,"),
    (0x201F, "\""),
    (0x2020, "+"),
    (0x2022, "o"),
    (0x2024, "."),
    (0x2025, ".."),
    (0x2026, "..."),
    (0x2039, "<"),
    (0x203A, ">"),
    (0x20AC, "EUR"),
    (0x2122, "TM"),
    (0x2190, "<-"),
    (0x2192, "->"),
    (0x2194, "<->"),
    (0x2212, "-"),
    (0x2215, "/"),
    (0x2216, "\\"),
    (0x2217, "*"),
    (0x2223, "|"),
    (0x2236, ":"),
    (0x223C, "~"),
    (0x2264, "<="),
    (0x2265, ">="),
    (0xFB00, "ff"),
    (0xFB01, "fi"),
    (0xFB02, "fl"),
    (0xFB03, "ffi"),
    (0xFB04, "ffl"),
];

/// O substituto de `c` no locale corrente (`utf8_locale`: `C.UTF-8`; senão, `C`).
pub fn lookup(c: u32, utf8_locale: bool) -> Option<&'static str> {
    if utf8_locale {
        combining(c)
    } else {
        C_TRANSLIT
            .binary_search_by_key(&c, |e| e.0)
            .ok()
            .map(|i| C_TRANSLIT[i].1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables() {
        assert_eq!(lookup(0xE9, true), Some("e"));
        assert_eq!(lookup(0x0142, true), Some("l"));
        assert_eq!(lookup(0x0301, true), Some(""));
        assert_eq!(lookup(0x20AC, true), None);
        assert_eq!(lookup(0x20AC, false), Some("EUR"));
        assert_eq!(lookup(0xE9, false), None);
        assert!(C_TRANSLIT.windows(2).all(|w| w[0].0 < w[1].0));
    }
}
