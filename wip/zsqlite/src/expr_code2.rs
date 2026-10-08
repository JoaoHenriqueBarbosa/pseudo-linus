//! Tradução de `expr.c`, terceira parte (chunks `expr_c.013` a `expr_c.017`): fatoração de
//! constantes (`sqlite3ExprCodeRunJustOnce`), `sqlite3ExprCode` e variantes, listas de expressões,
//! `BETWEEN`, saltos condicionais (`sqlite3ExprIfTrue`/`sqlite3ExprIfFalse`), comparação profunda
//! de árvores (`sqlite3ExprCompare`), implicação (`sqlite3ExprImpliesExpr`), os walkers de
//! `sqlite3ExprImpliesNonNullRow`, `sqlite3ExprCoveredByIndex` e `sqlite3ReferencesSrcList`, a
//! análise de agregados (`sqlite3ExprAnalyzeAggregates`) e o gerenciador de registradores
//! temporários. O fim de `sqlite3ExprCodeTarget` está em `expr_code.rs`.
//!
//! Convenções (as mesmas de `expr.rs` e `expr_code.rs`, ver CONVENTIONS.md):
//!
//! - Funções de geração de código recebem `(db: &mut Connection, parse: &mut Parse, ...)` e, por
//!   último, `own: Option<&Rc<Table>>`, a tabela dona da árvore (para `TabRef::Own`).
//! - Onde o C monta `Expr` na pilha apontando para subárvores da árvore original
//!   (`exprCodeBetween`, o `opCompare` do `CASE`), a subárvore é MOVIDA para o nó temporário
//!   (`Option::take`) e devolvida depois. Onde o C compartilha um mesmo nó entre dois pais
//!   (`pDel` de `exprCodeBetween`), cada pai recebe um `Expr::clone` depois de `exprToRegister`.
//! - `sqlite3ExprSimplifiedAndOr` devolve um nó da própria árvore; como `sqlite3ExprIfTrue` e
//!   `sqlite3ExprIfFalse` precisam dele mutável, o caminho até o nó é achado com a função
//!   imutável (comparação de endereços) e percorrido de novo por empréstimo mutável.
//! - `AggInfoCol.p_c_expr` e `AggInfoFunc.p_f_expr` são CÓPIAS (ver `sqlite_int.rs`): a cópia é
//!   tirada depois de gravar `i_agg`, `p_agg_info` e o `TK_AGG_COLUMN` no nó original, para
//!   que ela tenha o estado que o nó compartilhado tem no C. Por isso o
//!   `agginfoPersistExprCb` do C (que só existe para trocar um ponteiro direto por uma cópia)
//!   não tem o que fazer aqui.
//! - `sqlite3VdbeReleaseRegisters` só existe sob `SQLITE_DEBUG` (macro vazia fora dele) e
//!   `sqlite3FirstAvailableRegister` só sob `SQLITE_ENABLE_STAT4` ou `SQLITE_DEBUG`: ambos somem,
//!   como `sqlite3NoTempsInRange`.

use std::rc::Rc;

use crate::build::table_column_to_index;
use crate::callback::find_function;
use crate::connection::{Connection, Parse};
use crate::consts::{
    EP_COMMUTED, EP_DISTINCT, EP_FIXED_COL, EP_HAS_FUNC, EP_INNER_ON, EP_INT_VALUE, EP_OUTER_ON,
    EP_REDUCED, EP_SUBQUERY, EP_TOKEN_ONLY, EP_WIN_FUNC, EP_X_IS_SELECT, NC_INAGGFUNC, OP_COPY,
    OP_IF, OP_IFNOT, OP_ONCE, OP_SCOPY, SQLITE_AFF_BLOB, SQLITE_ECEL_DUP, SQLITE_ECEL_FACTOR,
    SQLITE_ECEL_OMITREF, SQLITE_ECEL_REF, SQLITE_FUNC_NEEDCOLL, SQLITE_JUMPIFNULL, SQLITE_NULLEQ,
    SQLITE_SUBTYPE, SQLITE_TEXT, SQLITE_UTF8, TK_AGG_COLUMN, TK_AGG_FUNCTION, TK_AND, TK_BETWEEN,
    TK_BITAND, TK_BITNOT, TK_BITOR, TK_CASE, TK_COLLATE, TK_COLUMN, TK_CONCAT, TK_EQ, TK_FUNCTION,
    TK_GE, TK_GT, TK_IF_NULL_ROW, TK_IN, TK_IS, TK_ISNOT, TK_ISNULL, TK_LE, TK_LSHIFT, TK_LT,
    TK_MINUS, TK_NE, TK_NOT, TK_NOTNULL, TK_NULL, TK_OR, TK_ORDER, TK_PLUS, TK_RAISE,
    TK_REGISTER, TK_REM, TK_RSHIFT, TK_SLASH, TK_SPAN, TK_STAR, TK_STRING, TK_TRUEFALSE, TK_TRUTH,
    TK_UMINUS, TK_UPLUS, TK_VARIABLE, TK_VECTOR, WRC_ABORT, WRC_CONTINUE, WRC_PRUNE,
};
use crate::expr::{
    code_compare, expr_dup, expr_is_vector, expr_list_append, expr_skip_collate,
    expr_skip_collate_and_likely,
};
use crate::expr_code::{
    expr_code_in, expr_code_target, expr_code_vector, expr_is_constant_not_join,
    expr_simplified_and_or, expr_to_register, expr_truth_value, skip_collate_and_likely_mut,
    vdbe_of_parse,
};
use crate::mem::{mem_compare, value_from_expr, value_text, value_type, Mem};
use crate::sqlite_int::{
    AggInfoCol, AggInfoFunc, AggInfoId, Expr, ExprList, Index, NameContext, NcU, Select, SrcList,
    TabRef, Table, Walker,
};
use crate::util::{str_icmp, stricmp, strlen30};
use crate::vdbeaux::{
    add_op0, add_op2, add_op3, get_last_op, jump_here, make_label, resolve_label, typeof_column,
    vdbe_goto,
};
use crate::vdbeaux3::{vdbe_get_bound_value, vdbe_set_varmask};
use crate::walker::{
    expr_walk_noop, select_walk_noop, walk_expr, walk_expr_list, walker_depth_decrease,
    walker_depth_increase,
};
use crate::window::window_compare;

// ---------------------------------------------------------------------------------------------
// Auxiliares privados (não existem no C)
// ---------------------------------------------------------------------------------------------

/// `xJump` de `exprCodeBetween` é `NULL`: grava o booleano em `reg[dest]`.
const XJUMP_NONE: i32 = 0;
/// `xJump` de `exprCodeBetween` é `sqlite3ExprIfTrue`.
const XJUMP_IF_TRUE: i32 = 1;
/// `xJump` de `exprCodeBetween` é `sqlite3ExprIfFalse`.
const XJUMP_IF_FALSE: i32 = 2;

/// Reempresta o par `(db, parse)` para uma chamada recursiva sem consumi-lo.
fn reborrow<'x>(
    pp: &'x mut Option<(&mut Connection, &mut Parse)>,
) -> Option<(&'x mut Connection, &'x mut Parse)> {
    pp.as_mut().map(|(d, p)| (&mut **d, &mut **p))
}

/// `strcmp(a, b) == 0` sobre strings C guardadas sem o NUL final (ou com NUL embutido).
fn c_str_eq(a: &[u8], b: &[u8]) -> bool {
    a[..strlen30(a) as usize] == b[..strlen30(b) as usize]
}

// ---------------------------------------------------------------------------------------------
// Fatoração de constantes e sqlite3ExprCode (chunk 013)
// ---------------------------------------------------------------------------------------------

/// `sqlite3ExprCodeRunJustOnce`: gera o código que avalia `p_expr` uma única vez por execução do
/// comando preparado. Se a expressão usa funções (que podem lançar exceção) o código é guardado
/// por um `OP_Once`; senão ele vai para `Parse.p_const_expr`, na seção de inicialização no fim do
/// programa. Com `reg_dest > 0` o resultado fica sempre nesse registrador e não é reutilizável;
/// com `reg_dest < 0` a rotina escolhe o registrador. Devolve o registrador do resultado.
pub fn expr_code_run_just_once(
    db: &mut Connection,
    parse: &mut Parse,
    p_expr: &Expr,
    reg_dest: i32,
    own: Option<&Rc<Table>>,
) -> i32 {
    let mut reg_dest = reg_dest;
    debug_assert!(parse.ok_const_factor != 0);
    debug_assert!(reg_dest != 0);
    if reg_dest < 0 {
        if let Some(p) = parse.p_const_expr.as_deref() {
            for item in p.a.iter() {
                if item.fg.reusable
                    && expr_compare(None, item.p_expr.as_deref(), Some(p_expr), -1) == 0
                {
                    return item.i_const_expr_reg;
                }
            }
        }
    }
    let mut p_dup = expr_dup(Some(p_expr), 0);
    if p_dup.as_deref().is_some_and(|e| e.has_property(EP_HAS_FUNC)) {
        debug_assert!(parse.p_vdbe.is_some());
        let addr = add_op0(vdbe_of_parse(parse), OP_ONCE as i32);
        parse.ok_const_factor = 0;
        if db.malloc_failed == 0 {
            if reg_dest < 0 {
                parse.n_mem += 1;
                reg_dest = parse.n_mem;
            }
            if let Some(e) = p_dup.as_deref_mut() {
                expr_code(db, parse, e, reg_dest, own);
            }
        }
        parse.ok_const_factor = 1;
        drop(p_dup);
        jump_here(vdbe_of_parse(parse), addr);
    } else {
        let mut p = expr_list_append(parse.p_const_expr.take(), p_dup);
        if let Some(item) = p.as_deref_mut().and_then(|l| l.a.last_mut()) {
            item.fg.reusable = reg_dest < 0;
            if reg_dest < 0 {
                parse.n_mem += 1;
                reg_dest = parse.n_mem;
            }
            item.i_const_expr_reg = reg_dest;
        }
        parse.p_const_expr = p;
    }
    reg_dest
}

/// `sqlite3ExprCodeTemp`: gera o código que avalia `p_expr` num registrador e devolve o número
/// dele. Se o registrador é temporário e pode ser liberado, `*p_reg` recebe o número; senão 0.
/// Se a expressão é constante, o código pode ir para a seção de inicialização do programa.
pub fn expr_code_temp(
    db: &mut Connection,
    parse: &mut Parse,
    p_expr: &mut Expr,
    p_reg: &mut i32,
    own: Option<&Rc<Table>>,
) -> i32 {
    let Some(p_expr) = skip_collate_and_likely_mut(Some(p_expr)) else {
        debug_assert!(false);
        *p_reg = 0;
        return 0;
    };
    if parse.ok_const_factor != 0
        && p_expr.op != TK_REGISTER
        && expr_is_constant_not_join(Some((&mut *db, &mut *parse)), Some(&mut *p_expr)) != 0
    {
        *p_reg = 0;
        expr_code_run_just_once(db, parse, p_expr, -1, own)
    } else {
        let r1 = get_temp_reg(parse);
        let r2 = expr_code_target(db, parse, p_expr, r1, own);
        if r2 == r1 {
            *p_reg = r1;
        } else {
            release_temp_reg(parse, r1);
            *p_reg = 0;
        }
        r2
    }
}

/// `sqlite3ExprCode`: gera o código que avalia `p_expr` e grava o resultado no registrador
/// `target`; o resultado aparece garantidamente nele.
pub fn expr_code(
    db: &mut Connection,
    parse: &mut Parse,
    p_expr: &mut Expr,
    target: i32,
    own: Option<&Rc<Table>>,
) {
    debug_assert!(target > 0 && target <= parse.n_mem);
    debug_assert!(parse.p_vdbe.is_some() || db.malloc_failed != 0);
    if parse.p_vdbe.is_none() {
        return;
    }
    let in_reg = expr_code_target(db, parse, p_expr, target, own);
    if in_reg != target {
        let p_x = expr_skip_collate_and_likely(Some(&*p_expr));
        let op = if p_x.is_some_and(|x| x.has_property(EP_SUBQUERY) || x.op == TK_REGISTER) {
            OP_COPY
        } else {
            OP_SCOPY
        };
        add_op2(vdbe_of_parse(parse), op as i32, in_reg, target);
    }
}

/// `sqlite3ExprCodeCopy`: como [`expr_code`], mas sobre uma cópia transitória de `p_expr`, que
/// portanto fica garantidamente intacta.
pub fn expr_code_copy(
    db: &mut Connection,
    parse: &mut Parse,
    p_expr: Option<&Expr>,
    target: i32,
    own: Option<&Rc<Table>>,
) {
    let mut p_dup = expr_dup(p_expr, 0);
    if db.malloc_failed == 0 {
        if let Some(e) = p_dup.as_deref_mut() {
            expr_code(db, parse, e, target, own);
        }
    }
}

/// `sqlite3ExprCodeFactorable`: como [`expr_code`]; se a expressão é constante, pode codificá-la
/// na inicialização do programa.
pub fn expr_code_factorable(
    db: &mut Connection,
    parse: &mut Parse,
    p_expr: &mut Expr,
    target: i32,
    own: Option<&Rc<Table>>,
) {
    if parse.ok_const_factor != 0
        && expr_is_constant_not_join(Some((&mut *db, &mut *parse)), Some(&mut *p_expr)) != 0
    {
        expr_code_run_just_once(db, parse, p_expr, target, own);
    } else {
        expr_code_copy(db, parse, Some(&*p_expr), target, own);
    }
}

/// `sqlite3ExprCodeExprList`: empurra o valor de cada elemento de `p_list` para uma sequência de
/// registradores que começa em `target`. Devolve a quantidade de elementos avaliados (em geral
/// `p_list.a.len()`, menos se `SQLITE_ECEL_OMITREF` omite alguns).
///
/// `SQLITE_ECEL_DUP` impede o preenchimento por `OP_SCopy` (usa `OP_Copy`); `SQLITE_ECEL_FACTOR`
/// deixa fatorar os argumentos constantes; `SQLITE_ECEL_REF` copia de `src_reg` os termos com
/// `i_order_by_col > 0` (já avaliados), e com `SQLITE_ECEL_OMITREF` eles são só omitidos.
pub fn expr_code_expr_list(
    db: &mut Connection,
    parse: &mut Parse,
    p_list: &mut ExprList,
    target: i32,
    src_reg: i32,
    flags: u8,
    own: Option<&Rc<Table>>,
) -> i32 {
    let mut flags = flags;
    let copy_op = if (flags & SQLITE_ECEL_DUP) != 0 { OP_COPY } else { OP_SCOPY };
    debug_assert!(target > 0);
    debug_assert!(parse.p_vdbe.is_some());
    let mut n = p_list.a.len() as i32;
    if parse.ok_const_factor == 0 {
        flags &= !SQLITE_ECEL_FACTOR;
    }
    // `i` é o índice do resultado; `k`, o do item (os dois divergem quando um item é omitido).
    let mut i: i32 = 0;
    let mut k: usize = 0;
    while i < n {
        let j = p_list.a[k].i_order_by_col as i32;
        if (flags & SQLITE_ECEL_REF) != 0 && j > 0 {
            if (flags & SQLITE_ECEL_OMITREF) != 0 {
                i -= 1;
                n -= 1;
            } else {
                add_op2(vdbe_of_parse(parse), copy_op as i32, j + src_reg - 1, target + i);
            }
        } else if let Some(p_item_expr) = p_list.a[k].p_expr.as_deref_mut() {
            if (flags & SQLITE_ECEL_FACTOR) != 0
                && expr_is_constant_not_join(Some((&mut *db, &mut *parse)), Some(&mut *p_item_expr))
                    != 0
            {
                expr_code_run_just_once(db, parse, p_item_expr, target + i, own);
            } else {
                let in_reg = expr_code_target(db, parse, p_item_expr, target + i, own);
                if in_reg != target + i {
                    let v = vdbe_of_parse(parse);
                    let merged = copy_op == OP_COPY
                        && match get_last_op(v) {
                            Some(p_op)
                                if p_op.opcode == OP_COPY
                                    && p_op.p1 + p_op.p3 + 1 == in_reg
                                    && p_op.p2 + p_op.p3 + 1 == target + i
                                    && p_op.p5 == 0 =>
                            {
                                // A flag de "não fundir" precisa estar limpa.
                                p_op.p3 += 1;
                                true
                            }
                            _ => false,
                        };
                    if !merged {
                        add_op2(v, copy_op as i32, in_reg, target + i);
                    }
                }
            }
        }
        i += 1;
        k += 1;
    }
    n
}

/// `exprCodeBetween`: gera o código de `x BETWEEN y AND z`, equivalente a `x>=y AND x<=z`, com
/// eliminação da subexpressão comum `x`. `x_jump` diz o que fazer: `XJUMP_NONE` grava o booleano
/// em `reg[dest]`; `XJUMP_IF_TRUE` e `XJUMP_IF_FALSE` saltam para `dest`. `jump_if_null` é
/// ignorado quando `x_jump` é `XJUMP_NONE`. (Na chamada de `expr_code.rs`, `x_jump` é 0.)
pub fn expr_code_between(
    db: &mut Connection,
    parse: &mut Parse,
    p_expr: &mut Expr,
    dest: i32,
    x_jump: i32,
    jump_if_null: i32,
    own: Option<&Rc<Table>>,
) {
    let mut reg_free1 = 0;
    debug_assert!(p_expr.use_x_list());
    let mut p_del = expr_dup(p_expr.p_left.as_deref(), 0);
    if db.malloc_failed == 0 {
        if let Some(del) = p_del.as_deref_mut() {
            let reg = expr_code_vector(db, parse, del, &mut reg_free1, own);
            expr_to_register(del, reg);
            if x_jump == XJUMP_NONE {
                // Marca a expressão como vinda de um ON ou USING de junção, para que
                // `sqlite3ExprCodeTarget` não tente movê-la para `Parse.p_const_expr`. Devia ser
                // um bit novo, mas as flags de `Expr` acabaram e o bit `EP_OuterON` é reaproveitado.
                del.flags |= EP_OUTER_ON;
            }
        }
        // `compLeft` (x>=y), `compRight` (x<=z) e `exprAnd`. O `pDel` do C é compartilhado pelos
        // dois comparadores; aqui cada um leva uma cópia já convertida em `TK_REGISTER`.
        let mut comp_left = Expr { op: crate::consts::TK_GE, ..Expr::default() };
        let mut comp_right = Expr { op: crate::consts::TK_LE, ..Expr::default() };
        comp_left.p_left = p_del.clone();
        comp_right.p_left = p_del;
        if let Some(l) = p_expr.x_list_mut() {
            comp_left.p_right = l.a.get_mut(0).and_then(|it| it.p_expr.take());
            comp_right.p_right = l.a.get_mut(1).and_then(|it| it.p_expr.take());
        }
        let mut expr_and = Expr { op: TK_AND, ..Expr::default() };
        expr_and.p_left = Some(Box::new(comp_left));
        expr_and.p_right = Some(Box::new(comp_right));
        match x_jump {
            XJUMP_IF_TRUE => expr_if_true(db, parse, &mut expr_and, dest, jump_if_null, own),
            XJUMP_IF_FALSE => expr_if_false(db, parse, &mut expr_and, dest, jump_if_null, own),
            _ => {
                expr_code_target(db, parse, &mut expr_and, dest, own);
            }
        }
        release_temp_reg(parse, reg_free1);
        // Devolve os limites à lista do BETWEEN.
        let back_left = expr_and.p_left.as_deref_mut().and_then(|c| c.p_right.take());
        let back_right = expr_and.p_right.as_deref_mut().and_then(|c| c.p_right.take());
        if let Some(l) = p_expr.x_list_mut() {
            if let Some(it) = l.a.get_mut(0) {
                it.p_expr = back_left;
            }
            if let Some(it) = l.a.get_mut(1) {
                it.p_expr = back_right;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// sqlite3ExprIfTrue e sqlite3ExprIfFalse (chunks 013 e 014)
// ---------------------------------------------------------------------------------------------

/// Caminho (`false` é esquerda, `true` é direita) da raiz até o nó que
/// `sqlite3ExprSimplifiedAndOr` devolve; vazio se ele devolve a própria raiz.
fn simplified_and_or_path(p_expr: &Expr) -> Vec<bool> {
    let target = expr_simplified_and_or(p_expr);
    let mut path = Vec::new();
    if !std::ptr::eq(target, p_expr) {
        and_or_path_to(p_expr, target, &mut path);
    }
    path
}

/// Busca em profundidade, só através de nós AND/OR, do endereço `target`.
fn and_or_path_to(cur: &Expr, target: &Expr, path: &mut Vec<bool>) -> bool {
    if std::ptr::eq(cur, target) {
        return true;
    }
    if cur.op != TK_AND && cur.op != TK_OR {
        return false;
    }
    for (right, child) in [(false, cur.p_left.as_deref()), (true, cur.p_right.as_deref())] {
        if let Some(c) = child {
            path.push(right);
            if and_or_path_to(c, target, path) {
                return true;
            }
            path.pop();
        }
    }
    false
}

/// O nó alcançado seguindo `path` a partir de `p_expr`, para alterar.
fn descend_mut<'a>(p_expr: &'a mut Expr, path: &[bool]) -> &'a mut Expr {
    let mut cur = p_expr;
    for &right in path {
        let next = if right { cur.p_right.as_deref_mut() } else { cur.p_left.as_deref_mut() };
        match next {
            Some(n) => cur = n,
            None => unreachable!("caminho produzido por and_or_path_to"),
        }
    }
    cur
}

/// `sqlite3ExprIfTrue`: gera o código de uma expressão booleana de modo que há um salto para
/// `dest` se ela é verdadeira e a execução segue em frente se é falsa. Se o resultado é NULL, o
/// salto ocorre se `jump_if_null` é `SQLITE_JUMPIFNULL`.
///
/// O código depende de alguns valores `TK_*` serem iguais aos `OP_*` das operações
/// correspondentes (`TK_EQ` e `OP_Eq`, por exemplo); `consts` já os traz alinhados.
pub fn expr_if_true(
    db: &mut Connection,
    parse: &mut Parse,
    p_expr: &mut Expr,
    dest: i32,
    jump_if_null: i32,
    own: Option<&Rc<Table>>,
) {
    let mut jump_if_null = jump_if_null;
    let mut op = p_expr.op;
    let mut reg_free1 = 0;
    let mut reg_free2 = 0;
    debug_assert!(jump_if_null == SQLITE_JUMPIFNULL as i32 || jump_if_null == 0);
    if parse.p_vdbe.is_none() {
        return;
    }
    let mut default_expr = false;
    match op {
        TK_AND | TK_OR => {
            let path = simplified_and_or_path(p_expr);
            if !path.is_empty() {
                let alt = descend_mut(p_expr, &path);
                expr_if_true(db, parse, alt, dest, jump_if_null, own);
            } else if op == TK_AND {
                let d2 = make_label(parse);
                if let Some(l) = p_expr.p_left.as_deref_mut() {
                    expr_if_false(db, parse, l, d2, jump_if_null ^ SQLITE_JUMPIFNULL as i32, own);
                }
                if let Some(r) = p_expr.p_right.as_deref_mut() {
                    expr_if_true(db, parse, r, dest, jump_if_null, own);
                }
                resolve_label(parse, db, d2);
            } else {
                if let Some(l) = p_expr.p_left.as_deref_mut() {
                    expr_if_true(db, parse, l, dest, jump_if_null, own);
                }
                if let Some(r) = p_expr.p_right.as_deref_mut() {
                    expr_if_true(db, parse, r, dest, jump_if_null, own);
                }
            }
        }
        TK_NOT => {
            if let Some(l) = p_expr.p_left.as_deref_mut() {
                expr_if_false(db, parse, l, dest, jump_if_null, own);
            }
        }
        TK_TRUTH => {
            let is_not = (p_expr.op2 == TK_ISNOT) as i32;
            let is_true = p_expr.p_right.as_deref().map_or(0, expr_truth_value);
            let jin = if is_not != 0 { SQLITE_JUMPIFNULL as i32 } else { 0 };
            if let Some(l) = p_expr.p_left.as_deref_mut() {
                if (is_true ^ is_not) != 0 {
                    expr_if_true(db, parse, l, dest, jin, own);
                } else {
                    expr_if_false(db, parse, l, dest, jin, own);
                }
            }
        }
        TK_IS | TK_ISNOT | TK_LT | TK_LE | TK_GT | TK_GE | TK_NE | TK_EQ => {
            if op == TK_IS || op == TK_ISNOT {
                op = if op == TK_IS { TK_EQ } else { TK_NE };
                jump_if_null = SQLITE_NULLEQ as i32;
            }
            let is_commuted = p_expr.has_property(EP_COMMUTED) as i32;
            if let (Some(l), Some(r)) = (p_expr.p_left.as_deref_mut(), p_expr.p_right.as_deref_mut())
            {
                if expr_is_vector(l) {
                    default_expr = true;
                } else {
                    let r1 = expr_code_temp(db, parse, l, &mut reg_free1, own);
                    let r2 = expr_code_temp(db, parse, r, &mut reg_free2, own);
                    code_compare(
                        db, parse, l, r, op, r1, r2, dest, jump_if_null, is_commuted, own,
                    );
                }
            }
        }
        TK_ISNULL | TK_NOTNULL => {
            if let Some(l) = p_expr.p_left.as_deref_mut() {
                let r1 = expr_code_temp(db, parse, l, &mut reg_free1, own);
                let v = vdbe_of_parse(parse);
                typeof_column(v, r1);
                add_op2(v, op as i32, r1, dest);
            }
        }
        TK_BETWEEN => {
            expr_code_between(db, parse, p_expr, dest, XJUMP_IF_TRUE, jump_if_null, own);
        }
        TK_IN => {
            let dest_if_false = make_label(parse);
            let dest_if_null = if jump_if_null != 0 { dest } else { dest_if_false };
            expr_code_in(db, parse, p_expr, dest_if_false, dest_if_null, own);
            vdbe_goto(vdbe_of_parse(parse), dest);
            resolve_label(parse, db, dest_if_false);
        }
        _ => default_expr = true,
    }
    if default_expr {
        if p_expr.always_true() {
            vdbe_goto(vdbe_of_parse(parse), dest);
        } else if p_expr.always_false() {
            // Nada a fazer.
        } else {
            let r1 = expr_code_temp(db, parse, p_expr, &mut reg_free1, own);
            add_op3(vdbe_of_parse(parse), OP_IF as i32, r1, dest, (jump_if_null != 0) as i32);
        }
    }
    release_temp_reg(parse, reg_free1);
    release_temp_reg(parse, reg_free2);
}

/// `sqlite3ExprIfFalse`: o inverso de [`expr_if_true`]: salta para `dest` se a expressão é falsa
/// e segue em frente se é verdadeira. Se o resultado é NULL, salta se `jump_if_null` é
/// `SQLITE_JUMPIFNULL` e segue em frente se é 0.
pub fn expr_if_false(
    db: &mut Connection,
    parse: &mut Parse,
    p_expr: &mut Expr,
    dest: i32,
    jump_if_null: i32,
    own: Option<&Rc<Table>>,
) {
    let mut jump_if_null = jump_if_null;
    let mut reg_free1 = 0;
    let mut reg_free2 = 0;
    debug_assert!(jump_if_null == SQLITE_JUMPIFNULL as i32 || jump_if_null == 0);
    if parse.p_vdbe.is_none() {
        return;
    }
    // A relação entre `pExpr->op` e `op`:
    //
    //       pExpr->op            op
    //       TK_ISNULL          OP_NotNull
    //       TK_NOTNULL         OP_IsNull
    //       TK_NE              OP_Eq
    //       TK_EQ              OP_Ne
    //       TK_GT              OP_Le
    //       TK_LE              OP_Gt
    //       TK_GE              OP_Lt
    //       TK_LT              OP_Ge
    //
    // Para os outros `pExpr->op`, `op` é indefinido e não usado. Os valores `TK_` e `OP_` são
    // dispostos de modo que o mapa se calcule com a expressão abaixo.
    let one = (TK_ISNULL & 1) as i32;
    let mut op = (((p_expr.op as i32 + one) ^ 1) - one) as u8;
    let mut default_expr = false;
    match p_expr.op {
        TK_AND | TK_OR => {
            let path = simplified_and_or_path(p_expr);
            if !path.is_empty() {
                let alt = descend_mut(p_expr, &path);
                expr_if_false(db, parse, alt, dest, jump_if_null, own);
            } else if p_expr.op == TK_AND {
                if let Some(l) = p_expr.p_left.as_deref_mut() {
                    expr_if_false(db, parse, l, dest, jump_if_null, own);
                }
                if let Some(r) = p_expr.p_right.as_deref_mut() {
                    expr_if_false(db, parse, r, dest, jump_if_null, own);
                }
            } else {
                let d2 = make_label(parse);
                if let Some(l) = p_expr.p_left.as_deref_mut() {
                    expr_if_true(db, parse, l, d2, jump_if_null ^ SQLITE_JUMPIFNULL as i32, own);
                }
                if let Some(r) = p_expr.p_right.as_deref_mut() {
                    expr_if_false(db, parse, r, dest, jump_if_null, own);
                }
                resolve_label(parse, db, d2);
            }
        }
        TK_NOT => {
            if let Some(l) = p_expr.p_left.as_deref_mut() {
                expr_if_true(db, parse, l, dest, jump_if_null, own);
            }
        }
        TK_TRUTH => {
            let is_not = (p_expr.op2 == TK_ISNOT) as i32;
            let is_true = p_expr.p_right.as_deref().map_or(0, expr_truth_value);
            let jin = if is_not != 0 { 0 } else { SQLITE_JUMPIFNULL as i32 };
            if let Some(l) = p_expr.p_left.as_deref_mut() {
                if (is_true ^ is_not) != 0 {
                    // IS TRUE e IS NOT FALSE.
                    expr_if_false(db, parse, l, dest, jin, own);
                } else {
                    // IS FALSE e IS NOT TRUE.
                    expr_if_true(db, parse, l, dest, jin, own);
                }
            }
        }
        TK_IS | TK_ISNOT | TK_LT | TK_LE | TK_GT | TK_GE | TK_NE | TK_EQ => {
            if p_expr.op == TK_IS || p_expr.op == TK_ISNOT {
                op = if p_expr.op == TK_IS { TK_NE } else { TK_EQ };
                jump_if_null = SQLITE_NULLEQ as i32;
            }
            let is_commuted = p_expr.has_property(EP_COMMUTED) as i32;
            if let (Some(l), Some(r)) = (p_expr.p_left.as_deref_mut(), p_expr.p_right.as_deref_mut())
            {
                if expr_is_vector(l) {
                    default_expr = true;
                } else {
                    let r1 = expr_code_temp(db, parse, l, &mut reg_free1, own);
                    let r2 = expr_code_temp(db, parse, r, &mut reg_free2, own);
                    code_compare(
                        db, parse, l, r, op, r1, r2, dest, jump_if_null, is_commuted, own,
                    );
                }
            }
        }
        TK_ISNULL | TK_NOTNULL => {
            if let Some(l) = p_expr.p_left.as_deref_mut() {
                let r1 = expr_code_temp(db, parse, l, &mut reg_free1, own);
                let v = vdbe_of_parse(parse);
                typeof_column(v, r1);
                add_op2(v, op as i32, r1, dest);
            }
        }
        TK_BETWEEN => {
            expr_code_between(db, parse, p_expr, dest, XJUMP_IF_FALSE, jump_if_null, own);
        }
        TK_IN => {
            if jump_if_null != 0 {
                expr_code_in(db, parse, p_expr, dest, dest, own);
            } else {
                let dest_if_null = make_label(parse);
                expr_code_in(db, parse, p_expr, dest, dest_if_null, own);
                resolve_label(parse, db, dest_if_null);
            }
        }
        _ => default_expr = true,
    }
    if default_expr {
        if p_expr.always_false() {
            vdbe_goto(vdbe_of_parse(parse), dest);
        } else if p_expr.always_true() {
            // Nada a fazer.
        } else {
            let r1 = expr_code_temp(db, parse, p_expr, &mut reg_free1, own);
            add_op3(vdbe_of_parse(parse), OP_IFNOT as i32, r1, dest, (jump_if_null != 0) as i32);
        }
    }
    release_temp_reg(parse, reg_free1);
    release_temp_reg(parse, reg_free2);
}

/// `sqlite3ExprIfFalseDup`: como [`expr_if_false`], mas gera o código sobre uma cópia de
/// `p_expr`, que é descartada depois. A original fica intacta.
pub fn expr_if_false_dup(
    db: &mut Connection,
    parse: &mut Parse,
    p_expr: Option<&Expr>,
    dest: i32,
    jump_if_null: i32,
    own: Option<&Rc<Table>>,
) {
    let mut p_copy = expr_dup(p_expr, 0);
    if db.malloc_failed == 0 {
        if let Some(c) = p_copy.as_deref_mut() {
            expr_if_false(db, parse, c, dest, jump_if_null, own);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Comparação profunda (chunks 014 e 015)
// ---------------------------------------------------------------------------------------------

/// `exprCompareVariable`: `p_var` é garantidamente uma variável SQL e `p_expr` pode ser qualquer
/// expressão. Se `p_expr` é um valor simples (inteiro, real, texto, blob ou NULL), o VDBE em
/// preparo passa a ser repreparado a cada novo valor ligado a `p_var`. Devolve verdadeiro se
/// `p_expr` é um valor simples igual ao ligado hoje a `p_var`.
fn expr_compare_variable(
    db: &mut Connection,
    parse: &mut Parse,
    p_var: &Expr,
    p_expr: &Expr,
) -> bool {
    let mut res = false;
    let mut p_r: Option<Mem> = None;
    value_from_expr(db, Some(p_expr), SQLITE_UTF8 as u8, SQLITE_AFF_BLOB, &mut p_r);
    if let Some(p_r) = p_r.as_ref() {
        let i_var = p_var.i_column;
        if let Some(v) = parse.p_vdbe.as_deref_mut() {
            vdbe_set_varmask(v, i_var);
        }
        let p_reprepare = parse.p_reprepare;
        let p_l = vdbe_get_bound_value(p_reprepare.and_then(|id| db.stmt(id)), i_var, SQLITE_AFF_BLOB);
        if let Some(mut p_l) = p_l {
            if value_type(&p_l) == SQLITE_TEXT {
                // Garante que a codificação é UTF-8.
                let _ = value_text(&mut p_l, SQLITE_UTF8 as u8);
            }
            res = 0 == mem_compare(&p_l, p_r, None);
        }
    }
    res
}

/// `sqlite3ExprCompare`: comparação profunda de duas árvores. Devolve 0 se são idênticas, 1 se
/// diferem só por um COLLATE no topo e 2 se há outras diferenças. Se algum subelemento de `p_b`
/// tem `i_table == -1`, ele pode igualar um equivalente de `p_a` com `i_table == i_tab`. O lado A
/// pode usar `TK_REGISTER`; se B não usa mas é equivalente, ainda devolve 0. Na dúvida devolve 2.
///
/// Com `pp` presente (o `pParse` do C), um `TK_VARIABLE` de A com valor ligado em
/// `Parse.p_reprepare` pode casar com um literal de B, e `expmask` do VDBE é atualizada.
pub fn expr_compare(
    mut pp: Option<(&mut Connection, &mut Parse)>,
    p_a: Option<&Expr>,
    p_b: Option<&Expr>,
    i_tab: i32,
) -> i32 {
    let (a, b) = match (p_a, p_b) {
        (Some(a), Some(b)) => (a, b),
        (None, None) => return 0,
        _ => return 2,
    };
    if a.op == TK_VARIABLE {
        if let Some((db, parse)) = pp.as_mut() {
            if expr_compare_variable(db, parse, a, b) {
                return 0;
            }
        }
    }
    let combined_flags = a.flags | b.flags;
    if (combined_flags & EP_INT_VALUE) != 0 {
        if (a.flags & b.flags & EP_INT_VALUE) != 0 && a.i_value() == b.i_value() {
            return 0;
        }
        return 2;
    }
    if a.op != b.op || a.op == TK_RAISE {
        if a.op == TK_COLLATE
            && expr_compare(reborrow(&mut pp), a.p_left.as_deref(), Some(b), i_tab) < 2
        {
            return 1;
        }
        if b.op == TK_COLLATE
            && expr_compare(reborrow(&mut pp), Some(a), b.p_left.as_deref(), i_tab) < 2
        {
            return 1;
        }
        if a.op == TK_AGG_COLUMN && b.op == TK_COLUMN && b.i_table < 0 && a.i_table == i_tab {
            // Segue adiante.
        } else {
            return 2;
        }
    }
    debug_assert!(!a.has_property(EP_INT_VALUE));
    debug_assert!(!b.has_property(EP_INT_VALUE));
    if let Some(za) = a.z_token() {
        let zb = b.z_token();
        if a.op == TK_FUNCTION || a.op == TK_AGG_FUNCTION {
            if str_icmp(za, zb.unwrap_or(&[])) != 0 {
                return 2;
            }
            debug_assert!(a.op == b.op);
            if a.has_property(EP_WIN_FUNC) != b.has_property(EP_WIN_FUNC) {
                return 2;
            }
            if a.has_property(EP_WIN_FUNC)
                && window_compare(reborrow(&mut pp), a.y_win(), b.y_win(), 1) != 0
            {
                return 2;
            }
        } else if a.op == TK_NULL {
            return 0;
        } else if a.op == TK_COLLATE {
            if stricmp(Some(za), zb) != 0 {
                return 2;
            }
        } else if let Some(zb) = zb {
            if a.op != TK_COLUMN && a.op != TK_AGG_COLUMN && !c_str_eq(za, zb) {
                return 2;
            }
        }
    }
    if (a.flags & (EP_DISTINCT | EP_COMMUTED)) != (b.flags & (EP_DISTINCT | EP_COMMUTED)) {
        return 2;
    }
    if (combined_flags & EP_TOKEN_ONLY) == 0 {
        if (combined_flags & EP_X_IS_SELECT) != 0 {
            return 2;
        }
        if (combined_flags & EP_FIXED_COL) == 0
            && expr_compare(reborrow(&mut pp), a.p_left.as_deref(), b.p_left.as_deref(), i_tab) != 0
        {
            return 2;
        }
        if expr_compare(reborrow(&mut pp), a.p_right.as_deref(), b.p_right.as_deref(), i_tab) != 0 {
            return 2;
        }
        if expr_list_compare(a.x_list(), b.x_list(), i_tab) != 0 {
            return 2;
        }
        if a.op != TK_STRING && a.op != TK_TRUEFALSE && (combined_flags & EP_REDUCED) == 0 {
            if a.i_column != b.i_column {
                return 2;
            }
            if a.op2 != b.op2 && a.op == TK_TRUTH {
                return 2;
            }
            if a.op != TK_IN && a.i_table != b.i_table && a.i_table != i_tab {
                return 2;
            }
        }
    }
    0
}

/// `sqlite3ExprListCompare`: compara duas listas. 0 se idênticas, 1 se certamente diferentes, 2
/// se não é possível dizer. Dois nulos são iguais; nulo e não nulo sempre diferem.
pub fn expr_list_compare(p_a: Option<&ExprList>, p_b: Option<&ExprList>, i_tab: i32) -> i32 {
    let (a, b) = match (p_a, p_b) {
        (None, None) => return 0,
        (Some(a), Some(b)) => (a, b),
        _ => return 1,
    };
    if a.a.len() != b.a.len() {
        return 1;
    }
    for (ia, ib) in a.a.iter().zip(b.a.iter()) {
        if ia.fg.sort_flags != ib.fg.sort_flags {
            return 1;
        }
        let res = expr_compare(None, ia.p_expr.as_deref(), ib.p_expr.as_deref(), i_tab);
        if res != 0 {
            return res;
        }
    }
    0
}

/// `sqlite3ExprCompareSkip`: como [`expr_compare`], ignorando os COLLATE do topo.
pub fn expr_compare_skip(p_a: Option<&Expr>, p_b: Option<&Expr>, i_tab: i32) -> i32 {
    expr_compare(None, expr_skip_collate(p_a), expr_skip_collate(p_b), i_tab)
}

// ---------------------------------------------------------------------------------------------
// Implicação (chunk 015)
// ---------------------------------------------------------------------------------------------

/// `exprImpliesNotNull`: verdadeiro se `p` só pode ser verdadeira quando `p_nn` não é NULL; com
/// `seen_not`, se `p` só pode ser não NULL quando `p_nn` não é NULL.
fn expr_implies_not_null(
    mut pp: Option<(&mut Connection, &mut Parse)>,
    p: Option<&Expr>,
    p_nn: Option<&Expr>,
    i_tab: i32,
    seen_not: i32,
) -> i32 {
    let mut seen_not = seen_not;
    if expr_compare(reborrow_pp(&mut pp), p, p_nn, i_tab) == 0 {
        return p_nn.map_or(0, |n| (n.op != TK_NULL) as i32);
    }
    let Some(p) = p else {
        return 0;
    };
    match p.op {
        TK_IN => {
            if seen_not != 0 && p.has_property(EP_X_IS_SELECT) {
                return 0;
            }
            expr_implies_not_null(reborrow_pp(&mut pp), p.p_left.as_deref(), p_nn, i_tab, 1)
        }
        TK_BETWEEN => {
            debug_assert!(p.use_x_list());
            if seen_not != 0 {
                return 0;
            }
            let (e0, e1) = match p.x_list() {
                Some(l) => (
                    l.a.first().and_then(|i| i.p_expr.as_deref()),
                    l.a.get(1).and_then(|i| i.p_expr.as_deref()),
                ),
                None => (None, None),
            };
            if expr_implies_not_null(reborrow_pp(&mut pp), e0, p_nn, i_tab, 1) != 0
                || expr_implies_not_null(reborrow_pp(&mut pp), e1, p_nn, i_tab, 1) != 0
            {
                return 1;
            }
            expr_implies_not_null(reborrow_pp(&mut pp), p.p_left.as_deref(), p_nn, i_tab, 1)
        }
        // Os operadores que ficam "não NULL" só se os dois lados forem (`seen_not = 1`), e os que
        // vêm do `deliberate_fall_through`: o lado direito, depois o esquerdo.
        TK_EQ | TK_NE | TK_LT | TK_LE | TK_GT | TK_GE | TK_PLUS | TK_MINUS | TK_BITOR
        | TK_LSHIFT | TK_RSHIFT | TK_CONCAT | TK_STAR | TK_REM | TK_BITAND | TK_SLASH => {
            if matches!(
                p.op,
                TK_EQ | TK_NE | TK_LT | TK_LE | TK_GT | TK_GE | TK_PLUS | TK_MINUS | TK_BITOR
                    | TK_LSHIFT | TK_RSHIFT | TK_CONCAT
            ) {
                seen_not = 1;
            }
            if expr_implies_not_null(reborrow_pp(&mut pp), p.p_right.as_deref(), p_nn, i_tab, seen_not) != 0 {
                return 1;
            }
            expr_implies_not_null(reborrow_pp(&mut pp), p.p_left.as_deref(), p_nn, i_tab, seen_not)
        }
        TK_SPAN | TK_COLLATE | TK_UPLUS | TK_UMINUS => {
            expr_implies_not_null(reborrow_pp(&mut pp), p.p_left.as_deref(), p_nn, i_tab, seen_not)
        }
        TK_TRUTH => {
            if seen_not != 0 || p.op2 != TK_IS {
                return 0;
            }
            expr_implies_not_null(reborrow_pp(&mut pp), p.p_left.as_deref(), p_nn, i_tab, 1)
        }
        TK_BITNOT | TK_NOT => {
            expr_implies_not_null(reborrow_pp(&mut pp), p.p_left.as_deref(), p_nn, i_tab, 1)
        }
        _ => 0,
    }
}

/// Reempresta o par opcional `(db, pParse)` do C, onde `pParse` pode ser nulo (por exemplo com
/// `SQLITE_EnableQPSG`, que proíbe comparar com os valores ligados).
pub(crate) fn reborrow_pp<'a>(
    pp: &'a mut Option<(&mut Connection, &mut Parse)>,
) -> Option<(&'a mut Connection, &'a mut Parse)> {
    pp.as_mut().map(|(d, p)| (&mut **d, &mut **p))
}

/// `sqlite3ExprImpliesExpr`: verdadeiro se é possível provar que `p_e2` é sempre verdadeira
/// quando `p_e1` é. Falso se a prova não se completa ou se `p_e2` pode ser falsa. Na dúvida,
/// falso. Se `p_e2` tem `i_table < 0` nos `TK_COLUMN`, assume-se a tabela `i_tab`. Os valores
/// ligados de `p_e1` são comparados com literais de `p_e2` e `expmask` é atualizada.
pub fn expr_implies_expr(
    mut pp: Option<(&mut Connection, &mut Parse)>,
    p_e1: Option<&Expr>,
    p_e2: Option<&Expr>,
    i_tab: i32,
) -> i32 {
    if expr_compare(reborrow_pp(&mut pp), p_e1, p_e2, i_tab) == 0 {
        return 1;
    }
    let Some(e2) = p_e2 else {
        return 0;
    };
    if e2.op == TK_OR
        && (expr_implies_expr(reborrow_pp(&mut pp), p_e1, e2.p_left.as_deref(), i_tab) != 0
            || expr_implies_expr(reborrow_pp(&mut pp), p_e1, e2.p_right.as_deref(), i_tab) != 0)
    {
        return 1;
    }
    if e2.op == TK_NOTNULL
        && expr_implies_not_null(reborrow_pp(&mut pp), p_e1, e2.p_left.as_deref(), i_tab, 0) != 0
    {
        return 1;
    }
    0
}

/// `bothImplyNotNullRow`: só põe `e_code` em 1 se as duas expressões, separadamente, têm a
/// propriedade de implicar linha não nula.
fn both_imply_not_null_row(w: &mut Walker<i32>, p_e1: Option<&mut Expr>, p_e2: Option<&mut Expr>) {
    if w.e_code == 0 {
        walk_expr(w, p_e1);
        if w.e_code != 0 {
            w.e_code = 0;
            walk_expr(w, p_e2);
        }
    }
}

/// `impliesNotNullRow`: callback do walker de `sqlite3ExprImpliesNonNullRow`. Se o nó exige que a
/// tabela `w.u` (o `u.iCur`) tenha uma coluna não NULL, põe `e_code` em 1 e aborta.
/// `m_w_flags` diferente de zero indica um RIGHT (ou FULL) JOIN. Falsos positivos são fatais;
/// falsos negativos são só uma otimização perdida.
fn implies_not_null_row(w: &mut Walker<i32>, p_expr: &mut Expr) -> i32 {
    if p_expr.has_property(EP_OUTER_ON) {
        return WRC_PRUNE;
    }
    if p_expr.has_property(EP_INNER_ON) && w.m_w_flags != 0 {
        // O uso de `iCur` num ON de junção interna à esquerda de um RIGHT JOIN não prova que a
        // tabela é não nula; qualquer uso vindo de junção interna é ignorado.
        return WRC_PRUNE;
    }
    match p_expr.op {
        TK_ISNOT | TK_ISNULL | TK_NOTNULL | TK_IS | TK_VECTOR | TK_FUNCTION | TK_TRUTH
        | TK_CASE => WRC_PRUNE,
        TK_COLUMN => {
            if w.u == p_expr.i_table {
                w.e_code = 1;
                WRC_ABORT
            } else {
                WRC_PRUNE
            }
        }
        TK_OR | TK_AND => {
            // Os dois lados precisam, separadamente, implicar linha não nula.
            both_imply_not_null_row(w, p_expr.p_left.as_deref_mut(), p_expr.p_right.as_deref_mut());
            WRC_PRUNE
        }
        TK_IN => {
            // Cuidado com "x NOT IN ()" e "x NOT IN (SELECT 1 WHERE false)", que podem ser
            // verdadeiros; fora isso, se o lado esquerdo é NULL o IN é NULL.
            if p_expr.use_x_list() && p_expr.x_list().is_some_and(|l| !l.a.is_empty()) {
                walk_expr(w, p_expr.p_left.as_deref_mut());
            }
            WRC_PRUNE
        }
        TK_BETWEEN => {
            // Em "x NOT BETWEEN y AND z" ou x é linha não nula ou y e z são.
            debug_assert!(p_expr.use_x_list());
            walk_expr(w, p_expr.p_left.as_deref_mut());
            let (e0, e1) = match p_expr.x_list_mut() {
                Some(l) => {
                    let n_head = 1.min(l.a.len());
                    let (head, tail) = l.a.split_at_mut(n_head);
                    (
                        head.first_mut().and_then(|i| i.p_expr.as_deref_mut()),
                        tail.first_mut().and_then(|i| i.p_expr.as_deref_mut()),
                    )
                }
                None => (None, None),
            };
            both_imply_not_null_row(w, e0, e1);
            WRC_PRUNE
        }
        // Tabelas virtuais aceitam restrições como x=NULL: um termo x=y não prova que y é não
        // nulo se x é coluna de tabela virtual.
        TK_EQ | TK_NE | TK_LT | TK_LE | TK_GT | TK_GE => {
            let l_virtual = p_expr.p_left.as_deref().is_some_and(|e| e.is_vtab(None));
            let r_virtual = p_expr.p_right.as_deref().is_some_and(|e| e.is_vtab(None));
            if l_virtual || r_virtual {
                WRC_PRUNE
            } else {
                WRC_CONTINUE
            }
        }
        _ => WRC_CONTINUE,
    }
}

/// `sqlite3ExprImpliesNonNullRow`: verdadeiro se `p` só pode ser verdadeira quando ao menos uma
/// coluna da tabela `i_tab` é não nula. Falsos negativos são aceitáveis; falsos positivos não.
/// Termos marcados `EP_OuterON` ficam fora da análise. Serve para decidir se um LEFT JOIN vira
/// junção comum.
///
/// O walker exige `&mut Expr` (o C não altera nada); como a entrada é imutável, o trecho a
/// percorrer é copiado uma vez.
pub fn expr_implies_non_null_row(p: Option<&Expr>, i_tab: i32, is_rj: i32) -> i32 {
    let Some(mut p) = expr_skip_collate_and_likely(p) else {
        return 0;
    };
    if p.op == TK_NOTNULL {
        match p.p_left.as_deref() {
            Some(l) => p = l,
            None => return 0,
        }
    } else {
        while p.op == TK_AND {
            if expr_implies_non_null_row(p.p_left.as_deref(), i_tab, is_rj) != 0 {
                return 1;
            }
            match p.p_right.as_deref() {
                Some(r) => p = r,
                None => return 0,
            }
        }
    }
    let mut copy = p.clone();
    let mut w: Walker<i32> = Walker::default();
    w.x_expr_callback = Some(implies_not_null_row);
    w.e_code = 0;
    w.m_w_flags = (is_rj != 0) as u16;
    w.u = i_tab;
    walk_expr(&mut w, Some(&mut copy));
    w.e_code as i32
}

// ---------------------------------------------------------------------------------------------
// Cobertura por índice (chunk 015)
// ---------------------------------------------------------------------------------------------

/// `struct IdxCover`: o índice a testar e o cursor da tabela correspondente.
struct IdxCover<'a> {
    /// O índice cuja cobertura se testa.
    p_idx: &'a Index,
    /// Cursor da tabela do índice.
    i_cur: i32,
}

/// `exprIdxCover`: verifica se há referência a colunas da tabela de `i_cur` que o índice não
/// consegue satisfazer.
fn expr_idx_cover(w: &mut Walker<IdxCover<'_>>, p_expr: &mut Expr) -> i32 {
    if p_expr.op == TK_COLUMN
        && p_expr.i_table == w.u.i_cur
        && table_column_to_index(w.u.p_idx, p_expr.i_column as i16) < 0
    {
        w.e_code = 1;
        return WRC_ABORT;
    }
    WRC_CONTINUE
}

/// `sqlite3ExprCoveredByIndex`: verdadeiro se o índice `p_idx` da tabela de cursor `i_cur`
/// cobre `p_expr`, isto é, se a expressão se avalia só com o índice, sem consultar a tabela.
pub fn expr_covered_by_index(p_expr: &mut Expr, i_cur: i32, p_idx: &Index) -> i32 {
    let mut w: Walker<IdxCover<'_>> = Walker {
        x_expr_callback: Some(expr_idx_cover),
        x_select_callback: None,
        x_select_callback2: None,
        walker_depth: 0,
        e_code: 0,
        m_w_flags: 0,
        u: IdxCover { p_idx, i_cur },
    };
    walk_expr(&mut w, Some(p_expr));
    (w.e_code == 0) as i32
}

// ---------------------------------------------------------------------------------------------
// sqlite3ReferencesSrcList (chunk 016)
// ---------------------------------------------------------------------------------------------

/// `struct RefSrcList`: o que passa pelo walker de `sqlite3ReferencesSrcList`. `nExclude` é
/// `exclude.len()`; `db` some (só servia de alocador).
struct RefSrcList<'a> {
    /// Procura referências a estas tabelas.
    p_ref: Option<&'a SrcList>,
    /// Cursores das tabelas a excluir da busca.
    exclude: Vec<i32>,
}

/// `selectRefEnter`: ao entrar numa subconsulta, põe as entradas do FROM dela na lista de
/// exclusão.
fn select_ref_enter(w: &mut Walker<RefSrcList<'_>>, p_select: &mut Select) -> i32 {
    let Some(p_src) = p_select.p_src.as_deref() else {
        return WRC_CONTINUE;
    };
    if p_src.a.is_empty() {
        return WRC_CONTINUE;
    }
    w.u.exclude.extend(p_src.a.iter().map(|it| it.i_cursor));
    WRC_CONTINUE
}

/// `selectRefLeave`: ao sair da subconsulta, tira da lista de exclusão as entradas dela.
fn select_ref_leave(w: &mut Walker<RefSrcList<'_>>, p_select: &mut Select) {
    let n_src = p_select.p_src.as_deref().map_or(0, |s| s.a.len());
    let p = &mut w.u;
    if !p.exclude.is_empty() {
        debug_assert!(p.exclude.len() >= n_src);
        let n = p.exclude.len() - n_src.min(p.exclude.len());
        p.exclude.truncate(n);
    }
}

/// `exprRefToSrcList`: callback de expressão. Liga o bit 0x01 de `e_code` se há referência a
/// alguma tabela de `p_ref`, e o bit 0x02 se há referência a tabela que não está nem em `p_ref`
/// nem na lista de exclusão.
fn expr_ref_to_src_list(w: &mut Walker<RefSrcList<'_>>, p_expr: &mut Expr) -> i32 {
    if p_expr.op == TK_COLUMN || p_expr.op == TK_AGG_COLUMN {
        if let Some(p_src) = w.u.p_ref {
            if p_src.a.iter().any(|it| it.i_cursor == p_expr.i_table) {
                w.e_code |= 1;
                return WRC_CONTINUE;
            }
        }
        if !w.u.exclude.contains(&p_expr.i_table) {
            w.e_code |= 2;
        }
    }
    WRC_CONTINUE
}

/// `sqlite3ReferencesSrcList`: verifica se `p_expr` (sempre uma chamada de função agregada)
/// referencia tabelas de `p_src_list`. Devolve 1 se referencia; 0 se referencia alguma tabela que
/// não está em `p_src_list` nem nas subconsultas de `p_expr`; -1 se não referencia tabela alguma
/// ou só as definidas em subconsultas de `p_expr`.
///
/// `db` e `parse` somem (o C só os usava para realocar o vetor de exclusão); o walker exige
/// `&mut Expr` embora nada seja alterado.
pub fn references_src_list(p_expr: &mut Expr, p_src_list: Option<&SrcList>) -> i32 {
    let mut w: Walker<RefSrcList<'_>> = Walker {
        x_expr_callback: Some(expr_ref_to_src_list),
        x_select_callback: Some(select_ref_enter),
        x_select_callback2: Some(select_ref_leave),
        walker_depth: 0,
        e_code: 0,
        m_w_flags: 0,
        u: RefSrcList { p_ref: p_src_list, exclude: Vec::new() },
    };
    debug_assert!(p_expr.op == TK_AGG_FUNCTION);
    debug_assert!(p_expr.use_x_list());
    walk_expr_list(&mut w, p_expr.x_list_mut());
    if let Some(l) = p_expr.p_left.as_deref_mut() {
        debug_assert!(l.op == TK_ORDER);
        debug_assert!(l.use_x_list());
        walk_expr_list(&mut w, l.x_list_mut());
    }
    if p_expr.has_property(EP_WIN_FUNC) {
        if let Some(win) = p_expr.y_win_mut() {
            walk_expr(&mut w, win.p_filter.as_deref_mut());
        }
    }
    if (w.e_code & 0x01) != 0 {
        1
    } else if w.e_code != 0 {
        0
    } else {
        -1
    }
}

// ---------------------------------------------------------------------------------------------
// AggInfo (chunks 016 e 017)
// ---------------------------------------------------------------------------------------------

/// `sqlite3AggInfoPersistWalkerInit`: prepara o walker que persistiria as entradas de `AggInfo`
/// referenciadas pela árvore. O callback do C (`agginfoPersistExprCb`) troca por cópia o nó para o
/// qual o `AggInfo` aponta diretamente; aqui o `AggInfo` já guarda cópias, então o callback é o
/// percurso vazio (`expr_walk_noop`) e o de SELECT é `select_walk_noop`.
pub fn agg_info_persist_walker_init<C>(w: &mut Walker<C>) {
    w.x_expr_callback = Some(expr_walk_noop::<C>);
    w.x_select_callback = Some(select_walk_noop::<C>);
    w.x_select_callback2 = None;
    w.walker_depth = 0;
    w.e_code = 0;
    w.m_w_flags = 0;
}

/// `findOrCreateAggInfoColumn`: procura em `aCol[]` a entrada com `i_table` e `i_column` de
/// `p_expr`; se não há, cria uma. A expressão é ajustada para apontar para a entrada
/// (`TK_COLUMN` vira `TK_AGG_COLUMN`).
///
/// A identidade `pCol->pCExpr==pExpr` do C vira "o nó já aponta para esta entrada" (mesmo
/// `p_agg_info` e `i_agg` igual ao índice), porque `p_c_expr` é uma cópia.
fn find_or_create_agg_info_column(parse: &mut Parse, agg_id: AggInfoId, p_expr: &mut Expr) {
    let Some(info) = parse.agg_infos.get_mut(agg_id.0 as usize) else {
        debug_assert!(false);
        return;
    };
    debug_assert!(info.i_first_reg == 0);
    let mut found: Option<usize> = None;
    for (idx, col) in info.a_col.iter().enumerate() {
        if p_expr.p_agg_info == Some(agg_id) && p_expr.i_agg as usize == idx && col.p_c_expr.is_some()
        {
            return;
        }
        if col.i_table == p_expr.i_table
            && col.i_column as i32 == p_expr.i_column
            && p_expr.op != TK_IF_NULL_ROW
        {
            found = Some(idx);
            break;
        }
    }
    let (k, is_new) = match found {
        Some(k) => (k, false),
        None => {
            debug_assert!(p_expr.use_y_tab());
            let mut col = AggInfoCol {
                p_tab: match p_expr.y_tab() {
                    Some(TabRef::Rc(t)) => Some(t.clone()),
                    _ => None,
                },
                i_table: p_expr.i_table,
                i_column: p_expr.i_column as i16,
                i_sorter_column: -1,
                ..AggInfoCol::default()
            };
            if p_expr.op != TK_IF_NULL_ROW {
                if let Some(gb) = info.p_group_by.as_deref() {
                    for (j, term) in gb.a.iter().enumerate() {
                        if let Some(e) = term.p_expr.as_deref() {
                            if e.op == TK_COLUMN
                                && e.i_table == p_expr.i_table
                                && e.i_column == p_expr.i_column
                            {
                                col.i_sorter_column = j as i16;
                                break;
                            }
                        }
                    }
                }
            }
            if col.i_sorter_column < 0 {
                col.i_sorter_column = info.n_sorting_column as i16;
                info.n_sorting_column += 1;
            }
            info.a_col.push(col);
            (info.a_col.len() - 1, true)
        }
    };
    // fix_up_expr:
    p_expr.p_agg_info = Some(agg_id);
    if p_expr.op == TK_COLUMN {
        p_expr.op = TK_AGG_COLUMN;
    }
    p_expr.i_agg = k as i16;
    if is_new {
        // `pCol->pCExpr = pExpr`: a cópia leva o estado já ajustado.
        info.a_col[k].p_c_expr = expr_dup(Some(&*p_expr), 0);
    }
}

/// O contexto do walker de `sqlite3ExprAnalyzeAggregates`: o `pNC` do C (sem o `pParse`, que
/// vem à parte), com a conexão e o `Parse` emprestados.
struct AnalyzeAggCtx<'a> {
    /// A conexão (para `sqlite3FindFunction`).
    db: &'a mut Connection,
    /// O `pNC->pParse`.
    parse: &'a mut Parse,
    /// `pNC->pSrcList`.
    p_src_list: Option<&'a mut SrcList>,
    /// `pNC->uNC.pAggInfo`.
    agg_id: AggInfoId,
    /// `pNC->ncFlags`.
    nc_flags: i32,
}

/// `analyzeAggregate`: callback de expressão de `sqlite3ExprAnalyzeAggregates`.
fn analyze_aggregate(w: &mut Walker<AnalyzeAggCtx<'_>>, p_expr: &mut Expr) -> i32 {
    let walker_depth = w.walker_depth;
    let ctx = &mut w.u;
    let agg_id = ctx.agg_id;
    match p_expr.op {
        TK_IF_NULL_ROW | TK_AGG_COLUMN | TK_COLUMN => {
            // Vê se a coluna está numa das tabelas do FROM da consulta agregada.
            if let Some(src) = ctx.p_src_list.as_deref() {
                for item in src.a.iter() {
                    debug_assert!(!p_expr.has_property(EP_TOKEN_ONLY | EP_REDUCED));
                    if p_expr.i_table == item.i_cursor {
                        find_or_create_agg_info_column(ctx.parse, agg_id, p_expr);
                        break;
                    }
                }
            }
            WRC_CONTINUE
        }
        TK_AGG_FUNCTION => {
            if (ctx.nc_flags & NC_INAGGFUNC) == 0
                && walker_depth == p_expr.op2 as i32
                && p_expr.p_agg_info.is_none()
            {
                // Vê se é duplicata de outra agregada que já está no `AggInfo`.
                let found = ctx.parse.agg_infos.get(agg_id.0 as usize).and_then(|info| {
                    info.a_func.iter().position(|f| {
                        expr_compare(None, f.p_f_expr.as_deref(), Some(&*p_expr), -1) == 0
                    })
                });
                let i = match found {
                    Some(i) => i,
                    None => {
                        // `pExpr` é original: nova entrada em `aFunc[]`.
                        let Some(info) = ctx.parse.agg_infos.get(agg_id.0 as usize) else {
                            debug_assert!(false);
                            return WRC_CONTINUE;
                        };
                        let i = info.a_func.len();
                        let enc = ctx.db.enc;
                        debug_assert!(!p_expr.has_property(EP_X_IS_SELECT));
                        debug_assert!(p_expr.use_u_token());
                        let n_arg = p_expr.x_list().map_or(0, |l| l.a.len()) as i32;
                        let z_name = p_expr.z_token().unwrap_or(&[]).to_vec();
                        let p_func = find_function(ctx.db, &z_name, n_arg, enc, 0);
                        // O nó aponta para a entrada antes da cópia, que `pFExpr` do C
                        // (o próprio nó) enxerga já ajustado.
                        p_expr.i_agg = i as i16;
                        p_expr.p_agg_info = Some(agg_id);
                        let mut item = AggInfoFunc {
                            p_f_expr: expr_dup(Some(&*p_expr), 0),
                            p_func: p_func.clone(),
                            ..AggInfoFunc::default()
                        };
                        let func_flags = p_func.as_ref().map_or(0, |f| f.func_flags);
                        if p_expr.p_left.is_some() && (func_flags & SQLITE_FUNC_NEEDCOLL) == 0 {
                            // O teste de NEEDCOLL faz com que o ORDER BY de min() e max() seja
                            // ignorado.
                            debug_assert!(n_arg > 0);
                            item.i_ob_tab = ctx.parse.n_tab;
                            ctx.parse.n_tab += 1;
                            let p_ob_list = p_expr.p_left.as_deref().and_then(|l| l.x_list());
                            debug_assert!(p_ob_list.is_some_and(|l| !l.a.is_empty()));
                            let same_single = p_ob_list.is_some_and(|ob| {
                                ob.a.len() == 1
                                    && n_arg == 1
                                    && expr_compare(
                                        None,
                                        ob.a[0].p_expr.as_deref(),
                                        p_expr
                                            .x_list()
                                            .and_then(|l| l.a.first())
                                            .and_then(|it| it.p_expr.as_deref()),
                                        0,
                                    ) == 0
                            });
                            if same_single {
                                item.b_ob_payload = 0;
                                item.b_ob_unique = p_expr.has_property(EP_DISTINCT) as u8;
                            } else {
                                item.b_ob_payload = 1;
                            }
                            item.b_use_subtype = ((func_flags & SQLITE_SUBTYPE as u32) != 0) as u8;
                        } else {
                            item.i_ob_tab = -1;
                        }
                        if p_expr.has_property(EP_DISTINCT) && item.b_ob_unique == 0 {
                            item.i_distinct = ctx.parse.n_tab;
                            ctx.parse.n_tab += 1;
                        } else {
                            item.i_distinct = -1;
                        }
                        if let Some(info) = ctx.parse.agg_infos.get_mut(agg_id.0 as usize) {
                            info.a_func.push(item);
                        }
                        i
                    }
                };
                // Faz `pExpr` apontar para a entrada certa de `aFunc[]`.
                debug_assert!(!p_expr.has_property(EP_TOKEN_ONLY | EP_REDUCED));
                p_expr.i_agg = i as i16;
                p_expr.p_agg_info = Some(agg_id);
                WRC_PRUNE
            } else {
                WRC_CONTINUE
            }
        }
        _ => {
            debug_assert!(ctx.parse.i_self_tab == 0);
            if (ctx.nc_flags & NC_INAGGFUNC) == 0 {
                return WRC_CONTINUE;
            }
            if ctx.parse.p_idx_epr.is_empty() {
                return WRC_CONTINUE;
            }
            // A lista do C é percorrida do mais novo (o último do `Vec`) para o mais antigo.
            let mut hit: Option<(i32, i32, i32)> = None;
            for ie in ctx.parse.p_idx_epr.iter().rev() {
                let i_data_cur = ie.i_data_cur;
                if i_data_cur < 0 {
                    continue;
                }
                if expr_compare(None, Some(&*p_expr), ie.p_expr.as_deref(), i_data_cur) == 0 {
                    hit = Some((i_data_cur, ie.i_idx_cur, ie.i_idx_col));
                    break;
                }
            }
            let Some((i_data_cur, i_idx_cur, i_idx_col)) = hit else {
                return WRC_CONTINUE;
            };
            if !p_expr.use_y_tab() {
                return WRC_CONTINUE;
            }
            // O C compara sempre `a[0].iCursor` (e não `a[i]`), e isso se mantém.
            let in_src = ctx.p_src_list.as_deref().is_some_and(|s| {
                !s.a.is_empty() && s.a[0].i_cursor == i_data_cur
            });
            if !in_src {
                return WRC_CONTINUE;
            }
            if p_expr.p_agg_info.is_some() {
                // Resolvida por um contexto externo.
                return WRC_CONTINUE;
            }
            if ctx.parse.n_err != 0 {
                return WRC_ABORT;
            }
            // A expressão pode virar uma referência a uma coluna do índice.
            let mut tmp = Expr {
                op: TK_AGG_COLUMN,
                i_table: i_idx_cur,
                i_column: i_idx_col,
                ..Expr::default()
            };
            find_or_create_agg_info_column(ctx.parse, agg_id, &mut tmp);
            if ctx.parse.n_err != 0 {
                return WRC_ABORT;
            }
            debug_assert!((tmp.i_agg as usize) < ctx.parse.agg_infos[agg_id.0 as usize].a_col.len());
            p_expr.p_agg_info = Some(agg_id);
            p_expr.i_agg = tmp.i_agg;
            // `pCExpr = pExpr`: a cópia leva o estado já ajustado.
            let copy = expr_dup(Some(&*p_expr), 0);
            ctx.parse.agg_infos[agg_id.0 as usize].a_col[tmp.i_agg as usize].p_c_expr = copy;
            WRC_PRUNE
        }
    }
}

/// `sqlite3ExprAnalyzeAggregates`: analisa `p_expr` atrás de funções agregadas e de variáveis
/// que precisam entrar no `AggInfo` de `nc` (`NC_UAggInfo`). Só deve ser chamada depois de
/// `sqlite3ResolveExprNames()`. O `pNC->pParse` do C é o `parse` explícito.
pub fn expr_analyze_aggregates(
    db: &mut Connection,
    parse: &mut Parse,
    nc: &mut NameContext<'_>,
    p_expr: Option<&mut Expr>,
) {
    debug_assert!(nc.p_src_list.is_some());
    debug_assert!((nc.nc_flags & crate::consts::NC_UAGGINFO) != 0);
    let NcU::AggInfo(agg_id) = nc.u_nc else {
        debug_assert!(false);
        return;
    };
    let mut w: Walker<AnalyzeAggCtx<'_>> = Walker {
        x_expr_callback: Some(analyze_aggregate),
        x_select_callback: Some(walker_depth_increase),
        x_select_callback2: Some(walker_depth_decrease),
        walker_depth: 0,
        e_code: 0,
        m_w_flags: 0,
        u: AnalyzeAggCtx {
            db,
            parse,
            p_src_list: nc.p_src_list.as_deref_mut(),
            agg_id,
            nc_flags: nc.nc_flags,
        },
    };
    walk_expr(&mut w, p_expr);
}

/// `sqlite3ExprAnalyzeAggList`: chama [`expr_analyze_aggregates`] para cada expressão da lista.
pub fn expr_analyze_agg_list(
    db: &mut Connection,
    parse: &mut Parse,
    nc: &mut NameContext<'_>,
    p_list: Option<&mut ExprList>,
) {
    if let Some(l) = p_list {
        for item in l.a.iter_mut() {
            expr_analyze_aggregates(db, parse, nc, item.p_expr.as_deref_mut());
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Registradores temporários (chunk 017)
// ---------------------------------------------------------------------------------------------

/// `sqlite3GetTempReg`: aloca um registrador para um resultado intermediário.
pub fn get_temp_reg(parse: &mut Parse) -> i32 {
    if parse.n_temp_reg == 0 {
        parse.n_mem += 1;
        return parse.n_mem;
    }
    parse.n_temp_reg -= 1;
    parse.a_temp_reg[parse.n_temp_reg as usize]
}

/// `sqlite3ReleaseTempReg`: devolve um registrador para reaproveitamento.
pub fn release_temp_reg(parse: &mut Parse, i_reg: i32) {
    if i_reg != 0 && (parse.n_temp_reg as usize) < parse.a_temp_reg.len() {
        parse.a_temp_reg[parse.n_temp_reg as usize] = i_reg;
        parse.n_temp_reg += 1;
    }
}

/// `sqlite3GetTempRange`: aloca um bloco de `n_reg` registradores consecutivos.
pub fn get_temp_range(parse: &mut Parse, n_reg: i32) -> i32 {
    if n_reg == 1 {
        return get_temp_reg(parse);
    }
    let mut i = parse.i_range_reg;
    let n = parse.n_range_reg;
    if n_reg <= n {
        parse.i_range_reg += n_reg;
        parse.n_range_reg -= n_reg;
    } else {
        i = parse.n_mem + 1;
        parse.n_mem += n_reg;
    }
    i
}

/// `sqlite3ReleaseTempRange`: devolve um bloco de registradores.
pub fn release_temp_range(parse: &mut Parse, i_reg: i32, n_reg: i32) {
    if n_reg == 1 {
        release_temp_reg(parse, i_reg);
        return;
    }
    if n_reg > parse.n_range_reg {
        parse.n_range_reg = n_reg;
        parse.i_range_reg = i_reg;
    }
}

/// `sqlite3ClearTempRegCache`: marca todos os registradores temporários como indisponíveis. Deve
/// ser chamada depois de codificar uma sub-rotina ou co-rotina que outras partes do código
/// possam invocar, para que ela não compartilhe registradores com quem a invoca.
pub fn clear_temp_reg_cache(parse: &mut Parse) {
    parse.n_temp_reg = 0;
    parse.n_range_reg = 0;
}

/// `sqlite3TouchRegister`: garante que `i_reg` é um número de registrador válido.
pub fn touch_register(parse: &mut Parse, i_reg: i32) {
    if parse.n_mem < i_reg {
        parse.n_mem = i_reg;
    }
}
