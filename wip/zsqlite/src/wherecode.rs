//! Tradução de `wherecode.c` (chunks `wherecode_c.000` a `wherecode_c.007`): o gerador de código
//! do laço do WHERE (EXPLAIN QUERY PLAN de cada varredura, termos de igualdade e IN, o código de
//! início de cada nível do laço, a otimização OR e o laço do RIGHT JOIN).
//!
//! O modelo de dados é o de `where_int.rs` (arenas por `WhereInfo`, `ExprRef`, handles). Decisões
//! desta fatia:
//!
//! - `pParse->db` e `pParse->pVdbe` seguem o modelo v2: as funções recebem `db: &mut Connection` e
//!   `parse: &mut Parse` (o `Vdbe` é `parse.p_vdbe`). `WhereInfo.pTabList` e `WhereInfo.pSelect`
//!   não existem no `WhereInfo`: `p_tab_list` (e `p_select`, só onde o C o lê) são parâmetros.
//!   `pLevel` é o índice `i_level` em `wi.a`; `pLoop` é o `LoopId` de `wi.a[i_level].p_w_loop`;
//!   `pTerm` é um `TermId`; `pWC` é `wi.s_wc`.
//! - As comparações de identidade de expressão (`pLoop->aLTerm[i]->pExpr==pX`) são a igualdade de
//!   `ExprRef` dos termos, como definido em `where_int.rs`.
//! - Funções `static` do C que só este módulo usa são privadas. As três que o resto do planejador
//!   chama são públicas: `where_explain_one_scan`, `where_explain_bloom_filter`,
//!   `where_code_one_loop_start` e `where_right_join_loop`.
//! - Não existem na build do Debian e foram omitidos: `SQLITE_ENABLE_CURSOR_HINTS`
//!   (`codeCursorHint` e suas funções do `Walker`), `WHERETRACE_ENABLED`, `VdbeCoverage*`,
//!   `testcase`, `VdbeModuleComment` (só com `SQLITE_ENABLE_MODULE_COMMENTS`),
//!   `SQLITE_EXPLAIN_ESTIMATED_ROWS` e `sqlite3VdbeNoJumpsOutsideSubrtn` (só com `SQLITE_DEBUG`).
//!   `sqlite3WhereAddScanStatus` e `WhereLevel.addrVisit` também não existem (ver `where_int.rs`).
//!   As falhas de alocação (`mallocFailed`) não existem em Rust.
//! - `sqlite3WhereBegin` e `sqlite3WhereEnd` são `where_::where_begin` e `where_::where_end`, com
//!   as assinaturas que `delete.rs` e `update.rs` já usam
//!   (`where_begin(db, parse, &mut SrcList, Option<&mut Expr>, Option<..>, Option<..>, Option<..>,
//!   u16, i32) -> Option<Box<WhereInfo>>` e `where_end(db, parse, Box<WhereInfo>)`). Os três
//!   `None` do meio são `pOrderBy`, `pResultSet` e `pSelect`.
//! - `sqlite3ExprIfFalse(pParse, &sEAlt, ...)` do código de transitividade copia o nó do termo
//!   alternativo e troca o `pLeft`; em Rust o nó é clonado (a árvore é possuída) e o `p_left` da
//!   cópia é um clone do `p_left` do termo original.
//! - O `pCompare` do `OP_VFilter`/IN (`pCompare->pLeft = pLeft` e depois `= 0`) empresta o
//!   `Box` do `pLeft` por `take()` e o devolve ao lugar depois do `sqlite3ExprIfFalse`.

use std::rc::Rc;

pub(crate) use crate::vdbeaux::{vdbe_of_parse, current_addr};
use crate::build::{primary_key_index, table_column_to_index, table_column_to_storage, text_arg};
use crate::build3::src_item_arg;
use crate::connection::{Connection, Parse};
use crate::consts::{
    Bitmask, EP_INNER_ON, EP_OUTER_ON, EP_SUBQUERY, EP_SUBRTN, EP_X_IS_SELECT, IN_INDEX_INDEX_DESC,
    IN_INDEX_LOOP, IN_INDEX_ROWID, JT_LEFT, JT_LTORJ, JT_RIGHT, OPFLAG_USESEEKRESULT,
    OP_AFFINITY, OP_BEGINSUBRTN, OP_COLUMN, OP_COPY, OP_DEFERREDSEEK, OP_EXPLAIN, OP_FILTER,
    OP_FILTERADD, OP_FOUND, OP_GE, OP_GOSUB, OP_GOTO, OP_GT, OP_IDXGE, OP_IDXGT, OP_IDXINSERT,
    OP_IDXLE, OP_IDXLT, OP_IF, OP_IFNOT, OP_INITCOROUTINE, OP_INTEGER, OP_ISNULL, OP_LAST, OP_LE,
    OP_LT, OP_MAKERECORD, OP_MUSTBEINT, OP_NEXT, OP_NOOP, OP_NOTFOUND, OP_NULL, OP_NULLROW,
    OP_OPENEPHEMERAL, OP_PREV, OP_RETURN, OP_REWIND, OP_ROWID, OP_ROWSETTEST, OP_SEEKGE,
    OP_SEEKGT, OP_SEEKHIT, OP_SEEKLE, OP_SEEKLT, OP_SEEKROWID, OP_SEEKSCAN, OP_VFILTER,
    OP_VINITIN, OP_VNEXT, OP_YIELD, SQLITE_AFF_BLOB, SQLITE_AFF_NUMERIC, SQLITE_INDEX_CONSTRAINT_OFFSET,
    SQLITE_JUMPIFNULL, SQLITE_MAX_LENGTH, SQLITE_PRINTF_INTERNAL, SQLITE_SO_ASC, SQLITE_SO_DESC,
    SQLITE_STMTSTATUS_FULLSCAN_STEP, TERM_CODED, TERM_IS, TERM_LIKECOND, TERM_LIKEOPT,
    TERM_LIKE, TERM_ORINFO, TERM_SLICE, TERM_VARSELECT, TERM_VIRTUAL, TERM_VNULL, TK_AND, TK_EQ, TK_GT,
    TK_IN, TK_IS, TK_ISNULL, TK_LE, TK_LT, TK_REGISTER, TK_SELECT, TK_VECTOR, WHERE_AUTO_INDEX,
    WHERE_BIGNULL_SORT, WHERE_BOTH_LIMIT, WHERE_BTM_LIMIT, WHERE_COLUMN_EQ, WHERE_COLUMN_IN,
    WHERE_COLUMN_RANGE, WHERE_CONSTRAINT, WHERE_DUPLICATES_OK, WHERE_IDX_ONLY, WHERE_INDEXED,
    WHERE_IN_ABLE, WHERE_IN_EARLYOUT, WHERE_IN_SEEKSCAN, WHERE_IPK, WHERE_MULTI_OR,
    WHERE_ONEROW, WHERE_ORDERBY_MAX, WHERE_ORDERBY_MIN, WHERE_OR_SUBCLAUSE, WHERE_PARTIALIDX,
    WHERE_RIGHT_JOIN, WHERE_TOP_LIMIT, WHERE_TRANSCONS, WHERE_UNQ_WANTED, WHERE_VIRTUALTABLE,
    WO_ALL, WO_AND, WO_AUX, WO_EQ, WO_EQUIV, WO_GE, WO_IN, WO_IS, WO_ISNULL, WO_LE, WO_OR,
    WO_ROWVAL, XN_EXPR, XN_ROWID,
};
use crate::expr::{
    compare_affinity, expr as new_expr, expr_and, expr_dup, expr_is_vector, expr_list_append,
    expr_vector_size, p_expr as new_p_expr, vector_field_subexpr,
};
use crate::expr_code::{
    code_rhs_of_in, code_subselect, expr_can_be_null, expr_code_get_column_of_table,
    expr_code_target, expr_needs_no_affinity_change, find_in_index,
};
use crate::expr_code2::{
    expr_code, expr_code_temp, expr_compare, expr_covered_by_index, expr_if_false,
    get_temp_range, get_temp_reg, release_temp_range, release_temp_reg,
};
use crate::insert::index_affinity_str;
use crate::printf::{PrintfArg, StrAccum};
use crate::sqlite_int::{Expr, ExprX, Index, Select, SrcList, Table};
use crate::vdbe_types::{Vdbe, P4};
use crate::vdbeaux::{
    add_op0, add_op1, add_op2, add_op3, add_op4, add_op4_int, change_p1, change_p2, change_p4,
    change_p5, explain, explain_pop, get_last_op, get_op_ref, is_stmt_scanstatus, jump_here,
    make_label, resolve_label, scan_status, vdbe_comment, vdbe_goto,
};
use crate::where_::{
    where_begin, where_continue_label, where_end, where_find_term, where_uses_deferred_seek,
};
use crate::where_int::{
    ClauseId, InLoop, LoopId, TermId, WhereInfo, WhereLoop, WhereRightJoin,
};

// ---------------------------------------------------------------------------------------------
// Auxiliares privados (não existem no C)
// ---------------------------------------------------------------------------------------------



/// `SMASKBIT32(n)`: o bit `n` de uma máscara de 32 bits, ou 0 se `n` não cabe.
fn smaskbit32(n: usize) -> u32 {
    if n <= 31 {
        1u32 << n
    } else {
        0
    }
}

/// `pIdx->aSortOrder[i]`. O vetor do C tem uma posição para a coluna do rowid (sempre 0, ASC);
/// aqui `a_sort_order` pode ter só `n_key_col` entradas, e o que passa do fim é ASC.
fn sort_order(p_idx: &Index, i: usize) -> i32 {
    p_idx.a_sort_order.get(i).copied().unwrap_or(0) as i32
}

/// `pLoop->aLTerm[i]`, o termo da posição `i` (ou `None` se a vaga é nula).
fn l_term(wi: &WhereInfo, lp: LoopId, i: usize) -> Option<TermId> {
    wi.w_loop(lp).a_l_term.get(i).copied().flatten()
}

/// A tabela do FROM do nível `i_level` (`pTabList->a[pLevel->iFrom].pTab`).
fn level_table(p_tab_list: &SrcList, wi: &WhereInfo, i_level: usize) -> Rc<Table> {
    p_tab_list.a[wi.a[i_level].i_from as usize].p_tab.clone().expect("pItem->pTab")
}

/// Os dois índices são o mesmo objeto (`==` de ponteiros do C; dois nulos também são iguais).
fn same_index(a: &Option<Rc<Index>>, b: &Option<Rc<Index>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => Rc::ptr_eq(x, y),
        _ => false,
    }
}

/// `sqlite3ExprCodeTarget(pParse, p, target)` quando `p` pode ser o ponteiro nulo: o C trata
/// `pExpr==0` como `TK_NULL`.
fn code_target_opt(
    db: &mut Connection,
    parse: &mut Parse,
    p: Option<&mut Expr>,
    target: i32,
) -> i32 {
    match p {
        Some(e) => expr_code_target(db, parse, e, target, None),
        None => {
            add_op2(vdbe_of_parse(parse), OP_NULL as i32, 0, target);
            target
        }
    }
}

/// `sqlite3ExprIsVector(pX->pRight)` do termo (o ponteiro nulo não é vetor).
fn right_is_vector(wi: &WhereInfo, t: TermId) -> bool {
    wi.expr(t).p_right.as_deref().map_or(false, expr_is_vector)
}

// ---------------------------------------------------------------------------------------------
// EXPLAIN QUERY PLAN (chunk 000)
// ---------------------------------------------------------------------------------------------

/// `explainIndexColumnName`: o nome da i-ésima coluna do índice. `p_tab` é a tabela dona do
/// índice (o `Index` não tem `pTable`).
fn explain_index_column_name(p_tab: &Table, p_idx: &Index, i: usize) -> Vec<u8> {
    let c = p_idx.ai_column[i];
    if c == XN_EXPR {
        return b"<expr>".to_vec();
    }
    if c == XN_ROWID {
        return b"rowid".to_vec();
    }
    let z = &p_tab.a_col[c as usize].z_cn_name;
    let n = z.iter().position(|&b| b == 0).unwrap_or(z.len());
    z[..n].to_vec()
}

/// `explainAppendTerm`: auxiliar de `explainIndexRange`. `p_str` guarda o texto da expressão que
/// se monta um termo de cada vez; acrescenta um termo novo ao fim. Os termos são separados por
/// AND, então o texto " AND " só entra do segundo termo em diante (`b_and`).
fn explain_append_term(
    p_str: &mut StrAccum,
    p_tab: &Table,
    p_idx: &Index,
    n_term: usize,
    i_term: usize,
    b_and: bool,
    z_op: u8,
) {
    debug_assert!(n_term >= 1);
    if b_and {
        p_str.append(b" AND ");
    }
    if n_term > 1 {
        p_str.append(b"(");
    }
    for i in 0..n_term {
        if i != 0 {
            p_str.append(b",");
        }
        p_str.append_all(&explain_index_column_name(p_tab, p_idx, i_term + i));
    }
    if n_term > 1 {
        p_str.append(b")");
    }
    p_str.append(&[z_op]);
    if n_term > 1 {
        p_str.append(b"(");
    }
    for i in 0..n_term {
        if i != 0 {
            p_str.append(b",");
        }
        p_str.append(b"?");
    }
    if n_term > 1 {
        p_str.append(b")");
    }
}

/// `explainIndexRange`: o argumento descreve uma estratégia de varredura da tabela `p_tab`.
/// Acrescenta a `p_str` o texto que descreve o subconjunto de linhas varridas, na forma de uma
/// expressão SQL. Por exemplo, com `SELECT * FROM t1 WHERE a=1 AND b>2;` e um índice em (a, b), o
/// texto é parecido com " (a=? AND b>?)".
fn explain_index_range(p_str: &mut StrAccum, p_tab: &Table, p_loop: &WhereLoop) {
    let n_eq = p_loop.btree.n_eq as usize;
    let n_skip = p_loop.n_skip as usize;
    if n_eq == 0 && (p_loop.ws_flags & (WHERE_BTM_LIMIT | WHERE_TOP_LIMIT)) == 0 {
        return;
    }
    let Some(p_index) = p_loop.btree.p_index.as_deref() else {
        return;
    };
    p_str.append(b" (");
    let mut i = 0usize;
    while i < n_eq {
        let z = explain_index_column_name(p_tab, p_index, i);
        if i != 0 {
            p_str.append(b" AND ");
        }
        let fmt: &[u8] = if i >= n_skip { &b"%s=?"[..] } else { &b"ANY(%s)"[..] };
        p_str.appendf(fmt, &[text_arg(&z)]);
        i += 1;
    }
    let j = i;
    if (p_loop.ws_flags & WHERE_BTM_LIMIT) != 0 {
        explain_append_term(p_str, p_tab, p_index, p_loop.btree.n_btm as usize, j, i != 0, b'>');
        i = 1;
    }
    if (p_loop.ws_flags & WHERE_TOP_LIMIT) != 0 {
        explain_append_term(p_str, p_tab, p_index, p_loop.btree.n_top as usize, j, i != 0, b'<');
    }
    p_str.append(b")");
}

/// `sqlite3WhereExplainOneScan`: não faz nada a menos que um EXPLAIN QUERY PLAN esteja sendo
/// processado (ou `stmt_scanstatus` esteja ligado). Se faz, acrescenta um único `OP_Explain` que
/// descreve a estratégia de varredura do nível `i_level` (que lê de `p_tab_list`). `wctrl_flags`
/// são as flags passadas a `sqlite3WhereBegin()`.
///
/// Se o `OP_Explain` entrou no VM devolve o seu endereço; senão, zero.
pub fn where_explain_one_scan(
    db: &Connection,
    parse: &mut Parse,
    p_tab_list: &SrcList,
    wi: &WhereInfo,
    i_level: usize,
    wctrl_flags: u16,
) -> i32 {
    let mut ret = 0;
    if parse.toplevel().explain == 2 || is_stmt_scanstatus(db) {
        let p_level = &wi.a[i_level];
        let p_item = &p_tab_list.a[p_level.i_from as usize];
        let p_loop = wi.w_loop(p_level.p_w_loop);
        let flags = p_loop.ws_flags;
        let wctrl = wctrl_flags as u32;
        if (flags & WHERE_MULTI_OR) != 0 || (wctrl & WHERE_OR_SUBCLAUSE) != 0 {
            return 0;
        }

        // Verdadeiro para um SEARCH, falso para um SCAN.
        let is_search = (flags & (WHERE_BTM_LIMIT | WHERE_TOP_LIMIT)) != 0
            || ((flags & WHERE_VIRTUALTABLE) == 0 && p_loop.btree.n_eq > 0)
            || (wctrl & (WHERE_ORDERBY_MIN | WHERE_ORDERBY_MAX)) != 0;

        let mut acc = StrAccum::with_base(100, SQLITE_MAX_LENGTH as u32);
        acc.has_db = true;
        acc.printf_flags = SQLITE_PRINTF_INTERNAL;
        let z_verb: &[u8] = if is_search { &b"SEARCH"[..] } else { &b"SCAN"[..] };
        acc.appendf(b"%s %S", &[text_arg(z_verb), src_item_arg(p_item)]);
        let p_tab = p_item.p_tab.as_deref();
        if (flags & (WHERE_IPK | WHERE_VIRTUALTABLE)) == 0 {
            let mut z_fmt: Option<&[u8]> = None;
            if let (Some(p_idx), Some(p_tab)) = (p_loop.btree.p_index.as_deref(), p_tab) {
                debug_assert!((flags & WHERE_AUTO_INDEX) == 0 || (flags & WHERE_IDX_ONLY) != 0);
                if !p_tab.has_rowid() && p_idx.is_primary_key_index() {
                    if is_search {
                        z_fmt = Some(&b"PRIMARY KEY"[..]);
                    }
                } else if (flags & WHERE_PARTIALIDX) != 0 {
                    z_fmt = Some(&b"AUTOMATIC PARTIAL COVERING INDEX"[..]);
                } else if (flags & WHERE_AUTO_INDEX) != 0 {
                    z_fmt = Some(&b"AUTOMATIC COVERING INDEX"[..]);
                } else if (flags & WHERE_IDX_ONLY) != 0 {
                    z_fmt = Some(&b"COVERING INDEX %s"[..]);
                } else {
                    z_fmt = Some(&b"INDEX %s"[..]);
                }
                if let Some(fmt) = z_fmt {
                    acc.append(b" USING ");
                    acc.appendf(fmt, &[text_arg(&p_idx.z_name)]);
                    explain_index_range(&mut acc, p_tab, p_loop);
                }
            }
        } else if (flags & WHERE_IPK) != 0 && (flags & WHERE_CONSTRAINT) != 0 {
            let z_rowid: &[u8] = b"rowid";
            acc.appendf(b" USING INTEGER PRIMARY KEY (%s", &[text_arg(z_rowid)]);
            let c_range_op: u8;
            if (flags & (WHERE_COLUMN_EQ | WHERE_COLUMN_IN)) != 0 {
                c_range_op = b'=';
            } else if (flags & WHERE_BOTH_LIMIT) == WHERE_BOTH_LIMIT {
                acc.appendf(b">? AND %s", &[text_arg(z_rowid)]);
                c_range_op = b'<';
            } else if (flags & WHERE_BTM_LIMIT) != 0 {
                c_range_op = b'>';
            } else {
                debug_assert!((flags & WHERE_TOP_LIMIT) != 0);
                c_range_op = b'<';
            }
            acc.appendf(b"%c?)", &[PrintfArg::Char(c_range_op as u32)]);
        } else if (flags & WHERE_VIRTUALTABLE) != 0 {
            acc.appendf(
                b" VIRTUAL TABLE INDEX %d:%s",
                &[
                    PrintfArg::Int(p_loop.vtab.idx_num as i64),
                    PrintfArg::Text(p_loop.vtab.idx_str.clone()),
                ],
            );
        }
        if (p_item.fg.jointype & JT_LEFT) != 0 {
            acc.appendf(b" LEFT-JOIN", &[]);
        }
        let z_msg = acc.finish();
        let addr_explain = parse.addr_explain;
        let v = vdbe_of_parse(parse);
        let i_this = v.n_op();
        ret = add_op4(
            v,
            OP_EXPLAIN as i32,
            i_this,
            addr_explain,
            0,
            z_msg.map_or(P4::None, P4::Text),
        );
    }
    ret
}

/// `sqlite3WhereExplainBloomFilter`: acrescenta um único `OP_Explain` que descreve um filtro de
/// Bloom. Devolve o endereço dele.
pub fn where_explain_bloom_filter(
    db: &Connection,
    parse: &mut Parse,
    wi: &WhereInfo,
    p_tab_list: &SrcList,
    i_level: usize,
) -> i32 {
    let p_level = &wi.a[i_level];
    let p_item = &p_tab_list.a[p_level.i_from as usize];
    let mut acc = StrAccum::with_base(100, SQLITE_MAX_LENGTH as u32);
    acc.has_db = true;
    acc.printf_flags = SQLITE_PRINTF_INTERNAL;
    acc.appendf(b"BLOOM FILTER ON %S (", &[src_item_arg(p_item)]);
    let p_loop = wi.w_loop(p_level.p_w_loop);
    if (p_loop.ws_flags & WHERE_IPK) != 0 {
        let p_tab = p_item.p_tab.as_deref().expect("pItem->pTab");
        if p_tab.i_p_key >= 0 {
            let z = &p_tab.a_col[p_tab.i_p_key as usize].z_cn_name;
            let n = z.iter().position(|&b| b == 0).unwrap_or(z.len());
            acc.appendf(b"%s=?", &[text_arg(&z[..n])]);
        } else {
            acc.appendf(b"rowid=?", &[]);
        }
    } else {
        let p_tab = p_item.p_tab.as_deref().expect("pItem->pTab");
        let p_idx = p_loop.btree.p_index.as_deref().expect("pLoop->u.btree.pIndex");
        let n_skip = p_loop.n_skip as usize;
        for i in n_skip..p_loop.btree.n_eq as usize {
            let z = explain_index_column_name(p_tab, p_idx, i);
            if i > n_skip {
                acc.append(b" AND ");
            }
            acc.appendf(b"%s=?", &[text_arg(&z)]);
        }
    }
    acc.append(b")");
    let z_msg = acc.finish();
    let addr_explain = parse.addr_explain;
    let v = vdbe_of_parse(parse);
    let i_this = v.n_op();
    let ret = add_op4(
        v,
        OP_EXPLAIN as i32,
        i_this,
        addr_explain,
        0,
        z_msg.map_or(P4::None, P4::Text),
    );
    let a = v.n_op() - 1;
    scan_status(v, db, a, 0, 0, 0, None);
    ret
}

// ---------------------------------------------------------------------------------------------
// Desabilitar termos, afinidades, IN com vetor (chunks 000 e 001)
// ---------------------------------------------------------------------------------------------

/// `disableTerm`: desabilita um termo da cláusula WHERE. Exceto que não o desabilita se ele
/// controla um LEFT OUTER JOIN e não nasceu na cláusula ON ou USING desse join.
///
/// Considere o termo t2.z='ok' nas consultas:
///
/// ```text
///   (1)  SELECT * FROM t1 LEFT JOIN t2 ON t1.a=t2.x WHERE t2.z='ok'
///   (2)  SELECT * FROM t1 LEFT JOIN t2 ON t1.a=t2.x AND t2.z='ok'
///   (3)  SELECT * FROM t1, t2 WHERE t1.a=t2.x AND t2.z='ok'
/// ```
///
/// O t2.z='ok' é desabilitado em (2) porque nasce na cláusula ON. Em (3) também, porque não faz
/// parte de um LEFT OUTER JOIN. Em (1) não é desabilitado.
///
/// Desabilitar um termo faz com que ele não seja testado no laço interno da junção. É uma
/// otimização: o resultado sai certo mesmo que nada seja desabilitado, mas as junções podem ficar
/// um pouco mais lentas. Se todos os filhos de um termo estão desabilitados, o próprio termo
/// também é desabilitado. Assim, termos viram desabilitados se os termos virtuais derivados são
/// testados primeiro. Por exemplo:
///
/// ```text
///      x GLOB 'abc*' AND x>='abc' AND x<'acd'
///      \___________/     \______/     \_____/
///         parent          child1       child2
/// ```
///
/// Só o termo pai estava no WHERE original. Os termos filhos foram acrescentados pela otimização
/// do LIKE. Se os dois filhos virtuais valem, o teste do pai pode ser pulado.
///
/// Em geral o termo pai é marcado `TERM_CODED`. Mas se o pai era originalmente `TERM_LIKE`, ele
/// recebe `TERM_LIKECOND`, que indica que o termo deve ser codificado dentro de uma condição que
/// só vale na segunda passada de um laço da otimização do LIKE, quando varre BLOBs em vez de
/// strings.
fn disable_term(wi: &mut WhereInfo, i_level: usize, p_term: TermId) {
    let mut n_loop = 0;
    let mut t = p_term;
    loop {
        let term = wi.term(t);
        if (term.wt_flags & TERM_CODED) != 0 {
            break;
        }
        let lvl = &wi.a[i_level];
        if !(lvl.i_left_join == 0 || wi.expr(t).has_property(EP_OUTER_ON)) {
            break;
        }
        if (lvl.not_ready & term.prereq_all) != 0 {
            break;
        }
        let i_parent = term.i_parent;
        let p_wc = term.p_wc;
        if n_loop != 0 && (term.wt_flags & TERM_LIKE) != 0 {
            wi.term_mut(t).wt_flags |= TERM_LIKECOND;
        } else {
            wi.term_mut(t).wt_flags |= TERM_CODED;
        }
        if i_parent < 0 {
            break;
        }
        t = wi.term_at(p_wc, i_parent as usize);
        let p = wi.term_mut(t);
        p.n_child = p.n_child.wrapping_sub(1);
        if p.n_child != 0 {
            break;
        }
        n_loop += 1;
    }
}

/// `codeApplyAffinity`: gera um `OP_Affinity` que aplica a string de afinidades `z_aff` aos `n`
/// registradores a partir de `base`.
///
/// Como otimização, as entradas `SQLITE_AFF_BLOB` e `SQLITE_AFF_NONE` (que não fazem nada) no
/// começo e no fim de `z_aff` são ignoradas. Se todas as entradas são BLOB ou NONE, nenhum código
/// é gerado. A rotina faz a própria cópia de `z_aff` (o texto do P4), para o chamador ficar livre
/// para alterá-la depois.
fn code_apply_affinity(parse: &mut Parse, base: i32, n: i32, z_aff: &[u8]) {
    debug_assert!(parse.p_vdbe.is_some());
    // Ajusta `base` e `n` para pular as entradas BLOB e NONE do começo e do fim da string.
    let mut base = base;
    let mut n = n;
    let mut off = 0usize;
    while n > 0 && z_aff[off] <= SQLITE_AFF_BLOB {
        n -= 1;
        base += 1;
        off += 1;
    }
    while n > 1 && z_aff[off + n as usize - 1] <= SQLITE_AFF_BLOB {
        n -= 1;
    }
    // Gera o `OP_Affinity` se sobrou algo a fazer.
    if n > 0 {
        let z = z_aff[off..off + n as usize].to_vec();
        add_op4(vdbe_of_parse(parse), OP_AFFINITY as i32, base, n, 0, P4::Text(z));
    }
}

/// `updateRangeAffinityStr`: a expressão `p_right`, o lado direito de uma comparação, é um vetor
/// de `n` elementos ou, se `n==1`, uma expressão escalar. Antes da comparação, a afinidade de
/// `z_aff` será aplicada aos valores de `p_right`. Esta função muda para `SQLITE_AFF_BLOB` os
/// caracteres da string de afinidades quando a comparação é feita sem afinidade ou quando a
/// mudança de afinidade não pode mudar o valor.
fn update_range_affinity_str(p_right: &Expr, n: i32, z_aff: &mut [u8]) {
    for i in 0..n as usize {
        if let Some(p) = vector_field_subexpr(p_right, i as i32) {
            if compare_affinity(p, z_aff[i], None) == SQLITE_AFF_BLOB
                || expr_needs_no_affinity_change(p, z_aff[i])
            {
                z_aff[i] = SQLITE_AFF_BLOB;
            }
        }
    }
}

/// `removeUnindexableInClauseTerms`: `p_x` (a expressão `p_x_ref` do `WhereInfo`) tem a forma
/// `(vetor) IN (SELECT ...)`: um operador IN vetorial com um SELECT no lado direito. Mas nem
/// todos os termos do vetor são indexáveis e eles podem não estar na ordem certa para o índice.
///
/// A rotina copia `p_x` e ajusta o vetor do lado esquerdo, com as mudanças correspondentes no
/// SELECT, para que o vetor contenha só termos do índice e na ordem certa. Devolve o IN
/// modificado; o chamador é dono dele.
///
/// Exemplo:
///
/// ```text
///    CREATE TABLE t1(a,b,c,d,e,f);
///    CREATE INDEX t1x1 ON t1(e,c);
///    SELECT * FROM t1 WHERE (a,b,c,d,e) IN (SELECT v,w,x,y,z FROM t2)
///                           \_______________________________________/
///                                     A expressão pX
/// ```
///
/// Como só as colunas e e c podem usar o índice, nessa ordem, o IN devolvido é
/// `(e,c) IN (SELECT z,x FROM t2)`.
///
/// O pX reduzido só serve à indexação, para melhorar o desempenho. O IN original, sem alteração,
/// também precisa rodar a cada linha de saída, por correção.
fn remove_unindexable_in_clause_terms(
    wi: &WhereInfo,
    i_eq: usize,
    p_loop: LoopId,
    p_x_ref: &crate::where_int::ExprRef,
) -> Box<Expr> {
    let mut p_new = expr_dup(wi.exprs.get(p_x_ref), 0).expect("pX");
    let n_l_term = wi.w_loop(p_loop).n_l_term as usize;
    {
        let Expr { p_left, x, .. } = &mut *p_new;
        let mut cur: Option<&mut Select> = match x {
            ExprX::Select(s) => Some(&mut **s),
            _ => None,
        };
        let mut first = true;
        while let Some(p_select) = cur {
            // O lado esquerdo só é alterado para o primeiro SELECT do composto.
            let mut p_orig_lhs: Option<Box<crate::sqlite_int::ExprList>> = if first {
                p_left.as_deref_mut().and_then(|l| match std::mem::replace(&mut l.x, ExprX::None) {
                    ExprX::List(list) => Some(list),
                    other => {
                        l.x = other;
                        None
                    }
                })
            } else {
                None
            };
            let mut p_orig_rhs = p_select.p_e_list.take().unwrap_or_default();
            let mut p_rhs: Option<Box<crate::sqlite_int::ExprList>> = None;
            let mut p_lhs: Option<Box<crate::sqlite_int::ExprList>> = None;
            for i in i_eq..n_l_term {
                let Some(t) = l_term(wi, p_loop, i) else {
                    continue;
                };
                if wi.term(t).p_expr == *p_x_ref {
                    debug_assert!((wi.term(t).e_operator & (WO_OR | WO_AND)) == 0);
                    let i_field = (wi.term(t).i_field - 1) as usize;
                    // Coluna duplicada da PK.
                    let Some(item) = p_orig_rhs.a.get_mut(i_field) else {
                        continue;
                    };
                    if item.p_expr.is_none() {
                        continue;
                    }
                    p_rhs = expr_list_append(p_rhs, item.p_expr.take());
                    if let Some(lhs) = p_orig_lhs.as_deref_mut() {
                        debug_assert!(lhs.a[i_field].p_expr.is_some());
                        p_lhs = expr_list_append(p_lhs, lhs.a[i_field].p_expr.take());
                    }
                }
            }
            // `sqlite3ExprListDelete(db, pOrigRhs)`: `p_orig_rhs` cai aqui.
            drop(p_orig_rhs);
            if p_orig_lhs.is_some() {
                // `sqlite3ExprListDelete(db, pOrigLhs)` e `pNew->pLeft->x.pList = pLhs`.
                drop(p_orig_lhs.take());
                match p_lhs {
                    Some(mut lhs) if lhs.a.len() == 1 => {
                        // Cuidado para não gerar um TK_VECTOR com um único valor. Como o analisador
                        // nunca cria isso, algumas sub-rotinas não tratam esse caso.
                        let p = lhs.a[0].p_expr.take();
                        *p_left = p;
                    }
                    other => {
                        if let Some(l) = p_left.as_deref_mut() {
                            l.x = match other {
                                Some(lhs) => ExprX::List(lhs),
                                None => ExprX::None,
                            };
                        }
                    }
                }
            }
            p_select.p_e_list = p_rhs;
            if let Some(ob) = p_select.p_order_by.as_deref_mut() {
                // Se o SELECT tem ORDER BY, zera os `iOrderByCol`. Eles são não nulos quando um
                // termo do ORDER BY casa exatamente com um do conjunto de resultados. Como o
                // conjunto de resultados pode ter sido modificado ou reordenado, esses valores não
                // estão mais certos. Eles são só uma otimização, então é mais fácil zerá-los.
                for item in ob.a.iter_mut() {
                    item.i_order_by_col = 0;
                }
            }
            first = false;
            cur = p_select.p_prior.as_deref_mut();
        }
    }
    p_new
}

// ---------------------------------------------------------------------------------------------
// Termos de igualdade e IN (chunk 001)
// ---------------------------------------------------------------------------------------------

/// `codeEqualityTerm`: gera o código de um único termo de igualdade da cláusula WHERE. Um termo
/// de igualdade pode ser `X=expr` ou `X IN (...)`. `p_term` é o termo a codificar.
///
/// O valor corrente da restrição fica num registrador, cujo número é devolvido. Tenta-se
/// guardar o resultado em `i_target`, mas isso só é garantido para restrições `TK_ISNULL` e
/// `TK_IN`. Para `TK_EQ` ou `TK_IS`, o valor pode ficar em outro registrador, e compensar isso é
/// responsabilidade do chamador.
///
/// Para uma restrição `X=expr`, a expressão é avaliada em código linear. Para `X IN (...)`, a
/// rotina monta um laço que percorre todos os valores de X.
fn code_equality_term(
    db: &mut Connection,
    parse: &mut Parse,
    wi: &mut WhereInfo,
    p_term: TermId,
    i_level: usize,
    i_eq: usize,
    b_rev: bool,
    i_target: i32,
) -> i32 {
    let mut b_rev = b_rev;
    let lp = wi.a[i_level].p_w_loop;
    let x_op = wi.expr(p_term).op;
    let i_reg: i32;

    debug_assert!(l_term(wi, lp, i_eq) == Some(p_term));
    debug_assert!(i_target > 0);
    if x_op == TK_EQ || x_op == TK_IS {
        i_reg = code_target_opt(db, parse, wi.expr_mut(p_term).p_right.as_deref_mut(), i_target);
    } else if x_op == TK_ISNULL {
        i_reg = i_target;
        add_op2(vdbe_of_parse(parse), OP_NULL as i32, 0, i_reg);
    } else {
        let x_ref = wi.term(p_term).p_expr.clone();
        let mut n_eq: usize = 0;

        if (wi.w_loop(lp).ws_flags & WHERE_VIRTUALTABLE) == 0 {
            if let Some(p_idx) = wi.w_loop(lp).btree.p_index.as_deref() {
                if sort_order(p_idx, i_eq) != 0 {
                    b_rev = !b_rev;
                }
            }
        }
        debug_assert!(x_op == TK_IN);
        i_reg = i_target;

        for i in 0..i_eq {
            if let Some(t) = l_term(wi, lp, i) {
                if wi.term(t).p_expr == x_ref {
                    disable_term(wi, i_level, p_term);
                    return i_target;
                }
            }
        }
        let n_l = wi.w_loop(lp).n_l_term as usize;
        for i in i_eq..n_l {
            debug_assert!(l_term(wi, lp, i).is_some());
            if let Some(t) = l_term(wi, lp, i) {
                if wi.term(t).p_expr == x_ref {
                    n_eq += 1;
                }
            }
        }

        let mut i_tab: i32 = 0;
        let e_type: i32;
        let mut ai_map: Option<Vec<i32>> = None;
        let single = {
            let e = wi.expr(p_term);
            !e.use_x_select()
                || e.x_select()
                    .and_then(|s| s.p_e_list.as_deref())
                    .map_or(false, |l| l.a.len() == 1)
        };
        if single {
            e_type = find_in_index(
                db,
                parse,
                wi.expr_mut(p_term),
                IN_INDEX_LOOP,
                None,
                None,
                &mut i_tab,
                None,
            );
        } else {
            let (i_table, has_subrtn) = {
                let e = wi.expr(p_term);
                (e.i_table, e.has_property(EP_SUBRTN))
            };
            if i_table == 0 || !has_subrtn {
                let mut p_new = remove_unindexable_in_clause_terms(wi, i_eq, lp, &x_ref);
                let mut m = vec![0i32; n_eq];
                e_type = find_in_index(
                    db,
                    parse,
                    &mut *p_new,
                    IN_INDEX_LOOP,
                    None,
                    Some(m.as_mut_slice()),
                    &mut i_tab,
                    None,
                );
                wi.expr_mut(p_term).i_table = i_tab;
                ai_map = Some(m);
                // `sqlite3ExprDelete(db, pX)`: `p_new` cai aqui.
            } else {
                let n = wi.expr(p_term).p_left.as_deref().map_or(1, expr_vector_size) as usize;
                let mut m = vec![0i32; n_eq.max(n)];
                e_type = find_in_index(
                    db,
                    parse,
                    wi.expr_mut(p_term),
                    IN_INDEX_LOOP,
                    None,
                    Some(m.as_mut_slice()),
                    &mut i_tab,
                    None,
                );
                ai_map = Some(m);
            }
        }

        if e_type == IN_INDEX_INDEX_DESC {
            b_rev = !b_rev;
        }
        add_op2(vdbe_of_parse(parse), (if b_rev { OP_LAST } else { OP_REWIND }) as i32, i_tab, 0);

        debug_assert!((wi.w_loop(lp).ws_flags & WHERE_MULTI_OR) == 0);
        wi.w_loop_mut(lp).ws_flags |= WHERE_IN_ABLE;
        if wi.a[i_level].a_in_loop.is_empty() {
            wi.a[i_level].addr_nxt = make_label(parse);
        }
        if i_eq > 0 && (wi.w_loop(lp).ws_flags & WHERE_IN_SEEKSCAN) == 0 {
            wi.w_loop_mut(lp).ws_flags |= WHERE_IN_EARLYOUT;
        }

        let first_in = wi.a[i_level].a_in_loop.len();
        wi.a[i_level].a_in_loop.resize(first_in + n_eq, InLoop::default());
        let mut p_in = first_in;
        let mut i_map = 0usize;
        for i in i_eq..n_l {
            let Some(t) = l_term(wi, lp, i) else {
                continue;
            };
            if wi.term(t).p_expr == x_ref {
                let i_out = i_reg + (i - i_eq) as i32;
                let addr_in_top = if e_type == IN_INDEX_ROWID {
                    add_op2(vdbe_of_parse(parse), OP_ROWID as i32, i_tab, i_out)
                } else {
                    let i_col = match ai_map.as_ref() {
                        Some(m) => {
                            let c = m[i_map];
                            i_map += 1;
                            c
                        }
                        None => 0,
                    };
                    add_op3(vdbe_of_parse(parse), OP_COLUMN as i32, i_tab, i_col, i_out)
                };
                wi.a[i_level].a_in_loop[p_in].addr_in_top = addr_in_top;
                add_op1(vdbe_of_parse(parse), OP_ISNULL as i32, i_out);
                let p = &mut wi.a[i_level].a_in_loop[p_in];
                if i == i_eq {
                    p.i_cur = i_tab;
                    p.e_end_loop_op = if b_rev { OP_PREV } else { OP_NEXT };
                    if i_eq > 0 {
                        p.i_base = i_reg - i as i32;
                        p.n_prefix = i as i32;
                    } else {
                        p.n_prefix = 0;
                    }
                } else {
                    p.e_end_loop_op = OP_NOOP;
                }
                p_in += 1;
            }
        }
        if i_eq > 0
            && (wi.w_loop(lp).ws_flags & (WHERE_IN_SEEKSCAN | WHERE_VIRTUALTABLE)) == 0
        {
            let i_idx_cur = wi.a[i_level].i_idx_cur;
            add_op3(vdbe_of_parse(parse), OP_SEEKHIT as i32, i_idx_cur, 0, i_eq as i32);
        }
    }

    // Como otimização, tenta desabilitar o termo da cláusula WHERE que está dirigindo o índice,
    // já que ele será sempre verdadeiro. A resposta sai certa de qualquer jeito, mas pode sair
    // com menos ciclos de CPU se o termo for omitido.
    //
    // Mas não desabilita o termo a menos que se tenha certeza de que ele não é uma restrição
    // transitiva. Um exemplo em que isso não funciona está em
    // https://sqlite.org/forum/forumpost/eb8613976a (2021-05-04).
    if (wi.w_loop(lp).ws_flags & WHERE_TRANSCONS) == 0
        || (wi.term(p_term).e_operator & WO_EQUIV) == 0
    {
        disable_term(wi, i_level, p_term);
    }

    i_reg
}

/// `codeAllEqualityTerms`: gera o código que avalia todas as restrições `==` e `IN` de uma
/// varredura de índice.
///
/// Por exemplo, com a tabela t1(a,b,c,d,e,f) e o índice i1(a,b,c), suponha o WHERE
/// `a==5 AND b IN (1,2,3) AND c>5 AND c<10`. O índice poderia ter até três restrições de
/// igualdade, mas neste exemplo o terceiro valor "c" é uma desigualdade. Então só duas
/// restrições são codificadas: esta rotina gera o código de `a==5` e de `b IN (1,2,3)`. Os
/// valores correntes de a e b ficam em registradores consecutivos, e o índice do primeiro
/// registrador é devolvido.
///
/// No exemplo, nEq==2. Mas a rotina vale para qualquer nEq, inclusive 0. Com nEq==0 ela quase
/// não faz nada: só aloca as células de memória e calcula a string de afinidades.
///
/// O parâmetro `n_extra_reg` é 0 ou 1. É 0 se todas as restrições do WHERE são `==` ou `IN` e
/// estão cobertas por nEq. É 1 se há uma desigualdade (como o "c>=5 AND c<10" do exemplo) depois
/// das igualdades.
///
/// A rotina aloca `nEq+nExtraReg` células de memória e devolve o índice da primeira. O código
/// que a chama usa essa faixa para guardar as chaves das condições de início e término do laço.
/// Se um ou mais operadores IN aparecem, a rotina aloca também nEq células adicionais para uso
/// interno.
///
/// Devolve junto uma cópia da string de afinidades de colunas do índice. As entradas associadas
/// às restrições de igualdade que usam afinidade BLOB ou NONE ficam `SQLITE_AFF_BLOB`. Isso trata
/// SQL como:
///
/// ```text
///   CREATE TABLE t1(a TEXT PRIMARY KEY, b);
///   SELECT ... FROM t1 AS t2, t1 WHERE t1.a = t2.b;
/// ```
///
/// No exemplo, o índice em t1(a) tem afinidade TEXT. Mas como o lado direito (t2.b) tem afinidade
/// BLOB/NONE, nenhuma conversão deve ser tentada antes de usar um valor de t2.b como parte de uma
/// chave de busca no índice. Logo o primeiro byte da string devolvida é `SQLITE_AFF_BLOB`.
///
/// `p_tab` é a tabela do nível (dona do índice).
fn code_all_equality_terms(
    db: &mut Connection,
    parse: &mut Parse,
    wi: &mut WhereInfo,
    p_tab: &Rc<Table>,
    i_level: usize,
    b_rev: bool,
    n_extra_reg: i32,
) -> (i32, Vec<u8>) {
    // Esta rotina só é chamada em planos que usam um índice.
    let lp = wi.a[i_level].p_w_loop;
    debug_assert!((wi.w_loop(lp).ws_flags & WHERE_VIRTUALTABLE) == 0);
    let n_eq = wi.w_loop(lp).btree.n_eq as usize;
    let n_skip = wi.w_loop(lp).n_skip as usize;
    let p_idx: Rc<Index> = wi.w_loop(lp).btree.p_index.clone().expect("pLoop->u.btree.pIndex");

    // Descobre quantas células de memória serão necessárias e as aloca.
    let mut reg_base = parse.n_mem + 1;
    let n_reg = n_eq as i32 + n_extra_reg;
    parse.n_mem += n_reg;

    let mut z_aff: Vec<u8> = index_affinity_str(&p_idx, p_tab);

    if n_skip != 0 {
        let i_idx_cur = wi.a[i_level].i_idx_cur;
        add_op3(vdbe_of_parse(parse), OP_NULL as i32, 0, reg_base, reg_base + n_skip as i32 - 1);
        add_op1(vdbe_of_parse(parse), (if b_rev { OP_LAST } else { OP_REWIND }) as i32, i_idx_cur);
        vdbe_comment(vdbe_of_parse(parse), b"begin skip-scan on %s", &[text_arg(&p_idx.z_name)]);
        let j = add_op0(vdbe_of_parse(parse), OP_GOTO as i32);
        debug_assert!(wi.a[i_level].addr_skip == 0);
        wi.a[i_level].addr_skip = add_op4_int(
            vdbe_of_parse(parse),
            (if b_rev { OP_SEEKLT } else { OP_SEEKGT }) as i32,
            i_idx_cur,
            0,
            reg_base,
            n_skip as i32,
        );
        jump_here(vdbe_of_parse(parse), j);
        for j in 0..n_skip {
            add_op3(vdbe_of_parse(parse), OP_COLUMN as i32, i_idx_cur, j as i32, reg_base + j as i32);
            vdbe_comment(
                vdbe_of_parse(parse),
                b"%s",
                &[text_arg(&explain_index_column_name(p_tab, &p_idx, j))],
            );
        }
    }

    // Avalia as restrições de igualdade.
    debug_assert!(z_aff.len() >= n_eq);
    for j in n_skip..n_eq {
        let p_term = l_term(wi, lp, j).expect("pLoop->aLTerm[j]");
        // O caso abaixo ocorre em índices com colunas redundantes.
        // Ex: CREATE INDEX i1 ON t1(a,b,a); SELECT * FROM t1 WHERE a=0 AND b=0;
        let r1 = code_equality_term(db, parse, wi, p_term, i_level, j, b_rev, reg_base + j as i32);
        if r1 != reg_base + j as i32 {
            if n_reg == 1 {
                release_temp_reg(parse, reg_base);
                reg_base = r1;
            } else {
                add_op2(vdbe_of_parse(parse), OP_COPY as i32, r1, reg_base + j as i32);
            }
        }
        if (wi.term(p_term).e_operator & WO_IN) != 0 {
            if wi.expr(p_term).has_property(EP_X_IS_SELECT) {
                // Nenhuma afinidade precisa (nem deve) ser aplicada a um valor do lado direito
                // de um "? IN (SELECT ...)". A rotina `sqlite3FindInIndex()` já garantiu que a
                // afinidade da comparação foi aplicada ao valor.
                z_aff[j] = SQLITE_AFF_BLOB;
            }
        } else if (wi.term(p_term).e_operator & WO_ISNULL) == 0 {
            if (wi.term(p_term).wt_flags & TERM_IS) == 0
                && expr_can_be_null(wi.expr(p_term).p_right.as_deref())
            {
                let addr_brk = wi.a[i_level].addr_brk;
                add_op2(vdbe_of_parse(parse), OP_ISNULL as i32, reg_base + j as i32, addr_brk);
            }
            if parse.n_err == 0 {
                if let Some(p_right) = wi.expr(p_term).p_right.as_deref() {
                    if compare_affinity(p_right, z_aff[j], None) == SQLITE_AFF_BLOB {
                        z_aff[j] = SQLITE_AFF_BLOB;
                    }
                    if expr_needs_no_affinity_change(p_right, z_aff[j]) {
                        z_aff[j] = SQLITE_AFF_BLOB;
                    }
                }
            }
        }
    }
    (reg_base, z_aff)
}

// ---------------------------------------------------------------------------------------------
// Auxiliares do início do laço (chunks 002 e 003)
// ---------------------------------------------------------------------------------------------

/// `whereLikeOptimizationStringFixup`: se a instrução codificada mais recentemente é uma
/// restrição de intervalo constante (um literal string) que veio da otimização do LIKE, grava P3
/// e P5 do `OP_String8` para que a string seja convertida em BLOB nos momentos certos.
///
/// A otimização do LIKE tenta avaliar "x LIKE 'abc%'" como uma expressão de intervalo:
/// "x>='ABC' AND x<'abd'". Mas isso exige que o laço de varredura do intervalo rode duas vezes,
/// uma para strings e outra para BLOBs. Os `OP_String8` da segunda passada convertem as constantes
/// de limite inferior e superior em blobs. Esta rotina faz as mudanças necessárias nos opcodes.
///
/// `i_like_rep_cntr` é o `pLevel->iLikeRepCntr` e `wt_flags` o `pTerm->wtFlags` do limite que
/// acabou de ser codificado.
fn where_like_optimization_string_fixup(v: &mut Vdbe, i_like_rep_cntr: u32, wt_flags: u16) {
    if (wt_flags & TERM_LIKEOPT) != 0 {
        debug_assert!(i_like_rep_cntr > 0);
        if let Some(p_op) = get_last_op(v) {
            // Registrador que guarda o contador.
            p_op.p3 = (i_like_rep_cntr >> 1) as i32;
            // ASC ou DESC.
            p_op.p5 = (i_like_rep_cntr & 1) as u16;
        }
    }
}

/// `codeDeferredSeek`: o cursor `i_cur` está aberto numa árvore-b intkey (uma tabela). O registrador
/// com o rowid acabou de ser lido do cursor `i_idx_cur`, aberto no índice `p_idx`. Gera o código
/// do seek adiado de `i_cur` para esse rowid.
///
/// Normalmente é só `OP_DeferredSeek $iCur $iRowid`, que faz o seek em `$iCur` para a linha de
/// rowid `$iRowid`.
///
/// Porém, se a varredura é um ramo de um laço OR e o comando é um SELECT, informação adicional
/// pode permitir que `OP_Column` omita o seek e faça a busca no índice, evitando um seek caro.
/// Para isso o P3 do `OP_DeferredSeek` recebe `i_idx_cur` e o P4 um vetor de inteiros com uma
/// entrada por coluna da tabela: se a coluna é a i-ésima do índice, a entrada é (i+1); se a
/// coluna não aparece no índice, 0. `OP_Column` consulta o vetor para saber se a coluna que quer
/// está no índice e, se está, troca o cursor e o número da coluna.
fn code_deferred_seek(
    parse: &mut Parse,
    wi: &mut WhereInfo,
    p_idx: &Index,
    p_tab: &Rc<Table>,
    i_cur: i32,
    i_idx_cur: i32,
) {
    debug_assert!(i_idx_cur > 0);
    debug_assert!(p_idx.ai_column[p_idx.n_column as usize - 1] == -1);

    wi.b_deferred_seek = true;
    add_op3(vdbe_of_parse(parse), OP_DEFERREDSEEK as i32, i_idx_cur, 0, i_cur);
    if (wi.wctrl_flags as u32 & (WHERE_OR_SUBCLAUSE | WHERE_RIGHT_JOIN)) != 0
        && parse.toplevel().write_mask == 0
    {
        let n_col = p_tab.n_col as usize;
        let mut ai = vec![0u32; n_col + 1];
        ai[0] = n_col as u32;
        for i in 0..(p_idx.n_column as usize - 1) {
            debug_assert!((p_idx.ai_column[i] as i32) < p_tab.n_col as i32);
            let x1 = p_idx.ai_column[i];
            let x2 = table_column_to_storage(p_tab, x1);
            if x1 >= 0 {
                ai[x2 as usize + 1] = i as u32 + 1;
            }
        }
        change_p4(vdbe_of_parse(parse), -1, P4::IntArray(ai));
    }
}

/// `codeExprOrVector`: se a expressão `p` é um vetor, gera o código que escreve os primeiros
/// `n_reg` elementos dele num vetor de registradores a partir de `i_reg`. Se não é vetor, `n_reg`
/// deve ser 1, e o código avalia a expressão e deixa o resultado em `i_reg`.
fn code_expr_or_vector(
    db: &mut Connection,
    parse: &mut Parse,
    p: Option<&mut Expr>,
    i_reg: i32,
    n_reg: i32,
) {
    debug_assert!(n_reg > 0);
    match p {
        Some(p) if expr_is_vector(p) => {
            if p.use_x_select() {
                debug_assert!(p.op == TK_SELECT);
                let i_select = code_subselect(db, parse, p);
                add_op3(vdbe_of_parse(parse), OP_COPY as i32, i_select, i_reg, n_reg - 1);
            } else {
                debug_assert!(p.x_list().map_or(false, |l| n_reg as usize <= l.a.len()));
                for i in 0..n_reg as usize {
                    if let Some(e) = p
                        .x_list_mut()
                        .and_then(|l| l.a.get_mut(i))
                        .and_then(|it| it.p_expr.as_deref_mut())
                    {
                        expr_code(db, parse, e, i_reg + i as i32, None);
                    }
                }
            }
        }
        Some(p) => {
            debug_assert!(n_reg == 1 || parse.n_err != 0);
            expr_code(db, parse, p, i_reg, None);
        }
        None => {
            // `sqlite3ExprCode(pParse, 0, iReg)` codifica o `TK_NULL`.
            add_op2(vdbe_of_parse(parse), OP_NULL as i32, 0, i_reg);
        }
    }
}

/// `whereApplyPartialIndexConstraints`: a expressão `p_truth` é sempre verdadeira porque é o
/// WHERE de um índice parcial que dirige um laço da consulta. Percorre todos os termos do WHERE
/// da consulta e, se algum deles precisa ser verdadeiro porque `p_truth` é verdadeira, o marca
/// como codificado.
fn where_apply_partial_index_constraints(
    p_truth: &Expr,
    i_tab_cur: i32,
    wi: &mut WhereInfo,
    wc: ClauseId,
) {
    let mut p_truth = p_truth;
    while p_truth.op == TK_AND {
        if let Some(l) = p_truth.p_left.as_deref() {
            where_apply_partial_index_constraints(l, i_tab_cur, wi, wc);
        }
        match p_truth.p_right.as_deref() {
            Some(r) => p_truth = r,
            None => return,
        }
    }
    for i in 0..wi.n_term(wc) {
        let t = wi.term_at(wc, i);
        if (wi.term(t).wt_flags & TERM_CODED) != 0 {
            continue;
        }
        let same = expr_compare(None, wi.exprs.get(&wi.term(t).p_expr), Some(p_truth), i_tab_cur)
            == 0;
        if same {
            wi.term_mut(t).wt_flags |= TERM_CODED;
        }
    }
}

/// `filterPullDown`: chamada logo depois de gerado um `OP_Filter` e antes da busca correspondente
/// no índice. Verifica se há outros filtros de Bloom em laços internos que podem ser testados
/// antes da busca. Havendo, avalia esses filtros agora, antes da busca no índice. A ideia é que
/// testar um filtro de Bloom é bem mais rápido que uma busca no índice, e o filtro pode responder
/// "falso", o que dispensa a busca.
///
/// Um laço interno usa um filtro de Bloom quando tem `WhereLevel.regFilter` definido. Se o filtro
/// de um laço interno é testado aqui, `regFilter` é zerado para impedir que o filtro seja testado
/// de novo quando o laço interno for codificado.
fn filter_pull_down(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab_list: &SrcList,
    wi: &mut WhereInfo,
    i_level: usize,
    addr_nxt: i32,
    not_ready: Bitmask,
) {
    let mut i_level = i_level;
    loop {
        i_level += 1;
        if i_level >= wi.n_level as usize {
            break;
        }
        let lp = wi.a[i_level].p_w_loop;
        if wi.a[i_level].reg_filter == 0 {
            continue;
        }
        if wi.w_loop(lp).n_skip != 0 {
            continue;
        }
        // Porque `sqlite3ConstructBloomFilter()` não teria posto `regFilter` se isto fosse
        // verdade.
        if (wi.w_loop(lp).prereq & not_ready) != 0 {
            continue;
        }
        debug_assert!(wi.a[i_level].addr_brk == 0);
        wi.a[i_level].addr_brk = addr_nxt;
        if (wi.w_loop(lp).ws_flags & WHERE_IPK) != 0 {
            let p_term = l_term(wi, lp, 0).expect("pLoop->aLTerm[0]");
            let reg_rowid = get_temp_reg(parse);
            let reg_rowid = code_equality_term(db, parse, wi, p_term, i_level, 0, false, reg_rowid);
            add_op2(vdbe_of_parse(parse), OP_MUSTBEINT as i32, reg_rowid, addr_nxt);
            let reg_filter = wi.a[i_level].reg_filter;
            add_op4_int(vdbe_of_parse(parse), OP_FILTER as i32, reg_filter, addr_nxt, reg_rowid, 1);
        } else {
            let n_eq = wi.w_loop(lp).btree.n_eq as i32;
            debug_assert!((wi.w_loop(lp).ws_flags & WHERE_INDEXED) != 0);
            debug_assert!((wi.w_loop(lp).ws_flags & WHERE_COLUMN_IN) == 0);
            let p_tab = level_table(p_tab_list, wi, i_level);
            let (r1, z_start_aff) =
                code_all_equality_terms(db, parse, wi, &p_tab, i_level, false, 0);
            code_apply_affinity(parse, r1, n_eq, &z_start_aff);
            let reg_filter = wi.a[i_level].reg_filter;
            add_op4_int(vdbe_of_parse(parse), OP_FILTER as i32, reg_filter, addr_nxt, r1, n_eq);
        }
        wi.a[i_level].reg_filter = 0;
        wi.a[i_level].addr_brk = 0;
    }
}

/// `whereLoopIsOneRow`: o laço `lp` é um nível `WHERE_INDEXED` que usa pelo menos um operador
/// `IN(...)`. Devolve verdadeiro se o nível visita no máximo uma linha para cada chave gerada
/// para o índice.
fn where_loop_is_one_row(wi: &WhereInfo, lp: LoopId) -> bool {
    let p_loop = wi.w_loop(lp);
    let Some(p_idx) = p_loop.btree.p_index.as_deref() else {
        return false;
    };
    if p_idx.on_error != 0 && p_loop.n_skip == 0 && p_loop.btree.n_eq == p_idx.n_key_col {
        for ii in 0..p_loop.btree.n_eq as usize {
            if let Some(t) = l_term(wi, lp, ii) {
                if (wi.term(t).e_operator & (WO_IS | WO_ISNULL)) != 0 {
                    return false;
                }
            }
        }
        return true;
    }
    false
}

// ---------------------------------------------------------------------------------------------
// sqlite3WhereCodeOneLoopStart (chunks 003 a 006)
// ---------------------------------------------------------------------------------------------

/// `aStartOp` do caso 4: indexado por `(start_constraints<<2) + (startEq<<1) + bRev`.
const A_START_OP: [u8; 8] =
    [0, 0, OP_REWIND, OP_LAST, OP_SEEKGT, OP_SEEKLT, OP_SEEKGE, OP_SEEKLE];

/// `aEndOp` do caso 4: indexado por `bRev*2 + endEq`.
const A_END_OP: [u8; 4] = [OP_IDXGE, OP_IDXGT, OP_IDXLE, OP_IDXLT];

/// `sqlite3WhereCodeOneLoopStart`: gera o código do início do laço de número `i_level` da
/// implementação do WHERE descrita por `wi`. `not_ready` são as tabelas disponíveis no momento.
/// `p_select` é o `pWInfo->pSelect` (só lido para o `iOffset` do OFFSET de tabelas virtuais).
/// Devolve o `notReady` do nível.
pub fn where_code_one_loop_start(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab_list: &mut SrcList,
    p_select: Option<&Select>,
    wi: &mut WhereInfo,
    i_level: usize,
    not_ready: Bitmask,
) -> Bitmask {
    let s_wc: ClauseId = wi.s_wc;
    let lp = wi.a[i_level].p_w_loop;
    let i_from = wi.a[i_level].i_from as usize;
    let (i_cur, jointype, via_coroutine, reg_return, addr_fill_sub, is_recursive, p_tab) = {
        let it = &p_tab_list.a[i_from];
        (
            it.i_cursor,
            it.fg.jointype,
            it.fg.via_coroutine,
            it.reg_return,
            it.addr_fill_sub,
            it.fg.is_recursive,
            it.p_tab.clone().expect("pTabItem->pTab"),
        )
    };
    let mut p_idx: Option<Rc<Index>> = None;

    wi.a[i_level].not_ready = not_ready & !wi.s_mask_set.get_mask(i_cur);
    let b_rev = ((wi.rev_mask >> i_level) & 1) != 0;

    // Cria os rótulos "break" e "continue" do laço corrente. Salta-se para addrBrk para sair do
    // laço e para addrCont para ir direto à próxima iteração. Com um operador IN também há o
    // rótulo addrNxt, que continua com a próxima combinação de valores do IN. Sem IN, addrNxt é
    // o mesmo que addrBrk.
    let addr_brk = make_label(parse);
    wi.a[i_level].addr_brk = addr_brk;
    wi.a[i_level].addr_nxt = addr_brk;
    let addr_cont = make_label(parse);
    wi.a[i_level].addr_cont = addr_cont;

    // Se esta é a tabela direita de um LEFT OUTER JOIN, aloca e inicia uma célula de memória que
    // registra se a tabela casa com alguma linha da tabela esquerda do join.
    if wi.a[i_level].i_from > 0 && (jointype & JT_LEFT) != 0 {
        parse.n_mem += 1;
        let i_lj = parse.n_mem;
        wi.a[i_level].i_left_join = i_lj;
        add_op2(vdbe_of_parse(parse), OP_INTEGER as i32, 0, i_lj);
        vdbe_comment(vdbe_of_parse(parse), b"init LEFT JOIN match flag", &[]);
    }

    // Calcula um endereço seguro para saltar se a tabela deste laço está vazia e nunca poderá
    // contribuir com conteúdo.
    let mut jj = i_level;
    while jj > 0 {
        if wi.a[jj].i_left_join != 0 {
            break;
        }
        if wi.a[jj].p_rj.is_some() {
            break;
        }
        jj -= 1;
    }
    let addr_halt = wi.a[jj].addr_brk;

    let ws0 = wi.w_loop(lp).ws_flags;
    if via_coroutine {
        // Caso especial: subconsulta do FROM implementada como co-rotina.
        let reg_yield = reg_return;
        add_op3(vdbe_of_parse(parse), OP_INITCOROUTINE as i32, reg_yield, 0, addr_fill_sub);
        let a = add_op2(vdbe_of_parse(parse), OP_YIELD as i32, reg_yield, addr_brk);
        wi.a[i_level].p2 = a;
        vdbe_comment(vdbe_of_parse(parse), b"next row of %s", &[text_arg(&p_tab.z_name)]);
        wi.a[i_level].op = OP_GOTO;
    } else if (ws0 & WHERE_VIRTUALTABLE) != 0 {
        // Caso 1: a tabela é virtual. Usa VFilter e VNext para acessar os dados.
        let n_constraint = wi.w_loop(lp).n_l_term as usize;
        let i_reg = get_temp_range(parse, n_constraint as i32 + 2);
        let mut addr_not_found = wi.a[i_level].addr_brk;
        for j in 0..n_constraint {
            let i_target = i_reg + j as i32 + 2;
            let Some(p_term) = l_term(wi, lp, j) else {
                continue;
            };
            if (wi.term(p_term).e_operator & WO_IN) != 0 {
                if (smaskbit32(j) & wi.w_loop(lp).vtab.m_handle_in) != 0 {
                    let i_tab = parse.n_tab;
                    parse.n_tab += 1;
                    parse.n_mem += 1;
                    let i_cache = parse.n_mem;
                    code_rhs_of_in(db, parse, wi.expr_mut(p_term), i_tab, None);
                    add_op3(vdbe_of_parse(parse), OP_VINITIN as i32, i_tab, i_target, i_cache);
                } else {
                    code_equality_term(db, parse, wi, p_term, i_level, j, b_rev, i_target);
                    addr_not_found = wi.a[i_level].addr_nxt;
                }
            } else {
                code_expr_or_vector(
                    db,
                    parse,
                    wi.expr_mut(p_term).p_right.as_deref_mut(),
                    i_target,
                    1,
                );
                if wi.term(p_term).e_match_op as i32 == SQLITE_INDEX_CONSTRAINT_OFFSET
                    && wi.w_loop(lp).vtab.b_omit_offset
                {
                    debug_assert!(wi.term(p_term).e_operator == WO_AUX);
                    debug_assert!(p_select.map_or(false, |s| s.i_offset > 0));
                    let i_offset = p_select.map_or(0, |s| s.i_offset);
                    add_op2(vdbe_of_parse(parse), OP_INTEGER as i32, 0, i_offset);
                    vdbe_comment(vdbe_of_parse(parse), b"Zero OFFSET counter", &[]);
                }
            }
        }
        let idx_num = wi.w_loop(lp).vtab.idx_num;
        add_op2(vdbe_of_parse(parse), OP_INTEGER as i32, idx_num, i_reg);
        add_op2(vdbe_of_parse(parse), OP_INTEGER as i32, n_constraint as i32, i_reg + 1);
        let p4 = match wi.w_loop(lp).vtab.idx_str.clone() {
            Some(s) => P4::Text(s),
            None => P4::None,
        };
        add_op4(vdbe_of_parse(parse), OP_VFILTER as i32, i_cur, addr_not_found, i_reg, p4);
        wi.w_loop_mut(lp).vtab.need_free = false;
        wi.a[i_level].p1 = i_cur;
        wi.a[i_level].op = if wi.e_one_pass != 0 { OP_NOOP } else { OP_VNEXT };
        wi.a[i_level].p2 = current_addr(parse);
        debug_assert!((wi.w_loop(lp).ws_flags & WHERE_MULTI_OR) == 0);

        for j in 0..n_constraint {
            let Some(p_term) = l_term(wi, lp, j) else {
                continue;
            };
            if j < 16 && ((wi.w_loop(lp).vtab.omit_mask >> j) & 1) != 0 {
                disable_term(wi, i_level, p_term);
                continue;
            }
            if (wi.term(p_term).e_operator & WO_IN) != 0
                && (smaskbit32(j) & wi.w_loop(lp).vtab.m_handle_in) == 0
            {
                let target_reg = i_reg + j as i32 + 2;
                // Recarrega o valor da restrição em reg[iReg+j+2]. O mesmo valor foi posto no
                // mesmo registrador antes do OP_VFilter, mas o xFilter pode ter mudado o tipo ou
                // a codificação do valor, então ele PRECISA ser recarregado.
                let n_in = wi.a[i_level].a_in_loop.len();
                for i_in in 0..n_in {
                    let addr = wi.a[i_level].a_in_loop[i_in].addr_in_top;
                    let op = get_op_ref(vdbe_of_parse(parse), addr).map(|o| (o.opcode, o.p1, o.p2, o.p3));
                    if let Some((opc, p1, p2, p3)) = op {
                        if (opc == OP_COLUMN && p3 == target_reg)
                            || (opc == OP_ROWID && p2 == target_reg)
                        {
                            add_op3(vdbe_of_parse(parse), opc as i32, p1, p2, p3);
                            break;
                        }
                    }
                }

                // Gera o código que continua para a próxima linha se a restrição IN não é
                // satisfeita.
                let mut p_compare = new_p_expr(db, parse, TK_EQ as i32, None, None);
                if let Some(cmp) = p_compare.as_deref_mut() {
                    let i_fld = wi.term(p_term).i_field;
                    let borrowed: Option<Box<Expr>> = {
                        let e = wi.expr_mut(p_term);
                        if i_fld > 0 {
                            debug_assert!(e.p_left.as_deref().map_or(false, |l| l.op == TK_VECTOR));
                            e.p_left
                                .as_deref_mut()
                                .and_then(|l| l.x_list_mut())
                                .and_then(|l| l.a.get_mut(i_fld as usize - 1))
                                .and_then(|it| it.p_expr.take())
                        } else {
                            e.p_left.take()
                        }
                    };
                    cmp.p_left = borrowed;
                    if let Some(mut p_right) = new_expr(TK_REGISTER as i32, None) {
                        p_right.i_table = target_reg;
                        cmp.p_right = Some(p_right);
                        let a_cont = wi.a[i_level].addr_cont;
                        expr_if_false(db, parse, cmp, a_cont, SQLITE_JUMPIFNULL as i32, None);
                    }
                    // `pCompare->pLeft = 0`: devolve o `Box` ao lugar de onde saiu.
                    let back = cmp.p_left.take();
                    let e = wi.expr_mut(p_term);
                    if i_fld > 0 {
                        if let Some(it) = e
                            .p_left
                            .as_deref_mut()
                            .and_then(|l| l.x_list_mut())
                            .and_then(|l| l.a.get_mut(i_fld as usize - 1))
                        {
                            it.p_expr = back;
                        }
                    } else {
                        e.p_left = back;
                    }
                }
            }
        }
        // Estes registradores precisam ser preservados caso haja um laço de operador IN. Dava
        // para liberá-los aqui se `WHERE_IN_ABLE` fosse zero, mas é mais simples e seguro não
        // reutilizá-los.
    } else if (ws0 & WHERE_IPK) != 0 && (ws0 & (WHERE_COLUMN_IN | WHERE_COLUMN_EQ)) != 0 {
        // Caso 2: referencia-se uma única linha direto por uma igualdade com o campo ROWID, ou
        // várias linhas por um "rowid IN (...)".
        debug_assert!(wi.w_loop(lp).btree.n_eq == 1);
        let p_term = l_term(wi, lp, 0).expect("pLoop->aLTerm[0]");
        parse.n_mem += 1;
        let i_release_reg = parse.n_mem;
        let i_rowid_reg =
            code_equality_term(db, parse, wi, p_term, i_level, 0, b_rev, i_release_reg);
        if i_rowid_reg != i_release_reg {
            release_temp_reg(parse, i_release_reg);
        }
        let addr_nxt = wi.a[i_level].addr_nxt;
        if wi.a[i_level].reg_filter != 0 {
            let reg_filter = wi.a[i_level].reg_filter;
            add_op2(vdbe_of_parse(parse), OP_MUSTBEINT as i32, i_rowid_reg, addr_nxt);
            add_op4_int(vdbe_of_parse(parse), OP_FILTER as i32, reg_filter, addr_nxt, i_rowid_reg, 1);
            filter_pull_down(db, parse, &*p_tab_list, wi, i_level, addr_nxt, not_ready);
        }
        add_op3(vdbe_of_parse(parse), OP_SEEKROWID as i32, i_cur, addr_nxt, i_rowid_reg);
        wi.a[i_level].op = OP_NOOP;
    } else if (ws0 & WHERE_IPK) != 0 && (ws0 & WHERE_COLUMN_RANGE) != 0 {
        // Caso 3: há uma desigualdade com o campo ROWID.
        let mut test_op: u8 = OP_NOOP;
        let mut mem_end_value = 0;
        let mut j = 0usize;
        let mut p_start: Option<TermId> = None;
        let mut p_end: Option<TermId> = None;
        if (ws0 & WHERE_BTM_LIMIT) != 0 {
            p_start = l_term(wi, lp, j);
            j += 1;
        }
        if (ws0 & WHERE_TOP_LIMIT) != 0 {
            p_end = l_term(wi, lp, j);
        }
        debug_assert!(p_start.is_some() || p_end.is_some());
        if b_rev {
            std::mem::swap(&mut p_start, &mut p_end);
        }
        if let Some(p_start) = p_start {
            // Mapeia os códigos TK_xx para os opcodes de seek. Depende da ordem dos TK_xx.
            const A_MOVE_OP: [u8; 4] = [OP_SEEKGT, OP_SEEKLE, OP_SEEKLT, OP_SEEKGE];
            debug_assert!(wi.term(p_start).wt_flags & TERM_VNULL == 0);
            let x_op = wi.expr(p_start).op;
            let r1: i32;
            let mut r_temp = 0;
            let op: u8;
            if right_is_vector(wi, p_start) {
                r_temp = get_temp_reg(parse);
                r1 = r_temp;
                code_expr_or_vector(
                    db,
                    parse,
                    wi.expr_mut(p_start).p_right.as_deref_mut(),
                    r1,
                    1,
                );
                op = A_MOVE_OP[(((x_op as i32 - TK_GT as i32 - 1) & 0x3) | 0x1) as usize];
            } else {
                let e = wi.expr_mut(p_start).p_right.as_deref_mut().expect("pX->pRight");
                r1 = expr_code_temp(db, parse, e, &mut r_temp, None);
                disable_term(wi, i_level, p_start);
                op = A_MOVE_OP[(x_op - TK_GT) as usize];
            }
            add_op3(vdbe_of_parse(parse), op as i32, i_cur, addr_brk, r1);
            vdbe_comment(vdbe_of_parse(parse), b"pk", &[]);
            release_temp_reg(parse, r_temp);
        } else {
            add_op2(vdbe_of_parse(parse), (if b_rev { OP_LAST } else { OP_REWIND }) as i32, i_cur, addr_halt);
        }
        if let Some(p_end) = p_end {
            debug_assert!(wi.term(p_end).wt_flags & TERM_VNULL == 0);
            let x_op = wi.expr(p_end).op;
            parse.n_mem += 1;
            mem_end_value = parse.n_mem;
            code_expr_or_vector(
                db,
                parse,
                wi.expr_mut(p_end).p_right.as_deref_mut(),
                mem_end_value,
                1,
            );
            if !right_is_vector(wi, p_end) && (x_op == TK_LT || x_op == TK_GT) {
                test_op = if b_rev { OP_LE } else { OP_GE };
            } else {
                test_op = if b_rev { OP_LT } else { OP_GT };
            }
            if !right_is_vector(wi, p_end) {
                disable_term(wi, i_level, p_end);
            }
        }
        let start = current_addr(parse);
        wi.a[i_level].op = if b_rev { OP_PREV } else { OP_NEXT };
        wi.a[i_level].p1 = i_cur;
        wi.a[i_level].p2 = start;
        debug_assert!(wi.a[i_level].p5 == 0);
        if test_op != OP_NOOP {
            parse.n_mem += 1;
            let i_rowid_reg = parse.n_mem;
            add_op2(vdbe_of_parse(parse), OP_ROWID as i32, i_cur, i_rowid_reg);
            add_op3(vdbe_of_parse(parse), test_op as i32, mem_end_value, addr_brk, i_rowid_reg);
            change_p5(vdbe_of_parse(parse), (SQLITE_AFF_NUMERIC | SQLITE_JUMPIFNULL) as u16);
        }
    } else if (ws0 & WHERE_INDEXED) != 0 {
        // Caso 4: varredura usando um índice.
        //
        // O WHERE pode ter zero ou mais termos de igualdade ("==" ou "IN") sobre as N colunas
        // mais à esquerda do índice. Pode ter também desigualdades (>, <, >= ou <=) sobre a
        // coluna do índice que vem logo depois das N igualdades. Só a coluna mais à direita pode
        // ter desigualdade, as outras usam "==" e "IN". Por exemplo, com o índice em (x,y,z),
        // todos estes são otimizados: x=5; x=5 AND y=10; x=5 AND y<10; x=5 AND y>5 AND y<10;
        // x=5 AND y=5 AND z<=10. O z<10 de "x=5 AND z<10" não pode ser usado, só o x=5.
        //
        // N pode ser zero se há desigualdades. Se não há desigualdades, N é pelo menos um.
        //
        // Este caso também vale quando não há restrições no WHERE mas um índice é escolhido para
        // forçar a ordem de saída a obedecer ao ORDER BY.
        let n_eq = wi.w_loop(lp).btree.n_eq as i32;
        let mut n_btm = wi.w_loop(lp).btree.n_btm as i32;
        let mut n_top = wi.w_loop(lp).btree.n_top as i32;
        let n_skip = wi.w_loop(lp).n_skip as i32;
        let idx: Rc<Index> = wi.w_loop(lp).btree.p_index.clone().expect("pLoop->u.btree.pIndex");
        let i_idx_cur = wi.a[i_level].i_idx_cur;
        let mut p_range_start: Option<TermId> = None;
        let mut p_range_end: Option<TermId> = None;
        let mut n_extra_reg = 0;
        let mut b_seek_past_null = false;
        let mut b_stop_at_null = false;
        let mut reg_bignull = 0;
        let mut addr_seek_scan = 0;
        let mut i_rowid_reg;
        debug_assert!(n_eq >= n_skip);
        p_idx = Some(idx.clone());

        // Acha as desigualdades do começo e do fim do intervalo.
        let mut j = n_eq as usize;
        if (ws0 & WHERE_BTM_LIMIT) != 0 {
            p_range_start = l_term(wi, lp, j);
            j += 1;
            n_extra_reg = n_extra_reg.max(n_btm);
        }
        if (ws0 & WHERE_TOP_LIMIT) != 0 {
            p_range_end = l_term(wi, lp, j);
            n_extra_reg = n_extra_reg.max(n_top);
            if let Some(re) = p_range_end {
                if (wi.term(re).wt_flags & TERM_LIKEOPT) != 0 {
                    // As restrições de intervalo da otimização do LIKE sempre vêm em pares.
                    debug_assert!(p_range_start.is_some());
                    parse.n_mem += 1;
                    let cntr = parse.n_mem as u32;
                    add_op2(vdbe_of_parse(parse), OP_INTEGER as i32, 1, cntr as i32);
                    vdbe_comment(vdbe_of_parse(parse), b"LIKE loop counter", &[]);
                    wi.a[i_level].addr_like_rep = current_addr(parse);
                    // `iLikeRepCntr` guarda o DOBRO do número do registrador do contador. O bit
                    // de baixo indica se a ordem da busca é ASC ou DESC.
                    let so_desc = (sort_order(&idx, n_eq as usize) == SQLITE_SO_DESC) as u32;
                    wi.a[i_level].i_like_rep_cntr = (cntr << 1) | ((b_rev as u32) ^ so_desc);
                }
            }
            if p_range_start.is_none() {
                let c = idx.ai_column[n_eq as usize];
                if (c >= 0 && p_tab.a_col[c as usize].not_null == 0) || c == XN_EXPR {
                    b_seek_past_null = true;
                }
            }
        }

        // Se `WHERE_BIGNULL_SORT` está ligada, a coluna nEq do índice usa uma ordenação "big-null"
        // fora do padrão (ASC NULLS LAST ou DESC NULLS FIRST). Nos dois casos fazem-se varreduras
        // ordenadas separadas das entradas do índice em que a coluna é nula e das em que não é.
        // Em ASC as não nulas vêm primeiro. Em DESC, as nulas.
        if (ws0 & (WHERE_TOP_LIMIT | WHERE_BTM_LIMIT)) == 0 && (ws0 & WHERE_BIGNULL_SORT) != 0 {
            n_extra_reg = 1;
            b_seek_past_null = true;
            parse.n_mem += 1;
            reg_bignull = parse.n_mem;
            wi.a[i_level].reg_bignull = reg_bignull;
            if wi.a[i_level].i_left_join != 0 {
                add_op2(vdbe_of_parse(parse), OP_INTEGER as i32, 0, reg_bignull);
            }
            wi.a[i_level].addr_bignull = make_label(parse);
        }

        // Numa varredura reversa de índice ascendente, ou direta de índice descendente, troca o
        // começo e o fim (pRangeStart e pRangeEnd).
        if (n_eq as usize) < idx.n_column as usize
            && b_rev == (sort_order(&idx, n_eq as usize) == SQLITE_SO_ASC)
        {
            std::mem::swap(&mut p_range_end, &mut p_range_start);
            std::mem::swap(&mut b_seek_past_null, &mut b_stop_at_null);
            std::mem::swap(&mut n_btm, &mut n_top);
        }

        if i_level > 0 && (wi.w_loop(lp).ws_flags & WHERE_IN_SEEKSCAN) != 0 {
            // Caso `OP_SeekScan` seja usado, garante que o cursor do índice não aponte para uma
            // linha válida na primeira iteração deste laço.
            add_op1(vdbe_of_parse(parse), OP_NULLROW as i32, i_idx_cur);
        }

        // Gera o código que avalia todos os termos com == ou IN e guarda os valores num vetor de
        // registradores a partir de regBase.
        let (reg_base, mut z_start_aff) =
            code_all_equality_terms(db, parse, wi, &p_tab, i_level, b_rev, n_extra_reg);
        debug_assert!(z_start_aff.len() >= n_eq as usize);
        let mut z_end_aff: Option<Vec<u8>> = None;
        if n_top > 0 {
            z_end_aff = Some(z_start_aff[n_eq as usize..].to_vec());
        }
        let addr_nxt =
            if reg_bignull != 0 { wi.a[i_level].addr_bignull } else { wi.a[i_level].addr_nxt };

        let mut start_eq = p_range_start
            .map_or(true, |t| (wi.term(t).e_operator & (WO_LE | WO_GE)) != 0);
        let mut end_eq =
            p_range_end.map_or(true, |t| (wi.term(t).e_operator & (WO_LE | WO_GE)) != 0);
        let mut start_constraints = p_range_start.is_some() || n_eq > 0;

        // Posiciona o cursor do índice no começo do intervalo.
        let mut n_constraint = n_eq;
        if let Some(rs) = p_range_start {
            code_expr_or_vector(
                db,
                parse,
                wi.expr_mut(rs).p_right.as_deref_mut(),
                reg_base + n_eq,
                n_btm,
            );
            where_like_optimization_string_fixup(
                vdbe_of_parse(parse),
                wi.a[i_level].i_like_rep_cntr,
                wi.term(rs).wt_flags,
            );
            if (wi.term(rs).wt_flags & TERM_VNULL) == 0
                && expr_can_be_null(wi.expr(rs).p_right.as_deref())
            {
                add_op2(vdbe_of_parse(parse), OP_ISNULL as i32, reg_base + n_eq, addr_nxt);
            }
            if let Some(r) = wi.expr(rs).p_right.as_deref() {
                update_range_affinity_str(r, n_btm, &mut z_start_aff[n_eq as usize..]);
            }
            n_constraint += n_btm;
            if !right_is_vector(wi, rs) {
                disable_term(wi, i_level, rs);
            } else {
                start_eq = true;
            }
            b_seek_past_null = false;
        } else if b_seek_past_null {
            start_eq = false;
            add_op2(vdbe_of_parse(parse), OP_NULL as i32, 0, reg_base + n_eq);
            start_constraints = true;
            n_constraint += 1;
        } else if reg_bignull != 0 {
            add_op2(vdbe_of_parse(parse), OP_NULL as i32, 0, reg_base + n_eq);
            start_constraints = true;
            n_constraint += 1;
        }
        code_apply_affinity(parse, reg_base, n_constraint - b_seek_past_null as i32, &z_start_aff);
        if n_skip > 0 && n_constraint == n_skip {
            // A lógica de skip-scan dentro de `codeAllEqualityTerms()` já deixou o cursor na
            // linha certa, então não é preciso fazer seek.
        } else {
            if reg_bignull != 0 {
                add_op2(vdbe_of_parse(parse), OP_INTEGER as i32, 1, reg_bignull);
                vdbe_comment(vdbe_of_parse(parse), b"NULL-scan pass ctr", &[]);
            }
            if wi.a[i_level].reg_filter != 0 {
                let reg_filter = wi.a[i_level].reg_filter;
                add_op4_int(vdbe_of_parse(parse), OP_FILTER as i32, reg_filter, addr_nxt, reg_base, n_eq);
                filter_pull_down(db, parse, &*p_tab_list, wi, i_level, addr_nxt, not_ready);
            }

            let op = A_START_OP
                [((start_constraints as usize) << 2) + ((start_eq as usize) << 1) + b_rev as usize];
            debug_assert!(op != 0);
            if (wi.w_loop(lp).ws_flags & WHERE_IN_SEEKSCAN) != 0 && op == OP_SEEKGE {
                debug_assert!(reg_bignull == 0);
                // AJUSTE: o `OP_SeekScan` busca reduzir o número de seeks caros trocando um único
                // seek por 1 ou mais passos. A pergunta é quantos passos tentar antes de desistir
                // e fazer o seek. O custo de um seek é proporcional ao logaritmo do número de
                // entradas da árvore, então basear o número de passos na estimativa de linhas da
                // árvore-b parece um bom palpite.
                addr_seek_scan = add_op1(
                    vdbe_of_parse(parse),
                    OP_SEEKSCAN as i32,
                    (idx.ai_row_log_est[0] as i32 + 9) / 10,
                );
                if p_range_start.is_some() || p_range_end.is_some() {
                    change_p5(vdbe_of_parse(parse), 1);
                    let a = current_addr(parse) + 1;
                    change_p2(vdbe_of_parse(parse), addr_seek_scan, a);
                    addr_seek_scan = 0;
                }
            }
            add_op4_int(vdbe_of_parse(parse), op as i32, i_idx_cur, addr_nxt, reg_base, n_constraint);

            debug_assert!(!b_seek_past_null || !b_stop_at_null);
            if reg_bignull != 0 {
                debug_assert!(b_seek_past_null || b_stop_at_null);
                debug_assert!(b_seek_past_null == !b_stop_at_null);
                debug_assert!(b_stop_at_null == start_eq);
                let a = current_addr(parse) + 2;
                add_op2(vdbe_of_parse(parse), OP_GOTO as i32, 0, a);
                let op2 = A_START_OP[((n_constraint > 1) as usize) * 4 + 2 + b_rev as usize];
                add_op4_int(
                    vdbe_of_parse(parse),
                    op2 as i32,
                    i_idx_cur,
                    addr_nxt,
                    reg_base,
                    n_constraint - start_eq as i32,
                );
                debug_assert!(
                    op2 == OP_REWIND || op2 == OP_LAST || op2 == OP_SEEKGE || op2 == OP_SEEKLE
                );
            }
        }

        // Carrega o valor da desigualdade do fim do intervalo (se houver).
        n_constraint = n_eq;
        debug_assert!(wi.a[i_level].p2 == 0);
        if let Some(re) = p_range_end {
            debug_assert!(addr_seek_scan == 0);
            code_expr_or_vector(
                db,
                parse,
                wi.expr_mut(re).p_right.as_deref_mut(),
                reg_base + n_eq,
                n_top,
            );
            where_like_optimization_string_fixup(
                vdbe_of_parse(parse),
                wi.a[i_level].i_like_rep_cntr,
                wi.term(re).wt_flags,
            );
            if (wi.term(re).wt_flags & TERM_VNULL) == 0
                && expr_can_be_null(wi.expr(re).p_right.as_deref())
            {
                add_op2(vdbe_of_parse(parse), OP_ISNULL as i32, reg_base + n_eq, addr_nxt);
            }
            if let Some(z_end) = z_end_aff.as_mut() {
                if let Some(r) = wi.expr(re).p_right.as_deref() {
                    update_range_affinity_str(r, n_top, z_end);
                }
                code_apply_affinity(parse, reg_base + n_eq, n_top, z_end);
            }
            n_constraint += n_top;
            if !right_is_vector(wi, re) {
                disable_term(wi, i_level, re);
            } else {
                end_eq = true;
            }
        } else if b_stop_at_null {
            if reg_bignull == 0 {
                add_op2(vdbe_of_parse(parse), OP_NULL as i32, 0, reg_base + n_eq);
                end_eq = false;
            }
            n_constraint += 1;
        }

        // Topo do corpo do laço.
        wi.a[i_level].p2 = current_addr(parse);

        // Verifica se o cursor do índice passou do fim do intervalo.
        if n_constraint != 0 {
            if reg_bignull != 0 {
                // Pula a verificação do fim do intervalo durante a varredura de NULLs.
                let a = current_addr(parse) + 3;
                add_op2(vdbe_of_parse(parse), OP_IFNOT as i32, reg_bignull, a);
                vdbe_comment(vdbe_of_parse(parse), b"If NULL-scan 2nd pass", &[]);
            }
            let op = A_END_OP[(b_rev as usize) * 2 + end_eq as usize];
            add_op4_int(vdbe_of_parse(parse), op as i32, i_idx_cur, addr_nxt, reg_base, n_constraint);
            if addr_seek_scan != 0 {
                jump_here(vdbe_of_parse(parse), addr_seek_scan);
            }
        }
        if reg_bignull != 0 {
            // Durante a varredura de NULLs, verifica se chegou ao fim dos NULLs.
            debug_assert!(b_seek_past_null == !b_stop_at_null);
            debug_assert!(n_constraint + b_seek_past_null as i32 > 0);
            let a = current_addr(parse) + 2;
            add_op2(vdbe_of_parse(parse), OP_IF as i32, reg_bignull, a);
            vdbe_comment(vdbe_of_parse(parse), b"If NULL-scan 1st pass", &[]);
            let op = A_END_OP[(b_rev as usize) * 2 + b_seek_past_null as usize];
            add_op4_int(
                vdbe_of_parse(parse),
                op as i32,
                i_idx_cur,
                addr_nxt,
                reg_base,
                n_constraint + b_seek_past_null as i32,
            );
        }

        if (wi.w_loop(lp).ws_flags & WHERE_IN_EARLYOUT) != 0 {
            add_op3(vdbe_of_parse(parse), OP_SEEKHIT as i32, i_idx_cur, n_eq, n_eq);
        }

        // Faz o seek do cursor da tabela, se preciso.
        let omit_table = (wi.w_loop(lp).ws_flags & WHERE_IDX_ONLY) != 0
            && (wi.wctrl_flags as u32 & (WHERE_OR_SUBCLAUSE | WHERE_RIGHT_JOIN)) == 0;
        if omit_table {
            // `p_idx` é um índice de cobertura. Não é preciso acessar a tabela principal.
        } else if p_tab.has_rowid() {
            code_deferred_seek(parse, wi, &idx, &p_tab, i_cur, i_idx_cur);
        } else if i_cur != i_idx_cur {
            let p_pk = primary_key_index(&p_tab).cloned().expect("sqlite3PrimaryKeyIndex");
            i_rowid_reg = get_temp_range(parse, p_pk.n_key_col as i32);
            for jk in 0..p_pk.n_key_col as usize {
                let k = table_column_to_index(&idx, p_pk.ai_column[jk]);
                add_op3(vdbe_of_parse(parse), OP_COLUMN as i32, i_idx_cur, k as i32, i_rowid_reg + jk as i32);
            }
            add_op4_int(
                vdbe_of_parse(parse),
                OP_NOTFOUND as i32,
                i_cur,
                addr_cont,
                i_rowid_reg,
                p_pk.n_key_col as i32,
            );
        }

        if wi.a[i_level].i_left_join == 0 {
            // Se um índice parcial dirige o laço, tenta eliminar da consulta os termos do WHERE
            // que precisam ser verdadeiros por causa do WHERE do índice parcial.
            //
            // 2019-11-02, ticket 623eff57e76d45f6: esta otimização não funciona num LEFT JOIN.
            if let Some(truth) = idx.p_partial_idx_where.as_deref() {
                where_apply_partial_index_constraints(truth, i_cur, wi, s_wc);
            }
        } else {
            // O OR-optimization não funciona para a tabela direita de um LEFT JOIN. Isto é só uma
            // observação, não um requisito.
            debug_assert!(
                (wi.wctrl_flags as u32 & (WHERE_OR_SUBCLAUSE | WHERE_RIGHT_JOIN)) == 0
            );
        }

        // Registra a instrução que termina o laço.
        let ws_now = wi.w_loop(lp).ws_flags;
        if (ws_now & WHERE_ONEROW) != 0
            || (!wi.a[i_level].a_in_loop.is_empty()
                && reg_bignull == 0
                && where_loop_is_one_row(wi, lp))
        {
            wi.a[i_level].op = OP_NOOP;
        } else if b_rev {
            wi.a[i_level].op = OP_PREV;
        } else {
            wi.a[i_level].op = OP_NEXT;
        }
        wi.a[i_level].p1 = i_idx_cur;
        wi.a[i_level].p3 = if (ws_now & WHERE_UNQ_WANTED) != 0 { 1 } else { 0 };
        if (ws_now & WHERE_CONSTRAINT) == 0 {
            wi.a[i_level].p5 = SQLITE_STMTSTATUS_FULLSCAN_STEP as u8;
        } else {
            debug_assert!(wi.a[i_level].p5 == 0);
        }
        if omit_table {
            p_idx = None;
        }
    } else if (ws0 & WHERE_MULTI_OR) != 0 {
        // Caso 5: dois ou mais termos indexados separadamente ligados por OR.
        //
        // Exemplo:
        //
        //   CREATE TABLE t1(a,b,c,d);
        //   CREATE INDEX i1 ON t1(a);
        //   CREATE INDEX i2 ON t1(b);
        //   CREATE INDEX i3 ON t1(c);
        //
        //   SELECT * FROM t1 WHERE a=5 OR b=7 OR (c=11 AND d=13)
        //
        // No topo do laço há um `Null 1` (zera o rowset no registrador 1). Depois, para cada
        // termo indexado, um sqlite3WhereBegin(<termo>), um `RowSetTest` que insere o rowid no
        // rowset (se já está presente, o controle pula o Gosub e vai direto para o código gerado
        // por WhereEnd()), um `Gosub 2 A` e um sqlite3WhereEnd(). Depois disso vem o código que
        // termina o laço: o rótulo A, alvo do Gosub, salta para a instrução logo depois do Goto.
        //
        // Adicionado em 2014-05-26: se a tabela é WITHOUT ROWID, usa-se um índice efêmero em vez
        // de um RowSet para registrar as chaves primárias das linhas já vistas.
        let mut p_cov: Option<Rc<Index>> = None;
        let i_cov_cur = parse.n_tab;
        parse.n_tab += 1;

        parse.n_mem += 1;
        let reg_return_or = parse.n_mem;
        let mut reg_rowset = 0;
        let mut reg_rowid = 0;
        let i_loop_body = make_label(parse);
        let mut untested_terms = false;
        let mut p_and_expr: Option<Box<Expr>> = None;

        let p_term = l_term(wi, lp, 0).expect("pLoop->aLTerm[0]");
        debug_assert!((wi.term(p_term).e_operator & WO_OR) != 0);
        debug_assert!((wi.term(p_term).wt_flags & TERM_ORINFO) != 0);
        let or_wc: ClauseId = wi.term(p_term).p_or_info.as_ref().expect("u.pOrInfo").wc;
        wi.a[i_level].op = OP_RETURN;
        wi.a[i_level].p1 = reg_return_or;

        // Monta um SrcList novo em `pOrTab` com a tabela varrida por este laço em a[0] e todas as
        // tabelas notReady em a[1..]. Ele vira o SrcList da chamada recursiva a
        // sqlite3WhereBegin().
        let mut or_tab_owned = SrcList::default();
        let p_or_tab: &mut SrcList = if wi.n_level > 1 {
            let n_not_ready = wi.n_level as usize - i_level - 1;
            let mut a = Vec::with_capacity(n_not_ready + 1);
            a.push(p_tab_list.a[i_from].clone());
            for k in 1..=n_not_ready {
                a.push(p_tab_list.a[wi.a[i_level + k].i_from as usize].clone());
            }
            or_tab_owned.a = a;
            &mut or_tab_owned
        } else {
            &mut *p_tab_list
        };

        // Inicia o registrador do rowset com NULL (um NULL SQL equivale a um rowset vazio). Ou
        // cria um índice efêmero capaz de guardar chaves primárias, no caso de WITHOUT ROWID.
        //
        // Também inicia regReturn com o endereço da instrução logo depois do OP_Return do fim do
        // laço. Isso é preciso em alguns casos obscuros de LEFT JOIN em que o controle pula o topo
        // do laço e cai no corpo dele. Nesse caso a resposta certa do código de fim de laço (o
        // OP_Return) é seguir para a próxima instrução, como um OP_Next num cursor não iniciado.
        if (wi.wctrl_flags as u32 & WHERE_DUPLICATES_OK) == 0 {
            if p_tab.has_rowid() {
                parse.n_mem += 1;
                reg_rowset = parse.n_mem;
                add_op2(vdbe_of_parse(parse), OP_NULL as i32, 0, reg_rowset);
            } else {
                let p_pk = primary_key_index(&p_tab).cloned().expect("sqlite3PrimaryKeyIndex");
                reg_rowset = parse.n_tab;
                parse.n_tab += 1;
                add_op2(vdbe_of_parse(parse), OP_OPENEPHEMERAL as i32, reg_rowset, p_pk.n_key_col as i32);
                crate::vdbeaux::set_p4_key_info(parse, db, &p_pk);
            }
            parse.n_mem += 1;
            reg_rowid = parse.n_mem;
        }
        let i_ret_init = add_op2(vdbe_of_parse(parse), OP_INTEGER as i32, 0, reg_return_or);

        // Se o WHERE original é da forma (x1 OR x2 OR ...) AND y, então para cada termo xN avalia
        // a subexpressão xN AND y. Assim, os termos de y fatorados na disjunção são pegos pelas
        // chamadas recursivas a sqlite3WhereBegin() abaixo.
        //
        // Na verdade cada subexpressão vira "xN AND w", em que w são os termos "interessantes" de
        // z: os que não nasceram na cláusula ON ou USING de um LEFT JOIN e os que são usáveis em
        // índices.
        //
        // A otimização também só vale se o termo (x1 OR x2 OR ...) não está na cláusula ON de um
        // LEFT JOIN. Ver https://www.sqlite.org/src/info/f2369304e4
        //
        // 2022-02-04: não empurra fatias de uma comparação de vetor, para que a inicialização do
        // operando direito não deixe de ocorrer num ramo OR que não é tomado.
        //
        // 2022-03-03: não empurra expressões com subconsultas (tag-20220303a).
        // https://sqlite.org/forum/forumpost/36937b197273d403
        if wi.n_term(s_wc) > 1 {
            for i_term in 0..wi.n_term(s_wc) {
                let tid = wi.term_at(s_wc, i_term);
                if tid == p_term {
                    continue;
                }
                let wt = wi.term(tid).wt_flags;
                if (wt & (TERM_VIRTUAL | TERM_CODED | TERM_SLICE)) != 0 {
                    continue;
                }
                if (wi.term(tid).e_operator & WO_ALL) == 0 {
                    continue;
                }
                if wi.expr(tid).has_property(EP_SUBQUERY) {
                    continue; // tag-20220303a
                }
                let dup = expr_dup(Some(wi.expr(tid)), 0);
                p_and_expr = expr_and(db, parse, p_and_expr.take(), dup);
            }
            if p_and_expr.is_some() {
                // O bit extra 0x10000 do opcode é mascarado e não entra no Expr.op. Mas faz a
                // comparação op==TK_AND dentro de sqlite3PExpr() dar falso, o que impede a
                // otimização de curto-circuito do AND, que aqui não se quer.
                p_and_expr =
                    new_p_expr(db, parse, TK_AND as i32 | 0x10000, None, p_and_expr.take());
            }
        }

        // Roda um WHERE separado para cada termo da cláusula OR. Depois de eliminar duplicatas dos
        // outros WHERE, a ação de cada sub-WHERE é chamar o corpo principal do laço como
        // sub-rotina.
        explain(parse, &*db, true, b"MULTI-INDEX OR", &[]);
        let n_or_terms = wi.n_term(or_wc);
        for ii in 0..n_or_terms {
            let t_or = wi.term_at(or_wc, ii);
            if wi.term(t_or).left_cursor == i_cur || (wi.term(t_or).e_operator & WO_AND) != 0 {
                let mut jmp1 = 0;
                let p_or_expr = expr_dup(Some(wi.expr(t_or)), 0).expect("pOrExpr");
                let has_and = p_and_expr.is_some();
                let mut sub_where: Box<Expr> = match p_and_expr.take() {
                    Some(mut a) => {
                        a.p_left = Some(p_or_expr);
                        a
                    }
                    None => p_or_expr,
                };
                // Percorre as entradas da tabela que casam com o termo pOrTerm.
                explain(parse, &*db, true, b"INDEX %d", &[PrintfArg::Int(ii as i64 + 1)]);
                let p_sub = where_begin(
                    db,
                    parse,
                    p_or_tab,
                    Some(&mut *sub_where),
                    None,
                    None,
                    None,
                    WHERE_OR_SUBCLAUSE as u16,
                    i_cov_cur,
                );
                debug_assert!(p_sub.is_some() || parse.n_err != 0);
                if let Some(p_sub) = p_sub {
                    where_explain_one_scan(&*db, parse, &*p_or_tab, &p_sub, 0, 0);

                    // Este é o corpo do sub-WHERE. Primeiro pula as linhas duplicadas de sub-WHERE
                    // anteriores e registra o rowid (ou a PRIMARY KEY) da linha corrente para que
                    // a mesma linha seja pulada nos sub-WHERE seguintes.
                    if (wi.wctrl_flags as u32 & WHERE_DUPLICATES_OK) == 0 {
                        let i_set: i32 = if ii == n_or_terms - 1 { -1 } else { ii as i32 };
                        if p_tab.has_rowid() {
                            expr_code_get_column_of_table(db, parse, &p_tab, i_cur, -1, reg_rowid);
                            jmp1 = add_op4_int(
                                vdbe_of_parse(parse),
                                OP_ROWSETTEST as i32,
                                reg_rowset,
                                0,
                                reg_rowid,
                                i_set,
                            );
                        } else {
                            let p_pk =
                                primary_key_index(&p_tab).cloned().expect("sqlite3PrimaryKeyIndex");
                            let n_pk = p_pk.n_key_col as i32;
                            // Lê a PK para um vetor de registradores temporários.
                            let r = get_temp_range(parse, n_pk);
                            for i_pk in 0..n_pk {
                                let i_col = p_pk.ai_column[i_pk as usize] as i32;
                                expr_code_get_column_of_table(
                                    db, parse, &p_tab, i_cur, i_col, r + i_pk,
                                );
                            }
                            // Verifica se a tabela temporária já contém esta chave. Se sim, a
                            // linha já entrou no resultado e pode ser ignorada (saltando o Gosub
                            // abaixo). Senão, insere a chave na tabela temporária e processa a
                            // linha.
                            //
                            // Usa algumas das otimizações do OP_RowSetTest: se iSet é zero,
                            // supõe que a chave não pode estar na tabela temporária. E se iSet é
                            // -1, supõe que não há necessidade de inserir a chave, pois ela nunca
                            // será testada.
                            if i_set != 0 {
                                jmp1 = add_op4_int(
                                    vdbe_of_parse(parse),
                                    OP_FOUND as i32,
                                    reg_rowset,
                                    0,
                                    r,
                                    n_pk,
                                );
                            }
                            if i_set >= 0 {
                                add_op3(vdbe_of_parse(parse), OP_MAKERECORD as i32, r, n_pk, reg_rowid);
                                add_op4_int(
                                    vdbe_of_parse(parse),
                                    OP_IDXINSERT as i32,
                                    reg_rowset,
                                    reg_rowid,
                                    r,
                                    n_pk,
                                );
                                if i_set != 0 {
                                    change_p5(vdbe_of_parse(parse), OPFLAG_USESEEKRESULT as u16);
                                }
                            }
                            // Libera o vetor de registradores temporários.
                            release_temp_range(parse, r, n_pk);
                        }
                    }

                    // Chama o corpo principal do laço como sub-rotina.
                    add_op2(vdbe_of_parse(parse), OP_GOSUB as i32, reg_return_or, i_loop_body);

                    // Salta para cá (pulando a sub-rotina do corpo do laço) se a linha corrente do
                    // sub-WHERE é duplicata de sub-WHERE anteriores.
                    if jmp1 != 0 {
                        jump_here(vdbe_of_parse(parse), jmp1);
                    }

                    // O flag untestedTerms do sub-WHERE significa que este termo OR continha um ou
                    // mais termos AND de uma tabela notReady. Esses termos não puderam ser testados
                    // e terão de ser testados depois.
                    if p_sub.untested_terms {
                        untested_terms = true;
                    }

                    // Se todos os termos ligados por OR são otimizados pelo mesmo índice, e o
                    // índice é aberto com o mesmo número de cursor em cada chamada a
                    // sqlite3WhereBegin() deste laço, o índice pode servir de índice de cobertura.
                    //
                    // Se a chamada acima resultou numa varredura que usa índice, e este é o
                    // primeiro termo OR ou o índice é o mesmo de todos os anteriores, pCov é o
                    // candidato a índice de cobertura. Senão, pCov é nulo.
                    let sub_loop = p_sub.w_loop(p_sub.a[0].p_w_loop);
                    debug_assert!((sub_loop.ws_flags & WHERE_AUTO_INDEX) == 0);
                    if (sub_loop.ws_flags & WHERE_INDEXED) != 0
                        && (ii == 0 || same_index(&sub_loop.btree.p_index, &p_cov))
                        && (p_tab.has_rowid()
                            || !sub_loop
                                .btree
                                .p_index
                                .as_deref()
                                .map_or(false, |x| x.is_primary_key_index()))
                    {
                        debug_assert!(p_sub.a[0].i_idx_cur == i_cov_cur);
                        p_cov = sub_loop.btree.p_index.clone();
                    } else {
                        p_cov = None;
                    }
                    if where_uses_deferred_seek(&p_sub) {
                        wi.b_deferred_seek = true;
                    }

                    // Termina o laço pelas entradas da tabela que casam com o termo pOrTerm.
                    where_end(db, parse, p_or_tab, p_sub);
                    explain_pop(parse);
                }
                // `sqlite3ExprDelete(db, pDelete)`: a cópia do ramo OR cai com o AND.
                if has_and {
                    sub_where.p_left = None;
                    p_and_expr = Some(sub_where);
                }
            }
        }
        explain_pop(parse);
        debug_assert!(wi.a[i_level].p_w_loop == lp);
        debug_assert!((wi.w_loop(lp).ws_flags & WHERE_MULTI_OR) != 0);
        debug_assert!((wi.w_loop(lp).ws_flags & WHERE_IN_ABLE) == 0);
        let has_cov = p_cov.is_some();
        wi.a[i_level].p_covering_idx = p_cov;
        if has_cov {
            wi.a[i_level].i_idx_cur = i_cov_cur;
        }
        // `pAndExpr->pLeft = 0; sqlite3ExprDelete(db, pAndExpr)`.
        drop(p_and_expr);
        let a = current_addr(parse);
        change_p1(vdbe_of_parse(parse), i_ret_init, a);
        vdbe_goto(vdbe_of_parse(parse), wi.a[i_level].addr_brk);
        resolve_label(parse, db, i_loop_body);

        // Põe o operando P2 do OP_Return que termina o laço corrente neste ponto, que é o topo do
        // próximo laço que o contém. O formatador de bytecode usa esse P2 como dica para indentar
        // tudo entre este ponto e o OP_Return final. Ver tag-20220407a em vdbe.c e shell.c.
        debug_assert!(wi.a[i_level].op == OP_RETURN);
        wi.a[i_level].p2 = current_addr(parse);

        if !untested_terms {
            disable_term(wi, i_level, p_term);
        }
    } else {
        // Caso 6: não há índice utilizável. É preciso varrer a tabela inteira.
        const A_STEP: [u8; 2] = [OP_NEXT, OP_PREV];
        const A_START: [u8; 2] = [OP_REWIND, OP_LAST];
        if is_recursive {
            // Tabelas marcadas isRecursive têm uma única linha, guardada num pseudo-cursor. Não é
            // preciso Rewind nem Next nesses cursores.
            wi.a[i_level].op = OP_NOOP;
        } else {
            wi.a[i_level].op = A_STEP[b_rev as usize];
            wi.a[i_level].p1 = i_cur;
            let a = add_op2(vdbe_of_parse(parse), A_START[b_rev as usize] as i32, i_cur, addr_halt);
            wi.a[i_level].p2 = 1 + a;
            wi.a[i_level].p5 = SQLITE_STMTSTATUS_FULLSCAN_STEP as u8;
        }
    }

    // Insere código que testa toda subexpressão que pode ser calculada por completo com o
    // conjunto corrente de tabelas.
    //
    // Este laço roda de uma a três vezes, conforme as restrições a gerar. A variável i_loop decide
    // as restrições de cada iteração:
    //
    // i_loop==1: só as expressões inteiramente cobertas por pIdx.
    // i_loop==2: as demais expressões que não têm subconsultas correlacionadas.
    // i_loop==3: todas as demais.
    //
    // Faz-se um esforço para pular iterações desnecessárias. Esta otimização, que faz as
    // restrições simples ocorrerem antes das complexas, é o "MySQL push-down" (há outra, sem
    // relação, a "WHERE-clause push-down").
    let mut i_loop: i32 = if p_idx.is_some() { 1 } else { 2 };
    loop {
        let mut i_next = 0;
        let n_term = wi.n_term(s_wc);
        for j in 0..n_term {
            let tid = wi.term_at(s_wc, j);
            let wt = wi.term(tid).wt_flags;
            if (wt & (TERM_VIRTUAL | TERM_CODED)) != 0 {
                continue;
            }
            if (wi.term(tid).prereq_all & wi.a[i_level].not_ready) != 0 {
                wi.untested_terms = true;
                continue;
            }
            let (e_flags, e_join) = {
                let e = wi.expr(tid);
                (e.flags, e.i_join())
            };
            if (jointype & (JT_LEFT | JT_LTORJ | JT_RIGHT)) != 0 {
                if (e_flags & (EP_OUTER_ON | EP_INNER_ON)) == 0 {
                    // Adia as restrições do WHERE até depois do processamento do outer join.
                    // tag-20220513a
                    continue;
                } else if (jointype & JT_LEFT) == JT_LEFT && (e_flags & EP_OUTER_ON) == 0 {
                    continue;
                } else {
                    let m = wi.s_mask_set.get_mask(e_join);
                    if (m & wi.a[i_level].not_ready) != 0 {
                        // Uma cláusula ON que ainda não está madura.
                        continue;
                    }
                }
            }
            if i_loop == 1 {
                let i_tab_cur = wi.a[i_level].i_tab_cur;
                let covered = match p_idx.as_deref() {
                    Some(ix) => expr_covered_by_index(wi.expr_mut(tid), i_tab_cur, ix) != 0,
                    None => true,
                };
                if !covered {
                    i_next = 2;
                    continue;
                }
            }
            if i_loop < 3 && (wt & TERM_VARSELECT) != 0 {
                if i_next == 0 {
                    i_next = 3;
                }
                continue;
            }

            let mut skip_like_addr = 0;
            if (wt & TERM_LIKECOND) != 0 {
                // Se `TERM_LIKECOND` está ligado, a busca por intervalo basta para garantir que o
                // LIKE é verdadeiro, então dá para pular a chamada de like(A,B). Mas isso só vale
                // para strings: não se pula a chamada na passada que compara BLOBs.
                let x = wi.a[i_level].i_like_rep_cntr;
                if x > 0 {
                    skip_like_addr = add_op1(
                        vdbe_of_parse(parse),
                        (if (x & 1) != 0 { OP_IFNOT } else { OP_IF }) as i32,
                        (x >> 1) as i32,
                    );
                }
            }
            expr_if_false(db, parse, wi.expr_mut(tid), addr_cont, SQLITE_JUMPIFNULL as i32, None);
            if skip_like_addr != 0 {
                jump_here(vdbe_of_parse(parse), skip_like_addr);
            }
            wi.term_mut(tid).wt_flags |= TERM_CODED;
        }
        i_loop = i_next;
        if i_loop <= 0 {
            break;
        }
    }

    // Insere código que testa as restrições implícitas pela transitividade do operador "==".
    //
    // Exemplo: se o WHERE tem "t1.a=t2.b" e "t2.b=123" e se codifica o laço de t1 sem que o laço
    // de t2 tenha sido codificado, não se pode usar "t1.a=t2.b", mas se pode codificar a restrição
    // implícita "t1.a=123".
    let n_base = wi.clause(s_wc).n_base as usize;
    for j in 0..n_base {
        let tid = wi.term_at(s_wc, j);
        let (wt, e_op, left_cursor, left_column) = {
            let t = wi.term(tid);
            (t.wt_flags, t.e_operator, t.left_cursor, t.left_column)
        };
        if (wt & (TERM_VIRTUAL | TERM_CODED)) != 0 {
            continue;
        }
        if (e_op & (WO_EQ | WO_IS)) == 0 {
            continue;
        }
        if (e_op & WO_EQUIV) == 0 {
            continue;
        }
        if left_cursor != i_cur {
            continue;
        }
        if (jointype & (JT_LEFT | JT_LTORJ | JT_RIGHT)) != 0 {
            continue;
        }
        debug_assert!(!wi.expr(tid).has_property(EP_OUTER_ON));
        debug_assert!((wi.term(tid).prereq_right & wi.a[i_level].not_ready) != 0);
        debug_assert!((e_op & (WO_OR | WO_AND)) == 0);
        let Some(p_alt) = where_find_term(
            db,
            parse,
            &*wi,
            s_wc,
            i_cur,
            left_column,
            not_ready,
            (WO_EQ | WO_IN | WO_IS) as u32,
            None,
        ) else {
            continue;
        };
        if (wi.term(p_alt).wt_flags & TERM_CODED) != 0 {
            continue;
        }
        if (wi.term(p_alt).e_operator & WO_IN) != 0 {
            let e = wi.expr(p_alt);
            if e.use_x_select()
                && e.x_select().and_then(|s| s.p_e_list.as_deref()).map_or(0, |l| l.a.len()) > 1
            {
                continue;
            }
        }
        // `sEAlt = *pAlt->pExpr; sEAlt.pLeft = pE->pLeft;`
        let mut s_e_alt: Expr = wi.expr(p_alt).clone();
        s_e_alt.p_left = wi.expr(tid).p_left.clone();
        expr_if_false(db, parse, &mut s_e_alt, addr_cont, SQLITE_JUMPIFNULL as i32, None);
        wi.term_mut(p_alt).wt_flags |= TERM_CODED;
    }

    // Para um RIGHT OUTER JOIN, registra que a linha corrente casou pelo menos uma vez.
    if wi.a[i_level].p_rj.is_some() {
        let p_rj: WhereRightJoin = **wi.a[i_level].p_rj.as_ref().expect("pLevel->pRJ");
        // `p_tab` é a tabela direita do RIGHT JOIN. Gera o código que registra que a linha
        // corrente dela casou ao menos uma vez, guardando a PK da linha tanto no índice iMatch
        // quanto no filtro de Bloom regBloom.
        let n_pk: i32;
        let r: i32;
        if p_tab.has_rowid() {
            r = get_temp_range(parse, 2);
            let i_tab_cur = wi.a[i_level].i_tab_cur;
            expr_code_get_column_of_table(db, parse, &p_tab, i_tab_cur, -1, r + 1);
            n_pk = 1;
        } else {
            let p_pk = primary_key_index(&p_tab).cloned().expect("sqlite3PrimaryKeyIndex");
            n_pk = p_pk.n_key_col as i32;
            r = get_temp_range(parse, n_pk + 1);
            for i_pk in 0..n_pk {
                let i_col = p_pk.ai_column[i_pk as usize] as i32;
                expr_code_get_column_of_table(db, parse, &p_tab, i_cur, i_col, r + 1 + i_pk);
            }
        }
        let jmp1 = add_op4_int(vdbe_of_parse(parse), OP_FOUND as i32, p_rj.i_match, 0, r + 1, n_pk);
        vdbe_comment(vdbe_of_parse(parse), b"match against %s", &[text_arg(&p_tab.z_name)]);
        add_op3(vdbe_of_parse(parse), OP_MAKERECORD as i32, r + 1, n_pk, r);
        add_op4_int(vdbe_of_parse(parse), OP_IDXINSERT as i32, p_rj.i_match, r, r + 1, n_pk);
        add_op4_int(vdbe_of_parse(parse), OP_FILTERADD as i32, p_rj.reg_bloom, 0, r + 1, n_pk);
        change_p5(vdbe_of_parse(parse), OPFLAG_USESEEKRESULT as u16);
        jump_here(vdbe_of_parse(parse), jmp1);
        release_temp_range(parse, r, n_pk + 1);
    }

    // Para um LEFT OUTER JOIN, gera o código que registra que pelo menos uma linha da tabela
    // direita casou com a tabela esquerda.
    let mut do_outer_constraints = false;
    if wi.a[i_level].i_left_join != 0 {
        let a = current_addr(parse);
        wi.a[i_level].addr_first = a;
        let i_lj = wi.a[i_level].i_left_join;
        add_op2(vdbe_of_parse(parse), OP_INTEGER as i32, 1, i_lj);
        vdbe_comment(vdbe_of_parse(parse), b"record LEFT JOIN hit", &[]);
        if wi.a[i_level].p_rj.is_none() {
            do_outer_constraints = true; // restrições do WHERE
        }
    }

    if wi.a[i_level].p_rj.is_some() {
        // Cria uma sub-rotina usada para processar todos os laços internos e o código do RIGHT
        // JOIN. Na operação normal, a sub-rotina fica em linha com o resto do código. Mas no fim
        // um laço separado a chama para as linhas de pTab sem correspondência, com todas as
        // tabelas à esquerda como NULL.
        let reg_ret = wi.a[i_level].p_rj.as_ref().expect("pLevel->pRJ").reg_return;
        add_op2(vdbe_of_parse(parse), OP_BEGINSUBRTN as i32, 0, reg_ret);
        let a = current_addr(parse);
        wi.a[i_level].p_rj.as_mut().expect("pLevel->pRJ").addr_subrtn = a;
        debug_assert!(parse.within_rj_subrtn < 255);
        parse.within_rj_subrtn += 1;
        do_outer_constraints = true;
    }

    if do_outer_constraints {
        // As restrições do WHERE precisam ser adiadas até depois da eliminação de linhas do outer
        // join, pois elas se aplicam aos resultados do OUTER JOIN. Este laço gera as verificações
        // das restrições do WHERE. tag-20220513a
        let n_base = wi.clause(s_wc).n_base as usize;
        for j in 0..n_base {
            let tid = wi.term_at(s_wc, j);
            if (wi.term(tid).wt_flags & (TERM_VIRTUAL | TERM_CODED)) != 0 {
                continue;
            }
            if (wi.term(tid).prereq_all & wi.a[i_level].not_ready) != 0 {
                debug_assert!(wi.untested_terms);
                continue;
            }
            if (jointype & JT_LTORJ) != 0 {
                continue;
            }
            expr_if_false(db, parse, wi.expr_mut(tid), addr_cont, SQLITE_JUMPIFNULL as i32, None);
            wi.term_mut(tid).wt_flags |= TERM_CODED;
        }
    }

    wi.a[i_level].not_ready
}

// ---------------------------------------------------------------------------------------------
// RIGHT JOIN (chunk 007)
// ---------------------------------------------------------------------------------------------

/// `sqlite3WhereRightJoinLoop`: gera o código do laço que acha todos os termos sem
/// correspondência de um RIGHT JOIN (o nível `i_level` de `wi`).
pub fn where_right_join_loop(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab_list: &SrcList,
    wi: &WhereInfo,
    i_level: usize,
) {
    let p_rj: WhereRightJoin = **wi.a[i_level].p_rj.as_ref().expect("pLevel->pRJ");
    let mut p_sub_where: Option<Box<Expr>> = None;
    let s_wc = wi.s_wc;
    let lp = wi.a[i_level].p_w_loop;
    let p_tab_item = &p_tab_list.a[wi.a[i_level].i_from as usize];
    let p_tab: Rc<Table> = p_tab_item.p_tab.clone().expect("pTabItem->pTab");
    let mut m_all: Bitmask = 0;

    explain(parse, &*db, true, b"RIGHT-JOIN %s", &[text_arg(&p_tab.z_name)]);
    for k in 0..i_level {
        debug_assert!(wi.w_loop(wi.a[k].p_w_loop).i_tab == wi.a[k].i_from);
        let p_right = &p_tab_list.a[wi.a[k].i_from as usize];
        m_all |= wi.w_loop(wi.a[k].p_w_loop).mask_self;
        if p_right.fg.via_coroutine {
            let n_expr = p_right
                .p_select
                .as_deref()
                .and_then(|s| s.p_e_list.as_deref())
                .map_or(0, |l| l.a.len()) as i32;
            add_op3(
                vdbe_of_parse(parse),
                OP_NULL as i32,
                0,
                p_right.reg_result,
                p_right.reg_result + n_expr - 1,
            );
        }
        add_op1(vdbe_of_parse(parse), OP_NULLROW as i32, wi.a[k].i_tab_cur);
        let i_idx_cur = wi.a[k].i_idx_cur;
        if i_idx_cur != 0 {
            add_op1(vdbe_of_parse(parse), OP_NULLROW as i32, i_idx_cur);
        }
    }
    if (p_tab_item.fg.jointype & JT_LTORJ) == 0 {
        m_all |= wi.w_loop(lp).mask_self;
        for k in 0..wi.n_term(s_wc) {
            let tid = wi.term_at(s_wc, k);
            let term = wi.term(tid);
            if (term.wt_flags & (TERM_VIRTUAL | TERM_SLICE)) != 0 && term.e_operator != WO_ROWVAL {
                break;
            }
            if (term.prereq_all & !m_all) != 0 {
                continue;
            }
            if wi.expr(tid).has_property(EP_OUTER_ON | EP_INNER_ON) {
                continue;
            }
            let dup = expr_dup(Some(wi.expr(tid)), 0);
            p_sub_where = expr_and(db, parse, p_sub_where.take(), dup);
        }
    }
    let mut s_from = SrcList { a: vec![p_tab_item.clone()] };
    s_from.a[0].fg.jointype = 0;
    debug_assert!(parse.within_rj_subrtn < 100);
    parse.within_rj_subrtn += 1;
    let p_sub_w_info = where_begin(
        db,
        parse,
        &mut s_from,
        p_sub_where.as_deref_mut(),
        None,
        None,
        None,
        WHERE_RIGHT_JOIN as u16,
        0,
    );
    if let Some(p_sub) = p_sub_w_info {
        let i_cur = wi.a[i_level].i_tab_cur;
        parse.n_mem += 1;
        let r = parse.n_mem;
        let addr_cont = where_continue_label(&p_sub);
        let n_pk: i32;
        if p_tab.has_rowid() {
            expr_code_get_column_of_table(db, parse, &p_tab, i_cur, -1, r);
            n_pk = 1;
        } else {
            let p_pk = primary_key_index(&p_tab).cloned().expect("sqlite3PrimaryKeyIndex");
            n_pk = p_pk.n_key_col as i32;
            parse.n_mem += n_pk - 1;
            for i_pk in 0..n_pk {
                let i_col = p_pk.ai_column[i_pk as usize] as i32;
                expr_code_get_column_of_table(db, parse, &p_tab, i_cur, i_col, r + i_pk);
            }
        }
        let jmp = add_op4_int(vdbe_of_parse(parse), OP_FILTER as i32, p_rj.reg_bloom, 0, r, n_pk);
        add_op4_int(vdbe_of_parse(parse), OP_FOUND as i32, p_rj.i_match, addr_cont, r, n_pk);
        jump_here(vdbe_of_parse(parse), jmp);
        add_op2(vdbe_of_parse(parse), OP_GOSUB as i32, p_rj.reg_return, p_rj.addr_subrtn);
        where_end(db, parse, &s_from, p_sub);
    }
    // `sqlite3ExprDelete(pParse->db, pSubWhere)`: cai aqui.
    drop(p_sub_where);
    explain_pop(parse);
    debug_assert!(parse.within_rj_subrtn > 0);
    parse.within_rj_subrtn -= 1;
}
