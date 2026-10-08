//! `insert.c` (segunda metade): chunks `insert_c.004` a `insert_c.008` do SQLite 3.46.1.
//! `sqlite3ExprReferencesUpdatedColumn`, `sqlite3GenerateConstraintChecks`,
//! `sqlite3CompleteInsertion`, `sqlite3OpenTableAndIndices` e a otimização de transferência
//! (`xferOptimization`, `xferCompatibleIndex`).
//!
//! Convenções (as mesmas de `insert.rs`, ver o cabeçalho dele e CONVENTIONS.md):
//!
//! - Funções de código recebem `(db: &mut Connection, parse: &mut Parse, ...)`; o `pParse->db`
//!   some. O `Vdbe` é `parse.p_vdbe`; as funções do `vdbeaux` recebem `&mut Vdbe`.
//! - `Table` e `Index` do esquema são `Rc` imutáveis; o `Index` não tem `pTable` nem `pNext`: a
//!   lista de índices é `Table.p_index` e a tabela dona vem por parâmetro.
//! - Opções do Debian: `SQLITE_ENABLE_PREUPDATE_HOOK` está ligada (então vale o `OP_Delete` com
//!   `OPFLAG_ISNOOP`, o `codeWithoutRowidPreupdate` e o ramo `OP_RowData` da transferência, e o
//!   bloco "collision detection may be omitted" de `sqlite3GenerateConstraintChecks`, que só
//!   existe sem a opção, some). `SQLITE_ENABLE_NULL_TRIM`, `SQLITE_ENABLE_HIDDEN_COLUMNS`,
//!   `SQLITE_TEST` (`sqlite3_xferopt_count`) e `SQLITE_DEBUG` (`sqlite3VdbeVerifyAbortable`,
//!   `VdbeModuleComment`, `VdbeCoverage`) não existem; `sqlite3SetMakeRecordP5` é macro vazia.
//! - O `IndexIterator` do C (lista encadeada ou vetor de `IndexListTerm`) vira um `Vec<usize>`
//!   com as posições em `Table.p_index` na ordem de visita. Ao fim do laço o `ix` do C vale
//!   sempre o número de índices, que é onde mora o registrador do registro da tabela.
//! - `pUpsertClause` do C (ponteiro para um elo da cadeia `pNextUpsert`) vira a posição do elo
//!   (`Option<usize>`): `pUpsertClause==pUpsert` é `Some(0)`. O `pUpsert->pToFree` some (o vetor
//!   de índices é local).
//! - `sqlite3FaultSim(411)` devolve sempre `SQLITE_OK` fora dos testes, então não é chamado.

use std::rc::Rc;

pub(crate) use crate::vdbeaux::{vdbe_of_parse, current_addr};
use crate::build::{
    column_coll, column_expr, locate_table_item, primary_key_index, table_column_to_index,
    table_column_to_storage, table_lock, text_arg,
};
use crate::build3::{
    code_verify_schema, halt_constraint, may_abort, multi_write, rowid_constraint,
    unique_constraint,
};
use crate::callback::locate_coll_seq;
use crate::connection::{Connection, Parse};
use crate::consts::opcodes::OPCODE_PROPERTY;
use crate::consts::{
    COLFLAG_GENERATED, DBFLAG_VACUUM, DBFLAG_VACUUM_INTO, OE_ABORT, OE_DEFAULT, OE_FAIL,
    OE_IGNORE, OE_NONE, OE_REPLACE, OE_ROLLBACK, OE_UPDATE, ONEPASS_OFF, ONEPASS_SINGLE,
    OPFLAG_APPEND, OPFLAG_BULKCSR, OPFLAG_ISNOOP, OPFLAG_LASTROWID, OPFLAG_NCHANGE,
    OPFLAG_PREFORMAT, OPFLAG_SAVEPOSITION, OPFLAG_USESEEKRESULT, OPFLG_JUMP, OP_ADDIMM,
    OP_CLOSE, OP_COLUMN, OP_CURSORLOCK, OP_CURSORUNLOCK, OP_DELETE, OP_EQ, OP_GOTO, OP_HALT,
    OP_HALTIFNULL, OP_IDXINSERT, OP_IDXROWID, OP_IFNOT, OP_INSERT, OP_INTCOPY, OP_INTEGER,
    OP_ISNULL, OP_MAKERECORD, OP_NE, OP_NEWROWID, OP_NEXT, OP_NOCONFLICT, OP_NOTEXISTS,
    OP_NOTNULL, OP_NULL, OP_OPENREAD, OP_OPENWRITE, OP_REWIND, OP_ROWCELL, OP_ROWDATA, OP_ROWID,
    OP_SCOPY, OP_SEEKEND, P4_TRANSIENT, P5_CONSTRAINTCHECK, P5_CONSTRAINTNOTNULL,
    SF_DISTINCT, SQLITE_CONSTRAINT_CHECK, SQLITE_CONSTRAINT_NOTNULL, SQLITE_COUNT_ROWS,
    SQLITE_FOREIGN_KEYS, SQLITE_IGNORE_CHECKS, SQLITE_JUMPIFNULL, SQLITE_NOTNULL, SQLITE_OK,
    SQLITE_REC_TRIGGERS, TF_HAS_GENERATED, TF_HAS_NOT_NULL, TF_STRICT, TK_ASTERISK, TK_COLUMN,
    TK_DELETE, XN_EXPR, XN_ROWID,
};
use crate::delete::{generate_row_delete, generate_row_index_delete};
use crate::expr::expr_dup;
use crate::expr_code2::{
    expr_code_copy, expr_compare, expr_if_false_dup, expr_if_true, expr_list_compare,
    get_temp_range, get_temp_reg, release_temp_range, release_temp_reg,
};
use crate::fkey::fk_required;
use crate::insert::{
    auto_inc_begin, auto_inc_step, auto_increment_end, compute_generated_columns, open_table,
    table_affinity,
};
use crate::prepare::schema_to_index;
use crate::printf::{mprintf, PrintfArg};
use crate::select::get_vdbe;
use crate::sqlite_int::{Expr, Index, Select, Table, Trigger, Upsert, Walker};
use crate::trigger::triggers_exist;
use crate::upsert::{upsert_do_update, upsert_next_is_ipk, upsert_of_index};
use crate::util::{str_icmp, STR_BINARY};
use crate::vdbe_types::{Vdbe, P4};
use crate::vdbeaux::{
    add_op0, add_op1, add_op2, add_op3, add_op4, add_op4_int, append_p4, change_p4, change_p5,
    get_op_ref, jump_here, make_label, noop_comment, resolve_label, set_p4_key_info,
    vdbe_comment, vdbe_goto,
};
use crate::walker::walk_expr;

// Os `OE_*` aparecem como `i32` na maior parte do código (`onError` do C é `int`).
const E_NONE: i32 = OE_NONE as i32;
const E_ROLLBACK: i32 = OE_ROLLBACK as i32;
const E_ABORT: i32 = OE_ABORT as i32;
const E_FAIL: i32 = OE_FAIL as i32;
const E_IGNORE: i32 = OE_IGNORE as i32;
const E_REPLACE: i32 = OE_REPLACE as i32;
const E_UPDATE: i32 = OE_UPDATE as i32;
const E_DEFAULT: i32 = OE_DEFAULT as i32;

// ---------------------------------------------------------------------------------------------
// Auxiliares privados (não existem no C)
// ---------------------------------------------------------------------------------------------


/// `sqlite3_stricmp` com os ponteiros nulos do C: nulo é menor que qualquer texto.
fn stricmp_opt(a: Option<&[u8]>, b: Option<&[u8]>) -> i32 {
    match (a, b) {
        (None, None) => 0,
        (None, Some(_)) => -1,
        (Some(_), None) => 1,
        (Some(x), Some(y)) => str_icmp(x, y),
    }
}

/// O `n`-ésimo elo (a partir de 0) da cadeia de `Upsert`. O chamador só passa posições que
/// existem na cadeia.
fn upsert_nth(p: &Upsert, n: usize) -> &Upsert {
    let mut cur = p;
    for _ in 0..n {
        cur = cur.p_next_upsert.as_deref().expect("cadeia de upsert");
    }
    cur
}

/// `sqlite3UpsertOfIndex` devolvendo a posição do elo na cadeia (ver o cabeçalho). `None` é o
/// ponteiro nulo do C; `Some(0)` é o próprio `pUpsert`.
fn upsert_clause_pos(head: &Upsert, p_idx: Option<&Rc<Index>>) -> Option<usize> {
    let found = upsert_of_index(head, p_idx)?;
    let mut cur = head;
    let mut n = 0usize;
    loop {
        if std::ptr::eq(cur, found) {
            return Some(n);
        }
        cur = cur.p_next_upsert.as_deref()?;
        n += 1;
    }
}

/// Cópia de um `P4` para o laço de recheck de `sqlite3GenerateConstraintChecks`: o C copia
/// `x.p4.z` (ou o inteiro, no `P4_INT32`) para um opcode novo do mesmo tipo.
fn copy_p4(p: &P4) -> P4 {
    match p {
        P4::None => P4::None,
        P4::Int32(i) => P4::Int32(*i),
        P4::Int64(i) => P4::Int64(*i),
        P4::Real(r) => P4::Real(*r),
        P4::Text(t) => P4::Text(t.clone()),
        P4::Blob(b) => P4::Blob(b.clone()),
        P4::Coll(c) => P4::Coll(c.clone()),
        P4::KeyInfo(k) => P4::KeyInfo(Rc::clone(k)),
        P4::FuncDef(f) => P4::FuncDef(Rc::clone(f)),
        P4::FuncCtx(f) => P4::FuncCtx(Rc::clone(f)),
        P4::Mem(m) => P4::Mem(m.clone()),
        P4::Table(t) => P4::Table(Rc::clone(t)),
        P4::TableRef(t) => P4::TableRef(Rc::clone(t)),
        P4::Vtab(v) => P4::Vtab(*v),
        P4::Subprogram(s) => P4::Subprogram(Rc::clone(s)),
        P4::IntArray(a) => P4::IntArray(a.clone()),
        P4::Expr(e) => P4::Expr(e.clone()),
    }
}

// ---------------------------------------------------------------------------------------------
// sqlite3ExprReferencesUpdatedColumn (chunk 004)
// ---------------------------------------------------------------------------------------------

/// Bits de `pWalker->eCode` em `sqlite3ExprReferencesUpdatedColumn()`: a CHECK usa uma coluna
/// que muda.
const CKCNSTRNT_COLUMN: u16 = 0x01;
/// A CHECK referencia o ROWID.
const CKCNSTRNT_ROWID: u16 = 0x02;

/// `checkConstraintExprNode`: callback do walker de `sqlite3ExprReferencesUpdatedColumn()`. Liga
/// os bits de `e_code` se o nó referencia uma das colunas que o UPDATE modifica (`u` é o
/// `aiChng`, o `pWalker->u.aiCol` do C) ou o rowid.
fn check_constraint_expr_node(w: &mut Walker<Vec<i32>>, p_expr: &mut Expr) -> i32 {
    if p_expr.op == TK_COLUMN {
        debug_assert!(p_expr.i_column >= -1);
        if p_expr.i_column >= 0 {
            if w.u.get(p_expr.i_column as usize).copied().unwrap_or(-1) >= 0 {
                w.e_code |= CKCNSTRNT_COLUMN;
            }
        } else {
            w.e_code |= CKCNSTRNT_ROWID;
        }
    }
    crate::consts::WRC_CONTINUE
}

/// `sqlite3ExprReferencesUpdatedColumn`: `p_expr` é uma CHECK de uma linha que sofre UPDATE. As
/// únicas colunas modificadas são as que têm `ai_chng[i]>=0`, mais o ROWID se `chng_rowid`.
/// Verdadeiro se a CHECK usa alguma coluna que muda (ou o rowid, se ele muda), isto é, se ela
/// precisa ser validada para a linha nova. Também serve para uma expressão de índice sobre
/// expressões. O `Walker` pede `&mut Expr`, então a árvore é duplicada antes de percorrida.
pub fn expr_references_updated_column(p_expr: &Expr, ai_chng: &[i32], chng_rowid: bool) -> bool {
    let mut w: Walker<Vec<i32>> = Walker {
        x_expr_callback: Some(check_constraint_expr_node),
        e_code: 0,
        u: ai_chng.to_vec(),
        ..Default::default()
    };
    let mut p_copy = expr_dup(Some(p_expr), 0);
    walk_expr(&mut w, p_copy.as_deref_mut());
    if !chng_rowid {
        w.e_code &= !CKCNSTRNT_ROWID;
    }
    w.e_code != 0
}

// ---------------------------------------------------------------------------------------------
// sqlite3GenerateConstraintChecks (chunks 004 a 006)
// ---------------------------------------------------------------------------------------------

/// `sqlite3GenerateConstraintChecks`: gera o código das verificações de restrições antes de um
/// INSERT ou UPDATE na tabela `p_tab`.
///
/// `reg_new_data` é o primeiro registrador de um bloco de `nCol+1` com os dados a inserir (ou
/// depois do UPDATE): o primeiro guarda o rowid novo (ou NULL numa WITHOUT ROWID) e os seguintes
/// as colunas. `reg_old_data` é parecido, com os dados anteriores ao UPDATE; é zero num INSERT,
/// e é assim que a rotina distingue os dois. Num UPDATE `pk_chng` diz se a chave verdadeira
/// (rowid, ou PRIMARY KEY da WITHOUT ROWID) pode ter mudado; num INSERT diz se o rowid foi dado
/// explicitamente.
///
/// O código gerado guarda as entradas novas dos índices nos registradores `a_reg_idx[i]` (nenhuma
/// entrada para os que têm zero), na ordem de `Table.p_index`. Numa tabela com rowid também cria
/// o registro da tabela em `a_reg_idx[nIdx]`, durante as verificações, para que mudanças de
/// afinidade posteriores não o alterem.
///
/// O chamador já abriu cursores de escrita na tabela e em todos os índices aplicáveis.
/// `i_data_cur` é o cursor da tabela (ou do índice PRIMARY KEY de uma WITHOUT ROWID) e
/// `i_idx_cur` o do primeiro índice; os demais são `i_idx_cur+N`.
///
/// A ação em caso de falha é `override_error` se não for `OE_Default`; senão `pParse->onError`;
/// senão a da restrição. `ignore_dest` é o rótulo para onde saltar em `OE_Ignore`.
/// `pb_may_replace` recebe verdadeiro se alguma restrição pode causar um REPLACE. `ai_chng`
/// (`None` num INSERT): a coluna i não muda se `ai_chng[i]<0`. `p_upsert` são as cláusulas ON
/// CONFLICT, se houver.
pub fn generate_constraint_checks(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab: &Rc<Table>,
    a_reg_idx: &[i32],
    i_data_cur: i32,
    i_idx_cur: i32,
    reg_new_data: i32,
    reg_old_data: i32,
    pk_chng: u8,
    override_error: u8,
    ignore_dest: i32,
    pb_may_replace: &mut i32,
    ai_chng: Option<&[i32]>,
    p_upsert: Option<&mut Upsert>,
) {
    let mut ups = p_upsert;
    let mut override_error = override_error;
    let mut seen_replace = false; // verdadeiro se REPLACE resolve conflito da INT PK
    let mut upsert_clause: Option<usize> = None; // `pUpsertClause`
    let is_update = reg_old_data != 0;
    let mut b_affinity_done = false; // o OP_Affinity já foi gerado
    let mut upsert_ipk_return: i32 = 0; // endereço do Goto no fim da checagem de unicidade do IPK
    let mut upsert_ipk_delay: i32 = 0; // endereço do Goto que pula a checagem inicial do IPK
    let mut ipk_top: i32 = 0; // topo da checagem de unicidade do IPK
    let mut ipk_bottom: i32 = 0; // Goto no fim da checagem de unicidade do IPK
    let mut addr_recheck: i32 = 0; // salto para reconferir todas as restrições de unicidade
    let mut lbl_recheck_ok: i32 = 0; // cada reconferência salta aqui se passar
    let mut n_replace_trig: i32 = 0; // quantidade de gatilhos de replace gerados
    let n_col = p_tab.n_col as usize;
    let n_idx = p_tab.p_index.len();
    debug_assert!(parse.p_vdbe.is_some());
    debug_assert!(!p_tab.is_view()); // esta tabela não é uma VIEW

    // `pPk` é o índice PRIMARY KEY das WITHOUT ROWID e nulo nas tabelas com rowid. `nPkField` é
    // o número de campos da chave verdadeira da tabela.
    let (p_pk, n_pk_field): (Option<&Rc<Index>>, i32) = if p_tab.has_rowid() {
        (None, 1)
    } else {
        let pk = primary_key_index(p_tab);
        debug_assert!(pk.is_some());
        (pk, pk.map_or(1, |p| p.n_key_col as i32))
    };

    // Testa todas as restrições NOT NULL.
    if (p_tab.tab_flags & TF_HAS_NOT_NULL) != 0 {
        let mut b2nd_pass = false; // verdadeiro na 2a passada
        let mut n_seen_replace = 0; // número de operações ON CONFLICT REPLACE
        let mut n_generated = 0; // colunas geradas com NOT NULL
        loop {
            // Duas passadas sobre as colunas; sai pelo `break`.
            for i in 0..n_col {
                let p_col = &p_tab.a_col[i];
                let mut on_error = p_col.not_null as i32;
                if on_error == E_NONE {
                    continue; // sem NOT NULL nesta coluna
                }
                if i as i16 == p_tab.i_p_key {
                    continue; // o ROWID nunca é NULL
                }
                let is_generated = (p_col.col_flags & COLFLAG_GENERATED) != 0;
                if is_generated && !b2nd_pass {
                    n_generated += 1;
                    continue; // as geradas são tratadas na 2a passada
                }
                if let Some(ch) = ai_chng {
                    if ch[i] < 0 && !is_generated {
                        // Não verifica NOT NULL em colunas que não mudam.
                        continue;
                    }
                }
                if override_error != OE_DEFAULT {
                    on_error = override_error as i32;
                } else if on_error == E_DEFAULT {
                    on_error = E_ABORT;
                }
                if on_error == E_REPLACE {
                    if b2nd_pass        // REPLACE vira ABORT na 2a passada
                        || p_col.i_dflt == 0 // REPLACE é ABORT se não há DEFAULT
                    {
                        on_error = E_ABORT;
                    } else {
                        debug_assert!(!is_generated);
                    }
                } else if b2nd_pass && !is_generated {
                    continue;
                }
                debug_assert!(
                    on_error == E_ROLLBACK
                        || on_error == E_ABORT
                        || on_error == E_FAIL
                        || on_error == E_IGNORE
                        || on_error == E_REPLACE
                );
                let i_reg = table_column_to_storage(p_tab, i as i16) as i32 + reg_new_data + 1;
                match on_error {
                    E_REPLACE => {
                        let addr1 = add_op1(vdbe_of_parse(parse), OP_NOTNULL as i32, i_reg);
                        debug_assert!((p_col.col_flags & COLFLAG_GENERATED) == 0);
                        n_seen_replace += 1;
                        expr_code_copy(db, parse, column_expr(p_tab, p_col), i_reg, Some(p_tab));
                        jump_here(vdbe_of_parse(parse), addr1);
                    }
                    E_ABORT | E_ROLLBACK | E_FAIL => {
                        if on_error == E_ABORT {
                            may_abort(parse);
                        }
                        let z_msg = mprintf(
                            b"%s.%s",
                            &[text_arg(&p_tab.z_name), text_arg(&p_col.z_cn_name)],
                        );
                        let v = vdbe_of_parse(parse);
                        add_op3(
                            v,
                            OP_HALTIFNULL as i32,
                            SQLITE_CONSTRAINT_NOTNULL,
                            on_error,
                            i_reg,
                        );
                        if let Some(m) = z_msg {
                            append_p4(v, P4::Text(m));
                        }
                        change_p5(v, P5_CONSTRAINTNOTNULL);
                    }
                    _ => {
                        debug_assert!(on_error == E_IGNORE);
                        add_op2(vdbe_of_parse(parse), OP_ISNULL as i32, i_reg, ignore_dest);
                    }
                }
            }
            if n_generated == 0 && n_seen_replace == 0 {
                // Sem colunas geradas com NOT NULL nem NOT NULL ON CONFLICT REPLACE, uma
                // passada basta.
                break;
            }
            if b2nd_pass {
                break; // nunca precisa de mais de 2 passadas
            }
            b2nd_pass = true;
            if n_seen_replace > 0 && (p_tab.tab_flags & TF_HAS_GENERATED) != 0 {
                // Se algum NOT NULL ON CONFLICT REPLACE disparou na 1a passada, recalcula as
                // colunas geradas, cujos valores podem depender das colunas afetadas.
                compute_generated_columns(db, parse, reg_new_data + 1, p_tab);
            }
        }
    }

    // Testa todas as restrições CHECK.
    if let Some(p_check) = p_tab.p_check.as_deref() {
        if (db.flags & SQLITE_IGNORE_CHECKS) == 0 {
            parse.i_self_tab = -(reg_new_data + 1);
            let mut on_error = if override_error != OE_DEFAULT {
                override_error as i32
            } else {
                E_ABORT
            };
            for item in p_check.a.iter() {
                let p_expr = item.p_expr.as_deref();
                if let (Some(ch), Some(e)) = (ai_chng, p_expr) {
                    if !expr_references_updated_column(e, ch, pk_chng != 0) {
                        // As CHECK não referenciam as colunas do UPDATE: não há o que verificar.
                        continue;
                    }
                }
                if !b_affinity_done {
                    table_affinity(vdbe_of_parse(parse), p_tab, reg_new_data + 1);
                    b_affinity_done = true;
                }
                let all_ok = make_label(parse);
                let mut p_copy = expr_dup(p_expr, 0);
                if db.malloc_failed == 0 {
                    if let Some(c) = p_copy.as_deref_mut() {
                        expr_if_true(db, parse, c, all_ok, SQLITE_JUMPIFNULL as i32, Some(p_tab));
                    }
                }
                drop(p_copy);
                if on_error == E_IGNORE {
                    vdbe_goto(vdbe_of_parse(parse), ignore_dest);
                } else {
                    let z_name = item.z_e_name.as_deref();
                    debug_assert!(z_name.is_some() || db.malloc_failed != 0);
                    if on_error == E_REPLACE {
                        on_error = E_ABORT; // IMP: R-26383-51744
                    }
                    halt_constraint(
                        parse,
                        SQLITE_CONSTRAINT_CHECK,
                        on_error,
                        z_name,
                        P4_TRANSIENT,
                        P5_CONSTRAINTCHECK,
                    );
                }
                resolve_label(parse, db, all_ok);
            }
            parse.i_self_tab = 0;
        }
    }

    // As restrições UNIQUE e PRIMARY KEY são tratadas nesta ordem:
    //
    //   (1)  OE_Update
    //   (2)  OE_Abort, OE_Fail, OE_Rollback, OE_Ignore
    //   (3)  OE_Replace
    //
    // OE_Fail e OE_Ignore têm de acontecer antes de qualquer mudança. OE_Update garante que só
    // uma linha muda, então vem antes de OE_Replace. Tecnicamente OE_Abort e OE_Rollback podiam
    // vir em qualquer ordem, mas ficam agrupados na frente por conveniência.
    //
    // 2018-08-14: Ticket https://www.sqlite.org/src/info/908f001483982c43 (o PostgreSQL confere a
    // restrição OE_Update antes das outras, então ela foi movida).
    //
    // O código sai nesta ordem: (A) a restrição do rowid; (B) as restrições de índices únicos que
    // não têm OE_Replace como resolução padrão; (C) os índices únicos com OE_Replace. A ordem de
    // (2) e (3) vem de a lista de índices da tabela pôr os OE_Replace por último (ver
    // `sqlite3CreateIndex()`).
    let mut idx_order: Vec<usize> = (0..n_idx).collect();
    let mut drop_upsert = false;
    if let Some(u) = ups.as_deref() {
        if u.p_upsert_target.is_none() {
            // Há só uma cláusula ON CONFLICT e ela não tem alvo de restrição.
            debug_assert!(u.p_next_upsert.is_none());
            if !u.is_do_update {
                // Um único ON CONFLICT DO NOTHING sem alvo: toda resolução de unicidade vira
                // OE_Ignore.
                override_error = OE_IGNORE;
                drop_upsert = true;
            } else {
                // Um único ON CONFLICT DO UPDATE: toda resolução vira OE_Update.
                override_error = OE_UPDATE;
            }
        } else if n_idx != 0 {
            // Senão é preciso a versão do iterador por vetor, para que todas as condições ON
            // CONFLICT sejam conferidas primeiro e em ordem.
            for ix in 0..n_idx {
                debug_assert!(a_reg_idx[ix] > 0);
            }
            let mut b_used = vec![false; n_idx];
            let mut order: Vec<usize> = Vec::with_capacity(n_idx);
            let mut p_term = Some(u);
            while let Some(t) = p_term {
                if t.p_upsert_target.is_none() {
                    break;
                }
                p_term = t.p_next_upsert.as_deref();
                let Some(ti) = t.p_upsert_idx.as_ref() else {
                    continue; // pula o ON CONFLICT do IPK
                };
                let mut jj = 0usize;
                while jj < n_idx && !Rc::ptr_eq(&p_tab.p_index[jj], ti) {
                    jj += 1;
                }
                if jj >= n_idx || b_used[jj] {
                    continue; // cláusula ON CONFLICT duplicada é ignorada
                }
                b_used[jj] = true;
                order.push(jj);
            }
            for jj in 0..n_idx {
                if b_used[jj] {
                    continue;
                }
                order.push(jj);
            }
            debug_assert!(order.len() == n_idx);
            idx_order = order;
        }
    }
    if drop_upsert {
        ups = None;
    }

    // Determina se gatilhos (explícitos ou ações de FK) podem rodar por causa dos deletes feitos
    // quando a resolução é OE_Replace (os "gatilhos de replace"). Se algum roda, é preciso
    // reconferir todas as restrições de unicidade depois deles, mas na reconferência a
    // resolução é OE_Abort em vez de OE_Replace.
    //
    // Se gatilhos de replace são possíveis: (1) aloca `reg_trig_cnt` e o zera (conta os
    // gatilhos que disparam; a reconferência só ocorre se for positivo); (2) `p_trigger` vira a
    // lista de gatilhos DELETE da tabela; (3) inicializa `addr_recheck` e `lbl_recheck_ok`. Os
    // testes da reconferência ficam separados uns dos outros no bytecode e são ligados por essas
    // duas variáveis.
    let p_trigger: Vec<Rc<Trigger>>;
    let mut reg_trig_cnt: i32;
    if (db.flags & (SQLITE_REC_TRIGGERS | SQLITE_FOREIGN_KEYS)) == 0 {
        // Sem gatilhos DELETE nem restrições de FK, não há reconferência.
        p_trigger = Vec::new();
        reg_trig_cnt = 0;
    } else {
        if (db.flags & SQLITE_REC_TRIGGERS) != 0 {
            let mut tmask: i32 = 0;
            p_trigger = triggers_exist(db, parse, p_tab, TK_DELETE as i32, None, &mut tmask);
            reg_trig_cnt = (!p_trigger.is_empty() || fk_required(db, parse, p_tab, None, 0) != 0)
                as i32;
        } else {
            p_trigger = Vec::new();
            reg_trig_cnt = fk_required(db, parse, p_tab, None, 0);
        }
        if reg_trig_cnt != 0 {
            // Pode haver gatilhos de replace: aloca o contador e o zera.
            parse.n_mem += 1;
            reg_trig_cnt = parse.n_mem;
            let v = vdbe_of_parse(parse);
            add_op2(v, OP_INTEGER as i32, 0, reg_trig_cnt);
            vdbe_comment(v, b"trigger count", &[]);
            lbl_recheck_ok = make_label(parse);
            addr_recheck = lbl_recheck_ok;
        }
    }

    // Se o rowid muda, garante que o rowid novo ainda não existe na tabela.
    if pk_chng != 0 && p_pk.is_none() {
        let addr_rowid_ok = make_label(parse);

        // Decide o que fazer numa colisão de rowid.
        let mut on_error = p_tab.key_conf as i32;
        if override_error != OE_DEFAULT {
            on_error = override_error as i32;
        } else if on_error == E_DEFAULT {
            on_error = E_ABORT;
        }

        // Decide se o upsert se aplica neste caso.
        if let Some(u) = ups.as_deref() {
            upsert_clause = upsert_clause_pos(u, None);
            if let Some(c) = upsert_clause {
                if !upsert_nth(u, c).is_do_update {
                    on_error = E_IGNORE; // DO NOTHING é o mesmo que INSERT OR IGNORE
                } else {
                    on_error = E_UPDATE; // DO UPDATE
                }
            }
            if upsert_clause != Some(0) {
                // A primeira cláusula ON CONFLICT tem alvo diferente do IPK: salta para ela
                // primeiro e volta aqui depois para tratar o IPK.
                upsert_ipk_delay = add_op0(vdbe_of_parse(parse), OP_GOTO as i32);
            }
        }

        // Se a resposta a conflito de rowid é REPLACE mas a de outra restrição UNIQUE é FAIL ou
        // IGNORE, a checagem do rowid é adiada para depois das UNIQUE.
        if on_error == E_REPLACE            // regra do IPK é REPLACE
            && on_error != override_error as i32 // regras das outras restrições diferem
            && !p_tab.p_index.is_empty()    // existem outras restrições
            && upsert_ipk_delay == 0        // checagem do IPK ainda não adiada pelo UPSERT
        {
            let v = vdbe_of_parse(parse);
            ipk_top = add_op0(v, OP_GOTO as i32) + 1;
            vdbe_comment(v, b"defer IPK REPLACE until last", &[]);
        }

        if is_update {
            // `pkChng!=0` não quer dizer que o rowid mudou, só que pode ter mudado. Pula a
            // lógica de conflito abaixo se o rowid não mudou.
            let v = vdbe_of_parse(parse);
            add_op3(v, OP_EQ as i32, reg_new_data, addr_rowid_ok, reg_old_data);
            change_p5(v, SQLITE_NOTNULL as u16);
        }

        // Vê se o rowid novo já existe na tabela. Se não existe, pula a lógica de conflito.
        let v = vdbe_of_parse(parse);
        noop_comment(v, b"uniqueness check for ROWID", &[]);
        add_op3(v, OP_NOTEXISTS as i32, i_data_cur, addr_rowid_ok, reg_new_data);

        if !matches!(on_error, E_ROLLBACK | E_ABORT | E_FAIL | E_REPLACE | E_UPDATE | E_IGNORE) {
            // O `default` do C cai no caso OE_Abort.
            on_error = E_ABORT;
        }
        match on_error {
            E_ROLLBACK | E_ABORT | E_FAIL => {
                rowid_constraint(parse, on_error, p_tab);
            }
            E_REPLACE => {
                // Se há gatilhos DELETE na tabela e a flag recursive-triggers está ligada,
                // chama GenerateRowDelete() para remover a linha em conflito (isso dispara os
                // gatilhos e remove as entradas da tabela e dos índices).
                //
                // Senão, se não há gatilhos ou a flag está desligada, mas a tabela tem
                // índices, chama GenerateRowIndexDelete(): remove só as entradas dos índices; a
                // entrada da tabela será substituída pelo OP_Insert seguinte.
                //
                // Chamando qualquer das duas também chama MultiWrite() para indicar que este
                // VDBE pode precisar de rollback de comando. Versões antigas chamavam
                // sqlite3MultiWrite() sempre, mas ser seletivo aqui deixa comandos como
                // `REPLACE INTO t(rowid) VALUES($newrowid)` rodar sem journal de comando se a
                // tabela não tem índices.
                if reg_trig_cnt != 0 {
                    multi_write(parse);
                    generate_row_delete(
                        db,
                        parse,
                        p_tab,
                        &p_trigger,
                        i_data_cur,
                        i_idx_cur,
                        reg_new_data,
                        1,
                        0,
                        OE_REPLACE,
                        1,
                        -1,
                    );
                    add_op2(vdbe_of_parse(parse), OP_ADDIMM as i32, reg_trig_cnt, 1); // conta gatilho
                    n_replace_trig += 1;
                } else {
                    debug_assert!(p_tab.has_rowid());
                    // Este OP_Delete dispara só o pre-update-hook, sem mexer no b-tree: é mais
                    // eficiente deixar o OP_Insert seguinte substituir a entrada que apagar e
                    // inserir de novo.
                    let v = vdbe_of_parse(parse);
                    add_op2(v, OP_DELETE as i32, i_data_cur, OPFLAG_ISNOOP as i32);
                    append_p4(v, P4::Table(Rc::clone(p_tab)));
                    if !p_tab.p_index.is_empty() {
                        multi_write(parse);
                        generate_row_index_delete(db, parse, p_tab, i_data_cur, i_idx_cur, None, -1);
                    }
                }
                seen_replace = true;
            }
            E_UPDATE => {
                if let Some(u) = ups.as_deref_mut() {
                    upsert_do_update(db, parse, u, p_tab, None, i_data_cur);
                }
                vdbe_goto(vdbe_of_parse(parse), ignore_dest);
            }
            _ => {
                debug_assert!(on_error == E_IGNORE);
                vdbe_goto(vdbe_of_parse(parse), ignore_dest);
            }
        }
        resolve_label(parse, db, addr_rowid_ok);
        if ups.is_some() && upsert_clause != Some(0) {
            upsert_ipk_return = add_op0(vdbe_of_parse(parse), OP_GOTO as i32);
        } else if ipk_top != 0 {
            ipk_bottom = add_op0(vdbe_of_parse(parse), OP_GOTO as i32);
            jump_here(vdbe_of_parse(parse), ipk_top - 1);
        }
    }

    // Testa todas as restrições UNIQUE criando entradas para cada índice UNIQUE e conferindo
    // que não há duplicatas. Calcula os registros revistos dos índices no caminho. Este laço
    // também trata o índice PRIMARY KEY de uma WITHOUT ROWID.
    for &ix in idx_order.iter() {
        let p_idx = &p_tab.p_index[ix];
        if a_reg_idx[ix] == 0 {
            continue; // pula os índices que não mudam
        }
        if let Some(u) = ups.as_deref() {
            upsert_clause = upsert_clause_pos(u, Some(p_idx));
            if upsert_ipk_delay != 0 && upsert_clause == Some(0) {
                jump_here(vdbe_of_parse(parse), upsert_ipk_delay);
            }
        }
        let addr_unique_ok = make_label(parse); // salta aqui se a UNIQUE é satisfeita
        if !b_affinity_done {
            table_affinity(vdbe_of_parse(parse), p_tab, reg_new_data + 1);
            b_affinity_done = true;
        }
        noop_comment(vdbe_of_parse(parse), b"prep index %s", &[text_arg(&p_idx.z_name)]);
        let i_this_cur = i_idx_cur + ix as i32; // cursor deste índice UNIQUE

        // Pula os índices parciais cuja cláusula WHERE não é verdadeira.
        if let Some(w) = p_idx.p_partial_idx_where.as_deref() {
            add_op2(vdbe_of_parse(parse), OP_NULL as i32, 0, a_reg_idx[ix]);
            parse.i_self_tab = -(reg_new_data + 1);
            expr_if_false_dup(
                db,
                parse,
                Some(w),
                addr_unique_ok,
                SQLITE_JUMPIFNULL as i32,
                Some(p_tab),
            );
            parse.i_self_tab = 0;
        }

        // Cria um registro para esta entrada do índice como ela deve ficar depois do INSERT ou
        // UPDATE e o guarda no registrador `a_reg_idx[ix]`.
        let reg_idx = a_reg_idx[ix] + 1; // faixa de registradores com o conteúdo do índice
        for i in 0..p_idx.n_column as usize {
            let i_field = p_idx.ai_column[i];
            if i_field == XN_EXPR {
                parse.i_self_tab = -(reg_new_data + 1);
                let e = index_col_expr(p_idx, i);
                expr_code_copy(db, parse, e, reg_idx + i as i32, Some(p_tab));
                parse.i_self_tab = 0;
                vdbe_comment(
                    vdbe_of_parse(parse),
                    b"%s column %d",
                    &[text_arg(&p_idx.z_name), PrintfArg::Int(i as i64)],
                );
            } else if i_field == XN_ROWID || i_field == p_tab.i_p_key {
                let v = vdbe_of_parse(parse);
                add_op2(v, OP_INTCOPY as i32, reg_new_data, reg_idx + i as i32);
                vdbe_comment(v, b"rowid", &[]);
            } else {
                let x = table_column_to_storage(p_tab, i_field) as i32 + reg_new_data + 1;
                let v = vdbe_of_parse(parse);
                add_op2(v, OP_SCOPY as i32, x, reg_idx + i as i32);
                vdbe_comment(
                    v,
                    b"%s",
                    &[text_arg(&p_tab.a_col[i_field as usize].z_cn_name)],
                );
            }
        }
        {
            let v = vdbe_of_parse(parse);
            add_op3(v, OP_MAKERECORD as i32, reg_idx, p_idx.n_column as i32, a_reg_idx[ix]);
            vdbe_comment(v, b"for %s", &[text_arg(&p_idx.z_name)]);
        }

        let is_pk = p_pk.map_or(false, |pk| Rc::ptr_eq(pk, p_idx));

        // Num UPDATE, se este índice é o PRIMARY KEY de uma WITHOUT ROWID e a chave não mudou,
        // não há colisão possível e toda a lógica de detecção abaixo é pulada.
        if is_update && is_pk && pk_chng == 0 {
            resolve_label(parse, db, addr_unique_ok);
            continue;
        }

        // Descobre a ação em caso de conflito de unicidade.
        let mut on_error = p_idx.on_error as i32;
        if on_error == E_NONE {
            resolve_label(parse, db, addr_unique_ok);
            continue; // p_idx não é um índice UNIQUE
        }
        if override_error != OE_DEFAULT {
            on_error = override_error as i32;
        } else if on_error == E_DEFAULT {
            on_error = E_ABORT;
        }

        // Vê se a cláusula do upsert se aplica a este índice.
        if let (Some(u), Some(c)) = (ups.as_deref(), upsert_clause) {
            if !upsert_nth(u, c).is_do_update {
                on_error = E_IGNORE; // DO NOTHING é o mesmo que INSERT OR IGNORE
            } else {
                on_error = E_UPDATE; // DO UPDATE
            }
        }

        // A detecção de colisão podia ser omitida (REPLACE, WITHOUT ROWID, sem índices
        // secundários, sem gatilhos DELETE e sem contadores de FK), mas não com
        // SQLITE_ENABLE_PREUPDATE_HOOK: a linha precisa ser apagada explicitamente para que o
        // pre-update-hook seja chamado. O Debian liga a opção, então o bloco não existe.

        // Vê se a entrada nova do índice será única.
        debug_assert!(p_tab.is_ordinary_table());
        let addr_conflict_ck = // primeiro opcode da lógica de checagem de conflito
            add_op4_int(
                vdbe_of_parse(parse),
                OP_NOCONFLICT as i32,
                i_this_cur,
                addr_unique_ok,
                reg_idx,
                p_idx.n_key_col as i32,
            );

        // Gera o código que trata as colisões.
        let reg_r = if is_pk {
            reg_idx // faixa de registradores com a PK em conflito
        } else {
            get_temp_range(parse, n_pk_field)
        };
        if is_update || on_error == E_REPLACE {
            if p_tab.has_rowid() {
                let v = vdbe_of_parse(parse);
                add_op2(v, OP_IDXROWID as i32, i_this_cur, reg_r);
                // Só há conflito se o rowid da entrada existente do índice é diferente do rowid
                // antigo.
                if is_update {
                    add_op3(v, OP_EQ as i32, reg_r, addr_unique_ok, reg_old_data);
                    change_p5(v, SQLITE_NOTNULL as u16);
                }
            } else if let Some(pk) = p_pk {
                // Extrai a PRIMARY KEY do fim da entrada do índice e a guarda nos registradores
                // `reg_r`..`reg_r+nPk-1`.
                if !is_pk {
                    for i in 0..pk.n_key_col as usize {
                        debug_assert!(pk.ai_column[i] >= 0);
                        let x = table_column_to_index(p_idx, pk.ai_column[i]);
                        let v = vdbe_of_parse(parse);
                        add_op3(v, OP_COLUMN as i32, i_this_cur, x as i32, reg_r + i as i32);
                        vdbe_comment(
                            v,
                            b"%s.%s",
                            &[
                                text_arg(&p_tab.z_name),
                                text_arg(&p_tab.a_col[pk.ai_column[i] as usize].z_cn_name),
                            ],
                        );
                    }
                }
                if is_update {
                    // Se processa a PRIMARY KEY de uma WITHOUT ROWID, só há conflito se os
                    // valores novos da PK são realmente diferentes dos antigos. Para um índice
                    // UNIQUE, só há conflito se a PK da linha casada é diferente da PK original
                    // da linha antes do UPDATE. (Ver TH3 withoutrowid04.test.)
                    let n_key = pk.n_key_col as i32;
                    let mut addr_jump = vdbe_of_parse(parse).n_op() + n_key;
                    let mut op = OP_NE;
                    let reg_cmp = if p_idx.is_primary_key_index() { reg_idx } else { reg_r };
                    for i in 0..pk.n_key_col as usize {
                        let p4 = locate_coll_seq(db, parse, &pk.az_coll[i]);
                        let x0 = pk.ai_column[i];
                        debug_assert!(x0 >= 0);
                        if i as i32 == n_key - 1 {
                            addr_jump = addr_unique_ok;
                            op = OP_EQ;
                        }
                        let x = table_column_to_storage(p_tab, x0) as i32;
                        let v = vdbe_of_parse(parse);
                        add_op4(
                            v,
                            op as i32,
                            reg_old_data + 1 + x,
                            addr_jump,
                            reg_cmp + i as i32,
                            P4::Coll(p4),
                        );
                        change_p5(v, SQLITE_NOTNULL as u16);
                    }
                }
            }
        }

        // Gera o código que roda se a entrada nova do índice não é única.
        debug_assert!(
            on_error == E_ROLLBACK
                || on_error == E_ABORT
                || on_error == E_FAIL
                || on_error == E_IGNORE
                || on_error == E_REPLACE
                || on_error == E_UPDATE
        );
        match on_error {
            E_ROLLBACK | E_ABORT | E_FAIL => {
                unique_constraint(db, parse, on_error, p_tab, p_idx);
            }
            E_UPDATE => {
                if let Some(u) = ups.as_deref_mut() {
                    upsert_do_update(db, parse, u, p_tab, Some(p_idx), i_idx_cur + ix as i32);
                }
                vdbe_goto(vdbe_of_parse(parse), ignore_dest);
            }
            E_IGNORE => {
                vdbe_goto(vdbe_of_parse(parse), ignore_dest);
            }
            _ => {
                debug_assert!(on_error == E_REPLACE);
                // Número de opcodes da lógica de checagem de conflito.
                let mut n_conflict_ck = vdbe_of_parse(parse).n_op() - addr_conflict_ck;
                debug_assert!(n_conflict_ck > 0);
                if reg_trig_cnt != 0 {
                    multi_write(parse);
                    n_replace_trig += 1;
                }
                if !p_trigger.is_empty() && is_update {
                    add_op1(vdbe_of_parse(parse), OP_CURSORLOCK as i32, i_data_cur);
                }
                generate_row_delete(
                    db,
                    parse,
                    p_tab,
                    &p_trigger,
                    i_data_cur,
                    i_idx_cur,
                    reg_r,
                    n_pk_field,
                    0,
                    OE_REPLACE,
                    if is_pk { ONEPASS_SINGLE } else { ONEPASS_OFF },
                    i_this_cur,
                );
                if !p_trigger.is_empty() && is_update {
                    add_op1(vdbe_of_parse(parse), OP_CURSORUNLOCK as i32, i_data_cur);
                }
                if reg_trig_cnt != 0 {
                    add_op2(vdbe_of_parse(parse), OP_ADDIMM as i32, reg_trig_cnt, 1); // conta gatilho
                    let addr_bypass = add_op0(vdbe_of_parse(parse), OP_GOTO as i32); // pula o recheck
                    vdbe_comment(vdbe_of_parse(parse), b"bypass recheck", &[]);

                    // Aqui entra o código que roda depois de todas as checagens de restrição,
                    // se e só se um ou mais gatilhos de replace dispararam.
                    resolve_label(parse, db, lbl_recheck_ok);
                    lbl_recheck_ok = make_label(parse);
                    if p_idx.p_partial_idx_where.is_some() {
                        // Pula a reconferência se este índice parcial não vale para a linha.
                        add_op2(vdbe_of_parse(parse), OP_ISNULL as i32, reg_idx - 1, lbl_recheck_ok);
                    }
                    // Copia o código de checagem de restrição de cima, trocando o destino do
                    // salto "restrição ok" para o endereço do próximo bloco de reteste.
                    let mut addr = addr_conflict_ck;
                    while n_conflict_ck > 0 {
                        // O add_op4 pode realocar o vetor de opcodes: copia o opcode inteiro
                        // em vez de manter uma referência.
                        let snap = match get_op_ref(vdbe_of_parse(parse), addr) {
                            Some(o) => (o.opcode, o.p1, o.p2, o.p3, o.p5, copy_p4(&o.p4)),
                            None => break,
                        };
                        let (x_opcode, x_p1, x_p2, x_p3, x_p5, x_p4) = snap;
                        if x_opcode != OP_IDXROWID {
                            // Novo P2 do opcode copiado.
                            let p2 = if (OPCODE_PROPERTY[x_opcode as usize] & OPFLG_JUMP) != 0 {
                                lbl_recheck_ok
                            } else {
                                x_p2
                            };
                            let v = vdbe_of_parse(parse);
                            add_op4(v, x_opcode as i32, x_p1, p2, x_p3, x_p4);
                            change_p5(v, x_p5);
                        }
                        n_conflict_ck -= 1;
                        addr += 1;
                    }
                    // Se o reteste falha, aborta.
                    unique_constraint(db, parse, E_ABORT, p_tab, p_idx);

                    jump_here(vdbe_of_parse(parse), addr_bypass); // termina o desvio do recheck
                }
                seen_replace = true;
            }
        }
        resolve_label(parse, db, addr_unique_ok);
        if reg_r != reg_idx {
            release_temp_range(parse, reg_r, n_pk_field);
        }
        if upsert_ipk_return != 0 {
            if let (Some(u), Some(c)) = (ups.as_deref(), upsert_clause) {
                if upsert_next_is_ipk(upsert_nth(u, c)) {
                    let v = vdbe_of_parse(parse);
                    vdbe_goto(v, upsert_ipk_delay + 1);
                    jump_here(v, upsert_ipk_return);
                    upsert_ipk_return = 0;
                }
            }
        }
    }

    // Se a restrição do IPK é um REPLACE, roda por último.
    if ipk_top != 0 {
        let v = vdbe_of_parse(parse);
        vdbe_goto(v, ipk_top);
        vdbe_comment(v, b"Do IPK REPLACE", &[]);
        debug_assert!(ipk_bottom > 0);
        jump_here(v, ipk_bottom);
    }

    // Reconfere todas as restrições de unicidade depois que os gatilhos de replace rodaram.
    debug_assert!(reg_trig_cnt != 0 || n_replace_trig == 0);
    if n_replace_trig != 0 {
        add_op2(vdbe_of_parse(parse), OP_IFNOT as i32, reg_trig_cnt, lbl_recheck_ok);
        if p_pk.is_none() {
            if is_update {
                let v = vdbe_of_parse(parse);
                add_op3(v, OP_EQ as i32, reg_new_data, addr_recheck, reg_old_data);
                change_p5(v, SQLITE_NOTNULL as u16);
            }
            add_op3(
                vdbe_of_parse(parse),
                OP_NOTEXISTS as i32,
                i_data_cur,
                addr_recheck,
                reg_new_data,
            );
            rowid_constraint(parse, E_ABORT, p_tab);
        } else {
            vdbe_goto(vdbe_of_parse(parse), addr_recheck);
        }
        resolve_label(parse, db, lbl_recheck_ok);
    }

    // Gera o registro da tabela. Aqui `ix` do C vale `nIdx`, a posição depois do último índice.
    if p_tab.has_rowid() {
        let reg_rec = a_reg_idx[n_idx];
        add_op3(
            vdbe_of_parse(parse),
            OP_MAKERECORD as i32,
            reg_new_data + 1,
            p_tab.n_nv_col as i32,
            reg_rec,
        );
        // `sqlite3SetMakeRecordP5` é macro vazia sem SQLITE_ENABLE_NULL_TRIM.
        if !b_affinity_done {
            table_affinity(vdbe_of_parse(parse), p_tab, 0);
        }
    }

    *pb_may_replace = seen_replace as i32;
}

// ---------------------------------------------------------------------------------------------
// sqlite3CompleteInsertion e sqlite3OpenTableAndIndices (chunk 007)
// ---------------------------------------------------------------------------------------------

/// `codeWithoutRowidPreupdate`: a tabela `p_tab` é WITHOUT ROWID e está sendo escrita; o cursor
/// é `i_cur` e o registrador `reg_data` tem o registro novo do índice PK. Acrescenta o código que
/// chama o pre-update-hook, se houver um registrado (um `OP_Insert` com `OPFLAG_ISNOOP`).
fn code_without_rowid_preupdate(parse: &mut Parse, p_tab: &Rc<Table>, i_cur: i32, reg_data: i32) {
    let r = get_temp_reg(parse);
    debug_assert!(!p_tab.has_rowid());
    let v = vdbe_of_parse(parse);
    add_op2(v, OP_INTEGER as i32, 0, r);
    add_op4(v, OP_INSERT as i32, i_cur, reg_data, r, P4::Table(Rc::clone(p_tab)));
    change_p5(v, OPFLAG_ISNOOP as u16);
    release_temp_reg(parse, r);
}

/// `sqlite3CompleteInsertion`: gera o código que termina o INSERT ou UPDATE começado por uma
/// chamada anterior a [`generate_constraint_checks`]. Um intervalo consecutivo de registradores a
/// partir de `reg_new_data` tem o rowid e o conteúdo a inserir. Os argumentos devem ser os mesmos
/// dos seis primeiros de `generate_constraint_checks`.
///
/// `update_flags` é zero (INSERT), `OPFLAG_ISUPDATE` ou `OPFLAG_ISUPDATE|OPFLAG_SAVEPOSITION`;
/// `append_bias` é verdadeiro se provavelmente é um append; `use_seek_result` liga
/// `OPFLAG_USESEEKRESULT` nos `OP_[Idx]Insert`.
pub fn complete_insertion(
    _db: &mut Connection,
    parse: &mut Parse,
    p_tab: &Rc<Table>,
    i_data_cur: i32,
    i_idx_cur: i32,
    reg_new_data: i32,
    a_reg_idx: &[i32],
    update_flags: i32,
    append_bias: i32,
    use_seek_result: i32,
) {
    debug_assert!(
        update_flags == 0
            || update_flags == crate::consts::OPFLAG_ISUPDATE as i32
            || update_flags
                == (crate::consts::OPFLAG_ISUPDATE as i32 | OPFLAG_SAVEPOSITION as i32)
    );
    debug_assert!(parse.p_vdbe.is_some());
    debug_assert!(!p_tab.is_view()); // esta tabela não é uma VIEW
    let n_idx = p_tab.p_index.len();
    for (i, p_idx) in p_tab.p_index.iter().enumerate() {
        // Todos os índices REPLACE ficam no fim da lista.
        debug_assert!(
            p_idx.on_error != OE_REPLACE
                || p_tab.p_index.get(i + 1).map_or(true, |n| n.on_error == OE_REPLACE)
        );
        if a_reg_idx[i] == 0 {
            continue;
        }
        if p_idx.p_partial_idx_where.is_some() {
            let v = vdbe_of_parse(parse);
            let dest = v.n_op() + 2;
            add_op2(v, OP_ISNULL as i32, a_reg_idx[i], dest);
        }
        // Flags passadas ao btree insert.
        let mut pik_flags: u8 = if use_seek_result != 0 { OPFLAG_USESEEKRESULT } else { 0 };
        if p_idx.is_primary_key_index() && !p_tab.has_rowid() {
            pik_flags |= OPFLAG_NCHANGE;
            pik_flags |= update_flags as u8 & OPFLAG_SAVEPOSITION;
            if update_flags == 0 {
                code_without_rowid_preupdate(parse, p_tab, i_idx_cur + i as i32, a_reg_idx[i]);
            }
        }
        let v = vdbe_of_parse(parse);
        add_op4_int(
            v,
            OP_IDXINSERT as i32,
            i_idx_cur + i as i32,
            a_reg_idx[i],
            a_reg_idx[i] + 1,
            if p_idx.uniq_not_null { p_idx.n_key_col as i32 } else { p_idx.n_column as i32 },
        );
        change_p5(v, pik_flags as u16);
    }
    if !p_tab.has_rowid() {
        return;
    }
    let mut pik_flags: u8;
    if parse.nested != 0 {
        pik_flags = 0;
    } else {
        pik_flags = OPFLAG_NCHANGE;
        pik_flags |= if update_flags != 0 { update_flags as u8 } else { OPFLAG_LASTROWID };
    }
    if append_bias != 0 {
        pik_flags |= OPFLAG_APPEND;
    }
    if use_seek_result != 0 {
        pik_flags |= OPFLAG_USESEEKRESULT;
    }
    let nested = parse.nested;
    let v = vdbe_of_parse(parse);
    add_op3(v, OP_INSERT as i32, i_data_cur, a_reg_idx[n_idx], reg_new_data);
    if nested == 0 {
        append_p4(v, P4::Table(Rc::clone(p_tab)));
    }
    change_p5(v, pik_flags as u16);
}

/// `sqlite3OpenTableAndIndices`: aloca cursores para a tabela `p_tab` e todos os seus índices e
/// gera o código que os abre e inicializa.
///
/// O cursor do objeto que contém os dados completos (normalmente a tabela, mas o índice PRIMARY
/// KEY numa WITHOUT ROWID) volta em `pi_data_cur`; o do primeiro índice em `pi_idx_cur`. O
/// número de índices é o retorno. Se `i_base` não é negativo é o primeiro cursor (o da tabela
/// com rowid, ou o do primeiro índice de uma WITHOUT ROWID); se é negativo aloca o próximo
/// disponível. Numa tabela com rowid `pi_data_cur` é exatamente `pi_idx_cur-1`; numa WITHOUT
/// ROWID cai na faixa dos cursores de índice, conforme a posição do índice PRIMARY KEY em
/// `Table.p_index`. `a_to_open` (opcional) tem um booleano para a tabela e cada índice; `p5` é o
/// P5 dos `OP_Open*` (menos na WITHOUT ROWID). Numa tabela virtual não faz nada e deixa os dois
/// cursores em -999 (números ilegais, para detectar erro).
pub fn open_table_and_indices(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab: &Rc<Table>,
    op: u8,
    p5: u8,
    i_base: i32,
    a_to_open: Option<&[u8]>,
    pi_data_cur: &mut i32,
    pi_idx_cur: &mut i32,
) -> i32 {
    debug_assert!(op == OP_OPENREAD || op == OP_OPENWRITE);
    debug_assert!(op == OP_OPENWRITE || p5 == 0);
    let mut p5 = p5;
    if p_tab.is_virtual() {
        *pi_data_cur = -999;
        *pi_idx_cur = -999;
        return 0;
    }
    let i_db = schema_to_index(db, p_tab.p_schema);
    debug_assert!(parse.p_vdbe.is_some());
    let mut i_base = if i_base < 0 { parse.n_tab } else { i_base };
    let i_data_cur = i_base;
    i_base += 1;
    *pi_data_cur = i_data_cur;
    if p_tab.has_rowid() && a_to_open.map_or(true, |a| a[0] != 0) {
        open_table(db, parse, i_data_cur, i_db, p_tab, op);
    } else if db.no_shared_cache == 0 {
        table_lock(db, parse, i_db, p_tab.tnum, op == OP_OPENWRITE, &p_tab.z_name);
    }
    *pi_idx_cur = i_base;
    for (i, p_idx) in p_tab.p_index.iter().enumerate() {
        let i_idx_cur = i_base;
        i_base += 1;
        if p_idx.is_primary_key_index() && !p_tab.has_rowid() {
            *pi_data_cur = i_idx_cur;
            p5 = 0;
        }
        if a_to_open.map_or(true, |a| a[i + 1] != 0) {
            add_op3(vdbe_of_parse(parse), op as i32, i_idx_cur, p_idx.tnum as i32, i_db);
            set_p4_key_info(parse, db, p_idx);
            let v = vdbe_of_parse(parse);
            change_p5(v, p5 as u16);
            vdbe_comment(v, b"%s", &[text_arg(&p_idx.z_name)]);
        }
    }
    if i_base > parse.n_tab {
        parse.n_tab = i_base;
    }
    p_tab.p_index.len() as i32
}

// ---------------------------------------------------------------------------------------------
// Otimização de transferência (chunk 007)
// ---------------------------------------------------------------------------------------------

/// `pIdx->aColExpr->a[i].pExpr`: a expressão da coluna `i` de um índice sobre expressões.
fn index_col_expr(p_idx: &Index, i: usize) -> Option<&Expr> {
    p_idx.a_col_expr.as_deref().and_then(|l| l.a.get(i)).and_then(|it| it.p_expr.as_deref())
}

/// `xferCompatibleIndex`: vê se o índice `p_src` serve de fonte de dados para o índice `p_dest`
/// numa otimização de transferência. Os índices são compatíveis se cobrem o mesmo conjunto de
/// colunas, com as mesmas marcas DESC/ASC, o mesmo processamento de `onError`, a mesma colação
/// em cada coluna e exatamente a mesma cláusula WHERE.
fn xfer_compatible_index(p_dest: &Index, p_src: &Index) -> bool {
    if p_dest.n_key_col != p_src.n_key_col || p_dest.n_column != p_src.n_column {
        return false; // número de colunas diferente
    }
    if p_dest.on_error != p_src.on_error {
        return false; // estratégias de resolução de conflito diferentes
    }
    for i in 0..p_src.n_key_col as usize {
        if p_src.ai_column[i] != p_dest.ai_column[i] {
            return false; // colunas indexadas diferentes
        }
        if p_src.ai_column[i] == XN_EXPR {
            debug_assert!(p_src.a_col_expr.is_some() && p_dest.a_col_expr.is_some());
            if expr_compare(None, index_col_expr(p_src, i), index_col_expr(p_dest, i), -1) != 0 {
                return false; // expressões diferentes no índice
            }
        }
        if p_src.a_sort_order[i] != p_dest.a_sort_order[i] {
            return false; // ordens diferentes
        }
        if str_icmp(&p_src.az_coll[i], &p_dest.az_coll[i]) != 0 {
            return false; // colações diferentes
        }
    }
    if expr_compare(
        None,
        p_src.p_partial_idx_where.as_deref(),
        p_dest.p_partial_idx_where.as_deref(),
        -1,
    ) != 0
    {
        return false; // cláusulas WHERE diferentes
    }
    // Se nenhum teste falha os índices são compatíveis.
    true
}

/// `xferOptimization`: tenta a otimização de transferência nos INSERTs da forma
///
/// ```text
///     INSERT INTO tab1 SELECT * FROM tab2;
/// ```
///
/// Ela transfere registros crus de `tab2` para `tab1`, sem decodificar nem remontar as colunas,
/// e faz o mesmo com os registros crus dos índices. Só é tentada se `tab1` e `tab2` são
/// compatíveis (as regras estão comentadas no código). Devolve verdadeiro se a otimização vai
/// ser usada com certeza. Às vezes ela só funciona se a tabela de destino está vazia, o que só
/// se sabe em tempo de execução: nesse caso gera o código da otimização mais um teste de
/// "destino vazio" que salta por cima dela, e devolve falso para o chamador gerar também a
/// transferência sem otimização. Também devolve falso se não há chance de aplicar. Particularmente
/// útil para acelerar o VACUUM.
pub fn xfer_optimization(
    db: &mut Connection,
    parse: &mut Parse,
    p_dest: &Rc<Table>,
    p_select: &Select,
    on_error: i32,
    i_db_dest: i32,
) -> bool {
    let mut on_error = on_error;
    if parse.p_with.is_some() || p_select.p_with.is_some() {
        // Não tenta se há cláusulas WITH: prosseguir poderia gerar um falso "no such table: xxx"
        // se o SELECT lê de uma CTE chamada "xxx".
        return false;
    }
    if p_dest.is_virtual() {
        return false; // tab1 não pode ser tabela virtual
    }
    if on_error == E_DEFAULT {
        if p_dest.i_p_key >= 0 {
            on_error = p_dest.key_conf as i32;
        }
        if on_error == E_DEFAULT {
            on_error = E_ABORT;
        }
    }
    let Some(p_src_list) = p_select.p_src.as_deref() else {
        return false; // alocado mesmo sem cláusula FROM
    };
    if p_src_list.a.len() != 1 {
        return false; // o FROM tem de ter exatamente um termo
    }
    if p_src_list.a[0].p_select.is_some() {
        return false; // o FROM não pode ter subconsulta
    }
    if p_select.p_where.is_some() {
        return false; // o SELECT não pode ter WHERE
    }
    if p_select.p_order_by.is_some() {
        return false; // nem ORDER BY
    }
    // Não precisa testar HAVING: com HAVING e sem ORDER BY dá erro.
    if p_select.p_group_by.is_some() {
        return false; // nem GROUP BY
    }
    if p_select.p_limit.is_some() {
        return false; // nem LIMIT
    }
    if p_select.p_prior.is_some() {
        return false; // nem ser consulta composta
    }
    if (p_select.sel_flags & SF_DISTINCT) != 0 {
        return false; // nem DISTINCT
    }
    let Some(p_e_list) = p_select.p_e_list.as_deref() else {
        return false;
    };
    if p_e_list.a.len() != 1 {
        return false; // o resultado tem de ter exatamente uma coluna
    }
    if p_e_list.a[0].p_expr.as_deref().map_or(true, |e| e.op != TK_ASTERISK) {
        return false; // e ela tem de ser o operador especial "*"
    }

    // Aqui já se sabe que o comando tem a forma sintática certa. Agora checa a semântica.
    let p_item = &p_src_list.a[0];
    let Some(p_src) = locate_table_item(db, parse, 0, p_item) else {
        return false; // o FROM não tem uma tabela de verdade
    };
    if p_src.tnum == p_dest.tnum && p_src.p_schema == p_dest.p_schema {
        // Possível por causa de sqlite_schema.rootpage ruim.
        return false; // tab1 e tab2 não podem ser a mesma tabela
    }
    if p_dest.has_rowid() != p_src.has_rowid() {
        return false; // origem e destino têm de ser as duas WITHOUT ROWID ou as duas não
    }
    if !p_src.is_ordinary_table() {
        return false; // tab2 não pode ser view nem tabela virtual
    }
    if p_dest.n_col != p_src.n_col {
        return false; // o número de colunas tem de ser o mesmo
    }
    if p_dest.i_p_key != p_src.i_p_key {
        return false; // as duas têm de ter o mesmo INTEGER PRIMARY KEY
    }
    if (p_dest.tab_flags & TF_STRICT) != 0 && (p_src.tab_flags & TF_STRICT) == 0 {
        return false; // não alimenta tabela STRICT com uma não STRICT
    }
    let vacuum = (db.m_db_flags & DBFLAG_VACUUM) != 0;
    for i in 0..p_dest.n_col as usize {
        let p_dest_col = &p_dest.a_col[i];
        let p_src_col = &p_src.a_col[i];
        // Mesmo que t1 e t2 tenham esquemas idênticos, se têm colunas geradas o comando
        // `INSERT INTO t2 SELECT * FROM t1;` é semanticamente incorreto: os valores das colunas
        // geradas voltam do SELECT à direita, mas o INSERT à esquerda quer que sejam omitidos.
        // Mesmo assim é uma abreviação útil para pedir a transferência em bloco de todo o
        // conteúdo de t1 para t2. Em teoria podia ser desligada (exceto para o VACUUM, que
        // precisa dela), mas parece inofensiva e presta um serviço útil.
        if (p_dest_col.col_flags & COLFLAG_GENERATED) != (p_src_col.col_flags & COLFLAG_GENERATED) {
            return false; // as duas colunas têm o mesmo tipo de coluna gerada
        }
        // Mas a transferência só vale se origem e destino têm exatamente as mesmas expressões
        // nas colunas geradas (podia ser relaxado para VIRTUAL).
        if (p_dest_col.col_flags & COLFLAG_GENERATED) != 0
            && expr_compare(
                None,
                column_expr(&p_src, p_src_col),
                column_expr(p_dest, p_dest_col),
                -1,
            ) != 0
        {
            return false; // expressões geradoras diferentes
        }
        if p_dest_col.affinity != p_src_col.affinity {
            return false; // a afinidade tem de ser a mesma em todas as colunas
        }
        if stricmp_opt(column_coll(p_dest_col), column_coll(p_src_col)) != 0 {
            return false; // a colação tem de ser a mesma em todas as colunas
        }
        if p_dest_col.not_null != 0 && p_src_col.not_null == 0 {
            return false; // tab2 tem de ser NOT NULL se tab1 é
        }
        // Os DEFAULT da segunda coluna em diante têm de ser iguais.
        if (p_dest_col.col_flags & COLFLAG_GENERATED) == 0 && i > 0 {
            let p_dest_expr = column_expr(p_dest, p_dest_col);
            let p_src_expr = column_expr(&p_src, p_src_col);
            debug_assert!(p_dest_expr.map_or(true, |e| e.op == crate::consts::TK_SPAN));
            debug_assert!(p_src_expr.map_or(true, |e| e.op == crate::consts::TK_SPAN));
            if p_dest_expr.is_some() != p_src_expr.is_some() {
                return false; // os DEFAULT têm de ser iguais em todas as colunas
            }
            if let (Some(d), Some(s)) = (p_dest_expr, p_src_expr) {
                if d.z_token() != s.z_token() {
                    return false; // os DEFAULT têm de ser iguais em todas as colunas
                }
            }
        }
    }
    let mut dest_has_unique_idx = false; // verdadeiro se o destino tem índice UNIQUE
    for p_dest_idx in p_dest.p_index.iter() {
        if p_dest_idx.is_unique_index() {
            dest_has_unique_idx = true;
        }
        let Some(p_src_idx) = p_src.p_index.iter().find(|s| xfer_compatible_index(p_dest_idx, s))
        else {
            return false; // p_dest_idx não tem índice correspondente na origem
        };
        // O `sqlite3FaultSim(411)` do C devolve sempre SQLITE_OK fora dos testes.
        if p_src_idx.tnum == p_dest_idx.tnum && p_src.p_schema == p_dest.p_schema {
            return false; // esquema corrompido: dois índices no mesmo btree
        }
    }
    if p_dest.p_check.is_some()
        && !vacuum
        && expr_list_compare(p_src.p_check.as_deref(), p_dest.p_check.as_deref(), -1) != 0
    {
        return false; // tabelas com restrições CHECK diferentes (ticket #2252)
    }
    // Proíbe a transferência se o destino tem chaves estrangeiras. É mais restritivo que o
    // necessário, mas o maior beneficiado é o VACUUM, que desliga as chaves estrangeiras.
    // Ticket [6284df89debdfa61db8073e062908af0c9b6118e].
    debug_assert!(p_dest.is_ordinary_table());
    if (db.flags & SQLITE_FOREIGN_KEYS) != 0
        && p_dest.u_tab().map_or(false, |t| !t.p_f_key.is_empty())
    {
        return false;
    }
    if (db.flags & SQLITE_COUNT_ROWS) != 0 {
        return false; // a transferência não combina com PRAGMA count_changes
    }

    // Chegando aqui a otimização é pelo menos possível, embora possa só funcionar se o destino
    // (tab1) estiver inicialmente vazio.
    let i_db_src = schema_to_index(db, p_src.p_schema);
    let _ = get_vdbe(db, parse);
    code_verify_schema(db, parse, i_db_src);
    let i_src = parse.n_tab; // cursor da origem
    parse.n_tab += 1;
    let i_dest = parse.n_tab; // cursor do destino
    parse.n_tab += 1;
    let reg_autoinc = auto_inc_begin(db, parse, i_db_dest, p_dest);
    let reg_data = get_temp_reg(parse);
    add_op2(vdbe_of_parse(parse), OP_NULL as i32, 0, reg_data);
    let reg_rowid = get_temp_reg(parse);
    open_table(db, parse, i_dest, i_db_dest, p_dest, OP_OPENWRITE);
    debug_assert!(p_dest.has_rowid() || dest_has_unique_idx);
    let mut empty_dest_test: i32 = 0; // endereço do teste de pDest vazio
    let mut empty_src_test: i32 = 0; // endereço do teste de pSrc vazio
    let mut addr1: i32;
    if !vacuum
        && ((p_dest.i_p_key < 0 && !p_dest.p_index.is_empty()) // (1)
            || dest_has_unique_idx                             // (2)
            || (on_error != E_ABORT && on_error != E_ROLLBACK)) // (3)
    {
        // Em algumas circunstâncias só se pode rodar a otimização se o destino está inicialmente
        // vazio. Sem DBFLAG_Vacuum este bloco gera o código que descobre isso (com a flag o
        // destino está sempre vazio).
        //
        // Condições em que o destino tem de estar vazio:
        //
        // (1) Não há INTEGER PRIMARY KEY mas há índices. (Se o destino não está vazio os campos
        //     de rowid das entradas dos índices poderiam precisar mudar.)
        //
        // (2) O destino tem índice UNIQUE. (A transferência não consegue testar unicidade.)
        //
        // (3) `on_error` é algo diferente de OE_Abort e OE_Rollback.
        let v = vdbe_of_parse(parse);
        addr1 = add_op2(v, OP_REWIND as i32, i_dest, 0);
        empty_dest_test = add_op0(v, OP_GOTO as i32);
        jump_here(v, addr1);
    }
    if p_src.has_rowid() {
        let ins_flags: u8;
        open_table(db, parse, i_src, i_db_src, &p_src, OP_OPENREAD);
        empty_src_test = add_op2(vdbe_of_parse(parse), OP_REWIND as i32, i_src, 0);
        if p_dest.i_p_key >= 0 {
            addr1 = add_op2(vdbe_of_parse(parse), OP_ROWID as i32, i_src, reg_rowid);
            if !vacuum {
                let addr2 = add_op3(
                    vdbe_of_parse(parse),
                    OP_NOTEXISTS as i32,
                    i_dest,
                    0,
                    reg_rowid,
                );
                rowid_constraint(parse, on_error, p_dest);
                jump_here(vdbe_of_parse(parse), addr2);
            }
            auto_inc_step(parse, reg_autoinc, reg_rowid);
        } else if p_dest.p_index.is_empty() && (db.m_db_flags & DBFLAG_VACUUM_INTO) == 0 {
            addr1 = add_op2(vdbe_of_parse(parse), OP_NEWROWID as i32, i_dest, reg_rowid);
        } else {
            addr1 = add_op2(vdbe_of_parse(parse), OP_ROWID as i32, i_src, reg_rowid);
            debug_assert!((p_dest.tab_flags & crate::consts::TF_AUTOINCREMENT) == 0);
        }

        let v = vdbe_of_parse(parse);
        if vacuum {
            add_op1(v, OP_SEEKEND as i32, i_dest);
            ins_flags = OPFLAG_APPEND | OPFLAG_USESEEKRESULT | OPFLAG_PREFORMAT;
        } else {
            ins_flags = OPFLAG_NCHANGE | OPFLAG_LASTROWID | OPFLAG_APPEND | OPFLAG_PREFORMAT;
        }
        // Com SQLITE_ENABLE_PREUPDATE_HOOK, fora do VACUUM o registro é lido inteiro (OP_RowData)
        // e deixa de ser pré-formatado; no VACUUM copia a célula crua (OP_RowCell).
        let ins_flags = if !vacuum {
            add_op3(v, OP_ROWDATA as i32, i_src, reg_data, 1);
            ins_flags & !OPFLAG_PREFORMAT
        } else {
            add_op3(v, OP_ROWCELL as i32, i_dest, i_src, reg_rowid);
            ins_flags
        };
        add_op3(v, OP_INSERT as i32, i_dest, reg_data, reg_rowid);
        if !vacuum {
            change_p4(v, -1, P4::Table(Rc::clone(p_dest)));
        }
        change_p5(v, ins_flags as u16);

        add_op2(v, OP_NEXT as i32, i_src, addr1);
        add_op2(v, OP_CLOSE as i32, i_src, 0);
        add_op2(v, OP_CLOSE as i32, i_dest, 0);
    } else {
        table_lock(db, parse, i_db_dest, p_dest.tnum, true, &p_dest.z_name);
        table_lock(db, parse, i_db_src, p_src.tnum, false, &p_src.z_name);
    }
    for p_dest_idx in p_dest.p_index.iter() {
        let mut idx_ins_flags: u8 = 0;
        let Some(p_src_idx) = p_src.p_index.iter().find(|s| xfer_compatible_index(p_dest_idx, s))
        else {
            debug_assert!(false);
            continue;
        };
        add_op3(vdbe_of_parse(parse), OP_OPENREAD as i32, i_src, p_src_idx.tnum as i32, i_db_src);
        set_p4_key_info(parse, db, p_src_idx);
        vdbe_comment(vdbe_of_parse(parse), b"%s", &[text_arg(&p_src_idx.z_name)]);
        add_op3(
            vdbe_of_parse(parse),
            OP_OPENWRITE as i32,
            i_dest,
            p_dest_idx.tnum as i32,
            i_db_dest,
        );
        set_p4_key_info(parse, db, p_dest_idx);
        {
            let v = vdbe_of_parse(parse);
            change_p5(v, OPFLAG_BULKCSR as u16);
            vdbe_comment(v, b"%s", &[text_arg(&p_dest_idx.z_name)]);
        }
        addr1 = add_op2(vdbe_of_parse(parse), OP_REWIND as i32, i_src, 0);
        if vacuum {
            // Este INSERT é parte de um VACUUM, que garante que a tabela de destino está vazia.
            // Se todas as colunas indexadas usam a colação BINARY, também se pode supor que o
            // índice é povoado inserindo chaves em ordem estritamente crescente. Nesse caso, em
            // vez de procurar dentro do b-tree a cada OP_IdxInsert, um OP_SeekEnd vai antes do
            // OP_IdxInsert para posicionar no ponto onde cada chave entra, o que é mais rápido.
            //
            // Se alguma coluna indexada usa colação diferente de BINARY a otimização é
            // desligada: o usuário pode redefinir a colação e rodar VACUUM, e as chaves não
            // seriam gravadas em ordem estrita.
            let all_binary = p_src_idx
                .az_coll
                .iter()
                .take(p_src_idx.n_column as usize)
                .all(|z_coll| str_icmp(STR_BINARY.as_bytes(), z_coll) == 0);
            if all_binary {
                idx_ins_flags = OPFLAG_USESEEKRESULT | OPFLAG_PREFORMAT;
                let v = vdbe_of_parse(parse);
                add_op1(v, OP_SEEKEND as i32, i_dest);
                add_op2(v, OP_ROWCELL as i32, i_dest, i_src);
            }
        } else if !p_src.has_rowid()
            && p_dest_idx.idx_type == crate::consts::SQLITE_IDXTYPE_PRIMARYKEY
        {
            idx_ins_flags |= OPFLAG_NCHANGE;
        }
        if idx_ins_flags != (OPFLAG_USESEEKRESULT | OPFLAG_PREFORMAT) {
            add_op3(vdbe_of_parse(parse), OP_ROWDATA as i32, i_src, reg_data, 1);
            if !vacuum && !p_dest.has_rowid() && p_dest_idx.is_primary_key_index() {
                code_without_rowid_preupdate(parse, p_dest, i_dest, reg_data);
            }
        }
        let v = vdbe_of_parse(parse);
        add_op2(v, OP_IDXINSERT as i32, i_dest, reg_data);
        change_p5(v, (idx_ins_flags | OPFLAG_APPEND) as u16);
        add_op2(v, OP_NEXT as i32, i_src, addr1 + 1);
        jump_here(v, addr1);
        add_op2(v, OP_CLOSE as i32, i_src, 0);
        add_op2(v, OP_CLOSE as i32, i_dest, 0);
    }
    if empty_src_test != 0 {
        jump_here(vdbe_of_parse(parse), empty_src_test);
    }
    release_temp_reg(parse, reg_rowid);
    release_temp_reg(parse, reg_data);
    if empty_dest_test != 0 {
        auto_increment_end(db, parse);
        let v = vdbe_of_parse(parse);
        add_op2(v, OP_HALT as i32, SQLITE_OK, 0);
        jump_here(v, empty_dest_test);
        add_op2(v, OP_CLOSE as i32, i_dest, 0);
        false
    } else {
        true
    }
}
