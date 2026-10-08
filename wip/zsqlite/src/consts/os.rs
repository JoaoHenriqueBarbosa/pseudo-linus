//! Constantes extraídas de os_h (codegen do material de consulta em legacy/).
#![allow(unused_imports)]
use super::*;
pub const SQLITE_MAX_PATHLEN: usize = 4096;
pub const SQLITE_MAX_SYMLINK: u32 = 200;
pub const SQLITE_DEFAULT_SECTOR_SIZE: u32 = 4096;
pub const SQLITE_TEMP_FILE_PREFIX: &[u8] = b"etilqs_";
pub const NO_LOCK: i32 = 0;
pub const SHARED_LOCK: i32 = 1;
pub const RESERVED_LOCK: i32 = 2;
pub const PENDING_LOCK: i32 = 3;
pub const EXCLUSIVE_LOCK: i32 = 4;
pub const SHARED_SIZE: i32 = 510;
/// `PENDING_BYTE`: com `SQLITE_OMIT_WSD` ausente o C usa a variável
/// `sqlite3PendingByte`, só alterável por `sqlite3_test_control`, que este
/// porte não expõe; vale o padrão (primeiro byte depois de 1 GiB).
pub const PENDING_BYTE: i64 = 0x40000000;
pub const RESERVED_BYTE: i64 = PENDING_BYTE + 1;
pub const SHARED_FIRST: i64 = PENDING_BYTE + 2;
pub const SQLITE_FCNTL_DB_UNCHANGED: u32 = 0xca093fa0;
