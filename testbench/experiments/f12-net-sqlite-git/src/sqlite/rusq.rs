//! Caminhos 1 e 2: o SQLite de verdade (rusqlite com `bundled`), com o banco morando no nosso FS.
//!
//! - [`SerializeBackend`]: cada comando abre um banco em memória, carrega os bytes do arquivo com
//!   `deserialize` e, no fim, grava de volta com `serialize`. Nenhum VFS próprio.
//! - [`PluginBackend`]: um VFS nosso (trait segura `sqlite_plugin::vfs::Vfs`) sobre o [`SharedFs`],
//!   registrado uma vez no processo. O SQLite faz o I/O e as travas por ele.

use std::borrow::Cow;
use std::ffi::CString;
use std::sync::{Arc, OnceLock};

use harness::MemTree;
use rusqlite::types::ValueRef;
use rusqlite::{Connection, MAIN_DB, OpenFlags};
use sqlite_plugin::flags::{AccessFlags, LockLevel, OpenOpts};
use sqlite_plugin::vars;
use sqlite_plugin::vfs::{RegisterOpts, Vfs, VfsHandle, VfsResult, register_static};

use super::cli::{Backend, Cell, EngineError, Session};
use super::memfs::{Level, Node, SharedFs, db_family, next_id};

/// Roda um comando numa conexão rusqlite e traduz linhas e erros pro CLI.
pub fn execute(conn: &Connection, sql: &str, on_row: &mut dyn FnMut(&[String], &[Cell])) -> Result<(), EngineError> {
    let mut stmt = conn.prepare(sql).map_err(prepare_error)?;
    let names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
    let n = names.len();
    let mut rows = stmt.raw_query();
    loop {
        match rows.next() {
            Ok(Some(row)) => {
                let mut cells = Vec::with_capacity(n);
                for i in 0..n {
                    let v = row.get_ref(i).map_err(step_error)?;
                    cells.push(match v {
                        ValueRef::Null => Cell::Null,
                        ValueRef::Integer(i) => Cell::Integer(i),
                        ValueRef::Real(f) => Cell::Real(f),
                        ValueRef::Text(t) => Cell::Text(t.to_vec()),
                        ValueRef::Blob(b) => Cell::Blob(b.to_vec()),
                    });
                }
                on_row(&names, &cells);
            }
            Ok(None) => return Ok(()),
            Err(e) => return Err(step_error(e)),
        }
    }
}

fn prepare_error(e: rusqlite::Error) -> EngineError {
    match e {
        rusqlite::Error::SqlInputError { msg, offset, .. } => {
            EngineError::prepare(msg, usize::try_from(offset).ok())
        }
        rusqlite::Error::SqliteFailure(err, msg) => EngineError {
            phase: super::cli::Phase::Prepare,
            message: msg.unwrap_or_else(|| err.to_string()),
            offset: None,
            code: err.extended_code & 0xff,
        },
        other => EngineError::prepare(other.to_string(), None),
    }
}

fn step_error(e: rusqlite::Error) -> EngineError {
    match e {
        rusqlite::Error::SqliteFailure(err, msg) => {
            EngineError::step(msg.unwrap_or_else(|| err.to_string()), err.extended_code & 0xff)
        }
        other => EngineError::step(other.to_string(), 1),
    }
}

// ---------------------------------------------------------------------------------------------
// Caminho 1: serialize/deserialize na fronteira do comando
// ---------------------------------------------------------------------------------------------

#[derive(Default)]
pub struct SerializeBackend;

pub struct SerializeSession {
    conn: Connection,
    path: Option<String>,
    readonly: bool,
    /// O arquivo estava em modo WAL (cabeçalho 18/19 = 2): volta assim ao ser gravado.
    wal_header: bool,
}

/// Cabeçalho em modo WAL (bytes 18 e 19 iguais a 2).
pub fn is_wal_header(bytes: &[u8]) -> bool {
    bytes.len() > 19 && bytes[18] == 2 && bytes[19] == 2
}

impl SerializeSession {
    /// Abre um banco em memória com o conteúdo de `bytes` (vazio = banco novo), sem tratar WAL.
    pub fn from_bytes(bytes: &[u8], readonly: bool) -> Result<Connection, String> {
        let mut conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
        if !bytes.is_empty() {
            conn.deserialize_read_exact(MAIN_DB, bytes, bytes.len(), readonly).map_err(|e| e.to_string())?;
        }
        Ok(conn)
    }

    /// Como [`Self::from_bytes`], mas aceita arquivo em modo WAL já sem `-wal` pendente: o banco em
    /// memória não suporta WAL, então o cabeçalho vira rollback (1/1) na carga e volta a 2/2 na gravação.
    pub fn from_file_image(bytes: &[u8], readonly: bool) -> Result<(Connection, bool), String> {
        if is_wal_header(bytes) {
            let mut patched = bytes.to_vec();
            patched[18] = 1;
            patched[19] = 1;
            return Ok((Self::from_bytes(&patched, readonly)?, true));
        }
        Ok((Self::from_bytes(bytes, readonly)?, false))
    }

    pub fn to_file_image(conn: &Connection, wal_header: bool) -> Result<Vec<u8>, String> {
        let mut bytes = Self::to_bytes(conn)?;
        if wal_header && bytes.len() > 19 {
            bytes[18] = 2;
            bytes[19] = 2;
        }
        Ok(bytes)
    }

    /// Bytes do banco (vazio se nada foi gravado).
    pub fn to_bytes(conn: &Connection) -> Result<Vec<u8>, String> {
        let pages: i64 = conn.query_row("PRAGMA page_count", [], |r| r.get(0)).map_err(|e| e.to_string())?;
        if pages == 0 {
            return Ok(Vec::new());
        }
        let data = conn.serialize(MAIN_DB).map_err(|e| e.to_string())?;
        Ok(data.to_vec())
    }
}

impl Backend for SerializeBackend {
    fn name(&self) -> String {
        format!("rusqlite {} serialize (SQLite {})", "0.40.2", rusqlite::version())
    }

    fn open(&mut self, fs: &mut MemTree, path: Option<&str>, readonly: bool) -> Result<Box<dyn Session>, String> {
        let bytes = path.and_then(|p| fs.read(p)).map(<[u8]>::to_vec).unwrap_or_default();
        if let Some(p) = path
            && fs.read(&format!("{p}-wal")).is_some_and(|w| !w.is_empty())
        {
            // Sem VFS não há como reaplicar o WAL: os commits que estão nele se perderiam.
            return Err("database has a pending WAL file (-wal); checkpoint it first".into());
        }
        let (conn, wal_header) = SerializeSession::from_file_image(&bytes, readonly)?;
        Ok(Box::new(SerializeSession { conn, path: path.map(str::to_string), readonly, wal_header }))
    }
}

impl Session for SerializeSession {
    fn execute(&mut self, sql: &str, on_row: &mut dyn FnMut(&[String], &[Cell])) -> Result<(), EngineError> {
        execute(&self.conn, sql, on_row)
    }

    fn close(self: Box<Self>, fs: &mut MemTree, abrupt: bool) -> Result<(), String> {
        let Some(path) = &self.path else { return Ok(()) };
        if self.readonly {
            return Ok(());
        }
        if abrupt && !self.conn.is_autocommit() {
            // Não existe journal neste caminho: o equivalente a recuperar o journal quente é desfazer.
            self.conn.execute_batch("ROLLBACK").map_err(|e| e.to_string())?;
        }
        let bytes = SerializeSession::to_file_image(&self.conn, self.wal_header)?;
        let changed = fs.read(path).map(|old| old != bytes.as_slice()).unwrap_or(true);
        if changed && !(bytes.is_empty() && fs.read(path).is_some()) {
            crate::shell::write_file(fs, path, bytes);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// Caminho 2: VFS próprio com a trait segura do sqlite-plugin
// ---------------------------------------------------------------------------------------------

/// Nome do VFS registrado no SQLite.
pub const VFS_NAME: &str = "f12mem";

pub struct MemVfs {
    fs: Arc<SharedFs>,
}

pub struct MemHandle {
    id: u64,
    node: Arc<Node>,
    path: Option<String>,
    readonly: bool,
    delete_on_close: bool,
    level: Level,
}

impl VfsHandle for MemHandle {
    fn readonly(&self) -> bool {
        self.readonly
    }

    fn in_memory(&self) -> bool {
        false
    }
}

fn level_of(l: LockLevel) -> Level {
    match l {
        LockLevel::Unlocked => Level::None,
        LockLevel::Shared => Level::Shared,
        LockLevel::Reserved => Level::Reserved,
        LockLevel::Pending => Level::Pending,
        LockLevel::Exclusive => Level::Exclusive,
    }
}

impl Vfs for MemVfs {
    type Handle = MemHandle;

    fn canonical_path<'a>(&self, path: Cow<'a, str>) -> VfsResult<Cow<'a, str>> {
        Ok(path)
    }

    fn open(&self, path: Option<&str>, opts: OpenOpts) -> VfsResult<Self::Handle> {
        let mode = opts.mode();
        let node = match path {
            Some(p) => {
                if let Some(n) = self.fs.get(p) {
                    if mode.must_create() {
                        return Err(vars::SQLITE_CANTOPEN);
                    }
                    n
                } else if mode.is_readonly() || matches!(mode, sqlite_plugin::flags::OpenMode::ReadWrite { create: sqlite_plugin::flags::CreateMode::None }) {
                    return Err(vars::SQLITE_CANTOPEN);
                } else {
                    self.fs.get_or_create(p)
                }
            }
            None => Arc::new(Node::default()),
        };
        Ok(MemHandle {
            id: next_id(),
            node,
            path: path.map(str::to_string),
            readonly: mode.is_readonly(),
            delete_on_close: opts.delete_on_close(),
            level: Level::None,
        })
    }

    fn delete(&self, path: &str) -> VfsResult<()> {
        if self.fs.remove(path) { Ok(()) } else { Err(vars::SQLITE_IOERR_DELETE_NOENT) }
    }

    fn access(&self, path: &str, _flags: AccessFlags) -> VfsResult<bool> {
        Ok(self.fs.exists(path))
    }

    fn file_size(&self, handle: &mut Self::Handle) -> VfsResult<usize> {
        Ok(handle.node.len())
    }

    fn truncate(&self, handle: &mut Self::Handle, size: usize) -> VfsResult<()> {
        handle.node.truncate(size);
        Ok(())
    }

    fn write(&self, handle: &mut Self::Handle, offset: usize, data: &[u8]) -> VfsResult<usize> {
        handle.node.write_at(offset, data);
        Ok(data.len())
    }

    fn read(&self, handle: &mut Self::Handle, offset: usize, data: &mut [u8]) -> VfsResult<usize> {
        Ok(handle.node.read_at(offset, data))
    }

    fn lock(&self, handle: &mut Self::Handle, level: LockLevel) -> VfsResult<()> {
        let want = level_of(level);
        if handle.node.lock(handle.id, handle.level, want) {
            handle.level = want;
            Ok(())
        } else {
            if want >= Level::Pending {
                // Ficou com PENDING mesmo devolvendo BUSY.
                handle.level = handle.level.max(Level::Pending).min(Level::Pending);
            }
            Err(vars::SQLITE_BUSY)
        }
    }

    fn unlock(&self, handle: &mut Self::Handle, level: LockLevel) -> VfsResult<()> {
        let want = level_of(level);
        handle.node.unlock(handle.id, want);
        handle.level = want.min(handle.level);
        Ok(())
    }

    fn check_reserved_lock(&self, handle: &mut Self::Handle) -> VfsResult<bool> {
        Ok(handle.node.reserved_held())
    }

    fn close(&self, handle: Self::Handle) -> VfsResult<()> {
        handle.node.unlock(handle.id, Level::None);
        if handle.delete_on_close
            && let Some(p) = &handle.path
        {
            self.fs.remove(p);
        }
        Ok(())
    }
}

static REGISTERED: OnceLock<Result<(), i32>> = OnceLock::new();

/// Registra o VFS uma vez por processo.
pub fn register_vfs() -> Result<(), String> {
    let r = REGISTERED.get_or_init(|| {
        let name = CString::new(VFS_NAME).expect("nome do VFS");
        register_static(name, MemVfs { fs: SharedFs::global() }, RegisterOpts { make_default: false }).map(|_| ())
    });
    r.map_err(|code| format!("register_static falhou com código {code}"))
}

/// Abre uma conexão pelo VFS em `path` (caminho absoluto no [`SharedFs`]).
pub fn open_vfs(path: &str, readonly: bool) -> Result<Connection, String> {
    register_vfs()?;
    let flags = if readonly {
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX
    } else {
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_NO_MUTEX
    };
    Connection::open_with_flags_and_vfs(path, flags, VFS_NAME).map_err(|e| e.to_string())
}

#[derive(Default)]
pub struct PluginBackend;

pub struct PluginSession {
    conn: Option<Connection>,
    ns: String,
    names: Vec<String>,
}

impl Backend for PluginBackend {
    fn name(&self) -> String {
        format!("rusqlite {} + sqlite-plugin 0.11.0 (SQLite {})", "0.40.2", rusqlite::version())
    }

    fn open(&mut self, fs: &mut MemTree, path: Option<&str>, readonly: bool) -> Result<Box<dyn Session>, String> {
        let shared = SharedFs::global();
        let ns = format!("/ns{}", next_id());
        let (conn, names) = match path {
            Some(p) => {
                let names = db_family(p);
                shared.load(&ns, fs, &names);
                let conn = open_vfs(&format!("{ns}/{p}"), readonly)?;
                if fs.read(p).is_some_and(is_wal_header) {
                    // Sem shm_map, WAL só funciona em modo de trava exclusivo (índice do WAL no heap),
                    // e isso tem que ser ligado antes do primeiro acesso ao banco.
                    conn.execute_batch("PRAGMA locking_mode=EXCLUSIVE").map_err(|e| e.to_string())?;
                }
                (conn, names)
            }
            None => (Connection::open_in_memory().map_err(|e| e.to_string())?, Vec::new()),
        };
        Ok(Box::new(PluginSession { conn: Some(conn), ns, names }))
    }
}

impl Session for PluginSession {
    fn execute(&mut self, sql: &str, on_row: &mut dyn FnMut(&[String], &[Cell])) -> Result<(), EngineError> {
        execute(self.conn.as_ref().expect("conexão aberta"), sql, on_row)
    }

    fn close(mut self: Box<Self>, fs: &mut MemTree, abrupt: bool) -> Result<(), String> {
        let shared = SharedFs::global();
        if abrupt {
            // O processo "morre" antes do close: o FS fica como está, journal quente incluído.
            shared.store(&self.ns, fs, &self.names);
        }
        if let Some(conn) = self.conn.take() {
            conn.close().map_err(|(_, e)| e.to_string())?;
        }
        if !abrupt {
            shared.store(&self.ns, fs, &self.names);
        }
        shared.clear(&self.ns);
        Ok(())
    }
}
