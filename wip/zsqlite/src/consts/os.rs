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
pub const SQLITE_FCNTL_DB_UNCHANGED: u32 = 0xca093fa0;
