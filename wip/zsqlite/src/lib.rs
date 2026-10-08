//! zsqlite: SQLite 3.46.1 traduzido para Rust seguro (modelo v2, ver CONVENTIONS.md).
#![forbid(unsafe_code)]

pub mod consts;

// Camada 0.
pub mod bitvec;
pub mod complete;
pub mod ctype;
pub mod hash;
pub mod keywordhash;
pub mod printf;
pub mod random;
pub mod rowset;
pub mod tokenize;
pub mod utf;
pub mod util;

// Camada 1.
pub mod os;
pub mod pcache;
pub mod pcache1;

// Camada 4 (base de valores e registros do VDBE).
pub mod mem;
pub mod record;
pub mod memjournal;
pub mod memdb;
pub mod os_unix;
pub mod wal;
pub mod global;
pub mod pager;
pub mod btree_types;
pub mod pager_ext;
pub mod btree;
pub mod btree_cursor;
pub mod btree_write;
pub mod sqlite_int;
pub mod connection;
pub mod vdbe_types;
pub mod vdbesort;
