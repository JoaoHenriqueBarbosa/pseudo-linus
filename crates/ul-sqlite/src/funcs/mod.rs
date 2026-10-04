//! Funções SQL e utilidades do CLI que precisam do SQLite.
//!
//! - As funções que o shell.c registra em toda conexão (`shell_add_schema`, `strtod`, `dtostr`...).
//! - As que trocamos pra que nada venha do host: data e hora (porte do date.c com o relógio e o fuso
//!   do sandbox), `random()`/`randomblob()` com o `getrandom` do sandbox, e `load_extension()`, que
//!   no sandbox nunca abre biblioteca nenhuma.
//! - O estado por thread que os callbacks do SQLite compartilham com o CLI: fila de saída do
//!   `shell_putsnl` e do `.progress`, o pedido de WAL visto pelo authorizer, o tempo do comando.

pub mod date;
pub mod ext;

use std::cell::{Cell as StdCell, RefCell};

use rusqlite::functions::FunctionFlags;
use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
use rusqlite::types::{Value, ValueRef};
use rusqlite::{Connection, OpenFlags};
use sysabi::sys;

use crate::cli::Shell;
use crate::cli::text;
use crate::{unwind, vfs};

pub const PROGRESS_QUIET: u32 = 0x01;
pub const PROGRESS_RESET: u32 = 0x02;
pub const PROGRESS_ONCE: u32 = 0x04;

#[derive(Default)]
struct Progress {
    /// `.progress N` (0 = desligado).
    every: i32,
    count: u32,
    max: u32,
    flags: u32,
}

thread_local! {
    /// Conexão em memória só pra formatação (`printf` do SQLite).
    static FMT: RefCell<Option<Connection>> = const { RefCell::new(None) };
    /// Texto escrito por callbacks (shell_putsnl, .progress) esperando a vez na saída.
    static PUTS: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static PROGRESS: RefCell<Progress> = RefCell::new(Progress::default());
    /// SIGINT vistos pelo progress handler e ainda não contados pelo CLI.
    static INTERRUPTS: StdCell<u32> = const { StdCell::new(0) };
    /// `.auth on`.
    static AUTH_TRACE: StdCell<bool> = const { StdCell::new(false) };
    /// Esquemas que um comando vai pôr em WAL (o authorizer viu `PRAGMA journal_mode=WAL`).
    static WAL_REQUEST: RefCell<Vec<Option<String>>> = const { RefCell::new(Vec::new()) };
    /// Arquivos que um ATTACH vai abrir em WAL.
    static ATTACH_WAL: StdCell<bool> = const { StdCell::new(false) };
    /// `.timeout`: milissegundos do busy handler nosso.
    static BUSY_MS: StdCell<i32> = const { StdCell::new(0) };
}

/// Texto do `printf` do SQLite (`fmt` é um formato com um argumento, ex.: `%!.15g`).
pub fn sqlite_printf(fmt: &str, v: Value) -> Vec<u8> {
    FMT.with(|f| {
        let mut f = f.borrow_mut();
        if f.is_none() {
            *f = Connection::open_with_flags(":memory:", OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX).ok();
        }
        let Some(conn) = f.as_ref() else { return Vec::new() };
        let sql = format!("SELECT printf('{}', ?1)", fmt.replace('\'', "''"));
        let r = conn.prepare_cached(&sql).and_then(|mut st| {
            st.query_row([v], |r| {
                Ok(match r.get_ref(0)? {
                    ValueRef::Text(t) | ValueRef::Blob(t) => t.to_vec(),
                    _ => Vec::new(),
                })
            })
        });
        r.unwrap_or_default()
    })
}

/// REAL como o SQLite escreve: `%!.15g` (o `sqlite3_column_text`) ou `%!.20g` (modos quote, insert
/// e json do CLI).
pub fn fmt_real(f: f64, digits: u32) -> Vec<u8> {
    if digits == 15 {
        sqlite_printf("%!.15g", Value::Real(f))
    } else {
        sqlite_printf("%!.20g", Value::Real(f))
    }
}

/// `sqlite3_errstr`.
pub fn errstr(code: i32) -> String {
    rusqlite::ffi::code_to_str(code).to_string()
}

/// `sqlite3_sourceid()` da biblioteca ligada.
pub fn source_id() -> String {
    FMT.with(|_| ());
    let c = Connection::open_with_flags(":memory:", OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX);
    c.ok()
        .and_then(|c| c.query_row("SELECT sqlite_source_id()", [], |r| r.get::<_, String>(0)).ok())
        .unwrap_or_default()
}

/// Nomes de VFS que a URI pode pedir sem sair do sandbox.
const SAFE_VFS: &[&str] = &["unix", "unix-excl", "unix-dotfile", "unix-none", "memdb"];

/// Nome de arquivo em forma de URI com `vfs=` que não é nosso: devolve a mensagem de erro.
pub fn reject_foreign_vfs_uri(name: &[u8]) -> Option<String> {
    let s = String::from_utf8_lossy(name);
    let rest = s.strip_prefix("file:")?;
    let query = rest.split_once('?')?.1;
    let query = query.split('#').next().unwrap_or("");
    for kv in query.split('&') {
        if let Some(v) = kv.strip_prefix("vfs=")
            && !SAFE_VFS.contains(&v)
        {
            return Some(format!("no such vfs: {v}"));
        }
    }
    None
}

/// `sqlite3_deserialize` do banco principal (redimensionável, como o `--deserialize` do CLI).
pub fn deserialize_main(conn: &mut Connection, data: &[u8]) -> Result<(), i32> {
    let r = conn.deserialize_read_exact(rusqlite::MAIN_DB, data, data.len(), false);
    unwind::reraise();
    r.map_err(|e| crate::cli::exec::error_parts(&e).0)
}

/// Fila de saída dos callbacks.
pub fn queue_output(data: &[u8]) {
    PUTS.with(|p| p.borrow_mut().extend_from_slice(data));
}

/// Esvazia a fila de saída dos callbacks na saída corrente do CLI.
pub fn drain_puts(sh: &mut Shell) {
    let data = PUTS.with(|p| std::mem::take(&mut *p.borrow_mut()));
    if !data.is_empty() {
        sh.oput(&data);
    }
}

/// SIGINT que o progress handler viu.
pub fn take_interrupts() -> u32 {
    INTERRUPTS.with(|i| i.replace(0))
}

pub fn set_auth_trace(on: bool) {
    AUTH_TRACE.with(|a| a.set(on));
}

/// `.progress`.
pub fn set_progress(every: i32, max: u32, flags: u32) {
    PROGRESS.with(|p| {
        let mut p = p.borrow_mut();
        p.every = every.max(0);
        p.max = max;
        p.flags = flags;
        p.count = 0;
    });
}

/// Passos da VM entre duas chamadas do progress handler quando o `.progress` está desligado: o
/// bastante pra preempção e sinais chegarem a tempo sem custar caro.
const CHECKPOINT_OPS: i32 = 1000;

fn install_progress(conn: &Connection) {
    let every = PROGRESS.with(|p| p.borrow().every);
    let n = if every > 0 { every } else { CHECKPOINT_OPS };
    let _ = conn.progress_handler(
        n,
        Some(|| {
            let r = unwind::guard(|| {
                let s = sys::current();
                s.checkpoint();
                let mut stop = false;
                for sig in s.take_caught_signals() {
                    if sig == sysabi::Signal::SIGINT {
                        INTERRUPTS.with(|i| i.set(i.get() + 1));
                        stop = true;
                    }
                }
                PROGRESS.with(|p| {
                    let mut p = p.borrow_mut();
                    if p.every <= 0 {
                        return;
                    }
                    p.count += 1;
                    if p.count >= p.max && p.max > 0 {
                        queue_output(format!("Progress limit reached ({})\n", p.count).as_bytes());
                        if p.flags & PROGRESS_RESET != 0 {
                            p.count = 0;
                        }
                        if p.flags & PROGRESS_ONCE != 0 {
                            p.max = 0;
                        }
                        stop = true;
                        return;
                    }
                    if p.flags & PROGRESS_QUIET == 0 {
                        queue_output(format!("Progress {}\n", p.count).as_bytes());
                    }
                });
                stop
            });
            r.unwrap_or(true)
        }),
    );
}

/// Busy handler do `.timeout`: o mesmo calendário de esperas do `sqliteDefaultBusyCallback`, mas
/// dormindo pelo `nanosleep` do sandbox.
fn busy_callback(count: i32) -> bool {
    const DELAYS: [i32; 12] = [1, 2, 5, 10, 15, 20, 25, 25, 25, 50, 50, 100];
    const TOTALS: [i32; 12] = [0, 1, 3, 8, 18, 33, 53, 78, 103, 128, 178, 228];
    let tmout = BUSY_MS.with(|b| b.get());
    let c = count.max(0) as usize;
    let (mut delay, prior) = if c < DELAYS.len() {
        (DELAYS[c], TOTALS[c])
    } else {
        (DELAYS[11], TOTALS[11] + DELAYS[11] * (count - 11))
    };
    if prior + delay > tmout {
        delay = tmout - prior;
        if delay <= 0 {
            return false;
        }
    }
    unwind::guard(|| {
        let _ = sys::current().nanosleep(std::time::Duration::from_millis(delay as u64));
    })
    .is_some()
}

/// `.timeout MS` (`sqlite3_busy_timeout`).
pub fn set_busy_timeout(conn: &Connection, ms: i32) {
    BUSY_MS.with(|b| b.set(ms));
    if ms > 0 {
        let _ = conn.busy_handler(Some(busy_callback));
    } else {
        let _ = conn.busy_handler(None);
    }
}

const AUTH_NAMES: [&str; 34] = [
    "",
    "CREATE_INDEX",
    "CREATE_TABLE",
    "CREATE_TEMP_INDEX",
    "CREATE_TEMP_TABLE",
    "CREATE_TEMP_TRIGGER",
    "CREATE_TEMP_VIEW",
    "CREATE_TRIGGER",
    "CREATE_VIEW",
    "DELETE",
    "DROP_INDEX",
    "DROP_TABLE",
    "DROP_TEMP_INDEX",
    "DROP_TEMP_TABLE",
    "DROP_TEMP_TRIGGER",
    "DROP_TEMP_VIEW",
    "DROP_TRIGGER",
    "DROP_VIEW",
    "INSERT",
    "PRAGMA",
    "READ",
    "SELECT",
    "TRANSACTION",
    "UPDATE",
    "ATTACH",
    "DETACH",
    "ALTER_TABLE",
    "REINDEX",
    "ANALYZE",
    "CREATE_VTABLE",
    "DROP_VTABLE",
    "FUNCTION",
    "SAVEPOINT",
    "RECURSIVE",
];

/// O authorizer de toda conexão: barra URI com VFS de fora, anota quem vai entrar em WAL (pra ligar
/// o modo de trava exclusivo antes, ver STATUS.md) e faz o `.auth on`.
fn authorizer(ctx: AuthContext<'_>) -> Authorization {
    match &ctx.action {
        AuthAction::Attach { filename } => {
            if reject_foreign_vfs_uri(filename.as_bytes()).is_some() {
                return Authorization::Deny;
            }
            let wal = unwind::guard(|| vfs::header_is_wal(filename.as_bytes())).unwrap_or(false);
            if wal {
                ATTACH_WAL.with(|a| a.set(true));
            }
        }
        AuthAction::Pragma { pragma_name, pragma_value } => {
            if pragma_name.eq_ignore_ascii_case("journal_mode")
                && pragma_value.is_some_and(|v| v.eq_ignore_ascii_case("wal"))
            {
                let schema = ctx.database_name.map(str::to_string);
                WAL_REQUEST.with(|w| w.borrow_mut().push(schema));
            }
        }
        _ => {}
    }
    if AUTH_TRACE.with(|a| a.get()) {
        let (code, a1, a2) = auth_parts(&ctx.action);
        let name = AUTH_NAMES.get(code as usize).copied().unwrap_or("");
        let mut line = format!("authorizer: {name}").into_bytes();
        for v in [a1.as_deref(), a2.as_deref(), ctx.database_name, ctx.accessor] {
            line.push(b' ');
            match v {
                Some(s) => line.extend(text::c_string(s.as_bytes())),
                None => line.extend_from_slice(b"NULL"),
            }
        }
        line.push(b'\n');
        queue_output(&line);
    }
    Authorization::Allow
}

fn auth_parts(a: &AuthAction<'_>) -> (i32, Option<String>, Option<String>) {
    use AuthAction as A;
    let s = |x: &str| Some(x.to_string());
    match a {
        A::CreateIndex { index_name, table_name } => (1, s(index_name), s(table_name)),
        A::CreateTable { table_name } => (2, s(table_name), None),
        A::CreateTempIndex { index_name, table_name } => (3, s(index_name), s(table_name)),
        A::CreateTempTable { table_name } => (4, s(table_name), None),
        A::CreateTempTrigger { trigger_name, table_name } => (5, s(trigger_name), s(table_name)),
        A::CreateTempView { view_name } => (6, s(view_name), None),
        A::CreateTrigger { trigger_name, table_name } => (7, s(trigger_name), s(table_name)),
        A::CreateView { view_name } => (8, s(view_name), None),
        A::Delete { table_name } => (9, s(table_name), None),
        A::DropIndex { index_name, table_name } => (10, s(index_name), s(table_name)),
        A::DropTable { table_name } => (11, s(table_name), None),
        A::DropTempIndex { index_name, table_name } => (12, s(index_name), s(table_name)),
        A::DropTempTable { table_name } => (13, s(table_name), None),
        A::DropTempTrigger { trigger_name, table_name } => (14, s(trigger_name), s(table_name)),
        A::DropTempView { view_name } => (15, s(view_name), None),
        A::DropTrigger { trigger_name, table_name } => (16, s(trigger_name), s(table_name)),
        A::DropView { view_name } => (17, s(view_name), None),
        A::Insert { table_name } => (18, s(table_name), None),
        A::Pragma { pragma_name, pragma_value } => (19, s(pragma_name), pragma_value.map(str::to_string)),
        A::Read { table_name, column_name } => (20, s(table_name), s(column_name)),
        A::Select => (21, None, None),
        A::Transaction { operation } => (22, Some(format!("{operation:?}").to_ascii_uppercase()), None),
        A::Update { table_name, column_name } => (23, s(table_name), s(column_name)),
        A::Attach { filename } => (24, s(filename), None),
        A::Detach { database_name } => (25, s(database_name), None),
        A::AlterTable { database_name, table_name } => (26, s(database_name), s(table_name)),
        A::Reindex { index_name } => (27, s(index_name), None),
        A::Analyze { table_name } => (28, s(table_name), None),
        A::CreateVtable { table_name, module_name } => (29, s(table_name), s(module_name)),
        A::DropVtable { table_name, module_name } => (30, s(table_name), s(module_name)),
        A::Function { function_name } => (31, None, s(function_name)),
        A::Savepoint { operation, savepoint_name } => {
            (32, Some(format!("{operation:?}").to_ascii_uppercase()), s(savepoint_name))
        }
        A::Recursive => (33, None, None),
        _ => (0, None, None),
    }
}

/// Antes de preparar um comando: limpa o que o authorizer anota.
pub fn before_prepare() {
    WAL_REQUEST.with(|w| w.borrow_mut().clear());
    ATTACH_WAL.with(|a| a.set(false));
}

/// Depois de preparar e antes do primeiro passo: liga o modo de trava exclusivo onde o comando vai
/// abrir WAL (sem `xShmMap`, o índice do WAL só pode morar no heap, e isso exige o modo exclusivo
/// antes da troca), e zera o relógio do comando.
pub fn before_step(conn: &Connection) {
    date::reset_statement_time();
    let reqs = WAL_REQUEST.with(|w| std::mem::take(&mut *w.borrow_mut()));
    for schema in reqs {
        let sql = match schema {
            Some(s) => format!("PRAGMA \"{}\".locking_mode=EXCLUSIVE", s.replace('"', "\"\"")),
            None => "PRAGMA locking_mode=EXCLUSIVE".to_string(),
        };
        let _ = conn.execute_batch(&sql);
    }
    if ATTACH_WAL.with(|a| a.replace(false)) {
        let _ = conn.execute_batch("PRAGMA locking_mode=EXCLUSIVE");
    }
    unwind::reraise();
}

/// Depois do comando.
pub fn after_statement(_conn: &Connection) {}

/// O `bind_table_init`.
pub fn bind_table_init(conn: &Connection) {
    use rusqlite::config::DbConfig;
    let def = conn.db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE).unwrap_or(false);
    let wr = conn.db_config(DbConfig::SQLITE_DBCONFIG_WRITABLE_SCHEMA).unwrap_or(false);
    let _ = conn.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, false);
    let _ = conn.set_db_config(DbConfig::SQLITE_DBCONFIG_WRITABLE_SCHEMA, true);
    let _ = conn.execute_batch("CREATE TABLE IF NOT EXISTS temp.sqlite_parameters(\n  key TEXT PRIMARY KEY,\n  value\n) WITHOUT ROWID;");
    let _ = conn.set_db_config(DbConfig::SQLITE_DBCONFIG_WRITABLE_SCHEMA, wr);
    let _ = conn.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, def);
    unwind::reraise();
}

/// (somente leitura, estado da transação: 0 nenhuma, 1 leitura, 2 escrita).
pub fn db_state(conn: &Connection, schema: &str) -> (bool, i32) {
    let ro = conn.is_readonly(schema).unwrap_or(false);
    let txn = if conn.is_autocommit() { 0 } else { 2 };
    (ro, txn)
}

/// Mensagem do `sqlite3_load_extension`: no sandbox não existe biblioteca que o dlopen do host
/// possa abrir, então a falha é sempre a de arquivo inexistente.
pub fn load_extension_error(file: &[u8]) -> String {
    format!("{}.so: cannot open shared object file: No such file or directory", String::from_utf8_lossy(file))
}

/// `.backup`/`.restore`: copia `src_db` de `src` pra `dst_db` de `dst`, 100 páginas por passo.
/// `restore` dá até 3 voltas de 100 ms quando a origem está ocupada, como o original.
pub fn backup(src: &Connection, src_db: &str, dst: &mut Connection, dst_db: &str, restore: bool) -> Result<(), String> {
    use rusqlite::backup::{Backup, StepResult};
    let b = match Backup::new_with_names(src, src_db, dst, dst_db) {
        Ok(b) => b,
        Err(e) => {
            unwind::reraise();
            return Err(crate::cli::exec::error_parts(&e).1);
        }
    };
    let mut busy = 0;
    let outcome = loop {
        let r = b.step(100);
        unwind::reraise();
        match r {
            Ok(StepResult::Done) => break Ok(()),
            Ok(StepResult::More) => continue,
            Ok(StepResult::Busy) | Ok(StepResult::Locked) if restore => {
                busy += 1;
                if busy > 3 {
                    break Err("source database is busy".to_string());
                }
                let _ = unwind::guard(|| sys::current().nanosleep(std::time::Duration::from_millis(100)));
            }
            Ok(_) => break Err("database is locked".to_string()),
            Err(e) => break Err(crate::cli::exec::error_parts(&e).1),
        }
    };
    drop(b);
    outcome
}

/// `.dbinfo`.
pub fn dbinfo(sh: &mut Shell, _args: &[Vec<u8>]) -> i32 {
    sh.eputs("Error: .dbinfo is not supported by this build\n");
    1
}

/// `.sha3sum`.
pub fn sha3sum_command(sh: &mut Shell, _args: &[Vec<u8>]) -> i32 {
    sh.eputs(".sha3sum failed.\n");
    1
}

/// Deixa o `.schema` saber que está rodando (o `shell_add_schema` usa a conexão).
pub fn set_schema_conn_hint(_on: bool) {}

/// Argumentos do `.import`.
pub struct ImportArgs<'a> {
    pub data: &'a [u8],
    pub file_name: &'a [u8],
    pub table: &'a [u8],
    pub schema: Option<&'a [u8]>,
    pub col_sep: u8,
    pub row_sep: u8,
    pub ascii: bool,
    pub skip: i64,
    pub verbose: i32,
}

pub use ext::import;

fn text_arg(v: ValueRef<'_>) -> Option<Vec<u8>> {
    match v {
        ValueRef::Null => None,
        ValueRef::Integer(i) => Some(i.to_string().into_bytes()),
        ValueRef::Real(f) => Some(fmt_real(f, 15)),
        ValueRef::Text(t) | ValueRef::Blob(t) => Some(t.to_vec()),
    }
}

/// `shell_add_schema(S,X,N)`: põe o esquema `X` no `CREATE` de `S`.
fn shell_add_schema(sql: Option<Vec<u8>>, schema: Option<Vec<u8>>) -> Option<Vec<u8>> {
    let z = sql?;
    const PREFIX: [&str; 6] = ["TABLE", "INDEX", "UNIQUE INDEX", "VIEW", "TRIGGER", "VIRTUAL TABLE"];
    if !z.starts_with(b"CREATE ") {
        return Some(z);
    }
    for p in PREFIX {
        let n = p.len();
        if z.len() > n + 7 && &z[7..7 + n] == p.as_bytes() && z[n + 7] == b' ' {
            if let Some(s) = &schema {
                let mut out = z[..n + 7].to_vec();
                out.push(b' ');
                if text::needs_quote(s) && !s.eq_ignore_ascii_case(b"temp") {
                    out.extend(text::dquote(s));
                } else {
                    out.extend_from_slice(s);
                }
                out.push(b'.');
                out.extend_from_slice(&z[n + 8..]);
                return Some(out);
            }
            return Some(z);
        }
    }
    Some(z)
}

/// Registra as funções do CLI e as trocadas pelo sandbox numa conexão.
pub fn register_all(conn: &Connection) {
    let det = FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC;
    let nondet = FunctionFlags::SQLITE_UTF8;
    let _ = conn.authorizer(Some(authorizer));
    install_progress(conn);
    let _ = conn.create_scalar_function("shell_add_schema", 3, det, |ctx| {
        Ok(shell_add_schema(text_arg(ctx.get_raw(0)), text_arg(ctx.get_raw(1))).map(Value::Blob).map(blob_to_text))
    });
    let _ = conn.create_scalar_function("shell_module_schema", 1, det, |_ctx| Ok(Value::Null));
    let _ = conn.create_scalar_function("shell_putsnl", 1, nondet, |ctx| {
        let v = ctx.get_raw(0);
        let mut line = text_arg(v).map(|t| text::cstr(&t).to_vec()).unwrap_or_else(|| b"(null)".to_vec());
        line.push(b'\n');
        queue_output(&line);
        Ok(owned(v))
    });
    let _ = conn.create_scalar_function("strtod", 1, det, |ctx| {
        let Some(t) = text_arg(ctx.get_raw(0)) else { return Ok(Value::Null) };
        Ok(Value::Real(ext::c_strtod(&t)))
    });
    for n in [1, 2] {
        let _ = conn.create_scalar_function("dtostr", n, det, move |ctx| {
            let r = match ctx.get_raw(0) {
                ValueRef::Real(f) => f,
                ValueRef::Integer(i) => i as f64,
                other => text_arg(other).map(|t| ext::c_strtod(&t)).unwrap_or(0.0),
            };
            let digits = if ctx.len() >= 2 { ctx.get::<i64>(1).unwrap_or(26) } else { 26 }.clamp(1, 350);
            Ok(Value::Text(String::from_utf8_lossy(&sqlite_printf(&format!("%#+.{digits}e"), Value::Real(r))).into_owned()))
        });
    }
    let _ = conn.create_scalar_function("usleep", 1, nondet, |ctx| {
        let us = ctx.get::<i64>(0).unwrap_or(0);
        let ok = unwind::guard(|| {
            if us > 0 {
                let _ = sys::current().nanosleep(std::time::Duration::from_micros(us as u64));
            }
        });
        if ok.is_none() {
            return Err(rusqlite::Error::UserFunctionError("interrupted".into()));
        }
        Ok(Value::Integer(us))
    });
    for n in [1, 2] {
        let _ = conn.create_scalar_function("edit", n, nondet, |_ctx| {
            Err::<Value, _>(rusqlite::Error::UserFunctionError("EDITOR returned non-zero".into()))
        });
    }
    for n in [1, 2] {
        let _ = conn.create_scalar_function("load_extension", n, nondet | FunctionFlags::SQLITE_DIRECTONLY, |ctx| {
            let f = text_arg(ctx.get_raw(0)).unwrap_or_default();
            Err::<Value, _>(rusqlite::Error::UserFunctionError(load_extension_error(&f).into()))
        });
    }
    let innocuous = FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_INNOCUOUS;
    let _ = conn.create_scalar_function("random", 0, innocuous, |_ctx| {
        let mut b = [0u8; 8];
        let ok = unwind::guard(|| sys::current().getrandom(&mut b));
        if ok.is_none() {
            return Err(rusqlite::Error::UserFunctionError("interrupted".into()));
        }
        let mut r = i64::from_le_bytes(b);
        // Como o randomFunc: evita o -r de i64::MIN dar overflow.
        if r < 0 {
            r = -(r & i64::MAX);
        }
        Ok(r)
    });
    let _ = conn.create_scalar_function("randomblob", 1, innocuous, |ctx| {
        let n = match ctx.get_raw(0) {
            ValueRef::Integer(i) => i,
            ValueRef::Real(f) => f as i64,
            ValueRef::Null => 0,
            other => text_arg(other).map(|t| ext::c_strtod(&t) as i64).unwrap_or(0),
        }
        .max(1) as usize;
        let mut v = Vec::new();
        if v.try_reserve_exact(n).is_err() {
            return Err(rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_NOMEM), None));
        }
        v.resize(n, 0);
        let ok = unwind::guard(|| sys::current().getrandom(&mut v));
        if ok.is_none() {
            return Err(rusqlite::Error::UserFunctionError("interrupted".into()));
        }
        Ok(Value::Blob(v))
    });
    date::register(conn);
    ext::register(conn);
    let _ = rusqlite::vtab::series::load_module(conn);
    unwind::reraise();
}

/// Cópia de um argumento como valor próprio (texto inválido em UTF-8 vira blob).
pub fn owned(v: ValueRef<'_>) -> Value {
    match v {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => Value::Integer(i),
        ValueRef::Real(f) => Value::Real(f),
        ValueRef::Text(t) => blob_to_text(Value::Blob(t.to_vec())),
        ValueRef::Blob(b) => Value::Blob(b.to_vec()),
    }
}

fn blob_to_text(v: Value) -> Value {
    match v {
        Value::Blob(b) => match String::from_utf8(b) {
            Ok(s) => Value::Text(s),
            Err(e) => Value::Blob(e.into_bytes()),
        },
        other => other,
    }
}
