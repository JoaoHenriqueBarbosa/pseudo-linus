//! LevelDB em Rust seguro, no formato em disco do leveldb que o Chromium usa (`third_party/leveldatabase`).
//!
//! Os arquivos de um banco: `CURRENT` (o nome do MANIFEST vigente), `MANIFEST-<n>` (um log de `VersionEdit`),
//! `<n>.log` (um log de `WriteBatch`, o que ainda não foi para uma tabela), `<n>.ldb` (tabelas ordenadas), `LOCK` e
//! `LOG` (o registro de texto). Esta crate escreve e lê esses formatos byte a byte, de modo que o leveldb do Chromium
//! abre o que ela escreve e ela abre o que o Chromium escreve.
//!
//! Módulos: [`coding`] (varints e strings prefixadas), [`log`] (o formato de log em blocos de 32 KiB com CRC-32C
//! mascarado), [`batch`] (o `WriteBatch`) e [`version_edit`] (as mudanças de versão que o MANIFEST guarda).

pub mod batch;
pub mod coding;
pub mod log;
pub mod version_edit;

/// Erro de um arquivo do banco que não segue o formato.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// O arquivo está corrompido (CRC, tamanho ou tag inválida); o texto diz onde.
    Corruption(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Corruption(message) => write!(f, "Corruption: {message}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
