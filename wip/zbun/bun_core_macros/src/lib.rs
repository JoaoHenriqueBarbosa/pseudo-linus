//! Porte dos proc-macros de `bun_core` (`src/bun_core_macros` do bun 1.4.2). Por ora só o gerador do
//! `comptime_string_map!` e do `comptime_string_set!`; `pretty_fmt!` e os derives de contagem de referência entram
//! quando um módulo portado os pedir.

use proc_macro::TokenStream;

mod comptime_string_map;

/// Implementation detail of `bun_core::comptime_string_map!`: invoke that
/// `macro_rules!` wrapper instead (it injects the `@crate_path` prefix).
#[proc_macro]
pub fn comptime_string_map_impl(input: TokenStream) -> TokenStream {
    comptime_string_map::expand_map(input.into())
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

/// Implementation detail of `bun_core::comptime_string_set!`: invoke that
/// `macro_rules!` wrapper instead (it injects the `@crate_path` prefix).
#[proc_macro]
pub fn comptime_string_set_impl(input: TokenStream) -> TokenStream {
    comptime_string_map::expand_set(input.into())
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}
