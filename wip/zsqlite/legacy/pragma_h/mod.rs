// Mesclado das partes traduzidas de pragma_h (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

// Tipos de pragma
pub const PRAG_TYP_ACTIVATE_EXTENSIONS: u8 = 0;
pub const PRAG_TYP_ANALYSIS_LIMIT: u8 = 1;
pub const PRAG_TYP_HEADER_VALUE: u8 = 2;
pub const PRAG_TYP_AUTO_VACUUM: u8 = 3;
pub const PRAG_TYP_FLAG: u8 = 4;
pub const PRAG_TYP_BUSY_TIMEOUT: u8 = 5;
pub const PRAG_TYP_CACHE_SIZE: u8 = 6;
pub const PRAG_TYP_CACHE_SPILL: u8 = 7;
pub const PRAG_TYP_CASE_SENSITIVE_LIKE: u8 = 8;
pub const PRAG_TYP_COLLATION_LIST: u8 = 9;
pub const PRAG_TYP_COMPILE_OPTIONS: u8 = 10;
pub const PRAG_TYP_DATA_STORE_DIRECTORY: u8 = 11;
pub const PRAG_TYP_DATABASE_LIST: u8 = 12;
pub const PRAG_TYP_DEFAULT_CACHE_SIZE: u8 = 13;
pub const PRAG_TYP_ENCODING: u8 = 14;
pub const PRAG_TYP_FOREIGN_KEY_CHECK: u8 = 15;
pub const PRAG_TYP_FOREIGN_KEY_LIST: u8 = 16;
pub const PRAG_TYP_FUNCTION_LIST: u8 = 17;
pub const PRAG_TYP_HARD_HEAP_LIMIT: u8 = 18;
pub const PRAG_TYP_INCREMENTAL_VACUUM: u8 = 19;
pub const PRAG_TYP_INDEX_INFO: u8 = 20;
pub const PRAG_TYP_INDEX_LIST: u8 = 21;
pub const PRAG_TYP_INTEGRITY_CHECK: u8 = 22;
pub const PRAG_TYP_JOURNAL_MODE: u8 = 23;
pub const PRAG_TYP_JOURNAL_SIZE_LIMIT: u8 = 24;
pub const PRAG_TYP_LOCK_PROXY_FILE: u8 = 25;
pub const PRAG_TYP_LOCKING_MODE: u8 = 26;
pub const PRAG_TYP_PAGE_COUNT: u8 = 27;
pub const PRAG_TYP_MMAP_SIZE: u8 = 28;
pub const PRAG_TYP_MODULE_LIST: u8 = 29;
pub const PRAG_TYP_OPTIMIZE: u8 = 30;
pub const PRAG_TYP_PAGE_SIZE: u8 = 31;
pub const PRAG_TYP_PRAGMA_LIST: u8 = 32;
pub const PRAG_TYP_SECURE_DELETE: u8 = 33;
pub const PRAG_TYP_SHRINK_MEMORY: u8 = 34;
pub const PRAG_TYP_SOFT_HEAP_LIMIT: u8 = 35;
pub const PRAG_TYP_SYNCHRONOUS: u8 = 36;
pub const PRAG_TYP_TABLE_INFO: u8 = 37;
pub const PRAG_TYP_TABLE_LIST: u8 = 38;
pub const PRAG_TYP_TEMP_STORE: u8 = 39;
pub const PRAG_TYP_TEMP_STORE_DIRECTORY: u8 = 40;
pub const PRAG_TYP_THREADS: u8 = 41;
pub const PRAG_TYP_WAL_AUTOCHECKPOINT: u8 = 42;
pub const PRAG_TYP_WAL_CHECKPOINT: u8 = 43;
pub const PRAG_TYP_LOCK_STATUS: u8 = 44;
pub const PRAG_TYP_STATS: u8 = 45;

// Sinalizadores de propriedade associados a diversos pragmas
pub const PRAG_FLG_NEED_SCHEMA: u8 = 0x01; // Força carregamento de esquema antes de executar
pub const PRAG_FLG_NO_COLUMNS: u8 = 0x02; // OP_ResultRow chamado com zero colunas
pub const PRAG_FLG_NO_COLUMNS1: u8 = 0x04; // Zero colunas se o argumento do lado direito estiver presente
pub const PRAG_FLG_READ_ONLY: u8 = 0x08; // HEADER_VALUE somente leitura
pub const PRAG_FLG_RESULT0: u8 = 0x10; // Atua como consulta quando não há argumento
pub const PRAG_FLG_RESULT1: u8 = 0x20; // Atua como consulta quando há um argumento
pub const PRAG_FLG_SCHEMA_OPT: u8 = 0x40; // Esquema restringe a busca de nome se presente
pub const PRAG_FLG_SCHEMA_REQ: u8 = 0x80; // Esquema obrigatório; "main" é o padrão

// Nomes de colunas para pragmas que retornam resultado multi-coluna
// ou que retornam resultado de coluna única cujo nome é diferente do nome do pragma
pub static PRAG_C_NAME: &[&str] = &[
    /*   0 */ "id",          // Usado por: foreign_key_list
    /*   1 */ "seq",
    /*   2 */ "table",
    /*   3 */ "from",
    /*   4 */ "to",
    /*   5 */ "on_update",
    /*   6 */ "on_delete",
    /*   7 */ "match",
    /*   8 */ "cid",         // Usado por: table_xinfo
    /*   9 */ "name",
    /*  10 */ "type",
    /*  11 */ "notnull",
    /*  12 */ "dflt_value",
    /*  13 */ "pk",
    /*  14 */ "hidden",
    // table_info reutiliza 8
    /*  15 */ "schema",      // Usado por: table_list
    /*  16 */ "name",
    /*  17 */ "type",
    /*  18 */ "ncol",
    /*  19 */ "wr",
    /*  20 */ "strict",
    /*  21 */ "seqno",       // Usado por: index_xinfo
    /*  22 */ "cid",
    /*  23 */ "name",
    /*  24 */ "desc",
    /*  25 */ "coll",
    /*  26 */ "key",
    /*  27 */ "name",        // Usado por: function_list
    /*  28 */ "builtin",
    /*  29 */ "type",
    /*  30 */ "enc",
    /*  31 */ "narg",
    /*  32 */ "flags",
    /*  33 */ "tbl",         // Usado por: stats
    /*  34 */ "idx",
    /*  35 */ "wdth",
    /*  36 */ "hght",
    /*  37 */ "flgs",
    /*  38 */ "seq",         // Usado por: index_list
    /*  39 */ "name",
    /*  40 */ "unique",
    /*  41 */ "origin",
    /*  42 */ "partial",
    /*  43 */ "table",       // Usado por: foreign_key_check
    /*  44 */ "rowid",
    /*  45 */ "parent",
    /*  46 */ "fkid",
    // index_info reutiliza 21
    /*  47 */ "seq",         // Usado por: database_list
    /*  48 */ "name",
    /*  49 */ "file",
    /*  50 */ "busy",        // Usado por: wal_checkpoint
    /*  51 */ "log",
    /*  52 */ "checkpointed",
    // collation_list reutiliza 38
    /*  53 */ "database",    // Usado por: lock_status
    /*  54 */ "status",
    /*  55 */ "cache_size",  // Usado por: default_cache_size
    // module_list e pragma_list reutilizam 9
    /*  56 */ "timeout",     // Usado por: busy_timeout
];

/// Definição de cada pragma embutido.
#[derive(Clone, Copy)]
pub struct PragmaName {
    pub z_name: &'static str,  // Nome do pragma
    pub e_prag_typ: u8,        // Valor PRAG_TYP_XXX
    pub m_prag_flg: u8,        // Zero ou mais valores PRAG_FLG_XXX
    pub i_prag_c_name: u8,     // Início dos nomes de coluna em PRAG_C_NAME
    pub n_prag_c_name: u8,     // Quantidade de nomes de coluna; 0 significa usar o nome do pragma
    pub i_arg: u64,            // Argumento extra
}

// Os `#if` do C foram resolvidos para o build do Debian 13: nenhum SQLITE_OMIT_* definido, e
// ficam de fora os pragmas de SQLITE_ENABLE_CEROD (activate_extensions), de Windows
// (data_store_directory), de SQLITE_ENABLE_LOCKING_STYLE (lock_proxy_file) e de
// SQLITE_DEBUG/SQLITE_TEST (lock_status, parser_trace, sql_trace, stats, vdbe_*).
pub static A_PRAGMA_NAME: &[PragmaName] = &[
    PragmaName {
        z_name: "analysis_limit",
        e_prag_typ: PRAG_TYP_ANALYSIS_LIMIT,
        m_prag_flg: PRAG_FLG_RESULT0,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: 0,
    },
    PragmaName {
        z_name: "application_id",
        e_prag_typ: PRAG_TYP_HEADER_VALUE,
        m_prag_flg: PRAG_FLG_NO_COLUMNS1 | PRAG_FLG_RESULT0,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: BTREE_APPLICATION_ID as u64,
    },
    PragmaName {
        z_name: "auto_vacuum",
        e_prag_typ: PRAG_TYP_AUTO_VACUUM,
        m_prag_flg: PRAG_FLG_NEED_SCHEMA | PRAG_FLG_RESULT0 | PRAG_FLG_SCHEMA_REQ | PRAG_FLG_NO_COLUMNS1,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: 0,
    },
    PragmaName {
        z_name: "automatic_index",
        e_prag_typ: PRAG_TYP_FLAG,
        m_prag_flg: PRAG_FLG_RESULT0 | PRAG_FLG_NO_COLUMNS1,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: SQLITE_AUTO_INDEX as u64,
    },
    PragmaName {
        z_name: "busy_timeout",
        e_prag_typ: PRAG_TYP_BUSY_TIMEOUT,
        m_prag_flg: PRAG_FLG_RESULT0,
        i_prag_c_name: 56,
        n_prag_c_name: 1,
        i_arg: 0,
    },
    PragmaName {
        z_name: "cache_size",
        e_prag_typ: PRAG_TYP_CACHE_SIZE,
        m_prag_flg: PRAG_FLG_NEED_SCHEMA | PRAG_FLG_RESULT0 | PRAG_FLG_SCHEMA_REQ | PRAG_FLG_NO_COLUMNS1,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: 0,
    },
    PragmaName {
        z_name: "cache_spill",
        e_prag_typ: PRAG_TYP_CACHE_SPILL,
        m_prag_flg: PRAG_FLG_RESULT0 | PRAG_FLG_SCHEMA_REQ | PRAG_FLG_NO_COLUMNS1,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: 0,
    },
    PragmaName {
        z_name: "case_sensitive_like",
        e_prag_typ: PRAG_TYP_CASE_SENSITIVE_LIKE,
        m_prag_flg: PRAG_FLG_NO_COLUMNS,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: 0,
    },
    PragmaName {
        z_name: "cell_size_check",
        e_prag_typ: PRAG_TYP_FLAG,
        m_prag_flg: PRAG_FLG_RESULT0 | PRAG_FLG_NO_COLUMNS1,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: SQLITE_CELL_SIZE_CK as u64,
    },
    PragmaName {
        z_name: "checkpoint_fullfsync",
        e_prag_typ: PRAG_TYP_FLAG,
        m_prag_flg: PRAG_FLG_RESULT0 | PRAG_FLG_NO_COLUMNS1,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: SQLITE_CKPT_FULL_FSYNC as u64,
    },
    PragmaName {
        z_name: "collation_list",
        e_prag_typ: PRAG_TYP_COLLATION_LIST,
        m_prag_flg: PRAG_FLG_RESULT0,
        i_prag_c_name: 38,
        n_prag_c_name: 2,
        i_arg: 0,
    },
    PragmaName {
        z_name: "compile_options",
        e_prag_typ: PRAG_TYP_COMPILE_OPTIONS,
        m_prag_flg: PRAG_FLG_RESULT0,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: 0,
    },
    PragmaName {
        z_name: "count_changes",
        e_prag_typ: PRAG_TYP_FLAG,
        m_prag_flg: PRAG_FLG_RESULT0 | PRAG_FLG_NO_COLUMNS1,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: SQLITE_COUNT_ROWS as u64,
    },
    PragmaName {
        z_name: "data_version",
        e_prag_typ: PRAG_TYP_HEADER_VALUE,
        m_prag_flg: PRAG_FLG_READ_ONLY | PRAG_FLG_RESULT0,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: BTREE_DATA_VERSION as u64,
    },
    PragmaName {
        z_name: "database_list",
        e_prag_typ: PRAG_TYP_DATABASE_LIST,
        m_prag_flg: PRAG_FLG_RESULT0,
        i_prag_c_name: 47,
        n_prag_c_name: 3,
        i_arg: 0,
    },
    PragmaName {
        z_name: "default_cache_size",
        e_prag_typ: PRAG_TYP_DEFAULT_CACHE_SIZE,
        m_prag_flg: PRAG_FLG_NEED_SCHEMA | PRAG_FLG_RESULT0 | PRAG_FLG_SCHEMA_REQ | PRAG_FLG_NO_COLUMNS1,
        i_prag_c_name: 55,
        n_prag_c_name: 1,
        i_arg: 0,
    },
    PragmaName {
        z_name: "defer_foreign_keys",
        e_prag_typ: PRAG_TYP_FLAG,
        m_prag_flg: PRAG_FLG_RESULT0 | PRAG_FLG_NO_COLUMNS1,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: SQLITE_DEFER_FKS as u64,
    },
    PragmaName {
        z_name: "empty_result_callbacks",
        e_prag_typ: PRAG_TYP_FLAG,
        m_prag_flg: PRAG_FLG_RESULT0 | PRAG_FLG_NO_COLUMNS1,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: SQLITE_NULL_CALLBACK as u64,
    },
    PragmaName {
        z_name: "encoding",
        e_prag_typ: PRAG_TYP_ENCODING,
        m_prag_flg: PRAG_FLG_RESULT0 | PRAG_FLG_NO_COLUMNS1,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: 0,
    },
    PragmaName {
        z_name: "foreign_key_check",
        e_prag_typ: PRAG_TYP_FOREIGN_KEY_CHECK,
        m_prag_flg: PRAG_FLG_NEED_SCHEMA | PRAG_FLG_RESULT0 | PRAG_FLG_RESULT1 | PRAG_FLG_SCHEMA_OPT,
        i_prag_c_name: 43,
        n_prag_c_name: 4,
        i_arg: 0,
    },
    PragmaName {
        z_name: "foreign_key_list",
        e_prag_typ: PRAG_TYP_FOREIGN_KEY_LIST,
        m_prag_flg: PRAG_FLG_NEED_SCHEMA | PRAG_FLG_RESULT1 | PRAG_FLG_SCHEMA_OPT,
        i_prag_c_name: 0,
        n_prag_c_name: 8,
        i_arg: 0,
    },
    PragmaName {
        z_name: "foreign_keys",
        e_prag_typ: PRAG_TYP_FLAG,
        m_prag_flg: PRAG_FLG_RESULT0 | PRAG_FLG_NO_COLUMNS1,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: SQLITE_FOREIGN_KEYS as u64,
    },
    PragmaName {
        z_name: "freelist_count",
        e_prag_typ: PRAG_TYP_HEADER_VALUE,
        m_prag_flg: PRAG_FLG_READ_ONLY | PRAG_FLG_RESULT0,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: BTREE_FREE_PAGE_COUNT as u64,
    },
    PragmaName {
        z_name: "full_column_names",
        e_prag_typ: PRAG_TYP_FLAG,
        m_prag_flg: PRAG_FLG_RESULT0 | PRAG_FLG_NO_COLUMNS1,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: SQLITE_FULL_COL_NAMES as u64,
    },
    PragmaName {
        z_name: "fullfsync",
        e_prag_typ: PRAG_TYP_FLAG,
        m_prag_flg: PRAG_FLG_RESULT0 | PRAG_FLG_NO_COLUMNS1,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: SQLITE_FULL_FSYNC as u64,
    },
    PragmaName {
        z_name: "function_list",
        e_prag_typ: PRAG_TYP_FUNCTION_LIST,
        m_prag_flg: PRAG_FLG_RESULT0,
        i_prag_c_name: 27,
        n_prag_c_name: 6,
        i_arg: 0,
    },
    PragmaName {
        z_name: "hard_heap_limit",
        e_prag_typ: PRAG_TYP_HARD_HEAP_LIMIT,
        m_prag_flg: PRAG_FLG_RESULT0,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: 0,
    },
    PragmaName {
        z_name: "ignore_check_constraints",
        e_prag_typ: PRAG_TYP_FLAG,
        m_prag_flg: PRAG_FLG_RESULT0 | PRAG_FLG_NO_COLUMNS1,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: SQLITE_IGNORE_CHECKS as u64,
    },
    PragmaName {
        z_name: "incremental_vacuum",
        e_prag_typ: PRAG_TYP_INCREMENTAL_VACUUM,
        m_prag_flg: PRAG_FLG_NEED_SCHEMA | PRAG_FLG_NO_COLUMNS,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: 0,
    },
    PragmaName {
        z_name: "index_info",
        e_prag_typ: PRAG_TYP_INDEX_INFO,
        m_prag_flg: PRAG_FLG_NEED_SCHEMA | PRAG_FLG_RESULT1 | PRAG_FLG_SCHEMA_OPT,
        i_prag_c_name: 21,
        n_prag_c_name: 3,
        i_arg: 0,
    },
    PragmaName {
        z_name: "index_list",
        e_prag_typ: PRAG_TYP_INDEX_LIST,
        m_prag_flg: PRAG_FLG_NEED_SCHEMA | PRAG_FLG_RESULT1 | PRAG_FLG_SCHEMA_OPT,
        i_prag_c_name: 38,
        n_prag_c_name: 5,
        i_arg: 0,
    },
    PragmaName {
        z_name: "index_xinfo",
        e_prag_typ: PRAG_TYP_INDEX_INFO,
        m_prag_flg: PRAG_FLG_NEED_SCHEMA | PRAG_FLG_RESULT1 | PRAG_FLG_SCHEMA_OPT,
        i_prag_c_name: 21,
        n_prag_c_name: 6,
        i_arg: 1,
    },
    PragmaName {
        z_name: "integrity_check",
        e_prag_typ: PRAG_TYP_INTEGRITY_CHECK,
        m_prag_flg: PRAG_FLG_NEED_SCHEMA | PRAG_FLG_RESULT0 | PRAG_FLG_RESULT1 | PRAG_FLG_SCHEMA_OPT,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: 0,
    },
    PragmaName {
        z_name: "journal_mode",
        e_prag_typ: PRAG_TYP_JOURNAL_MODE,
        m_prag_flg: PRAG_FLG_NEED_SCHEMA | PRAG_FLG_RESULT0 | PRAG_FLG_SCHEMA_REQ,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: 0,
    },
    PragmaName {
        z_name: "journal_size_limit",
        e_prag_typ: PRAG_TYP_JOURNAL_SIZE_LIMIT,
        m_prag_flg: PRAG_FLG_RESULT0 | PRAG_FLG_SCHEMA_REQ,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: 0,
    },
    PragmaName {
        z_name: "legacy_alter_table",
        e_prag_typ: PRAG_TYP_FLAG,
        m_prag_flg: PRAG_FLG_RESULT0 | PRAG_FLG_NO_COLUMNS1,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: SQLITE_LEGACY_ALTER as u64,
    },
    PragmaName {
        z_name: "locking_mode",
        e_prag_typ: PRAG_TYP_LOCKING_MODE,
        m_prag_flg: PRAG_FLG_RESULT0 | PRAG_FLG_SCHEMA_REQ,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: 0,
    },
    PragmaName {
        z_name: "max_page_count",
        e_prag_typ: PRAG_TYP_PAGE_COUNT,
        m_prag_flg: PRAG_FLG_NEED_SCHEMA | PRAG_FLG_RESULT0 | PRAG_FLG_SCHEMA_REQ,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: 0,
    },
    PragmaName {
        z_name: "mmap_size",
        e_prag_typ: PRAG_TYP_MMAP_SIZE,
        m_prag_flg: 0,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: 0,
    },
    PragmaName {
        z_name: "module_list",
        e_prag_typ: PRAG_TYP_MODULE_LIST,
        m_prag_flg: PRAG_FLG_RESULT0,
        i_prag_c_name: 9,
        n_prag_c_name: 1,
        i_arg: 0,
    },
    PragmaName {
        z_name: "optimize",
        e_prag_typ: PRAG_TYP_OPTIMIZE,
        m_prag_flg: PRAG_FLG_RESULT1 | PRAG_FLG_NEED_SCHEMA,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: 0,
    },
    PragmaName {
        z_name: "page_count",
        e_prag_typ: PRAG_TYP_PAGE_COUNT,
        m_prag_flg: PRAG_FLG_NEED_SCHEMA | PRAG_FLG_RESULT0 | PRAG_FLG_SCHEMA_REQ,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: 0,
    },
    PragmaName {
        z_name: "page_size",
        e_prag_typ: PRAG_TYP_PAGE_SIZE,
        m_prag_flg: PRAG_FLG_RESULT0 | PRAG_FLG_SCHEMA_REQ | PRAG_FLG_NO_COLUMNS1,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: 0,
    },
    PragmaName {
        z_name: "pragma_list",
        e_prag_typ: PRAG_TYP_PRAGMA_LIST,
        m_prag_flg: PRAG_FLG_RESULT0,
        i_prag_c_name: 9,
        n_prag_c_name: 1,
        i_arg: 0,
    },
    PragmaName {
        z_name: "query_only",
        e_prag_typ: PRAG_TYP_FLAG,
        m_prag_flg: PRAG_FLG_RESULT0 | PRAG_FLG_NO_COLUMNS1,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: SQLITE_QUERY_ONLY as u64,
    },
    PragmaName {
        z_name: "quick_check",
        e_prag_typ: PRAG_TYP_INTEGRITY_CHECK,
        m_prag_flg: PRAG_FLG_NEED_SCHEMA | PRAG_FLG_RESULT0 | PRAG_FLG_RESULT1 | PRAG_FLG_SCHEMA_OPT,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: 0,
    },
    PragmaName {
        z_name: "read_uncommitted",
        e_prag_typ: PRAG_TYP_FLAG,
        m_prag_flg: PRAG_FLG_RESULT0 | PRAG_FLG_NO_COLUMNS1,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: SQLITE_READ_UNCOMMIT as u64,
    },
    PragmaName {
        z_name: "recursive_triggers",
        e_prag_typ: PRAG_TYP_FLAG,
        m_prag_flg: PRAG_FLG_RESULT0 | PRAG_FLG_NO_COLUMNS1,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: SQLITE_REC_TRIGGERS as u64,
    },
    PragmaName {
        z_name: "reverse_unordered_selects",
        e_prag_typ: PRAG_TYP_FLAG,
        m_prag_flg: PRAG_FLG_RESULT0 | PRAG_FLG_NO_COLUMNS1,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: SQLITE_REVERSE_ORDER as u64,
    },
    PragmaName {
        z_name: "schema_version",
        e_prag_typ: PRAG_TYP_HEADER_VALUE,
        m_prag_flg: PRAG_FLG_NO_COLUMNS1 | PRAG_FLG_RESULT0,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: BTREE_SCHEMA_VERSION as u64,
    },
    PragmaName {
        z_name: "secure_delete",
        e_prag_typ: PRAG_TYP_SECURE_DELETE,
        m_prag_flg: PRAG_FLG_RESULT0,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: 0,
    },
    PragmaName {
        z_name: "short_column_names",
        e_prag_typ: PRAG_TYP_FLAG,
        m_prag_flg: PRAG_FLG_RESULT0 | PRAG_FLG_NO_COLUMNS1,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: SQLITE_SHORT_COL_NAMES as u64,
    },
    PragmaName {
        z_name: "shrink_memory",
        e_prag_typ: PRAG_TYP_SHRINK_MEMORY,
        m_prag_flg: PRAG_FLG_NO_COLUMNS,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: 0,
    },
    PragmaName {
        z_name: "soft_heap_limit",
        e_prag_typ: PRAG_TYP_SOFT_HEAP_LIMIT,
        m_prag_flg: PRAG_FLG_RESULT0,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: 0,
    },
    PragmaName {
        z_name: "synchronous",
        e_prag_typ: PRAG_TYP_SYNCHRONOUS,
        m_prag_flg: PRAG_FLG_NEED_SCHEMA | PRAG_FLG_RESULT0 | PRAG_FLG_SCHEMA_REQ | PRAG_FLG_NO_COLUMNS1,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: 0,
    },
    PragmaName {
        z_name: "table_info",
        e_prag_typ: PRAG_TYP_TABLE_INFO,
        m_prag_flg: PRAG_FLG_NEED_SCHEMA | PRAG_FLG_RESULT1 | PRAG_FLG_SCHEMA_OPT,
        i_prag_c_name: 8,
        n_prag_c_name: 6,
        i_arg: 0,
    },
    PragmaName {
        z_name: "table_list",
        e_prag_typ: PRAG_TYP_TABLE_LIST,
        m_prag_flg: PRAG_FLG_NEED_SCHEMA | PRAG_FLG_RESULT1,
        i_prag_c_name: 15,
        n_prag_c_name: 6,
        i_arg: 0,
    },
    PragmaName {
        z_name: "table_xinfo",
        e_prag_typ: PRAG_TYP_TABLE_INFO,
        m_prag_flg: PRAG_FLG_NEED_SCHEMA | PRAG_FLG_RESULT1 | PRAG_FLG_SCHEMA_OPT,
        i_prag_c_name: 8,
        n_prag_c_name: 7,
        i_arg: 1,
    },
    PragmaName {
        z_name: "temp_store",
        e_prag_typ: PRAG_TYP_TEMP_STORE,
        m_prag_flg: PRAG_FLG_RESULT0 | PRAG_FLG_NO_COLUMNS1,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: 0,
    },
    PragmaName {
        z_name: "temp_store_directory",
        e_prag_typ: PRAG_TYP_TEMP_STORE_DIRECTORY,
        m_prag_flg: PRAG_FLG_NO_COLUMNS1,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: 0,
    },
    PragmaName {
        z_name: "threads",
        e_prag_typ: PRAG_TYP_THREADS,
        m_prag_flg: PRAG_FLG_RESULT0,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: 0,
    },
    PragmaName {
        z_name: "trusted_schema",
        e_prag_typ: PRAG_TYP_FLAG,
        m_prag_flg: PRAG_FLG_RESULT0 | PRAG_FLG_NO_COLUMNS1,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: SQLITE_TRUSTED_SCHEMA as u64,
    },
    PragmaName {
        z_name: "user_version",
        e_prag_typ: PRAG_TYP_HEADER_VALUE,
        m_prag_flg: PRAG_FLG_NO_COLUMNS1 | PRAG_FLG_RESULT0,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: BTREE_USER_VERSION as u64,
    },
    PragmaName {
        z_name: "wal_autocheckpoint",
        e_prag_typ: PRAG_TYP_WAL_AUTOCHECKPOINT,
        m_prag_flg: 0,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: 0,
    },
    PragmaName {
        z_name: "wal_checkpoint",
        e_prag_typ: PRAG_TYP_WAL_CHECKPOINT,
        m_prag_flg: PRAG_FLG_NEED_SCHEMA,
        i_prag_c_name: 50,
        n_prag_c_name: 3,
        i_arg: 0,
    },
    PragmaName {
        z_name: "writable_schema",
        e_prag_typ: PRAG_TYP_FLAG,
        m_prag_flg: PRAG_FLG_RESULT0 | PRAG_FLG_NO_COLUMNS1,
        i_prag_c_name: 0,
        n_prag_c_name: 0,
        i_arg: (SQLITE_WRITE_SCHEMA | SQLITE_NO_SCHEMA_ERROR) as u64,
    },
];


// ---- part_001.rs ----

// O trecho 001 do C contém apenas o comentário "Number of pragmas: 68 on by default, 78 total."
// e nenhum item, então esta parte não tem código.

