//! Porte para Rust seguro do subconjunto de `bun_core` (`src/bun_core` do bun 1.4.2) que o transpilador usa.
//! Os módulos mantêm nome e estrutura do upstream; o README de `wip/zbun` diz o que já entrou.
#![forbid(unsafe_code)]

pub mod comptime_string_map;
pub mod fmt;
pub mod generic_index;
pub mod strings;

pub use generic_index::{GenericIndex, GenericIndexInt, GenericIndexOptional};

/// `bun.trim`: tira do começo e do fim os bytes que estão em `values_to_strip`.
pub fn trim<'a>(slice: &'a [u8], values_to_strip: &[u8]) -> &'a [u8] {
    let begin = slice.iter().position(|b| !values_to_strip.contains(b)).unwrap_or(slice.len());
    let end = slice.iter().rposition(|b| !values_to_strip.contains(b)).map_or(begin, |i| i + 1);
    &slice[begin..end]
}
