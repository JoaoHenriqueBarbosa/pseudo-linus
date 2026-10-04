//! `file` 5.46 do Debian 13: identifica o tipo de arquivos pelo conteúdo.
//!
//! Porte para Rust do file 5.46 (Ian F. Darwin, Christos Zoulas e outros; licença BSD de duas
//! cláusulas, reproduzida no começo de cada módulo portado), com os patches do pacote Debian
//! 5.46-5. O banco de regras é o `Magdir` do mesmo pacote, embutido ([`magdir`]).

pub mod apprentice;
pub mod cutil;
pub mod encoding;
pub mod magdir;
pub mod regex;
