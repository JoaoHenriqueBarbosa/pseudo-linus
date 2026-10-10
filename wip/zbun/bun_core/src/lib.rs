//! Porte para Rust seguro do subconjunto de `bun_core` (`src/bun_core` do bun 1.4.2) que o transpilador usa.
//! Os módulos mantêm nome e estrutura do upstream; o README de `wip/zbun` diz o que já entrou.
#![forbid(unsafe_code)]

pub mod comptime_string_map;
pub mod fmt;
pub mod generic_index;
pub mod strings;

pub use generic_index::{GenericIndex, GenericIndexInt, GenericIndexOptional};

/// `assert_ffi_discr!` de `src/opaque/lib.rs` do upstream (reexportado pelo `bun_core` lá): fixa em tempo de
/// compilação o tamanho, o alinhamento e os discriminantes de um enum que atravessa FFI.
#[macro_export]
macro_rules! assert_ffi_discr {
    ($T:ty, $int:ty $(; $($var:ident = $val:expr),+ $(,)?)?) => {
        const _: () = {
            ::core::assert!(
                ::core::mem::size_of::<$T>() == ::core::mem::size_of::<$int>(),
                concat!(
                    "FFI discriminant: size_of::<", stringify!($T),
                    ">() != size_of::<", stringify!($int), ">()"
                ),
            );
            ::core::assert!(
                ::core::mem::align_of::<$T>() == ::core::mem::align_of::<$int>(),
                concat!(
                    "FFI discriminant: align_of::<", stringify!($T),
                    ">() != align_of::<", stringify!($int), ">()"
                ),
            );
            $($(
                ::core::assert!(
                    <$T>::$var as $int == $val,
                    concat!(
                        "FFI discriminant: ", stringify!($T), "::",
                        stringify!($var), " != ", stringify!($val)
                    ),
                );
            )+)?
        };
    };
}

/// `bun.trim`: tira do começo e do fim os bytes que estão em `values_to_strip`.
pub fn trim<'a>(slice: &'a [u8], values_to_strip: &[u8]) -> &'a [u8] {
    let begin = slice.iter().position(|b| !values_to_strip.contains(b)).unwrap_or(slice.len());
    let end = slice.iter().rposition(|b| !values_to_strip.contains(b)).map_or(begin, |i| i + 1);
    &slice[begin..end]
}
