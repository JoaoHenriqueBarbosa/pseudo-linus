//! F02: grep e sed.
//!
//! - [`grep`]: grep montado com as crates do ripgrep (`grep-searcher`, `grep-matcher`, `grep-printer`)
//!   e front-end de flags nosso, sobre a árvore do caso em memória ([`fsview`]).

pub mod bashkit_run;
pub mod effort;
pub mod exec;
pub mod execmode;
pub mod fsview;
pub mod grep;
