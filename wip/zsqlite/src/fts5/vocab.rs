//! `fts5_vocab.c`: o módulo virtual `fts5vocab`, que expõe o vocabulário de uma tabela FTS5
//! existente. Há três tipos de tabela:
//!
//! * `col`: `CREATE TABLE vocab(term, col, doc, cnt, PRIMARY KEY(term, col))`. Uma linha por
//!   combinação termo/coluna; `doc` é o número de linhas FTS5 com pelo menos uma instância do
//!   termo na coluna e `cnt` o total de instâncias do termo na coluna.
//! * `row`: `CREATE TABLE vocab(term, doc, cnt, PRIMARY KEY(term))`. Uma linha por termo.
//! * `instance`: `CREATE TABLE vocab(term, doc, col, offset, PRIMARY KEY(<todos os campos>))`.
//!   Uma linha por instância de termo.
//!
//! Desvios do C, decorrentes do modelo v2:
//!
//! * O cursor guarda o handle da tabela FTS5 (`VTableId`), não um ponteiro: cada operação que
//!   toca o índice retira a instância da tabela FTS5 com [`with_vtab`] ([`with_fts5`]) e recebe a
//!   conexão, o índice e a configuração. A tabela vem de `fts5_table_from_csrid`.
//! * O `Fts5Global` do `pAux` não é usado (a busca por id percorre `db.vtabs`) e `db` é o que cada
//!   método recebe.
//! * O cursor copia de `Fts5Config` o que as colunas leem (`azCol`, `eDetail`) e o tipo da tabela
//!   (`eType`), para o `xColumn` não precisar da tabela FTS5 nem da conexão.

use std::any::Any;
use std::rc::Rc;

use crate::build::text_arg;
use crate::connection::{
    Connection, Context, IndexInfo, ModuleCaps, StmtId, VTableId, Vtab, VtabCursor, VtabModule,
};
use crate::consts::{
    SQLITE_ERROR, SQLITE_INDEX_CONSTRAINT_EQ, SQLITE_INDEX_CONSTRAINT_GE,
    SQLITE_INDEX_CONSTRAINT_GT, SQLITE_INDEX_CONSTRAINT_LE, SQLITE_INDEX_CONSTRAINT_LT, SQLITE_OK,
    SQLITE_ROW,
};
use crate::mem::{Mem, StrDtor};
use crate::prepare::prepare_v2;
use crate::printf::mprintf;
use crate::util::stricmp;
use crate::vdbeapi::{
    column_int64, finalize, result_int, result_int64, result_text, step, text_of,
};
use crate::vtab::{create_module, declare_vtab, with_vtab};

use super::buffer::{fts5_pos2column, fts5_pos2offset, fts5_poslist_next64, Fts5Buffer};
use super::config::fts5_dequote;
use super::index::{Fts5Index, Fts5Iter};
use super::int::{
    Fts5Config, FTS5INDEX_QUERY_NOTOKENDATA, FTS5INDEX_QUERY_SCAN, FTS5_CORRUPT, FTS5_DETAIL_COLUMNS,
    FTS5_DETAIL_FULL, FTS5_DETAIL_NONE,
};
use super::main::{fts5_table_from_csrid, tab_of};

const FTS5_VOCAB_COL: i32 = 0;
const FTS5_VOCAB_ROW: i32 = 1;
const FTS5_VOCAB_INSTANCE: i32 = 2;

/// Os bits de `idxNum` do `xBestIndex` para o `xFilter`.
const FTS5_VOCAB_TERM_EQ: i32 = 0x01;
const FTS5_VOCAB_TERM_GE: i32 = 0x02;
const FTS5_VOCAB_TERM_LE: i32 = 0x04;

/// `Fts5VocabTable`.
struct Fts5VocabTable {
    /// `base.zErrMsg`.
    z_err_msg: Option<Vec<u8>>,
    /// `zFts5Tbl`: o nome da tabela FTS5.
    z_fts5_tbl: Vec<u8>,
    /// `zFts5Db`: o banco que a contém.
    z_fts5_db: Vec<u8>,
    /// `eType`: `FTS5_VOCAB_COL`, `ROW` ou `INSTANCE`.
    e_type: i32,
    /// `bBusy`.
    b_busy: bool,
}

/// `Fts5VocabCursor`.
struct Fts5VocabCursor {
    /// `pStmt`: o comando que mantém o cursor `*id` da tabela FTS5 aberto.
    p_stmt: Option<StmtId>,
    /// `pFts5`: a tabela FTS5.
    vid: VTableId,
    /// `eType` da tabela do cursor.
    e_type: i32,
    /// `pConfig->azCol` e `pConfig->eDetail` da tabela FTS5.
    az_col: Vec<Vec<u8>>,
    e_detail: i32,

    /// `bEof`.
    b_eof: bool,
    /// `pIter`.
    p_iter: Option<Fts5Iter>,
    /// `pStruct`: a identidade da estrutura do índice.
    p_struct: u64,

    /// `nLeTerm` (negativo: sem limite) e `zLeTerm`.
    n_le_term: i32,
    z_le_term: Option<Vec<u8>>,

    /* Só nas tabelas `col` */
    /// `iCol`.
    i_col: i32,
    /// `aCnt`.
    a_cnt: Vec<i64>,
    /// `aDoc`.
    a_doc: Vec<i64>,

    /// `rowid`: o rowid desta tabela.
    rowid: i64,
    /// `term`: o valor da coluna `term`.
    term: Fts5Buffer,

    /* Só nas tabelas `instance` */
    /// `iInstPos`.
    i_inst_pos: i64,
    /// `iInstOff`.
    i_inst_off: i32,
}

/// Roda `f` com a conexão, o índice e a configuração da tabela FTS5 `vid`.
fn with_fts5<R>(
    db: &mut Connection,
    vid: VTableId,
    f: impl FnOnce(&mut Connection, &mut Fts5Index, &mut Fts5Config) -> R,
) -> Option<R> {
    with_vtab(db, vid, |db, v| {
        let t = tab_of(v);
        f(db, &mut t.index, &mut t.config)
    })
}

/// `fts5VocabTableType`: o tipo da tabela a partir do texto do argumento.
fn fts5_vocab_table_type(z_type: &[u8], pz_err: &mut Option<Vec<u8>>) -> Result<i32, i32> {
    let mut z_copy = z_type.to_vec();
    fts5_dequote(&mut z_copy);
    if stricmp(Some(z_copy.as_slice()), Some(b"col".as_slice())) == 0 {
        Ok(FTS5_VOCAB_COL)
    } else if stricmp(Some(z_copy.as_slice()), Some(b"row".as_slice())) == 0 {
        Ok(FTS5_VOCAB_ROW)
    } else if stricmp(Some(z_copy.as_slice()), Some(b"instance".as_slice())) == 0 {
        Ok(FTS5_VOCAB_INSTANCE)
    } else {
        *pz_err = mprintf(b"fts5vocab: unknown table type: %Q", &[text_arg(&z_copy)]);
        Err(SQLITE_ERROR)
    }
}

/// `fts5VocabInitVtab`: `xCreate` e `xConnect`. O `argv` é `[módulo, banco, tabela, tabela FTS5,
/// tipo]` ou, só no esquema TEMP, `[módulo, "temp", tabela, banco da FTS5, tabela FTS5, tipo]`.
fn fts5_vocab_init_vtab(
    db: &mut Connection,
    argv: &[Vec<u8>],
    pz_err: &mut Option<Vec<u8>>,
) -> Result<Box<dyn Vtab>, i32> {
    const AZ_SCHEMA: [&[u8]; 3] = [
        b"CREATE TABlE vocab(term, col, doc, cnt)",
        b"CREATE TABlE vocab(term, doc, cnt)",
        b"CREATE TABlE vocab(term, doc, col, offset)",
    ];
    let argc = argv.len();
    let b_db = argc == 6 && argv[1].as_slice() == b"temp";

    if argc != 5 && !b_db {
        *pz_err = mprintf(b"wrong number of vtable arguments", &[]);
        return Err(SQLITE_ERROR);
    }

    let z_db = if b_db { &argv[3] } else { &argv[1] };
    let z_tab = if b_db { &argv[4] } else { &argv[3] };
    let z_type = if b_db { &argv[5] } else { &argv[4] };

    let e_type = fts5_vocab_table_type(z_type, pz_err)?;
    debug_assert!(e_type >= 0 && (e_type as usize) < AZ_SCHEMA.len());
    let rc = declare_vtab(db, AZ_SCHEMA[e_type as usize]);
    if rc != SQLITE_OK {
        return Err(rc);
    }

    let mut z_fts5_tbl = z_tab.clone();
    let mut z_fts5_db = z_db.clone();
    fts5_dequote(&mut z_fts5_tbl);
    fts5_dequote(&mut z_fts5_db);
    Ok(Box::new(Fts5VocabTable { z_err_msg: None, z_fts5_tbl, z_fts5_db, e_type, b_busy: false }))
}

impl Vtab for Fts5VocabTable {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn err_msg_mut(&mut self) -> &mut Option<Vec<u8>> {
        &mut self.z_err_msg
    }

    /// `fts5VocabBestIndexMethod`: só `term <=`, `term ==` e `term >=` são interpretados (`<` e
    /// `<=` são tratados igual, `>` e `>=` também).
    fn best_index(&mut self, _db: &mut Connection, p_info: &mut IndexInfo) -> i32 {
        let mut i_term_eq: Option<usize> = None;
        let mut i_term_ge: Option<usize> = None;
        let mut i_term_le: Option<usize> = None;
        let mut idx_num = 0;
        let mut n_arg = 0;

        let n_constraint = p_info.a_constraint.len();
        if p_info.a_constraint_usage.len() < n_constraint {
            p_info.a_constraint_usage.resize(n_constraint, Default::default());
        }

        for (i, p) in p_info.a_constraint.iter().enumerate() {
            if !p.usable {
                continue;
            }
            if p.i_column == 0 {
                /* a coluna `term` */
                let op = p.op as i32;
                if op == SQLITE_INDEX_CONSTRAINT_EQ {
                    i_term_eq = Some(i);
                }
                if op == SQLITE_INDEX_CONSTRAINT_LE {
                    i_term_le = Some(i);
                }
                if op == SQLITE_INDEX_CONSTRAINT_LT {
                    i_term_le = Some(i);
                }
                if op == SQLITE_INDEX_CONSTRAINT_GE {
                    i_term_ge = Some(i);
                }
                if op == SQLITE_INDEX_CONSTRAINT_GT {
                    i_term_ge = Some(i);
                }
            }
        }

        if let Some(i) = i_term_eq {
            idx_num |= FTS5_VOCAB_TERM_EQ;
            n_arg += 1;
            p_info.a_constraint_usage[i].argv_index = n_arg;
            p_info.estimated_cost = 100.0;
        } else {
            p_info.estimated_cost = 1000000.0;
            if let Some(i) = i_term_ge {
                idx_num |= FTS5_VOCAB_TERM_GE;
                n_arg += 1;
                p_info.a_constraint_usage[i].argv_index = n_arg;
                p_info.estimated_cost /= 2.0;
            }
            if let Some(i) = i_term_le {
                idx_num |= FTS5_VOCAB_TERM_LE;
                n_arg += 1;
                p_info.a_constraint_usage[i].argv_index = n_arg;
                p_info.estimated_cost /= 2.0;
            }
        }

        /* Esta tabela sempre entrega os resultados em ordem crescente de `term` (coluna 0). Se o
        ** usuário pediu isso, `orderByConsumed` avisa o núcleo que a saída já está ordenada. */
        if p_info.a_order_by.len() == 1 && p_info.a_order_by[0].i_column == 0 && !p_info.a_order_by[0].desc {
            p_info.order_by_consumed = 1;
        }

        p_info.idx_num = idx_num;
        SQLITE_OK
    }

    /// `fts5VocabDisconnectMethod`.
    fn disconnect(&mut self, _db: &mut Connection) -> i32 {
        SQLITE_OK
    }

    /// `fts5VocabDestroyMethod`.
    fn destroy(&mut self, _db: &mut Connection) -> i32 {
        SQLITE_OK
    }

    /// `fts5VocabOpenMethod`.
    fn open(&mut self, db: &mut Connection) -> Result<Box<dyn VtabCursor>, i32> {
        if self.b_busy {
            self.z_err_msg = mprintf(
                b"recursive definition for %s.%s",
                &[text_arg(&self.z_fts5_db), text_arg(&self.z_fts5_tbl)],
            );
            return Err(SQLITE_ERROR);
        }
        let mut rc = SQLITE_OK;
        let mut p_stmt: Option<StmtId> = None;
        match mprintf(
            b"SELECT t.%Q FROM %Q.%Q AS t WHERE t.%Q MATCH '*id'",
            &[
                text_arg(&self.z_fts5_tbl),
                text_arg(&self.z_fts5_db),
                text_arg(&self.z_fts5_tbl),
                text_arg(&self.z_fts5_tbl),
            ],
        ) {
            None => rc = crate::consts::SQLITE_NOMEM,
            Some(z_sql) => {
                let (rc2, stmt, _tail) = prepare_v2(db, &z_sql, -1);
                rc = rc2;
                p_stmt = stmt;
            }
        }
        debug_assert!(rc == SQLITE_OK || p_stmt.is_none());
        if rc == SQLITE_ERROR {
            rc = SQLITE_OK;
        }

        self.b_busy = true;
        let mut p_fts5: Option<VTableId> = None;
        if let Some(s) = p_stmt {
            if step(db, s) == SQLITE_ROW {
                let i_id = column_int64(db, s, 0);
                p_fts5 = fts5_table_from_csrid(db, i_id);
            }
        }
        self.b_busy = false;

        let mut info: Option<(Vec<Vec<u8>>, i32)> = None;
        if rc == SQLITE_OK {
            match p_fts5 {
                None => {
                    rc = match p_stmt.take() {
                        Some(s) => finalize(db, s),
                        None => SQLITE_OK,
                    };
                    if rc == SQLITE_OK {
                        self.z_err_msg = mprintf(
                            b"no such fts5 table: %s.%s",
                            &[text_arg(&self.z_fts5_db), text_arg(&self.z_fts5_tbl)],
                        );
                        rc = SQLITE_ERROR;
                    }
                }
                Some(vid) => {
                    /* `sqlite3Fts5FlushToDisk` e a leitura do que as colunas usam. */
                    let r = with_vtab(db, vid, |db, v| {
                        let t = tab_of(v);
                        let rc = t.flush_to_disk(db);
                        (rc, t.config.az_col.clone(), t.config.e_detail)
                    });
                    match r {
                        Some((rc2, az_col, e_detail)) => {
                            rc = rc2;
                            info = Some((az_col, e_detail));
                        }
                        None => rc = SQLITE_ERROR,
                    }
                }
            }
        }

        match (rc, p_fts5, info) {
            (SQLITE_OK, Some(vid), Some((az_col, e_detail))) => {
                let n_col = az_col.len();
                Ok(Box::new(Fts5VocabCursor {
                    p_stmt,
                    vid,
                    e_type: self.e_type,
                    az_col,
                    e_detail,
                    b_eof: false,
                    p_iter: None,
                    p_struct: 0,
                    n_le_term: -1,
                    z_le_term: None,
                    i_col: 0,
                    a_cnt: vec![0; n_col],
                    a_doc: vec![0; n_col],
                    rowid: 0,
                    term: Fts5Buffer::new(),
                    i_inst_pos: 0,
                    i_inst_off: 0,
                }))
            }
            _ => {
                if let Some(s) = p_stmt {
                    finalize(db, s);
                }
                Err(if rc == SQLITE_OK { SQLITE_ERROR } else { rc })
            }
        }
    }
}

/// `fts5VocabResetCursor` com o índice à mão.
fn reset_cursor_inner(db: &mut Connection, idx: &mut Fts5Index, csr: &mut Fts5VocabCursor) {
    csr.rowid = 0;
    if let Some(it) = csr.p_iter.take() {
        it.close(idx, db);
    }
    csr.p_struct = 0;
    csr.n_le_term = -1;
    csr.z_le_term = None;
    csr.b_eof = false;
}

/// Compara `z_le_term` com `z_term` como o C (`memcmp` dos primeiros bytes e depois o tamanho):
/// verdadeiro se o termo passou do limite superior.
fn past_le_term(csr: &Fts5VocabCursor, z_term: &[u8]) -> bool {
    if csr.n_le_term >= 0 {
        let z_le = csr.z_le_term.as_deref().unwrap_or(&[]);
        let n_cmp = z_term.len().min(csr.n_le_term as usize).min(z_le.len());
        let b_cmp = z_le[..n_cmp].cmp(&z_term[..n_cmp]);
        if b_cmp == std::cmp::Ordering::Less
            || (b_cmp == std::cmp::Ordering::Equal && (csr.n_le_term as usize) < z_term.len())
        {
            return true;
        }
    }
    false
}

/// `fts5VocabInstanceNewTerm`.
fn instance_new_term(csr: &mut Fts5VocabCursor) -> i32 {
    let eof = csr.p_iter.as_ref().map_or(true, |it| it.eof());
    if eof {
        csr.b_eof = true;
    } else {
        let z_term: Vec<u8> = csr.p_iter.as_ref().map(|it| it.term().to_vec()).unwrap_or_default();
        if past_le_term(csr, &z_term) {
            csr.b_eof = true;
        }
        csr.term.set(&z_term);
    }
    SQLITE_OK
}

/// `fts5VocabInstanceNext`.
fn instance_next(
    db: &mut Connection,
    idx: &mut Fts5Index,
    cfg: &Fts5Config,
    csr: &mut Fts5VocabCursor,
) -> i32 {
    let e_detail = cfg.e_detail;
    let mut rc = SQLITE_OK;

    debug_assert!(csr.p_iter.as_ref().map_or(false, |it| !it.eof()));
    debug_assert!(!csr.b_eof);
    loop {
        let more = e_detail == FTS5_DETAIL_NONE
            || match csr.p_iter.as_ref() {
                Some(it) => fts5_poslist_next64(it.data(), &mut csr.i_inst_off, &mut csr.i_inst_pos) != 0,
                None => true,
            };
        if !more {
            break;
        }
        csr.i_inst_pos = 0;
        csr.i_inst_off = 0;

        rc = match csr.p_iter.as_mut() {
            Some(it) => it.next_scan(idx, db, cfg),
            None => SQLITE_ERROR,
        };
        if rc == SQLITE_OK {
            rc = instance_new_term(csr);
            if csr.b_eof || e_detail == FTS5_DETAIL_NONE {
                break;
            }
        }
        if rc != SQLITE_OK {
            csr.b_eof = true;
            break;
        }
    }
    rc
}

/// `fts5VocabNextMethod`: avança para a próxima linha da tabela.
fn vocab_next(
    db: &mut Connection,
    idx: &mut Fts5Index,
    cfg: &mut Fts5Config,
    csr: &mut Fts5VocabCursor,
) -> i32 {
    let n_col = cfg.n_col();

    let mut rc = idx.structure_test(csr.p_struct);
    if rc != SQLITE_OK {
        return rc;
    }
    csr.rowid += 1;

    if csr.e_type == FTS5_VOCAB_INSTANCE {
        return instance_next(db, idx, cfg, csr);
    }

    if csr.e_type == FTS5_VOCAB_COL {
        csr.i_col += 1;
        while csr.i_col < n_col {
            if csr.a_doc[csr.i_col as usize] != 0 {
                break;
            }
            csr.i_col += 1;
        }
    }

    if csr.e_type != FTS5_VOCAB_COL || csr.i_col >= n_col {
        let eof = csr.p_iter.as_ref().map_or(true, |it| it.eof());
        if eof {
            csr.b_eof = true;
        } else {
            let z_term: Vec<u8> =
                csr.p_iter.as_ref().map(|it| it.term().to_vec()).unwrap_or_default();
            if past_le_term(csr, &z_term) {
                csr.b_eof = true;
                return SQLITE_OK;
            }

            csr.term.set(&z_term);
            for x in csr.a_cnt.iter_mut() {
                *x = 0;
            }
            for x in csr.a_doc.iter_mut() {
                *x = 0;
            }
            csr.i_col = 0;

            debug_assert!(csr.e_type == FTS5_VOCAB_COL || csr.e_type == FTS5_VOCAB_ROW);
            while rc == SQLITE_OK {
                let e_detail = cfg.e_detail;
                /* A poslist do termo corrente (cópia: `next_scan` altera o iterador). */
                let p_pos: Vec<u8> =
                    csr.p_iter.as_ref().map(|it| it.data().to_vec()).unwrap_or_default();
                let mut i_pos: i64 = 0; /* posição de 64 bits lida da poslist */
                let mut i_off: i32 = 0; /* deslocamento corrente na poslist */

                match csr.e_type {
                    FTS5_VOCAB_ROW => {
                        if e_detail == FTS5_DETAIL_FULL {
                            while 0 == fts5_poslist_next64(&p_pos, &mut i_off, &mut i_pos) {
                                csr.a_cnt[0] += 1;
                            }
                        }
                        csr.a_doc[0] += 1;
                    }
                    FTS5_VOCAB_COL => {
                        if e_detail == FTS5_DETAIL_FULL {
                            let mut i_col: i32 = -1;
                            while 0 == fts5_poslist_next64(&p_pos, &mut i_off, &mut i_pos) {
                                let ii = fts5_pos2column(i_pos);
                                if i_col != ii {
                                    if ii < 0 || ii >= n_col {
                                        rc = FTS5_CORRUPT;
                                        break;
                                    }
                                    csr.a_doc[ii as usize] += 1;
                                    i_col = ii;
                                }
                                csr.a_cnt[ii as usize] += 1;
                            }
                        } else if e_detail == FTS5_DETAIL_COLUMNS {
                            while 0 == fts5_poslist_next64(&p_pos, &mut i_off, &mut i_pos) {
                                if i_pos < 0 || i_pos >= n_col as i64 {
                                    rc = FTS5_CORRUPT;
                                    break;
                                }
                                csr.a_doc[i_pos as usize] += 1;
                            }
                        } else {
                            debug_assert!(e_detail == FTS5_DETAIL_NONE);
                            csr.a_doc[0] += 1;
                        }
                    }
                    _ => {
                        debug_assert!(csr.e_type == FTS5_VOCAB_INSTANCE);
                    }
                }

                if rc == SQLITE_OK {
                    rc = match csr.p_iter.as_mut() {
                        Some(it) => it.next_scan(idx, db, cfg),
                        None => SQLITE_ERROR,
                    };
                }
                if csr.e_type == FTS5_VOCAB_INSTANCE {
                    break;
                }

                if rc == SQLITE_OK {
                    let (changed, eof) = match csr.p_iter.as_ref() {
                        Some(it) => {
                            let z = it.term();
                            (z != csr.term.p.as_slice(), it.eof())
                        }
                        None => (true, true),
                    };
                    if changed || eof {
                        break;
                    }
                }
            }
        }
    }

    if rc == SQLITE_OK && !csr.b_eof && csr.e_type == FTS5_VOCAB_COL {
        while csr.i_col < n_col && csr.a_doc[csr.i_col as usize] == 0 {
            csr.i_col += 1;
        }
        if csr.i_col == n_col {
            rc = FTS5_CORRUPT;
        }
    }
    rc
}

/// `fts5VocabFilterMethod`.
fn vocab_filter(
    db: &mut Connection,
    idx: &mut Fts5Index,
    cfg: &mut Fts5Config,
    csr: &mut Fts5VocabCursor,
    idx_num: i32,
    ap_val: &[Mem],
) -> i32 {
    let e_type = csr.e_type;
    let mut rc = SQLITE_OK;

    let mut i_val = 0usize;
    let mut f = FTS5INDEX_QUERY_SCAN;
    let mut z_term: Option<Vec<u8>> = None;

    let mut p_eq: Option<&Mem> = None;
    let mut p_ge: Option<&Mem> = None;
    let mut p_le: Option<&Mem> = None;

    reset_cursor_inner(db, idx, csr);
    if idx_num & FTS5_VOCAB_TERM_EQ != 0 {
        p_eq = ap_val.get(i_val);
        i_val += 1;
    }
    if idx_num & FTS5_VOCAB_TERM_GE != 0 {
        p_ge = ap_val.get(i_val);
        i_val += 1;
    }
    if idx_num & FTS5_VOCAB_TERM_LE != 0 {
        p_le = ap_val.get(i_val);
    }

    if let Some(eq) = p_eq {
        z_term = text_of(eq).map(|t| t.into_owned());
        f = FTS5INDEX_QUERY_NOTOKENDATA;
    } else {
        if let Some(ge) = p_ge {
            z_term = text_of(ge).map(|t| t.into_owned());
        }
        if let Some(le) = p_le {
            let z_copy: Vec<u8> = text_of(le).map(|t| t.into_owned()).unwrap_or_default();
            csr.n_le_term = z_copy.len() as i32;
            csr.z_le_term = Some(z_copy);
        }
    }

    if rc == SQLITE_OK {
        match idx.query(db, cfg, z_term.as_deref().unwrap_or(&[]), f, None) {
            Ok(it) => {
                csr.p_iter = Some(it);
                csr.p_struct = idx.structure_ref();
            }
            Err(e) => rc = e,
        }
    }
    if rc == SQLITE_OK && e_type == FTS5_VOCAB_INSTANCE {
        rc = instance_new_term(csr);
    }
    if rc == SQLITE_OK && !csr.b_eof && (e_type != FTS5_VOCAB_INSTANCE || cfg.e_detail != FTS5_DETAIL_NONE) {
        rc = vocab_next(db, idx, cfg, csr);
    }

    rc
}

impl VtabCursor for Fts5VocabCursor {
    /// `fts5VocabCloseMethod`.
    fn close(&mut self, db: &mut Connection, _vtab: &mut dyn Vtab) -> i32 {
        let vid = self.vid;
        if self.p_iter.is_some() {
            with_fts5(db, vid, |db, idx, _cfg| reset_cursor_inner(db, idx, self));
        }
        self.term.free();
        if let Some(s) = self.p_stmt.take() {
            finalize(db, s);
        }
        SQLITE_OK
    }

    /// `fts5VocabFilterMethod`.
    fn filter(
        &mut self,
        db: &mut Connection,
        _vtab: &mut dyn Vtab,
        idx_num: i32,
        _idx_str: Option<&[u8]>,
        argv: &[Mem],
    ) -> i32 {
        let vid = self.vid;
        with_fts5(db, vid, |db, idx, cfg| vocab_filter(db, idx, cfg, self, idx_num, argv))
            .unwrap_or(SQLITE_ERROR)
    }

    /// `fts5VocabNextMethod`.
    fn next(&mut self, db: &mut Connection, _vtab: &mut dyn Vtab) -> i32 {
        let vid = self.vid;
        with_fts5(db, vid, |db, idx, cfg| vocab_next(db, idx, cfg, self)).unwrap_or(SQLITE_ERROR)
    }

    /// `fts5VocabEofMethod`.
    fn eof(&mut self, _vtab: &mut dyn Vtab) -> i32 {
        self.b_eof as i32
    }

    /// `fts5VocabColumnMethod`.
    fn column(&mut self, _vtab: &mut dyn Vtab, ctx: &mut Context<'_>, i_col: i32) -> i32 {
        let e_detail = self.e_detail;
        let e_type = self.e_type;
        let mut i_val: i64 = 0;

        if i_col == 0 {
            result_text(ctx, Some(&self.term.p), self.term.n(), StrDtor::Transient);
        } else if e_type == FTS5_VOCAB_COL {
            debug_assert!(i_col == 1 || i_col == 2 || i_col == 3);
            if i_col == 1 {
                if e_detail != FTS5_DETAIL_NONE {
                    if let Some(z) = self.az_col.get(self.i_col as usize) {
                        result_text(ctx, Some(z), z.len() as i32, StrDtor::Transient);
                    }
                }
            } else if i_col == 2 {
                i_val = self.a_doc[self.i_col as usize];
            } else {
                i_val = self.a_cnt[self.i_col as usize];
            }
        } else if e_type == FTS5_VOCAB_ROW {
            debug_assert!(i_col == 1 || i_col == 2);
            if i_col == 1 {
                i_val = self.a_doc[0];
            } else {
                i_val = self.a_cnt[0];
            }
        } else {
            debug_assert!(e_type == FTS5_VOCAB_INSTANCE);
            match i_col {
                1 => {
                    if let Some(it) = self.p_iter.as_ref() {
                        result_int64(ctx, it.rowid());
                    }
                }
                2 => {
                    let mut ii: i32 = -1;
                    if e_detail == FTS5_DETAIL_FULL {
                        ii = fts5_pos2column(self.i_inst_pos);
                    } else if e_detail == FTS5_DETAIL_COLUMNS {
                        ii = self.i_inst_pos as i32;
                    }
                    if ii >= 0 && (ii as usize) < self.az_col.len() {
                        let z = &self.az_col[ii as usize];
                        result_text(ctx, Some(z), z.len() as i32, StrDtor::Transient);
                    }
                }
                _ => {
                    debug_assert!(i_col == 3);
                    if e_detail == FTS5_DETAIL_FULL {
                        let ii = fts5_pos2offset(self.i_inst_pos);
                        result_int(ctx, ii);
                    }
                }
            }
        }

        if i_val > 0 {
            result_int64(ctx, i_val);
        }
        SQLITE_OK
    }

    /// `fts5VocabRowidMethod`.
    fn rowid(&mut self, _vtab: &mut dyn Vtab, p_rowid: &mut i64) -> i32 {
        *p_rowid = self.rowid;
        SQLITE_OK
    }
}

/// `fts5Vocab`: o módulo `fts5vocab`. O `xCreate` e o `xConnect` são funções distintas no C (que
/// fazem o mesmo), então a tabela não é eponímia.
struct Fts5VocabModule;

impl VtabModule for Fts5VocabModule {
    fn i_version(&self) -> i32 {
        2
    }

    fn caps(&self) -> ModuleCaps {
        ModuleCaps { create: true, ..ModuleCaps::default() }
    }

    /// `fts5VocabCreateMethod`.
    fn x_create(
        &self,
        db: &mut Connection,
        _aux: &Option<Rc<dyn Any>>,
        argv: &[Vec<u8>],
        err: &mut Option<Vec<u8>>,
    ) -> Result<Box<dyn Vtab>, i32> {
        fts5_vocab_init_vtab(db, argv, err)
    }

    /// `fts5VocabConnectMethod`.
    fn x_connect(
        &self,
        db: &mut Connection,
        _aux: &Option<Rc<dyn Any>>,
        argv: &[Vec<u8>],
        err: &mut Option<Vec<u8>>,
    ) -> Result<Box<dyn Vtab>, i32> {
        fts5_vocab_init_vtab(db, argv, err)
    }
}

/// `sqlite3Fts5VocabInit`: registra o módulo `fts5vocab`. `p_global` é o `Fts5Global` do `pAux`.
pub fn fts5_vocab_init(db: &mut Connection, p_global: Rc<dyn Any>) -> i32 {
    create_module(db, b"fts5vocab", Some(Rc::new(Fts5VocabModule)), Some(p_global), None)
}
