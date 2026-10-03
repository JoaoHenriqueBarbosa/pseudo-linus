//! (b) Proc macro de outra crate que gera `unsafe` com span `call_site`.

pub fn as_str(bytes: &[u8]) -> &str {
    unsafe_proc_macro::utf8_unchecked_call_site!(bytes)
}
