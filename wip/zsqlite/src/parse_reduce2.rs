//! `parse.c`, as ações semânticas do lemon para as regras acima de 200 (`yy_reduce`,
//! `parse_c.010` a `parse_c.012`). As regras de 0 a 200, mais os braços compartilhados 201 a 204,
//! 219, 222, 231 a 234, 237, 242, 246, 247, 251, 252, 258, 259 e 324, ficam em
//! `crate::parse_reduce::yy_reduce`, que chama esta função no braço `_` do `match`.
//!
//! As convenções são as de `parse_reduce.rs` (modelo v2): `yymsp[k].minor.yyNNN` é um acessor
//! `take_*`/`get_*`/`set_*` sobre `yyp.msp(k)`; `Token.z` é uma cópia do texto e `Token.i_ofst` o
//! deslocamento no SQL; `yy168` (`const char*`) é o deslocamento em bytes. Nenhuma ação chama
//! `yy_reduce_goto`: quem fecha a redução é o chamador.
//!
//! Dois pontos onde o C usa um bit ou um ponteiro que o modelo não tem:
//!
//! - `likeop` guarda o `NOT` em `yy0.n |= 0x80000000`. `Token` não tem `n` (é `z.len()`), então o
//!   bit vai em `i_ofst` (`LIKEOP_NOT`, o bit 30). O SQL tem no máximo 1000000000 bytes, menos que
//!   2^30, e o deslocamento de um token vindo do texto nunca é negativo, então o bit nunca colide
//!   com um deslocamento de verdade. A regra 206/207 o limpa antes de usar o token.
//! - Listas ligadas (`TriggerStep.pNext`/`pLast`, `Window.pNextWin`): na pilha do parser o valor é
//!   um único `Option<Box<...>>`, então a cadeia é por `p_next`/`p_next_win` (ver o relatório de
//!   campos novos). O `pLast` do C some: o acréscimo no fim percorre a cadeia.

use crate::alter::{
    alter_begin_add_column, alter_drop_column, alter_finish_add_column, alter_rename_column,
    alter_rename_table,
};
use crate::analyze::analyze;
use crate::attach::{attach, detach};
use crate::build::{create_index, drop_index, reindex, src_list_append, src_list_func_args};
use crate::connection::{Connection, Parse};
use crate::consts::{
    EP_INFIX_FUNC, M10D_ANY, M10D_NO, M10D_YES, OE_ABORT, OE_FAIL, OE_IGNORE, OE_NONE,
    OE_ROLLBACK, SQLITE_IDXTYPE_APPDEF, SQLITE_SO_ASC, TK_BEFORE, TK_BETWEEN, TK_CASE,
    TK_CURRENT, TK_EQ, TK_EXISTS, TK_FILTER, TK_INSTEAD, TK_IN, TK_IS, TK_ISNOT, TK_ISNULL,
    TK_NOT, TK_NOTNULL, TK_PLUS, TK_RAISE, TK_SELECT, TK_STRING, TK_UNBOUNDED, TK_UPDATE,
    TK_UPLUS, TK_VECTOR,
};
use crate::expr::{
    expr, expr_alloc, expr_function, expr_list_append, expr_set_height_and_flags,
    expr_list_to_values, expr_unmap_and_delete, p_expr, p_expr_add_select,
};
use crate::expr_code::{expr_id_to_true_false, expr_is_constant};
use crate::parse_reduce::{
    binary_to_unary_if_null, disable_lookaside, get_i32, get_ofst, get_tok, major_at,
    parser_add_expr_id_list_term, parser_double_link_select, set_expr, set_i32, set_list,
    set_tok, span_token, take_expr, take_id_list, take_list, take_select, take_src, take_upsert,
    take_window, take_with, set_window, set_with,
};
use crate::parse_tables::{FrameBound, TrigEvent, YyMinor, YyParser};
use crate::pragma::pragma;
use crate::select::select_new;
use crate::sqlite_int::{Cte, Expr, ExprX, Token, TriggerStep, Window};
use crate::trigger::{
    begin_trigger, drop_trigger, finish_trigger, trigger_delete_step, trigger_insert_step,
    trigger_select_step, trigger_update_step,
};
use crate::util::{dequote_number, error_msg};
use crate::vacuum::vacuum;
use crate::vtab::{
    vtab_arg_extend, vtab_arg_init, vtab_begin_parse, vtab_finish_parse,
};
use crate::window::{window_alloc, window_assemble, window_chain};
use crate::with::{cte_new, with_add, with_push};

/// O bit que `likeop ::= NOT LIKE_KW|MATCH` põe em `Token.i_ofst` no lugar do
/// `n |= 0x80000000` do C (ver o comentário do módulo).
const LIKEOP_NOT: i32 = 0x4000_0000;

// ---------------------------------------------------------------------------------------------
// Acessores das variantes que só esta fatia usa
// ---------------------------------------------------------------------------------------------

/// `yymsp[k].minor.yy427` movido para fora da pilha (o passo de trigger).
fn take_step(yyp: &mut YyParser, k: isize) -> Option<Box<TriggerStep>> {
    match &mut yyp.msp(k).minor {
        YyMinor::Yy427(v) => v.take(),
        _ => None,
    }
}

/// `yymsp[k].minor.yy427 = v`.
fn set_step(yyp: &mut YyParser, k: isize, v: Option<Box<TriggerStep>>) {
    yyp.msp(k).minor = YyMinor::Yy427(v);
}

/// `yymsp[k].minor.yy67` movido para fora da pilha (a CTE).
fn take_cte(yyp: &mut YyParser, k: isize) -> Option<Box<Cte>> {
    match &mut yyp.msp(k).minor {
        YyMinor::Yy67(v) => v.take(),
        _ => None,
    }
}

/// `yymsp[k].minor.yy67 = v`.
fn set_cte(yyp: &mut YyParser, k: isize, v: Option<Box<Cte>>) {
    yyp.msp(k).minor = YyMinor::Yy67(v);
}

/// `yymsp[k].minor.yy462`.
fn get_u8(yyp: &mut YyParser, k: isize) -> u8 {
    match &yyp.msp(k).minor {
        YyMinor::Yy462(v) => *v,
        _ => 0,
    }
}

/// `yymsp[k].minor.yy462 = v`.
fn set_u8(yyp: &mut YyParser, k: isize, v: u8) {
    yyp.msp(k).minor = YyMinor::Yy462(v);
}

/// `yymsp[k].minor.yy509` movido para fora da pilha (o limite de frame).
fn take_bound(yyp: &mut YyParser, k: isize) -> FrameBound {
    match &mut yyp.msp(k).minor {
        YyMinor::Yy509(b) => std::mem::take(b),
        _ => FrameBound::default(),
    }
}

/// `yymsp[k].minor.yy509 = {e_type, p_expr}`.
fn set_bound(yyp: &mut YyParser, k: isize, e_type: i32, p_expr: Option<Box<Expr>>) {
    yyp.msp(k).minor = YyMinor::Yy509(FrameBound { e_type, p_expr });
}

/// `yymsp[k].minor.yy286 = {a, b}`.
fn set_event(yyp: &mut YyParser, k: isize, a: i32, b: Option<Box<crate::sqlite_int::IdList>>) {
    yyp.msp(k).minor = YyMinor::Yy286(TrigEvent { a, b });
}

/// `pLast->pNext = step; pLast = step`: acrescenta `step` ao fim da cadeia que começa em `head`.
fn trigger_step_append(head: &mut TriggerStep, step: Box<TriggerStep>) {
    let mut cur = head;
    while cur.p_next.is_some() {
        cur = cur.p_next.as_deref_mut().unwrap();
    }
    cur.p_next = Some(step);
}

/// `ExprList` com o item `e` acrescentado em seguida ao item `e1` (`ExprListAppend` duas vezes).
fn list2(
    e1: Option<Box<Expr>>,
    e2: Option<Box<Expr>>,
) -> Option<Box<crate::sqlite_int::ExprList>> {
    let l = expr_list_append(None, e1);
    expr_list_append(l, e2)
}

/// `x.pList = list` num nó que existe; sem nó a lista é solta pelo `Drop`.
fn attach_list(node: Option<&mut Expr>, list: Option<Box<crate::sqlite_int::ExprList>>) {
    if let Some(e) = node {
        e.x = match list {
            Some(l) => ExprX::List(l),
            None => ExprX::None,
        };
    }
}

// ---------------------------------------------------------------------------------------------
// yy_reduce, regras acima de 200
// ---------------------------------------------------------------------------------------------

/// Executa a ação semântica da regra `yyruleno` (maior que 200) que `yy_reduce` não trata nos
/// braços compartilhados. Não fecha a redução: o chamador chama `yy_reduce_goto`. Regras sem ação
/// no `parse.y` (as do `default:` do C) não fazem nada.
pub fn yy_reduce_tail(
    yyp: &mut YyParser,
    yyruleno: u32,
    _yy_lookahead: i32,
    _yy_lookahead_token: &Token,
    p_parse: &mut Parse,
    db: &mut Connection,
) {
    match yyruleno {
        // likeop ::= NOT LIKE_KW|MATCH
        205 => {
            let mut t = get_tok(yyp, 0);
            t.i_ofst |= LIKEOP_NOT;
            set_tok(yyp, -1, t);
        }
        // expr ::= expr likeop expr
        206 => {
            let mut t = get_tok(yyp, -1);
            let b_not = (t.i_ofst & LIKEOP_NOT) != 0;
            t.i_ofst &= !LIKEOP_NOT;
            set_tok(yyp, -1, t.clone());
            let right = take_expr(yyp, 0);
            let left = take_expr(yyp, -2);
            let list = list2(right, left);
            let mut r = expr_function(db, p_parse, list, &t, 0);
            if b_not {
                r = p_expr(db, p_parse, TK_NOT as i32, r, None);
            }
            if let Some(e) = r.as_deref_mut() {
                e.flags |= EP_INFIX_FUNC;
            }
            set_expr(yyp, -2, r);
        }
        // expr ::= expr likeop expr ESCAPE expr
        207 => {
            let mut t = get_tok(yyp, -3);
            let b_not = (t.i_ofst & LIKEOP_NOT) != 0;
            t.i_ofst &= !LIKEOP_NOT;
            set_tok(yyp, -3, t.clone());
            let escape = take_expr(yyp, 0);
            let pattern = take_expr(yyp, -2);
            let left = take_expr(yyp, -4);
            let mut list = expr_list_append(None, pattern);
            list = expr_list_append(list, left);
            list = expr_list_append(list, escape);
            let mut r = expr_function(db, p_parse, list, &t, 0);
            if b_not {
                r = p_expr(db, p_parse, TK_NOT as i32, r, None);
            }
            if let Some(e) = r.as_deref_mut() {
                e.flags |= EP_INFIX_FUNC;
            }
            set_expr(yyp, -4, r);
        }
        // expr ::= expr ISNULL|NOTNULL
        208 => {
            let op = major_at(yyp, 0);
            let l = take_expr(yyp, -1);
            let r = p_expr(db, p_parse, op, l, None);
            set_expr(yyp, -1, r);
        }
        // expr ::= expr NOT NULL
        209 => {
            let l = take_expr(yyp, -2);
            let r = p_expr(db, p_parse, TK_NOTNULL as i32, l, None);
            set_expr(yyp, -2, r);
        }
        // expr ::= expr IS expr
        210 => {
            let l = take_expr(yyp, -2);
            let y = take_expr(yyp, 0);
            let mut r = p_expr(db, p_parse, TK_IS as i32, l, y);
            binary_to_unary_if_null(p_parse, r.as_deref_mut(), TK_ISNULL);
            set_expr(yyp, -2, r);
        }
        // expr ::= expr IS NOT expr
        211 => {
            let l = take_expr(yyp, -3);
            let y = take_expr(yyp, 0);
            let mut r = p_expr(db, p_parse, TK_ISNOT as i32, l, y);
            binary_to_unary_if_null(p_parse, r.as_deref_mut(), TK_NOTNULL);
            set_expr(yyp, -3, r);
        }
        // expr ::= expr IS NOT DISTINCT FROM expr
        212 => {
            let l = take_expr(yyp, -5);
            let y = take_expr(yyp, 0);
            let mut r = p_expr(db, p_parse, TK_IS as i32, l, y);
            binary_to_unary_if_null(p_parse, r.as_deref_mut(), TK_ISNULL);
            set_expr(yyp, -5, r);
        }
        // expr ::= expr IS DISTINCT FROM expr
        213 => {
            let l = take_expr(yyp, -4);
            let y = take_expr(yyp, 0);
            let mut r = p_expr(db, p_parse, TK_ISNOT as i32, l, y);
            binary_to_unary_if_null(p_parse, r.as_deref_mut(), TK_NOTNULL);
            set_expr(yyp, -4, r);
        }
        // expr ::= NOT expr; expr ::= BITNOT expr
        214 | 215 => {
            let op = major_at(yyp, -1);
            let e = take_expr(yyp, 0);
            let r = p_expr(db, p_parse, op, e, None);
            set_expr(yyp, -1, r);
        }
        // expr ::= PLUS|MINUS expr
        216 => {
            let p = take_expr(yyp, 0);
            let op = (major_at(yyp, -1) + (TK_UPLUS as i32 - TK_PLUS as i32)) as u8;
            match p {
                Some(mut b) if b.op == TK_UPLUS => {
                    b.op = op;
                    set_expr(yyp, -1, Some(b));
                }
                other => {
                    let r = p_expr(db, p_parse, op as i32, other, None);
                    set_expr(yyp, -1, r);
                }
            }
        }
        // expr ::= expr PTR expr
        217 => {
            let left = take_expr(yyp, -2);
            let right = take_expr(yyp, 0);
            let list = list2(left, right);
            let t = get_tok(yyp, -1);
            let r = expr_function(db, p_parse, list, &t, 0);
            set_expr(yyp, -2, r);
        }
        // between_op ::= BETWEEN; in_op ::= IN
        218 | 221 => set_i32(yyp, 0, 0),
        // expr ::= expr between_op expr AND expr
        220 => {
            let hi = take_expr(yyp, 0);
            let lo = take_expr(yyp, -2);
            let list = list2(lo, hi);
            let left = take_expr(yyp, -4);
            let mut r = p_expr(db, p_parse, TK_BETWEEN as i32, left, None);
            attach_list(r.as_deref_mut(), list);
            if get_i32(yyp, -3) != 0 {
                r = p_expr(db, p_parse, TK_NOT as i32, r, None);
            }
            set_expr(yyp, -4, r);
        }
        // expr ::= expr in_op LP exprlist RP
        223 => {
            let list = take_list(yyp, -1);
            let left = take_expr(yyp, -4);
            let negate = get_i32(yyp, -3) != 0;
            let r: Option<Box<Expr>>;
            match list {
                None => {
                    // `expr1 IN ()` e `expr1 NOT IN ()` simplificam para as constantes 0 (falso)
                    // e 1 (verdadeiro), qualquer que seja `expr1`.
                    expr_unmap_and_delete(p_parse, left);
                    let z: &[u8] = if negate { b"true" } else { b"false" };
                    let mut c = expr(TK_STRING as i32, Some(z));
                    if let Some(e) = c.as_deref_mut() {
                        expr_id_to_true_false(e);
                    }
                    r = c;
                }
                Some(mut list) => {
                    let n_expr = list.a.len();
                    let left_is_vector =
                        left.as_deref().map_or(false, |l| l.op == TK_VECTOR);
                    let rhs_is_constant = n_expr == 1
                        && expr_is_constant(Some((&mut *db, &mut *p_parse)), list.a[0].p_expr.as_deref_mut())
                            != 0
                        && !left_is_vector;
                    let rhs_is_select = n_expr == 1
                        && list.a[0].p_expr.as_deref().map_or(false, |e| e.op == TK_SELECT);
                    let mut out: Option<Box<Expr>>;
                    if rhs_is_constant {
                        let rhs = list.a[0].p_expr.take();
                        drop(list);
                        let rhs = p_expr(db, p_parse, TK_UPLUS as i32, rhs, None);
                        out = p_expr(db, p_parse, TK_EQ as i32, left, rhs);
                    } else if rhs_is_select {
                        let mut rhs = list.a[0].p_expr.take();
                        out = p_expr(db, p_parse, TK_IN as i32, left, None);
                        let sel = rhs.as_deref_mut().and_then(|x| {
                            match std::mem::take(&mut x.x) {
                                ExprX::Select(s) => Some(s),
                                _ => None,
                            }
                        });
                        p_expr_add_select(db, p_parse, out.as_deref_mut(), sel);
                        drop(rhs);
                        drop(list);
                    } else {
                        out = p_expr(db, p_parse, TK_IN as i32, left, None);
                        match out.as_deref_mut() {
                            None => drop(list),
                            Some(node) => {
                                let vec_n = node
                                    .p_left
                                    .as_deref()
                                    .filter(|l| l.op == TK_VECTOR)
                                    .map(|l| l.x_list().map_or(0, |x| x.a.len() as i32));
                                match vec_n {
                                    Some(n) => {
                                        let rhs_sel = expr_list_to_values(db, p_parse, n, list);
                                        if let Some(mut s) = rhs_sel {
                                            parser_double_link_select(db, p_parse, &mut s);
                                            p_expr_add_select(db, p_parse, Some(node), Some(s));
                                        }
                                    }
                                    None => {
                                        node.x = ExprX::List(list);
                                        expr_set_height_and_flags(db, p_parse, node);
                                    }
                                }
                            }
                        }
                    }
                    if negate {
                        out = p_expr(db, p_parse, TK_NOT as i32, out, None);
                    }
                    r = out;
                }
            }
            set_expr(yyp, -4, r);
        }
        // expr ::= LP select RP
        224 => {
            let mut r = p_expr(db, p_parse, TK_SELECT as i32, None, None);
            let sel = take_select(yyp, -1);
            p_expr_add_select(db, p_parse, r.as_deref_mut(), sel);
            set_expr(yyp, -2, r);
        }
        // expr ::= expr in_op LP select RP
        225 => {
            let left = take_expr(yyp, -4);
            let mut r = p_expr(db, p_parse, TK_IN as i32, left, None);
            let sel = take_select(yyp, -1);
            p_expr_add_select(db, p_parse, r.as_deref_mut(), sel);
            if get_i32(yyp, -3) != 0 {
                r = p_expr(db, p_parse, TK_NOT as i32, r, None);
            }
            set_expr(yyp, -4, r);
        }
        // expr ::= expr in_op nm dbnm paren_exprlist
        226 => {
            let nm = get_tok(yyp, -2);
            let dbnm = get_tok(yyp, -1);
            let src = src_list_append(db, p_parse, None, Some(&nm), Some(&dbnm));
            let mut sel = select_new(p_parse, None, src, None, None, None, None, 0, None);
            let args = take_list(yyp, 0);
            if args.is_some() {
                // `pSelect ? pSrc : 0`: sem SELECT o FROM já foi solto e a lista é descartada.
                let src_ref = sel.as_deref_mut().and_then(|s| s.p_src.as_deref_mut());
                src_list_func_args(db, p_parse, src_ref, args);
            }
            let left = take_expr(yyp, -4);
            let mut r = p_expr(db, p_parse, TK_IN as i32, left, None);
            p_expr_add_select(db, p_parse, r.as_deref_mut(), sel);
            if get_i32(yyp, -3) != 0 {
                r = p_expr(db, p_parse, TK_NOT as i32, r, None);
            }
            set_expr(yyp, -4, r);
        }
        // expr ::= EXISTS LP select RP
        227 => {
            let mut r = p_expr(db, p_parse, TK_EXISTS as i32, None, None);
            let sel = take_select(yyp, -1);
            p_expr_add_select(db, p_parse, r.as_deref_mut(), sel);
            set_expr(yyp, -3, r);
        }
        // expr ::= CASE case_operand case_exprlist case_else END
        228 => {
            let case_else = take_expr(yyp, -1);
            let list = take_list(yyp, -2);
            let operand = take_expr(yyp, -3);
            let mut r = p_expr(db, p_parse, TK_CASE as i32, operand, None);
            if let Some(e) = r.as_deref_mut() {
                let l = if case_else.is_some() { expr_list_append(list, case_else) } else { list };
                attach_list(Some(&mut *e), l);
                expr_set_height_and_flags(db, p_parse, e);
            }
            set_expr(yyp, -4, r);
        }
        // case_exprlist ::= case_exprlist WHEN expr THEN expr
        229 => {
            let prior = take_list(yyp, -4);
            let when = take_expr(yyp, -2);
            let then = take_expr(yyp, 0);
            let mut l = expr_list_append(prior, when);
            l = expr_list_append(l, then);
            set_list(yyp, -4, l);
        }
        // case_exprlist ::= WHEN expr THEN expr
        230 => {
            let when = take_expr(yyp, -2);
            let then = take_expr(yyp, 0);
            let l = list2(when, then);
            set_list(yyp, -3, l);
        }
        // nexprlist ::= nexprlist COMMA expr
        235 => {
            let prior = take_list(yyp, -2);
            let e = take_expr(yyp, 0);
            let l = expr_list_append(prior, e);
            set_list(yyp, -2, l);
        }
        // nexprlist ::= expr
        236 => {
            let e = take_expr(yyp, 0);
            let l = expr_list_append(None, e);
            set_list(yyp, 0, l);
        }
        // paren_exprlist ::= LP exprlist RP; eidlist_opt ::= LP eidlist RP
        238 | 243 => {
            let l = take_list(yyp, -1);
            set_list(yyp, -2, l);
        }
        // cmd ::= createkw uniqueflag INDEX ifnotexists nm dbnm ON nm LP sortlist RP where_opt
        239 => {
            let name1 = get_tok(yyp, -7);
            let name2 = get_tok(yyp, -6);
            let tbl = get_tok(yyp, -4);
            let src = src_list_append(db, p_parse, None, Some(&tbl), None);
            let list = take_list(yyp, -2);
            let on_error = get_i32(yyp, -10);
            let start = get_tok(yyp, -11);
            let where_ = take_expr(yyp, 0);
            let if_not_exist = get_i32(yyp, -8);
            create_index(
                db,
                p_parse,
                Some(&name1),
                Some(&name2),
                src,
                list,
                on_error,
                Some(&start),
                where_,
                SQLITE_SO_ASC,
                if_not_exist,
                SQLITE_IDXTYPE_APPDEF,
            );
            if p_parse.in_rename_object() {
                let addr = p_parse.p_new_index.last().map(|i| i.z_name.as_ptr() as usize);
                if let Some(addr) = addr {
                    crate::alter::rename_token_map(p_parse, addr, &tbl);
                }
            }
        }
        // uniqueflag ::= UNIQUE; raisetype ::= ABORT
        240 | 282 => set_i32(yyp, 0, OE_ABORT as i32),
        // uniqueflag ::=
        241 => set_i32(yyp, 1, OE_NONE as i32),
        // eidlist ::= eidlist COMMA nm collate sortorder
        244 => {
            let prior = take_list(yyp, -4);
            let nm = get_tok(yyp, -2);
            let has_collate = get_i32(yyp, -1);
            let sort_order = get_i32(yyp, 0);
            let r = parser_add_expr_id_list_term(db, p_parse, prior, &nm, has_collate, sort_order);
            set_list(yyp, -4, r);
        }
        // eidlist ::= nm collate sortorder
        245 => {
            let nm = get_tok(yyp, -2);
            let has_collate = get_i32(yyp, -1);
            let sort_order = get_i32(yyp, 0);
            let r = parser_add_expr_id_list_term(db, p_parse, None, &nm, has_collate, sort_order);
            set_list(yyp, -2, r);
        }
        // cmd ::= DROP INDEX ifexists fullname
        248 => {
            let name = take_src(yyp, 0);
            let no_err = get_i32(yyp, -1);
            if let Some(name) = name {
                drop_index(db, p_parse, name, no_err);
            }
        }
        // cmd ::= VACUUM vinto
        249 => {
            let into = take_expr(yyp, 0);
            vacuum(db, p_parse, None, into);
        }
        // cmd ::= VACUUM nm vinto
        250 => {
            let nm = get_tok(yyp, -1);
            let into = take_expr(yyp, 0);
            vacuum(db, p_parse, Some(&nm), into);
        }
        // cmd ::= PRAGMA nm dbnm
        253 => {
            let nm = get_tok(yyp, -1);
            let dbnm = get_tok(yyp, 0);
            pragma(db, p_parse, &nm, &dbnm, None, 0);
        }
        // cmd ::= PRAGMA nm dbnm EQ nmnum
        254 => {
            let nm = get_tok(yyp, -3);
            let dbnm = get_tok(yyp, -2);
            let v = get_tok(yyp, 0);
            pragma(db, p_parse, &nm, &dbnm, Some(&v), 0);
        }
        // cmd ::= PRAGMA nm dbnm LP nmnum RP
        255 => {
            let nm = get_tok(yyp, -4);
            let dbnm = get_tok(yyp, -3);
            let v = get_tok(yyp, -1);
            pragma(db, p_parse, &nm, &dbnm, Some(&v), 0);
        }
        // cmd ::= PRAGMA nm dbnm EQ minus_num
        256 => {
            let nm = get_tok(yyp, -3);
            let dbnm = get_tok(yyp, -2);
            let v = get_tok(yyp, 0);
            pragma(db, p_parse, &nm, &dbnm, Some(&v), 1);
        }
        // cmd ::= PRAGMA nm dbnm LP minus_num RP
        257 => {
            let nm = get_tok(yyp, -4);
            let dbnm = get_tok(yyp, -3);
            let v = get_tok(yyp, -1);
            pragma(db, p_parse, &nm, &dbnm, Some(&v), 1);
        }
        // cmd ::= createkw trigger_decl BEGIN trigger_cmd_list END
        260 => {
            // `all.z = yymsp[-3].yy0.z; all.n = (END.z - all.z) + END.n`.
            let first = get_tok(yyp, -3);
            let last = get_tok(yyp, 0);
            let all = span_token(p_parse, &first, &last);
            let steps = take_step(yyp, -1);
            finish_trigger(db, p_parse, steps, &all);
        }
        // trigger_decl ::= temp TRIGGER ifnotexists nm dbnm trigger_time trigger_event ON
        //                  fullname foreach_clause when_clause
        261 => {
            let name1 = get_tok(yyp, -7);
            let name2 = get_tok(yyp, -6);
            let tr_tm = get_i32(yyp, -5);
            let (op, columns) = match &mut yyp.msp(-4).minor {
                YyMinor::Yy286(ev) => (ev.a, ev.b.take()),
                _ => (0, None),
            };
            let tab = take_src(yyp, -2);
            let when = take_expr(yyp, 0);
            let is_temp = get_i32(yyp, -10);
            let no_err = get_i32(yyp, -8);
            begin_trigger(db, p_parse, &name1, &name2, tr_tm, op, columns, tab, when, is_temp, no_err);
            let name = if name2.z.is_empty() { name1 } else { name2 };
            set_tok(yyp, -10, name);
        }
        // trigger_time ::= BEFORE|AFTER
        262 => {
            let m = major_at(yyp, 0);
            set_i32(yyp, 0, m);
        }
        // trigger_time ::= INSTEAD OF
        263 => set_i32(yyp, -1, TK_INSTEAD as i32),
        // trigger_time ::=
        264 => set_i32(yyp, 1, TK_BEFORE as i32),
        // trigger_event ::= DELETE|INSERT; trigger_event ::= UPDATE
        265 | 266 => {
            let m = major_at(yyp, 0);
            set_event(yyp, 0, m, None);
        }
        // trigger_event ::= UPDATE OF idlist
        267 => {
            let cols = take_id_list(yyp, 0);
            set_event(yyp, -2, TK_UPDATE as i32, cols);
        }
        // when_clause ::=; key_opt ::=
        268 | 287 => set_expr(yyp, 1, None),
        // when_clause ::= WHEN expr; key_opt ::= KEY expr
        269 | 288 => {
            let e = take_expr(yyp, 0);
            set_expr(yyp, -1, e);
        }
        // trigger_cmd_list ::= trigger_cmd_list trigger_cmd SEMI
        270 => {
            let step = take_step(yyp, -1);
            let mut head = take_step(yyp, -2);
            debug_assert!(head.is_some());
            if let (Some(h), Some(s)) = (head.as_deref_mut(), step) {
                trigger_step_append(h, s);
            }
            set_step(yyp, -2, head);
        }
        // trigger_cmd_list ::= trigger_cmd SEMI
        271 => {
            // `pLast = pLast`: a cadeia de um só passo já termina nele.
            debug_assert!(matches!(&yyp.msp(-1).minor, YyMinor::Yy427(Some(_))));
        }
        // trnm ::= nm DOT nm
        272 => {
            let t = get_tok(yyp, 0);
            set_tok(yyp, -2, t);
            error_msg(
                db,
                p_parse,
                b"qualified table names are not allowed on INSERT, UPDATE, and DELETE \
                  statements within triggers",
                &[],
            );
        }
        // tridxby ::= INDEXED BY nm
        273 => {
            error_msg(
                db,
                p_parse,
                b"the INDEXED BY clause is not allowed on UPDATE or DELETE statements \
                  within triggers",
                &[],
            );
        }
        // tridxby ::= NOT INDEXED
        274 => {
            error_msg(
                db,
                p_parse,
                b"the NOT INDEXED clause is not allowed on UPDATE or DELETE statements \
                  within triggers",
                &[],
            );
        }
        // trigger_cmd ::= UPDATE orconf trnm tridxby SET setlist from where_opt scanpt
        275 => {
            let target = get_tok(yyp, -6);
            let from = take_src(yyp, -2);
            let set = take_list(yyp, -3);
            let where_ = take_expr(yyp, -1);
            let orconf = get_i32(yyp, -7);
            let z_start = get_tok(yyp, -8).i_ofst;
            let z_end = get_ofst(yyp, 0);
            let r = trigger_update_step(
                db, p_parse, &target, from, set, where_, orconf, z_start, z_end,
            );
            set_step(yyp, -8, r);
        }
        // trigger_cmd ::= scanpt insert_cmd INTO trnm idlist_opt select upsert scanpt
        276 => {
            let target = get_tok(yyp, -4);
            let cols = take_id_list(yyp, -3);
            let sel = take_select(yyp, -2);
            let orconf = get_i32(yyp, -6);
            let up = take_upsert(yyp, -1);
            let z_start = get_ofst(yyp, -7);
            let z_end = get_ofst(yyp, 0);
            let r = trigger_insert_step(
                db, p_parse, &target, cols, sel, orconf, up, z_start, z_end,
            );
            set_step(yyp, -7, r);
        }
        // trigger_cmd ::= DELETE FROM trnm tridxby where_opt scanpt
        277 => {
            let target = get_tok(yyp, -3);
            let where_ = take_expr(yyp, -1);
            let z_start = get_tok(yyp, -5).i_ofst;
            let z_end = get_ofst(yyp, 0);
            let r = trigger_delete_step(db, p_parse, &target, where_, z_start, z_end);
            set_step(yyp, -5, r);
        }
        // trigger_cmd ::= scanpt select scanpt
        278 => {
            let sel = take_select(yyp, -1);
            let z_start = get_ofst(yyp, -2);
            let z_end = get_ofst(yyp, 0);
            let r = trigger_select_step(db, p_parse, sel, z_start, z_end);
            set_step(yyp, -2, r);
        }
        // expr ::= RAISE LP IGNORE RP
        279 => {
            let mut r = p_expr(db, p_parse, TK_RAISE as i32, None, None);
            if let Some(e) = r.as_deref_mut() {
                e.aff_expr = OE_IGNORE;
            }
            set_expr(yyp, -3, r);
        }
        // expr ::= RAISE LP raisetype COMMA nm RP
        280 => {
            let t = get_tok(yyp, -1);
            let mut r = expr_alloc(TK_RAISE as i32, Some(&t), 1);
            let action = get_i32(yyp, -3);
            if let Some(e) = r.as_deref_mut() {
                e.aff_expr = action as u8;
            }
            set_expr(yyp, -5, r);
        }
        // raisetype ::= ROLLBACK
        281 => set_i32(yyp, 0, OE_ROLLBACK as i32),
        // raisetype ::= FAIL
        283 => set_i32(yyp, 0, OE_FAIL as i32),
        // cmd ::= DROP TRIGGER ifexists fullname
        284 => {
            let name = take_src(yyp, 0);
            let no_err = get_i32(yyp, -1);
            drop_trigger(db, p_parse, name, no_err);
        }
        // cmd ::= ATTACH database_kw_opt expr AS expr key_opt
        285 => {
            let file = take_expr(yyp, -3);
            let name = take_expr(yyp, -1);
            let key = take_expr(yyp, 0);
            attach(db, p_parse, file, name, key);
        }
        // cmd ::= DETACH database_kw_opt expr
        286 => {
            let e = take_expr(yyp, 0);
            detach(db, p_parse, e);
        }
        // cmd ::= REINDEX
        289 => reindex(db, p_parse, None, None),
        // cmd ::= REINDEX nm dbnm
        290 => {
            let nm = get_tok(yyp, -1);
            let dbnm = get_tok(yyp, 0);
            reindex(db, p_parse, Some(&nm), Some(&dbnm));
        }
        // cmd ::= ANALYZE
        291 => analyze(db, p_parse, None, None),
        // cmd ::= ANALYZE nm dbnm
        292 => {
            let nm = get_tok(yyp, -1);
            let dbnm = get_tok(yyp, 0);
            analyze(db, p_parse, Some(&nm), Some(&dbnm));
        }
        // cmd ::= ALTER TABLE fullname RENAME TO nm
        293 => {
            let tab = take_src(yyp, -3);
            let new_name = get_tok(yyp, 0);
            alter_rename_table(db, p_parse, tab, &new_name);
        }
        // cmd ::= ALTER TABLE add_column_fullname ADD kwcolumn_opt columnname carglist
        294 => {
            // `yymsp[-1].yy0.n = (pParse->sLastToken.z - yymsp[-1].yy0.z) + sLastToken.n`.
            let first = get_tok(yyp, -1);
            let last = p_parse.s_last_token.clone();
            let all = span_token(p_parse, &first, &last);
            set_tok(yyp, -1, all.clone());
            alter_finish_add_column(db, p_parse, &all);
        }
        // cmd ::= ALTER TABLE fullname DROP kwcolumn_opt nm
        295 => {
            let tab = take_src(yyp, -3);
            let col = get_tok(yyp, 0);
            alter_drop_column(db, p_parse, tab, &col);
        }
        // add_column_fullname ::= fullname
        296 => {
            disable_lookaside(db, p_parse);
            let tab = take_src(yyp, 0);
            alter_begin_add_column(db, p_parse, tab);
        }
        // cmd ::= ALTER TABLE fullname RENAME kwcolumn_opt nm TO nm
        297 => {
            let tab = take_src(yyp, -5);
            let old = get_tok(yyp, -2);
            let new = get_tok(yyp, 0);
            alter_rename_column(db, p_parse, tab, &old, &new);
        }
        // cmd ::= create_vtab
        298 => vtab_finish_parse(db, p_parse, None),
        // cmd ::= create_vtab LP vtabarglist RP
        299 => {
            let rp = get_tok(yyp, 0);
            vtab_finish_parse(db, p_parse, Some(&rp));
        }
        // create_vtab ::= createkw VIRTUAL TABLE ifnotexists nm dbnm USING nm
        300 => {
            let name1 = get_tok(yyp, -3);
            let name2 = get_tok(yyp, -2);
            let module = get_tok(yyp, 0);
            let no_err = get_i32(yyp, -4);
            vtab_begin_parse(db, p_parse, &name1, &name2, &module, no_err);
        }
        // vtabarg ::=
        301 => vtab_arg_init(db, p_parse),
        // vtabargtoken ::= ANY; vtabargtoken ::= lp anylist RP; lp ::= LP
        302 | 303 | 304 => {
            let t = get_tok(yyp, 0);
            vtab_arg_extend(p_parse, &t);
        }
        // with ::= WITH wqlist; with ::= WITH RECURSIVE wqlist
        305 | 306 => {
            let w = take_with(yyp, 0);
            with_push(db, p_parse, w, 1);
        }
        // wqas ::= AS
        307 => set_u8(yyp, 0, M10D_ANY),
        // wqas ::= AS MATERIALIZED
        308 => set_u8(yyp, -1, M10D_YES),
        // wqas ::= AS NOT MATERIALIZED
        309 => set_u8(yyp, -2, M10D_NO),
        // wqitem ::= withnm eidlist_opt wqas LP select RP
        310 => {
            let name = get_tok(yyp, -5);
            let cols = take_list(yyp, -4);
            let sel = take_select(yyp, -1);
            let m10d = get_u8(yyp, -3);
            let r = cte_new(db, p_parse, &name, cols, sel, m10d);
            set_cte(yyp, -5, r);
        }
        // withnm ::= nm
        311 => p_parse.b_has_with = 1,
        // wqlist ::= wqitem
        312 => {
            let cte = take_cte(yyp, 0);
            let r = with_add(db, p_parse, None, cte);
            set_with(yyp, 0, r);
        }
        // wqlist ::= wqlist COMMA wqitem
        313 => {
            let prior = take_with(yyp, -2);
            let cte = take_cte(yyp, 0);
            let r = with_add(db, p_parse, prior, cte);
            set_with(yyp, -2, r);
        }
        // windowdefn_list ::= windowdefn_list COMMA windowdefn
        314 => {
            let mut win = take_window(yyp, 0);
            let list = take_window(yyp, -2);
            debug_assert!(win.is_some());
            if let Some(w) = win.as_deref_mut() {
                window_chain(db, p_parse, w, list.as_deref());
                w.p_next_win = list;
            }
            set_window(yyp, -2, win);
        }
        // windowdefn ::= nm AS LP window RP
        315 => {
            let mut win = take_window(yyp, -1);
            if let Some(w) = win.as_deref_mut() {
                w.z_name = Some(get_tok(yyp, -4).z);
            }
            set_window(yyp, -4, win);
        }
        // window ::= PARTITION BY nexprlist orderby_opt frame_opt
        316 => {
            let frame = take_window(yyp, 0);
            let part = take_list(yyp, -2);
            let order = take_list(yyp, -1);
            let r = window_assemble(db, p_parse, frame, part, order, None);
            set_window(yyp, -4, r);
        }
        // window ::= nm PARTITION BY nexprlist orderby_opt frame_opt
        317 => {
            let frame = take_window(yyp, 0);
            let part = take_list(yyp, -2);
            let order = take_list(yyp, -1);
            let base = get_tok(yyp, -5);
            let r = window_assemble(db, p_parse, frame, part, order, Some(&base));
            set_window(yyp, -5, r);
        }
        // window ::= ORDER BY sortlist frame_opt
        318 => {
            let frame = take_window(yyp, 0);
            let order = take_list(yyp, -1);
            let r = window_assemble(db, p_parse, frame, None, order, None);
            set_window(yyp, -3, r);
        }
        // window ::= nm ORDER BY sortlist frame_opt
        319 => {
            let frame = take_window(yyp, 0);
            let order = take_list(yyp, -1);
            let base = get_tok(yyp, -4);
            let r = window_assemble(db, p_parse, frame, None, order, Some(&base));
            set_window(yyp, -4, r);
        }
        // window ::= nm frame_opt
        320 => {
            let frame = take_window(yyp, 0);
            let base = get_tok(yyp, -1);
            let r = window_assemble(db, p_parse, frame, None, None, Some(&base));
            set_window(yyp, -1, r);
        }
        // frame_opt ::=
        321 => {
            let r = window_alloc(
                db,
                p_parse,
                0,
                TK_UNBOUNDED as i32,
                None,
                TK_CURRENT as i32,
                None,
                0,
            );
            set_window(yyp, 1, r);
        }
        // frame_opt ::= range_or_rows frame_bound_s frame_exclude_opt
        322 => {
            let start = take_bound(yyp, -1);
            let e_type = get_i32(yyp, -2);
            let exclude = get_u8(yyp, 0);
            let r = window_alloc(
                db,
                p_parse,
                e_type,
                start.e_type,
                start.p_expr,
                TK_CURRENT as i32,
                None,
                exclude,
            );
            set_window(yyp, -2, r);
        }
        // frame_opt ::= range_or_rows BETWEEN frame_bound_s AND frame_bound_e frame_exclude_opt
        323 => {
            let start = take_bound(yyp, -3);
            let end = take_bound(yyp, -1);
            let e_type = get_i32(yyp, -5);
            let exclude = get_u8(yyp, 0);
            let r = window_alloc(
                db,
                p_parse,
                e_type,
                start.e_type,
                start.p_expr,
                end.e_type,
                end.p_expr,
                exclude,
            );
            set_window(yyp, -5, r);
        }
        // frame_bound_s ::= frame_bound; frame_bound_e ::= frame_bound: o valor já está na
        // entrada (`yylhsminor.yy509 = yymsp[0].yy509` volta para o mesmo lugar).
        325 | 327 => {}
        // frame_bound_s ::= UNBOUNDED PRECEDING; frame_bound_e ::= UNBOUNDED FOLLOWING;
        // frame_bound ::= CURRENT ROW
        326 | 328 | 330 => {
            let m = major_at(yyp, -1);
            set_bound(yyp, -1, m, None);
        }
        // frame_bound ::= expr PRECEDING|FOLLOWING
        329 => {
            let m = major_at(yyp, 0);
            let e = take_expr(yyp, -1);
            set_bound(yyp, -1, m, e);
        }
        // frame_exclude_opt ::=
        331 => set_u8(yyp, 1, 0),
        // frame_exclude_opt ::= EXCLUDE frame_exclude
        332 => {
            let v = get_u8(yyp, 0);
            set_u8(yyp, -1, v);
        }
        // frame_exclude ::= NO OTHERS; frame_exclude ::= CURRENT ROW
        333 | 334 => {
            let m = major_at(yyp, -1);
            set_u8(yyp, -1, m as u8);
        }
        // frame_exclude ::= GROUP|TIES
        335 => {
            let m = major_at(yyp, 0);
            set_u8(yyp, 0, m as u8);
        }
        // window_clause ::= WINDOW windowdefn_list
        336 => {
            let w = take_window(yyp, 0);
            set_window(yyp, -1, w);
        }
        // filter_over ::= filter_clause over_clause
        337 => {
            let filter = take_expr(yyp, -1);
            let mut win = take_window(yyp, 0);
            match win.as_deref_mut() {
                Some(w) => w.p_filter = filter,
                None => drop(filter),
            }
            set_window(yyp, -1, win);
        }
        // filter_over ::= over_clause: o valor já está na entrada.
        338 => {}
        // filter_over ::= filter_clause
        339 => {
            let filter = take_expr(yyp, 0);
            let w = Window {
                e_frm_type: TK_FILTER,
                p_filter: filter,
                ..Window::default()
            };
            set_window(yyp, 0, Some(Box::new(w)));
        }
        // over_clause ::= OVER LP window RP
        340 => {
            let w = take_window(yyp, -1);
            debug_assert!(w.is_some());
            set_window(yyp, -3, w);
        }
        // over_clause ::= OVER nm
        341 => {
            let nm = get_tok(yyp, 0);
            let w = Window { z_name: Some(nm.z), ..Window::default() };
            set_window(yyp, -1, Some(Box::new(w)));
        }
        // filter_clause ::= FILTER LP WHERE expr RP
        342 => {
            let e = take_expr(yyp, -1);
            set_expr(yyp, -4, e);
        }
        // term ::= QNUMBER
        343 => {
            let major = major_at(yyp, 0);
            let t = get_tok(yyp, 0);
            let mut e = crate::parse_reduce::token_expr(p_parse, major, &t);
            dequote_number(db, p_parse, e.as_deref_mut());
            set_expr(yyp, 0, e);
        }
        // Regras sem ação (o `default:` do C: 344 a 408, mais as que o lemon otimizou) e as
        // regras compartilhadas, que `yy_reduce` já tratou, não fazem nada.
        _ => {}
    }
}
