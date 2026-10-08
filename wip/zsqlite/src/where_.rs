//! Tradução de `where.c`, parte 1 (chunks `where_c.000` a `where_c.005`): consultas simples sobre
//! o `WhereInfo`, o conjunto de custos do OR, o iterador de termos (`whereScanInit`/`Next`),
//! `sqlite3WhereFindTerm`, a verificação de DISTINCT redundante, o índice automático, o filtro de
//! Bloom, a montagem do `sqlite3_index_info` das tabelas virtuais e a estimativa de varreduras de
//! intervalo.
//!
//! O modelo de dados é o de `where_int.rs` (arenas por `WhereInfo`, `ExprRef`, handles). Decisões
//! desta fatia:
//!
//! - `WhereClause*` vira `(wi, wc: ClauseId)`; `WhereTerm*` vira `TermId`; `WhereLoop*` vira
//!   `LoopId` quando o laço vive na arena de `wi`. `pWC->pWInfo->pParse` e `pParse->db` viram os
//!   parâmetros explícitos `db: &mut Connection, parse: &mut Parse`; `pWInfo->pTabList` é o
//!   parâmetro `p_tab_list`. `pLevel` é o índice `i_level` em `wi.a`.
//! - `Index` não guarda `pTable` (CONVENTIONS, item 7): onde o C usa `pIdx->pTable`, a função
//!   recebe o par `(&Rc<Table>, &Index)` (`p_idx: Option<(tabela, índice)>`).
//! - Funções `static` que o resto de `where.c` (e `wherecode.c`) chama são `pub(crate)`; as
//!   demais são privadas. Predicados que o C devolve como `int` booleano devolvem `bool`; códigos
//!   de retorno continuam `i32`.
//! - `sqlite3WhereMalloc`, `sqlite3WhereRealloc` e `freeIndexInfo` não existem: a posse e o `Drop`
//!   fazem o que faziam. `explainAutomaticIndex`, `sqlite3VdbeScanStatusCounters` e
//!   `sqlite3VdbeScanStatusRange` só agem com `SQLITE_ENABLE_STMT_SCANSTATUS`, ausente no Debian
//!   (ver `where_int.rs`), e somem. `whereKeyStats`, `whereRangeSkipScanEst`, `whereEqualScanEst`,
//!   `whereInScanEst` e `sqlite3IndexColumnAffinity` só existem com `SQLITE_ENABLE_STAT4`, e
//!   `whereTraceIndexInfo*` e `sqlite3WhereTermPrint` só com `WHERETRACE_ENABLED`: também somem.
//! - `whereLoopResize` é declarada à frente no C e definida mais adiante; fica aqui, como
//!   `where_loop_resize`, porque `constructAutomaticIndex` a usa. O restante de `where.c` deve
//!   importá-la daqui e não redefini-la.
//! - `sqlite3_index_info` é o `IndexInfo` de `connection.rs`, que não tem a cauda escondida
//!   (`HiddenIndexInfo`). Ela é a struct `HiddenIndexInfo` deste módulo, devolvida junto do
//!   `IndexInfo` por `allocate_index_info`; quem implementa `sqlite3_vtab_collation` e
//!   `sqlite3_vtab_in*` precisa dela ao lado do `IndexInfo`. Falta de memória não existe em Rust:
//!   o ramo "out of memory" de `allocateIndexInfo` some.
//! - Dependências de módulos ainda não traduzidos, com as assinaturas assumidas:
//!   `wherecode::where_explain_bloom_filter(db: &Connection, parse: &mut Parse, wi: &WhereInfo,
//!   p_tab_list: &SrcList, i_level: usize)` (`sqlite3WhereExplainBloomFilter`),
//!   `vtab::get_vtable(db: &Connection, tab: &Rc<Table>) -> Option<VTableId>` (`sqlite3GetVTable`)
//!   e `vtab::vtab_uses_all_schemas(db: &mut Connection, parse: &mut Parse)`
//!   (`sqlite3VtabUsesAllSchemas`).

// Fachada do mesmo arquivo C dividido em módulos: os chamadores importam de `crate::where_`.
pub use crate::where2::*;
pub use crate::where3::*;

use std::rc::Rc;

pub(crate) use crate::vdbeaux::{vdbe_of_parse, current_addr};
use crate::build::{column_coll, text_arg};
use crate::build2::allocate_index_object;
use crate::connection::{
    Connection, IndexConstraint, IndexConstraintUsage, IndexInfo, IndexOrderBy, Parse,
    OPFLAG_USESEEKRESULT,
};
use crate::consts::{
    Bitmask, ALLBITS, BMS, EP_FIXED_COL, EP_INNER_ON, EP_OUTER_ON, JT_LEFT, JT_LTORJ, JT_RIGHT,
    KEYINFO_ORDER_BIGNULL, KEYINFO_ORDER_DESC, N_OR_COST, OP_BLOB, OP_COLUMN, OP_COPY,
    OP_FILTERADD, OP_IDXINSERT, OP_INITCOROUTINE, OP_INTEGER, OP_NEXT, OP_ONCE,
    OP_OPENAUTOINDEX, OP_REWIND, OP_ROWID, OP_SEQUENCE, OP_YIELD, SQLITE_AFF_TEXT,
    SQLITE_BLOOM_FILTER, SQLITE_BLOOM_PULLDOWN, SQLITE_CONSTRAINT, SQLITE_ERROR, SQLITE_JUMPIFNULL,
    SQLITE_NOMEM, SQLITE_OK, SQLITE_STMTSTATUS_AUTOINDEX, SQLITE_WARNING_AUTOINDEX, TERM_OK,
    TERM_SLICE, TERM_VIRTUAL, TERM_VNULL, TK_AGG_COLUMN, TK_COLLATE, TK_COLUMN, TK_EQ, TOPBIT,
    WHERE_AUTO_INDEX, WHERE_BLOOMFILTER, WHERE_COLUMN_EQ, WHERE_COLUMN_IN, WHERE_DISTINCTBY,
    WHERE_GROUPBY, WHERE_IDX_ONLY, WHERE_INDEXED, WHERE_IPK, WHERE_PARTIALIDX,
    WHERE_SORTBYGROUP, WO_ALL, WO_AUX, WO_EQ, WO_EQUIV, WO_GE, WO_GT, WO_IN, WO_IS, WO_ISNULL,
    WO_LE, WO_LT, XN_EXPR, XN_ROWID, SQLITE_INDEX_CONSTRAINT_IS, SQLITE_INDEX_CONSTRAINT_ISNULL,
};
use crate::delete::generate_index_key;
use crate::expr::{
    expr_affinity, expr_and, expr_compare_coll_seq, expr_dup, expr_is_vector, expr_nn_coll_seq,
    expr_skip_collate_and_likely, index_affinity_ok,
};
use crate::expr_code::{
    expr_code_load_index_column, expr_is_constant, expr_is_single_table_constraint,
};
use crate::expr_code2::{
    expr_compare_skip, expr_if_false, get_temp_range, get_temp_reg, release_temp_range,
    release_temp_reg,
};
use crate::global::log;
use crate::mem::Mem;
use crate::sqlite_int::{Expr, ExprList, Index, LogEst, SrcItem, SrcList, Table};
use crate::util::{err_str, error_msg, log_est, log_est_to_int, str_icmp, STR_BINARY};
use crate::vdbe_types::Vdbe;
use crate::vdbeaux::{
    add_op0, add_op1, add_op2, add_op3, add_op4_int, change_p2, change_p5, jump_here, make_label,
    resolve_label, set_p4_key_info, vdbe_comment, vdbe_goto,
};
use crate::vtab::get_vtable;
pub(crate) use crate::where_int::{
    ClauseId, TermId, WhereInfo, WhereLoop, WhereMaskSet, WhereOrCost, WhereOrSet, WhereScan,
    WhereTerm,
};
use crate::wherecode::where_explain_bloom_filter;

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

// ---------------------------------------------------------------------------------------------
// HiddenIndexInfo
// ---------------------------------------------------------------------------------------------

/// `HiddenIndexInfo`: informação anexada ao fim do `sqlite3_index_info` e invisível para o
/// `xBestIndex`. `pParse` não existe (é parâmetro) e `pWC` é o `ClauseId`. `aRhs` tem uma entrada
/// por restrição.
#[derive(Default)]
pub struct HiddenIndexInfo {
    /// A cláusula WHERE analisada.
    pub p_wc: ClauseId,
    /// Valor devolvido por `sqlite3_vtab_distinct()`.
    pub e_distinct: i32,
    /// Máscara dos termos que são `<col> IN (...)`.
    pub m_in: u32,
    /// Termos que a tabela virtual trata como `<col> IN (...)`.
    pub m_handle_in: u32,
    /// Valores do lado direito das restrições.
    pub a_rhs: Vec<Option<Mem>>,
}

// ---------------------------------------------------------------------------------------------
// Consultas simples sobre o WhereInfo (chunk 000)
// ---------------------------------------------------------------------------------------------

/// `sqlite3WhereOutputRowCount`: o número estimado de linhas de saída de uma cláusula WHERE.
pub fn where_output_row_count(wi: &WhereInfo) -> LogEst {
    wi.n_row_out
}

/// `sqlite3WhereIsDistinct`: um dos valores `WHERE_DISTINCT_xxxxx`, que indica como a cláusula
/// WHERE devolve as saídas para o processamento do DISTINCT.
pub fn where_is_distinct(wi: &WhereInfo) -> i32 {
    wi.e_distinct as i32
}

/// `sqlite3WhereIsOrdered`: o número de termos do ORDER BY satisfeitos pela cláusula WHERE. 0
/// significa que a saída precisa ser toda ordenada; igual ao número de termos, que não precisa de
/// ordenação; positivo mas menor, que precisa de ordenação por blocos.
pub fn where_is_ordered(wi: &WhereInfo) -> i32 {
    if wi.n_ob_sat < 0 {
        0
    } else {
        wi.n_ob_sat as i32
    }
}

/// `sqlite3WhereOrderByLimitOptLabel`: na otimização ORDER BY LIMIT, se o laço mais interno emite
/// linhas em ordem crescente e a última não coube no ordenador, as demais linhas da iteração
/// corrente do laço interno podem ser puladas (também não caberiam). Devolve o rótulo para onde
/// saltar quando uma linha não cabe: a continuação do segundo laço mais interno se a otimização
/// vale, senão a do mais interno. Devolver sempre a do mais interno é seguro.
pub fn where_order_by_limit_opt_label(wi: &WhereInfo) -> i32 {
    if !wi.b_ordered_inner_loop {
        // A otimização não vale: salta para a continuação do laço mais interno.
        return wi.i_continue;
    }
    let p_inner = &wi.a[wi.n_level as usize - 1];
    debug_assert!(p_inner.addr_nxt != 0);
    if p_inner.p_rj.is_some() {
        wi.i_continue
    } else {
        p_inner.addr_nxt
    }
}

/// `sqlite3WhereMinMaxOptEarlyOut`: depois do passo agregado de `min()` ou `max()` na otimização
/// de mín/máx, codifica um `OP_Goto` que pula o processamento restante quando a ordem de saída
/// garante que a resposta certa já foi achada. É só uma otimização: a resposta sai igual sem ele.
pub fn where_min_max_opt_early_out(v: &mut Vdbe, wi: &WhereInfo) {
    if !wi.b_ordered_inner_loop {
        return;
    }
    if wi.n_ob_sat == 0 {
        return;
    }
    for i in (0..wi.n_level as usize).rev() {
        let p_inner = &wi.a[i];
        if (wi.w_loop(p_inner.p_w_loop).ws_flags & WHERE_COLUMN_IN) != 0 {
            vdbe_goto(v, p_inner.addr_nxt);
            return;
        }
    }
    vdbe_goto(v, wi.i_break);
}

/// `sqlite3WhereContinueLabel`: o endereço ou rótulo para onde saltar para continuar de imediato
/// com a próxima linha da cláusula WHERE.
pub fn where_continue_label(wi: &WhereInfo) -> i32 {
    debug_assert!(wi.i_continue != 0);
    wi.i_continue
}

/// `sqlite3WhereBreakLabel`: o endereço ou rótulo para onde saltar para sair do laço do WHERE.
pub fn where_break_label(wi: &WhereInfo) -> i32 {
    wi.i_break
}

/// `sqlite3WhereOkOnePass`: devolve `ONEPASS_OFF` (0) se um UPDATE ou DELETE não consegue operar
/// direto nos rowids que a cláusula WHERE produz, `ONEPASS_SINGLE` (1) se pode porque só uma
/// linha muda, ou `ONEPASS_MULTI` (2) se a otimização de passada única vale para várias linhas.
/// Escreve em `ai_cur` os cursores abertos para escrita usados pela otimização: `ai_cur[0]` é o
/// da tabela de dados e `ai_cur[1]` o de um índice auxiliar (qualquer um pode ser -1); os dois
/// são -1 quando a otimização não vale.
pub fn where_ok_one_pass(wi: &WhereInfo, ai_cur: &mut [i32; 2]) -> i32 {
    *ai_cur = wi.ai_cur_one_pass;
    wi.e_one_pass as i32
}

/// `sqlite3WhereUsesDeferredSeek`: verdadeiro se o laço do WHERE usa `OP_DeferredSeek` para mover
/// o cursor de dados à linha escolhida pelo cursor do índice.
pub fn where_uses_deferred_seek(wi: &WhereInfo) -> bool {
    wi.b_deferred_seek
}

// ---------------------------------------------------------------------------------------------
// WhereOrSet, máscaras
// ---------------------------------------------------------------------------------------------

/// `whereOrMove`: move o conteúdo de `p_src` para `p_dest` (só as `n` primeiras entradas).
pub(crate) fn where_or_move(p_dest: &mut WhereOrSet, p_src: &WhereOrSet) {
    p_dest.n = p_src.n;
    let n = p_dest.n as usize;
    p_dest.a[..n].copy_from_slice(&p_src.a[..n]);
}

/// `whereOrInsert`: tenta inserir uma entrada nova de pré-requisitos e custo em `p_set`. A
/// entrada nova pode sobrescrever uma existente, ser acrescentada ou ser descartada, de modo que
/// `p_set` guarde as `N_OR_COST` melhores entradas vistas até agora. Devolve 1 se a entrada
/// entrou, 0 se foi descartada.
pub(crate) fn where_or_insert(
    p_set: &mut WhereOrSet,
    prereq: Bitmask,
    r_run: LogEst,
    n_out: LogEst,
) -> i32 {
    let mut found: Option<usize> = None;
    for i in 0..p_set.n as usize {
        let p = &p_set.a[i];
        if r_run <= p.r_run && (prereq & p.prereq) == prereq {
            found = Some(i);
            break;
        }
        if p.r_run <= r_run && (p.prereq & prereq) == p.prereq {
            return 0;
        }
    }
    let idx = match found {
        Some(i) => i,
        None => {
            if (p_set.n as usize) < N_OR_COST {
                let i = p_set.n as usize;
                p_set.n += 1;
                p_set.a[i].n_out = n_out;
                i
            } else {
                let mut i_min = 0usize;
                for i in 1..p_set.n as usize {
                    if p_set.a[i_min].r_run > p_set.a[i].r_run {
                        i_min = i;
                    }
                }
                if p_set.a[i_min].r_run <= r_run {
                    return 0;
                }
                i_min
            }
        }
    };
    let p: &mut WhereOrCost = &mut p_set.a[idx];
    p.prereq = prereq;
    p.r_run = r_run;
    if p.n_out > n_out {
        p.n_out = n_out;
    }
    1
}

/// `createMask`: cria uma máscara nova para o cursor `i_cursor`. Há um cursor por tabela da
/// cláusula FROM e o número de tabelas é limitado no começo de `sqlite3WhereBegin()`, então
/// `ix[]` nunca transborda.
pub(crate) fn create_mask(p_mask_set: &mut WhereMaskSet, i_cursor: i32) {
    debug_assert!((p_mask_set.n as usize) < p_mask_set.ix.len());
    p_mask_set.ix[p_mask_set.n as usize] = i_cursor;
    p_mask_set.n += 1;
}

// ---------------------------------------------------------------------------------------------
// O iterador de termos
// ---------------------------------------------------------------------------------------------

/// `whereRightSubexprIsColumn`: se o ramo direito da expressão é um `TK_COLUMN`, devolve esse
/// ramo; senão `None`.
fn where_right_subexpr_is_column(p: &Expr) -> Option<&Expr> {
    let q = expr_skip_collate_and_likely(p.p_right.as_deref())?;
    if q.op == TK_COLUMN && !q.has_property(EP_FIXED_COL) {
        Some(q)
    } else {
        None
    }
}

/// `indexInAffinityOk`: `p_term` é um termo `WO_IN`, talvez um componente de um IN vetorial
/// `(x, y, ...) IN (SELECT ...)`. Verifica se ele é compatível com uma coluna de índice de
/// afinidade `idxaff`. Se for, devolve o nome da colação (por exemplo "BINARY" ou "NOCASE") usada
/// na comparação do termo; senão `None`. `p_x` é a expressão do termo.
fn index_in_affinity_ok(
    db: &mut Connection,
    parse: &mut Parse,
    p_term: &WhereTerm,
    p_x: &Expr,
    idxaff: u8,
) -> Option<Vec<u8>> {
    debug_assert!((p_term.e_operator & WO_IN) != 0);
    let inexpr: Option<Expr> = if p_x.p_left.as_deref().map_or(false, expr_is_vector) {
        // O `inexpr` do C: um `x = y` sintético com o par de campos do vetor.
        let i_field = (p_term.i_field - 1) as usize;
        let left = p_x.p_left.as_deref()?.x_list()?.a.get(i_field)?.p_expr.as_deref()?.clone();
        debug_assert!(p_x.use_x_select());
        let right = p_x.x_select()?.p_e_list.as_deref()?.a.get(i_field)?.p_expr.as_deref()?.clone();
        Some(Expr {
            flags: 0,
            op: TK_EQ,
            p_left: Some(Box::new(left)),
            p_right: Some(Box::new(right)),
            ..Expr::default()
        })
    } else {
        None
    };
    let p_x = inexpr.as_ref().unwrap_or(p_x);

    if index_affinity_ok(p_x, idxaff, None) {
        let p_ret = expr_compare_coll_seq(db, parse, p_x, None);
        Some(p_ret.map_or_else(|| STR_BINARY.as_bytes().to_vec(), |c| c.name.clone()))
    } else {
        None
    }
}

/// `whereScanNext`: avança para o próximo termo que casa com os critérios definidos quando o
/// `scan` foi iniciado por `where_scan_init`. Devolve `None` quando não há mais termos.
pub(crate) fn where_scan_next(
    db: &mut Connection,
    parse: &mut Parse,
    wi: &WhereInfo,
    scan: &mut WhereScan,
) -> Option<TermId> {
    let mut k = scan.k as usize; // Por onde começar a varrer
    debug_assert!(scan.i_equiv <= scan.n_equiv);
    let mut wc = scan.p_wc;
    loop {
        // A coluna e o cursor do lado esquerdo do termo (coluna -1 é o IPK).
        let i_column = scan.ai_column[scan.i_equiv as usize - 1];
        let i_cur = scan.ai_cur[scan.i_equiv as usize - 1];
        debug_assert!(i_cur >= 0);
        loop {
            for kk in k..wi.n_term(wc) {
                let tid = wi.term_at(wc, kk);
                let p_term = wi.term(tid);
                let p_expr = wi.expr(tid);
                debug_assert!(
                    (p_term.e_operator & (crate::consts::WO_OR | crate::consts::WO_AND)) == 0
                        || p_term.left_cursor < 0
                );
                if p_term.left_cursor == i_cur
                    && p_term.left_column == i_column as i32
                    && (i_column != XN_EXPR
                        || expr_compare_skip(
                            p_expr.p_left.as_deref(),
                            scan.p_idx_expr.as_deref(),
                            i_cur,
                        ) == 0)
                    && (scan.i_equiv <= 1 || !p_expr.has_property(EP_OUTER_ON))
                {
                    if (p_term.e_operator & WO_EQUIV) != 0
                        && (scan.n_equiv as usize) < scan.ai_cur.len()
                    {
                        if let Some(p_x) = where_right_subexpr_is_column(p_expr) {
                            let n_equiv = scan.n_equiv as usize;
                            let mut j = 0usize;
                            while j < n_equiv {
                                if scan.ai_cur[j] == p_x.i_table
                                    && scan.ai_column[j] as i32 == p_x.i_column
                                {
                                    break;
                                }
                                j += 1;
                            }
                            if j == n_equiv {
                                scan.ai_cur[j] = p_x.i_table;
                                scan.ai_column[j] = p_x.i_column as i16;
                                scan.n_equiv += 1;
                            }
                        }
                    }
                    if (p_term.e_operator as u32 & scan.op_mask) != 0 {
                        // Verifica se a afinidade e a sequência de colação casam.
                        if scan.z_coll_name.is_some() && (p_term.e_operator & WO_ISNULL) == 0 {
                            let z_coll_name: Vec<u8>;
                            if (p_term.e_operator & WO_IN) != 0 {
                                match index_in_affinity_ok(db, parse, p_term, p_expr, scan.idxaff)
                                {
                                    Some(z) => z_coll_name = z,
                                    None => continue,
                                }
                            } else {
                                if !index_affinity_ok(p_expr, scan.idxaff, None) {
                                    continue;
                                }
                                debug_assert!(p_expr.p_left.is_some());
                                let p_coll = expr_compare_coll_seq(db, parse, p_expr, None);
                                z_coll_name = p_coll.map_or_else(
                                    || STR_BINARY.as_bytes().to_vec(),
                                    |c| c.name.clone(),
                                );
                            }
                            if let Some(z_scan) = scan.z_coll_name.as_deref() {
                                if str_icmp(&z_coll_name, z_scan) != 0 {
                                    continue;
                                }
                            }
                        }
                        if (p_term.e_operator & (WO_EQ | WO_IS)) != 0 {
                            if let Some(p_x) = p_expr.p_right.as_deref() {
                                if p_x.op == TK_COLUMN
                                    && p_x.i_table == scan.ai_cur[0]
                                    && p_x.i_column == scan.ai_column[0] as i32
                                {
                                    continue;
                                }
                            }
                        }
                        scan.p_wc = wc;
                        scan.k = (kk + 1) as i32;
                        return Some(tid);
                    }
                }
            }
            match wi.clause(wc).p_outer {
                Some(outer) => {
                    wc = outer;
                    k = 0;
                }
                None => break,
            }
        }
        if scan.i_equiv >= scan.n_equiv {
            break;
        }
        wc = scan.p_orig_wc;
        k = 0;
        scan.i_equiv += 1;
    }
    None
}

/// `whereScanInitIndexExpr`: o `whereScanInit()` do caso de índice sobre expressão, separado
/// para o `whereScanInit()` comum, de alta frequência, não precisar empilhar registradores.
/// `p_tab` é a tabela dona do índice (dona também da árvore `p_idx_expr`).
fn where_scan_init_index_expr(
    db: &mut Connection,
    parse: &mut Parse,
    wi: &WhereInfo,
    scan: &mut WhereScan,
    p_tab: &Rc<Table>,
) -> Option<TermId> {
    scan.idxaff = scan.p_idx_expr.as_deref().map_or(0, |e| expr_affinity(e, Some(p_tab)));
    where_scan_next(db, parse, wi, scan)
}

/// `whereScanInit`: inicia o iterador de termos da cláusula `wc` e devolve o primeiro que casa
/// (`None` se não há).
///
/// O iterador procura termos da forma "X <op> <expr>" em que X é a coluna `i_column` da tabela
/// `i_cur`, ou, se `p_idx` é dado, a coluna `i_column` do índice (que deve ser um dos índices da
/// tabela `i_cur`; o par é `(tabela, índice)`). O `<op>` deve ser um dos de `op_mask`. Se a
/// cláusula tem X=Y, a busca também pode devolver termos "Y <op> <expr>", com um número limitado
/// de níveis de transitividade. Se X não é a INTEGER PRIMARY KEY, X deve ser compatível com o
/// índice `p_idx`.
pub(crate) fn where_scan_init(
    db: &mut Connection,
    parse: &mut Parse,
    wi: &WhereInfo,
    scan: &mut WhereScan,
    wc: ClauseId,
    i_cur: i32,
    i_column: i32,
    op_mask: u32,
    p_idx: Option<(&Rc<Table>, &Index)>,
) -> Option<TermId> {
    scan.p_orig_wc = wc;
    scan.p_wc = wc;
    scan.p_idx_expr = None;
    scan.idxaff = 0;
    scan.z_coll_name = None;
    scan.op_mask = op_mask;
    scan.k = 0;
    scan.ai_cur[0] = i_cur;
    scan.n_equiv = 1;
    scan.i_equiv = 1;
    let mut i_column = i_column;
    if let Some((p_tab, idx)) = p_idx {
        let j = i_column as usize;
        i_column = idx.ai_column[j] as i32;
        if i_column == p_tab.i_p_key as i32 {
            i_column = XN_ROWID as i32;
        } else if i_column >= 0 {
            scan.idxaff = p_tab.a_col[i_column as usize].affinity;
            scan.z_coll_name = Some(idx.az_coll[j].clone());
        } else if i_column == XN_EXPR as i32 {
            scan.p_idx_expr = idx
                .a_col_expr
                .as_deref()
                .and_then(|l| l.a.get(j))
                .and_then(|item| item.p_expr.clone());
            scan.z_coll_name = Some(idx.az_coll[j].clone());
            scan.ai_column[0] = XN_EXPR;
            return where_scan_init_index_expr(db, parse, wi, scan, p_tab);
        }
    } else if i_column == XN_EXPR as i32 {
        return None;
    }
    scan.ai_column[0] = i_column as i16;
    where_scan_next(db, parse, wi, scan)
}

/// `sqlite3WhereFindTerm`: procura na cláusula `wc` um termo da forma "X <op> <expr>", em que X
/// é uma referência à coluna `i_column` da tabela `i_cur` (ou do índice `p_idx`, se dado) e `<op>`
/// é um dos códigos `WO_xx` de `op`. Devolve o termo, ou `None` se não achou.
///
/// O termo devolvido pode ser Y=<expr> se outra restrição da cláusula diz que X=Y (o bit
/// `WO_EQUIV` de `eOperator`). `ai_cur[]`/`ai_column[]` guardam X e seus equivalentes: com 11
/// vagas, a busca por X devolve <expr> se X=A1 e A1=A2 e ... e A10=<expr>.
///
/// Havendo vários termos "X <op> <expr>", tenta-se o que não depende de <expr>, isto é, em que
/// <expr> é constante. Só se devolve "X <op> Y" (Y coluna de outra tabela) se não existe termo
/// com RHS constante; sem nenhum, tenta-se um termo que não use `WO_EQUIV`.
pub fn where_find_term(
    db: &mut Connection,
    parse: &mut Parse,
    wi: &WhereInfo,
    wc: ClauseId,
    i_cur: i32,
    i_column: i32,
    not_ready: Bitmask,
    op: u32,
    p_idx: Option<(&Rc<Table>, &Index)>,
) -> Option<TermId> {
    let mut p_result: Option<TermId> = None;
    let mut scan = WhereScan::default();
    let mut p = where_scan_init(db, parse, wi, &mut scan, wc, i_cur, i_column, op, p_idx);
    let op = op & (WO_EQ | WO_IS) as u32;
    while let Some(t) = p {
        let term = wi.term(t);
        if (term.prereq_right & not_ready) == 0 {
            if term.prereq_right == 0 && (term.e_operator as u32 & op) != 0 {
                return Some(t);
            }
            if p_result.is_none() {
                p_result = Some(t);
            }
        }
        p = where_scan_next(db, parse, wi, &mut scan);
    }
    p_result
}

// ---------------------------------------------------------------------------------------------
// DISTINCT redundante e estimativas simples
// ---------------------------------------------------------------------------------------------

/// `findIndexCol`: procura em `p_list` uma entrada que case com a coluna `i_col` do índice
/// `p_idx`. Devolve a posição em `p_list->a[]`, ou -1 se não há.
fn find_index_col(
    db: &mut Connection,
    parse: &mut Parse,
    p_list: &ExprList,
    i_base: i32,
    p_idx: &Index,
    i_col: usize,
) -> i32 {
    let z_coll = &p_idx.az_coll[i_col];
    for (i, item) in p_list.a.iter().enumerate() {
        if let Some(p) = expr_skip_collate_and_likely(item.p_expr.as_deref()) {
            if (p.op == TK_COLUMN || p.op == TK_AGG_COLUMN)
                && p.i_column == p_idx.ai_column[i_col] as i32
                && p.i_table == i_base
            {
                let p_coll = expr_nn_coll_seq(db, parse, item.p_expr.as_deref(), None);
                if str_icmp(&p_coll.name, z_coll) == 0 {
                    return i as i32;
                }
            }
        }
    }
    -1
}

/// `indexColumnNotNull`: verdadeiro se a coluna `i_col` do índice `p_idx` (da tabela `p_tab`) é
/// NOT NULL.
pub(crate) fn index_column_not_null(p_idx: &Index, p_tab: &Table, i_col: usize) -> bool {
    debug_assert!(i_col < p_idx.n_column as usize);
    let j = p_idx.ai_column[i_col];
    if j >= 0 {
        p_tab.a_col[j as usize].not_null != 0
    } else if j == -1 {
        true
    } else {
        debug_assert!(j == -2);
        false // Supõe-se que um índice sobre expressão sempre pode dar NULL.
    }
}

/// `isDistinctRedundant`: verdadeiro se a lista DISTINCT `p_distinct` é redundante, isto é, se
/// algum subconjunto das suas colunas é, coletivamente, único e, individualmente, não nulo.
pub(crate) fn is_distinct_redundant(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab_list: &SrcList,
    wi: &WhereInfo,
    wc: ClauseId,
    p_distinct: &ExprList,
) -> bool {
    // Com mais de uma tabela ou subconsulta no FROM não dá para mostrar que o DISTINCT é
    // redundante.
    if p_tab_list.a.len() != 1 {
        return false;
    }
    let i_base = p_tab_list.a[0].i_cursor;
    let Some(p_tab) = p_tab_list.a[0].p_tab.as_ref() else {
        return false;
    };

    // Se alguma expressão é uma coluna IPK da tabela `i_base`, é verdadeiro. A parte
    // `p->iTable==iBase` pode ser falsa quando o SELECT é uma subconsulta correlacionada.
    for item in p_distinct.a.iter() {
        let Some(p) = expr_skip_collate_and_likely(item.p_expr.as_deref()) else {
            continue;
        };
        if p.op != TK_COLUMN && p.op != TK_AGG_COLUMN {
            continue;
        }
        if p.i_table == i_base && p.i_column < 0 {
            return true;
        }
    }

    // Percorre os índices da tabela vendo se algum torna o DISTINCT redundante. Isso vale se:
    //
    //   1. o índice é UNIQUE, e
    //
    //   2. todas as colunas do índice fazem parte da lista `p_distinct` ou a cláusula WHERE tem
    //      um termo "col=X" com X constante. As colações da comparação e das expressões do
    //      SELECT devem ser as do índice.
    //
    //   3. as colunas do índice sem termo "col=X" no WHERE são NOT NULL.
    for p_idx in p_tab.p_index.iter() {
        let p_idx: &Index = p_idx;
        if !p_idx.is_unique_index() {
            continue;
        }
        if p_idx.p_partial_idx_where.is_some() {
            continue;
        }
        let n_key_col = p_idx.n_key_col as usize;
        let mut i = 0usize;
        while i < n_key_col {
            if where_find_term(
                db,
                parse,
                wi,
                wc,
                i_base,
                i as i32,
                ALLBITS,
                WO_EQ as u32,
                Some((p_tab, p_idx)),
            )
            .is_none()
            {
                if find_index_col(db, parse, p_distinct, i_base, p_idx, i) < 0 {
                    break;
                }
                if !index_column_not_null(p_idx, p_tab, i) {
                    break;
                }
            }
            i += 1;
        }
        if i == n_key_col {
            // Este índice implica que o DISTINCT é redundante.
            return true;
        }
    }

    false
}

/// `estLog`: estima o logaritmo na base 2 do valor.
pub(crate) fn est_log(n: LogEst) -> LogEst {
    if n <= 10 {
        0
    } else {
        log_est(n as i64 as u64) - 33
    }
}

/// `translateColumnToCopy`: converte os `OP_Column` do código já gerado em `OP_Copy`, quando a
/// tabela é acessada por co-rotina e não por consulta à tabela. Roda do endereço `i_start` até o
/// fim. Se `i_autoidx_cur` não é zero, os `OP_Rowid` sobre o cursor `i_tab_cur` viram `OP_Sequence`
/// do cursor `i_autoidx_cur`, para gerar rowids únicos para o índice automático.
pub(crate) fn translate_column_to_copy(
    db: &Connection,
    parse: &mut Parse,
    i_start: i32,
    i_tab_cur: i32,
    i_register: i32,
    i_autoidx_cur: i32,
) {
    if db.malloc_failed != 0 {
        return;
    }
    let v = vdbe_of_parse(parse);
    let i_end = v.n_op();
    for p_op in v.a_op[i_start as usize..i_end as usize].iter_mut() {
        if p_op.p1 != i_tab_cur {
            continue;
        }
        if p_op.opcode == OP_COLUMN {
            p_op.opcode = OP_COPY;
            p_op.p1 = p_op.p2 + i_register;
            p_op.p2 = p_op.p3;
            p_op.p3 = 0;
            p_op.p5 = 2; // Faz o flag `MEM_Subtype` ser limpo.
        } else if p_op.opcode == OP_ROWID {
            p_op.opcode = OP_SEQUENCE;
            p_op.p1 = i_autoidx_cur;
        }
    }
}

/// `constraintCompatibleWithOuterJoin`: `p_src` é operando de um outer join. Devolve verdadeiro
/// se a restrição `p_expr` (a expressão de um termo) é compatível com esse join.
///
/// O termo deve ser `EP_OuterON` se `p_src` é o operando direito de um outer join; pode ser
/// `EP_OuterON` ou `EP_InnerON` se `p_src` é o operando esquerdo de um RIGHT join. Ver
/// https://sqlite.org/forum/forumpost/206d99a16dd9212f para um exemplo de restrição do WHERE que
/// não pode ser usada na tabela direita de um RIGHT JOIN, porque implica uma condição de não
/// nulo sobre a tabela esquerda.
pub(crate) fn constraint_compatible_with_outer_join(p_expr: &Expr, p_src: &SrcItem) -> bool {
    debug_assert!((p_src.fg.jointype & (JT_LEFT | JT_LTORJ | JT_RIGHT)) != 0); // Pelo chamador
    if !p_expr.has_property(EP_OUTER_ON | EP_INNER_ON) || p_expr.i_join() != p_src.i_cursor {
        return false;
    }
    if (p_src.fg.jointype & (JT_LEFT | JT_RIGHT)) != 0 && p_expr.has_property(EP_INNER_ON) {
        return false;
    }
    true
}

/// `termCanDriveIndex`: verdadeiro se o termo `tid` tem uma forma que o deixaria usar um índice
/// para acessar `p_src`, supondo que existisse um índice apropriado.
pub(crate) fn term_can_drive_index(
    wi: &WhereInfo,
    tid: TermId,
    p_src: &SrcItem,
    not_ready: Bitmask,
) -> bool {
    let p_term = wi.term(tid);
    let p_expr = wi.expr(tid);
    if p_term.left_cursor != p_src.i_cursor {
        return false;
    }
    if (p_term.e_operator & (WO_EQ | WO_IS)) == 0 {
        return false;
    }
    debug_assert!((p_src.fg.jointype & JT_RIGHT) == 0);
    if (p_src.fg.jointype & (JT_LEFT | JT_LTORJ | JT_RIGHT)) != 0
        && !constraint_compatible_with_outer_join(p_expr, p_src)
    {
        return false; // Ver https://sqlite.org/forum/forumpost/51e6959f61
    }
    if (p_term.prereq_right & not_ready) != 0 {
        return false;
    }
    debug_assert!((p_term.e_operator & (crate::consts::WO_OR | crate::consts::WO_AND)) == 0);
    if p_term.left_column < 0 {
        return false;
    }
    let Some(p_tab) = p_src.p_tab.as_ref() else {
        return false;
    };
    let aff = p_tab.a_col[p_term.left_column as usize].affinity;
    index_affinity_ok(p_expr, aff, None)
}

/// `whereLoopResize`: garante que `p.a_l_term` tenha pelo menos `n` vagas. O `nLSlot` do C é
/// `a_l_term.len()`; as vagas novas ficam `None`. Devolve `SQLITE_OK` (falta de memória não
/// existe em Rust).
pub(crate) fn where_loop_resize(p: &mut WhereLoop, n: usize) -> i32 {
    if p.a_l_term.len() >= n {
        return SQLITE_OK;
    }
    let n = (n + 7) & !7usize;
    p.a_l_term.resize(n, None);
    SQLITE_OK
}

// ---------------------------------------------------------------------------------------------
// Índice automático (chunk 002)
// ---------------------------------------------------------------------------------------------

/// `constructAutomaticIndex`: gera o código que constrói o objeto `Index` de um índice automático
/// e prepara o `WhereLevel` `wi.a[i_level]` para que o gerador de código o use. `wc` é a cláusula
/// WHERE.
pub(crate) fn construct_automatic_index(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab_list: &mut SrcList,
    wi: &mut WhereInfo,
    wc: ClauseId,
    not_ready: Bitmask,
    i_level: usize,
) {
    let mut sent_warning = false; // Verdadeiro se um aviso já foi emitido
    let mut use_bloom_filter = false; // Verdadeiro para acrescentar também um filtro de Bloom
    let mut p_partial: Option<Box<Expr>> = None; // Expressão do índice parcial
    let mut i_continue = 0; // Salte aqui para pular as linhas excluídas
    let mut addr_counter = 0; // Endereço onde o contador inteiro é iniciado

    // Gera código para pular a criação e a inicialização do índice transitório na segunda
    // iteração do laço e nas seguintes.
    debug_assert!(parse.p_vdbe.is_some());
    let addr_init = add_op0(vdbe_of_parse(parse), OP_ONCE as i32);

    // Conta as colunas que entram no índice e casam com restrições do WHERE.
    let mut n_key_col: i32 = 0;
    let i_from = wi.a[i_level].i_from as usize;
    let p_table: Rc<Table> = p_tab_list.a[i_from].p_tab.clone().expect("pSrc->pTab");
    let n_wc_term = wi.n_term(wc);
    let l = wi.a[i_level].p_w_loop;
    let mut idx_cols: Bitmask = 0; // Mapa de bits das colunas usadas na indexação
    for i in 0..n_wc_term {
        let tid = wi.term_at(wc, i);
        // Torna o índice automático parcial se há termos no WHERE (ou no ON de um LEFT join) que
        // restringem as linhas de `pSrc` que podem ser usadas.
        if (wi.term(tid).wt_flags & TERM_VIRTUAL) == 0
            && expr_is_single_table_constraint(wi.expr_mut(tid), p_tab_list, i_from, false) != 0
        {
            let dup = expr_dup(Some(wi.expr(tid)), 0);
            p_partial = expr_and(db, parse, p_partial.take(), dup);
        }
        if term_can_drive_index(wi, tid, &p_tab_list.a[i_from], not_ready) {
            debug_assert!((wi.term(tid).e_operator & (crate::consts::WO_OR | crate::consts::WO_AND)) == 0);
            let i_col = wi.term(tid).left_column;
            let c_mask: Bitmask = if i_col >= BMS { TOPBIT } else { 1u64 << i_col };
            if !sent_warning {
                log(
                    SQLITE_WARNING_AUTOINDEX,
                    b"automatic index on %s(%s)",
                    &[text_arg(&p_table.z_name), text_arg(&p_table.a_col[i_col as usize].z_cn_name)],
                );
                sent_warning = true;
            }
            if (idx_cols & c_mask) == 0 {
                if where_loop_resize(wi.w_loop_mut(l), n_key_col as usize + 1) != 0 {
                    return;
                }
                wi.w_loop_mut(l).a_l_term[n_key_col as usize] = Some(tid);
                n_key_col += 1;
                idx_cols |= c_mask;
            }
        }
    }
    {
        let p_loop = wi.w_loop_mut(l);
        p_loop.btree.n_eq = n_key_col as u16;
        p_loop.n_l_term = n_key_col as u16;
        p_loop.ws_flags = WHERE_COLUMN_EQ | WHERE_IDX_ONLY | WHERE_INDEXED | WHERE_AUTO_INDEX;
    }

    // Conta as colunas adicionais necessárias para o índice ser de cobertura. Um índice de
    // cobertura contém todas as colunas de que a consulta precisa, e a tabela original nunca é
    // acessada. O índice automático precisa ser de cobertura porque não é atualizado se a tabela
    // original muda, e tabela e índice não podem ser usados juntos se saem de sincronia.
    let col_used = p_tab_list.a[i_from].col_used;
    let extra_cols: Bitmask = if p_table.is_view() {
        ALLBITS & !idx_cols
    } else {
        col_used & (!idx_cols | TOPBIT)
    };
    let mx_bit_col = std::cmp::min(BMS - 1, p_table.n_col as i32);
    for i in 0..mx_bit_col {
        if (extra_cols & (1u64 << i)) != 0 {
            n_key_col += 1;
        }
    }
    if (col_used & TOPBIT) != 0 {
        n_key_col += p_table.n_col as i32 - BMS + 1;
    }

    // Constrói o objeto `Index` que descreve o índice.
    let mut p_idx = allocate_index_object((n_key_col + 1) as i16);
    p_idx.z_name = b"auto-index".to_vec();
    let mut n: usize = 0;
    idx_cols = 0;
    for i in 0..n_wc_term {
        let tid = wi.term_at(wc, i);
        if term_can_drive_index(wi, tid, &p_tab_list.a[i_from], not_ready) {
            debug_assert!((wi.term(tid).e_operator & (crate::consts::WO_OR | crate::consts::WO_AND)) == 0);
            let i_col = wi.term(tid).left_column;
            let c_mask: Bitmask = if i_col >= BMS { TOPBIT } else { 1u64 << i_col };
            if (idx_cols & c_mask) == 0 {
                let p_x = wi.expr(tid);
                idx_cols |= c_mask;
                p_idx.ai_column[n] = i_col as i16;
                let p_coll = expr_compare_coll_seq(db, parse, p_x, None);
                debug_assert!(p_coll.is_some() || parse.n_err > 0); // TH3 collate01.800
                p_idx.az_coll[n] =
                    p_coll.map_or_else(|| STR_BINARY.as_bytes().to_vec(), |c| c.name.clone());
                n += 1;
                if let Some(p_left) = p_x.p_left.as_deref() {
                    if expr_affinity(p_left, None) != SQLITE_AFF_TEXT {
                        // TUNING: só usa um filtro de Bloom num índice automático se alguma
                        // coluna chave pode guardar valores numéricos, porque todas as strings
                        // têm o mesmo hash no filtro de Bloom e um filtro sobre coluna de texto
                        // normalmente não ajuda.
                        use_bloom_filter = true;
                    }
                }
            }
        }
    }
    debug_assert!(n == wi.w_loop(l).btree.n_eq as usize);

    // Acrescenta as colunas adicionais que fazem do índice automático um índice de cobertura.
    for i in 0..mx_bit_col {
        if (extra_cols & (1u64 << i)) != 0 {
            p_idx.ai_column[n] = i as i16;
            p_idx.az_coll[n] = STR_BINARY.as_bytes().to_vec();
            n += 1;
        }
    }
    if (col_used & TOPBIT) != 0 {
        for i in (BMS - 1)..p_table.n_col as i32 {
            p_idx.ai_column[n] = i as i16;
            p_idx.az_coll[n] = STR_BINARY.as_bytes().to_vec();
            n += 1;
        }
    }
    debug_assert!(n as i32 == n_key_col);
    p_idx.ai_column[n] = XN_ROWID;
    p_idx.az_coll[n] = STR_BINARY.as_bytes().to_vec();
    let p_idx: Rc<Index> = Rc::new(p_idx);
    wi.w_loop_mut(l).btree.p_index = Some(p_idx.clone());

    // Cria o índice automático.
    debug_assert!(wi.a[i_level].i_idx_cur >= 0);
    wi.a[i_level].i_idx_cur = parse.n_tab;
    parse.n_tab += 1;
    let i_idx_cur = wi.a[i_level].i_idx_cur;
    add_op2(vdbe_of_parse(parse), OP_OPENAUTOINDEX as i32, i_idx_cur, n_key_col + 1);
    set_p4_key_info(parse, db, &p_idx);
    vdbe_comment(vdbe_of_parse(parse), b"for %s", &[text_arg(&p_table.z_name)]);
    if db.optimization_enabled(SQLITE_BLOOM_FILTER) && use_bloom_filter {
        where_explain_bloom_filter(db, parse, wi, p_tab_list, i_level);
        parse.n_mem += 1;
        wi.a[i_level].reg_filter = parse.n_mem;
        add_op2(vdbe_of_parse(parse), OP_BLOB as i32, 10000, wi.a[i_level].reg_filter);
    }

    // Preenche o índice automático com o conteúdo.
    let i_tab_cur = wi.a[i_level].i_tab_cur;
    let via_coroutine = p_tab_list.a[i_from].fg.via_coroutine;
    let reg_return = p_tab_list.a[i_from].reg_return;
    let addr_fill_sub = p_tab_list.a[i_from].addr_fill_sub;
    let reg_result = p_tab_list.a[i_from].reg_result;
    let addr_top;
    if via_coroutine {
        let reg_yield = reg_return;
        addr_counter = add_op2(vdbe_of_parse(parse), OP_INTEGER as i32, 0, 0);
        add_op3(vdbe_of_parse(parse), OP_INITCOROUTINE as i32, reg_yield, 0, addr_fill_sub);
        addr_top = add_op1(vdbe_of_parse(parse), OP_YIELD as i32, reg_yield);
        vdbe_comment(vdbe_of_parse(parse), b"next row of %s", &[text_arg(&p_table.z_name)]);
    } else {
        addr_top = add_op1(vdbe_of_parse(parse), OP_REWIND as i32, i_tab_cur);
    }
    if let Some(partial) = p_partial.as_deref_mut() {
        i_continue = make_label(parse);
        expr_if_false(db, parse, partial, i_continue, SQLITE_JUMPIFNULL as i32, None);
        wi.w_loop_mut(l).ws_flags |= WHERE_PARTIALIDX;
    }
    let reg_record = get_temp_reg(parse);
    let mut part_idx_label = 0;
    let reg_base = generate_index_key(
        db,
        parse,
        &p_table,
        &p_idx,
        i_tab_cur,
        reg_record,
        0,
        &mut part_idx_label,
        None,
        0,
    );
    if wi.a[i_level].reg_filter != 0 {
        add_op4_int(
            vdbe_of_parse(parse),
            OP_FILTERADD as i32,
            wi.a[i_level].reg_filter,
            0,
            reg_base,
            wi.w_loop(l).btree.n_eq as i32,
        );
    }
    add_op2(vdbe_of_parse(parse), OP_IDXINSERT as i32, i_idx_cur, reg_record);
    change_p5(vdbe_of_parse(parse), OPFLAG_USESEEKRESULT);
    if p_partial.is_some() {
        resolve_label(parse, db, i_continue);
    }
    if via_coroutine {
        change_p2(vdbe_of_parse(parse), addr_counter, reg_base + n as i32);
        debug_assert!(i_idx_cur > 0);
        translate_column_to_copy(db, parse, addr_top, i_tab_cur, reg_result, i_idx_cur);
        vdbe_goto(vdbe_of_parse(parse), addr_top);
        p_tab_list.a[i_from].fg.via_coroutine = false;
    } else {
        add_op2(vdbe_of_parse(parse), OP_NEXT as i32, i_tab_cur, addr_top + 1);
        change_p5(vdbe_of_parse(parse), SQLITE_STMTSTATUS_AUTOINDEX as u16);
    }
    jump_here(vdbe_of_parse(parse), addr_top);
    release_temp_reg(parse, reg_record);

    // Salta para cá ao pular a inicialização.
    jump_here(vdbe_of_parse(parse), addr_init);
    // `p_partial` cai aqui (`sqlite3ExprDelete`).
}

// ---------------------------------------------------------------------------------------------
// Filtro de Bloom (chunk 003)
// ---------------------------------------------------------------------------------------------

/// `sqlite3ConstructBloomFilter`: gera o bytecode que inicia um filtro de Bloom adequado ao nível
/// `wi.a[i_level]`.
///
/// Se há laços internos com o flag `WHERE_BLOOMFILTER`, inicia um filtro de Bloom para eles
/// também, exceto se a otimização `SQLITE_BloomPulldown` foi desligada. Ao iniciar o filtro, o
/// flag `WHERE_BLOOMFILTER` é limpo do laço e `regFilter` recebe o registrador que implementa o
/// filtro. Com `regFilter` positivo, `sqlite3WhereCodeOneLoopStart()` gera o código que testa o
/// filtro e pula a busca seguinte na árvore-b quando o filtro indica que não há linha
/// correspondente. Só deve ser chamada se já se decidiu que o laço se beneficia do filtro e o bit
/// `WHERE_BLOOMFILTER` está ligado.
pub(crate) fn construct_bloom_filter(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab_list: &SrcList,
    wi: &mut WhereInfo,
    i_level: usize,
    not_ready: Bitmask,
) {
    let mut i_level = i_level;
    // Cópias salvas de `Parse.pIdxEpr` e `Parse.pIdxPartExpr`.
    let saved_p_idx_epr = std::mem::take(&mut parse.p_idx_epr);
    let saved_p_idx_part_expr = std::mem::take(&mut parse.p_idx_part_expr);

    debug_assert!(parse.p_vdbe.is_some());
    debug_assert!((wi.w_loop(wi.a[i_level].p_w_loop).ws_flags & WHERE_BLOOMFILTER) != 0);
    debug_assert!((wi.w_loop(wi.a[i_level].p_w_loop).ws_flags & WHERE_IDX_ONLY) == 0);

    let addr_once = add_op0(vdbe_of_parse(parse), OP_ONCE as i32);
    loop {
        let l = wi.a[i_level].p_w_loop;
        where_explain_bloom_filter(db, parse, wi, p_tab_list, i_level);
        let addr_cont = make_label(parse); // Salte aqui para pular uma linha
        let i_cur = wi.a[i_level].i_tab_cur; // Cursor da tabela que recebe o filtro
        parse.n_mem += 1;
        wi.a[i_level].reg_filter = parse.n_mem;
        let reg_filter = wi.a[i_level].reg_filter;

        // O filtro de Bloom é um Blob guardado num registrador. Inicia-se com um blob de zeros
        // de pelo menos 80K bits, ou mais se o tamanho estimado da tabela é maior. Daria para
        // medir a tabela em tempo de execução com `OP_Count` com P3==1 e usar esse valor para
        // iniciar o blob, mas isso complica os testes. Baseando o tamanho do blob no valor da
        // tabela `sqlite_stat1`, os testes ficam bem mais fáceis.
        let i_src = wi.a[i_level].i_from as usize;
        let p_item = &p_tab_list.a[i_src];
        let p_tab = p_item.p_tab.clone().expect("pItem->pTab");
        let mut sz: u64 = log_est_to_int(p_tab.n_row_log_est);
        if sz < 10000 {
            sz = 10000;
        } else if sz > 10000000 {
            sz = 10000000;
        }
        add_op2(vdbe_of_parse(parse), OP_BLOB as i32, sz as i32, reg_filter);

        let addr_top = add_op1(vdbe_of_parse(parse), OP_REWIND as i32, i_cur);
        let s_wc = wi.s_wc;
        for i in 0..wi.n_term(s_wc) {
            let tid = wi.term_at(s_wc, i);
            if (wi.term(tid).wt_flags & TERM_VIRTUAL) == 0
                && expr_is_single_table_constraint(wi.expr_mut(tid), p_tab_list, i_src, false)
                    != 0
            {
                expr_if_false(
                    db,
                    parse,
                    wi.expr_mut(tid),
                    addr_cont,
                    SQLITE_JUMPIFNULL as i32,
                    None,
                );
            }
        }
        if (wi.w_loop(l).ws_flags & WHERE_IPK) != 0 {
            let r1 = get_temp_reg(parse);
            add_op2(vdbe_of_parse(parse), OP_ROWID as i32, i_cur, r1);
            add_op4_int(vdbe_of_parse(parse), OP_FILTERADD as i32, reg_filter, 0, r1, 1);
            release_temp_reg(parse, r1);
        } else {
            let p_idx = wi.w_loop(l).btree.p_index.clone().expect("pLoop->u.btree.pIndex");
            let n = wi.w_loop(l).btree.n_eq as i32;
            let r1 = get_temp_range(parse, n);
            for jj in 0..n {
                expr_code_load_index_column(db, parse, &p_tab, &p_idx, i_cur, jj, r1 + jj);
            }
            add_op4_int(vdbe_of_parse(parse), OP_FILTERADD as i32, reg_filter, 0, r1, n);
            release_temp_range(parse, r1, n);
        }
        resolve_label(parse, db, addr_cont);
        add_op2(vdbe_of_parse(parse), OP_NEXT as i32, wi.a[i_level].i_tab_cur, addr_top + 1);
        jump_here(vdbe_of_parse(parse), addr_top);
        wi.w_loop_mut(l).ws_flags &= !WHERE_BLOOMFILTER;
        if db.optimization_disabled(SQLITE_BLOOM_PULLDOWN) {
            break;
        }
        loop {
            i_level += 1;
            if i_level >= wi.n_level as usize {
                break;
            }
            let p_tab_item = &p_tab_list.a[wi.a[i_level].i_from as usize];
            if (p_tab_item.fg.jointype & (JT_LEFT | JT_LTORJ)) != 0 {
                continue;
            }
            let p_loop = wi.w_loop(wi.a[i_level].p_w_loop);
            if (p_loop.prereq & not_ready) != 0 {
                continue;
            }
            if (p_loop.ws_flags & (WHERE_BLOOMFILTER | WHERE_COLUMN_IN)) == WHERE_BLOOMFILTER {
                // Candidato à descida do filtro de Bloom (avaliação antecipada). Omitir
                // `WHERE_COLUMN_IN` é importante: não dá para avaliar antes filtros que usam o
                // operador IN.
                break;
            }
        }
        if i_level >= wi.n_level as usize {
            break;
        }
    }
    jump_here(vdbe_of_parse(parse), addr_once);
    parse.p_idx_epr = saved_p_idx_epr;
    parse.p_idx_part_expr = saved_p_idx_part_expr;
}

// ---------------------------------------------------------------------------------------------
// Tabelas virtuais: sqlite3_index_info e xBestIndex (chunks 003 e 004)
// ---------------------------------------------------------------------------------------------

/// `allocateIndexInfo`: aloca e preenche o `sqlite3_index_info` do termo `wc` da tabela virtual
/// `p_src`. Devolve o `IndexInfo`, a cauda `HiddenIndexInfo` e a máscara `mNoOmit` (os termos que
/// não devem ser omitidos). Cada termo da cláusula que serve à tabela fica com `TERM_OK`.
/// `m_unusable` ignora os termos com esses pré-requisitos.
pub(crate) fn allocate_index_info(
    wi: &mut WhereInfo,
    wc: ClauseId,
    m_unusable: Bitmask,
    p_src: &SrcItem,
) -> (IndexInfo, HiddenIndexInfo, u16) {
    let mut m_no_omit: u16 = 0;
    let mut e_distinct = 0;
    let p_tab = p_src.p_tab.clone().expect("pSrc->pTab");
    debug_assert!(p_tab.is_virtual());

    // Acha todas as restrições do WHERE que se referem a esta tabela virtual. Marca cada termo
    // com `TERM_OK` e conta em `n_term`.
    let n_clause = wi.n_term(wc);
    let mut n_term = 0usize;
    for i in 0..n_clause {
        let tid = wi.term_at(wc, i);
        wi.term_mut(tid).wt_flags &= !TERM_OK;
        let p_term = wi.term(tid);
        if p_term.left_cursor != p_src.i_cursor {
            continue;
        }
        if (p_term.prereq_right & m_unusable) != 0 {
            continue;
        }
        // `IsPowerOfTwo(X)` do C é `(X & (X-1)) == 0`, e vale para 0.
        debug_assert!(
            (p_term.e_operator & !WO_EQUIV) & (p_term.e_operator & !WO_EQUIV).wrapping_sub(1) == 0
        );
        if (p_term.e_operator & !WO_EQUIV) == 0 {
            continue;
        }
        if (p_term.wt_flags & TERM_VNULL) != 0 {
            continue;
        }

        debug_assert!((p_term.e_operator & (crate::consts::WO_OR | crate::consts::WO_AND)) == 0);
        debug_assert!(p_term.left_column >= XN_ROWID as i32);
        debug_assert!(p_term.left_column < p_tab.n_col as i32);
        if (p_src.fg.jointype & (JT_LEFT | JT_LTORJ | JT_RIGHT)) != 0
            && !constraint_compatible_with_outer_join(wi.expr(tid), p_src)
        {
            continue;
        }
        n_term += 1;
        wi.term_mut(tid).wt_flags |= TERM_OK;
    }

    // Se o ORDER BY só tem colunas da tabela virtual corrente, prepara o `aOrderBy` do
    // `sqlite3_index_info`.
    let mut n_order_by = 0usize;
    if let Some(p_order_by) = wi.p_order_by.as_deref_mut() {
        let n = p_order_by.a.len();
        let mut i = 0usize;
        while i < n {
            let item = &mut p_order_by.a[i];

            // Pula os termos constantes do ORDER BY.
            if expr_is_constant(None, item.p_expr.as_deref_mut()) != 0 {
                i += 1;
                continue;
            }

            // As tabelas virtuais não sabem tratar NULLS FIRST.
            if (item.fg.sort_flags & KEYINFO_ORDER_BIGNULL) != 0 {
                break;
            }
            let Some(p_expr) = item.p_expr.as_deref_mut() else {
                break;
            };

            // 1o caso: referência direta a coluna, sem COLLATE.
            if p_expr.op == TK_COLUMN && p_expr.i_table == p_src.i_cursor {
                debug_assert!(p_expr.i_column >= XN_ROWID as i32 && p_expr.i_column < p_tab.n_col as i32);
                i += 1;
                continue;
            }

            // 2o caso: referência a coluna com COLLATE. Só casa se o COLLATE é o da coluna.
            if p_expr.op == TK_COLLATE {
                let e2_col = match p_expr.p_left.as_deref() {
                    Some(e2) if e2.op == TK_COLUMN && e2.i_table == p_src.i_cursor => {
                        Some(e2.i_column)
                    }
                    _ => None,
                };
                if let Some(i_col2) = e2_col {
                    debug_assert!(!p_expr.has_property(crate::consts::EP_INT_VALUE));
                    debug_assert!(p_expr.z_token().is_some());
                    debug_assert!(i_col2 >= XN_ROWID as i32 && i_col2 < p_tab.n_col as i32);
                    p_expr.i_column = i_col2;
                    if i_col2 < 0 {
                        // A colação não importa para o rowid.
                        i += 1;
                        continue;
                    }
                    let z_coll = column_coll(&p_tab.a_col[i_col2 as usize])
                        .unwrap_or(STR_BINARY.as_bytes());
                    if str_icmp(p_expr.z_token().unwrap_or(&[]), z_coll) == 0 {
                        i += 1;
                        continue;
                    }
                }
            }

            // Sem casamento: sai do laço.
            break;
        }
        if i == n {
            n_order_by = n;
            let wctrl = wi.wctrl_flags as u32;
            if (wctrl & WHERE_DISTINCTBY) != 0 && !p_src.fg.rowid_used {
                e_distinct = 2 + ((wctrl & WHERE_SORTBYGROUP) != 0) as i32;
            } else if (wctrl & WHERE_GROUPBY) != 0 {
                e_distinct = 1;
            }
        }
    }

    // Aloca o `sqlite3_index_info` e a cauda escondida.
    let mut p_hidden = HiddenIndexInfo {
        p_wc: wc,
        e_distinct,
        m_in: 0,
        m_handle_in: 0,
        a_rhs: (0..n_term).map(|_| None).collect(),
    };
    let mut p_idx_info = IndexInfo {
        a_constraint: Vec::with_capacity(n_term),
        a_order_by: Vec::with_capacity(n_order_by),
        a_constraint_usage: vec![IndexConstraintUsage::default(); n_term],
        ..IndexInfo::default()
    };
    for i in 0..n_clause {
        let tid = wi.term_at(wc, i);
        let p_term = wi.term(tid);
        if (p_term.wt_flags & TERM_OK) == 0 {
            continue;
        }
        let j = p_idx_info.a_constraint.len();
        let mut cons = IndexConstraint {
            i_column: p_term.left_column,
            i_term_offset: i as i32,
            ..IndexConstraint::default()
        };
        let mut op = p_term.e_operator & WO_ALL;
        if op == WO_IN {
            if (p_term.wt_flags & TERM_SLICE) == 0 {
                p_hidden.m_in |= smaskbit32(j);
            }
            op = WO_EQ;
        }
        if op == WO_AUX {
            cons.op = p_term.e_match_op;
        } else if (op & (WO_ISNULL | WO_IS)) != 0 {
            if op == WO_ISNULL {
                cons.op = SQLITE_INDEX_CONSTRAINT_ISNULL as u8;
            } else {
                cons.op = SQLITE_INDEX_CONSTRAINT_IS as u8;
            }
        } else {
            // A atribuição direta só vale porque os códigos `WO_` e `SQLITE_INDEX_CONSTRAINT_`
            // são idênticos.
            cons.op = op as u8;
            debug_assert!((p_term.e_operator & (WO_IN | WO_EQ | WO_LT | WO_LE | WO_GT | WO_GE | WO_AUX)) != 0);

            if (op & (WO_LT | WO_LE | WO_GT | WO_GE)) != 0
                && wi.expr(tid).p_right.as_deref().map_or(false, expr_is_vector)
            {
                if j < 16 {
                    m_no_omit |= 1 << j;
                }
                if op == WO_LT {
                    cons.op = WO_LE as u8;
                }
                if op == WO_GT {
                    cons.op = WO_GE as u8;
                }
            }
        }
        p_idx_info.a_constraint.push(cons);
    }
    debug_assert!(p_idx_info.a_constraint.len() == n_term);
    if let Some(p_order_by) = wi.p_order_by.as_deref_mut() {
        for i in 0..n_order_by {
            let item = &mut p_order_by.a[i];
            if expr_is_constant(None, item.p_expr.as_deref_mut()) != 0 {
                continue;
            }
            let sort_flags = item.fg.sort_flags;
            let Some(p_expr) = item.p_expr.as_deref() else {
                continue;
            };
            debug_assert!(
                p_expr.op == TK_COLUMN
                    || (p_expr.op == TK_COLLATE
                        && p_expr.p_left.as_deref().map_or(false, |e| e.op == TK_COLUMN
                            && p_expr.i_column == e.i_column))
            );
            p_idx_info.a_order_by.push(IndexOrderBy {
                i_column: p_expr.i_column,
                desc: (sort_flags & KEYINFO_ORDER_DESC) != 0,
            });
        }
    }

    (p_idx_info, p_hidden, m_no_omit)
}

/// `vtabBestIndex`: chama o `xBestIndex()` da tabela virtual `p_tab` com o `sqlite3_index_info`
/// `p`. Se há erro, `parse` recebe a mensagem e o código é devolvido. Um `SQLITE_CONSTRAINT` de
/// `xBestIndex` não é erro: indica que a configuração atual dos flags "unusable" não dá um plano
/// válido. O `idxStr` de `p` é do próprio `IndexInfo` (posse), então não há o que liberar.
pub(crate) fn vtab_best_index(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab: &Rc<Table>,
    p: &mut IndexInfo,
) -> i32 {
    let Some(vid) = get_vtable(db, p_tab) else {
        return SQLITE_ERROR;
    };
    let Some(mut vt) = db.vtabs.take(vid.slot()) else {
        return SQLITE_ERROR;
    };
    let mut rc = SQLITE_ERROR;
    let mut z_err_msg: Option<Vec<u8>> = None;
    if let Some(mut p_vtab) = vt.p_vtab.take() {
        db.n_schema_lock += 1;
        rc = p_vtab.best_index(db, p);
        db.n_schema_lock -= 1;
        // `sqlite3_free(pVtab->zErrMsg); pVtab->zErrMsg = 0;` ao fim: a mensagem é lida e limpa.
        z_err_msg = p_vtab.err_msg_mut().take();
        vt.p_vtab = Some(p_vtab);
    }
    let b_all_schemas = vt.b_all_schemas != 0;
    db.vtabs.put(vid.slot(), vt);

    if rc != SQLITE_OK && rc != SQLITE_CONSTRAINT {
        if rc == SQLITE_NOMEM {
            // `sqlite3OomFault(db)`.
            db.malloc_failed = 1;
        } else if let Some(z) = z_err_msg.as_deref() {
            error_msg(db, parse, b"%s", &[text_arg(z)]);
        } else {
            error_msg(db, parse, b"%s", &[text_arg(err_str(rc).as_bytes())]);
        }
    }
    if b_all_schemas {
        vtab_uses_all_schemas(db, parse);
    }
    rc
}

// ---------------------------------------------------------------------------------------------
// Estimativa de varreduras de intervalo (chunk 004 e 005)
// ---------------------------------------------------------------------------------------------

/// `whereRangeAdjust`: se `p_term` não é nulo, é um termo que dá um limite superior ou inferior
/// numa varredura de intervalo. Sem considerá-lo, estima-se que a varredura visite `n_new`
/// linhas; devolve o número estimado depois de considerar `p_term`.
///
/// Se o usuário deu explicitamente um `likelihood()` para o termo, o resultado é essa
/// probabilidade vezes o número de linhas de entrada. Senão, supõe-se que um termo "IS NOT NULL"
/// tem probabilidade 0,50 e qualquer outro 0,25.
fn where_range_adjust(p_term: Option<&WhereTerm>, n_new: LogEst) -> LogEst {
    let mut n_ret = n_new;
    if let Some(p_term) = p_term {
        if p_term.truth_prob <= 0 {
            n_ret = n_ret.wrapping_add(p_term.truth_prob);
        } else if (p_term.wt_flags & TERM_VNULL) == 0 {
            n_ret = n_ret.wrapping_sub(20);
            debug_assert!(20 == log_est(4));
        }
    }
    n_ret
}

/// `whereRangeScanEst`: estima o número de linhas visitadas por uma varredura de índice num
/// intervalo, com limite superior, inferior ou os dois. Os termos que dão os limites são
/// `p_lower` e `p_upper` (`None` quando o limite não existe). `p_loop` é o laço modelo do
/// construtor (`pBuilder->pNew`, por valor no construtor): a função altera o seu `n_out`.
///
/// Sem dados `sqlite_stat4` (desligado no Debian), uma única desigualdade reduz o espaço de busca
/// por um fator 4 e um par (x>? AND x<?) o reduz por um fator 64. Devolve `SQLITE_OK`.
pub(crate) fn where_range_scan_est(
    parse: &Parse,
    wi: &WhereInfo,
    p_lower: Option<TermId>,
    p_upper: Option<TermId>,
    p_loop: &mut WhereLoop,
) -> i32 {
    let rc = SQLITE_OK;
    let mut n_out: i32 = p_loop.n_out as i32;
    let p_lower = p_lower.map(|t| wi.term(t));
    let p_upper = p_upper.map(|t| wi.term(t));

    debug_assert!(p_lower.is_some() || p_upper.is_some());
    debug_assert!(
        p_upper.map_or(true, |t| (t.wt_flags & TERM_VNULL) == 0) || parse.n_err > 0
    );
    let mut n_new: LogEst = where_range_adjust(p_lower, n_out as LogEst);
    n_new = where_range_adjust(p_upper, n_new);

    // TUNING: se há limite superior e inferior e nenhum tem `likelihood()` definido pela
    // aplicação, supõe-se que o intervalo é reduzido em mais 75%. Isto é, por padrão uma consulta
    // de intervalo aberto (col > ?) casa 1/4 das linhas do índice e uma fechada (col BETWEEN ?
    // AND ?) casa 1/64 do índice.
    if let (Some(lo), Some(up)) = (p_lower, p_upper) {
        if lo.truth_prob > 0 && up.truth_prob > 0 {
            n_new -= 20;
        }
    }

    n_out -= p_lower.is_some() as i32 + p_upper.is_some() as i32;
    if n_new < 10 {
        n_new = 10;
    }
    if (n_new as i32) < n_out {
        n_out = n_new as i32;
    }
    p_loop.n_out = n_out as LogEst;
    rc
}
