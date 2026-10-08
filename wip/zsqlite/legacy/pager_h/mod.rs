// Mesclado das partes traduzidas de pager_h (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

/// Tipo usado para representar um número de página.
/// A primeira página em um arquivo é chamada página 1.
/// 0 é usado para representar "não é uma página".
pub type Pgno = u32;

/// Cada arquivo aberto é gerenciado por uma instância separada da estrutura `Pager`,
/// definida em pager.c (módulo `pager`, reexportada no prelude).

/// Tipo de manipulador para páginas (`PgHdr` é definido em pcache.h).
pub type DbPage = PgHdr;

/// Tamanho máximo padrão para arquivos de journal persistentes.
/// Um valor negativo significa sem limite.
/// Esse valor pode ser substituído usando a API sqlite3PagerJournalSizeLimit().
/// Veja também "PRAGMA journal_size_limit".
pub const DEFAULT_JOURNAL_SIZE_LIMIT: i64 = -1;

/// Número de página PAGER_SJ_PGNO nunca é usado em um banco de dados SQLite
/// (é reservado para contornar uma incompatibilidade Windows/POSIX).
/// É usado no journal para significar que o resto do arquivo de journal
/// é dedicado ao armazenamento de um nome de super-journal, sem mais páginas para fazer rollback.
/// Veja comentários para a função writeSuperJournal() em pager.c para detalhes.
#[inline]
pub fn pager_sj_pgno_computed(x: &Pager) -> Pgno {
    ((PENDING_BYTE / x.page_size as u32) as Pgno).wrapping_add(1)
}

#[inline]
pub fn pager_sj_pgno(x: &Pager) -> Pgno {
    x.lck_pgno
}

/// Valores permitidos para o parâmetro flags de sqlite3PagerOpen().
/// NOTA: Esses valores devem corresponder aos valores BTREE_ em btree.h.
pub const PAGER_OMIT_JOURNAL: i32 = 0x0001;
pub const PAGER_MEMORY: i32 = 0x0002;

/// Valores válidos para o segundo argumento de sqlite3PagerLockingMode().
pub const PAGER_LOCKINGMODE_QUERY: i32 = -1;
pub const PAGER_LOCKINGMODE_NORMAL: i32 = 0;
pub const PAGER_LOCKINGMODE_EXCLUSIVE: i32 = 1;

/// Constantes numéricas que codificam o modo de journal.
/// Os valores numéricos codificados aqui (exceto PAGER_JOURNALMODE_QUERY)
/// são expostos na API via comando "PRAGMA journal_mode" e portanto
/// não podem ser alterados sem quebrar compatibilidade.
pub const PAGER_JOURNALMODE_QUERY: i32 = -1;
pub const PAGER_JOURNALMODE_DELETE: i32 = 0;
pub const PAGER_JOURNALMODE_PERSIST: i32 = 1;
pub const PAGER_JOURNALMODE_OFF: i32 = 2;
pub const PAGER_JOURNALMODE_TRUNCATE: i32 = 3;
pub const PAGER_JOURNALMODE_MEMORY: i32 = 4;
pub const PAGER_JOURNALMODE_WAL: i32 = 5;

/// Flags que compõem a máscara passada para sqlite3PagerGet().
pub const PAGER_GET_NOCONTENT: i32 = 0x01;
pub const PAGER_GET_READONLY: i32 = 0x02;

/// Flags para sqlite3PagerSetFlags()
/// Restrições de valor (verificadas via assert()):
///     PAGER_FULLFSYNC      == SQLITE_FullFSync
///     PAGER_CKPT_FULLFSYNC == SQLITE_CkptFullFSync
///     PAGER_CACHE_SPILL    == SQLITE_CacheSpill
pub const PAGER_SYNCHRONOUS_OFF: u32 = 0x01;
pub const PAGER_SYNCHRONOUS_NORMAL: u32 = 0x02;
pub const PAGER_SYNCHRONOUS_FULL: u32 = 0x03;
pub const PAGER_SYNCHRONOUS_EXTRA: u32 = 0x04;
pub const PAGER_SYNCHRONOUS_MASK: u32 = 0x07;
pub const PAGER_FULLFSYNC: u32 = 0x08;
pub const PAGER_CKPT_FULLFSYNC: u32 = 0x10;
pub const PAGER_CACHESPILL: u32 = 0x20;
pub const PAGER_FLAGS_MASK: u32 = 0x38;

/// Sem SQLITE_ENABLE_SETLK_TIMEOUT: `sqlite3PagerWalWriteLock(y,z)` vira SQLITE_OK
/// e `sqlite3PagerWalDb(x,y)` não faz nada.
#[inline]
pub fn pager_wal_write_lock(_p: &Pager, _z: i32) -> i32 {
    SQLITE_OK
}

#[inline]
pub fn pager_wal_db(_p: &Pager, _db: &SqliteRef) {}

/// Sem SQLITE_TEST: as duas macros de E/S simulada são vazias.
#[inline]
pub fn disable_simulated_io_errors() {}

#[inline]
pub fn enable_simulated_io_errors() {}

