// Mesclado das partes traduzidas de loadext_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

// Interface de carregamento de extensões do SQLite (loadext.c).
//
// Com SQLITE_CORE definido e as opções de compilação do Debian 13, nenhuma das macros que
// trocam uma API por 0 (OMIT_UTF16, OMIT_COMPLETE, OMIT_DECLTYPE, OMIT_TRACE, OMIT_INCRBLOB,
// OMIT_VIRTUALTABLE, ...) está ativa, e SQLITE_ENABLE_COLUMN_METADATA está ligada. Logo, a única
// entrada nula da tabela é a posição de `sqlite3_global_recover`, que é fixa no C, e a de
// `sqlite3_normalized_sql`, porque SQLITE_ENABLE_NORMALIZE não faz parte da lista do Debian.

/// Ponto de entrada de uma extensão (`sqlite3_loadext_entry`): recebe a conexão, o lugar onde
/// deixar uma mensagem de erro e a tabela de rotinas da API. Devolve um código de resultado.
pub type Sqlite3LoadextEntry = fn(&Sqlite3Ref, &mut Option<Vec<u8>>, &Sqlite3ApiRoutines) -> i32;

/// Tabela de rotinas da API entregue às extensões (`sqlite3_api_routines`).
///
/// Em Rust seguro um ponteiro de função cru não existe, então cada posição da tabela é
/// representada pelo nome da rotina (sem o prefixo `sqlite3_`), na ordem exata do C. A ordem
/// precisa ser preservada para compatibilidade com versões anteriores: novas APIs entram sempre
/// no fim. Posição sem rotina é a string vazia.
pub struct Sqlite3ApiRoutines {
    /// Nomes das rotinas, na ordem das posições da estrutura do C.
    pub slots: &'static [&'static str],
}

/// Nomes das rotinas da API, na ordem do inicializador de `sqlite3Apis`.
pub const SQLITE3_APIS_SLOTS: &[&str] = &[
    "aggregate_context",
    "aggregate_count",
    "bind_blob",
    "bind_double",
    "bind_int",
    "bind_int64",
    "bind_null",
    "bind_parameter_count",
    "bind_parameter_index",
    "bind_parameter_name",
    "bind_text",
    "bind_text16",
    "bind_value",
    "busy_handler",
    "busy_timeout",
    "changes",
    "close",
    "collation_needed",
    "collation_needed16",
    "column_blob",
    "column_bytes",
    "column_bytes16",
    "column_count",
    "column_database_name",
    "column_database_name16",
    "column_decltype",
    "column_decltype16",
    "column_double",
    "column_int",
    "column_int64",
    "column_name",
    "column_name16",
    "column_origin_name",
    "column_origin_name16",
    "column_table_name",
    "column_table_name16",
    "column_text",
    "column_text16",
    "column_type",
    "column_value",
    "commit_hook",
    "complete",
    "complete16",
    "create_collation",
    "create_collation16",
    "create_function",
    "create_function16",
    "create_module",
    "data_count",
    "db_handle",
    "declare_vtab",
    "enable_shared_cache",
    "errcode",
    "errmsg",
    "errmsg16",
    "exec",
    "expired",
    "finalize",
    "free",
    "free_table",
    "get_autocommit",
    "get_auxdata",
    "get_table",
    "", // Era sqlite3_global_recover(), mas essa função é obsoleta
    "interrupt",
    "last_insert_rowid",
    "libversion",
    "libversion_number",
    "malloc",
    "mprintf",
    "open",
    "open16",
    "prepare",
    "prepare16",
    "profile",
    "progress_handler",
    "realloc",
    "reset",
    "result_blob",
    "result_double",
    "result_error",
    "result_error16",
    "result_int",
    "result_int64",
    "result_null",
    "result_text",
    "result_text16",
    "result_text16be",
    "result_text16le",
    "result_value",
    "rollback_hook",
    "set_authorizer",
    "set_auxdata",
    "snprintf",
    "step",
    "table_column_metadata",
    "thread_cleanup",
    "total_changes",
    "trace",
    "transfer_bindings",
    "update_hook",
    "user_data",
    "value_blob",
    "value_bytes",
    "value_bytes16",
    "value_double",
    "value_int",
    "value_int64",
    "value_numeric_type",
    "value_text",
    "value_text16",
    "value_text16be",
    "value_text16le",
    "value_type",
    "vmprintf",
    // O conjunto original da API termina aqui.
    "overload_function",
    // Adicionadas depois da 3.3.13
    "prepare_v2",
    "prepare16_v2",
    "clear_bindings",
    // Adicionada na 3.4.1
    "create_module_v2",
    // Adicionadas na 3.5.0
    "bind_zeroblob",
    "blob_bytes",
    "blob_close",
    "blob_open",
    "blob_read",
    "blob_write",
    "create_collation_v2",
    "file_control",
    "memory_highwater",
    "memory_used",
    "mutex_alloc",
    "mutex_enter",
    "mutex_free",
    "mutex_leave",
    "mutex_try",
    "open_v2",
    "release_memory",
    "result_error_nomem",
    "result_error_toobig",
    "sleep",
    "soft_heap_limit",
    "vfs_find",
    "vfs_register",
    "vfs_unregister",
    // Adicionadas na 3.5.8
    "threadsafe",
    "result_zeroblob",
    "result_error_code",
    "test_control",
    "randomness",
    "context_db_handle",
    // Adicionadas na 3.6.0
    "extended_result_codes",
    "limit",
    "next_stmt",
    "sql",
    "status",
    // Adicionadas na 3.7.4
    "backup_finish",
    "backup_init",
    "backup_pagecount",
    "backup_remaining",
    "backup_step",
    "compileoption_get",
    "compileoption_used",
    "create_function_v2",
    "db_config",
    "db_mutex",
    "db_status",
    "extended_errcode",
    "log",
    "soft_heap_limit64",
    "sourceid",
    "stmt_status",
    "strnicmp",
    "unlock_notify",
    "wal_autocheckpoint",
    "wal_checkpoint",
    "wal_hook",
    "blob_reopen",
    "vtab_config",
    "vtab_on_conflict",
    "close_v2",
    "db_filename",
    "db_readonly",
    "db_release_memory",
    "errstr",
    "stmt_busy",
    "stmt_readonly",
    "stricmp",
    "uri_boolean",
    "uri_int64",
    "uri_parameter",
    "vsnprintf",
    "wal_checkpoint_v2",
    // Versão 3.8.7 e posteriores
    "auto_extension",
    "bind_blob64",
    "bind_text64",
    "cancel_auto_extension",
    "load_extension",
    "malloc64",
    "msize",
    "realloc64",
    "reset_auto_extension",
    "result_blob64",
    "result_text64",
    "strglob",
    // Versão 3.8.11 e posteriores
    "value_dup",
    "value_free",
    "result_zeroblob64",
    "bind_zeroblob64",
    // Versão 3.9.0 e posteriores
    "value_subtype",
    "result_subtype",
    // Versão 3.10.0 e posteriores
    "status64",
    "strlike",
    "db_cacheflush",
    // Versão 3.12.0 e posteriores
    "system_errno",
    // Versão 3.14.0 e posteriores
    "trace_v2",
    "expanded_sql",
    // Versão 3.18.0 e posteriores
    "set_last_insert_rowid",
    // Versão 3.20.0 e posteriores
    "prepare_v3",
    "prepare16_v3",
    "bind_pointer",
    "result_pointer",
    "value_pointer",
    // Versão 3.22.0 e posteriores
    "vtab_nochange",
    "value_nochange",
    "vtab_collation",
    // Versão 3.24.0 e posteriores
    "keyword_count",
    "keyword_name",
    "keyword_check",
    "str_new",
    "str_finish",
    "str_appendf",
    "str_vappendf",
    "str_append",
    "str_appendall",
    "str_appendchar",
    "str_reset",
    "str_errcode",
    "str_length",
    "str_value",
    // Versão 3.25.0 e posteriores
    "create_window_function",
    // Versão 3.26.0 e posteriores (SQLITE_ENABLE_NORMALIZE desligada)
    "",
    // Versão 3.28.0 e posteriores
    "stmt_isexplain",
    "value_frombind",
    // Versão 3.30.0 e posteriores
    "drop_modules",
    // Versão 3.31.0 e posteriores
    "hard_heap_limit64",
    "uri_key",
    "filename_database",
    "filename_journal",
    "filename_wal",
    // Versão 3.32.0 e posteriores
    "create_filename",
    "free_filename",
    "database_file_object",
    // Versão 3.34.0 e posteriores
    "txn_state",
    // Versão 3.36.1 e posteriores
    "changes64",
    "total_changes64",
    // Versão 3.37.0 e posteriores
    "autovacuum_pages",
    // Versão 3.38.0 e posteriores
    "error_offset",
    "vtab_rhs_value",
    "vtab_distinct",
    "vtab_in",
    "vtab_in_first",
    "vtab_in_next",
    // Versão 3.39.0 e posteriores
    "deserialize",
    "serialize",
    "db_name",
    // Versão 3.40.0 e posteriores
    "value_encoding",
    // Versão 3.41.0 e posteriores
    "is_interrupted",
    // Versão 3.43.0 e posteriores
    "stmt_explain",
    // Versão 3.44.0 e posteriores
    "get_clientdata",
    "set_clientdata",
];

/// Estrutura com todas as rotinas da API. Um ponteiro para ela é passado às extensões quando
/// são carregadas, para que possam chamar de volta a biblioteca.
pub static SQLITE3_APIS: Sqlite3ApiRoutines = Sqlite3ApiRoutines {
    slots: SQLITE3_APIS_SLOTS,
};


// ---- part_001.rs ----

/// Verdadeiro se `x` é o caractere separador de diretório.
#[inline]
pub fn dir_sep(x: u8) -> bool {
    x == b'/'
}

/// Corta o texto no primeiro NUL, como o C faz ao tratar um `const char*` como string.
#[inline]
fn c_str_prefix(z: &[u8]) -> &[u8] {
    match z.iter().position(|&b| b == 0) {
        Some(n) => &z[..n],
        None => z,
    }
}

/// Monta a mensagem de erro e deixa o VFS sobrescrevê-la com o texto do dlerror(), se houver
/// (o `sqlite3OsDlError(pVfs, nMsg-1, zErrmsg)` do C, que escreve no mesmo buffer).
fn fill_dl_error_message(vfs: &Rc<dyn Vfs>, n_msg: u64, mut z_errmsg: Vec<u8>) -> Vec<u8> {
    os_dl_error(vfs, (n_msg - 1) as i32, &mut z_errmsg);
    z_errmsg
}

/// Caminho `extension_not_found` da função de carga: "unable to open shared library [...]".
fn extension_not_found(
    vfs: &Rc<dyn Vfs>,
    z_file: &[u8],
    n_msg: u64,
    p_z_err_msg: &mut Option<Vec<u8>>,
    want_msg: bool,
) -> i32 {
    if want_msg {
        let n_msg = n_msg + 300;
        let mut z_errmsg: Vec<u8> = b"unable to open shared library [".to_vec();
        // "%.*s" com SQLITE_MAX_PATHLEN: no máximo essa quantidade de bytes, parando em NUL
        let lim = (SQLITE_MAX_PATHLEN as usize).min(z_file.len());
        z_errmsg.extend_from_slice(&z_file[..lim]);
        z_errmsg.push(b']');
        *p_z_err_msg = Some(fill_dl_error_message(vfs, n_msg, z_errmsg));
    }
    SQLITE_ERROR
}

/// Tenta carregar uma biblioteca de extensão SQLite contida no arquivo `z_file`. O ponto de
/// entrada é `z_proc`; se for `None`, usa o nome padrão `sqlite3_extension_init`. O uso do nome
/// padrão é recomendado.
///
/// Devolve `SQLITE_OK` em caso de sucesso e `SQLITE_ERROR` se algo der errado. Se ocorrer um
/// erro e `want_msg` for verdadeiro, `p_z_err_msg` recebe o texto da mensagem de erro.
fn load_extension(
    db: &Sqlite3Ref,
    z_file: &[u8],
    z_proc: Option<&[u8]>,
    p_z_err_msg: &mut Option<Vec<u8>>,
    want_msg: bool,
) -> i32 {
    let z_file = c_str_prefix(z_file);
    let p_vfs = db.borrow().p_vfs.clone();
    let mut n_msg: u64 = z_file.len() as u64;

    // Extensões de biblioteca compartilhada a tentar se z_file não puder ser carregado como veio
    const AZ_ENDINGS: [&[u8]; 1] = [b"so"];

    if want_msg {
        *p_z_err_msg = None;
    }

    // Ticket #1863. Para não criar problemas de segurança em aplicações antigas que religam contra
    // versões novas do SQLite, o load_extension vem desligado por padrão. É preciso chamar
    // sqlite3_enable_load_extension(db) ou sqlite3_db_config(db,
    // SQLITE_DBCONFIG_ENABLE_LOAD_EXTENSION, 1, 0) para ligar o carregamento de extensões.
    if (db.borrow().flags & SQLITE_LOADEXTENSION) == 0 {
        if want_msg {
            *p_z_err_msg = Some(b"not authorized".to_vec());
        }
        return SQLITE_ERROR;
    }

    let mut z_entry: Vec<u8> = match z_proc {
        Some(p) => c_str_prefix(p).to_vec(),
        None => b"sqlite3_extension_init".to_vec(),
    };

    // tag-20210611-1. Algumas implementações de dlopen() dão segfault com nome de arquivo grande
    // demais. A maioria dos sistemas de arquivos limita o caminho a 4K, então o nome do arquivo
    // da extensão é limitado a cerca do dobro disso (2023-03-25: guarda mais 6 bytes para o sufixo
    // do nome). Ver https://sqlite.org/forum/forumpost/08a0d6d9bf e
    // https://sqlite.org/forum/forumpost/24083b579d.
    if n_msg > SQLITE_MAX_PATHLEN as u64 {
        return extension_not_found(&p_vfs, z_file, n_msg, p_z_err_msg, want_msg);
    }

    // Não deixa sqlite3_load_extension() ligar com uma cópia da aplicação em execução quando o
    // nome de arquivo vem vazio.
    if n_msg == 0 {
        return extension_not_found(&p_vfs, z_file, n_msg, p_z_err_msg, want_msg);
    }

    let mut handle = os_dl_open(&p_vfs, z_file);
    let mut ii = 0;
    while ii < AZ_ENDINGS.len() && handle.is_none() {
        let mut z_alt_file: Vec<u8> = z_file.to_vec();
        z_alt_file.push(b'.');
        z_alt_file.extend_from_slice(AZ_ENDINGS[ii]);
        if n_msg + AZ_ENDINGS[ii].len() as u64 + 1 <= SQLITE_MAX_PATHLEN as u64 {
            handle = os_dl_open(&p_vfs, &z_alt_file);
        }
        ii += 1;
    }
    let handle = match handle {
        Some(h) => h,
        None => return extension_not_found(&p_vfs, z_file, n_msg, p_z_err_msg, want_msg),
    };
    let mut x_init: Option<Sqlite3LoadextEntry> = os_dl_sym(&p_vfs, handle, &z_entry);

    // Se nenhum ponto de entrada foi dado e o nome legado padrão "sqlite3_extension_init" não foi
    // achado, monta o nome "sqlite3_X_init" onde X é cada caractere alfabético ASCII do nome do
    // arquivo depois da última "/" até o primeiro ".", em minúsculas, descartando os três
    // primeiros caracteres se forem "lib". Exemplos:
    //
    //    /usr/local/lib/libExample5.4.3.so ==>  sqlite3_example_init
    //    C:/lib/mathfuncs.dll              ==>  sqlite3_mathfuncs_init
    if x_init.is_none() && z_proc.is_none() {
        let nc_file = z_file.len() as i32;
        let mut z_alt_entry: Vec<u8> = b"sqlite3_".to_vec();
        let mut i_file = nc_file - 1;
        while i_file >= 0 && !dir_sep(z_file[i_file as usize]) {
            i_file -= 1;
        }
        i_file += 1;
        let mut i_file = i_file as usize;
        if z_file.len() - i_file >= 3 && z_file[i_file..i_file + 3].eq_ignore_ascii_case(b"lib") {
            i_file += 3;
        }
        while i_file < z_file.len() && z_file[i_file] != b'.' {
            let c = z_file[i_file];
            if c.is_ascii_alphabetic() {
                z_alt_entry.push(c.to_ascii_lowercase());
            }
            i_file += 1;
        }
        z_alt_entry.extend_from_slice(b"_init");
        z_entry = z_alt_entry;
        x_init = os_dl_sym(&p_vfs, handle, &z_entry);
    }
    let x_init = match x_init {
        Some(f) => f,
        None => {
            if want_msg {
                n_msg += z_entry.len() as u64 + 300;
                let mut z_errmsg: Vec<u8> = b"no entry point [".to_vec();
                z_errmsg.extend_from_slice(&z_entry);
                z_errmsg.extend_from_slice(b"] in shared library [");
                z_errmsg.extend_from_slice(z_file);
                z_errmsg.push(b']');
                *p_z_err_msg = Some(fill_dl_error_message(&p_vfs, n_msg, z_errmsg));
            }
            os_dl_close(&p_vfs, handle);
            return SQLITE_ERROR;
        }
    };
    let mut z_errmsg: Option<Vec<u8>> = None;
    let rc = x_init(db, &mut z_errmsg, &SQLITE3_APIS);
    if rc != 0 {
        if rc == SQLITE_OK_LOAD_PERMANENTLY {
            return SQLITE_OK;
        }
        if want_msg {
            // "%s" com ponteiro nulo sai como texto vazio no printf do SQLite
            let mut msg: Vec<u8> = b"error during initialization: ".to_vec();
            if let Some(m) = &z_errmsg {
                msg.extend_from_slice(c_str_prefix(m));
            }
            *p_z_err_msg = Some(msg);
        }
        os_dl_close(&p_vfs, handle);
        return SQLITE_ERROR;
    }

    // Acrescenta o novo handle da biblioteca compartilhada ao vetor db->aExtension.
    db.borrow_mut().a_extension.push(handle);
    SQLITE_OK
}

/// API pública `sqlite3_load_extension`: pega o mutex da conexão, carrega, aplica `api_exit` e
/// solta o mutex.
pub fn api_load_extension(
    db: &Sqlite3Ref,
    z_file: &[u8],
    z_proc: Option<&[u8]>,
    p_z_err_msg: &mut Option<Vec<u8>>,
    want_msg: bool,
) -> i32 {
    let mutex = db.borrow().mutex.clone();
    mutex_enter(&mutex);
    let mut rc = load_extension(db, z_file, z_proc, p_z_err_msg, want_msg);
    rc = api_exit(db, rc);
    mutex_leave(&mutex);
    rc
}

/// Chamar quando a conexão estiver fechando, para limpar as extensões carregadas.
pub fn close_extensions(db: &Sqlite3Ref) {
    let p_vfs = db.borrow().p_vfs.clone();
    let handles = std::mem::take(&mut db.borrow_mut().a_extension);
    for h in handles {
        os_dl_close(&p_vfs, h);
    }
}

/// Liga ou desliga o carregamento de extensões. Vem desligado por padrão para não abrir buracos
/// de segurança em aplicações antigas.
pub fn api_enable_load_extension(db: &Sqlite3Ref, onoff: i32) -> i32 {
    let mutex = db.borrow().mutex.clone();
    mutex_enter(&mutex);
    if onoff != 0 {
        db.borrow_mut().flags |= SQLITE_LOADEXTENSION | SQLITE_LOADEXTFUNC;
    } else {
        db.borrow_mut().flags &= !(SQLITE_LOADEXTENSION | SQLITE_LOADEXTFUNC);
    }
    mutex_leave(&mutex);
    SQLITE_OK
}

/// Lista das extensões carregadas automaticamente. É compartilhada entre threads; o `Mutex`
/// faz o papel do SQLITE_MUTEX_STATIC_MAIN, que precisa estar preso ao mexer na lista.
pub struct Sqlite3AutoExtList {
    /// Ponteiros para as funções de inicialização das extensões (`nExt` é o comprimento).
    pub a_ext: Vec<Sqlite3LoadextEntry>,
}

pub static SQLITE3_AUTOEXT: std::sync::Mutex<Sqlite3AutoExtList> =
    std::sync::Mutex::new(Sqlite3AutoExtList { a_ext: Vec::new() });

/// Trava a lista de auto-extensões, ignorando envenenamento (o C não tem esse conceito).
pub fn lock_autoext() -> std::sync::MutexGuard<'static, Sqlite3AutoExtList> {
    SQLITE3_AUTOEXT.lock().unwrap_or_else(|e| e.into_inner())
}

/// Registra uma extensão ligada estaticamente que é carregada automaticamente por toda nova
/// conexão de banco de dados.
pub fn api_auto_extension(x_init: Sqlite3LoadextEntry) -> i32 {
    let rc = api::initialize();
    if rc != SQLITE_OK {
        return rc;
    }
    let mut g = lock_autoext();
    if !g.a_ext.iter().any(|&f| f == x_init) {
        g.a_ext.push(x_init);
    }
    rc
}

/// Cancela uma chamada anterior a `sqlite3_auto_extension`. Remove `x_init` do conjunto de
/// rotinas invocadas a cada nova conexão, se estiver na lista; senão não faz nada.
///
/// Devolve 1 se `x_init` estava na lista e foi removida, 0 se não estava.
pub fn api_cancel_auto_extension(x_init: Sqlite3LoadextEntry) -> i32 {
    let mut g = lock_autoext();
    let mut n = 0;
    let mut i = g.a_ext.len() as i32 - 1;
    while i >= 0 {
        if g.a_ext[i as usize] == x_init {
            // aExt[i] = aExt[--nExt]
            g.a_ext.swap_remove(i as usize);
            n += 1;
            break;
        }
        i -= 1;
    }
    n
}

/// Zera o mecanismo de carregamento automático de extensões.
pub fn api_reset_auto_extension() {
    if api::initialize() == SQLITE_OK {
        let mut g = lock_autoext();
        g.a_ext = Vec::new();
    }
}


// ---- part_002.rs ----

/// Carrega todas as extensões automáticas.
///
/// Se algo der errado, define um erro na conexão do banco de dados.
pub fn auto_load_extensions(db: &Sqlite3Ref) {
    let mut i: usize = 0;
    let mut go = true;

    if lock_autoext().a_ext.is_empty() {
        // Caso comum: sai cedo sem nunca pegar o mutex
        return;
    }
    while go {
        let x_init: Option<Sqlite3LoadextEntry> = {
            let g = lock_autoext();
            if i >= g.a_ext.len() {
                go = false;
                None
            } else {
                Some(g.a_ext[i])
            }
        };
        let mut z_errmsg: Option<Vec<u8>> = None;
        if let Some(f) = x_init {
            let rc = f(db, &mut z_errmsg, &SQLITE3_APIS);
            if rc != 0 {
                // "%s" com ponteiro nulo sai como texto vazio no printf do SQLite
                let mut msg: Vec<u8> = b"automatic extension loading failed: ".to_vec();
                if let Some(m) = &z_errmsg {
                    msg.extend_from_slice(m);
                }
                error_with_msg(db, rc, Some(&msg));
                go = false;
            }
        }
        i += 1;
    }
}

