//! Controle positivo: proc macro que gera `unsafe` com o span da entrada. Para o compilador o `unsafe`
//! pertence a este crate, então o `forbid` precisa disparar (prova que o lint está ativo).

pub fn as_str(bytes: &[u8]) -> &str {
    unsafe_proc_macro::utf8_unchecked_input_span!(bytes)
}
