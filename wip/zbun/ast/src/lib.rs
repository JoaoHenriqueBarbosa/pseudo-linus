//! Porte para Rust seguro do crate `bun_ast` do bun 1.4.2 (`src/ast` do tag `bun-v1.4.2`). Os módulos mantêm o
//! nome e a estrutura do upstream para a ressincronização; o README de `wip/zbun` diz quais já foram portados e o
//! que mudou em cada um.
#![forbid(unsafe_code)]

pub mod nodes;
pub mod op;

pub use nodes::AssignTarget;
