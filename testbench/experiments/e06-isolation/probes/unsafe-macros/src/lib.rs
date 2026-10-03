//! Sonda do E06 (H20): `macro_rules!` exportadas que expandem pra código `unsafe` dentro da crate que
//! chama. Esta crate não contém código unsafe compilado: o corpo de uma macro só vira código na
//! expansão, por isso ela mesma passa com `unsafe_code = "forbid"`.
//!
//! Toda expansão usa uma operação que exige `unsafe` de verdade (`from_utf8_unchecked`, `unsafe impl
//! Send`, `#[unsafe(no_mangle)]`): se o bloco não fosse unsafe, nem compilaria.

/// (a) Bloco `unsafe` gerado por `macro_rules!` de outra crate.
#[macro_export]
macro_rules! utf8_unchecked {
    ($bytes:expr) => {
        unsafe { ::core::str::from_utf8_unchecked($bytes) }
    };
}

/// (d) `unsafe impl Send` gerado por `macro_rules!` de outra crate.
#[macro_export]
macro_rules! impl_send {
    ($ty:ty) => {
        unsafe impl ::core::marker::Send for $ty {}
    };
}

/// Função inteira com `#[allow(unsafe_code)]` gerada pela macro: testa se a macro consegue rebaixar o
/// `forbid` de quem chama.
#[macro_export]
macro_rules! allowed_unsafe_fn {
    ($name:ident) => {
        #[allow(unsafe_code)]
        pub fn $name(bytes: &[u8]) -> &str {
            unsafe { ::core::str::from_utf8_unchecked(bytes) }
        }
    };
}

/// Atributo unsafe (`#[unsafe(no_mangle)]`), que o lint `unsafe_code` também cobre.
#[macro_export]
macro_rules! no_mangle_fn {
    ($name:ident) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn $name() -> u32 {
            42
        }
    };
}
