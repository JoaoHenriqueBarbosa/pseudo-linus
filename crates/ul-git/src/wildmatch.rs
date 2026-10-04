//! Casamento de padrões de caminho (`.gitignore`, pathspec), pelo `wildmatch` do `gix-glob`.

use gix_glob::wildmatch::Mode;

/// `*` e `?` não casam `/`; `**` entre barras cruza diretórios.
pub const PATHNAME: u32 = 1;
pub const CASEFOLD: u32 = 2;

pub fn wildmatch(pattern: &[u8], text: &[u8], flags: u32) -> bool {
    let mut mode = Mode::empty();
    if flags & PATHNAME != 0 {
        mode |= Mode::NO_MATCH_SLASH_LITERAL;
    }
    if flags & CASEFOLD != 0 {
        mode |= Mode::IGNORE_CASE;
    }
    gix_glob::wildmatch(pattern.into(), text.into(), mode)
}

/// Tem caractere especial de glob?
pub fn has_glob(s: &[u8]) -> bool {
    s.iter().any(|c| matches!(c, b'*' | b'?' | b'[' | b'\\'))
}

/// Comprimento do prefixo sem curinga.
pub fn literal_prefix_len(s: &[u8]) -> usize {
    s.iter().position(|c| matches!(c, b'*' | b'?' | b'[' | b'\\')).unwrap_or(s.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basics() {
        assert!(wildmatch(b"*.rs", b"main.rs", PATHNAME));
        assert!(!wildmatch(b"*.rs", b"src/main.rs", PATHNAME));
        assert!(wildmatch(b"*.rs", b"src/main.rs", 0));
        assert!(wildmatch(b"**/foo", b"a/b/foo", PATHNAME));
        assert!(wildmatch(b"**/foo", b"foo", PATHNAME));
        assert!(wildmatch(b"a/**/b", b"a/b", PATHNAME));
        assert!(wildmatch(b"a/**/b", b"a/x/y/b", PATHNAME));
        assert!(wildmatch(b"a/**", b"a/x/y", PATHNAME));
        assert!(!wildmatch(b"a/**", b"a", PATHNAME));
        assert!(wildmatch(b"[a-c]x", b"bx", PATHNAME));
        assert!(!wildmatch(b"[!a-c]x", b"bx", PATHNAME));
        assert!(wildmatch(b"[[:digit:]]*", b"9z", PATHNAME));
        assert!(wildmatch(b"foo?bar", b"foo-bar", PATHNAME));
        assert!(!wildmatch(b"foo?bar", b"foo/bar", PATHNAME));
        assert!(wildmatch(b"\\*x", b"*x", PATHNAME));
        assert!(wildmatch(b"*/b", b"a/b", PATHNAME));
        assert!(!wildmatch(b"*/b", b"a/c/b", PATHNAME));
        assert!(wildmatch(b"FOO", b"foo", CASEFOLD));
        assert!(wildmatch(b"a*b*c", b"axxbyyc", 0));
    }
}
