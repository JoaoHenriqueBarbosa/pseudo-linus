//! Tradução de `select.c`, terceira e última parte (chunks `select_c.012` a `select_c.019`): a
//! resolução de tabelas FROM que são CTE (`resolveFromTermToCte`), o expansor de SELECT
//! (`selectExpander`, `sqlite3SelectExpand`, `sqlite3ExpandSubquery`), a informação de tipos das
//! subconsultas (`sqlite3SelectAddTypeInfo`), `sqlite3SelectPrep`, os agregados
//! (`analyzeAggFuncArgs`, `resetAccumulator`, `finalizeAggFunctions`, `updateAccumulator`,
//! `havingToWhere`, ...), as otimizações de FROM (`countOfViewOptimization`, `isSelfJoinView`,
//! `fromClauseTermCanBeCoroutine`) e o próprio `sqlite3Select`. As decisões do modelo são as de
//! `select.rs` e `select2.rs` e acrescentam o seguinte:
//!
//! - **Pilha de WITH.** O `pParse->pWith` do C é uma lista ligada por `pOuter` em que
//!   `resolveFromTermToCte` troca o topo para um nível mais baixo (`pParse->pWith = pWith`), de
//!   modo que o encadeamento forma uma ÁRVORE (um WITH empilhado depois da troca tem `pOuter`
//!   no nível mais baixo, não no antigo topo). Aqui a árvore é uma arena (`ExpandCtx.arena`,
//!   `WithNode { with, outer }`) mais o nó corrente (`ExpandCtx.cur`), que vivem no contexto do
//!   walker do expansor. `search_with` recebe a cadeia do nó corrente até a raiz (do mais
//!   interno ao mais externo). O WITH que o chamador já deixou em `Parse.p_with` (comandos DML com
//!   WITH) é o nó-base: `select_expand` o move para a arena e o devolve ao `Parse` no fim.
//!   `sqlite3WithPush(pParse, p->pWith, 0)` vira a CÓPIA do `With` do select na arena (o `With`
//!   original continua em `Select.p_with`, como no C, onde `alter.c` e `attach.c` ainda o
//!   percorrem). Como o C só muta `Cte.zCteErr` (transitório) e `Cte.pUse` (um handle de
//!   `Parse.cte_uses`, que sobrevive) no `With` empilhado, a cópia é indistinguível.
//!   `sqlite3SelectPopWith` só desempilha quando o empilhamento aconteceu: o início de cada cadeia
//!   composta (o select raiz, ou o termo mais à esquerda da parte não recursiva de uma CTE
//!   recursiva) registra um "empilhou ou não" em `ExpandCtx.chain_pushed`, e o `xSelectCallback2`
//!   do termo mais à esquerda o consome (`findRightmost(p)->pWith`, que aqui não se alcança).
//! - **Tabela compartilhada por `Rc`.** O C compartilha o mesmo `Table*` entre o `pFrom->pTab`, as
//!   referências recursivas de uma CTE (`nTabRef`) e todas as expressões `TK_COLUMN` já resolvidas
//!   (`y.pTab`), e `sqlite3SubqueryColumnTypes` altera esse `Table` no lugar depois da resolução
//!   de nomes. Com `Rc<Table>` imutável, `select_add_subquery_type_info` monta a tabela nova e
//!   `retarget_table` troca, em todo o select, cada `Rc` que apontava para a antiga (itens do FROM
//!   e `y.pTab` das expressões), que é o que o ponteiro compartilhado fazia. Pelo mesmo motivo
//!   as referências recursivas de uma CTE recebem primeiro uma tabela provisória e, quando as
//!   colunas da CTE são conhecidas, a tabela definitiva (`resolve_from_term_to_cte`).
//! - **`AggInfo` e `CteUse`.** Vivem em `Parse.agg_infos` e `Parse.cte_uses` (campos que
//!   `connection.rs` ainda precisa ganhar: `Vec<AggInfo>` e `Vec<CteUse>`), por handle. As
//!   expressões `pFExpr` e `pCExpr` do `AggInfo` são CÓPIAS (ver `AggInfoCol`): quem precisa
//!   delas junto com `&mut Parse` as tira com `take()` e devolve.
//! - **`sSort.pOrderBy`** é uma cópia possuída do ORDER BY do select (`p.p_order_by.clone()`); a
//!   análise de agregados e o laço interno agem sobre a cópia, e `Select.p_order_by` não é mais
//!   lido depois dela em `sqlite3Select`.
//! - **Planejador (`where_`).** Chamado pelos nomes determinísticos (`where_begin`, `where_end`,
//!   `where_output_row_count`, `where_is_distinct`, `where_is_ordered`,
//!   `where_order_by_limit_opt_label`, `where_continue_label`, `where_break_label`,
//!   `where_is_sorted`, `where_min_max_opt_early_out`). `pTabList` é `p.p_src`, que o `Select`
//!   possui; como `pSelect` e `pTabList` se sobrepõem, o `SrcList` sai do select (`take`) durante
//!   `where_begin` e volta logo depois, de modo que `p_select.p_src` é `None` dentro dele.
//!   Janelas: `window_rewrite`, `window_code_init` e `window_code_step`.
//! - `TREETRACE`, `SQLITE_DEBUG` e os asserts de varredura do `AggInfo` não existem.
//! - Cada falha de alocação do C (`mallocFailed`) some; `db.malloc_failed` ainda é consultado
//!   onde o C o consulta.

use std::rc::Rc;

use crate::alter::rename_token_remap;
use crate::auth::auth_check;
use crate::build::{locate_table_item, primary_key_index, table_lock};
use crate::build2::{publish_view, view_get_column_names};
use crate::build3::{
    code_verify_schema, id_list_index, key_info_of_index, src_item_arg, src_list_assign_cursors,
};
use crate::connection::{Connection, Parse};
use crate::consts::{
    BTREE_UNORDERED, COLFLAG_NOEXPAND, ENAME_NAME, ENAME_ROWID, ENAME_TAB, EP_COLLATE, EP_HAS_FUNC,
    EP_INT_VALUE, EP_SKIP, EP_SUBQUERY, EP_UNLIKELY, EP_WIN_FUNC, JT_CROSS, JT_LEFT, JT_LTORJ,
    JT_OUTER, JT_RIGHT, KEYINFO_ORDER_DESC, M10D_NO, M10D_YES, NC_INAGGFUNC, NC_UAGGINFO,
    OP_AGGFINAL, OP_AGGSTEP, OP_CLOSE, OP_COLLSEQ, OP_COLUMN, OP_COMPARE, OP_COPY, OP_COUNT,
    OP_GETSUBTYPE, OP_GOSUB, OP_GOTO, OP_IDXINSERT, OP_IF, OP_IFPOS, OP_INITCOROUTINE, OP_INTEGER,
    OP_JUMP, OP_MAKERECORD, OP_NEXT, OP_NULL, OP_ONCE, OP_OPENDUP, OP_OPENEPHEMERAL,
    OP_OPENPSEUDO, OP_OPENREAD, OP_RETURN, OP_REWIND, OP_SEQUENCE, OP_SETSUBTYPE, OP_SORTERDATA,
    OP_SORTERINSERT, OP_SORTERNEXT, OP_SORTEROPEN, OP_SORTERSORT, SF_AGGREGATE, SF_COMPLEXRESULT,
    SF_COMPOUND, SF_COPYCTE, SF_DISTINCT, SF_EXPANDED, SF_FIXEDLIMIT, SF_HASTYPEINFO,
    SF_INCLUDEHIDDEN, SF_NESTEDFROM, SF_NOOPORDERBY, SF_ORDERBYREQD, SF_PUSHDOWN, SF_RECURSIVE,
    SF_RESOLVED, SF_UFSRCCHECK, SF_UPDATEFROM, SF_VIEW, SQLITE_AFF_NONE, SQLITE_COROUTINES,
    SQLITE_COUNT_OF_VIEW, SQLITE_ECEL_DUP, SQLITE_ENABLE_VIEW, SQLITE_ERROR, SQLITE_FULL_COL_NAMES,
    SQLITE_FUNC_NEEDCOLL, SQLITE_GROUP_BY_ORDER, SQLITE_JUMPIFNULL, SQLITE_LIMIT_COLUMN,
    SQLITE_NULL_UNUSED_COLS, SQLITE_OK, SQLITE_OMIT_ORDER_BY, SQLITE_PROPAGATE_CONST,
    SQLITE_PUSH_DOWN, SQLITE_QUERY_FLATTENER, SQLITE_READ, SQLITE_SELECT, SQLITE_SHORT_COL_NAMES,
    SQLITE_SIMPLIFY_JOIN, SQLITE_TRUSTED_SCHEMA, SRT_COROUTINE, SRT_DISCARD, SRT_DISTFIFO,
    SRT_DISTQUEUE, SRT_EPHEMTAB, SRT_EXCEPT, SRT_EXISTS, SRT_FIFO, SRT_OUTPUT, SRT_QUEUE,
    SRT_UNION, TABTYP_VIEW, TF_EPHEMERAL, TF_NO_VISIBLE_ROWID, TK_AGG_COLUMN, TK_AGG_FUNCTION,
    TK_ALL, TK_AND, TK_ASTERISK, TK_DOT, TK_FUNCTION, TK_ID, TK_IF_NULL_ROW, TK_INTEGER, TK_NULL,
    TK_ORDER, TK_PLUS, TK_SELECT, TK_UNION, WHERE_AGG_DISTINCT, WHERE_DISTINCTBY,
    WHERE_DISTINCT_NOOP, WHERE_DISTINCT_UNORDERED, WHERE_GROUPBY, WHERE_ORDERBY_NORMAL,
    WHERE_SORTBYGROUP, WHERE_USE_LIMIT, WHERE_WANT_DISTINCT, WRC_ABORT, WRC_CONTINUE, WRC_PRUNE,
};
use crate::expr::{
    expr, expr_and, expr_coll_seq, expr_dup, expr_list_append, expr_list_dup,
    expr_set_error_offset, p_expr, p_expr_add_select, select_dup, select_expr_height,
};
use crate::expr_code::{expr_code_move, expr_is_constant_or_group_by, rowid_alias};
use crate::expr_code2::{
    clear_temp_reg_cache, expr_analyze_agg_list, expr_analyze_aggregates, expr_code,
    expr_code_expr_list, expr_if_false, expr_implies_non_null_row, expr_list_compare,
    get_temp_range, get_temp_reg, release_temp_range, release_temp_reg,
};
use crate::mem::{CollSeq, KeyInfo};
use crate::prepare::schema_to_index;
use crate::printf::{mprintf, PrintfArg};
use crate::resolve::{match_e_name, resolve_select_names};
use crate::select::{
    code_distinct, column_name, columns_from_expr_list, compute_limit_registers, current_addr,
    explain_temp_table, fix_distinct_open_eph, generate_column_names, generate_sort_tail, get_vdbe,
    key_info_from_expr_list, n_expr, nth_prior_mut, process_join, select_dest_init,
    select_inner_loop, subquery_column_types, text, unset_join_expr, vdbe_of_parse, DistinctCtx,
    SortCtx, SORTFLAG_USE_SORTER,
};
use crate::select2::{
    cannot_be_function, convert_compound_select_to_subquery, disable_unused_subquery_result_columns,
    flatten_subquery, indexed_by_lookup, is_simple_count, min_max_query, multi_select,
    propagate_constants, push_down_where_terms, search_with, ConvertCtx,
};
use crate::sqlite_int::{
    AggInfo, AggInfoId, CteUse, CteUseId, Expr, ExprList, ExprListItem, ExprX, ExprY, IdList, Index,
    NameContext, NcU, SchemaId, Select, SelectDest, SrcItem, SrcList, SrcU1, SrcU2, TabRef, Table,
    Walker, With, YnVar,
};
use crate::util::{error_msg, log_est, str_icmp, stricmp};
use crate::vdbe_types::P4;
use crate::vdbeaux::{
    add_op0, add_op1, add_op2, add_op3, add_op4, add_op4_int, append_p4, change_opcode, change_p4,
    change_p5, change_to_noop, end_coroutine, explain, explain_pop, jump_here,
    jump_here_or_pop_inst, make_label, noop_comment, resolve_label, scan_status_counters,
    scan_status_range, vdbe_comment, vdbe_goto,
};
use crate::vtab::get_vtable;
use crate::walker::{
    expr_walk_noop, select_walk_noop, walk_expr, walk_select, walk_select_expr, walk_select_from,
    WALKER_FLAG_IN_RENAME, WALKER_FLAG_POP_WITH,
};
use crate::where_::{
    where_begin, where_break_label, where_continue_label, where_end, where_is_distinct,
    where_is_ordered, where_is_sorted, where_min_max_opt_early_out,
    where_order_by_limit_opt_label, where_output_row_count, WhereInfo,
};
use crate::window::{window_code_init, window_code_step, window_rewrite};

// ---------------------------------------------------------------------------------------------
// Auxiliares privados (não existem no C)
// ---------------------------------------------------------------------------------------------

/// Monta um `Walker` sem callbacks, com `eCode` zero.
fn walker_new<C>(u: C, m_w_flags: u16) -> Walker<C> {
    Walker {
        x_expr_callback: None,
        x_select_callback: None,
        x_select_callback2: None,
        walker_depth: 0,
        e_code: 0,
        m_w_flags,
        u,
    }
}

/// Tira de `e` o `x.pList` (o `ExprX::List`), deixando `ExprX::None`; devolve `None` se `e`
/// não guardava uma lista (nesse caso `x` fica como estava).
fn take_x_list(e: &mut Expr) -> Option<Box<ExprList>> {
    match std::mem::take(&mut e.x) {
        ExprX::List(l) => Some(l),
        other => {
            e.x = other;
            None
        }
    }
}

/// Devolve a `e` a lista que `take_x_list` tirou.
fn put_x_list(e: &mut Expr, l: Option<Box<ExprList>>) {
    if let Some(l) = l {
        e.x = ExprX::List(l);
    }
}

/// O endereço do nó, a chave de identidade de `sqlite3RenameTokenRemap`.
fn expr_addr(e: Option<&Expr>) -> usize {
    e.map_or(0, |e| e as *const Expr as usize)
}

// ---------------------------------------------------------------------------------------------
// Chunk 012: CTE, expansão de subconsulta e o expansor
// ---------------------------------------------------------------------------------------------

/// Um nó da árvore de WITH ativos (ver a nota do módulo): o `With` empilhado e o nó que era o
/// corrente quando ele entrou (`pOuter`).
struct WithNode {
    with: Box<With>,
    outer: Option<usize>,
}

/// O contexto do walker do expansor: o `pWalker->pParse` do C (com a conexão) e a pilha de WITH.
struct ExpandCtx<'a> {
    /// A conexão (o `pParse->db`).
    db: &'a mut Connection,
    /// O contexto de análise.
    parse: &'a mut Parse,
    /// Todos os WITH já empilhados (a arena da árvore `pOuter`).
    arena: Vec<WithNode>,
    /// O nó que o `pParse->pWith` do C aponta; `None` é a pilha vazia.
    cur: Option<usize>,
    /// Uma entrada por cadeia composta em expansão: se o início dela empilhou um WITH.
    chain_pushed: Vec<bool>,
    /// O próximo select visitado começa uma cadeia mesmo tendo `has_next` (a parte não recursiva
    /// de uma CTE recursiva é percorrida a partir do meio da cadeia).
    force_chain_start: bool,
}

impl ExpandCtx<'_> {
    /// A cadeia do nó corrente até a raiz, do mais interno ao mais externo (índices na arena).
    fn chain_nodes(&self) -> Vec<usize> {
        let mut v = Vec::new();
        let mut c = self.cur;
        while let Some(i) = c {
            v.push(i);
            c = self.arena[i].outer;
        }
        v
    }
}

/// `resolveFromTermToCte`: confere se `p_from` se refere a uma CTE declarada por um WITH da pilha
/// do analisador (e, se estamos dentro de uma CTE, se a referência é recursiva). Se casa, preenche
/// `p_from.p_tab` e os demais campos.
///
/// Devolve 0 se não casa, 1 se casa e 2 se houve erro.
fn resolve_from_term_to_cte(w: &mut Walker<ExpandCtx<'_>>, p_from: &mut SrcItem) -> i32 {
    debug_assert!(p_from.p_tab.is_none());
    if w.u.cur.is_none() {
        // Não há WITH na pilha: nenhum casamento é possível.
        return 0;
    }
    if w.u.parse.n_err != 0 {
        // Erros anteriores podem ter deixado a pilha num estado estranho: não vai adiante.
        return 0;
    }
    if p_from.z_database.is_some() {
        // O termo tem qualificador de esquema (main.t1): não pode ser uma CTE.
        return 0;
    }
    if p_from.fg.not_cte {
        // O termo está excluído de casar com CTE: (1) faz parte de um gatilho que tinha
        // zDatabase tirado por sqlite3FixTriggerStep(); (2) é o primeiro termo do FROM de um
        // UPDATE.
        return 0;
    }
    let z_name: Vec<u8> = p_from.z_name.clone().unwrap_or_default();
    let nodes = w.u.chain_nodes();
    let found = {
        let refs: Vec<&With> = nodes.iter().map(|&i| &*w.u.arena[i].with).collect();
        search_with(&refs, &z_name)
    };
    let Some((level, i_cte)) = found else {
        return 0; // Sem casamento.
    };
    let node = nodes[level]; // o `pWith` do casamento

    // Se `pCte->zCteErr` não é nulo, esta é uma referência recursiva ilegal à CTE.
    let (z_cte_err, cte_name, cte_cols, cte_p_use, cte_m10d) = {
        let cte = &w.u.arena[node].with.a[i_cte];
        (
            cte.z_cte_err,
            cte.z_name.clone().unwrap_or_default(),
            cte.p_cols.clone(),
            cte.p_use,
            cte.e_m10d,
        )
    };
    if let Some(z_err) = z_cte_err {
        error_msg(w.u.db, w.u.parse, z_err.as_bytes(), &[text(&cte_name)]);
        return 2;
    }
    if cannot_be_function(w.u.db, w.u.parse, p_from) != 0 {
        return 2;
    }

    debug_assert!(p_from.p_tab.is_none());
    let mut tab = Table::default();
    let use_id = match cte_p_use {
        Some(id) => id,
        None => {
            let id = CteUseId(w.u.parse.cte_uses.len() as u32);
            w.u.parse.cte_uses.push(CteUse { e_m10d: cte_m10d, ..CteUse::default() });
            w.u.arena[node].with.a[i_cte].p_use = Some(id);
            id
        }
    };
    tab.z_name = cte_name.clone();
    tab.i_p_key = -1;
    tab.n_row_log_est = 200; // 200 == sqlite3LogEst(1048576)
    tab.tab_flags |= TF_EPHEMERAL | TF_NO_VISIBLE_ROWID;
    p_from.p_select = {
        let cte = &w.u.arena[node].with.a[i_cte];
        select_dup(cte.p_select.as_deref(), 0)
    };
    match p_from.p_select.as_deref_mut() {
        Some(s) => s.sel_flags |= SF_COPYCTE,
        None => return 2,
    }
    if p_from.fg.is_indexed_by {
        let z_idx = match &p_from.u1 {
            SrcU1::IndexedBy(Some(z)) => z.clone(),
            _ => Vec::new(),
        };
        error_msg(w.u.db, w.u.parse, b"no such index: \"%s\"", &[text(&z_idx)]);
        return 2;
    }
    p_from.fg.is_cte = true;
    p_from.u2 = SrcU2::CteUse(Some(use_id));
    w.u.parse.cte_uses[use_id.0 as usize].n_use += 1;

    // As referências recursivas ganham a tabela provisória (sem colunas) e, depois que as colunas
    // da CTE são conhecidas, a definitiva (ver a nota do módulo).
    let placeholder: Rc<Table> = Rc::new(tab.clone());
    let mut rec_items: Vec<(usize, usize)> = Vec::new();

    // Confere se é uma CTE recursiva.
    let sel_op = p_from.p_select.as_deref().map_or(0, |s| s.op);
    let b_may_recursive = sel_op == TK_ALL || sel_op == TK_UNION;
    let mut rec_depth: usize = 0; // pRecTerm = nth_prior(pSel, rec_depth)
    let mut i_rec_tab: i32 = -1;
    {
        let p_sel = p_from.p_select.as_deref_mut().expect("pSelect");
        while b_may_recursive && nth_prior_mut(p_sel, rec_depth).op == sel_op {
            let p_rec_term = nth_prior_mut(p_sel, rec_depth);
            debug_assert!(p_rec_term.p_prior.is_some());
            let n_item = p_rec_term.p_src.as_deref().map_or(0, |s| s.a.len());
            for i in 0..n_item {
                let hit = {
                    let item = &p_rec_term.p_src.as_deref().unwrap().a[i];
                    item.z_database.is_none()
                        && item.z_name.is_some()
                        && str_icmp(item.z_name.as_deref().unwrap(), &cte_name) == 0
                };
                if hit {
                    {
                        let item = &mut p_rec_term.p_src.as_deref_mut().unwrap().a[i];
                        item.p_tab = Some(Rc::clone(&placeholder));
                        item.fg.is_recursive = true;
                    }
                    if (p_rec_term.sel_flags & SF_RECURSIVE) != 0 {
                        error_msg(
                            w.u.db,
                            w.u.parse,
                            b"multiple references to recursive table: %s",
                            &[text(&cte_name)],
                        );
                        return 2;
                    }
                    p_rec_term.sel_flags |= SF_RECURSIVE;
                    if i_rec_tab < 0 {
                        i_rec_tab = w.u.parse.n_tab;
                        w.u.parse.n_tab += 1;
                    }
                    p_rec_term.p_src.as_deref_mut().unwrap().a[i].i_cursor = i_rec_tab;
                    rec_items.push((rec_depth, i));
                }
            }
            if (p_rec_term.sel_flags & SF_RECURSIVE) == 0 {
                break;
            }
            rec_depth += 1;
        }
    }

    w.u.arena[node].with.a[i_cte].z_cte_err = Some("circular reference: %s");
    let saved_cur = w.u.cur; // pSavedWith
    let saved_marks = w.u.chain_pushed.len();
    w.u.cur = Some(node); // pParse->pWith = pWith
    let root_recursive = (p_from.p_select.as_deref().map_or(0, |s| s.sel_flags) & SF_RECURSIVE) != 0;
    if root_recursive {
        // Percorre só os termos não recursivos (a âncora), que começam em pRecTerm, levando junto
        // o WITH do select raiz.
        let p_sel = p_from.p_select.as_deref_mut().expect("pSelect");
        debug_assert!(rec_depth > 0);
        let with_of_root = p_sel.p_with.clone();
        let rc = {
            let p_rec_term = nth_prior_mut(p_sel, rec_depth);
            debug_assert!((p_rec_term.sel_flags & SF_RECURSIVE) == 0);
            debug_assert!(p_rec_term.has_next);
            debug_assert!(p_rec_term.p_with.is_none());
            p_rec_term.p_with = with_of_root;
            w.u.force_chain_start = true;
            let rc = walk_select(w, Some(&mut *p_rec_term));
            w.u.force_chain_start = false;
            p_rec_term.p_with = None;
            rc
        };
        if rc != 0 {
            w.u.cur = saved_cur;
            w.u.chain_pushed.truncate(saved_marks);
            return 2;
        }
    } else if walk_select(w, p_from.p_select.as_deref_mut()) != 0 {
        w.u.cur = saved_cur;
        w.u.chain_pushed.truncate(saved_marks);
        return 2;
    }
    w.u.cur = Some(node);
    w.u.chain_pushed.truncate(saved_marks);

    // `pEList` do select mais à esquerda, ou os nomes de colunas da própria CTE.
    let mut left_list: Option<Box<ExprList>> = {
        let mut p_left = p_from.p_select.as_deref().expect("pSelect");
        while let Some(prior) = p_left.p_prior.as_deref() {
            p_left = prior;
        }
        p_left.p_e_list.clone()
    };
    if let Some(cols) = cte_cols {
        let n_left = left_list.as_deref().map(|l| l.a.len());
        if let Some(n_left) = n_left {
            if n_left != cols.a.len() {
                error_msg(
                    w.u.db,
                    w.u.parse,
                    b"table %s has %d values for %d columns",
                    &[
                        text(&cte_name),
                        PrintfArg::Int(n_left as i64),
                        PrintfArg::Int(cols.a.len() as i64),
                    ],
                );
                w.u.cur = saved_cur;
                return 2;
            }
        }
        left_list = Some(cols);
    }

    columns_from_expr_list(w.u.db, w.u.parse, left_list.as_deref(), &mut tab.n_col, &mut tab.a_col);
    let p_tab_final: Rc<Table> = Rc::new(tab);
    for (depth, idx) in rec_items {
        let p_sel = p_from.p_select.as_deref_mut().expect("pSelect");
        let rec = nth_prior_mut(p_sel, depth);
        if let Some(src) = rec.p_src.as_deref_mut() {
            src.a[idx].p_tab = Some(Rc::clone(&p_tab_final));
        }
    }
    p_from.p_tab = Some(Rc::clone(&p_tab_final));
    if b_may_recursive {
        let root_flags = p_from.p_select.as_deref().map_or(0, |s| s.sel_flags);
        w.u.arena[node].with.a[i_cte].z_cte_err = Some(if (root_flags & SF_RECURSIVE) != 0 {
            "multiple recursive references: %s"
        } else {
            "recursive reference in a subquery: %s"
        });
        walk_select(w, p_from.p_select.as_deref_mut());
    }
    w.u.arena[node].with.a[i_cte].z_cte_err = None;
    w.u.cur = saved_cur;
    w.u.chain_pushed.truncate(saved_marks);
    1 // Sucesso
}

/// `sqlite3SelectPopWith`: se o SELECT tem uma cláusula WITH associada, tira-a da pilha do
/// analisador. É o `xSelectCallback2` de `sqlite3SelectExpand()`.
fn select_pop_with(w: &mut Walker<ExpandCtx<'_>>, p: &mut Select) {
    if p.p_prior.is_none() {
        // O fim da cadeia: consome o "empilhou ou não" do início dela.
        if w.u.chain_pushed.pop() == Some(true) {
            if let Some(c) = w.u.cur {
                w.u.cur = w.u.arena[c].outer; // pParse->pWith = pWith->pOuter
            }
        }
    }
}

/// `sqlite3ExpandSubquery`: `p_from` representa uma subconsulta da cláusula FROM. Aloca e preenche
/// `p_from.p_tab`. Devolve `SQLITE_OK`, ou `SQLITE_ERROR` se há erro no `Parse`.
pub fn expand_subquery(db: &mut Connection, parse: &mut Parse, p_from: &mut SrcItem) -> i32 {
    debug_assert!(p_from.p_select.is_some());
    let mut tab = Table::default();
    tab.z_name = match &p_from.z_alias {
        Some(a) => a.clone(),
        None => mprintf(b"%!S", &[src_item_arg(p_from)]).unwrap_or_default(),
    };
    {
        let mut p_sel = p_from.p_select.as_deref().expect("pSelect");
        while let Some(prior) = p_sel.p_prior.as_deref() {
            p_sel = prior;
        }
        columns_from_expr_list(db, parse, p_sel.p_e_list.as_deref(), &mut tab.n_col, &mut tab.a_col);
    }
    tab.i_p_key = -1;
    tab.e_tab_type = TABTYP_VIEW;
    tab.n_row_log_est = 200; // 200 == sqlite3LogEst(1048576)
    // O caso usual: não se permite ROWID numa subconsulta.
    tab.tab_flags |= TF_EPHEMERAL | TF_NO_VISIBLE_ROWID;
    p_from.p_tab = Some(Rc::new(tab));
    if parse.n_err != 0 {
        SQLITE_ERROR
    } else {
        SQLITE_OK
    }
}

/// `inAnyUsingClause`: confere os `n` itens à direita de `p_src.a[i_base]`. Se algum tem um USING
/// que contém `z_name`, devolve verdadeiro. Se `n` é zero, ou nenhum USING contém `z_name`, falso.
fn in_any_using_clause(z_name: &[u8], p_src: &SrcList, i_base: usize, n: usize) -> bool {
    for k in 1..=n {
        let item = &p_src.a[i_base + k];
        if !item.fg.is_using {
            continue;
        }
        let Some(using) = item.p_using() else {
            continue;
        };
        if id_list_index(using, z_name) >= 0 {
            return true;
        }
    }
    false
}

/// `selectExpander`: callback do walker que "expande" um SELECT. Expandir é: (1) garantir que todo
/// elemento do FROM tem um cursor do VDBE; (2) preencher `pTabList->a[].pTab`, e nas views e CTEs
/// `pSelect` com uma cópia do SELECT que as implementa; (3) acrescentar ao WHERE os termos do
/// NATURAL, ON e USING; (4) expandir cada "*" e "TABELA.*" do resultado em todas as colunas.
fn select_expander(w: &mut Walker<ExpandCtx<'_>>, p: &mut Select) -> i32 {
    let sel_flags = p.sel_flags;
    let mut elist_flags: u32 = 0;
    let force_start = std::mem::take(&mut w.u.force_chain_start);

    p.sel_flags |= SF_EXPANDED;
    if w.u.db.malloc_failed != 0 {
        return WRC_ABORT;
    }
    debug_assert!(p.p_src.is_some());
    if (sel_flags & SF_EXPANDED) != 0 {
        return WRC_PRUNE;
    }
    if w.e_code != 0 {
        // Renumera selId porque foi copiado de uma view.
        w.u.parse.n_select += 1;
        p.sel_id = w.u.parse.n_select as u32;
    }
    if w.u.cur.is_some() && (p.sel_flags & SF_VIEW) != 0 {
        p.p_with.get_or_insert_with(Default::default).b_view = 1;
    }
    // sqlite3WithPush(pParse, p->pWith, 0): só o início da cadeia empilha (ver a nota do módulo).
    if !p.has_next || force_start {
        let mut pushed = false;
        if w.u.parse.n_err == 0 {
            if let Some(with) = p.p_with.as_deref() {
                let outer = w.u.cur;
                w.u.arena.push(WithNode { with: Box::new(with.clone()), outer });
                w.u.cur = Some(w.u.arena.len() - 1);
                pushed = true;
            }
        }
        w.u.chain_pushed.push(pushed);
    }

    // Garante que todas as entradas do FROM têm número de cursor.
    src_list_assign_cursors(w.u.parse, p.p_src.as_deref_mut().expect("pSrc"));

    // Procura cada tabela nomeada no FROM. Se a entrada é uma subconsulta, cria uma tabela
    // transitória que a descreve.
    let n_src = p.p_src.as_deref().map_or(0, |s| s.a.len());
    for i in 0..n_src {
        let p_from: &mut SrcItem = &mut p.p_src.as_deref_mut().unwrap().a[i];
        debug_assert!(!p_from.fg.is_recursive || p_from.p_tab.is_some());
        if p_from.p_tab.is_some() {
            continue;
        }
        debug_assert!(!p_from.fg.is_recursive);
        if p_from.z_name.is_none() {
            // Uma subconsulta no FROM de um SELECT.
            debug_assert!(p_from.p_select.is_some());
            if walk_select(w, p_from.p_select.as_deref_mut()) != 0 {
                return WRC_ABORT;
            }
            if expand_subquery(w.u.db, w.u.parse, p_from) != 0 {
                return WRC_ABORT;
            }
        } else {
            let rc = resolve_from_term_to_cte(w, p_from);
            if rc != 0 {
                if rc > 1 {
                    return WRC_ABORT;
                }
                debug_assert!(p_from.p_tab.is_some());
            } else {
                // Um nome comum de tabela ou view no FROM.
                debug_assert!(p_from.p_tab.is_none());
                let Some(mut p_tab) = locate_table_item(w.u.db, w.u.parse, 0, p_from) else {
                    return WRC_ABORT;
                };
                // O `nTabRef>=0xffff` do C: a contagem do `Rc` já inclui o esquema e este handle.
                if Rc::strong_count(&p_tab) > 0xffff {
                    error_msg(
                        w.u.db,
                        w.u.parse,
                        b"too many references to \"%s\": max 65535",
                        &[text(&p_tab.z_name)],
                    );
                    p_from.p_tab = None;
                    return WRC_ABORT;
                }
                p_from.p_tab = Some(Rc::clone(&p_tab));
                if !p_tab.is_virtual() && cannot_be_function(w.u.db, w.u.parse, p_from) != 0 {
                    return WRC_ABORT;
                }
                if !p_tab.is_ordinary_table() {
                    let e_code_orig = w.e_code;
                    if view_get_column_names(w.u.db, w.u.parse, &mut p_tab) != 0 {
                        return WRC_ABORT;
                    }
                    debug_assert!(p_from.p_select.is_none());
                    if p_tab.is_view() {
                        let temp_schema = w.u.db.dbs.get(1).map(|d| d.schema.id);
                        if (w.u.db.flags & SQLITE_ENABLE_VIEW) == 0
                            && Some(p_tab.p_schema) != temp_schema
                        {
                            error_msg(
                                w.u.db,
                                w.u.parse,
                                b"access to view \"%s\" prohibited",
                                &[text(&p_tab.z_name)],
                            );
                        }
                        p_from.p_select =
                            select_dup(p_tab.u_view().and_then(|v| v.p_select.as_deref()), 0);
                    } else if p_tab.is_virtual() && p_from.fg.from_ddl {
                        let risk = get_vtable(w.u.db, &p_tab)
                            .and_then(|id| w.u.db.vtabs.get(id.slot()))
                            .map(|vt| vt.e_vtab_risk);
                        let trusted = ((w.u.db.flags & SQLITE_TRUSTED_SCHEMA) != 0) as u8;
                        if risk.map_or(false, |r| r > trusted) {
                            error_msg(
                                w.u.db,
                                w.u.parse,
                                b"unsafe use of virtual table \"%s\"",
                                &[text(&p_tab.z_name)],
                            );
                        }
                    }
                    // `pTab->nCol = -1` marca a view como "em resolução" (detecta view circular);
                    // vale na tabela do esquema, que as buscas seguintes enxergam.
                    let n_col = p_tab.n_col;
                    if p_from.p_select.is_some() {
                        let i_db = schema_to_index(w.u.db, p_tab.p_schema);
                        Rc::make_mut(&mut p_tab).n_col = -1;
                        publish_view(w.u.db, i_db, &p_tab);
                        w.e_code = 1; // Liga a renumeração de Select.selId
                        walk_select(w, p_from.p_select.as_deref_mut());
                        w.e_code = e_code_orig;
                        Rc::make_mut(&mut p_tab).n_col = n_col;
                        publish_view(w.u.db, i_db, &p_tab);
                    }
                    p_from.p_tab = Some(Rc::clone(&p_tab));
                }
            }
        }

        // Localiza o índice nomeado pelo INDEXED BY, se houver.
        if p_from.fg.is_indexed_by && indexed_by_lookup(w.u.db, w.u.parse, p_from) != 0 {
            return WRC_ABORT;
        }
    }

    // Processa NATURAL, e as cláusulas ON e USING das junções.
    if w.u.parse.n_err != 0 || process_join(w.u.db, w.u.parse, p) != 0 {
        return WRC_ABORT;
    }

    // Para cada "*" da lista de colunas, insere os nomes de todas as colunas de todas as tabelas, e
    // para cada TABELA.* os nomes das colunas de TABELA. O primeiro laço só confere se existe algum
    // "*" por expandir.
    let n_expr = p.p_e_list.as_deref().map_or(0, |l| l.a.len());
    let mut k = 0usize;
    while k < n_expr {
        let p_e = p.p_e_list.as_deref().unwrap().a[k].p_expr.as_deref().expect("pExpr");
        if p_e.op == TK_ASTERISK {
            break;
        }
        debug_assert!(p_e.op != TK_DOT || p_e.p_right.is_some());
        debug_assert!(
            p_e.op != TK_DOT
                || (p_e.p_left.as_deref().map_or(false, |l| l.op == TK_ID))
        );
        if p_e.op == TK_DOT && p_e.p_right.as_deref().map_or(false, |r| r.op == TK_ASTERISK) {
            break;
        }
        elist_flags |= p_e.flags;
        k += 1;
    }
    if k < n_expr {
        // O resultado tem um ou mais "*" a expandir: percorre cada expressão e expande uma a uma.
        let old: Box<ExprList> = p.p_e_list.take().expect("pEList");
        let mut p_new: Option<Box<ExprList>> = None;
        let flags = w.u.db.flags;
        let long_names = (flags & SQLITE_FULL_COL_NAMES) != 0 && (flags & SQLITE_SHORT_COL_NAMES) == 0;
        let db: &mut Connection = &mut *w.u.db;
        let parse: &mut Parse = &mut *w.u.parse;

        for item in old.a.into_iter() {
            let ExprListItem { p_expr: p_e_box, z_e_name, fg, .. } = item;
            let p_e: Box<Expr> = p_e_box.expect("pExpr");
            elist_flags |= p_e.flags;
            let p_right_is_star =
                p_e.p_right.as_deref().map_or(false, |r| r.op == TK_ASTERISK);
            debug_assert!(p_e.op != TK_DOT || p_e.p_right.is_some());
            if p_e.op != TK_ASTERISK && (p_e.op != TK_DOT || !p_right_is_star) {
                // Esta expressão não precisa ser expandida.
                p_new = expr_list_append(p_new, Some(p_e));
                if let Some(last) = p_new.as_deref_mut().and_then(|l| l.a.last_mut()) {
                    last.z_e_name = z_e_name;
                    last.fg.e_e_name = fg.e_e_name;
                }
                continue;
            }

            // Esta expressão é "*" ou "TABELA.*" e precisa ser expandida.
            let mut table_seen = false; // 1 quando TABELA casa
            let z_t_name: Option<Vec<u8>>; // texto do nome de TABELA
            let i_err_ofst: i32;
            if p_e.op == TK_DOT {
                debug_assert!((sel_flags & SF_NESTEDFROM) == 0);
                let left = p_e.p_left.as_deref().expect("pLeft");
                debug_assert!(!left.has_property(EP_INT_VALUE));
                z_t_name = Some(left.z_token().map(|z| z.to_vec()).unwrap_or_default());
                i_err_ofst = p_e.p_right.as_deref().map_or(0, |r| r.i_ofst());
            } else {
                z_t_name = None;
                i_err_ofst = p_e.i_ofst();
            }
            let p_src: &SrcList = p.p_src.as_deref().expect("pSrc");
            let n_src = p_src.a.len();
            for i in 0..n_src {
                let p_from = &p_src.a[i];
                let p_tab: &Rc<Table> = p_from.p_tab.as_ref().expect("pTab");
                let z_tab_name: &[u8] = match &p_from.z_alias {
                    Some(a) => a,
                    None => &p_tab.z_name,
                };
                if db.malloc_failed != 0 {
                    break;
                }
                debug_assert!(p_from.fg.is_nested_from == Select::is_nested_from(p_from.p_select.as_deref()));
                let p_nested_from: Option<&ExprList>;
                let mut z_schema_name: Option<Vec<u8>> = None; // nome do esquema desta fonte
                if p_from.fg.is_nested_from {
                    let nf = p_from
                        .p_select
                        .as_deref()
                        .and_then(|s| s.p_e_list.as_deref())
                        .expect("pNestedFrom");
                    debug_assert!(nf.a.len() == p_tab.n_col as usize);
                    p_nested_from = Some(nf);
                } else {
                    if let Some(zt) = &z_t_name {
                        if str_icmp(zt, z_tab_name) != 0 {
                            continue;
                        }
                    }
                    p_nested_from = None;
                    let i_db = schema_to_index(db, p_tab.p_schema);
                    z_schema_name = Some(if i_db >= 0 {
                        db.dbs[i_db as usize].z_db_s_name.clone()
                    } else {
                        b"*".to_vec()
                    });
                }
                // USING de pFrom[1]
                let mut p_using: Option<&IdList> = None;
                if i + 1 < n_src
                    && p_src.a[i + 1].fg.is_using
                    && (sel_flags & SF_NESTEDFROM) != 0
                {
                    let using = p_src.a[i + 1].p_using().expect("pUsing");
                    for ii in 0..using.a.len() {
                        let z_uname: Vec<u8> = using.a[ii].z_name.clone().unwrap_or_default();
                        let mut p_right = expr(TK_ID as i32, Some(&z_uname));
                        expr_set_error_offset(p_right.as_deref_mut(), i_err_ofst);
                        p_new = expr_list_append(p_new, p_right);
                        if let Some(px) = p_new.as_deref_mut().and_then(|l| l.a.last_mut()) {
                            debug_assert!(px.z_e_name.is_none());
                            px.z_e_name = mprintf(b"..%s", &[text(&z_uname)]);
                            px.fg.e_e_name = ENAME_TAB;
                            px.fg.b_using_term = true;
                        }
                    }
                    p_using = Some(using);
                }

                let n_col = p_tab.n_col as usize;
                let mut n_add = n_col; // número de colunas, com o rowid
                if p_tab.visible_rowid() && (sel_flags & SF_NESTEDFROM) != 0 {
                    n_add += 1;
                }
                for j in 0..n_add {
                    let z_name: Vec<u8>;
                    if j == n_col {
                        match rowid_alias(p_tab) {
                            Some(z) => z_name = z.to_vec(),
                            None => continue,
                        }
                    } else {
                        let col = &p_tab.a_col[j];
                        z_name = column_name(col).to_vec();

                        // Se pTab é uma subconsulta SF_NestedFrom, não expande as colunas
                        // ENAME_ROWID.
                        if let Some(nf) = p_nested_from {
                            if nf.a[j].fg.e_e_name == ENAME_ROWID {
                                continue;
                            }
                        }
                        if let (Some(zt), Some(nf)) = (&z_t_name, p_nested_from) {
                            if match_e_name(&nf.a[j], None, Some(zt), None, None) == 0 {
                                continue;
                            }
                        }

                        // Uma coluna "hidden" fica fora da lista expandida, a menos que o SELECT
                        // tenha o bit SF_IncludeHidden.
                        if (p.sel_flags & SF_INCLUDEHIDDEN) == 0 && col.is_hidden() {
                            continue;
                        }
                        if (col.col_flags & COLFLAG_NOEXPAND) != 0
                            && z_t_name.is_none()
                            && (sel_flags & SF_NESTEDFROM) == 0
                        {
                            continue;
                        }
                    }
                    table_seen = true;

                    if i > 0 && z_t_name.is_none() && (sel_flags & SF_NESTEDFROM) == 0 {
                        if p_from.fg.is_using
                            && p_from.p_using().map_or(false, |u| id_list_index(u, &z_name) >= 0)
                        {
                            // Numa junção com USING, omite as colunas do USING da tabela da
                            // direita.
                            continue;
                        }
                    }
                    let p_right = expr(TK_ID as i32, Some(&z_name));
                    let p_expr_new: Option<Box<Expr>>;
                    if (n_src > 1
                        && ((p_from.fg.jointype & JT_LTORJ) == 0
                            || (sel_flags & SF_NESTEDFROM) != 0
                            || !in_any_using_clause(&z_name, p_src, i, n_src - i - 1)))
                        || parse.in_rename_object()
                    {
                        let p_left = expr(TK_ID as i32, Some(z_tab_name));
                        let left_addr = expr_addr(p_left.as_deref());
                        let mut px = p_expr(db, parse, TK_DOT as i32, p_left, p_right);
                        if parse.in_rename_object() && p_e.p_left.is_some() {
                            rename_token_remap(parse, left_addr, expr_addr(p_e.p_left.as_deref()));
                        }
                        if let Some(zs) = &z_schema_name {
                            let p_left = expr(TK_ID as i32, Some(zs));
                            px = p_expr(db, parse, TK_DOT as i32, p_left, px);
                        }
                        p_expr_new = px;
                    } else {
                        p_expr_new = p_right;
                    }
                    let mut p_expr_new = p_expr_new;
                    expr_set_error_offset(p_expr_new.as_deref_mut(), i_err_ofst);
                    p_new = expr_list_append(p_new, p_expr_new);
                    let px = p_new.as_deref_mut().and_then(|l| l.a.last_mut()).expect("pNew");
                    debug_assert!(px.z_e_name.is_none());
                    if (sel_flags & SF_NESTEDFROM) != 0 && !parse.in_rename_object() {
                        if let Some(nf) = p_nested_from {
                            debug_assert!(j < nf.a.len());
                            px.z_e_name = nf.a[j].z_e_name.clone();
                        } else {
                            px.z_e_name = mprintf(
                                b"%s.%s.%s",
                                &[
                                    text(z_schema_name.as_deref().unwrap_or(b"")),
                                    text(z_tab_name),
                                    text(&z_name),
                                ],
                            );
                        }
                        px.fg.e_e_name = if j == n_col { ENAME_ROWID } else { ENAME_TAB };
                        if (p_from.fg.is_using
                            && p_from.p_using().map_or(false, |u| id_list_index(u, &z_name) >= 0))
                            || p_using.map_or(false, |u| id_list_index(u, &z_name) >= 0)
                            || (j < n_col && (p_tab.a_col[j].col_flags & COLFLAG_NOEXPAND) != 0)
                        {
                            px.fg.b_no_expand = true;
                        }
                    } else if long_names {
                        px.z_e_name = mprintf(b"%s.%s", &[text(z_tab_name), text(&z_name)]);
                        px.fg.e_e_name = ENAME_NAME;
                    } else {
                        px.z_e_name = Some(z_name.clone());
                        px.fg.e_e_name = ENAME_NAME;
                    }
                }
            }
            if !table_seen {
                if let Some(zt) = &z_t_name {
                    error_msg(db, parse, b"no such table: %s", &[text(zt)]);
                } else {
                    error_msg(db, parse, b"no tables specified", &[]);
                }
            }
        }
        p.p_e_list = p_new;
    }
    if let Some(el) = p.p_e_list.as_deref() {
        if el.a.len() as i32 > w.u.db.a_limit[SQLITE_LIMIT_COLUMN as usize] {
            error_msg(w.u.db, w.u.parse, b"too many columns in result set", &[]);
            return WRC_ABORT;
        }
        if (elist_flags & (EP_HAS_FUNC | EP_SUBQUERY)) != 0 {
            p.sel_flags |= SF_COMPLEXRESULT;
        }
    }
    WRC_CONTINUE
}

/// `sqlite3SelectExpand`: "expande" um SELECT e todas as suas subconsultas (ver `select_expander`).
/// É o primeiro passo do processamento e vem antes da resolução de nomes. Se algo dá errado, a
/// mensagem fica em `parse` (`parse.n_err`).
fn select_expand(db: &mut Connection, parse: &mut Parse, p_select: &mut Select) {
    let m_rename: u16 = if parse.in_rename_object() { WALKER_FLAG_IN_RENAME } else { 0 };
    if parse.has_compound != 0 {
        let mut w = walker_new(ConvertCtx { db: &mut *db, parse: &mut *parse }, m_rename);
        w.x_expr_callback = Some(expr_walk_noop::<ConvertCtx<'_>>);
        w.x_select_callback = Some(convert_compound_select_to_subquery);
        w.x_select_callback2 = None;
        walk_select(&mut w, Some(&mut *p_select));
    }
    // O WITH que o chamador deixou em `Parse.p_with` é o nó-base da árvore de WITH.
    let base = parse.p_with.take();
    let had_base = base.is_some();
    let mut ctx = ExpandCtx {
        db: &mut *db,
        parse: &mut *parse,
        arena: Vec::new(),
        cur: None,
        chain_pushed: Vec::new(),
        force_chain_start: false,
    };
    if let Some(b) = base {
        ctx.arena.push(WithNode { with: b, outer: None });
        ctx.cur = Some(0);
    }
    let mut w = walker_new(ctx, m_rename | WALKER_FLAG_POP_WITH);
    w.x_expr_callback = Some(expr_walk_noop::<ExpandCtx<'_>>);
    w.x_select_callback = Some(select_expander);
    w.x_select_callback2 = Some(select_pop_with);
    w.e_code = 0;
    walk_select(&mut w, Some(&mut *p_select));
    let ExpandCtx { arena, parse: parse_back, .. } = w.u;
    if had_base {
        if let Some(first) = arena.into_iter().next() {
            parse_back.p_with = Some(first.with);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Chunk 013: informação de tipos das subconsultas, SelectPrep e a análise de agregados
// ---------------------------------------------------------------------------------------------

/// O contexto de `retarget_table`: a tabela antiga e a nova que a substitui.
struct Retarget {
    old: Rc<Table>,
    new: Rc<Table>,
}

/// Troca o `y.pTab` de uma expressão que apontava para a tabela antiga.
fn retarget_expr(w: &mut Walker<Retarget>, p_expr: &mut Expr) -> i32 {
    if p_expr.use_y_tab() {
        if let ExprY::Tab(Some(TabRef::Rc(t))) = &mut p_expr.y {
            if Rc::ptr_eq(t, &w.u.old) {
                *t = Rc::clone(&w.u.new);
            }
        }
    }
    WRC_CONTINUE
}

/// Troca o `pTab` dos itens do FROM de `p` que apontavam para a tabela antiga.
fn retarget_src(w: &mut Walker<Retarget>, p: &mut Select) -> i32 {
    if let Some(src) = p.p_src.as_deref_mut() {
        for item in src.a.iter_mut() {
            if let Some(t) = item.p_tab.as_mut() {
                if Rc::ptr_eq(t, &w.u.old) {
                    *t = Rc::clone(&w.u.new);
                }
            }
        }
    }
    WRC_CONTINUE
}

/// Faz o que o ponteiro `Table*` compartilhado do C fazia quando a tabela de uma subconsulta
/// ganha tipos depois da resolução de nomes: todo item do FROM e toda expressão de `p` (e das suas
/// subconsultas) que apontava para a tabela `old` passa a apontar para `new`.
fn retarget_table(p: &mut Select, old: &Rc<Table>, new: &Rc<Table>) {
    let mut w = walker_new(Retarget { old: Rc::clone(old), new: Rc::clone(new) }, 0);
    w.x_expr_callback = Some(retarget_expr);
    w.x_select_callback = Some(retarget_src);
    retarget_src(&mut w, p);
    walk_select_expr(&mut w, p);
    walk_select_from(&mut w, p);
}

/// `selectAddSubqueryTypeInfo`: `xSelectCallback2` de `sqlite3SelectAddTypeInfo`. Para cada
/// subconsulta do FROM, acrescenta `Column.zType`, `Column.zColl` e `Column.affinity` à tabela que
/// representa o resultado dela. A tabela foi montada por `selectExpander`, que omitiu essa
/// informação porque os identificadores ainda não estavam resolvidos; esta rotina roda depois da
/// resolução de nomes.
fn select_add_subquery_type_info(w: &mut Walker<ConvertCtx<'_>>, p: &mut Select) {
    if (p.sel_flags & SF_HASTYPEINFO) != 0 {
        return;
    }
    p.sel_flags |= SF_HASTYPEINFO;
    debug_assert!((p.sel_flags & SF_RESOLVED) != 0);
    let n_src = p.p_src.as_deref().map_or(0, |s| s.a.len());
    for i in 0..n_src {
        let swap = {
            let item = &p.p_src.as_deref().unwrap().a[i];
            let Some(old) = item.p_tab.as_ref() else {
                continue;
            };
            if (old.tab_flags & TF_EPHEMERAL) == 0 {
                continue;
            }
            // Uma subconsulta no FROM de um SELECT.
            let Some(p_sel) = item.p_select.as_deref() else {
                continue;
            };
            let mut t: Table = (**old).clone();
            subquery_column_types(w.u.db, w.u.parse, &mut t, p_sel, SQLITE_AFF_NONE);
            (Rc::clone(old), Rc::new(t))
        };
        retarget_table(p, &swap.0, &swap.1);
    }
}

/// `sqlite3SelectAddTypeInfo`: acrescenta tipo de dado e colação às tabelas de todas as subconsultas
/// do FROM de `p_select`. Use depois da resolução de nomes.
fn select_add_type_info(db: &mut Connection, parse: &mut Parse, p_select: &mut Select) {
    let m_rename: u16 = if parse.in_rename_object() { WALKER_FLAG_IN_RENAME } else { 0 };
    let mut w = walker_new(ConvertCtx { db, parse }, m_rename);
    w.x_select_callback = Some(select_walk_noop::<ConvertCtx<'_>>);
    w.x_select_callback2 = Some(select_add_subquery_type_info);
    w.x_expr_callback = Some(expr_walk_noop::<ConvertCtx<'_>>);
    walk_select(&mut w, Some(p_select));
}

/// `sqlite3SelectPrep`: prepara um SELECT para o processamento. Atribui cursores a todos os termos
/// do FROM, cria `Table` efêmeras para as subconsultas, desloca ON e USING para o WHERE, expande
/// "*" e "TABELA.*" e casa os identificadores das expressões com as tabelas. Age recursivamente em
/// todas as subconsultas.
pub fn select_prep(
    db: &mut Connection,
    parse: &mut Parse,
    p: &mut Select,
    p_outer_nc: Option<&mut NameContext<'_>>,
) {
    if db.malloc_failed != 0 {
        return;
    }
    if (p.sel_flags & SF_HASTYPEINFO) != 0 {
        return;
    }
    select_expand(db, parse, p);
    if parse.n_err != 0 {
        return;
    }
    resolve_select_names(db, parse, p, p_outer_nc);
    if parse.n_err != 0 {
        return;
    }
    select_add_type_info(db, parse, p);
}

/// O `NameContext` com que se analisa o `AggInfo` (o `sNC` de `sqlite3Select`).
fn agg_name_context(p_src_list: Option<&mut SrcList>, agg_id: AggInfoId) -> NameContext<'_> {
    NameContext {
        p_src_list,
        p_win_defn: Vec::new(),
        n_win_linked: None,
        u_nc: NcU::AggInfo(agg_id),
        p_next: None,
        n_ref: 0,
        n_nc_err: 0,
        nc_flags: NC_UAGGINFO,
        n_nested_select: 0,
    }
}

/// `analyzeAggFuncArgs`: analisa os argumentos das funções agregadas. Cria novas entradas
/// `aCol[]` para as colunas que são argumentos de agregadas mas não são usadas de outro modo.
///
/// As entradas de `aCol[]` anteriores a `nAccumulator` são colunas referenciadas fora de funções
/// agregadas (por exemplo as do GROUP BY). As de `aCol[nAccumulator]` em diante são referências a
/// colunas usadas só como argumentos de agregadas; esta rotina as calcula (ou recalcula).
fn analyze_agg_func_args(
    db: &mut Connection,
    parse: &mut Parse,
    agg_id: AggInfoId,
    nc: &mut NameContext<'_>,
) {
    let idx = agg_id.0 as usize;
    debug_assert!(parse.agg_infos[idx].i_first_reg == 0);
    nc.nc_flags |= NC_INAGGFUNC;
    let n_func = parse.agg_infos[idx].a_func.len();
    for i in 0..n_func {
        let mut p_expr = parse.agg_infos[idx].a_func[i].p_f_expr.take().expect("pFExpr");
        debug_assert!(p_expr.op == TK_FUNCTION || p_expr.op == TK_AGG_FUNCTION);
        expr_analyze_agg_list(db, parse, nc, p_expr.x_list_mut());
        if let Some(left) = p_expr.p_left.as_deref_mut() {
            debug_assert!(left.op == TK_ORDER);
            expr_analyze_agg_list(db, parse, nc, left.x_list_mut());
        }
        debug_assert!(!p_expr.is_window_func());
        if p_expr.has_property(EP_WIN_FUNC) {
            let p_filter = p_expr.y_win_mut().and_then(|win| win.p_filter.as_deref_mut());
            expr_analyze_aggregates(db, parse, nc, p_filter);
        }
        parse.agg_infos[idx].a_func[i].p_f_expr = Some(p_expr);
    }
    nc.nc_flags &= !NC_INAGGFUNC;
}

/// `optimizeAggregateUseOfIndexedExpr`: um índice sobre expressões está sendo usado no laço interno
/// de uma consulta agregada com GROUP BY. Tenta ajustar o `AggInfo` para aproveitar o índice, e
/// talvez usá-lo como índice de cobertura.
fn optimize_aggregate_use_of_indexed_expr(
    db: &mut Connection,
    parse: &mut Parse,
    p_group_by: Option<&ExprList>,
    agg_id: AggInfoId,
    nc: &mut NameContext<'_>,
) {
    let idx = agg_id.0 as usize;
    debug_assert!(parse.agg_infos[idx].i_first_reg == 0);
    debug_assert!(p_group_by.is_some());
    {
        let info = &mut parse.agg_infos[idx];
        info.a_col.truncate(info.n_accumulator as usize);
        if info.n_sorting_column > 0 {
            let mut mx = p_group_by.map_or(0, |g| g.a.len() as i32) - 1;
            for col in info.a_col.iter() {
                let k = col.i_sorter_column as i32;
                if k > mx {
                    mx = k;
                }
            }
            info.n_sorting_column = (mx + 1) as u16;
        }
    }
    analyze_agg_func_args(db, parse, agg_id, nc);
}

/// `aggregateIdxEprRefToColCallback`: callback de `aggregateConvertIndexedExprRefToColumn`.
fn aggregate_idx_epr_ref_to_col_callback(w: &mut Walker<ConvertCtx<'_>>, p_expr: &mut Expr) -> i32 {
    let Some(agg_id) = p_expr.p_agg_info else {
        return WRC_CONTINUE;
    };
    if p_expr.op == TK_AGG_COLUMN {
        return WRC_CONTINUE;
    }
    if p_expr.op == TK_AGG_FUNCTION {
        return WRC_CONTINUE;
    }
    if p_expr.op == TK_IF_NULL_ROW {
        return WRC_CONTINUE;
    }
    let info = &w.u.parse.agg_infos[agg_id.0 as usize];
    if p_expr.i_agg < 0 || p_expr.i_agg as usize >= info.a_col.len() {
        return WRC_CONTINUE;
    }
    let col = &info.a_col[p_expr.i_agg as usize];
    p_expr.op = TK_AGG_COLUMN;
    p_expr.i_table = col.i_table;
    p_expr.i_column = col.i_column as YnVar;
    p_expr.clear_property(EP_SKIP | EP_COLLATE | EP_UNLIKELY);
    WRC_PRUNE
}

/// `aggregateConvertIndexedExprRefToColumn`: converte em `TK_AGG_COLUMN` todo nó de cada
/// `pAggInfo->aFunc[].pFExpr` que tem `pAggInfo` preenchido.
fn aggregate_convert_indexed_expr_ref_to_column(
    db: &mut Connection,
    parse: &mut Parse,
    agg_id: AggInfoId,
) {
    let idx = agg_id.0 as usize;
    let n_func = parse.agg_infos[idx].a_func.len();
    let mut exprs: Vec<Option<Box<Expr>>> =
        (0..n_func).map(|i| parse.agg_infos[idx].a_func[i].p_f_expr.take()).collect();
    {
        let mut w = walker_new(ConvertCtx { db: &mut *db, parse: &mut *parse }, 0);
        w.x_expr_callback = Some(aggregate_idx_epr_ref_to_col_callback);
        for fe in exprs.iter_mut() {
            walk_expr(&mut w, fe.as_deref_mut());
        }
    }
    for (i, fe) in exprs.into_iter().enumerate() {
        parse.agg_infos[idx].a_func[i].p_f_expr = fe;
    }
}

/// `assignAggregateRegisters`: aloca um bloco de registradores, um para cada entrada de `aCol[]`
/// e de `aFunc[]`. O primeiro deles vai para `iFirstReg`. Só pode ser chamada uma vez por
/// `AggInfo`; depois dela `aCol[]` e `aFunc[]` ficam fixos e `column_reg`/`func_reg` valem.
fn assign_aggregate_registers(parse: &mut Parse, agg_id: AggInfoId) {
    let idx = agg_id.0 as usize;
    debug_assert!(parse.agg_infos[idx].i_first_reg == 0);
    parse.agg_infos[idx].i_first_reg = parse.n_mem + 1;
    let n = (parse.agg_infos[idx].a_col.len() + parse.agg_infos[idx].a_func.len()) as i32;
    parse.n_mem += n;
}

/// `resetAccumulator`: reinicia o acumulador do agregado, o conjunto de células de memória que
/// guarda os resultados intermediários. Gera o código que grava NULL em todas elas.
fn reset_accumulator(db: &mut Connection, parse: &mut Parse, agg_id: AggInfoId) {
    let idx = agg_id.0 as usize;
    let n_func = parse.agg_infos[idx].a_func.len();
    let n_reg = (n_func + parse.agg_infos[idx].a_col.len()) as i32;
    let i_first_reg = parse.agg_infos[idx].i_first_reg;
    debug_assert!(i_first_reg > 0);
    if n_reg == 0 {
        return;
    }
    if parse.n_err != 0 {
        return;
    }
    add_op3(vdbe_of_parse(parse), OP_NULL as i32, 0, i_first_reg, i_first_reg + n_reg - 1);
    for i in 0..n_func {
        let (i_distinct, i_ob_tab, b_ob_unique, b_ob_payload, b_use_subtype) = {
            let f = &parse.agg_infos[idx].a_func[i];
            (f.i_distinct, f.i_ob_tab, f.b_ob_unique != 0, f.b_ob_payload != 0, f.b_use_subtype != 0)
        };
        let z_func: Vec<u8> = parse.agg_infos[idx].a_func[i]
            .p_func
            .as_ref()
            .map(|f| f.z_name.clone())
            .unwrap_or_default();
        let p_f_expr = parse.agg_infos[idx].a_func[i].p_f_expr.take().expect("pFExpr");
        if i_distinct >= 0 {
            let n_list = p_f_expr.x_list().map(|l| l.a.len());
            if n_list != Some(1) {
                error_msg(db, parse, b"DISTINCT aggregates must have exactly one argument", &[]);
                parse.agg_infos[idx].a_func[i].i_distinct = -1;
            } else {
                let p_key_info =
                    key_info_from_expr_list(db, parse, p_f_expr.x_list().unwrap(), 0, 0);
                let addr = add_op4(
                    vdbe_of_parse(parse),
                    OP_OPENEPHEMERAL as i32,
                    i_distinct,
                    0,
                    0,
                    P4::KeyInfo(p_key_info),
                );
                parse.agg_infos[idx].a_func[i].i_dist_addr = addr;
                explain(parse, db, false, b"USE TEMP B-TREE FOR %s(DISTINCT)", &[text(&z_func)]);
            }
        }
        if i_ob_tab >= 0 {
            debug_assert!(p_f_expr.p_left.as_deref().map_or(false, |l| l.op == TK_ORDER));
            let p_ob_list: &ExprList = p_f_expr
                .p_left
                .as_deref()
                .and_then(|l| l.x_list())
                .expect("pOBList");
            let n_arg = p_f_expr.x_list().map_or(0, |l| l.a.len() as i32);
            let mut n_extra: i32 = 0;
            if !b_ob_unique {
                n_extra += 1; // Uma coluna extra para o OP_Sequence
            }
            if b_ob_payload {
                // Colunas extras para os argumentos da função
                n_extra += n_arg;
            }
            if b_use_subtype {
                n_extra += n_arg;
            }
            let mut p_key_info = key_info_from_expr_list(db, parse, p_ob_list, 0, n_extra);
            if !b_ob_unique && parse.n_err == 0 {
                Rc::make_mut(&mut p_key_info).n_key_field += 1;
            }
            add_op4(
                vdbe_of_parse(parse),
                OP_OPENEPHEMERAL as i32,
                i_ob_tab,
                p_ob_list.a.len() as i32 + n_extra,
                0,
                P4::KeyInfo(p_key_info),
            );
            explain(parse, db, false, b"USE TEMP B-TREE FOR %s(ORDER BY)", &[text(&z_func)]);
        }
        parse.agg_infos[idx].a_func[i].p_f_expr = Some(p_f_expr);
    }
}

// ---------------------------------------------------------------------------------------------
// Chunk 014: agregados, HAVING e junção da view consigo mesma
// ---------------------------------------------------------------------------------------------

/// `finalizeAggFunctions`: gera o `OP_AggFinal` de cada função agregada do `AggInfo`.
fn finalize_agg_functions(parse: &mut Parse, agg_id: AggInfoId) {
    let idx = agg_id.0 as usize;
    let n_func = parse.agg_infos[idx].a_func.len();
    for i in 0..n_func {
        let (i_ob_tab, b_ob_unique, b_ob_payload, b_use_subtype) = {
            let f = &parse.agg_infos[idx].a_func[i];
            (f.i_ob_tab, f.b_ob_unique != 0, f.b_ob_payload != 0, f.b_use_subtype != 0)
        };
        let p_func = parse.agg_infos[idx].a_func[i].p_func.clone().expect("pFunc");
        let func_reg = parse.agg_infos[idx].func_reg(i as i32);
        let p_f_expr = parse.agg_infos[idx].a_func[i].p_f_expr.take().expect("pFExpr");
        let n_list: Option<usize> = p_f_expr.x_list().map(|l| l.a.len());
        if i_ob_tab >= 0 {
            // Num agregado com ORDER BY, as chamadas a OP_AggStep foram adiadas. As entradas ficaram
            // na tabela efêmera `i_ob_tab`. Aqui são extraídas (na ordem do ORDER BY) e todas as
            // chamadas a OP_AggStep acontecem antes do OP_AggFinal.
            let n_arg = n_list.unwrap_or(0) as i32; // colunas a extrair
            let reg_agg = get_temp_range(parse, n_arg); // extrai para este vetor
            let n_key: i32 = if !b_ob_payload {
                0
            } else {
                // colunas-chave a pular
                debug_assert!(p_f_expr.p_left.is_some());
                let n_ob = p_f_expr
                    .p_left
                    .as_deref()
                    .and_then(|l| l.x_list())
                    .expect("pOBList")
                    .a
                    .len() as i32;
                if !b_ob_unique {
                    n_ob + 1
                } else {
                    n_ob
                }
            };
            let i_top = add_op1(vdbe_of_parse(parse), OP_REWIND as i32, i_ob_tab); // início do laço
            for j in (0..n_arg).rev() {
                add_op3(vdbe_of_parse(parse), OP_COLUMN as i32, i_ob_tab, n_key + j, reg_agg + j);
            }
            if b_use_subtype {
                let reg_subtype = get_temp_reg(parse);
                let i_base_col = n_key + n_arg + (!b_ob_payload && !b_ob_unique) as i32;
                for j in (0..n_arg).rev() {
                    let v = vdbe_of_parse(parse);
                    add_op3(v, OP_COLUMN as i32, i_ob_tab, i_base_col + j, reg_subtype);
                    add_op2(v, OP_SETSUBTYPE as i32, reg_subtype, reg_agg + j);
                }
                release_temp_reg(parse, reg_subtype);
            }
            {
                let v = vdbe_of_parse(parse);
                add_op3(v, OP_AGGSTEP as i32, 0, reg_agg, func_reg);
                append_p4(v, P4::FuncDef(Rc::clone(&p_func)));
                change_p5(v, (n_arg as u8) as u16);
                add_op2(v, OP_NEXT as i32, i_ob_tab, i_top + 1);
                jump_here(v, i_top);
            }
            release_temp_range(parse, reg_agg, n_arg);
        }
        {
            let v = vdbe_of_parse(parse);
            add_op2(v, OP_AGGFINAL as i32, func_reg, n_list.unwrap_or(0) as i32);
            append_p4(v, P4::FuncDef(Rc::clone(&p_func)));
        }
        parse.agg_infos[idx].a_func[i].p_f_expr = Some(p_f_expr);
    }
}

/// `updateAccumulator`: gera o código que atualiza as células de memória do acumulador de um
/// agregado conforme a posição corrente do cursor.
///
/// Se `reg_acc` não é zero e `pAggInfo` não tem min() ou max(), só preenche os `nAccumulator`
/// registradores se `reg_acc` vale 0; quem chama liga e desliga `reg_acc`.
///
/// Num agregado com ORDER BY, a atualização do acumulador fica adiada até que todas as linhas de
/// entrada cheguem, para poderem ser processadas na ordem pedida: em vez de OP_AggStep, os
/// argumentos entram na tabela efêmera de ordenação (junto da chave).
fn update_accumulator(
    db: &mut Connection,
    parse: &mut Parse,
    reg_acc: i32,
    agg_id: AggInfoId,
    e_distinct_type: u8,
) {
    let idx = agg_id.0 as usize;
    let mut reg_hit: i32 = 0;
    let mut addr_hit_test: i32 = 0;

    debug_assert!(parse.agg_infos[idx].i_first_reg > 0);
    if parse.n_err != 0 {
        return;
    }
    parse.agg_infos[idx].direct_mode = 1;
    let n_func = parse.agg_infos[idx].a_func.len();
    let n_accumulator = parse.agg_infos[idx].n_accumulator;
    for i in 0..n_func {
        let mut n_arg: i32;
        let mut addr_next: i32 = 0;
        let reg_agg: i32;
        let mut reg_agg_sz: i32 = 0;
        let mut reg_distinct: i32 = 0;
        let (i_distinct, i_ob_tab, b_ob_unique, b_ob_payload, b_use_subtype) = {
            let f = &parse.agg_infos[idx].a_func[i];
            (f.i_distinct, f.i_ob_tab, f.b_ob_unique != 0, f.b_ob_payload != 0, f.b_use_subtype != 0)
        };
        let p_func = parse.agg_infos[idx].a_func[i].p_func.clone().expect("pFunc");
        let func_reg = parse.agg_infos[idx].func_reg(i as i32);
        let mut p_f_expr = parse.agg_infos[idx].a_func[i].p_f_expr.take().expect("pFExpr");
        debug_assert!(!p_f_expr.is_window_func());
        let mut p_list: Option<Box<ExprList>> = take_x_list(&mut p_f_expr);
        let mut p_ob_list: Option<Box<ExprList>> = match p_f_expr.p_left.as_deref_mut() {
            Some(l) => take_x_list(l),
            None => None,
        };
        if p_f_expr.has_property(EP_WIN_FUNC) {
            if n_accumulator != 0
                && (p_func.func_flags & SQLITE_FUNC_NEEDCOLL) != 0
                && reg_acc != 0
            {
                // Se reg_acc==0, existe algum min() ou max() sem FILTER que garante que os
                // registradores "ímã" são preenchidos.
                if reg_hit == 0 {
                    parse.n_mem += 1;
                    reg_hit = parse.n_mem;
                }
                // Se esta é a primeira linha do grupo (reg_acc contém 0), zera o registrador
                // "ímã" reg_hit para que os registradores do acumulador sejam preenchidos se o
                // FILTER saltar por cima da chamada de min() ou max(). Se não é a primeira linha
                // (reg_acc contém 1), liga o "ímã" para que os acumuladores só sejam preenchidos
                // se min()/max() for chamada e indicar que devem ser.
                add_op2(vdbe_of_parse(parse), OP_COPY as i32, reg_acc, reg_hit);
            }
            addr_next = make_label(parse);
            let p_filter = p_f_expr
                .y_win_mut()
                .and_then(|win| win.p_filter.as_deref_mut())
                .expect("pFilter");
            expr_if_false(db, parse, p_filter, addr_next, SQLITE_JUMPIFNULL as i32, None);
        }
        if i_ob_tab >= 0 {
            // Em vez de OP_AggStep, os argumentos que iriam para ele vão para a tabela de
            // ordenação.
            let list = p_list.as_deref_mut().expect("pList");
            n_arg = list.a.len() as i32;
            debug_assert!(n_arg > 0);
            debug_assert!(p_f_expr.p_left.as_deref().map_or(false, |l| l.op == TK_ORDER));
            let ob = p_ob_list.as_deref_mut().expect("pOBList");
            debug_assert!(!ob.a.is_empty());
            reg_agg_sz = ob.a.len() as i32;
            if !b_ob_unique {
                reg_agg_sz += 1; // Um registrador para OP_Sequence
            }
            if b_ob_payload {
                reg_agg_sz += n_arg;
            }
            if b_use_subtype {
                reg_agg_sz += n_arg;
            }
            reg_agg_sz += 1; // Um registrador extra para o resultado de MakeRecord
            reg_agg = get_temp_range(parse, reg_agg_sz);
            reg_distinct = reg_agg;
            expr_code_expr_list(db, parse, ob, reg_agg, 0, SQLITE_ECEL_DUP, None);
            let mut jj = ob.a.len() as i32; // registradores usados até agora no registro
            if !b_ob_unique {
                add_op2(vdbe_of_parse(parse), OP_SEQUENCE as i32, i_ob_tab, reg_agg + jj);
                jj += 1;
            }
            if b_ob_payload {
                reg_distinct = reg_agg + jj;
                expr_code_expr_list(db, parse, list, reg_distinct, 0, SQLITE_ECEL_DUP, None);
                jj += n_arg;
            }
            if b_use_subtype {
                let reg_base = if b_ob_payload { reg_distinct } else { reg_agg };
                for kk in 0..n_arg {
                    add_op2(vdbe_of_parse(parse), OP_GETSUBTYPE as i32, reg_base + kk, reg_agg + jj);
                    jj += 1;
                }
            }
        } else if let Some(list) = p_list.as_deref_mut() {
            n_arg = list.a.len() as i32;
            reg_agg = get_temp_range(parse, n_arg);
            reg_distinct = reg_agg;
            expr_code_expr_list(db, parse, list, reg_agg, 0, SQLITE_ECEL_DUP, None);
        } else {
            n_arg = 0;
            reg_agg = 0;
        }
        if i_distinct >= 0 && p_list.is_some() {
            if addr_next == 0 {
                addr_next = make_label(parse);
            }
            let new_distinct = code_distinct(
                db,
                parse,
                e_distinct_type,
                i_distinct,
                addr_next,
                p_list.as_deref().unwrap(),
                reg_distinct,
            );
            parse.agg_infos[idx].a_func[i].i_distinct = new_distinct;
        }
        if i_ob_tab >= 0 {
            // Insere um registro novo na tabela do ORDER BY.
            let v = vdbe_of_parse(parse);
            add_op3(v, OP_MAKERECORD as i32, reg_agg, reg_agg_sz - 1, reg_agg + reg_agg_sz - 1);
            add_op4_int(
                v,
                OP_IDXINSERT as i32,
                i_ob_tab,
                reg_agg + reg_agg_sz - 1,
                reg_agg,
                reg_agg_sz - 1,
            );
            release_temp_range(parse, reg_agg, reg_agg_sz);
        } else {
            // Chama a função AggStep.
            if (p_func.func_flags & SQLITE_FUNC_NEEDCOLL) != 0 {
                let mut p_coll: Option<Rc<CollSeq>> = None;
                debug_assert!(p_list.is_some()); // pList!=0 se pF->pFunc tem NEEDCOLL
                if let Some(list) = p_list.as_deref() {
                    let mut j = 0usize;
                    while p_coll.is_none() && j < n_arg as usize {
                        p_coll = expr_coll_seq(db, parse, list.a[j].p_expr.as_deref(), None);
                        j += 1;
                    }
                }
                if p_coll.is_none() {
                    p_coll = db.p_dflt_coll.clone();
                }
                if reg_hit == 0 && n_accumulator != 0 {
                    parse.n_mem += 1;
                    reg_hit = parse.n_mem;
                }
                add_op4(vdbe_of_parse(parse), OP_COLLSEQ as i32, reg_hit, 0, 0, P4::Coll(p_coll));
            }
            {
                let v = vdbe_of_parse(parse);
                add_op3(v, OP_AGGSTEP as i32, 0, reg_agg, func_reg);
                append_p4(v, P4::FuncDef(Rc::clone(&p_func)));
                change_p5(v, (n_arg as u8) as u16);
            }
            release_temp_range(parse, reg_agg, n_arg);
        }
        if addr_next != 0 {
            resolve_label(parse, db, addr_next);
        }
        // Devolve as listas e a expressão ao `AggInfo`.
        if let Some(l) = p_f_expr.p_left.as_deref_mut() {
            put_x_list(l, p_ob_list);
        }
        put_x_list(&mut p_f_expr, p_list);
        parse.agg_infos[idx].a_func[i].p_f_expr = Some(p_f_expr);
    }
    if reg_hit == 0 && n_accumulator != 0 {
        reg_hit = reg_acc;
    }
    if reg_hit != 0 {
        addr_hit_test = add_op1(vdbe_of_parse(parse), OP_IF as i32, reg_hit);
    }
    for i in 0..n_accumulator as usize {
        let reg = parse.agg_infos[idx].column_reg(i as i32);
        let mut p_c_expr = parse.agg_infos[idx].a_col[i].p_c_expr.take();
        if let Some(e) = p_c_expr.as_deref_mut() {
            expr_code(db, parse, e, reg, None);
        }
        parse.agg_infos[idx].a_col[i].p_c_expr = p_c_expr;
    }

    parse.agg_infos[idx].direct_mode = 0;
    if addr_hit_test != 0 {
        jump_here_or_pop_inst(vdbe_of_parse(parse), addr_hit_test);
    }
}

/// `explainSimpleCount`: acrescenta um único `OP_Explain` que explica uma consulta simples
/// `SELECT count(*) FROM tabela`.
fn explain_simple_count(db: &mut Connection, parse: &mut Parse, p_tab: &Table, p_idx: Option<&Index>) {
    if parse.explain == 2 {
        let b_cover = p_idx.map_or(false, |i| p_tab.has_rowid() || !i.is_primary_key_index());
        explain(
            parse,
            db,
            false,
            b"SCAN %s%s%s",
            &[
                text(&p_tab.z_name),
                text(if b_cover { &b" USING COVERING INDEX "[..] } else { &b""[..] }),
                text(match p_idx {
                    Some(i) if b_cover => &i.z_name[..],
                    _ => &b""[..],
                }),
            ],
        );
    }
}

/// O contexto de `having_to_where_expr_cb`: o `pWalker->pParse` do C, o WHERE do select (que o
/// callback aumenta) e o GROUP BY (que ele só lê); o HAVING é o que o walker percorre.
struct HavingCtx<'a> {
    db: &'a mut Connection,
    parse: &'a mut Parse,
    p_where: &'a mut Option<Box<Expr>>,
    p_group_by: Option<&'a ExprList>,
}

/// `havingToWhereExprCb`: callback do `sqlite3WalkExpr()` de `havingToWhere()`.
///
/// Se o nó é um `TK_AND`, devolve `WRC_CONTINUE` para percorrer os filhos. Senão devolve
/// `WRC_PRUNE`; e, se a subexpressão tem os requisitos para ir ao WHERE, a acrescenta ao WHERE e a
/// troca, dentro do HAVING, pela constante "1".
fn having_to_where_expr_cb(w: &mut Walker<HavingCtx<'_>>, p_expr: &mut Expr) -> i32 {
    if p_expr.op != TK_AND {
        // Esta rotina roda antes da análise de agregados do HAVING do select corrente. Se
        // `pAggInfo` está preenchido aqui, é uma referência correlacionada a uma coluna de uma
        // consulta agregada externa, ou uma função agregada que pertence a uma consulta externa.
        // Nesse caso obscuro não move a expressão para o WHERE: isso poderia corromper o `AggInfo`
        // do select externo.
        let ctx = &mut w.u;
        let ok = expr_is_constant_or_group_by(
            &mut *ctx.db,
            &mut *ctx.parse,
            Some(&mut *p_expr),
            ctx.p_group_by,
            None,
        ) != 0
            && !p_expr.always_false()
            && p_expr.p_agg_info.is_none();
        if ok {
            if let Some(mut p_new) = expr(TK_INTEGER as i32, Some(b"1")) {
                std::mem::swap(&mut *p_new, p_expr); // SWAP(Expr, *pNew, *pExpr)
                let p_where = ctx.p_where.take();
                *ctx.p_where = expr_and(&mut *ctx.db, &mut *ctx.parse, p_where, Some(p_new));
                w.e_code = 1;
            }
        }
        return WRC_PRUNE;
    }
    WRC_CONTINUE
}

/// `havingToWhere`: transfere termos elegíveis do HAVING (processado depois do agrupamento) para o
/// WHERE (antes do agrupamento). Por exemplo
/// `SELECT * FROM <tabelas> WHERE a=? GROUP BY b HAVING b=? AND c=?` vira
/// `... WHERE a=? AND b=? GROUP BY b HAVING c=?`. Um termo é elegível se só tem constantes e
/// expressões que também são termos do GROUP BY com a colação "BINARY".
fn having_to_where(db: &mut Connection, parse: &mut Parse, p: &mut Select) {
    let m_rename: u16 = if parse.in_rename_object() { WALKER_FLAG_IN_RENAME } else { 0 };
    let ctx = HavingCtx { db, parse, p_where: &mut p.p_where, p_group_by: p.p_group_by.as_deref() };
    let mut s_walker = walker_new(ctx, m_rename);
    s_walker.x_expr_callback = Some(having_to_where_expr_cb);
    walk_expr(&mut s_walker, p.p_having.as_deref_mut());
}

/// `isSelfJoinView`: confere se o item `i_this` de `p_tab_list` é uma auto-junção de outra view.
/// Procura nos itens `i_first..i_end` (inclui `i_first`, para antes de `i_end`). Se é auto-junção,
/// devolve o índice da primeira outra instância da view; senão `None`.
fn is_self_join_view(
    p_tab_list: &SrcList,
    i_this: usize,
    i_first: usize,
    i_end: usize,
) -> Option<usize> {
    let p_this = &p_tab_list.a[i_this];
    let this_sel = p_this.p_select.as_deref().expect("pThis->pSelect");
    if (this_sel.sel_flags & SF_PUSHDOWN) != 0 {
        return None;
    }
    let mut i_first = i_first;
    while i_first < i_end {
        let i_item = i_first;
        let p_item = &p_tab_list.a[i_item];
        i_first += 1;
        let Some(s1) = p_item.p_select.as_deref() else {
            continue;
        };
        if p_item.fg.via_coroutine {
            continue;
        }
        if p_item.z_name.is_none() {
            continue;
        }
        debug_assert!(p_item.p_tab.is_some());
        debug_assert!(p_this.p_tab.is_some());
        let schema_item = p_item.p_tab.as_ref().map(|t| t.p_schema);
        let schema_this = p_this.p_tab.as_ref().map(|t| t.p_schema);
        if schema_item != schema_this {
            continue;
        }
        if stricmp(p_item.z_name.as_deref(), p_this.z_name.as_deref()) != 0 {
            continue;
        }
        if schema_item == Some(SchemaId(0)) && this_sel.sel_id != s1.sel_id {
            // O achatador deixou duas tabelas CTE diferentes de mesmo nome no mesmo FROM.
            continue;
        }
        if (s1.sel_flags & SF_PUSHDOWN) != 0 {
            // A view foi modificada por outra otimização, como pushDownWhereTerms().
            continue;
        }
        return Some(i_item);
    }
    None
}

/// O `CteUse` do item (o `pItem->u2.pCteUse` do C); um item `isCte` sempre tem um.
fn cte_use_of(parse: &Parse, p_item: &SrcItem) -> CteUse {
    match &p_item.u2 {
        SrcU2::CteUse(Some(id)) => parse.cte_uses[id.0 as usize],
        _ => CteUse::default(),
    }
}

// ---------------------------------------------------------------------------------------------
// Chunk 015: count(*) de UNION ALL, apelidos repetidos, co-rotinas
// ---------------------------------------------------------------------------------------------

/// `countOfViewOptimization`: tenta transformar uma consulta da forma
///
/// ```text
///    SELECT count(*) FROM (SELECT x FROM t1 UNION ALL SELECT y FROM t2)
/// ```
///
/// nesta:
///
/// ```text
///    SELECT (SELECT count(*) FROM t1)+(SELECT count(*) FROM t2)
/// ```
///
/// A transformação só vale se: a subconsulta é um UNION ALL de dois ou mais termos; não tem LIMIT;
/// não há WHERE, GROUP BY nem HAVING nas subconsultas; e a consulta externa é um count(*) simples
/// sem WHERE nem outra sintaxe extra. Devolve verdadeiro se a otimização foi feita.
fn count_of_view_optimization(db: &mut Connection, parse: &mut Parse, p: &mut Select) -> bool {
    if (p.sel_flags & SF_AGGREGATE) == 0 {
        return false; // É um agregado
    }
    {
        let Some(el) = p.p_e_list.as_deref() else {
            return false;
        };
        if el.a.len() != 1 {
            return false; // Uma só coluna de resultado
        }
        if p.p_where.is_some() {
            return false;
        }
        if p.p_having.is_some() {
            return false;
        }
        if p.p_group_by.is_some() {
            return false;
        }
        if p.p_order_by.is_some() {
            return false;
        }
        let p_expr = el.a[0].p_expr.as_deref().expect("pExpr");
        if p_expr.op != TK_AGG_FUNCTION {
            return false; // O resultado é um agregado
        }
        debug_assert!(p_expr.use_u_token());
        if stricmp(p_expr.z_token(), Some(b"count")) != 0 {
            return false; // É count()
        }
        debug_assert!(p_expr.use_x_list());
        if p_expr.x_list().is_some() {
            return false; // Tem de ser count(*)
        }
        let Some(src) = p.p_src.as_deref() else {
            return false;
        };
        if src.a.len() != 1 {
            return false; // Uma tabela no FROM
        }
        if p_expr.has_property(EP_WIN_FUNC) {
            return false; // Não é função de janela
        }
        let Some(p_sub) = src.a[0].p_select.as_deref() else {
            return false; // O FROM é uma subconsulta
        };
        if p_sub.p_prior.is_none() {
            return false; // Tem de ser composta
        }
        if (p_sub.sel_flags & SF_COPYCTE) != 0 {
            return false; // Não é CTE
        }
        let mut cur = Some(p_sub);
        while let Some(x) = cur {
            if x.op != TK_ALL && x.p_prior.is_some() {
                return false; // Tem de ser UNION ALL
            }
            if x.p_where.is_some() {
                return false; // Sem WHERE
            }
            if x.p_limit.is_some() {
                return false; // Sem LIMIT
            }
            if (x.sel_flags & SF_AGGREGATE) != 0 {
                return false; // Não é agregado
            }
            debug_assert!(x.p_having.is_none()); // Pelo teste anterior
            cur = x.p_prior.as_deref(); // Repete sobre o composto
        }
    }

    // Se chegou aqui, a transformação é permitida.
    let p_count: Box<Expr> = p.p_e_list.as_deref_mut().unwrap().a[0].p_expr.take().expect("pExpr");
    let mut p_count = Some(p_count);
    let mut p_sub: Option<Box<Select>> = p.p_src.as_deref_mut().unwrap().a[0].p_select.take();
    p.p_src = Some(Box::new(SrcList::default())); // sqlite3SrcListDelete + DbMallocZero
    let mut p_expr_new: Option<Box<Expr>> = None;
    while let Some(mut sub) = p_sub {
        let p_prior = sub.p_prior.take();
        sub.has_next = false;
        sub.sel_flags |= SF_AGGREGATE;
        sub.sel_flags &= !SF_COMPOUND;
        sub.n_select_row = 0;
        // O C adia o apagamento do pEList antigo (sqlite3ParserAddCleanup); aqui ele cai ao ser
        // substituído.
        let p_term = if p_prior.is_some() { expr_dup(p_count.as_deref(), 0) } else { p_count.take() };
        sub.p_e_list = expr_list_append(None, p_term);
        let mut p_term = p_expr(db, parse, TK_SELECT as i32, None, None);
        p_expr_add_select(db, parse, p_term.as_deref_mut(), Some(sub));
        p_expr_new = match p_expr_new {
            None => p_term,
            Some(prev) => p_expr(db, parse, TK_PLUS as i32, p_term, Some(prev)),
        };
        p_sub = p_prior;
    }
    p.p_e_list.as_deref_mut().unwrap().a[0].p_expr = p_expr_new;
    p.sel_flags &= !SF_AGGREGATE;
    true
}

/// `sameSrcAlias`: se algum termo de `p_src`, ou de uma subconsulta SF_NestedFrom dele, não é o
/// próprio `p0` mas tem o mesmo apelido que `p0`, devolve verdadeiro. `i_skip` é a posição de
/// `p0` em `p_src` (o `p1==p0` do C), ou `None` quando `p0` não pertence à lista.
fn same_src_alias(p0: &SrcItem, p_src: &SrcList, i_skip: Option<usize>) -> bool {
    for (i, p1) in p_src.a.iter().enumerate() {
        if Some(i) == i_skip {
            continue;
        }
        let same_tab = match (&p0.p_tab, &p1.p_tab) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if same_tab && stricmp(p0.z_alias.as_deref(), p1.z_alias.as_deref()) == 0 {
            return true;
        }
        if let Some(s) = p1.p_select.as_deref() {
            if (s.sel_flags & SF_NESTEDFROM) != 0 {
                if let Some(src2) = s.p_src.as_deref() {
                    if same_src_alias(p0, src2, None) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// `fromClauseTermCanBeCoroutine`: verdadeiro se o i-ésimo termo de `p_tab_list` (que é
/// garantidamente uma subconsulta) pode ser implementado como co-rotina.
///
/// A subconsulta é uma co-rotina se: (1) provavelmente fica no laço externo (a: é o único termo do
/// FROM; b: é o termo mais à esquerda e um CROSS JOIN ou parecido exige que seja o laço externo;
/// c: é o mais à esquerda, nada impede que `sqlite3WhereBegin()` o nomeie laço externo e a
/// consulta não é UPDATE ... FROM); (2) não é uma CTE que deva ser materializada (a: AS
/// MATERIALIZED, b: usada várias vezes sem NOT MATERIALIZED); (3) não é parte do operando esquerdo
/// de um RIGHT JOIN; (4) a otimização `Coroutines` não está desligada; (5) não é auto-junção.
fn from_clause_term_can_be_coroutine(
    db: &Connection,
    parse: &Parse,
    p_tab_list: &SrcList,
    i: usize,
    sel_flags: u32,
) -> bool {
    let p_item = &p_tab_list.a[i];
    if p_item.fg.is_cte {
        let cu = cte_use_of(parse, p_item);
        if cu.e_m10d == M10D_YES {
            return false; // (2a)
        }
        if cu.n_use >= 2 && cu.e_m10d != M10D_NO {
            return false; // (2b)
        }
    }
    if (p_tab_list.a[0].fg.jointype & JT_LTORJ) != 0 {
        return false; // (3)
    }
    if db.optimization_disabled(SQLITE_COROUTINES) {
        return false; // (4)
    }
    if is_self_join_view(p_tab_list, i, i + 1, p_tab_list.a.len()).is_some() {
        return false; // (5)
    }
    if i == 0 {
        if p_tab_list.a.len() == 1 {
            return true; // (1a)
        }
        if (p_tab_list.a[1].fg.jointype & JT_CROSS) != 0 {
            return true; // (1b)
        }
        if (sel_flags & SF_UPDATEFROM) != 0 {
            return false; // (1c-iii)
        }
        return true;
    }
    if (sel_flags & SF_UPDATEFROM) != 0 {
        return false; // (1c-iii)
    }
    let mut i = i;
    loop {
        if (p_tab_list.a[i].fg.jointype & (JT_OUTER | JT_CROSS)) != 0 {
            return false; // (1c-ii)
        }
        if i == 0 {
            break;
        }
        i -= 1;
        if p_tab_list.a[i].p_select.is_some() {
            return false; // (1c-i)
        }
    }
    true
}

/// `sqlite3WhereBegin(pParse, p->pSrc, p->pWhere, ...)` para o select `p`: o `SrcList` do select
/// sai dele durante a chamada e volta depois (ver a nota do módulo). `p_order_by` e
/// `p_result_set` são cópias ou partes que não pertencem a `p`.
fn where_begin_for_select(
    db: &mut Connection,
    parse: &mut Parse,
    p: &mut Select,
    p_order_by: Option<&mut ExprList>,
    p_result_set: Option<&mut ExprList>,
    wctrl_flags: u32,
    i_aux_arg: i32,
) -> Option<Box<WhereInfo>> {
    let mut tab = p.p_src.take().expect("pSrc");
    let mut p_where = p.p_where.take();
    let wi = where_begin(
        db,
        parse,
        &mut tab,
        p_where.as_deref_mut(),
        p_order_by,
        p_result_set,
        Some(&mut *p),
        wctrl_flags as u16,
        i_aux_arg,
    );
    p.p_where = p_where;
    p.p_src = Some(tab);
    wi
}

// ---------------------------------------------------------------------------------------------
// Chunks 015 a 018: sqlite3Select
// ---------------------------------------------------------------------------------------------

/// `sqlite3Select`: gera o código do SELECT `p`. Os resultados saem conforme `p_dest` (ver os
/// comentários de `SelectDest` em sqliteInt.h). Devolve o número de erros; se houve erros, a
/// mensagem fica em `parse.z_err_msg`.
///
/// Esta rotina NÃO libera o `Select`: quem chama cuida disso.
pub fn select(
    db: &mut Connection,
    parse: &mut Parse,
    p: &mut Select,
    p_dest: &mut SelectDest,
) -> i32 {
    get_vdbe(db, parse);
    if parse.n_err != 0 {
        return 1;
    }
    debug_assert!(db.malloc_failed == 0);
    if auth_check(db, parse, SQLITE_SELECT, None, None, None) != 0 {
        return 1;
    }

    debug_assert!(p.p_order_by.is_none() || p_dest.e_dest != SRT_DISTFIFO);
    debug_assert!(p.p_order_by.is_none() || p_dest.e_dest != SRT_FIFO);
    debug_assert!(p.p_order_by.is_none() || p_dest.e_dest != SRT_DISTQUEUE);
    debug_assert!(p.p_order_by.is_none() || p_dest.e_dest != SRT_QUEUE);
    if p_dest.ignorable_distinct() {
        debug_assert!(
            p_dest.e_dest == SRT_EXISTS
                || p_dest.e_dest == SRT_UNION
                || p_dest.e_dest == SRT_EXCEPT
                || p_dest.e_dest == SRT_DISCARD
                || p_dest.e_dest == SRT_DISTQUEUE
                || p_dest.e_dest == SRT_DISTFIFO
        );
        // Todos esses destinos também sabem ignorar a cláusula ORDER BY.
        if p.p_order_by.is_some() {
            // O C adia o apagamento (sqlite3ParserAddCleanup); aqui ele cai na hora.
            p.p_order_by = None;
        }
        p.sel_flags &= !SF_DISTINCT;
        p.sel_flags |= SF_NOOPORDERBY;
    }

    let mut p_min_max_order_by: Option<Box<ExprList>> = None; // ORDER BY acrescentado a min/max
    let rc: i32 = 'select_end: {
        select_prep(db, parse, p, None);
        if parse.n_err != 0 {
            break 'select_end 1;
        }
        debug_assert!(db.malloc_failed == 0);
        debug_assert!(p.p_e_list.is_some());

        // Se a flag SF_UFSrcCheck está ligada, esta função roda para preencher a tabela temporária
        // de um UPDATE...FROM. Aí é erro o nome ou apelido do objeto-alvo (pSrc->a[0]) aparecer de
        // novo no FROM (pSrc->a[1..n]). O Postgres também proíbe: outros sistemas tratam o caso de
        // modos diferentes, e isso confunde. Seguimos o Postgres.
        if (p.sel_flags & SF_UFSRCCHECK) != 0 {
            let src = p.p_src.as_deref().expect("pSrc");
            let p0 = &src.a[0];
            if same_src_alias(p0, src, Some(0)) {
                let z_name: Vec<u8> = match &p0.z_alias {
                    Some(a) => a.clone(),
                    None => p0.p_tab.as_ref().map(|t| t.z_name.clone()).unwrap_or_default(),
                };
                error_msg(
                    db,
                    parse,
                    b"target object/alias may not appear in FROM clause: %s",
                    &[text(&z_name)],
                );
                break 'select_end 1;
            }

            // Desliga a flag: a verificação já foi feita, e deixá-la ligada pode causar erros se um
            // composto de p->pSrc for achatado neste select e esta função rodar de novo pelo
            // processamento do SELECT composto.
            p.sel_flags &= !SF_UFSRCCHECK;
        }

        if p_dest.e_dest == SRT_OUTPUT {
            generate_column_names(db, parse, p);
        }

        if window_rewrite(db, parse, p) != 0 {
            debug_assert!(parse.n_err != 0);
            break 'select_end 1;
        }
        let is_agg = (p.sel_flags & SF_AGGREGATE) != 0; // verdadeiro em listas como "count(*)"
        let mut s_sort = SortCtx::default();
        s_sort.p_order_by = p.p_order_by.clone();

        // Tenta várias otimizações (achatar subconsultas, redução de força das junções) da cláusula
        // FROM para dentro da consulta principal.
        let mut i: i64 = 0;
        while p.p_prior.is_none() && (i as usize) < p.p_src.as_deref().unwrap().a.len() {
            'item: {
                let iu = i as usize;
                let (jointype, i_cursor) = {
                    let item = &p.p_src.as_deref().unwrap().a[iu];
                    (item.fg.jointype, item.i_cursor)
                };

                // Tenta simplificar as junções:
                //
                //      LEFT JOIN  ->  JOIN
                //     RIGHT JOIN  ->  JOIN
                //      FULL JOIN  ->  RIGHT JOIN
                //
                // Se termos da i-ésima tabela são usados no WHERE de modo que ela não possa ser a
                // linha NULL de uma junção, faz a simplificação cabível ("OUTER JOIN strength
                // reduction" na documentação do SQLite).
                if (jointype & (JT_LEFT | JT_LTORJ)) != 0
                    && expr_implies_non_null_row(
                        p.p_where.as_deref(),
                        i_cursor,
                        (jointype & JT_LTORJ) as i32,
                    ) != 0
                    && db.optimization_enabled(SQLITE_SIMPLIFY_JOIN)
                {
                    if (jointype & JT_LEFT) != 0 {
                        if (jointype & JT_RIGHT) != 0 {
                            p.p_src.as_deref_mut().unwrap().a[iu].fg.jointype &= !JT_LEFT;
                        } else {
                            p.p_src.as_deref_mut().unwrap().a[iu].fg.jointype &=
                                !(JT_LEFT | JT_OUTER);
                            unset_join_expr(p.p_where.as_deref_mut(), i_cursor, false);
                        }
                    }
                    if (p.p_src.as_deref().unwrap().a[iu].fg.jointype & JT_LTORJ) != 0 {
                        let n = p.p_src.as_deref().unwrap().a.len();
                        for j in iu + 1..n {
                            let (jt2, cur2) = {
                                let it = &p.p_src.as_deref().unwrap().a[j];
                                (it.fg.jointype, it.i_cursor)
                            };
                            if (jt2 & JT_RIGHT) != 0 {
                                if (jt2 & JT_LEFT) != 0 {
                                    p.p_src.as_deref_mut().unwrap().a[j].fg.jointype &= !JT_RIGHT;
                                } else {
                                    p.p_src.as_deref_mut().unwrap().a[j].fg.jointype &=
                                        !(JT_RIGHT | JT_OUTER);
                                    unset_join_expr(p.p_where.as_deref_mut(), cur2, true);
                                }
                            }
                        }
                        for j in (0..n).rev() {
                            let it = &mut p.p_src.as_deref_mut().unwrap().a[j];
                            it.fg.jointype &= !JT_LTORJ;
                            if (it.fg.jointype & JT_RIGHT) != 0 {
                                break;
                            }
                        }
                    }
                }

                // Sem mais nada a fazer se este termo do FROM não é uma subconsulta.
                if p.p_src.as_deref().unwrap().a[iu].p_select.is_none() {
                    break 'item;
                }

                // Pega a diferença entre as colunas declaradas de uma view e o número de colunas do
                // SELECT à direita.
                {
                    let item = &p.p_src.as_deref().unwrap().a[iu];
                    let tab = item.p_tab.as_ref().expect("pTab");
                    let n_sub = item
                        .p_select
                        .as_deref()
                        .and_then(|s| s.p_e_list.as_deref())
                        .map_or(0, |l| l.a.len()) as i64;
                    if tab.n_col as i64 != n_sub {
                        error_msg(
                            db,
                            parse,
                            b"expected %d columns for '%s' but got %d",
                            &[PrintfArg::Int(tab.n_col as i64), text(&tab.z_name), PrintfArg::Int(n_sub)],
                        );
                        break 'select_end 1;
                    }
                }

                // Não tenta as otimizações usuais (achatar e eliminar ORDER BY) numa CTE
                // MATERIALIZED, que é uma barreira de otimização.
                {
                    let item = &p.p_src.as_deref().unwrap().a[iu];
                    if item.fg.is_cte && cte_use_of(parse, item).e_m10d == M10D_YES {
                        break 'item;
                    }
                }

                // Não tenta achatar uma subconsulta agregada: só dá se a externa não é junção, e
                // nesse caso a subconsulta vira co-rotina e achatar não traz vantagem.
                {
                    let sub = p.p_src.as_deref().unwrap().a[iu].p_select.as_deref().unwrap();
                    if (sub.sel_flags & SF_AGGREGATE) != 0 {
                        break 'item;
                    }
                    debug_assert!(sub.p_group_by.is_none());
                }

                // Se a subconsulta do FROM tem um ORDER BY que não faz nada, apaga-o agora para
                // não atrapalhar o achatamento. Não se pode omitir quando: (1) há LIMIT; (2) a
                // subconsulta foi criada para ajudar o processamento de funções de janela; (3) a
                // subconsulta está no FROM de um UPDATE; (4) a externa usa agregada que não seja
                // count(), min() ou max(); (5) o ORDER BY não adiantaria nada porque: (a) a externa
                // tem outro ORDER BY, ou (b) a subconsulta faz parte de uma junção. Também mantém o
                // ORDER BY se a otimização OmitOrderBy está desligada.
                {
                    let outer_has_ob = p.p_order_by.is_some();
                    let outer_ob_reqd = (p.sel_flags & SF_ORDERBYREQD) != 0;
                    let n_src = p.p_src.as_deref().unwrap().a.len();
                    let omit_ok = db.optimization_enabled(SQLITE_OMIT_ORDER_BY);
                    let sub = p
                        .p_src
                        .as_deref_mut()
                        .unwrap()
                        .a[iu]
                        .p_select
                        .as_deref_mut()
                        .unwrap();
                    if sub.p_order_by.is_some()
                        && (outer_has_ob || n_src > 1)
                        && sub.p_limit.is_none()
                        && (sub.sel_flags & SF_ORDERBYREQD) == 0
                        && !outer_ob_reqd
                        && omit_ok
                    {
                        sub.p_order_by = None;
                    }
                }

                // Se a externa tem resultado "complexo" (usa funções ou subconsultas), e a
                // subconsulta tem ORDER BY e será uma co-rotina, não acha. Isso permite
                //
                //  SELECT expensive_function(x)
                //    FROM (SELECT x FROM tab ORDER BY y LIMIT 10);
                //
                // calcular expensive_function() só nas 10 linhas de saída.
                {
                    let list = p.p_src.as_deref().unwrap();
                    let sub = list.a[iu].p_select.as_deref().unwrap();
                    if sub.p_order_by.is_some()
                        && i == 0
                        && (p.sel_flags & SF_COMPLEXRESULT) != 0
                        && (list.a.len() == 1
                            || (list.a[1].fg.jointype & (JT_OUTER | JT_CROSS)) != 0)
                    {
                        break 'item;
                    }
                }

                if flatten_subquery(db, parse, p, iu, is_agg) != 0 {
                    if parse.n_err != 0 {
                        break 'select_end 1;
                    }
                    // Esta subconsulta pode ser absorvida pela externa.
                    i = -1;
                }
                if db.malloc_failed != 0 {
                    break 'select_end 1;
                }
                if !p_dest.ignorable_orderby() {
                    s_sort.p_order_by = p.p_order_by.clone();
                }
            }
            i += 1;
        }

        // Trata os SELECT compostos pela rotina separada multiSelect().
        if p.p_prior.is_some() {
            let rc = multi_select(db, parse, p, p_dest);
            if !p.has_next {
                explain_pop(parse);
            }
            return rc;
        }

        // Faz a otimização de propagação de constantes do WHERE se é uma junção. Não vale gastar
        // tempo em consultas sem junção: o planejador faz a otimização equivalente em
        // sqlite3WhereBegin().
        if p.p_where.as_deref().map_or(false, |wh| wh.op == TK_AND)
            && db.optimization_enabled(SQLITE_PROPAGATE_CONST)
            && propagate_constants(db, parse, p) != 0
        {
            // A propagação ajudou (só o rastreio TREETRACE do C reage a isso).
        }

        if db.optimization_enabled(SQLITE_QUERY_FLATTENER | SQLITE_COUNT_OF_VIEW)
            && count_of_view_optimization(db, parse, p)
        {
            if db.malloc_failed != 0 {
                break 'select_end 1;
            }
        }

        // Para cada termo do FROM faz duas coisas: (1) autoriza tabelas não referenciadas; (2)
        // gera o código de todas as subconsultas.
        let mut i: usize = 0;
        while i < p.p_src.as_deref().unwrap().a.len() {
            'item: {
                // Emite autorizações SQLITE_READ com um nome de coluna falso para as tabelas
                // referenciadas das quais nenhum valor é extraído. Exemplos:
                //
                //     SELECT count(*) FROM t1;   -- SQLITE_READ t1.""
                //     SELECT t1.* FROM t1, t2;   -- SQLITE_READ t2.""
                //
                // O nome falso é a string vazia (e não NULL) porque callbacks de autorização
                // antigos podem supor que o nome de coluna não é NULL.
                {
                    let item = &p.p_src.as_deref().unwrap().a[i];
                    if item.col_used == 0 && item.z_name.is_some() {
                        auth_check(
                            db,
                            parse,
                            SQLITE_READ,
                            item.z_name.as_deref(),
                            Some(&b""[..]),
                            item.z_database.as_deref(),
                        );
                    }
                }

                // Gera o código de todas as subconsultas do FROM.
                {
                    let item = &p.p_src.as_deref().unwrap().a[i];
                    if item.p_select.is_none() || item.addr_fill_sub != 0 {
                        break 'item;
                    }
                }

                // Soma a `Parse.nHeight` a altura da maior árvore de expressão deste select, o
                // pai. O filho só pode ter árvores de altura (SQLITE_MAX_EXPR_DEPTH-nHeight): mais
                // conservador que o necessário, mas bem mais simples que impor um limite exato.
                parse.n_height += select_expr_height(Some(&*p));

                // Copia termos constantes do WHERE da consulta externa para dentro da subconsulta,
                // o que pode ajudá-la a rodar melhor.
                let push_down_ok = db.optimization_enabled(SQLITE_PUSH_DOWN) && {
                    let item = &p.p_src.as_deref().unwrap().a[i];
                    !item.fg.is_cte || {
                        let cu = cte_use_of(parse, item);
                        cu.e_m10d != M10D_YES && cu.n_use < 2
                    }
                };
                if push_down_ok {
                    let mut p_sub = p.p_src.as_deref_mut().unwrap().a[i].p_select.take().unwrap();
                    let n_chng = push_down_where_terms(
                        db,
                        parse,
                        &mut p_sub,
                        p.p_where.as_deref_mut(),
                        p.p_src.as_deref().unwrap(),
                        i,
                    );
                    p.p_src.as_deref_mut().unwrap().a[i].p_select = Some(p_sub);
                    debug_assert!(
                        n_chng == 0
                            || (p.p_src.as_deref().unwrap().a[i]
                                .p_select
                                .as_deref()
                                .map_or(false, |s| (s.sel_flags & SF_PUSHDOWN) != 0))
                    );
                }

                // Converte colunas de resultado da subconsulta que não são usadas em expressões
                // NULL simples, para evitar busca e cálculo desnecessários.
                if db.optimization_enabled(SQLITE_NULL_UNUSED_COLS) {
                    disable_unused_subquery_result_columns(&mut p.p_src.as_deref_mut().unwrap().a[i]);
                }

                let z_saved_auth_context = parse.z_auth_context.clone();
                parse.z_auth_context = p.p_src.as_deref().unwrap().a[i].z_name.clone();
                let item_arg: PrintfArg = src_item_arg(&p.p_src.as_deref().unwrap().a[i]);
                let mut dest = SelectDest::default();

                // Gera o código que implementa a subconsulta.
                if from_clause_term_can_be_coroutine(
                    db,
                    parse,
                    p.p_src.as_deref().unwrap(),
                    i,
                    p.sel_flags,
                ) {
                    // Implementa uma co-rotina que devolve uma linha do resultado a cada chamada.
                    let addr_top = current_addr(parse) + 1;
                    parse.n_mem += 1;
                    let reg_return = parse.n_mem;
                    p.p_src.as_deref_mut().unwrap().a[i].reg_return = reg_return;
                    {
                        let v = vdbe_of_parse(parse);
                        add_op3(v, OP_INITCOROUTINE as i32, reg_return, 0, addr_top);
                        vdbe_comment(v, b"%!S", &[item_arg.clone()]);
                    }
                    p.p_src.as_deref_mut().unwrap().a[i].addr_fill_sub = addr_top;
                    select_dest_init(&mut dest, SRT_COROUTINE as i32, reg_return);
                    explain(parse, db, true, b"CO-ROUTINE %!S", &[item_arg.clone()]);
                    let mut p_sub = p.p_src.as_deref_mut().unwrap().a[i].p_select.take().unwrap();
                    select(db, parse, &mut p_sub, &mut dest);
                    let n_select_row = p_sub.n_select_row;
                    {
                        let item = &mut p.p_src.as_deref_mut().unwrap().a[i];
                        item.p_select = Some(p_sub);
                        Rc::make_mut(item.p_tab.as_mut().expect("pTab")).n_row_log_est = n_select_row;
                        item.fg.via_coroutine = true;
                        item.reg_result = dest.i_sdst;
                    }
                    end_coroutine(parse, reg_return);
                    jump_here(vdbe_of_parse(parse), addr_top - 1);
                    clear_temp_reg_cache(parse);
                } else {
                    let cte_materialized: Option<CteUse> = {
                        let item = &p.p_src.as_deref().unwrap().a[i];
                        if item.fg.is_cte {
                            let cu = cte_use_of(parse, item);
                            if cu.addr_m9e > 0 {
                                Some(cu)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    };
                    if let Some(cu) = cte_materialized {
                        // Esta é uma CTE cujo código de materialização já foi gerado. Chama a
                        // sub-rotina que a calcula e faz de pItem->iCursor uma cópia da tabela
                        // efêmera com o resultado da materialização.
                        add_op2(vdbe_of_parse(parse), OP_GOSUB as i32, cu.reg_rtn, cu.addr_m9e);
                        let i_cursor = p.p_src.as_deref().unwrap().a[i].i_cursor;
                        if i_cursor != cu.i_cur {
                            let v = vdbe_of_parse(parse);
                            add_op2(v, OP_OPENDUP as i32, i_cursor, cu.i_cur);
                            vdbe_comment(v, b"%!S", &[item_arg.clone()]);
                        }
                        p.p_src.as_deref_mut().unwrap().a[i]
                            .p_select
                            .as_deref_mut()
                            .unwrap()
                            .n_select_row = cu.n_row_est;
                    } else if let Some(j) = is_self_join_view(p.p_src.as_deref().unwrap(), i, 0, i) {
                        // Esta view já foi materializada por uma entrada anterior do mesmo FROM:
                        // reaproveita.
                        let (prior_fill, prior_ret, prior_cur, prior_rows) = {
                            let pr = &p.p_src.as_deref().unwrap().a[j];
                            (
                                pr.addr_fill_sub,
                                pr.reg_return,
                                pr.i_cursor,
                                pr.p_select.as_deref().map_or(0, |s| s.n_select_row),
                            )
                        };
                        if prior_fill != 0 {
                            add_op2(vdbe_of_parse(parse), OP_GOSUB as i32, prior_ret, prior_fill);
                        }
                        let i_cursor = p.p_src.as_deref().unwrap().a[i].i_cursor;
                        add_op2(vdbe_of_parse(parse), OP_OPENDUP as i32, i_cursor, prior_cur);
                        p.p_src.as_deref_mut().unwrap().a[i]
                            .p_select
                            .as_deref_mut()
                            .unwrap()
                            .n_select_row = prior_rows;
                    } else {
                        // Materializa a view. Se não é correlacionada, gera uma sub-rotina que faz
                        // a materialização para os usos seguintes da mesma view a reaproveitarem.
                        let mut once_addr: i32 = 0;

                        parse.n_mem += 1;
                        let reg_return = parse.n_mem;
                        p.p_src.as_deref_mut().unwrap().a[i].reg_return = reg_return;
                        let top_addr = add_op0(vdbe_of_parse(parse), OP_GOTO as i32);
                        {
                            let item = &mut p.p_src.as_deref_mut().unwrap().a[i];
                            item.addr_fill_sub = top_addr + 1;
                            item.fg.is_materialized = true;
                        }
                        if !p.p_src.as_deref().unwrap().a[i].fg.is_correlated {
                            // Se a subconsulta não é correlacionada, e não estamos dentro de um
                            // gatilho, o valor só precisa ser calculado uma vez.
                            let v = vdbe_of_parse(parse);
                            once_addr = add_op0(v, OP_ONCE as i32);
                            vdbe_comment(v, b"materialize %!S", &[item_arg.clone()]);
                        } else {
                            noop_comment(vdbe_of_parse(parse), b"materialize %!S", &[item_arg.clone()]);
                        }
                        let i_cursor = p.p_src.as_deref().unwrap().a[i].i_cursor;
                        select_dest_init(&mut dest, SRT_EPHEMTAB as i32, i_cursor);

                        let addr_explain =
                            explain(parse, db, true, b"MATERIALIZE %!S", &[item_arg.clone()]);
                        let mut p_sub =
                            p.p_src.as_deref_mut().unwrap().a[i].p_select.take().unwrap();
                        select(db, parse, &mut p_sub, &mut dest);
                        let n_select_row = p_sub.n_select_row;
                        p.p_src.as_deref_mut().unwrap().a[i].p_select = Some(p_sub);
                        {
                            let item = &mut p.p_src.as_deref_mut().unwrap().a[i];
                            Rc::make_mut(item.p_tab.as_mut().expect("pTab")).n_row_log_est =
                                n_select_row;
                        }
                        {
                            let v = vdbe_of_parse(parse);
                            if once_addr != 0 {
                                jump_here(v, once_addr);
                            }
                            add_op2(v, OP_RETURN as i32, reg_return, top_addr + 1);
                            vdbe_comment(v, b"end %!S", &[item_arg.clone()]);
                            scan_status_range(v, db, addr_explain, addr_explain, -1);
                            jump_here(v, top_addr);
                        }
                        clear_temp_reg_cache(parse);
                        let (is_cte, is_correlated, use_id, addr_fill_sub) = {
                            let item = &p.p_src.as_deref().unwrap().a[i];
                            let id = match &item.u2 {
                                SrcU2::CteUse(Some(id)) => Some(*id),
                                _ => None,
                            };
                            (item.fg.is_cte, item.fg.is_correlated, id, item.addr_fill_sub)
                        };
                        if is_cte && !is_correlated {
                            if let Some(id) = use_id {
                                let cu = &mut parse.cte_uses[id.0 as usize];
                                cu.addr_m9e = addr_fill_sub;
                                cu.reg_rtn = reg_return;
                                cu.i_cur = i_cursor;
                                cu.n_row_est = n_select_row;
                            }
                        }
                    }
                }
                if db.malloc_failed != 0 {
                    break 'select_end 1;
                }
                parse.n_height -= select_expr_height(Some(&*p));
                parse.z_auth_context = z_saved_auth_context;
            }
            i += 1;
        }

        // Vários elementos do SELECT copiados para variáveis locais por conveniência.
        let mut s_distinct = DistinctCtx::default(); // como codificar o DISTINCT
        s_distinct.is_tnct = ((p.sel_flags & SF_DISTINCT) != 0) as u8;

        // Se a consulta é DISTINCT com ORDER BY, não é agregada, e a lista de resultado é igual ao
        // ORDER BY, ela pode ser reescrita como GROUP BY: `SELECT DISTINCT xyz FROM ... ORDER BY
        // xyz` vira `SELECT xyz FROM ... GROUP BY xyz ORDER BY xyz`. A segunda forma é preferível
        // porque um só índice (ou tabela temporária) serve ao ORDER BY e ao DISTINCT.
        if (p.sel_flags & (SF_DISTINCT | SF_AGGREGATE)) == SF_DISTINCT
            && expr_list_compare(s_sort.p_order_by.as_deref(), p.p_e_list.as_deref(), -1) == 0
            && p.n_win_linked == 0
        {
            p.sel_flags &= !SF_DISTINCT;
            p.p_group_by = expr_list_dup(p.p_e_list.as_deref(), 0);
            p.sel_flags |= SF_AGGREGATE;
            // Embora SF_Distinct tenha saído de p->selFlags, isTnct continua ligado: ele guarda a
            // configuração original da flag SF_Distinct, não a de agora.
            debug_assert!(s_distinct.is_tnct != 0);
            s_distinct.is_tnct = 2;
        }

        // Se há ORDER BY, cria um índice efêmero para ordenar. Esse índice pode acabar sem uso se
        // os dados puderem ser extraídos já ordenados; nesse caso o OP_OpenEphemeral vira OP_Noop
        // (`addr_sort_index` serve para essa mudança).
        if let Some(ob) = s_sort.p_order_by.as_deref() {
            let n_ob = ob.a.len() as i32;
            let n_e = n_expr(p.p_e_list.as_deref());
            let p_key_info = key_info_from_expr_list(db, parse, ob, 0, n_e);
            s_sort.i_e_cursor = parse.n_tab;
            parse.n_tab += 1;
            s_sort.addr_sort_index = add_op4(
                vdbe_of_parse(parse),
                OP_OPENEPHEMERAL as i32,
                s_sort.i_e_cursor,
                n_ob + 1 + n_e,
                0,
                P4::KeyInfo(p_key_info),
            );
        } else {
            s_sort.addr_sort_index = -1;
        }

        // Se a saída vai para uma tabela temporária, abre-a.
        if p_dest.e_dest == SRT_EPHEMTAB {
            add_op2(
                vdbe_of_parse(parse),
                OP_OPENEPHEMERAL as i32,
                p_dest.i_sd_parm,
                n_expr(p.p_e_list.as_deref()),
            );
            if (p.sel_flags & SF_NESTEDFROM) != 0 {
                // Apaga ou põe NULL nas colunas de resultado que nunca serão usadas.
                let el = p.p_e_list.as_deref_mut().expect("pEList");
                while el.a.len() > 1 && !el.a[el.a.len() - 1].fg.b_used {
                    el.a.pop(); // sqlite3ExprDelete + sqlite3DbFree(zEName) + nExpr--
                }
                for item in el.a.iter_mut() {
                    if !item.fg.b_used {
                        if let Some(e) = item.p_expr.as_deref_mut() {
                            e.op = TK_NULL;
                        }
                    }
                }
            }
        }

        // Prepara o limitador.
        let i_end = make_label(parse); // endereço do fim da consulta
        if (p.sel_flags & SF_FIXEDLIMIT) == 0 {
            p.n_select_row = 320; // 4 bilhões de linhas
        }
        if p.p_limit.is_some() {
            compute_limit_registers(db, parse, p, i_end);
        }
        if p.i_limit == 0 && s_sort.addr_sort_index >= 0 {
            change_opcode(vdbe_of_parse(parse), s_sort.addr_sort_index, OP_SORTEROPEN);
            s_sort.sort_flags |= SORTFLAG_USE_SORTER;
        }

        // Abre um índice efêmero para o conjunto DISTINCT.
        if (p.sel_flags & SF_DISTINCT) != 0 {
            s_distinct.tab_tnct = parse.n_tab;
            parse.n_tab += 1;
            let p_key_info =
                key_info_from_expr_list(db, parse, p.p_e_list.as_deref().expect("pEList"), 0, 0);
            let v = vdbe_of_parse(parse);
            s_distinct.addr_tnct = add_op4(
                v,
                OP_OPENEPHEMERAL as i32,
                s_distinct.tab_tnct,
                0,
                0,
                P4::KeyInfo(p_key_info),
            );
            change_p5(v, BTREE_UNORDERED as u16);
            s_distinct.e_tnct_type = WHERE_DISTINCT_UNORDERED as u8;
        } else {
            s_distinct.e_tnct_type = WHERE_DISTINCT_NOOP as u8;
        }

        if !is_agg && p.p_group_by.is_none() {
            // Sem funções agregadas e sem GROUP BY.
            let wctrl_flags: u32 = (if s_distinct.is_tnct != 0 { WHERE_WANT_DISTINCT } else { 0 })
                | (p.sel_flags & SF_FIXEDLIMIT);
            let has_win = p.n_win_linked > 0; // objeto de janela principal (ou nenhum)
            if has_win {
                window_code_init(db, parse, p);
            }
            debug_assert!(WHERE_USE_LIMIT == SF_FIXEDLIMIT);

            // Começa a varredura do banco.
            let mut e_list_copy = p.p_e_list.clone();
            let n_row_hint = p.n_select_row as i32;
            let Some(wi) = where_begin_for_select(
                db,
                parse,
                p,
                s_sort.p_order_by.as_deref_mut(),
                e_list_copy.as_deref_mut(),
                wctrl_flags,
                n_row_hint,
            ) else {
                break 'select_end 1;
            };
            if (where_output_row_count(&wi) as i16) < p.n_select_row {
                p.n_select_row = where_output_row_count(&wi) as i16;
            }
            if s_distinct.is_tnct != 0 && where_is_distinct(&wi) != 0 {
                s_distinct.e_tnct_type = where_is_distinct(&wi) as u8;
            }
            if let Some(ob_len) = s_sort.p_order_by.as_deref().map(|l| l.a.len() as i32) {
                s_sort.n_ob_sat = where_is_ordered(&wi) as i32;
                s_sort.label_ob_lopt = where_order_by_limit_opt_label(&wi) as i32;
                if s_sort.n_ob_sat == ob_len {
                    s_sort.p_order_by = None;
                }
            }

            // Se o índice de ordenação criado antes pelo OP_OpenEphemeral acabou desnecessário,
            // troca o OP_OpenEphemeral por OP_Noop.
            if s_sort.addr_sort_index >= 0 && s_sort.p_order_by.is_none() {
                change_to_noop(vdbe_of_parse(parse), db, s_sort.addr_sort_index);
            }

            if has_win {
                let addr_gosub = make_label(parse);
                let i_cont = make_label(parse);
                let i_break = make_label(parse);
                parse.n_mem += 1;
                let reg_gosub = parse.n_mem;

                window_code_step(db, parse, p, *wi, reg_gosub, addr_gosub);

                add_op2(vdbe_of_parse(parse), OP_GOTO as i32, 0, i_break);
                resolve_label(parse, db, addr_gosub);
                noop_comment(vdbe_of_parse(parse), b"inner-loop subroutine", &[]);
                s_sort.label_ob_lopt = 0;
                select_inner_loop(
                    db,
                    parse,
                    p,
                    -1,
                    Some(&mut s_sort),
                    Some(&s_distinct),
                    p_dest,
                    i_cont,
                    i_break,
                );
                resolve_label(parse, db, i_cont);
                {
                    let v = vdbe_of_parse(parse);
                    add_op1(v, OP_RETURN as i32, reg_gosub);
                    vdbe_comment(v, b"end inner-loop subroutine", &[]);
                }
                resolve_label(parse, db, i_break);
            } else {
                // Usa o laço interno padrão.
                let i_continue = where_continue_label(&wi) as i32;
                let i_break = where_break_label(&wi) as i32;
                select_inner_loop(
                    db,
                    parse,
                    p,
                    -1,
                    Some(&mut s_sort),
                    Some(&s_distinct),
                    p_dest,
                    i_continue,
                    i_break,
                );

                // Termina o laço de varredura do banco.
                where_end(db, parse, p.p_src.as_deref().expect("pSrc"), wi);
            }
        } else {
            // Existem funções agregadas, ou GROUP BY, ou os dois.
            let mut order_by_grp = false; // verdadeiro se GROUP BY e ORDER BY são iguais

            // Remove todos os apelidos entre o resultado e o GROUP BY.
            if p.p_group_by.is_some() {
                for item in p.p_e_list.as_deref_mut().expect("pEList").a.iter_mut() {
                    item.i_alias = 0;
                }
                for item in p.p_group_by.as_deref_mut().unwrap().a.iter_mut() {
                    item.i_alias = 0;
                }
                debug_assert!(66 == log_est(100));
                if p.n_select_row > 66 {
                    p.n_select_row = 66;
                }

                // Se há GROUP BY e ORDER BY iguais, pode ser possível desligar o ORDER BY porque o
                // GROUP BY já faz os elementos saírem na ordem certa. Também pode não ser: o GROUP
                // BY pode usar um índice que agrupa como pedido mas não ordena. De qualquer modo
                // registra em `order_by_grp` que os dois são iguais.
                let gb_len = p.p_group_by.as_deref().unwrap().a.len();
                if s_sort.p_order_by.as_deref().map_or(false, |ob| ob.a.len() == gb_len) {
                    // O GROUP BY não liga se as linhas chegam em ordem ASC ou DESC, só que cada
                    // grupo saia contíguo. Então copia os ASC/DESC do ORDER BY para o GROUP BY, para
                    // maximizar a chance de as linhas virem numa ordem que torne o ORDER BY
                    // redundante.
                    {
                        let ob = s_sort.p_order_by.as_deref().unwrap();
                        let gb = p.p_group_by.as_deref_mut().unwrap();
                        for ii in 0..gb_len {
                            let sort_flags = ob.a[ii].fg.sort_flags & KEYINFO_ORDER_DESC;
                            gb.a[ii].fg.sort_flags = sort_flags;
                        }
                    }
                    if expr_list_compare(p.p_group_by.as_deref(), s_sort.p_order_by.as_deref(), -1) == 0 {
                        order_by_grp = true;
                    }
                }
            } else {
                debug_assert!(0 == log_est(1));
                p.n_select_row = 0;
            }

            // Cria um rótulo para onde saltar quando se quer abortar a consulta.
            let addr_end = make_label(parse); // fim do processamento deste SELECT

            // Converte nós TK_COLUMN em TK_AGG_COLUMN e cria entradas em `AggInfo` para todos os nós
            // TK_AGG_FUNCTION das expressões do SELECT.
            let agg_id = AggInfoId(parse.agg_infos.len() as u32);
            let idx = agg_id.0 as usize;
            let n_gb_init = p.p_group_by.as_deref().map_or(0, |g| g.a.len());
            parse.agg_infos.push(AggInfo {
                sel_id: p.sel_id,
                n_sorting_column: n_gb_init as u16,
                p_group_by: expr_list_dup(p.p_group_by.as_deref(), 0),
                ..AggInfo::default()
            });
            {
                let mut nc = agg_name_context(p.p_src.as_deref_mut(), agg_id);
                expr_analyze_agg_list(db, parse, &mut nc, p.p_e_list.as_deref_mut());
                expr_analyze_agg_list(db, parse, &mut nc, s_sort.p_order_by.as_deref_mut());
            }
            if p.p_having.is_some() {
                if p.p_group_by.is_some() {
                    having_to_where(db, parse, p);
                }
                let mut nc = agg_name_context(p.p_src.as_deref_mut(), agg_id);
                expr_analyze_aggregates(db, parse, &mut nc, p.p_having.as_deref_mut());
            }
            parse.agg_infos[idx].n_accumulator = parse.agg_infos[idx].a_col.len() as i32;
            let min_max_flag: u32 = if p.p_group_by.is_none()
                && p.p_having.is_none()
                && parse.agg_infos[idx].a_func.len() == 1
            {
                let f_expr = parse.agg_infos[idx].a_func[0].p_f_expr.as_deref().expect("pFExpr");
                min_max_query(&*db, f_expr, &mut p_min_max_order_by)
            } else {
                WHERE_ORDERBY_NORMAL
            };
            {
                let mut nc = agg_name_context(p.p_src.as_deref_mut(), agg_id);
                analyze_agg_func_args(db, parse, agg_id, &mut nc);
            }
            if db.malloc_failed != 0 {
                break 'select_end 1;
            }

            // O processamento de agregados com GROUP BY é muito diferente e bem mais complexo que
            // o dos agregados sem GROUP BY.
            if p.p_group_by.is_some() {
                let mut p_distinct: Option<Box<ExprList>> = None;
                let mut dist_flag: u32 = 0;
                #[allow(unused_assignments)]
                let mut e_dist: u8 = WHERE_DISTINCT_NOOP as u8;

                {
                    let info = &parse.agg_infos[idx];
                    if info.a_func.len() == 1
                        && info.a_func[0].i_distinct >= 0
                        && info.a_func[0]
                            .p_f_expr
                            .as_deref()
                            .and_then(|e| e.x_list())
                            .map_or(false, |l| !l.a.is_empty())
                    {
                        let p_e0 = info.a_func[0]
                            .p_f_expr
                            .as_deref()
                            .and_then(|e| e.x_list())
                            .and_then(|l| l.a[0].p_expr.as_deref());
                        let p_expr_dup = expr_dup(p_e0, 0);
                        p_distinct = expr_list_dup(p.p_group_by.as_deref(), 0);
                        p_distinct = expr_list_append(p_distinct, p_expr_dup);
                        dist_flag = if p_distinct.is_some() {
                            WHERE_WANT_DISTINCT | WHERE_AGG_DISTINCT
                        } else {
                            0
                        };
                    }
                }

                // Se há GROUP BY pode ser preciso um índice de ordenação para implementá-lo. Aloca-o
                // agora; se não for preciso, o OP_SorterOpen vira Noop.
                parse.agg_infos[idx].sorting_idx = parse.n_tab;
                parse.n_tab += 1;
                let n_column = parse.agg_infos[idx].a_col.len() as i32;
                let p_key_info =
                    key_info_from_expr_list(db, parse, p.p_group_by.as_deref().unwrap(), 0, n_column);
                let sorting_idx_cur = parse.agg_infos[idx].sorting_idx;
                let n_sorting_col = parse.agg_infos[idx].n_sorting_column as i32;
                let addr_sorting_idx = add_op4(
                    vdbe_of_parse(parse),
                    OP_SORTEROPEN as i32,
                    sorting_idx_cur,
                    n_sorting_col,
                    0,
                    P4::KeyInfo(Rc::clone(&p_key_info)),
                );

                // Inicializa as posições de memória do processamento de GROUP BY.
                let n_gb = p.p_group_by.as_deref().unwrap().a.len() as i32;
                parse.n_mem += 1;
                let i_use_flag = parse.n_mem;
                parse.n_mem += 1;
                let i_abort_flag = parse.n_mem;
                parse.n_mem += 1;
                let reg_output_row = parse.n_mem;
                let mut addr_output_row = make_label(parse);
                parse.n_mem += 1;
                let reg_reset = parse.n_mem;
                let addr_reset = make_label(parse);
                let i_a_mem = parse.n_mem + 1;
                parse.n_mem += n_gb;
                let i_b_mem = parse.n_mem + 1;
                parse.n_mem += n_gb;
                {
                    let v = vdbe_of_parse(parse);
                    add_op2(v, OP_INTEGER as i32, 0, i_abort_flag);
                    vdbe_comment(v, b"clear abort flag", &[]);
                    add_op3(v, OP_NULL as i32, 0, i_a_mem, i_a_mem + n_gb - 1);
                }

                // Começa um laço que extrai todas as linhas de origem na ordem do GROUP BY. Pode
                // ser dois laços separados com um OP_Sort no meio, ou um só laço que usa um índice
                // para extrair já na ordem certa.
                add_op2(vdbe_of_parse(parse), OP_GOSUB as i32, reg_reset, addr_reset);
                let mut group_by_copy = p.p_group_by.clone();
                let wi_opt = where_begin_for_select(
                    db,
                    parse,
                    p,
                    group_by_copy.as_deref_mut(),
                    p_distinct.as_deref_mut(),
                    (if s_distinct.is_tnct == 2 { WHERE_DISTINCTBY } else { WHERE_GROUPBY })
                        | (if order_by_grp { WHERE_SORTBYGROUP } else { 0 })
                        | dist_flag,
                    0,
                );
                let Some(wi) = wi_opt else {
                    break 'select_end 1;
                };
                let mut wi_slot: Option<Box<WhereInfo>> = Some(wi);
                if !parse.p_idx_epr.is_empty() {
                    let mut nc = agg_name_context(p.p_src.as_deref_mut(), agg_id);
                    optimize_aggregate_use_of_indexed_expr(
                        db,
                        parse,
                        p.p_group_by.as_deref(),
                        agg_id,
                        &mut nc,
                    );
                }
                assign_aggregate_registers(parse, agg_id);
                e_dist = where_is_distinct(wi_slot.as_deref().unwrap()) as u8;
                let group_by_sort: bool; // as linhas chegam da origem na ordem do GROUP BY
                let mut sort_p_tab: i32 = 0; // pseudotabela que decodifica o resultado do ordenador
                let mut sort_out: i32 = 0; // registrador de saída do ordenador
                if where_is_ordered(wi_slot.as_deref().unwrap()) as i32 == n_gb {
                    // O otimizador consegue entregar as linhas na ordem do GROUP BY, então não é
                    // preciso ordenar. O OP_OpenEphemeral é cancelado depois, porque ainda se usa o
                    // pKeyInfo.
                    group_by_sort = false;
                } else {
                    // As linhas saem em ordem indeterminada. Cada uma entra num índice de
                    // ordenação, o primeiro laço termina, e um segundo laço percorre o índice para
                    // obter a saída ordenada.
                    let addr_exp = explain(
                        parse,
                        db,
                        false,
                        b"USE TEMP B-TREE FOR %s",
                        &[text(
                            if s_distinct.is_tnct != 0 && (p.sel_flags & SF_DISTINCT) == 0 {
                                &b"DISTINCT"[..]
                            } else {
                                &b"GROUP BY"[..]
                            },
                        )],
                    );

                    group_by_sort = true;
                    let n_group_by = n_gb;
                    let mut n_col = n_group_by;
                    let mut j = n_group_by;
                    for k in 0..parse.agg_infos[idx].a_col.len() {
                        if parse.agg_infos[idx].a_col[k].i_sorter_column as i32 >= j {
                            n_col += 1;
                            j += 1;
                        }
                    }
                    let reg_base = get_temp_range(parse, n_col);
                    expr_code_expr_list(
                        db,
                        parse,
                        p.p_group_by.as_deref_mut().unwrap(),
                        reg_base,
                        0,
                        0,
                        None,
                    );
                    j = n_group_by;
                    parse.agg_infos[idx].direct_mode = 1;
                    for k in 0..parse.agg_infos[idx].a_col.len() {
                        if parse.agg_infos[idx].a_col[k].i_sorter_column as i32 >= j {
                            let mut p_c_expr = parse.agg_infos[idx].a_col[k].p_c_expr.take();
                            if let Some(e) = p_c_expr.as_deref_mut() {
                                expr_code(db, parse, e, j + reg_base, None);
                            }
                            parse.agg_infos[idx].a_col[k].p_c_expr = p_c_expr;
                            j += 1;
                        }
                    }
                    parse.agg_infos[idx].direct_mode = 0;
                    let reg_record = get_temp_reg(parse);
                    let sorting_idx = parse.agg_infos[idx].sorting_idx;
                    {
                        let v = vdbe_of_parse(parse);
                        let cur = v.n_op();
                        scan_status_counters(v, db, addr_exp, 0, cur);
                        add_op3(v, OP_MAKERECORD as i32, reg_base, n_col, reg_record);
                        add_op2(v, OP_SORTERINSERT as i32, sorting_idx, reg_record);
                        let cur = v.n_op();
                        scan_status_range(v, db, addr_exp, cur - 2, -1);
                    }
                    release_temp_reg(parse, reg_record);
                    release_temp_range(parse, reg_base, n_col);
                    if let Some(w) = wi_slot.take() {
                        where_end(db, parse, p.p_src.as_deref().expect("pSrc"), w);
                    }
                    sort_p_tab = parse.n_tab;
                    parse.n_tab += 1;
                    parse.agg_infos[idx].sorting_idx_p_tab = sort_p_tab;
                    sort_out = get_temp_reg(parse);
                    {
                        let v = vdbe_of_parse(parse);
                        let cur = v.n_op();
                        scan_status_counters(v, db, addr_exp, cur, 0);
                        add_op3(v, OP_OPENPSEUDO as i32, sort_p_tab, sort_out, n_col);
                        add_op2(v, OP_SORTERSORT as i32, sorting_idx, addr_end);
                        vdbe_comment(v, b"GROUP BY sort", &[]);
                    }
                    parse.agg_infos[idx].use_sorting_idx = 1;
                    {
                        let v = vdbe_of_parse(parse);
                        scan_status_range(v, db, addr_exp, -1, sort_p_tab);
                        scan_status_range(v, db, addr_exp, -1, sorting_idx);
                    }
                }

                // Se há entradas em `aFunc[]` com subexpressões indexadas (identificadas e marcadas
                // antes por optimizeAggregateUseOfIndexedExpr()), elas viram nós TK_AGG_COLUMN para
                // o valor sair do índice, e não ser recalculado.
                if !parse.p_idx_epr.is_empty() {
                    aggregate_convert_indexed_expr_ref_to_column(db, parse, agg_id);
                }

                // Se o índice ou a tabela temporária do GROUP BY já entrega as linhas na ordem do
                // ORDER BY, cancela a tabela efêmera aberta antes. É só uma otimização (a resposta
                // seria correta de qualquer modo); SQLITE_GroupByOrder a desliga nos testes.
                if order_by_grp
                    && db.optimization_enabled(SQLITE_GROUP_BY_ORDER)
                    && (group_by_sort
                        || wi_slot.as_deref().map_or(false, |w| where_is_sorted(w)))
                {
                    s_sort.p_order_by = None;
                    change_to_noop(vdbe_of_parse(parse), db, s_sort.addr_sort_index);
                }

                // Calcula os termos GROUP BY da linha corrente e guarda em b0, b1, b2... (b0 é a
                // posição iBMem+0, b1 é iBMem+1, ...). Compara com os termos da linha anterior,
                // guardados em a0, a1, a2...
                let addr_top_of_loop = current_addr(parse);
                let sorting_idx = parse.agg_infos[idx].sorting_idx;
                if group_by_sort {
                    add_op3(
                        vdbe_of_parse(parse),
                        OP_SORTERDATA as i32,
                        sorting_idx,
                        sort_out,
                        sort_p_tab,
                    );
                }
                for j in 0..n_gb {
                    if group_by_sort {
                        add_op3(
                            vdbe_of_parse(parse),
                            OP_COLUMN as i32,
                            sort_p_tab,
                            j,
                            i_b_mem + j,
                        );
                    } else {
                        parse.agg_infos[idx].direct_mode = 1;
                        let e = p
                            .p_group_by
                            .as_deref_mut()
                            .unwrap()
                            .a[j as usize]
                            .p_expr
                            .as_deref_mut()
                            .expect("pExpr");
                        expr_code(db, parse, e, i_b_mem + j, None);
                    }
                }
                add_op4(
                    vdbe_of_parse(parse),
                    OP_COMPARE as i32,
                    i_a_mem,
                    i_b_mem,
                    n_gb,
                    P4::KeyInfo(Rc::clone(&p_key_info)),
                );
                let addr1 = current_addr(parse); // salto da comparação A contra B
                add_op3(vdbe_of_parse(parse), OP_JUMP as i32, addr1 + 1, 0, addr1 + 1);

                // Gera o código que roda quando o GROUP BY muda. A mudança é detectada pelo bloco
                // anterior; se não houve, este é pulado. Copia os termos atuais b0,b1,b2... para
                // a0,a1,a2..., chama a sub-rotina de saída e reinicia os registradores do
                // acumulador para o próximo grupo.
                expr_code_move(parse, i_b_mem, i_a_mem, n_gb);
                {
                    let v = vdbe_of_parse(parse);
                    add_op2(v, OP_GOSUB as i32, reg_output_row, addr_output_row);
                    vdbe_comment(v, b"output one row", &[]);
                    add_op2(v, OP_IFPOS as i32, i_abort_flag, addr_end);
                    vdbe_comment(v, b"check abort flag", &[]);
                    add_op2(v, OP_GOSUB as i32, reg_reset, addr_reset);
                    vdbe_comment(v, b"reset accumulator", &[]);
                }

                // Atualiza os acumuladores do agregado conforme o conteúdo da linha corrente.
                jump_here(vdbe_of_parse(parse), addr1);
                update_accumulator(db, parse, i_use_flag, agg_id, e_dist);
                {
                    let v = vdbe_of_parse(parse);
                    add_op2(v, OP_INTEGER as i32, 1, i_use_flag);
                    vdbe_comment(v, b"indicate data in accumulator", &[]);
                }

                // Fim do laço.
                if group_by_sort {
                    add_op2(
                        vdbe_of_parse(parse),
                        OP_SORTERNEXT as i32,
                        sorting_idx,
                        addr_top_of_loop,
                    );
                } else {
                    if let Some(w) = wi_slot.take() {
                        where_end(db, parse, p.p_src.as_deref().expect("pSrc"), w);
                    }
                    change_to_noop(vdbe_of_parse(parse), db, addr_sorting_idx);
                }
                drop(p_distinct);

                // Emite a linha final do resultado.
                {
                    let v = vdbe_of_parse(parse);
                    add_op2(v, OP_GOSUB as i32, reg_output_row, addr_output_row);
                    vdbe_comment(v, b"output final row", &[]);

                    // Salta por cima das sub-rotinas.
                    vdbe_goto(v, addr_end);
                }

                // Gera uma sub-rotina que emite uma linha do resultado. Ela olha antes o
                // `i_use_flag`: se vale zero ou menos, a sub-rotina não faz nada. Se o
                // processamento pede para abortar a consulta, ela incrementa `i_abort_flag` antes
                // de voltar, sinalizando ao chamador.
                let addr_set_abort = current_addr(parse); // grava a flag de abortar e volta
                {
                    let v = vdbe_of_parse(parse);
                    add_op2(v, OP_INTEGER as i32, 1, i_abort_flag);
                    vdbe_comment(v, b"set abort flag", &[]);
                    add_op1(v, OP_RETURN as i32, reg_output_row);
                }
                resolve_label(parse, db, addr_output_row);
                addr_output_row = current_addr(parse);
                {
                    let v = vdbe_of_parse(parse);
                    add_op2(v, OP_IFPOS as i32, i_use_flag, addr_output_row + 2);
                    vdbe_comment(v, b"Groupby result generator entry point", &[]);
                    add_op1(v, OP_RETURN as i32, reg_output_row);
                }
                finalize_agg_functions(parse, agg_id);
                if let Some(h) = p.p_having.as_deref_mut() {
                    expr_if_false(db, parse, h, addr_output_row + 1, SQLITE_JUMPIFNULL as i32, None);
                }
                select_inner_loop(
                    db,
                    parse,
                    p,
                    -1,
                    Some(&mut s_sort),
                    Some(&s_distinct),
                    p_dest,
                    addr_output_row + 1,
                    addr_set_abort,
                );
                {
                    let v = vdbe_of_parse(parse);
                    add_op1(v, OP_RETURN as i32, reg_output_row);
                    vdbe_comment(v, b"end groupby result generator", &[]);
                }

                // Gera uma sub-rotina que reinicia o acumulador do GROUP BY.
                resolve_label(parse, db, addr_reset);
                reset_accumulator(db, parse, agg_id);
                {
                    let v = vdbe_of_parse(parse);
                    add_op2(v, OP_INTEGER as i32, 0, i_use_flag);
                    vdbe_comment(v, b"indicate accumulator empty", &[]);
                    add_op1(v, OP_RETURN as i32, reg_reset);
                }

                if dist_flag != 0 && e_dist != WHERE_DISTINCT_NOOP as u8 {
                    let (i_distinct, i_dist_addr) = {
                        let f = &parse.agg_infos[idx].a_func[0];
                        (f.i_distinct, f.i_dist_addr)
                    };
                    fix_distinct_open_eph(db, parse, e_dist, i_distinct, i_dist_addr);
                }
            } else {
                // Fim do ramo com GROUP BY; começam as consultas agregadas sem GROUP BY.
                let simple_tab: Option<Rc<Table>> =
                    is_simple_count(&*p, agg_id, &parse.agg_infos[idx]);
                if let Some(p_tab) = simple_tab {
                    // Se isSimpleCount() devolve uma tabela, o comando é da forma
                    //
                    //   SELECT count(*) FROM <tbl>
                    //
                    // e a tabela devolvida representa <tbl>. Este comando é tão comum que tem
                    // tratamento especial: o OP_Count roda na árvore intkey que guarda os dados da
                    // tabela, ou num dos seus índices. É melhor num índice, pois quase sempre ele
                    // ocupa menos páginas que a tabela.
                    let i_db = schema_to_index(db, p_tab.p_schema);
                    let i_csr = parse.n_tab; // cursor que percorre a árvore-b
                    parse.n_tab += 1;
                    let mut p_best: Option<Rc<Index>> = None; // melhor índice até agora
                    let mut i_root: u32 = p_tab.tnum; // página raiz da árvore percorrida

                    code_verify_schema(db, parse, i_db);
                    table_lock(db, parse, i_db, p_tab.tnum, false, &p_tab.z_name);

                    // Procura o índice de menor custo de varredura.
                    //
                    // (2011-04-15) Não faz varredura completa de índice desordenado.
                    // (2013-10-03) Não conta as entradas de índice parcial.
                    //
                    // Na prática o KeyInfo não será usado: só é passado para o OP_OpenRead ficar
                    // satisfeito.
                    if !p_tab.has_rowid() {
                        p_best = primary_key_index(&p_tab).cloned();
                    }
                    if !p.p_src.as_deref().unwrap().a[0].fg.not_indexed {
                        for p_idx in p_tab.p_index.iter() {
                            if !p_idx.b_unordered
                                && p_idx.sz_idx_row < p_tab.sz_tab_row
                                && p_idx.p_partial_idx_where.is_none()
                                && p_best.as_ref().map_or(true, |b| p_idx.sz_idx_row < b.sz_idx_row)
                            {
                                p_best = Some(Rc::clone(p_idx));
                            }
                        }
                    }
                    let mut p_key_info: Option<Rc<KeyInfo>> = None; // keyinfo do índice varrido
                    if let Some(b) = p_best.as_deref() {
                        i_root = b.tnum;
                        p_key_info = key_info_of_index(parse, db, b);
                    }

                    // Abre um cursor só de leitura, executa o OP_Count e fecha o cursor.
                    add_op4_int(
                        vdbe_of_parse(parse),
                        OP_OPENREAD as i32,
                        i_csr,
                        i_root as i32,
                        i_db,
                        1,
                    );
                    if let Some(ki) = p_key_info {
                        change_p4(vdbe_of_parse(parse), -1, P4::KeyInfo(ki));
                    }
                    assign_aggregate_registers(parse, agg_id);
                    let reg0 = parse.agg_infos[idx].func_reg(0);
                    {
                        let v = vdbe_of_parse(parse);
                        add_op2(v, OP_COUNT as i32, i_csr, reg0);
                        add_op1(v, OP_CLOSE as i32, i_csr);
                    }
                    explain_simple_count(db, parse, &p_tab, p_best.as_deref());
                } else {
                    let mut reg_acc: i32 = 0; // flag "preencher os acumuladores"
                    let mut p_distinct: Option<Box<ExprList>> = None;
                    let mut dist_flag: u32 = 0;

                    // Se há registradores de acumulador mas nenhum min() ou max() sem cláusula
                    // FILTER, aloca `reg_acc`, que vale 0 na primeira vez que o laço interno roda e
                    // 1 depois. O código de updateAccumulator() usa isso para garantir que os
                    // registradores do acumulador (a) são atualizados só uma vez se não há min() ou
                    // max(), e (b) são sempre atualizados na primeira linha visitada pelo agregado,
                    // para serem atualizados ao menos uma vez mesmo que o FILTER faça min() ou max()
                    // visitar zero linhas.
                    let n_func = parse.agg_infos[idx].a_func.len();
                    if parse.agg_infos[idx].n_accumulator != 0 {
                        let mut k = 0usize;
                        while k < n_func {
                            let f = &parse.agg_infos[idx].a_func[k];
                            if f.p_f_expr.as_deref().map_or(false, |e| e.has_property(EP_WIN_FUNC)) {
                                k += 1;
                                continue;
                            }
                            if f.p_func.as_ref().map_or(false, |fd| (fd.func_flags & SQLITE_FUNC_NEEDCOLL) != 0) {
                                break;
                            }
                            k += 1;
                        }
                        if k == n_func {
                            parse.n_mem += 1;
                            reg_acc = parse.n_mem;
                            add_op2(vdbe_of_parse(parse), OP_INTEGER as i32, 0, reg_acc);
                        }
                    } else if n_func == 1 && parse.agg_infos[idx].a_func[0].i_distinct >= 0 {
                        let list = parse.agg_infos[idx].a_func[0]
                            .p_f_expr
                            .as_deref()
                            .and_then(|e| e.x_list());
                        p_distinct = expr_list_dup(list, 0);
                        dist_flag = if p_distinct.is_some() {
                            WHERE_WANT_DISTINCT | WHERE_AGG_DISTINCT
                        } else {
                            0
                        };
                    }
                    assign_aggregate_registers(parse, agg_id);

                    // Este caso roda se o agregado não tem GROUP BY. O processamento é bem mais
                    // simples porque há uma só linha de saída.
                    debug_assert!(p.p_group_by.is_none());
                    reset_accumulator(db, parse, agg_id);

                    // Se a consulta é candidata à otimização min/max, `min_max_flag` já vale
                    // WHERE_ORDERBY_MIN ou WHERE_ORDERBY_MAX e `p_min_max_order_by` é um ORDER BY
                    // adequado.
                    debug_assert!(min_max_flag == WHERE_ORDERBY_NORMAL || p_min_max_order_by.is_some());
                    debug_assert!(p_min_max_order_by.as_deref().map_or(true, |l| l.a.len() == 1));

                    let Some(wi) = where_begin_for_select(
                        db,
                        parse,
                        p,
                        p_min_max_order_by.as_deref_mut(),
                        p_distinct.as_deref_mut(),
                        min_max_flag | dist_flag,
                        0,
                    ) else {
                        break 'select_end 1;
                    };
                    let e_dist = where_is_distinct(&wi) as u8;
                    update_accumulator(db, parse, reg_acc, agg_id, e_dist);
                    if e_dist != WHERE_DISTINCT_NOOP as u8 {
                        let first = parse.agg_infos[idx].a_func.first().map(|f| (f.i_distinct, f.i_dist_addr));
                        if let Some((i_distinct, i_dist_addr)) = first {
                            fix_distinct_open_eph(db, parse, e_dist, i_distinct, i_dist_addr);
                        }
                    }

                    if reg_acc != 0 {
                        add_op2(vdbe_of_parse(parse), OP_INTEGER as i32, 1, reg_acc);
                    }
                    if min_max_flag != 0 {
                        where_min_max_opt_early_out(vdbe_of_parse(parse), &wi);
                    }
                    where_end(db, parse, p.p_src.as_deref().expect("pSrc"), wi);
                    finalize_agg_functions(parse, agg_id);
                }

                s_sort.p_order_by = None;
                if let Some(h) = p.p_having.as_deref_mut() {
                    expr_if_false(db, parse, h, addr_end, SQLITE_JUMPIFNULL as i32, None);
                }
                select_inner_loop(db, parse, p, -1, None, None, p_dest, addr_end, addr_end);
            }
            resolve_label(parse, db, addr_end);
        } // fim do ramo da consulta agregada

        if s_distinct.e_tnct_type == WHERE_DISTINCT_UNORDERED as u8 {
            explain_temp_table(db, parse, "DISTINCT");
        }

        // Se há ORDER BY, é preciso ordenar os resultados e mandá-los ao callback um a um.
        if s_sort.p_order_by.is_some() {
            let n_column = n_expr(p.p_e_list.as_deref());
            generate_sort_tail(db, parse, &*p, &s_sort, n_column, &*p_dest);
        }

        // Salta para aqui para pular esta consulta.
        resolve_label(parse, db, i_end);

        // O SELECT foi codificado. Se há erro no Parse, o código de retorno é 1; senão 0.
        (parse.n_err > 0) as i32
    };

    // O controle salta para cá se há erro acima, ou depois da codificação com sucesso do SELECT.
    drop(p_min_max_order_by);
    explain_pop(parse);
    rc
}
