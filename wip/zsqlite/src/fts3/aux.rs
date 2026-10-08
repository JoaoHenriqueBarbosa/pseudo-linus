//! `fts3_aux.c`: o módulo virtual `fts4aux`, que expõe o vocabulário de uma tabela FTS3/FTS4. O
//! esquema é `CREATE TABLE x(term, col, documents, occurrences, languageid HIDDEN)`. Cria-se
//! assim:
//!
//! ```text
//!   CREATE VIRTUAL TABLE xxx USING fts4aux(fts4-table);
//!   CREATE VIRTUAL TABLE xxx USING fts4aux(fts4-table-db, fts4-table);
//! ```
//!
//! A tabela não tem representação persistente, então `xCreate` e `xConnect` são a mesma operação
//! (e por isso a tabela epônima existe, embora os argumentos a tornem inútil).
//!
//! # Contrato com `write.rs` (`fts3_write.c`, fatia 2)
//!
//! O cursor itera pelos segmentos com as funções abaixo, todas de `super::write`. O
//! `Fts3Table` e o leitor `Fts3MultiSegReader` são os de `int.rs`; `db` vem na frente porque a
//! tabela abre comandos e blobs na conexão (a `Fts3Table` não guarda o `sqlite3 *`). O que o C
//! recebia só com o leitor (`SegReaderFinish`) também recebe `db`, para fechar o blob de
//! `%_segments` que um leitor pode ter aberto.
//!
//! ```text
//!   fts3_seg_reader_cursor(db, p, i_langid, i_index, i_level, z_term: Option<&[u8]>,
//!                          is_prefix: bool, is_scan: bool, csr) -> i32
//!   fts3_seg_reader_start(db, p, csr, filter: &Fts3SegFilter) -> i32   (copia o filtro)
//!   fts3_seg_reader_step(db, p, csr) -> i32     (SQLITE_ROW, SQLITE_OK no fim, ou erro)
//!   fts3_seg_reader_finish(db, csr)
//!   fts3_segments_close(db, p)
//! ```
//!
//! # Desvios do C
//!
//! * O C aloca a `Fts3Table` embutida na `Fts3auxTable`, com `zDb` e `zName` colados no mesmo bloco,
//!   `db` e `nIndex=1`; aqui é um `Fts3Table::new` com `n_index = 1` (o `aIndex` fica vazio, como no
//!   C, que o deixa nulo).
//! * O `memset` do cursor em `xFilter` (zera `csr`, `filter`, `zStop`, `nStop`, `iLangid`, `isEof`,
//!   `iRowid`, `iCol`, `nStat` e `aStat`) é o [`Fts3auxCursor::reset`].

use std::any::Any;
use std::cmp::Ordering;
use std::rc::Rc;

use crate::connection::{
    Connection, Context, IndexInfo, ModuleCaps, Vtab, VtabCursor, VtabModule,
};
use crate::consts::{
    SQLITE_CORRUPT_VTAB, SQLITE_ERROR, SQLITE_INDEX_CONSTRAINT_EQ, SQLITE_INDEX_CONSTRAINT_GE,
    SQLITE_INDEX_CONSTRAINT_GT, SQLITE_INDEX_CONSTRAINT_LE, SQLITE_INDEX_CONSTRAINT_LT, SQLITE_NOMEM,
    SQLITE_OK, SQLITE_ROW,
};
use crate::mem::{Mem, StrDtor};
use crate::printf::mprintf;
use crate::util::strnicmp;
use crate::vdbeapi::{finalize, result_int, result_int64, result_text, text_of, value_int};
use crate::vtab::{create_module, declare_vtab};

use super::int::{
    fts3_dequote, Fts3MultiSegReader, Fts3SegFilter, Fts3Table, FTS3_SEGCURSOR_ALL,
    FTS3_SEGMENT_IGNORE_EMPTY, FTS3_SEGMENT_REQUIRE_POS, FTS3_SEGMENT_SCAN,
};
use super::varint::fts3_get_varint;
use super::write::{
    fts3_seg_reader_cursor, fts3_seg_reader_finish, fts3_seg_reader_start, fts3_seg_reader_step,
    fts3_segments_close,
};

/// `FTS3_AUX_SCHEMA`: o esquema da tabela de termos.
const FTS3_AUX_SCHEMA: &[u8] = b"CREATE TABLE x(term, col, documents, occurrences, languageid HIDDEN)";

/// `FTS4AUX_EQ_CONSTRAINT`, `FTS4AUX_GE_CONSTRAINT`, `FTS4AUX_LE_CONSTRAINT`: os bits do `idxNum`.
const FTS4AUX_EQ_CONSTRAINT: i32 = 1;
const FTS4AUX_GE_CONSTRAINT: i32 = 2;
const FTS4AUX_LE_CONSTRAINT: i32 = 4;

/// `Fts3auxTable`.
struct Fts3auxTable {
    /// `base.zErrMsg`.
    z_err_msg: Option<Vec<u8>>,
    /// `pFts3Tab`: a tabela de mentira por onde se lê o índice.
    p_fts3_tab: Fts3Table,
}

/// `struct Fts3auxColstats`.
#[derive(Clone, Copy, Default)]
struct Fts3auxColstats {
    /// `nDoc`: o valor de `documents` da linha corrente.
    n_doc: i64,
    /// `nOcc`: o valor de `occurrences` da linha corrente.
    n_occ: i64,
}

/// `Fts3auxCursor`.
#[derive(Default)]
struct Fts3auxCursor {
    /// `csr`.
    csr: Fts3MultiSegReader,
    /// `filter`.
    filter: Fts3SegFilter,
    /// `zStop`/`nStop`: o limite superior do termo (`term <= ?`).
    z_stop: Option<Vec<u8>>,
    /// `iLangid`: o idioma consultado.
    i_langid: i32,
    /// `isEof`.
    is_eof: bool,
    /// `iRowid`: o rowid corrente.
    i_rowid: i64,
    /// `iCol`: o valor corrente da coluna `col`.
    i_col: i32,
    /// `aStat`/`nStat`.
    a_stat: Vec<Fts3auxColstats>,
}

/// A `Fts3auxTable` de uma instância `Vtab`.
fn aux_table(vtab: &mut dyn Vtab) -> &mut Fts3auxTable {
    vtab.as_any_mut().downcast_mut::<Fts3auxTable>().expect("fts4aux: instância de outro módulo")
}

/// `fts3auxConnectMethod`: o trabalho de `xConnect` e de `xCreate`. O `argv` é
/// `[módulo, banco, tabela, tabela FTS]` ou, só no esquema TEMP, `[módulo, "temp", tabela, banco
/// da FTS, tabela FTS]`.
fn fts3aux_connect(
    db: &mut Connection,
    argv: &[Vec<u8>],
    pz_err: &mut Option<Vec<u8>>,
) -> Result<Box<dyn Vtab>, i32> {
    let argc = argv.len();

    /* O usuário escreve numa de duas formas (ver o cabeçalho do módulo). */
    let bad_args = |pz_err: &mut Option<Vec<u8>>| {
        *pz_err = mprintf(b"invalid arguments to fts4aux constructor", &[]);
        Err(SQLITE_ERROR)
    };
    if argc != 4 && argc != 5 {
        return bad_args(pz_err);
    }

    let mut z_db: &[u8] = &argv[1];
    let z_fts3: &[u8];
    if argc == 5 {
        if z_db.len() == 4 && strnicmp(Some(b"temp".as_slice()), Some(z_db), 4) == 0 {
            z_db = &argv[3];
            z_fts3 = &argv[4];
        } else {
            return bad_args(pz_err);
        }
    } else {
        z_fts3 = &argv[3];
    }

    let rc = declare_vtab(db, FTS3_AUX_SCHEMA);
    if rc != SQLITE_OK {
        return Err(rc);
    }

    let mut z_name = z_fts3.to_vec();
    fts3_dequote(&mut z_name);
    let mut p_fts3_tab = Fts3Table::new(z_db.to_vec(), z_name);
    p_fts3_tab.n_index = 1;

    Ok(Box::new(Fts3auxTable { z_err_msg: None, p_fts3_tab }))
}

impl Vtab for Fts3auxTable {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn err_msg_mut(&mut self) -> &mut Option<Vec<u8>> {
        &mut self.z_err_msg
    }

    /// `fts3auxBestIndexMethod`: acha as restrições de igualdade e de faixa em `term` e a de
    /// igualdade na coluna oculta `languageid`.
    fn best_index(&mut self, _db: &mut Connection, p_info: &mut IndexInfo) -> i32 {
        let mut i_eq: Option<usize> = None;
        let mut i_ge: Option<usize> = None;
        let mut i_le: Option<usize> = None;
        let mut i_langid: Option<usize> = None;
        let mut i_next = 1; /* o próximo `argvIndex` livre */

        if p_info.a_constraint_usage.len() < p_info.a_constraint.len() {
            p_info.a_constraint_usage.resize(p_info.a_constraint.len(), Default::default());
        }

        /* Esta tabela entrega sempre os resultados em ordem `ORDER BY term ASC`. */
        if p_info.a_order_by.len() == 1 && p_info.a_order_by[0].i_column == 0 && !p_info.a_order_by[0].desc {
            p_info.order_by_consumed = 1;
        }

        /* Procura restrições de igualdade e de faixa em `term` e de igualdade em `languageid`. */
        for (i, c) in p_info.a_constraint.iter().enumerate() {
            if c.usable {
                let op = c.op as i32;
                let i_col = c.i_column;

                if i_col == 0 {
                    if op == SQLITE_INDEX_CONSTRAINT_EQ {
                        i_eq = Some(i);
                    }
                    if op == SQLITE_INDEX_CONSTRAINT_LT {
                        i_le = Some(i);
                    }
                    if op == SQLITE_INDEX_CONSTRAINT_LE {
                        i_le = Some(i);
                    }
                    if op == SQLITE_INDEX_CONSTRAINT_GT {
                        i_ge = Some(i);
                    }
                    if op == SQLITE_INDEX_CONSTRAINT_GE {
                        i_ge = Some(i);
                    }
                }
                if i_col == 4 && op == SQLITE_INDEX_CONSTRAINT_EQ {
                    i_langid = Some(i);
                }
            }
        }

        if let Some(i) = i_eq {
            p_info.idx_num = FTS4AUX_EQ_CONSTRAINT;
            p_info.a_constraint_usage[i].argv_index = i_next;
            i_next += 1;
            p_info.estimated_cost = 5.0;
        } else {
            p_info.idx_num = 0;
            p_info.estimated_cost = 20000.0;
            if let Some(i) = i_ge {
                p_info.idx_num += FTS4AUX_GE_CONSTRAINT;
                p_info.a_constraint_usage[i].argv_index = i_next;
                i_next += 1;
                p_info.estimated_cost /= 2.0;
            }
            if let Some(i) = i_le {
                p_info.idx_num += FTS4AUX_LE_CONSTRAINT;
                p_info.a_constraint_usage[i].argv_index = i_next;
                i_next += 1;
                p_info.estimated_cost /= 2.0;
            }
        }
        if let Some(i) = i_langid {
            p_info.a_constraint_usage[i].argv_index = i_next;
            p_info.estimated_cost -= 1.0;
        }

        SQLITE_OK
    }

    /// `fts3auxDisconnectMethod`: finaliza os comandos que a tabela guardou.
    fn disconnect(&mut self, db: &mut Connection) -> i32 {
        for slot in self.p_fts3_tab.a_stmt.iter_mut() {
            if let Some(stmt) = slot.take() {
                finalize(db, stmt);
            }
        }
        SQLITE_OK
    }

    /// `xDestroy` é o mesmo `fts3auxDisconnectMethod`.
    fn destroy(&mut self, db: &mut Connection) -> i32 {
        self.disconnect(db)
    }

    /// `fts3auxOpenMethod`.
    fn open(&mut self, _db: &mut Connection) -> Result<Box<dyn VtabCursor>, i32> {
        Ok(Box::new(Fts3auxCursor::default()))
    }
}

/// `fts3auxGrowStatArray`: garante pelo menos `n_size` elementos em `a_stat`, os novos zerados.
/// Como o `sqlite3_realloc64`, recusa um bloco de `0x7fffff00` bytes ou mais.
fn fts3aux_grow_stat_array(a_stat: &mut Vec<Fts3auxColstats>, n_size: usize) -> i32 {
    if n_size > a_stat.len() {
        if (n_size as u64) * (std::mem::size_of::<Fts3auxColstats>() as u64) >= 0x7fff_ff00 {
            return SQLITE_NOMEM;
        }
        a_stat.resize(n_size, Fts3auxColstats::default());
    }
    SQLITE_OK
}

impl Fts3auxCursor {
    /// O `memset` de `fts3auxFilterMethod`: devolve ao cursor o estado de recém-aberto (o filtro, o
    /// limite e as estatísticas são soltos).
    fn reset(&mut self) {
        *self = Fts3auxCursor::default();
    }
}

/// `fts3auxNextMethod`: avança o cursor para a linha seguinte, se houver.
fn fts3aux_next(db: &mut Connection, p_fts3: &mut Fts3Table, p_csr: &mut Fts3auxCursor) -> i32 {
    /* Incrementa o rowid de mentira. */
    p_csr.i_rowid += 1;

    p_csr.i_col += 1;
    while (p_csr.i_col as usize) < p_csr.a_stat.len() {
        if p_csr.a_stat[p_csr.i_col as usize].n_doc > 0 {
            return SQLITE_OK;
        }
        p_csr.i_col += 1;
    }

    let mut rc = fts3_seg_reader_step(db, p_fts3, &mut p_csr.csr);
    if rc == SQLITE_ROW {
        let mut i = 0usize;
        let n_doclist = p_csr.csr.a_doclist.len();
        let mut e_state = 0;

        if let Some(z_stop) = p_csr.z_stop.as_deref() {
            let z_term = p_csr.csr.z_term.as_slice();
            let n = z_stop.len().min(z_term.len());
            let mc = z_stop[..n].cmp(&z_term[..n]);
            if mc == Ordering::Less || (mc == Ordering::Equal && z_term.len() > z_stop.len()) {
                p_csr.is_eof = true;
                return SQLITE_OK;
            }
        }

        if fts3aux_grow_stat_array(&mut p_csr.a_stat, 2) != SQLITE_OK {
            return SQLITE_NOMEM;
        }
        for st in p_csr.a_stat.iter_mut() {
            *st = Fts3auxColstats::default();
        }
        let mut i_col: i32 = 0;
        rc = SQLITE_OK;

        let a_doclist = &p_csr.csr.a_doclist;
        let a_stat = &mut p_csr.a_stat;
        while i < n_doclist {
            let (n_read, v) = fts3_get_varint(&a_doclist[i..]);
            i += n_read as usize;

            /* No estado 1 esperamos um 1 (o próximo inteiro é uma coluna) ou o começo da lista de
            ** posições da coluna 0. A única diferença entre os estados 1 e 2 é que, se o inteiro
            ** lido no estado 1 não é 0 nem 1, o `nDoc` da coluna 0 deste termo sobe. */
            if e_state == 1 {
                debug_assert!(i_col == 0);
                if v > 1 {
                    a_stat[1].n_doc += 1;
                }
                e_state = 2;
            }

            match e_state {
                /* Estado 0: o inteiro lido foi um docid. */
                0 => {
                    a_stat[0].n_doc += 1;
                    e_state = 1;
                    i_col = 0;
                }

                /* Estado 2 (e o 1 que caiu nele). */
                2 => {
                    if v == 0 {
                        /* 0x00: o próximo inteiro é um docid. */
                        e_state = 0;
                    } else if v == 1 {
                        /* 0x01: o próximo inteiro é um número de coluna. */
                        e_state = 3;
                    } else {
                        /* 2 ou mais: uma posição. */
                        a_stat[(i_col + 1) as usize].n_occ += 1;
                        a_stat[0].n_occ += 1;
                    }
                }

                /* Estado 3: o inteiro lido é um número de coluna. */
                _ => {
                    debug_assert!(e_state == 3);
                    i_col = v as i32;
                    if i_col < 1 {
                        /* O `break` do C sai só do `switch`: o laço continua. */
                        rc = SQLITE_CORRUPT_VTAB;
                        continue;
                    }
                    if fts3aux_grow_stat_array(a_stat, (i_col as i64 + 2) as usize) != SQLITE_OK {
                        return SQLITE_NOMEM;
                    }
                    a_stat[(i_col + 1) as usize].n_doc += 1;
                    e_state = 2;
                }
            }
        }

        p_csr.i_col = 0;
    } else {
        p_csr.is_eof = true;
    }
    rc
}

impl VtabCursor for Fts3auxCursor {
    /// `fts3auxCloseMethod`.
    fn close(&mut self, db: &mut Connection, vtab: &mut dyn Vtab) -> i32 {
        let p_fts3 = &mut aux_table(vtab).p_fts3_tab;
        fts3_segments_close(db, p_fts3);
        fts3_seg_reader_finish(db, &mut self.csr);
        SQLITE_OK
    }

    /// `fts3auxFilterMethod`.
    fn filter(
        &mut self,
        db: &mut Connection,
        vtab: &mut dyn Vtab,
        idx_num: i32,
        idx_str: Option<&[u8]>,
        argv: &[Mem],
    ) -> i32 {
        let p_fts3 = &mut aux_table(vtab).p_fts3_tab;
        let mut is_scan = false;
        let mut i_lang_val = 0; /* o idioma consultado */

        let mut i_eq: Option<usize> = None; /* o índice de `term=?` em `argv` */
        let mut i_ge: Option<usize> = None; /* o de `term>=?` */
        let mut i_le: Option<usize> = None; /* o de `term<=?` */
        let mut i_langid: Option<usize> = None; /* o de `languageid=?` */
        let mut i_next = 0usize;

        debug_assert!(idx_str.is_none());
        debug_assert!(
            idx_num == FTS4AUX_EQ_CONSTRAINT
                || idx_num == 0
                || idx_num == FTS4AUX_LE_CONSTRAINT
                || idx_num == FTS4AUX_GE_CONSTRAINT
                || idx_num == (FTS4AUX_LE_CONSTRAINT | FTS4AUX_GE_CONSTRAINT)
        );

        if idx_num == FTS4AUX_EQ_CONSTRAINT {
            i_eq = Some(i_next);
            i_next += 1;
        } else {
            is_scan = true;
            if (idx_num & FTS4AUX_GE_CONSTRAINT) != 0 {
                i_ge = Some(i_next);
                i_next += 1;
            }
            if (idx_num & FTS4AUX_LE_CONSTRAINT) != 0 {
                i_le = Some(i_next);
                i_next += 1;
            }
        }
        if i_next < argv.len() {
            i_langid = Some(i_next);
        }

        /* Se o cursor está sendo reaproveitado, fecha e zera. */
        fts3_seg_reader_finish(db, &mut self.csr);
        self.reset();

        self.filter.flags = FTS3_SEGMENT_REQUIRE_POS | FTS3_SEGMENT_IGNORE_EMPTY;
        if is_scan {
            self.filter.flags |= FTS3_SEGMENT_SCAN;
        }

        if i_eq.is_some() || i_ge.is_some() {
            /* `sqlite3_mprintf("%s", zStr)`: o termo vale até o primeiro NUL. */
            if let Some(z_str) = text_of(&argv[0]) {
                let n = z_str.iter().position(|&c| c == 0).unwrap_or(z_str.len());
                self.filter.z_term = Some(z_str[..n].to_vec());
            }
        }

        if let Some(i) = i_le {
            /* `%s` de um texto nulo escreve `(null)`. */
            let z = text_of(&argv[i]);
            let z = z.as_deref().unwrap_or(b"(null)".as_slice());
            let n = z.iter().position(|&c| c == 0).unwrap_or(z.len());
            self.z_stop = Some(z[..n].to_vec());
        }

        if let Some(i) = i_langid {
            i_lang_val = value_int(&argv[i]);

            /* Se o usuário deu um languageid negativo, usa zero: a restrição `languageid=?` também
            ** é testada pelo VDBE (e sempre falha, pois este módulo nunca devolve uma linha com
            ** languageid negativo), então a consulta devolve zero linhas. */
            if i_lang_val < 0 {
                i_lang_val = 0;
            }
        }
        self.i_langid = i_lang_val;

        let mut rc = fts3_seg_reader_cursor(
            db,
            p_fts3,
            i_lang_val,
            0,
            FTS3_SEGCURSOR_ALL,
            self.filter.z_term.as_deref(),
            false,
            is_scan,
            &mut self.csr,
        );
        if rc == SQLITE_OK {
            rc = fts3_seg_reader_start(db, p_fts3, &mut self.csr, &self.filter);
        }

        if rc == SQLITE_OK {
            rc = fts3aux_next(db, p_fts3, self);
        }
        rc
    }

    /// `fts3auxNextMethod`.
    fn next(&mut self, db: &mut Connection, vtab: &mut dyn Vtab) -> i32 {
        let p_fts3 = &mut aux_table(vtab).p_fts3_tab;
        fts3aux_next(db, p_fts3, self)
    }

    /// `fts3auxEofMethod`.
    fn eof(&mut self, _vtab: &mut dyn Vtab) -> i32 {
        self.is_eof as i32
    }

    /// `fts3auxColumnMethod`.
    fn column(&mut self, _vtab: &mut dyn Vtab, ctx: &mut Context<'_>, i_col: i32) -> i32 {
        debug_assert!(!self.is_eof);
        match i_col {
            /* term */
            0 => result_text(ctx, Some(&self.csr.z_term), self.csr.z_term.len() as i32, StrDtor::Transient),

            /* col */
            1 => {
                if self.i_col != 0 {
                    result_int(ctx, self.i_col - 1);
                } else {
                    result_text(ctx, Some(b"*".as_slice()), -1, StrDtor::Static);
                }
            }

            /* documents */
            2 => result_int64(ctx, self.a_stat[self.i_col as usize].n_doc),

            /* occurrences */
            3 => result_int64(ctx, self.a_stat[self.i_col as usize].n_occ),

            /* languageid */
            _ => {
                debug_assert!(i_col == 4);
                result_int(ctx, self.i_langid);
            }
        }

        SQLITE_OK
    }

    /// `fts3auxRowidMethod`.
    fn rowid(&mut self, _vtab: &mut dyn Vtab, p_rowid: &mut i64) -> i32 {
        *p_rowid = self.i_rowid;
        SQLITE_OK
    }
}

/// `fts3aux_module`: o `xCreate` e o `xConnect` são a mesma função, então o módulo tem tabela
/// epônima.
struct Fts3auxModule;

impl VtabModule for Fts3auxModule {
    fn i_version(&self) -> i32 {
        0
    }

    fn caps(&self) -> ModuleCaps {
        ModuleCaps { create: true, ..ModuleCaps::default() }
    }

    fn create_is_connect(&self) -> bool {
        true
    }

    /// `fts3auxConnectMethod` como `xCreate`.
    fn x_create(
        &self,
        db: &mut Connection,
        _aux: &Option<Rc<dyn Any>>,
        argv: &[Vec<u8>],
        err: &mut Option<Vec<u8>>,
    ) -> Result<Box<dyn Vtab>, i32> {
        fts3aux_connect(db, argv, err)
    }

    /// `fts3auxConnectMethod`.
    fn x_connect(
        &self,
        db: &mut Connection,
        _aux: &Option<Rc<dyn Any>>,
        argv: &[Vec<u8>],
        err: &mut Option<Vec<u8>>,
    ) -> Result<Box<dyn Vtab>, i32> {
        fts3aux_connect(db, argv, err)
    }
}

/// `sqlite3Fts3InitAux`: registra o módulo `fts4aux` na conexão.
pub fn fts3_init_aux(db: &mut Connection) -> i32 {
    create_module(db, b"fts4aux", Some(Rc::new(Fts3auxModule)), None, None)
}
