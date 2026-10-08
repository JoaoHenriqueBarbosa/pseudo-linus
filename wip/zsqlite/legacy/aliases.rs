//! Apelidos de tipo exigidos pelas partes traduzidas (nomes que os cabeçalhos usam sem definir).
#![allow(unused_imports)]

use crate::prelude::*;

pub type SqliteRef = Sqlite3Ref;
pub type DbRef = Sqlite3Ref;
pub type sqlite3 = Sqlite3;
pub type Sqlite3Value = Mem;
pub type sqlite3_value = Mem;
pub type sqlite3_mutex = Sqlite3Mutex;
pub type sqlite3_vtab_cursor = Sqlite3VtabCursor;
pub type sqlite3_mem_methods = Sqlite3MemMethods;
pub type sqlite3_mutex_methods = Sqlite3MutexMethods;
pub type sqlite3_pcache_methods2 = Sqlite3PcacheMethods2;
pub type Sqlite3MutexRef = Rc<Sqlite3Mutex>;
pub type ParseRef = Rc<RefCell<Parse>>;
pub type ExprRef = Rc<RefCell<Expr>>;
pub type ExprListRef = Rc<RefCell<ExprList>>;
pub type SrcListRef = Rc<RefCell<SrcList>>;
pub type SelectRef = Rc<RefCell<Select>>;
pub type WithRef = Rc<RefCell<With>>;
pub type CteUseRef = Rc<RefCell<CteUse>>;
pub type ynVar = i16;
pub type YDbMask = u32;
