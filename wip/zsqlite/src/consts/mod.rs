pub mod sqlite3;
pub use sqlite3::*;
pub mod opcodes;
pub use opcodes::*;
pub mod parse;
pub use parse::*;
pub mod limits;
pub use limits::*;
pub mod pragma;
pub use pragma::*;
pub mod sqlite_int;
pub use sqlite_int::*;
pub mod btree_int;
pub use btree_int::*;
pub mod vdbe_int;
pub use vdbe_int::*;
pub mod pager;
pub use pager::*;
pub mod where_int;
pub use where_int::*;
pub mod os;
pub use os::*;
pub mod btree;
pub use btree::*;
pub mod vdbe;
pub use vdbe::*;
pub mod wal;
pub use wal::*;
pub mod os_common;
pub use os_common::*;

/// Máscara de bits de tabelas/colunas do planejador (Bitmask do C).
pub type Bitmask = u64;

/// Macro `HI(X)` do sqliteInt.h: coloca o valor nos 32 bits altos.
pub const fn hi(x: u64) -> u64 {
    x << 32
}
