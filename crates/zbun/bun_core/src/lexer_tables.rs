//! `lexer_tables` de `src/bun_core/string/mod.rs` do upstream: a tabela das palavras reservadas do modo estrito.

crate::comptime_string_map! {
    /// The 9 strict-mode reserved words (ES2015 §11.6.2.2) mapped to the
    /// underscore-prefixed replacement used by
    /// `MutableString::ensure_valid_identifier` to mangle a name that is
    /// already a syntactically valid identifier but would collide with a
    /// strict-mode reserved word. Single source of truth:
    /// [`STRICT_MODE_RESERVED_WORDS`], [`is_strict_mode_reserved_word`],
    /// and [`strict_mode_reserved_word_remap`] all derive from it.
    static STRICT_MODE_RESERVED_WORD_REMAP: &'static [u8] = {
        b"implements" => b"_implements",
        b"interface" => b"_interface",
        b"let" => b"_let",
        b"package" => b"_package",
        b"private" => b"_private",
        b"protected" => b"_protected",
        b"public" => b"_public",
        b"static" => b"_static",
        b"yield" => b"_yield",
    };
}

/// The 9 strict-mode reserved words as a plain array, for callers that
/// only need `.len()` / `.iter()`.
pub const STRICT_MODE_RESERVED_WORDS: [&[u8]; 9] = {
    let entries = __ComptimeStringMap_STRICT_MODE_RESERVED_WORD_REMAP::ENTRIES;
    assert!(entries.len() == 9);
    let mut out: [&[u8]; 9] = [&[]; 9];
    let mut i = 0;
    while i < out.len() {
        out[i] = entries[i].0;
        i += 1;
    }
    out
};

/// Hot-path strict-mode reserved-word membership check.
#[inline]
pub fn is_strict_mode_reserved_word(s: &[u8]) -> bool {
    STRICT_MODE_RESERVED_WORD_REMAP.contains_key(s)
}

/// Underscore-prefixed replacement for a strict-mode reserved word
/// (`b"let"` → `b"_let"`); `None` for any other input.
#[inline]
pub fn strict_mode_reserved_word_remap(s: &[u8]) -> Option<&'static [u8]> {
    STRICT_MODE_RESERVED_WORD_REMAP.get(s).copied()
}
