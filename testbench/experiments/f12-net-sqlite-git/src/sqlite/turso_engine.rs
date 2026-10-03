//! Caminho 3: `turso_core` (Rust puro) com IO plugável. O [`NsIo`] implementa `turso_core::IO` e
//! `turso_core::File` sobre o [`SharedFs`], e o relógio de parede vem de nós (`Clock`), o que deixa o
//! `datetime('now')` obedecer ao relógio do sandbox.

use std::sync::Arc;

use harness::MemTree;
use turso_core::io::FileSyncType;
use turso_core::io::clock::{DefaultClock, MonotonicInstant, WallClockInstant};
use turso_core::{
    Buffer, CheckpointMode, Clock, Completion, Connection, Database, File, IO, LimboError, Numeric, OpenFlags, OpenOptions,
    SqliteDialect, Value,
};

use super::cli::{Backend, Cell, EngineError, Phase, Session};
use super::memfs::{Level, Node, SharedFs, db_family, next_id};

/// IO do turso sobre o FS compartilhado.
pub struct NsIo {
    fs: Arc<SharedFs>,
    /// Relógio de parede fixo (sandbox), ou `None` pra usar o do host.
    wall: Option<WallClockInstant>,
}

impl NsIo {
    pub fn new(fs: Arc<SharedFs>, wall: Option<WallClockInstant>) -> NsIo {
        NsIo { fs, wall }
    }
}

impl Clock for NsIo {
    fn current_time_monotonic(&self) -> MonotonicInstant {
        DefaultClock.current_time_monotonic()
    }

    fn current_time_wall_clock(&self) -> WallClockInstant {
        self.wall.unwrap_or_else(|| DefaultClock.current_time_wall_clock())
    }
}

impl IO for NsIo {
    fn open_file(&self, path: &str, flags: OpenFlags, _direct: bool) -> turso_core::Result<Arc<dyn File>> {
        let node = match self.fs.get(path) {
            Some(n) => n,
            None if flags.contains(OpenFlags::Create) => self.fs.get_or_create(path),
            None => {
                return Err(turso_core::CompletionError::IOError(std::io::ErrorKind::NotFound, "open").into());
            }
        };
        Ok(Arc::new(NsFile { node, lock_id: next_id() }))
    }

    fn remove_file(&self, path: &str) -> turso_core::Result<()> {
        self.fs.remove(path);
        Ok(())
    }

    fn file_id(&self, path: &str) -> turso_core::Result<turso_core::io::FileId> {
        Ok(turso_core::io::FileId::from_path_hash(path))
    }

    fn supports_shared_wal_coordination(&self) -> bool {
        false
    }
}

pub struct NsFile {
    node: Arc<Node>,
    lock_id: u64,
}

impl File for NsFile {
    fn lock_file(&self, exclusive: bool) -> turso_core::Result<()> {
        let level = if exclusive { Level::Exclusive } else { Level::Shared };
        if self.node.lock(self.lock_id, Level::None, level) {
            Ok(())
        } else {
            self.node.unlock(self.lock_id, Level::None);
            Err(LimboError::LockingError("database is locked by another process".into()))
        }
    }

    fn unlock_file(&self) -> turso_core::Result<()> {
        self.node.unlock(self.lock_id, Level::None);
        Ok(())
    }

    fn pread(&self, pos: u64, c: Completion) -> turso_core::Result<Completion> {
        let n = {
            let buf = c.as_read().buf();
            let dst = buf.as_mut_slice();
            let n = self.node.read_at(pos as usize, dst);
            dst[n..].fill(0);
            n
        };
        c.complete(n as i32);
        Ok(c)
    }

    fn pwrite(&self, pos: u64, buffer: Arc<Buffer>, c: Completion) -> turso_core::Result<Completion> {
        self.node.write_at(pos as usize, buffer.as_slice());
        c.complete(buffer.len() as i32);
        Ok(c)
    }

    fn sync(&self, c: Completion, _sync_type: FileSyncType) -> turso_core::Result<Completion> {
        c.complete(0);
        Ok(c)
    }

    fn size(&self) -> turso_core::Result<u64> {
        Ok(self.node.len() as u64)
    }

    fn truncate(&self, len: u64, c: Completion) -> turso_core::Result<Completion> {
        self.node.truncate(len as usize);
        c.complete(0);
        Ok(c)
    }
}

/// Abre (ou reaproveita, pelo registro do processo) o banco em `path` do FS compartilhado.
pub fn open_db(fs: Arc<SharedFs>, path: &str, wall: Option<WallClockInstant>) -> Result<Arc<Database>, String> {
    let io: Arc<dyn IO> = Arc::new(NsIo::new(fs, wall));
    Database::open(io, path, OpenOptions::new(Arc::new(SqliteDialect))).map_err(|e| e.to_string())
}

fn value_cell(v: &Value) -> Cell {
    match v {
        Value::Null => Cell::Null,
        Value::Numeric(Numeric::Integer(i)) => Cell::Integer(*i),
        Value::Numeric(Numeric::Float(f)) => Cell::Real(f64::from(*f)),
        Value::Text(t) => Cell::Text(t.as_str().as_bytes().to_vec()),
        Value::Blob(b) => Cell::Blob(b.to_vec()),
    }
}

fn message(e: &LimboError) -> String {
    let s = e.to_string();
    s.strip_prefix("Parse error: ").map(str::to_string).unwrap_or(s)
}

fn code(e: &LimboError) -> i32 {
    match e {
        LimboError::Constraint(_) | LimboError::ForeignKeyConstraint(_) | LimboError::Raise(..) => 19,
        LimboError::Busy => 5,
        LimboError::ReadOnly => 8,
        _ => 1,
    }
}

/// Roda um comando numa conexão turso.
pub fn execute(conn: &Arc<Connection>, sql: &str, on_row: &mut dyn FnMut(&[String], &[Cell])) -> Result<(), EngineError> {
    let mut stmt = conn.prepare(sql).map_err(|e| EngineError { phase: Phase::Prepare, message: message(&e), offset: None, code: 1 })?;
    let names: Vec<String> = (0..stmt.num_columns()).map(|i| stmt.get_column_name(i).into_owned()).collect();
    stmt.run_with_row_callback(|row| {
        let cells: Vec<Cell> = row.get_values().map(value_cell).collect();
        on_row(&names, &cells);
        Ok(())
    })
    .map_err(|e| EngineError::step(message(&e), code(&e)))
}

pub struct TursoBackend {
    /// Relógio do sandbox repassado ao IO.
    pub wall: Option<WallClockInstant>,
}

pub struct TursoSession {
    db: Option<Arc<Database>>,
    conn: Option<Arc<Connection>>,
    ns: String,
    names: Vec<String>,
    path: Option<String>,
}

impl Backend for TursoBackend {
    fn name(&self) -> String {
        "turso_core 0.8.1 (IO próprio sobre FS em memória)".to_string()
    }

    fn open(&mut self, fs: &mut MemTree, path: Option<&str>, _readonly: bool) -> Result<Box<dyn Session>, String> {
        let shared = SharedFs::global();
        let ns = format!("/ns{}", next_id());
        let names = path.map(db_family).unwrap_or_default();
        shared.load(&ns, fs, &names);
        let full = match path {
            Some(p) => format!("{ns}/{p}"),
            // Banco "em memória": um arquivo anônimo no namespace, descartado no fim.
            None => format!("{ns}/:memory-anon"),
        };
        let db = open_db(shared, &full, self.wall)?;
        let conn = db.connect().map_err(|e| e.to_string())?;
        Ok(Box::new(TursoSession { db: Some(db), conn: Some(conn), ns, names, path: path.map(str::to_string) }))
    }
}

impl Session for TursoSession {
    fn execute(&mut self, sql: &str, on_row: &mut dyn FnMut(&[String], &[Cell])) -> Result<(), EngineError> {
        execute(self.conn.as_ref().expect("conexão aberta"), sql, on_row)
    }

    fn close(mut self: Box<Self>, fs: &mut MemTree, _abrupt: bool) -> Result<(), String> {
        // O turso só tem WAL: imitar a saída sem close do sqlite3 deixaria os dados confirmados num
        // `-wal` em vez de um journal quente, o que não é equivalente. Como o CLI é nosso, aqui o
        // fechamento é sempre limpo (checkpoint + close).
        let shared = SharedFs::global();
        if let Some(conn) = self.conn.take() {
            // Como o sqlite3 ao fechar a última conexão: leva o WAL pro arquivo principal.
            let _ = conn.checkpoint(CheckpointMode::Truncate { upper_bound_inclusive: None });
            conn.close().map_err(|e| e.to_string())?;
        }
        self.db.take();
        if let Some(p) = &self.path {
            let wal = format!("{}/{p}-wal", self.ns);
            if shared.get(&wal).is_some_and(|n| n.is_empty()) {
                shared.remove(&wal);
            }
            shared.store(&self.ns, fs, &self.names);
        }
        shared.clear(&self.ns);
        Ok(())
    }
}
