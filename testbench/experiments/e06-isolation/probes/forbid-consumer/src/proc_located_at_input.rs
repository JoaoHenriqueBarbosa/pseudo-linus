//! Proc macro que gera `unsafe` com contexto de `call_site` e localização da entrada.

pub fn as_str(bytes: &[u8]) -> &str {
    unsafe_proc_macro::utf8_unchecked_located_at_input!(bytes)
}
