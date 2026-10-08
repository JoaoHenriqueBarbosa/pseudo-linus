//! `parser/LexerUnicodeProperties.{h,cpp}` com a tabela gerada `LexerUnicodePropertyTables.h`
//! (`generateLexerUnicodePropertyTables.py` sobre o `ucd/` 17.0 do JavaScriptCore).

/// `isNonLatin1WhiteSpace`: os espaços Zs fora do Latin1, mais o BOM (U+FEFF), que o ECMAScript
/// também conta como espaço.
pub fn is_non_latin1_white_space(ch: u16) -> bool {
    ch == 0xFEFF
        || ch == 0x1680
        || (0x2000..=0x200A).contains(&ch)
        || ch == 0x202F
        || ch == 0x205F
        || ch == 0x3000
}
