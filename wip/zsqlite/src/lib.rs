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
pub mod mem2;
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

// Camada 4 (VDBE): construção, execução e API.
pub mod vdbeaux;
pub mod vdbeaux2;
pub mod vdbeaux3;
pub mod vdbe;
pub mod vdbe_ops;
pub mod vdbe_ops2;
pub mod vdbe_ops3;
pub mod vdbeapi;
pub mod vdbetrace;

// Camada 5: árvores de sintaxe, esquema e geração de código.
pub mod parse_tables;
pub mod parse_reduce;
pub mod parse_reduce2;
pub mod walker;
pub mod resolve;
pub mod expr;
pub mod expr_code;
pub mod expr_code2;
pub mod build;
pub mod build2;
pub mod build3;
pub mod callback;
pub mod prepare;
pub mod select;
pub mod select2;
pub mod select3;
pub mod insert;
pub mod insert2;
pub mod delete;
pub mod update;
pub mod upsert;
pub mod where_int;
pub mod whereexpr;
pub mod where_;
pub mod where2;
pub mod where3;
pub mod wherecode;
pub mod window;
pub mod func;
pub mod date;
pub mod json;
pub mod json2;
pub mod main;
pub mod legacy;
pub mod auth;
pub mod attach;
pub mod opcodes;
pub mod trigger;
pub mod fkey;
pub mod alter;
pub mod backup;
pub mod ctime;
pub mod vtab;
pub mod with;
pub mod vacuum;
pub mod pragma;
pub mod pragma2;
pub mod analyze;
pub mod loadext;
pub mod notify;
pub mod stmt;
pub mod dbpage;
pub mod dbstat;
pub mod rtree;
pub mod rtree2;
pub mod fts5;
pub mod fts3;
#[cfg(test)]
mod btree_oracle_test;
