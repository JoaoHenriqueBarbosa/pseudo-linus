//! Porte para Rust seguro do subconjunto de `bun_core` (`src/bun_core` do bun 1.4.2) que o transpilador usa.
//! Os módulos mantêm nome e estrutura do upstream; o README de `wip/zbun` diz o que já entrou.
#![forbid(unsafe_code)]

pub mod comptime_string_map;
