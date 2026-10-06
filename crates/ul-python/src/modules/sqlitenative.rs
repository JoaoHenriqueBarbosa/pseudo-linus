//! Módulo nativo `_sqlite3`: a conexão ao SQLite (mesmo motor e VFS do programa `sqlite3`) com o
//! mínimo que o pacote `sqlite3` em Python precisa: executar uma instrução com parâmetros
//! (materializando as linhas), rodar um script, e registrar funções, agregados e collations escritos
//! em Python. A semântica do DB-API (cursores, transações implícitas, `Row`, conversores) fica no
//! Python, em `modules/py/sqlite3.py`.
//!
//! As operações devolvem uma tupla `(tipo, mensagem)` no erro (o Python levanta a classe certa) ou o
//! resultado. Os chamáveis Python ficam num registro por thread e os fechamentos do SQLite só levam
//! o índice (o SQLite exige `Send`).

use std::cell::RefCell;
use std::cmp::Ordering;
use std::rc::Rc;

use rusqlite::functions::{Aggregate, Context, FunctionFlags};
use rusqlite::types::{Value as Sql, ValueRef};
use rusqlite::{Connection, ErrorCode, OpenFlags};

use crate::modules::ModuleBuilder;
use crate::native_util::bind;
use crate::object::{ExtObject, Kw, ModuleObj, Value};
use crate::vm::{PyException, PyResult, Vm};

thread_local! {
    /// Chamáveis registrados (funções, classes de agregado, collations), pelo índice.
    static CALLBACKS: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
    /// Instâncias vivas de agregados, pelo índice (`None` depois do `finalize`).
    static AGGREGATES: RefCell<Vec<Option<Value>>> = const { RefCell::new(Vec::new()) };
}

fn register(v: Value) -> usize {
    CALLBACKS.with(|c| {
        let mut c = c.borrow_mut();
        c.push(v);
        c.len() - 1
    })
}

fn callback(id: usize) -> Value {
    CALLBACKS.with(|c| c.borrow()[id].clone())
}

fn err(kind: &str, msg: impl Into<String>) -> Value {
    Value::tuple(vec![Value::str(kind.to_string()), Value::str(msg.into())])
}

fn kind_of(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::InternalMalfunction | ErrorCode::NotFound => "InternalError",
        ErrorCode::ConstraintViolation | ErrorCode::TypeMismatch => "IntegrityError",
        ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase => "DatabaseError",
        ErrorCode::TooBig => "DataError",
        ErrorCode::ApiMisuse | ErrorCode::ParameterOutOfRange => "InterfaceError",
        ErrorCode::OutOfMemory => "MemoryError",
        _ => "OperationalError",
    }
}

fn map_error(e: &rusqlite::Error) -> Value {
    use rusqlite::Error as E;
    match e {
        E::MultipleStatement => err("ProgrammingError", "You can only execute one statement at a time."),
        E::InvalidParameterCount(given, expected) => err(
            "ProgrammingError",
            format!("Incorrect number of bindings supplied. The current statement uses {expected}, and there are {given} supplied."),
        ),
        E::SqlInputError { error, msg, .. } => err(kind_of(error.code), msg.clone()),
        E::SqliteFailure(f, msg) => err(kind_of(f.code), msg.clone().unwrap_or_else(|| e.to_string())),
        E::UserFunctionError(_) => err("OperationalError", "user-defined function raised exception"),
        other => err("DatabaseError", other.to_string()),
    }
}

/// Valor Python para parâmetro SQL.
fn to_sql(v: &Value) -> Result<Sql, String> {
    Ok(match v {
        Value::None => Sql::Null,
        Value::Bool(b) => Sql::Integer(i64::from(*b)),
        Value::Int(i) => Sql::Integer(*i),
        Value::Big(_) => return Err("OverflowError:Python int too large to convert to SQLite INTEGER".into()),
        Value::Float(f) => Sql::Real(*f),
        Value::Str(s) => Sql::Text(s.as_str().to_string()),
        other => match other.bytes_like() {
            Some(b) => Sql::Blob(b.to_vec()),
            None => return Err(format!("ProgrammingError:type '{}' is not supported", other.type_name())),
        },
    })
}

fn from_sql(v: ValueRef<'_>) -> Value {
    match v {
        ValueRef::Null => Value::None,
        ValueRef::Integer(i) => Value::Int(i),
        ValueRef::Real(f) => Value::Float(f),
        ValueRef::Text(t) => Value::str(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(b) => Value::bytes(b.to_vec()),
    }
}

fn user_err(msg: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::UserFunctionError(Box::<dyn std::error::Error + Send + Sync>::from(msg.into()))
}

fn call_py(f: &Value, args: Vec<Value>) -> Result<Value, rusqlite::Error> {
    let mut vm = crate::vm::current().ok_or_else(|| user_err("no vm"))?;
    vm.call_value(f, args, Vec::new()).map_err(|e: PyException| user_err(e.msg))
}

fn ctx_args(ctx: &Context<'_>) -> Vec<Value> {
    (0..ctx.len()).map(|i| from_sql(ctx.get_raw(i))).collect()
}

struct PyAggregate {
    class: usize,
}

impl Aggregate<usize, Sql> for PyAggregate {
    fn init(&self, _ctx: &mut Context<'_>) -> rusqlite::Result<usize> {
        let inst = call_py(&callback(self.class), Vec::new())?;
        Ok(AGGREGATES.with(|a| {
            let mut a = a.borrow_mut();
            a.push(Some(inst));
            a.len() - 1
        }))
    }

    fn step(&self, ctx: &mut Context<'_>, acc: &mut usize) -> rusqlite::Result<()> {
        let inst = AGGREGATES.with(|a| a.borrow()[*acc].clone()).ok_or_else(|| user_err("aggregate finished"))?;
        let mut vm = crate::vm::current().ok_or_else(|| user_err("no vm"))?;
        let step = vm.getattr(&inst, "step").map_err(|e| user_err(e.msg))?;
        call_py(&step, ctx_args(ctx))?;
        Ok(())
    }

    fn finalize(&self, _ctx: &mut Context<'_>, acc: Option<usize>) -> rusqlite::Result<Sql> {
        let Some(idx) = acc else { return Ok(Sql::Null) };
        let inst = AGGREGATES.with(|a| a.borrow_mut()[idx].take()).ok_or_else(|| user_err("aggregate finished"))?;
        let mut vm = crate::vm::current().ok_or_else(|| user_err("no vm"))?;
        let fin = vm.getattr(&inst, "finalize").map_err(|e| user_err(e.msg))?;
        let out = call_py(&fin, Vec::new())?;
        to_sql(&out).map_err(user_err)
    }
}

struct SqliteConn {
    conn: RefCell<Option<Connection>>,
}

impl SqliteConn {
    fn with<R>(&self, f: impl FnOnce(&Connection) -> R) -> Result<R, Value> {
        match self.conn.borrow().as_ref() {
            Some(c) => Ok(f(c)),
            None => Err(err("ProgrammingError", "Cannot operate on a closed database.")),
        }
    }

    fn run(&self, sql: &str, params: &Value) -> Value {
        self.with(|conn| run_statement(conn, sql, params)).unwrap_or_else(|e| e)
    }
}

/// `(None, nomes, tipos_declarados, linhas, mudanças, último_rowid)` ou `(tipo, mensagem)`.
fn run_statement(conn: &Connection, sql: &str, params: &Value) -> Value {
    let mut stmt = match conn.prepare(sql) {
        Ok(s) => s,
        Err(e) => return map_error(&e),
    };
    let count = stmt.parameter_count();
    let mut bound: Vec<(usize, Sql)> = Vec::with_capacity(count);
    let bad = |msg: String| -> Value {
        match msg.split_once(':') {
            Some((k, m)) if k.ends_with("Error") => err(k, m.to_string()),
            _ => err("ProgrammingError", msg),
        }
    };
    match params {
        Value::Dict(d) => {
            for i in 1..=count {
                let Some(name) = stmt.parameter_name(i) else {
                    return err("ProgrammingError", format!("Binding {i} has no name, but you supplied a dictionary (which has only names)."));
                };
                let key = name.trim_start_matches([':', '@', '$', '?']).to_string();
                let found = d.borrow().get(&Value::str(key.clone())).ok().flatten();
                let Some(v) = found else {
                    return err("ProgrammingError", format!("You did not supply a value for binding parameter :{key}."));
                };
                match to_sql(&v) {
                    Ok(s) => bound.push((i, s)),
                    Err(m) => return bad(format!("{}", rewrite_binding(&m, i))),
                }
            }
        }
        other => {
            let items: Vec<Value> = match other {
                Value::List(l) => l.borrow().clone(),
                Value::Tuple(t) => t.to_vec(),
                Value::None => Vec::new(),
                _ => return err("ProgrammingError", "parameters are of unsupported type"),
            };
            if items.len() != count {
                return err(
                    "ProgrammingError",
                    format!("Incorrect number of bindings supplied. The current statement uses {count}, and there are {} supplied.", items.len()),
                );
            }
            for (i, v) in items.iter().enumerate() {
                match to_sql(v) {
                    Ok(s) => bound.push((i + 1, s)),
                    Err(m) => return bad(rewrite_binding(&m, i + 1)),
                }
            }
        }
    }
    for (i, v) in &bound {
        if let Err(e) = stmt.raw_bind_parameter(*i, v) {
            return map_error(&e);
        }
    }
    let names: Vec<Value> = stmt.column_names().iter().map(|n| Value::str((*n).to_string())).collect();
    let decls: Vec<Value> = stmt
        .columns()
        .iter()
        .map(|c| c.decl_type().map_or(Value::None, |t| Value::str(t.to_string())))
        .collect();
    let mut rows: Vec<Value> = Vec::new();
    if names.is_empty() {
        if let Err(e) = stmt.raw_execute() {
            return map_error(&e);
        }
    } else {
        let ncols = names.len();
        let mut q = stmt.raw_query();
        loop {
            match q.next() {
                Ok(Some(row)) => {
                    let mut cells = Vec::with_capacity(ncols);
                    for i in 0..ncols {
                        match row.get_ref(i) {
                            Ok(v) => cells.push(from_sql(v)),
                            Err(e) => return map_error(&e),
                        }
                    }
                    rows.push(Value::tuple(cells));
                }
                Ok(None) => break,
                Err(e) => return map_error(&e),
            }
        }
    }
    Value::tuple(vec![
        Value::None,
        Value::tuple(names),
        Value::tuple(decls),
        Value::list(rows),
        Value::Int(conn.changes() as i64),
        Value::Int(conn.last_insert_rowid()),
    ])
}

/// Mensagem de `to_sql` com o número do parâmetro, no formato do CPython.
fn rewrite_binding(m: &str, idx: usize) -> String {
    match m.split_once(':') {
        Some((k, rest)) if k == "ProgrammingError" => format!("{k}:Error binding parameter {idx}: {rest}"),
        Some((k, rest)) => format!("{k}:{rest}"),
        None => m.to_string(),
    }
}

impl ExtObject for SqliteConn {
    fn type_name(&self) -> &'static str {
        "_sqlite3.Handle"
    }

    fn methods(&self) -> &'static [&'static str] {
        &[
            "run",
            "script",
            "is_autocommit",
            "total_changes",
            "close",
            "is_open",
            "create_function",
            "create_aggregate",
            "create_collation",
        ]
    }

    fn call_method(&self, _vm: &mut Vm, name: &str, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        let text = |i: usize| match args.get(i) {
            Some(Value::Str(s)) => s.as_str().to_string(),
            _ => String::new(),
        };
        Ok(match name {
            "run" => self.run(&text(0), args.get(1).unwrap_or(&Value::None)),
            "script" => self
                .with(|c| match c.execute_batch(&text(0)) {
                    Ok(()) => Value::None,
                    Err(e) => map_error(&e),
                })
                .unwrap_or_else(|e| e),
            "is_autocommit" => Value::Bool(self.conn.borrow().as_ref().is_none_or(|c| c.is_autocommit())),
            "total_changes" => Value::Int(self.conn.borrow().as_ref().map_or(0, |c| c.total_changes() as i64)),
            "is_open" => Value::Bool(self.conn.borrow().is_some()),
            "close" => {
                self.conn.borrow_mut().take();
                Value::None
            }
            "create_function" => {
                let narg = match args.get(1) {
                    Some(Value::Int(n)) => *n as i32,
                    _ => -1,
                };
                let deterministic = args.get(3).is_some_and(Value::is_true);
                let id = register(args.get(2).cloned().unwrap_or(Value::None));
                let mut flags = FunctionFlags::SQLITE_UTF8;
                if deterministic {
                    flags |= FunctionFlags::SQLITE_DETERMINISTIC;
                }
                self.with(|c| {
                    let r = c.create_scalar_function(text(0).as_str(), narg, flags, move |ctx: &Context<'_>| {
                        let out = call_py(&callback(id), ctx_args(ctx))?;
                        to_sql(&out).map_err(user_err)
                    });
                    match r {
                        Ok(()) => Value::None,
                        Err(e) => map_error(&e),
                    }
                })
                .unwrap_or_else(|e| e)
            }
            "create_aggregate" => {
                let narg = match args.get(1) {
                    Some(Value::Int(n)) => *n as i32,
                    _ => -1,
                };
                let class = register(args.get(2).cloned().unwrap_or(Value::None));
                self.with(|c| {
                    match c.create_aggregate_function(text(0).as_str(), narg, FunctionFlags::SQLITE_UTF8, PyAggregate { class }) {
                        Ok(()) => Value::None,
                        Err(e) => map_error(&e),
                    }
                })
                .unwrap_or_else(|e| e)
            }
            "create_collation" => {
                let id = register(args.get(1).cloned().unwrap_or(Value::None));
                self.with(|c| {
                    let r = c.create_collation(text(0).as_str(), move |a: &str, b: &str| {
                        let out = call_py(&callback(id), vec![Value::str(a.to_string()), Value::str(b.to_string())]);
                        match out {
                            Ok(Value::Int(n)) => n.cmp(&0),
                            _ => Ordering::Equal,
                        }
                    });
                    match r {
                        Ok(()) => Value::None,
                        Err(e) => map_error(&e),
                    }
                })
                .unwrap_or_else(|e| e)
            }
            other => return Err(crate::vm::type_error(format!("_sqlite3.Handle has no method '{other}'"))),
        })
    }
}

fn connect(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind("connect", args, kw, &["database", "uri"], 1)?;
    let path = match a[0].as_ref() {
        Some(Value::Str(s)) => s.as_str().to_string(),
        _ => return Err(crate::vm::type_error("connect() argument 1 must be str")),
    };
    let mut flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE;
    if a[1].as_ref().is_some_and(Value::is_true) {
        flags |= OpenFlags::SQLITE_OPEN_URI;
    }
    match ul_sqlite::cli::open_conn(&path, flags) {
        Ok(c) => Ok(Value::Ext(Rc::new(SqliteConn { conn: RefCell::new(Some(c)) }))),
        Err(msg) => Ok(err("OperationalError", msg)),
    }
}

pub fn build(_vm: &mut Vm) -> Rc<ModuleObj> {
    ModuleBuilder::new("_sqlite3")
        .func("connect", connect)
        .value("sqlite_version", Value::str(rusqlite::version().to_string()))
        .build()
}
