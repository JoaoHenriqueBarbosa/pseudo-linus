// Mesclado das partes traduzidas de wal_h (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

// Macros para extrair os flags de sincronização apropriados para commits de transação (WAL_SYNC_FLAGS(X))
// ou para operações de checkpoint (CKPT_SYNC_FLAGS(X)).

/// Extrai os flags de sincronização para commits de transação a partir de um inteiro.
#[inline]
pub fn wal_sync_flags(x: i32) -> i32 {
    x & 0x03
}

/// Extrai os flags de sincronização para operações de checkpoint a partir de um inteiro.
#[inline]
pub fn ckpt_sync_flags(x: i32) -> i32 {
    (x >> 2) & 0x03
}

pub const WAL_SAVEPOINT_NDATA: usize = 4;

// O tipo `Wal` (conexão com um arquivo de write-ahead log, um objeto por pager) é opaco neste
// cabeçalho e definido em wal.c: a struct vive em `wal::Wal`, sem declaração aqui para não colidir.
// As declarações de função do cabeçalho (sqlite3WalOpen e demais) são só protótipos, e as
// implementações são traduzidas em wal.c (`wal_open`, `wal_close`, ...). Os blocos
// SQLITE_OMIT_WAL, SQLITE_ENABLE_SNAPSHOT, SQLITE_ENABLE_ZIPVFS, SQLITE_ENABLE_SETLK_TIMEOUT e
// SQLITE_USE_SEH não valem na configuração do Debian 13 e foram omitidos.

