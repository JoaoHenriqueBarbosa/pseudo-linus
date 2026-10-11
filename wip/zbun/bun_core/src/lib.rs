//! Porte para Rust seguro do subconjunto de `bun_core` (`src/bun_core` do bun 1.4.2) que o transpilador usa.
//! Os módulos mantêm nome e estrutura do upstream; o README de `wip/zbun` diz o que já entrou.
#![forbid(unsafe_code)]

pub mod comptime_string_map;
pub mod error;
pub use error::{Error, Error as CrateError, Result as CrateResult};
pub mod feature_flags;
pub mod fmt;
pub mod generic_index;
pub mod identifier;
pub mod io;
pub mod lexer_tables;
pub mod mutable_string;
pub mod output;
pub mod printer;
pub mod stack_check;
pub mod strings;

pub use feature_flags as FeatureFlags;
pub use mutable_string::MutableString;

pub use output as Output;
pub use stack_check::StackCheck;

pub use strings::CodePoint;

pub use fmt::{ParseIntError, parse_int};

pub use generic_index::{GenericIndex, GenericIndexInt, GenericIndexOptional};

/// `bun_core::wtf`: no upstream reexporta o `parse_double` de `fmt`.
pub mod wtf {
    pub use crate::fmt::parse_double;
}

/// `bun.Environment`, só com o que o transpilador consulta. O porte roda nativo (não é o alvo wasm do upstream).
pub mod env {
    pub const IS_NATIVE: bool = true;
}
pub use env as Environment;

/// Stamp out `impl Display + impl Error` for one or more `strum::IntoStaticStr`-deriving error enums whose
/// user-facing string is exactly the variant tag.
#[macro_export]
macro_rules! impl_tag_error {
    ($($t:ty),+ $(,)?) => {$(
        impl ::core::fmt::Display for $t {
            #[inline]
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                f.write_str(<&'static str>::from(self))
            }
        }
        impl ::core::error::Error for $t {}
    )+};
}

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

pub use bun_alloc::out_of_memory;

/// Unwrap a `Result`, calling `outOfMemory()` on **any** `Err`.
#[inline]
#[track_caller]
pub fn handle_oom<T, E>(r: core::result::Result<T, E>) -> T {
    match r {
        Ok(v) => v,
        Err(_) => out_of_memory(),
    }
}

/// `bun.perf`. No upstream, `trace` emite um intervalo do ftrace quando `BUN_TRACE=1` e o kernel aceita o
/// `Bun__linux_trace_init` (FFI em C). Esse backend fica fora do porte, e o caminho que sobra é o do upstream
/// com o rastreio desligado: o `Ctx` vazio.
pub mod perf {
    pub struct Ctx {}

    impl Ctx {
        pub(crate) const DISABLED: Ctx = Ctx {};
        #[inline]
        pub fn end(&mut self) {}
    }

    #[inline]
    pub fn trace(name: &'static str) -> Ctx {
        let _ = name;
        Ctx::DISABLED
    }
}
