//! Constantes extraídas de sqliteLimit_h (codegen do material de consulta em legacy/).
#![allow(unused_imports)]
use super::*;
pub const SQLITE_MAX_LENGTH: i32 = 1000000000;
pub const SQLITE_MAX_COLUMN: i32 = 2000;
pub const SQLITE_MAX_SQL_LENGTH: i32 = 1000000000;
pub const SQLITE_MAX_EXPR_DEPTH: i32 = 1000;
pub const SQLITE_MAX_COMPOUND_SELECT: i32 = 500;
pub const SQLITE_MAX_VDBE_OP: i32 = 250000000;
pub const SQLITE_MAX_FUNCTION_ARG: i32 = 127;
pub const SQLITE_DEFAULT_CACHE_SIZE: i32 = -2000;
pub const SQLITE_DEFAULT_WAL_AUTOCHECKPOINT: i32 = 1000;
pub const SQLITE_MAX_ATTACHED: i32 = 10;
pub const SQLITE_MAX_VARIABLE_NUMBER: i32 = 250000;
pub const SQLITE_MAX_PAGE_SIZE: i32 = 65536;
pub const SQLITE_DEFAULT_PAGE_SIZE: i32 = 4096;
pub const SQLITE_MAX_DEFAULT_PAGE_SIZE: i32 = 8192;
pub const SQLITE_MAX_PAGE_COUNT: u32 = 0xfffffffe;
pub const SQLITE_MAX_LIKE_PATTERN_LENGTH: i32 = 50000;
pub const SQLITE_MAX_TRIGGER_DEPTH: i32 = 1000;
