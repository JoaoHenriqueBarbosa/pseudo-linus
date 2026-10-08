//! Tradução de `expr.c`, segunda parte (chunks `expr_c.006` a `expr_c.011`, mais o miolo de
//! `sqlite3ExprCodeTarget` que atravessa `expr_c.012`): predicados de constância, `IN`,
//! subconsultas escalares (`sqlite3CodeSubselect`), `sqlite3ExprCodeIN`, leitura de colunas e
//! `sqlite3ExprCodeTarget`.
//!
//! Convenções (as mesmas de `expr.rs`, ver CONVENTIONS.md):
//!
//! - Funções recebem `(db: &mut Connection, parse: &mut Parse, ...)`; o `pParse->db` some.
//! - `own: Option<&Rc<Table>>` é a tabela dona da árvore (para `TabRef::Own`).
//! - Texto é `Vec<u8>`/`&[u8]`; opcodes `OP_*` são `u8` e as funções de `vdbeaux` recebem `i32`.
//! - Os walkers usam `Walker<C>` com o contexto próprio de cada passagem em `u`.
//! - Os callbacks de walker que precisam da conexão (`sqlite3FindFunction`) a levam emprestada
//!   dentro do contexto (`ConstCtx<'a>`).

use std::rc::Rc;

// Funções de outras fatias, chamadas pelo nome determinístico (assinaturas supostas no relatório).
pub(crate) use crate::vdbeaux::{vdbe_of_parse, current_addr};
use crate::build::{
    affinity_type, code_verify_schema, column_expr, key_info_alloc, may_abort, primary_key_index,
    table_column_to_index, table_column_to_storage, table_lock,
};
use crate::callback::find_function;
use crate::connection::{Connection, Parse, UserData};
use crate::consts::{
    Bitmask, BMS, COLFLAG_BUSY, COLFLAG_GENERATED, COLFLAG_NOTAVAIL, COLFLAG_VIRTUAL,
    EP_CAN_BE_NULL, EP_COLLATE, EP_COMMUTED, EP_CONST_FUNC, EP_FIXED_COL, EP_FROM_DDL,
    EP_INFIX_FUNC, EP_INNER_ON, EP_INT_VALUE, EP_IS_FALSE, EP_IS_TRUE, EP_LEAF, EP_OUTER_ON,
    EP_QUOTED, EP_REDUCED, EP_SKIP, EP_SUBRTN, EP_TOKEN_ONLY, EP_UNLIKELY, EP_VAR_SELECT,
    EP_WIN_FUNC, EP_X_IS_SELECT, IN_INDEX_EPH, IN_INDEX_INDEX_ASC, IN_INDEX_INDEX_DESC,
    IN_INDEX_LOOP, IN_INDEX_MEMBERSHIP, IN_INDEX_NOOP, IN_INDEX_NOOP_OK, IN_INDEX_ROWID,
    INLINEFUNC_AFFINITY, INLINEFUNC_COALESCE, INLINEFUNC_EXPR_COMPARE,
    INLINEFUNC_EXPR_IMPLIES_EXPR, INLINEFUNC_IIF, INLINEFUNC_IMPLIES_NONNULL_ROW,
    INLINEFUNC_SQLITE_OFFSET, JT_LEFT, JT_LTORJ, OE_ABORT, OE_FAIL, OE_IGNORE, OE_ROLLBACK,
    OPFLAG_BYTELENARG, OPFLAG_LENGTHARG, OPFLAG_NOCHNG, OPFLAG_TYPEOFARG, OP_ADDIMM, OP_AFFINITY,
    OP_BEGINSUBRTN, OP_BITAND, OP_BLOB, OP_CAST, OP_CLRSUBTYPE, OP_COLLSEQ, OP_COLUMN, OP_COPY,
    OP_EQ, OP_FOUND, OP_GOSUB, OP_GOTO, OP_HALT, OP_IDXINSERT, OP_IFNULLROW, OP_INT64,
    OP_INTEGER, OP_ISNULL, OP_ISTRUE, OP_MAKERECORD, OP_MOVE, OP_NE, OP_NEXT, OP_NOTFOUND,
    OP_NOTNULL, OP_NULL, OP_NULLROW, OP_OFFSET, OP_ONCE, OP_OPENDUP, OP_OPENEPHEMERAL,
    OP_OPENREAD, OP_PARAM, OP_REAL, OP_REALAFFINITY, OP_RETURN, OP_REWIND, OP_ROWID, OP_SCOPY,
    OP_SEEKROWID, OP_SUBTRACT, OP_VARIABLE, OP_VCOLUMN, OP_ZEROORNULL, P4_INT64, P4_REAL,
    P4_STATIC, SF_AGGREGATE, SF_CORRELATED, SF_DISTINCT, SQLITE_AFF_BLOB, SQLITE_AFF_FLEXNUM,
    SQLITE_AFF_NONE, SQLITE_AFF_NUMERIC, SQLITE_AFF_REAL, SQLITE_AFF_TEXT,
    SQLITE_CONSTRAINT_TRIGGER, SQLITE_ECEL_FACTOR, SQLITE_ERROR, SQLITE_FUNC_CONSTANT,
    SQLITE_FUNC_DIRECT, SQLITE_FUNC_INLINE, SQLITE_FUNC_LENGTH, SQLITE_FUNC_NEEDCOLL,
    SQLITE_FUNC_SLOCHNG, SQLITE_FUNC_TYPEOF, SQLITE_FUNC_UNSAFE, SQLITE_JUMPIFNULL,
    SQLITE_NULLEQ, SQLITE_OK, SQLITE_UTF8, SRT_EXISTS, SRT_MEM, SRT_SET, TK_AGG_COLUMN,
    TK_AGG_FUNCTION, TK_AND, TK_BETWEEN, TK_BITAND, TK_BITNOT, TK_BITOR, TK_BLOB, TK_CASE,
    TK_CAST, TK_COLLATE, TK_COLUMN, TK_CONCAT, TK_DOT, TK_EQ, TK_ERROR, TK_EXISTS, TK_FLOAT,
    TK_FUNCTION, TK_GE, TK_GT, TK_ID, TK_IF_NULL_ROW, TK_IN, TK_INTEGER, TK_IS, TK_ISNOT,
    TK_ISNULL, TK_LE, TK_LIMIT, TK_LSHIFT, TK_LT, TK_MINUS, TK_NE, TK_NOT, TK_NOTNULL, TK_NULL,
    TK_OR, TK_ORDER, TK_PLUS, TK_RAISE, TK_REGISTER, TK_REM, TK_RSHIFT, TK_SELECT,
    TK_SELECT_COLUMN, TK_SLASH, TK_SPAN, TK_STAR, TK_STRING, TK_TRIGGER, TK_TRUEFALSE, TK_TRUTH,
    TK_UMINUS, TK_UPLUS, TK_VARIABLE, TK_VECTOR, WRC_ABORT, WRC_CONTINUE, WRC_PRUNE, XN_EXPR,
    XN_ROWID,
};
use crate::expr::{
    binary_compare_coll_seq, code_compare, code_vector_compare, compare_affinity, expr as new_expr,
    expr_affinity, expr_code,
    expr_code_between, expr_code_copy, expr_code_expr_list, expr_code_factorable,
    expr_code_run_just_once, expr_code_temp, expr_coll_seq, expr_compare, expr_deferred_delete,
    expr_dup, expr_function_usable, expr_if_false, expr_implies_expr, expr_implies_non_null_row,
    expr_is_vector, expr_nn_coll_seq, expr_skip_collate_and_likely, expr_token_arg,
    expr_vector_size, get_temp_range, get_temp_reg, p_expr as new_p_expr, release_temp_range,
    release_temp_reg, select_dup, table_column_affinity, vector_field_subexpr,
    clear_temp_reg_cache,
};
use crate::insert::open_table;
use crate::update::column_default;
use crate::build::halt_constraint;
use crate::prepare::schema_to_index;
use crate::mem::CollSeq;
use crate::printf::PrintfArg;
use crate::select::{select, select_dest_init};
use crate::sqlite_int::{
    is_numeric_affinity, AggInfo, AggInfoId, Expr, ExprList, ExprU, ExprX, ExprY, Index, Select,
    SelectDest, SrcList, TabRef, Table, Walker,
};
use crate::util::{
    at, atof, dec_or_hex_to_i64, error_msg, hex_to_blob, is_nan, str_icmp, stricmp, strlen30,
    strnicmp,
};
use crate::vdbe_types::{Vdbe, P4};
use crate::vdbeaux::{
    add_function_call, add_op0, add_op1, add_op2, add_op3, add_op4, add_op4_dup8, add_op4_int,
    change_p3, change_p4, change_p5, change_to_noop, explain, get_last_op, jump_here, load_string,
    make_label, noop_comment as vdbe_noop_comment, resolve_label,
    scan_status_counters, scan_status_range, set_p4_key_info, vdbe_comment, vdbe_goto,
};
use crate::vtab::vtab_overload_function;
use crate::walker::{walk_expr, walk_expr_list};

// ---------------------------------------------------------------------------------------------
// Auxiliares privados (não existem no C)
// ---------------------------------------------------------------------------------------------



/// A tabela a que um `TK_COLUMN` se refere; `own` é a tabela dona da árvore.
fn table_of<'a>(p: &'a Expr, own: Option<&'a Rc<Table>>) -> Option<&'a Rc<Table>> {
    match p.y_tab() {
        Some(TabRef::Rc(t)) => Some(t),
        Some(TabRef::Own) => own,
        None => None,
    }
}

/// `pSelect->pEList->a[i].pExpr`.
fn select_result_expr(s: &Select, i: usize) -> Option<&Expr> {
    s.p_e_list.as_deref()?.a.get(i)?.p_expr.as_deref()
}

// ---------------------------------------------------------------------------------------------
// Constância (chunk 006)
// ---------------------------------------------------------------------------------------------

/// `sqlite3SelectWalkFail`: callback de SELECT que sempre "falha" (`eCode = 0` e aborta).
pub fn select_walk_fail<C>(w: &mut Walker<C>, _not_used: &mut Select) -> i32 {
    w.e_code = 0;
    WRC_ABORT
}

/// `sqlite3IsTrueOrFalse`: `EP_IsTrue` para "true", `EP_IsFalse` para "false", senão 0.
pub fn is_true_or_false(z_in: &[u8]) -> u32 {
    if str_icmp(z_in, b"true") == 0 {
        return EP_IS_TRUE;
    }
    if str_icmp(z_in, b"false") == 0 {
        return EP_IS_FALSE;
    }
    0
}

/// `sqlite3ExprIdToTrueFalse`: converte um ID "true"/"false" em `TK_TRUEFALSE`. Devolve diferente
/// de zero se converteu.
pub fn expr_id_to_true_false(p_expr: &mut Expr) -> i32 {
    debug_assert!(p_expr.op == TK_ID || p_expr.op == TK_STRING);
    if !p_expr.has_property(EP_QUOTED | EP_INT_VALUE) {
        let v = match p_expr.z_token() {
            Some(z) => is_true_or_false(&z[..strlen30(z) as usize]),
            None => 0,
        };
        if v != 0 {
            p_expr.op = TK_TRUEFALSE;
            p_expr.set_property(v);
            return 1;
        }
    }
    0
}

/// `sqlite3ExprTruthValue`: 1 para um `TK_TRUEFALSE` verdadeiro, 0 para falso.
pub fn expr_truth_value(p_expr: &Expr) -> i32 {
    let p = expr_skip_collate_and_likely(Some(p_expr)).unwrap_or(p_expr);
    debug_assert!(p.op == TK_TRUEFALSE);
    debug_assert!(!p.has_property(EP_INT_VALUE));
    // `zToken[4]==0`: "true" tem 4 letras, "false" tem 5.
    let z = p.z_token().unwrap_or(&[]);
    (at(z, 4) == 0) as i32
}

/// `sqlite3ExprSimplifiedAndOr`: elimina os termos sempre verdadeiros ou falsos de um AND/OR.
/// Devolve o nó simplificado, que é um nó da própria árvore (ou o original).
pub fn expr_simplified_and_or(p_expr: &Expr) -> &Expr {
    if p_expr.op == TK_AND || p_expr.op == TK_OR {
        let (Some(l), Some(r)) = (p_expr.p_left.as_deref(), p_expr.p_right.as_deref()) else {
            return p_expr;
        };
        let p_right = expr_simplified_and_or(r);
        let p_left = expr_simplified_and_or(l);
        if p_left.always_true() || p_right.always_false() {
            return if p_expr.op == TK_AND { p_right } else { p_left };
        } else if p_right.always_true() || p_left.always_false() {
            return if p_expr.op == TK_AND { p_left } else { p_right };
        }
    }
    p_expr
}

/// Contexto dos walkers de constância: a conexão (só para achar funções; `None` equivale a
/// `pWalker->pParse == 0`), o cursor de `u.iCur`, o `pGroupBy` de `exprNodeIsConstantOrGroupBy`
/// e a conta de `sqlite3WalkerDepth`.
#[derive(Default)]
pub struct ConstCtx<'a> {
    /// A conexão e o `Parse`, presentes quando `pWalker->pParse != 0`.
    pub pp: Option<(&'a mut Connection, &'a mut Parse)>,
    /// `u.iCur`.
    pub i_cur: i32,
    /// `u.pGroupBy`, o GROUP BY de `exprNodeIsConstantOrGroupBy` (cópia só leitura).
    pub p_group_by: Option<&'a ExprList>,
    /// A tabela dona das árvores (para `expr_compare`).
    pub own: Option<&'a Rc<Table>>,
}

/// `exprNodeIsConstantFunction`: o nó é `TK_FUNCTION`; descobre se a função é constante. Põe
/// `eCode = 0` se não for. Devolve sempre `WRC_ABORT` ou `WRC_PRUNE`.
fn expr_node_is_constant_function(w: &mut Walker<ConstCtx<'_>>, p_expr: &mut Expr) -> i32 {
    debug_assert!(p_expr.op == TK_FUNCTION);
    let n;
    if p_expr.has_property(EP_TOKEN_ONLY) || p_expr.x_list().is_none() {
        n = 0;
    } else {
        n = p_expr.x_list().map_or(0, |l| l.a.len() as i32);
        walk_expr_list(w, p_expr.x_list_mut());
        if w.e_code == 0 {
            return WRC_ABORT;
        }
    }
    let p_def = match w.u.pp.as_mut() {
        Some((db, _)) => {
            let enc = db.enc;
            find_function(db, p_expr.z_token().unwrap_or(&[]), n, enc, 0)
        }
        None => None,
    };
    match p_def {
        Some(d)
            if d.x_finalize.is_none()
                && (d.func_flags & (SQLITE_FUNC_CONSTANT | SQLITE_FUNC_SLOCHNG)) != 0
                && !p_expr.has_property(EP_WIN_FUNC) =>
        {
            WRC_PRUNE
        }
        _ => {
            w.e_code = 0;
            WRC_ABORT
        }
    }
}

/// `exprNodeIsConstant`: callback de expressão dos predicados de constância (ver o comentário do
/// C sobre os valores de `eCode`: 1, 2, 3, 4 ou 5).
fn expr_node_is_constant(w: &mut Walker<ConstCtx<'_>>, p_expr: &mut Expr) -> i32 {
    debug_assert!(w.e_code > 0);
    if w.e_code == 2 && p_expr.has_property(EP_OUTER_ON) {
        w.e_code = 0;
        return WRC_ABORT;
    }
    // O `switch` do C com quedas deliberadas vira três estágios.
    let mut stage = 0; // 0: TK_ID, 1: TK_COLUMN..., 2: TK_IF_NULL_ROW..., 3: TK_VARIABLE
    match p_expr.op {
        TK_FUNCTION => {
            if (w.e_code >= 4 || p_expr.has_property(EP_CONST_FUNC))
                && !p_expr.has_property(EP_WIN_FUNC)
            {
                if w.e_code == 5 {
                    p_expr.set_property(EP_FROM_DDL);
                }
                return WRC_CONTINUE;
            } else if w.u.pp.is_some() {
                return expr_node_is_constant_function(w, p_expr);
            } else {
                w.e_code = 0;
                return WRC_ABORT;
            }
        }
        TK_ID => {
            if expr_id_to_true_false(p_expr) != 0 {
                return WRC_PRUNE;
            }
            stage = 1;
        }
        TK_COLUMN | TK_AGG_FUNCTION | TK_AGG_COLUMN => stage = 1,
        TK_IF_NULL_ROW | TK_REGISTER | TK_DOT | TK_RAISE => stage = 2,
        TK_VARIABLE => stage = 3,
        _ => return WRC_CONTINUE,
    }
    if stage == 3 {
        if w.e_code == 5 {
            // Variável dentro de um CREATE lido do sqlite_schema: vira NULL em silêncio.
            p_expr.op = TK_NULL;
        } else if w.e_code == 4 {
            // Variável num CREATE vindo de sqlite3_prepare(): erro.
            w.e_code = 0;
            return WRC_ABORT;
        }
        return WRC_CONTINUE;
    }
    if stage == 1 {
        if p_expr.has_property(EP_FIXED_COL) && w.e_code != 2 {
            return WRC_CONTINUE;
        }
        if w.e_code == 3 && p_expr.i_table == w.u.i_cur {
            return WRC_CONTINUE;
        }
    }
    w.e_code = 0;
    WRC_ABORT
}

/// `exprIsConst`: roda o walker de constância com o `eCode` inicial dado.
fn expr_is_const(
    pp: Option<(&mut Connection, &mut Parse)>,
    p: Option<&mut Expr>,
    init_flag: u16,
) -> i32 {
    let mut w = Walker {
        x_expr_callback: Some(expr_node_is_constant),
        x_select_callback: Some(select_walk_fail::<ConstCtx<'_>>),
        x_select_callback2: None,
        walker_depth: 0,
        e_code: init_flag,
        m_w_flags: 0,
        u: ConstCtx { pp, i_cur: 0, p_group_by: None, own: None },
    };
    walk_expr(&mut w, p);
    w.e_code as i32
}

/// `sqlite3ExprIsConstant`: não zero se a expressão é constante. Com `pp` nulo (o `pParse`
/// nulo do C) toda chamada de função conta como não constante.
pub fn expr_is_constant(pp: Option<(&mut Connection, &mut Parse)>, p: Option<&mut Expr>) -> i32 {
    expr_is_const(pp, p, 1)
}

/// `sqlite3ExprIsConstantNotJoin` (`static` no C, usada por outros módulos do `expr.c`).
pub fn expr_is_constant_not_join(
    pp: Option<(&mut Connection, &mut Parse)>,
    p: Option<&mut Expr>,
) -> i32 {
    expr_is_const(pp, p, 2)
}

/// `exprSelectWalkTableConstant`: subconsultas só são constantes se não correlacionadas.
fn expr_select_walk_table_constant<C>(w: &mut Walker<C>, p_select: &mut Select) -> i32 {
    debug_assert!(w.e_code == 3 || w.e_code == 0);
    if (p_select.sel_flags & SF_CORRELATED) != 0 {
        w.e_code = 0;
        return WRC_ABORT;
    }
    WRC_PRUNE
}

/// `sqlite3ExprIsTableConstant`: constante para qualquer linha da tabela do cursor `i_cur`.
pub fn expr_is_table_constant(p: Option<&mut Expr>, i_cur: i32, b_allow_subq: bool) -> i32 {
    let mut w = Walker {
        x_expr_callback: Some(expr_node_is_constant),
        x_select_callback: Some(if b_allow_subq {
            expr_select_walk_table_constant::<ConstCtx<'_>>
        } else {
            select_walk_fail::<ConstCtx<'_>>
        }),
        x_select_callback2: None,
        walker_depth: 0,
        e_code: 3,
        m_w_flags: 0,
        u: ConstCtx { pp: None, i_cur, p_group_by: None, own: None },
    };
    walk_expr(&mut w, p);
    w.e_code as i32
}

/// `sqlite3ExprIsSingleTableConstraint`: `p_expr` restringe apenas a fonte `p_src_list.a[i_src]`
/// e não depende de nada mais. Otimização: na dúvida, devolve 0.
pub fn expr_is_single_table_constraint(
    p_expr: &mut Expr,
    p_src_list: &SrcList,
    i_src: usize,
    b_allow_subq: bool,
) -> i32 {
    let p_src = &p_src_list.a[i_src];
    if (p_src.fg.jointype & JT_LTORJ) != 0 {
        return 0; // regra (3)
    }
    if (p_src.fg.jointype & JT_LEFT) != 0 {
        if !p_expr.has_property(EP_OUTER_ON) {
            return 0; // regra (4a)
        }
        if p_expr.i_join() != p_src.i_cursor {
            return 0; // regra (4b)
        }
    } else if p_expr.has_property(EP_OUTER_ON) {
        return 0; // regra (5)
    }
    if p_expr.has_property(EP_OUTER_ON | EP_INNER_ON)
        && (p_src_list.a[0].fg.jointype & JT_LTORJ) != 0
    {
        for jj in 0..i_src {
            if p_expr.i_join() == p_src_list.a[jj].i_cursor {
                if (p_src_list.a[jj].fg.jointype & JT_LTORJ) != 0 {
                    return 0; // restrição (6)
                }
                break;
            }
        }
    }
    // Regras (1), (2a) e (2b):
    expr_is_table_constant(Some(p_expr), p_src.i_cursor, b_allow_subq)
}

// ---------------------------------------------------------------------------------------------
// Constância (chunk 007), inteiros, NULL, afinidade, IN
// ---------------------------------------------------------------------------------------------

/// `sqlite3IsBinary`: a colação é BINARY (ou ausente).
pub(crate) fn is_binary(p_coll: Option<&Rc<CollSeq>>) -> bool {
    p_coll.map_or(true, |c| c.name.as_slice() == b"BINARY")
}

/// `exprNodeIsConstantOrGroupBy`: callback de `sqlite3ExprIsConstantOrGroupBy`.
fn expr_node_is_constant_or_group_by(w: &mut Walker<ConstCtx<'_>>, p_expr: &mut Expr) -> i32 {
    let p_group_by = w.u.p_group_by;
    let own = w.u.own;
    // Se `p_expr` é idêntica a algum termo do GROUP BY, vale como constante.
    if let (Some(gb), Some((db, parse))) = (p_group_by, w.u.pp.as_mut()) {
        for item in &gb.a {
            let p = item.p_expr.as_deref();
            if expr_compare(None, Some(&*p_expr), p, -1) < 2 {
                let p_coll = expr_nn_coll_seq(db, parse, p, own);
                if is_binary(Some(&p_coll)) {
                    return WRC_PRUNE;
                }
            }
        }
    }
    // Uma subconsulta conta como variável.
    if p_expr.use_x_select() {
        w.e_code = 0;
        return WRC_ABORT;
    }
    expr_node_is_constant(w, p_expr)
}

/// `sqlite3ExprIsConstantOrGroupBy`: não zero se a expressão só tem constantes ou cópias de
/// termos de `p_group_by` que ordenam em BINARY (promoção de HAVING para WHERE).
pub fn expr_is_constant_or_group_by(
    db: &mut Connection,
    parse: &mut Parse,
    p: Option<&mut Expr>,
    p_group_by: Option<&ExprList>,
    own: Option<&Rc<Table>>,
) -> i32 {
    let mut w = Walker {
        x_expr_callback: Some(expr_node_is_constant_or_group_by),
        x_select_callback: None,
        x_select_callback2: None,
        walker_depth: 0,
        e_code: 1,
        m_w_flags: 0,
        u: ConstCtx { pp: Some((db, parse)), i_cur: 0, p_group_by, own },
    };
    walk_expr(&mut w, p);
    w.e_code as i32
}

/// `sqlite3ExprIsConstantOrFunction`: o DEFAULT de uma coluna é aceitável (constante ou chamada
/// de função com argumentos constantes)? `is_init` é verdadeiro ao ler o `sqlite_schema`.
pub fn expr_is_constant_or_function(p: Option<&mut Expr>, is_init: u8) -> i32 {
    debug_assert!(is_init == 0 || is_init == 1);
    expr_is_const(None, p, 4 + is_init as u16)
}

/// `sqlite3ExprIsInteger`: se `p` é um inteiro constante de 32 bits, devolve 1 e grava em `*p_value`.
pub fn expr_is_integer(p: &Expr, p_value: &mut i32) -> i32 {
    let mut rc = 0;
    if p.flags & EP_INT_VALUE != 0 {
        *p_value = p.i_value();
        return 1;
    }
    match p.op {
        TK_UPLUS => {
            if let Some(l) = p.p_left.as_deref() {
                rc = expr_is_integer(l, p_value);
            }
        }
        TK_UMINUS => {
            let mut v = 0;
            if let Some(l) = p.p_left.as_deref() {
                if expr_is_integer(l, &mut v) != 0 {
                    *p_value = v.wrapping_neg();
                    rc = 1;
                }
            }
        }
        _ => {}
    }
    rc
}

/// `sqlite3ExprCanBeNull` com a tabela dona da árvore: falso se a expressão nunca é NULL.
/// Na dúvida devolve verdadeiro.
pub fn expr_can_be_null_in(p: &Expr, own: Option<&Rc<Table>>) -> bool {
    let mut p = p;
    while p.op == TK_UPLUS || p.op == TK_UMINUS {
        match p.p_left.as_deref() {
            Some(l) => p = l,
            None => return true,
        }
    }
    let mut op = p.op;
    if op == TK_REGISTER {
        op = p.op2;
    }
    match op {
        TK_INTEGER | TK_STRING | TK_FLOAT | TK_BLOB => false,
        TK_COLUMN => {
            if p.has_property(EP_CAN_BE_NULL) {
                return true;
            }
            let Some(t) = table_of(p, own) else {
                return true; // referência a coluna de índice sobre expressão
            };
            p.i_column >= 0
                && !t.a_col.is_empty()
                && p.i_column < t.n_col as i32
                && t.a_col[p.i_column as usize].not_null == 0
        }
        _ => true,
    }
}

/// `sqlite3ExprCanBeNull`: como `expr_can_be_null_in` sem a tabela dona (uma coluna cuja tabela
/// é `TabRef::Own` conta como possivelmente NULL).
pub fn expr_can_be_null(p: Option<&Expr>) -> bool {
    match p {
        Some(e) => expr_can_be_null_in(e, None),
        None => true,
    }
}

/// `sqlite3ExprNeedsNoAffinityChange`: a constante não muda com o `OP_Affinity` de `aff`?
pub fn expr_needs_no_affinity_change(p: &Expr, aff: u8) -> bool {
    let mut p = p;
    let mut unary_minus = false;
    if aff == SQLITE_AFF_BLOB {
        return true;
    }
    while p.op == TK_UPLUS || p.op == TK_UMINUS {
        if p.op == TK_UMINUS {
            unary_minus = true;
        }
        match p.p_left.as_deref() {
            Some(l) => p = l,
            None => return false,
        }
    }
    let mut op = p.op;
    if op == TK_REGISTER {
        op = p.op2;
    }
    match op {
        TK_INTEGER | TK_FLOAT => aff >= SQLITE_AFF_NUMERIC,
        TK_STRING => !unary_minus && aff == SQLITE_AFF_TEXT,
        TK_BLOB => !unary_minus,
        TK_COLUMN => aff >= SQLITE_AFF_NUMERIC && p.i_column < 0,
        _ => false,
    }
}

/// `sqlite3IsRowid`: o nome é um alias de rowid?
pub fn is_rowid(z: &[u8]) -> bool {
    str_icmp(z, b"_ROWID_") == 0 || str_icmp(z, b"ROWID") == 0 || str_icmp(z, b"OID") == 0
}

/// `sqlite3RowidAlias`: um alias de rowid utilizável para `p_tab` (nenhuma coluna do usuário com
/// o mesmo nome), ou `None`.
pub fn rowid_alias(p_tab: &Table) -> Option<&'static [u8]> {
    const AZ_OPT: [&[u8]; 3] = [b"_ROWID_", b"ROWID", b"OID"];
    debug_assert!(p_tab.visible_rowid());
    for opt in AZ_OPT {
        let found = p_tab
            .a_col
            .iter()
            .take(p_tab.n_col.max(0) as usize)
            .any(|c| stricmp(Some(opt), Some(&c.z_cn_name[..strlen30(&c.z_cn_name) as usize])) == 0);
        if !found {
            return Some(opt);
        }
    }
    None
}

/// `isCandidateForInOpt`: se o lado direito do IN é um SELECT simplificável para acesso direto a
/// uma tabela, devolve o SELECT; senão `None`.
fn is_candidate_for_in_opt(p_x: &Expr) -> Option<&Select> {
    if !p_x.use_x_select() {
        return None; // não é subconsulta
    }
    if p_x.has_property(EP_VAR_SELECT) {
        return None; // subconsulta correlacionada
    }
    let p = p_x.x_select()?;
    if p.p_prior.is_some() {
        return None; // SELECT composto
    }
    if p.sel_flags & (SF_DISTINCT | SF_AGGREGATE) != 0 {
        return None;
    }
    if p.p_limit.is_some() {
        return None;
    }
    if p.p_where.is_some() {
        return None;
    }
    let p_src = p.p_src.as_deref()?;
    if p_src.a.len() != 1 {
        return None;
    }
    if p_src.a[0].p_select.is_some() {
        return None; // FROM não é subconsulta nem view
    }
    let p_tab = p_src.a[0].p_tab.as_ref()?;
    debug_assert!(!p_tab.is_view());
    if p_tab.is_virtual() {
        return None;
    }
    let p_e_list = p.p_e_list.as_deref()?;
    for item in &p_e_list.a {
        let p_res = item.p_expr.as_deref()?;
        if p_res.op != TK_COLUMN {
            return None;
        }
    }
    Some(p)
}

/// `sqlite3SetHasNullFlag`: gera o código que confere se a coluna mais à esquerda do índice
/// `i_cur` tem NULLs. `reg_has_null` fica NULL se houver, não NULL se não houver.
fn set_has_null_flag(v: &mut Vdbe, i_cur: i32, reg_has_null: i32) {
    add_op2(v, OP_INTEGER as i32, 0, reg_has_null);
    let addr1 = add_op1(v, OP_REWIND as i32, i_cur);
    add_op3(v, OP_COLUMN as i32, i_cur, 0, reg_has_null);
    change_p5(v, OPFLAG_TYPEOFARG as u16);
    vdbe_comment(v, b"first_entry_in(%d)", &[PrintfArg::Int(i_cur as i64)]);
    jump_here(v, addr1);
}

/// `sqlite3InRhsIsConstant`: o IN tem uma lista (não subconsulta) à direita; a lista é constante?
fn in_rhs_is_constant(db: &mut Connection, parse: &mut Parse, p_in: &mut Expr) -> bool {
    debug_assert!(!p_in.has_property(EP_X_IS_SELECT));
    let p_lhs = p_in.p_left.take();
    let res = expr_is_constant(Some((db, parse)), Some(p_in));
    p_in.p_left = p_lhs;
    res != 0
}

/// `sqlite3FindInIndex`: encontra ou cria a árvore-b que serve de lado direito do IN, abre um
/// cursor sobre ela (em `*pi_tab`) e devolve o tipo (`IN_INDEX_*`). Ver o comentário longo do C.
pub fn find_in_index(
    db: &mut Connection,
    parse: &mut Parse,
    p_x: &mut Expr,
    in_flags: u32,
    pr_rhs_has_null: Option<&mut i32>,
    mut ai_map: Option<&mut [i32]>,
    pi_tab: &mut i32,
    own: Option<&Rc<Table>>,
) -> i32 {
    let mut e_type = 0;
    debug_assert!(p_x.op == TK_IN);
    let must_be_unique = (in_flags & IN_INDEX_LOOP) != 0;
    let mut i_tab = parse.n_tab;
    parse.n_tab += 1;
    let mut pr_rhs_has_null = pr_rhs_has_null;

    // Se o SELECT à direita não pode produzir NULL, não há o que registrar.
    if pr_rhs_has_null.is_some() && p_x.use_x_select() {
        if let Some(p_e_list) = p_x.x_select().and_then(|s| s.p_e_list.as_deref()) {
            let any_null = p_e_list.a.iter().any(|it| expr_can_be_null_in_opt(it.p_expr.as_deref(), own));
            if !any_null {
                pr_rhs_has_null = None;
            }
        }
    }

    // Tenta usar uma tabela ou índice existente (`isCandidateForInOpt`); só a tabela da fonte é
    // copiada, para soltar o empréstimo de `p_x`.
    let candidate: Option<Rc<Table>> = if parse.n_err == 0 {
        is_candidate_for_in_opt(p_x).and_then(|p| p.p_src.as_deref()?.a[0].p_tab.clone())
    } else {
        None
    };
    if let Some(p_tab) = candidate {
        let n_expr = p_x
            .x_select()
            .and_then(|s| s.p_e_list.as_deref())
            .map_or(0, |l| l.a.len());
        // OP_Transaction e OP_TableLock para <table>.
        let i_db = schema_to_index(db, p_tab.p_schema);
        code_verify_schema(db, parse, i_db);
        table_lock(db, parse, i_db, p_tab.tnum, false, &p_tab.z_name);

        let first_col = p_x
            .x_select()
            .and_then(|s| select_result_expr(s, 0))
            .map_or(0, |e| e.i_column);
        if n_expr == 1 && first_col < 0 {
            // O caso "x IN (SELECT rowid FROM table)".
            let i_addr = add_op0(vdbe_of_parse(parse), OP_ONCE as i32);
            open_table(db, parse, i_tab, i_db, &p_tab, OP_OPENREAD);
            e_type = IN_INDEX_ROWID;
            explain(
                parse,
                db,
                false,
                b"USING ROWID SEARCH ON TABLE %s FOR IN-OPERATOR",
                &[PrintfArg::Text(Some(p_tab.z_name.clone()))],
            );
            jump_here(vdbe_of_parse(parse), i_addr);
        } else {
            let mut affinity_ok = true;
            // A afinidade usada em cada comparação precisa ser a da coluna da tabela à direita.
            let mut i = 0;
            while i < n_expr && affinity_ok {
                let idxaff;
                let cmpaff;
                {
                    let p_lhs = p_x.p_left.as_deref().and_then(|l| vector_field_subexpr(l, i as i32));
                    let i_col = p_x
                        .x_select()
                        .and_then(|s| select_result_expr(s, i))
                        .map_or(0, |e| e.i_column);
                    idxaff = table_column_affinity(&p_tab, i_col);
                    cmpaff = match p_lhs {
                        Some(l) => compare_affinity(l, idxaff, own),
                        None => idxaff,
                    };
                }
                match cmpaff {
                    SQLITE_AFF_BLOB => {}
                    SQLITE_AFF_TEXT => {
                        debug_assert!(idxaff == SQLITE_AFF_TEXT);
                    }
                    _ => affinity_ok = is_numeric_affinity(idxaff),
                }
                i += 1;
            }

            if affinity_ok {
                // Procura um índice existente que sirva.
                for p_idx in p_tab.p_index.iter() {
                    if e_type != 0 {
                        break;
                    }
                    if (p_idx.n_column as usize) < n_expr {
                        continue;
                    }
                    if p_idx.p_partial_idx_where.is_some() {
                        continue;
                    }
                    if p_idx.n_column as i32 >= BMS - 1 {
                        continue;
                    }
                    if must_be_unique
                        && (p_idx.n_key_col as usize > n_expr
                            || (p_idx.n_column as usize > n_expr && !p_idx.is_unique_index()))
                    {
                        continue; // índice não é único sobre as colunas do IN
                    }
                    let mut col_used: Bitmask = 0;
                    let mut i = 0;
                    while i < n_expr {
                        let p_lhs = p_x.p_left.as_deref().and_then(|l| vector_field_subexpr(l, i as i32));
                        let p_rhs = p_x
                            .x_select()
                            .and_then(|s| select_result_expr(s, i))
                            .expect("pRhs");
                        let p_req = match p_lhs {
                            Some(l) => binary_compare_coll_seq(db, parse, l, Some(p_rhs), own),
                            None => None,
                        };
                        let mut j = 0;
                        while j < n_expr {
                            if p_idx.ai_column[j] as i32 != p_rhs.i_column {
                                j += 1;
                                continue;
                            }
                            if let Some(req) = p_req.as_ref() {
                                if str_icmp(&req.name, &p_idx.az_coll[j]) != 0 {
                                    j += 1;
                                    continue;
                                }
                            }
                            break;
                        }
                        if j == n_expr {
                            break;
                        }
                        let m_col: Bitmask = 1 << j;
                        if m_col & col_used != 0 {
                            break; // cada coluna só uma vez
                        }
                        col_used |= m_col;
                        if let Some(map) = ai_map.as_deref_mut() {
                            map[i] = j as i32;
                        }
                        i += 1;
                    }
                    let all: Bitmask = (1 << n_expr) - 1;
                    if col_used == all {
                        // O índice é utilizável.
                        let i_addr = add_op0(vdbe_of_parse(parse), OP_ONCE as i32);
                        explain(
                            parse,
                            db,
                            false,
                            b"USING INDEX %s FOR IN-OPERATOR",
                            &[PrintfArg::Text(Some(p_idx.z_name.clone()))],
                        );
                        add_op3(vdbe_of_parse(parse), OP_OPENREAD as i32, i_tab, p_idx.tnum as i32, i_db);
                        set_p4_key_info(parse, db, p_idx);
                        vdbe_comment(
                            vdbe_of_parse(parse),
                            b"%s",
                            &[PrintfArg::Text(Some(p_idx.z_name.clone()))],
                        );
                        debug_assert!(IN_INDEX_INDEX_DESC == IN_INDEX_INDEX_ASC + 1);
                        e_type = IN_INDEX_INDEX_ASC + p_idx.a_sort_order[0] as i32;

                        if let Some(r) = pr_rhs_has_null.as_deref_mut() {
                            parse.n_mem += 1;
                            *r = parse.n_mem;
                            if n_expr == 1 {
                                set_has_null_flag(vdbe_of_parse(parse), i_tab, *r);
                            }
                        }
                        jump_here(vdbe_of_parse(parse), i_addr);
                    }
                }
            }
        }
    }

    // Sem índice pré-existente, IN_INDEX_NOOP permitido, lado direito é lista e (não constante ou
    // com no máximo dois termos): não vale a pena criar tabela efêmera.
    if e_type == 0
        && (in_flags & IN_INDEX_NOOP_OK) != 0
        && p_x.use_x_list()
        && (!in_rhs_is_constant(db, parse, p_x)
            || p_x.x_list().map_or(0, |l| l.a.len()) <= 2)
    {
        parse.n_tab -= 1; // desfaz a alocação do cursor não usado
        i_tab = -1;
        e_type = IN_INDEX_NOOP;
    }

    if e_type == 0 {
        // Gera uma tabela efêmera.
        let saved_n_query_loop = parse.n_query_loop;
        let mut r_may_have_null = 0;
        e_type = IN_INDEX_EPH;
        if (in_flags & IN_INDEX_LOOP) != 0 {
            parse.n_query_loop = 0;
        } else if let Some(r) = pr_rhs_has_null.as_deref_mut() {
            parse.n_mem += 1;
            r_may_have_null = parse.n_mem;
            *r = r_may_have_null;
        }
        debug_assert!(p_x.op == TK_IN);
        code_rhs_of_in(db, parse, p_x, i_tab, own);
        if r_may_have_null != 0 {
            set_has_null_flag(vdbe_of_parse(parse), i_tab, r_may_have_null);
        }
        parse.n_query_loop = saved_n_query_loop;
    }

    if let Some(map) = ai_map {
        if e_type != IN_INDEX_INDEX_ASC && e_type != IN_INDEX_INDEX_DESC {
            let n = p_x.p_left.as_deref().map_or(0, expr_vector_size);
            for i in 0..n as usize {
                map[i] = i as i32;
            }
        }
    }
    *pi_tab = i_tab;
    e_type
}

/// `expr_can_be_null_in` sobre uma expressão opcional (nula conta como "pode ser NULL").
fn expr_can_be_null_in_opt(p: Option<&Expr>, own: Option<&Rc<Table>>) -> bool {
    p.map_or(true, |e| expr_can_be_null_in(e, own))
}

// ---------------------------------------------------------------------------------------------
// Subconsultas e IN (chunk 008)
// ---------------------------------------------------------------------------------------------

/// `pExpr->y.sub`: `(iAddr, regReturn)` de uma expressão com `EP_Subrtn`.
fn sub_of(p: &Expr) -> (i32, i32) {
    match p.y {
        ExprY::Sub { i_addr, reg_return } => (i_addr, reg_return),
        _ => (0, 0),
    }
}

/// `exprINAffinity`: a afinidade a usar em cada coluna da comparação de um `(?, ?...) IN(...)`.
fn expr_in_affinity(p_expr: &Expr, own: Option<&Rc<Table>>) -> Vec<u8> {
    let p_left = p_expr.p_left.as_deref().expect("pLeft");
    let n_val = expr_vector_size(p_left);
    let p_select = if p_expr.use_x_select() { p_expr.x_select() } else { None };
    debug_assert!(p_expr.op == TK_IN);
    let mut z_ret = Vec::with_capacity(n_val as usize);
    for i in 0..n_val {
        let a = vector_field_subexpr(p_left, i).map_or(0, |p_a| expr_affinity(p_a, own));
        match p_select {
            Some(s) => {
                let e = select_result_expr(s, i as usize).expect("pEList");
                z_ret.push(compare_affinity(e, a, own));
            }
            None => z_ret.push(a),
        }
    }
    z_ret
}

/// `sqlite3SubselectError`: "sub-select returns N columns - expected M".
pub fn subselect_error(db: &mut Connection, parse: &mut Parse, n_actual: i32, n_expect: i32) {
    if parse.n_err == 0 {
        error_msg(
            db,
            parse,
            b"sub-select returns %d columns - expected %d",
            &[PrintfArg::Int(n_actual as i64), PrintfArg::Int(n_expect as i64)],
        );
    }
}

/// `sqlite3VectorErrorMsg`: o vetor foi usado onde não é permitido.
pub fn vector_error_msg(db: &mut Connection, parse: &mut Parse, p_expr: &Expr) {
    if p_expr.use_x_select() {
        let n = p_expr
            .x_select()
            .and_then(|s| s.p_e_list.as_deref())
            .map_or(0, |l| l.a.len() as i32);
        subselect_error(db, parse, n, 1);
    } else {
        error_msg(db, parse, b"row value misused", &[]);
    }
}

/// `sqlite3CodeRhsOfIN`: gera o código que constrói a tabela efêmera com os termos do lado direito
/// de um IN (`x IN (4,5,11)` ou `x IN (SELECT a FROM b)`), no cursor `i_tab`.
pub fn code_rhs_of_in(
    db: &mut Connection,
    parse: &mut Parse,
    p_expr: &mut Expr,
    i_tab: i32,
    own: Option<&Rc<Table>>,
) {
    let mut addr_once = 0;
    debug_assert!(parse.p_vdbe.is_some());

    // A avaliação do IN se repete a cada encontro se o lado direito é uma subconsulta
    // correlacionada, uma lista com variáveis, ou se estamos num gatilho.
    if !p_expr.has_property(EP_VAR_SELECT) && parse.i_self_tab == 0 {
        // O lado direito pode ser reaproveitado.
        if p_expr.has_property(EP_SUBRTN) {
            addr_once = add_op0(vdbe_of_parse(parse), OP_ONCE as i32);
            if p_expr.use_x_select() {
                let sel_id = p_expr.x_select().map_or(0, |s| s.sel_id);
                explain(
                    parse,
                    db,
                    false,
                    b"REUSE LIST SUBQUERY %d",
                    &[PrintfArg::Int(sel_id as i64)],
                );
            }
            debug_assert!(p_expr.use_y_sub());
            let (i_addr, reg_return) = sub_of(p_expr);
            let v = vdbe_of_parse(parse);
            add_op2(v, OP_GOSUB as i32, reg_return, i_addr);
            debug_assert!(i_tab != p_expr.i_table);
            add_op2(v, OP_OPENDUP as i32, i_tab, p_expr.i_table);
            jump_here(v, addr_once);
            return;
        }

        // Começa a gerar a sub-rotina.
        debug_assert!(!p_expr.use_y_win());
        p_expr.set_property(EP_SUBRTN);
        debug_assert!(!p_expr.has_property(EP_TOKEN_ONLY | EP_REDUCED));
        parse.n_mem += 1;
        let reg_return = parse.n_mem;
        let i_addr = add_op2(vdbe_of_parse(parse), OP_BEGINSUBRTN as i32, 0, reg_return) + 1;
        p_expr.y = ExprY::Sub { i_addr, reg_return };

        addr_once = add_op0(vdbe_of_parse(parse), OP_ONCE as i32);
    }

    // É um IN vetorial?
    let n_val = p_expr.p_left.as_deref().map_or(1, expr_vector_size);

    // Constrói a tabela efêmera que guarda o lado direito.
    p_expr.i_table = i_tab;
    let addr = add_op2(vdbe_of_parse(parse), OP_OPENEPHEMERAL as i32, p_expr.i_table, n_val);
    if p_expr.use_x_select() {
        let sel_id = p_expr.x_select().map_or(0, |s| s.sel_id);
        vdbe_comment(vdbe_of_parse(parse), b"Result of SELECT %u", &[PrintfArg::Int(sel_id as i64)]);
    } else {
        vdbe_comment(vdbe_of_parse(parse), b"RHS of IN operator", &[]);
    }
    let mut p_key_info = Some(key_info_alloc(db, n_val, 1));

    if p_expr.use_x_select() {
        // Caso 1: expr IN (SELECT ...). Grava o resultado do select na tabela efêmera.
        let sel_id = p_expr.x_select().map_or(0, |s| s.sel_id);
        explain(
            parse,
            db,
            true,
            b"%sLIST SUBQUERY %d",
            &[
                PrintfArg::Text(Some(if addr_once != 0 { b"".to_vec() } else { b"CORRELATED ".to_vec() })),
                PrintfArg::Int(sel_id as i64),
            ],
        );
        let n_expr = p_expr
            .x_select()
            .and_then(|s| s.p_e_list.as_deref())
            .map_or(0, |l| l.a.len() as i32);
        // Se o lado esquerdo e o direito não casam, o erro já foi apanhado bem antes daqui.
        if n_expr == n_val {
            let mut dest = SelectDest::default();
            select_dest_init(&mut dest, SRT_SET as i32, i_tab);
            dest.z_aff_sdst = Some(expr_in_affinity(p_expr, own));
            if let Some(s) = p_expr.x_select_mut() {
                s.i_limit = 0;
            }
            let mut p_copy = select_dup(p_expr.x_select(), 0);
            let rc = if db.malloc_failed != 0 {
                1
            } else {
                match p_copy.as_deref_mut() {
                    Some(c) => select(db, parse, c, &mut dest),
                    None => 1,
                }
            };
            drop(p_copy);
            if rc != 0 {
                return;
            }
            let p_left = p_expr.p_left.as_deref();
            let p_e_list = p_expr.x_select().and_then(|s| s.p_e_list.as_deref());
            for i in 0..n_val as usize {
                let p = p_left.and_then(|l| vector_field_subexpr(l, i as i32));
                let rhs = p_e_list.and_then(|l| l.a.get(i)).and_then(|it| it.p_expr.as_deref());
                if let (Some(p), ki) = (p, p_key_info.as_mut()) {
                    let c = binary_compare_coll_seq(db, parse, p, rhs, own);
                    if let Some(ki) = ki {
                        ki.a_coll[i] = c;
                    }
                }
            }
        }
    } else if p_expr.x_list().is_some() {
        // Caso 2: expr IN (exprlist). Para cada expressão monta uma chave e guarda na tabela.
        let p_left = p_expr.p_left.as_deref();
        let mut affinity = p_left.map_or(0, |l| expr_affinity(l, own));
        if affinity <= SQLITE_AFF_NONE {
            affinity = SQLITE_AFF_BLOB;
        } else if affinity == SQLITE_AFF_REAL {
            affinity = SQLITE_AFF_NUMERIC;
        }
        if p_key_info.is_some() {
            let c = expr_coll_seq(db, parse, p_expr.p_left.as_deref(), own);
            if let Some(ki) = p_key_info.as_mut() {
                ki.a_coll[0] = c;
            }
        }

        let r1 = get_temp_reg(parse);
        let r2 = get_temp_reg(parse);
        let n_list = p_expr.x_list().map_or(0, |l| l.a.len());
        for i in 0..n_list {
            // Uma expressão não constante obriga a refazer este código a cada vez: desliga o
            // teste "só uma vez" gerado acima.
            if addr_once != 0 {
                let is_const = {
                    let e = p_expr
                        .x_list_mut()
                        .and_then(|l| l.a.get_mut(i))
                        .and_then(|it| it.p_expr.as_deref_mut());
                    expr_is_constant(Some((&mut *db, &mut *parse)), e) != 0
                };
                if !is_const {
                    change_to_noop(vdbe_of_parse(parse), db, addr_once - 1);
                    change_to_noop(vdbe_of_parse(parse), db, addr_once);
                    p_expr.clear_property(EP_SUBRTN);
                    addr_once = 0;
                }
            }

            // Avalia a expressão e insere na tabela temporária.
            if let Some(e) = p_expr
                .x_list_mut()
                .and_then(|l| l.a.get_mut(i))
                .and_then(|it| it.p_expr.as_deref_mut())
            {
                expr_code(db, parse, e, r1, own);
            }
            let v = vdbe_of_parse(parse);
            add_op4(v, OP_MAKERECORD as i32, r1, 1, r2, P4::Text(vec![affinity]));
            add_op4_int(v, OP_IDXINSERT as i32, i_tab, r2, r1, 1);
        }
        release_temp_reg(parse, r1);
        release_temp_reg(parse, r2);
    }
    if let Some(ki) = p_key_info {
        change_p4(vdbe_of_parse(parse), addr, P4::KeyInfo(Rc::new(ki)));
    }
    if addr_once != 0 {
        let v = vdbe_of_parse(parse);
        add_op1(v, OP_NULLROW as i32, i_tab);
        jump_here(v, addr_once);
        // Retorno da sub-rotina.
        debug_assert!(p_expr.use_y_sub());
        let (i_addr, reg_return) = sub_of(p_expr);
        add_op3(v, OP_RETURN as i32, reg_return, i_addr, 1);
        clear_temp_reg_cache(parse);
    }
}

/// `sqlite3CodeSubselect`: gera o código de uma subconsulta escalar (`(SELECT a FROM b)`) ou
/// de um EXISTS. Devolve o registrador do resultado (o mais à esquerda num SELECT de várias
/// colunas) ou 0 em caso de erro.
pub fn code_subselect(db: &mut Connection, parse: &mut Parse, p_expr: &mut Expr) -> i32 {
    let mut addr_once = 0;
    let mut r_reg = 0;
    debug_assert!(parse.p_vdbe.is_some());
    if parse.n_err != 0 {
        return 0;
    }
    debug_assert!(p_expr.op == TK_EXISTS || p_expr.op == TK_SELECT);
    debug_assert!(p_expr.use_x_select());
    let sel_id = p_expr.x_select().map_or(0, |s| s.sel_id);

    // Se já foi gerada, chama como sub-rotina.
    if p_expr.has_property(EP_SUBRTN) {
        explain(parse, db, false, b"REUSE SUBQUERY %d", &[PrintfArg::Int(sel_id as i64)]);
        debug_assert!(p_expr.use_y_sub());
        let (i_addr, reg_return) = sub_of(p_expr);
        add_op2(vdbe_of_parse(parse), OP_GOSUB as i32, reg_return, i_addr);
        return p_expr.i_table;
    }

    // Começa a gerar a sub-rotina.
    debug_assert!(!p_expr.use_y_win());
    debug_assert!(!p_expr.has_property(EP_REDUCED | EP_TOKEN_ONLY));
    p_expr.set_property(EP_SUBRTN);
    parse.n_mem += 1;
    let reg_return = parse.n_mem;
    let i_addr = add_op2(vdbe_of_parse(parse), OP_BEGINSUBRTN as i32, 0, reg_return) + 1;
    p_expr.y = ExprY::Sub { i_addr, reg_return };

    // Se não é correlacionada, roda uma vez só e reaproveita o resultado.
    if !p_expr.has_property(EP_VAR_SELECT) {
        addr_once = add_op0(vdbe_of_parse(parse), OP_ONCE as i32);
    }

    // SELECT: os valores das colunas da primeira linha vão para um vetor de registradores.
    // EXISTS: grava 0 ou 1 num registrador. Nos dois casos o select recebe "LIMIT 1".
    let addr_explain = explain(
        parse,
        db,
        true,
        b"%sSCALAR SUBQUERY %d",
        &[
            PrintfArg::Text(Some(if addr_once != 0 { b"".to_vec() } else { b"CORRELATED ".to_vec() })),
            PrintfArg::Int(sel_id as i64),
        ],
    );
    scan_status_counters(vdbe_of_parse(parse), db, addr_explain, addr_explain, -1);
    let n_reg = if p_expr.op == TK_SELECT {
        p_expr.x_select().and_then(|s| s.p_e_list.as_deref()).map_or(0, |l| l.a.len() as i32)
    } else {
        1
    };
    let mut dest = SelectDest::default();
    select_dest_init(&mut dest, 0, parse.n_mem + 1);
    parse.n_mem += n_reg;
    if p_expr.op == TK_SELECT {
        dest.e_dest = SRT_MEM;
        dest.i_sdst = dest.i_sd_parm;
        dest.n_sdst = n_reg;
        let v = vdbe_of_parse(parse);
        add_op3(v, OP_NULL as i32, 0, dest.i_sd_parm, dest.i_sd_parm + n_reg - 1);
        vdbe_comment(v, b"Init subquery result", &[]);
    } else {
        dest.e_dest = SRT_EXISTS;
        let v = vdbe_of_parse(parse);
        add_op2(v, OP_INTEGER as i32, 0, dest.i_sd_parm);
        vdbe_comment(v, b"Init EXISTS result", &[]);
    }
    let has_limit = p_expr.x_select().map_or(false, |s| s.p_limit.is_some());
    if has_limit {
        // O subselect já tem limite X: o novo limite é X<>0, que vale 1 ou 0.
        let mut p_limit = new_expr(TK_INTEGER as i32, Some(b"0"));
        if let Some(l) = p_limit.as_deref_mut() {
            l.aff_expr = SQLITE_AFF_NUMERIC;
            let left_dup = p_expr
                .x_select()
                .and_then(|s| s.p_limit.as_deref())
                .and_then(|lim| expr_dup(lim.p_left.as_deref(), 0));
            p_limit = new_p_expr(db, parse, TK_NE as i32, left_dup, p_limit);
        }
        if let Some(lim) = p_expr.x_select_mut().and_then(|s| s.p_limit.as_deref_mut()) {
            expr_deferred_delete(lim.p_left.take());
            lim.p_left = p_limit;
        }
    } else {
        // Sem limite prévio: acrescenta LIMIT 1.
        let p_limit = new_expr(TK_INTEGER as i32, Some(b"1"));
        let lim = new_p_expr(db, parse, TK_LIMIT as i32, p_limit, None);
        if let Some(s) = p_expr.x_select_mut() {
            s.p_limit = lim;
        }
    }
    if let Some(s) = p_expr.x_select_mut() {
        s.i_limit = 0;
    }
    let rc = match p_expr.x_select_mut() {
        Some(s) => select(db, parse, s, &mut dest),
        None => 1,
    };
    if rc != 0 {
        p_expr.op2 = p_expr.op;
        p_expr.op = TK_ERROR;
        return 0;
    }
    r_reg = dest.i_sd_parm;
    p_expr.i_table = r_reg;
    if addr_once != 0 {
        jump_here(vdbe_of_parse(parse), addr_once);
    }
    scan_status_range(vdbe_of_parse(parse), db, addr_explain, addr_explain, -1);

    // Retorno da sub-rotina.
    debug_assert!(p_expr.use_y_sub());
    let (i_addr, reg_return) = sub_of(p_expr);
    add_op3(vdbe_of_parse(parse), OP_RETURN as i32, reg_return, i_addr, 1);
    clear_temp_reg_cache(parse);
    r_reg
}

// ---------------------------------------------------------------------------------------------
// IN, literais (chunk 009)
// ---------------------------------------------------------------------------------------------

/// `sqlite3ExprCheckIN`: confere que o lado direito do IN tem tantas colunas quanto o vetor da
/// esquerda (ou que a esquerda é escalar se a direita não é subconsulta). Devolve 1 se há erro.
pub fn expr_check_in(db: &mut Connection, parse: &mut Parse, p_in: &Expr) -> i32 {
    let n_vector = p_in.p_left.as_deref().map_or(1, expr_vector_size);
    if p_in.use_x_select() && db.malloc_failed == 0 {
        let n = p_in
            .x_select()
            .and_then(|s| s.p_e_list.as_deref())
            .map_or(0, |l| l.a.len() as i32);
        if n_vector != n {
            subselect_error(db, parse, n, n_vector);
            return 1;
        }
    } else if n_vector != 1 {
        if let Some(l) = p_in.p_left.as_deref() {
            vector_error_msg(db, parse, l);
        }
        return 1;
    }
    0
}

/// `sqlite3ExprCodeIN`: gera o código de `x IN (SELECT ...)` ou `x IN (valor, valor, ...)`.
/// Salta para `dest_if_false` se o lado esquerdo não está no direito e para `dest_if_null` se o
/// resultado é desconhecido por causa de NULLs; se está, segue adiante.
pub(crate) fn expr_code_in(
    db: &mut Connection,
    parse: &mut Parse,
    p_expr: &mut Expr,
    dest_if_false: i32,
    dest_if_null: i32,
    own: Option<&Rc<Table>>,
) {
    let mut r_rhs_has_null = 0;
    let mut dest_step6 = 0;
    let mut i_tab = 0;
    let ok_const_factor = parse.ok_const_factor;

    if expr_check_in(db, parse, p_expr) != 0 {
        return;
    }
    let z_aff = expr_in_affinity(p_expr, own);
    let n_vector = p_expr.p_left.as_deref().map_or(1, expr_vector_size);
    let mut ai_map = vec![0i32; n_vector as usize];
    if db.malloc_failed != 0 {
        return;
    }

    // Tenta calcular o lado direito. Depois deste passo, se não vier IN_INDEX_NOOP, a tabela
    // aberta no cursor i_tab tem os valores do lado direito.
    debug_assert!(parse.p_vdbe.is_some());
    vdbe_noop_comment(vdbe_of_parse(parse), b"begin IN expr", &[]);
    let e_type = {
        let want_null = dest_if_false != dest_if_null;
        find_in_index(
            db,
            parse,
            p_expr,
            IN_INDEX_MEMBERSHIP | IN_INDEX_NOOP_OK,
            if want_null { Some(&mut r_rhs_has_null) } else { None },
            Some(&mut ai_map),
            &mut i_tab,
            own,
        )
    };
    debug_assert!(
        parse.n_err != 0
            || n_vector == 1
            || e_type == IN_INDEX_EPH
            || e_type == IN_INDEX_INDEX_ASC
            || e_type == IN_INDEX_INDEX_DESC
    );

    // Gera o lado esquerdo. Evita fatorar o LHS para fora do laço, mesmo constante, pois o
    // OP_Affinity pode ser aplicado ao registrador pelo código abaixo.
    debug_assert!(parse.ok_const_factor == ok_const_factor);
    parse.ok_const_factor = 0;
    let mut i_dummy = 0;
    let r_lhs_orig = match p_expr.p_left.as_deref_mut() {
        Some(l) => expr_code_vector(db, parse, l, &mut i_dummy, own),
        None => 0,
    };
    parse.ok_const_factor = ok_const_factor;
    let mut i = 0;
    while i < n_vector as usize && ai_map[i] == i as i32 {
        i += 1;
    }
    let r_lhs;
    if i == n_vector as usize {
        // Os campos do LHS não foram reordenados.
        r_lhs = r_lhs_orig;
    } else {
        // Reordena os campos do LHS segundo ai_map.
        r_lhs = get_temp_range(parse, n_vector);
        for i in 0..n_vector as usize {
            add_op3(
                vdbe_of_parse(parse),
                OP_COPY as i32,
                r_lhs_orig + i as i32,
                r_lhs + ai_map[i],
                0,
            );
        }
    }

    'finished: {
        // Se não há índice adequado, avalia com uma sequência de comparações (passo 1).
        if e_type == IN_INDEX_NOOP {
            let label_ok = make_label(parse);
            let mut reg_ck_null = 0;
            debug_assert!(p_expr.use_x_list());
            let n_list = p_expr.x_list().map_or(0, |l| l.a.len());
            let p_coll = expr_coll_seq(db, parse, p_expr.p_left.as_deref(), own);
            if dest_if_null != dest_if_false {
                reg_ck_null = get_temp_reg(parse);
                add_op3(vdbe_of_parse(parse), OP_BITAND as i32, r_lhs, r_lhs, reg_ck_null);
            }
            for ii in 0..n_list {
                let mut reg_to_free = 0;
                let (r2, can_be_null) = {
                    let e = p_expr
                        .x_list_mut()
                        .and_then(|l| l.a.get_mut(ii))
                        .and_then(|it| it.p_expr.as_deref_mut());
                    match e {
                        Some(e) => {
                            let r2 = expr_code_temp(db, parse, e, &mut reg_to_free, own);
                            (r2, expr_can_be_null_in(e, own))
                        }
                        None => (0, true),
                    }
                };
                if reg_ck_null != 0 && can_be_null {
                    add_op3(vdbe_of_parse(parse), OP_BITAND as i32, reg_ck_null, r2, reg_ck_null);
                }
                release_temp_reg(parse, reg_to_free);
                if (ii as i32) < n_list as i32 - 1 || dest_if_null != dest_if_false {
                    let op = if r_lhs != r2 { OP_EQ } else { OP_NOTNULL };
                    let v = vdbe_of_parse(parse);
                    add_op4(v, op as i32, r_lhs, label_ok, r2, P4::Coll(p_coll.clone()));
                    change_p5(v, z_aff[0] as u16);
                } else {
                    let op = if r_lhs != r2 { OP_NE } else { OP_ISNULL };
                    debug_assert!(dest_if_null == dest_if_false);
                    let v = vdbe_of_parse(parse);
                    add_op4(v, op as i32, r_lhs, dest_if_false, r2, P4::Coll(p_coll.clone()));
                    change_p5(v, (z_aff[0] | SQLITE_JUMPIFNULL) as u16);
                }
            }
            if reg_ck_null != 0 {
                let v = vdbe_of_parse(parse);
                add_op2(v, OP_ISNULL as i32, reg_ck_null, dest_if_null);
                vdbe_goto(v, dest_if_false);
            }
            resolve_label(parse, db, label_ok);
            release_temp_reg(parse, reg_ck_null);
            break 'finished;
        }

        // Passo 2: confere se o LHS tem colunas NULL. Se tem, o resultado é FALSE ou NULL e a
        // busca binária no RHS é pulada.
        let dest_step2;
        if dest_if_null == dest_if_false {
            dest_step2 = dest_if_false;
        } else {
            dest_step6 = make_label(parse);
            dest_step2 = dest_step6;
        }
        for i in 0..n_vector {
            let can_be_null = p_expr
                .p_left
                .as_deref()
                .and_then(|l| vector_field_subexpr(l, i))
                .map_or(true, |p| expr_can_be_null_in(p, own));
            if parse.n_err != 0 {
                return; // oom_error
            }
            if can_be_null {
                add_op2(vdbe_of_parse(parse), OP_ISNULL as i32, r_lhs + i, dest_step2);
            }
        }

        // Passo 3: o LHS é não NULL. Busca binária no RHS usando o LHS como sonda.
        let addr_truth_op;
        if e_type == IN_INDEX_ROWID {
            // O RHS é o ROWID de uma tabela, logo é não NULL: passos 3 e 4 viram um opcode só.
            let v = vdbe_of_parse(parse);
            add_op3(v, OP_SEEKROWID as i32, i_tab, dest_if_false, r_lhs);
            addr_truth_op = add_op0(v, OP_GOTO as i32); // retorna verdadeiro
        } else {
            let v = vdbe_of_parse(parse);
            add_op4(v, OP_AFFINITY as i32, r_lhs, n_vector, 0, P4::Text(z_aff.clone()));
            if dest_if_false == dest_if_null {
                // Passos 3 e 5 num opcode só.
                add_op4_int(v, OP_NOTFOUND as i32, i_tab, dest_if_false, r_lhs, n_vector);
                break 'finished;
            }
            // Passo 3 comum, quando FALSE e NULL são distintos.
            addr_truth_op = add_op4_int(v, OP_FOUND as i32, i_tab, 0, r_lhs, n_vector);
        }

        // Passo 4: se o RHS é conhecidamente não NULL e não achou, o resultado é FALSE.
        if r_rhs_has_null != 0 && n_vector == 1 {
            add_op2(vdbe_of_parse(parse), OP_NOTNULL as i32, r_rhs_has_null, dest_if_false);
        }

        // Passo 5: se FALSE e NULL não diferem, devolve falso.
        if dest_if_false == dest_if_null {
            vdbe_goto(vdbe_of_parse(parse), dest_if_false);
        }

        // Passo 6: percorre as linhas do RHS comparando com o LHS. Qualquer comparação NULL dá
        // NULL; se todas dão FALSE, o resultado é FALSE. Para LHS escalar basta a primeira linha.
        if dest_step6 != 0 {
            resolve_label(parse, db, dest_step6);
        }
        let addr_top = add_op2(vdbe_of_parse(parse), OP_REWIND as i32, i_tab, dest_if_false);
        let dest_not_null = if n_vector > 1 {
            make_label(parse)
        } else {
            // Para n_vector==1 combina os passos 6 e 7.
            dest_if_false
        };
        for i in 0..n_vector {
            let r3 = get_temp_reg(parse);
            let p_coll = {
                let p = p_expr.p_left.as_deref().and_then(|l| vector_field_subexpr(l, i));
                expr_coll_seq(db, parse, p, own)
            };
            let v = vdbe_of_parse(parse);
            add_op3(v, OP_COLUMN as i32, i_tab, i, r3);
            add_op4(v, OP_NE as i32, r_lhs + i, dest_not_null, r3, P4::Coll(p_coll));
            release_temp_reg(parse, r3);
        }
        add_op2(vdbe_of_parse(parse), OP_GOTO as i32, 0, dest_if_null);
        if n_vector > 1 {
            resolve_label(parse, db, dest_not_null);
            let v = vdbe_of_parse(parse);
            add_op2(v, OP_NEXT as i32, i_tab, addr_top + 1);
            // Passo 7: se chegou aqui, o resultado é FALSE.
            add_op2(v, OP_GOTO as i32, 0, dest_if_false);
        }

        // Aqui é onde se salta para devolver verdadeiro.
        jump_here(vdbe_of_parse(parse), addr_truth_op);
    }

    // sqlite3ExprCodeIN_finished:
    if r_lhs != r_lhs_orig {
        release_temp_reg(parse, r_lhs);
    }
    vdbe_comment(vdbe_of_parse(parse), b"end IN expr", &[]);
}

/// `codeReal`: gera a instrução que põe o real descrito por `z` no registrador `i_mem`.
fn code_real(v: &mut Vdbe, z: Option<&[u8]>, negate_flag: bool, i_mem: i32) {
    if let Some(z) = z {
        let (_, mut value) = atof(z, strlen30(z), SQLITE_UTF8 as u8, true);
        debug_assert!(!is_nan(value));
        if negate_flag {
            value = -value;
        }
        add_op4_dup8(v, OP_REAL as i32, 0, i_mem, 0, value.to_ne_bytes(), P4_REAL);
    }
}

/// `codeInteger`: gera a instrução que põe o inteiro do texto de `p_expr` no registrador `i_mem`.
fn code_integer(
    db: &mut Connection,
    parse: &mut Parse,
    p_expr: &Expr,
    neg_flag: bool,
    i_mem: i32,
) {
    if p_expr.flags & EP_INT_VALUE != 0 {
        let mut i = p_expr.i_value();
        debug_assert!(i >= 0);
        if neg_flag {
            i = -i;
        }
        add_op2(vdbe_of_parse(parse), OP_INTEGER as i32, i, i_mem);
    } else {
        let mut value: i64 = 0;
        let z = p_expr.z_token().unwrap_or(&[]);
        let c = dec_or_hex_to_i64(z, &mut value);
        if (c == 3 && !neg_flag) || c == 2 || (neg_flag && value == i64::MIN) {
            if strnicmp(Some(z), Some(b"0x"), 2) == 0 {
                error_msg(
                    db,
                    parse,
                    b"hex literal too big: %s%#T",
                    &[
                        PrintfArg::Text(Some(if neg_flag { b"-".to_vec() } else { b"".to_vec() })),
                        expr_token_arg(p_expr),
                    ],
                );
            } else {
                code_real(vdbe_of_parse(parse), Some(z), neg_flag, i_mem);
            }
        } else {
            if neg_flag {
                value = if c == 3 { i64::MIN } else { -value };
            }
            add_op4_dup8(
                vdbe_of_parse(parse),
                OP_INT64 as i32,
                0,
                i_mem,
                0,
                value.to_ne_bytes(),
                P4_INT64,
            );
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Leitura de colunas, vetores, funções inline (chunk 010)
// ---------------------------------------------------------------------------------------------

/// `sqlite3ExprCodeLoadIndexColumn`: gera o código que carrega em `reg_out` o valor da coluna
/// `i_idx_col` do índice `p_idx` (da tabela `p_tab`, o `pIdx->pTable` do C).
pub fn expr_code_load_index_column(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab: &Rc<Table>,
    p_idx: &Index,
    i_tab_cur: i32,
    i_idx_col: i32,
    reg_out: i32,
) {
    let i_tab_col = p_idx.ai_column[i_idx_col as usize];
    if i_tab_col == XN_EXPR {
        debug_assert!(p_idx.a_col_expr.is_some());
        let e = p_idx
            .a_col_expr
            .as_deref()
            .and_then(|l| l.a.get(i_idx_col as usize))
            .and_then(|it| it.p_expr.as_deref());
        debug_assert!(e.is_some());
        parse.i_self_tab = i_tab_cur + 1;
        expr_code_copy(db, parse, e, reg_out, Some(p_tab));
        parse.i_self_tab = 0;
    } else {
        expr_code_get_column_of_table(db, parse, p_tab, i_tab_cur, i_tab_col as i32, reg_out);
    }
}

/// `sqlite3ExprCodeGeneratedColumn`: gera o código que calcula a coluna gerada `i_col` de
/// `p_tab` e guarda o resultado em `reg_out`. `p_tab` é a tabela dona da expressão da coluna.
pub fn expr_code_generated_column(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab: &Rc<Table>,
    i_col: usize,
    reg_out: i32,
) {
    let n_err = parse.n_err;
    debug_assert!(parse.p_vdbe.is_some());
    debug_assert!(parse.i_self_tab != 0);
    let i_addr = if parse.i_self_tab > 0 {
        let i_self = parse.i_self_tab;
        add_op3(vdbe_of_parse(parse), OP_IFNULLROW as i32, i_self - 1, 0, reg_out)
    } else {
        0
    };
    let p_col = &p_tab.a_col[i_col];
    expr_code_copy(db, parse, column_expr(p_tab, p_col), reg_out, Some(p_tab));
    if p_col.affinity >= SQLITE_AFF_TEXT {
        add_op4(vdbe_of_parse(parse), OP_AFFINITY as i32, reg_out, 1, 0, P4::Text(vec![p_col.affinity]));
    }
    if i_addr != 0 {
        jump_here(vdbe_of_parse(parse), i_addr);
    }
    if parse.n_err > n_err {
        db.err_byte_offset = -1;
    }
}

/// `sqlite3ExprCodeGetColumnOfTable`: gera o código que extrai a coluna `i_col` de uma tabela.
///
/// Diferença do C: o `COLFLAG_BUSY` da coluna gerada (detecção de laço) não pode ser gravado na
/// tabela do esquema, que é imutável e compartilhada. Em vez disso a expressão da coluna é
/// gerada com uma cópia da tabela com o bit ligado como tabela dona da árvore; qualquer
/// referência recursiva à mesma coluna (`TabRef::Own`) encontra o bit e dá o mesmo erro.
pub fn expr_code_get_column_of_table(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab: &Rc<Table>,
    i_tab_cur: i32,
    i_col: i32,
    reg_out: i32,
) {
    debug_assert!(parse.p_vdbe.is_some());
    debug_assert!(i_col != XN_EXPR as i32);
    if i_col < 0 || i_col == p_tab.i_p_key as i32 {
        let v = vdbe_of_parse(parse);
        add_op2(v, OP_ROWID as i32, i_tab_cur, reg_out);
        vdbe_comment(v, b"%s.rowid", &[PrintfArg::Text(Some(p_tab.z_name.clone()))]);
    } else {
        let op;
        let x;
        if p_tab.is_virtual() {
            op = OP_VCOLUMN;
            x = i_col;
        } else if (p_tab.a_col[i_col as usize].col_flags & COLFLAG_VIRTUAL) != 0 {
            let p_col = &p_tab.a_col[i_col as usize];
            if (p_col.col_flags & COLFLAG_BUSY) != 0 {
                let name = &p_col.z_cn_name;
                error_msg(
                    db,
                    parse,
                    b"generated column loop on \"%s\"",
                    &[PrintfArg::Text(Some(name[..strlen30(name) as usize].to_vec()))],
                );
            } else {
                let saved_self_tab = parse.i_self_tab;
                let mut busy = (**p_tab).clone();
                busy.a_col[i_col as usize].col_flags |= COLFLAG_BUSY;
                let busy = Rc::new(busy);
                parse.i_self_tab = i_tab_cur + 1;
                expr_code_generated_column(db, parse, &busy, i_col as usize, reg_out);
                parse.i_self_tab = saved_self_tab;
            }
            return;
        } else if !p_tab.has_rowid() {
            x = match primary_key_index(p_tab) {
                Some(pk) => table_column_to_index(pk, i_col as i16) as i32,
                None => i_col,
            };
            op = OP_COLUMN;
        } else {
            x = table_column_to_storage(p_tab, i_col as i16) as i32;
            op = OP_COLUMN;
        }
        add_op3(vdbe_of_parse(parse), op as i32, i_tab_cur, x, reg_out);
        column_default(db, parse, p_tab, i_col, reg_out);
    }
}

/// `sqlite3ExprCodeGetColumn`: extrai a coluna `i_column` de `p_tab` (cursor `i_table`) para
/// `i_reg`. Com `i_column<0` extrai o rowid.
pub fn expr_code_get_column(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab: &Rc<Table>,
    i_column: i32,
    i_table: i32,
    i_reg: i32,
    p5: u8,
) -> i32 {
    debug_assert!(parse.p_vdbe.is_some());
    debug_assert!((p5 & (OPFLAG_NOCHNG | OPFLAG_TYPEOFARG | OPFLAG_LENGTHARG)) == p5);
    debug_assert!(p_tab.is_virtual() || (p5 & OPFLAG_NOCHNG) == 0);
    expr_code_get_column_of_table(db, parse, p_tab, i_table, i_column, i_reg);
    if p5 != 0 {
        if let Some(p_op) = get_last_op(vdbe_of_parse(parse)) {
            if p_op.opcode == OP_COLUMN {
                p_op.p5 = p5 as u16;
            }
            if p_op.opcode == OP_VCOLUMN {
                p_op.p5 = (p5 & OPFLAG_NOCHNG) as u16;
            }
        }
    }
    i_reg
}

/// `sqlite3ExprCodeMove`: move o conteúdo dos registradores `i_from..` para `i_to..`.
pub fn expr_code_move(parse: &mut Parse, i_from: i32, i_to: i32, n_reg: i32) {
    add_op3(vdbe_of_parse(parse), OP_MOVE as i32, i_from, i_to, n_reg);
}

/// `sqlite3ExprSkipCollateAndLikely`, versão mutável (só serve a `expr_to_register`).
pub(crate) fn skip_collate_and_likely_mut(p: Option<&mut Expr>) -> Option<&mut Expr> {
    let e = p?;
    if !e.has_property(EP_SKIP | EP_UNLIKELY) {
        return Some(e);
    }
    if e.has_property(EP_UNLIKELY) {
        return skip_collate_and_likely_mut(
            e.x_list_mut().and_then(|l| l.a.first_mut()).and_then(|it| it.p_expr.as_deref_mut()),
        );
    }
    if e.op == TK_COLLATE {
        return skip_collate_and_likely_mut(e.p_left.as_deref_mut());
    }
    Some(e)
}

/// `exprToRegister`: converte um nó escalar em `TK_REGISTER` apontando para `i_reg`, que o
/// chamador garante já conter o valor correto.
pub(crate) fn expr_to_register(p_expr: &mut Expr, i_reg: i32) {
    if let Some(p) = skip_collate_and_likely_mut(Some(p_expr)) {
        p.op2 = p.op;
        p.op = TK_REGISTER;
        p.i_table = i_reg;
        p.clear_property(EP_SKIP);
    }
}

/// `exprCodeVector`: avalia um vetor ou escalar em registradores contíguos e devolve o primeiro.
/// Se o resultado é um escalar temporário, `*pi_freeable` recebe o registrador; senão 0.
pub(crate) fn expr_code_vector(
    db: &mut Connection,
    parse: &mut Parse,
    p: &mut Expr,
    pi_freeable: &mut i32,
    own: Option<&Rc<Table>>,
) -> i32 {
    let n_result = expr_vector_size(p);
    if n_result == 1 {
        expr_code_temp(db, parse, p, pi_freeable, own)
    } else {
        *pi_freeable = 0;
        if p.op == TK_SELECT {
            code_subselect(db, parse, p)
        } else {
            let i_result = parse.n_mem + 1;
            parse.n_mem += n_result;
            debug_assert!(p.use_x_list());
            for i in 0..n_result as usize {
                if let Some(e) = p
                    .x_list_mut()
                    .and_then(|l| l.a.get_mut(i))
                    .and_then(|it| it.p_expr.as_deref_mut())
                {
                    expr_code_factorable(db, parse, e, i as i32 + i_result, own);
                }
            }
            i_result
        }
    }
}

/// `setDoNotMergeFlagOnCopy`: se o último opcode é `OP_Copy`, marca-o como não fundível.
fn set_do_not_merge_flag_on_copy(v: &mut Vdbe) {
    if get_last_op(v).map_or(false, |o| o.opcode == OP_COPY) {
        change_p5(v, 1);
    }
}

/// `exprCodeInlineFunction`: gera o código das funções SQL implementadas em linha.
fn expr_code_inline_function(
    db: &mut Connection,
    parse: &mut Parse,
    p_farg: &mut ExprList,
    i_func_id: i32,
    target: i32,
    own: Option<&Rc<Table>>,
) -> i32 {
    let mut target = target;
    let n_farg = p_farg.a.len();
    debug_assert!(parse.p_vdbe.is_some());
    debug_assert!(n_farg > 0); // toda função inline tem ao menos um argumento
    match i_func_id {
        INLINEFUNC_COALESCE => {
            // Implementação direta de COALESCE() e IFNULL(): evita avaliar argumentos depois do
            // primeiro não NULL.
            let end_coalesce = make_label(parse);
            debug_assert!(n_farg >= 2);
            if let Some(e) = p_farg.a[0].p_expr.as_deref_mut() {
                expr_code(db, parse, e, target, own);
            }
            for i in 1..n_farg {
                add_op2(vdbe_of_parse(parse), OP_NOTNULL as i32, target, end_coalesce);
                if let Some(e) = p_farg.a[i].p_expr.as_deref_mut() {
                    expr_code(db, parse, e, target, own);
                }
            }
            set_do_not_merge_flag_on_copy(vdbe_of_parse(parse));
            resolve_label(parse, db, end_coalesce);
        }
        INLINEFUNC_IIF => {
            // O `caseExpr` do C aponta para a lista dos argumentos; aqui a lista é movida para
            // o nó temporário e devolvida depois.
            let mut case_expr = Expr::default();
            case_expr.op = TK_CASE;
            case_expr.x = ExprX::List(Box::new(std::mem::take(p_farg)));
            let r = expr_code_target(db, parse, &mut case_expr, target, own);
            if let ExprX::List(l) = case_expr.x {
                *p_farg = *l;
            }
            return r;
        }
        INLINEFUNC_SQLITE_OFFSET => {
            let (is_col, i_table, i_column) = match p_farg.a[0].p_expr.as_deref() {
                Some(a) => (a.op == TK_COLUMN && a.i_table >= 0, a.i_table, a.i_column),
                None => (false, 0, 0),
            };
            let v = vdbe_of_parse(parse);
            if is_col {
                add_op3(v, OP_OFFSET as i32, i_table, i_column, target);
            } else {
                add_op2(v, OP_NULL as i32, 0, target);
            }
        }
        INLINEFUNC_EXPR_COMPARE => {
            // Compara duas expressões com sqlite3ExprCompare().
            debug_assert!(n_farg == 2);
            let r = expr_compare(
                None,
                p_farg.a[0].p_expr.as_deref(),
                p_farg.a[1].p_expr.as_deref(),
                -1,
            );
            add_op2(vdbe_of_parse(parse), OP_INTEGER as i32, r, target);
        }
        INLINEFUNC_EXPR_IMPLIES_EXPR => {
            debug_assert!(n_farg == 2);
            let r = expr_implies_expr(
                Some((&mut *db, &mut *parse)),
                p_farg.a[0].p_expr.as_deref(),
                p_farg.a[1].p_expr.as_deref(),
                -1,
            );
            add_op2(vdbe_of_parse(parse), OP_INTEGER as i32, r, target);
        }
        INLINEFUNC_IMPLIES_NONNULL_ROW => {
            // Resultado de sqlite3ExprImpliesNonNullRow().
            debug_assert!(n_farg == 2);
            let a1 = p_farg.a[1].p_expr.as_deref().map(|a| (a.op == TK_COLUMN, a.i_table));
            match a1 {
                Some((true, i_table)) => {
                    let r = expr_implies_non_null_row(p_farg.a[0].p_expr.as_deref(), i_table, 1);
                    add_op2(vdbe_of_parse(parse), OP_INTEGER as i32, r, target);
                }
                _ => {
                    add_op2(vdbe_of_parse(parse), OP_NULL as i32, 0, target);
                }
            }
        }
        INLINEFUNC_AFFINITY => {
            // AFFINITY() devolve um texto que descreve a afinidade do argumento (só para teste).
            const AZ_AFF: [&[u8]; 6] = [b"blob", b"text", b"numeric", b"integer", b"real", b"flexnum"];
            debug_assert!(n_farg == 1);
            let aff = p_farg.a[0].p_expr.as_deref().map_or(0, |e| expr_affinity(e, own));
            debug_assert!(aff <= SQLITE_AFF_NONE || (aff >= SQLITE_AFF_BLOB && aff <= SQLITE_AFF_FLEXNUM));
            let name: &[u8] = if aff <= SQLITE_AFF_NONE {
                b"none"
            } else {
                AZ_AFF[(aff - SQLITE_AFF_BLOB) as usize]
            };
            load_string(vdbe_of_parse(parse), target, name);
        }
        _ => {
            // UNLIKELY() não faz nada: o resultado é o valor do primeiro argumento.
            debug_assert!(n_farg == 1 || n_farg == 2);
            if let Some(e) = p_farg.a[0].p_expr.as_deref_mut() {
                target = expr_code_target(db, parse, e, target, own);
            }
        }
    }
    target
}

/// `sqlite3IndexedExprLookup`: se `p_expr` é uma das expressões indexadas de `Parse.p_idx_epr`,
/// lê o valor do índice e devolve o registrador (`target`); senão devolve -1.
fn indexed_expr_lookup(
    db: &mut Connection,
    parse: &mut Parse,
    p_expr: &mut Expr,
    target: i32,
    own: Option<&Rc<Table>>,
) -> i32 {
    for n in 0..parse.p_idx_epr.len() {
        let mut i_data_cur = parse.p_idx_epr[n].i_data_cur;
        if i_data_cur < 0 {
            continue;
        }
        if parse.i_self_tab != 0 {
            if parse.p_idx_epr[n].i_data_cur != parse.i_self_tab - 1 {
                continue;
            }
            i_data_cur = -1;
        }
        if expr_compare(None, Some(&*p_expr), parse.p_idx_epr[n].p_expr.as_deref(), i_data_cur) != 0 {
            continue;
        }
        let (p_aff, may_null, i_idx_cur, i_idx_col, z_idx_name) = {
            let p = &parse.p_idx_epr[n];
            (p.aff, p.b_maybe_null_row, p.i_idx_cur, p.i_idx_col, p.z_idx_name.clone())
        };
        debug_assert!(p_aff >= SQLITE_AFF_BLOB && p_aff <= SQLITE_AFF_NUMERIC);
        let expr_aff = expr_affinity(p_expr, own);
        if (expr_aff <= SQLITE_AFF_BLOB && p_aff != SQLITE_AFF_BLOB)
            || (expr_aff == SQLITE_AFF_TEXT && p_aff != SQLITE_AFF_TEXT)
            || (expr_aff >= SQLITE_AFF_NUMERIC && p_aff != SQLITE_AFF_NUMERIC)
        {
            // Afinidade divergente numa coluna gerada.
            continue;
        }
        let comment_args = [
            PrintfArg::Text(Some(z_idx_name.unwrap_or_default())),
            PrintfArg::Int(i_idx_col as i64),
        ];
        debug_assert!(parse.p_vdbe.is_some());
        if may_null {
            // Se o índice está numa linha NULL por causa de um outer join, o valor não pode ser
            // lido do índice: calcula-se pela expressão original.
            let addr = current_addr(parse);
            let v = vdbe_of_parse(parse);
            add_op3(v, OP_IFNULLROW as i32, i_idx_cur, addr + 3, target);
            add_op3(v, OP_COLUMN as i32, i_idx_cur, i_idx_col, target);
            vdbe_comment(v, b"%s expr-column %d", &comment_args);
            vdbe_goto(v, 0);
            let saved = std::mem::take(&mut parse.p_idx_epr);
            expr_code(db, parse, p_expr, target, own);
            parse.p_idx_epr = saved;
            jump_here(vdbe_of_parse(parse), addr + 2);
        } else {
            let v = vdbe_of_parse(parse);
            add_op3(v, OP_COLUMN as i32, i_idx_cur, i_idx_col, target);
            vdbe_comment(v, b"%s expr-column %d", &comment_args);
        }
        return target;
    }
    -1 // não achou
}

// ---------------------------------------------------------------------------------------------
// sqlite3ExprCodeTarget (chunks 011 e 012)
// ---------------------------------------------------------------------------------------------

/// `pExpr->pAggInfo` resolvido em `Parse.agg_infos` (ver o relatório: o campo ainda não existe).
fn agg_info_of(parse: &Parse, id: Option<AggInfoId>) -> Option<&AggInfo> {
    parse.agg_infos.get(id?.0 as usize)
}

/// `exprPartidxExprLookup`: se a coluna `p_expr` (um `TK_COLUMN` ou equivalente) pode ser trocada
/// por uma constante de `Parse.p_idx_part_expr`, gera o código e devolve o registrador; senão 0.
fn expr_partidx_expr_lookup(
    db: &mut Connection,
    parse: &mut Parse,
    p_expr: &Expr,
    i_target: i32,
    own: Option<&Rc<Table>>,
) -> i32 {
    for n in 0..parse.p_idx_part_expr.len() {
        let (i_idx_col, i_data_cur, may_null, i_idx_cur, aff) = {
            let p = &parse.p_idx_part_expr[n];
            (p.i_idx_col, p.i_data_cur, p.b_maybe_null_row, p.i_idx_cur, p.aff)
        };
        if p_expr.i_column == i_idx_col && p_expr.i_table == i_data_cur {
            let mut addr = 0;
            if may_null {
                addr = add_op1(vdbe_of_parse(parse), OP_IFNULLROW as i32, i_idx_cur);
            }
            // O C passa o próprio `p->pExpr`; aqui uma cópia, porque a geração de código precisa
            // de `parse` emprestado como mutável.
            let ret = match parse.p_idx_part_expr[n].p_expr.clone() {
                Some(mut e) => expr_code_target(db, parse, &mut e, i_target, own),
                None => 0,
            };
            let v = vdbe_of_parse(parse);
            add_op4(v, OP_AFFINITY as i32, ret, 1, 0, P4::Text(vec![aff]));
            if addr != 0 {
                jump_here(v, addr);
                change_p3(v, addr, ret);
            }
            return ret;
        }
    }
    0
}

/// `sqlite3ExprCodeTarget`: gera código para avaliar a expressão e tenta guardar o resultado em
/// `target`. Devolve o registrador onde o resultado ficou (não há garantia de ser `target`).
pub fn expr_code_target(
    db: &mut Connection,
    parse: &mut Parse,
    p_expr: &mut Expr,
    target: i32,
    own: Option<&Rc<Table>>,
) -> i32 {
    let mut in_reg = target;
    let mut reg_free1 = 0;
    let mut reg_free2 = 0;
    let mut p5: u8 = 0;
    debug_assert!(target > 0 && target <= parse.n_mem);
    debug_assert!(parse.p_vdbe.is_some());

    let mut cur: &mut Expr = p_expr;
    // `expr_code_doover` é o `continue` deste laço; o `break` é sair do `switch` do C.
    loop {
        let mut op = cur.op;
        if !parse.p_idx_epr.is_empty() && !cur.has_property(EP_LEAF) {
            let r1 = indexed_expr_lookup(db, parse, cur, target, own);
            if r1 >= 0 {
                return r1;
            }
        }
        debug_assert!(op != TK_ORDER);
        match op {
            TK_AGG_COLUMN | TK_COLUMN => {
                if op == TK_AGG_COLUMN {
                    // Copia o que precisa do AggInfo para soltar o empréstimo de `parse`.
                    let Some(info) = agg_info_of(parse, cur.p_agg_info) else {
                        debug_assert!(false);
                        add_op2(vdbe_of_parse(parse), OP_NULL as i32, 0, target);
                        break;
                    };
                    debug_assert!(cur.i_agg >= 0);
                    if cur.i_agg as usize >= info.a_col.len() {
                        // Acontece quando a tabela esquerda de um RIGHT JOIN é nula e usa
                        // um índice sobre expressão.
                        add_op2(vdbe_of_parse(parse), OP_NULL as i32, 0, target);
                        break;
                    }
                    let direct_mode = info.direct_mode != 0;
                    let use_sorting_idx = info.use_sorting_idx != 0;
                    let sorting_idx_p_tab = info.sorting_idx_p_tab;
                    let column_reg = info.column_reg(cur.i_agg as i32);
                    let (p_col_tab, i_column, i_sorter_column) = {
                        let c = &info.a_col[cur.i_agg as usize];
                        (c.p_tab.clone(), c.i_column, c.i_sorter_column)
                    };
                    if !direct_mode {
                        return column_reg;
                    } else if use_sorting_idx {
                        let v = vdbe_of_parse(parse);
                        add_op3(v, OP_COLUMN as i32, sorting_idx_p_tab, i_sorter_column as i32, target);
                        if let Some(p_tab) = p_col_tab {
                            if i_column < 0 {
                                vdbe_comment(v, b"%s.rowid", &[PrintfArg::Text(Some(p_tab.z_name.clone()))]);
                            } else {
                                let col = &p_tab.a_col[i_column as usize];
                                vdbe_comment(
                                    v,
                                    b"%s.%s",
                                    &[
                                        PrintfArg::Text(Some(p_tab.z_name.clone())),
                                        PrintfArg::Text(Some(col.z_cn_name[..strlen30(&col.z_cn_name) as usize].to_vec())),
                                    ],
                                );
                                if col.affinity == SQLITE_AFF_REAL {
                                    add_op1(v, OP_REALAFFINITY as i32, target);
                                }
                            }
                        }
                        return target;
                    } else if cur.y_tab().is_none() {
                        // Acontece quando o argumento de um agregado foi reescrito por
                        // aggregateConvertIndexedExprRefToColumn().
                        add_op3(
                            vdbe_of_parse(parse),
                            OP_COLUMN as i32,
                            cur.i_table,
                            cur.i_column,
                            target,
                        );
                        return target;
                    }
                    // Senão cai no caso TK_COLUMN.
                }
                // case TK_COLUMN:
                let mut i_tab = cur.i_table;
                if cur.has_property(EP_FIXED_COL) {
                    // Esta coluna é na verdade uma constante por causa das restrições do WHERE,
                    // codificada por `p_left`; aplica a afinidade da coluna da tabela.
                    let i_reg = match cur.p_left.as_deref_mut() {
                        Some(l) => expr_code_target(db, parse, l, target, own),
                        None => target,
                    };
                    debug_assert!(cur.use_y_tab());
                    let aff = table_of(cur, own).map_or(0, |t| table_column_affinity(t, cur.i_column));
                    if aff > SQLITE_AFF_BLOB {
                        // zAff[(aff-'B')*2] é o caractere da própria afinidade.
                        add_op4(vdbe_of_parse(parse), OP_AFFINITY as i32, i_reg, 1, 0, P4::Text(vec![aff]));
                    }
                    return i_reg;
                }
                if i_tab < 0 {
                    if parse.i_self_tab < 0 {
                        // Outras colunas da mesma linha (CHECK, colunas geradas ou inserção num
                        // índice parcial): a linha está desempacotada em registradores a
                        // partir de `0-(iSelfTab)`; o rowid fica logo antes da primeira coluna.
                        let i_col = cur.i_column;
                        debug_assert!(cur.use_y_tab());
                        let p_tab = table_of(cur, own).cloned().expect("pTab");
                        debug_assert!(i_col >= XN_ROWID as i32);
                        debug_assert!(i_col < p_tab.n_col as i32);
                        if i_col < 0 {
                            return -1 - parse.i_self_tab;
                        }
                        let p_col = &p_tab.a_col[i_col as usize];
                        let i_src = table_column_to_storage(&p_tab, i_col as i16) as i32 - parse.i_self_tab;
                        if (p_col.col_flags & COLFLAG_GENERATED) != 0 {
                            if (p_col.col_flags & COLFLAG_BUSY) != 0 {
                                let name = &p_col.z_cn_name;
                                error_msg(
                                    db,
                                    parse,
                                    b"generated column loop on \"%s\"",
                                    &[PrintfArg::Text(Some(name[..strlen30(name) as usize].to_vec()))],
                                );
                                return 0;
                            }
                            if (p_col.col_flags & COLFLAG_NOTAVAIL) != 0 {
                                // O BUSY do C é gravado numa cópia da tabela (ver
                                // `expr_code_get_column_of_table`); o NOTAVAIL não é limpo
                                // porque a tabela é imutável (ver o relatório).
                                let mut busy = (*p_tab).clone();
                                busy.a_col[i_col as usize].col_flags |= COLFLAG_BUSY;
                                let busy = Rc::new(busy);
                                expr_code_generated_column(db, parse, &busy, i_col as usize, i_src);
                            }
                            return i_src;
                        } else if p_col.affinity == SQLITE_AFF_REAL {
                            let v = vdbe_of_parse(parse);
                            add_op2(v, OP_SCOPY as i32, i_src, target);
                            add_op1(v, OP_REALAFFINITY as i32, target);
                            return target;
                        } else {
                            return i_src;
                        }
                    } else {
                        // Expressão de um índice cujos nomes de coluna se referem à tabela
                        // dona do índice.
                        i_tab = parse.i_self_tab - 1;
                    }
                } else if !parse.p_idx_part_expr.is_empty() {
                    let r1 = expr_partidx_expr_lookup(db, parse, cur, target, own);
                    if r1 != 0 {
                        return r1;
                    }
                }
                debug_assert!(cur.use_y_tab());
                let p_tab = table_of(cur, own).cloned().expect("pTab");
                return expr_code_get_column(db, parse, &p_tab, cur.i_column, i_tab, target, cur.op2);
            }
            TK_INTEGER => {
                code_integer(db, parse, cur, false, target);
                return target;
            }
            TK_TRUEFALSE => {
                add_op2(vdbe_of_parse(parse), OP_INTEGER as i32, expr_truth_value(cur), target);
                return target;
            }
            TK_FLOAT => {
                debug_assert!(!cur.has_property(EP_INT_VALUE));
                code_real(vdbe_of_parse(parse), cur.z_token(), false, target);
                return target;
            }
            TK_STRING => {
                debug_assert!(!cur.has_property(EP_INT_VALUE));
                load_string(vdbe_of_parse(parse), target, cur.z_token().unwrap_or(&[]));
                return target;
            }
            TK_BLOB => {
                debug_assert!(!cur.has_property(EP_INT_VALUE));
                let tok = cur.z_token().unwrap_or(&[]);
                debug_assert!(at(tok, 0) == b'x' || at(tok, 0) == b'X');
                debug_assert!(at(tok, 1) == b'\'');
                let z = if tok.len() > 2 { &tok[2..] } else { &[][..] };
                let n = strlen30(z) - 1;
                debug_assert!(at(z, n as usize) == b'\'');
                let z_blob = hex_to_blob(z, n);
                add_op4(vdbe_of_parse(parse), OP_BLOB as i32, n / 2, target, 0, P4::Blob(z_blob));
                return target;
            }
            TK_VARIABLE => {
                debug_assert!(!cur.has_property(EP_INT_VALUE));
                debug_assert!(cur.z_token().map_or(false, |z| at(z, 0) != 0));
                add_op2(vdbe_of_parse(parse), OP_VARIABLE as i32, cur.i_column, target);
                return target;
            }
            TK_REGISTER => {
                return cur.i_table;
            }
            TK_CAST => {
                // Expressões da forma CAST(pLeft AS token).
                if let Some(l) = cur.p_left.as_deref_mut() {
                    expr_code(db, parse, l, target, own);
                }
                debug_assert!(in_reg == target);
                debug_assert!(!cur.has_property(EP_INT_VALUE));
                let aff = affinity_type(cur.z_token().unwrap_or(&[]), None);
                add_op2(vdbe_of_parse(parse), OP_CAST as i32, target, aff as i32);
                return in_reg;
            }
            TK_IS | TK_ISNOT | TK_LT | TK_LE | TK_GT | TK_GE | TK_NE | TK_EQ => {
                if op == TK_IS || op == TK_ISNOT {
                    op = if op == TK_IS { TK_EQ } else { TK_NE };
                    p5 = SQLITE_NULLEQ as u8;
                }
                let is_vector = cur.p_left.as_deref().map_or(false, expr_is_vector);
                if is_vector {
                    code_vector_compare(db, parse, cur, target, op, p5, own);
                } else {
                    let commuted = cur.has_property(EP_COMMUTED);
                    let (l, r) = (cur.p_left.as_deref_mut(), cur.p_right.as_deref_mut());
                    if let (Some(l), Some(r)) = (l, r) {
                        let r1 = expr_code_temp(db, parse, l, &mut reg_free1, own);
                        let r2 = expr_code_temp(db, parse, r, &mut reg_free2, own);
                        add_op2(vdbe_of_parse(parse), OP_INTEGER as i32, 1, in_reg);
                        let dest = current_addr(parse) + 2;
                        code_compare(db, parse, l, r, op, r1, r2, dest, p5 as i32, commuted as i32, own);
                        if p5 == SQLITE_NULLEQ as u8 {
                            add_op2(vdbe_of_parse(parse), OP_INTEGER as i32, 0, in_reg);
                        } else {
                            add_op3(vdbe_of_parse(parse), OP_ZEROORNULL as i32, r1, in_reg, r2);
                        }
                    }
                }
            }
            TK_AND | TK_OR | TK_PLUS | TK_STAR | TK_MINUS | TK_REM | TK_BITAND | TK_BITOR
            | TK_SLASH | TK_LSHIFT | TK_RSHIFT | TK_CONCAT => {
                // TK_x == OP_x para todos estes operadores.
                if let (Some(l), Some(r)) = (cur.p_left.as_deref_mut(), cur.p_right.as_deref_mut()) {
                    let r1 = expr_code_temp(db, parse, l, &mut reg_free1, own);
                    let r2 = expr_code_temp(db, parse, r, &mut reg_free2, own);
                    add_op3(vdbe_of_parse(parse), op as i32, r2, r1, target);
                }
            }
            TK_UMINUS => {
                let Some(l) = cur.p_left.as_deref_mut() else {
                    debug_assert!(false);
                    break;
                };
                if l.op == TK_INTEGER {
                    code_integer(db, parse, l, true, target);
                    return target;
                } else if l.op == TK_FLOAT {
                    debug_assert!(!l.has_property(EP_INT_VALUE));
                    code_real(vdbe_of_parse(parse), l.z_token(), true, target);
                    return target;
                } else {
                    let mut temp_x = Expr::default();
                    temp_x.op = TK_INTEGER;
                    temp_x.flags = EP_INT_VALUE | EP_TOKEN_ONLY;
                    temp_x.u = ExprU::IValue(0);
                    let r1 = expr_code_temp(db, parse, &mut temp_x, &mut reg_free1, own);
                    let r2 = expr_code_temp(db, parse, l, &mut reg_free2, own);
                    add_op3(vdbe_of_parse(parse), OP_SUBTRACT as i32, r2, r1, target);
                }
            }
            TK_BITNOT | TK_NOT => {
                // TK_BITNOT == OP_BitNot e TK_NOT == OP_Not.
                if let Some(l) = cur.p_left.as_deref_mut() {
                    let r1 = expr_code_temp(db, parse, l, &mut reg_free1, own);
                    add_op2(vdbe_of_parse(parse), op as i32, r1, in_reg);
                }
            }
            TK_TRUTH => {
                let r1 = match cur.p_left.as_deref_mut() {
                    Some(l) => expr_code_temp(db, parse, l, &mut reg_free1, own),
                    None => 0,
                };
                let is_true = cur.p_right.as_deref().map_or(0, expr_truth_value);
                let b_normal = (cur.op2 == TK_IS) as i32;
                add_op4_int(
                    vdbe_of_parse(parse),
                    OP_ISTRUE as i32,
                    r1,
                    in_reg,
                    (is_true == 0) as i32,
                    is_true ^ b_normal,
                );
            }
            TK_ISNULL | TK_NOTNULL => {
                // TK_ISNULL == OP_IsNull e TK_NOTNULL == OP_NotNull.
                add_op2(vdbe_of_parse(parse), OP_INTEGER as i32, 1, target);
                let r1 = match cur.p_left.as_deref_mut() {
                    Some(l) => expr_code_temp(db, parse, l, &mut reg_free1, own),
                    None => 0,
                };
                let v = vdbe_of_parse(parse);
                let addr = add_op1(v, op as i32, r1);
                add_op2(v, OP_INTEGER as i32, 0, target);
                jump_here(v, addr);
            }
            TK_AGG_FUNCTION => {
                let reg = agg_info_of(parse, cur.p_agg_info).and_then(|info| {
                    if cur.i_agg < 0 || cur.i_agg as usize >= info.a_func.len() {
                        None
                    } else {
                        Some(info.func_reg(cur.i_agg as i32))
                    }
                });
                match reg {
                    Some(r) => return r,
                    None => {
                        debug_assert!(!cur.has_property(EP_INT_VALUE));
                        error_msg(db, parse, b"misuse of aggregate: %#T()", &[expr_token_arg(cur)]);
                    }
                }
            }
            TK_FUNCTION => {
                let enc = db.enc;
                if cur.has_property(EP_WIN_FUNC) {
                    return cur.y_win().map_or(0, |w| w.reg_result);
                }
                if parse.ok_const_factor != 0
                    && expr_is_constant_not_join(Some((&mut *db, &mut *parse)), Some(&mut *cur)) != 0
                {
                    // Funções SQL podem ser caras: evita rodá-las várias vezes se o resultado é
                    // sempre o mesmo.
                    return expr_code_run_just_once(db, parse, cur, -1, own);
                }
                debug_assert!(!cur.has_property(EP_TOKEN_ONLY));
                debug_assert!(cur.use_x_list());
                let n_farg = cur.x_list().map_or(0, |l| l.a.len()) as i32;
                debug_assert!(!cur.has_property(EP_INT_VALUE));
                let z_id = cur.z_token().unwrap_or(&[]).to_vec();
                let p_def = find_function(db, &z_id, n_farg, enc, 0);
                let p_def = match p_def {
                    Some(d) if d.x_finalize.is_none() => d,
                    _ => {
                        error_msg(db, parse, b"unknown function: %#T()", &[expr_token_arg(cur)]);
                        break;
                    }
                };
                let has_farg = cur.x_list().is_some();
                if (p_def.func_flags & SQLITE_FUNC_INLINE) != 0 && has_farg {
                    debug_assert!((p_def.func_flags & SQLITE_FUNC_UNSAFE) == 0);
                    debug_assert!((p_def.func_flags & SQLITE_FUNC_DIRECT) == 0);
                    let id = match p_def.p_user_data {
                        UserData::Int(i) => i as i32,
                        _ => 0,
                    };
                    let p_farg = cur.x_list_mut().expect("pFarg");
                    return expr_code_inline_function(db, parse, p_farg, id, target, own);
                } else if (p_def.func_flags & (SQLITE_FUNC_DIRECT | SQLITE_FUNC_UNSAFE)) != 0 {
                    expr_function_usable(db, parse, cur, &p_def);
                }

                let mut const_mask: u32 = 0;
                let mut p_coll: Option<Rc<CollSeq>> = None;
                for i in 0..n_farg as usize {
                    let is_const = {
                        let e = cur
                            .x_list_mut()
                            .and_then(|l| l.a.get_mut(i))
                            .and_then(|it| it.p_expr.as_deref_mut());
                        e.map_or(false, |e| expr_is_constant(Some((&mut *db, &mut *parse)), Some(e)) != 0)
                    };
                    if i < 32 && is_const {
                        const_mask |= 1u32 << i;
                    }
                    if (p_def.func_flags & SQLITE_FUNC_NEEDCOLL) != 0 && p_coll.is_none() {
                        let e = cur.x_list().and_then(|l| l.a.get(i)).and_then(|it| it.p_expr.as_deref());
                        p_coll = expr_coll_seq(db, parse, e, own);
                    }
                }
                let r1;
                if has_farg {
                    if const_mask != 0 {
                        r1 = parse.n_mem + 1;
                        parse.n_mem += n_farg;
                    } else {
                        r1 = get_temp_range(parse, n_farg);
                    }

                    // Para length(), typeof() e octet_length() o P5 do OP_Column vira
                    // OPFLAG_LENGTHARG, OPFLAG_TYPEOFARG ou OPFLAG_BYTELENARG, evitando carga de
                    // dados desnecessária.
                    if (p_def.func_flags & (SQLITE_FUNC_LENGTH | SQLITE_FUNC_TYPEOF)) != 0 {
                        debug_assert!(n_farg == 1);
                        if let Some(a0) = cur
                            .x_list_mut()
                            .and_then(|l| l.a.get_mut(0))
                            .and_then(|it| it.p_expr.as_deref_mut())
                        {
                            if a0.op == TK_COLUMN || a0.op == TK_AGG_COLUMN {
                                a0.op2 = (p_def.func_flags & OPFLAG_BYTELENARG as u32) as u8;
                            }
                        }
                    }

                    if let Some(l) = cur.x_list_mut() {
                        expr_code_expr_list(db, parse, l, r1, 0, SQLITE_ECEL_FACTOR, own);
                    }
                } else {
                    r1 = 0;
                }
                // Possível sobrecarga da função se o primeiro argumento é coluna de tabela
                // virtual. Nas funções infixas (LIKE, GLOB, REGEXP, MATCH) vale o segundo.
                let mut p_def = p_def;
                {
                    let arg_for_vtab = if n_farg >= 2 && cur.has_property(EP_INFIX_FUNC) {
                        cur.x_list().and_then(|l| l.a.get(1)).and_then(|it| it.p_expr.as_deref())
                    } else if n_farg > 0 {
                        cur.x_list().and_then(|l| l.a.first()).and_then(|it| it.p_expr.as_deref())
                    } else {
                        None
                    };
                    if n_farg > 0 {
                        p_def = vtab_overload_function(db, p_def, n_farg, arg_for_vtab, own);
                    }
                }
                if (p_def.func_flags & SQLITE_FUNC_NEEDCOLL) != 0 {
                    if p_coll.is_none() {
                        p_coll = db.p_dflt_coll.clone();
                    }
                    add_op4(vdbe_of_parse(parse), OP_COLLSEQ as i32, 0, 0, 0, P4::Coll(p_coll));
                }
                add_function_call(parse, const_mask as i32, r1, target, n_farg, &p_def, cur.op2 as i32);
                if n_farg != 0 && const_mask == 0 {
                    // Com `const_mask` não nulo o C chama `sqlite3VdbeReleaseRegisters`, que é
                    // macro vazia fora de SQLITE_DEBUG.
                    release_temp_range(parse, r1, n_farg);
                }
                return target;
            }
            TK_EXISTS | TK_SELECT => {
                if db.malloc_failed != 0 {
                    return 0;
                }
                let n_col = cur
                    .x_select()
                    .and_then(|s| s.p_e_list.as_deref())
                    .map_or(0, |l| l.a.len() as i32);
                if op == TK_SELECT && cur.use_x_select() && n_col != 1 {
                    subselect_error(db, parse, n_col, 1);
                } else {
                    return code_subselect(db, parse, cur);
                }
            }
            TK_SELECT_COLUMN => {
                let within = parse.within_rj_subrtn;
                let i_table = cur.i_table;
                let i_column = cur.i_column;
                let Some(l) = cur.p_left.as_deref_mut() else {
                    debug_assert!(false);
                    break;
                };
                if l.i_table == 0 || within > l.op2 {
                    l.i_table = code_subselect(db, parse, l);
                    l.op2 = within;
                }
                debug_assert!(l.op == TK_SELECT || l.op == TK_ERROR);
                let n = expr_vector_size(l);
                if i_table != n {
                    error_msg(
                        db,
                        parse,
                        b"%d columns assigned %d values",
                        &[PrintfArg::Int(i_table as i64), PrintfArg::Int(n as i64)],
                    );
                }
                return l.i_table + i_column;
            }
            TK_IN => {
                let dest_if_false = make_label(parse);
                let dest_if_null = make_label(parse);
                add_op2(vdbe_of_parse(parse), OP_NULL as i32, 0, target);
                expr_code_in(db, parse, cur, dest_if_false, dest_if_null, own);
                add_op2(vdbe_of_parse(parse), OP_INTEGER as i32, 1, target);
                resolve_label(parse, db, dest_if_false);
                add_op2(vdbe_of_parse(parse), OP_ADDIMM as i32, target, 0);
                resolve_label(parse, db, dest_if_null);
                return target;
            }
            // x BETWEEN y AND z equivale a x>=y AND x<=z; X em pLeft, Y e Z em x.pList.
            TK_BETWEEN => {
                expr_code_between(db, parse, cur, target, 0, 0, own);
                return target;
            }
            TK_COLLATE => {
                if !cur.has_property(EP_COLLATE) {
                    // "SOFT-COLLATE" acrescentado pela otimização de push-down do WHERE:
                    // limpa os subtipos, que não atravessam a fronteira de uma subconsulta.
                    debug_assert!(cur.p_left.is_some());
                    if let Some(l) = cur.p_left.as_deref_mut() {
                        expr_code(db, parse, l, target, own);
                    }
                    add_op1(vdbe_of_parse(parse), OP_CLRSUBTYPE as i32, target);
                    return target;
                } else {
                    match cur.p_left.is_some() {
                        true => {
                            cur = cur.p_left.as_deref_mut().expect("pLeft");
                            continue; // expr_code_doover
                        }
                        false => {
                            add_op2(vdbe_of_parse(parse), OP_NULL as i32, 0, target);
                            return target;
                        }
                    }
                }
            }
            TK_SPAN | TK_UPLUS => {
                if cur.p_left.is_some() {
                    cur = cur.p_left.as_deref_mut().expect("pLeft");
                    continue; // expr_code_doover
                }
                add_op2(vdbe_of_parse(parse), OP_NULL as i32, 0, target);
                return target;
            }
            TK_TRIGGER => {
                // Referência a coluna das pseudotabelas new.* ou old.* de um gatilho
                // (iTable é 1 para new e 0 para old; iColumn é a coluna, ou -1 para o rowid).
                // Implementada com OP_Param: p1 é 0 para old.rowid, i+1 para old.<col i>,
                // n+1 para new.rowid e n+2+i para new.<col i>.
                debug_assert!(cur.use_y_tab());
                let p_tab = table_of(cur, own).cloned().expect("pTab");
                let i_col = cur.i_column;
                let p1 = cur.i_table * (p_tab.n_col as i32 + 1)
                    + 1
                    + table_column_to_storage(&p_tab, i_col as i16) as i32;
                debug_assert!(cur.i_table == 0 || cur.i_table == 1);
                debug_assert!(i_col >= -1 && i_col < p_tab.n_col as i32);
                debug_assert!(p_tab.i_p_key < 0 || i_col != p_tab.i_p_key as i32);
                debug_assert!(p1 >= 0 && p1 < p_tab.n_col as i32 * 2 + 2);

                let v = vdbe_of_parse(parse);
                add_op2(v, OP_PARAM as i32, p1, target);
                let col_name = if i_col < 0 {
                    b"rowid".to_vec()
                } else {
                    let n = &p_tab.a_col[i_col as usize].z_cn_name;
                    n[..strlen30(n) as usize].to_vec()
                };
                vdbe_comment(
                    v,
                    b"r[%d]=%s.%s",
                    &[
                        PrintfArg::Int(target as i64),
                        PrintfArg::Text(Some(if cur.i_table != 0 { b"new".to_vec() } else { b"old".to_vec() })),
                        PrintfArg::Text(Some(col_name)),
                    ],
                );

                // Coluna REAL pode estar guardada como inteiro: OP_RealAffinity garante que
                // seja real de fato (EVIDENCE-OF: R-60985-57662).
                if i_col >= 0 && p_tab.a_col[i_col as usize].affinity == SQLITE_AFF_REAL {
                    add_op1(v, OP_REALAFFINITY as i32, target);
                }
            }
            TK_VECTOR => {
                error_msg(db, parse, b"row value misused", &[]);
            }
            // TK_IF_NULL_ROW é inserido antes de expressões derivadas da tabela à direita de um
            // LEFT JOIN; só é avaliado se a tabela não estiver numa linha NULL do LEFT JOIN.
            TK_IF_NULL_ROW => {
                let ok_const_factor = parse.ok_const_factor;
                let mut handled = false;
                if let Some(info) = agg_info_of(parse, cur.p_agg_info) {
                    debug_assert!(cur.i_agg >= 0 && (cur.i_agg as usize) < info.a_col.len());
                    if info.direct_mode == 0 {
                        in_reg = info.column_reg(cur.i_agg as i32);
                        handled = true;
                    } else if info.use_sorting_idx != 0 {
                        let (ptab, sorter) = (
                            info.sorting_idx_p_tab,
                            info.a_col.get(cur.i_agg as usize).map_or(0, |c| c.i_sorter_column),
                        );
                        add_op3(vdbe_of_parse(parse), OP_COLUMN as i32, ptab, sorter as i32, target);
                        in_reg = target;
                        handled = true;
                    }
                }
                if !handled {
                    let addr_inr = add_op3(vdbe_of_parse(parse), OP_IFNULLROW as i32, cur.i_table, 0, target);
                    // O OP_IfNullRow pode sobrescrever o registrador de resultado com NULL:
                    // (1) desliga a fatoração de constantes e (2) garante que o valor fique em
                    // `target`.
                    parse.ok_const_factor = 0;
                    if let Some(l) = cur.p_left.as_deref_mut() {
                        expr_code(db, parse, l, target, own);
                    }
                    debug_assert!(target == in_reg);
                    parse.ok_const_factor = ok_const_factor;
                    jump_here(vdbe_of_parse(parse), addr_inr);
                }
            }
            // Forma A: CASE x WHEN e1 THEN r1 ... ELSE y END; forma B: CASE WHEN e1 THEN r1 ...
            // A forma A equivale à B com `x=ei`. Y é o último elemento de x.pList se o número de
            // elementos é ímpar (o ELSE é opcional); Ei fica em a[i*2] e Ri em a[i*2+1].
            TK_CASE => {
                debug_assert!(cur.use_x_list() && cur.x_list().is_some());
                let n_expr = cur.x_list().map_or(0, |l| l.a.len());
                debug_assert!(n_expr > 0);
                let end_label = make_label(parse);
                let has_x = cur.p_left.is_some();
                let mut op_compare = Expr::default();
                if has_x {
                    let mut p_del = expr_dup(cur.p_left.as_deref(), 0);
                    if db.malloc_failed != 0 {
                        break;
                    }
                    if let Some(d) = p_del.as_deref_mut() {
                        let reg = expr_code_vector(db, parse, d, &mut reg_free1, own);
                        expr_to_register(d, reg);
                    }
                    op_compare.op = TK_EQ;
                    op_compare.p_left = p_del;
                    // Ticket b351d95f9cd5ef17e9d9dbae18f5ca8611190001: o valor em regFree1 pode
                    // sofrer SCopy para o resultado; o registrador não deve ser reaproveitado.
                    reg_free1 = 0;
                }
                let mut i = 0;
                while i + 1 < n_expr {
                    let next_case = make_label(parse);
                    if has_x {
                        // pTest é X==Ei: o Ei do C é apontado, aqui é movido para o nó de
                        // comparação e devolvido depois.
                        op_compare.p_right = cur
                            .x_list_mut()
                            .and_then(|l| l.a.get_mut(i))
                            .and_then(|it| it.p_expr.take());
                        expr_if_false(db, parse, &mut op_compare, next_case, SQLITE_JUMPIFNULL as i32, own);
                        if let Some(l) = cur.x_list_mut() {
                            l.a[i].p_expr = op_compare.p_right.take();
                        }
                    } else if let Some(t) = cur
                        .x_list_mut()
                        .and_then(|l| l.a.get_mut(i))
                        .and_then(|it| it.p_expr.as_deref_mut())
                    {
                        expr_if_false(db, parse, t, next_case, SQLITE_JUMPIFNULL as i32, own);
                    }
                    if let Some(e) = cur
                        .x_list_mut()
                        .and_then(|l| l.a.get_mut(i + 1))
                        .and_then(|it| it.p_expr.as_deref_mut())
                    {
                        expr_code(db, parse, e, target, own);
                    }
                    vdbe_goto(vdbe_of_parse(parse), end_label);
                    resolve_label(parse, db, next_case);
                    i += 2;
                }
                if (n_expr & 1) != 0 {
                    if let Some(e) = cur
                        .x_list_mut()
                        .and_then(|l| l.a.get_mut(n_expr - 1))
                        .and_then(|it| it.p_expr.as_deref_mut())
                    {
                        expr_code(db, parse, e, target, own);
                    }
                } else {
                    add_op2(vdbe_of_parse(parse), OP_NULL as i32, 0, target);
                }
                drop(op_compare);
                set_do_not_merge_flag_on_copy(vdbe_of_parse(parse));
                resolve_label(parse, db, end_label);
            }
            TK_RAISE => {
                debug_assert!(
                    cur.aff_expr == OE_ROLLBACK
                        || cur.aff_expr == OE_ABORT
                        || cur.aff_expr == OE_FAIL
                        || cur.aff_expr == OE_IGNORE
                );
                if parse.p_trigger_tab.is_none() && parse.nested == 0 {
                    error_msg(db, parse, b"RAISE() may only be used within a trigger-program", &[]);
                    return 0;
                }
                if cur.aff_expr == OE_ABORT {
                    may_abort(parse);
                }
                debug_assert!(!cur.has_property(EP_INT_VALUE));
                let z_token = cur.z_token().map(|z| z[..strlen30(z) as usize].to_vec());
                if cur.aff_expr == OE_IGNORE {
                    add_op4(
                        vdbe_of_parse(parse),
                        OP_HALT as i32,
                        SQLITE_OK,
                        OE_IGNORE as i32,
                        0,
                        z_token.map_or(P4::None, P4::Text),
                    );
                } else {
                    let err = if parse.p_trigger_tab.is_some() { SQLITE_CONSTRAINT_TRIGGER } else { SQLITE_ERROR };
                    halt_constraint(parse, err, cur.aff_expr as i32, z_token.as_deref(), P4_STATIC, 0);
                }
            }
            // Qualquer outro nó (TK_NULL, TK_ERROR): NULL é o caso padrão, para que um nó
            // ilegal seja tratado com sanidade e não derrube o programa.
            _ => {
                debug_assert!(op == TK_NULL || op == TK_ERROR || db.malloc_failed != 0);
                add_op2(vdbe_of_parse(parse), OP_NULL as i32, 0, target);
                return target;
            }
        }
        break;
    }
    release_temp_reg(parse, reg_free1);
    release_temp_reg(parse, reg_free2);
    in_reg
}
