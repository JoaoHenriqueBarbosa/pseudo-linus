//! `ext/misc/stmt.c`: a tabela virtual epônima `sqlite_stmt`, que lista os comandos preparados da
//! conexão (`SQLITE_ENABLE_STMTVTAB` está ligada no Debian 13).
//!
//! Desvios do C, decorrentes do modelo v2 (CONVENTIONS.md):
//!
//! - O `sqlite3 *db` que `stmt_vtab` e `stmt_cursor` guardam é o `&mut Connection` que cada método
//!   recebe.
//! - A lista encadeada de `StmtRow` é um `Vec` percorrido por índice (`stmtNext` só avança).
//! - O comando em execução (o próprio `SELECT ... FROM sqlite_stmt`) está fora do slab enquanto o
//!   `xFilter` roda (`Connection.stmts.take`, ver `connection.rs`), então `Connection::stmt`
//!   devolve `None` para ele e não há o que ler dele. A linha desse comando não é emitida, mas o
//!   `rowid` dela é contado, para que os demais comandos mantenham o `rowid` que têm no C.

use std::any::Any;
use std::rc::Rc;

use crate::connection::{
    Connection, Context, IndexInfo, ModuleCaps, Vtab, VtabCursor, VtabModule,
};
use crate::consts::{
    SQLITE_OK, SQLITE_STMTSTATUS_AUTOINDEX, SQLITE_STMTSTATUS_FULLSCAN_STEP,
    SQLITE_STMTSTATUS_MEMUSED, SQLITE_STMTSTATUS_REPREPARE, SQLITE_STMTSTATUS_RUN,
    SQLITE_STMTSTATUS_SORT, SQLITE_STMTSTATUS_VM_STEP,
};
use crate::mem::{Mem, StrDtor};
use crate::vdbeapi::{
    column_count, next_stmt, result_int, result_text, sql, stmt_busy, stmt_readonly, stmt_status,
};
use crate::vtab::{create_module, declare_vtab};

/// `STMT_NUM_INTEGER_COLUMN`.
const STMT_NUM_INTEGER_COLUMN: usize = 10;

// Os números das colunas.
const STMT_COLUMN_SQL: i32 = 0; // SQL do comando
const STMT_COLUMN_NCOL: usize = 1; // Número de colunas do resultado
const STMT_COLUMN_RO: usize = 2; // Verdadeiro se somente leitura
const STMT_COLUMN_BUSY: usize = 3; // Verdadeiro se ocupado agora
const STMT_COLUMN_NSCAN: usize = 4; // SQLITE_STMTSTATUS_FULLSCAN_STEP
const STMT_COLUMN_NSORT: usize = 5; // SQLITE_STMTSTATUS_SORT
const STMT_COLUMN_NAIDX: usize = 6; // SQLITE_STMTSTATUS_AUTOINDEX
const STMT_COLUMN_NSTEP: usize = 7; // SQLITE_STMTSTATUS_VM_STEP
const STMT_COLUMN_REPREP: usize = 8; // SQLITE_STMTSTATUS_REPREPARE
const STMT_COLUMN_RUN: usize = 9; // SQLITE_STMTSTATUS_RUN
const STMT_COLUMN_MEM: usize = 10; // SQLITE_STMTSTATUS_MEMUSED

/// `struct StmtRow`: uma linha do resultado.
struct StmtRow {
    /// `iRowid`.
    i_rowid: i64,
    /// Coluna `sql`.
    z_sql: Option<Vec<u8>>,
    /// `aCol`: as demais colunas.
    a_col: [i32; STMT_NUM_INTEGER_COLUMN + 1],
}

/// `struct stmt_vtab`.
struct StmtVtab {
    /// `zErrMsg`.
    z_err_msg: Option<Vec<u8>>,
}

/// `struct stmt_cursor`: as linhas já colhidas pelo `xFilter` e a posição corrente.
#[derive(Default)]
struct StmtCursor {
    /// As linhas, na ordem de `sqlite3_next_stmt()`.
    rows: Vec<StmtRow>,
    /// `pRow`: a linha corrente (o fim é `rows.len()`).
    pos: usize,
}

impl Vtab for StmtVtab {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn err_msg_mut(&mut self) -> &mut Option<Vec<u8>> {
        &mut self.z_err_msg
    }

    /// `stmtBestIndex`: o custo é fixo.
    fn best_index(&mut self, _db: &mut Connection, info: &mut IndexInfo) -> i32 {
        info.estimated_cost = 500.0;
        info.estimated_rows = 500;
        SQLITE_OK
    }

    /// `stmtDisconnect`.
    fn disconnect(&mut self, _db: &mut Connection) -> i32 {
        SQLITE_OK
    }

    /// `xDestroy` é nulo no módulo (a tabela é epônima e não se destrói).
    fn destroy(&mut self, _db: &mut Connection) -> i32 {
        SQLITE_OK
    }

    /// `stmtOpen`.
    fn open(&mut self, _db: &mut Connection) -> Result<Box<dyn VtabCursor>, i32> {
        Ok(Box::new(StmtCursor::default()))
    }
}

impl VtabCursor for StmtCursor {
    /// `stmtClose`: as linhas somem com o cursor.
    fn close(&mut self, _db: &mut Connection, _vtab: &mut dyn Vtab) -> i32 {
        self.rows.clear();
        SQLITE_OK
    }

    /// `stmtFilter`: recomeça no primeiro comando preparado, colhendo uma linha por comando.
    fn filter(
        &mut self,
        db: &mut Connection,
        _vtab: &mut dyn Vtab,
        _idx_num: i32,
        _idx_str: Option<&[u8]>,
        _argv: &[Mem],
    ) -> i32 {
        self.rows.clear();
        self.pos = 0;
        let mut i_rowid: i64 = 1;
        let mut cur = next_stmt(db, None);
        while let Some(p) = cur {
            let row_id = i_rowid;
            i_rowid += 1;
            if db.stmt(p).is_some() {
                let z_sql = sql(db, p).map(|z| {
                    // `strlen(zSql)`: o SQL vale até o primeiro NUL.
                    let end = z.iter().position(|&b| b == 0).unwrap_or(z.len());
                    z[..end].to_vec()
                });
                let mut a_col = [0i32; STMT_NUM_INTEGER_COLUMN + 1];
                a_col[STMT_COLUMN_NCOL] = column_count(db, p);
                a_col[STMT_COLUMN_RO] = stmt_readonly(db, p) as i32;
                a_col[STMT_COLUMN_BUSY] = stmt_busy(db, p) as i32;
                let counters = [
                    (STMT_COLUMN_NSCAN, SQLITE_STMTSTATUS_FULLSCAN_STEP),
                    (STMT_COLUMN_NSORT, SQLITE_STMTSTATUS_SORT),
                    (STMT_COLUMN_NAIDX, SQLITE_STMTSTATUS_AUTOINDEX),
                    (STMT_COLUMN_NSTEP, SQLITE_STMTSTATUS_VM_STEP),
                    (STMT_COLUMN_REPREP, SQLITE_STMTSTATUS_REPREPARE),
                    (STMT_COLUMN_RUN, SQLITE_STMTSTATUS_RUN),
                    (STMT_COLUMN_MEM, SQLITE_STMTSTATUS_MEMUSED),
                ];
                for (col, op) in counters {
                    a_col[col] = stmt_status(db, p, op, false);
                }
                self.rows.push(StmtRow { i_rowid: row_id, z_sql, a_col });
            }
            cur = next_stmt(db, Some(p));
        }
        SQLITE_OK
    }

    /// `stmtNext`.
    fn next(&mut self, _db: &mut Connection, _vtab: &mut dyn Vtab) -> i32 {
        self.pos += 1;
        SQLITE_OK
    }

    /// `stmtEof`.
    fn eof(&mut self, _vtab: &mut dyn Vtab) -> i32 {
        (self.pos >= self.rows.len()) as i32
    }

    /// `stmtColumn`.
    fn column(&mut self, _vtab: &mut dyn Vtab, ctx: &mut Context<'_>, i: i32) -> i32 {
        let Some(row) = self.rows.get(self.pos) else {
            return SQLITE_OK;
        };
        if i == STMT_COLUMN_SQL {
            result_text(ctx, row.z_sql.as_deref(), -1, StrDtor::Transient);
        } else {
            let v = usize::try_from(i).ok().and_then(|k| row.a_col.get(k)).copied().unwrap_or(0);
            result_int(ctx, v);
        }
        SQLITE_OK
    }

    /// `stmtRowid`: o `rowid` da linha corrente.
    fn rowid(&mut self, _vtab: &mut dyn Vtab, rowid: &mut i64) -> i32 {
        *rowid = self.rows.get(self.pos).map_or(0, |r| r.i_rowid);
        SQLITE_OK
    }
}

/// `stmtModule`: sem `xCreate` (portanto epônimo) e com `iVersion` 0.
struct StmtModule;

impl VtabModule for StmtModule {
    fn i_version(&self) -> i32 {
        0
    }

    fn caps(&self) -> ModuleCaps {
        ModuleCaps::default()
    }

    /// `stmtConnect`: só declara o esquema do resultado.
    fn x_connect(
        &self,
        db: &mut Connection,
        _aux: &Option<Rc<dyn Any>>,
        _argv: &[Vec<u8>],
        _err: &mut Option<Vec<u8>>,
    ) -> Result<Box<dyn Vtab>, i32> {
        let rc = declare_vtab(
            db,
            b"CREATE TABLE x(sql,ncol,ro,busy,nscan,nsort,naidx,nstep,reprep,run,mem)",
        );
        if rc == SQLITE_OK {
            Ok(Box::new(StmtVtab { z_err_msg: None }))
        } else {
            Err(rc)
        }
    }
}

/// `sqlite3StmtVtabInit`: registra o módulo `sqlite_stmt`.
pub fn stmt_vtab_init(db: &mut Connection) -> i32 {
    let module: Rc<dyn VtabModule> = Rc::new(StmtModule);
    create_module(db, b"sqlite_stmt", Some(module), None, None)
}
