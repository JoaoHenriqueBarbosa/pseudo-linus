//! `file` 5.46 do Debian 13: identifica o tipo de arquivos pelo conteúdo.
//!
//! Porte para Rust do file 5.46 (Ian F. Darwin, Christos Zoulas e outros; licença BSD de duas
//! cláusulas, reproduzida no começo de cada módulo portado), com os patches do pacote Debian
//! 5.46-5. O banco de regras é o `Magdir` do mesmo pacote, embutido ([`magdir`]).
//!
//! Fora do porte, por ora: o leitor de ELF (`readelf.c`), o de documentos OLE (`cdf.c`), o de
//! fitas SIMH e os descompressores que não o gzip no `-z`; as regras do banco continuam
//! descrevendo esses formatos, só sem os detalhes que esses leitores acrescentam.

pub mod apprentice;
pub mod cfmt;
pub mod cli;
pub mod cutil;
pub mod encoding;
pub mod funcs;
pub mod magdir;
pub mod magic;
pub mod regex;
pub mod softmagic;
pub mod wchar;
mod wctype_table;
