//! LevelDB em Rust seguro, no formato em disco do leveldb que o Chromium usa (`third_party/leveldatabase`).
//!
//! Os arquivos de um banco: `CURRENT` (o nome do MANIFEST vigente), `MANIFEST-<n>` (um log de `VersionEdit`),
//! `<n>.log` (um log de `WriteBatch`, o que ainda não foi para uma tabela), `<n>.ldb` (tabelas ordenadas), `LOCK` e
//! `LOG` (o registro de texto). Esta crate escreve e lê esses formatos byte a byte, de modo que o leveldb do Chromium
//! abre o que ela escreve e ela abre o que o Chromium escreve.
//!
//! Módulos: [`coding`] (varints e strings prefixadas), [`log`] (o formato de log em blocos de 32 KiB com CRC-32C
//! mascarado), [`batch`] (o `WriteBatch`), [`version_edit`] (as mudanças de versão que o MANIFEST guarda), [`snappy`],
//! [`block`] e [`table`] (as tabelas `.ldb`), [`memtable`] (com a contabilidade de memória do arena, que decide
//! quando a memtable vira tabela), [`version`] (o conjunto de versões e o MANIFEST), [`dbformat`] (chaves internas e
//! nomes de arquivo), [`env`] (os arquivos, atrás de um trait) e [`db`] (o banco).

pub mod batch;
pub mod block;
pub mod coding;
pub mod compaction;
pub mod db;
pub mod dbformat;
pub mod env;
pub mod log;
pub mod memtable;
pub mod snappy;
pub mod table;
pub mod version;
pub mod version_edit;

pub use db::{Db, Options};

/// Erro de um banco: arquivo fora do formato, argumento inválido ou falha do sistema de arquivos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// O arquivo está corrompido (CRC, tamanho ou tag inválida); o texto diz onde.
    Corruption(String),
    InvalidArgument(String),
    Io(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Corruption(message) => write!(f, "Corruption: {message}"),
            Error::InvalidArgument(message) => write!(f, "Invalid argument: {message}"),
            Error::Io(message) => write!(f, "IO error: {message}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
