//! (c) Proc macro de outra crate que gera `unsafe` com span `mixed_site`.

pub fn as_str(bytes: &[u8]) -> &str {
    unsafe_proc_macro::utf8_unchecked_mixed_site!(bytes)
}
