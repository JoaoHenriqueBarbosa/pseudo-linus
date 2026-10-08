//! Tradução de `whereexpr.c` (chunks `whereexpr_c.000` a `whereexpr_c.004`): a análise das
//! subexpressões da cláusula WHERE que o planejador usa (`exprAnalyze`, a otimização de OR,
//! BETWEEN, LIKE, vetores, operadores auxiliares de tabelas virtuais) e a decomposição da cláusula
//! em termos.
//!
//! Convenções desta fatia (modelo v2, ver CONVENTIONS.md e o cabeçalho de `where_int.rs`, que é o
//! contrato de `where.rs` e `wherecode.rs`):
//!
//! - `WhereClause*` vira `(wi: &mut WhereInfo, wc: ClauseId)`; `WhereTerm*` vira `TermId`. O
//!   `pWC->pWInfo->pParse->db` some: as funções recebem `db: &mut Connection, parse: &mut Parse`
//!   antes do `wi`, e `pWC->pWInfo->sMaskSet` é `wi.s_mask_set`.
//! - `WhereTerm.p_expr` é um `ExprRef` (raiz mais caminho) numa arena do `WhereInfo`: o
//!   aliasing do C (subtermos de um OR dentro do termo pai, fatias do mesmo IN, mutações no lugar
//!   por `exprCommute`) fica idêntico. `whereClauseInsert(pWC, p, wtFlags)` recebe um `ExprRef`;
//!   para um termo `TERM_DYNAMIC` o chamador antes passa a posse do `Box<Expr>` novo à arena com
//!   `wi.exprs.add_root(..)` e entrega a referência devolvida.
//! - As falhas de alocação (`db->mallocFailed`, `whereClauseInsert` devolvendo 0) não existem em
//!   Rust: os ramos que só tratam OOM somem. Por isso `whereClauseInsert` devolve sempre a posição
//!   do termo novo.
//! - `sqlite3WhereClauseInit` devolve o `ClauseId` da cláusula nova na arena; `WhereOrInfo` e
//!   `WhereAndInfo` guardam o `ClauseId` das subcláusulas (nunca ficam por valor).
//! - `whereOrInfoDelete` e `whereAndInfoDelete` são só `sqlite3WhereClauseClear` mais o `free`; o
//!   `free` é o `Drop`, então o que sobra delas está dentro de `where_clause_clear`.
//! - `sqlite3WhereGetMask` é `WhereMaskSet::get_mask` (em `where_int.rs`).
//! - As funções de uso (`sqlite3WhereExprUsage*`) recebem o `&mut WhereMaskSet` porque gravam
//!   `bVarSelect`.
//! - `pExpr` de `isAuxiliaryVtabOperator` e `isLikeOrGlob`: os `Expr*` que o C devolve por ponteiro
//!   de saída (`ppLeft`, `ppRight`) são caminhos relativos (`ExprStep`) ao nó examinado, para o
//!   chamador alterar o `WhereInfo` sem emprestar a árvore; `ppPrefix` é um `Box<Expr>` novo.
//! - Dependências de módulos ainda não traduzidos, com as assinaturas assumidas:
//!   `func::is_like_function(db: &Connection, p_expr: &Expr, p_is_nocase: &mut bool,
//!   a_wc: &mut [u8; 4]) -> bool` (`sqlite3IsLikeFunction`), `vtab::get_vtable(db, &Rc<Table>) ->
//!   Option<VTableId>` (`sqlite3GetVTable`) e `expr_code::is_binary` (`sqlite3IsBinary`) público.

use std::rc::Rc;

use crate::connection::{Connection, Parse};
use crate::consts::{
    Bitmask, COLFLAG_HIDDEN, EP_FIXED_COL, EP_IF_NULL_ROW, EP_INNER_ON, EP_IS_FALSE, EP_OUTER_ON,
    EP_UNLIKELY, EP_SKIP, EP_VAR_SELECT, EP_X_IS_SELECT, EP_COMMUTED, EP_INT_VALUE, EP_LEAF,
    EP_TOKEN_ONLY, JT_LEFT, JT_LTORJ, JT_RIGHT, KEYINFO_ORDER_BIGNULL, SF_AGGREGATE, SF_COMPOUND,
    SF_DISTINCT, SF_VALUES, SQLITE_AFF_BLOB, SQLITE_AFF_TEXT, SQLITE_ENABLE_QPSG,
    SQLITE_INDEX_CONSTRAINT_FUNCTION, SQLITE_INDEX_CONSTRAINT_GLOB,
    SQLITE_INDEX_CONSTRAINT_ISNOT, SQLITE_INDEX_CONSTRAINT_ISNOTNULL,
    SQLITE_INDEX_CONSTRAINT_LIKE, SQLITE_INDEX_CONSTRAINT_LIMIT, SQLITE_INDEX_CONSTRAINT_MATCH,
    SQLITE_INDEX_CONSTRAINT_NE, SQLITE_INDEX_CONSTRAINT_OFFSET, SQLITE_INDEX_CONSTRAINT_REGEXP,
    SQLITE_TEXT, SQLITE_TRANSITIVE, SQLITE_UTF8, TERM_ANDINFO, TERM_CODED, TERM_COPIED,
    TERM_DYNAMIC, TERM_IS, TERM_LIKE, TERM_LIKEOPT, TERM_OK, TERM_ORINFO, TERM_SLICE,
    TERM_VARSELECT, TERM_VIRTUAL, TERM_VNULL, TK_AND, TK_AS, TK_BETWEEN, TK_COLLATE, TK_COLUMN,
    TK_EQ, TK_FUNCTION, TK_GE, TK_GT, TK_IF_NULL_ROW, TK_IN, TK_INTEGER, TK_IS, TK_ISNOT,
    TK_ISNULL, TK_LE, TK_LIMIT, TK_LT, TK_MATCH, TK_NE, TK_NOTNULL, TK_NULL, TK_OR, TK_REGISTER,
    TK_STRING, TK_TRUEFALSE, TK_UPLUS, TK_VARIABLE, TK_VECTOR, WO_AND, WO_ALL, WO_AUX, WO_EQ,
    WO_EQUIV, WO_GE, WO_GT, WO_IN, WO_IS, WO_ISNULL, WO_LE, WO_LT, WO_OR, WO_ROWVAL, WO_SINGLE,
    XN_EXPR,
};
use crate::ctype::{to_lower, to_upper};
use crate::expr::{
    binary_compare_coll_seq, expr, expr_add_collate_string, expr_affinity, expr_alloc,
    expr_coll_seq_match, expr_compare_coll_seq, expr_dup, expr_for_vector_field, expr_list_append,
    expr_skip_collate, expr_vector_size, p_expr as new_p_expr,
};
use crate::expr_code::{
    expr_can_be_null_in, expr_check_in, expr_code_target, expr_is_constant, expr_is_integer,
    is_binary, vdbe_of_parse,
};
use crate::expr_code2::{expr_compare, expr_compare_skip, get_temp_reg, release_temp_reg};
use crate::func::is_like_function;
use crate::mem::{value_text, value_type, USE_LONG_DOUBLE};
use crate::printf::PrintfArg;
use crate::resolve::expr_col_used;
use crate::select::set_join_expr;
use crate::sqlite_int::{
    is_numeric_affinity, Expr, ExprU, ExprX, Select, SrcItem, SrcList, SrcU1, TabRef, Table,
    ExprY, ExprList,
};
use crate::mem::CollSeq;
use crate::util::{at, atof, error_msg, log_est, str_icmp, strlen30};
use crate::vdbeaux::change_p3;
use crate::vdbeaux3::{vdbe_get_bound_value, vdbe_set_varmask};
use crate::vtab::get_vtable;
use crate::where_int::{
    expr_child, ClauseId, ExprRef, ExprStep, TermId, WhereAndInfo, WhereClause, WhereInfo,
    WhereMaskSet, WhereOrInfo, WhereTerm,
};

// ---------------------------------------------------------------------------------------------
// Auxiliares privados (não existem no C)
// ---------------------------------------------------------------------------------------------

/// Os dois `CollSeq*` de uma comparação são o mesmo objeto (`!=` de ponteiros do C).
fn same_coll(a: &Option<Rc<CollSeq>>, b: &Option<Rc<CollSeq>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => Rc::ptr_eq(x, y),
        _ => false,
    }
}

/// `sqlite3ExprSkipCollateAndLikely` sobre uma referência: a referência ao nó que resta depois
/// de pular os `TK_COLLATE` e as funções `unlikely()`, `likelihood()` e `likely()` da raiz. Os
/// passos são os mesmos de `expr_skip_collate_and_likely`.
fn skip_collate_and_likely_ref(exprs: &crate::where_int::WhereExprs, r: &ExprRef) -> ExprRef {
    let mut cur = r.clone();
    loop {
        let Some(e) = exprs.get(&cur) else {
            return r.clone();
        };
        if !e.has_property(EP_SKIP | EP_UNLIKELY) {
            return cur;
        }
        if e.has_property(EP_UNLIKELY) {
            cur = cur.child(ExprStep::List(0));
        } else if e.op == TK_COLLATE {
            cur = cur.child(ExprStep::Left);
        } else {
            return cur;
        }
    }
}

/// O texto do token de um nó que o C edita no lugar (`pStr1->u.zToken[i] = ...`).
fn z_token_mut(e: &mut Expr) -> &mut Vec<u8> {
    match &mut e.u {
        ExprU::Token(Some(v)) => v,
        _ => panic!("Expr.u.zToken"),
    }
}

// ---------------------------------------------------------------------------------------------
// Chunk 000: inserção de termos, operadores, LIKE e operadores auxiliares de tabelas virtuais
// ---------------------------------------------------------------------------------------------

/// `sqlite3WhereClauseInit`: acrescenta uma cláusula vazia à arena e devolve o seu id. O
/// `pWInfo` do C é o `wi` que as demais funções recebem.
pub fn where_clause_init(wi: &mut WhereInfo) -> ClauseId {
    wi.clauses.push(WhereClause::default());
    ClauseId((wi.clauses.len() - 1) as u32)
}

/// `whereClauseInsert`: acrescenta um termo novo à cláusula, construído da expressão `p` e de
/// `wt_flags`. Devolve a posição do termo em `pWC->a[]`. Com `TERM_DYNAMIC` a cláusula passa a
/// ser dona da raiz de `p` (o chamador a pôs na arena com `add_root`).
fn where_clause_insert(wi: &mut WhereInfo, wc: ClauseId, p: &ExprRef, wt_flags: u16) -> usize {
    let truth_prob = match wi.exprs.get(p) {
        Some(e) if e.has_property(EP_UNLIKELY) => log_est(e.i_table as u64) - 270,
        _ => 1,
    };
    let p_expr = skip_collate_and_likely_ref(&wi.exprs, p);
    // O `memset` do C zera tudo de `eOperator` em diante (inclusive `iParent`, que o C pôs em -1
    // logo antes): `exprAnalyze` o põe de volta em -1 nos termos que analisa.
    let term = WhereTerm {
        p_expr,
        p_wc: wc,
        truth_prob,
        wt_flags,
        ..WhereTerm::default()
    };
    wi.terms.push(term);
    let id = TermId((wi.terms.len() - 1) as u32);
    let c = &mut wi.clauses[wc.0 as usize];
    c.a.push(id);
    let idx = c.a.len() - 1;
    if (wt_flags & TERM_VIRTUAL) == 0 {
        c.n_base = c.a.len() as i32;
    }
    idx
}

/// `allowedOp`: verdadeiro se `op` é um dos operadores permitidos num termo indexável da cláusula
/// WHERE: "=", "<", ">", "<=", ">=", "IN", "IS" e "IS NULL".
fn allowed_op(op: u8) -> bool {
    debug_assert!(TK_GT > TK_EQ && TK_GT < TK_GE);
    debug_assert!(TK_LT > TK_EQ && TK_LT < TK_GE);
    debug_assert!(TK_LE > TK_EQ && TK_LE < TK_GE);
    debug_assert!(TK_GE == TK_EQ + 4);
    op == TK_IN || (op >= TK_EQ && op <= TK_GE) || op == TK_ISNULL || op == TK_IS
}

/// `exprCommute`: comuta um operador de comparação. "X op Y" vira "Y op X". Devolve 0 (o valor
/// entra nas `wtFlags` do termo).
fn expr_commute(db: &mut Connection, parse: &mut Parse, p: &mut Expr) -> u16 {
    let differs = {
        let l = p.p_left.as_deref().expect("pExpr->pLeft");
        let r = p.p_right.as_deref().expect("pExpr->pRight");
        l.op == TK_VECTOR
            || r.op == TK_VECTOR
            || !same_coll(
                &binary_compare_coll_seq(db, parse, l, Some(r), None),
                &binary_compare_coll_seq(db, parse, r, Some(l), None),
            )
    };
    if differs {
        p.flags ^= EP_COMMUTED;
    }
    std::mem::swap(&mut p.p_left, &mut p.p_right);
    if p.op >= TK_GT {
        debug_assert!(TK_LT == TK_GT + 2);
        debug_assert!(TK_GE == TK_LE + 2);
        debug_assert!(TK_GT > TK_EQ);
        debug_assert!(TK_GT < TK_LE);
        debug_assert!(p.op >= TK_GT && p.op <= TK_GE);
        p.op = ((p.op - TK_GT) ^ 2) + TK_GT;
    }
    0
}

/// `operatorMask`: traduz de operador `TK_xx` para a máscara `WO_xx`.
fn operator_mask(op: u8) -> u16 {
    debug_assert!(allowed_op(op));
    let c: u16 = if op == TK_IN {
        WO_IN
    } else if op == TK_ISNULL {
        WO_ISNULL
    } else if op == TK_IS {
        WO_IS
    } else {
        debug_assert!(((WO_EQ as u32) << (op - TK_EQ)) < 0x7fff);
        WO_EQ << (op - TK_EQ) as u32
    };
    debug_assert!(op != TK_ISNULL || c == WO_ISNULL);
    debug_assert!(op != TK_IN || c == WO_IN);
    debug_assert!(op != TK_EQ || c == WO_EQ);
    debug_assert!(op != TK_LT || c == WO_LT);
    debug_assert!(op != TK_LE || c == WO_LE);
    debug_assert!(op != TK_GT || c == WO_GT);
    debug_assert!(op != TK_GE || c == WO_GE);
    debug_assert!(op != TK_IS || c == WO_IS);
    c
}

/// O resultado de `isLikeOrGlob` quando a otimização é possível.
struct LikeInfo {
    /// `*ppPrefix`: o `TK_STRING` com o prefixo do padrão, sem escapes.
    prefix: Box<Expr>,
    /// `*pisComplete`: o único curinga é `%` no último caractere.
    is_complete: bool,
    /// `*pnoCase`: maiúscula equivale a minúscula.
    no_case: bool,
}

/// `isLikeOrGlob`: verifica se a expressão é um operador LIKE ou GLOB que pode ser otimizado com
/// restrições de desigualdade. O lado direito deve ser um literal de texto que não começa com
/// curinga; o esquerdo, uma coluna que só pode ser NULL, texto ou BLOB; e a colação da coluna
/// deve ser adequada ao operador.
fn is_like_or_glob(db: &mut Connection, parse: &mut Parse, p_expr: &Expr) -> Option<LikeInfo> {
    let mut no_case = false;
    let mut wc = [0u8; 4];
    if !is_like_function(db, p_expr, &mut no_case, &mut wc) {
        return None;
    }
    debug_assert!(p_expr.use_x_list());
    let list = p_expr.x_list()?;
    let p_left = list.a.get(1)?.p_expr.as_deref()?;
    let p_right = expr_skip_collate(list.a.first()?.p_expr.as_deref())?;
    let op = p_right.op;
    // O padrão como cadeia C (o fim do slice é o NUL).
    let mut z: Option<Vec<u8>> = None;
    if op == TK_VARIABLE && (db.flags & SQLITE_ENABLE_QPSG) == 0 {
        let i_col = p_right.i_column;
        let p_reprepare = parse.p_reprepare;
        let mut p_val = vdbe_get_bound_value(
            p_reprepare.and_then(|id| db.stmt(id)),
            i_col,
            SQLITE_AFF_BLOB,
        );
        if let Some(v) = p_val.as_mut() {
            if value_type(v) == SQLITE_TEXT {
                z = value_text(v, SQLITE_UTF8 as u8).map(|s| s.to_vec());
            }
        }
        vdbe_set_varmask(vdbe_of_parse(parse), i_col);
        debug_assert!(p_right.op == TK_VARIABLE || p_right.op == TK_REGISTER);
    } else if op == TK_STRING {
        debug_assert!(!p_right.has_property(EP_INT_VALUE));
        z = p_right.z_token().map(|t| t.to_vec());
    }
    let z = z?;
    let zc = |i: usize| at(&z, i);

    // Conta os caracteres do prefixo antes do primeiro curinga.
    let mut cnt: usize = 0;
    let mut c: u8;
    loop {
        c = zc(cnt);
        if c == 0 || c == wc[0] || c == wc[1] || c == wc[2] {
            break;
        }
        cnt += 1;
        if c == wc[3] && zc(cnt) != 0 {
            cnt += 1;
        }
    }

    // A otimização só é possível se (1) o padrão não começa com curinga e (2) o prefixo sem
    // curinga não termina com o caractere 0xff ilegal, ou (3) o padrão não é um único caractere
    // de escape. A segunda condição permite incrementar a chave do prefixo para achar o limite
    // superior; a terceira é porque quem chama supõe que sobra ao menos um caractere depois de
    // remover os escapes.
    if !((cnt > 1 || (cnt > 0 && zc(0) != wc[3])) && 255 != zc(cnt - 1)) {
        return None;
    }

    // Uma correspondência "completa" se o padrão termina com "*" ou "%".
    let is_complete = c == wc[0] && zc(cnt + 1) == 0;

    // O prefixo do padrão, sem os escapes. A cópia do texto vai até `cnt` (o C grava um NUL em
    // `zNew[cnt]`; ler `zNew[cnt]` depois dá 0).
    let base = &z[..strlen30(&z) as usize];
    let base_len = base.len();
    let zn_at = |i: usize| -> u8 {
        if i < cnt && i < base_len {
            base[i]
        } else {
            0
        }
    };
    let mut z_new: Vec<u8> = Vec::with_capacity(cnt);
    let mut i_from = 0usize;
    while i_from < cnt {
        if zn_at(i_from) == wc[3] {
            i_from += 1;
        }
        z_new.push(zn_at(i_from));
        i_from += 1;
    }
    let i_to = z_new.len();
    debug_assert!(i_to > 0);

    // Se o LHS não é uma coluna comum com afinidade TEXT, nenhum dos limites do prefixo pode
    // parecer um número: senão o padrão poderia ser tratado como número e a otimização LIKE
    // deixaria de valer.
    if p_left.op != TK_COLUMN
        || expr_affinity(p_left, None) != SQLITE_AFF_TEXT
        || (p_left.use_y_tab()
            && match p_left.y_tab() {
                Some(TabRef::Rc(t)) => t.is_virtual(),
                _ => false,
            })
    {
        let (mut is_num, _) = atof(&z_new, i_to as i32, SQLITE_UTF8 as u8, USE_LONG_DOUBLE);
        if is_num <= 0 {
            if i_to == 1 && z_new[0] == b'-' {
                is_num = 1;
            } else {
                z_new[i_to - 1] = z_new[i_to - 1].wrapping_add(1);
                is_num = atof(&z_new, i_to as i32, SQLITE_UTF8 as u8, USE_LONG_DOUBLE).0;
                z_new[i_to - 1] = z_new[i_to - 1].wrapping_sub(1);
            }
        }
        if is_num > 0 {
            return None;
        }
    }
    let n = strlen30(&z_new) as usize;
    z_new.truncate(n);
    let prefix = expr(TK_STRING as i32, Some(&z_new))?;

    // Se o padrão do RHS é um parâmetro ligado, prepara a recompilação do comando quando o
    // parâmetro for religado.
    if op == TK_VARIABLE {
        let i_col = p_right.i_column;
        vdbe_set_varmask(vdbe_of_parse(parse), i_col);
        debug_assert!(!p_right.has_property(EP_INT_VALUE));
        if is_complete && at(p_right.z_token().unwrap_or(&[]), 1) != 0 {
            // Se o RHS do LIKE é uma variável e o valor atual dela dispensa a chamada da função
            // LIKE, nenhum OP_Variable seria acrescentado, o que atrapalha
            // `sqlite3_bind_parameter_name()`. Para contornar, acrescenta um OP_Variable falso.
            let r1 = get_temp_reg(parse);
            let mut tmp = p_right.clone();
            expr_code_target(db, parse, &mut tmp, r1, None);
            let v = vdbe_of_parse(parse);
            let addr = v.n_op() - 1;
            change_p3(v, addr, 0);
            release_temp_reg(parse, r1);
        }
    }
    Some(LikeInfo { prefix, is_complete, no_case })
}

/// O que `isAuxiliaryVtabOperator` achou: `res` (0, 1 ou 2), o `op2` e onde estão, em relação ao
/// nó examinado, a coluna (`left`) e a expressão (`right`).
struct AuxOp {
    res: i32,
    e_op2: u8,
    left: Option<ExprStep>,
    right: Option<ExprStep>,
}

/// `xFindFunction` do módulo da tabela virtual `tab`, com 2 argumentos: o código do operador
/// (0 se o módulo não sobrecarrega a função).
fn vtab_find_function(db: &mut Connection, tab: &Rc<Table>, name: &[u8]) -> i32 {
    let Some(id) = get_vtable(db, tab) else {
        return 0;
    };
    let Some(mut vt) = db.vtabs.take(id.slot()) else {
        return 0;
    };
    let mut res = 0;
    if vt.p_mod.p_module.caps().find_function {
        if let Some(mut v) = vt.p_vtab.take() {
            let mut not_used = None;
            res = v.find_function(db, 2, name, &mut not_used);
            vt.p_vtab = Some(v);
        }
    }
    db.vtabs.put(id.slot(), vt);
    res
}

/// `isAuxiliaryVtabOperator`: verifica se `p_expr` tem uma forma que precisa ser passada ao
/// `xBestIndex` das tabelas virtuais:
///
/// ```text
///      1.  column MATCH expr            SQLITE_INDEX_CONSTRAINT_MATCH
///      2.  column GLOB expr             SQLITE_INDEX_CONSTRAINT_GLOB
///      3.  column LIKE expr             SQLITE_INDEX_CONSTRAINT_LIKE
///      4.  column REGEXP expr           SQLITE_INDEX_CONSTRAINT_REGEXP
///      5.  column != expr               SQLITE_INDEX_CONSTRAINT_NE
///      6.  expr != column               SQLITE_INDEX_CONSTRAINT_NE
///      7.  column IS NOT expr           SQLITE_INDEX_CONSTRAINT_ISNOT
///      8.  expr IS NOT column           SQLITE_INDEX_CONSTRAINT_ISNOT
///      9.  column IS NOT NULL           SQLITE_INDEX_CONSTRAINT_ISNOTNULL
/// ```
///
/// Em todos os casos "column" é coluna de tabela virtual. Se há casamento, `left` é a "column",
/// `right` é a "expr" (mesmo nas formas 6 e 8, em que a coluna está à direita) e `e_op2` o
/// operador. O resultado é 1 ou 2 se há casamento (2 se o RHS também é coluna de tabela virtual
/// nas formas 5 e 7), senão 0.
fn is_auxiliary_vtab_operator(db: &mut Connection, p_expr: &Expr) -> AuxOp {
    let none = AuxOp { res: 0, e_op2: 0, left: None, right: None };
    if p_expr.op == TK_FUNCTION {
        const A_OP: [(&[u8], i32); 4] = [
            (b"match", SQLITE_INDEX_CONSTRAINT_MATCH),
            (b"glob", SQLITE_INDEX_CONSTRAINT_GLOB),
            (b"like", SQLITE_INDEX_CONSTRAINT_LIKE),
            (b"regexp", SQLITE_INDEX_CONSTRAINT_REGEXP),
        ];
        debug_assert!(p_expr.use_x_list());
        let Some(list) = p_expr.x_list() else {
            return none;
        };
        if list.a.len() != 2 {
            return none;
        }

        // Os operadores MATCH, GLOB, LIKE e REGEXP embutidos se ligam a uma tabela virtual no
        // segundo argumento, que é o operando esquerdo da forma infixa:
        //
        //       vtab_column MATCH expression
        //       MATCH(expression,vtab_column)
        let p_col = list.a[1].p_expr.as_deref();
        if p_col.map_or(false, |c| c.is_vtab(None)) {
            for (z_op, e_op2) in A_OP.iter() {
                debug_assert!(!p_expr.has_property(EP_INT_VALUE));
                if str_icmp(p_expr.z_token().unwrap_or(&[]), z_op) == 0 {
                    return AuxOp {
                        res: 1,
                        e_op2: *e_op2 as u8,
                        left: Some(ExprStep::List(1)),
                        right: Some(ExprStep::List(0)),
                    };
                }
            }
        }

        // Também casa com a primeira coluna das funções sobrecarregadas em que `xFindFunction`
        // devolve ao menos `SQLITE_INDEX_CONSTRAINT_FUNCTION`:
        //
        //      OVERLOADED(vtab_column,expression)
        //
        // Historicamente `xFindFunction` esperava nomes de função em minúsculas, mas aqui ele
        // deve tratar nomes em qualquer caixa.
        let p_col = list.a[0].p_expr.as_deref();
        if let Some(c) = p_col {
            if c.is_vtab(None) {
                if let Some(TabRef::Rc(t)) = c.y_tab() {
                    debug_assert!(!p_expr.has_property(EP_INT_VALUE));
                    let t = t.clone();
                    let i = vtab_find_function(db, &t, p_expr.z_token().unwrap_or(&[]));
                    if i >= SQLITE_INDEX_CONSTRAINT_FUNCTION {
                        return AuxOp {
                            res: 1,
                            e_op2: i as u8,
                            left: Some(ExprStep::List(0)),
                            right: Some(ExprStep::List(1)),
                        };
                    }
                }
            }
        }
    } else if p_expr.op == TK_NE || p_expr.op == TK_ISNOT || p_expr.op == TK_NOTNULL {
        let mut res = 0;
        let mut left = Some(ExprStep::Left);
        let mut right = Some(ExprStep::Right);
        let p_left = p_expr.p_left.as_deref();
        let p_right = p_expr.p_right.as_deref();
        if p_left.map_or(false, |l| l.is_vtab(None)) {
            res += 1;
        }
        if p_right.map_or(false, |r| r.is_vtab(None)) {
            res += 1;
            std::mem::swap(&mut left, &mut right);
        }
        if p_right.is_none() {
            right = None;
        }
        let mut e_op2 = 0u8;
        if p_expr.op == TK_NE {
            e_op2 = SQLITE_INDEX_CONSTRAINT_NE as u8;
        }
        if p_expr.op == TK_ISNOT {
            e_op2 = SQLITE_INDEX_CONSTRAINT_ISNOT as u8;
        }
        if p_expr.op == TK_NOTNULL {
            e_op2 = SQLITE_INDEX_CONSTRAINT_ISNOTNULL as u8;
        }
        return AuxOp { res, e_op2, left, right };
    }
    none
}

/// `transferJoinMarkings`: se `base` nasceu do ON ou do USING de uma junção, passa as marcas
/// para `derived`.
fn transfer_join_markings(derived: Option<&mut Expr>, base: &Expr) {
    if let Some(d) = derived {
        if base.has_property(EP_OUTER_ON | EP_INNER_ON) {
            d.flags |= base.flags & (EP_OUTER_ON | EP_INNER_ON);
            d.w = base.w;
        }
    }
}

/// `markTermAsChild`: marca o termo `i_child` como filho do termo `i_parent` (posições em
/// `pWC->a[]`).
fn mark_term_as_child(wi: &mut WhereInfo, wc: ClauseId, i_child: usize, i_parent: usize) {
    let child = wi.term_at(wc, i_child);
    let parent = wi.term_at(wc, i_parent);
    let prob = wi.terms[parent.0 as usize].truth_prob;
    wi.terms[child.0 as usize].i_parent = i_parent as i32;
    wi.terms[child.0 as usize].truth_prob = prob;
    let p = &mut wi.terms[parent.0 as usize];
    p.n_child = p.n_child.wrapping_add(1);
}

/// `whereNthSubterm`: o N-ésimo subtermo ligado por AND de `term`. Se o termo não é uma
/// conjunção, devolve o próprio termo para N==0. Se N passa do número de subtermos, devolve
/// `None`.
fn where_nth_subterm(wi: &WhereInfo, term: TermId, n: usize) -> Option<TermId> {
    let t = wi.term(term);
    if t.e_operator != WO_AND {
        return if n == 0 { Some(term) } else { None };
    }
    let wc = t.p_and_info.as_ref()?.wc;
    if n < wi.n_term(wc) {
        Some(wi.term_at(wc, n))
    } else {
        None
    }
}

/// `whereCombineDisjuncts`: os subtermos `p_one` e `p_two` estão em `pWC` e em disjunção (OR).
///
/// Se os dois têm a forma "A op B" com os mesmos A e B mas operadores diferentes e compatíveis
/// (um é = e o outro <, por exemplo), acrescenta a `pWC` um termo AND virtual que é a combinação
/// dos dois:
///
/// ```text
///    x<y OR x=y    -->     x<=y
///    x=y OR x=y    -->     x=y
///    x<=y OR x<y   -->     x<=y
/// ```
///
/// Não gera `x<y OR x>y --> x!=y`.
fn where_combine_disjuncts(
    db: &mut Connection,
    parse: &mut Parse,
    wi: &mut WhereInfo,
    p_src: &SrcList,
    wc: ClauseId,
    p_one: TermId,
    p_two: TermId,
) {
    let one_op = wi.term(p_one).e_operator;
    let two_op = wi.term(p_two).e_operator;
    let mut e_op: u16 = one_op | two_op;
    if ((wi.term(p_one).wt_flags | wi.term(p_two).wt_flags) & TERM_VNULL) != 0 {
        return;
    }
    if (one_op & (WO_EQ | WO_LT | WO_LE | WO_GT | WO_GE)) == 0 {
        return;
    }
    if (two_op & (WO_EQ | WO_LT | WO_LE | WO_GT | WO_GE)) == 0 {
        return;
    }
    if (e_op & (WO_EQ | WO_LT | WO_LE)) != e_op && (e_op & (WO_EQ | WO_GT | WO_GE)) != e_op {
        return;
    }
    {
        let e1 = wi.expr(p_one);
        let e2 = wi.expr(p_two);
        debug_assert!(e1.p_left.is_some() && e1.p_right.is_some());
        debug_assert!(e2.p_left.is_some() && e2.p_right.is_some());
        if expr_compare(None, e1.p_left.as_deref(), e2.p_left.as_deref(), -1) != 0 {
            return;
        }
        if expr_compare(None, e1.p_right.as_deref(), e2.p_right.as_deref(), -1) != 0 {
            return;
        }
    }
    // Se chegou aqui, os dois subtermos podem ser combinados.
    if (e_op & (e_op - 1)) != 0 {
        if (e_op & (WO_LT | WO_LE)) != 0 {
            e_op = WO_LE;
        } else {
            debug_assert!((e_op & (WO_GT | WO_GE)) != 0);
            e_op = WO_GE;
        }
    }
    let Some(mut p_new) = expr_dup(Some(wi.expr(p_one)), 0) else {
        return;
    };
    let mut op = TK_EQ;
    while e_op != (WO_EQ << (op - TK_EQ) as u32) {
        op += 1;
        debug_assert!(op < TK_GE);
    }
    p_new.op = op;
    let r = wi.exprs.add_root(p_new);
    let idx_new = where_clause_insert(wi, wc, &r, TERM_VIRTUAL | TERM_DYNAMIC);
    expr_analyze(db, parse, wi, p_src, wc, idx_new);
}

// ---------------------------------------------------------------------------------------------
// Chunk 001: otimização de OR (exprAnalyzeOrTerm)
// ---------------------------------------------------------------------------------------------

/// `exprAnalyzeOrTerm`: analisa um termo composto de dois ou mais subtermos ligados por OR. Em
///
/// ```text
///     ... WHERE  (a=5) AND (b=7 OR c=9 OR d=13) AND (d=13)
///                          ^^^^^^^^^^^^^^^^^^^^
/// ```
///
/// é o termo do meio. Um `WhereOrInfo` é calculado e ligado ao termo, qualquer que seja o
/// resultado da análise:
///
/// ```text
///     WhereTerm.wtFlags   |=  TERM_ORINFO
///     WhereTerm.u.pOrInfo  =  um WhereOrInfo
/// ```
///
/// O termo precisa ter dois ou mais subtermos ligados por OR. Um subtermo pode ser um conjunto de
/// subsubtermos ligados por AND. Exemplos de termos analisados:
///
/// ```text
///     (A)     t1.x=t2.y OR t1.x=t2.z OR t1.y=15 OR t1.z=t3.a+5
///     (B)     x=expr1 OR expr2=x OR x=expr3
///     (C)     t1.x=t2.y OR (t1.x=t2.z AND t1.y=15)
///     (D)     x=expr1 OR (y>11 AND y<22 AND z LIKE '*hello*')
///     (E)     (p.a=1 AND q.b=2 AND r.c=3) OR (p.x=4 AND q.y=5 AND r.z=6)
///     (F)     x>A OR (x=A AND y>=B)
/// ```
///
/// CASO 1: se todos os subtermos têm a forma T.C=expr para uma só coluna C de uma só tabela T
/// (exemplo B), cria um termo virtual que é o IN equivalente: `x = expr1 OR expr2 = x OR x =
/// expr3` ganha `x IN (expr1,expr2,expr3)`.
///
/// CASO 2: com exatamente dois disjuntos em que um lado tem x>A e o outro x=A (mesmos x e A),
/// acrescenta um conjunto virtual `x>=A`. Exemplo: `x>A OR (x=A AND y>B)` ganha `x>=A`.
///
/// CASO 3: se todos os subtermos são indexáveis por uma só tabela T, grava
/// `WhereTerm.eOperator = WO_OR` e `WhereTerm.u.pOrInfo->indexable |= cursor de T`. Um subtermo é
/// indexável se tem a forma "T.C <op> <expr>" com `<op>` entre "=", "<", "<=", ">", ">=", "IS
/// NULL" e "IN", ou se é um AND de subsubtermos dos quais ao menos um é indexável (esses têm
/// `eOperator = WO_AND` e `u.pAndInfo`). Quando o termo também cumpre o caso 1 o otimizador
/// sempre prefere o caso 1, e então finge-se que o caso 3 não vale. Vários cursores podem ser
/// indexáveis (em E, P, Q e R).
///
/// SENÃO: se nenhum caso vale, `eOperator` fica 0 e o termo não serve para busca.
fn expr_analyze_or_term(
    db: &mut Connection,
    parse: &mut Parse,
    wi: &mut WhereInfo,
    p_src: &SrcList,
    wc: ClauseId,
    idx_term: usize,
) {
    let term = wi.term_at(wc, idx_term);
    let ti = term.0 as usize;
    let eref = wi.terms[ti].p_expr.clone();

    // Quebra a cláusula OR em subtermos. Eles ficam numa cláusula própria, ligada ao termo por
    // um `WhereOrInfo`.
    debug_assert!((wi.terms[ti].wt_flags & (TERM_DYNAMIC | TERM_ORINFO | TERM_ANDINFO)) == 0);
    debug_assert!(wi.exprs.get(&eref).map_or(false, |e| e.op == TK_OR));
    let or_wc = where_clause_init(wi);
    wi.terms[ti].p_or_info = Some(WhereOrInfo { wc: or_wc, indexable: 0 });
    wi.terms[ti].wt_flags |= TERM_ORINFO;
    where_split(wi, or_wc, Some(&eref), TK_OR);
    where_expr_analyze(db, parse, wi, p_src, or_wc);
    let n_or = wi.n_term(or_wc);
    debug_assert!(n_or >= 2);

    // Calcula o conjunto de tabelas que podem satisfazer os casos 1 ou 3.
    let mut indexable: Bitmask = !0;
    let mut chng_to_in: Bitmask = !0;
    let mut k = 0usize;
    while k < n_or && indexable != 0 {
        let or_term = wi.term_at(or_wc, k);
        let oi = or_term.0 as usize;
        if (wi.terms[oi].e_operator & WO_SINGLE) == 0 {
            debug_assert!((wi.terms[oi].wt_flags & (TERM_ANDINFO | TERM_ORINFO)) == 0);
            chng_to_in = 0;
            let and_wc = where_clause_init(wi);
            wi.terms[oi].p_and_info = Some(WhereAndInfo { wc: and_wc });
            wi.terms[oi].wt_flags |= TERM_ANDINFO;
            wi.terms[oi].e_operator = WO_AND;
            wi.terms[oi].left_cursor = -1;
            let or_ref = wi.terms[oi].p_expr.clone();
            where_split(wi, and_wc, Some(&or_ref), TK_AND);
            where_expr_analyze(db, parse, wi, p_src, and_wc);
            wi.clauses[and_wc.0 as usize].p_outer = Some(wc);
            let mut b: Bitmask = 0;
            for j in 0..wi.n_term(and_wc) {
                let and_term = wi.term_at(and_wc, j);
                if allowed_op(wi.expr(and_term).op)
                    || wi.terms[and_term.0 as usize].e_operator == WO_AUX
                {
                    b |= wi.s_mask_set.get_mask(wi.terms[and_term.0 as usize].left_cursor);
                }
            }
            indexable &= b;
        } else if (wi.terms[oi].wt_flags & TERM_COPIED) != 0 {
            // Pula este termo por enquanto. Ele é revisto quando o termo TERM_VIRTUAL
            // correspondente é processado.
        } else {
            let mut b = wi.s_mask_set.get_mask(wi.terms[oi].left_cursor);
            if (wi.terms[oi].wt_flags & TERM_VIRTUAL) != 0 {
                let other = wi.term_at(or_wc, wi.terms[oi].i_parent as usize);
                b |= wi.s_mask_set.get_mask(wi.terms[other.0 as usize].left_cursor);
            }
            indexable &= b;
            if (wi.terms[oi].e_operator & WO_EQ) == 0 {
                chng_to_in = 0;
            } else {
                chng_to_in &= b;
            }
        }
        k += 1;
    }

    // Grava o conjunto de tabelas que satisfazem o caso 3. Ele pode ser vazio.
    if let Some(info) = wi.terms[ti].p_or_info.as_mut() {
        info.indexable = indexable;
    }
    wi.terms[ti].e_operator = WO_OR;
    wi.terms[ti].left_cursor = -1;
    if indexable != 0 {
        wi.clauses[wc.0 as usize].has_or = 1;
    }

    // Para um OR de duas vias, tenta o caso 2.
    if indexable != 0 && n_or == 2 {
        let first = wi.term_at(or_wc, 0);
        let second = wi.term_at(or_wc, 1);
        let mut i_one = 0usize;
        while let Some(p_one) = where_nth_subterm(wi, first, i_one) {
            i_one += 1;
            let mut i_two = 0usize;
            while let Some(p_two) = where_nth_subterm(wi, second, i_two) {
                i_two += 1;
                where_combine_disjuncts(db, parse, wi, p_src, wc, p_one, p_two);
            }
        }
    }

    // `chng_to_in` tem um conjunto de tabelas que PODEM satisfazer o caso 1; falta conferir.
    //
    // Ele terá 0, 1 ou 2 bits. Com 0 bits não há como transformar o OR num IN, porque algum
    // termo tem algo diferente de == sobre uma coluna da única tabela. Com 1 bit, todo termo tem
    // a forma "tabela.coluna=expr" para uma só tabela, e falta conferir se a mesma coluna vale em
    // todos. Com 2 bits, todos os termos têm a forma "tabela1.coluna=tabela2.coluna", e talvez dê
    // para formar um IN com qualquer das duas colunas à esquerda se ela é comum a todos os
    // termos.
    //
    // Termos "tabela.coluna1=tabela.coluna2" (a mesma tabela nos dois lados) não são otimizáveis.
    if chng_to_in != 0 {
        let mut ok_to_chng_to_in = false;
        let mut i_column: i32 = -1;
        let mut i_cursor: i32 = -1;
        let mut p_left_term: Option<TermId> = None;
        let mut j = 0;
        while j < 2 && !ok_to_chng_to_in {
            p_left_term = None;
            // Procura uma tabela e coluna que aparece de um lado ou do outro do == em todo
            // subtermo.
            let mut k = 0usize;
            let mut found = false;
            while k < n_or {
                let or_term = wi.term_at(or_wc, k);
                let oi = or_term.0 as usize;
                debug_assert!((wi.terms[oi].e_operator & WO_EQ) != 0);
                wi.terms[oi].wt_flags &= !TERM_OK;
                if wi.terms[oi].left_cursor == i_cursor {
                    // É o caso de 2 bits, na segunda iteração, e o termo é da primeira. Pula.
                    debug_assert!(j == 1);
                    k += 1;
                    continue;
                }
                if (chng_to_in & wi.s_mask_set.get_mask(wi.terms[oi].left_cursor)) == 0 {
                    // O termo é "t1.a==t2.b" com t2 em `chng_to_in` e t1 fora. Ele será
                    // precedido ou seguido por uma cópia invertida (t2.b==t1.a): usa a inversão.
                    debug_assert!(
                        (wi.terms[oi].wt_flags & (TERM_COPIED | TERM_VIRTUAL)) != 0
                    );
                    k += 1;
                    continue;
                }
                debug_assert!((wi.terms[oi].e_operator & (WO_OR | WO_AND)) == 0);
                i_column = wi.terms[oi].left_column;
                i_cursor = wi.terms[oi].left_cursor;
                p_left_term = Some(or_term);
                found = true;
                break;
            }
            if !found {
                // Nenhum candidato. Só pode ocorrer na segunda iteração.
                debug_assert!(j == 1);
                debug_assert!(chng_to_in.is_power_of_two());
                debug_assert!(chng_to_in == wi.s_mask_set.get_mask(i_cursor));
                break;
            }

            // Achou uma tabela e coluna candidatas. Confere se são comuns a todos os termos.
            ok_to_chng_to_in = true;
            while k < n_or && ok_to_chng_to_in {
                let or_term = wi.term_at(or_wc, k);
                let oi = or_term.0 as usize;
                debug_assert!((wi.terms[oi].e_operator & WO_EQ) != 0);
                debug_assert!((wi.terms[oi].e_operator & (WO_OR | WO_AND)) == 0);
                if wi.terms[oi].left_cursor != i_cursor {
                    wi.terms[oi].wt_flags &= !TERM_OK;
                } else if wi.terms[oi].left_column != i_column
                    || (i_column == XN_EXPR as i32
                        && expr_compare(
                            Some((&mut *db, &mut *parse)),
                            wi.expr(or_term).p_left.as_deref(),
                            p_left_term.and_then(|t| wi.expr(t).p_left.as_deref()),
                            -1,
                        ) != 0)
                {
                    ok_to_chng_to_in = false;
                } else {
                    // Se o lado direito também é coluna, as afinidades dos dois lados devem ser
                    // tais que nenhuma conversão de tipo seja necessária à direita (ticket
                    // #2249).
                    let e = wi.expr(or_term);
                    let aff_right = e.p_right.as_deref().map_or(0, |r| expr_affinity(r, None));
                    let aff_left = e.p_left.as_deref().map_or(0, |l| expr_affinity(l, None));
                    if aff_right != 0 && aff_right != aff_left {
                        ok_to_chng_to_in = false;
                    } else {
                        wi.terms[oi].wt_flags |= TERM_OK;
                    }
                }
                k += 1;
            }
            j += 1;
        }

        // Aqui `ok_to_chng_to_in` é verdadeiro se o termo original cumpre o caso 1. Então
        // constrói um termo virtual que é o termo convertido num operador IN.
        if ok_to_chng_to_in {
            let mut p_list: Option<Box<ExprList>> = None;
            let mut p_left_src: Option<TermId> = None;
            for k in 0..n_or {
                let or_term = wi.term_at(or_wc, k);
                let oi = or_term.0 as usize;
                if (wi.terms[oi].wt_flags & TERM_OK) == 0 {
                    continue;
                }
                debug_assert!((wi.terms[oi].e_operator & WO_EQ) != 0);
                debug_assert!((wi.terms[oi].e_operator & (WO_OR | WO_AND)) == 0);
                debug_assert!(wi.terms[oi].left_cursor == i_cursor);
                debug_assert!(wi.terms[oi].left_column == i_column);
                let p_dup = expr_dup(wi.expr(or_term).p_right.as_deref(), 0);
                p_list = expr_list_append(p_list, p_dup);
                p_left_src = Some(or_term);
            }
            let p_left_src = p_left_src.expect("pLeft");
            let p_dup = expr_dup(wi.expr(p_left_src).p_left.as_deref(), 0);
            let mut p_new = new_p_expr(db, parse, TK_IN as i32, p_dup, None);
            if let Some(n) = p_new.as_deref_mut() {
                transfer_join_markings(Some(n), wi.exprs.get(&eref).expect("pExpr"));
                debug_assert!(n.use_x_list());
                n.x = match p_list {
                    Some(l) => ExprX::List(l),
                    None => ExprX::None,
                };
                let r = wi.exprs.add_root(p_new.take().expect("pNew"));
                let idx_new = where_clause_insert(wi, wc, &r, TERM_VIRTUAL | TERM_DYNAMIC);
                expr_analyze(db, parse, wi, p_src, wc, idx_new);
                mark_term_as_child(wi, wc, idx_new, idx_term);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Chunk 002: equivalências, uso de tabelas, indexação e exprAnalyze
// ---------------------------------------------------------------------------------------------

/// `termIsEquivalence`: já se sabe que `p_expr` é um operador binário com duas colunas. Confere
/// se é uma relação de equivalência:
///
/// ```text
///   1.  A otimização SQLITE_Transitive deve estar ligada
///   2.  Deve ser == ou IS
///   3.  Não pode vir do ON de um OUTER JOIN
///   4.  As afinidades de A e B devem ser compatíveis
///   5a. Os dois operandos usam a mesma colação OU
///   5b. A colação geral é BINARY
/// ```
///
/// Se devolve verdadeiro, o RHS pode ser substituído pelo LHS em qualquer outro ponto do WHERE em
/// que a coluna do LHS aparece. É uma otimização: devolver falso não causa dano, mas devolver
/// verdadeiro quando não deveria pode dar respostas erradas.
fn term_is_equivalence(db: &mut Connection, parse: &mut Parse, p_expr: &Expr) -> bool {
    if !db.optimization_enabled(SQLITE_TRANSITIVE) {
        return false;
    }
    if p_expr.op != TK_EQ && p_expr.op != TK_IS {
        return false;
    }
    if p_expr.has_property(EP_OUTER_ON) {
        return false;
    }
    let (Some(l), Some(r)) = (p_expr.p_left.as_deref(), p_expr.p_right.as_deref()) else {
        return false;
    };
    let aff1 = expr_affinity(l, None);
    let aff2 = expr_affinity(r, None);
    if aff1 != aff2 && (!is_numeric_affinity(aff1) || !is_numeric_affinity(aff2)) {
        return false;
    }
    let p_coll = expr_compare_coll_seq(db, parse, p_expr, None);
    if is_binary(p_coll.as_ref()) {
        return true;
    }
    expr_coll_seq_match(db, parse, l, r, None)
}

/// `exprSelectUsage`: percorre recursivamente as expressões de um SELECT e gera a máscara de
/// bits das tabelas usadas nele.
fn expr_select_usage(mask_set: &mut WhereMaskSet, p_s: Option<&Select>) -> Bitmask {
    let mut mask: Bitmask = 0;
    let mut p_s = p_s;
    while let Some(s) = p_s {
        mask |= where_expr_list_usage(mask_set, s.p_e_list.as_deref());
        mask |= where_expr_list_usage(mask_set, s.p_group_by.as_deref());
        mask |= where_expr_list_usage(mask_set, s.p_order_by.as_deref());
        mask |= where_expr_usage(mask_set, s.p_where.as_deref());
        mask |= where_expr_usage(mask_set, s.p_having.as_deref());
        if let Some(p_src) = s.p_src.as_deref() {
            for item in &p_src.a {
                mask |= expr_select_usage(mask_set, item.p_select.as_deref());
                if !item.fg.is_using {
                    mask |= where_expr_usage(mask_set, item.p_on());
                }
                if item.fg.is_tab_func {
                    if let SrcU1::FuncArg(l) = &item.u1 {
                        mask |= where_expr_list_usage(mask_set, l.as_deref());
                    }
                }
            }
        }
        p_s = s.p_prior.as_deref();
    }
    mask
}

/// `exprMightBeIndexed2`: `p_expr` é um operando de comparação que pode ser útil ao índice.
/// Confere se ele aparece num índice sobre expressão; se sim, devolve o cursor da tabela e
/// `XN_EXPR`. Começa a procurar na `j`-ésima entrada de `p_from`.
fn expr_might_be_indexed2(p_from: &SrcList, p_expr: &Expr, j: usize) -> Option<[i32; 2]> {
    let mut j = j;
    loop {
        let i_cur = p_from.a[j].i_cursor;
        if let Some(tab) = p_from.a[j].p_tab.as_ref() {
            for p_idx in &tab.p_index {
                let Some(col_expr) = p_idx.a_col_expr.as_deref() else {
                    continue;
                };
                for i in 0..p_idx.n_key_col as usize {
                    if p_idx.ai_column[i] != XN_EXPR {
                        continue;
                    }
                    debug_assert!(p_idx.b_has_expr);
                    let col = col_expr.a[i].p_expr.as_deref();
                    if expr_compare_skip(Some(p_expr), col, i_cur) == 0 {
                        let mut tmp = col.cloned();
                        if expr_is_constant(None, tmp.as_mut()) == 0 {
                            return Some([i_cur, XN_EXPR as i32]);
                        }
                    }
                }
            }
        }
        j += 1;
        if j >= p_from.a.len() {
            break;
        }
    }
    None
}

/// `exprMightBeIndexed`: `p_expr` é um operando de um operador de comparação que pode servir à
/// indexação. Devolve `Some([cursor, coluna])` se ele aparece num índice (a coluna é `XN_EXPR` se
/// o índice é sobre expressão). Para `TK_COLUMN` devolve sempre `Some`, mesmo se a coluna não é
/// indexada, porque ela pode ganhar um índice automático depois.
fn expr_might_be_indexed(p_from: &SrcList, p_expr: &Expr, op: u8) -> Option<[i32; 2]> {
    let mut p_expr = p_expr;

    // Se a expressão é um vetor à esquerda ou à direita de uma desigualdade (>, <, >= ou <=),
    // processa o primeiro elemento do vetor.
    debug_assert!(TK_GT + 1 == TK_LE && TK_GT + 2 == TK_LT && TK_GT + 3 == TK_GE);
    debug_assert!(TK_IS < TK_GE && TK_ISNULL < TK_GE && TK_IN < TK_GE);
    debug_assert!(op <= TK_GE);
    if p_expr.op == TK_VECTOR && (op >= TK_GT && op <= TK_GE) {
        debug_assert!(p_expr.use_x_list());
        p_expr = p_expr.x_list()?.a.first()?.p_expr.as_deref()?;
    }

    if p_expr.op == TK_COLUMN {
        return Some([p_expr.i_table, p_expr.i_column]);
    }

    for i in 0..p_from.a.len() {
        if let Some(tab) = p_from.a[i].p_tab.as_ref() {
            for p_idx in &tab.p_index {
                if p_idx.a_col_expr.is_some() {
                    return expr_might_be_indexed2(p_from, p_expr, i);
                }
            }
        }
    }
    None
}

/// `exprAnalyze`: a entrada é um `WhereTerm` só com `pExpr` preenchido. Analisa a subexpressão e
/// preenche os outros campos do termo.
///
/// Se a expressão é "<expr> <op> X" ela é comutada para a forma padrão "X <op> <expr>".
///
/// Se é "X <op> Y" com X e Y colunas, a expressão original fica como está e um novo termo virtual
/// "Y <op> X" é acrescentado ao WHERE e analisado em separado. O original é marcado com
/// `TERM_COPIED` e o novo com `TERM_DYNAMIC` (a expressão dele é da cláusula) e `TERM_VIRTUAL`
/// (cópia comutada de um termo anterior). O original tem `nChild=1` e a cópia tem `iParent` com a
/// posição do original.
fn expr_analyze(
    db: &mut Connection,
    parse: &mut Parse,
    wi: &mut WhereInfo,
    p_src: &SrcList,
    wc: ClauseId,
    idx_term: usize,
) {
    debug_assert!(wi.n_term(wc) > idx_term);
    let term = wi.term_at(wc, idx_term);
    let ti = term.0 as usize;
    let eref = wi.terms[ti].p_expr.clone();
    let wc_op = wi.clauses[wc.0 as usize].op;
    wi.s_mask_set.b_var_select = 0;
    let prereq_left: Bitmask;
    let mut prereq_all: Bitmask;
    let op: u8;
    {
        let Some(p_expr) = wi.exprs.get(&eref) else {
            return;
        };
        debug_assert!(p_expr.op != TK_AS && p_expr.op != TK_COLLATE);
        prereq_left = where_expr_usage(&mut wi.s_mask_set, p_expr.p_left.as_deref());
        op = p_expr.op;
        if op == TK_IN {
            debug_assert!(p_expr.p_right.is_none());
            if expr_check_in(db, parse, p_expr) != 0 {
                return;
            }
            let pr = if p_expr.use_x_select() {
                expr_select_usage(&mut wi.s_mask_set, p_expr.x_select())
            } else {
                where_expr_list_usage(&mut wi.s_mask_set, p_expr.x_list())
            };
            wi.terms[ti].prereq_right = pr;
            prereq_all = prereq_left | pr;
        } else {
            let pr = where_expr_usage(&mut wi.s_mask_set, p_expr.p_right.as_deref());
            wi.terms[ti].prereq_right = pr;
            if p_expr.p_left.is_none()
                || p_expr.has_property(EP_X_IS_SELECT | EP_IF_NULL_ROW)
                || !matches!(p_expr.x, ExprX::None)
            {
                prereq_all = where_expr_usage_nn(&mut wi.s_mask_set, p_expr);
            } else {
                prereq_all = prereq_left | pr;
            }
        }
    }
    if wi.s_mask_set.b_var_select != 0 {
        wi.terms[ti].wt_flags |= TERM_VARSELECT;
    }

    let mut extra_right: Bitmask = 0; // Dependências extras no LEFT JOIN.
    let (has_on, has_outer, i_join) = {
        let e = wi.exprs.get(&eref).expect("pExpr");
        (
            e.has_property(EP_OUTER_ON | EP_INNER_ON),
            e.has_property(EP_OUTER_ON),
            e.i_join(),
        )
    };
    if has_on {
        let x = wi.s_mask_set.get_mask(i_join);
        if has_outer {
            prereq_all |= x;
            // Termos do ON não podem ser usados com um índice da tabela à esquerda de um LEFT
            // JOIN (ticket #3015).
            extra_right = x.wrapping_sub(1);
            if (prereq_all >> 1) >= x {
                error_msg(db, parse, b"ON clause references tables to its right", &[]);
                return;
            }
        } else if (prereq_all >> 1) >= x {
            // O ON de um INNER JOIN referencia uma tabela à sua direita. A maioria dos outros
            // bancos dá erro, mas o SQLite 3.0 a 3.38 só punha a restrição no WHERE e seguia.
            // A partir da 3.39 só dá erro se há RIGHT ou FULL JOIN na consulta. Isso aproxima o
            // SQLite dos outros e preserva o legado.
            if !p_src.a.is_empty() && (p_src.a[0].fg.jointype & JT_LTORJ) != 0 {
                error_msg(db, parse, b"ON clause references tables to its right", &[]);
                return;
            }
            wi.exprs.get_mut(&eref).expect("pExpr").clear_property(EP_INNER_ON);
        }
    }
    {
        let t = &mut wi.terms[ti];
        t.prereq_all = prereq_all;
        t.left_cursor = -1;
        t.i_parent = -1;
        t.e_operator = 0;
    }

    // Primeira cadeia: operadores indexáveis, BETWEEN, OR, NOT NULL e LIKE/GLOB.
    if allowed_op(op) {
        let i_field = wi.terms[ti].i_field;
        let op_mask: u16 =
            if (wi.terms[ti].prereq_right & prereq_left) == 0 { WO_ALL } else { WO_EQUIV };

        // Tudo o que o C lê de `pLeft` e `pRight` sem efeito colateral, antes de mutar a
        // árvore.
        let (left_cc, right_cc, fold_is_null) = {
            let e = wi.exprs.get(&eref).expect("pExpr");
            let mut p_left = expr_skip_collate(e.p_left.as_deref());
            let p_right = expr_skip_collate(e.p_right.as_deref());
            if i_field > 0 {
                debug_assert!(op == TK_IN);
                debug_assert!(p_left.map_or(false, |l| l.op == TK_VECTOR));
                p_left = p_left
                    .and_then(|l| l.x_list())
                    .and_then(|l| l.a.get(i_field as usize - 1))
                    .and_then(|it| it.p_expr.as_deref());
            }
            let left_cc = p_left.and_then(|l| expr_might_be_indexed(p_src, l, op));
            let right_cc = match p_right {
                Some(r) if !r.has_property(EP_FIXED_COL) => expr_might_be_indexed(p_src, r, op),
                _ => None,
            };
            let fold = right_cc.is_none()
                && op == TK_ISNULL
                && !e.has_property(EP_OUTER_ON)
                && !p_left.map_or(true, |l| expr_can_be_null_in(l, None));
            (left_cc, right_cc, fold)
        };

        if let Some(cc) = left_cc {
            let t = &mut wi.terms[ti];
            t.left_cursor = cc[0];
            debug_assert!((t.e_operator & (WO_OR | WO_AND)) == 0);
            t.left_column = cc[1];
            t.e_operator = operator_mask(op) & op_mask;
        }
        if op == TK_IS {
            wi.terms[ti].wt_flags |= TERM_IS;
        }
        if let Some(rcc) = right_cc {
            let mut e_extra_op: u16 = 0; // Bits extras para `pNew->eOperator`.
            debug_assert!(wi.terms[ti].i_field == 0);
            let p_new: TermId;
            let dup_ref: ExprRef;
            if wi.terms[ti].left_cursor >= 0 {
                let p_dup = expr_dup(wi.exprs.get(&eref), 0).expect("pDup");
                dup_ref = wi.exprs.add_root(p_dup);
                let idx_new = where_clause_insert(wi, wc, &dup_ref, TERM_VIRTUAL | TERM_DYNAMIC);
                p_new = wi.term_at(wc, idx_new);
                mark_term_as_child(wi, wc, idx_new, idx_term);
                if op == TK_IS {
                    wi.terms[p_new.0 as usize].wt_flags |= TERM_IS;
                }
                wi.terms[ti].wt_flags |= TERM_COPIED;
                let p_dup_ref = wi.exprs.get(&dup_ref).expect("pDup");
                if term_is_equivalence(db, parse, p_dup_ref) {
                    wi.terms[ti].e_operator |= WO_EQUIV;
                    e_extra_op = WO_EQUIV;
                }
            } else {
                dup_ref = eref.clone();
                p_new = term;
            }
            let flags = expr_commute(db, parse, wi.exprs.get_mut(&dup_ref).expect("pDup"));
            let dup_op = wi.exprs.get(&dup_ref).expect("pDup").op;
            let n = &mut wi.terms[p_new.0 as usize];
            n.wt_flags |= flags;
            n.left_cursor = rcc[0];
            debug_assert!((wi.terms[ti].e_operator & (WO_OR | WO_AND)) == 0);
            let n = &mut wi.terms[p_new.0 as usize];
            n.left_column = rcc[1];
            n.prereq_right = prereq_left | extra_right;
            n.prereq_all = prereq_all;
            n.e_operator = (operator_mask(dup_op) + e_extra_op) & op_mask;
        } else if fold_is_null {
            let e = wi.exprs.get_mut(&eref).expect("pExpr");
            debug_assert!(!e.has_property(EP_INT_VALUE));
            e.op = TK_TRUEFALSE; // Ver tag-20230504-1.
            e.u = ExprU::Token(Some(b"false".to_vec()));
            e.set_property(EP_IS_FALSE);
            wi.terms[ti].prereq_all = 0;
            wi.terms[ti].e_operator = 0;
        }
    }
    // Se um termo é o operador BETWEEN, cria dois termos virtuais que definem a faixa dele:
    //
    //      a BETWEEN b AND c
    //
    // vira
    //
    //      (a BETWEEN b AND c) AND (a>=b) AND (a<=c)
    //
    // Os dois termos novos são "dinâmicos" e filhos do BETWEEN: se o BETWEEN é codificado, os
    // filhos são pulados; se os filhos são satisfeitos por um índice, o BETWEEN é pulado.
    else if op == TK_BETWEEN && wc_op == TK_AND {
        const OPS: [u8; 2] = [TK_GE, TK_LE];
        for i in 0..2 {
            let p_new_expr = {
                let e = wi.exprs.get(&eref).expect("pExpr");
                debug_assert!(e.use_x_list());
                let list = e.x_list().expect("pList");
                debug_assert!(list.a.len() == 2);
                let l = expr_dup(e.p_left.as_deref(), 0);
                let r = expr_dup(list.a[i].p_expr.as_deref(), 0);
                let mut n = new_p_expr(db, parse, OPS[i] as i32, l, r);
                transfer_join_markings(n.as_deref_mut(), e);
                n.expect("pNewExpr")
            };
            let r = wi.exprs.add_root(p_new_expr);
            let idx_new = where_clause_insert(wi, wc, &r, TERM_VIRTUAL | TERM_DYNAMIC);
            expr_analyze(db, parse, wi, p_src, wc, idx_new);
            mark_term_as_child(wi, wc, idx_new, idx_term);
        }
    }
    // Analisa um termo composto de dois ou mais subtermos ligados por OR.
    else if op == TK_OR {
        debug_assert!(wc_op == TK_AND);
        expr_analyze_or_term(db, parse, wi, p_src, wc, idx_term);
    }
    // A forma "x IS NOT NULL" às vezes é avaliada com mais eficiência como "x>NULL" se x não é
    // INTEGER PRIMARY KEY. Constrói um termo virtual dessa forma, marcado com `TERM_VNULL`.
    else if op == TK_NOTNULL {
        let (is_col, i_table, i_column, outer) = {
            let e = wi.exprs.get(&eref).expect("pExpr");
            let l = e.p_left.as_deref().expect("pLeft");
            (
                l.op == TK_COLUMN && l.i_column >= 0,
                l.i_table,
                l.i_column,
                e.has_property(EP_OUTER_ON),
            )
        };
        if is_col && !outer {
            let p_new_expr = {
                let e = wi.exprs.get(&eref).expect("pExpr");
                let l = expr_dup(e.p_left.as_deref(), 0);
                new_p_expr(db, parse, TK_GT as i32, l, expr_alloc(TK_NULL as i32, None, 0))
                    .expect("pNewExpr")
            };
            let r = wi.exprs.add_root(p_new_expr);
            let idx_new =
                where_clause_insert(wi, wc, &r, TERM_VIRTUAL | TERM_DYNAMIC | TERM_VNULL);
            let nt = wi.term_at(wc, idx_new).0 as usize;
            wi.terms[nt].prereq_right = 0;
            wi.terms[nt].left_cursor = i_table;
            wi.terms[nt].left_column = i_column;
            wi.terms[nt].e_operator = WO_GT;
            mark_term_as_child(wi, wc, idx_new, idx_term);
            wi.terms[ti].wt_flags |= TERM_COPIED;
            wi.terms[nt].prereq_all = wi.terms[ti].prereq_all;
        }
    }
    // Acrescenta restrições que reduzem o espaço de busca de um LIKE ou GLOB.
    //
    // Um padrão "x LIKE 'aBc%'" vira
    //
    //          x>='ABC' AND x<'abd' AND x LIKE 'aBc%'
    //
    // O último caractere do prefixo "abc" é incrementado para formar a condição de término
    // "abd". Se maiúscula e minúscula não diferem (o padrão do LIKE), o limite inferior vai todo
    // em maiúsculas e o superior todo em minúsculas, para que os limites valham também ao
    // comparar BLOBs.
    else if op == TK_FUNCTION && wc_op == TK_AND {
        let like = {
            let e = wi.exprs.get(&eref).expect("pExpr");
            is_like_or_glob(db, parse, e)
        };
        if let Some(like) = like {
            const WT_FLAGS: u16 = TERM_LIKEOPT | TERM_VIRTUAL | TERM_DYNAMIC;
            let LikeInfo { prefix: mut p_str1, mut is_complete, no_case } = like;
            let mut p_str2 = p_str1.clone();
            debug_assert!(!p_str1.has_property(EP_INT_VALUE));
            debug_assert!(!p_str2.has_property(EP_INT_VALUE));

            // Converte o limite inferior para maiúsculas e o superior para minúsculas
            // (maiúscula é menor que minúscula em ASCII) para que as restrições de faixa
            // valham também para BLOBs.
            if no_case {
                wi.terms[ti].wt_flags |= TERM_LIKE;
                let n = z_token_mut(&mut p_str1).len();
                for i in 0..n {
                    let c = z_token_mut(&mut p_str1)[i];
                    if c == 0 {
                        break;
                    }
                    z_token_mut(&mut p_str1)[i] = to_upper(c);
                    z_token_mut(&mut p_str2)[i] = to_lower(c);
                }
            }

            {
                // O último caractere antes do primeiro curinga.
                let tok = z_token_mut(&mut p_str2);
                let last = strlen30(tok) as usize - 1;
                let mut c = tok[last];
                if no_case {
                    // A ideia é incrementar o último caractere antes do primeiro curinga. Mas
                    // incrementar '@' o empurra para a faixa alfabética, em que as conversões de
                    // caixa estragam a desigualdade. Para evitar isso, roda o LIKE completo em
                    // todas as expressões candidatas, desligando `isComplete`.
                    if c == b'A' - 1 {
                        is_complete = false;
                    }
                    c = to_lower(c);
                }
                tok[last] = c.wrapping_add(1);
            }
            let z_coll_seq_name: &[u8] = if no_case { b"NOCASE" } else { b"BINARY" };
            let (l1, l2) = {
                let e = wi.exprs.get(&eref).expect("pExpr");
                debug_assert!(e.use_x_list());
                let p_left = e.x_list().and_then(|l| l.a.get(1)).and_then(|it| it.p_expr.as_deref());
                (expr_dup(p_left, 0), expr_dup(p_left, 0))
            };
            let mut p_new_expr1 = new_p_expr(
                db,
                parse,
                TK_GE as i32,
                expr_add_collate_string(l1, z_coll_seq_name),
                Some(p_str1),
            );
            transfer_join_markings(
                p_new_expr1.as_deref_mut(),
                wi.exprs.get(&eref).expect("pExpr"),
            );
            let r1 = wi.exprs.add_root(p_new_expr1.expect("pNewExpr1"));
            let idx_new1 = where_clause_insert(wi, wc, &r1, WT_FLAGS);
            let mut p_new_expr2 = new_p_expr(
                db,
                parse,
                TK_LT as i32,
                expr_add_collate_string(l2, z_coll_seq_name),
                Some(p_str2),
            );
            transfer_join_markings(
                p_new_expr2.as_deref_mut(),
                wi.exprs.get(&eref).expect("pExpr"),
            );
            let r2 = wi.exprs.add_root(p_new_expr2.expect("pNewExpr2"));
            let idx_new2 = where_clause_insert(wi, wc, &r2, WT_FLAGS);
            expr_analyze(db, parse, wi, p_src, wc, idx_new1);
            expr_analyze(db, parse, wi, p_src, wc, idx_new2);
            if is_complete {
                mark_term_as_child(wi, wc, idx_new1, idx_term);
                mark_term_as_child(wi, wc, idx_new2, idx_term);
            }
        }
    }

    // Segunda cadeia: comparações de vetor e operadores auxiliares de tabelas virtuais.
    //
    // Um termo "==" ou IS de vetor, como "(a, b) == (?, ?)", ganha um termo novo para cada
    // comparação de componente, "a = ?" e "b = ?". Os termos novos substituem por completo a
    // comparação vetorial original, que deixa de ser usada. Só é preciso se ao menos um lado da
    // comparação não é uma subconsulta. (tag-20220128a)
    let vec_eq_n: Option<i32> = {
        let e = wi.exprs.get(&eref).expect("pExpr");
        match (e.p_left.as_deref(), e.p_right.as_deref()) {
            (Some(l), Some(r))
                if (e.op == TK_EQ || e.op == TK_IS)
                    && expr_vector_size(l) > 1
                    && expr_vector_size(r) == expr_vector_size(l)
                    && ((l.flags & EP_X_IS_SELECT) == 0 || (r.flags & EP_X_IS_SELECT) == 0)
                    && wc_op == TK_AND =>
            {
                Some(expr_vector_size(l))
            }
            _ => None,
        }
    };
    // Um IN de vetor, como "(a, b) IN (SELECT ...)", ganha um termo virtual por componente. A
    // expressão de cada termo é o próprio `pExpr` (o IN de vetor inteiro); `WhereTerm.u.x.iField`
    // identifica o índice dentro do vetor à esquerda que o termo virtual representa. Só vale se o
    // RHS é um SELECT simples (não composto) sem funções de janela.
    let vec_in_n: Option<i32> = {
        let e = wi.exprs.get(&eref).expect("pExpr");
        if e.op == TK_IN
            && wi.terms[ti].i_field == 0
            && e.p_left.as_deref().map_or(false, |l| l.op == TK_VECTOR)
            && e.use_x_select()
            && e.x_select().map_or(false, |s| {
                (s.p_prior.is_none() || (s.sel_flags & SF_VALUES) != 0) && s.n_win_linked == 0
            })
            && wc_op == TK_AND
        {
            e.p_left.as_deref().map(expr_vector_size)
        } else {
            None
        }
    };
    if let Some(n_left) = vec_eq_n {
        for i in 0..n_left {
            let p_new = {
                let e = wi.exprs.get_mut(&eref).expect("pExpr");
                let e_op = e.op;
                let p_left = expr_for_vector_field(
                    db,
                    parse,
                    e.p_left.as_deref_mut().expect("pLeft"),
                    i,
                    n_left,
                );
                let p_right = expr_for_vector_field(
                    db,
                    parse,
                    e.p_right.as_deref_mut().expect("pRight"),
                    i,
                    n_left,
                );
                let mut n = new_p_expr(db, parse, e_op as i32, p_left, p_right);
                transfer_join_markings(n.as_deref_mut(), e);
                n.expect("pNew")
            };
            let r = wi.exprs.add_root(p_new);
            let idx_new = where_clause_insert(wi, wc, &r, TERM_DYNAMIC | TERM_SLICE);
            expr_analyze(db, parse, wi, p_src, wc, idx_new);
        }
        let t = &mut wi.terms[ti];
        t.wt_flags |= TERM_CODED | TERM_VIRTUAL; // Desabilita o original.
        t.e_operator = WO_ROWVAL;
    } else if let Some(n_vec) = vec_in_n {
        for i in 0..n_vec {
            let idx_new = where_clause_insert(wi, wc, &eref, TERM_VIRTUAL | TERM_SLICE);
            let nt = wi.term_at(wc, idx_new).0 as usize;
            wi.terms[nt].i_field = i + 1;
            expr_analyze(db, parse, wi, p_src, wc, idx_new);
            mark_term_as_child(wi, wc, idx_new, idx_term);
        }
    }
    // Acrescenta um termo auxiliar WO_AUX ao conjunto de restrições se a expressão tem a forma
    // "coluna OP expr" em que OP é um operador passado às tabelas virtuais mas que não é
    // otimizado nas tabelas comuns: MATCH, LIKE, GLOB, REGEXP, !=, IS, IS NOT ou NOT NULL. A
    // informação serve ao `xBestIndex` das tabelas virtuais; o otimizador nativo não faz nada
    // com as funções MATCH.
    else if wc_op == TK_AND {
        let aux = {
            let e = wi.exprs.get(&eref).expect("pExpr");
            is_auxiliary_vtab_operator(db, e)
        };
        let mut res = aux.res;
        let mut p_left = aux.left;
        let mut p_right = aux.right;
        while res > 0 {
            res -= 1;
            let (prereq_expr, prereq_column) = {
                let e = wi.exprs.get(&eref).expect("pExpr");
                let r = p_right.and_then(|s| expr_child(e, s));
                let l = p_left.and_then(|s| expr_child(e, s));
                let pe = where_expr_usage(&mut wi.s_mask_set, r);
                let pc = where_expr_usage(&mut wi.s_mask_set, l);
                (pe, pc)
            };
            if (prereq_expr & prereq_column) == 0 {
                let (p_new_expr, l_table, l_column) = {
                    let e = wi.exprs.get(&eref).expect("pExpr");
                    let r = p_right.and_then(|s| expr_child(e, s));
                    let l = p_left.and_then(|s| expr_child(e, s)).expect("pLeft");
                    let mut n = new_p_expr(db, parse, TK_MATCH as i32, None, expr_dup(r, 0));
                    if e.has_property(EP_OUTER_ON) {
                        if let Some(nn) = n.as_deref_mut() {
                            nn.set_property(EP_OUTER_ON);
                            nn.w = e.w;
                        }
                    }
                    (n.expect("pNewExpr"), l.i_table, l.i_column)
                };
                let r = wi.exprs.add_root(p_new_expr);
                let idx_new = where_clause_insert(wi, wc, &r, TERM_VIRTUAL | TERM_DYNAMIC);
                let nt = wi.term_at(wc, idx_new).0 as usize;
                wi.terms[nt].prereq_right = prereq_expr;
                wi.terms[nt].left_cursor = l_table;
                wi.terms[nt].left_column = l_column;
                wi.terms[nt].e_operator = WO_AUX;
                wi.terms[nt].e_match_op = aux.e_op2;
                mark_term_as_child(wi, wc, idx_new, idx_term);
                wi.terms[ti].wt_flags |= TERM_COPIED;
                wi.terms[nt].prereq_all = wi.terms[ti].prereq_all;
            }
            std::mem::swap(&mut p_left, &mut p_right);
        }
    }

    // Impede que termos do ON de um LEFT JOIN conduzam um índice de tabelas à esquerda da junção.
    wi.terms[ti].prereq_right |= extra_right;
}

// ---------------------------------------------------------------------------------------------
// Chunks 002 (fim), 003 e 004: interface com o resto do subsistema where.c
// ---------------------------------------------------------------------------------------------

/// `sqlite3WhereSplit`: identifica as subexpressões da cláusula WHERE separadas pelo operador
/// AND (ou outro dado em `op`) e as acrescenta a `wc`. Em
///
/// ```text
///    WHERE  a=='hello' AND coalesce(b,11)<10 AND (c+12!=d OR c==22)
///           \________/     \_______________/     \________________/
///            slot[0]            slot[1]               slot[2]
/// ```
///
/// a expressão original não é alterada: os termos só apontam para a subestrutura dela. A
/// árvore dos termos está na arena do `wi`; `p_expr` é a referência ao nó a dividir.
pub fn where_split(wi: &mut WhereInfo, wc: ClauseId, p_expr: Option<&ExprRef>, op: u8) {
    wi.clauses[wc.0 as usize].op = op;
    let Some(r) = p_expr else {
        return;
    };
    let r2 = skip_collate_and_likely_ref(&wi.exprs, r);
    let Some(e2_op) = wi.exprs.get(&r2).map(|e| e.op) else {
        return;
    };
    if e2_op != op {
        where_clause_insert(wi, wc, r, 0);
    } else {
        where_split(wi, wc, Some(&r2.child(ExprStep::Left)), op);
        where_split(wi, wc, Some(&r2.child(ExprStep::Right)), op);
    }
}

/// `whereAddLimitExpr`: acrescenta ao WHERE um termo LIMIT (`SQLITE_INDEX_CONSTRAINT_LIMIT`) ou
/// OFFSET (`SQLITE_INDEX_CONSTRAINT_OFFSET`). O valor está no registrador `i_reg`.
///
/// No caso comum em que o valor é um inteiro simples (`LIMIT 5 OFFSET 10`) a expressão é
/// codificada como `TK_INTEGER`, para ficar disponível a `sqlite3_vtab_rhs_value()`. Senão é um
/// `TK_REGISTER`.
fn where_add_limit_expr(
    db: &mut Connection,
    parse: &mut Parse,
    wi: &mut WhereInfo,
    wc: ClauseId,
    i_reg: i32,
    p_expr: Option<&Expr>,
    i_csr: i32,
    e_match_op: i32,
) {
    let mut i_val = 0;
    let p_new = if p_expr.map_or(false, |e| expr_is_integer(e, &mut i_val) != 0) && i_val >= 0 {
        let Some(mut p_val) = expr(TK_INTEGER as i32, None) else {
            return;
        };
        p_val.set_property(EP_INT_VALUE);
        p_val.u = ExprU::IValue(i_val);
        new_p_expr(db, parse, TK_MATCH as i32, None, Some(p_val))
    } else {
        let Some(mut p_val) = expr(TK_REGISTER as i32, None) else {
            return;
        };
        p_val.i_table = i_reg;
        new_p_expr(db, parse, TK_MATCH as i32, None, Some(p_val))
    };
    if let Some(p_new) = p_new {
        let r = wi.exprs.add_root(p_new);
        let idx = where_clause_insert(wi, wc, &r, TERM_DYNAMIC | TERM_VIRTUAL);
        let t = wi.term_at(wc, idx).0 as usize;
        wi.terms[t].left_cursor = i_csr;
        wi.terms[t].e_operator = WO_AUX;
        wi.terms[t].e_match_op = e_match_op as u8;
    }
}

/// `sqlite3WhereAddLimit`: acrescenta, se cabe, os termos de LIMIT e OFFSET do SELECT `p` ao
/// WHERE. Só são acrescentados se:
///
/// ```text
///   1. O SELECT tem LIMIT, e
///   2. O SELECT não é de agregado nem DISTINCT, e
///   3. O SELECT tem exatamente um objeto no FROM, que é uma tabela virtual, e
///   4. Nenhum termo do WHERE deixará de ser passado ao `xBestIndex`, e
///   5. O ORDER BY, se existe, será passado ao `xBestIndex`.
/// ```
///
/// Os termos de LIMIT e OFFSET são ignorados pela maior parte do planejador. Existem só para
/// serem passados ao `xBestIndex` da única tabela virtual do FROM.
pub fn where_add_limit(
    db: &mut Connection,
    parse: &mut Parse,
    wi: &mut WhereInfo,
    wc: ClauseId,
    p: &Select,
) {
    debug_assert!(p.p_limit.is_some()); // 1: conferido por quem chama.
    let Some(p_src) = p.p_src.as_deref() else {
        return;
    };
    if p.p_group_by.is_none()
        && (p.sel_flags & (SF_DISTINCT | SF_AGGREGATE)) == 0 // 2
        && (p_src.a.len() == 1 && p_src.a[0].p_tab.as_ref().map_or(false, |t| t.is_virtual()))
    // 3
    {
        let p_order_by = p.p_order_by.as_deref();
        let i_csr = p_src.a[0].i_cursor;

        // Confere a condição 4 e sai cedo se ela não vale.
        for ii in 0..wi.n_term(wc) {
            let t = wi.term(wi.term_at(wc, ii));
            if (t.wt_flags & TERM_CODED) != 0 {
                // Este termo é uma operação vetorial decomposta em outros termos, posteriores.
                // Pode ser ignorado. Ver tag-20220128a.
                debug_assert!((t.wt_flags & TERM_VIRTUAL) != 0);
                debug_assert!(t.e_operator == WO_ROWVAL);
                continue;
            }
            if t.n_child != 0 {
                // Se o termo tem filhos, eles também estão em `pWC->a[]`. Então este termo pode
                // ser ignorado: o LIMIT só é acrescentado se cada filho passa no teste de
                // `leftCursor==iCsr` abaixo.
                continue;
            }
            if t.left_cursor != i_csr {
                return;
            }
            if t.prereq_right != 0 {
                return;
            }
        }

        // Confere a condição 5 e sai cedo se ela não vale.
        if let Some(ob) = p_order_by {
            for item in &ob.a {
                let Some(e) = item.p_expr.as_deref() else {
                    return;
                };
                if e.op != TK_COLUMN {
                    return;
                }
                if e.i_table != i_csr {
                    return;
                }
                if (item.fg.sort_flags & KEYINFO_ORDER_BIGNULL) != 0 {
                    return;
                }
            }
        }

        // Todas as condições valem: acrescenta os termos ao WHERE.
        let p_limit = p.p_limit.as_deref();
        debug_assert!(p_limit.map_or(false, |l| l.op == TK_LIMIT));
        if p.i_offset != 0 && (p.sel_flags & SF_COMPOUND) == 0 {
            where_add_limit_expr(
                db,
                parse,
                wi,
                wc,
                p.i_offset,
                p_limit.and_then(|l| l.p_right.as_deref()),
                i_csr,
                SQLITE_INDEX_CONSTRAINT_OFFSET,
            );
        }
        if p.i_offset == 0 || (p.sel_flags & SF_COMPOUND) == 0 {
            where_add_limit_expr(
                db,
                parse,
                wi,
                wc,
                p.i_limit,
                p_limit.and_then(|l| l.p_left.as_deref()),
                i_csr,
                SQLITE_INDEX_CONSTRAINT_LIMIT,
            );
        }
    }
}

/// `sqlite3WhereClauseClear`: libera uma cláusula (a estrutura em si, a entrada da arena, fica).
/// É o inverso de `sqlite3WhereClauseInit()`: solta as expressões dos termos dinâmicos e
/// recursivamente as subcláusulas dos termos OR e AND.
pub fn where_clause_clear(wi: &mut WhereInfo, wc: ClauseId) {
    let n = wi.n_term(wc);
    debug_assert!(n as i32 >= wi.clause(wc).n_base);
    #[cfg(debug_assertions)]
    {
        // Confere que todo termo depois de `nBase` é virtual.
        for i in wi.clause(wc).n_base as usize..n {
            debug_assert!((wi.term(wi.term_at(wc, i)).wt_flags & TERM_VIRTUAL) != 0);
        }
    }
    for i in 0..n {
        let id = wi.term_at(wc, i);
        let ti = id.0 as usize;
        debug_assert!(wi.terms[ti].e_match_op == 0 || wi.terms[ti].e_operator == WO_AUX);
        if (wi.terms[ti].wt_flags & TERM_DYNAMIC) != 0 {
            let r = wi.terms[ti].p_expr.clone();
            wi.exprs.remove_root(&r);
        }
        if (wi.terms[ti].wt_flags & (TERM_ORINFO | TERM_ANDINFO)) != 0 {
            if (wi.terms[ti].wt_flags & TERM_ORINFO) != 0 {
                debug_assert!((wi.terms[ti].wt_flags & TERM_ANDINFO) == 0);
                if let Some(info) = wi.terms[ti].p_or_info.take() {
                    where_clause_clear(wi, info.wc);
                }
            } else {
                debug_assert!((wi.terms[ti].wt_flags & TERM_ANDINFO) != 0);
                if let Some(info) = wi.terms[ti].p_and_info.take() {
                    where_clause_clear(wi, info.wc);
                }
            }
        }
    }
}

/// `sqlite3WhereExprUsageFull`: uso interno, chamado só por `sqlite3WhereExprUsageNN()` para as
/// expressões complexas. Muitas chamadas de `sqlite3WhereExprUsageNN()` não precisam da análise
/// mais complexa desta rotina.
fn where_expr_usage_full(mask_set: &mut WhereMaskSet, p: &Expr) -> Bitmask {
    let mut mask: Bitmask =
        if p.op == TK_IF_NULL_ROW { mask_set.get_mask(p.i_table) } else { 0 };
    if let Some(l) = p.p_left.as_deref() {
        mask |= where_expr_usage_nn(mask_set, l);
    }
    if let Some(r) = p.p_right.as_deref() {
        mask |= where_expr_usage_nn(mask_set, r);
        debug_assert!(matches!(p.x, ExprX::None));
    } else if p.use_x_select() {
        if p.has_property(EP_VAR_SELECT) {
            mask_set.b_var_select = 1;
        }
        mask |= expr_select_usage(mask_set, p.x_select());
    } else if let Some(l) = p.x_list() {
        mask |= where_expr_list_usage(mask_set, Some(l));
    }
    if (p.op == TK_FUNCTION || p.op == crate::consts::TK_AGG_FUNCTION) && p.use_y_win() {
        let w = p.y_win().expect("y.pWin");
        mask |= where_expr_list_usage(mask_set, w.p_partition.as_deref());
        mask |= where_expr_list_usage(mask_set, w.p_order_by.as_deref());
        mask |= where_expr_usage(mask_set, w.p_filter.as_deref());
    }
    mask
}

/// `sqlite3WhereExprUsageNN`: como `sqlite3WhereExprUsage()`, mas a expressão não pode ser nula
/// ("NN" é "Not Null"). Devolve a máscara de todas as tabelas referenciadas.
pub fn where_expr_usage_nn(mask_set: &mut WhereMaskSet, p: &Expr) -> Bitmask {
    if p.op == TK_COLUMN && !p.has_property(EP_FIXED_COL) {
        mask_set.get_mask(p.i_table)
    } else if p.has_property(EP_TOKEN_ONLY | EP_LEAF) {
        debug_assert!(p.op != TK_IF_NULL_ROW);
        0
    } else {
        where_expr_usage_full(mask_set, p)
    }
}

/// `sqlite3WhereExprUsage`: percorre a árvore da expressão e gera a máscara das tabelas
/// referenciadas. A expressão pode ser nula (devolve 0).
pub fn where_expr_usage(mask_set: &mut WhereMaskSet, p: Option<&Expr>) -> Bitmask {
    match p {
        Some(e) => where_expr_usage_nn(mask_set, e),
        None => 0,
    }
}

/// `sqlite3WhereExprListUsage`: a máscara de todas as tabelas referenciadas por toda expressão
/// da lista. A lista pode ser nula (devolve 0).
pub fn where_expr_list_usage(mask_set: &mut WhereMaskSet, p_list: Option<&ExprList>) -> Bitmask {
    let mut mask: Bitmask = 0;
    if let Some(l) = p_list {
        for item in &l.a {
            mask |= where_expr_usage(mask_set, item.p_expr.as_deref());
        }
    }
    mask
}

/// `sqlite3WhereExprAnalyze`: chama `exprAnalyze` em todos os termos da cláusula.
///
/// `exprAnalyze()` pode acrescentar termos virtuais ao fim da cláusula, e não se quer analisá-los;
/// por isso a análise começa do fim e anda para a frente, e os termos virtuais novos nunca são
/// processados.
pub fn where_expr_analyze(
    db: &mut Connection,
    parse: &mut Parse,
    wi: &mut WhereInfo,
    p_tab_list: &SrcList,
    wc: ClauseId,
) {
    let n = wi.n_term(wc);
    for i in (0..n).rev() {
        expr_analyze(db, parse, wi, p_tab_list, wc, i);
    }
}

/// `sqlite3WhereTabFuncArgs`: para funções com valor de tabela, transforma os argumentos da
/// função em termos novos do WHERE. Cada argumento vira uma restrição de igualdade contra uma
/// coluna HIDDEN da tabela.
pub fn where_tab_func_args(
    db: &mut Connection,
    parse: &mut Parse,
    wi: &mut WhereInfo,
    wc: ClauseId,
    p_item: &mut SrcItem,
) {
    if !p_item.fg.is_tab_func {
        return;
    }
    let Some(p_tab) = p_item.p_tab.clone() else {
        return;
    };
    let SrcU1::FuncArg(Some(p_args)) = &p_item.u1 else {
        return;
    };
    let mut k: usize = 0;
    for j in 0..p_args.a.len() {
        while k < p_tab.n_col as usize && !p_tab.a_col[k].is_hidden() {
            k += 1;
        }
        if k >= p_tab.n_col as usize {
            error_msg(
                db,
                parse,
                b"too many arguments on %s() - max %d",
                &[PrintfArg::Text(Some(p_tab.z_name.clone())), PrintfArg::Int(j as i64)],
            );
            return;
        }
        let Some(mut p_col_ref) = expr_alloc(TK_COLUMN as i32, None, 0) else {
            return;
        };
        p_col_ref.i_table = p_item.i_cursor;
        p_col_ref.i_column = k as i32;
        k += 1;
        debug_assert!(p_col_ref.use_y_tab());
        p_col_ref.y = ExprY::Tab(Some(TabRef::Rc(p_tab.clone())));
        p_item.col_used |= expr_col_used(&p_col_ref, None);
        let p_rhs = new_p_expr(
            db,
            parse,
            TK_UPLUS as i32,
            expr_dup(p_args.a[j].p_expr.as_deref(), 0),
            None,
        );
        let mut p_term = new_p_expr(db, parse, crate::consts::TK_EQ as i32, Some(p_col_ref), p_rhs)
            .expect("pTerm");
        let join_type = if (p_item.fg.jointype & (JT_LEFT | JT_RIGHT)) != 0 {
            EP_OUTER_ON
        } else {
            EP_INNER_ON
        };
        set_join_expr(Some(&mut p_term), p_item.i_cursor, join_type);
        let r = wi.exprs.add_root(p_term);
        where_clause_insert(wi, wc, &r, TERM_DYNAMIC);
    }
}
