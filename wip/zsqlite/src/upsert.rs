//! `upsert.c`: chunk `upsert_c.000` do SQLite 3.46.1. Aspectos do processamento de UPSERT e do
//! objeto `Upsert`: `sqlite3UpsertNew`, `sqlite3UpsertDup`, `sqlite3UpsertAnalyzeTarget`,
//! `sqlite3UpsertNextIsIPK`, `sqlite3UpsertOfIndex` e `sqlite3UpsertDoUpdate`.
//!
//! Convenções (as mesmas de `insert.rs`, `insert2.rs` e `delete.rs`, ver CONVENTIONS.md):
//!
//! - Funções de código recebem `(db: &mut Connection, parse: &mut Parse, ...)`; o `pParse->db`
//!   some. O `Vdbe` é `parse.p_vdbe`.
//! - A lista de `Upsert` é a cadeia `p_next_upsert` possuída em ordem. O `pUpsert` que o C passa
//!   como ponteiro para um elo vira `&Upsert` quando só se lê, e a posição do elo (`usize`) quando
//!   a rotina precisa comparar identidade de elos ou alterá-lo.
//! - `sqlite3UpsertDelete` (e `upsertDelete`) é o `Drop` do `Box<Upsert>`: não existe como função.
//!   Os `sqlite3...Delete` dos ramos de falha de alocação de `sqlite3UpsertNew` também (a
//!   alocação de `Box` não falha).
//! - `sqlite3UpsertNew` e `sqlite3UpsertDup` não recebem `db`: só serviam ao alocador do C.
//! - `sqlite3UpsertAnalyzeTarget` recebe a lista FROM por referência imutável, como o chamador em
//!   `insert.rs`. O `NameContext` aponta para uma cópia dela: o único efeito do C sobre a lista
//!   original é o `colUsed` que a resolução liga em `pTabList->a[0]`, e o INSERT nunca o lê (o
//!   UPDATE do `DO UPDATE` zera o `colUsed` da sua própria cópia, `pUpsertSrc`).
//! - `pUpsert->pUpsertIdx` é `Option<Rc<Index>>`: a comparação de ponteiros do C é `Rc::ptr_eq`.
//! - `sqlite3UpsertDoUpdate` recebe o `Upsert` da cabeça (o `pTop` do C); o elo que se aplica é
//!   achado por `sqlite3UpsertOfIndex` e passado a `sqlite3Update`, que só o lê.
//! - Ramos `SQLITE_DEBUG` (`VdbeCoverage`, `sqlite3VdbeVerifyAbortable`) não existem.
//!   `VdbeComment` e `VdbeNoopComment` valem (o Debian liga `SQLITE_ENABLE_EXPLAIN_COMMENTS`).

use std::rc::Rc;

pub(crate) use crate::vdbeaux::{vdbe_of_parse, current_addr};
use crate::build::{primary_key_index, table_column_to_index, text_arg};
use crate::build3::may_abort;
use crate::connection::{Connection, Parse};
use crate::consts::{
    OE_ABORT, OP_COLUMN, OP_FOUND, OP_HALT, OP_IDXROWID, OP_REALAFFINITY, OP_SEEKROWID,
    SQLITE_AFF_REAL, SQLITE_CORRUPT, SQLITE_ERROR, SQLITE_OK, TK_COLLATE, TK_COLUMN, XN_EXPR,
    XN_ROWID,
};
use crate::expr::{expr_dup, expr_list_dup, src_list_dup};
use crate::expr_code2::{expr_compare, get_temp_reg, release_temp_reg};
use crate::printf::{mprintf, PrintfArg};
use crate::resolve::{name_context_new, resolve_expr_list_names, resolve_expr_names};
use crate::sqlite_int::{Expr, ExprList, ExprU, Index, SrcList, Table, Upsert};
use crate::update::update;
use crate::util::error_msg;
use crate::vdbe_types::{Vdbe, P4};
use crate::vdbeaux::{add_op1, add_op2, add_op3, add_op4, add_op4_int, jump_here, noop_comment,
    vdbe_comment};

// ---------------------------------------------------------------------------------------------
// Auxiliares privados (não existem no C)
// ---------------------------------------------------------------------------------------------


/// O `n`-ésimo elo (a partir de 0) da cadeia de `Upsert`, ou `None` se a cadeia é mais curta.
fn clause_at(head: &Upsert, n: usize) -> Option<&Upsert> {
    let mut cur = head;
    for _ in 0..n {
        cur = cur.p_next_upsert.as_deref()?;
    }
    Some(cur)
}

/// O `n`-ésimo elo (a partir de 0), para alteração. O chamador só passa posições que existem.
fn clause_at_mut(head: &mut Upsert, n: usize) -> &mut Upsert {
    let mut cur = head;
    for _ in 0..n {
        cur = cur.p_next_upsert.as_deref_mut().expect("cadeia de upsert");
    }
    cur
}

/// A comparação `pUpsert->pUpsertIdx != pIdx` do C, que compara ponteiros (nulo com nulo é igual).
fn idx_differs(p: &Upsert, p_idx: Option<&Rc<Index>>) -> bool {
    match (&p.p_upsert_idx, p_idx) {
        (None, None) => false,
        (Some(a), Some(b)) => !Rc::ptr_eq(a, b),
        _ => true,
    }
}

/// `sqlite3UpsertOfIndex` devolvendo a POSIÇÃO do elo na cadeia (o C devolve o ponteiro e o
/// chamador compara identidade; aqui a identidade é a posição). `None` é o ponteiro nulo.
fn of_index_pos(head: &Upsert, p_idx: Option<&Rc<Index>>) -> Option<usize> {
    let mut cur = Some(head);
    let mut n = 0usize;
    while let Some(p) = cur {
        if p.p_upsert_target.is_none() || !idx_differs(p, p_idx) {
            return Some(n);
        }
        cur = p.p_next_upsert.as_deref();
        n += 1;
    }
    None
}

// ---------------------------------------------------------------------------------------------
// chunk 000
// ---------------------------------------------------------------------------------------------

/// `sqlite3UpsertNew`: cria um objeto `Upsert` novo. Os `Box` entregues passam a ser dele.
pub fn upsert_new(
    p_target: Option<Box<ExprList>>,
    p_target_where: Option<Box<Expr>>,
    p_set: Option<Box<ExprList>>,
    p_where: Option<Box<Expr>>,
    p_next: Option<Box<Upsert>>,
) -> Option<Box<Upsert>> {
    let is_do_update = p_set.is_some();
    Some(Box::new(Upsert {
        p_upsert_target: p_target,
        p_upsert_target_where: p_target_where,
        p_upsert_set: p_set,
        p_upsert_where: p_where,
        is_do_update,
        p_next_upsert: p_next,
        ..Upsert::default()
    }))
}

/// `sqlite3UpsertDup`: duplica um objeto `Upsert` (e toda a cadeia dele).
pub fn upsert_dup(p: Option<&Upsert>) -> Option<Box<Upsert>> {
    let p = p?;
    upsert_new(
        expr_list_dup(p.p_upsert_target.as_deref(), 0),
        expr_dup(p.p_upsert_target_where.as_deref(), 0),
        expr_list_dup(p.p_upsert_set.as_deref(), 0),
        expr_dup(p.p_upsert_where.as_deref(), 0),
        upsert_dup(p.p_next_upsert.as_deref()),
    )
}

/// `sqlite3UpsertAnalyzeTarget`: analisa a cláusula ON CONFLICT que começa no elo `k` da cadeia
/// `p_all` (`pUpsert` do C é o elo `k` e `pAll` é a cabeça). Resolve todos os símbolos do alvo do
/// conflito. Devolve `SQLITE_OK` se tudo dá certo, ou um código de erro se algo está errado.
///
/// Como no C, o laço percorre também as cláusulas seguintes a `k` (o INSERT chama esta rotina uma
/// vez por cláusula com alvo), e o ordinal da mensagem de erro conta a partir de `k`.
pub fn upsert_analyze_target(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab_list: &SrcList,
    p_all: &mut Upsert,
    k: usize,
) -> i32 {
    debug_assert!(p_tab_list.a.len() == 1);
    debug_assert!(p_tab_list.a[0].p_tab.is_some());
    debug_assert!(clause_at(p_all, k).map_or(false, |c| c.p_upsert_target.is_some()));

    // Cópia da lista FROM para o contexto de nomes (ver o cabeçalho).
    let mut src: SrcList = p_tab_list.clone();
    let i_cursor: i32 = src.a[0].i_cursor; // cursor usado por pTab
    let Some(p_tab): Option<Rc<Table>> = src.a[0].p_tab.clone() else {
        return SQLITE_ERROR;
    };

    // Resolve todos os nomes simbólicos da cláusula do alvo do conflito, que inclui tanto a lista
    // de colunas quanto a cláusula WHERE opcional do índice parcial.
    let mut s_nc = name_context_new();
    s_nc.p_src_list = Some(&mut src);

    let mut n_clause: usize = 0; // contador das cláusulas ON CONFLICT
    loop {
        let pos = k + n_clause;
        let has_target = clause_at(p_all, pos).map_or(false, |c| c.p_upsert_target.is_some());
        if !has_target {
            break;
        }
        'body: {
            {
                let c = clause_at_mut(p_all, pos);
                let rc = resolve_expr_list_names(db, parse, &mut s_nc, c.p_upsert_target.as_deref_mut());
                if rc != 0 {
                    return rc;
                }
                let rc = resolve_expr_names(db, parse, &mut s_nc, c.p_upsert_target_where.as_deref_mut());
                if rc != 0 {
                    return rc;
                }
            }

            // Confere se o alvo do conflito casa com o rowid.
            let mut found_idx: Option<Rc<Index>> = None;
            {
                let Some(c) = clause_at(p_all, pos) else {
                    break 'body;
                };
                let Some(p_target) = c.p_upsert_target.as_deref() else {
                    break 'body;
                };
                if p_tab.has_rowid()
                    && p_target.a.len() == 1
                    && p_target.a[0]
                        .p_expr
                        .as_deref()
                        .map_or(false, |t| t.op == TK_COLUMN && t.i_column == XN_ROWID as i32)
                {
                    // O alvo do conflito é o rowid da tabela primária.
                    debug_assert!(c.p_upsert_idx.is_none());
                    break 'body;
                }

                // Compara com os outros índices. Cada coluna do índice vira uma árvore
                // `TK_COLLATE(TK_COLUMN)` (sCol[0] e sCol[1] do C) comparada com o alvo.
                for p_idx in p_tab.p_index.iter() {
                    if !p_idx.is_unique_index() {
                        continue;
                    }
                    if p_target.a.len() != p_idx.n_key_col as usize {
                        continue;
                    }
                    if let Some(p_part) = p_idx.p_partial_idx_where.as_deref() {
                        if c.p_upsert_target_where.is_none() {
                            continue;
                        }
                        if expr_compare(
                            Some((&mut *db, &mut *parse)),
                            c.p_upsert_target_where.as_deref(),
                            Some(p_part),
                            i_cursor,
                        ) != 0
                        {
                            continue;
                        }
                    }
                    let nn = p_idx.n_key_col as usize;
                    let mut ii = 0usize;
                    while ii < nn {
                        // `sCol[0].u.zToken = pIdx->azColl[ii]`.
                        let mut s_col0 = Expr::default();
                        s_col0.op = TK_COLLATE;
                        s_col0.u = ExprU::Token(Some(p_idx.az_coll[ii].clone()));
                        let p_expr: Box<Expr>;
                        if p_idx.ai_column[ii] == XN_EXPR {
                            debug_assert!(p_idx.a_col_expr.is_some());
                            debug_assert!(p_idx.b_has_expr);
                            let e = p_idx
                                .a_col_expr
                                .as_deref()
                                .and_then(|l| l.a.get(ii))
                                .and_then(|it| it.p_expr.as_deref());
                            debug_assert!(e.is_some());
                            let Some(e) = e else {
                                break;
                            };
                            if e.op != TK_COLLATE {
                                s_col0.p_left = expr_dup(Some(e), 0);
                                p_expr = Box::new(s_col0);
                            } else {
                                match expr_dup(Some(e), 0) {
                                    Some(d) => p_expr = d,
                                    None => break,
                                }
                            }
                        } else {
                            let mut s_col1 = Expr::default();
                            s_col1.op = TK_COLUMN;
                            s_col1.i_table = i_cursor;
                            s_col1.i_column = p_idx.ai_column[ii] as i32;
                            s_col0.p_left = Some(Box::new(s_col1));
                            p_expr = Box::new(s_col0);
                        }
                        let mut jj = 0usize;
                        while jj < nn {
                            if expr_compare(None, p_target.a[jj].p_expr.as_deref(), Some(&p_expr), i_cursor)
                                < 2
                            {
                                break; // a coluna ii do índice casa com a coluna jj do alvo
                            }
                            jj += 1;
                        }
                        if jj >= nn {
                            // O alvo não tem termo que case com a coluna jj do índice.
                            break;
                        }
                        ii += 1;
                    }
                    if ii < nn {
                        // A coluna ii do índice não casou com nenhum termo do alvo do conflito.
                        // Continua a busca com o próximo índice.
                        continue;
                    }
                    found_idx = Some(Rc::clone(p_idx));
                    break;
                }
            }

            if let Some(p_idx) = found_idx {
                clause_at_mut(p_all, pos).p_upsert_idx = Some(Rc::clone(&p_idx));
                if of_index_pos(p_all, Some(&p_idx)) != Some(pos) {
                    // Na verdade isto deveria ser um erro: a cláusula ON CONFLICT isDup nunca
                    // dispara. Mas o problema só foi descoberto três anos depois de o
                    // multi-CONFLICT upsert ser acrescentado, então ele é ignorado em silêncio
                    // para não quebrar aplicações que podem ter cláusulas ON CONFLICT
                    // redundantes.
                    clause_at_mut(p_all, pos).is_dup = true;
                }
            }
            let (no_idx, has_next) = match clause_at(p_all, pos) {
                Some(c) => (c.p_upsert_idx.is_none(), c.p_next_upsert.is_some()),
                None => break 'body,
            };
            if no_idx {
                let z_which: Vec<u8> = if n_clause == 0 && !has_next {
                    Vec::new()
                } else {
                    mprintf(b"%r ", &[PrintfArg::Int((n_clause + 1) as i64)]).unwrap_or_default()
                };
                error_msg(
                    db,
                    parse,
                    b"%sON CONFLICT clause does not match any PRIMARY KEY or UNIQUE constraint",
                    &[PrintfArg::Text(Some(z_which))],
                );
                return SQLITE_ERROR;
            }
        }
        n_clause += 1;
    }
    SQLITE_OK
}

/// `sqlite3UpsertNextIsIPK`: verdadeiro se `p_upsert` é a última cláusula ON CONFLICT com alvo de
/// conflito, ou se é seguida por outra cláusula ON CONFLICT que tem como alvo o INTEGER PRIMARY
/// KEY.
pub fn upsert_next_is_ipk(p_upsert: &Upsert) -> bool {
    let mut p_next = p_upsert.p_next_upsert.as_deref();
    loop {
        let Some(n) = p_next else {
            return true;
        };
        if n.p_upsert_target.is_none() {
            return true;
        }
        if n.p_upsert_idx.is_none() {
            return true;
        }
        if !n.is_dup {
            return false;
        }
        p_next = n.p_next_upsert.as_deref();
    }
}

/// `sqlite3UpsertOfIndex`: dada a lista de cláusulas ON CONFLICT e um índice `p_idx`, devolve a
/// cláusula ON CONFLICT que se aplica ao índice. Ou, se o índice não está sujeito a nenhuma
/// cláusula, devolve `None`. Com `p_idx` nulo (`None`) acha a cláusula do IPK.
pub fn upsert_of_index<'a>(p_upsert: &'a Upsert, p_idx: Option<&Rc<Index>>) -> Option<&'a Upsert> {
    let mut cur = Some(p_upsert);
    while let Some(p) = cur {
        if p.p_upsert_target.is_some() && idx_differs(p, p_idx) {
            cur = p.p_next_upsert.as_deref();
        } else {
            return Some(p);
        }
    }
    None
}

/// `sqlite3UpsertDoUpdate`: gera o bytecode que faz um UPDATE como parte de um upsert.
///
/// Se `p_idx` é `None`, a restrição UNIQUE que falhou foi o IPK. Nesse caso `i_cur` é um cursor
/// aberto na árvore da tabela que aponta para a linha conflitante. Senão, `p_idx` é a restrição
/// que falhou e `i_cur` é um cursor que aponta para a linha conflitante.
pub fn upsert_do_update(
    db: &mut Connection,
    parse: &mut Parse,
    p_upsert: &Upsert,
    p_tab: &Rc<Table>,
    p_idx: Option<&Rc<Index>>,
    i_cur: i32,
) {
    let p_top = p_upsert;
    debug_assert!(parse.p_vdbe.is_some());
    let i_data_cur = p_upsert.i_data_cur;
    let Some(p_clause) = upsert_of_index(p_top, p_idx) else {
        return;
    };
    noop_comment(vdbe_of_parse(parse), b"Begin DO UPDATE of UPSERT", &[]);
    if let Some(p_idx) = p_idx {
        if i_cur != i_data_cur {
            if p_tab.has_rowid() {
                let reg_rowid = get_temp_reg(parse);
                let v = vdbe_of_parse(parse);
                add_op2(v, OP_IDXROWID as i32, i_cur, reg_rowid);
                add_op3(v, OP_SEEKROWID as i32, i_data_cur, 0, reg_rowid);
                release_temp_reg(parse, reg_rowid);
            } else {
                let Some(p_pk) = primary_key_index(p_tab).cloned() else {
                    return;
                };
                let n_pk = p_pk.n_key_col as i32;
                let i_pk = parse.n_mem + 1;
                parse.n_mem += n_pk;
                for i in 0..n_pk {
                    debug_assert!(p_pk.ai_column[i as usize] >= 0);
                    let col = p_pk.ai_column[i as usize];
                    let k = table_column_to_index(p_idx, col) as i32;
                    let v = vdbe_of_parse(parse);
                    add_op3(v, OP_COLUMN as i32, i_cur, k, i_pk + i);
                    vdbe_comment(
                        v,
                        b"%s.%s",
                        &[text_arg(&p_idx.z_name), text_arg(&p_tab.a_col[col as usize].z_cn_name)],
                    );
                }
                let v = vdbe_of_parse(parse);
                let i = add_op4_int(v, OP_FOUND as i32, i_data_cur, 0, i_pk, n_pk);
                add_op4(
                    v,
                    OP_HALT as i32,
                    SQLITE_CORRUPT,
                    OE_ABORT as i32,
                    0,
                    P4::Text(b"corrupt database".to_vec()),
                );
                may_abort(parse);
                jump_here(vdbe_of_parse(parse), i);
            }
        }
    }
    // `pUpsert` não é dono de `pTop->pUpsertSrc`: o INSERT externo é. Então é preciso fazer uma
    // cópia antes de passá-la a `sqlite3Update()`.
    let p_src = src_list_dup(p_top.p_upsert_src.as_deref(), 0);
    // As colunas `excluded.*` do tipo REAL precisam ser convertidas num real de verdade.
    for i in 0..p_tab.n_col as i32 {
        if p_tab.a_col[i as usize].affinity == SQLITE_AFF_REAL {
            add_op1(vdbe_of_parse(parse), OP_REALAFFINITY as i32, p_top.reg_data + i);
        }
    }
    update(
        db,
        parse,
        p_src,
        expr_list_dup(p_clause.p_upsert_set.as_deref(), 0),
        expr_dup(p_clause.p_upsert_where.as_deref(), 0),
        OE_ABORT as i32,
        None,
        None,
        Some(p_clause),
    );
    noop_comment(vdbe_of_parse(parse), b"End DO UPDATE of UPSERT", &[]);
}
