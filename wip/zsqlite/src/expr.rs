//! Tradução de `expr.c`, primeira metade (chunks `expr_c.000` a `expr_c.005`): afinidade, colação,
//! comparação de vetores, altura de expressões, alocação de nós, listas de expressões, duplicação
//! (`exprDup`, `sqlite3ExprListDup`, `sqlite3SrcListDup`, `sqlite3SelectDup`...) e as rotinas de
//! `ExprList`. A segunda metade do arquivo (geração de código) é de outra fatia.
//!
//! Convenções desta fatia (modelo v2, ver CONVENTIONS.md):
//!
//! - `sqlite3 *db` que só servia de alocador some. Onde o C lê `db` para outra coisa (limites,
//!   `flags`, `enc`, mensagens de erro), a função recebe `db: &mut Connection` e depois
//!   `parse: &mut Parse`. `Parse *` que só carregava o `db` também some.
//! - `sqlite3ExprDelete`, `sqlite3ExprListDelete`, `sqlite3ExprDeleteGeneric`,
//!   `sqlite3ClearOnOrUsing` e `sqlite3SelectDelete` não existem: a posse em árvore faz o `Drop`.
//!   `EP_Static` ainda é gravado pela duplicação reduzida, mas nada o consulta.
//! - `zToken` e `zEName` são `Vec<u8>` SEM o NUL final (depois de `dequote` o excedente é
//!   cortado no primeiro NUL). Toda leitura "de string C" usa `util::at`/`strlen30`, que tratam o
//!   fim da fatia como NUL.
//! - Um `Expr` ligado a uma tabela do esquema guarda `TabRef::Own` (ver `sqlite_int.rs`). As
//!   funções que leem `y.pTab` (afinidade e colação) recebem `own`, a tabela dona da árvore.
//! - `EXPRDUP_REDUCE` é emulado: o nó reduzido perde `iTable`, `iColumn`, `iAgg`, `w`, `pAggInfo`
//!   e `y` (ficam zerados) e o TokenOnly perde também filhos e `x`, exatamente como o `memcpy`
//!   parcial do C, e as flags `EP_Reduced`/`EP_TokenOnly`/`EP_Static` são gravadas como no C.
//! - `TK_SELECT_COLUMN`: o `pLeft` do C é COMPARTILHADO entre as colunas de um mesmo vetor. Com
//!   `p_left: Option<Box<Expr>>` cada nó guarda uma cópia (`Expr::clone`). Ver o relatório.

// Fachada do mesmo arquivo C dividido em módulos: os chamadores importam de `crate::expr`.
pub use crate::expr_code::*;
pub use crate::expr_code2::*;

use std::rc::Rc;

pub(crate) use crate::vdbeaux::{vdbe_of_parse, current_addr};
use crate::alter::{rename_expr_unmap, rename_token_map};
use crate::build::{affinity_type, column_coll};
use crate::callback::{check_coll_seq, find_coll_seq, get_coll_seq};
use crate::connection::{Connection, FuncDef, Parse};
use crate::consts::{
    ENAME_SPAN, EP_COLLATE, EP_COMMUTED, EP_DISTINCT, EP_FROM_DDL,
    EP_FULL_SIZE, EP_HAS_FUNC, EP_IF_NULL_ROW, EP_INNER_ON, EP_INT_VALUE, EP_IS_FALSE, EP_IS_TRUE,
    EP_LEAF, EP_OUTER_ON, EP_PROPAGATE, EP_REDUCED, EP_SKIP, EP_STATIC, EP_SUBQUERY,
    EP_TOKEN_ONLY, EP_UNLIKELY, EP_WIN_FUNC, EP_X_IS_SELECT, EXPRDUP_REDUCE, KEYINFO_ORDER_BIGNULL,
    OP_ELSEEQ, OP_GOTO, OP_INTEGER, OP_NOT, OP_NOTNULL, OP_ZEROORNULL, SF_DISTINCT, SF_MULTIVALUE,
    SF_USESEPHEMERAL, SF_VALUES, SQLITE_AFF_BLOB, SQLITE_AFF_INTEGER, SQLITE_AFF_NONE,
    SQLITE_AFF_NUMERIC, SQLITE_AFF_TEXT, SQLITE_FUNC_DIRECT, SQLITE_LIMIT_COLUMN,
    SQLITE_LIMIT_EXPR_DEPTH, SQLITE_LIMIT_FUNCTION_ARG, SQLITE_LIMIT_VARIABLE_NUMBER, SQLITE_NULLEQ,
    SQLITE_OK, SQLITE_ERROR, SQLITE_SO_UNDEFINED, SQLITE_TRUSTED_SCHEMA, SQLITE_UTF8, TK_AGG_COLUMN,
    TK_AGG_FUNCTION, TK_ALL, TK_AND, TK_BLOB, TK_CASE, TK_CAST, TK_COLLATE, TK_COLUMN, TK_CONCAT,
    TK_EQ, TK_FUNCTION, TK_GE, TK_GT, TK_IF_NULL_ROW, TK_INTEGER, TK_LE, TK_LT, TK_NE, TK_NULL,
    TK_ORDER, TK_REGISTER, TK_SELECT, TK_SELECT_COLUMN, TK_STRING, TK_TRIGGER, TK_UPLUS,
    TK_VARIABLE, TK_VECTOR, WRC_CONTINUE, WRC_PRUNE,
};
use crate::ctype::is_quote;
use crate::mem::CollSeq;
use crate::printf::{PrintfArg, PrintfExprToken, PrintfToken};
use crate::select::select_new;
use crate::sqlite_int::{
    is_numeric_affinity, Cte, Expr, ExprList, ExprListItem, ExprU, ExprX, ExprY, IdList,
    IdListItem, Select, SrcItem, SrcList, SrcU1, SrcU3, TabRef, Table, Token, Walker, With,
};
use crate::util::{
    at, atoi64, dequote, dequote_expr, error_msg, get_int32, record_error_offset_of_expr, str_icmp,
    strlen30,
};
use crate::vdbe_types::{VListEntry, Vdbe, P4};
use crate::vdbeaux::{
    add_op0, add_op2, add_op3, add_op4, change_p5,
    jump_here, make_label, resolve_label,
};
use crate::walker::walk_select;
use crate::window::{window_dup, window_link, window_list_dup};

// ---------------------------------------------------------------------------------------------
// Auxiliares privados (não existem no C)
// ---------------------------------------------------------------------------------------------


/// A tabela a que um `TK_COLUMN`/`TK_AGG_COLUMN`/`TK_TRIGGER` se refere (`y.pTab`); `own` é a
/// tabela dona da árvore, para o caso `TabRef::Own`.
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

/// `pList->a[0].pExpr`.
fn list_first_expr(l: &ExprList) -> Option<&Expr> {
    l.a.first()?.p_expr.as_deref()
}

/// O argumento `%T` do printf para um `Token` (o deslocamento é relativo a `Parse.zTail`).
fn token_arg(parse: &Parse, t: &Token) -> PrintfArg {
    let tail = parse.z_tail as i32;
    let tail_offset = if t.i_ofst >= tail { Some(t.i_ofst - tail) } else { None };
    PrintfArg::Token(Some(PrintfToken { z: t.z.clone(), tail_offset }))
}

/// O primeiro `iOfst > 0` descendo por `pLeft` (o que `sqlite3RecordErrorOffsetOfExpr` grava).
fn expr_error_offset(e: &Expr) -> Option<i32> {
    let mut p = Some(e);
    while let Some(x) = p {
        if x.has_property(EP_OUTER_ON | EP_INNER_ON) || x.w <= 0 {
            p = x.p_left.as_deref();
        } else {
            return Some(x.w);
        }
    }
    None
}

/// O argumento `%#T` do printf para uma expressão (`EP_IntValue` vira o ponteiro nulo).
pub(crate) fn expr_token_arg(e: &Expr) -> PrintfArg {
    match e.z_token() {
        Some(z) if e.use_u_token() => PrintfArg::ExprToken(Some(PrintfExprToken {
            z_token: z.to_vec(),
            err_offset: expr_error_offset(e),
        })),
        _ => PrintfArg::ExprToken(None),
    }
}

/// `sqlite3VListNumToName` sobre `Parse.p_v_list`.
fn vlist_entry_name(list: &[VListEntry], i_var: i32) -> Option<&[u8]> {
    list.iter().find(|e| e.i_var == i_var).map(|e| e.name.as_slice())
}

/// `sqlite3VListNameToNum` sobre `Parse.p_v_list` (zero se o nome não existe).
fn vlist_entry_number(list: &[VListEntry], name: &[u8]) -> i32 {
    list.iter().find(|e| e.name.as_slice() == name).map_or(0, |e| e.i_var)
}

// ---------------------------------------------------------------------------------------------
// Afinidade e colação
// ---------------------------------------------------------------------------------------------

/// `sqlite3TableColumnAffinity`: a afinidade de uma coluna de tabela.
pub fn table_column_affinity(tab: &Table, i_col: i32) -> u8 {
    if i_col < 0 || i_col >= tab.n_col as i32 {
        return SQLITE_AFF_INTEGER;
    }
    tab.a_col.get(i_col as usize).map_or(SQLITE_AFF_INTEGER, |c| c.affinity)
}

/// `sqlite3ExprAffinity`: a afinidade da expressão, ou 0x00 se não tem.
///
/// Se é uma coluna, uma referência a coluna por alias `AS` ou uma subconsulta que devolve uma
/// coluna, vale a afinidade da coluna.
pub fn expr_affinity(p_expr: &Expr, own: Option<&Rc<Table>>) -> u8 {
    let mut p = p_expr;
    let mut op = p.op;
    loop {
        if op == TK_COLUMN || (op == TK_AGG_COLUMN && p.y_tab().is_some()) {
            if let Some(t) = table_of(p, own) {
                return table_column_affinity(t, p.i_column);
            }
        }
        if op == TK_SELECT {
            if let Some(e) = p.x_select().and_then(|s| select_result_expr(s, 0)) {
                return expr_affinity(e, own);
            }
        }
        if op == TK_CAST {
            return affinity_type(p.z_token().unwrap_or(&[]), None);
        }
        if op == TK_SELECT_COLUMN {
            let e = p
                .p_left
                .as_deref()
                .and_then(|l| l.x_select())
                .and_then(|s| select_result_expr(s, p.i_column as usize));
            if let Some(e) = e {
                return expr_affinity(e, own);
            }
        }
        if op == TK_VECTOR {
            if let Some(e) = p.x_list().and_then(list_first_expr) {
                return expr_affinity(e, own);
            }
        }
        if p.has_property(EP_SKIP | EP_IF_NULL_ROW) {
            if let Some(l) = p.p_left.as_deref() {
                p = l;
                op = p.op;
                continue;
            }
        }
        if op != TK_REGISTER {
            break;
        }
        op = p.op2;
        if op == TK_REGISTER {
            break;
        }
    }
    p.aff_expr
}

/// `sqlite3ExprDataType`: um palpite sobre todos os tipos possíveis do resultado, em máscara:
/// 0x01 numérico, 0x02 texto, 0x04 blob. Zero se a expressão só pode ser NULL.
pub fn expr_data_type(p_expr: Option<&Expr>, own: Option<&Rc<Table>>) -> i32 {
    let mut cur = p_expr;
    while let Some(e) = cur {
        match e.op {
            TK_COLLATE | TK_IF_NULL_ROW | TK_UPLUS => {
                cur = e.p_left.as_deref();
            }
            TK_NULL => {
                cur = None;
            }
            TK_STRING => return 0x02,
            TK_BLOB => return 0x04,
            TK_CONCAT => return 0x06,
            TK_VARIABLE | TK_AGG_FUNCTION | TK_FUNCTION => return 0x07,
            TK_COLUMN | TK_AGG_COLUMN | TK_SELECT | TK_CAST | TK_SELECT_COLUMN | TK_VECTOR => {
                let aff = expr_affinity(e, own);
                if aff >= SQLITE_AFF_NUMERIC {
                    return 0x05;
                }
                if aff == SQLITE_AFF_TEXT {
                    return 0x06;
                }
                return 0x07;
            }
            TK_CASE => {
                let mut res = 0;
                if let Some(list) = e.x_list() {
                    let n = list.a.len();
                    let mut ii = 1;
                    while ii < n {
                        res |= expr_data_type(list.a[ii].p_expr.as_deref(), own);
                        ii += 2;
                    }
                    if n % 2 != 0 {
                        res |= expr_data_type(list.a[n - 1].p_expr.as_deref(), own);
                    }
                }
                return res;
            }
            _ => return 0x01,
        }
    }
    0x00
}

/// `sqlite3ExprAddCollateToken`: um novo nó `TK_COLLATE` que implementa o operador COLLATE.
pub fn expr_add_collate_token(
    p_expr: Option<Box<Expr>>,
    p_coll_name: &Token,
    dequote: i32,
) -> Option<Box<Expr>> {
    if !p_coll_name.z.is_empty() {
        if let Some(mut p_new) = expr_alloc(TK_COLLATE as i32, Some(p_coll_name), dequote) {
            p_new.p_left = p_expr;
            p_new.flags |= EP_COLLATE | EP_SKIP;
            return Some(p_new);
        }
    }
    p_expr
}

/// `sqlite3ExprAddCollateString`.
pub fn expr_add_collate_string(p_expr: Option<Box<Expr>>, z_c: &[u8]) -> Option<Box<Expr>> {
    let s = Token { z: z_c[..strlen30(z_c) as usize].to_vec(), i_ofst: -1 };
    expr_add_collate_token(p_expr, &s, 0)
}

/// `sqlite3ExprSkipCollate`: pula os operadores `TK_COLLATE`.
pub fn expr_skip_collate(p_expr: Option<&Expr>) -> Option<&Expr> {
    let mut p = p_expr;
    while let Some(e) = p {
        if !e.has_property(EP_SKIP) {
            break;
        }
        p = e.p_left.as_deref();
    }
    p
}

/// `sqlite3ExprSkipCollate` sobre um nó mutável: o C devolve o mesmo ponteiro e o chamador o
/// altera no lugar.
pub fn expr_skip_collate_mut(mut e: &mut Expr) -> &mut Expr {
    while e.has_property(EP_SKIP) && e.p_left.is_some() {
        e = e.p_left.as_deref_mut().expect("p_left");
    }
    e
}

/// `sqlite3ExprSkipCollateAndLikely`: pula `TK_COLLATE` e as funções `unlikely()`,
/// `likelihood()` e `likely()` na raiz da expressão.
pub fn expr_skip_collate_and_likely(p_expr: Option<&Expr>) -> Option<&Expr> {
    let mut p = p_expr;
    while let Some(e) = p {
        if !e.has_property(EP_SKIP | EP_UNLIKELY) {
            break;
        }
        if e.has_property(EP_UNLIKELY) {
            p = e.x_list().and_then(list_first_expr);
        } else if e.op == TK_COLLATE {
            p = e.p_left.as_deref();
        } else {
            break;
        }
    }
    p
}

/// `sqlite3ExprCollSeq`: a sequência de colação da expressão, ou `None` se não há.
///
/// Pode ser definida por um operador COLLATE ou por uma coluna com colação. COLLATE tem
/// precedência, e o operando esquerdo vence o direito.
pub fn expr_coll_seq(
    db: &mut Connection,
    parse: &mut Parse,
    p_expr: Option<&Expr>,
    own: Option<&Rc<Table>>,
) -> Option<Rc<CollSeq>> {
    let enc = db.enc;
    let mut p_coll: Option<Rc<CollSeq>> = None;
    let mut p = p_expr;
    while let Some(e) = p {
        let mut op = e.op;
        if op == TK_REGISTER {
            op = e.op2;
        }
        if (op == TK_AGG_COLUMN && e.y_tab().is_some()) || op == TK_COLUMN || op == TK_TRIGGER {
            let j = e.i_column;
            if j >= 0 {
                if let Some(t) = table_of(e, own) {
                    let z_coll = t.a_col.get(j as usize).and_then(column_coll);
                    p_coll = find_coll_seq(db, enc, z_coll, 0);
                }
            }
            break;
        }
        if op == TK_CAST || op == TK_UPLUS {
            p = e.p_left.as_deref();
            continue;
        }
        if op == TK_VECTOR {
            p = e.x_list().and_then(list_first_expr);
            continue;
        }
        if op == TK_COLLATE {
            p_coll = get_coll_seq(db, parse, enc, None, e.z_token());
            break;
        }
        if (e.flags & EP_COLLATE) != 0 {
            let left_has = e.p_left.as_deref().map_or(false, |l| (l.flags & EP_COLLATE) != 0);
            if left_has {
                p = e.p_left.as_deref();
            } else {
                let mut p_next = e.p_right.as_deref();
                if e.use_x_list() && db.malloc_failed == 0 {
                    if let Some(list) = e.x_list() {
                        for item in &list.a {
                            if let Some(x) = item.p_expr.as_deref() {
                                if x.has_property(EP_COLLATE) {
                                    p_next = Some(x);
                                    break;
                                }
                            }
                        }
                    }
                }
                p = p_next;
            }
        } else {
            break;
        }
    }
    if check_coll_seq(db, parse, p_coll.as_ref()) != 0 {
        p_coll = None;
    }
    p_coll
}

/// `sqlite3ExprNNCollSeq`: como `expr_coll_seq`, mas devolve a colação padrão se a expressão
/// não define nenhuma.
pub fn expr_nn_coll_seq(
    db: &mut Connection,
    parse: &mut Parse,
    p_expr: Option<&Expr>,
    own: Option<&Rc<Table>>,
) -> Rc<CollSeq> {
    match expr_coll_seq(db, parse, p_expr, own) {
        Some(p) => p,
        None => db.p_dflt_coll.clone().expect("db->pDfltColl"),
    }
}

/// `sqlite3ExprCollSeqMatch`: verdadeiro se as duas expressões têm colações equivalentes.
pub fn expr_coll_seq_match(
    db: &mut Connection,
    parse: &mut Parse,
    p_e1: &Expr,
    p_e2: &Expr,
    own: Option<&Rc<Table>>,
) -> bool {
    let p_coll1 = expr_nn_coll_seq(db, parse, Some(p_e1), own);
    let p_coll2 = expr_nn_coll_seq(db, parse, Some(p_e2), own);
    str_icmp(&p_coll1.name, &p_coll2.name) == 0
}

/// `sqlite3CompareAffinity`: `p_expr` é um operando de comparação e `aff2` a afinidade do outro;
/// devolve a afinidade a usar na comparação.
pub fn compare_affinity(p_expr: &Expr, aff2: u8, own: Option<&Rc<Table>>) -> u8 {
    let aff1 = expr_affinity(p_expr, own);
    if aff1 > SQLITE_AFF_NONE && aff2 > SQLITE_AFF_NONE {
        // Os dois lados são colunas. Se um tem afinidade numérica, vale ela; senão, nenhuma.
        if is_numeric_affinity(aff1) || is_numeric_affinity(aff2) {
            SQLITE_AFF_NUMERIC
        } else {
            SQLITE_AFF_BLOB
        }
    } else {
        // Um lado é coluna e o outro não: vale a afinidade da coluna.
        (if aff1 <= SQLITE_AFF_NONE { aff2 } else { aff1 }) | SQLITE_AFF_NONE
    }
}

/// `comparisonAffinity`: a afinidade a aplicar aos dois operandos antes de comparar.
fn comparison_affinity(p_expr: &Expr, own: Option<&Rc<Table>>) -> u8 {
    let mut aff = p_expr.p_left.as_deref().map_or(0, |l| expr_affinity(l, own));
    if let Some(r) = p_expr.p_right.as_deref() {
        aff = compare_affinity(r, aff, own);
    } else if p_expr.use_x_select() {
        if let Some(e) = p_expr.x_select().and_then(|s| select_result_expr(s, 0)) {
            aff = compare_affinity(e, aff, own);
        }
    } else if aff == 0 {
        aff = SQLITE_AFF_BLOB;
    }
    aff
}

/// `sqlite3IndexAffinityOk`: verdadeiro se um índice com afinidade `idx_affinity` pode
/// implementar a comparação `p_expr`.
pub fn index_affinity_ok(p_expr: &Expr, idx_affinity: u8, own: Option<&Rc<Table>>) -> bool {
    let aff = comparison_affinity(p_expr, own);
    if aff < SQLITE_AFF_TEXT {
        return true;
    }
    if aff == SQLITE_AFF_TEXT {
        return idx_affinity == SQLITE_AFF_TEXT;
    }
    is_numeric_affinity(idx_affinity)
}

/// `binaryCompareP5`: o P5 de um opcode de comparação binária (`OP_Eq`, `OP_Ge`...).
fn binary_compare_p5(
    p_expr1: &Expr,
    p_expr2: &Expr,
    jump_if_null: i32,
    own: Option<&Rc<Table>>,
) -> u8 {
    let aff = expr_affinity(p_expr2, own);
    compare_affinity(p_expr1, aff, own) | (jump_if_null as u8)
}

/// `sqlite3BinaryCompareCollSeq`: a colação de uma comparação binária entre `p_left` e `p_right`.
///
/// Vale a do operando esquerdo se ele tem colação; senão a do direito; senão `None` (BINARY).
/// `p_right` pode ser nulo.
pub fn binary_compare_coll_seq(
    db: &mut Connection,
    parse: &mut Parse,
    p_left: &Expr,
    p_right: Option<&Expr>,
    own: Option<&Rc<Table>>,
) -> Option<Rc<CollSeq>> {
    if (p_left.flags & EP_COLLATE) != 0 {
        expr_coll_seq(db, parse, Some(p_left), own)
    } else if p_right.map_or(false, |r| (r.flags & EP_COLLATE) != 0) {
        expr_coll_seq(db, parse, p_right, own)
    } else {
        let p_coll = expr_coll_seq(db, parse, Some(p_left), own);
        if p_coll.is_none() {
            expr_coll_seq(db, parse, p_right, own)
        } else {
            p_coll
        }
    }
}

/// `sqlite3ExprCompareCollSeq`: a colação adequada ao operador de comparação `p`. Se
/// `EP_Commuted` está ligada, a ordem dos operandos é invertida.
pub fn expr_compare_coll_seq(
    db: &mut Connection,
    parse: &mut Parse,
    p: &Expr,
    own: Option<&Rc<Table>>,
) -> Option<Rc<CollSeq>> {
    if p.has_property(EP_COMMUTED) {
        let r = p.p_right.as_deref()?;
        binary_compare_coll_seq(db, parse, r, p.p_left.as_deref(), own)
    } else {
        let l = p.p_left.as_deref()?;
        binary_compare_coll_seq(db, parse, l, p.p_right.as_deref(), own)
    }
}

/// `codeCompare`: gera o código de um operador de comparação.
pub(crate) fn code_compare(
    db: &mut Connection,
    parse: &mut Parse,
    p_left: &Expr,
    p_right: &Expr,
    opcode: u8,
    in1: i32,
    in2: i32,
    dest: i32,
    jump_if_null: i32,
    is_commuted: i32,
    own: Option<&Rc<Table>>,
) -> i32 {
    if parse.n_err != 0 {
        return 0;
    }
    let p4 = if is_commuted != 0 {
        binary_compare_coll_seq(db, parse, p_right, Some(p_left), own)
    } else {
        binary_compare_coll_seq(db, parse, p_left, Some(p_right), own)
    };
    let p5 = binary_compare_p5(p_left, p_right, jump_if_null, own);
    let v = vdbe_of_parse(parse);
    let addr = add_op4(v, opcode as i32, in2, dest, in1, P4::Coll(p4));
    change_p5(v, p5 as u16);
    addr
}

// ---------------------------------------------------------------------------------------------
// Vetores
// ---------------------------------------------------------------------------------------------

/// `sqlite3ExprIsVector`: verdadeiro se a expressão tem duas ou mais colunas de resultado.
pub fn expr_is_vector(p_expr: &Expr) -> bool {
    expr_vector_size(p_expr) > 1
}

/// `sqlite3ExprVectorSize`: quantos elementos tem um `TK_VECTOR`, quantas colunas tem uma
/// subconsulta, ou 1 para qualquer outra expressão.
pub fn expr_vector_size(p_expr: &Expr) -> i32 {
    let mut op = p_expr.op;
    if op == TK_REGISTER {
        op = p_expr.op2;
    }
    if op == TK_VECTOR {
        p_expr.x_list().map_or(0, |l| l.a.len() as i32)
    } else if op == TK_SELECT {
        p_expr.x_select().and_then(|s| s.p_e_list.as_deref()).map_or(0, |l| l.a.len() as i32)
    } else {
        1
    }
}

/// `sqlite3VectorFieldSubexpr`: a subexpressão do i-ésimo campo (de 0) do vetor. Se `p_vector`
/// é escalar, devolve o próprio `p_vector`. `p_vector` continua dono do resultado.
pub fn vector_field_subexpr(p_vector: &Expr, i: i32) -> Option<&Expr> {
    if expr_is_vector(p_vector) {
        if p_vector.op == TK_SELECT || p_vector.op2 == TK_SELECT {
            p_vector.x_select().and_then(|s| select_result_expr(s, i as usize))
        } else {
            p_vector.x_list().and_then(|l| l.a.get(i as usize)).and_then(|it| it.p_expr.as_deref())
        }
    } else {
        Some(p_vector)
    }
}

/// `sqlite3ExprForVectorField`: um novo `Expr` que, passado a `sqlite3ExprCode()`, gera o código
/// do campo `i_field` do vetor `p_vector`. O chamador é dono do resultado.
///
/// Se `p_vector` é um `TK_SELECT`, o nó devolvido é um `TK_SELECT_COLUMN` cujo `p_left` é uma
/// cópia do `TK_SELECT` (no C é o mesmo ponteiro, ver o cabeçalho do módulo).
pub fn expr_for_vector_field(
    db: &mut Connection,
    parse: &mut Parse,
    p_vector: &mut Expr,
    i_field: i32,
    n_field: i32,
) -> Option<Box<Expr>> {
    if p_vector.op == TK_SELECT {
        // O nó TK_SELECT_COLUMN: pLeft é o vetor com o TK_SELECT; pRight não é usado mas é
        // apagado recursivamente, então o vetor pode ser pendurado nele para o nó assumir a
        // posse; iColumn é o índice da coluna no vetor; iTable é 0 ou o número de colunas do
        // lado esquerdo de uma atribuição.
        let mut p_ret = p_expr(db, parse, TK_SELECT_COLUMN as i32, None, None);
        if let Some(r) = p_ret.as_deref_mut() {
            r.set_property(EP_FULL_SIZE);
            r.i_table = n_field;
            r.i_column = i_field;
            r.p_left = Some(Box::new(p_vector.clone()));
        }
        p_ret
    } else {
        if p_vector.op == TK_VECTOR && parse.in_rename_object() {
            // Deve ser um UPDATE vetorial dentro de um gatilho: o elemento sai da lista.
            if let Some(item) = p_vector.x_list_mut().and_then(|l| l.a.get_mut(i_field as usize)) {
                return item.p_expr.take();
            }
        }
        let src = if p_vector.op == TK_VECTOR {
            p_vector
                .x_list()
                .and_then(|l| l.a.get(i_field as usize))
                .and_then(|it| it.p_expr.as_deref())
        } else {
            Some(&*p_vector)
        };
        expr_dup(src, 0)
    }
}

/// `exprCodeSubselect`: se `p_expr` é um `TK_SELECT`, gera o código e devolve o registrador do
/// resultado (o primeiro de um vetor de registradores); senão devolve 0.
fn expr_code_subselect(db: &mut Connection, parse: &mut Parse, p_expr: &mut Expr) -> i32 {
    if p_expr.op == TK_SELECT {
        code_subselect(db, parse, p_expr)
    } else {
        0
    }
}

/// `exprVectorRegister`: o registrador que contém o elemento `i_field` de um vetor (`TK_VECTOR`,
/// `TK_REGISTER` ou `TK_SELECT` com várias colunas). Para `TK_VECTOR` gera o código do campo e
/// `*p_reg_free` pode receber um temporário que o chamador libera.
///
/// O `*ppExpr` de saída do C (o `Expr` do elemento) não existe aqui: o chamador o obtém
/// depois, só para leitura, com `vector_field_subexpr` (não dá para devolver uma referência
/// enquanto `expr_code_temp` precisa de `&mut`).
fn expr_vector_register(
    db: &mut Connection,
    parse: &mut Parse,
    p_vector: &mut Expr,
    i_field: i32,
    reg_select: i32,
    p_reg_free: &mut i32,
    own: Option<&Rc<Table>>,
) -> i32 {
    let op = p_vector.op;
    if op == TK_REGISTER {
        return p_vector.i_table + i_field;
    }
    if op == TK_SELECT {
        return reg_select + i_field;
    }
    if op == TK_VECTOR {
        let e = p_vector
            .x_list_mut()
            .and_then(|l| l.a.get_mut(i_field as usize))
            .and_then(|it| it.p_expr.as_deref_mut());
        return match e {
            Some(e) => expr_code_temp(db, parse, e, p_reg_free, own),
            None => 0,
        };
    }
    0
}

/// `codeVectorCompare`: `p_expr` é uma comparação entre dois vetores. Calcula o resultado (1, 0
/// ou NULL) e o grava no registrador `dest`.
///
/// Pré-condições: se `p_expr.op==TK_IS`, `op==TK_EQ` e `p5==SQLITE_NULLEQ`; se `TK_ISNOT`,
/// `op==TK_NE` e `p5==SQLITE_NULLEQ`; senão `op==p_expr.op` e `p5==0`.
pub(crate) fn code_vector_compare(
    db: &mut Connection,
    parse: &mut Parse,
    p_expr: &mut Expr,
    dest: i32,
    op: u8,
    p5: u8,
    own: Option<&Rc<Table>>,
) {
    let is_commuted = p_expr.has_property(EP_COMMUTED);
    let (Some(p_left), Some(p_right)) =
        (p_expr.p_left.as_deref_mut(), p_expr.p_right.as_deref_mut())
    else {
        return;
    };
    let n_left = expr_vector_size(p_left);
    let mut opx = op;
    let mut addr_cmp = 0;
    let addr_done = make_label(parse);

    if parse.n_err != 0 {
        return;
    }
    if n_left != expr_vector_size(p_right) {
        error_msg(db, parse, b"row value misused", &[]);
        return;
    }

    if op == TK_LE {
        opx = TK_LT;
    }
    if op == TK_GE {
        opx = TK_GT;
    }
    if op == TK_NE {
        opx = TK_EQ;
    }

    let reg_left = expr_code_subselect(db, parse, p_left);
    let reg_right = expr_code_subselect(db, parse, p_right);

    add_op2(vdbe_of_parse(parse), OP_INTEGER as i32, 1, dest);
    let mut i = 0;
    loop {
        let mut reg_free1 = 0;
        let mut reg_free2 = 0;
        if addr_cmp != 0 {
            jump_here(vdbe_of_parse(parse), addr_cmp);
        }
        let r1 = expr_vector_register(db, parse, p_left, i, reg_left, &mut reg_free1, own);
        let r2 = expr_vector_register(db, parse, p_right, i, reg_right, &mut reg_free2, own);
        addr_cmp = current_addr(parse);
        let p_l = vector_field_subexpr(p_left, i).expect("elemento do vetor esquerdo");
        let p_r = vector_field_subexpr(p_right, i).expect("elemento do vetor direito");
        code_compare(
            db,
            parse,
            p_l,
            p_r,
            opx,
            r1,
            r2,
            addr_done,
            p5 as i32,
            is_commuted as i32,
            own,
        );
        release_temp_reg(parse, reg_free1);
        release_temp_reg(parse, reg_free2);
        if (opx == TK_LT || opx == TK_GT) && i < n_left - 1 {
            addr_cmp = add_op0(vdbe_of_parse(parse), OP_ELSEEQ as i32);
        }
        if p5 == SQLITE_NULLEQ {
            add_op2(vdbe_of_parse(parse), OP_INTEGER as i32, 0, dest);
        } else {
            add_op3(vdbe_of_parse(parse), OP_ZEROORNULL as i32, r1, dest, r2);
        }
        if i == n_left - 1 {
            break;
        }
        if opx == TK_EQ {
            add_op2(vdbe_of_parse(parse), OP_NOTNULL as i32, dest, addr_done);
        } else {
            add_op2(vdbe_of_parse(parse), OP_GOTO as i32, 0, addr_done);
            if i == n_left - 2 {
                opx = op;
            }
        }
        i += 1;
    }
    jump_here(vdbe_of_parse(parse), addr_cmp);
    resolve_label(parse, db, addr_done);
    if op == TK_NE {
        add_op2(vdbe_of_parse(parse), OP_NOT as i32, dest, dest);
    }
}

// ---------------------------------------------------------------------------------------------
// Altura das expressões
// ---------------------------------------------------------------------------------------------

/// `sqlite3ExprCheckHeight`: confere se `n_height` não passa da profundidade máxima de expressão;
/// se passar, deixa a mensagem de erro em `parse`.
pub fn expr_check_height(db: &mut Connection, parse: &mut Parse, n_height: i32) -> i32 {
    let mut rc = SQLITE_OK;
    let mx_height = db.a_limit[SQLITE_LIMIT_EXPR_DEPTH as usize];
    if n_height > mx_height {
        error_msg(
            db,
            parse,
            b"Expression tree is too large (maximum depth %d)",
            &[PrintfArg::Int(mx_height as i64)],
        );
        rc = SQLITE_ERROR;
    }
    rc
}

/// `heightOfExpr`.
fn height_of_expr(p: Option<&Expr>, pn_height: &mut i32) {
    if let Some(p) = p {
        if p.n_height > *pn_height {
            *pn_height = p.n_height;
        }
    }
}

/// `heightOfExprList`.
fn height_of_expr_list(p: Option<&ExprList>, pn_height: &mut i32) {
    if let Some(p) = p {
        for item in &p.a {
            height_of_expr(item.p_expr.as_deref(), pn_height);
        }
    }
}

/// `heightOfSelect`: percorre o select e todos os `p_prior`.
fn height_of_select(p_select: Option<&Select>, pn_height: &mut i32) {
    let mut p = p_select;
    while let Some(s) = p {
        height_of_expr(s.p_where.as_deref(), pn_height);
        height_of_expr(s.p_having.as_deref(), pn_height);
        height_of_expr(s.p_limit.as_deref(), pn_height);
        height_of_expr_list(s.p_e_list.as_deref(), pn_height);
        height_of_expr_list(s.p_group_by.as_deref(), pn_height);
        height_of_expr_list(s.p_order_by.as_deref(), pn_height);
        p = s.p_prior.as_deref();
    }
}

/// `exprSetHeight`: grava `Expr.nHeight`. Um nó sem filhos, lista ou select tem altura 1; os
/// demais têm a maior altura dos referenciados mais um. Propaga também `EP_Propagate` de
/// `x.pList` para `flags`.
fn expr_set_height(p: &mut Expr) {
    let mut n_height = p.p_left.as_deref().map_or(0, |l| l.n_height);
    if let Some(r) = p.p_right.as_deref() {
        if r.n_height > n_height {
            n_height = r.n_height;
        }
    }
    if p.use_x_select() {
        height_of_select(p.x_select(), &mut n_height);
    } else if let Some(l) = p.x_list() {
        height_of_expr_list(Some(l), &mut n_height);
        let f = expr_list_flags(l);
        p.flags |= EP_PROPAGATE & f;
    }
    p.n_height = n_height + 1;
}

/// `sqlite3ExprSetHeightAndFlags`: grava `nHeight` e, se passar da profundidade máxima, deixa
/// o erro em `parse`. Propaga as flags `EP_Propagate` da lista.
pub fn expr_set_height_and_flags(db: &mut Connection, parse: &mut Parse, p: &mut Expr) {
    if parse.n_err != 0 {
        return;
    }
    expr_set_height(p);
    expr_check_height(db, parse, p.n_height);
}

/// `sqlite3SelectExprHeight`: a maior altura de qualquer expressão do select.
pub fn select_expr_height(p: Option<&Select>) -> i32 {
    let mut n_height = 0;
    height_of_select(p, &mut n_height);
    n_height
}

/// `sqlite3ExprSetErrorOffset`.
pub fn expr_set_error_offset(p_expr: Option<&mut Expr>, i_ofst: i32) {
    if let Some(e) = p_expr {
        if e.use_w_join() {
            return;
        }
        e.w = i_ofst;
    }
}

// ---------------------------------------------------------------------------------------------
// Alocação de nós
// ---------------------------------------------------------------------------------------------

/// `sqlite3ExprAlloc`: o alocador central dos nós `Expr`.
///
/// Se `dequote` é verdadeiro o token é desaspado (e `EP_DblQuoted` é ligada para aspas
/// duplas). Caso especial (tag-20240227-a): com `op==TK_INTEGER` e um token que cabe em 32
/// bits, o texto não é guardado: o inteiro vai para `u.iValue` e liga `EP_IntValue`.
pub fn expr_alloc(op: i32, p_token: Option<&Token>, dequote: i32) -> Option<Box<Expr>> {
    let mut p_new = Box::new(Expr::default());
    p_new.op = op as u8;
    p_new.i_agg = -1;
    if let Some(t) = p_token {
        let int_value = if op == TK_INTEGER as i32 && !t.z.is_empty() {
            get_int32(&t.z)
        } else {
            None
        };
        match int_value {
            Some(i_value) => {
                p_new.flags |=
                    EP_INT_VALUE | EP_LEAF | (if i_value != 0 { EP_IS_TRUE } else { EP_IS_FALSE });
                p_new.u = ExprU::IValue(i_value);
            }
            None => {
                p_new.u = ExprU::Token(Some(t.z.clone()));
                if dequote != 0 && is_quote(at(&t.z, 0)) {
                    dequote_expr(&mut p_new);
                }
            }
        }
    }
    p_new.n_height = 1;
    Some(p_new)
}

/// `sqlite3Expr`: um nó a partir de um token terminado em NUL e já desaspado.
pub fn expr(op: i32, z_token: Option<&[u8]>) -> Option<Box<Expr>> {
    let z = z_token.unwrap_or(&[]);
    let x = Token { z: z[..strlen30(z) as usize].to_vec(), i_ofst: -1 };
    expr_alloc(op, Some(&x), 0)
}

/// `sqlite3ExprAttachSubtrees`: pendura `p_left` e `p_right` no nó `p_root`. Se `p_root` é
/// nulo as subárvores são descartadas.
pub fn expr_attach_subtrees(
    p_root: Option<&mut Expr>,
    p_left: Option<Box<Expr>>,
    p_right: Option<Box<Expr>>,
) {
    let Some(root) = p_root else {
        return;
    };
    if let Some(r) = p_right {
        root.flags |= EP_PROPAGATE & r.flags;
        root.n_height = r.n_height + 1;
        root.p_right = Some(r);
    } else {
        root.n_height = 1;
    }
    if let Some(l) = p_left {
        root.flags |= EP_PROPAGATE & l.flags;
        if l.n_height >= root.n_height {
            root.n_height = l.n_height + 1;
        }
        root.p_left = Some(l);
    }
}

/// `sqlite3PExpr`: um nó que une até duas subárvores.
pub fn p_expr(
    db: &mut Connection,
    parse: &mut Parse,
    op: i32,
    p_left: Option<Box<Expr>>,
    p_right: Option<Box<Expr>>,
) -> Option<Box<Expr>> {
    let mut p = Box::new(Expr::default());
    p.op = (op & 0xff) as u8;
    p.i_agg = -1;
    expr_attach_subtrees(Some(&mut p), p_left, p_right);
    expr_check_height(db, parse, p.n_height);
    Some(p)
}

/// `sqlite3PExprAddSelect`: grava `p_select` em `Expr.x.pSelect`.
pub fn p_expr_add_select(
    db: &mut Connection,
    parse: &mut Parse,
    p_expr: Option<&mut Expr>,
    p_select: Option<Box<Select>>,
) {
    if let Some(e) = p_expr {
        e.x = match p_select {
            Some(s) => ExprX::Select(s),
            None => ExprX::None,
        };
        e.set_property(EP_X_IS_SELECT | EP_SUBQUERY);
        expr_set_height_and_flags(db, parse, e);
    }
}

/// `sqlite3ExprListToValues`: converte uma lista de vetores `((1,2),(3,4),(5,6))` num SELECT
/// `VALUES(1,2),(3,4),(5,6)`. Cada vetor precisa ter exatamente `n_elem` termos; senão deixa
/// um erro em `parse`. Usado em `IN ((1,2),(3,4))`.
pub fn expr_list_to_values(
    db: &mut Connection,
    parse: &mut Parse,
    n_elem: i32,
    mut p_e_list: Box<ExprList>,
) -> Option<Box<Select>> {
    let mut p_ret: Option<Box<Select>> = None;
    for item in p_e_list.a.iter_mut() {
        let Some(p_expr) = item.p_expr.as_deref_mut() else {
            continue;
        };
        let n_expr_elem = if p_expr.op == TK_VECTOR {
            p_expr.x_list().map_or(0, |l| l.a.len() as i32)
        } else {
            1
        };
        if n_expr_elem != n_elem {
            error_msg(
                db,
                parse,
                b"IN(...) element has %d term%s - expected %d",
                &[
                    PrintfArg::Int(n_expr_elem as i64),
                    PrintfArg::Text(Some(if n_expr_elem > 1 { b"s".to_vec() } else { Vec::new() })),
                    PrintfArg::Int(n_elem as i64),
                ],
            );
            break;
        }
        let p_list = match std::mem::take(&mut p_expr.x) {
            ExprX::List(l) => Some(l),
            _ => None,
        };
        let p_sel = select_new(parse, p_list, None, None, None, None, None, SF_VALUES, None);
        if let Some(mut sel) = p_sel {
            if let Some(prev) = p_ret.take() {
                sel.op = TK_ALL;
                sel.p_prior = Some(prev);
            }
            p_ret = Some(sel);
        }
    }

    if let Some(r) = p_ret.as_deref_mut() {
        if r.p_prior.is_some() {
            r.sel_flags |= SF_MULTIVALUE;
        }
    }
    p_ret
}

/// `sqlite3ExprAnd`: junta duas expressões com AND. Se uma é nula devolve a outra. Se um lado é
/// sabidamente falso e nenhum faz parte de um ON, devolve a constante 0.
pub fn expr_and(
    db: &mut Connection,
    parse: &mut Parse,
    p_left: Option<Box<Expr>>,
    p_right: Option<Box<Expr>>,
) -> Option<Box<Expr>> {
    match (p_left, p_right) {
        (None, r) => r,
        (l, None) => l,
        (Some(l), Some(r)) => {
            let f = l.flags | r.flags;
            if (f & (EP_OUTER_ON | EP_INNER_ON | EP_IS_FALSE)) == EP_IS_FALSE
                && !parse.in_rename_object()
            {
                expr_deferred_delete(Some(l));
                expr_deferred_delete(Some(r));
                expr(TK_INTEGER as i32, Some(&b"0"[..]))
            } else {
                p_expr(db, parse, TK_AND as i32, Some(l), Some(r))
            }
        }
    }
}

/// `sqlite3ExprFunction`: um nó para uma função com vários argumentos.
pub fn expr_function(
    db: &mut Connection,
    parse: &mut Parse,
    p_list: Option<Box<ExprList>>,
    p_token: &Token,
    e_distinct: i32,
) -> Option<Box<Expr>> {
    let mut p_new = expr_alloc(TK_FUNCTION as i32, Some(p_token), 1)?;
    p_new.w = p_token.i_ofst.wrapping_sub(parse.z_tail as i32);
    if let Some(l) = p_list.as_deref() {
        if l.a.len() as i32 > db.a_limit[SQLITE_LIMIT_FUNCTION_ARG as usize] && parse.nested == 0 {
            let arg = token_arg(parse, p_token);
            error_msg(db, parse, b"too many arguments on function %T", &[arg]);
        }
    }
    p_new.x = match p_list {
        Some(l) => ExprX::List(l),
        None => ExprX::None,
    };
    p_new.set_property(EP_HAS_FUNC);
    expr_set_height_and_flags(db, parse, &mut p_new);
    if e_distinct == SF_DISTINCT as i32 {
        p_new.set_property(EP_DISTINCT);
    }
    Some(p_new)
}

/// `sqlite3ExprOrderByAggregateError`: ORDER BY dentro dos argumentos de função não agregada.
pub fn expr_order_by_aggregate_error(db: &mut Connection, parse: &mut Parse, p: &Expr) {
    error_msg(
        db,
        parse,
        b"ORDER BY may not be used with non-aggregate %#T()",
        &[expr_token_arg(p)],
    );
}

/// `sqlite3ExprAddFunctionOrderBy`: pendura um ORDER BY numa chamada de função, num novo nó
/// `TK_ORDER` em `pLeft` do nó `TK_FUNCTION`.
pub fn expr_add_function_order_by(
    db: &mut Connection,
    parse: &mut Parse,
    p_expr: Option<&mut Expr>,
    p_order_by: Option<Box<ExprList>>,
) {
    let Some(p_order_by) = p_order_by else {
        return;
    };
    let Some(p_expr) = p_expr else {
        return;
    };
    if p_expr.x_list().map_or(true, |l| l.a.is_empty()) {
        // ORDER BY em agregado sem argumentos é ignorado.
        return;
    }
    if p_expr.is_window_func() {
        expr_order_by_aggregate_error(db, parse, p_expr);
        return;
    }
    if let Some(mut p_ob) = expr_alloc(TK_ORDER as i32, None, 0) {
        p_ob.x = ExprX::List(p_order_by);
        p_ob.set_property(EP_FULL_SIZE);
        p_expr.p_left = Some(p_ob);
    }
}

/// `sqlite3ExprFunctionUsable`: confere se a função é usável pelas regras de acesso atuais
/// (`SQLITE_FUNC_DIRECT`, `SQLITE_FUNC_UNSAFE`); se não for, cria um erro.
pub fn expr_function_usable(
    db: &mut Connection,
    parse: &mut Parse,
    p_expr: &Expr,
    p_def: &FuncDef,
) {
    if p_expr.has_property(EP_FROM_DDL)
        && ((p_def.func_flags & SQLITE_FUNC_DIRECT) != 0 || (db.flags & SQLITE_TRUSTED_SCHEMA) == 0)
    {
        // Funções proibidas em gatilhos e views se são DIRECTONLY, ou se não são INNOCUOUS e
        // o esquema pode estar contaminado (TRUSTED_SCHEMA desligado).
        error_msg(db, parse, b"unsafe use of %#T()", &[expr_token_arg(p_expr)]);
    }
}

/// `sqlite3ExprAssignVarNumber`: atribui um número de variável ao curinga da expressão.
///
/// `?` recebe o próximo número; `?nnn` recebe `nnn` (limitado); `:aaa`, `@aaa` e `$aaa`
/// reaproveitam o número da ocorrência anterior do mesmo nome, ou o próximo número.
pub fn expr_assign_var_number(
    db: &mut Connection,
    parse: &mut Parse,
    p_expr: Option<&mut Expr>,
    n: u32,
) {
    let Some(p_expr) = p_expr else {
        return;
    };
    let z: Vec<u8> = p_expr.z_token().unwrap_or(&[]).to_vec();
    let limit = db.a_limit[SQLITE_LIMIT_VARIABLE_NUMBER as usize];
    let x: i32;
    if at(&z, 1) == 0 {
        // Curinga "?": o próximo número de variável.
        parse.n_var += 1;
        x = parse.n_var;
    } else {
        let mut do_add = false;
        if at(&z, 0) == b'?' {
            // Curinga "?nnn": converte "nnn" em inteiro e o usa como número da variável.
            let mut i: i64 = 0;
            let b_ok;
            if n == 2 {
                i = at(&z, 1) as i64 - b'0' as i64;
                b_ok = true;
            } else {
                b_ok = 0 == atoi64(&z[1..], &mut i, n as i32 - 1, SQLITE_UTF8 as u8);
            }
            if !b_ok || i < 1 || i > limit as i64 {
                error_msg(
                    db,
                    parse,
                    b"variable number must be between ?1 and ?%d",
                    &[PrintfArg::Int(limit as i64)],
                );
                record_error_offset_of_expr(db, Some(&*p_expr));
                return;
            }
            x = i as i32;
            if x > parse.n_var {
                parse.n_var = x;
                do_add = true;
            } else if vlist_entry_name(&parse.p_v_list, x).is_none() {
                do_add = true;
            }
        } else {
            // Curinga ":aaa", "$aaa" ou "@aaa": reusa o número da ocorrência anterior.
            let name = &z[..(n as usize).min(z.len())];
            let mut xx = vlist_entry_number(&parse.p_v_list, name);
            if xx == 0 {
                parse.n_var += 1;
                xx = parse.n_var;
                do_add = true;
            }
            x = xx;
        }
        if do_add {
            let name = z[..(n as usize).min(z.len())].to_vec();
            parse.p_v_list.push(VListEntry { i_var: x, name });
        }
    }
    p_expr.i_column = x;
    if x > limit {
        error_msg(db, parse, b"too many SQL variables", &[]);
        record_error_offset_of_expr(db, Some(&*p_expr));
    }
}

/// `sqlite3ExprDeferredDelete`: o C adia o delete até o `Parse` morrer; com posse em árvore a
/// expressão simplesmente cai aqui. Devolve 0, como "adiado com sucesso".
pub fn expr_deferred_delete(_p_expr: Option<Box<Expr>>) -> i32 {
    0
}

/// `sqlite3ExprUnmapAndDelete`: em modo RENAME tira a expressão do mapa de tokens e a apaga.
pub fn expr_unmap_and_delete(parse: &mut Parse, p: Option<Box<Expr>>) {
    if let Some(e) = p {
        if parse.in_rename_object() {
            rename_expr_unmap(parse, &e);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Duplicação
// ---------------------------------------------------------------------------------------------

/// O tamanho de um `Expr` no C (`EXPR_FULLSIZE`, `EXPR_REDUCEDSIZE`, `EXPR_TOKENONLYSIZE`),
/// que aqui só decide quais campos são copiados.
#[derive(Clone, Copy, PartialEq, Eq)]
enum StructSize {
    TokenOnly,
    Reduced,
    Full,
}

/// `exprStructSize`.
fn expr_struct_size(p: &Expr) -> StructSize {
    if p.has_property(EP_TOKEN_ONLY) {
        StructSize::TokenOnly
    } else if p.has_property(EP_REDUCED) {
        StructSize::Reduced
    } else {
        StructSize::Full
    }
}

/// `dupedExprStructSize`: o tamanho da cópia (com `EP_Reduced` ou `EP_TokenOnly` implícito).
fn duped_expr_struct_size(p: &Expr, flags: u32) -> StructSize {
    if flags == 0 || p.has_property(EP_FULL_SIZE) {
        StructSize::Full
    } else if p.p_left.is_some() || !matches!(p.x, ExprX::None) {
        StructSize::Reduced
    } else {
        StructSize::TokenOnly
    }
}

/// Cópia de `u`: o texto vai até o primeiro NUL (como o `strlen` do C).
fn dup_u(p: &Expr) -> ExprU {
    match &p.u {
        ExprU::Token(Some(z)) => ExprU::Token(Some(z[..strlen30(z) as usize].to_vec())),
        other => other.clone(),
    }
}

/// `exprDup`: cópia profunda de um nó. `static_flag` é verdadeiro para os filhos de uma cópia
/// reduzida (no C, o espaço vem do buffer do pai e não do malloc: `EP_Static`).
fn expr_dup_node(p: &Expr, dup_flags: u32, static_flag: bool) -> Box<Expr> {
    let new_size = duped_expr_struct_size(p, dup_flags);
    let copy_size = if dup_flags != 0 { new_size } else { expr_struct_size(p) };
    let mut p_new = Box::new(Expr::default());
    p_new.op = p.op;
    p_new.aff_expr = p.aff_expr;
    p_new.op2 = p.op2;
    p_new.flags = p.flags;
    p_new.u = dup_u(p);
    if copy_size != StructSize::TokenOnly {
        p_new.n_height = p.n_height;
    }
    if copy_size == StructSize::Full {
        p_new.i_table = p.i_table;
        p_new.i_column = p.i_column;
        p_new.i_agg = p.i_agg;
        p_new.w = p.w;
        p_new.p_agg_info = p.p_agg_info;
        p_new.y = match &p.y {
            ExprY::Win(_) => ExprY::default(),
            other => other.clone(),
        };
    }

    // Liga EP_Reduced, EP_TokenOnly e EP_Static como o C.
    p_new.flags &= !(EP_REDUCED | EP_TOKEN_ONLY | EP_STATIC);
    match new_size {
        StructSize::Reduced => p_new.flags |= EP_REDUCED,
        StructSize::TokenOnly => p_new.flags |= EP_TOKEN_ONLY,
        StructSize::Full => {}
    }
    if static_flag {
        p_new.flags |= EP_STATIC;
    }

    if ((p.flags | p_new.flags) & (EP_TOKEN_ONLY | EP_LEAF)) == 0 {
        // Preenche pNew->x.pSelect ou pNew->x.pList.
        if p.use_x_select() {
            p_new.x = match select_dup(p.x_select(), dup_flags) {
                Some(s) => ExprX::Select(s),
                None => ExprX::None,
            };
        } else {
            let f = if p.op != TK_ORDER { dup_flags } else { 0 };
            p_new.x = match expr_list_dup(p.x_list(), f) {
                Some(l) => ExprX::List(l),
                None => ExprX::None,
            };
        }

        if p.has_property(EP_WIN_FUNC) {
            if let Some(w) = p.y_win() {
                p_new.y = ExprY::Win(window_dup(w));
            }
        }

        // Preenche pNew->pLeft e pNew->pRight.
        if dup_flags != 0 {
            p_new.p_left = if p.op == TK_SELECT_COLUMN {
                p.p_left.clone()
            } else {
                p.p_left.as_deref().map(|l| expr_dup_node(l, EXPRDUP_REDUCE, true))
            };
            p_new.p_right = p.p_right.as_deref().map(|r| expr_dup_node(r, EXPRDUP_REDUCE, true));
        } else {
            p_new.p_left = if p.op == TK_SELECT_COLUMN {
                p.p_left.clone()
            } else {
                expr_dup(p.p_left.as_deref(), 0)
            };
            p_new.p_right = expr_dup(p.p_right.as_deref(), 0);
        }
    }
    p_new
}

/// `sqlite3WithDup`: cópia profunda de uma cláusula WITH.
pub fn with_dup(p: Option<&With>) -> Option<Box<With>> {
    let p = p?;
    let mut p_ret = With::default();
    for cte in &p.a {
        p_ret.a.push(Cte {
            p_select: select_dup(cte.p_select.as_deref(), 0),
            p_cols: expr_list_dup(cte.p_cols.as_deref(), 0),
            z_name: cte.z_name.clone(),
            e_m10d: cte.e_m10d,
            ..Cte::default()
        });
    }
    Some(Box::new(p_ret))
}

/// Contexto de `gatherSelectWindows`: quantas janelas já foram ligadas e se o select raiz já
/// passou pelo callback (o `p==pWalker->u.pSelect` do C).
#[derive(Default)]
struct GatherWindows {
    n_linked: u32,
    seen_root: bool,
}

/// `gatherSelectWindowsCallback`: liga ao select cada janela de função encontrada.
fn gather_select_windows_callback(w: &mut Walker<GatherWindows>, p_expr: &mut Expr) -> i32 {
    if p_expr.op == TK_FUNCTION && p_expr.has_property(EP_WIN_FUNC) {
        if let Some(p_win) = p_expr.y_win_mut() {
            window_link(&mut w.u.n_linked, p_win);
        }
    }
    WRC_CONTINUE
}

/// `gatherSelectWindowsSelectCallback`: só o select raiz é percorrido, os aninhados são podados.
fn gather_select_windows_select_callback(w: &mut Walker<GatherWindows>, _p: &mut Select) -> i32 {
    if w.u.seen_root {
        WRC_PRUNE
    } else {
        w.u.seen_root = true;
        WRC_CONTINUE
    }
}

/// `gatherSelectWindows`: junta todos os `Window` das expressões de um select recém duplicado.
fn gather_select_windows(p: &mut Select) {
    let mut w = Walker {
        x_expr_callback: Some(gather_select_windows_callback),
        x_select_callback: Some(gather_select_windows_select_callback),
        x_select_callback2: None,
        walker_depth: 0,
        e_code: 0,
        m_w_flags: 0,
        u: GatherWindows::default(),
    };
    walk_select(&mut w, Some(&mut *p));
    p.n_win_linked = w.u.n_linked;
}

/// `sqlite3ExprDup`: cópia profunda da expressão. Com `EXPRDUP_REDUCE` a cópia é a versão
/// truncada que vai para o esquema.
pub fn expr_dup(p: Option<&Expr>, flags: u32) -> Option<Box<Expr>> {
    p.map(|e| expr_dup_node(e, flags, false))
}

/// `sqlite3ExprListDup`: cópia profunda de uma lista de expressões.
///
/// A correção de `pLeft`/`pRight` entre `TK_SELECT_COLUMN` irmãos que o C faz aqui (para o
/// `pLeft` compartilhado) não existe: cada nó já saiu de `expr_dup_node` com a sua cópia.
pub fn expr_list_dup(p: Option<&ExprList>, flags: u32) -> Option<Box<ExprList>> {
    let p = p?;
    let mut p_new = ExprList { a: Vec::with_capacity(p.a.len()) };
    for old in &p.a {
        let mut item = ExprListItem::default();
        item.p_expr = expr_dup(old.p_expr.as_deref(), flags);
        item.z_e_name = old.z_e_name.clone();
        item.fg = old.fg;
        item.fg.done = false;
        item.i_order_by_col = old.i_order_by_col;
        item.i_alias = old.i_alias;
        item.i_const_expr_reg = old.i_const_expr_reg;
        p_new.a.push(item);
    }
    Some(Box::new(p_new))
}

/// `sqlite3SrcListDup`: cópia profunda de uma lista FROM. As tabelas do esquema não são
/// duplicadas (o `Rc` é compartilhado, que é o `pTab->nTabRef++` do C).
pub fn src_list_dup(p: Option<&SrcList>, flags: u32) -> Option<Box<SrcList>> {
    let p = p?;
    let mut p_new = SrcList { a: Vec::with_capacity(p.a.len()) };
    for old in &p.a {
        let mut ni = SrcItem::default();
        ni.p_schema = old.p_schema;
        ni.z_database = old.z_database.clone();
        ni.z_name = old.z_name.clone();
        ni.z_alias = old.z_alias.clone();
        ni.fg = old.fg;
        ni.i_cursor = old.i_cursor;
        ni.addr_fill_sub = old.addr_fill_sub;
        ni.reg_return = old.reg_return;
        ni.reg_result = old.reg_result;
        ni.u1 = if ni.fg.is_indexed_by {
            SrcU1::IndexedBy(match &old.u1 {
                SrcU1::IndexedBy(z) => z.clone(),
                _ => None,
            })
        } else if ni.fg.is_tab_func {
            SrcU1::FuncArg(match &old.u1 {
                SrcU1::FuncArg(l) => expr_list_dup(l.as_deref(), flags),
                _ => None,
            })
        } else {
            SrcU1::NRow(match &old.u1 {
                SrcU1::NRow(n) => *n,
                _ => 0,
            })
        };
        // O C faz `pCteUse->nUse++` aqui quando `fg.isCte`; falta o acesso a `Parse.cte_uses`.
        ni.u2 = old.u2.clone();
        ni.p_tab = old.p_tab.clone();
        ni.p_select = select_dup(old.p_select.as_deref(), flags);
        ni.u3 = if old.fg.is_using {
            SrcU3::Using(match &old.u3 {
                SrcU3::Using(l) => id_list_dup(l.as_deref()),
                _ => None,
            })
        } else {
            SrcU3::On(match &old.u3 {
                SrcU3::On(e) => expr_dup(e.as_deref(), flags),
                _ => None,
            })
        };
        ni.col_used = old.col_used;
        p_new.a.push(ni);
    }
    Some(Box::new(p_new))
}

/// `sqlite3IdListDup`: cópia profunda de uma lista de identificadores.
pub fn id_list_dup(p: Option<&IdList>) -> Option<Box<IdList>> {
    let p = p?;
    let mut p_new = IdList { e_u4: p.e_u4, a: Vec::with_capacity(p.a.len()) };
    for old in &p.a {
        p_new.a.push(IdListItem { z_name: old.z_name.clone(), idx: old.idx });
    }
    Some(Box::new(p_new))
}

/// `sqlite3SelectDup`: cópia profunda de um select, com todos os `p_prior` de um composto.
pub fn select_dup(p_dup: Option<&Select>, flags: u32) -> Option<Box<Select>> {
    // A cópia sai da raiz para a esquerda; o encadeamento por `p_prior` é refeito no fim.
    let mut copies: Vec<Select> = Vec::new();
    let mut p = p_dup;
    while let Some(s) = p {
        let mut p_new = Select::default();
        p_new.p_e_list = expr_list_dup(s.p_e_list.as_deref(), flags);
        p_new.p_src = src_list_dup(s.p_src.as_deref(), flags);
        p_new.p_where = expr_dup(s.p_where.as_deref(), flags);
        p_new.p_group_by = expr_list_dup(s.p_group_by.as_deref(), flags);
        p_new.p_having = expr_dup(s.p_having.as_deref(), flags);
        p_new.p_order_by = expr_list_dup(s.p_order_by.as_deref(), flags);
        p_new.op = s.op;
        p_new.has_next = !copies.is_empty();
        p_new.p_prior = None;
        p_new.p_limit = expr_dup(s.p_limit.as_deref(), flags);
        p_new.i_limit = 0;
        p_new.i_offset = 0;
        p_new.sel_flags = s.sel_flags & !SF_USESEPHEMERAL;
        p_new.addr_open_ephm = [-1, -1];
        p_new.n_select_row = s.n_select_row;
        p_new.p_with = with_dup(s.p_with.as_deref());
        p_new.n_win_linked = 0;
        p_new.p_win_defn = window_list_dup(&s.p_win_defn);
        if s.n_win_linked > 0 {
            gather_select_windows(&mut p_new);
        }
        p_new.sel_id = s.sel_id;
        copies.push(p_new);
        p = s.p_prior.as_deref();
    }
    let mut p_ret: Option<Box<Select>> = None;
    for mut s in copies.into_iter().rev() {
        s.p_prior = p_ret.take();
        p_ret = Some(Box::new(s));
    }
    p_ret
}

// ---------------------------------------------------------------------------------------------
// ExprList
// ---------------------------------------------------------------------------------------------

/// `sqlite3ExprListAppend`: acrescenta uma expressão ao fim da lista; se a lista é nula, cria
/// uma. (`sqlite3ExprListAppendNew` e `sqlite3ExprListAppendGrow` são o `Vec::push`.)
pub fn expr_list_append(
    p_list: Option<Box<ExprList>>,
    p_expr: Option<Box<Expr>>,
) -> Option<Box<ExprList>> {
    let mut l = p_list.unwrap_or_default();
    l.a.push(ExprListItem { p_expr, ..ExprListItem::default() });
    Some(l)
}

/// `sqlite3ExprListAppendVector`: `p_columns` e `p_expr` formam uma atribuição vetorial do SET
/// de um UPDATE: `(a,b,c) = (expr1,expr2,expr3)` ou `(a,b,c) = (SELECT x,y,z ...)`. Acrescenta
/// um item para cada termo; com subconsulta à direita, itens `TK_SELECT_COLUMN`.
pub fn expr_list_append_vector(
    db: &mut Connection,
    parse: &mut Parse,
    mut p_list: Option<Box<ExprList>>,
    mut p_columns: Option<Box<IdList>>,
    mut p_expr: Option<Box<Expr>>,
) -> Option<Box<ExprList>> {
    let i_first = p_list.as_deref().map_or(0, |l| l.a.len());
    'body: {
        let Some(cols) = p_columns.as_deref_mut() else {
            break 'body;
        };
        let Some(vec_expr) = p_expr.as_deref_mut() else {
            break 'body;
        };
        let n_id = cols.a.len() as i32;

        // Se o lado direito é um vetor, o tamanho já se confere aqui. Se é um SELECT, o "*" do
        // resultado precisa ser expandido antes, e a conferência fica para a geração de código.
        let is_select = vec_expr.op == TK_SELECT;
        if !is_select {
            let n = expr_vector_size(vec_expr);
            if n_id != n {
                error_msg(
                    db,
                    parse,
                    b"%d columns assigned %d values",
                    &[PrintfArg::Int(n_id as i64), PrintfArg::Int(n as i64)],
                );
                break 'body;
            }
        }

        for i in 0..cols.a.len() {
            let Some(p_sub_expr) = expr_for_vector_field(db, parse, vec_expr, i as i32, n_id)
            else {
                continue;
            };
            p_list = expr_list_append(p_list, Some(p_sub_expr));
            if let Some(last) = p_list.as_deref_mut().and_then(|l| l.a.last_mut()) {
                last.z_e_name = cols.a[i].z_name.take();
            }
        }

        if is_select {
            if let Some(l) = p_list.as_deref_mut() {
                if let Some(p_first) = l.a.get_mut(i_first).and_then(|it| it.p_expr.as_deref_mut())
                {
                    // O SELECT fica em pRight para ser apagado junto com a lista.
                    p_first.p_right = p_expr.take();
                    // Guarda o tamanho do lado esquerdo para conferir na geração de código.
                    p_first.i_table = n_id;
                }
            }
        }
    }
    expr_unmap_and_delete(parse, p_expr);
    p_list
}

/// `sqlite3ExprListSetSortOrder`: a ordem do último elemento da lista.
pub fn expr_list_set_sort_order(p: Option<&mut ExprList>, mut i_sort_order: i32, e_nulls: i32) {
    let Some(p) = p else {
        return;
    };
    let Some(p_item) = p.a.last_mut() else {
        return;
    };
    if i_sort_order == SQLITE_SO_UNDEFINED {
        i_sort_order = crate::consts::SQLITE_SO_ASC;
    }
    p_item.fg.sort_flags = i_sort_order as u8;
    if e_nulls != SQLITE_SO_UNDEFINED {
        p_item.fg.b_nulls = true;
        if i_sort_order != e_nulls {
            p_item.fg.sort_flags |= KEYINFO_ORDER_BIGNULL;
        }
    }
}

/// `sqlite3ExprListSetName`: o `zEName` do elemento mais recente da lista.
pub fn expr_list_set_name(
    parse: &mut Parse,
    p_list: Option<&mut ExprList>,
    p_name: &Token,
    dequote_name: i32,
) {
    let Some(l) = p_list else {
        return;
    };
    let Some(p_item) = l.a.last_mut() else {
        return;
    };
    let mut z = p_name.z.clone();
    if dequote_name != 0 {
        // Sem dequote o nome não vem de um DDL tratado pelo parser e não entra no mapa de tokens.
        dequote(&mut z);
        let n = strlen30(&z) as usize;
        z.truncate(n);
        p_item.z_e_name = Some(z);
        if parse.in_rename_object() {
            let addr = p_item.z_e_name.as_ref().map_or(0, |z| z.as_ptr() as usize);
            rename_token_map(parse, addr, p_name);
        }
    } else {
        p_item.z_e_name = Some(z);
    }
}

/// `sqlite3ExprListSetSpan`: o `zEName` (como span) do elemento mais recente. `z_span` é o texto
/// de `zStart` até `zEnd`.
pub fn expr_list_set_span(p_list: Option<&mut ExprList>, z_span: &[u8]) {
    if let Some(l) = p_list {
        if let Some(p_item) = l.a.last_mut() {
            if p_item.z_e_name.is_none() {
                p_item.z_e_name = Some(crate::util::db_span_dup(z_span));
                p_item.fg.e_e_name = ENAME_SPAN;
            }
        }
    }
}

/// `sqlite3ExprListCheckLength`: erro se a lista passa do limite de colunas.
pub fn expr_list_check_length(
    db: &mut Connection,
    parse: &mut Parse,
    p_e_list: Option<&ExprList>,
    z_object: &[u8],
) {
    let mx = db.a_limit[SQLITE_LIMIT_COLUMN as usize];
    if let Some(l) = p_e_list {
        if l.a.len() as i32 > mx {
            error_msg(
                db,
                parse,
                b"too many columns in %s",
                &[PrintfArg::Text(Some(z_object.to_vec()))],
            );
        }
    }
}

/// `sqlite3ExprListFlags`: o OR de todos os `Expr.flags` da lista.
pub fn expr_list_flags(p_list: &ExprList) -> u32 {
    let mut m = 0;
    for item in &p_list.a {
        if let Some(e) = item.p_expr.as_deref() {
            m |= e.flags;
        }
    }
    m
}
