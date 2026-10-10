//! Porte para Rust seguro do subconjunto de `bun_collections` (`src/collections` do bun 1.4.2) que o transpilador
//! usa. Os módulos mantêm nome e estrutura do upstream; o README de `wip/zbun` diz o que já entrou.
#![forbid(unsafe_code)]

pub mod array_hash_map;
pub mod bit_set;

pub use array_hash_map::{AllocError, ArrayHashMap, OwnKey, StringArrayHashMap};
pub use bit_set::IntegerBitSet;
