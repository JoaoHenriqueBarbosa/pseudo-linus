//! `ext/rtree/rtree.c` (parte 2): a tabela virtual `rtree`/`rtree_i32` (os métodos de
//! `sqlite3_module`, o `xBestIndex`, o `xFilter`, o `xUpdate`), a inicialização (`rtreeInit`), as
//! funções SQL `rtreenode()`, `rtreedepth()` e `rtreecheck()`, a verificação de integridade, o
//! registro (`sqlite3RtreeInit`) e as APIs de MATCH `sqlite3_rtree_geometry_callback()` e
//! `sqlite3_rtree_query_callback()`. O resto está em [`crate::rtree`], que reexporta este módulo.
//!
//! Desvios do C, decorrentes do modelo v2 (CONVENTIONS.md; os do nó e do blob estão no cabeçalho
//! de `rtree.rs`):
//!
//! - O `sqlite3_vtab` é o próprio [`Rtree`]; o `sqlite3 *db` de `Rtree.db` é o `&mut Connection`
//!   que cada método recebe; o cursor chega ao `Rtree` pelo `vtab` que o trait passa.
//! - `xRowid` não recebe a conexão, mas pode precisar reler o nó da entrada corrente do banco
//!   (`rtreeNodeOfFirstSearchPoint`). Por isso `rtree_step_to_leaf` já deixa em cache o nó da
//!   entrada em que para, o que o C só faz quando `xRowid` ou `xColumn` o pedem; a diferença só
//!   aparece num r-tree corrompido consultado sem ler rowid nem colunas.
//! - `sqlite3_value_numeric_type()` num argumento de `xFilter` converte uma cópia do valor (a lista
//!   `argv` é imutável), o que não muda nada observável: a conversão serve só à leitura que vem
//!   logo depois.
//! - O `pContext`/`xDestructor` das APIs de MATCH viram um `Rc<dyn Any>` e um [`DestroyFn`]; o
//!   `RtreeMatchArg` é o valor ponteiro `"RtreeMatchArg"` (sem o `iSize`, que só existia para o
//!   `memcpy`) e o `rtreeMatchArgFree` é o `Drop` do valor. A `xDelUser` de `sqlite3_rtree_geometry`
//!   é o `Drop` do `pUser`.
//! - `%.*s` com o tamanho de token de `rtreeTokenLength` vira `%s` sobre o token já cortado.

use std::any::Any;
use std::rc::Rc;

use crate::build::text_arg;
use crate::connection::{
    Connection, Context, DestroyFn, IndexInfo, ModuleCaps, UserData, Vtab, VtabCursor, VtabModule,
};
use crate::consts::{
    SQLITE_ABORT, SQLITE_ANY, SQLITE_BLOB, SQLITE_CONSTRAINT, SQLITE_CORRUPT, SQLITE_CORRUPT_VTAB,
    SQLITE_DONE, SQLITE_ERROR, SQLITE_FLOAT, SQLITE_INDEX_CONSTRAINT_EQ, SQLITE_INDEX_CONSTRAINT_GE,
    SQLITE_INDEX_CONSTRAINT_GT, SQLITE_INDEX_CONSTRAINT_LE, SQLITE_INDEX_CONSTRAINT_LT,
    SQLITE_INDEX_CONSTRAINT_MATCH, SQLITE_INDEX_SCAN_UNIQUE, SQLITE_INTEGER, SQLITE_LIMIT_LENGTH,
    SQLITE_LOCKED_VTAB, SQLITE_MAX_LENGTH, SQLITE_NOMEM, SQLITE_NULL, SQLITE_OK,
    SQLITE_PREPARE_NO_VTAB, SQLITE_PREPARE_PERSISTENT, SQLITE_REPLACE, SQLITE_ROW, SQLITE_UTF8,
    SQLITE_VTAB_CONSTRAINT_SUPPORT, SQLITE_VTAB_INNOCUOUS,
};
use crate::legacy::exec;
use crate::main::{create_function_api, errmsg, table_column_metadata};
use crate::mem::{int_float_compare, value_numeric_type, value_type, Mem, StrDtor};
use crate::mem2::mem_set_pointer;
use crate::prepare::{prepare_v2, prepare_v3};
use crate::printf::{mprintf, PrintfArg, StrAccum};
use crate::rtree::{
    choose_leaf, deserialize_geometry, finalize_stmt, find_leaf_node, node_acquire,
    node_blob_reset, node_get_coord, node_get_rowid, node_release,
    node_rowid_index, read_int16, read_int64, reset_cursor, rtree_delete_rowid, rtree_insert_cell,
    rtree_new_rowid, rtree_node_of_first_search_point, rtree_release, rtree_search_point_first,
    rtree_search_point_new,
    rtree_search_point_pop, rtree_step_to_leaf, rtree_value_down, rtree_value_up, cell_from_data,
    read_coord, NodeId, Rtree, RtreeCell, RtreeConstraint, RtreeCoord, RtreeCursor,
    RtreeGeomCallback, RtreeGeomFn, RtreeMatchArg, RtreeQueryFn, PARTLY_WITHIN, RTREE_COORD_INT32,
    RTREE_COORD_REAL32, RTREE_DEFAULT_ROWEST, RTREE_EQ, RTREE_FALSE, RTREE_GE, RTREE_GT, RTREE_LE,
    RTREE_LT, RTREE_MATCH, RTREE_MAXCELLS, RTREE_MAX_AUX_COLUMN, RTREE_MAX_DEPTH,
    RTREE_MAX_DIMENSIONS, RTREE_MIN_ROWEST, RTREE_TRUE,
};
use crate::tokenize::get_token;
use crate::vdbeapi::{
    bind_int64, bind_value, column_blob, column_bytes, column_count, column_int, column_int64,
    column_name, column_type, column_value, finalize, reset, result_double, result_error,
    result_error_code, result_error_nomem, result_int, result_int64, result_text, result_value,
    step, text_of, user_data, value_dup, value_double, value_int, value_int64,
};
use crate::vtab::{create_module, declare_vtab, vtab_config, vtab_on_conflict};

/// O `Rtree` da instância que o cursor ou o trait entregam.
fn rtree_of(vtab: &mut dyn Vtab) -> &mut Rtree {
    vtab.as_any_mut().downcast_mut::<Rtree>().expect("rtree: a instância não é um Rtree")
}

/// Os métodos `xSync`, `xCommit` e `xRollback`, que no C são todos o `rtreeEndTransaction`: chamado
/// quando uma transação termina (COMMIT ou ROLLBACK), solta o blob de leitura de nós.
macro_rules! rtree_end_transaction_methods {
    ($($name:ident),*) => {
        $(
            fn $name(&mut self, db: &mut Connection) -> i32 {
                self.in_wr_trans = 0;
                node_blob_reset(db, self);
                SQLITE_OK
            }
        )*
    };
}

/// `rtreeConstraintError`: uma restrição falhou ao inserir uma linha. Grava a mensagem de erro em
/// `Rtree.base.zErrMsg` e devolve `SQLITE_CONSTRAINT`. `i_col` é o índice da coluna mais à
/// esquerda envolvida na falha: 0 é a restrição UNIQUE da coluna `id`, senão é a restrição
/// (c1<=c2) das colunas `i_col` e `i_col+1`.
fn rtree_constraint_error(db: &mut Connection, p_rtree: &mut Rtree, i_col: i32) -> i32 {
    debug_assert!(i_col == 0 || i_col % 2 != 0);
    let mut p_stmt = None;
    let z_sql = mprintf(
        b"SELECT * FROM %Q.%Q",
        &[text_arg(&p_rtree.z_db), text_arg(&p_rtree.z_name)],
    );
    let rc = match z_sql {
        Some(z) => {
            let (rc, stmt, _) = prepare_v2(db, &z, -1);
            p_stmt = stmt;
            rc
        }
        None => SQLITE_NOMEM,
    };

    if rc == SQLITE_OK {
        let id = p_stmt.unwrap_or_default();
        if i_col == 0 {
            let z_col = column_name(db, id, 0);
            p_rtree.z_err_msg = mprintf(
                b"UNIQUE constraint failed: %s.%s",
                &[text_arg(&p_rtree.z_name), PrintfArg::Text(z_col)],
            );
        } else {
            let z_col1 = column_name(db, id, i_col);
            let z_col2 = column_name(db, id, i_col + 1);
            p_rtree.z_err_msg = mprintf(
                b"rtree constraint failed: %s.(%s<=%s)",
                &[text_arg(&p_rtree.z_name), PrintfArg::Text(z_col1), PrintfArg::Text(z_col2)],
            );
        }
    }

    finalize_stmt(db, &mut p_stmt);
    if rc == SQLITE_OK {
        SQLITE_CONSTRAINT
    } else {
        rc
    }
}

impl Vtab for Rtree {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn err_msg_mut(&mut self) -> &mut Option<Vec<u8>> {
        &mut self.z_err_msg
    }

    /// `rtreeBestIndex`: há duas estratégias de varredura, da mais para a menos desejável.
    ///
    /// | idxNum | idxStr          | Estratégia                         |
    /// |--------|-----------------|------------------------------------|
    /// | 1      | sem uso         | busca direta pelo rowid            |
    /// | 2      | veja abaixo     | consulta r-tree ou varredura total |
    ///
    /// Na estratégia 2, `idxStr` leva 2 bytes por restrição usada, na ordem de `argvIndex`. O
    /// primeiro byte é o operador (`=` 'A', `<=` 'B', `<` 'C', `>=` 'D', `>` 'E', MATCH 'F') e o
    /// segundo é a coluna de coordenada ('0' é a mais à esquerda).
    fn best_index(&mut self, _db: &mut Connection, info: &mut IndexInfo) -> i32 {
        let mut z_idx_str: Vec<u8> = Vec::new();

        // Há restrição MATCH, mesmo inutilizável? Então o plano de busca por rowid não vale, pois
        // exigiria que o VDBE avaliasse o MATCH, o que não é possível.
        let b_match = info.a_constraint.iter().any(|c| c.op as i32 == SQLITE_INDEX_CONSTRAINT_MATCH);

        debug_assert!(info.idx_str.is_none());
        let mut ii = 0usize;
        while ii < info.a_constraint.len() && z_idx_str.len() < RTREE_MAX_DIMENSIONS * 8 {
            let p = info.a_constraint[ii];

            if !b_match && p.usable && p.i_column <= 0 && p.op as i32 == SQLITE_INDEX_CONSTRAINT_EQ {
                // Igualdade no rowid: estratégia 1.
                for jj in 0..ii {
                    info.a_constraint_usage[jj].argv_index = 0;
                    info.a_constraint_usage[jj].omit = false;
                }
                info.idx_num = 1;
                info.a_constraint_usage[ii].argv_index = 1;
                info.a_constraint_usage[ii].omit = true;

                // Duas buscas de rowid em árvores B e uma busca linear num nó r-tree: quase tão
                // rápido quanto a busca direta (custo interno 0.0 do SQLite). Devolve uma linha.
                info.estimated_cost = 30.0;
                info.estimated_rows = 1;
                info.idx_flags = SQLITE_INDEX_SCAN_UNIQUE;
                return SQLITE_OK;
            }

            if p.usable
                && ((p.i_column > 0 && p.i_column <= self.n_dim2 as i32)
                    || p.op as i32 == SQLITE_INDEX_CONSTRAINT_MATCH)
            {
                let mut do_omit = true;
                let op = match p.op as i32 {
                    SQLITE_INDEX_CONSTRAINT_EQ => {
                        do_omit = false;
                        RTREE_EQ
                    }
                    SQLITE_INDEX_CONSTRAINT_GT => {
                        do_omit = false;
                        RTREE_GT
                    }
                    SQLITE_INDEX_CONSTRAINT_LE => RTREE_LE,
                    SQLITE_INDEX_CONSTRAINT_LT => {
                        do_omit = false;
                        RTREE_LT
                    }
                    SQLITE_INDEX_CONSTRAINT_GE => RTREE_GE,
                    SQLITE_INDEX_CONSTRAINT_MATCH => RTREE_MATCH,
                    _ => 0,
                };
                if op != 0 {
                    z_idx_str.push(op as u8);
                    z_idx_str.push((p.i_column - 1 + b'0' as i32) as u8);
                    info.a_constraint_usage[ii].argv_index = (z_idx_str.len() / 2) as i32;
                    info.a_constraint_usage[ii].omit = do_omit;
                }
            }
            ii += 1;
        }

        info.idx_num = 2;
        let i_idx = z_idx_str.len();
        if i_idx > 0 {
            info.idx_str = Some(z_idx_str);
        }

        let n_row = self.n_row_est >> (i_idx / 2);
        info.estimated_cost = 6.0 * n_row as f64;
        info.estimated_rows = n_row;

        SQLITE_OK
    }

    /// `rtreeDisconnect`.
    fn disconnect(&mut self, db: &mut Connection) -> i32 {
        rtree_release(db, self);
        SQLITE_OK
    }

    /// `rtreeDestroy`.
    fn destroy(&mut self, db: &mut Connection) -> i32 {
        let z_create = mprintf(
            b"DROP TABLE '%q'.'%q_node';DROP TABLE '%q'.'%q_rowid';DROP TABLE '%q'.'%q_parent';",
            &[
                text_arg(&self.z_db),
                text_arg(&self.z_name),
                text_arg(&self.z_db),
                text_arg(&self.z_name),
                text_arg(&self.z_db),
                text_arg(&self.z_name),
            ],
        );
        let rc = match z_create {
            None => SQLITE_NOMEM,
            Some(z) => {
                node_blob_reset(db, self);
                exec(db, &z, None)
            }
        };
        if rc == SQLITE_OK {
            rtree_release(db, self);
        }
        rc
    }

    /// `rtreeOpen`.
    fn open(&mut self, _db: &mut Connection) -> Result<Box<dyn VtabCursor>, i32> {
        self.n_cursor += 1;
        Ok(Box::new(RtreeCursor::new()))
    }

    /// `rtreeUpdate`: o `xUpdate`. Uma operação de escrita pode devolver `SQLITE_CONSTRAINT` por
    /// dois motivos: rowid duplicado (se o modo é REPLACE a linha conflitante sai e a operação
    /// segue) ou dado que viola `x2>=x1` (sempre `SQLITE_CONSTRAINT`, qualquer que seja o modo).
    fn update(&mut self, db: &mut Connection, a_data: &[Mem], p_rowid: &mut i64) -> i32 {
        let n_data = a_data.len() as i32;
        let mut rc = SQLITE_OK;
        let mut cell = RtreeCell::default();
        let mut b_have_rowid = false;

        if self.n_node_ref != 0 {
            // Não dá para escrever na árvore B enquanto outro cursor lê dela: a escrita pode
            // rebalancear e atrapalhar o cursor de leitura.
            return SQLITE_LOCKED_VTAB;
        }
        self.n_busy += 1;
        debug_assert!(n_data >= 1);

        'constraint: {
            if n_data > 1 {
                let mut nn = n_data - 4;
                if nn > self.n_dim2 as i32 {
                    nn = self.n_dim2 as i32;
                }
                // Preenche `cell.aCoord[]`; a primeira coordenada é `aData[3]`.
                //
                // `nData` só é menor que `nDim*2+3` se o r-tree foi declarado errado, com "colunas"
                // que são lidas como restrições de tabela (`rtree(x,y,CHECK(y>5))`). Isso foi
                // descoberto depois de anos de uso, então tais tabelas são ignoradas em silêncio.
                if self.e_coord_type == RTREE_COORD_REAL32 {
                    let mut ii = 0usize;
                    while (ii as i32) < nn {
                        cell.a_coord[ii] = RtreeCoord::from_f(rtree_value_down(&a_data[ii + 3]));
                        cell.a_coord[ii + 1] = RtreeCoord::from_f(rtree_value_up(&a_data[ii + 4]));
                        if cell.a_coord[ii].f() > cell.a_coord[ii + 1].f() {
                            rc = rtree_constraint_error(db, self, ii as i32 + 1);
                            break 'constraint;
                        }
                        ii += 2;
                    }
                } else {
                    let mut ii = 0usize;
                    while (ii as i32) < nn {
                        cell.a_coord[ii] = RtreeCoord::from_i(value_int(&a_data[ii + 3]));
                        cell.a_coord[ii + 1] = RtreeCoord::from_i(value_int(&a_data[ii + 4]));
                        if cell.a_coord[ii].i() > cell.a_coord[ii + 1].i() {
                            rc = rtree_constraint_error(db, self, ii as i32 + 1);
                            break 'constraint;
                        }
                        ii += 2;
                    }
                }

                // Se veio um rowid, vê se já existe na tabela: então a restrição falhou.
                if value_type(&a_data[2]) != SQLITE_NULL {
                    cell.i_rowid = value_int64(&a_data[2]);
                    if value_type(&a_data[0]) == SQLITE_NULL
                        || value_int64(&a_data[0]) != cell.i_rowid
                    {
                        let p_read = self.p_read_rowid.unwrap_or_default();
                        bind_int64(db, p_read, 1, cell.i_rowid);
                        let steprc = step(db, p_read);
                        rc = reset(db, p_read);
                        if steprc == SQLITE_ROW {
                            if vtab_on_conflict(db) == SQLITE_REPLACE {
                                rc = rtree_delete_rowid(db, self, cell.i_rowid);
                            } else {
                                rc = rtree_constraint_error(db, self, 0);
                                break 'constraint;
                            }
                        }
                    }
                    b_have_rowid = true;
                }
            }

            // Se `aData[0]` não é NULL, é o rowid de um registro a apagar da tabela.
            if value_type(&a_data[0]) != SQLITE_NULL {
                rc = rtree_delete_rowid(db, self, value_int64(&a_data[0]));
            }

            // Se `aData[]` tem mais de um elemento, `aData[2]..aData[argc-1]` é um registro novo a
            // inserir na estrutura.
            if rc == SQLITE_OK && n_data > 1 {
                let mut p_leaf: Option<NodeId> = None;

                // Descobre o rowid da linha nova.
                if !b_have_rowid {
                    rc = rtree_new_rowid(db, self, &mut cell.i_rowid);
                }
                *p_rowid = cell.i_rowid;

                if rc == SQLITE_OK {
                    rc = choose_leaf(db, self, &cell, 0, &mut p_leaf);
                }
                if rc == SQLITE_OK {
                    match p_leaf {
                        Some(leaf) => {
                            rc = rtree_insert_cell(db, self, leaf, &cell, 0);
                            let rc2 = node_release(db, self, Some(leaf));
                            if rc == SQLITE_OK {
                                rc = rc2;
                            }
                        }
                        None => rc = SQLITE_CORRUPT_VTAB,
                    }
                }
                if rc == SQLITE_OK && self.n_aux != 0 {
                    let p_up = self.p_write_aux.unwrap_or_default();
                    bind_int64(db, p_up, 1, *p_rowid);
                    for jj in 0..self.n_aux as usize {
                        bind_value(db, p_up, jj as i32 + 2, &a_data[self.n_dim2 as usize + 3 + jj]);
                    }
                    step(db, p_up);
                    rc = reset(db, p_up);
                }
            }
        }

        rtree_release(db, self);
        rc
    }

    /// `rtreeBeginTransaction`.
    fn begin(&mut self, _db: &mut Connection) -> i32 {
        self.in_wr_trans = 1;
        SQLITE_OK
    }

    rtree_end_transaction_methods!(sync, commit, rollback);

    /// `rtreeRename`.
    fn rename(&mut self, db: &mut Connection, z_new_name: &[u8]) -> i32 {
        let z_sql = mprintf(
            b"ALTER TABLE %Q.'%q_node'   RENAME TO \"%w_node\";\
              ALTER TABLE %Q.'%q_parent' RENAME TO \"%w_parent\";\
              ALTER TABLE %Q.'%q_rowid'  RENAME TO \"%w_rowid\";",
            &[
                text_arg(&self.z_db),
                text_arg(&self.z_name),
                text_arg(z_new_name),
                text_arg(&self.z_db),
                text_arg(&self.z_name),
                text_arg(z_new_name),
                text_arg(&self.z_db),
                text_arg(&self.z_name),
                text_arg(z_new_name),
            ],
        );
        match z_sql {
            Some(z) => {
                node_blob_reset(db, self);
                exec(db, &z, None)
            }
            None => SQLITE_NOMEM,
        }
    }

    /// `rtreeSavepoint`: este módulo não precisa fazer nada para dar suporte a savepoints, mas
    /// usa o gancho para fechar o blob aberto. Um DROP TABLE (que sempre abre um savepoint) não
    /// consegue terminar com blob aberto: sem isso `BEGIN; INSERT INTO rtree...; DROP TABLE t;`
    /// falharia com `SQLITE_LOCKED`.
    fn savepoint(&mut self, db: &mut Connection, _i_savepoint: i32) -> i32 {
        let iwt = self.in_wr_trans;
        self.in_wr_trans = 0;
        node_blob_reset(db, self);
        self.in_wr_trans = iwt;
        SQLITE_OK
    }

    /// `rtreeIntegrity`: o `xIntegrity`.
    fn integrity(
        &mut self,
        db: &mut Connection,
        _z_schema: &[u8],
        _z_name: &[u8],
        _is_quick: i32,
        pz_err: &mut Option<Vec<u8>>,
    ) -> i32 {
        let mut report: Option<Vec<u8>> = None;
        let z_db = self.z_db.clone();
        let z_name = self.z_name.clone();
        let rc = rtree_check_table(db, Some(&z_db), Some(&z_name), &mut report);
        if rc == SQLITE_OK {
            if let Some(r) = report {
                *pz_err = Some(
                    mprintf(
                        b"In RTree %s.%s:\n%s",
                        &[text_arg(&self.z_db), text_arg(&self.z_name), text_arg(&r)],
                    )
                    .unwrap_or_default(),
                );
            }
        } else {
            *pz_err = report;
        }
        rc
    }
}

impl VtabCursor for RtreeCursor {
    /// `rtreeClose`.
    fn close(&mut self, db: &mut Connection, vtab: &mut dyn Vtab) -> i32 {
        let p_rtree = rtree_of(vtab);
        debug_assert!(p_rtree.n_cursor > 0);
        reset_cursor(db, p_rtree, self);
        finalize_stmt(db, &mut self.p_read_aux);
        p_rtree.n_cursor -= 1;
        if p_rtree.n_cursor == 0 && p_rtree.in_wr_trans == 0 {
            node_blob_reset(db, p_rtree);
        }
        SQLITE_OK
    }

    /// `rtreeFilter`.
    fn filter(
        &mut self,
        db: &mut Connection,
        vtab: &mut dyn Vtab,
        idx_num: i32,
        idx_str: Option<&[u8]>,
        argv: &[Mem],
    ) -> i32 {
        let p_rtree = rtree_of(vtab);
        let mut p_root: Option<NodeId> = None;
        let mut rc = SQLITE_OK;
        let mut i_cell = 0;
        let argc = argv.len();

        p_rtree.n_busy += 1;

        // Põe o cursor no mesmo estado em que `rtreeOpen()` o deixa.
        reset_cursor(db, p_rtree, self);

        self.i_strategy = idx_num;
        if idx_num == 1 {
            // Caso especial: busca por rowid.
            let mut p_leaf: Option<NodeId> = None;
            let i_rowid = value_int64(&argv[0]);
            let mut i_node: i64 = 0;
            let mut v0 = argv[0].clone();
            let e_type = value_numeric_type(&mut v0);
            if e_type == SQLITE_INTEGER
                || (e_type == SQLITE_FLOAT && 0 == int_float_compare(i_rowid, value_double(&v0)))
            {
                rc = find_leaf_node(db, p_rtree, i_rowid, &mut p_leaf, Some(&mut i_node));
            } else {
                rc = SQLITE_OK;
                p_leaf = None;
            }
            if rc == SQLITE_OK && p_leaf.is_some() {
                let sp = rtree_search_point_new(db, p_rtree, self, 0.0, 0);
                self.a_node[0] = p_leaf;
                {
                    let p = self.point_mut(sp);
                    p.id = i_node;
                    p.e_within = PARTLY_WITHIN as u8;
                }
                if let Some(leaf) = p_leaf {
                    rc = node_rowid_index(p_rtree, leaf, i_rowid, &mut i_cell);
                }
                self.point_mut(sp).i_cell = i_cell as u8;
            } else {
                self.at_eof = 1;
            }
        } else {
            // Caso normal: varredura do r-tree. Monta `aConstraint` com as restrições
            // configuradas.
            rc = node_acquire(db, p_rtree, 1, None, &mut p_root);
            if rc == SQLITE_OK && argc > 0 {
                self.a_constraint = (0..argc).map(|_| RtreeConstraint::default()).collect();
                let n_zero = (p_rtree.i_depth + 1).clamp(0, RTREE_MAX_DEPTH + 1) as usize;
                self.an_queue[..n_zero].fill(0);
                let idx = idx_str.unwrap_or(&[]);
                debug_assert!(idx.len() == argc * 2);
                for ii in 0..argc {
                    let mut v = argv[ii].clone();
                    let e_type = value_numeric_type(&mut v);
                    let p = &mut self.a_constraint[ii];
                    p.op = idx.get(ii * 2).copied().unwrap_or(0) as i32;
                    p.i_coord = idx.get(ii * 2 + 1).copied().unwrap_or(0) as i32 - b'0' as i32;
                    if p.op >= RTREE_MATCH {
                        // Um operador MATCH: o lado direito é um blob que vira um `RtreeMatchArg`,
                        // criado por uma função SQL de `sqlite3_rtree_geometry_callback()`.
                        rc = deserialize_geometry(&argv[ii], p);
                        if rc != SQLITE_OK {
                            break;
                        }
                        if let Some(info) = p.p_info.as_mut() {
                            info.n_coord = p_rtree.n_dim2 as i32;
                            info.mx_level = p_rtree.i_depth + 1;
                        }
                    } else if e_type == SQLITE_INTEGER {
                        let i_val = value_int64(&v);
                        p.r_value = i_val as f64;
                        if i_val >= (1i64 << 48) || i_val <= -(1i64 << 48) {
                            if p.op == RTREE_LT {
                                p.op = RTREE_LE;
                            }
                            if p.op == RTREE_GT {
                                p.op = RTREE_GE;
                            }
                        }
                    } else if e_type == SQLITE_FLOAT {
                        p.r_value = value_double(&v);
                    } else {
                        p.r_value = 0.0;
                        if e_type == SQLITE_NULL {
                            p.op = RTREE_FALSE;
                        } else if p.op == RTREE_LT || p.op == RTREE_LE {
                            p.op = RTREE_TRUE;
                        } else {
                            p.op = RTREE_FALSE;
                        }
                    }
                }
            }
            if rc == SQLITE_OK {
                debug_assert!(self.b_point == 0);
                let depth = (p_rtree.i_depth + 1) as u8;
                let sp = rtree_search_point_new(db, p_rtree, self, 0.0, depth);
                {
                    let p_new = self.point_mut(sp);
                    p_new.id = 1;
                    p_new.i_cell = 0;
                    p_new.e_within = PARTLY_WITHIN as u8;
                }
                debug_assert!(self.b_point == 1);
                self.a_node[0] = p_root;
                p_root = None;
                rc = rtree_step_to_leaf(db, p_rtree, self);
            }
        }

        node_release(db, p_rtree, p_root);
        rtree_release(db, p_rtree);
        rc
    }

    /// `rtreeNext`: passa para a próxima entrada que satisfaz as restrições configuradas.
    fn next(&mut self, db: &mut Connection, vtab: &mut dyn Vtab) -> i32 {
        let p_rtree = rtree_of(vtab);
        if self.b_aux_valid != 0 {
            self.b_aux_valid = 0;
            reset(db, self.p_read_aux.unwrap_or_default());
        }
        rtree_search_point_pop(db, p_rtree, self);
        rtree_step_to_leaf(db, p_rtree, self)
    }

    /// `rtreeEof`: diferente de zero se o cursor não aponta para um registro válido, isto é, se a
    /// varredura terminou.
    fn eof(&mut self, _vtab: &mut dyn Vtab) -> i32 {
        self.at_eof as i32
    }

    /// `rtreeColumn`.
    fn column(&mut self, vtab: &mut dyn Vtab, ctx: &mut Context<'_>, i: i32) -> i32 {
        let p_rtree = rtree_of(vtab);
        let p = rtree_search_point_first(self);
        let mut rc = SQLITE_OK;
        let p_node = rtree_node_of_first_search_point(ctx.db, p_rtree, self, &mut rc);

        if rc != SQLITE_OK {
            return rc;
        }
        let (Some(p), Some(p_node)) = (p, p_node) else {
            return SQLITE_OK;
        };
        let i_cell = self.point(p).i_cell as i32;
        if i_cell >= p_rtree.n_cell(p_node) {
            return SQLITE_ABORT;
        }
        if i == 0 {
            result_int64(ctx, node_get_rowid(p_rtree, p_node, i_cell));
        } else if i <= p_rtree.n_dim2 as i32 {
            let c = node_get_coord(p_rtree, p_node, i_cell, i - 1);
            if p_rtree.e_coord_type == RTREE_COORD_REAL32 {
                result_double(ctx, c.f() as f64);
            } else {
                debug_assert!(p_rtree.e_coord_type == RTREE_COORD_INT32);
                result_int(ctx, c.i());
            }
        } else {
            if self.b_aux_valid == 0 {
                if self.p_read_aux.is_none() {
                    let z_sql = p_rtree.z_read_aux_sql.clone().unwrap_or_default();
                    let (rc, stmt, _) = prepare_v3(ctx.db, &z_sql, -1, 0);
                    if rc != SQLITE_OK {
                        return rc;
                    }
                    self.p_read_aux = stmt;
                }
                let aux = self.p_read_aux.unwrap_or_default();
                bind_int64(ctx.db, aux, 1, node_get_rowid(p_rtree, p_node, i_cell));
                rc = step(ctx.db, aux);
                if rc == SQLITE_ROW {
                    self.b_aux_valid = 1;
                } else {
                    reset(ctx.db, aux);
                    if rc == SQLITE_DONE {
                        rc = SQLITE_OK;
                    }
                    return rc;
                }
            }
            let aux = self.p_read_aux.unwrap_or_default();
            let v = column_value(ctx.db, aux, i - p_rtree.n_dim2 as i32 + 1);
            result_value(ctx, &v);
        }
        SQLITE_OK
    }

    /// `rtreeRowid`.
    fn rowid(&mut self, vtab: &mut dyn Vtab, p_rowid: &mut i64) -> i32 {
        let p_rtree = rtree_of(vtab);
        let Some(p) = rtree_search_point_first(self) else {
            return SQLITE_OK;
        };
        // O nó já está em cache: `rtree_step_to_leaf` e o `xFilter` por rowid o deixam lá (ver o
        // cabeçalho do módulo).
        let Some(p_node) = self.a_node[1 - self.b_point as usize] else {
            return SQLITE_CORRUPT_VTAB;
        };
        let i_cell = self.point(p).i_cell as i32;
        if i_cell >= p_rtree.n_cell(p_node) {
            SQLITE_ABORT
        } else {
            *p_rowid = node_get_rowid(p_rtree, p_node, i_cell);
            SQLITE_OK
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Inicialização
// ---------------------------------------------------------------------------------------------

/// `rtreeQueryStat1`: preenche `Rtree.nRowEst` com uma estimativa do número de linhas, baseada em
/// `sqlite_stat1` se possível, senão em `RTREE_DEFAULT_ROWEST`.
fn rtree_query_stat1(db: &mut Connection, p_rtree: &mut Rtree) -> i32 {
    let mut n_row = RTREE_MIN_ROWEST;

    let z_db = p_rtree.z_db.clone();
    let (mut rc, _) = table_column_metadata(db, Some(&z_db), b"sqlite_stat1", None);
    if rc != SQLITE_OK {
        p_rtree.n_row_est = RTREE_DEFAULT_ROWEST;
        return if rc == SQLITE_ERROR { SQLITE_OK } else { rc };
    }
    let z_sql = mprintf(
        b"SELECT stat FROM %Q.sqlite_stat1 WHERE tbl = '%q_rowid'",
        &[text_arg(&p_rtree.z_db), text_arg(&p_rtree.z_name)],
    );
    match z_sql {
        None => rc = SQLITE_NOMEM,
        Some(z) => {
            let (rc1, p, _) = prepare_v2(db, &z, -1);
            rc = rc1;
            if rc == SQLITE_OK {
                let p = p.unwrap_or_default();
                if step(db, p) == SQLITE_ROW {
                    n_row = column_int64(db, p, 0);
                }
                rc = finalize(db, p);
            }
        }
    }
    p_rtree.n_row_est = n_row.max(RTREE_MIN_ROWEST);
    rc
}

/// `rtreeSqlInit`: cria (se `is_create`) as tabelas sombra e prepara os comandos da tabela.
fn rtree_sql_init(
    db: &mut Connection,
    p_rtree: &mut Rtree,
    z_db: &[u8],
    z_prefix: &[u8],
    is_create: bool,
) -> i32 {
    const AZ_SQL: [&[u8]; 8] = [
        // Grava `xxx_node`.
        b"INSERT OR REPLACE INTO '%q'.'%q_node' VALUES(?1, ?2)",
        b"DELETE FROM '%q'.'%q_node' WHERE nodeno = ?1",
        // Lê e grava `xxx_rowid`.
        b"SELECT nodeno FROM '%q'.'%q_rowid' WHERE rowid = ?1",
        b"INSERT OR REPLACE INTO '%q'.'%q_rowid' VALUES(?1, ?2)",
        b"DELETE FROM '%q'.'%q_rowid' WHERE rowid = ?1",
        // Lê e grava `xxx_parent`.
        b"SELECT parentnode FROM '%q'.'%q_parent' WHERE nodeno = ?1",
        b"INSERT OR REPLACE INTO '%q'.'%q_parent' VALUES(?1, ?2)",
        b"DELETE FROM '%q'.'%q_parent' WHERE nodeno = ?1",
    ];
    let f = SQLITE_PREPARE_PERSISTENT | SQLITE_PREPARE_NO_VTAB;
    let n_aux = p_rtree.n_aux as i32;
    let n_node_size = p_rtree.i_node_size;
    let mut rc;

    let a_db_prefix = [text_arg(z_db), text_arg(z_prefix)];

    if is_create {
        let mut p = StrAccum::new(db.a_limit[SQLITE_LIMIT_LENGTH as usize] as u32);
        p.appendf(
            b"CREATE TABLE \"%w\".\"%w_rowid\"(rowid INTEGER PRIMARY KEY,nodeno",
            &a_db_prefix,
        );
        for ii in 0..n_aux {
            p.appendf(b",a%d", &[PrintfArg::Int(ii as i64)]);
        }
        p.appendf(b");CREATE TABLE \"%w\".\"%w_node\"(nodeno INTEGER PRIMARY KEY,data);", &a_db_prefix);
        p.appendf(
            b"CREATE TABLE \"%w\".\"%w_parent\"(nodeno INTEGER PRIMARY KEY,parentnode);",
            &a_db_prefix,
        );
        p.appendf(
            b"INSERT INTO \"%w\".\"%w_node\"VALUES(1,zeroblob(%d))",
            &[a_db_prefix[0].clone(), a_db_prefix[1].clone(), PrintfArg::Int(n_node_size as i64)],
        );
        let Some(z_create) = p.finish() else {
            return SQLITE_NOMEM;
        };
        rc = exec(db, &z_create, None);
        if rc != SQLITE_OK {
            return rc;
        }
    }

    rc = rtree_query_stat1(db, p_rtree);
    {
        let mut app_stmt = [
            &mut p_rtree.p_write_node,
            &mut p_rtree.p_delete_node,
            &mut p_rtree.p_read_rowid,
            &mut p_rtree.p_write_rowid,
            &mut p_rtree.p_delete_rowid,
            &mut p_rtree.p_read_parent,
            &mut p_rtree.p_write_parent,
            &mut p_rtree.p_delete_parent,
        ];
        let mut i = 0;
        while i < AZ_SQL.len() && rc == SQLITE_OK {
            let z_format: &[u8] = if i != 3 || n_aux == 0 {
                AZ_SQL[i]
            } else {
                // Um UPSERT é um pouco mais lento que REPLACE, mas é preciso com colunas
                // auxiliares.
                b"INSERT INTO\"%w\".\"%w_rowid\"(rowid,nodeno)VALUES(?1,?2)\
                  ON CONFLICT(rowid)DO UPDATE SET nodeno=excluded.nodeno"
            };
            match mprintf(z_format, &a_db_prefix) {
                Some(z_sql) => {
                    let (rc1, stmt, _) = prepare_v3(db, &z_sql, -1, f);
                    rc = rc1;
                    *app_stmt[i] = stmt;
                }
                None => rc = SQLITE_NOMEM,
            }
            i += 1;
        }
    }
    if n_aux != 0 && rc != SQLITE_NOMEM {
        p_rtree.z_read_aux_sql = mprintf(
            b"SELECT * FROM \"%w\".\"%w_rowid\" WHERE rowid=?1",
            &a_db_prefix,
        );
        if p_rtree.z_read_aux_sql.is_none() {
            rc = SQLITE_NOMEM;
        } else {
            let mut p = StrAccum::new(db.a_limit[SQLITE_LIMIT_LENGTH as usize] as u32);
            p.appendf(b"UPDATE \"%w\".\"%w_rowid\"SET ", &a_db_prefix);
            for ii in 0..n_aux {
                if ii != 0 {
                    p.append(b",");
                }
                p.appendf(
                    b"a%d=?%d",
                    &[PrintfArg::Int(ii as i64), PrintfArg::Int(ii as i64 + 2)],
                );
            }
            p.appendf(b" WHERE rowid=?1", &[]);
            match p.finish() {
                None => rc = SQLITE_NOMEM,
                Some(z_sql) => {
                    let (rc1, stmt, _) = prepare_v3(db, &z_sql, -1, f);
                    rc = rc1;
                    p_rtree.p_write_aux = stmt;
                }
            }
        }
    }

    rc
}

/// `getIntFromStmt`: o SQL devolve um único inteiro; compila e executa. Em sucesso grava o valor
/// em `pi_val`.
fn get_int_from_stmt(db: &mut Connection, z_sql: Option<&[u8]>, pi_val: &mut i32) -> i32 {
    let Some(z_sql) = z_sql else {
        return SQLITE_NOMEM;
    };
    let (rc, p_stmt, _) = prepare_v2(db, z_sql, -1);
    if rc != SQLITE_OK {
        return rc;
    }
    let p = p_stmt.unwrap_or_default();
    if SQLITE_ROW == step(db, p) {
        *pi_val = column_int(db, p, 0);
    }
    finalize(db, p)
}

/// `getNodeSize`: descobre o tamanho de nó da tabela que está sendo criada ou conectada. No
/// `xConnect` vem da raiz da árvore; no `xCreate` são 64 bytes a menos que o tamanho de página,
/// para cada nó caber numa página do banco (ou menos, se coubessem mais de `RTREE_MAXCELLS`).
fn get_node_size(
    db: &mut Connection,
    p_rtree: &mut Rtree,
    is_create: bool,
    pz_err: &mut Option<Vec<u8>>,
) -> i32 {
    let mut rc;
    if is_create {
        let mut i_page_size = 0;
        let z_sql = mprintf(b"PRAGMA %Q.page_size", &[text_arg(&p_rtree.z_db)]);
        rc = get_int_from_stmt(db, z_sql.as_deref(), &mut i_page_size);
        if rc == SQLITE_OK {
            p_rtree.i_node_size = i_page_size - 64;
            if (4 + p_rtree.n_bytes_per_cell as i32 * RTREE_MAXCELLS) < p_rtree.i_node_size {
                p_rtree.i_node_size = 4 + p_rtree.n_bytes_per_cell as i32 * RTREE_MAXCELLS;
            }
        } else {
            *pz_err = mprintf(b"%s", &[PrintfArg::Text(Some(errmsg(db)))]);
        }
    } else {
        let z_sql = mprintf(
            b"SELECT length(data) FROM '%q'.'%q_node' WHERE nodeno = 1",
            &[text_arg(&p_rtree.z_db), text_arg(&p_rtree.z_name)],
        );
        let mut size = p_rtree.i_node_size;
        rc = get_int_from_stmt(db, z_sql.as_deref(), &mut size);
        p_rtree.i_node_size = size;
        if rc != SQLITE_OK {
            *pz_err = mprintf(b"%s", &[PrintfArg::Text(Some(errmsg(db)))]);
        } else if p_rtree.i_node_size < (512 - 64) {
            rc = SQLITE_CORRUPT_VTAB;
            *pz_err = mprintf(
                b"undersize RTree blobs in \"%q_node\"",
                &[text_arg(&p_rtree.z_name)],
            );
        }
    }
    rc
}

/// `rtreeTokenLength`: o comprimento de um token.
fn rtree_token_length(z: &[u8]) -> usize {
    let mut dummy = 0;
    get_token(z, &mut dummy) as usize
}

/// O token que começa em `z`, cortado no comprimento dele.
fn rtree_token(z: &[u8]) -> &[u8] {
    &z[..rtree_token_length(z).min(z.len())]
}

/// `rtreeInit`: o trabalho de `xCreate` e `xConnect`. `argv` é `[módulo, banco, tabela, colunas...]`
/// e `aux` não nulo escolhe `rtree_i32`.
fn rtree_init_vtab(
    db: &mut Connection,
    aux: &Option<Rc<dyn Any>>,
    argv: &[Vec<u8>],
    pz_err: &mut Option<Vec<u8>>,
    is_create: bool,
) -> Result<Box<dyn Vtab>, i32> {
    const A_ERR_MSG: [&[u8]; 5] = [
        b"",
        b"Wrong number of columns for an rtree table",
        b"Too few columns for an rtree table",
        b"Too many columns for an rtree table",
        b"Auxiliary rtree columns must be last",
    ];
    let argc = argv.len() as i32;
    let e_coord_type = if aux.is_some() { RTREE_COORD_INT32 } else { RTREE_COORD_REAL32 };

    debug_assert!(RTREE_MAX_AUX_COLUMN < 256); // as colunas auxiliares são contadas por um u8
    if !(6..=RTREE_MAX_AUX_COLUMN + 3).contains(&argc) {
        *pz_err = mprintf(b"%s", &[text_arg(A_ERR_MSG[2 + (argc >= 6) as usize])]);
        return Err(SQLITE_ERROR);
    }

    vtab_config(db, SQLITE_VTAB_CONSTRAINT_SUPPORT, 1);
    vtab_config(db, SQLITE_VTAB_INNOCUOUS, 0);

    // Aloca a estrutura da tabela virtual.
    let mut p_rtree = Rtree::new(e_coord_type, argv[1].clone(), argv[2].clone());

    // Cria ou conecta ao esquema relacional por baixo. Se der certo, `sqlite3_declare_vtab()`
    // configura o esquema da tabela r-tree.
    let mut p_sql = StrAccum::new(db.a_limit[SQLITE_LIMIT_LENGTH as usize] as u32);
    p_sql.appendf(b"CREATE TABLE x(%s INT", &[text_arg(rtree_token(&argv[3]))]);
    let mut ii = 4usize;
    while ii < argv.len() {
        let z_arg = &argv[ii];
        if z_arg.first() == Some(&b'+') {
            p_rtree.n_aux += 1;
            p_sql.appendf(b",%s", &[text_arg(rtree_token(&z_arg[1..]))]);
        } else if p_rtree.n_aux > 0 {
            break;
        } else {
            const AZ_FORMAT: [&[u8]; 2] = [b",%s REAL", b",%s INT"];
            p_rtree.n_dim2 += 1;
            p_sql.appendf(AZ_FORMAT[e_coord_type as usize], &[text_arg(rtree_token(z_arg))]);
        }
        ii += 1;
    }
    p_sql.appendf(b");", &[]);
    let z_sql = p_sql.finish();
    let mut rc;
    match z_sql {
        None => rc = SQLITE_NOMEM,
        Some(z) => {
            if ii < argv.len() {
                *pz_err = mprintf(b"%s", &[text_arg(A_ERR_MSG[4])]);
                rc = SQLITE_ERROR;
            } else {
                rc = declare_vtab(db, &z);
                if rc != SQLITE_OK {
                    *pz_err = mprintf(b"%s", &[PrintfArg::Text(Some(errmsg(db)))]);
                }
            }
        }
    }

    'fail: {
        if rc != SQLITE_OK {
            break 'fail;
        }
        p_rtree.n_dim = p_rtree.n_dim2 / 2;
        let i_err: usize = if p_rtree.n_dim < 1 {
            2
        } else if p_rtree.n_dim2 as usize > RTREE_MAX_DIMENSIONS * 2 {
            3
        } else if p_rtree.n_dim2 % 2 != 0 {
            1
        } else {
            0
        };
        if i_err != 0 {
            *pz_err = mprintf(b"%s", &[text_arg(A_ERR_MSG[i_err])]);
            break 'fail;
        }
        p_rtree.n_bytes_per_cell = 8 + p_rtree.n_dim2 * 4;

        // Descobre o tamanho de nó a usar.
        rc = get_node_size(db, &mut p_rtree, is_create, pz_err);
        if rc != SQLITE_OK {
            break 'fail;
        }
        rc = rtree_sql_init(db, &mut p_rtree, &argv[1], &argv[2], is_create);
        if rc != SQLITE_OK {
            *pz_err = mprintf(b"%s", &[PrintfArg::Text(Some(errmsg(db)))]);
            break 'fail;
        }

        return Ok(Box::new(p_rtree));
    }

    if rc == SQLITE_OK {
        rc = SQLITE_ERROR;
    }
    debug_assert!(p_rtree.n_busy == 1);
    rtree_release(db, &mut p_rtree);
    Err(rc)
}

/// `rtreeModule`: o módulo `rtree`/`rtree_i32` (o `pAux` não nulo escolhe `rtree_i32`).
struct RtreeModule;

impl VtabModule for RtreeModule {
    fn i_version(&self) -> i32 {
        4
    }

    fn caps(&self) -> ModuleCaps {
        ModuleCaps {
            create: true,
            update: true,
            begin: true,
            sync: true,
            commit: true,
            rollback: true,
            find_function: false,
            rename: true,
            savepoint: true,
            release: false,
            rollback_to: false,
            integrity: true,
        }
    }

    /// `rtreeCreate`.
    fn x_create(
        &self,
        db: &mut Connection,
        aux: &Option<Rc<dyn Any>>,
        argv: &[Vec<u8>],
        err: &mut Option<Vec<u8>>,
    ) -> Result<Box<dyn Vtab>, i32> {
        rtree_init_vtab(db, aux, argv, err, true)
    }

    /// `rtreeConnect`.
    fn x_connect(
        &self,
        db: &mut Connection,
        aux: &Option<Rc<dyn Any>>,
        argv: &[Vec<u8>],
        err: &mut Option<Vec<u8>>,
    ) -> Result<Box<dyn Vtab>, i32> {
        rtree_init_vtab(db, aux, argv, err, false)
    }

    /// `rtreeShadowName`: verdadeiro se `z_name` é a extensão de uma tabela sombra do módulo.
    fn x_shadow_name(&self, z_name: &[u8]) -> bool {
        const AZ_NAME: [&[u8]; 3] = [b"node", b"parent", b"rowid"];
        AZ_NAME.iter().any(|n| crate::util::str_icmp(z_name, n) == 0)
    }
}

// ---------------------------------------------------------------------------------------------
// rtreenode(), rtreedepth() e rtreecheck()
// ---------------------------------------------------------------------------------------------

/// `rtreenode`: a função escalar que decodifica nós r-tree em texto, para depuração e análise.
/// Recebe o número de dimensões (1 a 5) e um blob com um nó; o texto é uma lista Tcl com uma
/// entrada por célula (o rowid de 8 bytes ou número de página, e as `2*dimensões` coordenadas).
/// `SELECT rtreenode(2, data) FROM rt_node;`
fn rtreenode(ctx: &mut Context<'_>, ap_arg: &[Mem]) {
    let n_dim = value_int(&ap_arg[0]) as u8;
    if !(1..=5).contains(&n_dim) {
        return;
    }
    let n_dim2 = n_dim as usize * 2;
    let n_bytes_per_cell = 8 + 8 * n_dim as usize;
    let blob = crate::json::blob_of(&ap_arg[1]);
    if blob.is_empty() {
        return;
    }
    let n_data = blob.len();
    if n_data < 4 {
        return;
    }
    let n_cell = read_int16(&blob[2..]) as usize;
    if n_data < n_cell * n_bytes_per_cell {
        return;
    }
    // O C lê até 4 bytes depois do blob no caso limite (`nData` entre `nCell*bytes` e
    // `4+nCell*bytes`); aqui esses bytes são zeros.
    let mut data = blob.into_owned();
    if data.len() < 4 + n_cell * n_bytes_per_cell {
        data.resize(4 + n_cell * n_bytes_per_cell, 0);
    }

    let mut p_out = StrAccum::new(SQLITE_MAX_LENGTH as u32);
    for ii in 0..n_cell {
        let cell = cell_from_data(n_bytes_per_cell, n_dim2, &data, ii);
        if ii > 0 {
            p_out.append(b" ");
        }
        p_out.appendf(b"{%lld", &[PrintfArg::Int(cell.i_rowid)]);
        for jj in 0..n_dim2 {
            p_out.appendf(b" %g", &[PrintfArg::Double(cell.a_coord[jj].f() as f64)]);
        }
        p_out.append(b"}");
    }
    let err_code = p_out.errcode();
    let z = p_out.finish();
    match z {
        Some(z) => result_text(ctx, Some(&z[..]), z.len() as i32, StrDtor::Transient),
        None => result_text(ctx, None, -1, StrDtor::Transient),
    }
    result_error_code(ctx, err_code);
}

/// `rtreedepth`: a função SQL que devolve o parâmetro "depth" do começo de um blob que é um nó
/// r-tree. `SELECT rtreedepth(data) FROM rt_node WHERE nodeno=1;` O valor é 0 em todos os nós
/// menos na raiz (`nodeno=1`). Só para testes e análise.
fn rtreedepth(ctx: &mut Context<'_>, ap_arg: &[Mem]) {
    let blob = crate::json::blob_of(&ap_arg[0]);
    if value_type(&ap_arg[0]) != SQLITE_BLOB || blob.len() < 2 {
        let msg = b"Invalid argument to rtreedepth()";
        result_error(ctx, msg, msg.len() as i32);
    } else {
        result_int(ctx, read_int16(&blob));
    }
}

/// `struct RtreeCheck`: o contexto compartilhado pelas rotinas de `rtreecheck()`.
struct RtreeCheck {
    /// Banco que contém a tabela r-tree.
    z_db: Option<Vec<u8>>,
    /// Nome da tabela r-tree.
    z_tab: Option<Vec<u8>>,
    /// Verdadeiro para uma tabela `rtree_i32`.
    b_int: bool,
    /// Número de dimensões.
    n_dim: i32,
    /// Comando que lê nós.
    p_get_node: Option<crate::connection::StmtId>,
    /// Comandos que consultam `%_parent` e `%_rowid`.
    a_check_mapping: [Option<crate::connection::StmtId>; 2],
    /// Número de células de folha da tabela.
    n_leaf: i64,
    /// Número de células que não são de folha.
    n_non_leaf: i64,
    /// Código de retorno.
    rc: i32,
    /// A mensagem a relatar.
    z_report: Option<Vec<u8>>,
    /// Número de linhas de `z_report`.
    n_err: i32,
}

/// Máximo de linhas do relatório.
const RTREE_CHECK_MAX_ERROR: i32 = 100;

/// O `PrintfArg` de um nome que pode ser nulo.
fn opt_arg(z: &Option<Vec<u8>>) -> PrintfArg {
    PrintfArg::Text(z.clone())
}

/// `rtreeCheckReset`: reinicia o comando; se `sqlite3_reset()` falha e `RtreeCheck.rc` é
/// `SQLITE_OK`, o erro vai para `rc`.
fn rtree_check_reset(db: &mut Connection, p_check: &mut RtreeCheck, p_stmt: crate::connection::StmtId) {
    let rc = reset(db, p_stmt);
    if p_check.rc == SQLITE_OK {
        p_check.rc = rc;
    }
}

/// `rtreeCheckPrepare`: formata o SQL e o compila. Sem sucesso devolve `None` e deixa o código
/// em `RtreeCheck.rc`.
fn rtree_check_prepare(
    db: &mut Connection,
    p_check: &mut RtreeCheck,
    z_fmt: &[u8],
    args: &[PrintfArg],
) -> Option<crate::connection::StmtId> {
    let z = mprintf(z_fmt, args);
    let mut p_ret = None;
    if p_check.rc == SQLITE_OK {
        match z {
            None => p_check.rc = SQLITE_NOMEM,
            Some(z) => {
                let (rc, stmt, _) = prepare_v2(db, &z, -1);
                p_check.rc = rc;
                p_ret = stmt;
            }
        }
    }
    p_ret
}

/// `rtreeCheckAppendMsg`: formata a mensagem e a acrescenta ao relatório.
fn rtree_check_append_msg(p_check: &mut RtreeCheck, z_fmt: &[u8], args: &[PrintfArg]) {
    if p_check.rc == SQLITE_OK && p_check.n_err < RTREE_CHECK_MAX_ERROR {
        match mprintf(z_fmt, args) {
            None => p_check.rc = SQLITE_NOMEM,
            Some(z) => {
                let mut report = p_check.z_report.take().map(|mut r| {
                    r.push(b'\n');
                    r
                }).unwrap_or_default();
                report.extend_from_slice(&z);
                p_check.z_report = Some(report);
            }
        }
        p_check.n_err += 1;
    }
}

/// `rtreeCheckGetNode`: sem erro anterior, carrega o conteúdo do nó `i_node` do banco. Devolve
/// `None` (e deixa o erro em `RtreeCheck.rc`) se não consegue.
fn rtree_check_get_node(db: &mut Connection, p_check: &mut RtreeCheck, i_node: i64) -> Option<Vec<u8>> {
    let mut p_ret: Option<Vec<u8>> = None;

    if p_check.rc == SQLITE_OK && p_check.p_get_node.is_none() {
        let args = [opt_arg(&p_check.z_db), opt_arg(&p_check.z_tab)];
        p_check.p_get_node =
            rtree_check_prepare(db, p_check, b"SELECT data FROM %Q.'%q_node' WHERE nodeno=?", &args);
    }

    if p_check.rc == SQLITE_OK {
        let p_get = p_check.p_get_node.unwrap_or_default();
        bind_int64(db, p_get, 1, i_node);
        if step(db, p_get) == SQLITE_ROW {
            let _n_node = column_bytes(db, p_get, 0);
            p_ret = Some(column_blob(db, p_get, 0).map(<[u8]>::to_vec).unwrap_or_default());
        }
        rtree_check_reset(db, p_check, p_get);
        if p_check.rc == SQLITE_OK && p_ret.is_none() {
            rtree_check_append_msg(p_check, b"Node %lld missing from database", &[PrintfArg::Int(i_node)]);
        }
    }

    p_ret
}

/// `rtreeCheckMapping`: confere se `%_parent` (se `!b_leaf`) ou `%_rowid` (se `b_leaf`) tem uma
/// entrada com a chave `i_key` e a segunda coluna igual a `i_val`.
fn rtree_check_mapping(
    db: &mut Connection,
    p_check: &mut RtreeCheck,
    b_leaf: usize,
    i_key: i64,
    i_val: i64,
) {
    const AZ_SQL: [&[u8]; 2] = [
        b"SELECT parentnode FROM %Q.'%q_parent' WHERE nodeno=?1",
        b"SELECT nodeno FROM %Q.'%q_rowid' WHERE rowid=?1",
    ];
    debug_assert!(b_leaf == 0 || b_leaf == 1);
    if p_check.a_check_mapping[b_leaf].is_none() {
        let args = [opt_arg(&p_check.z_db), opt_arg(&p_check.z_tab)];
        p_check.a_check_mapping[b_leaf] = rtree_check_prepare(db, p_check, AZ_SQL[b_leaf], &args);
    }
    if p_check.rc != SQLITE_OK {
        return;
    }

    let z_table: &[u8] = if b_leaf != 0 { b"%_rowid" } else { b"%_parent" };
    let p_stmt = p_check.a_check_mapping[b_leaf].unwrap_or_default();
    bind_int64(db, p_stmt, 1, i_key);
    let rc = step(db, p_stmt);
    if rc == SQLITE_DONE {
        rtree_check_append_msg(
            p_check,
            b"Mapping (%lld -> %lld) missing from %s table",
            &[PrintfArg::Int(i_key), PrintfArg::Int(i_val), text_arg(z_table)],
        );
    } else if rc == SQLITE_ROW {
        let ii = column_int64(db, p_stmt, 0);
        if ii != i_val {
            rtree_check_append_msg(
                p_check,
                b"Found (%lld -> %lld) in %s table, expected (%lld -> %lld)",
                &[
                    PrintfArg::Int(i_key),
                    PrintfArg::Int(ii),
                    text_arg(z_table),
                    PrintfArg::Int(i_key),
                    PrintfArg::Int(i_val),
                ],
            );
        }
    }
    rtree_check_reset(db, p_check, p_stmt);
}

/// `rtreeCheckCellCoord`: `p_cell` são as coordenadas de uma célula. Confere que são coerentes
/// entre si (sem x1>x2) e, se `p_parent` não é nulo (as coordenadas do pai que envolvem a página
/// da célula), que as duas séries são coerentes entre si.
fn rtree_check_cell_coord(
    p_check: &mut RtreeCheck,
    i_node: i64,
    i_cell: i32,
    p_cell: &[u8],
    p_parent: Option<&[u8]>,
) {
    for i in 0..p_check.n_dim as usize {
        let c1: RtreeCoord = read_coord(&p_cell[4 * 2 * i..]);
        let c2: RtreeCoord = read_coord(&p_cell[4 * (2 * i + 1)..]);

        let bad = if p_check.b_int { c1.i() > c2.i() } else { c1.f() > c2.f() };
        if bad {
            rtree_check_append_msg(
                p_check,
                b"Dimension %d of cell %d on node %lld is corrupt",
                &[PrintfArg::Int(i as i64), PrintfArg::Int(i_cell as i64), PrintfArg::Int(i_node)],
            );
        }

        if let Some(p_parent) = p_parent {
            let p1 = read_coord(&p_parent[4 * 2 * i..]);
            let p2 = read_coord(&p_parent[4 * (2 * i + 1)..]);
            let out = if p_check.b_int {
                c1.i() < p1.i() || c2.i() > p2.i()
            } else {
                c1.f() < p1.f() || c2.f() > p2.f()
            };
            if out {
                rtree_check_append_msg(
                    p_check,
                    b"Dimension %d of cell %d on node %lld is corrupt relative to parent",
                    &[PrintfArg::Int(i as i64), PrintfArg::Int(i_cell as i64), PrintfArg::Int(i_node)],
                );
            }
        }
    }
}

/// `rtreeCheckNode`: roda as conferências do `rtreecheck()` no nó `i_node`, que está à
/// profundidade `i_depth` (0 é folha). `a_parent` são as coordenadas que o envolvem no pai.
fn rtree_check_node(
    db: &mut Connection,
    p_check: &mut RtreeCheck,
    mut i_depth: i32,
    a_parent: Option<&[u8]>,
    i_node: i64,
) {
    debug_assert!(i_node == 1 || a_parent.is_some());
    debug_assert!(p_check.n_dim > 0);

    let Some(a_node) = rtree_check_get_node(db, p_check, i_node) else {
        return;
    };
    let n_node = a_node.len() as i32;
    if n_node < 4 {
        rtree_check_append_msg(
            p_check,
            b"Node %lld is too small (%d bytes)",
            &[PrintfArg::Int(i_node), PrintfArg::Int(n_node as i64)],
        );
    } else {
        if a_parent.is_none() {
            i_depth = read_int16(&a_node);
            if i_depth > RTREE_MAX_DEPTH {
                rtree_check_append_msg(
                    p_check,
                    b"Rtree depth out of range (%d)",
                    &[PrintfArg::Int(i_depth as i64)],
                );
                return;
            }
        }
        let n_cell = read_int16(&a_node[2..]);
        let cell_size = 8 + p_check.n_dim * 2 * 4;
        if (4 + n_cell * cell_size) > n_node {
            rtree_check_append_msg(
                p_check,
                b"Node %lld is too small for cell count of %d (%d bytes)",
                &[PrintfArg::Int(i_node), PrintfArg::Int(n_cell as i64), PrintfArg::Int(n_node as i64)],
            );
        } else {
            for i in 0..n_cell {
                let off = (4 + i * cell_size) as usize;
                let p_cell = &a_node[off..];
                let i_val = read_int64(p_cell);
                rtree_check_cell_coord(p_check, i_node, i, &p_cell[8..], a_parent);

                if i_depth > 0 {
                    rtree_check_mapping(db, p_check, 0, i_val, i_node);
                    rtree_check_node(db, p_check, i_depth - 1, Some(&p_cell[8..]), i_val);
                    p_check.n_non_leaf += 1;
                } else {
                    rtree_check_mapping(db, p_check, 1, i_val, i_node);
                    p_check.n_leaf += 1;
                }
            }
        }
    }
}

/// `rtreeCheckCount`: `z_tbl` é `"_rowid"` ou `"_parent"`. Confere que a tabela `%_rowid` ou
/// `%_parent` tem exatamente `n_expect` entradas; se não, acrescenta uma mensagem ao relatório.
fn rtree_check_count(db: &mut Connection, p_check: &mut RtreeCheck, z_tbl: &[u8], n_expect: i64) {
    if p_check.rc == SQLITE_OK {
        let args = [opt_arg(&p_check.z_db), opt_arg(&p_check.z_tab), text_arg(z_tbl)];
        let p_count = rtree_check_prepare(db, p_check, b"SELECT count(*) FROM %Q.'%q%s'", &args);
        if let Some(p_count) = p_count {
            if step(db, p_count) == SQLITE_ROW {
                let n_actual = column_int64(db, p_count, 0);
                if n_actual != n_expect {
                    rtree_check_append_msg(
                        p_check,
                        b"Wrong number of entries in %%%s table - expected %lld, actual %lld",
                        &[text_arg(z_tbl), PrintfArg::Int(n_expect), PrintfArg::Int(n_actual)],
                    );
                }
            }
            p_check.rc = finalize(db, p_count);
        }
    }
}

/// `rtreeCheckTable`: o grosso da verificação de integridade, chamado por `rtreecheck()` e pelo
/// `xIntegrity`. O relatório (se há) sai em `pz_report`.
fn rtree_check_table(
    db: &mut Connection,
    z_db: Option<&[u8]>,
    z_tab: Option<&[u8]>,
    pz_report: &mut Option<Vec<u8>>,
) -> i32 {
    let mut check = RtreeCheck {
        z_db: z_db.map(<[u8]>::to_vec),
        z_tab: z_tab.map(<[u8]>::to_vec),
        b_int: false,
        n_dim: 0,
        p_get_node: None,
        a_check_mapping: [None, None],
        n_leaf: 0,
        n_non_leaf: 0,
        rc: SQLITE_OK,
        z_report: None,
        n_err: 0,
    };
    let mut n_aux = 0;
    let args = [opt_arg(&check.z_db), opt_arg(&check.z_tab)];

    // Acha o número de colunas auxiliares.
    let p_stmt = rtree_check_prepare(db, &mut check, b"SELECT * FROM %Q.'%q_rowid'", &args);
    if let Some(p_stmt) = p_stmt {
        n_aux = column_count(db, p_stmt) - 2;
        finalize(db, p_stmt);
    } else if check.rc != SQLITE_NOMEM {
        check.rc = SQLITE_OK;
    }

    // Acha o número de dimensões do r-tree.
    let p_stmt = rtree_check_prepare(db, &mut check, b"SELECT * FROM %Q.%Q", &args);
    if let Some(p_stmt) = p_stmt {
        check.n_dim = (column_count(db, p_stmt) - 1 - n_aux) / 2;
        if check.n_dim < 1 {
            rtree_check_append_msg(&mut check, b"Schema corrupt or not an rtree", &[]);
        } else if SQLITE_ROW == step(db, p_stmt) {
            check.b_int = column_type(db, p_stmt, 1) == SQLITE_INTEGER;
        }
        let rc = finalize(db, p_stmt);
        if rc != SQLITE_CORRUPT {
            check.rc = rc;
        }
    }

    // A verificação de fato.
    if check.n_dim >= 1 {
        if check.rc == SQLITE_OK {
            rtree_check_node(db, &mut check, 0, None, 1);
        }
        let n_leaf = check.n_leaf;
        rtree_check_count(db, &mut check, b"_rowid", n_leaf);
        let n_non_leaf = check.n_non_leaf;
        rtree_check_count(db, &mut check, b"_parent", n_non_leaf);
    }

    // Finaliza os comandos SQL usados pela verificação.
    finalize_stmt(db, &mut check.p_get_node);
    finalize_stmt(db, &mut check.a_check_mapping[0]);
    finalize_stmt(db, &mut check.a_check_mapping[1]);

    *pz_report = check.z_report.take();
    check.rc
}

/// `rtreecheck`: `rtreecheck(<tabela-rtree>)` ou `rtreecheck(<banco>, <tabela-rtree>)` roda uma
/// verificação de integridade na tabela r-tree. Confere, para cada célula da estrutura (tabela
/// `%_node`), que (a) em cada dimensão coord1 <= coord2, (b) fora da raiz, a célula está contida
/// na do pai, (c) numa folha, a tabela `%_rowid` tem uma entrada para o rowid que aponta para o
/// nó certo, (d) num nó interno, a tabela `%_parent` mapeia o filho para o nó em que a célula
/// está. Confere também que `%_rowid` e `%_parent` têm tantas entradas quantas células de folha
/// e de nó interno, e que cada entrada tem a célula correspondente.
fn rtreecheck(ctx: &mut Context<'_>, ap_arg: &[Mem]) {
    let n_arg = ap_arg.len();
    if n_arg != 1 && n_arg != 2 {
        let msg = b"wrong number of arguments to function rtreecheck()";
        result_error(ctx, msg, msg.len() as i32);
    } else {
        let mut z_report: Option<Vec<u8>> = None;
        let mut z_db: Option<Vec<u8>> = text_of(&ap_arg[0]).map(|c| c.into_owned());
        let z_tab: Option<Vec<u8>>;
        if n_arg == 1 {
            z_tab = z_db.take();
            z_db = Some(b"main".to_vec());
        } else {
            z_tab = text_of(&ap_arg[1]).map(|c| c.into_owned());
        }
        let rc = rtree_check_table(ctx.db, z_db.as_deref(), z_tab.as_deref(), &mut z_report);
        if rc == SQLITE_OK {
            let z: &[u8] = z_report.as_deref().unwrap_or(b"ok");
            result_text(ctx, Some(z), z.len() as i32, StrDtor::Transient);
        } else {
            result_error_code(ctx, rc);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Registro e callbacks de MATCH
// ---------------------------------------------------------------------------------------------

/// `sqlite3RtreeInit`: registra o módulo `rtree` e o `rtree_i32` e as funções SQL `rtreenode`,
/// `rtreedepth` e `rtreecheck` na conexão.
pub fn rtree_init(db: &mut Connection) -> i32 {
    let utf8 = SQLITE_UTF8;
    let mut rc = create_function_api(
        db, b"rtreenode", 2, utf8, UserData::None, Some(rtreenode), None, None, None, None, None,
    );
    if rc == SQLITE_OK {
        rc = create_function_api(
            db, b"rtreedepth", 1, utf8, UserData::None, Some(rtreedepth), None, None, None, None, None,
        );
    }
    if rc == SQLITE_OK {
        rc = create_function_api(
            db, b"rtreecheck", -1, utf8, UserData::None, Some(rtreecheck), None, None, None, None,
            None,
        );
    }
    if rc == SQLITE_OK {
        // `pAux` nulo é `RTREE_COORD_REAL32`.
        rc = create_module(db, b"rtree", Some(Rc::new(RtreeModule)), None, None);
    }
    if rc == SQLITE_OK {
        let c: Rc<dyn Any> = Rc::new(RTREE_COORD_INT32);
        rc = create_module(db, b"rtree_i32", Some(Rc::new(RtreeModule)), Some(c), None);
    }
    rc
}

/// `geomCallback`: cada chamada de `sqlite3_rtree_geometry_callback()` ou
/// `sqlite3_rtree_query_callback()` cria uma função SQL comum, que é esta. Ela só monta o
/// `RtreeMatchArg` (os callbacks e a lista de parâmetros da função) e o devolve como valor
/// ponteiro, que o operador MATCH do r-tree lê para saber quais elementos devolver.
fn geom_callback(ctx: &mut Context<'_>, a_arg: &[Mem]) {
    let UserData::Ptr(p_user) = user_data(ctx) else {
        return;
    };
    let Some(p_geom_ctx) = p_user.downcast_ref::<RtreeGeomCallback>() else {
        return;
    };

    let mut mem_err = false;
    let mut ap_sql_param: Vec<Mem> = Vec::with_capacity(a_arg.len());
    let mut a_param: Vec<f64> = Vec::with_capacity(a_arg.len());
    for arg in a_arg {
        match value_dup(arg) {
            Some(v) => ap_sql_param.push(v),
            None => {
                mem_err = true;
                ap_sql_param.push(Mem::value_new());
            }
        }
        a_param.push(value_double(arg));
    }
    if mem_err {
        result_error_nomem(ctx);
    } else {
        let p_blob = RtreeMatchArg {
            cb: p_geom_ctx.clone(),
            n_param: a_arg.len() as i32,
            ap_sql_param: Rc::new(ap_sql_param),
            a_param,
        };
        mem_set_pointer(&mut ctx.out, Box::new(p_blob), b"RtreeMatchArg");
    }
}

/// `sqlite3_rtree_geometry_callback`: registra uma função de geometria para o operador MATCH do
/// r-tree.
pub fn rtree_geometry_callback(
    db: &mut Connection,
    z_geom: &[u8],
    x_geom: RtreeGeomFn,
    p_context: Option<Rc<dyn Any>>,
) -> i32 {
    // Aloca e preenche o objeto de contexto.
    let p_geom_ctx = RtreeGeomCallback { x_geom: Some(x_geom), x_query_func: None, p_context };
    create_function_api(
        db,
        z_geom,
        -1,
        SQLITE_ANY,
        UserData::Ptr(Rc::new(p_geom_ctx)),
        Some(geom_callback),
        None,
        None,
        None,
        None,
        None,
    )
}

/// `sqlite3_rtree_query_callback`: registra uma função de geometria de segunda geração para o
/// operador MATCH do r-tree. `x_destructor` recebe o `p_context` quando a função SQL some (o
/// `rtreeFreeCallback`).
pub fn rtree_query_callback(
    db: &mut Connection,
    z_query_func: &[u8],
    x_query_func: RtreeQueryFn,
    p_context: Option<Rc<dyn Any>>,
    x_destructor: Option<DestroyFn>,
) -> i32 {
    let p_geom_ctx = RtreeGeomCallback { x_geom: None, x_query_func: Some(x_query_func), p_context };
    let x_destroy: Option<DestroyFn> = x_destructor.map(|d| {
        Box::new(move |p: Option<Rc<dyn Any>>| {
            let ctx = p
                .as_ref()
                .and_then(|u| u.downcast_ref::<RtreeGeomCallback>())
                .and_then(|g| g.p_context.clone());
            d(ctx);
        }) as DestroyFn
    });
    create_function_api(
        db,
        z_query_func,
        -1,
        SQLITE_ANY,
        UserData::Ptr(Rc::new(p_geom_ctx)),
        Some(geom_callback),
        None,
        None,
        None,
        None,
        x_destroy,
    )
}
