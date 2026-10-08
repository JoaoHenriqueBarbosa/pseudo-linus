//! `pragma.c` e `pragma.h`: o comando PRAGMA. Este módulo traz a tabela `aPragmaName` (as opções do
//! Debian 13 já resolvidas: sem `SQLITE_DEBUG`, `SQLITE_TEST`, Windows ou `ENABLE_LOCKING_STYLE`, o
//! que tira `lock_status`, `stats`, `parser_trace`, `sql_trace`, `vdbe_*`, `data_store_directory`,
//! `lock_proxy_file` e `activate_extensions`), o despacho de `sqlite3Pragma` e os casos curtos. Os
//! casos longos (`integrity_check`, `table_info`, `foreign_key_check`, `optimize` e companhia) e a
//! tabela virtual `pragma_xxx` ficam em `pragma2.rs`, reexportado aqui.
//!
//! Desvios do C, decorrentes do modelo v2 (CONVENTIONS.md):
//!
//! - `Token *pId2` vira `&Token`; o `pId2->n = 1` do `journal_mode` vira a variável local
//!   `PragmaCtx.id2_n`, e `pId2->z == 0` é `!id2_has_z`;
//! - `char *aFcntl[4]` é `FileControlArg::Pragma { result, name, value }`;
//! - `VdbeOp *aOp` devolvido por `sqlite3VdbeAddOpList` é o índice da primeira operação inserida;
//! - o limite de `sqlite3GlobalConfig.szMmap` é `SQLITE_DEFAULT_MMAP_SIZE` (a configuração global
//!   ainda não tem leitor);
//! - `sqlite3VdbeVerifyNoMallocRequired`, `VdbeCoverage` e as asserções de depuração somem.

pub use crate::pragma2::*;

use crate::btree::{
    btree_get_auto_vacuum, btree_get_page_size, btree_secure_delete, btree_set_auto_vacuum,
    btree_set_cache_size, btree_set_mmap_limit, btree_set_page_size, btree_set_pager_flags,
    btree_set_spill_size,
};
use crate::build::{
    name_from_token, reset_all_schemas_of_connection, text_arg, token_arg, two_part_name,
};
use crate::build3::{begin_write_operation, code_verify_schema, open_temp_database};
use crate::callback::set_text_encoding;
use crate::connection::{Connection, Parse};
use crate::consts::{
    BTREE_APPLICATION_ID, BTREE_AUTOVACUUM_FULL, BTREE_AUTOVACUUM_INCR, BTREE_AUTOVACUUM_NONE,
    BTREE_DATA_VERSION, BTREE_DEFAULT_CACHE_SIZE, BTREE_FREE_PAGE_COUNT, BTREE_INCR_VACUUM,
    BTREE_LARGEST_ROOT_PAGE, BTREE_SCHEMA_VERSION, BTREE_USER_VERSION, COLNAME_NAME,
    DBFLAG_ENCODING_FIXED, OE_ABORT, OP_ADDIMM, OP_EXPIRE, OP_HALT, OP_IF, OP_IFPOS,
    OP_INCRVACUUM, OP_INTEGER, OP_INT64, OP_JOURNALMODE, OP_MAXPGCNT, OP_NOOP, OP_PAGECOUNT,
    OP_READCOOKIE, OP_RESULTROW, OP_SETCOOKIE, OP_SUBTRACT, OP_TRANSACTION, OP_CHECKPOINT,
    PRAG_FLG_NEED_SCHEMA, PRAG_FLG_NO_COLUMNS, PRAG_FLG_NO_COLUMNS1, PRAG_FLG_READ_ONLY,
    PRAG_FLG_RESULT0, PRAG_FLG_RESULT1, PRAG_FLG_SCHEMA_OPT, PRAG_FLG_SCHEMA_REQ,
    PRAG_TYP_ANALYSIS_LIMIT, PRAG_TYP_AUTO_VACUUM, PRAG_TYP_BUSY_TIMEOUT, PRAG_TYP_CACHE_SIZE,
    PRAG_TYP_CACHE_SPILL, PRAG_TYP_CASE_SENSITIVE_LIKE, PRAG_TYP_COLLATION_LIST,
    PRAG_TYP_COMPILE_OPTIONS, PRAG_TYP_DATABASE_LIST, PRAG_TYP_DEFAULT_CACHE_SIZE,
    PRAG_TYP_ENCODING, PRAG_TYP_FLAG, PRAG_TYP_FOREIGN_KEY_CHECK, PRAG_TYP_FOREIGN_KEY_LIST,
    PRAG_TYP_FUNCTION_LIST, PRAG_TYP_HARD_HEAP_LIMIT, PRAG_TYP_HEADER_VALUE,
    PRAG_TYP_INCREMENTAL_VACUUM, PRAG_TYP_INDEX_INFO, PRAG_TYP_INDEX_LIST,
    PRAG_TYP_INTEGRITY_CHECK, PRAG_TYP_JOURNAL_MODE, PRAG_TYP_JOURNAL_SIZE_LIMIT,
    PRAG_TYP_LOCKING_MODE, PRAG_TYP_MMAP_SIZE, PRAG_TYP_MODULE_LIST, PRAG_TYP_OPTIMIZE,
    PRAG_TYP_PAGE_COUNT, PRAG_TYP_PAGE_SIZE, PRAG_TYP_PRAGMA_LIST, PRAG_TYP_SECURE_DELETE,
    PRAG_TYP_SHRINK_MEMORY, PRAG_TYP_SOFT_HEAP_LIMIT, PRAG_TYP_SYNCHRONOUS, PRAG_TYP_TABLE_INFO,
    PRAG_TYP_TABLE_LIST, PRAG_TYP_TEMP_STORE, PRAG_TYP_TEMP_STORE_DIRECTORY, PRAG_TYP_THREADS,
    PRAG_TYP_WAL_AUTOCHECKPOINT, PRAG_TYP_WAL_CHECKPOINT,
    P4_INT64, PAGER_FLAGS_MASK, PAGER_JOURNALMODE_OFF, PAGER_JOURNALMODE_QUERY,
    PAGER_LOCKINGMODE_EXCLUSIVE, PAGER_LOCKINGMODE_NORMAL, PAGER_LOCKINGMODE_QUERY,
    PAGER_SYNCHRONOUS_MASK, SQLITE_AUTO_INDEX, SQLITE_CACHE_SPILL, SQLITE_CELL_SIZE_CK,
    SQLITE_CHECKPOINT_FULL, SQLITE_CHECKPOINT_PASSIVE, SQLITE_CHECKPOINT_RESTART,
    SQLITE_CHECKPOINT_TRUNCATE, SQLITE_CKPT_FULL_FSYNC, SQLITE_COUNT_ROWS, SQLITE_DEFAULT_CACHE_SIZE,
    SQLITE_DEFAULT_MMAP_SIZE, SQLITE_DEFENSIVE, SQLITE_DEFER_FKS, SQLITE_ERROR,
    SQLITE_FCNTL_MMAP_SIZE, SQLITE_FCNTL_PRAGMA, SQLITE_FOREIGN_KEYS, SQLITE_FULL_COL_NAMES,
    SQLITE_FULL_FSYNC, SQLITE_IGNORE_CHECKS, SQLITE_LEGACY_ALTER, SQLITE_LIMIT_WORKER_THREADS,
    SQLITE_MAX_DB, SQLITE_NOMEM, SQLITE_NOTFOUND, SQLITE_NO_SCHEMA_ERROR, SQLITE_NULL_CALLBACK,
    SQLITE_OK, SQLITE_PRAGMA, SQLITE_QUERY_ONLY, SQLITE_READ_UNCOMMIT, SQLITE_REC_TRIGGERS,
    SQLITE_REVERSE_ORDER, SQLITE_SHORT_COL_NAMES, SQLITE_TRUSTED_SCHEMA, SQLITE_TXN_NONE,
    SQLITE_UTF16BE, SQLITE_UTF16LE, SQLITE_UTF16NATIVE, SQLITE_UTF8, SQLITE_WRITE_SCHEMA,
};
use crate::ctype::{is_digit, to_lower};
use crate::func::register_like_functions;
use crate::hash::hash_iter;
use crate::main::{
    busy_timeout, close_btree, compileoption_get, db_filename, file_control, limit, wal_autocheckpoint,
};
use crate::mem::StrDtor;
use crate::os::FileControlArg;
use crate::prepare::read_schema;
use crate::printf::PrintfArg;
use crate::select::get_vdbe;
use crate::sqlite_int::Token;
use crate::util::{
    abs_int32, atoi, dec_or_hex_to_i64, error_msg, get_int32, oom_fault, str_icmp, strlen30,
    strnicmp, at,
};
use crate::vdbe_types::{Vdbe, VdbeOpList};
use crate::vdbeaux::{
    add_op0, add_op1, add_op2, add_op3, add_op4_dup8, add_op_list, current_addr, jump_here,
    load_string, multi_load, reusable, run_only_once, vdbe_of_parse,
};
use crate::vdbeaux2::{set_col_name, set_num_cols, uses_btree};
use crate::btree_write::btree_txn_state;
use crate::auth::auth_check;

// ---------------------------------------------------------------------------------------------
// pragma.h
// ---------------------------------------------------------------------------------------------

// Os `PRAG_TYP_*` e `PRAG_FLG_*` (`PragTyp_XXXX` e `PragFlg_XXXX`) já estão em `crate::consts`,
// extraídos de `pragma.h`.

/// `pragCName[]`: os nomes das colunas dos pragmas de resultado com várias colunas ou com nome
/// diferente do pragma.
pub(crate) const PRAG_C_NAME: [&str; 57] = [
    /*   0 */ "id", // foreign_key_list
    /*   1 */ "seq",
    /*   2 */ "table",
    /*   3 */ "from",
    /*   4 */ "to",
    /*   5 */ "on_update",
    /*   6 */ "on_delete",
    /*   7 */ "match",
    /*   8 */ "cid", // table_xinfo (table_info reusa 8)
    /*   9 */ "name",
    /*  10 */ "type",
    /*  11 */ "notnull",
    /*  12 */ "dflt_value",
    /*  13 */ "pk",
    /*  14 */ "hidden",
    /*  15 */ "schema", // table_list
    /*  16 */ "name",
    /*  17 */ "type",
    /*  18 */ "ncol",
    /*  19 */ "wr",
    /*  20 */ "strict",
    /*  21 */ "seqno", // index_xinfo (index_info reusa 21)
    /*  22 */ "cid",
    /*  23 */ "name",
    /*  24 */ "desc",
    /*  25 */ "coll",
    /*  26 */ "key",
    /*  27 */ "name", // function_list
    /*  28 */ "builtin",
    /*  29 */ "type",
    /*  30 */ "enc",
    /*  31 */ "narg",
    /*  32 */ "flags",
    /*  33 */ "tbl", // stats
    /*  34 */ "idx",
    /*  35 */ "wdth",
    /*  36 */ "hght",
    /*  37 */ "flgs",
    /*  38 */ "seq", // index_list (collation_list reusa 38)
    /*  39 */ "name",
    /*  40 */ "unique",
    /*  41 */ "origin",
    /*  42 */ "partial",
    /*  43 */ "table", // foreign_key_check
    /*  44 */ "rowid",
    /*  45 */ "parent",
    /*  46 */ "fkid",
    /*  47 */ "seq", // database_list (index_info reusa 21)
    /*  48 */ "name",
    /*  49 */ "file",
    /*  50 */ "busy", // wal_checkpoint
    /*  51 */ "log",
    /*  52 */ "checkpointed",
    /*  53 */ "database", // lock_status
    /*  54 */ "status",
    /*  55 */ "cache_size", // default_cache_size (module_list e pragma_list reusam 9)
    /*  56 */ "timeout", // busy_timeout
];

/// `struct PragmaName`: a definição de um pragma embutido.
pub(crate) struct PragmaName {
    /// Nome do pragma.
    pub z_name: &'static str,
    /// Um `PRAG_TYP_*`.
    pub e_prag_typ: u8,
    /// Zero ou mais `PRAG_FLG_*`.
    pub m_prag_flg: u8,
    /// Início dos nomes das colunas em [`PRAG_C_NAME`].
    pub i_prag_c_name: u8,
    /// Quantidade de nomes de colunas; zero usa o nome do pragma.
    pub n_prag_c_name: u8,
    /// Argumento extra.
    pub i_arg: u64,
}

/// Uma linha de `aPragmaName[]`.
const fn pn(
    z_name: &'static str,
    e_prag_typ: u8,
    m_prag_flg: u8,
    i_prag_c_name: u8,
    n_prag_c_name: u8,
    i_arg: u64,
) -> PragmaName {
    PragmaName { z_name, e_prag_typ, m_prag_flg, i_prag_c_name, n_prag_c_name, i_arg }
}

const NS: u8 = PRAG_FLG_NEED_SCHEMA;
const NC: u8 = PRAG_FLG_NO_COLUMNS;
const NC1: u8 = PRAG_FLG_NO_COLUMNS1;
const RO: u8 = PRAG_FLG_READ_ONLY;
const R0: u8 = PRAG_FLG_RESULT0;
const R1: u8 = PRAG_FLG_RESULT1;
const SO: u8 = PRAG_FLG_SCHEMA_OPT;
const SR: u8 = PRAG_FLG_SCHEMA_REQ;

/// `aPragmaName[]`: todos os pragmas embutidos, em ordem lexicográfica (a busca é binária).
pub(crate) static PRAGMA_NAMES: [PragmaName; 66] = [
    pn("analysis_limit", PRAG_TYP_ANALYSIS_LIMIT, R0, 0, 0, 0),
    pn("application_id", PRAG_TYP_HEADER_VALUE, NC1 | R0, 0, 0, BTREE_APPLICATION_ID as u64),
    pn("auto_vacuum", PRAG_TYP_AUTO_VACUUM, NS | R0 | SR | NC1, 0, 0, 0),
    pn("automatic_index", PRAG_TYP_FLAG, R0 | NC1, 0, 0, SQLITE_AUTO_INDEX),
    pn("busy_timeout", PRAG_TYP_BUSY_TIMEOUT, R0, 56, 1, 0),
    pn("cache_size", PRAG_TYP_CACHE_SIZE, NS | R0 | SR | NC1, 0, 0, 0),
    pn("cache_spill", PRAG_TYP_CACHE_SPILL, R0 | SR | NC1, 0, 0, 0),
    pn("case_sensitive_like", PRAG_TYP_CASE_SENSITIVE_LIKE, NC, 0, 0, 0),
    pn("cell_size_check", PRAG_TYP_FLAG, R0 | NC1, 0, 0, SQLITE_CELL_SIZE_CK),
    pn("checkpoint_fullfsync", PRAG_TYP_FLAG, R0 | NC1, 0, 0, SQLITE_CKPT_FULL_FSYNC),
    pn("collation_list", PRAG_TYP_COLLATION_LIST, R0, 38, 2, 0),
    pn("compile_options", PRAG_TYP_COMPILE_OPTIONS, R0, 0, 0, 0),
    pn("count_changes", PRAG_TYP_FLAG, R0 | NC1, 0, 0, SQLITE_COUNT_ROWS),
    pn("data_version", PRAG_TYP_HEADER_VALUE, RO | R0, 0, 0, BTREE_DATA_VERSION as u64),
    pn("database_list", PRAG_TYP_DATABASE_LIST, R0, 47, 3, 0),
    pn("default_cache_size", PRAG_TYP_DEFAULT_CACHE_SIZE, NS | R0 | SR | NC1, 55, 1, 0),
    pn("defer_foreign_keys", PRAG_TYP_FLAG, R0 | NC1, 0, 0, SQLITE_DEFER_FKS),
    pn("empty_result_callbacks", PRAG_TYP_FLAG, R0 | NC1, 0, 0, SQLITE_NULL_CALLBACK),
    pn("encoding", PRAG_TYP_ENCODING, R0 | NC1, 0, 0, 0),
    pn("foreign_key_check", PRAG_TYP_FOREIGN_KEY_CHECK, NS | R0 | R1 | SO, 43, 4, 0),
    pn("foreign_key_list", PRAG_TYP_FOREIGN_KEY_LIST, NS | R1 | SO, 0, 8, 0),
    pn("foreign_keys", PRAG_TYP_FLAG, R0 | NC1, 0, 0, SQLITE_FOREIGN_KEYS),
    pn("freelist_count", PRAG_TYP_HEADER_VALUE, RO | R0, 0, 0, BTREE_FREE_PAGE_COUNT as u64),
    pn("full_column_names", PRAG_TYP_FLAG, R0 | NC1, 0, 0, SQLITE_FULL_COL_NAMES),
    pn("fullfsync", PRAG_TYP_FLAG, R0 | NC1, 0, 0, SQLITE_FULL_FSYNC),
    pn("function_list", PRAG_TYP_FUNCTION_LIST, R0, 27, 6, 0),
    pn("hard_heap_limit", PRAG_TYP_HARD_HEAP_LIMIT, R0, 0, 0, 0),
    pn("ignore_check_constraints", PRAG_TYP_FLAG, R0 | NC1, 0, 0, SQLITE_IGNORE_CHECKS),
    pn("incremental_vacuum", PRAG_TYP_INCREMENTAL_VACUUM, NS | NC, 0, 0, 0),
    pn("index_info", PRAG_TYP_INDEX_INFO, NS | R1 | SO, 21, 3, 0),
    pn("index_list", PRAG_TYP_INDEX_LIST, NS | R1 | SO, 38, 5, 0),
    pn("index_xinfo", PRAG_TYP_INDEX_INFO, NS | R1 | SO, 21, 6, 1),
    pn("integrity_check", PRAG_TYP_INTEGRITY_CHECK, NS | R0 | R1 | SO, 0, 0, 0),
    pn("journal_mode", PRAG_TYP_JOURNAL_MODE, NS | R0 | SR, 0, 0, 0),
    pn("journal_size_limit", PRAG_TYP_JOURNAL_SIZE_LIMIT, R0 | SR, 0, 0, 0),
    pn("legacy_alter_table", PRAG_TYP_FLAG, R0 | NC1, 0, 0, SQLITE_LEGACY_ALTER),
    pn("locking_mode", PRAG_TYP_LOCKING_MODE, R0 | SR, 0, 0, 0),
    pn("max_page_count", PRAG_TYP_PAGE_COUNT, NS | R0 | SR, 0, 0, 0),
    pn("mmap_size", PRAG_TYP_MMAP_SIZE, 0, 0, 0, 0),
    pn("module_list", PRAG_TYP_MODULE_LIST, R0, 9, 1, 0),
    pn("optimize", PRAG_TYP_OPTIMIZE, R1 | NS, 0, 0, 0),
    pn("page_count", PRAG_TYP_PAGE_COUNT, NS | R0 | SR, 0, 0, 0),
    pn("page_size", PRAG_TYP_PAGE_SIZE, R0 | SR | NC1, 0, 0, 0),
    pn("pragma_list", PRAG_TYP_PRAGMA_LIST, R0, 9, 1, 0),
    pn("query_only", PRAG_TYP_FLAG, R0 | NC1, 0, 0, SQLITE_QUERY_ONLY),
    pn("quick_check", PRAG_TYP_INTEGRITY_CHECK, NS | R0 | R1 | SO, 0, 0, 0),
    pn("read_uncommitted", PRAG_TYP_FLAG, R0 | NC1, 0, 0, SQLITE_READ_UNCOMMIT),
    pn("recursive_triggers", PRAG_TYP_FLAG, R0 | NC1, 0, 0, SQLITE_REC_TRIGGERS),
    pn("reverse_unordered_selects", PRAG_TYP_FLAG, R0 | NC1, 0, 0, SQLITE_REVERSE_ORDER),
    pn("schema_version", PRAG_TYP_HEADER_VALUE, NC1 | R0, 0, 0, BTREE_SCHEMA_VERSION as u64),
    pn("secure_delete", PRAG_TYP_SECURE_DELETE, R0, 0, 0, 0),
    pn("short_column_names", PRAG_TYP_FLAG, R0 | NC1, 0, 0, SQLITE_SHORT_COL_NAMES),
    pn("shrink_memory", PRAG_TYP_SHRINK_MEMORY, NC, 0, 0, 0),
    pn("soft_heap_limit", PRAG_TYP_SOFT_HEAP_LIMIT, R0, 0, 0, 0),
    pn("synchronous", PRAG_TYP_SYNCHRONOUS, NS | R0 | SR | NC1, 0, 0, 0),
    pn("table_info", PRAG_TYP_TABLE_INFO, NS | R1 | SO, 8, 6, 0),
    pn("table_list", PRAG_TYP_TABLE_LIST, NS | R1, 15, 6, 0),
    pn("table_xinfo", PRAG_TYP_TABLE_INFO, NS | R1 | SO, 8, 7, 1),
    pn("temp_store", PRAG_TYP_TEMP_STORE, R0 | NC1, 0, 0, 0),
    pn("temp_store_directory", PRAG_TYP_TEMP_STORE_DIRECTORY, NC1, 0, 0, 0),
    pn("threads", PRAG_TYP_THREADS, R0, 0, 0, 0),
    pn("trusted_schema", PRAG_TYP_FLAG, R0 | NC1, 0, 0, SQLITE_TRUSTED_SCHEMA),
    pn("user_version", PRAG_TYP_HEADER_VALUE, NC1 | R0, 0, 0, BTREE_USER_VERSION as u64),
    pn("wal_autocheckpoint", PRAG_TYP_WAL_AUTOCHECKPOINT, 0, 0, 0, 0),
    pn("wal_checkpoint", PRAG_TYP_WAL_CHECKPOINT, NS, 50, 3, 0),
    pn("writable_schema", PRAG_TYP_FLAG, R0 | NC1, 0, 0, SQLITE_WRITE_SCHEMA | SQLITE_NO_SCHEMA_ERROR),
];

// ---------------------------------------------------------------------------------------------
// Auxiliares de pragma.c
// ---------------------------------------------------------------------------------------------

/// Uma linha de `VdbeOpList` (as tabelas de operações dos pragmas).
pub(crate) const fn ol(opcode: u8, p1: i8, p2: i8, p3: i8) -> VdbeOpList {
    VdbeOpList { opcode, p1, p2, p3 }
}

/// Os valores que `sqlite3Pragma` repassa aos casos: o que o C guarda em variáveis locais.
pub(crate) struct PragmaCtx<'a> {
    /// O pragma localizado.
    pub pragma: &'static PragmaName,
    /// O índice do banco (`iDb`).
    pub i_db: i32,
    /// O nome do pragma (`zLeft`).
    pub z_left: &'a [u8],
    /// O valor (`zRight`).
    pub z_right: Option<&'a [u8]>,
    /// O nome do banco, se foi dado (`zDb`).
    pub z_db: Option<&'a [u8]>,
    /// O token do valor (`pValue`).
    pub value: Option<&'a Token>,
    /// `pId2->n`.
    pub id2_n: usize,
    /// `pId2->z != 0`.
    pub id2_has_z: bool,
}

/// `getSafetyLevel`: interpreta `z` como nível de segurança. 0 é OFF, 1 é ON ou NORMAL, 2 é FULL
/// e 3 é EXTRA; 1 para vazio ou irreconhecível. FULL e EXTRA são recusados se `omit_full`.
///
/// Os valores devolvidos são um a menos que os de `sqlite3BtreeSetSafetyLevel()`, por causa do SQL
/// legado: o nível já foi booleano e scripts antigos usavam 0 para OFF e 1 para ON.
fn get_safety_level(z: &[u8], omit_full: bool, dflt: u8) -> u8 {
    //                                  123456789 123456789 123
    const Z_TEXT: &[u8] = b"onoffalseyestruextrafull";
    const I_OFFSET: [usize; 8] = [0, 1, 2, 4, 9, 12, 15, 20];
    const I_LENGTH: [usize; 8] = [2, 2, 3, 5, 3, 4, 5, 4];
    const I_VALUE: [u8; 8] = [1, 0, 0, 0, 1, 1, 3, 2];
    //                        on no off false yes true extra full
    if is_digit(at(z, 0)) {
        return atoi(z) as u8;
    }
    let n = strlen30(z);
    for i in 0..I_LENGTH.len() {
        if I_LENGTH[i] as i32 == n
            && strnicmp(Some(&Z_TEXT[I_OFFSET[i]..]), Some(z), n) == 0
            && (!omit_full || I_VALUE[i] <= 1)
        {
            return I_VALUE[i];
        }
    }
    dflt
}

/// `sqlite3GetBoolean`: interpreta `z` como booleano.
pub fn get_boolean(z: &[u8], dflt: bool) -> bool {
    get_safety_level(z, true, dflt as u8) != 0
}

/// `getLockingMode`: interpreta `z` como modo de trava.
fn get_locking_mode(z: Option<&[u8]>) -> i32 {
    if let Some(z) = z {
        if str_icmp(z, b"exclusive") == 0 {
            return PAGER_LOCKINGMODE_EXCLUSIVE;
        }
        if str_icmp(z, b"normal") == 0 {
            return PAGER_LOCKINGMODE_NORMAL;
        }
    }
    PAGER_LOCKINGMODE_QUERY
}

/// `getAutoVacuum`: interpreta `z` como modo de auto-vacuum: "none", "full" e "incremental", ou
/// os equivalentes numéricos 0, 1 e 2.
fn get_auto_vacuum(z: &[u8]) -> i32 {
    if str_icmp(z, b"none") == 0 {
        return BTREE_AUTOVACUUM_NONE as i32;
    }
    if str_icmp(z, b"full") == 0 {
        return BTREE_AUTOVACUUM_FULL as i32;
    }
    if str_icmp(z, b"incremental") == 0 {
        return BTREE_AUTOVACUUM_INCR as i32;
    }
    let i = atoi(z);
    if (0..=2).contains(&i) {
        i
    } else {
        0
    }
}

/// `getTempStore`: 1 para banco temporário em arquivo, 2 para em memória e 0 para o padrão de
/// compilação.
fn get_temp_store(z: &[u8]) -> i32 {
    if (b'0'..=b'2').contains(&at(z, 0)) {
        (at(z, 0) - b'0') as i32
    } else if str_icmp(z, b"file") == 0 {
        1
    } else if str_icmp(z, b"memory") == 0 {
        2
    } else {
        0
    }
}

/// `invalidateTempStorage`: invalida o armazenamento temporário, quando ele muda do padrão ou
/// quando é 'file' e o `temp_store_directory` mudou.
pub(crate) fn invalidate_temp_storage(db: &mut Connection, parse: &mut Parse) -> i32 {
    if db.dbs[1].bt.is_some() {
        if db.auto_commit == 0 || btree_txn_state(db.dbs[1].bt.as_ref()) != SQLITE_TXN_NONE {
            error_msg(db, parse, b"temporary storage cannot be changed from within a transaction", &[]);
            return SQLITE_ERROR;
        }
        if let Some(bt) = db.dbs[1].bt.take() {
            close_btree(db, bt);
        }
        reset_all_schemas_of_connection(db);
    }
    SQLITE_OK
}

/// `changeTempStorage`: se o banco TEMP está aberto, fecha-o e marca o esquema para ser relido.
/// Precisa ser feito ao usar os pragmas `temp_store` (`SQLITE_TEMP_STORE` e `DEFAULT_TEMP_STORE`).
fn change_temp_storage(db: &mut Connection, parse: &mut Parse, z_storage_type: &[u8]) -> i32 {
    let ts = get_temp_store(z_storage_type);
    if db.temp_store as i32 == ts {
        return SQLITE_OK;
    }
    if invalidate_temp_storage(db, parse) != SQLITE_OK {
        return SQLITE_ERROR;
    }
    db.temp_store = ts as u8;
    SQLITE_OK
}

/// `setPragmaResultColumnNames`: os nomes das colunas do resultado de um pragma.
fn set_pragma_result_column_names(v: &mut Vdbe, db: &Connection, p_pragma: &PragmaName) {
    let n = p_pragma.n_prag_c_name as i32;
    set_num_cols(v, if n == 0 { 1 } else { n });
    if n == 0 {
        set_col_name(v, db, 0, COLNAME_NAME, Some(p_pragma.z_name.as_bytes()), StrDtor::Static);
    } else {
        let first = p_pragma.i_prag_c_name as usize;
        for i in 0..n as usize {
            set_col_name(
                v,
                db,
                i as i32,
                COLNAME_NAME,
                Some(PRAG_C_NAME[first + i].as_bytes()),
                StrDtor::Static,
            );
        }
    }
}

/// `returnSingleInt`: gera o código que devolve um único inteiro.
pub(crate) fn return_single_int(v: &mut Vdbe, value: i64) {
    add_op4_dup8(v, OP_INT64 as i32, 0, 1, 0, value.to_ne_bytes(), P4_INT64);
    add_op2(v, OP_RESULTROW, 1, 1);
}

/// `returnSingleText`: gera o código que devolve um único texto (nada, se `z_value` é nulo).
pub(crate) fn return_single_text(v: &mut Vdbe, z_value: Option<&[u8]>) {
    if let Some(z) = z_value {
        load_string(v, 1, z);
        add_op2(v, OP_RESULTROW, 1, 1);
    }
}

/// `setAllPagerFlags`: ajusta o nível de segurança e as flags do pager de todos os bancos.
fn set_all_pager_flags(db: &mut Connection) {
    if db.auto_commit != 0 {
        let flags = (db.flags & PAGER_FLAGS_MASK as u64) as u32;
        for slot in db.dbs.iter_mut() {
            let level = slot.safety_level as u32;
            if let Some(bt) = slot.bt.as_mut() {
                btree_set_pager_flags(bt, level | flags);
            }
        }
    }
}

/// `sqlite3JournalModename`: o nome em minúsculas de um modo de journal (`PAGER_JOURNALMODE_*`).
const AZ_MODE_NAME: [&[u8]; 6] = [b"delete", b"persist", b"off", b"truncate", b"memory", b"wal"];

/// `sqlite3JournalModename`: o nome em minúsculas do modo de journal `e_mode` (um dos
/// `PAGER_JOURNALMODE_*`).
pub fn journal_modename(e_mode: i32) -> &'static [u8] {
    AZ_MODE_NAME.get(e_mode as usize).copied().unwrap_or(b"")
}

/// `pragmaLocate`: a posição do pragma `z_name` em [`PRAGMA_NAMES`], por busca binária.
pub(crate) fn pragma_locate(z_name: &[u8]) -> Option<usize> {
    let mut lwr = 0i32;
    let mut upr = PRAGMA_NAMES.len() as i32 - 1;
    while lwr <= upr {
        let mid = (lwr + upr) / 2;
        let rc = str_icmp(z_name, PRAGMA_NAMES[mid as usize].z_name.as_bytes());
        if rc == 0 {
            return Some(mid as usize);
        }
        if rc < 0 {
            upr = mid - 1;
        } else {
            lwr = mid + 1;
        }
    }
    None
}

// ---------------------------------------------------------------------------------------------
// sqlite3Pragma
// ---------------------------------------------------------------------------------------------

/// `sqlite3Pragma`: processa um comando PRAGMA, da forma `PRAGMA [schema.]id [= value]`. O
/// identificador pode ser uma cadeia; o valor é uma cadeia, um identificador ou um número. Se
/// `minus_flag`, o valor é um número precedido de um sinal de menos.
///
/// Se o lado esquerdo é "database.id", `p_id1` é o nome do banco e `p_id2` é o id; se é só "id",
/// `p_id1` é o id e `p_id2` é vazio.
pub fn pragma(
    db: &mut Connection,
    parse: &mut Parse,
    p_id1: &Token,
    p_id2: &Token,
    p_value: Option<&Token>,
    minus_flag: i32,
) {
    get_vdbe(db, parse);
    run_only_once(vdbe_of_parse(parse));
    parse.n_mem = 2;

    // Interpreta a parte [schema.] do comando. `i_db` é o índice do banco em `db.dbs[]`.
    let Some((i_db, p_id)) = two_part_name(db, parse, p_id1, p_id2) else {
        return;
    };

    // Se o banco temporário foi nomeado explicitamente, garante que ele está aberto.
    if i_db == 1 && open_temp_database(db, parse) != 0 {
        return;
    }

    let Some(z_left) = name_from_token(if p_id.z.is_empty() { None } else { Some(p_id) }) else {
        return;
    };
    let z_right: Option<Vec<u8>> = if minus_flag != 0 {
        let arg = match p_value {
            Some(t) => token_arg(parse, t),
            None => PrintfArg::Token(None),
        };
        crate::main::db_printf(db, b"-%T", &[arg])
    } else {
        match p_value {
            Some(t) if !t.z.is_empty() => name_from_token(Some(t)),
            _ => None,
        }
    };

    let z_db: Option<Vec<u8>> =
        if !p_id2.z.is_empty() { Some(db.dbs[i_db as usize].z_db_s_name.clone()) } else { None };
    if auth_check(db, parse, SQLITE_PRAGMA, Some(z_left.as_slice()), z_right.as_deref(), z_db.as_deref())
        != 0
    {
        return;
    }

    // Envia um SQLITE_FCNTL_PRAGMA ao arquivo do banco. Se o VFS devolve SQLITE_OK, considera que
    // ele tratou o pragma e gera um comando preparado que não faz nada.
    //
    // IMPLEMENTATION-OF: R-12238-55120 Whenever a PRAGMA statement is parsed, an
    // SQLITE_FCNTL_PRAGMA file control is sent to the open sqlite3_file object corresponding to
    // the database file to which the pragma statement refers.
    let mut a_fcntl = FileControlArg::Pragma {
        result: None,
        name: z_left.clone(),
        value: z_right.clone(),
    };
    db.busy_handler.n_busy.set(0);
    let rc = file_control(db, z_db.as_deref(), SQLITE_FCNTL_PRAGMA, &mut a_fcntl);
    let fcntl_result = match &mut a_fcntl {
        FileControlArg::Pragma { result, .. } => result.take(),
        _ => None,
    };
    if rc == SQLITE_OK {
        let v = vdbe_of_parse(parse);
        set_num_cols(v, 1);
        set_col_name(v, db, 0, COLNAME_NAME, fcntl_result.as_deref(), StrDtor::Transient);
        return_single_text(v, fcntl_result.as_deref());
        return;
    }
    if rc != SQLITE_NOTFOUND {
        if let Some(r) = &fcntl_result {
            error_msg(db, parse, b"%s", &[text_arg(r)]);
        }
        parse.n_err += 1;
        parse.rc = rc;
        return;
    }

    // Localiza o pragma na tabela.
    //
    // IMP: R-43042-22504 No error messages are generated if an unknown pragma is issued.
    let Some(p_idx) = pragma_locate(&z_left) else {
        return;
    };
    let p_pragma: &'static PragmaName = &PRAGMA_NAMES[p_idx];

    // Garante que o esquema está carregado se o pragma exige.
    if (p_pragma.m_prag_flg & PRAG_FLG_NEED_SCHEMA) != 0 && read_schema(db, parse) != 0 {
        return;
    }

    // Registra os nomes das colunas dos pragmas que devolvem resultado.
    if (p_pragma.m_prag_flg & PRAG_FLG_NO_COLUMNS) == 0
        && ((p_pragma.m_prag_flg & PRAG_FLG_NO_COLUMNS1) == 0 || z_right.is_none())
    {
        set_pragma_result_column_names(vdbe_of_parse(parse), db, p_pragma);
    }

    let c = PragmaCtx {
        pragma: p_pragma,
        i_db,
        z_left: &z_left,
        z_right: z_right.as_deref(),
        z_db: z_db.as_deref(),
        value: p_value,
        id2_n: p_id2.z.len(),
        id2_has_z: !p_id2.z.is_empty(),
    };

    // Salta para o tratador do pragma.
    match p_pragma.e_prag_typ {
        PRAG_TYP_DEFAULT_CACHE_SIZE => pragma_default_cache_size(db, parse, &c),
        PRAG_TYP_PAGE_SIZE => pragma_page_size(db, parse, &c),
        PRAG_TYP_SECURE_DELETE => pragma_secure_delete(db, parse, &c),
        PRAG_TYP_PAGE_COUNT => pragma_page_count(db, parse, &c),
        PRAG_TYP_LOCKING_MODE => pragma_locking_mode(db, parse, &c),
        PRAG_TYP_JOURNAL_MODE => pragma_journal_mode(db, parse, &c),
        PRAG_TYP_JOURNAL_SIZE_LIMIT => pragma_journal_size_limit(db, parse, &c),
        PRAG_TYP_AUTO_VACUUM => pragma_auto_vacuum(db, parse, &c),
        PRAG_TYP_INCREMENTAL_VACUUM => pragma_incremental_vacuum(db, parse, &c),
        PRAG_TYP_CACHE_SIZE => pragma_cache_size(db, parse, &c),
        PRAG_TYP_CACHE_SPILL => pragma_cache_spill(db, parse, &c),
        PRAG_TYP_MMAP_SIZE => pragma_mmap_size(db, parse, &c),
        PRAG_TYP_TEMP_STORE => match c.z_right {
            None => return_single_int(vdbe_of_parse(parse), db.temp_store as i64),
            Some(zr) => {
                change_temp_storage(db, parse, zr);
            }
        },
        PRAG_TYP_TEMP_STORE_DIRECTORY => pragma_temp_store_directory(db, parse, &c),
        PRAG_TYP_SYNCHRONOUS => pragma_synchronous(db, parse, &c),
        PRAG_TYP_FLAG => pragma_flag(db, parse, &c),
        PRAG_TYP_TABLE_INFO => pragma_table_info(db, parse, &c),
        PRAG_TYP_TABLE_LIST => pragma_table_list(db, parse, &c),
        PRAG_TYP_INDEX_INFO => pragma_index_info(db, parse, &c),
        PRAG_TYP_INDEX_LIST => pragma_index_list(db, parse, &c),
        PRAG_TYP_DATABASE_LIST => pragma_database_list(db, parse),
        PRAG_TYP_COLLATION_LIST => pragma_collation_list(db, parse),
        PRAG_TYP_FUNCTION_LIST => pragma_function_list(db, parse),
        PRAG_TYP_MODULE_LIST => {
            parse.n_mem = 1;
            for (_, m) in hash_iter(&db.a_module) {
                multi_load(vdbe_of_parse(parse), 1, b"s", &[text_arg(&m.z_name)]);
            }
        }
        PRAG_TYP_PRAGMA_LIST => {
            for p in PRAGMA_NAMES.iter() {
                multi_load(vdbe_of_parse(parse), 1, b"s", &[text_arg(p.z_name.as_bytes())]);
            }
        }
        PRAG_TYP_FOREIGN_KEY_LIST => pragma_foreign_key_list(db, parse, &c),
        PRAG_TYP_FOREIGN_KEY_CHECK => pragma_foreign_key_check(db, parse, &c),
        PRAG_TYP_CASE_SENSITIVE_LIKE => {
            // Reinstala LIKE e GLOB. A variante de LIKE é sensível ou não à caixa conforme o
            // lado direito.
            if let Some(zr) = c.z_right {
                register_like_functions(db, get_boolean(zr, false) as i32);
            }
        }
        PRAG_TYP_INTEGRITY_CHECK => pragma_integrity_check(db, parse, &c),
        PRAG_TYP_ENCODING => pragma_encoding(db, parse, &c),
        PRAG_TYP_HEADER_VALUE => pragma_header_value(db, parse, &c),
        PRAG_TYP_COMPILE_OPTIONS => {
            parse.n_mem = 1;
            let v = vdbe_of_parse(parse);
            let mut i = 0;
            while let Some(z_opt) = compileoption_get(i) {
                i += 1;
                load_string(v, 1, z_opt.as_bytes());
                add_op2(v, OP_RESULTROW, 1, 1);
            }
            reusable(v);
        }
        PRAG_TYP_WAL_CHECKPOINT => {
            let i_bt = if c.id2_has_z { c.i_db } else { SQLITE_MAX_DB };
            let mut e_mode = SQLITE_CHECKPOINT_PASSIVE;
            if let Some(zr) = c.z_right {
                if str_icmp(zr, b"full") == 0 {
                    e_mode = SQLITE_CHECKPOINT_FULL;
                } else if str_icmp(zr, b"restart") == 0 {
                    e_mode = SQLITE_CHECKPOINT_RESTART;
                } else if str_icmp(zr, b"truncate") == 0 {
                    e_mode = SQLITE_CHECKPOINT_TRUNCATE;
                }
            }
            parse.n_mem = 3;
            let v = vdbe_of_parse(parse);
            add_op3(v, OP_CHECKPOINT, i_bt, e_mode, 1);
            add_op2(v, OP_RESULTROW, 1, 3);
        }
        PRAG_TYP_WAL_AUTOCHECKPOINT => pragma_wal_autocheckpoint(db, parse, &c),
        PRAG_TYP_SHRINK_MEMORY => {
            // IMPLEMENTATION-OF: R-23445-46109 This pragma causes the database connection on
            // which it is invoked to free up as much memory as it can, by calling
            // sqlite3_db_release_memory().
            crate::main::db_release_memory(db);
        }
        PRAG_TYP_OPTIMIZE => pragma_optimize(db, parse, &c),
        PRAG_TYP_SOFT_HEAP_LIMIT => pragma_soft_heap_limit(parse, &c),
        PRAG_TYP_HARD_HEAP_LIMIT => pragma_hard_heap_limit(parse, &c),
        PRAG_TYP_THREADS => {
            let mut n = 0i64;
            if let Some(zr) = c.z_right {
                if dec_or_hex_to_i64(zr, &mut n) == SQLITE_OK && n >= 0 {
                    limit(db, SQLITE_LIMIT_WORKER_THREADS, (n & 0x7fffffff) as i32);
                }
            }
            let cur = limit(db, SQLITE_LIMIT_WORKER_THREADS, -1);
            return_single_int(vdbe_of_parse(parse), cur as i64);
        }
        PRAG_TYP_ANALYSIS_LIMIT => {
            // IMP: R-57594-65522
            let mut n = 0i64;
            if let Some(zr) = c.z_right {
                if dec_or_hex_to_i64(zr, &mut n) == SQLITE_OK && n >= 0 {
                    // IMP: R-40975-20399
                    db.n_analysis_limit = (n & 0x7fffffff) as i32;
                }
            }
            return_single_int(vdbe_of_parse(parse), db.n_analysis_limit as i64);
        }
        // PRAG_TYP_BUSY_TIMEOUT e o resto (`default:` do C).
        _ => {
            debug_assert!(p_pragma.e_prag_typ == PRAG_TYP_BUSY_TIMEOUT);
            if let Some(zr) = c.z_right {
                busy_timeout(db, atoi(zr));
            }
            return_single_int(vdbe_of_parse(parse), db.busy_timeout as i64);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Os casos curtos
// ---------------------------------------------------------------------------------------------

/// `PRAGMA [schema.]default_cache_size[=N]`: o primeiro modo informa o tamanho persistente do
/// cache de páginas (o máximo de páginas); o segundo grava o valor corrente e o persistente do
/// arquivo.
///
/// Versões antigas usavam um tamanho negativo para indicar synchronous=OFF. Hoje o synchronous
/// é sempre ligado por padrão, seja qual for o sinal, mas o valor absoluto continua valendo por
/// compatibilidade histórica.
fn pragma_default_cache_size(db: &mut Connection, parse: &mut Parse, c: &PragmaCtx) {
    let get_cache_size: [VdbeOpList; 9] = [
        ol(OP_TRANSACTION, 0, 0, 0),                                /* 0 */
        ol(OP_READCOOKIE, 0, 1, BTREE_DEFAULT_CACHE_SIZE as i8),     /* 1 */
        ol(OP_IFPOS, 1, 8, 0),
        ol(OP_INTEGER, 0, 2, 0),
        ol(OP_SUBTRACT, 1, 2, 1),
        ol(OP_IFPOS, 1, 8, 0),
        ol(OP_INTEGER, 0, 1, 0),                                    /* 6 */
        ol(OP_NOOP, 0, 0, 0),
        ol(OP_RESULTROW, 1, 1, 0),
    ];
    uses_btree(vdbe_of_parse(parse), c.i_db);
    match c.z_right {
        None => {
            parse.n_mem += 2;
            if let Some(a) = add_op_list(vdbe_of_parse(parse), &get_cache_size, 2) {
                let v = vdbe_of_parse(parse);
                v.a_op[a].p1 = c.i_db;
                v.a_op[a + 1].p1 = c.i_db;
                v.a_op[a + 6].p1 = SQLITE_DEFAULT_CACHE_SIZE;
            }
        }
        Some(zr) => {
            let size = abs_int32(atoi(zr));
            begin_write_operation(db, parse, 0, c.i_db);
            add_op3(vdbe_of_parse(parse), OP_SETCOOKIE, c.i_db, BTREE_DEFAULT_CACHE_SIZE as i32, size);
            let slot = &mut db.dbs[c.i_db as usize];
            slot.schema.cache_size = size;
            if let Some(bt) = slot.bt.as_mut() {
                btree_set_cache_size(bt, size);
            }
        }
    }
}

/// `PRAGMA [schema.]page_size[=N]`: informa ou muda o tamanho de página do banco. Só pode ser
/// mudado se o banco ainda não foi criado.
fn pragma_page_size(db: &mut Connection, parse: &mut Parse, c: &PragmaCtx) {
    match c.z_right {
        None => {
            let size = db.dbs[c.i_db as usize].bt.as_ref().map_or(0, btree_get_page_size);
            return_single_int(vdbe_of_parse(parse), size as i64);
        }
        Some(zr) => {
            // A alocação pode falhar ao mudar o tamanho de página, porque há um buffer interno
            // que o pager redimensiona com `sqlite3_realloc()`.
            db.next_pagesize = atoi(zr);
            let next = db.next_pagesize;
            let rc = match db.dbs[c.i_db as usize].bt.as_mut() {
                Some(bt) => btree_set_page_size(bt, next, 0, 0),
                None => SQLITE_OK,
            };
            if rc == SQLITE_NOMEM {
                oom_fault(db);
            }
        }
    }
}

/// `PRAGMA [schema.]secure_delete[=ON|OFF|FAST]`: informa ou muda a flag secure_delete e devolve
/// o valor novo.
fn pragma_secure_delete(db: &mut Connection, parse: &mut Parse, c: &PragmaCtx) {
    let mut b: i32 = -1;
    if let Some(zr) = c.z_right {
        if str_icmp(zr, b"fast") == 0 {
            b = 2;
        } else {
            b = get_boolean(zr, false) as i32;
        }
    }
    if c.id2_n == 0 && b >= 0 {
        for slot in db.dbs.iter_mut() {
            if let Some(bt) = slot.bt.as_mut() {
                btree_secure_delete(bt, b);
            }
        }
    }
    let b = match db.dbs[c.i_db as usize].bt.as_mut() {
        Some(bt) => btree_secure_delete(bt, b),
        None => 0,
    };
    return_single_int(vdbe_of_parse(parse), b as i64);
}

/// `PRAGMA [schema.]max_page_count[=N]` e `PRAGMA [schema.]page_count`: informam (e, o primeiro,
/// tentam mudar) o limite de páginas do arquivo, ou o número de páginas do banco.
///
/// O valor absoluto de N é usado. Isso não é documentado e pode mudar; serve só para testar
/// `sqlite3AbsInt32()` de modo fácil.
fn pragma_page_count(db: &mut Connection, parse: &mut Parse, c: &PragmaCtx) {
    let mut x: i64 = 0;
    code_verify_schema(db, parse, c.i_db);
    parse.n_mem += 1;
    let i_reg = parse.n_mem;
    if to_lower(at(c.z_left, 0)) == b'p' {
        add_op2(vdbe_of_parse(parse), OP_PAGECOUNT, c.i_db, i_reg);
    } else {
        match c.z_right {
            Some(zr) if dec_or_hex_to_i64(zr, &mut x) == 0 => {
                if x < 0 {
                    x = 0;
                } else if x > 0xfffffffe {
                    x = 0xfffffffe;
                }
            }
            _ => x = 0,
        }
        add_op3(vdbe_of_parse(parse), OP_MAXPGCNT, c.i_db, i_reg, x as i32);
    }
    add_op2(vdbe_of_parse(parse), OP_RESULTROW, i_reg, 1);
}

/// `PRAGMA [schema.]locking_mode[=normal|exclusive]`.
fn pragma_locking_mode(db: &mut Connection, parse: &mut Parse, c: &PragmaCtx) {
    let mut e_mode = get_locking_mode(c.z_right);
    if c.id2_n == 0 && e_mode == PAGER_LOCKINGMODE_QUERY {
        // Um simples "PRAGMA locking_mode;" é uma consulta do modo de trava padrão (que pode ser
        // diferente do modo do banco principal).
        e_mode = db.dflt_lock_mode as i32;
    } else {
        if c.id2_n == 0 {
            // Nenhum banco foi nomeado no comando: o modo vale para todos os bancos anexados e
            // para o principal. `dflt_lock_mode` também muda, para os bancos anexados depois.
            for ii in 2..db.dbs.len() {
                if let Some(bt) = db.dbs[ii].bt.as_mut() {
                    bt.bt.pager.locking_mode(e_mode);
                }
            }
            db.dflt_lock_mode = e_mode as u8;
        }
        if let Some(bt) = db.dbs[c.i_db as usize].bt.as_mut() {
            e_mode = bt.bt.pager.locking_mode(e_mode);
        }
    }
    debug_assert!(e_mode == PAGER_LOCKINGMODE_NORMAL || e_mode == PAGER_LOCKINGMODE_EXCLUSIVE);
    let z_ret: &[u8] = if e_mode == PAGER_LOCKINGMODE_EXCLUSIVE { b"exclusive" } else { b"normal" };
    return_single_text(vdbe_of_parse(parse), Some(z_ret));
}

/// `PRAGMA [schema.]journal_mode[=delete|persist|off|truncate|memory|wal]`.
fn pragma_journal_mode(db: &mut Connection, parse: &mut Parse, c: &PragmaCtx) {
    let mut i_db = c.i_db;
    let mut id2_n = c.id2_n;
    let mut e_mode: i32;
    match c.z_right {
        // Sem a parte "=MODO" é uma consulta ao modo corrente.
        None => e_mode = PAGER_JOURNALMODE_QUERY,
        Some(zr) => {
            let n = strlen30(zr);
            let found = AZ_MODE_NAME.iter().position(|z_mode| strnicmp(Some(zr), Some(*z_mode), n) == 0);
            // Se a parte "=MODO" não casa com nenhum modo conhecido, é uma consulta.
            e_mode = found.map_or(PAGER_JOURNALMODE_QUERY, |e| e as i32);
            if e_mode == PAGER_JOURNALMODE_OFF && (db.flags & SQLITE_DEFENSIVE) != 0 {
                // O journal-mode "OFF" é proibido no modo defensivo, porque o banco pode ser
                // corrompido por SQL comum com o journal desligado.
                e_mode = PAGER_JOURNALMODE_QUERY;
            }
        }
    }
    if e_mode == PAGER_JOURNALMODE_QUERY && id2_n == 0 {
        // Converte "PRAGMA journal_mode" em "PRAGMA main.journal_mode".
        i_db = 0;
        id2_n = 1;
    }
    for ii in (0..db.dbs.len() as i32).rev() {
        if db.dbs[ii as usize].bt.is_some() && (ii == i_db || id2_n == 0) {
            let v = vdbe_of_parse(parse);
            uses_btree(v, ii);
            add_op3(v, OP_JOURNALMODE, ii, 1, e_mode);
        }
    }
    add_op2(vdbe_of_parse(parse), OP_RESULTROW, 1, 1);
}

/// `PRAGMA [schema.]journal_size_limit[=N]`: informa ou muda o limite de tamanho dos arquivos de
/// journal de reversão.
fn pragma_journal_size_limit(db: &mut Connection, parse: &mut Parse, c: &PragmaCtx) {
    let mut i_limit: i64 = -2;
    if let Some(zr) = c.z_right {
        dec_or_hex_to_i64(zr, &mut i_limit);
        if i_limit < -1 {
            i_limit = -1;
        }
    }
    if let Some(bt) = db.dbs[c.i_db as usize].bt.as_mut() {
        i_limit = bt.bt.pager.journal_size_limit(i_limit);
    }
    return_single_int(vdbe_of_parse(parse), i_limit);
}

/// `PRAGMA [schema.]auto_vacuum[=N]`: informa ou muda o parâmetro 'auto-vacuum' do banco: 0 NONE,
/// 1 FULL, 2 INCREMENTAL.
fn pragma_auto_vacuum(db: &mut Connection, parse: &mut Parse, c: &PragmaCtx) {
    let Some(zr) = c.z_right else {
        let cur = db.dbs[c.i_db as usize].bt.as_ref().map_or(0, btree_get_auto_vacuum);
        return_single_int(vdbe_of_parse(parse), cur as i64);
        return;
    };
    let e_auto = get_auto_vacuum(zr);
    debug_assert!((0..=2).contains(&e_auto));
    db.next_autovac = e_auto as i8;
    // Chama `btree_set_auto_vacuum` para inicializar as flags internas de auto e incr-vacuum.
    // Isso é preciso caso esta conexão crie o arquivo, que precisa nascer capaz de auto-vacuum.
    let rc = match db.dbs[c.i_db as usize].bt.as_mut() {
        Some(bt) => btree_set_auto_vacuum(bt, e_auto),
        None => SQLITE_OK,
    };
    if rc == SQLITE_OK && (e_auto == 1 || e_auto == 2) {
        // Ao pôr o auto_vacuum em "full" ou "incremental", grava o valor de meta[6] no arquivo.
        // Antes, confere em meta[3] se o banco é mesmo capaz de auto-vacuum.
        let set_meta6: [VdbeOpList; 5] = [
            ol(OP_TRANSACTION, 0, 1, 0),                                  /* 0 */
            ol(OP_READCOOKIE, 0, 1, BTREE_LARGEST_ROOT_PAGE as i8),
            ol(OP_IF, 1, 0, 0),                                           /* 2 */
            ol(OP_HALT, SQLITE_OK as i8, OE_ABORT as i8, 0),              /* 3 */
            ol(OP_SETCOOKIE, 0, BTREE_INCR_VACUUM as i8, 0),              /* 4 */
        ];
        let i_addr = current_addr(parse);
        let v = vdbe_of_parse(parse);
        if let Some(a) = add_op_list(v, &set_meta6, 2) {
            v.a_op[a].p1 = c.i_db;
            v.a_op[a + 1].p1 = c.i_db;
            v.a_op[a + 2].p2 = i_addr + 4;
            v.a_op[a + 4].p1 = c.i_db;
            v.a_op[a + 4].p3 = e_auto - 1;
            uses_btree(v, c.i_db);
        }
    }
}

/// `PRAGMA [schema.]incremental_vacuum(N)`: faz N passos de vacuum incremental no banco.
fn pragma_incremental_vacuum(db: &mut Connection, parse: &mut Parse, c: &PragmaCtx) {
    let i_limit = match c.z_right.and_then(get_int32) {
        Some(n) if n > 0 => n,
        _ => 0x7fffffff,
    };
    begin_write_operation(db, parse, 0, c.i_db);
    let v = vdbe_of_parse(parse);
    add_op2(v, OP_INTEGER, i_limit, 1);
    let addr = add_op1(v, OP_INCRVACUUM, c.i_db);
    add_op1(v, OP_RESULTROW, 1);
    add_op2(v, OP_ADDIMM, 1, -1);
    add_op2(v, OP_IFPOS, 1, addr);
    jump_here(v, addr);
}

/// `PRAGMA [schema.]cache_size[=N]`: o primeiro modo informa o tamanho local do cache de
/// páginas; o segundo o muda. Se N é positivo é o número de páginas; se é negativo, o número de
/// páginas se ajusta para que o cache use -N kibibytes de memória.
fn pragma_cache_size(db: &mut Connection, parse: &mut Parse, c: &PragmaCtx) {
    let slot = &mut db.dbs[c.i_db as usize];
    match c.z_right {
        None => return_single_int(vdbe_of_parse(parse), slot.schema.cache_size as i64),
        Some(zr) => {
            let size = atoi(zr);
            slot.schema.cache_size = size;
            if let Some(bt) = slot.bt.as_mut() {
                btree_set_cache_size(bt, size);
            }
        }
    }
}

/// `PRAGMA [schema.]cache_spill`, `PRAGMA cache_spill=BOOLEAN` e `PRAGMA [schema.]cache_spill=N`:
/// o primeiro modo informa o tamanho local de spill do cache; o segundo liga ou desliga o spill
/// (ao ligar, o tamanho é o `cache_size` corrente); o terceiro fixa um tamanho que pode diferir do
/// do cache. Com N positivo é o número de páginas; com N negativo o cache usa -N kibibytes. Se as
/// páginas de spill são menos que as do cache, não há spill até a contagem passar as do cache.
///
/// O `cache_spill=BOOLEAN` vale para todos os esquemas anexados, não só o nomeado.
fn pragma_cache_spill(db: &mut Connection, parse: &mut Parse, c: &PragmaCtx) {
    match c.z_right {
        None => {
            let v = if (db.flags & SQLITE_CACHE_SPILL) == 0 {
                0
            } else {
                db.dbs[c.i_db as usize].bt.as_mut().map_or(0, |bt| btree_set_spill_size(bt, 0))
            };
            return_single_int(vdbe_of_parse(parse), v as i64);
        }
        Some(zr) => {
            let mut size = 1;
            if let Some(n) = get_int32(zr) {
                size = n;
                if let Some(bt) = db.dbs[c.i_db as usize].bt.as_mut() {
                    btree_set_spill_size(bt, size);
                }
            }
            if get_boolean(zr, size != 0) {
                db.flags |= SQLITE_CACHE_SPILL;
            } else {
                db.flags &= !SQLITE_CACHE_SPILL;
            }
            set_all_pager_flags(db);
        }
    }
}

/// `PRAGMA [schema.]mmap_size(N)`: fixa o limite do tamanho do mapeamento, que limita o tamanho
/// agregado de todas as regiões do arquivo mapeadas na memória. Zero desliga o mapeamento; N
/// negativo restaura o padrão de `sqlite3_config(SQLITE_CONFIG_MMAP_SIZE)`. N é em bytes.
///
/// O valor é uma sugestão: o VFS mapeia o que quiser, exceto com N zero, caso em que as camadas
/// de cima nunca chamam `xFetch`.
fn pragma_mmap_size(db: &mut Connection, parse: &mut Parse, c: &PragmaCtx) {
    if let Some(zr) = c.z_right {
        let mut sz: i64 = 0;
        dec_or_hex_to_i64(zr, &mut sz);
        if sz < 0 {
            sz = SQLITE_DEFAULT_MMAP_SIZE as i64;
        }
        if c.id2_n == 0 {
            db.sz_mmap = sz;
        }
        for ii in (0..db.dbs.len() as i32).rev() {
            if ii == c.i_db || c.id2_n == 0 {
                if let Some(bt) = db.dbs[ii as usize].bt.as_mut() {
                    btree_set_mmap_limit(bt, sz);
                }
            }
        }
    }
    let mut arg = FileControlArg::Int64(-1);
    let rc = file_control(db, c.z_db, SQLITE_FCNTL_MMAP_SIZE, &mut arg);
    if rc == SQLITE_OK {
        let sz = match arg {
            FileControlArg::Int64(n) => n,
            _ => -1,
        };
        return_single_int(vdbe_of_parse(parse), sz);
    } else if rc != SQLITE_NOTFOUND {
        parse.n_err += 1;
        parse.rc = rc;
    }
}

/// `PRAGMA [schema.]synchronous[=OFF|ON|NORMAL|FULL|EXTRA]`: informa ou muda o valor local da
/// flag synchronous. Mudar o valor local não altera o arquivo, e o padrão volta na próxima
/// abertura do banco.
fn pragma_synchronous(db: &mut Connection, parse: &mut Parse, c: &PragmaCtx) {
    match c.z_right {
        None => {
            let level = db.dbs[c.i_db as usize].safety_level as i64;
            return_single_int(vdbe_of_parse(parse), level - 1);
        }
        Some(zr) => {
            if db.auto_commit == 0 {
                error_msg(db, parse, b"Safety level may not be changed inside a transaction", &[]);
            } else if c.i_db != 1 {
                let mut i_level = (get_safety_level(zr, false, 1) as u32 + 1) & PAGER_SYNCHRONOUS_MASK;
                if i_level == 0 {
                    i_level = 1;
                }
                let slot = &mut db.dbs[c.i_db as usize];
                slot.safety_level = i_level as u8;
                slot.b_sync_set = true;
                set_all_pager_flags(db);
            }
        }
    }
}

/// Os pragmas de flag (`PragTyp_FLAG`): liga ou desliga os bits de `db.flags`.
fn pragma_flag(db: &mut Connection, parse: &mut Parse, c: &PragmaCtx) {
    match c.z_right {
        None => {
            set_pragma_result_column_names(vdbe_of_parse(parse), db, c.pragma);
            return_single_int(vdbe_of_parse(parse), ((db.flags & c.pragma.i_arg) != 0) as i64);
        }
        Some(zr) => {
            let mut mask: u64 = c.pragma.i_arg; // A máscara dos bits a ligar ou apagar.
            if db.auto_commit == 0 {
                // O suporte a chaves estrangeiras não pode ser ligado nem desligado fora do modo
                // de auto-commit.
                mask &= !SQLITE_FOREIGN_KEYS;
            }
            if get_boolean(zr, false) {
                if (mask & SQLITE_WRITE_SCHEMA) == 0 || (db.flags & SQLITE_DEFENSIVE) == 0 {
                    db.flags |= mask;
                }
            } else {
                db.flags &= !mask;
                if mask == SQLITE_DEFER_FKS {
                    db.n_deferred_imm_cons = 0;
                }
                if (mask & SQLITE_WRITE_SCHEMA) != 0 && str_icmp(zr, b"reset") == 0 {
                    // IMP: R-60817-01178 If the argument is "RESET" then schema writing is
                    // disabled (as with "PRAGMA writable_schema=OFF") and, in addition, the
                    // schema is reloaded.
                    reset_all_schemas_of_connection(db);
                }
            }
            // Muitos pragmas de flag mudam o código que o compilador SQL gera (como o
            // count_changes). Por isso um opcode expira todos os comandos compilados depois de
            // mudar um valor.
            add_op0(vdbe_of_parse(parse), OP_EXPIRE);
            set_all_pager_flags(db);
        }
    }
}

/// `PRAGMA database_list`: uma linha por banco aberto.
fn pragma_database_list(db: &mut Connection, parse: &mut Parse) {
    parse.n_mem = 3;
    for i in 0..db.dbs.len() {
        if db.dbs[i].bt.is_none() {
            continue;
        }
        let name = db.dbs[i].z_db_s_name.clone();
        let file = db_filename(db, Some(name.as_slice())).unwrap_or_default();
        multi_load(
            vdbe_of_parse(parse),
            1,
            b"iss",
            &[PrintfArg::Int(i as i64), text_arg(&name), text_arg(&file)],
        );
    }
}

/// `PRAGMA collation_list`: uma linha por colação registrada.
fn pragma_collation_list(db: &mut Connection, parse: &mut Parse) {
    parse.n_mem = 2;
    for (i, (key, colls)) in hash_iter(&db.a_coll_seq).enumerate() {
        let name = colls.iter().flatten().next().map_or_else(|| key.to_vec(), |p| p.name.clone());
        multi_load(vdbe_of_parse(parse), 1, b"is", &[PrintfArg::Int(i as i64), text_arg(&name)]);
    }
}

/// `PRAGMA [schema.]schema_version`, `user_version`, `freelist_count`, `data_version` e
/// `application_id`, com ou sem `=<inteiro>`: leem ou gravam o valor no cabeçalho do banco.
///
/// O cookie do esquema só costuma ser mexido pelo próprio SQLite: sobe a cada mudança do esquema
/// (criar ou apagar tabela ou índice), e o SQLite o confere a cada execução para saber se o cache
/// do esquema usado na compilação bate com o do banco contra o qual o comando vai rodar. Subverter
/// isso com `PRAGMA schema_version` é perigoso e pode derrubar programas ou corromper o banco. O
/// `user_version` não é usado pelo SQLite: serve às aplicações.
fn pragma_header_value(db: &mut Connection, parse: &mut Parse, c: &PragmaCtx) {
    let i_cookie = c.pragma.i_arg as i32; // O cookie a ler ou gravar.
    uses_btree(vdbe_of_parse(parse), c.i_db);
    match c.z_right {
        Some(zr) if (c.pragma.m_prag_flg & PRAG_FLG_READ_ONLY) == 0 => {
            // Grava o valor dado.
            let set_cookie: [VdbeOpList; 2] = [
                ol(OP_TRANSACTION, 0, 1, 0), /* 0 */
                ol(OP_SETCOOKIE, 0, 0, 0),   /* 1 */
            ];
            let v = vdbe_of_parse(parse);
            if let Some(a) = add_op_list(v, &set_cookie, 0) {
                v.a_op[a].p1 = c.i_db;
                v.a_op[a + 1].p1 = c.i_db;
                v.a_op[a + 1].p2 = i_cookie;
                v.a_op[a + 1].p3 = atoi(zr);
                v.a_op[a + 1].p5 = 1;
                if i_cookie == BTREE_SCHEMA_VERSION as i32 && (db.flags & SQLITE_DEFENSIVE) != 0 {
                    // O uso de PRAGMA schema_version=VALOR é proibido no modo defensivo: o
                    // OP_SetCookie vira um no-op.
                    v.a_op[a + 1].opcode = OP_NOOP;
                }
            }
        }
        _ => {
            // Lê o valor pedido.
            let read_cookie: [VdbeOpList; 3] = [
                ol(OP_TRANSACTION, 0, 0, 0), /* 0 */
                ol(OP_READCOOKIE, 0, 1, 0),  /* 1 */
                ol(OP_RESULTROW, 1, 1, 0),
            ];
            let v = vdbe_of_parse(parse);
            if let Some(a) = add_op_list(v, &read_cookie, 0) {
                v.a_op[a].p1 = c.i_db;
                v.a_op[a + 1].p1 = c.i_db;
                v.a_op[a + 1].p3 = i_cookie;
                reusable(v);
            }
        }
    }
}

/// `PRAGMA encoding` e `PRAGMA encoding = "utf-8"|"utf-16"|"utf-16le"|"utf-16be"`.
///
/// O primeiro modo devolve a codificação do banco principal, inicializando-o se preciso. O
/// segundo é um no-op se o arquivo principal já foi inicializado; senão fixa a codificação padrão
/// do arquivo principal caso um novo seja criado. Se um banco principal existente é aberto, vale
/// a codificação dele. Em todos os casos os bancos novos criados por ATTACH usam a mesma
/// codificação do principal; se o principal ainda não foi inicializado e criado, isso é feito
/// antes do ATTACH.
fn pragma_encoding(db: &mut Connection, parse: &mut Parse, c: &PragmaCtx) {
    const ENC_NAMES: [(&str, i32); 8] = [
        ("UTF8", SQLITE_UTF8),
        ("UTF-8", SQLITE_UTF8), // Precisa ser o elemento [1]
        ("UTF-16le", SQLITE_UTF16LE), // Precisa ser o elemento [2]
        ("UTF-16be", SQLITE_UTF16BE), // Precisa ser o elemento [3]
        ("UTF16le", SQLITE_UTF16LE),
        ("UTF16be", SQLITE_UTF16BE),
        ("UTF-16", 0), // SQLITE_UTF16NATIVE
        ("UTF16", 0),  // SQLITE_UTF16NATIVE
    ];
    match c.z_right {
        None => {
            // "PRAGMA encoding"
            if read_schema(db, parse) != 0 {
                return;
            }
            let z = ENC_NAMES[db.enc as usize].0;
            return_single_text(vdbe_of_parse(parse), Some(z.as_bytes()));
        }
        Some(zr) => {
            // "PRAGMA encoding = XXX": só muda `db.enc` se a conexão não foi inicializada. Se o
            // banco principal existe, o valor novo é sobrescrito na próxima carga do esquema; se
            // não existe, ele é criado com a codificação nova.
            if (db.m_db_flags & DBFLAG_ENCODING_FIXED) == 0 {
                let mut found = false;
                for (z_name, e) in ENC_NAMES.iter() {
                    if str_icmp(zr, z_name.as_bytes()) == 0 {
                        let enc = (if *e != 0 { *e } else { SQLITE_UTF16NATIVE }) as u8;
                        db.dbs[0].schema.enc = enc;
                        set_text_encoding(db, enc);
                        found = true;
                        break;
                    }
                }
                if !found {
                    error_msg(db, parse, b"unsupported encoding: %s", &[text_arg(zr)]);
                }
            }
        }
    }
}

/// `PRAGMA wal_autocheckpoint[=N]`: configura a conexão para fazer checkpoint do banco depois de
/// acumular N quadros no log, ou consulta o N corrente.
fn pragma_wal_autocheckpoint(db: &mut Connection, parse: &mut Parse, c: &PragmaCtx) {
    if let Some(zr) = c.z_right {
        let n = atoi(zr);
        wal_autocheckpoint(db, n);
        wal_autocheckpoint_remember(db, n);
    }
    let cur = wal_autocheckpoint_value(db);
    return_single_int(vdbe_of_parse(parse), cur as i64);
}
