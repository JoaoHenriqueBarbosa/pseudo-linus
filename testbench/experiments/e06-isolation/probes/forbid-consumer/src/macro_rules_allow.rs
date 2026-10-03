//! `macro_rules!` de outra crate que gera uma função com `#[allow(unsafe_code)]` e bloco `unsafe`.

unsafe_macros::allowed_unsafe_fn!(as_str);
