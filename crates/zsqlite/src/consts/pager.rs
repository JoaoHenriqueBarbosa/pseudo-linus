//! Constantes extraídas de pager_h (codegen do material de consulta em legacy/).
#![allow(unused_imports)]
use super::*;
pub const DEFAULT_JOURNAL_SIZE_LIMIT: i64 = -1;
pub const PAGER_OMIT_JOURNAL: i32 = 0x0001;
pub const PAGER_MEMORY: i32 = 0x0002;
pub const PAGER_LOCKINGMODE_QUERY: i32 = -1;
pub const PAGER_LOCKINGMODE_NORMAL: i32 = 0;
pub const PAGER_LOCKINGMODE_EXCLUSIVE: i32 = 1;
pub const PAGER_JOURNALMODE_QUERY: i32 = -1;
pub const PAGER_JOURNALMODE_DELETE: i32 = 0;
pub const PAGER_JOURNALMODE_PERSIST: i32 = 1;
pub const PAGER_JOURNALMODE_OFF: i32 = 2;
pub const PAGER_JOURNALMODE_TRUNCATE: i32 = 3;
pub const PAGER_JOURNALMODE_MEMORY: i32 = 4;
pub const PAGER_JOURNALMODE_WAL: i32 = 5;
pub const PAGER_GET_NOCONTENT: i32 = 0x01;
pub const PAGER_GET_READONLY: i32 = 0x02;
pub const PAGER_SYNCHRONOUS_OFF: u32 = 0x01;
pub const PAGER_SYNCHRONOUS_NORMAL: u32 = 0x02;
pub const PAGER_SYNCHRONOUS_FULL: u32 = 0x03;
pub const PAGER_SYNCHRONOUS_EXTRA: u32 = 0x04;
pub const PAGER_SYNCHRONOUS_MASK: u32 = 0x07;
pub const PAGER_FULLFSYNC: u32 = 0x08;
pub const PAGER_CKPT_FULLFSYNC: u32 = 0x10;
pub const PAGER_CACHESPILL: u32 = 0x20;
pub const PAGER_FLAGS_MASK: u32 = 0x38;
