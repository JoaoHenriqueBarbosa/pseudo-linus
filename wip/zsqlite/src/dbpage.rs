//! `dbpage.c`: a tabela virtual `sqlite_dbpage`, que lê e grava páginas inteiras do arquivo do
//! banco pelo pager (`SQLITE_ENABLE_DBPAGE_VTAB` está ligada no Debian 13).
//!
//! Desvios do C, decorrentes do modelo v2 (CONVENTIONS.md):
//!
//! - O `sqlite3 *db` da tabela e o `Pager *pPager` do cursor não existem como campos: cada método
//!   recebe `&mut Connection`, e o pager é o de `db.dbs[i_db]` (o `sqlite3BtreePager`), alcançado
//!   por `i_db`, que o cursor já guarda.
//! - `DbPage *` é o handle `PgId`. O dado da página não sobrevive à liberação dela, então o
//!   `xColumn` copia os bytes antes de soltar a referência (o C entrega o ponteiro ao
//!   `sqlite3_result_blob` com `SQLITE_TRANSIENT`, que também copia).

use std::any::Any;
use std::rc::Rc;

use crate::btree::{btree_get_page_size, btree_last_page};
use crate::btree_cursor::btree_begin_trans;
use crate::btree_types::Btree;
use crate::build::find_db_name;
use crate::connection::{
    Connection, Context, IndexInfo, ModuleCaps, Vtab, VtabCursor, VtabModule,
};
use crate::consts::{
    PENDING_BYTE, SQLITE_BLOB, SQLITE_CONSTRAINT, SQLITE_DEFENSIVE, SQLITE_ERROR,
    SQLITE_INDEX_CONSTRAINT_EQ, SQLITE_INDEX_SCAN_UNIQUE, SQLITE_NULL, SQLITE_OK,
    SQLITE_VTAB_DIRECTONLY, SQLITE_VTAB_USES_ALL_SCHEMAS,
};
use crate::mem::{value_type, Mem, StrDtor};
use crate::pcache::PgId;
use crate::printf::{mprintf, PrintfArg};
use crate::vdbeapi::{
    result_blob, result_int, result_text, result_zeroblob, text_of, value_blob, value_bytes,
    value_int,
};
use crate::vdbeaux2::with_bt_db;
use crate::vtab::{create_module, declare_vtab, vtab_config};

// Colunas.
const DBPAGE_COLUMN_PGNO: i32 = 0;
const DBPAGE_COLUMN_DATA: i32 = 1;
const DBPAGE_COLUMN_SCHEMA: i32 = 2;

/// `db->aDb[iDb].pBt`: a árvore do banco `i_db`, ou `None` se o índice é inválido ou o banco não
/// tem árvore aberta.
pub(crate) fn db_btree(db: &mut Connection, i_db: i32) -> Option<&mut Btree> {
    usize::try_from(i_db).ok().and_then(|i| db.dbs.get_mut(i)).and_then(|slot| slot.bt.as_mut())
}

/// `struct DbpageTable`.
struct DbpageTable {
    /// `zErrMsg`.
    z_err_msg: Option<Vec<u8>>,
}

/// `struct DbpageCursor`.
struct DbpageCursor {
    /// Número da página corrente.
    pgno: i32,
    /// Última página a visitar nesta varredura.
    mx_pgno: i32,
    /// Página 1 do banco, com a referência que o cursor guarda.
    p_page1: Option<PgId>,
    /// Índice do banco lido.
    i_db: i32,
    /// Tamanho de cada página em bytes.
    sz_page: i32,
}

impl DbpageCursor {
    /// `sqlite3PagerUnrefPageOne(pCsr->pPage1)`: solta a referência à página 1, se há. O pager é o
    /// do banco `i_db`; sem árvore (anexo desfeito) a referência já não existe.
    fn unref_page1(&mut self, db: &mut Connection) {
        if let Some(pg) = self.p_page1.take() {
            if let Some(bt) = db_btree(db, self.i_db) {
                bt.bt.pager.unref_page_one(pg);
            }
        }
    }
}

impl Vtab for DbpageTable {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn err_msg_mut(&mut self) -> &mut Option<Vec<u8>> {
        &mut self.z_err_msg
    }

    /// `dbpageBestIndex`. O `idxNum`:
    ///
    /// - 0: `schema=main`, varredura completa;
    /// - 1: `schema=main`, `pgno=?1`;
    /// - 2: `schema=?1`, varredura completa;
    /// - 3: `schema=?1`, `pgno=?2`.
    fn best_index(&mut self, _db: &mut Connection, info: &mut IndexInfo) -> i32 {
        let mut i_plan = 0;

        // Se há uma restrição `schema=`, ela precisa ser honrada. Sem como honrá-la, não há
        // solução.
        for i in 0..info.a_constraint.len() {
            let p = info.a_constraint[i];
            if p.i_column != DBPAGE_COLUMN_SCHEMA {
                continue;
            }
            if p.op as i32 != SQLITE_INDEX_CONSTRAINT_EQ {
                continue;
            }
            if !p.usable {
                return SQLITE_CONSTRAINT;
            }
            i_plan = 2;
            info.a_constraint_usage[i].argv_index = 1;
            info.a_constraint_usage[i].omit = true;
            break;
        }

        // Ou não há restrição de esquema (vale `main`) ou ela foi aceita.
        info.estimated_cost = 1.0e6;

        // Restrições sobre `pgno`.
        for i in 0..info.a_constraint.len() {
            let p = info.a_constraint[i];
            if p.usable && p.i_column <= 0 && p.op as i32 == SQLITE_INDEX_CONSTRAINT_EQ {
                info.estimated_rows = 1;
                info.idx_flags = SQLITE_INDEX_SCAN_UNIQUE;
                info.estimated_cost = 1.0;
                info.a_constraint_usage[i].argv_index = if i_plan != 0 { 2 } else { 1 };
                info.a_constraint_usage[i].omit = true;
                i_plan |= 1;
                break;
            }
        }
        info.idx_num = i_plan;

        if let Some(o) = info.a_order_by.first() {
            if o.i_column <= 0 && !o.desc {
                info.order_by_consumed = 1;
            }
        }
        SQLITE_OK
    }

    /// `dbpageDisconnect`.
    fn disconnect(&mut self, _db: &mut Connection) -> i32 {
        SQLITE_OK
    }

    /// `xDestroy` é o mesmo `dbpageDisconnect`.
    fn destroy(&mut self, _db: &mut Connection) -> i32 {
        SQLITE_OK
    }

    /// `dbpageOpen`.
    fn open(&mut self, _db: &mut Connection) -> Result<Box<dyn VtabCursor>, i32> {
        Ok(Box::new(DbpageCursor { pgno: -1, mx_pgno: 0, p_page1: None, i_db: 0, sz_page: 0 }))
    }

    /// `dbpageUpdate`: grava uma página inteira. Não se apaga nem se insere linha.
    fn update(&mut self, db: &mut Connection, args: &[Mem], _rowid: &mut i64) -> i32 {
        match dbpage_update(db, args) {
            Ok(rc) => rc,
            Err(z_err) => {
                self.z_err_msg = mprintf(b"%s", &[PrintfArg::Text(Some(z_err.to_vec()))]);
                SQLITE_ERROR
            }
        }
    }

    /// `dbpageBegin`: como não se sabe de antemão quais arquivos a tabela vai gravar, abre uma
    /// transação de escrita em todos.
    fn begin(&mut self, db: &mut Connection) -> i32 {
        for i in 0..db.dbs.len() {
            let _ = with_bt_db(db, i, |bt, bdb| btree_begin_trans(bt, 1, None, bdb));
        }
        SQLITE_OK
    }
}

/// O corpo de `dbpageUpdate`: `Err` leva a mensagem do `update_fail` (o chamador a grava em
/// `zErrMsg` e devolve `SQLITE_ERROR`); `Ok` é o código de retorno.
fn dbpage_update(db: &mut Connection, args: &[Mem]) -> Result<i32, &'static [u8]> {
    if db.flags & SQLITE_DEFENSIVE != 0 {
        return Err(b"read-only");
    }
    if args.len() == 1 {
        return Err(b"cannot delete");
    }
    let pgno = value_int(&args[0]) as u32;
    if value_type(&args[0]) == SQLITE_NULL || value_int(&args[1]) as u32 != pgno {
        return Err(b"cannot insert");
    }
    let z_schema = text_of(&args[4]);
    let i_db = match z_schema {
        Some(z) => find_db_name(db, Some(&z[..])),
        None => -1,
    };
    if i_db < 0 {
        return Err(b"no such schema");
    }
    let Some(bt) = db_btree(db, i_db) else {
        return Err(b"bad page number");
    };
    if pgno < 1 || pgno > btree_last_page(bt) {
        return Err(b"bad page number");
    }
    let sz_page = btree_get_page_size(bt);
    let mut data = args[3].clone();
    if value_type(&data) != SQLITE_BLOB || value_bytes(&mut data) != sz_page {
        return Err(b"bad page value");
    }
    let pager = &mut bt.bt.pager;
    let rc = match pager.get(pgno, 0) {
        Ok(pg) => {
            let mut rc = SQLITE_OK;
            if let Some(p_data) = value_blob(&mut data) {
                rc = pager.write(pg);
                if rc == SQLITE_OK {
                    let n = sz_page as usize;
                    pager.page_data_mut(pg)[..n].copy_from_slice(&p_data[..n]);
                }
            }
            pager.unref(Some(pg));
            rc
        }
        Err(e) => e,
    };
    Ok(rc)
}

impl VtabCursor for DbpageCursor {
    /// `dbpageClose`.
    fn close(&mut self, db: &mut Connection, _vtab: &mut dyn Vtab) -> i32 {
        self.unref_page1(db);
        SQLITE_OK
    }

    /// `dbpageFilter`. O `idxNum` está descrito em `best_index`.
    fn filter(
        &mut self,
        db: &mut Connection,
        _vtab: &mut dyn Vtab,
        idx_num: i32,
        _idx_str: Option<&[u8]>,
        argv: &[Mem],
    ) -> i32 {
        // O padrão é nenhuma linha de resultado.
        self.pgno = 1;
        self.mx_pgno = 0;

        if idx_num & 2 != 0 {
            debug_assert!(!argv.is_empty());
            let z_schema = text_of(&argv[0]);
            self.i_db = find_db_name(db, z_schema.as_deref());
            if self.i_db < 0 {
                return SQLITE_OK;
            }
        } else {
            self.i_db = 0;
        }
        let Some(bt) = db_btree(db, self.i_db) else {
            return SQLITE_OK;
        };
        self.sz_page = btree_get_page_size(bt);
        self.mx_pgno = btree_last_page(bt) as i32;
        if idx_num & 1 != 0 {
            debug_assert!(argv.len() > (idx_num >> 1) as usize);
            self.pgno = value_int(&argv[(idx_num >> 1) as usize]);
            if self.pgno < 1 || self.pgno > self.mx_pgno {
                self.pgno = 1;
                self.mx_pgno = 0;
            } else {
                self.mx_pgno = self.pgno;
            }
        } else {
            debug_assert!(self.pgno == 1);
        }
        if let Some(pg) = self.p_page1.take() {
            bt.bt.pager.unref_page_one(pg);
        }
        match bt.bt.pager.get(1, 0) {
            Ok(pg) => {
                self.p_page1 = Some(pg);
                SQLITE_OK
            }
            Err(rc) => rc,
        }
    }

    /// `dbpageNext`.
    fn next(&mut self, _db: &mut Connection, _vtab: &mut dyn Vtab) -> i32 {
        self.pgno += 1;
        SQLITE_OK
    }

    /// `dbpageEof`.
    fn eof(&mut self, _vtab: &mut dyn Vtab) -> i32 {
        (self.pgno > self.mx_pgno) as i32
    }

    /// `dbpageColumn`.
    fn column(&mut self, _vtab: &mut dyn Vtab, ctx: &mut Context<'_>, i: i32) -> i32 {
        let mut rc = SQLITE_OK;
        match i {
            DBPAGE_COLUMN_PGNO => result_int(ctx, self.pgno),
            DBPAGE_COLUMN_DATA => {
                if self.pgno as i64 == (PENDING_BYTE / self.sz_page as i64) + 1 {
                    // A página do byte pendente: supõe-se zerada. Pedi-la ao pager é um
                    // `SQLITE_CORRUPT`.
                    result_zeroblob(ctx, self.sz_page);
                } else {
                    let Some(bt) = db_btree(ctx.db, self.i_db) else {
                        return SQLITE_ERROR;
                    };
                    let pager = &mut bt.bt.pager;
                    match pager.get(self.pgno as u32, 0) {
                        Ok(pg) => {
                            let n = self.sz_page as usize;
                            let data = pager.page_data(pg)[..n].to_vec();
                            pager.unref(Some(pg));
                            result_blob(ctx, Some(&data), self.sz_page, StrDtor::Transient);
                        }
                        Err(e) => rc = e,
                    }
                }
            }
            _ => {
                // schema
                let name = usize::try_from(self.i_db)
                    .ok()
                    .and_then(|i| ctx.db.dbs.get(i))
                    .map(|slot| slot.z_db_s_name.clone());
                result_text(ctx, name.as_deref(), -1, StrDtor::Transient);
            }
        }
        rc
    }

    /// `dbpageRowid`.
    fn rowid(&mut self, _vtab: &mut dyn Vtab, rowid: &mut i64) -> i32 {
        *rowid = self.pgno as i64;
        SQLITE_OK
    }
}

/// `dbpage_module`: `xCreate` e `xConnect` são a mesma função, com `iVersion` 0.
struct DbpageModule;

impl VtabModule for DbpageModule {
    fn i_version(&self) -> i32 {
        0
    }

    fn caps(&self) -> ModuleCaps {
        ModuleCaps { create: true, update: true, begin: true, ..ModuleCaps::default() }
    }

    fn create_is_connect(&self) -> bool {
        true
    }

    /// `dbpageConnect`.
    fn x_connect(
        &self,
        db: &mut Connection,
        _aux: &Option<Rc<dyn Any>>,
        _argv: &[Vec<u8>],
        _err: &mut Option<Vec<u8>>,
    ) -> Result<Box<dyn Vtab>, i32> {
        vtab_config(db, SQLITE_VTAB_DIRECTONLY, 0);
        vtab_config(db, SQLITE_VTAB_USES_ALL_SCHEMAS, 0);
        let rc = declare_vtab(db, b"CREATE TABLE x(pgno INTEGER PRIMARY KEY, data BLOB, schema HIDDEN)");
        if rc == SQLITE_OK {
            Ok(Box::new(DbpageTable { z_err_msg: None }))
        } else {
            Err(rc)
        }
    }
}

/// `sqlite3DbpageRegister`: registra o módulo `sqlite_dbpage`.
pub fn dbpage_register(db: &mut Connection) -> i32 {
    let module: Rc<dyn VtabModule> = Rc::new(DbpageModule);
    create_module(db, b"sqlite_dbpage", Some(module), None, None)
}
