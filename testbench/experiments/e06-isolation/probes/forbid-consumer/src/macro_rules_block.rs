//! (a) `macro_rules!` de outra crate que expande pra bloco `unsafe`.

pub fn as_str(bytes: &[u8]) -> &str {
    unsafe_macros::utf8_unchecked!(bytes)
}
