//! Constantes extraídas de btreeInt_h (codegen do material de consulta em legacy/).
#![allow(unused_imports)]
use super::*;
pub const SQLITE_FILE_HEADER: &[u8; 16] = b"SQLite format 3\0";
pub const PTF_INTKEY: u8 = 0x01;
pub const PTF_ZERODATA: u8 = 0x02;
pub const PTF_LEAFDATA: u8 = 0x04;
pub const PTF_LEAF: u8 = 0x08;
pub const READ_LOCK: u8 = 1;
pub const WRITE_LOCK: u8 = 2;
pub const TRANS_NONE: u8 = 0;
pub const TRANS_READ: u8 = 1;
pub const TRANS_WRITE: u8 = 2;
pub const BTS_READ_ONLY: u16 = 0x0001;
pub const BTS_PAGESIZE_FIXED: u16 = 0x0002;
pub const BTS_SECURE_DELETE: u16 = 0x0004;
pub const BTS_OVERWRITE: u16 = 0x0008;
pub const BTS_FAST_SECURE: u16 = 0x000c;
pub const BTS_INITIALLY_EMPTY: u16 = 0x0010;
pub const BTS_NO_WAL: u16 = 0x0020;
pub const BTS_EXCLUSIVE: u16 = 0x0040;
pub const BTS_PENDING: u16 = 0x0080;
pub const BTCURSOR_MAX_DEPTH: usize = 20;
pub const BTCF_WRITE_FLAG: u8 = 0x01;
pub const BTCF_VALID_NKEY: u8 = 0x02;
pub const BTCF_VALID_OVFL: u8 = 0x04;
pub const BTCF_AT_LAST: u8 = 0x08;
pub const BTCF_INCRBLOB: u8 = 0x10;
pub const BTCF_MULTIPLE: u8 = 0x20;
pub const BTCF_PINNED: u8 = 0x40;
pub const CURSOR_VALID: u8 = 0;
pub const CURSOR_INVALID: u8 = 1;
pub const CURSOR_SKIPNEXT: u8 = 2;
pub const CURSOR_REQUIRESEEK: u8 = 3;
pub const CURSOR_FAULT: u8 = 4;
pub const BTALLOC_ANY: u8 = 0;   // Alocar qualquer página
pub const BTALLOC_EXACT: u8 = 1; // Alocar a página exata se possível
pub const BTALLOC_LE: u8 = 2;    // Alocar qualquer página <= o parâmetro
pub const PTRMAP_ROOTPAGE: u8 = 1;
pub const PTRMAP_FREEPAGE: u8 = 2;
pub const PTRMAP_OVERFLOW1: u8 = 3;
pub const PTRMAP_OVERFLOW2: u8 = 4;
pub const PTRMAP_BTREE: u8 = 5;
