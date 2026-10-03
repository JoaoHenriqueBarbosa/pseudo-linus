//! F01: motores de regex contra a semântica POSIX do GNU.
//!
//! - [`parse`]: parser dos dialetos GNU (BRE/ERE do grep e do sed) pra um AST comum ([`ast`]).
//! - [`emit`]: tradutor do AST pra sintaxe de cada motor candidato.
//! - [`engines`]: adaptadores dos motores.
//! - [`probe`]: as sondas (grep -n, grep -ob, sed s///, gawk match) e a emulação delas sobre um motor.

pub mod ast;
pub mod corpus;
pub mod emit;
pub mod engines;
pub mod mined;
pub mod parse;
pub mod probe;
pub mod report;
pub mod sample;
pub mod shell;
pub mod worker;
