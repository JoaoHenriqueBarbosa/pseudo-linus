//! `macro_rules!` de outra crate que gera `#[unsafe(no_mangle)]`, atributo coberto pelo lint `unsafe_code`.

unsafe_macros::no_mangle_fn!(e06_probe_no_mangle);
