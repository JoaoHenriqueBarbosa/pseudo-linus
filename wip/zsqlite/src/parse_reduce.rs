//! `parse.c`, as ações semânticas do lemon (`yy_reduce`, `parse_c.008` em diante) e os auxiliares do
//! `%include` do `parse.y` (`parse_c.000`). Esta fatia traduz as regras de 0 a 200; as regras
//! maiores ficam em `crate::parse_reduce2::yy_reduce_tail`, chamada no braço `_` do `match`.
//!
//! Convenções desta fatia (modelo v2, ver CONVENTIONS.md e `parse_tables.rs`):
//!
//! - `yymsp[k].minor.yyNNN` do C é um acessor `take_*`/`get_*`/`set_*` sobre `yyp.msp(k)`. Os
//!   valores possuídos (árvores, listas, SELECT) são MOVIDOS para fora da pilha com `take_*`
//!   (a entrada fica com a variante vazia), como o lemon faz quando a ação consome o símbolo.
//! - `Token.z` é uma cópia do texto; `Token.i_ofst` é o deslocamento no SQL. Todo `z + n`, `z - zSql`
//!   e "texto de zStart até zEnd" do C vira aritmética sobre `i_ofst` e uma fatia de `Parse.z_sql`
//!   (o texto completo do SQL que está sendo analisado: ver `sql_span`).
//! - Token vazio (`z = 0, n = 0` do C) é `Token { z: vec![], i_ofst: -1 }`. O token do
//!   `NOT INDEXED` (`z = 0, n = 1`) é `Token { z: vec![0], i_ofst: -1 }` (ver
//!   `not_indexed_token` e `is_not_indexed_token`).
//! - `yy168` (`const char*`) é o deslocamento em bytes no SQL (`Yy168(i32)`).
//! - Funções de outros módulos recebem `db: &mut Connection` e `parse: &mut Parse` (nessa ordem)
//!   quando o `Parse *` do C basta para chegar ao `db`; o resto segue o nome determinístico
//!   `sqlite3XxxYy` -> `crate::modulo::xxx_yy`.
//! - Nenhuma ação chama `yy_reduce_goto`: quem fecha a redução é `yy_reduce`, no fim.

use crate::alter::{rename_token_map, rename_token_remap};
use crate::build::{
    add_check_constraint, add_collate_type, add_column, add_default_value, add_generated,
    add_not_null, add_primary_key, add_returning, begin_transaction, create_foreign_key,
    create_index, create_view, defer_foreign_key, drop_table, end_table, end_transaction,
    finish_coding, name_from_token, savepoint, src_list_append, src_list_append_from_term,
    src_list_append_list, src_list_func_args, src_list_indexed_by, src_list_shift_join_type,
    start_table, token_arg, join_type,
};
use crate::connection::{Connection, Parse};
use crate::consts::{
    EP_LEAF, EP_PROPAGATE, JT_INNER, OE_CASCADE, OE_DEFAULT, OE_IGNORE, OE_NONE, OE_REPLACE,
    OE_RESTRICT, OE_SET_DFLT, OE_SET_NULL, SAVEPOINT_BEGIN, SAVEPOINT_RELEASE, SAVEPOINT_ROLLBACK,
    SF_ALL, SF_COMPOUND, SF_DISTINCT, SF_MULTIVALUE, SF_NESTEDFROM, SF_VALUES,
    SQLITE_IDXTYPE_UNIQUE, SQLITE_LIMIT_COMPOUND_SELECT, SQLITE_SO_ASC, SQLITE_SO_DESC,
    SQLITE_SO_UNDEFINED, SRT_OUTPUT, TF_NO_VISIBLE_ROWID, TF_STRICT, TF_WITHOUT_ROWID, TK_ALL,
    TK_ASTERISK, TK_CAST, TK_DEFERRED, TK_DOT, TK_ID, TK_INTEGER, TK_LIMIT, TK_REGISTER,
    TK_STRING, TK_UMINUS, TK_VARIABLE, TK_VECTOR,
};
use crate::ctype::{is_digit, is_quote};
use crate::expr::{
    expr, expr_add_collate_token, expr_alloc, expr_and, expr_assign_var_number,
    expr_attach_subtrees, expr_function, expr_list_append, expr_list_append_vector,
    expr_list_check_length, expr_list_set_name, expr_list_set_sort_order, expr_list_set_span,
    expr_set_error_offset, p_expr,
};
use crate::expr::expr_add_function_order_by;
use crate::expr_code::expr_id_to_true_false;
use crate::parse_tables::{yy_reduce_goto, ValueMask, YyMinor, YyParser, YYNOCODE};
use crate::printf::PrintfArg;
use crate::insert::{multi_values, multi_values_end};
use crate::select::{
    select, select_new, select_op_name,
};
use crate::sqlite_int::{
    Expr, ExprList, ExprU, ExprX, IdList, OnOrUsing, Select, SelectDest, SrcList, Token, Upsert,
    Window, With,
};
use crate::util::{at, dequote_expr, error_msg, get_int32, strnicmp};
use crate::window::{window_attach, window_list_to_vec};

// ---------------------------------------------------------------------------------------------
// Acessores da pilha (`yymsp[k].minor.yyNNN`)
// ---------------------------------------------------------------------------------------------

/// Gera o par `take_*`/`set_*` de uma variante `Option<Box<T>>` de `YyMinor`.
macro_rules! slot_boxed {
    ($take:ident, $set:ident, $variant:ident, $ty:ty) => {
        /// Move o valor da entrada `k` para fora da pilha (a entrada fica com a variante vazia).
        #[allow(dead_code)]
        pub(crate) fn $take(yyp: &mut YyParser, k: isize) -> Option<Box<$ty>> {
            match &mut yyp.msp(k).minor {
                YyMinor::$variant(v) => v.take(),
                _ => None,
            }
        }
        /// Grava o valor na entrada `k`.
        #[allow(dead_code)]
        pub(crate) fn $set(yyp: &mut YyParser, k: isize, v: Option<Box<$ty>>) {
            yyp.msp(k).minor = YyMinor::$variant(v);
        }
    };
}

slot_boxed!(take_expr, set_expr, Yy454, Expr);
slot_boxed!(take_list, set_list, Yy14, ExprList);
slot_boxed!(take_select, set_select, Yy555, Select);
slot_boxed!(take_src, set_src, Yy203, SrcList);
slot_boxed!(take_id_list, set_id_list, Yy132, IdList);
slot_boxed!(take_with, set_with, Yy59, With);
slot_boxed!(take_window, set_window, Yy211, Window);
slot_boxed!(take_upsert, set_upsert, Yy122, Upsert);

/// `yymsp[k].minor.yy144`.
pub(crate) fn get_i32(yyp: &mut YyParser, k: isize) -> i32 {
    match &yyp.msp(k).minor {
        YyMinor::Yy144(v) => *v,
        _ => 0,
    }
}

/// `yymsp[k].minor.yy144 = v`.
pub(crate) fn set_i32(yyp: &mut YyParser, k: isize, v: i32) {
    yyp.msp(k).minor = YyMinor::Yy144(v);
}

/// `yymsp[k].minor.yy391`.
pub(crate) fn get_u32(yyp: &mut YyParser, k: isize) -> u32 {
    match &yyp.msp(k).minor {
        YyMinor::Yy391(v) => *v,
        _ => 0,
    }
}

/// `yymsp[k].minor.yy391 = v`.
pub(crate) fn set_u32(yyp: &mut YyParser, k: isize, v: u32) {
    yyp.msp(k).minor = YyMinor::Yy391(v);
}

/// `yymsp[k].minor.yy383`.
pub(crate) fn get_value_mask(yyp: &mut YyParser, k: isize) -> ValueMask {
    match &yyp.msp(k).minor {
        YyMinor::Yy383(v) => *v,
        _ => ValueMask::default(),
    }
}

/// `yymsp[k].minor.yy383 = {value, mask}`.
pub(crate) fn set_value_mask(yyp: &mut YyParser, k: isize, value: i32, mask: i32) {
    yyp.msp(k).minor = YyMinor::Yy383(ValueMask { value, mask });
}

/// `yymsp[k].minor.yy168`.
pub(crate) fn get_ofst(yyp: &mut YyParser, k: isize) -> i32 {
    match &yyp.msp(k).minor {
        YyMinor::Yy168(v) => *v,
        _ => 0,
    }
}

/// `yymsp[k].minor.yy0` (cópia: o token é pequeno e as ações o leem várias vezes).
pub(crate) fn get_tok(yyp: &mut YyParser, k: isize) -> Token {
    match &yyp.msp(k).minor {
        YyMinor::Yy0(t) => t.clone(),
        _ => empty_token(),
    }
}

/// `yymsp[k].minor.yy0 = t`.
pub(crate) fn set_tok(yyp: &mut YyParser, k: isize, t: Token) {
    yyp.msp(k).minor = YyMinor::Yy0(t);
}

/// `yymsp[k].minor.yy269` movido para fora (o ON ou USING).
pub(crate) fn take_on_using(yyp: &mut YyParser, k: isize) -> OnOrUsing {
    match &mut yyp.msp(k).minor {
        YyMinor::Yy269(v) => std::mem::take(v),
        _ => OnOrUsing::default(),
    }
}

/// `yymsp[k].major` como inteiro (o código do token terminal).
pub(crate) fn major_at(yyp: &mut YyParser, k: isize) -> i32 {
    yyp.msp(k).major as i32
}

/// O token vazio do C (`z = 0, n = 0`).
pub(crate) fn empty_token() -> Token {
    Token { z: Vec::new(), i_ofst: -1 }
}

/// O token do `NOT INDEXED` (`z = 0, n = 1` no C): um byte e deslocamento negativo.
pub(crate) fn not_indexed_token() -> Token {
    Token { z: vec![0], i_ofst: -1 }
}

/// `pIndexedBy->n==1 && !pIndexedBy->z`: o token é o do `NOT INDEXED`. Quem consome o token
/// (`sqlite3SrcListIndexedBy`) usa esta função no lugar do teste do C.
pub fn is_not_indexed_token(t: &Token) -> bool {
    t.z.len() == 1 && t.i_ofst < 0
}

/// O texto do SQL entre os deslocamentos `start` (incluído) e `end` (excluído): o
/// "de `zStart` até `zEnd`" do C. Fora do texto, vazio.
pub(crate) fn sql_span(p_parse: &Parse, start: i32, end: i32) -> Vec<u8> {
    if start < 0 || end <= start {
        return Vec::new();
    }
    let z = &p_parse.z_sql;
    let s = start as usize;
    if s >= z.len() {
        return Vec::new();
    }
    let e = (end as usize).min(z.len());
    z[s..e].to_vec()
}

/// O token que cobre do início de `first` até o fim de `last` (`n = fim - início` do C).
pub(crate) fn span_token(p_parse: &Parse, first: &Token, last: &Token) -> Token {
    let end = last.i_ofst.wrapping_add(last.z.len() as i32);
    Token { z: sql_span(p_parse, first.i_ofst, end), i_ofst: first.i_ofst }
}

// ---------------------------------------------------------------------------------------------
// Auxiliares do `%include` do parse.y
// ---------------------------------------------------------------------------------------------

/// `disableLookaside`: desliga o lookaside para objetos que podem ser compartilhados entre
/// conexões (`DisableLookaside` do C).
pub(crate) fn disable_lookaside(db: &mut Connection, p_parse: &mut Parse) {
    p_parse.disable_lookaside = p_parse.disable_lookaside.wrapping_add(1);
    db.lookaside.b_disable = db.lookaside.b_disable.wrapping_add(1);
    db.lookaside.sz = 0;
}

/// `parserDoubleLinkSelect`: num SELECT composto, garante `p->pPrior->pNext==p` em todos os
/// elementos (aqui `has_next` do select à esquerda) e que o tamanho da lista não passa de
/// `SQLITE_LIMIT_COMPOUND_SELECT`.
pub(crate) fn parser_double_link_select(db: &mut Connection, p_parse: &mut Parse, p: &mut Select) {
    if p.p_prior.is_none() {
        return;
    }
    let sel_flags = p.sel_flags;
    let mut cnt: i32 = 1;
    let mut has_next = false;
    let mut cur: &mut Select = p;
    loop {
        cur.has_next = has_next;
        cur.sel_flags |= SF_COMPOUND;
        has_next = true;
        let next_op = cur.op;
        match cur.p_prior.as_deref_mut() {
            None => break,
            Some(prior) => {
                cnt += 1;
                if prior.p_order_by.is_some() || prior.p_limit.is_some() {
                    let clause: &[u8] =
                        if prior.p_order_by.is_some() { b"ORDER BY" } else { b"LIMIT" };
                    error_msg(
                        db,
                        p_parse,
                        b"%s clause should come after %s not before",
                        &[
                            PrintfArg::Text(Some(clause.to_vec())),
                            PrintfArg::Text(Some(select_op_name(next_op as i32).as_bytes().to_vec())),
                        ],
                    );
                    break;
                }
                cur = prior;
            }
        }
    }
    let mx_select = db.a_limit[SQLITE_LIMIT_COMPOUND_SELECT as usize];
    if (sel_flags & (SF_MULTIVALUE | SF_VALUES)) == 0 && mx_select > 0 && cnt > mx_select {
        error_msg(db, p_parse, b"too many terms in compound SELECT", &[]);
    }
}

/// `attachWithToSelect`: prende a cláusula WITH ao SELECT que ela prefixa.
pub(crate) fn attach_with_to_select(
    db: &mut Connection,
    p_parse: &mut Parse,
    p_select: Option<Box<Select>>,
    p_with: Option<Box<With>>,
) -> Option<Box<Select>> {
    match p_select {
        Some(mut s) => {
            s.p_with = p_with;
            parser_double_link_select(db, p_parse, &mut s);
            Some(s)
        }
        None => {
            drop(p_with);
            None
        }
    }
}

/// `tokenExpr`: um `Expr` novo a partir de um único token. O `db` do C só servia de alocador.
pub(crate) fn token_expr(p_parse: &mut Parse, op: i32, t: &Token) -> Option<Box<Expr>> {
    let mut p = Box::new(Expr::default());
    p.op = op as u8;
    p.aff_expr = 0;
    // `p->flags = EP_Leaf` seguido de `ExprClearVVAProperties(p)`: nenhum bit de EP_Leaf é uma
    // propriedade VVA, então o limpa-propriedades não muda nada.
    p.flags = EP_LEAF;
    p.i_agg = -1;
    p.u = ExprU::Token(Some(t.z.clone()));
    p.w = t.i_ofst.wrapping_sub(p_parse.z_tail as i32);
    if t.z.first().map_or(false, |&c| is_quote(c)) {
        dequote_expr(&mut p);
    }
    p.n_height = 1;
    if p_parse.in_rename_object() {
        let addr = &*p as *const Expr as usize;
        rename_token_map(p_parse, addr, t);
    }
    Some(p)
}

/// `binaryToUnaryIfNull`: converte um TK_IS ou TK_ISNOT binário em TK_ISNULL ou TK_NOTNULL
/// unário. No C `pY` é o `pRight` de `pA` (o nó foi criado por `sqlite3PExpr(op, ?, pY)`), então
/// aqui só se passa `p_a` e o operando é `p_a.p_right`.
pub(crate) fn binary_to_unary_if_null(p_parse: &Parse, p_a: Option<&mut Expr>, op: u8) {
    if let Some(a) = p_a {
        let y_is_null = a.p_right.as_deref().map_or(false, |y| y.op == crate::consts::TK_NULL);
        if y_is_null && !p_parse.in_rename_object() {
            a.op = op;
            a.p_right = None;
        }
    }
}

/// `parserAddExprIdListTerm`: acrescenta um termo a uma `ExprList` usada como lista de
/// identificadores. Dá erro se há COLLATE, ASC ou DESC, exceto ao analisar um esquema antigo.
pub(crate) fn parser_add_expr_id_list_term(
    db: &mut Connection,
    p_parse: &mut Parse,
    p_prior: Option<Box<ExprList>>,
    p_id_token: &Token,
    has_collate: i32,
    sort_order: i32,
) -> Option<Box<ExprList>> {
    let mut p = expr_list_append(p_prior, None);
    if (has_collate != 0 || sort_order != SQLITE_SO_UNDEFINED) && db.init.busy == 0 {
        error_msg(
            db,
            p_parse,
            b"syntax error after column name \"%.*s\"",
            &[
                PrintfArg::Int(p_id_token.z.len() as i64),
                PrintfArg::Text(Some(p_id_token.z.clone())),
            ],
        );
    }
    expr_list_set_name(p_parse, p.as_deref_mut(), p_id_token, 1);
    p
}

// ---------------------------------------------------------------------------------------------
// yy_reduce
// ---------------------------------------------------------------------------------------------

/// `yy_reduce`: executa a ação semântica da regra `yyruleno` e fecha a redução (`yy_reduce_goto`).
/// `yy_lookahead` é o token à frente (ou `YYNOCODE` se já consumido) e `yy_lookahead_token` o
/// valor dele. As regras acima de 200 ficam em `crate::parse_reduce2::yy_reduce_tail`.
///
/// Os `case` do C que dividem um corpo entre regras de lados diferentes de 200 ficam inteiros
/// aqui (as etiquetas 201 a 204, 219, 222, 231 a 234, 237, 242, 246, 247, 251, 252, 258, 259 e
/// 324); a cauda nunca é chamada para elas.
pub fn yy_reduce(
    yyp: &mut YyParser,
    yyruleno: u32,
    yy_lookahead: i32,
    yy_lookahead_token: &Token,
    p_parse: &mut Parse,
    db: &mut Connection,
) -> u16 {
    match yyruleno {
        // explain ::= EXPLAIN
        0 => {
            if p_parse.p_reprepare.is_none() {
                p_parse.explain = 1;
            }
        }
        // explain ::= EXPLAIN QUERY PLAN
        1 => {
            if p_parse.p_reprepare.is_none() {
                p_parse.explain = 2;
            }
        }
        // cmdx ::= cmd
        2 => finish_coding(db, p_parse),
        // cmd ::= BEGIN transtype trans_opt
        3 => {
            let ty = get_i32(yyp, -1);
            begin_transaction(db, p_parse, ty);
        }
        // transtype ::=
        4 => set_i32(yyp, 1, TK_DEFERRED as i32),
        // transtype ::= DEFERRED|IMMEDIATE|EXCLUSIVE; range_or_rows ::= RANGE|ROWS|GROUPS
        5 | 6 | 7 | 324 => {
            let m = major_at(yyp, 0);
            set_i32(yyp, 0, m);
        }
        // cmd ::= COMMIT|END trans_opt; cmd ::= ROLLBACK trans_opt
        8 | 9 => {
            let m = major_at(yyp, -1);
            end_transaction(db, p_parse, m);
        }
        // cmd ::= SAVEPOINT nm
        10 => {
            let t = get_tok(yyp, 0);
            savepoint(db, p_parse, SAVEPOINT_BEGIN, &t);
        }
        // cmd ::= RELEASE savepoint_opt nm
        11 => {
            let t = get_tok(yyp, 0);
            savepoint(db, p_parse, SAVEPOINT_RELEASE, &t);
        }
        // cmd ::= ROLLBACK trans_opt TO savepoint_opt nm
        12 => {
            let t = get_tok(yyp, 0);
            savepoint(db, p_parse, SAVEPOINT_ROLLBACK, &t);
        }
        // create_table ::= createkw temp TABLE ifnotexists nm dbnm
        13 => {
            let name1 = get_tok(yyp, -1);
            let name2 = get_tok(yyp, 0);
            let is_temp = get_i32(yyp, -4);
            let no_err = get_i32(yyp, -2);
            start_table(db, p_parse, &name1, &name2, is_temp, 0, 0, no_err);
        }
        // createkw ::= CREATE
        14 => disable_lookaside(db, p_parse),
        // ifnotexists ::=; temp ::=; autoinc ::=; init_deferred_pred_opt ::=;
        // defer_subclause_opt ::=; ifexists ::=; distinct ::=; collate ::=
        15 | 18 | 47 | 62 | 72 | 81 | 100 | 246 => set_i32(yyp, 1, 0),
        // ifnotexists ::= IF NOT EXISTS
        16 => set_i32(yyp, -2, 1),
        // temp ::= TEMP
        17 => set_i32(yyp, 0, (db.init.busy == 0) as i32),
        // create_table_args ::= LP columnlist conslist_opt RP table_option_set
        19 => {
            let cons = get_tok(yyp, -2);
            let end = get_tok(yyp, -1);
            let tab_opts = get_u32(yyp, 0);
            end_table(db, p_parse, Some(&cons), Some(&end), tab_opts, None);
        }
        // create_table_args ::= AS select
        20 => {
            let mut sel = take_select(yyp, 0);
            end_table(db, p_parse, None, None, 0, sel.as_deref_mut());
            drop(sel);
        }
        // table_option_set ::=
        21 => set_u32(yyp, 1, 0),
        // table_option_set ::= table_option_set COMMA table_option
        22 => {
            let v = get_u32(yyp, -2) | get_u32(yyp, 0);
            set_u32(yyp, -2, v);
        }
        // table_option ::= WITHOUT nm
        23 => {
            let t = get_tok(yyp, 0);
            if t.z.len() == 5 && strnicmp(Some(t.z.as_slice()), Some(&b"rowid"[..]), 5) == 0 {
                set_u32(yyp, -1, TF_WITHOUT_ROWID | TF_NO_VISIBLE_ROWID);
            } else {
                set_u32(yyp, -1, 0);
                error_msg(
                    db,
                    p_parse,
                    b"unknown table option: %.*s",
                    &[PrintfArg::Int(t.z.len() as i64), PrintfArg::Text(Some(t.z.clone()))],
                );
            }
        }
        // table_option ::= nm
        24 => {
            let t = get_tok(yyp, 0);
            if t.z.len() == 6 && strnicmp(Some(t.z.as_slice()), Some(&b"strict"[..]), 6) == 0 {
                set_u32(yyp, 0, TF_STRICT);
            } else {
                set_u32(yyp, 0, 0);
                error_msg(
                    db,
                    p_parse,
                    b"unknown table option: %.*s",
                    &[PrintfArg::Int(t.z.len() as i64), PrintfArg::Text(Some(t.z.clone()))],
                );
            }
        }
        // columnname ::= nm typetoken
        25 => {
            let name = get_tok(yyp, -1);
            let ty = get_tok(yyp, 0);
            add_column(db, p_parse, name, ty);
        }
        // typetoken ::=; conslist_opt ::=; as ::=
        26 | 65 | 106 => set_tok(yyp, 1, empty_token()),
        // typetoken ::= typename LP signed RP
        27 => {
            let first = get_tok(yyp, -3);
            let last = get_tok(yyp, 0);
            let t = span_token(p_parse, &first, &last);
            set_tok(yyp, -3, t);
        }
        // typetoken ::= typename LP signed COMMA signed RP
        28 => {
            let first = get_tok(yyp, -5);
            let last = get_tok(yyp, 0);
            let t = span_token(p_parse, &first, &last);
            set_tok(yyp, -5, t);
        }
        // typename ::= typename ID|STRING
        29 => {
            let first = get_tok(yyp, -1);
            let last = get_tok(yyp, 0);
            let t = span_token(p_parse, &first, &last);
            set_tok(yyp, -1, t);
        }
        // scanpt ::=
        30 => {
            debug_assert!(yy_lookahead != YYNOCODE as i32);
            set_ofst(yyp, 1, yy_lookahead_token.i_ofst);
        }
        // scantok ::=
        31 => {
            debug_assert!(yy_lookahead != YYNOCODE as i32);
            set_tok(yyp, 1, yy_lookahead_token.clone());
        }
        // ccons ::= CONSTRAINT nm; tcons ::= CONSTRAINT nm
        32 | 67 => p_parse.constraint_name = get_tok(yyp, 0),
        // ccons ::= DEFAULT scantok term
        33 => {
            let e = take_expr(yyp, 0);
            let scantok = get_tok(yyp, -1);
            add_default_value(db, p_parse, e, &scantok.z);
        }
        // ccons ::= DEFAULT LP expr RP
        34 => {
            let e = take_expr(yyp, -1);
            let lp = get_tok(yyp, -2);
            let rp = get_tok(yyp, 0);
            let span = sql_span(p_parse, lp.i_ofst.wrapping_add(1), rp.i_ofst);
            add_default_value(db, p_parse, e, &span);
        }
        // ccons ::= DEFAULT PLUS scantok term
        35 => {
            let e = take_expr(yyp, 0);
            let plus = get_tok(yyp, -2);
            let scantok = get_tok(yyp, -1);
            let span = span_token(p_parse, &plus, &scantok).z;
            add_default_value(db, p_parse, e, &span);
        }
        // ccons ::= DEFAULT MINUS scantok term
        36 => {
            let term = take_expr(yyp, 0);
            let p = p_expr(db, p_parse, TK_UMINUS as i32, term, None);
            let minus = get_tok(yyp, -2);
            let scantok = get_tok(yyp, -1);
            let span = span_token(p_parse, &minus, &scantok).z;
            add_default_value(db, p_parse, p, &span);
        }
        // ccons ::= DEFAULT scantok ID|INDEXED
        37 => {
            let t = get_tok(yyp, 0);
            let mut p = token_expr(p_parse, TK_STRING as i32, &t);
            if let Some(e) = p.as_deref_mut() {
                expr_id_to_true_false(e);
            }
            add_default_value(db, p_parse, p, &t.z);
        }
        // ccons ::= NOT NULL onconf
        38 => {
            let on_error = get_i32(yyp, 0);
            add_not_null(p_parse, on_error);
        }
        // ccons ::= PRIMARY KEY sortorder onconf autoinc
        39 => {
            let sort_order = get_i32(yyp, -2);
            let on_error = get_i32(yyp, -1);
            let auto_inc = get_i32(yyp, 0);
            add_primary_key(db, p_parse, None, on_error, auto_inc, sort_order);
        }
        // ccons ::= UNIQUE onconf
        40 => {
            let on_error = get_i32(yyp, 0);
            create_index(
                db,
                p_parse,
                None,
                None,
                None,
                None,
                on_error,
                None,
                None,
                0,
                0,
                SQLITE_IDXTYPE_UNIQUE,
            );
        }
        // ccons ::= CHECK LP expr RP
        41 => {
            let e = take_expr(yyp, -1);
            let lp = get_tok(yyp, -2);
            let rp = get_tok(yyp, 0);
            let span = Token { z: sql_span(p_parse, lp.i_ofst, rp.i_ofst), i_ofst: lp.i_ofst };
            add_check_constraint(db, p_parse, e, &span);
        }
        // ccons ::= REFERENCES nm eidlist_opt refargs
        42 => {
            let to = get_tok(yyp, -2);
            let to_col = take_list(yyp, -1);
            let flags = get_i32(yyp, 0);
            create_foreign_key(db, p_parse, None, &to, to_col, flags);
        }
        // ccons ::= defer_subclause
        43 => {
            let is_deferred = get_i32(yyp, 0);
            defer_foreign_key(db, p_parse, is_deferred);
        }
        // ccons ::= COLLATE ID|STRING
        44 => {
            let t = get_tok(yyp, 0);
            add_collate_type(db, p_parse, &t);
        }
        // generated ::= LP expr RP
        45 => {
            let e = take_expr(yyp, -1);
            add_generated(db, p_parse, e, None);
        }
        // generated ::= LP expr RP ID
        46 => {
            let e = take_expr(yyp, -2);
            let t = get_tok(yyp, 0);
            add_generated(db, p_parse, e, Some(&t));
        }
        // autoinc ::= AUTOINCR
        48 => set_i32(yyp, 0, 1),
        // refargs ::=
        49 => set_i32(yyp, 1, OE_NONE as i32 * 0x0101),
        // refargs ::= refargs refarg
        50 => {
            let vm = get_value_mask(yyp, 0);
            let v = (get_i32(yyp, -1) & !vm.mask) | vm.value;
            set_i32(yyp, -1, v);
        }
        // refarg ::= MATCH nm
        51 => set_value_mask(yyp, -1, 0, 0x000000),
        // refarg ::= ON INSERT refact
        52 => set_value_mask(yyp, -2, 0, 0x000000),
        // refarg ::= ON DELETE refact
        53 => {
            let v = get_i32(yyp, 0);
            set_value_mask(yyp, -2, v, 0x0000ff);
        }
        // refarg ::= ON UPDATE refact
        54 => {
            let v = get_i32(yyp, 0);
            set_value_mask(yyp, -2, v << 8, 0x00ff00);
        }
        // refact ::= SET NULL
        55 => set_i32(yyp, -1, OE_SET_NULL as i32),
        // refact ::= SET DEFAULT
        56 => set_i32(yyp, -1, OE_SET_DFLT as i32),
        // refact ::= CASCADE
        57 => set_i32(yyp, 0, OE_CASCADE as i32),
        // refact ::= RESTRICT
        58 => set_i32(yyp, 0, OE_RESTRICT as i32),
        // refact ::= NO ACTION
        59 => set_i32(yyp, -1, OE_NONE as i32),
        // defer_subclause ::= NOT DEFERRABLE init_deferred_pred_opt
        60 => set_i32(yyp, -2, 0),
        // defer_subclause ::= DEFERRABLE init_deferred_pred_opt; orconf ::= OR resolvetype;
        // insert_cmd ::= INSERT orconf
        61 | 76 | 173 => {
            let v = get_i32(yyp, 0);
            set_i32(yyp, -1, v);
        }
        // init_deferred_pred_opt ::= INITIALLY DEFERRED; ifexists ::= IF EXISTS;
        // between_op ::= NOT BETWEEN; in_op ::= NOT IN; collate ::= COLLATE ID|STRING
        63 | 80 | 219 | 222 | 247 => set_i32(yyp, -1, 1),
        // init_deferred_pred_opt ::= INITIALLY IMMEDIATE
        64 => set_i32(yyp, -1, 0),
        // tconscomma ::= COMMA
        66 => p_parse.constraint_name.z.clear(),
        // tcons ::= PRIMARY KEY LP sortlist autoinc RP onconf
        68 => {
            let list = take_list(yyp, -3);
            let on_error = get_i32(yyp, 0);
            let auto_inc = get_i32(yyp, -2);
            add_primary_key(db, p_parse, list, on_error, auto_inc, 0);
        }
        // tcons ::= UNIQUE LP sortlist RP onconf
        69 => {
            let list = take_list(yyp, -2);
            let on_error = get_i32(yyp, 0);
            create_index(
                db,
                p_parse,
                None,
                None,
                None,
                list,
                on_error,
                None,
                None,
                0,
                0,
                SQLITE_IDXTYPE_UNIQUE,
            );
        }
        // tcons ::= CHECK LP expr RP onconf
        70 => {
            let e = take_expr(yyp, -2);
            let lp = get_tok(yyp, -3);
            let rp = get_tok(yyp, -1);
            let span = Token { z: sql_span(p_parse, lp.i_ofst, rp.i_ofst), i_ofst: lp.i_ofst };
            add_check_constraint(db, p_parse, e, &span);
        }
        // tcons ::= FOREIGN KEY LP eidlist RP REFERENCES nm eidlist_opt refargs defer_subclause_opt
        71 => {
            let from_col = take_list(yyp, -6);
            let to = get_tok(yyp, -3);
            let to_col = take_list(yyp, -2);
            let flags = get_i32(yyp, -1);
            create_foreign_key(db, p_parse, from_col, &to, to_col, flags);
            let is_deferred = get_i32(yyp, 0);
            defer_foreign_key(db, p_parse, is_deferred);
        }
        // onconf ::=; orconf ::=
        73 | 75 => set_i32(yyp, 1, OE_DEFAULT as i32),
        // onconf ::= ON CONFLICT resolvetype
        74 => {
            let v = get_i32(yyp, 0);
            set_i32(yyp, -2, v);
        }
        // resolvetype ::= IGNORE
        77 => set_i32(yyp, 0, OE_IGNORE as i32),
        // resolvetype ::= REPLACE; insert_cmd ::= REPLACE
        78 | 174 => set_i32(yyp, 0, OE_REPLACE as i32),
        // cmd ::= DROP TABLE ifexists fullname
        79 => {
            let name = take_src(yyp, 0);
            let no_err = get_i32(yyp, -1);
            if let Some(name) = name {
                drop_table(db, p_parse, name, 0, no_err);
            };
        }
        // cmd ::= createkw temp VIEW ifnotexists nm dbnm eidlist_opt AS select
        82 => {
            let begin = get_tok(yyp, -8);
            let name1 = get_tok(yyp, -4);
            let name2 = get_tok(yyp, -3);
            let cols = take_list(yyp, -2);
            let sel = take_select(yyp, 0);
            let is_temp = get_i32(yyp, -7);
            let no_err = get_i32(yyp, -5);
            create_view(db, p_parse, &begin, &name1, &name2, cols, sel, is_temp, no_err);
        }
        // cmd ::= DROP VIEW ifexists fullname
        83 => {
            let name = take_src(yyp, 0);
            let no_err = get_i32(yyp, -1);
            if let Some(name) = name {
                drop_table(db, p_parse, name, 1, no_err);
            };
        }
        // cmd ::= select
        84 => {
            let mut dest = SelectDest { e_dest: SRT_OUTPUT, ..SelectDest::default() };
            let mut sel = take_select(yyp, 0);
            if let Some(s) = sel.as_deref_mut() {
                select(db, p_parse, s, &mut dest);
            }
            drop(sel);
        }
        // select ::= WITH wqlist selectnowith
        85 => {
            let sel = take_select(yyp, 0);
            let with = take_with(yyp, -1);
            let r = attach_with_to_select(db, p_parse, sel, with);
            set_select(yyp, -2, r);
        }
        // select ::= WITH RECURSIVE wqlist selectnowith
        86 => {
            let sel = take_select(yyp, 0);
            let with = take_with(yyp, -1);
            let r = attach_with_to_select(db, p_parse, sel, with);
            set_select(yyp, -3, r);
        }
        // select ::= selectnowith
        87 => {
            if let YyMinor::Yy555(Some(p)) = &mut yyp.msp(0).minor {
                parser_double_link_select(db, p_parse, p);
            }
        }
        // selectnowith ::= selectnowith multiselect_op oneselect
        88 => {
            let mut p_rhs = take_select(yyp, 0);
            let p_lhs = take_select(yyp, -2);
            let op = get_i32(yyp, -1);
            if p_rhs.as_ref().map_or(false, |r| r.p_prior.is_some()) {
                if let Some(r) = p_rhs.as_deref_mut() {
                    parser_double_link_select(db, p_parse, r);
                }
                let x = empty_token();
                let p_from = src_list_append_from_term(
                    db,
                    p_parse,
                    None,
                    None,
                    None,
                    Some(&x),
                    p_rhs.take(),
                    None,
                );
                p_rhs = select_new(p_parse, None, p_from, None, None, None, None, 0, None);
            }
            match p_rhs.as_deref_mut() {
                Some(rhs) => {
                    rhs.op = op as u8;
                    rhs.p_prior = p_lhs;
                    if let Some(l) = rhs.p_prior.as_deref_mut() {
                        l.sel_flags &= !SF_MULTIVALUE;
                    }
                    rhs.sel_flags &= !SF_MULTIVALUE;
                    if op != TK_ALL as i32 {
                        p_parse.has_compound = 1;
                    }
                }
                None => drop(p_lhs),
            }
            set_select(yyp, -2, p_rhs);
        }
        // multiselect_op ::= UNION; multiselect_op ::= EXCEPT|INTERSECT
        89 | 91 => {
            let m = major_at(yyp, 0);
            set_i32(yyp, 0, m);
        }
        // multiselect_op ::= UNION ALL
        90 => set_i32(yyp, -1, TK_ALL as i32),
        // oneselect ::= SELECT distinct selcollist from where_opt groupby_opt having_opt
        //               orderby_opt limit_opt
        92 => {
            let e_list = take_list(yyp, -6);
            let src = take_src(yyp, -5);
            let where_ = take_expr(yyp, -4);
            let group_by = take_list(yyp, -3);
            let having = take_expr(yyp, -2);
            let order_by = take_list(yyp, -1);
            let distinct = get_i32(yyp, -7) as u32;
            let limit = take_expr(yyp, 0);
            let r = select_new(
                p_parse, e_list, src, where_, group_by, having, order_by, distinct, limit,
            );
            set_select(yyp, -8, r);
        }
        // oneselect ::= SELECT distinct selcollist from where_opt groupby_opt having_opt
        //               window_clause orderby_opt limit_opt
        93 => {
            let e_list = take_list(yyp, -7);
            let src = take_src(yyp, -6);
            let where_ = take_expr(yyp, -5);
            let group_by = take_list(yyp, -4);
            let having = take_expr(yyp, -3);
            let order_by = take_list(yyp, -1);
            let distinct = get_i32(yyp, -8) as u32;
            let limit = take_expr(yyp, 0);
            let mut r = select_new(
                p_parse, e_list, src, where_, group_by, having, order_by, distinct, limit,
            );
            let win = take_window(yyp, -2);
            match r.as_deref_mut() {
                Some(sel) => sel.p_win_defn = window_list_to_vec(win),
                None => drop(win),
            }
            set_select(yyp, -9, r);
        }
        // values ::= VALUES LP nexprlist RP
        94 => {
            let list = take_list(yyp, -1);
            let r = select_new(p_parse, list, None, None, None, None, None, SF_VALUES, None);
            set_select(yyp, -3, r);
        }
        // oneselect ::= mvalues
        95 => {
            if let YyMinor::Yy555(s) = &mut yyp.msp(0).minor {
                if let Some(s) = s.as_deref() {
                    multi_values_end(p_parse, s);
                }
            }
        }
        // mvalues ::= values COMMA LP nexprlist RP; mvalues ::= mvalues COMMA LP nexprlist RP
        96 | 97 => {
            let left = take_select(yyp, -4);
            let row = take_list(yyp, -1);
            let r = match (left, row) {
                (Some(l), Some(r)) => Some(multi_values(db, p_parse, l, r)),
                _ => None,
            };
            set_select(yyp, -4, r);
        }
        // distinct ::= DISTINCT
        98 => set_i32(yyp, 0, SF_DISTINCT as i32),
        // distinct ::= ALL
        99 => set_i32(yyp, 0, SF_ALL as i32),
        // sclp ::=; orderby_opt ::=; groupby_opt ::=; exprlist ::=; paren_exprlist ::=;
        // eidlist_opt ::=
        101 | 134 | 144 | 234 | 237 | 242 => set_list(yyp, 1, None),
        // selcollist ::= sclp scanpt expr scanpt as
        102 => {
            let prior = take_list(yyp, -4);
            let e = take_expr(yyp, -2);
            let mut list = expr_list_append(prior, e);
            let as_tok = get_tok(yyp, 0);
            if !as_tok.z.is_empty() {
                expr_list_set_name(p_parse, list.as_deref_mut(), &as_tok, 1);
            }
            let start = get_ofst(yyp, -3);
            let end = get_ofst(yyp, -1);
            let span = sql_span(p_parse, start, end);
            expr_list_set_span(list.as_deref_mut(), &span);
            set_list(yyp, -4, list);
        }
        // selcollist ::= sclp scanpt STAR
        103 => {
            let mut p = expr(TK_ASTERISK as i32, None);
            let star = get_tok(yyp, 0);
            expr_set_error_offset(p.as_deref_mut(), star.i_ofst.wrapping_sub(p_parse.z_tail as i32));
            let prior = take_list(yyp, -2);
            let list = expr_list_append(prior, p);
            set_list(yyp, -2, list);
        }
        // selcollist ::= sclp scanpt nm DOT STAR
        104 => {
            let mut p_right = p_expr(db, p_parse, TK_ASTERISK as i32, None, None);
            let star = get_tok(yyp, 0);
            expr_set_error_offset(
                p_right.as_deref_mut(),
                star.i_ofst.wrapping_sub(p_parse.z_tail as i32),
            );
            let nm = get_tok(yyp, -2);
            let p_left = token_expr(p_parse, TK_ID as i32, &nm);
            let p_dot = p_expr(db, p_parse, TK_DOT as i32, p_left, p_right);
            let prior = take_list(yyp, -4);
            let list = expr_list_append(prior, p_dot);
            set_list(yyp, -4, list);
        }
        // as ::= AS nm; dbnm ::= DOT nm; plus_num ::= PLUS INTEGER|FLOAT;
        // minus_num ::= MINUS INTEGER|FLOAT
        105 | 117 | 258 | 259 => {
            let t = get_tok(yyp, 0);
            set_tok(yyp, -1, t);
        }
        // from ::=; stl_prefix ::=
        107 | 110 => set_src(yyp, 1, None),
        // from ::= FROM seltablist
        108 => {
            let mut l = take_src(yyp, 0);
            src_list_shift_join_type(db, p_parse, l.as_deref_mut());
            set_src(yyp, -1, l);
        }
        // stl_prefix ::= seltablist joinop
        109 => {
            let jt = get_i32(yyp, 0);
            if let YyMinor::Yy203(Some(l)) = &mut yyp.msp(-1).minor {
                if let Some(last) = l.a.last_mut() {
                    last.fg.jointype = jt as u8;
                }
            }
        }
        // seltablist ::= stl_prefix nm dbnm as on_using
        111 => {
            let prior = take_src(yyp, -4);
            let table = get_tok(yyp, -3);
            let database = get_tok(yyp, -2);
            let alias = get_tok(yyp, -1);
            let on_using = take_on_using(yyp, 0);
            let r = src_list_append_from_term(
                db,
                p_parse,
                prior,
                Some(&table),
                Some(&database),
                Some(&alias),
                None,
                Some(on_using),
            );
            set_src(yyp, -4, r);
        }
        // seltablist ::= stl_prefix nm dbnm as indexed_by on_using
        112 => {
            let prior = take_src(yyp, -5);
            let table = get_tok(yyp, -4);
            let database = get_tok(yyp, -3);
            let alias = get_tok(yyp, -2);
            let on_using = take_on_using(yyp, 0);
            let mut r = src_list_append_from_term(
                db,
                p_parse,
                prior,
                Some(&table),
                Some(&database),
                Some(&alias),
                None,
                Some(on_using),
            );
            let indexed_by = get_tok(yyp, -1);
            src_list_indexed_by(db, p_parse, r.as_deref_mut(), &indexed_by);
            set_src(yyp, -5, r);
        }
        // seltablist ::= stl_prefix nm dbnm LP exprlist RP as on_using
        113 => {
            let prior = take_src(yyp, -7);
            let table = get_tok(yyp, -6);
            let database = get_tok(yyp, -5);
            let alias = get_tok(yyp, -1);
            let on_using = take_on_using(yyp, 0);
            let mut r = src_list_append_from_term(
                db,
                p_parse,
                prior,
                Some(&table),
                Some(&database),
                Some(&alias),
                None,
                Some(on_using),
            );
            let args = take_list(yyp, -3);
            src_list_func_args(db, p_parse, r.as_deref_mut(), args);
            set_src(yyp, -7, r);
        }
        // seltablist ::= stl_prefix LP select RP as on_using
        114 => {
            let prior = take_src(yyp, -5);
            let sub = take_select(yyp, -3);
            let alias = get_tok(yyp, -1);
            let on_using = take_on_using(yyp, 0);
            let r = src_list_append_from_term(
                db,
                p_parse,
                prior,
                None,
                None,
                Some(&alias),
                sub,
                Some(on_using),
            );
            set_src(yyp, -5, r);
        }
        // seltablist ::= stl_prefix LP seltablist RP as on_using
        115 => {
            let prior = take_src(yyp, -5);
            let inner = take_src(yyp, -3);
            let alias = get_tok(yyp, -1);
            let on_using = take_on_using(yyp, 0);
            let result: Option<Box<SrcList>>;
            if prior.is_none()
                && alias.z.is_empty()
                && on_using.p_on.is_none()
                && on_using.p_using.is_none()
            {
                result = inner;
            } else if inner.as_ref().map_or(false, |l| l.a.len() == 1) {
                let mut inner = inner;
                let mut r = src_list_append_from_term(
                    db,
                    p_parse,
                    prior,
                    None,
                    None,
                    Some(&alias),
                    None,
                    Some(on_using),
                );
                if let (Some(new_list), Some(old_list)) = (r.as_deref_mut(), inner.as_deref_mut()) {
                    if let (Some(p_new), Some(p_old)) =
                        (new_list.a.last_mut(), old_list.a.first_mut())
                    {
                        p_new.z_name = p_old.z_name.take();
                        p_new.z_database = p_old.z_database.take();
                        p_new.p_select = p_old.p_select.take();
                        if p_new
                            .p_select
                            .as_deref()
                            .map_or(false, |s| (s.sel_flags & SF_NESTEDFROM) != 0)
                        {
                            p_new.fg.is_nested_from = true;
                        }
                        if p_old.fg.is_tab_func {
                            p_new.u1 = std::mem::take(&mut p_old.u1);
                            p_old.fg.is_tab_func = false;
                            p_new.fg.is_tab_func = true;
                        }
                    }
                }
                // `sqlite3SrcListDelete(db, inner)`: o `Drop` de `inner`.
                drop(inner);
                result = r;
            } else {
                let mut inner = inner;
                src_list_shift_join_type(db, p_parse, inner.as_deref_mut());
                let sub =
                    select_new(p_parse, None, inner, None, None, None, None, SF_NESTEDFROM, None);
                result = src_list_append_from_term(
                    db,
                    p_parse,
                    prior,
                    None,
                    None,
                    Some(&alias),
                    sub,
                    Some(on_using),
                );
            }
            set_src(yyp, -5, result);
        }
        // dbnm ::=; indexed_opt ::=
        116 | 131 => set_tok(yyp, 1, empty_token()),
        // fullname ::= nm
        118 => {
            let t = get_tok(yyp, 0);
            let r = src_list_append(db, p_parse, None, Some(&t), None);
            rename_first_src_name(p_parse, r.as_deref(), &t);
            set_src(yyp, 0, r);
        }
        // fullname ::= nm DOT nm
        119 => {
            let t1 = get_tok(yyp, -2);
            let t2 = get_tok(yyp, 0);
            let r = src_list_append(db, p_parse, None, Some(&t1), Some(&t2));
            rename_first_src_name(p_parse, r.as_deref(), &t2);
            set_src(yyp, -2, r);
        }
        // xfullname ::= nm
        120 => {
            let t = get_tok(yyp, 0);
            let r = src_list_append(db, p_parse, None, Some(&t), None);
            set_src(yyp, 0, r);
        }
        // xfullname ::= nm DOT nm
        121 => {
            let t1 = get_tok(yyp, -2);
            let t2 = get_tok(yyp, 0);
            let r = src_list_append(db, p_parse, None, Some(&t1), Some(&t2));
            set_src(yyp, -2, r);
        }
        // xfullname ::= nm DOT nm AS nm
        122 => {
            let t1 = get_tok(yyp, -4);
            let t2 = get_tok(yyp, -2);
            let alias = get_tok(yyp, 0);
            let mut r = src_list_append(db, p_parse, None, Some(&t1), Some(&t2));
            if let Some(item) = r.as_deref_mut().and_then(|l| l.a.first_mut()) {
                item.z_alias = name_from_token(Some(&alias));
            }
            set_src(yyp, -4, r);
        }
        // xfullname ::= nm AS nm
        123 => {
            let t1 = get_tok(yyp, -2);
            let alias = get_tok(yyp, 0);
            let mut r = src_list_append(db, p_parse, None, Some(&t1), None);
            if let Some(item) = r.as_deref_mut().and_then(|l| l.a.first_mut()) {
                item.z_alias = name_from_token(Some(&alias));
            }
            set_src(yyp, -2, r);
        }
        // joinop ::= COMMA|JOIN
        124 => set_i32(yyp, 0, JT_INNER as i32),
        // joinop ::= JOIN_KW JOIN
        125 => {
            let a = get_tok(yyp, -1);
            let r = join_type(db, p_parse, &a, None, None);
            set_i32(yyp, -1, r);
        }
        // joinop ::= JOIN_KW nm JOIN
        126 => {
            let a = get_tok(yyp, -2);
            let b = get_tok(yyp, -1);
            let r = join_type(db, p_parse, &a, Some(&b), None);
            set_i32(yyp, -2, r);
        }
        // joinop ::= JOIN_KW nm nm JOIN
        127 => {
            let a = get_tok(yyp, -3);
            let b = get_tok(yyp, -2);
            let c = get_tok(yyp, -1);
            let r = join_type(db, p_parse, &a, Some(&b), Some(&c));
            set_i32(yyp, -3, r);
        }
        // on_using ::= ON expr
        128 => {
            let e = take_expr(yyp, 0);
            yyp.msp(-1).minor = YyMinor::Yy269(OnOrUsing { p_on: e, p_using: None });
        }
        // on_using ::= USING LP idlist RP
        129 => {
            let l = take_id_list(yyp, -1);
            yyp.msp(-3).minor = YyMinor::Yy269(OnOrUsing { p_on: None, p_using: l });
        }
        // on_using ::=
        130 => yyp.msp(1).minor = YyMinor::Yy269(OnOrUsing::default()),
        // indexed_by ::= INDEXED BY nm
        132 => {
            let t = get_tok(yyp, 0);
            set_tok(yyp, -2, t);
        }
        // indexed_by ::= NOT INDEXED
        133 => set_tok(yyp, -1, not_indexed_token()),
        // orderby_opt ::= ORDER BY sortlist; groupby_opt ::= GROUP BY nexprlist
        135 | 145 => {
            let l = take_list(yyp, 0);
            set_list(yyp, -2, l);
        }
        // sortlist ::= sortlist COMMA expr sortorder nulls
        136 => {
            let prior = take_list(yyp, -4);
            let e = take_expr(yyp, -2);
            let mut list = expr_list_append(prior, e);
            let sort_order = get_i32(yyp, -1);
            let nulls = get_i32(yyp, 0);
            expr_list_set_sort_order(list.as_deref_mut(), sort_order, nulls);
            set_list(yyp, -4, list);
        }
        // sortlist ::= expr sortorder nulls
        137 => {
            let e = take_expr(yyp, -2);
            let mut list = expr_list_append(None, e);
            let sort_order = get_i32(yyp, -1);
            let nulls = get_i32(yyp, 0);
            expr_list_set_sort_order(list.as_deref_mut(), sort_order, nulls);
            set_list(yyp, -2, list);
        }
        // sortorder ::= ASC
        138 => set_i32(yyp, 0, SQLITE_SO_ASC),
        // sortorder ::= DESC
        139 => set_i32(yyp, 0, SQLITE_SO_DESC),
        // sortorder ::=; nulls ::=
        140 | 143 => set_i32(yyp, 1, SQLITE_SO_UNDEFINED),
        // nulls ::= NULLS FIRST
        141 => set_i32(yyp, -1, SQLITE_SO_ASC),
        // nulls ::= NULLS LAST
        142 => set_i32(yyp, -1, SQLITE_SO_DESC),
        // having_opt ::=; limit_opt ::=; where_opt ::=; where_opt_ret ::=; case_else ::=;
        // case_operand ::=; vinto ::=
        146 | 148 | 153 | 155 | 232 | 233 | 252 => set_expr(yyp, 1, None),
        // having_opt ::= HAVING expr; where_opt ::= WHERE expr; where_opt_ret ::= WHERE expr;
        // case_else ::= ELSE expr; vinto ::= INTO expr
        147 | 154 | 156 | 231 | 251 => {
            let e = take_expr(yyp, 0);
            set_expr(yyp, -1, e);
        }
        // limit_opt ::= LIMIT expr
        149 => {
            let e = take_expr(yyp, 0);
            let r = p_expr(db, p_parse, TK_LIMIT as i32, e, None);
            set_expr(yyp, -1, r);
        }
        // limit_opt ::= LIMIT expr OFFSET expr
        150 => {
            let l = take_expr(yyp, -2);
            let r = take_expr(yyp, 0);
            let e = p_expr(db, p_parse, TK_LIMIT as i32, l, r);
            set_expr(yyp, -3, e);
        }
        // limit_opt ::= LIMIT expr COMMA expr
        151 => {
            let l = take_expr(yyp, 0);
            let r = take_expr(yyp, -2);
            let e = p_expr(db, p_parse, TK_LIMIT as i32, l, r);
            set_expr(yyp, -3, e);
        }
        // cmd ::= with DELETE FROM xfullname indexed_opt where_opt_ret orderby_opt limit_opt
        // (gramática de `SQLITE_ENABLE_UPDATE_DELETE_LIMIT`: o `updateDeleteLimitError` do ramo
        // `#ifndef` não existe neste build)
        152 => {
            let mut tab = take_src(yyp, -4);
            let indexed_by = get_tok(yyp, -3);
            src_list_indexed_by(db, p_parse, tab.as_deref_mut(), &indexed_by);
            let where_ = take_expr(yyp, -2);
            let order_by = take_list(yyp, -1);
            let limit = take_expr(yyp, 0);
            crate::delete::delete_from(db, p_parse, tab, where_, order_by, limit);
        }
        // where_opt_ret ::= RETURNING selcollist
        157 => {
            let l = take_list(yyp, 0);
            add_returning(db, p_parse, l);
            set_expr(yyp, -1, None);
        }
        // where_opt_ret ::= WHERE expr RETURNING selcollist
        158 => {
            let l = take_list(yyp, 0);
            add_returning(db, p_parse, l);
            let e = take_expr(yyp, -2);
            set_expr(yyp, -3, e);
        }
        // cmd ::= with UPDATE orconf xfullname indexed_opt SET setlist from where_opt_ret orderby_opt limit_opt
        // (gramática de `SQLITE_ENABLE_UPDATE_DELETE_LIMIT`; o `sqlite3ExprListCheckLength` vem
        // depois do tratamento do `from`, como no `parse.c` gerado)
        159 => {
            let mut tab = take_src(yyp, -7);
            let indexed_by = get_tok(yyp, -6);
            src_list_indexed_by(db, p_parse, tab.as_deref_mut(), &indexed_by);
            let from = take_src(yyp, -3);
            if from.is_some() {
                let mut p_from_clause = from;
                if p_from_clause.as_ref().map_or(false, |l| l.a.len() > 1) {
                    let sub = select_new(
                        p_parse,
                        None,
                        p_from_clause,
                        None,
                        None,
                        None,
                        None,
                        SF_NESTEDFROM,
                        None,
                    );
                    let as_tok = empty_token();
                    p_from_clause = src_list_append_from_term(
                        db,
                        p_parse,
                        None,
                        None,
                        None,
                        Some(&as_tok),
                        sub,
                        None,
                    );
                }
                tab = src_list_append_list(db, p_parse, tab, p_from_clause);
            }
            let changes = take_list(yyp, -4);
            expr_list_check_length(db, p_parse, changes.as_deref(), b"set list");
            let where_ = take_expr(yyp, -2);
            let order_by = take_list(yyp, -1);
            let limit = take_expr(yyp, 0);
            let on_error = get_i32(yyp, -8);
            crate::update::update(db, p_parse, tab, changes, where_, on_error, order_by, limit, None);
        }
        // setlist ::= setlist COMMA nm EQ expr
        160 => {
            let prior = take_list(yyp, -4);
            let e = take_expr(yyp, 0);
            let mut list = expr_list_append(prior, e);
            let nm = get_tok(yyp, -2);
            expr_list_set_name(p_parse, list.as_deref_mut(), &nm, 1);
            set_list(yyp, -4, list);
        }
        // setlist ::= setlist COMMA LP idlist RP EQ expr
        161 => {
            let prior = take_list(yyp, -6);
            let cols = take_id_list(yyp, -3);
            let e = take_expr(yyp, 0);
            let list = expr_list_append_vector(db, p_parse, prior, cols, e);
            set_list(yyp, -6, list);
        }
        // setlist ::= nm EQ expr
        162 => {
            let e = take_expr(yyp, 0);
            let mut list = expr_list_append(None, e);
            let nm = get_tok(yyp, -2);
            expr_list_set_name(p_parse, list.as_deref_mut(), &nm, 1);
            set_list(yyp, -2, list);
        }
        // setlist ::= LP idlist RP EQ expr
        163 => {
            let cols = take_id_list(yyp, -3);
            let e = take_expr(yyp, 0);
            let list = expr_list_append_vector(db, p_parse, None, cols, e);
            set_list(yyp, -4, list);
        }
        // cmd ::= with insert_cmd INTO xfullname idlist_opt select upsert
        164 => {
            let tab = take_src(yyp, -3);
            let sel = take_select(yyp, -1);
            let cols = take_id_list(yyp, -2);
            let on_error = get_i32(yyp, -5);
            let up = take_upsert(yyp, 0);
            crate::insert::insert(db, p_parse, tab, sel, cols, on_error, up);
        }
        // cmd ::= with insert_cmd INTO xfullname idlist_opt DEFAULT VALUES returning
        165 => {
            let tab = take_src(yyp, -4);
            let cols = take_id_list(yyp, -3);
            let on_error = get_i32(yyp, -6);
            crate::insert::insert(db, p_parse, tab, None, cols, on_error, None);
        }
        // upsert ::=
        166 => set_upsert(yyp, 1, None),
        // upsert ::= RETURNING selcollist
        167 => {
            set_upsert(yyp, -1, None);
            let l = take_list(yyp, 0);
            add_returning(db, p_parse, l);
        }
        // upsert ::= ON CONFLICT LP sortlist RP where_opt DO UPDATE SET setlist where_opt upsert
        168 => {
            let target = take_list(yyp, -8);
            let target_where = take_expr(yyp, -6);
            let set = take_list(yyp, -2);
            let where_ = take_expr(yyp, -1);
            let next = take_upsert(yyp, 0);
            let r = crate::upsert::upsert_new(target, target_where, set, where_, next);
            set_upsert(yyp, -11, r);
        }
        // upsert ::= ON CONFLICT LP sortlist RP where_opt DO NOTHING upsert
        169 => {
            let target = take_list(yyp, -5);
            let target_where = take_expr(yyp, -3);
            let next = take_upsert(yyp, 0);
            let r = crate::upsert::upsert_new(target, target_where, None, None, next);
            set_upsert(yyp, -8, r);
        }
        // upsert ::= ON CONFLICT DO NOTHING returning
        170 => {
            let r = crate::upsert::upsert_new(None, None, None, None, None);
            set_upsert(yyp, -4, r);
        }
        // upsert ::= ON CONFLICT DO UPDATE SET setlist where_opt returning
        171 => {
            let set = take_list(yyp, -2);
            let where_ = take_expr(yyp, -1);
            let r = crate::upsert::upsert_new(None, None, set, where_, None);
            set_upsert(yyp, -7, r);
        }
        // returning ::= RETURNING selcollist
        172 => {
            let l = take_list(yyp, 0);
            add_returning(db, p_parse, l);
        }
        // idlist_opt ::=
        175 => set_id_list(yyp, 1, None),
        // idlist_opt ::= LP idlist RP
        176 => {
            let l = take_id_list(yyp, -1);
            set_id_list(yyp, -2, l);
        }
        // idlist ::= idlist COMMA nm
        177 => {
            let prior = take_id_list(yyp, -2);
            let nm = get_tok(yyp, 0);
            let r = crate::build::id_list_append(db, p_parse, prior, &nm);
            set_id_list(yyp, -2, r);
        }
        // idlist ::= nm
        178 => {
            let nm = get_tok(yyp, 0);
            let r = crate::build::id_list_append(db, p_parse, None, &nm);
            set_id_list(yyp, 0, r);
        }
        // expr ::= LP expr RP
        179 => {
            let e = take_expr(yyp, -1);
            set_expr(yyp, -2, e);
        }
        // expr ::= ID|INDEXED|JOIN_KW
        180 => {
            let t = get_tok(yyp, 0);
            let e = token_expr(p_parse, TK_ID as i32, &t);
            set_expr(yyp, 0, e);
        }
        // expr ::= nm DOT nm
        181 => {
            let t1 = get_tok(yyp, -2);
            let t2 = get_tok(yyp, 0);
            let temp1 = token_expr(p_parse, TK_ID as i32, &t1);
            let temp2 = token_expr(p_parse, TK_ID as i32, &t2);
            let e = p_expr(db, p_parse, TK_DOT as i32, temp1, temp2);
            set_expr(yyp, -2, e);
        }
        // expr ::= nm DOT nm DOT nm
        182 => {
            let t1 = get_tok(yyp, -4);
            let t2 = get_tok(yyp, -2);
            let t3 = get_tok(yyp, 0);
            let temp1 = token_expr(p_parse, TK_ID as i32, &t1);
            let temp2 = token_expr(p_parse, TK_ID as i32, &t2);
            let temp3 = token_expr(p_parse, TK_ID as i32, &t3);
            let temp4 = p_expr(db, p_parse, TK_DOT as i32, temp2, temp3);
            if p_parse.in_rename_object() {
                let from = temp1.as_deref().map_or(0, |e| e as *const Expr as usize);
                rename_token_remap(p_parse, 0, from);
            }
            let e = p_expr(db, p_parse, TK_DOT as i32, temp1, temp4);
            set_expr(yyp, -4, e);
        }
        // term ::= NULL|FLOAT|BLOB; term ::= STRING
        183 | 184 => {
            let op = major_at(yyp, 0);
            let t = get_tok(yyp, 0);
            let e = token_expr(p_parse, op, &t);
            set_expr(yyp, 0, e);
        }
        // term ::= INTEGER
        185 => {
            let t = get_tok(yyp, 0);
            let mut e = expr_alloc(TK_INTEGER as i32, Some(&t), 1);
            if let Some(ex) = e.as_deref_mut() {
                ex.w = t.i_ofst.wrapping_sub(p_parse.z_tail as i32);
            }
            set_expr(yyp, 0, e);
        }
        // expr ::= VARIABLE
        186 => {
            let t = get_tok(yyp, 0);
            if !(at(&t.z, 0) == b'#' && is_digit(at(&t.z, 1))) {
                let n = t.z.len() as u32;
                let mut e = token_expr(p_parse, TK_VARIABLE as i32, &t);
                expr_assign_var_number(db, p_parse, e.as_deref_mut(), n);
                set_expr(yyp, 0, e);
            } else {
                // Numa análise aninhada pode-se escrever termos como `#1 #2 ...`, que se
                // referem a registradores da máquina virtual: `#N` é o N-ésimo registrador.
                debug_assert!(t.z.len() >= 2);
                if p_parse.nested == 0 {
                    let arg = token_arg(p_parse, &t);
                    error_msg(db, p_parse, b"near \"%T\": syntax error", &[arg]);
                    set_expr(yyp, 0, None);
                } else {
                    let mut e = p_expr(db, p_parse, TK_REGISTER as i32, None, None);
                    if let Some(ex) = e.as_deref_mut() {
                        if let Some(v) = get_int32(&t.z[1..]) {
                            ex.i_table = v;
                        }
                    }
                    set_expr(yyp, 0, e);
                }
            }
        }
        // expr ::= expr COLLATE ID|STRING
        187 => {
            let e = take_expr(yyp, -2);
            let t = get_tok(yyp, 0);
            let r = expr_add_collate_token(e, &t, 1);
            set_expr(yyp, -2, r);
        }
        // expr ::= CAST LP expr AS typetoken RP
        188 => {
            let t = get_tok(yyp, -1);
            let mut p = expr_alloc(TK_CAST as i32, Some(&t), 1);
            let e = take_expr(yyp, -3);
            expr_attach_subtrees(p.as_deref_mut(), e, None);
            set_expr(yyp, -5, p);
        }
        // expr ::= ID|INDEXED|JOIN_KW LP distinct exprlist RP
        189 => {
            let list = take_list(yyp, -1);
            let t = get_tok(yyp, -4);
            let distinct = get_i32(yyp, -2);
            let r = expr_function(db, p_parse, list, &t, distinct);
            set_expr(yyp, -4, r);
        }
        // expr ::= ID|INDEXED|JOIN_KW LP distinct exprlist ORDER BY sortlist RP
        190 => {
            let list = take_list(yyp, -4);
            let t = get_tok(yyp, -7);
            let distinct = get_i32(yyp, -5);
            let mut r = expr_function(db, p_parse, list, &t, distinct);
            let order_by = take_list(yyp, -1);
            expr_add_function_order_by(db, p_parse, r.as_deref_mut(), order_by);
            set_expr(yyp, -7, r);
        }
        // expr ::= ID|INDEXED|JOIN_KW LP STAR RP
        191 => {
            let t = get_tok(yyp, -3);
            let r = expr_function(db, p_parse, None, &t, 0);
            set_expr(yyp, -3, r);
        }
        // expr ::= ID|INDEXED|JOIN_KW LP distinct exprlist RP filter_over
        192 => {
            let list = take_list(yyp, -2);
            let t = get_tok(yyp, -5);
            let distinct = get_i32(yyp, -3);
            let mut r = expr_function(db, p_parse, list, &t, distinct);
            let win = take_window(yyp, 0);
            window_attach(db, p_parse, r.as_deref_mut(), win);
            set_expr(yyp, -5, r);
        }
        // expr ::= ID|INDEXED|JOIN_KW LP distinct exprlist ORDER BY sortlist RP filter_over
        193 => {
            let list = take_list(yyp, -5);
            let t = get_tok(yyp, -8);
            let distinct = get_i32(yyp, -6);
            let mut r = expr_function(db, p_parse, list, &t, distinct);
            let win = take_window(yyp, 0);
            window_attach(db, p_parse, r.as_deref_mut(), win);
            let order_by = take_list(yyp, -2);
            expr_add_function_order_by(db, p_parse, r.as_deref_mut(), order_by);
            set_expr(yyp, -8, r);
        }
        // expr ::= ID|INDEXED|JOIN_KW LP STAR RP filter_over
        194 => {
            let t = get_tok(yyp, -4);
            let mut r = expr_function(db, p_parse, None, &t, 0);
            let win = take_window(yyp, 0);
            window_attach(db, p_parse, r.as_deref_mut(), win);
            set_expr(yyp, -4, r);
        }
        // term ::= CTIME_KW
        195 => {
            let t = get_tok(yyp, 0);
            let r = expr_function(db, p_parse, None, &t, 0);
            set_expr(yyp, 0, r);
        }
        // expr ::= LP nexprlist COMMA expr RP
        196 => {
            let prior = take_list(yyp, -3);
            let e = take_expr(yyp, -1);
            let list = expr_list_append(prior, e);
            let mut r = p_expr(db, p_parse, TK_VECTOR as i32, None, None);
            if let Some(ex) = r.as_deref_mut() {
                if let Some(l) = list {
                    let propagate = l
                        .a
                        .first()
                        .and_then(|it| it.p_expr.as_deref())
                        .map_or(0, |e| e.flags & EP_PROPAGATE);
                    ex.x = ExprX::List(l);
                    ex.flags |= propagate;
                }
            }
            // Sem nó, a lista é solta aqui (`sqlite3ExprListDelete`).
            set_expr(yyp, -4, r);
        }
        // expr ::= expr AND expr
        197 => {
            let l = take_expr(yyp, -2);
            let r = take_expr(yyp, 0);
            let e = expr_and(db, p_parse, l, r);
            set_expr(yyp, -2, e);
        }
        // expr ::= expr OR expr; expr ::= expr LT|GT|GE|LE expr; expr ::= expr EQ|NE expr;
        // expr ::= expr BITAND|BITOR|LSHIFT|RSHIFT expr; expr ::= expr PLUS|MINUS expr;
        // expr ::= expr STAR|SLASH|REM expr; expr ::= expr CONCAT expr
        198 | 199 | 200 | 201 | 202 | 203 | 204 => {
            let op = major_at(yyp, -1);
            let l = take_expr(yyp, -2);
            let r = take_expr(yyp, 0);
            let e = p_expr(db, p_parse, op, l, r);
            set_expr(yyp, -2, e);
        }
        // Regras sem ação (as de 0 a 200 que o lemon não lista) não fazem nada; as acima de 200
        // são da cauda.
        _ => {
            if yyruleno > 200 {
                crate::parse_reduce2::yy_reduce_tail(
                    yyp,
                    yyruleno,
                    yy_lookahead,
                    yy_lookahead_token,
                    p_parse,
                    db,
                );
            }
        }
    }
    yy_reduce_goto(yyp, yyruleno)
}

/// `yymsp[k].minor.yy168 = v`.
fn set_ofst(yyp: &mut YyParser, k: isize, v: i32) {
    yyp.msp(k).minor = YyMinor::Yy168(v);
}

/// `if( IN_RENAME_OBJECT && list ) sqlite3RenameTokenMap(pParse, list->a[0].zName, &tok)`.
fn rename_first_src_name(p_parse: &mut Parse, list: Option<&SrcList>, t: &Token) {
    if !p_parse.in_rename_object() {
        return;
    }
    if let Some(item) = list.and_then(|l| l.a.first()) {
        let addr = item.z_name.as_ref().map_or(0, |z| z.as_ptr() as usize);
        rename_token_map(p_parse, addr, t);
    }
}
