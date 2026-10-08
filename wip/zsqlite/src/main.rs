//! `main.c`: a interface programática do SQLite 3.46.1 (abrir e fechar a conexão, erros, limites,
//! funções e colações do usuário, ganchos, checkpoint, `db_config`, nomes de arquivo e URI,
//! controles de teste), no modelo v2 (ver `CONVENTIONS.md`). Também mora aqui o que o C põe em
//! `util.c`/`malloc.c` mas só existe para a conexão: `error`, `error_clear`, `error_with_msg`,
//! `api_exit`, `safety_check_ok`, `safety_check_sick_or_ok` e o `printf` com a conexão
//! (`db_printf`, o `sqlite3VMPrintf`). `oom_fault`, `oom_clear`, `system_error` e `error_msg`
//! ficam em `util.rs`.
//!
//! Desvios do C, todos decorrentes do modelo v2:
//!
//! * A conexão é `Connection`, possuída por quem a abriu (`Box<Connection>`). Os mutexes
//!   (`sqlite3_mutex_enter`/`leave`) e os ramos `SQLITE_ENABLE_API_ARMOR`, `SQLITE_DEBUG`,
//!   `SQLITE_ENABLE_SNAPSHOT`, `SQLITE_ENABLE_SQLLOG` e `SQLITE_USER_AUTHENTICATION` não
//!   existem. O ponteiro nulo do C não tem equivalente: `sqlite3_close(NULL)` é não chamar.
//!   `sqlite3LeaveMutexAndCloseZombie` solta os recursos e deixa a conexão em
//!   `SQLITE_STATE_CLOSED`; a memória do `Connection` é de quem o possui.
//! * `sqlite3_open*` devolve `(código, Option<Box<Connection>>)`: como no C, uma conexão que
//!   falhou ao abrir (exceto por falta de memória) é devolvida para ler `errmsg`.
//! * Ganchos e destrutores do usuário são fechamentos (a captura substitui o `void *pArg`); o
//!   destrutor roda quando o fechamento é solto (`Drop`), o que cobre o `xDestroy` de
//!   `create_function_v2`, `create_collation_v2`, `autovacuum_pages` e `set_clientdata`. As
//!   funções SQL continuam sendo ponteiros de função (`ScalarFn`, `FinalFn`) e o dado do usuário
//!   viaja em `UserData`.
//! * Os nomes são fatias de bytes (UTF-8, sem NUL final); as variantes UTF-16 recebem bytes
//!   UTF-16 nativos (little endian) terminados por um par de zeros.
//! * `sqlite3_va_list`: `config`, `db_config` e `test_control` recebem enums tipados no lugar dos
//!   argumentos variádicos.
//! * Um `sqlite3_filename` do C é um ponteiro para dentro de um bloco com quatro zeros antes (a
//!   rotina `databaseName` anda para trás). Aqui é uma fatia que COMEÇA no nome do banco, o que
//!   a `Pager.filename()` já devolve; `create_filename` entrega o bloco no mesmo formato.
//! * O estado global (`sqlite3GlobalConfig`) é a [`GlobalConfig`] atrás de um `Mutex`. O que já
//!   tem dono em outro módulo (`global.rs`: log e semente do PRNG; `os_unix.rs`: mmap e
//!   diretório temporário) é repassado a ele. O registro das funções embutidas é por thread
//!   (ver `callback.rs`), por isso `initialize` o refaz em cada thread nova.
//! * O gancho do WAL do modelo v2 (`WalHook`) não recebe a conexão, mas o gancho padrão de
//!   `wal_autocheckpoint` precisa fazer um checkpoint nela: ele só registra o banco que passou
//!   do limite, e quem chama o gancho roda [`wal_default_hook_flush`] logo depois.
//! * Pendência de modelagem: `FileControlArg` não tem variante de ponteiro, então
//!   `SQLITE_FCNTL_FILE_POINTER`, `VFS_POINTER` e `JOURNAL_POINTER` devolvem `SQLITE_OK` sem
//!   escrever nada no argumento.
//! * Opções do Debian resolvidas: `USE_URI=1` (URI ligada por padrão), `ENABLE_FTS3_TOKENIZER`,
//!   `ENABLE_LOAD_EXTENSION`, `THREADSAFE=1`, `DEFAULT_SYNCHRONOUS=2`, `DEFAULT_MEMSTATUS=0`,
//!   `DEFAULT_WAL_AUTOCHECKPOINT=1000`, `TEMP_STORE=1`.

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::bitvec::bitvec_builtin_test;
use crate::btree::{
    btree_close, btree_get_requested_reserve, btree_open, btree_set_mmap_limit,
    btree_set_page_size,
};
use crate::btree_cursor::{btree_rollback, BtDb};
use crate::btree_types::Btree;
use crate::btree_write::{btree_checkpoint, btree_clear_cache, btree_txn_state};
use crate::build::{
    collapse_database_array, column_coll, find_db_name, find_table,
    reset_all_schemas_of_connection,
};
use crate::callback::{
    find_coll_seq, find_coll_seq_entry, find_function, schema_clear, schema_get,
    set_text_encoding,
};
use crate::connection::{
    AutovacPagesFn, BusyHandler, CollNeededFn, CommitHook, Connection, Context, DbClientData,
    DbSlot, DestroyFn, FinalFn, FuncDef, FuncDestructor, PreUpdateFn, ProfileFn, ProgressFn,
    RollbackHook, ScalarFn, TraceEvent, TraceFn, UpdateHook, UserData, WalHook,
};
use crate::consts::{
    BTS_READ_ONLY, COLFLAG_HASTYPE, COLFLAG_PRIMKEY, DBFLAG_INTERNAL_FUNC, DBFLAG_SCHEMA_CHANGE,
    DB_SCHEMALOADED, LOOKASIDE_SMALL, PAGER_SYNCHRONOUS_OFF, PENDING_BYTE, SQLITE_ANY,
    SQLITE_AUTO_INDEX, SQLITE_BUSY, SQLITE_CACHE_SPILL, SQLITE_CANTOPEN,
    SQLITE_CHECKPOINT_PASSIVE, SQLITE_CHECKPOINT_TRUNCATE, SQLITE_CONFIG_COVERING_INDEX_SCAN,
    SQLITE_CONFIG_GETMALLOC, SQLITE_CONFIG_GETMUTEX, SQLITE_CONFIG_GETPCACHE,
    SQLITE_CONFIG_GETPCACHE2, SQLITE_CONFIG_LOG, SQLITE_CONFIG_LOOKASIDE, SQLITE_CONFIG_MALLOC,
    SQLITE_CONFIG_MEMDB_MAXSIZE, SQLITE_CONFIG_MEMSTATUS, SQLITE_CONFIG_MMAP_SIZE,
    SQLITE_CONFIG_MULTITHREAD, SQLITE_CONFIG_MUTEX, SQLITE_CONFIG_PAGECACHE, SQLITE_CONFIG_PCACHE,
    SQLITE_CONFIG_PCACHE2, SQLITE_CONFIG_PCACHE_HDRSZ, SQLITE_CONFIG_PMASZ,
    SQLITE_CONFIG_ROWID_IN_VIEW, SQLITE_CONFIG_SERIALIZED, SQLITE_CONFIG_SINGLETHREAD,
    SQLITE_CONFIG_SMALL_MALLOC, SQLITE_CONFIG_STMTJRNL_SPILL, SQLITE_CONFIG_URI, SQLITE_CORRUPT,
    SQLITE_CORRUPT_RD_ONLY, SQLITE_DBCONFIG_DEFENSIVE, SQLITE_DBCONFIG_DQS_DDL,
    SQLITE_DBCONFIG_DQS_DML, SQLITE_DBCONFIG_ENABLE_FKEY, SQLITE_DBCONFIG_ENABLE_FTS3_TOKENIZER,
    SQLITE_DBCONFIG_ENABLE_LOAD_EXTENSION, SQLITE_DBCONFIG_ENABLE_QPSG,
    SQLITE_DBCONFIG_ENABLE_TRIGGER, SQLITE_DBCONFIG_ENABLE_VIEW,
    SQLITE_DBCONFIG_LEGACY_ALTER_TABLE, SQLITE_DBCONFIG_LEGACY_FILE_FORMAT,
    SQLITE_DBCONFIG_LOOKASIDE, SQLITE_DBCONFIG_MAINDBNAME, SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE,
    SQLITE_DBCONFIG_RESET_DATABASE, SQLITE_DBCONFIG_REVERSE_SCANORDER,
    SQLITE_DBCONFIG_STMT_SCANSTATUS, SQLITE_DBCONFIG_TRIGGER_EQP, SQLITE_DBCONFIG_TRUSTED_SCHEMA,
    SQLITE_DBCONFIG_WRITABLE_SCHEMA, SQLITE_DEFAULT_MMAP_SIZE, SQLITE_DEFAULT_SYNCHRONOUS,
    SQLITE_DEFAULT_WAL_AUTOCHECKPOINT, SQLITE_DEFAULT_WORKER_THREADS, SQLITE_DEFENSIVE,
    SQLITE_DEFER_FKS, SQLITE_DETERMINISTIC, SQLITE_DIRECTONLY, SQLITE_DQS_DDL, SQLITE_DQS_DML,
    SQLITE_ENABLE_QPSG, SQLITE_ENABLE_TRIGGER, SQLITE_ENABLE_VIEW, SQLITE_ERROR,
    SQLITE_FCNTL_DATA_VERSION, SQLITE_FCNTL_FILE_POINTER, SQLITE_FCNTL_JOURNAL_POINTER,
    SQLITE_FCNTL_RESERVE_BYTES, SQLITE_FCNTL_RESET_CACHE, SQLITE_FCNTL_VFS_POINTER,
    SQLITE_FK_NO_ACTION, SQLITE_FOREIGN_KEYS, SQLITE_FTS3_TOKENIZER, SQLITE_FUNC_CONSTANT,
    SQLITE_FUNC_DIRECT, SQLITE_FUNC_ENCMASK, SQLITE_FUNC_UNSAFE, SQLITE_INNOCUOUS,
    SQLITE_INTERRUPT, SQLITE_IOERR_NOMEM, SQLITE_LEGACY_ALTER, SQLITE_LEGACY_FILE_FMT,
    SQLITE_LIMIT_LENGTH, SQLITE_LIMIT_WORKER_THREADS, SQLITE_LOAD_EXTENSION, SQLITE_MAX_ATTACHED,
    SQLITE_MAX_COLUMN, SQLITE_MAX_COMPOUND_SELECT, SQLITE_MAX_DB, SQLITE_MAX_EXPR_DEPTH,
    SQLITE_MAX_FUNCTION_ARG, SQLITE_MAX_LENGTH, SQLITE_MAX_LIKE_PATTERN_LENGTH,
    SQLITE_MAX_MMAP_SIZE, SQLITE_MAX_SQL_LENGTH, SQLITE_MAX_TRIGGER_DEPTH,
    SQLITE_MAX_VARIABLE_NUMBER, SQLITE_MAX_VDBE_OP, SQLITE_MAX_WORKER_THREADS,
    SQLITE_MEMDB_DEFAULT_MAXSIZE, SQLITE_MISUSE, SQLITE_NOMEM, SQLITE_NOMEM_BKPT,
    SQLITE_NO_CKPT_ON_CLOSE, SQLITE_NO_SCHEMA_ERROR, SQLITE_N_LIMIT, SQLITE_OK,
    SQLITE_OPEN_CREATE, SQLITE_OPEN_DELETEONCLOSE, SQLITE_OPEN_EXCLUSIVE, SQLITE_OPEN_EXRESCODE,
    SQLITE_OPEN_FULLMUTEX, SQLITE_OPEN_MAIN_DB, SQLITE_OPEN_MAIN_JOURNAL, SQLITE_OPEN_MEMORY,
    SQLITE_OPEN_NOMUTEX, SQLITE_OPEN_PRIVATECACHE, SQLITE_OPEN_READONLY, SQLITE_OPEN_READWRITE,
    SQLITE_OPEN_SHAREDCACHE, SQLITE_OPEN_SUBJOURNAL, SQLITE_OPEN_SUPER_JOURNAL,
    SQLITE_OPEN_TEMP_DB, SQLITE_OPEN_TEMP_JOURNAL, SQLITE_OPEN_TRANSIENT_DB, SQLITE_OPEN_URI,
    SQLITE_OPEN_WAL, SQLITE_PERM, SQLITE_RESET_DATABASE, SQLITE_RESULT_SUBTYPE,
    SQLITE_REVERSE_ORDER, SQLITE_SHORT_COL_NAMES, SQLITE_STATE_BUSY, SQLITE_STATE_CLOSED,
    SQLITE_STATE_ERROR, SQLITE_STATE_OPEN, SQLITE_STATE_SICK, SQLITE_STATE_ZOMBIE,
    SQLITE_STMTJRNL_SPILL, SQLITE_STMT_SCAN_STATUS, SQLITE_SUBTYPE, SQLITE_TRACE_CLOSE,
    SQLITE_TRACE_LEGACY, SQLITE_TRACE_NONLEGACY_MASK, SQLITE_TRACE_XPROFILE,
    SQLITE_TRIGGER_EQP, SQLITE_TRUSTED_SCHEMA, SQLITE_TXN_WRITE, SQLITE_UTF16,
    SQLITE_UTF16BE, SQLITE_UTF16LE, SQLITE_UTF16NATIVE, SQLITE_UTF16_ALIGNED, SQLITE_UTF8,
    SQLITE_VERSION, SQLITE_VERSION_NUMBER, SQLITE_WRITE_SCHEMA, TF_AUTOINCREMENT, TRANS_NONE,
};
use crate::ctype::{is_id_char, is_xdigit};
use crate::expr_code::is_rowid;
use crate::global::LogHook;
use crate::hash::{hash_clear, hash_find, hash_find_mut, hash_iter};
use crate::mem::{CollFn, CollSeq, Mem};
use crate::os::{os_file_control, vfs_find, FileControlArg, VfsRef};
use crate::pager::cstr;
use crate::pager_ext::PagerCloseDb;
use crate::printf::{mprintf, vm_printf, PrintfArg};
use crate::sqlite_int::{Column, Table};
use crate::utf::translate_bytes;
use crate::util::{
    at, dec_or_hex_to_i64, err_str, hex_to_int, log_est, log_est_from_double, log_est_to_int,
    oom_clear, oom_fault, str_icmp, strlen30, strnicmp, system_error, STD_TYPE, STR_BINARY,
};
use crate::vdbeapi::{result_error, result_int_real, user_data};
use crate::vdbeaux2::with_bt_db;
use crate::vdbeaux3::expire_prepared_statements;

// ---------------------------------------------------------------------------------------------
// chunk 000: versão, configuração global, inicialização
// ---------------------------------------------------------------------------------------------

/// `SQLITE_SOURCE_ID` do 3.46.1.
pub const SQLITE_SOURCE_ID: &str =
    "2024-08-13 09:16:08 c9c2ab54ba1f5f46360f1b4f35d849cd3f080e6fc2b6c60e91b16c63f69a1e33";

/// `sqlite3_libversion`.
pub fn libversion() -> &'static str {
    SQLITE_VERSION
}

/// `sqlite3_sourceid`.
pub fn sourceid() -> &'static str {
    SQLITE_SOURCE_ID
}

/// `sqlite3_libversion_number`.
pub fn libversion_number() -> i32 {
    SQLITE_VERSION_NUMBER
}

/// `sqlite3_threadsafe`: `SQLITE_THREADSAFE=1`.
pub fn threadsafe() -> i32 {
    1
}

/// `sqlite3GlobalConfig`: só os campos que não têm outro dono (ver o cabeçalho do módulo).
#[derive(Debug, Clone, Copy)]
pub struct GlobalConfig {
    /// `isInit`: `sqlite3_initialize()` terminou.
    pub is_init: bool,
    /// `inProgress`: dentro de `sqlite3_initialize()`.
    pub in_progress: bool,
    /// `bCoreMutex`.
    pub b_core_mutex: bool,
    /// `bFullMutex`.
    pub b_full_mutex: bool,
    /// `bOpenUri` (`SQLITE_USE_URI=1`).
    pub b_open_uri: bool,
    /// `bUseCis`: índice de cobertura em varreduras completas.
    pub b_use_cis: bool,
    /// `bMemstat` (`SQLITE_DEFAULT_MEMSTATUS=0`).
    pub b_memstat: bool,
    /// `bSmallMalloc`.
    pub b_small_malloc: bool,
    /// `sharedCacheEnabled`: o cache compartilhado não existe (CONVENTIONS, item 4).
    pub shared_cache_enabled: bool,
    /// `szLookaside`.
    pub sz_lookaside: i32,
    /// `nLookaside`.
    pub n_lookaside: i32,
    /// `szMmap`.
    pub sz_mmap: i64,
    /// `mxMmap`.
    pub mx_mmap: i64,
    /// `szPma` (`SQLITE_CONFIG_PMASZ`).
    pub sz_pma: u32,
    /// `nStmtSpill` (`SQLITE_CONFIG_STMTJRNL_SPILL`).
    pub n_stmt_spill: i32,
    /// `mxMemdbSize` (`SQLITE_CONFIG_MEMDB_MAXSIZE`).
    pub mx_memdb_size: i64,
}

const DEFAULT_CONFIG: GlobalConfig = GlobalConfig {
    is_init: false,
    in_progress: false,
    b_core_mutex: true,
    b_full_mutex: true,
    b_open_uri: true,
    b_use_cis: true,
    b_memstat: false,
    b_small_malloc: false,
    shared_cache_enabled: false,
    sz_lookaside: 1200,
    n_lookaside: 40,
    sz_mmap: SQLITE_DEFAULT_MMAP_SIZE as i64,
    mx_mmap: SQLITE_MAX_MMAP_SIZE as i64,
    sz_pma: 250,
    n_stmt_spill: SQLITE_STMTJRNL_SPILL,
    mx_memdb_size: SQLITE_MEMDB_DEFAULT_MAXSIZE,
};

static CONFIG: Mutex<GlobalConfig> = Mutex::new(DEFAULT_CONFIG);

/// `sqlite3_data_directory`.
static DATA_DIRECTORY: Mutex<Option<Vec<u8>>> = Mutex::new(None);

thread_local! {
    /// As funções embutidas são por thread (ver `callback.rs`): esta thread já as registrou?
    static BUILTINS_READY: Cell<bool> = const { Cell::new(false) };
    /// Os bancos que o gancho padrão de `wal_autocheckpoint` pediu para fazer checkpoint.
    static PENDING_CHECKPOINTS: Cell<Vec<Vec<u8>>> = const { Cell::new(Vec::new()) };
}

fn lock_config() -> MutexGuard<'static, GlobalConfig> {
    CONFIG.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Uma cópia da configuração global vigente (`sqlite3GlobalConfig` para leitura).
pub fn global_config() -> GlobalConfig {
    *lock_config()
}

/// `sqlite3_data_directory`, para leitura.
pub fn data_directory() -> Option<Vec<u8>> {
    DATA_DIRECTORY.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

/// Grava `sqlite3_data_directory` (`PRAGMA data_store_directory`).
pub fn set_data_directory(dir: Option<Vec<u8>>) {
    *DATA_DIRECTORY.lock().unwrap_or_else(PoisonError::into_inner) = dir;
}

/// `hasHighPrecisionDouble`: o `long double` do x86-64 (80 bits) funciona de verdade, então o
/// experimento do C sempre dá verdadeiro; `rc` só existia para o compilador não eliminar a conta.
pub fn has_high_precision_double(_rc: i32) -> bool {
    true
}

/// `sqlite3_initialize`: registra o VFS padrão e o `memdb`. É inofensiva depois da primeira vez.
/// As funções embutidas (por thread) são registradas na primeira chamada de cada thread.
pub fn initialize() -> i32 {
    if !BUILTINS_READY.with(Cell::get) {
        crate::func::register_builtin_functions();
        BUILTINS_READY.with(|c| c.set(true));
    }
    let mut cfg = lock_config();
    if cfg.is_init {
        return SQLITE_OK;
    }
    let mut rc = SQLITE_OK;
    if !cfg.in_progress {
        cfg.in_progress = true;
        rc = crate::os_unix::os_init();
        if rc == SQLITE_OK {
            rc = crate::memdb::memdb_init();
        }
        if rc == SQLITE_OK {
            cfg.is_init = true;
        }
        cfg.in_progress = false;
    }
    rc
}

/// `sqlite3_shutdown`: desfaz `initialize`.
pub fn shutdown() -> i32 {
    let mut cfg = lock_config();
    if cfg.is_init {
        crate::os_unix::os_end();
        crate::loadext::reset_auto_extension();
        cfg.is_init = false;
        BUILTINS_READY.with(|c| c.set(false));
        // `sqlite3_data_directory = 0; sqlite3_temp_directory = 0;`
        set_data_directory(None);
        crate::os_unix::set_temp_directory(None);
    }
    SQLITE_OK
}

/// O argumento de `sqlite3_config` (o que o `va_arg` leria). Os modos que o C aceita com
/// ponteiros de alocador, de mutex ou de cache de páginas (`MALLOC`, `GETMALLOC`, `MUTEX`,
/// `GETMUTEX`, `PAGECACHE`, `PCACHE2`, `GETPCACHE2`) não têm o que configurar em Rust e valem
/// como `ConfigArg::None`.
pub enum ConfigArg<'a> {
    /// Sem argumento (ou argumento sem efeito neste porte).
    None,
    /// Um `int`: `MEMSTATUS`, `SMALL_MALLOC`, `URI`, `COVERING_INDEX_SCAN`, `PMASZ`,
    /// `STMTJRNL_SPILL`, `SORTERREF_SIZE`.
    Int(i32),
    /// Dois `int`: `LOOKASIDE` (tamanho, quantidade).
    Int2(i32, i32),
    /// Um `sqlite3_int64`: `MEMDB_MAXSIZE`.
    Int64(i64),
    /// Dois `sqlite3_int64`: `MMAP_SIZE` (padrão, máximo).
    Int64x2(i64, i64),
    /// `SQLITE_CONFIG_LOG`: o gancho (nulo desliga o log).
    Log(Option<LogHook>),
    /// Um `int*` de saída: `PCACHE_HDRSZ` e `ROWID_IN_VIEW`.
    OutInt(&'a mut i32),
}

/// `sqlite3_config`: modifica a configuração global. Depois de `initialize` só `LOG` e
/// `PCACHE_HDRSZ` são aceitos.
pub fn config(op: i32, arg: ConfigArg<'_>) -> i32 {
    let mut rc = SQLITE_OK;
    if lock_config().is_init {
        // `mAnytimeConfigOption`.
        if !(op == SQLITE_CONFIG_LOG || op == SQLITE_CONFIG_PCACHE_HDRSZ) {
            return misuse_error(180223);
        }
    }
    match (op, arg) {
        (SQLITE_CONFIG_SINGLETHREAD, _) => {
            let mut cfg = lock_config();
            cfg.b_core_mutex = false;
            cfg.b_full_mutex = false;
        }
        (SQLITE_CONFIG_MULTITHREAD, _) => {
            let mut cfg = lock_config();
            cfg.b_core_mutex = true;
            cfg.b_full_mutex = false;
        }
        (SQLITE_CONFIG_SERIALIZED, _) => {
            let mut cfg = lock_config();
            cfg.b_core_mutex = true;
            cfg.b_full_mutex = true;
        }
        (SQLITE_CONFIG_MUTEX, _)
        | (SQLITE_CONFIG_GETMUTEX, _)
        | (SQLITE_CONFIG_MALLOC, _)
        | (SQLITE_CONFIG_GETMALLOC, _)
        | (SQLITE_CONFIG_PAGECACHE, _)
        | (SQLITE_CONFIG_PCACHE, _)
        | (SQLITE_CONFIG_PCACHE2, _)
        | (SQLITE_CONFIG_GETPCACHE2, _) => {}
        (SQLITE_CONFIG_GETPCACHE, _) => {
            // Agora é um erro.
            rc = SQLITE_ERROR;
        }
        (SQLITE_CONFIG_MEMSTATUS, ConfigArg::Int(v)) => {
            let mut cfg = lock_config();
            debug_assert!(!cfg.is_init);
            cfg.b_memstat = v != 0;
        }
        (SQLITE_CONFIG_SMALL_MALLOC, ConfigArg::Int(v)) => {
            lock_config().b_small_malloc = v != 0;
        }
        (SQLITE_CONFIG_PCACHE_HDRSZ, ConfigArg::OutInt(out)) => {
            // `sqlite3HeaderSizeBtree() + sqlite3HeaderSizePcache() + sqlite3HeaderSizePcache1()`
            // com o layout LP64 do C: `ROUND8(sizeof(MemPage))` 136, `ROUND8(sizeof(PgHdr))` 72
            // e `ROUND8(sizeof(PgHdr1))` 56.
            *out = 136 + 72 + 56;
        }
        (SQLITE_CONFIG_LOOKASIDE, ConfigArg::Int2(sz, cnt)) => {
            let mut cfg = lock_config();
            cfg.sz_lookaside = sz;
            cfg.n_lookaside = cnt;
        }
        (SQLITE_CONFIG_LOG, ConfigArg::Log(hook)) => {
            crate::global::set_logger(hook);
        }
        (SQLITE_CONFIG_URI, ConfigArg::Int(v)) => {
            lock_config().b_open_uri = v != 0;
        }
        (SQLITE_CONFIG_COVERING_INDEX_SCAN, ConfigArg::Int(v)) => {
            lock_config().b_use_cis = v != 0;
        }
        (SQLITE_CONFIG_MMAP_SIZE, ConfigArg::Int64x2(sz_mmap, mx_mmap)) => {
            let mut mx_mmap = mx_mmap;
            let mut sz_mmap = sz_mmap;
            // Negativo (ou acima do máximo de compilação) volta ao padrão de compilação.
            if mx_mmap < 0 || mx_mmap > SQLITE_MAX_MMAP_SIZE as i64 {
                mx_mmap = SQLITE_MAX_MMAP_SIZE as i64;
            }
            if sz_mmap < 0 {
                sz_mmap = SQLITE_DEFAULT_MMAP_SIZE as i64;
            }
            if sz_mmap > mx_mmap {
                sz_mmap = mx_mmap;
            }
            {
                let mut cfg = lock_config();
                cfg.mx_mmap = mx_mmap;
                cfg.sz_mmap = sz_mmap;
            }
            crate::os_unix::config_mmap_size(sz_mmap, mx_mmap);
        }
        (SQLITE_CONFIG_PMASZ, ConfigArg::Int(v)) => {
            lock_config().sz_pma = v as u32;
        }
        (SQLITE_CONFIG_STMTJRNL_SPILL, ConfigArg::Int(v)) => {
            lock_config().n_stmt_spill = v;
        }
        (SQLITE_CONFIG_MEMDB_MAXSIZE, ConfigArg::Int64(v)) => {
            lock_config().mx_memdb_size = v;
        }
        (SQLITE_CONFIG_ROWID_IN_VIEW, ConfigArg::OutInt(out)) => {
            // Sem `SQLITE_ALLOW_ROWID_IN_VIEW`.
            *out = 0;
        }
        _ => {
            rc = SQLITE_ERROR;
        }
    }
    rc
}

// ---------------------------------------------------------------------------------------------
// Erros de baixo nível (o `SQLITE_xxx_BKPT` do C) e estado de erro da conexão
// ---------------------------------------------------------------------------------------------

/// `sqlite3ReportError`: registra no log o lugar onde um erro de baixo nível nasceu e devolve o
/// código. `lineno` é a linha do `SQLITE_xxx_BKPT` na amalgamação do 3.46.1 (é o que a mensagem
/// do Debian mostra).
pub fn report_error(i_err: i32, lineno: i32, z_type: &str) -> i32 {
    crate::global::log(
        i_err,
        b"%s at line %d of [%.10s]",
        &[
            PrintfArg::Text(Some(z_type.as_bytes().to_vec())),
            PrintfArg::Int(lineno as i64),
            PrintfArg::Text(Some(SQLITE_SOURCE_ID.as_bytes()[20..].to_vec())),
        ],
    );
    i_err
}

/// `sqlite3CorruptError`: `SQLITE_CORRUPT_BKPT`.
pub fn corrupt_error(lineno: i32) -> i32 {
    report_error(SQLITE_CORRUPT, lineno, "database corruption")
}

/// `sqlite3MisuseError`: `SQLITE_MISUSE_BKPT`.
pub fn misuse_error(lineno: i32) -> i32 {
    report_error(SQLITE_MISUSE, lineno, "misuse")
}

/// `sqlite3CantopenError`: `SQLITE_CANTOPEN_BKPT`.
pub fn cantopen_error(lineno: i32) -> i32 {
    report_error(SQLITE_CANTOPEN, lineno, "cannot open file")
}

/// `sqlite3VMPrintf` e `sqlite3MPrintf(db, ...)`: formata com as extensões internas (`%T`, `%S`)
/// no limite `SQLITE_LIMIT_LENGTH` da conexão. Falta de memória liga `malloc_failed`; o
/// `%T`/`%#T` gravam `err_byte_offset` como o `sqlite3RecordErrorByteOffset` do C.
pub fn db_printf(db: &mut Connection, fmt: &[u8], args: &[PrintfArg]) -> Option<Vec<u8>> {
    let mx_alloc = db.a_limit[SQLITE_LIMIT_LENGTH as usize] as u32;
    let (z, acc) = vm_printf(mx_alloc, fmt, args);
    if let Some(off) = acc.err_byte_offset_token {
        if db.err_byte_offset == -2 {
            db.err_byte_offset = off;
        }
    }
    if let Some(off) = acc.err_byte_offset_expr {
        db.err_byte_offset = off;
    }
    if acc.acc_error == SQLITE_NOMEM as u8 {
        oom_fault(db);
    }
    z
}

/// `sqlite3Error`: grava o código de erro corrente e apaga a mensagem anterior. Também carrega
/// `i_sys_errno` quando o código pede.
pub fn error(db: &mut Connection, err_code: i32) {
    db.err_code = err_code;
    if err_code != 0 || db.err_msg.is_some() {
        // `sqlite3ErrorFinish`.
        db.err_msg = None;
        system_error(db, err_code);
    } else {
        db.err_byte_offset = -1;
    }
}

/// `sqlite3ErrorClear`: o equivalente de `error(db, SQLITE_OK)`: limpa o estado de erro.
pub fn error_clear(db: &mut Connection) {
    db.err_code = SQLITE_OK;
    db.err_byte_offset = -1;
    db.err_msg = None;
}

/// `sqlite3ErrorWithMsg`: grava o código e a mensagem (o formato e os argumentos em UTF-8). Sem
/// formato o C chama `sqlite3Error`: quem não tem mensagem chama [`error`] direto.
pub fn error_with_msg(db: &mut Connection, err_code: i32, fmt: &[u8], args: &[PrintfArg]) {
    db.err_code = err_code;
    system_error(db, err_code);
    db.err_msg = db_printf(db, fmt, args);
}

fn log_bad_connection(z_type: &str) {
    crate::global::log(
        SQLITE_MISUSE,
        b"API call with %s database connection pointer",
        &[PrintfArg::Text(Some(z_type.as_bytes().to_vec()))],
    );
}

/// `sqlite3SafetyCheckOk`: a conexão está aberta e pronta para uso? O ponteiro nulo do C não
/// existe aqui.
pub fn safety_check_ok(db: &Connection) -> bool {
    if db.e_open_state != SQLITE_STATE_OPEN {
        if safety_check_sick_or_ok(db) {
            log_bad_connection("unopened");
        }
        false
    } else {
        true
    }
}

/// `sqlite3SafetyCheckSickOrOk`: aceita também a conexão que falhou ao abrir (para `errmsg` e
/// `close`).
pub fn safety_check_sick_or_ok(db: &Connection) -> bool {
    let st = db.e_open_state;
    if st != SQLITE_STATE_SICK && st != SQLITE_STATE_OPEN && st != SQLITE_STATE_BUSY {
        log_bad_connection("invalid");
        false
    } else {
        true
    }
}

/// `sqlite3ApiExit`: tratamento de erro no fim de uma chamada da API. Devolve `rc` (com a
/// máscara de códigos estendidos) ou `SQLITE_NOMEM` se faltou memória desde a última chamada.
pub fn api_exit(db: &mut Connection, rc: i32) -> i32 {
    if db.malloc_failed != 0 || rc != 0 {
        // `apiHandleError`.
        if db.malloc_failed != 0 || rc == SQLITE_IOERR_NOMEM {
            oom_clear(db);
            error(db, SQLITE_NOMEM);
            return SQLITE_NOMEM_BKPT;
        }
        return rc & db.err_mask;
    }
    0
}

// ---------------------------------------------------------------------------------------------
// chunk 002: lookaside, memória, db_config, colações embutidas, rowid
// ---------------------------------------------------------------------------------------------

/// `setupLookaside`: o lookaside do C é um alocador de pequenos blocos; aqui só existe a
/// estatística que `sqlite3_db_status` e `db_config` expõem (a alocação normal serve), mas o
/// cálculo do tamanho e da quantidade de vagas é o do C. `has_buf` diz que o usuário entregou o
/// buffer (senão o C o aloca, e a quantidade de vagas segue o `malloc_usable_size`).
pub fn setup_lookaside(db: &mut Connection, has_buf: bool, sz: i32, cnt: i32) -> i32 {
    let mut sz = sz;
    let mut cnt = cnt;
    let mut sz_alloc: i64 = sz as i64 * cnt as i64;
    // `sqlite3LookasideUsed(db,0)>0`: nenhuma vaga é de fato emprestada.
    // O `sizeof(LookasideSlot*)` é 8.
    sz &= !7; // ROUNDDOWN8
    if sz <= 8 {
        sz = 0;
    }
    if cnt < 0 {
        cnt = 0;
    }
    let has_start;
    if sz == 0 || cnt == 0 {
        sz = 0;
        has_start = false;
    } else if !has_buf {
        has_start = true;
        sz_alloc = crate::printf::malloc_size(sz_alloc as u64) as i64;
    } else {
        has_start = true;
    }
    let small = LOOKASIDE_SMALL as i64;
    let sz64 = sz as i64;
    let (n_big, n_sm): (i64, i64);
    if sz64 >= small * 3 {
        n_big = sz_alloc / (3 * small + sz64);
        n_sm = (sz_alloc - sz64 * n_big) / small;
    } else if sz64 >= small * 2 {
        n_big = sz_alloc / (small + sz64);
        n_sm = (sz_alloc - sz64 * n_big) / small;
    } else if sz64 > 0 {
        n_big = sz_alloc / sz64;
        n_sm = 0;
    } else {
        n_big = 0;
        n_sm = 0;
    }
    db.lookaside.sz = sz as u16;
    db.lookaside.sz_true = sz as u16;
    if has_start {
        db.lookaside.b_disable = 0;
        db.lookaside.n_slot = (n_big + n_sm) as u32;
    } else {
        db.lookaside.b_disable = 1;
        db.lookaside.sz = 0;
        db.lookaside.n_slot = 0;
    }
    SQLITE_OK
}

/// `sqlite3_db_release_memory`: libera o máximo de memória das caches de páginas da conexão.
pub fn db_release_memory(db: &mut Connection) -> i32 {
    for slot in db.dbs.iter_mut() {
        if let Some(bt) = slot.bt.as_mut() {
            bt.bt.pager.pcache.shrink();
        }
    }
    SQLITE_OK
}

/// `sqlite3_db_cacheflush`: grava no disco as páginas sujas de todos os bancos anexados.
pub fn db_cacheflush(db: &mut Connection) -> i32 {
    let mut rc = SQLITE_OK;
    let mut b_seen_busy = false;
    let mut i = 0;
    while rc == SQLITE_OK && i < db.dbs.len() {
        if let Some(bt) = db.dbs[i].bt.as_mut() {
            if btree_txn_state(Some(&*bt)) == SQLITE_TXN_WRITE {
                rc = bt.bt.pager.flush();
                if rc == SQLITE_BUSY {
                    b_seen_busy = true;
                    rc = SQLITE_OK;
                }
            }
        }
        i += 1;
    }
    if rc == SQLITE_OK && b_seen_busy {
        SQLITE_BUSY
    } else {
        rc
    }
}

/// Copia `db.flags` para o espelho que cada árvore-b lê (`BtShared.db_flags`, o `pBt->db->flags`
/// do C). Quem altera `db.flags` depois de abrir (pragmas, `db_config`, controles de teste)
/// chama isto.
pub fn sync_btree_flags(db: &mut Connection) {
    let flags = db.flags;
    for slot in db.dbs.iter_mut() {
        if let Some(bt) = slot.bt.as_mut() {
            bt.bt.db_flags = flags;
        }
    }
}

/// O argumento de `sqlite3_db_config`.
pub enum DbConfigArg<'a> {
    /// `SQLITE_DBCONFIG_MAINDBNAME`: o novo nome do banco principal.
    MainDbName(&'a [u8]),
    /// `SQLITE_DBCONFIG_LOOKASIDE`: (o usuário entregou o buffer, tamanho, quantidade).
    Lookaside(bool, i32, i32),
    /// Os modos de bandeira: o `onoff` (maior que zero liga, zero desliga, negativo só consulta) e
    /// onde devolver o valor resultante.
    Flag(i32, Option<&'a mut i32>),
}

/// `aFlagOp[]` de `sqlite3_db_config`: o modo e a máscara de `Connection.flags`.
const FLAG_OPS: [(i32, u64); 18] = [
    (SQLITE_DBCONFIG_ENABLE_FKEY, SQLITE_FOREIGN_KEYS),
    (SQLITE_DBCONFIG_ENABLE_TRIGGER, SQLITE_ENABLE_TRIGGER),
    (SQLITE_DBCONFIG_ENABLE_VIEW, SQLITE_ENABLE_VIEW),
    (SQLITE_DBCONFIG_ENABLE_FTS3_TOKENIZER, SQLITE_FTS3_TOKENIZER),
    (SQLITE_DBCONFIG_ENABLE_LOAD_EXTENSION, SQLITE_LOAD_EXTENSION),
    (SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, SQLITE_NO_CKPT_ON_CLOSE),
    (SQLITE_DBCONFIG_ENABLE_QPSG, SQLITE_ENABLE_QPSG),
    (SQLITE_DBCONFIG_TRIGGER_EQP, SQLITE_TRIGGER_EQP),
    (SQLITE_DBCONFIG_RESET_DATABASE, SQLITE_RESET_DATABASE),
    (SQLITE_DBCONFIG_DEFENSIVE, SQLITE_DEFENSIVE),
    (SQLITE_DBCONFIG_WRITABLE_SCHEMA, SQLITE_WRITE_SCHEMA | SQLITE_NO_SCHEMA_ERROR),
    (SQLITE_DBCONFIG_LEGACY_ALTER_TABLE, SQLITE_LEGACY_ALTER),
    (SQLITE_DBCONFIG_DQS_DDL, SQLITE_DQS_DDL),
    (SQLITE_DBCONFIG_DQS_DML, SQLITE_DQS_DML),
    (SQLITE_DBCONFIG_LEGACY_FILE_FORMAT, SQLITE_LEGACY_FILE_FMT),
    (SQLITE_DBCONFIG_TRUSTED_SCHEMA, SQLITE_TRUSTED_SCHEMA),
    (SQLITE_DBCONFIG_STMT_SCANSTATUS, SQLITE_STMT_SCAN_STATUS),
    (SQLITE_DBCONFIG_REVERSE_SCANORDER, SQLITE_REVERSE_ORDER),
];

/// `sqlite3_db_config`: ajustes de uma conexão.
pub fn db_config(db: &mut Connection, op: i32, arg: DbConfigArg<'_>) -> i32 {
    match (op, arg) {
        (SQLITE_DBCONFIG_MAINDBNAME, DbConfigArg::MainDbName(z)) => {
            db.dbs[0].z_db_s_name = z.to_vec();
            SQLITE_OK
        }
        (SQLITE_DBCONFIG_LOOKASIDE, DbConfigArg::Lookaside(has_buf, sz, cnt)) => {
            setup_lookaside(db, has_buf, sz, cnt)
        }
        (op, DbConfigArg::Flag(onoff, res)) => {
            let Some(&(_, mask)) = FLAG_OPS.iter().find(|(o, _)| *o == op) else {
                return SQLITE_ERROR; // IMP: R-42790-23372
            };
            let old_flags = db.flags;
            if onoff > 0 {
                db.flags |= mask;
            } else if onoff == 0 {
                db.flags &= !mask;
            }
            if old_flags != db.flags {
                expire_prepared_statements(db, 0);
                sync_btree_flags(db);
            }
            if let Some(r) = res {
                *r = ((db.flags & mask) != 0) as i32;
            }
            SQLITE_OK
        }
        _ => SQLITE_ERROR,
    }
}

/// `sqlite3IsBinary`: a colação é a BINARY embutida (ou não há colação)?
pub fn is_binary(p: Option<&CollSeq>) -> bool {
    p.map_or(true, |c| matches!(c.x_cmp, CollFn::Binary))
}

/// `sqlite3_last_insert_rowid`.
pub fn last_insert_rowid(db: &Connection) -> i64 {
    db.last_rowid
}

/// `sqlite3_set_last_insert_rowid`.
pub fn set_last_insert_rowid(db: &mut Connection, i_rowid: i64) {
    db.last_rowid = i_rowid;
}

// ---------------------------------------------------------------------------------------------
// chunk 003: changes, fechamento da conexão, rollback
// ---------------------------------------------------------------------------------------------

/// `sqlite3_changes64`.
pub fn changes64(db: &Connection) -> i64 {
    db.n_change
}

/// `sqlite3_changes`.
pub fn changes(db: &Connection) -> i32 {
    db.n_change as i32
}

/// `sqlite3_total_changes64`.
pub fn total_changes64(db: &Connection) -> i64 {
    db.n_total_change
}

/// `sqlite3_total_changes`.
pub fn total_changes(db: &Connection) -> i32 {
    db.n_total_change as i32
}

/// `sqlite3CloseSavepoints`: fecha todos os savepoints. Só mexe em campos da conexão: não fecha
/// savepoints no nível da árvore-b ou do pager.
pub fn close_savepoints(db: &mut Connection) {
    db.p_savepoint.clear();
    db.n_savepoint = 0;
    db.n_statement = 0;
    db.is_transaction_savepoint = 0;
}

/// `disconnectAllVtab`: desconecta todos os `sqlite3_vtab` da conexão (ao fechá-la).
fn disconnect_all_vtab(db: &mut Connection) {
    let mut tabs: Vec<Rc<Table>> = Vec::new();
    for slot in &db.dbs {
        for (_, tab) in hash_iter(&slot.schema.tbl_hash) {
            if tab.is_virtual() {
                tabs.push(tab.clone());
            }
        }
    }
    // `pMod->pEpoTab` de cada módulo, na ordem dos módulos.
    for (name, _) in hash_iter(&db.a_module) {
        if let Some(tab) = hash_find(&db.a_epo_tab, name) {
            tabs.push(tab.clone());
        }
    }
    for tab in &tabs {
        crate::vtab::vtab_disconnect(db, tab);
    }
    crate::vtab::vtab_unlock_list(db);
}

/// `connectionIsBusy`: há comandos preparados ou backups pendentes?
fn connection_is_busy(db: &Connection) -> bool {
    if !db.stmt_list.is_empty() {
        return true;
    }
    db.dbs.iter().any(|slot| slot.bt.as_ref().is_some_and(|bt| bt.n_backup > 0))
}

/// `sqlite3Close`: fecha a conexão. Com `force_zombie` (o `sqlite3_close_v2`) a conexão com
/// comandos ou backups pendentes vira zumbi e é liberada quando o último deles terminar
/// (`leave_mutex_and_close_zombie`); sem ele devolve `SQLITE_BUSY` e a conexão continua aberta.
pub fn close(db: &mut Connection, force_zombie: bool) -> i32 {
    if !safety_check_sick_or_ok(db) {
        return misuse_error(181023);
    }
    if (db.m_trace as u32 & SQLITE_TRACE_CLOSE) != 0 {
        if let Some(trace) = db.x_trace.as_mut() {
            trace(&TraceEvent::Close);
        }
    }

    // Força o `xDisconnect` de todas as tabelas virtuais.
    disconnect_all_vtab(db);

    // Com uma transação aberta, o `disconnect_all_vtab` não chamou o `xDisconnect` das tabelas
    // de `a_v_trans`; o rollback chama. É preciso antes da checagem dos comandos ativos, pois a
    // implementação da tabela virtual pode guardar comandos preparados.
    crate::vtab::vtab_rollback(db);

    // Comportamento legado (`sqlite3_close()`): `SQLITE_BUSY` se não dá para fechar já.
    if !force_zombie && connection_is_busy(db) {
        error_with_msg(
            db,
            SQLITE_BUSY,
            b"unable to close due to unfinalized statements or unfinished backups",
            &[],
        );
        return SQLITE_BUSY;
    }

    // Os dados do cliente (o destrutor roda no `Drop`, do mais novo ao mais antigo).
    db.p_db_data.clear();

    // Transforma a conexão em zumbi e a fecha.
    db.e_open_state = SQLITE_STATE_ZOMBIE;
    leave_mutex_and_close_zombie(db);
    SQLITE_OK
}

/// `sqlite3_txn_state`: o estado de transação de um banco, ou o máximo entre os bancos anexados
/// se `z_schema` for `None`.
pub fn txn_state(db: &Connection, z_schema: Option<&[u8]>) -> i32 {
    let mut i_txn = -1;
    let mut i_db: i32;
    let n_db: i32;
    if let Some(z) = z_schema {
        i_db = find_db_name(db, Some(z));
        n_db = if i_db < 0 { i_db - 1 } else { i_db };
    } else {
        i_db = 0;
        n_db = db.dbs.len() as i32 - 1;
    }
    while i_db <= n_db {
        let x = btree_txn_state(db.dbs[i_db as usize].bt.as_ref());
        if x > i_txn {
            i_txn = x;
        }
        i_db += 1;
    }
    i_txn
}

/// O sinal de interrupção da conexão, no formato que o checkpoint e o fechamento do pager
/// esperam: zero para seguir, ou o código a propagar.
fn interrupt_closure(db: &Connection) -> impl FnMut() -> i32 + use<> {
    let flag = db.interrupted.clone();
    let malloc_failed = db.malloc_failed;
    move || {
        if flag.load(Ordering::SeqCst) {
            if malloc_failed != 0 {
                SQLITE_NOMEM_BKPT
            } else {
                SQLITE_INTERRUPT
            }
        } else {
            0
        }
    }
}

/// `sqlite3BtreeClose` com o que o `sqlite3PagerClose` lê da conexão.
pub(crate) fn close_btree(db: &Connection, bt: Btree) {
    let mut interrupt = interrupt_closure(db);
    btree_close(
        bt,
        &BtDb::default(),
        Some(PagerCloseDb {
            no_ckpt_on_close: (db.flags & SQLITE_NO_CKPT_ON_CLOSE) != 0,
            interrupt: &mut interrupt,
        }),
    );
}

/// `sqlite3LeaveMutexAndCloseZombie`: se a conexão é um zumbi (já passou por `close`) e não resta
/// comando nem backup, libera tudo. O `Connection` em si é de quem o possui; fica em
/// `SQLITE_STATE_CLOSED`.
pub fn leave_mutex_and_close_zombie(db: &mut Connection) {
    if db.e_open_state != SQLITE_STATE_ZOMBIE || connection_is_busy(db) {
        return;
    }

    // Se há transação aberta, desfaz. Isso também garante que um esquema alterado por uma
    // transação não confirmada seja redefinido.
    rollback_all(db, SQLITE_OK);

    // Libera os savepoints pendentes.
    close_savepoints(db);

    // Fecha todos os bancos.
    for j in 0..db.dbs.len() {
        if let Some(bt) = db.dbs[j].bt.take() {
            close_btree(db, bt);
            if j != 1 {
                // O esquema pertence à árvore-b: sai junto com ela.
                schema_clear(db, j);
            }
        }
    }
    // O esquema do TEMP é limpo à parte e por último.
    if db.dbs.len() > 1 {
        schema_clear(db, 1);
    }
    crate::vtab::vtab_unlock_list(db);

    // Compacta o vetor de bancos auxiliares.
    collapse_database_array(db);
    debug_assert!(db.dbs.len() <= 2);

    // `sqlite3ConnectionClosed` (notify.c).
    crate::notify::connection_closed(db);

    // As funções (o `functionDestroy` roda no `Drop` do `FuncDestructor`).
    hash_clear(&mut db.a_func);
    // As colações (o `xDel` roda no `Drop` do fechamento que a guarda).
    hash_clear(&mut db.a_coll_seq);
    let modules: Vec<Vec<u8>> = hash_iter(&db.a_module).map(|(k, _)| k.to_vec()).collect();
    for name in &modules {
        crate::vtab::vtab_eponymous_table_clear(db, name);
    }
    // `sqlite3VtabModuleUnref`: o `Drop` do `Module` roda o `xDestroy`.
    hash_clear(&mut db.a_module);

    error(db, SQLITE_OK); // Libera as mensagens de erro guardadas.
    crate::loadext::close_extensions(db);

    db.e_open_state = SQLITE_STATE_ERROR;

    // O destrutor do gancho de autovacuum.
    db.x_autovac_pages = None;
    db.e_open_state = SQLITE_STATE_CLOSED;
}

/// `sqlite3RollbackAll`: desfaz todos os arquivos de banco. Com `trip_code` diferente de
/// `SQLITE_OK` os cursores de escrita são invalidados e passam a devolver `trip_code`; os de
/// leitura continuam abertos mas são salvos.
pub fn rollback_all(db: &mut Connection, trip_code: i32) {
    let mut in_trans = false;

    let schema_change = (db.m_db_flags & DBFLAG_SCHEMA_CHANGE) != 0 && db.init.busy == 0;

    for i in 0..db.dbs.len() {
        if db.dbs[i].bt.is_none() {
            continue;
        }
        if btree_txn_state(db.dbs[i].bt.as_ref()) == SQLITE_TXN_WRITE {
            in_trans = true;
        }
        with_bt_db(db, i, |bt, bdb| btree_rollback(bt, trip_code, !schema_change, bdb));
    }
    crate::vtab::vtab_rollback(db);

    if schema_change {
        expire_prepared_statements(db, 0);
        reset_all_schemas_of_connection(db);
    }

    // Qualquer violação de restrição adiada está resolvida.
    db.n_deferred_cons = 0;
    db.n_deferred_imm_cons = 0;
    db.flags &= !(SQLITE_DEFER_FKS | SQLITE_CORRUPT_RD_ONLY);

    // Se há um gancho de rollback configurado, chama.
    if in_trans || db.auto_commit == 0 {
        if let Some(cb) = db.x_rollback_callback.as_mut() {
            cb();
        }
    }
}

// ---------------------------------------------------------------------------------------------
// chunk 004: busy handler, progresso, interrupção
// ---------------------------------------------------------------------------------------------

/// `sqliteDefaultBusyCallback`: dorme e tenta de novo até estourar `tmout` milissegundos.
/// Devolve diferente de zero para tentar a trava de novo.
fn default_busy_callback(vfs: &VfsRef, tmout: i32, count: i32) -> i32 {
    // `HAVE_NANOSLEEP`: dorme em frações de segundo.
    const DELAYS: [u8; 12] = [1, 2, 5, 10, 15, 20, 25, 25, 25, 50, 50, 100];
    const TOTALS: [u8; 12] = [0, 1, 3, 8, 18, 33, 53, 78, 103, 128, 178, 228];
    let ndelay = DELAYS.len() as i32;
    debug_assert!(count >= 0);
    let mut delay: i32;
    let prior: i32;
    if count < ndelay {
        delay = DELAYS[count as usize] as i32;
        prior = TOTALS[count as usize] as i32;
    } else {
        delay = DELAYS[DELAYS.len() - 1] as i32;
        prior = TOTALS[TOTALS.len() - 1] as i32 + delay * (count - (ndelay - 1));
    }
    if prior + delay > tmout {
        delay = tmout - prior;
        if delay <= 0 {
            return 0;
        }
    }
    vfs.sleep(delay * 1000);
    1
}

/// `sqlite3InvokeBusyHandler`: invoca o busy handler quando uma operação não conseguiu a trava
/// de um arquivo. Devolve diferente de zero para tentar de novo e zero para abortar com
/// `SQLITE_BUSY`.
pub fn invoke_busy_handler(p: &BusyHandler) -> i32 {
    let Some(handler) = p.x_busy_handler.as_ref() else {
        return 0;
    };
    let n_busy = p.n_busy.get();
    if n_busy < 0 {
        return 0;
    }
    let rc = handler(n_busy);
    if rc == 0 {
        p.n_busy.set(-1);
    } else {
        p.n_busy.set(n_busy + 1);
    }
    rc
}

/// Instala em todos os pagers o gancho que chama o busy handler corrente da conexão (o
/// `btreeInvokeBusyHandler` do C, que lê `pBt->db->busyHandler`). O busy handler é copiado para
/// dentro do gancho, então isto roda de novo sempre que ele muda e depois de anexar um banco.
pub fn install_busy_handlers(db: &mut Connection) {
    let handler = db.busy_handler.clone();
    for slot in db.dbs.iter_mut() {
        if let Some(bt) = slot.bt.as_mut() {
            let h = handler.clone();
            bt.bt.pager.set_busy_handler(Some(Box::new(move || invoke_busy_handler(&h))));
        }
    }
}

/// `sqlite3_busy_handler`: o gancho recebe quantas vezes a tabela já ocupou e devolve diferente
/// de zero para tentar de novo. `None` remove o gancho.
pub fn busy_handler(db: &mut Connection, x_busy: Option<Rc<dyn Fn(i32) -> i32>>) -> i32 {
    db.busy_handler.x_busy_handler = x_busy;
    db.busy_handler.n_busy.set(0);
    db.busy_timeout = 0;
    install_busy_handlers(db);
    SQLITE_OK
}

/// `sqlite3_progress_handler`: o gancho roda a cada `n_ops` instruções da máquina virtual; um
/// retorno diferente de zero interrompe. `n_ops <= 0` remove o gancho.
pub fn progress_handler(db: &mut Connection, n_ops: i32, x_progress: Option<ProgressFn>) {
    if n_ops > 0 {
        db.x_progress = x_progress;
        db.n_progress_ops = n_ops as u32;
    } else {
        db.x_progress = None;
        db.n_progress_ops = 0;
    }
}

/// `sqlite3_busy_timeout`: instala o busy handler padrão, que espera `ms` milissegundos.
pub fn busy_timeout(db: &mut Connection, ms: i32) -> i32 {
    if ms > 0 {
        let vfs = db.p_vfs.clone();
        busy_handler(
            db,
            Some(Rc::new(move |count| match vfs.as_ref() {
                Some(v) => default_busy_callback(v, ms, count),
                None => 0,
            })),
        );
        db.busy_timeout = ms;
    } else {
        busy_handler(db, None);
    }
    SQLITE_OK
}

/// `sqlite3_interrupt`: faz a operação em curso parar assim que possível. Pode ser chamada de
/// outra thread com um clone de `Connection.interrupted`.
pub fn interrupt(db: &Connection) {
    db.interrupted.store(true, Ordering::SeqCst);
}

/// `sqlite3_is_interrupted`.
pub fn is_interrupted(db: &Connection) -> bool {
    db.interrupted.load(Ordering::SeqCst)
}

// ---------------------------------------------------------------------------------------------
// chunk 005: funções do usuário
// ---------------------------------------------------------------------------------------------

/// `sqlite3CreateFunc`: cria ou redefine uma função SQL na conexão. `p_destructor` é compartilhado
/// pelas até três definições de uma chamada com `SQLITE_ANY`; o `xDestroy` roda quando a última
/// definição o solta (o `functionDestroy`/`nRef` do C é a contagem do `Rc`).
pub fn create_func(
    db: &mut Connection,
    z_function_name: &[u8],
    n_arg: i32,
    enc: i32,
    p_user_data: UserData,
    x_s_func: Option<ScalarFn>,
    x_step: Option<ScalarFn>,
    x_final: Option<FinalFn>,
    x_value: Option<FinalFn>,
    x_inverse: Option<ScalarFn>,
    p_destructor: Option<Rc<FuncDestructor>>,
) -> i32 {
    debug_assert!(x_value.is_none() || x_s_func.is_none());
    let z_function_name = cstr(z_function_name);
    if (x_s_func.is_some() && x_final.is_some()) // Não os dois, x_s_func e x_final
        || (x_final.is_none() != x_step.is_none()) // Os dois ou nenhum, x_final e x_step
        || (x_value.is_none() != x_inverse.is_none()) // Os dois ou nenhum, x_value e x_inverse
        || !(-1..=SQLITE_MAX_FUNCTION_ARG).contains(&n_arg)
        || 255 < strlen30(z_function_name)
    {
        return misuse_error(181678);
    }

    debug_assert!(SQLITE_FUNC_CONSTANT as i32 == SQLITE_DETERMINISTIC);
    debug_assert!(SQLITE_FUNC_DIRECT as i32 == SQLITE_DIRECTONLY);
    let mut extra_flags: u32 = (enc
        & (SQLITE_DETERMINISTIC
            | SQLITE_DIRECTONLY
            | SQLITE_SUBTYPE
            | SQLITE_INNOCUOUS
            | SQLITE_RESULT_SUBTYPE)) as u32;
    let mut enc = enc & (SQLITE_FUNC_ENCMASK as i32 | SQLITE_ANY);

    // O `SQLITE_INNOCUOUS` é o mesmo bit de `SQLITE_FUNC_UNSAFE`, mas com o sentido invertido.
    debug_assert!(SQLITE_FUNC_UNSAFE as i32 == SQLITE_INNOCUOUS);
    extra_flags ^= SQLITE_FUNC_UNSAFE; // tag-20230109-1

    // `SQLITE_UTF16` vira LE ou BE conforme a ordem nativa; `SQLITE_ANY` cria três versões.
    match enc {
        SQLITE_UTF16 => {
            enc = SQLITE_UTF16NATIVE;
        }
        SQLITE_ANY => {
            let mut rc = create_func(
                db,
                z_function_name,
                n_arg,
                ((SQLITE_UTF8 as u32 | extra_flags) ^ SQLITE_FUNC_UNSAFE) as i32,
                p_user_data.clone(),
                x_s_func,
                x_step,
                x_final,
                x_value,
                x_inverse,
                p_destructor.clone(),
            );
            if rc == SQLITE_OK {
                rc = create_func(
                    db,
                    z_function_name,
                    n_arg,
                    ((SQLITE_UTF16LE as u32 | extra_flags) ^ SQLITE_FUNC_UNSAFE) as i32,
                    p_user_data.clone(),
                    x_s_func,
                    x_step,
                    x_final,
                    x_value,
                    x_inverse,
                    p_destructor.clone(),
                );
            }
            if rc != SQLITE_OK {
                return rc;
            }
            enc = SQLITE_UTF16BE;
        }
        SQLITE_UTF8 | SQLITE_UTF16LE | SQLITE_UTF16BE => {}
        _ => {
            enc = SQLITE_UTF8;
        }
    }

    // Uma função existente sendo redefinida ou apagada? Com VMs ativas devolve `SQLITE_BUSY`;
    // sem elas a operação segue e os comandos já preparados são invalidados.
    let existing = find_function(db, z_function_name, n_arg, enc as u8, 0);
    match &existing {
        Some(p) if (p.func_flags & SQLITE_FUNC_ENCMASK) == enc as u32 && p.n_arg as i32 == n_arg => {
            if db.n_vdbe_active != 0 {
                error_with_msg(
                    db,
                    SQLITE_BUSY,
                    b"unable to delete/modify user-function due to active statements",
                    &[],
                );
                debug_assert!(db.malloc_failed == 0);
                return SQLITE_BUSY;
            }
            expire_prepared_statements(db, 0);
        }
        _ => {
            if x_s_func.is_none() && x_final.is_none() {
                // Apagar uma função que não existe não faz nada.
                // https://sqlite.org/forum/forumpost/726219164b
                return SQLITE_OK;
            }
        }
    }
    drop(existing);

    let Some(p) = find_function(db, z_function_name, n_arg, enc as u8, 1) else {
        return SQLITE_NOMEM_BKPT;
    };

    // O `FuncDef` compartilhado é imutável: a definição nova ocupa o lugar da vaga que
    // `find_function` acabou de criar (ou da que já existia) na cadeia do nome.
    let new_def = Rc::new(FuncDef {
        n_arg: n_arg as i8,
        func_flags: (p.func_flags & SQLITE_FUNC_ENCMASK) | extra_flags,
        p_user_data,
        x_s_func: x_s_func.or(x_step),
        x_finalize: x_final,
        x_value,
        x_inverse,
        z_name: p.z_name.clone(),
        p_destructor,
    });
    if let Some(chain) = hash_find_mut(&mut db.a_func, &p.z_name) {
        if let Some(slot) = chain.iter_mut().find(|q| Rc::ptr_eq(q, &p)) {
            // A definição antiga solta o destrutor (o `functionDestroy`).
            *slot = new_def;
        }
    }
    SQLITE_OK
}

/// `createFunctionApi`: o trabalho das APIs UTF-8 que criam funções (`create_function`,
/// `create_function_v2` e `create_window_function`). `x_destroy` é chamado quando a última
/// definição que usa o dado do usuário some, ou já aqui se a criação falhar.
pub fn create_function_api(
    db: &mut Connection,
    z_func: &[u8],
    n_arg: i32,
    enc: i32,
    p: UserData,
    x_s_func: Option<ScalarFn>,
    x_step: Option<ScalarFn>,
    x_final: Option<FinalFn>,
    x_value: Option<FinalFn>,
    x_inverse: Option<ScalarFn>,
    x_destroy: Option<DestroyFn>,
) -> i32 {
    let p_arg = x_destroy.map(|destroy| {
        let user = match &p {
            UserData::Ptr(rc) => Some(rc.clone()),
            _ => None,
        };
        Rc::new(FuncDestructor { p_user_data: user, x_destroy: Some(destroy) })
    });
    let rc = create_func(
        db, z_func, n_arg, enc, p, x_s_func, x_step, x_final, x_value, x_inverse, p_arg.clone(),
    );
    // `pArg->nRef==0`: nenhuma definição guardou o destrutor, então soltá-lo aqui o executa.
    drop(p_arg);
    api_exit(db, rc)
}

/// Lê um texto UTF-16 nativo terminado por um par de zeros e o converte para UTF-8
/// (`sqlite3Utf16to8(db, z, -1, SQLITE_UTF16NATIVE)`).
fn utf16_to_8(z: &[u8]) -> Vec<u8> {
    let mut n = 0usize;
    while n + 1 < z.len() && (z[n] | z[n + 1]) != 0 {
        n += 2;
    }
    translate_bytes(&z[..n.min(z.len())], SQLITE_UTF16NATIVE as u8, SQLITE_UTF8 as u8)
}

/// `sqlite3_create_function16`: o nome em UTF-16 nativo.
pub fn create_function16(
    db: &mut Connection,
    z_function_name: &[u8],
    n_arg: i32,
    e_text_rep: i32,
    p: UserData,
    x_s_func: Option<ScalarFn>,
    x_step: Option<ScalarFn>,
    x_final: Option<FinalFn>,
) -> i32 {
    debug_assert!(db.malloc_failed == 0);
    let z_func8 = utf16_to_8(z_function_name);
    let rc = create_func(db, &z_func8, n_arg, e_text_rep, p, x_s_func, x_step, x_final, None, None, None);
    api_exit(db, rc)
}

/// `sqlite3InvalidFunction`: a função que sempre falha dizendo que foi usada no contexto errado.
/// O `sqlite3_overload_function` a instala para a resolução de nomes; a tabela virtual a
/// sobrecarrega com o `xFindFunction`.
fn invalid_function(ctx: &mut Context<'_>, _args: &[Mem]) {
    let name = match user_data(ctx) {
        UserData::Ptr(p) => p.downcast_ref::<Vec<u8>>().cloned().unwrap_or_default(),
        _ => Vec::new(),
    };
    let z_err = mprintf(
        b"unable to use function %s in the requested context",
        &[PrintfArg::Text(Some(name))],
    )
    .unwrap_or_default();
    result_error(ctx, &z_err, -1);
}

/// `sqlite3_overload_function`: declara que uma função foi sobrecarregada por uma tabela
/// virtual. Se já existe como função global não faz nada; senão cria uma que sempre falha.
pub fn overload_function(db: &mut Connection, z_name: &[u8], n_arg: i32) -> i32 {
    let exists = find_function(db, z_name, n_arg, SQLITE_UTF8 as u8, 0).is_some();
    if exists {
        return SQLITE_OK;
    }
    let copy: Rc<dyn Any> = Rc::new(cstr(z_name).to_vec());
    create_function_api(
        db,
        z_name,
        n_arg,
        SQLITE_UTF8,
        UserData::Ptr(copy),
        Some(invalid_function),
        None,
        None,
        None,
        None,
        None,
    )
}

/// `sqlite3_trace`: o gancho recebe o texto de cada comando SQL. Devolve nada (o `pArg` anterior
/// do C é a captura do fechamento).
pub fn trace(db: &mut Connection, x_trace: Option<Box<dyn FnMut(&[u8])>>) {
    db.m_trace = if x_trace.is_some() { SQLITE_TRACE_LEGACY } else { 0 };
    db.x_trace = x_trace.map(|mut f| -> TraceFn {
        Box::new(move |ev: &TraceEvent| {
            if let TraceEvent::Stmt { sql, .. } = ev {
                f(sql);
            }
        })
    });
}

/// `sqlite3_trace_v2`: o gancho recebe os eventos pedidos em `m_trace` (`SQLITE_TRACE_STMT`,
/// `PROFILE`, `ROW`, `CLOSE`).
pub fn trace_v2(db: &mut Connection, m_trace: u32, x_trace: Option<TraceFn>) -> i32 {
    let mut m_trace = m_trace;
    let mut x_trace = x_trace;
    if m_trace == 0 {
        x_trace = None;
    }
    if x_trace.is_none() {
        m_trace = 0;
    }
    db.m_trace = m_trace as u8;
    db.x_trace = x_trace;
    SQLITE_OK
}

// ---------------------------------------------------------------------------------------------
// chunk 006: perfil, ganchos de commit, atualização, rollback, WAL, checkpoint
// ---------------------------------------------------------------------------------------------

/// `sqlite3_profile`: o gancho recebe o texto e a duração (ns) de cada comando concluído.
pub fn profile(db: &mut Connection, x_profile: Option<ProfileFn>) {
    db.x_profile = x_profile;
    db.m_trace &= SQLITE_TRACE_NONLEGACY_MASK;
    if db.x_profile.is_some() {
        db.m_trace |= SQLITE_TRACE_XPROFILE;
    }
}

/// `sqlite3_commit_hook`: se o gancho devolve diferente de zero, o commit vira rollback.
pub fn commit_hook(db: &mut Connection, x_callback: Option<CommitHook>) {
    db.x_commit_callback = x_callback;
}

/// `sqlite3_update_hook`: chamado a cada linha atualizada, inserida ou apagada.
pub fn update_hook(db: &mut Connection, x_callback: Option<UpdateHook>) {
    db.x_update_callback = x_callback;
}

/// `sqlite3_rollback_hook`: chamado a cada rollback de transação.
pub fn rollback_hook(db: &mut Connection, x_callback: Option<RollbackHook>) {
    db.x_rollback_callback = x_callback;
}

/// `sqlite3_preupdate_hook`.
pub fn preupdate_hook(db: &mut Connection, x_callback: Option<PreUpdateFn>) {
    db.x_pre_update_callback = x_callback;
}

/// `sqlite3_autovacuum_pages`: o gancho que decide quantas páginas o autovacuum libera. O
/// gancho anterior é solto (o seu destrutor roda no `Drop`).
pub fn autovacuum_pages(db: &mut Connection, x_callback: Option<AutovacPagesFn>) -> i32 {
    db.x_autovac_pages = x_callback;
    SQLITE_OK
}

/// `sqlite3_wal_hook`: o gancho recebe o nome do banco e o tamanho do WAL em quadros depois de
/// cada commit que escreve nele. Substitui o checkpoint automático de `wal_autocheckpoint`.
pub fn wal_hook(db: &mut Connection, x_callback: Option<WalHook>) {
    db.x_wal_callback = x_callback;
}

/// `sqlite3_wal_autocheckpoint`: faz checkpoint de um banco depois de um commit que deixe `n_frame`
/// ou mais quadros no WAL. Zero ou negativo desliga. O gancho padrão (`sqlite3WalDefaultHook`)
/// só anota o banco; [`wal_default_hook_flush`] roda o checkpoint (ver o cabeçalho do módulo).
pub fn wal_autocheckpoint(db: &mut Connection, n_frame: i32) -> i32 {
    if n_frame > 0 {
        wal_hook(
            db,
            Some(Box::new(move |z_db: &[u8], n: i32| {
                if n >= n_frame {
                    let mut pending = PENDING_CHECKPOINTS.with(Cell::take);
                    pending.push(z_db.to_vec());
                    PENDING_CHECKPOINTS.with(|c| c.set(pending));
                }
                SQLITE_OK
            })),
        );
    } else {
        wal_hook(db, None);
    }
    SQLITE_OK
}

/// Roda os checkpoints (PASSIVE) que o gancho padrão de `wal_autocheckpoint` anotou. Quem
/// invoca `Connection.x_wal_callback` chama isto no fim, para a mesma conexão.
pub fn wal_default_hook_flush(db: &mut Connection) {
    let pending = PENDING_CHECKPOINTS.with(Cell::take);
    for z_db in pending {
        wal_checkpoint_v2(db, Some(&z_db), SQLITE_CHECKPOINT_PASSIVE);
    }
}

/// `sqlite3_wal_checkpoint_v2`: faz checkpoint do banco `z_db` (todos se for nulo ou vazio).
/// Devolve o código, o tamanho do log em quadros e o total de quadros do checkpoint (-1 em
/// erro).
pub fn wal_checkpoint_v2(db: &mut Connection, z_db: Option<&[u8]>, e_mode: i32) -> (i32, i32, i32) {
    // Em caso de erro as saídas ficam em -1.
    let mut n_log = -1;
    let mut n_ckpt = -1;

    debug_assert!(SQLITE_CHECKPOINT_PASSIVE == 0);
    debug_assert!(SQLITE_CHECKPOINT_TRUNCATE == 3);
    if !(SQLITE_CHECKPOINT_PASSIVE..=SQLITE_CHECKPOINT_TRUNCATE).contains(&e_mode) {
        // EVIDENCE-OF: R-03996-12088 O parâmetro M precisa ser um modo de checkpoint válido.
        return (misuse_error(182293), n_log, n_ckpt);
    }

    let i_db = match z_db {
        Some(z) if z.first().is_some_and(|&c| c != 0) => find_db_name(db, Some(z)),
        _ => SQLITE_MAX_DB, // Processa todos os esquemas.
    };
    let mut rc;
    if i_db < 0 {
        rc = SQLITE_ERROR;
        error_with_msg(
            db,
            SQLITE_ERROR,
            b"unknown database: %s",
            &[PrintfArg::Text(z_db.map(|z| z.to_vec()))],
        );
    } else {
        db.busy_handler.n_busy.set(0);
        rc = checkpoint(db, i_db, e_mode, &mut n_log, &mut n_ckpt);
        error(db, rc);
    }
    rc = api_exit(db, rc);

    // Sem comandos ativos, limpa a bandeira de interrupção.
    if db.n_vdbe_active == 0 {
        db.interrupted.store(false, Ordering::SeqCst);
    }
    (rc, n_log, n_ckpt)
}

/// `sqlite3Checkpoint`: faz o checkpoint do banco `i_db` (todos se for `SQLITE_MAX_DB`). Não faz
/// nada se o banco não está em modo WAL. Com transação aberta no banco devolve `SQLITE_LOCKED`.
/// Um erro é devolvido na hora (os bancos restantes não são tentados). As saídas `pn_log` e
/// `pn_ckpt` recebem os valores do primeiro banco processado.
pub fn checkpoint(
    db: &mut Connection,
    i_db: i32,
    e_mode: i32,
    pn_log: &mut i32,
    pn_ckpt: &mut i32,
) -> i32 {
    let mut rc = SQLITE_OK;
    let mut b_busy = false;
    let mut first = true;
    let mut interrupt = interrupt_closure(db);

    debug_assert!(*pn_log == -1);
    debug_assert!(*pn_ckpt == -1);

    let mut i = 0usize;
    while i < db.dbs.len() && rc == SQLITE_OK {
        if i as i32 == i_db || i_db == SQLITE_MAX_DB {
            // Um banco de zero bytes que recebeu `PRAGMA journal_mode=WAL` ainda não abriu o WAL:
            // o `sqlite3PagerCheckpoint` do C roda `PRAGMA table_list` para iniciá-lo.
            let needs_wal_init = db.dbs[i].bt.as_ref().is_some_and(|b| {
                b.bt.in_transaction == TRANS_NONE && b.bt.pager.checkpoint_needs_wal_init()
            });
            if needs_wal_init {
                crate::legacy::exec(db, b"PRAGMA table_list", None);
            }
            let (l, c) = if first {
                (Some(&mut *pn_log), Some(&mut *pn_ckpt))
            } else {
                (None, None)
            };
            rc = btree_checkpoint(db.dbs[i].bt.as_mut(), &mut interrupt, e_mode, l, c);
            first = false;
            if rc == SQLITE_BUSY {
                b_busy = true;
                rc = SQLITE_OK;
            }
        }
        i += 1;
    }

    if rc == SQLITE_OK && b_busy {
        SQLITE_BUSY
    } else {
        rc
    }
}

// ---------------------------------------------------------------------------------------------
// chunk 007: erros da API, colações, limites, URI
// ---------------------------------------------------------------------------------------------

/// `sqlite3TempInMemory`: o armazenamento temporário (arquivos transitórios do pager e journals
/// de comando) deve ficar em memória? Com `SQLITE_TEMP_STORE=1`, só se `temp_store` for 2.
pub fn temp_in_memory(db: &Connection) -> bool {
    db.temp_store == 2
}

/// `sqlite3_errmsg`: a explicação, em inglês e UTF-8, do erro mais recente.
pub fn errmsg(db: &Connection) -> Vec<u8> {
    if !safety_check_sick_or_ok(db) {
        misuse_error(182429);
        return err_str(SQLITE_MISUSE).as_bytes().to_vec();
    }
    if db.malloc_failed != 0 {
        return err_str(SQLITE_NOMEM_BKPT).as_bytes().to_vec();
    }
    let z = if db.err_code != 0 { db.err_msg.clone() } else { None };
    z.unwrap_or_else(|| err_str(db.err_code).as_bytes().to_vec())
}

/// `sqlite3_error_offset`: o deslocamento, em bytes, do erro mais recente no SQL, ou -1.
pub fn error_offset(db: &Connection) -> i32 {
    if safety_check_sick_or_ok(db) && db.err_code != 0 {
        db.err_byte_offset
    } else {
        -1
    }
}

/// `sqlite3_errmsg16`: como [`errmsg`], em UTF-16 nativo (sem o par de zeros final).
pub fn errmsg16(db: &mut Connection) -> Vec<u8> {
    const OUT_OF_MEM: &[u8] = b"out of memory";
    const MISUSE: &[u8] = b"bad parameter or other API misuse";
    if !safety_check_sick_or_ok(db) {
        return translate_bytes(MISUSE, SQLITE_UTF8 as u8, SQLITE_UTF16NATIVE as u8);
    }
    let z8 = if db.malloc_failed != 0 {
        OUT_OF_MEM.to_vec()
    } else {
        let z = match db.err_msg.clone() {
            Some(z) => z,
            None => {
                let code = db.err_code;
                error_with_msg(db, code, err_str(code).as_bytes(), &[]);
                db.err_msg.clone().unwrap_or_default()
            }
        };
        // Pode ter faltado memória na conversão: limpa o sinal direto, sem `api_exit`, para não
        // gravar a mensagem de erro na conexão.
        oom_clear(db);
        z
    };
    translate_bytes(&z8, SQLITE_UTF8 as u8, SQLITE_UTF16NATIVE as u8)
}

/// `sqlite3_errcode`: o código de erro mais recente (sem os códigos estendidos, a menos que
/// `extended_result_codes` os tenha ligado).
pub fn errcode(db: &Connection) -> i32 {
    if !safety_check_sick_or_ok(db) {
        return misuse_error(182508);
    }
    if db.malloc_failed != 0 {
        return SQLITE_NOMEM_BKPT;
    }
    db.err_code & db.err_mask
}

/// `sqlite3_extended_errcode`.
pub fn extended_errcode(db: &Connection) -> i32 {
    if !safety_check_sick_or_ok(db) {
        return misuse_error(182517);
    }
    if db.malloc_failed != 0 {
        return SQLITE_NOMEM_BKPT;
    }
    db.err_code
}

/// `sqlite3_system_errno`.
pub fn system_errno(db: &Connection) -> i32 {
    db.i_sys_errno
}

/// O destrutor de uma colação do usuário: roda quando o fechamento da comparação (guardado no
/// `CollSeq`) é solto, isto é, quando a colação é substituída, removida ou a conexão fecha.
struct CollDestructor {
    del: Option<Box<dyn FnOnce()>>,
}

impl Drop for CollDestructor {
    fn drop(&mut self) {
        if let Some(del) = self.del.take() {
            del();
        }
    }
}

/// `createCollation`: cria ou substitui uma colação na conexão. `x_cmp` nulo remove a colação.
pub fn create_collation(db: &mut Connection, z_name: &[u8], enc: u8, x_cmp: Option<CollFn>) -> i32 {
    // `SQLITE_UTF16` e `SQLITE_UTF16_ALIGNED` viram a codificação nativa; `SQLITE_UTF16` não é
    // usada internamente.
    let mut enc2 = enc as i32;
    if enc2 == SQLITE_UTF16 || enc2 == SQLITE_UTF16_ALIGNED {
        enc2 = SQLITE_UTF16NATIVE;
    }
    if enc2 < SQLITE_UTF8 || enc2 > SQLITE_UTF16BE {
        return misuse_error(182565);
    }

    // Esta chamada está removendo ou substituindo uma colação? Se sim, com VMs ativas devolve
    // `SQLITE_BUSY`; sem elas os comandos já compilados são invalidados.
    if let Some(p_coll) = find_coll_seq(db, enc2 as u8, Some(z_name), 0) {
        if db.n_vdbe_active != 0 {
            error_with_msg(
                db,
                SQLITE_BUSY,
                b"unable to delete/modify collation sequence due to active statements",
                &[],
            );
            return SQLITE_BUSY;
        }
        expire_prepared_statements(db, 0);

        // Se a colação foi criada direto por `create_collation`, e não gerada por
        // `synthCollSeq()`, as cópias que o `synthCollSeq()` fez precisam ser invalidadas (e o
        // destrutor da colação, chamado).
        if (p_coll.enc & !(SQLITE_UTF16_ALIGNED as u8)) as i32 == enc2 {
            if let Some(slots) = find_coll_seq_entry(db, z_name, 0) {
                for slot in slots.iter_mut() {
                    if slot.as_ref().is_some_and(|p| p.enc == p_coll.enc) {
                        *slot = None;
                    }
                }
            }
        }
        drop(p_coll);
    }

    let Some(slots) = find_coll_seq_entry(db, z_name, 1) else {
        return SQLITE_NOMEM_BKPT;
    };
    slots[(enc2 - 1) as usize] = x_cmp.map(|f| {
        Rc::new(CollSeq {
            name: z_name.to_vec(),
            enc: (enc2 | (enc as i32 & SQLITE_UTF16_ALIGNED)) as u8,
            x_cmp: f,
        })
    });
    error(db, SQLITE_OK);
    SQLITE_OK
}

/// `sqlite3_create_collation_v2`: registra uma colação do usuário. `x_compare` compara dois
/// textos na codificação `enc`; nulo remove a colação. `x_del` roda quando a colação some.
pub fn create_collation_v2(
    db: &mut Connection,
    z_name: &[u8],
    enc: i32,
    x_compare: Option<Rc<dyn Fn(&[u8], &[u8]) -> i32>>,
    x_del: Option<Box<dyn FnOnce()>>,
) -> i32 {
    debug_assert!(db.malloc_failed == 0);
    let guard = x_del.map(|del| Rc::new(CollDestructor { del: Some(del) }));
    let x_cmp = x_compare.map(move |f| {
        CollFn::User(Rc::new(move |a: &[u8], b: &[u8]| {
            let _keep = &guard;
            f(a, b)
        }))
    });
    let rc = create_collation(db, z_name, enc as u8, x_cmp);
    api_exit(db, rc)
}

/// `sqlite3_create_collation16`: o nome em UTF-16 nativo.
pub fn create_collation16(
    db: &mut Connection,
    z_name: &[u8],
    enc: i32,
    x_compare: Option<Rc<dyn Fn(&[u8], &[u8]) -> i32>>,
) -> i32 {
    debug_assert!(db.malloc_failed == 0);
    let z_name8 = utf16_to_8(z_name);
    let x_cmp = x_compare.map(CollFn::User);
    let rc = create_collation(db, &z_name8, enc as u8, x_cmp);
    api_exit(db, rc)
}

/// `sqlite3_collation_needed`: a fábrica de colações, chamada quando uma colação desconhecida é
/// pedida (recebe a codificação e o nome em UTF-8).
pub fn collation_needed(db: &mut Connection, x_coll_needed: Option<CollNeededFn>) -> i32 {
    db.x_coll_needed = x_coll_needed;
    db.coll_needed_16 = false;
    SQLITE_OK
}

/// `sqlite3_collation_needed16`: como [`collation_needed`], com o nome em UTF-16 nativo.
pub fn collation_needed16(db: &mut Connection, x_coll_needed16: Option<CollNeededFn>) -> i32 {
    db.x_coll_needed = x_coll_needed16;
    db.coll_needed_16 = true;
    SQLITE_OK
}

/// `aHardLimit[]`: os limites máximos de compilação, na ordem dos `SQLITE_LIMIT_*`.
const HARD_LIMIT: [i32; SQLITE_N_LIMIT as usize] = [
    SQLITE_MAX_LENGTH,
    SQLITE_MAX_SQL_LENGTH,
    SQLITE_MAX_COLUMN,
    SQLITE_MAX_EXPR_DEPTH,
    SQLITE_MAX_COMPOUND_SELECT,
    SQLITE_MAX_VDBE_OP,
    SQLITE_MAX_FUNCTION_ARG,
    SQLITE_MAX_ATTACHED,
    SQLITE_MAX_LIKE_PATTERN_LENGTH,
    SQLITE_MAX_VARIABLE_NUMBER, // IMP: R-38091-32352
    SQLITE_MAX_TRIGGER_DEPTH,
    SQLITE_MAX_WORKER_THREADS,
];

/// `sqlite3_limit`: muda um limite e devolve o valor antigo (-1 se o índice é inválido). Um
/// limite novo negativo não muda nada. Um limite novo mais baixo não encolhe construções que já
/// existem: só impede que se formem outras acima dele.
pub fn limit(db: &mut Connection, limit_id: i32, new_limit: i32) -> i32 {
    if limit_id < 0 || limit_id >= SQLITE_N_LIMIT {
        return -1;
    }
    let old_limit = db.a_limit[limit_id as usize];
    let mut new_limit = new_limit;
    if new_limit >= 0 {
        // IMP: R-52476-28732
        if new_limit > HARD_LIMIT[limit_id as usize] {
            new_limit = HARD_LIMIT[limit_id as usize]; // IMP: R-51463-25634
        } else if new_limit < 1 && limit_id == SQLITE_LIMIT_LENGTH {
            new_limit = 1;
        }
        db.a_limit[limit_id as usize] = new_limit;
    }
    old_limit // IMP: R-53341-35419
}

/// `sqlite3ParseUri`: interpreta um nome de arquivo, ou URI, passado a `sqlite3_open*` ou a
/// `ATTACH`. `z_default_vfs` é o VFS a usar se a URI não tiver `vfs=xxx`. `*p_flags` entra com as
/// bandeiras de abertura e pode sair alterado por `cache=xxx` e `mode=xxx`. Em sucesso devolve
/// `SQLITE_OK`, o VFS em `*pp_vfs` e em `*pz_file` o `sqlite3_filename`: o nome do arquivo, um
/// NUL, os pares nome e valor dos parâmetros (cada um com seu NUL), e quatro zeros finais (o
/// formato de `create_filename`, sem os quatro zeros iniciais). Em erro devolve o código e, às
/// vezes, a mensagem em `*pz_err_msg`.
pub fn parse_uri(
    z_default_vfs: Option<&[u8]>,
    z_uri: &[u8],
    p_flags: &mut u32,
    pp_vfs: &mut Option<VfsRef>,
    pz_file: &mut Option<Vec<u8>>,
    pz_err_msg: &mut Option<Vec<u8>>,
) -> i32 {
    let mut rc = SQLITE_OK;
    let mut flags = *p_flags;
    let mut z_vfs: Option<Vec<u8>> = z_default_vfs.map(|v| v.to_vec());
    let z_uri = cstr(z_uri);
    let n_uri = z_uri.len();
    let mut z_file: Vec<u8> = Vec::new();
    debug_assert!(pz_err_msg.is_none());

    'parse_uri_out: {
        let b_open_uri = lock_config().b_open_uri;
        if ((flags & SQLITE_OPEN_URI as u32) != 0 // IMP: R-48725-32206
            || b_open_uri) // IMP: R-51689-46548
            && n_uri >= 5
            && &z_uri[..5] == b"file:"
        // IMP: R-57884-37496
        {
            // Garante que a bandeira `SQLITE_OPEN_URI` fique ligada, para o `xOpen` do VFS saber
            // que pode haver parâmetros depois do nome.
            flags |= SQLITE_OPEN_URI as u32;

            // Descarta o esquema e a autoridade da URI.
            let mut i_in = 5usize;
            if at(z_uri, 5) == b'/' && at(z_uri, 6) == b'/' {
                i_in = 7;
                while at(z_uri, i_in) != 0 && at(z_uri, i_in) != b'/' {
                    i_in += 1;
                }
                if i_in != 7 && (i_in != 16 || &z_uri[7..16] != b"localhost") {
                    *pz_err_msg = mprintf(
                        b"invalid uri authority: %.*s",
                        &[
                            PrintfArg::Int((i_in - 7) as i64),
                            PrintfArg::Text(Some(z_uri[7..i_in].to_vec())),
                        ],
                    );
                    rc = SQLITE_ERROR;
                    break 'parse_uri_out;
                }
            }

            // Copia o nome do arquivo e os parâmetros de consulta para `z_file`, decodificando
            // os códigos %HH pelo caminho.
            //
            // `e_state` vale 0 (lendo o nome do arquivo), 1 (lendo o nome de um parâmetro
            // nome=valor) ou 2 (lendo o valor).
            let mut e_state = 0;
            loop {
                let mut c = at(z_uri, i_in);
                if c == 0 || c == b'#' {
                    break;
                }
                i_in += 1;
                if c == b'%' && is_xdigit(at(z_uri, i_in)) && is_xdigit(at(z_uri, i_in + 1)) {
                    let mut octet = (hex_to_int(at(z_uri, i_in) as i32) as i32) << 4;
                    i_in += 1;
                    octet += hex_to_int(at(z_uri, i_in) as i32) as i32;
                    i_in += 1;

                    debug_assert!((0..256).contains(&octet));
                    if octet == 0 {
                        // "%00" na URI: ignora o resto do texto do caminho, nome ou valor em
                        // curso: pula até o próximo "?", "=" ou "&", conforme o caso.
                        loop {
                            c = at(z_uri, i_in);
                            if c == 0
                                || c == b'#'
                                || (e_state == 0 && c == b'?')
                                || (e_state == 1 && (c == b'=' || c == b'&'))
                                || (e_state == 2 && c == b'&')
                            {
                                break;
                            }
                            i_in += 1;
                        }
                        continue;
                    }
                    c = octet as u8;
                } else if e_state == 1 && (c == b'&' || c == b'=') {
                    if z_file[z_file.len() - 1] == 0 {
                        // Nome de opção vazio: ignora a opção toda.
                        while at(z_uri, i_in) != 0
                            && at(z_uri, i_in) != b'#'
                            && at(z_uri, i_in - 1) != b'&'
                        {
                            i_in += 1;
                        }
                        continue;
                    }
                    if c == b'&' {
                        z_file.push(0);
                    } else {
                        e_state = 2;
                    }
                    c = 0;
                } else if (e_state == 0 && c == b'?') || (e_state == 2 && c == b'&') {
                    c = 0;
                    e_state = 1;
                }
                z_file.push(c);
            }
            if e_state == 1 {
                z_file.push(0);
            }
            z_file.extend_from_slice(&[0, 0, 0, 0]); // Fim das opções e nomes de journal vazios.

            // Há opções para interpretar aqui? São "vfs" e as que correspondem às bandeiras que
            // `sqlite3_open_v2()` aceita.
            let mut z_opt = strlen30(&z_file) as usize + 1;
            while at(&z_file, z_opt) != 0 {
                let n_opt = strlen30(&z_file[z_opt..]) as usize;
                let z_val = z_opt + n_opt + 1;
                let n_val = strlen30(&z_file[z_val..]) as usize;
                let opt = z_file[z_opt..z_opt + n_opt].to_vec();
                let val = z_file[z_val..z_val + n_val].to_vec();

                if n_opt == 3 && opt == b"vfs" {
                    z_vfs = Some(val);
                } else {
                    const CACHE_MODES: [(&[u8], i32); 2] = [
                        (b"shared", SQLITE_OPEN_SHAREDCACHE),
                        (b"private", SQLITE_OPEN_PRIVATECACHE),
                    ];
                    const OPEN_MODES: [(&[u8], i32); 4] = [
                        (b"ro", SQLITE_OPEN_READONLY),
                        (b"rw", SQLITE_OPEN_READWRITE),
                        (b"rwc", SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE),
                        (b"memory", SQLITE_OPEN_MEMORY),
                    ];
                    let mut a_mode: Option<&[(&[u8], i32)]> = None;
                    let mut z_mode_type = "";
                    let mut mask = 0i32;
                    let mut limit = 0i32;

                    if n_opt == 5 && opt == b"cache" {
                        mask = SQLITE_OPEN_SHAREDCACHE | SQLITE_OPEN_PRIVATECACHE;
                        a_mode = Some(&CACHE_MODES);
                        limit = mask;
                        z_mode_type = "cache";
                    }
                    if n_opt == 4 && opt == b"mode" {
                        mask = SQLITE_OPEN_READONLY
                            | SQLITE_OPEN_READWRITE
                            | SQLITE_OPEN_CREATE
                            | SQLITE_OPEN_MEMORY;
                        a_mode = Some(&OPEN_MODES);
                        limit = mask & flags as i32;
                        z_mode_type = "access";
                    }

                    if let Some(modes) = a_mode {
                        let mut mode = 0i32;
                        for (z, m) in modes {
                            if n_val == z.len() && val == *z {
                                mode = *m;
                                break;
                            }
                        }
                        if mode == 0 {
                            *pz_err_msg = mprintf(
                                b"no such %s mode: %s",
                                &[
                                    PrintfArg::Text(Some(z_mode_type.as_bytes().to_vec())),
                                    PrintfArg::Text(Some(val)),
                                ],
                            );
                            rc = SQLITE_ERROR;
                            break 'parse_uri_out;
                        }
                        if (mode & !SQLITE_OPEN_MEMORY) > limit {
                            *pz_err_msg = mprintf(
                                b"%s mode not allowed: %s",
                                &[
                                    PrintfArg::Text(Some(z_mode_type.as_bytes().to_vec())),
                                    PrintfArg::Text(Some(val)),
                                ],
                            );
                            rc = SQLITE_PERM;
                            break 'parse_uri_out;
                        }
                        flags = (flags & !(mask as u32)) | mode as u32;
                    }
                }

                z_opt = z_val + n_val + 1;
            }
        } else {
            z_file.extend_from_slice(z_uri);
            z_file.extend_from_slice(&[0, 0, 0, 0]);
            flags &= !(SQLITE_OPEN_URI as u32);
        }

        *pp_vfs = vfs_find(z_vfs.as_deref());
        if pp_vfs.is_none() {
            *pz_err_msg = mprintf(b"no such vfs: %s", &[PrintfArg::Text(z_vfs.clone())]);
            rc = SQLITE_ERROR;
        }
    }
    *p_flags = flags;
    *pz_file = if rc == SQLITE_OK { Some(z_file) } else { None };
    rc
}

// ---------------------------------------------------------------------------------------------
// chunk 008 e 009: abrir a conexão
// ---------------------------------------------------------------------------------------------

/// `sqlite3BtreeOpen` do banco principal e o resto de `openDatabase` depois do URI: a árvore-b,
/// os esquemas, as funções, as extensões embutidas e o lookaside.
fn open_main_database(db: &mut Connection, z_open: &[u8], flags: u32, cfg: &GlobalConfig) -> bool {
    // Abre o driver do banco.
    let Some(vfs) = db.p_vfs.clone() else {
        return false;
    };
    match btree_open(vfs, Some(z_open), 0, flags as i32 | SQLITE_OPEN_MAIN_DB) {
        Ok(mut bt) => {
            // `sqlite3PagerSetMmapLimit(pBt->pPager, db->szMmap)` do `sqlite3BtreeOpen`.
            btree_set_mmap_limit(&mut bt, db.sz_mmap);
            db.dbs[0].bt = Some(bt);
            install_busy_handlers(db);
            sync_btree_flags(db);
        }
        Err(mut rc) => {
            if rc == SQLITE_IOERR_NOMEM {
                rc = SQLITE_NOMEM_BKPT;
            }
            error(db, rc);
            return false;
        }
    }
    db.dbs[0].schema = schema_get(db, None);
    if db.malloc_failed == 0 {
        let enc = db.schema_enc();
        set_text_encoding(db, enc);
    }
    db.dbs[1].schema = schema_get(db, None);

    // O `safety_level` padrão do banco principal é FULL; o do temporário é OFF. Casa com os
    // padrões da camada do pager.
    db.dbs[0].z_db_s_name = b"main".to_vec();
    db.dbs[0].safety_level = (SQLITE_DEFAULT_SYNCHRONOUS + 1) as u8;
    db.dbs[1].z_db_s_name = b"temp".to_vec();
    db.dbs[1].safety_level = PAGER_SYNCHRONOUS_OFF as u8;

    db.e_open_state = SQLITE_STATE_OPEN;
    if db.malloc_failed != 0 {
        return false;
    }

    // Registra todas as funções embutidas, mas não lê o esquema ainda: isso fica para o primeiro
    // acesso ao banco.
    error(db, SQLITE_OK);
    crate::func::register_per_connection_builtin_functions(db);
    let mut rc = errcode(db);

    // Carrega as extensões embutidas.
    for init in BUILTIN_EXTENSIONS {
        if rc != SQLITE_OK {
            break;
        }
        rc = init(db);
    }

    // Carrega as extensões automáticas (`sqlite3_auto_extension`).
    if rc == SQLITE_OK {
        crate::loadext::auto_load_extensions(db);
        rc = errcode(db);
        if rc != SQLITE_OK {
            return false;
        }
    }

    if rc != 0 {
        error(db, rc);
    }

    // Liga o alocador de lookaside.
    setup_lookaside(db, false, cfg.sz_lookaside, cfg.n_lookaside);

    wal_autocheckpoint(db, SQLITE_DEFAULT_WAL_AUTOCHECKPOINT);
    true
}

/// `sqlite3TestExtInit`: a extensão que não faz nada (só falha com o simulador de falhas em 500).
fn test_ext_init(_db: &mut Connection) -> i32 {
    0
}

/// As extensões embutidas (`sqlite3BuiltinExtensions[]`), na ordem do C.
const BUILTIN_EXTENSIONS: [fn(&mut Connection) -> i32; 8] = [
    crate::fts3::fts3_init,
    crate::fts5::fts5_init,
    crate::rtree::rtree_init,
    crate::dbpage::dbpage_register,
    crate::dbstat::dbstat_register,
    test_ext_init,
    crate::json::json_table_functions,
    crate::stmt::stmt_vtab_init,
];

/// `openDatabase`: abre uma conexão. `z_filename` (UTF-8, ou URI) e `z_vfs` são `None` quando
/// ausentes; `flags` são os `SQLITE_OPEN_*`. Devolve o código e a conexão; como no C, uma conexão
/// que falhou ao abrir (exceto por falta de memória) é devolvida, em `SQLITE_STATE_SICK`, para
/// que se leia a mensagem com `errmsg`.
pub fn open_database(
    z_filename: Option<&[u8]>,
    flags: u32,
    z_vfs: Option<&[u8]>,
) -> (i32, Option<Box<Connection>>) {
    let rc = initialize();
    if rc != 0 {
        return (rc, None);
    }
    let cfg = global_config();
    let mut flags = flags;

    if (flags & SQLITE_OPEN_PRIVATECACHE as u32) != 0 {
        flags &= !(SQLITE_OPEN_SHAREDCACHE as u32);
    } else if cfg.shared_cache_enabled {
        flags |= SQLITE_OPEN_SHAREDCACHE as u32;
    }

    // Remove os bits nocivos de `flags`. `NOMUTEX` e `FULLMUTEX` não têm efeito (não há mutex);
    // fora eles, só valem `READONLY`, `READWRITE`, `CREATE`, `SHAREDCACHE`, `PRIVATECACHE`,
    // `EXRESCODE` e alguns bits reservados.
    flags &= !((SQLITE_OPEN_DELETEONCLOSE
        | SQLITE_OPEN_EXCLUSIVE
        | SQLITE_OPEN_MAIN_DB
        | SQLITE_OPEN_TEMP_DB
        | SQLITE_OPEN_TRANSIENT_DB
        | SQLITE_OPEN_MAIN_JOURNAL
        | SQLITE_OPEN_TEMP_JOURNAL
        | SQLITE_OPEN_SUBJOURNAL
        | SQLITE_OPEN_SUPER_JOURNAL
        | SQLITE_OPEN_NOMUTEX
        | SQLITE_OPEN_FULLMUTEX
        | SQLITE_OPEN_WAL) as u32);

    // Aloca a estrutura da conexão.
    let mut db = Box::new(Connection::default());
    let mut z_open: Option<Vec<u8>> = None;

    'opendb_out: {
        db.err_mask = if (flags & SQLITE_OPEN_EXRESCODE as u32) != 0 { -1 } else { 0xff };
        db.dbs = vec![DbSlot::default(), DbSlot::default()];
        db.e_open_state = SQLITE_STATE_BUSY;
        db.lookaside.b_disable = 1;
        db.lookaside.sz = 0;

        db.a_limit = HARD_LIMIT;
        db.a_limit[SQLITE_LIMIT_WORKER_THREADS as usize] = SQLITE_DEFAULT_WORKER_THREADS;
        db.auto_commit = 1;
        db.next_autovac = -1;
        db.sz_mmap = cfg.sz_mmap;
        db.next_pagesize = 0;
        db.flags |= SQLITE_SHORT_COL_NAMES
            | SQLITE_ENABLE_TRIGGER
            | SQLITE_ENABLE_VIEW
            | SQLITE_CACHE_SPILL
            | SQLITE_TRUSTED_SCHEMA
            // `SQLITE_DQS` padrão (3): aspas duplas viram literais em DDL e DML.
            | SQLITE_DQS_DML
            | SQLITE_DQS_DDL
            | SQLITE_AUTO_INDEX
            | SQLITE_LOAD_EXTENSION
            | SQLITE_FTS3_TOKENIZER;

        // Acrescenta a colação padrão BINARY. Ela serve para UTF-8 e UTF-16, então há uma versão
        // para cada, evitando conversões. O único erro possível aqui é falta de memória.
        //
        // EVIDENCE-OF: R-52786-44878 O SQLite define três funções de colação embutidas.
        let binary = STR_BINARY.as_bytes();
        create_collation(&mut db, binary, SQLITE_UTF8 as u8, Some(CollFn::Binary));
        create_collation(&mut db, binary, SQLITE_UTF16BE as u8, Some(CollFn::Binary));
        create_collation(&mut db, binary, SQLITE_UTF16LE as u8, Some(CollFn::Binary));
        create_collation(&mut db, b"NOCASE", SQLITE_UTF8 as u8, Some(CollFn::NoCase));
        create_collation(&mut db, b"RTRIM", SQLITE_UTF8 as u8, Some(CollFn::RTrim));
        if db.malloc_failed != 0 {
            break 'opendb_out;
        }

        // Interpreta o nome do arquivo ou a URI. Só combinações sensatas de bits em `flags`:
        //
        //  1: SQLITE_OPEN_READONLY
        //  2: SQLITE_OPEN_READWRITE
        //  6: SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE
        db.open_flags = flags;
        debug_assert!(SQLITE_OPEN_READONLY == 0x01);
        debug_assert!(SQLITE_OPEN_READWRITE == 0x02);
        debug_assert!(SQLITE_OPEN_CREATE == 0x04);
        let mut z_err_msg: Option<Vec<u8>> = None;
        let rc = if ((1u32 << (flags & 7)) & 0x46) == 0 {
            misuse_error(183237) // IMP: R-18321-05872
        } else {
            let mut vfs: Option<VfsRef> = None;
            let rc = parse_uri(z_vfs, z_filename.unwrap_or(&[]), &mut flags, &mut vfs, &mut z_open, &mut z_err_msg);
            db.p_vfs = vfs;
            rc
        };
        if rc != SQLITE_OK {
            if rc == SQLITE_NOMEM {
                oom_fault(&mut db);
            }
            match z_err_msg {
                Some(msg) => error_with_msg(&mut db, rc, b"%s", &[PrintfArg::Text(Some(msg))]),
                None => error(&mut db, rc),
            }
            break 'opendb_out;
        }
        debug_assert!(db.p_vfs.is_some());

        let name = z_open.clone().unwrap_or_default();
        open_main_database(&mut db, &name, flags, &cfg);
    }

    let rc = errcode(&db);
    if (rc & 0xff) == SQLITE_NOMEM {
        close(&mut db, false);
        return (rc, None);
    } else if rc != SQLITE_OK {
        db.e_open_state = SQLITE_STATE_SICK;
    }
    (rc, Some(db))
}

/// `sqlite3_open`: abre para leitura e escrita, criando o arquivo se preciso.
pub fn open(z_filename: Option<&[u8]>) -> (i32, Option<Box<Connection>>) {
    open_database(z_filename, (SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE) as u32, None)
}

/// `sqlite3_open_v2`.
pub fn open_v2(
    z_filename: Option<&[u8]>,
    flags: i32,
    z_vfs: Option<&[u8]>,
) -> (i32, Option<Box<Connection>>) {
    open_database(z_filename, flags as u32, z_vfs)
}

/// `sqlite3_open16`: o nome em UTF-16 nativo (terminado por um par de zeros).
pub fn open16(z_filename: &[u8]) -> (i32, Option<Box<Connection>>) {
    let rc = initialize();
    if rc != 0 {
        return (rc, None);
    }
    let z_filename8 = utf16_to_8(z_filename);
    let (rc, mut db) = open_database(
        Some(&z_filename8),
        (SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE) as u32,
        None,
    );
    debug_assert!(db.is_some() || rc == SQLITE_NOMEM);
    if rc == SQLITE_OK {
        if let Some(d) = db.as_mut() {
            if !d.db_has_property(0, DB_SCHEMALOADED) {
                d.dbs[0].schema.enc = SQLITE_UTF16NATIVE as u8;
                d.enc = SQLITE_UTF16NATIVE as u8;
            }
        }
    }
    (rc & 0xff, db)
}

/// `sqlite3_get_clientdata`: o dado do cliente guardado sob `z_name`.
pub fn get_clientdata(db: &Connection, z_name: &[u8]) -> Option<Rc<dyn Any>> {
    db.p_db_data.iter().find(|p| p.z_name == z_name).and_then(|p| p.p_data.clone())
}

/// `sqlite3_set_clientdata`: guarda `p_data` sob `z_name` (nulo apaga). O destrutor anterior da
/// mesma chave roda antes; `x_destructor` roda quando o dado é substituído, apagado ou a conexão
/// fecha.
pub fn set_clientdata(
    db: &mut Connection,
    z_name: &[u8],
    p_data: Option<Rc<dyn Any>>,
    x_destructor: Option<DestroyFn>,
) -> i32 {
    let pos = db.p_db_data.iter().position(|p| p.z_name == z_name);
    match pos {
        Some(i) => {
            // O destrutor antigo roda ao soltar a entrada.
            let old = db.p_db_data.remove(i);
            drop(old);
            if p_data.is_none() {
                return SQLITE_OK;
            }
            db.p_db_data.insert(
                i,
                DbClientData { z_name: z_name.to_vec(), p_data, x_destructor },
            );
        }
        None => {
            if p_data.is_none() {
                return SQLITE_OK;
            }
            db.p_db_data.insert(
                0,
                DbClientData { z_name: z_name.to_vec(), p_data, x_destructor },
            );
        }
    }
    SQLITE_OK
}

/// `sqlite3_get_autocommit`: verdadeiro se a conexão está em modo auto-commit (o padrão; o
/// BEGIN o desliga e o COMMIT ou ROLLBACK o religa).
pub fn get_autocommit(db: &Connection) -> bool {
    db.auto_commit != 0
}

/// O que `sqlite3_table_column_metadata` devolve pelos ponteiros de saída.
#[derive(Debug, Clone, Default)]
pub struct ColumnMetadata {
    /// O tipo declarado.
    pub data_type: Option<Vec<u8>>,
    /// O nome da colação.
    pub coll_seq: Option<Vec<u8>>,
    /// Existe a restrição NOT NULL.
    pub not_null: bool,
    /// A coluna é parte da chave primária.
    pub primary_key: bool,
    /// A coluna é AUTOINCREMENT.
    pub autoinc: bool,
}

/// `sqlite3ColumnType` (util.c): o tipo declarado de uma coluna, ou `z_dflt` se não tem.
fn column_decl_type(p_col: &Column, z_dflt: Option<&[u8]>) -> Option<Vec<u8>> {
    if (p_col.col_flags & COLFLAG_HASTYPE) != 0 {
        let n = strlen30(&p_col.z_cn_name) as usize + 1;
        Some(cstr(p_col.z_cn_name.get(n..).unwrap_or(&[])).to_vec())
    } else if p_col.e_c_type != 0 {
        debug_assert!(p_col.e_c_type as usize <= STD_TYPE.len());
        Some(STD_TYPE[p_col.e_c_type as usize - 1].as_bytes().to_vec())
    } else {
        z_dflt.map(|z| z.to_vec())
    }
}

/// `sqlite3_table_column_metadata`: informação sobre uma coluna de tabela. Em erro, as saídas
/// vêm zeradas (o valor padrão de `ColumnMetadata`). `z_column_name` nulo só consulta se a
/// tabela existe.
pub fn table_column_metadata(
    db: &mut Connection,
    z_db_name: Option<&[u8]>,
    z_table_name: &[u8],
    z_column_name: Option<&[u8]>,
) -> (i32, ColumnMetadata) {
    let mut z_err_msg: Option<Vec<u8>> = None;
    let mut p_tab: Option<Rc<Table>> = None;
    let mut p_col: Option<usize> = None;
    let mut i_col: usize = 0;
    let mut meta = ColumnMetadata::default();

    // Garante que o esquema do banco foi carregado.
    let mut rc = crate::prepare::init(db, &mut z_err_msg);
    'error_out: {
        if SQLITE_OK != rc {
            break 'error_out;
        }

        // Localiza a tabela.
        let tab = match find_table(db, z_table_name, z_db_name) {
            Some(t) if !t.is_view() => t,
            _ => break 'error_out,
        };
        p_tab = Some(tab.clone());

        // Localiza a coluna pedida.
        if let Some(z_col) = z_column_name {
            let n_col = tab.n_col.max(0) as usize;
            i_col = 0;
            while i_col < n_col {
                p_col = Some(i_col);
                let col = &tab.a_col[i_col];
                let name = &col.z_cn_name[..strlen30(&col.z_cn_name) as usize];
                if 0 == str_icmp(name, z_col) {
                    break;
                }
                i_col += 1;
            }
            if i_col == n_col {
                if tab.has_rowid() && is_rowid(z_col) {
                    let ipk = tab.i_p_key as i32;
                    p_col = if ipk >= 0 { Some(ipk as usize) } else { None };
                    i_col = if ipk >= 0 { ipk as usize } else { 0 };
                } else {
                    p_tab = None;
                    break 'error_out;
                }
            }
        }

        // Guarda a informação a devolver em variáveis locais. Há duas possibilidades: (1) o nome
        // da coluna era "rowid", "oid" ou "_rowid_" sem uma coluna IPK declarada; (2) a tabela
        // não é uma view e o nome identificou uma coluna declarada: copia a informação dela.
        if let Some(ic) = p_col {
            let col = &tab.a_col[ic];
            meta.data_type = column_decl_type(col, None);
            meta.coll_seq = column_coll(col).map(|z| z.to_vec());
            meta.not_null = col.not_null != 0;
            meta.primary_key = (col.col_flags & COLFLAG_PRIMKEY) != 0;
            meta.autoinc = tab.i_p_key as i32 == ic as i32 && (tab.tab_flags & TF_AUTOINCREMENT) != 0;
        } else {
            meta.data_type = Some(b"INTEGER".to_vec());
            meta.primary_key = true;
        }
        if meta.coll_seq.is_none() {
            meta.coll_seq = Some(STR_BINARY.as_bytes().to_vec());
        }
    }

    // Tendo dado certo ou não, as saídas recebem o que as variáveis locais têm: em erro, tudo
    // zerado.
    if SQLITE_OK == rc && p_tab.is_none() {
        meta = ColumnMetadata::default();
        z_err_msg = mprintf(
            b"no such table column: %s.%s",
            &[
                PrintfArg::Text(Some(z_table_name.to_vec())),
                PrintfArg::Text(z_column_name.map(|z| z.to_vec())),
            ],
        );
        rc = SQLITE_ERROR;
    }
    match z_err_msg {
        Some(msg) => error_with_msg(db, rc, b"%s", &[PrintfArg::Text(Some(msg))]),
        None => error(db, rc),
    }
    rc = api_exit(db, rc);
    (rc, meta)
}

// ---------------------------------------------------------------------------------------------
// chunk 010: sleep, códigos estendidos, file_control, controles de teste
// ---------------------------------------------------------------------------------------------

/// `sqlite3_sleep`: dorme `ms` milissegundos no VFS padrão e devolve quanto dormiu.
pub fn sleep(ms: i32) -> i32 {
    let Some(vfs) = vfs_find(None) else {
        return 0;
    };
    // O `xSleep` trabalha em microssegundos.
    vfs.sleep(if ms < 0 { 0 } else { 1000 * ms }) / 1000
}

/// `sqlite3_extended_result_codes`: liga ou desliga os códigos de resultado estendidos.
pub fn extended_result_codes(db: &mut Connection, onoff: bool) -> i32 {
    db.err_mask = if onoff { -1 } else { 0xff };
    SQLITE_OK
}

/// `sqlite3DbNameToBtree`: o índice em `db.dbs` do banco com árvore-b chamado `z_db_name`
/// (`None` é o principal), ou `None` se não existe. O `Btree` mora em `db.dbs[i].bt`.
pub fn db_name_to_btree(db: &Connection, z_db_name: Option<&[u8]>) -> Option<usize> {
    let i_db = match z_db_name {
        Some(z) => find_db_name(db, Some(z)),
        None => 0,
    };
    if i_db < 0 {
        return None;
    }
    let i_db = i_db as usize;
    db.dbs.get(i_db).and_then(|slot| slot.bt.as_ref()).map(|_| i_db)
}

/// `sqlite3_file_control`: chama o `xFileControl` do arquivo de um banco. `FILE_POINTER`,
/// `VFS_POINTER` e `JOURNAL_POINTER` devolvem `SQLITE_OK` sem escrever em `arg` (o
/// `FileControlArg` não tem variante de ponteiro).
pub fn file_control(
    db: &mut Connection,
    z_db_name: Option<&[u8]>,
    op: i32,
    arg: &mut FileControlArg,
) -> i32 {
    let Some(i_db) = db_name_to_btree(db, z_db_name) else {
        return SQLITE_ERROR;
    };
    let n_busy = db.busy_handler.n_busy.clone();
    let Some(bt) = db.dbs[i_db].bt.as_mut() else {
        return SQLITE_ERROR;
    };
    if op == SQLITE_FCNTL_FILE_POINTER
        || op == SQLITE_FCNTL_VFS_POINTER
        || op == SQLITE_FCNTL_JOURNAL_POINTER
    {
        SQLITE_OK
    } else if op == SQLITE_FCNTL_DATA_VERSION {
        *arg = FileControlArg::UInt(bt.bt.pager.data_version());
        SQLITE_OK
    } else if op == SQLITE_FCNTL_RESERVE_BYTES {
        let i_new = match arg {
            FileControlArg::Int(n) => *n,
            _ => 0,
        };
        *arg = FileControlArg::Int(btree_get_requested_reserve(bt));
        if (0..=255).contains(&i_new) {
            btree_set_page_size(bt, 0, i_new, 0);
        }
        SQLITE_OK
    } else if op == SQLITE_FCNTL_RESET_CACHE {
        btree_clear_cache(bt);
        SQLITE_OK
    } else {
        let n_save = n_busy.get();
        let rc = os_file_control(bt.bt.pager.fd.as_deref_mut(), op, arg);
        n_busy.set(n_save);
        rc
    }
}

/// O argumento de `sqlite3_test_control`: um modo por variante, com os argumentos que o C lê do
/// `va_list`. Os modos que só têm efeito em compilação de depuração, ou que mexem em estado que
/// pertence a outro módulo (`PRNG_SAVE`, `PRNG_RESTORE`, `PENDING_BYTE`, `LOCALTIME_FAULT`,
/// `NEVER_CORRUPT`, `EXTRA_SCHEMA_CHECKS`, `ONCE_RESET_THRESHOLD`, `TRACEFLAGS`) não fazem nada
/// aqui.
pub enum TestControl<'a, 'b> {
    /// `SQLITE_TESTCTRL_PRNG_SAVE`.
    PrngSave,
    /// `SQLITE_TESTCTRL_PRNG_RESTORE`.
    PrngRestore,
    /// `SQLITE_TESTCTRL_PRNG_SEED`: a semente e, opcionalmente, a conexão cujo cookie de esquema
    /// vira a semente.
    PrngSeed(i32, Option<&'a Connection>),
    /// `SQLITE_TESTCTRL_FK_NO_ACTION`.
    FkNoAction(&'a mut Connection, bool),
    /// `SQLITE_TESTCTRL_BITVEC_TEST`: o tamanho e o programa.
    BitvecTest(i32, &'a mut [i32]),
    /// `SQLITE_TESTCTRL_FAULT_INSTALL`.
    FaultInstall,
    /// `SQLITE_TESTCTRL_BENIGN_MALLOC_HOOKS`.
    BenignMallocHooks,
    /// `SQLITE_TESTCTRL_PENDING_BYTE`.
    PendingByte(u32),
    /// `SQLITE_TESTCTRL_ASSERT`.
    Assert(i32),
    /// `SQLITE_TESTCTRL_ALWAYS`.
    Always(i32),
    /// `SQLITE_TESTCTRL_BYTEORDER`.
    ByteOrder,
    /// `SQLITE_TESTCTRL_OPTIMIZATIONS`: a máscara de otimizações desligadas.
    Optimizations(&'a mut Connection, u32),
    /// `SQLITE_TESTCTRL_LOCALTIME_FAULT`.
    LocaltimeFault(i32),
    /// `SQLITE_TESTCTRL_INTERNAL_FUNCTIONS`.
    InternalFunctions(&'a mut Connection),
    /// `SQLITE_TESTCTRL_NEVER_CORRUPT`.
    NeverCorrupt(i32),
    /// `SQLITE_TESTCTRL_EXTRA_SCHEMA_CHECKS`.
    ExtraSchemaChecks(i32),
    /// `SQLITE_TESTCTRL_ONCE_RESET_THRESHOLD`.
    OnceResetThreshold(i32),
    /// `SQLITE_TESTCTRL_VDBE_COVERAGE`.
    VdbeCoverage,
    /// `SQLITE_TESTCTRL_SORTER_MMAP`.
    SorterMmap(&'a mut Connection, i32),
    /// `SQLITE_TESTCTRL_ISINIT`.
    IsInit,
    /// `SQLITE_TESTCTRL_IMPOSTER`: a conexão, o banco, ligar/desligar e a página raiz.
    Imposter(&'a mut Connection, &'a [u8], i32, i32),
    /// `SQLITE_TESTCTRL_RESULT_INTREAL`.
    ResultIntReal(&'a mut Context<'b>),
    /// `SQLITE_TESTCTRL_SEEK_COUNT`: sem depuração o contador é sempre zero.
    SeekCount(&'a mut u64),
    /// `SQLITE_TESTCTRL_TRACEFLAGS`: a operação e o valor.
    TraceFlags(i32, &'a mut u32),
    /// `SQLITE_TESTCTRL_LOGEST`: a entrada e as três saídas.
    LogEst(f64, &'a mut i32, &'a mut u64, &'a mut i32),
    /// `SQLITE_TESTCTRL_USELONGDOUBLE`.
    UseLongDouble(i32),
    /// `SQLITE_TESTCTRL_JSON_SELFCHECK`: só tem efeito em depuração.
    JsonSelfcheck(&'a mut i32),
}

/// `sqlite3Config.bUseLongDouble` para o controle `USELONGDOUBLE` (só informativo: a aritmética
/// usa sempre o `long double`, ver `mem.rs`).
static USE_LONG_DOUBLE: AtomicBool = AtomicBool::new(true);

/// `sqlite3_test_control`: a interface para a lógica de teste. Devolve o valor do modo.
pub fn test_control(op: TestControl<'_, '_>) -> i32 {
    let mut rc = 0;
    match op {
        TestControl::PrngSave | TestControl::PrngRestore => {}
        TestControl::PrngSeed(x, db) => {
            let mut x = x;
            if let Some(db) = db {
                let y = db.dbs[0].schema.schema_cookie;
                if y != 0 {
                    x = y;
                }
            }
            crate::global::set_prng_seed(x as u32);
            crate::global::randomness_reset();
        }
        TestControl::FkNoAction(db, b) => {
            if b {
                db.flags |= SQLITE_FK_NO_ACTION;
            } else {
                db.flags &= !SQLITE_FK_NO_ACTION;
            }
            sync_btree_flags(db);
        }
        TestControl::BitvecTest(sz, prog) => {
            rc = bitvec_builtin_test(sz, prog, &mut crate::global::randomness);
        }
        TestControl::FaultInstall => {
            // `sqlite3FaultSim(0)` sem simulador instalado.
        }
        TestControl::BenignMallocHooks => {}
        TestControl::PendingByte(_) => {
            rc = PENDING_BYTE as i32;
        }
        TestControl::Assert(_) => {
            // Sem `assert()` (NDEBUG): `x` fica zero.
            rc = 0;
        }
        TestControl::Always(x) => {
            rc = if x != 0 { x } else { 0 };
        }
        TestControl::ByteOrder => {
            // SQLITE_BYTEORDER*100 + SQLITE_LITTLEENDIAN*10 + SQLITE_BIGENDIAN
            rc = 1234 * 100 + 10;
        }
        TestControl::Optimizations(db, n) => {
            db.db_opt_flags = n;
        }
        TestControl::LocaltimeFault(_) => {}
        TestControl::InternalFunctions(db) => {
            db.m_db_flags ^= DBFLAG_INTERNAL_FUNC;
        }
        TestControl::NeverCorrupt(_)
        | TestControl::ExtraSchemaChecks(_)
        | TestControl::OnceResetThreshold(_)
        | TestControl::VdbeCoverage => {}
        TestControl::SorterMmap(db, n) => {
            db.n_max_sorter_mmap = n;
        }
        TestControl::IsInit => {
            if !lock_config().is_init {
                rc = SQLITE_ERROR;
            }
        }
        TestControl::Imposter(db, z_db, on_off, tnum) => {
            let i_db = find_db_name(db, Some(z_db));
            if i_db >= 0 {
                db.init.i_db = i_db as u8;
                db.init.busy = on_off as u8;
                db.init.imposter_table = (on_off & 1) != 0;
                db.init.new_tnum = tnum as u32;
                if db.init.busy == 0 && db.init.new_tnum > 0 {
                    reset_all_schemas_of_connection(db);
                }
            }
        }
        TestControl::ResultIntReal(ctx) => {
            result_int_real(ctx);
        }
        TestControl::SeekCount(pn) => {
            *pn = 0;
        }
        TestControl::TraceFlags(op_trace, ptr) => {
            if op_trace == 0 || op_trace == 2 {
                *ptr = 0;
            }
        }
        TestControl::LogEst(r_in, p_i1, p_u64, p_i2) => {
            let r_log_est = log_est_from_double(r_in);
            *p_i1 = r_log_est as i32;
            *p_u64 = log_est_to_int(r_log_est);
            *p_i2 = log_est(*p_u64) as i32;
        }
        TestControl::UseLongDouble(b) => {
            let mut b = b;
            if b >= 2 {
                b = has_high_precision_double(b) as i32;
            }
            if b >= 0 {
                USE_LONG_DOUBLE.store(b > 0, Ordering::SeqCst);
            }
            rc = USE_LONG_DOUBLE.load(Ordering::SeqCst) as i32;
        }
        TestControl::JsonSelfcheck(_) => {}
    }
    rc
}

// ---------------------------------------------------------------------------------------------
// chunk 011: nomes de arquivo, parâmetros de URI e nomes dos bancos
// ---------------------------------------------------------------------------------------------

/// `sqlite3_create_filename`: monta um `sqlite3_filename` com o nome do banco, os pares de
/// parâmetros de URI, o nome do journal e o do WAL. O bloco devolvido tem o mesmo formato do que
/// o pager entrega aos VFS (começa no nome do banco): `banco NUL (nome NUL valor NUL)* NUL
/// journal NUL wal NUL NUL NUL`.
pub fn create_filename(
    z_database: &[u8],
    z_journal: &[u8],
    z_wal: &[u8],
    az_param: &[(&[u8], &[u8])],
) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut append_text = |z: &[u8]| {
        out.extend_from_slice(cstr(z));
        out.push(0);
    };
    append_text(z_database);
    for (name, value) in az_param {
        append_text(name);
        append_text(value);
    }
    out.push(0);
    out.extend_from_slice(cstr(z_journal));
    out.push(0);
    out.extend_from_slice(cstr(z_wal));
    out.push(0);
    out.push(0);
    out.push(0);
    out
}

/// `sqlite3_uri_parameter`: o valor do parâmetro `z_param` da URI de um `sqlite3_filename`
/// (fatia que começa no nome do banco), ou `None`.
pub fn uri_parameter<'a>(z_filename: &'a [u8], z_param: &[u8]) -> Option<&'a [u8]> {
    let mut pos = strlen30(z_filename) as usize + 1;
    while at(z_filename, pos) != 0 {
        let name = cstr(z_filename.get(pos..).unwrap_or(&[]));
        let x = name == z_param;
        pos += name.len() + 1;
        let value = cstr(z_filename.get(pos..).unwrap_or(&[]));
        if x {
            return Some(value);
        }
        pos += value.len() + 1;
    }
    None
}

/// `sqlite3_uri_key`: o nome do N-ésimo parâmetro da URI.
pub fn uri_key(z_filename: &[u8], n: i32) -> Option<&[u8]> {
    if n < 0 {
        return None;
    }
    let mut n = n;
    let mut pos = strlen30(z_filename) as usize + 1;
    while at(z_filename, pos) != 0 {
        let more = n > 0;
        n -= 1;
        if !more {
            break;
        }
        pos += strlen30(&z_filename[pos..]) as usize + 1;
        pos += strlen30(z_filename.get(pos..).unwrap_or(&[])) as usize + 1;
    }
    if at(z_filename, pos) != 0 {
        Some(cstr(&z_filename[pos..]))
    } else {
        None
    }
}

/// `sqlite3_uri_boolean`: o valor booleano de um parâmetro da URI.
pub fn uri_boolean(z_filename: &[u8], z_param: &[u8], b_dflt: bool) -> bool {
    match uri_parameter(z_filename, z_param) {
        Some(z) => crate::pragma::get_boolean(z, b_dflt),
        None => b_dflt,
    }
}

/// `sqlite3_uri_int64`: o valor inteiro de 64 bits de um parâmetro da URI.
pub fn uri_int64(z_filename: &[u8], z_param: &[u8], b_dflt: i64) -> i64 {
    let mut v = 0i64;
    match uri_parameter(z_filename, z_param) {
        Some(z) if dec_or_hex_to_i64(z, &mut v) == 0 => v,
        _ => b_dflt,
    }
}

/// `sqlite3_filename_database`: o nome do arquivo de banco de um `sqlite3_filename`.
pub fn filename_database(z_filename: &[u8]) -> &[u8] {
    cstr(z_filename)
}

/// `sqlite3_filename_journal`: o nome do arquivo de journal (a fatia começa nele).
pub fn filename_journal(z_filename: &[u8]) -> &[u8] {
    let mut pos = strlen30(z_filename) as usize + 1;
    while at(z_filename, pos) != 0 {
        pos += strlen30(&z_filename[pos..]) as usize + 1;
        pos += strlen30(z_filename.get(pos..).unwrap_or(&[])) as usize + 1;
    }
    z_filename.get(pos + 1..).unwrap_or(&[])
}

/// `sqlite3_filename_wal`: o nome do arquivo do WAL (a fatia começa nele).
pub fn filename_wal(z_filename: &[u8]) -> &[u8] {
    let journal = filename_journal(z_filename);
    journal.get(strlen30(journal) as usize + 1..).unwrap_or(&[])
}

/// `sqlite3_db_name`: o nome do N-ésimo esquema, ou `None` se `n` está fora da faixa.
pub fn db_name(db: &Connection, n: i32) -> Option<&[u8]> {
    if n < 0 || n as usize >= db.dbs.len() {
        None
    } else {
        Some(&db.dbs[n as usize].z_db_s_name)
    }
}

/// `sqlite3_db_filename`: o nome do arquivo do banco `z_db_name` (vazio para um banco em
/// memória; `None` se o banco não existe).
pub fn db_filename(db: &Connection, z_db_name: Option<&[u8]>) -> Option<Vec<u8>> {
    let i_db = db_name_to_btree(db, z_db_name)?;
    db.dbs[i_db].bt.as_ref().map(|bt| cstr(bt.bt.pager.filename(true)).to_vec())
}

/// `sqlite3_db_readonly`: 1 se o banco é somente leitura, 0 se lê e escreve, -1 se não existe.
pub fn db_readonly(db: &Connection, z_db_name: Option<&[u8]>) -> i32 {
    match db_name_to_btree(db, z_db_name).and_then(|i| db.dbs[i].bt.as_ref()) {
        Some(bt) => ((bt.bt.bts_flags & BTS_READ_ONLY) != 0) as i32,
        None => -1,
    }
}

// ---------------------------------------------------------------------------------------------
// chunk 012: opções de compilação
// ---------------------------------------------------------------------------------------------

/// `sqlite3_compileoption_used`: a opção de compilação foi usada? O nome pode começar com
/// `SQLITE_`, que não é obrigatório.
pub fn compileoption_used(z_opt_name: &[u8]) -> bool {
    let az_compile_opt = crate::ctime::compile_options();
    let mut z = z_opt_name;
    if strnicmp(Some(z), Some(&b"SQLITE_"[..]), 7) == 0 {
        z = &z[7..];
    }
    let n = strlen30(z);

    // `nOpt` costuma ter um dígito: a busca linear basta.
    az_compile_opt.iter().any(|opt| {
        strnicmp(Some(z), Some(opt.as_bytes()), n) == 0
            && !is_id_char(at(opt.as_bytes(), n as usize))
    })
}

/// `sqlite3_compileoption_get`: a N-ésima opção de compilação, ou `None` fora da faixa.
pub fn compileoption_get(n: i32) -> Option<&'static str> {
    let az_compile_opt = crate::ctime::compile_options();
    if n >= 0 && (n as usize) < az_compile_opt.len() {
        Some(az_compile_opt[n as usize])
    } else {
        None
    }
}
