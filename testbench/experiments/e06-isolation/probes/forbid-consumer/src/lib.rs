//! Sonda do E06 (H20): crate com `forbid(unsafe_code)` (atributo aqui e `[lints]` no Cargo.toml).
//! Nenhum `unsafe` deste crate foi escrito à mão: cada caso só invoca uma macro de outra crate. O E06
//! compila um caso por vez (uma feature por caso) e registra se compilou ou se o lint disparou.
#![forbid(unsafe_code)]

/// Controle sem macro nenhuma: compila em todas as variantes.
pub fn baseline(bytes: &[u8]) -> usize {
    bytes.len()
}

#[cfg(feature = "macro_rules_block")]
pub mod macro_rules_block;

#[cfg(feature = "proc_call_site")]
pub mod proc_call_site;

#[cfg(feature = "proc_mixed_site")]
pub mod proc_mixed_site;

#[cfg(feature = "proc_input_span")]
pub mod proc_input_span;

#[cfg(feature = "proc_located_at_input")]
pub mod proc_located_at_input;

#[cfg(feature = "macro_rules_unsafe_impl")]
pub mod macro_rules_unsafe_impl;

#[cfg(feature = "derive_bytemuck_pod")]
pub mod derive_bytemuck_pod;

#[cfg(feature = "intrusive_adapter")]
pub mod intrusive_adapter;

#[cfg(feature = "macro_rules_allow")]
pub mod macro_rules_allow;

#[cfg(feature = "macro_rules_no_mangle")]
pub mod macro_rules_no_mangle;
