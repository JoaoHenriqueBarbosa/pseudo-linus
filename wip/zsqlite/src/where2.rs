//! Tradução de `where.c` (chunks `where_c.006` a `where_c.011`): o gerenciamento da lista de
//! `WhereLoop` (`whereLoopInsert` e companhia), o ajuste de custo e de linhas de saída, a
//! construção dos loops de b-tree (`whereLoopAddBtree`, `whereLoopAddBtreeIndex`), das tabelas
//! virtuais (`whereLoopAddVirtual`), dos OR de vários índices (`whereLoopAddOr`) e de todas as
//! tabelas (`whereLoopAddAll`), e `wherePathSatisfiesOrderBy`.
//!
//! Convenções desta fatia (modelo v2, ver `where_int.rs`, que é o contrato):
//!
//! - `WhereLoopBuilder.pNew` é o `Box<WhereLoop>` do próprio `builder` (`builder.p_new`); como em
//!   todo chamador do C o `pTemplate` de `whereLoopInsert` e o `pLoop` de `whereLoopOutputAdjust`
//!   é esse mesmo `pNew`, `where_loop_insert` não recebe o molde: usa `builder.p_new`.
//!   `whereLoopAddOr` copia o construtor (`sSubBuild = *pBuilder`), e o `pNew` dos dois é o MESMO
//!   objeto no C: aqui o `p_new` é movido para o construtor interno e devolvido ao fim do laço.
//! - A lista `pLoops` é `wi.p_loops` (ids na ordem da lista do C). O `WhereLoop **` que
//!   `whereLoopFindLesser` devolve é a posição em `p_loops`; posição igual a `len` é o fim da
//!   lista (o ponteiro de ligação nulo do C).
//! - `WhereInfo.pParse`, `pTabList` e `pSelect` são parâmetros: `db: &mut Connection, parse: &mut
//!   Parse, wi: &mut WhereInfo, p_tab_list: &SrcList` e, onde `whereIsCoveringIndex` é
//!   alcançável, `p_select: Option<&mut Select>` (o `pSelect` do C, que só serve para essa busca).
//! - `Index` não guarda `pTable`: onde o C faz `pIdx->pTable` a função recebe a `&Table`
//!   (sempre a do `SrcItem`).
//! - O `Index` falso `sPk` (rowid) e o índice automático são `Rc<Index>` novos.
//! - Fora do que o Debian compila: STAT4 (`whereEqualScanEst`, `whereInScanEst`, `pRec`,
//!   `nRecValid`), `SQLITE_DEBUG`, `WHERETRACE_ENABLED` (`sqlite3WhereTermPrint`,
//!   `sqlite3WhereClausePrint`, `sqlite3WhereLoopPrint`, `sqlite3ShowWhereLoop*`) e
//!   `SQLITE_ENABLE_COSTMULT` somem.
//!
//! Dependências de `crate::where_` (traduzidas por outro agente, nomes determinísticos), com as
//! assinaturas assumidas:
//!
//! - `HiddenIndexInfo { p_wc: ClauseId, e_distinct: i32, m_in: u32, m_handle_in: u32,
//!   a_rhs: Vec<Option<Mem>> }`
//! - `where_or_insert(&mut WhereOrSet, Bitmask, LogEst, LogEst) -> i32`
//! - `where_or_move(&mut WhereOrSet, &WhereOrSet)`
//! - `where_scan_init(db, parse, &WhereInfo, &mut WhereScan, ClauseId, i_cur: i32, i_column: i32,
//!   op_mask: u32, Option<&Index>) -> Option<TermId>` e `where_scan_next(db, parse, &WhereInfo,
//!   &mut WhereScan) -> Option<TermId>`
//! - `where_find_term(db, parse, &WhereInfo, ClauseId, i_cur: i32, i_column: i32, not_ready:
//!   Bitmask, op: u32, Option<&Index>) -> Option<TermId>` (`sqlite3WhereFindTerm`)
//! - `where_range_scan_est(db, parse, &mut WhereInfo, &mut WhereLoopBuilder, p_lower:
//!   Option<TermId>, p_upper: Option<TermId>) -> i32` (o `pLoop` é `builder.p_new`)
//! - `vtab_best_index(db, parse, &Rc<Table>, &mut IndexInfo, &mut HiddenIndexInfo) -> i32`
//! - `allocate_index_info(db, parse, &mut WhereInfo, ClauseId, m_unusable: Bitmask, &SrcItem,
//!   &mut u16) -> Option<(IndexInfo, HiddenIndexInfo)>` (`freeIndexInfo` é o `Drop`)
//! - `est_log(LogEst) -> LogEst`
//! - `index_column_not_null(&Table, &Index, i_col: i32) -> bool`
//! - `constraint_compatible_with_outer_join(&WhereInfo, TermId, &SrcItem) -> bool`
//! - `term_can_drive_index(&WhereInfo, TermId, &SrcItem, not_ready: Bitmask) -> bool`
//!
//! Outras dependências ainda sem tradução:
//!
//! - `expr_code2::expr_implies_expr_no_parse(Option<&Expr>, Option<&Expr>, i_tab: i32) -> i32`:
//!   o `sqlite3ExprImpliesExpr(0, ...)` que `whereUsablePartialIndex` chama com `SQLITE_EnableQPSG`.
//! - `build::table_flags_or(db: &mut Connection, &Rc<Table>, u32)`: o `pTab->tabFlags |= ...` de
//!   `whereLoopAddBtree` (a `Table` é `Rc` compartilhada; quem tem acesso ao esquema aplica).
//! - `global::use_cis() -> bool` (`sqlite3GlobalConfig.bUseCis`).
//! - `mem::value_from_expr(db, &Expr, enc: u8, affinity: u8, &mut Option<Mem>) -> i32`
//!   (`sqlite3ValueFromExpr`).

use std::rc::Rc;

use crate::build::progress_check;
use crate::connection::{Connection, IndexConstraintUsage, IndexInfo, IndexedExpr, Parse};
use crate::consts::{
    Bitmask, ALLBITS, JT_CROSS, JT_LEFT, JT_LTORJ, JT_OUTER, JT_RIGHT, KEYINFO_ORDER_BIGNULL,
    KEYINFO_ORDER_DESC, OE_REPLACE, SQLITE_AFF_BLOB, SQLITE_AFF_TEXT, SQLITE_BIG_DBL,
    SQLITE_BLDF1_INDEXED, SQLITE_BLDF1_UNIQUE, SQLITE_CONSTRAINT, SQLITE_COVER_IDX_SCAN,
    SQLITE_DONE, SQLITE_ENABLE_QPSG, SQLITE_ERROR, SQLITE_IDXTYPE_IPK, SQLITE_IDXTYPE_PRIMARYKEY,
    SQLITE_INDEX_CONSTRAINT_LIMIT, SQLITE_INDEX_CONSTRAINT_OFFSET, SQLITE_INDEX_SCAN_UNIQUE,
    SQLITE_MISUSE, SQLITE_NOMEM, SQLITE_NOTFOUND, SQLITE_OK, SQLITE_ORDER_BY_IDX_JOIN,
    SQLITE_QUERY_PLANNER_LIMIT,
    SQLITE_QUERY_PLANNER_LIMIT_INCR, SQLITE_SEEK_SCAN, SQLITE_SKIP_SCAN, SQLITE_WARNING,
    TERM_HEURTRUTH, TERM_HIGHTRUTH, TERM_LIKEOPT, TERM_VIRTUAL, TERM_VNULL, TF_EPHEMERAL,
    TF_MAYBE_REANALYZE, TK_AGG_COLUMN, TK_AND, TK_COLUMN, TK_EQ, TK_IS, TOPBIT, WHERE_AUTO_INDEX,
    WHERE_BIGNULL_SORT, WHERE_BTM_LIMIT, WHERE_COLUMN_EQ, WHERE_COLUMN_IN, WHERE_COLUMN_NULL,
    WHERE_COLUMN_RANGE, WHERE_DISTINCTBY, WHERE_EXPRIDX, WHERE_GROUPBY, WHERE_IDX_ONLY,
    WHERE_INDEXED, WHERE_IN_SEEKSCAN, WHERE_IPK, WHERE_MULTI_OR, WHERE_ONEPASS_DESIRED,
    WHERE_ONEROW, WHERE_ORDERBY_LIMIT, WHERE_ORDERBY_MAX, WHERE_ORDERBY_MIN, WHERE_OR_SUBCLAUSE,
    WHERE_RIGHT_JOIN, WHERE_SELFCULL, WHERE_SKIPSCAN, WHERE_SORTBYGROUP, WHERE_TOP_LIMIT,
    WHERE_TRANSCONS, WHERE_UNQ_WANTED, WHERE_VIRTUALTABLE, WO_AND, WO_EQ, WO_GE, WO_GT, WO_IN,
    WO_IS, WO_ISNULL, WO_LE, WO_LT, WO_OR, XN_EXPR, XN_ROWID,
};
use crate::consts::{EP_OUTER_ON, SQLITE_AUTO_INDEX, WRC_ABORT, WRC_CONTINUE, WRC_PRUNE};
use crate::expr::{
    binary_compare_coll_seq, compare_affinity, expr_affinity, expr_compare_coll_seq, expr_dup,
    expr_nn_coll_seq, expr_skip_collate_and_likely, expr_vector_size, table_column_affinity,
};
use crate::printf::PrintfArg;
use crate::expr_code::{expr_is_constant, expr_is_integer, is_binary};
use crate::expr_code2::{
    expr_compare, expr_compare_skip, expr_covered_by_index, expr_implies_expr,
};
use crate::sqlite_int::{
    Expr, ExprList, Index, LogEst, Select, SrcItem, SrcList, SrcU2, Table, Walker,
};
use crate::util::{error_msg, log_est, log_est_add, log_est_from_double, str_icmp};
use crate::walker::{select_walk_noop, walk_expr, walk_select};
use crate::where_::{
    allocate_index_info, constraint_compatible_with_outer_join, est_log, index_column_not_null,
    term_can_drive_index, vtab_best_index, where_find_term, where_or_insert, where_or_move,
    where_range_scan_est, where_scan_init, where_scan_next, HiddenIndexInfo,
};
use crate::whereexpr::{where_clause_clear, where_expr_usage};
use crate::where_int::{
    ClauseId, LoopId, TermId, WhereClause, WhereInfo, WhereLoop, WhereLoopBuilder, WhereOrSet,
    WherePath, WhereScan,
};

/// `ArraySize(WhereLoop.aLTermSpace)`: as três vagas de `aLTerm` de um `WhereLoop` recém iniciado.
const WHERE_LOOP_SPACE: usize = 3;

// ---------------------------------------------------------------------------------------------
// Chunk 006: vida de um WhereLoop, comparação de custo e escolha do que substituir
// ---------------------------------------------------------------------------------------------

/// `whereLoopInit`: converte memória em bruto num `WhereLoop` válido, com as três vagas de
/// `aLTerm` e nenhum termo.
pub(crate) fn where_loop_init(p: &mut WhereLoop) {
    p.a_l_term = vec![None; WHERE_LOOP_SPACE];
    p.n_l_term = 0;
    p.ws_flags = 0;
}

/// `whereLoopClearUnion`: limpa o `WhereLoop.u`. Deixa `aLTerm` intacto. O índice automático e o
/// `idxStr` são posse do loop: soltá-los é largar o valor.
fn where_loop_clear_union(p: &mut WhereLoop) {
    if (p.ws_flags & (WHERE_VIRTUALTABLE | WHERE_AUTO_INDEX)) != 0 {
        if (p.ws_flags & WHERE_VIRTUALTABLE) != 0 && p.vtab.need_free {
            p.vtab.need_free = false;
            p.vtab.idx_str = None;
        } else if (p.ws_flags & WHERE_AUTO_INDEX) != 0 && p.btree.p_index.is_some() {
            p.btree.p_index = None;
        }
    }
}

/// `whereLoopClear`: libera a memória interna de um `WhereLoop` e o deixa como se fosse novo.
pub(crate) fn where_loop_clear(p: &mut WhereLoop) {
    if p.a_l_term.len() != WHERE_LOOP_SPACE {
        p.a_l_term = vec![None; WHERE_LOOP_SPACE];
    }
    where_loop_clear_union(p);
    p.n_l_term = 0;
    p.ws_flags = 0;
}

/// `whereLoopResize`: aumenta `aLTerm` para ter pelo menos `n` vagas.
pub(crate) fn where_loop_resize(p: &mut WhereLoop, n: i32) -> i32 {
    if p.a_l_term.len() as i32 >= n {
        return SQLITE_OK;
    }
    let n = ((n + 7) & !7) as usize;
    p.a_l_term.resize(n, None);
    SQLITE_OK
}

/// `whereLoopXfer`: transfere o conteúdo do segundo loop para o primeiro. Copia os campos até
/// `nSkip` (o `WHERE_LOOP_XFER_SZ` do C) e depois os `nLTerm` primeiros de `aLTerm`.
fn where_loop_xfer(p_to: &mut WhereLoop, p_from: &mut WhereLoop) -> i32 {
    where_loop_clear_union(p_to);
    if p_from.n_l_term as usize > p_to.a_l_term.len() {
        where_loop_resize(p_to, p_from.n_l_term as i32);
    }
    p_to.prereq = p_from.prereq;
    p_to.mask_self = p_from.mask_self;
    p_to.i_tab = p_from.i_tab;
    p_to.i_sort_idx = p_from.i_sort_idx;
    p_to.r_setup = p_from.r_setup;
    p_to.r_run = p_from.r_run;
    p_to.n_out = p_from.n_out;
    p_to.btree = p_from.btree.clone();
    p_to.vtab = p_from.vtab.clone();
    p_to.ws_flags = p_from.ws_flags;
    p_to.n_l_term = p_from.n_l_term;
    p_to.n_skip = p_from.n_skip;
    let n = p_to.n_l_term as usize;
    p_to.a_l_term[..n].copy_from_slice(&p_from.a_l_term[..n]);
    if (p_from.ws_flags & WHERE_VIRTUALTABLE) != 0 {
        p_from.vtab.need_free = false;
    } else if (p_from.ws_flags & WHERE_AUTO_INDEX) != 0 {
        p_from.btree.p_index = None;
    }
    SQLITE_OK
}

/// `whereLoopDelete`: apaga um `WhereLoop`. A vaga da arena fica (o id já saiu de `p_loops`).
fn where_loop_delete(wi: &mut WhereInfo, id: LoopId) {
    where_loop_clear(wi.w_loop_mut(id));
}

/// `whereInfoFree`: libera um `WhereInfo`: a cláusula, os loops e (por `Drop`) o resto.
pub(crate) fn where_info_free(wi: &mut WhereInfo) {
    let wc = wi.s_wc;
    where_clause_clear(wi, wc);
    while !wi.p_loops.is_empty() {
        let id = wi.p_loops.remove(0);
        where_loop_delete(wi, id);
    }
}

/// `whereLoopCheaperProperSubset`: verdadeiro se X é um subconjunto próprio de Y mas de custo igual
/// ou menor. Em outras palavras, a relação de custo entre X e Y está invertida e precisa de ajuste.
///
/// Caso 1: (1a) X e Y usam o mesmo índice, (1b) X tem menos termos `==` que Y, (1c) nenhum dos dois
/// usa skip-scan, (1d) X não tem custo maior que Y.
///
/// Caso 2: (2a) X tem custo igual ou menor, ou devolve o mesmo número de linhas ou menos, (2b) X
/// usa menos termos do WHERE que Y, (2c) todo termo usado por X é usado por Y, (2d) X pula pelo
/// menos tantas colunas quanto Y, (2e) se X é índice de cobertura, Y também é.
fn where_loop_cheaper_proper_subset(p_x: &WhereLoop, p_y: &WhereLoop) -> bool {
    if p_x.r_run > p_y.r_run && p_x.n_out > p_y.n_out {
        return false; /* (1d) e (2a) */
    }
    debug_assert!((p_x.ws_flags & WHERE_VIRTUALTABLE) == 0);
    debug_assert!((p_y.ws_flags & WHERE_VIRTUALTABLE) == 0);
    let same_index = match (&p_x.btree.p_index, &p_y.btree.p_index) {
        (None, None) => true,
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        _ => false,
    };
    if p_x.btree.n_eq < p_y.btree.n_eq /* (1b) */
        && same_index /* (1a) */
        && p_x.n_skip == 0
        && p_y.n_skip == 0
    /* (1c) */
    {
        return true; /* O caso 1 vale */
    }
    if p_x.n_l_term as i32 - p_x.n_skip as i32 >= p_y.n_l_term as i32 - p_y.n_skip as i32 {
        return false; /* (2b) */
    }
    if p_y.n_skip > p_x.n_skip {
        return false; /* (2d) */
    }
    for i in (0..p_x.n_l_term as usize).rev() {
        if p_x.a_l_term[i].is_none() {
            continue;
        }
        let mut found = false;
        for j in (0..p_y.n_l_term as usize).rev() {
            if p_y.a_l_term[j] == p_x.a_l_term[i] {
                found = true;
                break;
            }
        }
        if !found {
            return false; /* (2c) */
        }
    }
    if (p_x.ws_flags & WHERE_IDX_ONLY) != 0 && (p_y.ws_flags & WHERE_IDX_ONLY) == 0 {
        return false; /* (2e) */
    }
    true /* O caso 2 vale */
}

/// `whereLoopAdjustCost`: tenta ajustar o custo e o número de linhas de saída do molde `p_template`
/// para cima ou para baixo, de modo que (1) ele custe menos que qualquer outro loop que seja
/// subconjunto próprio dele e (2) custe mais que qualquer loop de que ele seja subconjunto próprio.
fn where_loop_adjust_cost(wi: &WhereInfo, p_template: &mut WhereLoop) {
    if (p_template.ws_flags & WHERE_INDEXED) == 0 {
        return;
    }
    for id in wi.p_loops.iter() {
        let p = wi.w_loop(*id);
        if p.i_tab != p_template.i_tab {
            continue;
        }
        if (p.ws_flags & WHERE_INDEXED) == 0 {
            continue;
        }
        if where_loop_cheaper_proper_subset(p, p_template) {
            /* Ajusta o custo do molde para baixo, para ficar mais barato que o subconjunto p. */
            p_template.r_run = p.r_run.min(p_template.r_run);
            p_template.n_out = (p.n_out - 1).min(p_template.n_out);
        } else if where_loop_cheaper_proper_subset(p_template, p) {
            /* Ajusta o custo do molde para cima, para ficar mais caro que p, de que ele é
            ** subconjunto próprio. */
            p_template.r_run = p.r_run.max(p_template.r_run);
            p_template.n_out = (p.n_out + 1).max(p_template.n_out);
        }
    }
}

/// `whereLoopFindLesser`: procura, a partir da posição `start` da lista `p_loops`, um loop que
/// possa ser substituído pelo molde.
///
/// Devolve `None` se o molde não pertence à lista (deve ser descartado). Se `p` pode ser
/// substituído, devolve a posição de `p`. Se o molde não substitui ninguém e precisa entrar como
/// entrada nova, devolve o fim da lista (`p_loops.len()`).
fn where_loop_find_lesser(wi: &WhereInfo, start: usize, p_template: &WhereLoop) -> Option<usize> {
    let mut idx = start;
    while idx < wi.p_loops.len() {
        let p = wi.w_loop(wi.p_loops[idx]);
        if p.i_tab != p_template.i_tab || p.i_sort_idx != p_template.i_sort_idx {
            /* Se o iTab ou o iSortIdx de dois WhereLoop diferem, eles são considerados em
            ** separado: nenhum é candidato a substituir o outro. */
            idx += 1;
            continue;
        }
        /* Na implementação atual o rSetup é zero ou o custo de montar um índice automático
        ** (NlogN), que é igual para WhereLoops compatíveis. */
        debug_assert!(
            p.r_setup == 0 || p_template.r_setup == 0 || p.r_setup == p_template.r_setup
        );

        /* whereLoopAddBtree() sempre gera e insere primeiro o caso do índice automático. Por isso
        ** candidatos compatíveis nunca têm rSetup maior (SETUP-INVARIANT). */
        debug_assert!(p.r_setup >= p_template.r_setup);

        /* Todo loop que usa um índice da aplicação (ou PRIMARY KEY ou UNIQUE) com uma ou mais
        ** restrições == é melhor que um índice automático. Exceto skip-scan. */
        if (p.ws_flags & WHERE_AUTO_INDEX) != 0
            && p_template.n_skip == 0
            && (p_template.ws_flags & WHERE_INDEXED) != 0
            && (p_template.ws_flags & WHERE_COLUMN_EQ) != 0
            && (p.prereq & p_template.prereq) == p_template.prereq
        {
            break;
        }

        /* Se o WhereLoop p existente é melhor que o molde, o molde é descartado. p é melhor se
        ** (1) tem no máximo as mesmas dependências e (2) custa igual ou menos. */
        if (p.prereq & p_template.prereq) == p.prereq /* (1)  */
            && p.r_setup <= p_template.r_setup /* (2a) */
            && p.r_run <= p_template.r_run /* (2b) */
            && p.n_out <= p_template.n_out
        /* (2c) */
        {
            return None; /* Descarta o molde */
        }

        /* Se o molde é sempre melhor que p, p é sobrescrito pelo molde. */
        if (p.prereq & p_template.prereq) == p_template.prereq /* (1)  */
            && p.r_run >= p_template.r_run /* (2a) */
            && p.n_out >= p_template.n_out
        /* (2b) */
        {
            debug_assert!(p.r_setup >= p_template.r_setup); /* SETUP-INVARIANT */
            break; /* Faz p ser sobrescrito pelo molde */
        }
        idx += 1;
    }
    Some(idx)
}

// ---------------------------------------------------------------------------------------------
// Chunk 007: inserção, ajuste de nOut, vetores de intervalo e o início de whereLoopAddBtreeIndex
// ---------------------------------------------------------------------------------------------

/// `whereLoopInsert`: insere ou substitui uma entrada `WhereLoop` usando o molde `builder.p_new`.
///
/// Uma entrada existente pode ser sobrescrita se o molde é melhor e tem menos dependências. Ou o
/// molde é ignorado se uma existente é mais rápida e tem menos dependências. Senão entra um
/// `WhereLoop` novo baseado no molde.
///
/// Se `builder.p_or_set` não é nulo, só importam os pré-requisitos e os custos `rRun` e `nOut`
/// dos N melhores loops, reunidos em `p_or_set` (modo especial do processamento de cláusulas OR).
///
/// Fora desse modo um loop pode ser sobrescrito se: (1) têm o mesmo `iTab`, (2) o mesmo
/// `iSortIdx`, (3) o molde tem as mesmas dependências ou menos, (4) o molde tem custo igual ou
/// menor.
pub(crate) fn where_loop_insert(wi: &mut WhereInfo, builder: &mut WhereLoopBuilder) -> i32 {
    /* Para a busca ao atingir o limite do planejador */
    if builder.i_plan_limit == 0 {
        if let Some(s) = builder.p_or_set.as_mut() {
            s.n = 0;
        }
        return SQLITE_DONE;
    }
    builder.i_plan_limit -= 1;

    where_loop_adjust_cost(wi, &mut builder.p_new);

    /* Se `p_or_set` está definido, só se guardam os custos e os pré-requisitos. */
    if let Some(or_set) = builder.p_or_set.as_mut() {
        if builder.p_new.n_l_term != 0 {
            where_or_insert(
                or_set,
                builder.p_new.prereq,
                builder.p_new.r_run,
                builder.p_new.n_out,
            );
        }
        return SQLITE_OK;
    }

    /* Procura um WhereLoop existente para substituir pelo molde */
    let Some(idx) = where_loop_find_lesser(wi, 0, &builder.p_new) else {
        /* Já existe na lista um WhereLoop melhor que o molde: ignora o molde. */
        return SQLITE_OK;
    };

    /* Aqui, ou p[] é sobrescrito pelo molde se p[] existe, ou (p==NULL) aloca-se um WhereLoop
    ** novo e insere-se. */
    let id: LoopId;
    if idx >= wi.p_loops.len() {
        /* Aloca um WhereLoop novo para acrescentar ao fim da lista */
        let mut l = WhereLoop::default();
        where_loop_init(&mut l);
        wi.loops.push(l);
        id = LoopId((wi.loops.len() - 1) as u32);
        wi.p_loops.push(id);
    } else {
        id = wi.p_loops[idx];
        /* Vai sobrescrever p[]. Antes, percorre o resto da lista e apaga qualquer outra entrada
        ** além de p[] que o molde também supere. */
        let mut tail = idx + 1;
        while tail < wi.p_loops.len() {
            let Some(t) = where_loop_find_lesser(wi, tail, &builder.p_new) else {
                break;
            };
            tail = t;
            if tail >= wi.p_loops.len() {
                break;
            }
            let to_del = wi.p_loops.remove(tail);
            where_loop_delete(wi, to_del);
        }
    }
    let rc = where_loop_xfer(wi.w_loop_mut(id), &mut builder.p_new);
    let p = wi.w_loop_mut(id);
    if (p.ws_flags & WHERE_VIRTUALTABLE) == 0 {
        if let Some(ix) = &p.btree.p_index {
            if ix.idx_type == SQLITE_IDXTYPE_IPK {
                p.btree.p_index = None;
            }
        }
    }
    rc
}

/// `whereLoopOutputAdjust`: ajusta `WhereLoop.nOut` para baixo, para levar em conta os termos do
/// WHERE que referenciam o loop mas não são usados por um índice.
///
/// Para todo termo que não é usado pelo índice e tem uma probabilidade de verdade atribuída por
/// `likelihood()`, `likely()` ou `unlikely()`, reduz-se o número estimado de linhas de saída por
/// essa probabilidade.
///
/// TUNING: para todo termo não usado pelo índice e sem probabilidade atribuída, usam-se
/// heurísticas.
///
/// Heurística 1: estima-se a probabilidade de verdade em 93,75% (-1 em LogEst): decrementa-se
/// `nOut` por termo.
///
/// Heurística 2: se há termos `x==EXPR` com EXPR diferente da constante 0 ou 1, garante-se que a
/// estimativa final não passa de 1/4 das linhas da tabela. Se EXPR é -1, 0 ou 1, limita-se a 1/2.
fn where_loop_output_adjust(
    db: &mut Connection,
    parse: &mut Parse,
    wi: &mut WhereInfo,
    p_tab_list: &SrcList,
    wc: ClauseId,
    p_loop: &mut WhereLoop,
    n_row: LogEst,
) {
    let not_allowed: Bitmask = !(p_loop.prereq | p_loop.mask_self);
    let mut i_reduce: LogEst = 0; /* pLoop->nOut não deve passar de nRow-iReduce */

    debug_assert!((p_loop.ws_flags & WHERE_AUTO_INDEX) == 0);
    let n_base = wi.clause(wc).n_base.max(0) as usize;
    for i in 0..n_base {
        let t = wi.term_at(wc, i);
        let (prereq_all, wt_flags, e_operator, truth_prob) = {
            let term = wi.term(t);
            (term.prereq_all, term.wt_flags, term.e_operator, term.truth_prob)
        };
        if (prereq_all & not_allowed) != 0 {
            continue;
        }
        if (prereq_all & p_loop.mask_self) == 0 {
            continue;
        }
        if (wt_flags & TERM_VIRTUAL) != 0 {
            continue;
        }
        let mut used = false;
        for j in (0..p_loop.n_l_term as usize).rev() {
            let Some(x) = p_loop.a_l_term[j] else {
                continue;
            };
            if x == t {
                used = true;
                break;
            }
            let i_parent = wi.term(x).i_parent;
            if i_parent >= 0 && wi.clause(wc).a.get(i_parent as usize) == Some(&t) {
                used = true;
                break;
            }
        }
        if !used {
            progress_check(parse, db);
            if p_loop.mask_self == prereq_all {
                /* Se há termos extras no WHERE, não usados por um índice, que dependem só da
                ** tabela varrida e tendem a omitir muitas linhas, marca-se a tabela como
                ** "autoexcludente".
                **
                ** 2022-03-24: a autoexclusão só vale se os termos extras são operadores de
                ** comparação que não são verdadeiros com operando NULL, ou se o loop não é um
                ** OUTER JOIN. */
                if (e_operator & 0x3f) != 0
                    || (p_tab_list.a[p_loop.i_tab as usize].fg.jointype & (JT_LEFT | JT_LTORJ)) == 0
                {
                    p_loop.ws_flags |= WHERE_SELFCULL;
                }
            }
            if truth_prob <= 0 {
                /* Se uma probabilidade de verdade vem das dicas likelihood(), usa-se a dada pela
                ** aplicação. */
                p_loop.n_out = p_loop.n_out.wrapping_add(truth_prob);
            } else {
                /* Sem probabilidade explícita, heurísticas chutam uma razoável. */
                p_loop.n_out = p_loop.n_out.wrapping_sub(1);
                if (e_operator & (WO_EQ | WO_IS)) != 0 && (wt_flags & TERM_HIGHTRUTH) == 0 {
                    /* tag-20200224-1 */
                    let mut k: i32 = 0;
                    let is_small_int = match wi.expr(t).p_right.as_deref() {
                        Some(p_right) => expr_is_integer(p_right, &mut k) != 0,
                        None => false,
                    };
                    if is_small_int && (-1..=1).contains(&k) {
                        k = 10;
                    } else {
                        k = 20;
                    }
                    if (i_reduce as i32) < k {
                        wi.term_mut(t).wt_flags |= TERM_HEURTRUTH;
                        i_reduce = k as LogEst;
                    }
                }
            }
        }
    }
    if p_loop.n_out as i32 > n_row as i32 - i_reduce as i32 {
        p_loop.n_out = (n_row as i32 - i_reduce as i32) as LogEst;
    }
}

/// `whereRangeVectorLen`: o termo `p_term` é uma comparação de intervalo de vetores. A primeira
/// comparação do vetor pode ser otimizada com a coluna `n_eq` do índice. Devolve o número total de
/// elementos do vetor que podem ser usados na comparação de intervalo.
///
/// Por exemplo, com `WHERE a = ? AND (b, c, d) > (?, ?, ?)` e o índice `(a, b, c, d, e)`, a função
/// é chamada com `n_eq=1` e devolve 3.
fn where_range_vector_len(
    db: &mut Connection,
    parse: &mut Parse,
    wi: &WhereInfo,
    i_cur: i32,
    p_tab: &Table,
    p_idx: &Index,
    n_eq: i32,
    p_term: TermId,
) -> i32 {
    let p_expr = wi.expr(p_term);
    let mut n_cmp = expr_vector_size(p_expr.p_left.as_deref().expect("pExpr->pLeft"));
    n_cmp = n_cmp.min(p_idx.n_column as i32 - n_eq);
    let mut i: i32 = 1;
    while i < n_cmp {
        /* Testa se a comparação i de pTerm é compatível com a coluna (i+nEq) do índice. Se não,
        ** sai do laço. */
        debug_assert!(p_expr.p_left.as_deref().map_or(false, |l| l.use_x_list()));
        let p_lhs = p_expr
            .p_left
            .as_deref()
            .and_then(|l| l.x_list())
            .and_then(|l| l.a.get(i as usize))
            .and_then(|it| it.p_expr.as_deref())
            .expect("pLeft->x.pList->a[i].pExpr");
        let p_rhs_top = p_expr.p_right.as_deref().expect("pExpr->pRight");
        let p_rhs: &Expr = if p_rhs_top.use_x_select() {
            p_rhs_top
                .x_select()
                .and_then(|s| s.p_e_list.as_deref())
                .and_then(|l| l.a.get(i as usize))
                .and_then(|it| it.p_expr.as_deref())
                .expect("pRhs->x.pSelect->pEList->a[i].pExpr")
        } else {
            p_rhs_top
                .x_list()
                .and_then(|l| l.a.get(i as usize))
                .and_then(|it| it.p_expr.as_deref())
                .expect("pRhs->x.pList->a[i].pExpr")
        };

        /* Confere que o lado esquerdo da comparação é uma referência à coluna certa da tabela
        ** certa, e que a ordem do índice nessa coluna é a da coluna mais à esquerda. */
        let k = (i + n_eq) as usize;
        let sort_k = p_idx.a_sort_order.get(k).copied().unwrap_or(0);
        let sort_eq = p_idx.a_sort_order.get(n_eq as usize).copied().unwrap_or(0);
        if p_lhs.op != TK_COLUMN
            || p_lhs.i_table != i_cur
            || p_lhs.i_column != p_idx.ai_column[k] as i32
            || sort_k != sort_eq
        {
            break;
        }

        let aff = compare_affinity(p_rhs, expr_affinity(p_lhs, None), None);
        let idxaff = table_column_affinity(p_tab, p_lhs.i_column as i32);
        if aff != idxaff {
            break;
        }

        let p_coll = binary_compare_coll_seq(db, parse, p_lhs, Some(p_rhs), None);
        let Some(p_coll) = p_coll else {
            break;
        };
        if str_icmp(&p_coll.name, &p_idx.az_coll[k]) != 0 {
            break;
        }
        i += 1;
    }
    i
}

/// `whereLoopAddBtreeIndex`: já casaram `builder.p_new.btree.n_eq` termos do índice `p_probe`;
/// tenta casar mais um.
///
/// Quando a função é chamada, `p_new.n_out` tem o número de linhas que se espera visitar ao
/// filtrar só pelos `n_eq` termos. Se for modificado, o valor é restaurado antes de retornar.
///
/// Se `p_probe.idx_type == SQLITE_IDXTYPE_IPK`, o índice é um falso, usado para a INTEGER PRIMARY
/// KEY.
fn where_loop_add_btree_index(
    db: &mut Connection,
    parse: &mut Parse,
    wi: &mut WhereInfo,
    p_tab_list: &SrcList,
    builder: &mut WhereLoopBuilder,
    p_src: &SrcItem,
    p_probe: &Rc<Index>,
    n_in_mul: LogEst,
) -> i32 {
    let mut rc = SQLITE_OK;
    let mut p_top: Option<TermId> = None; /* Restrições de intervalo superior e inferior */
    let mut p_btm: Option<TermId> = None;
    let p_tab: Rc<Table> = p_src.p_tab.clone().expect("pSrc->pTab");

    if parse.n_err != 0 {
        return parse.rc;
    }

    debug_assert!((builder.p_new.ws_flags & WHERE_VIRTUALTABLE) == 0);
    debug_assert!((builder.p_new.ws_flags & WHERE_TOP_LIMIT) == 0);
    let mut op_mask: u32; /* Operadores válidos para restrições */
    if (builder.p_new.ws_flags & WHERE_BTM_LIMIT) != 0 {
        op_mask = (WO_LT | WO_LE) as u32;
    } else {
        debug_assert!(builder.p_new.btree.n_btm == 0);
        op_mask = (WO_EQ | WO_IN | WO_GT | WO_GE | WO_LT | WO_LE | WO_ISNULL | WO_IS) as u32;
    }
    if p_probe.b_unordered || p_probe.b_low_qual {
        if p_probe.b_unordered {
            op_mask &= !((WO_GT | WO_GE | WO_LT | WO_LE) as u32);
        }
        if p_probe.b_low_qual && !p_src.fg.is_indexed_by {
            op_mask &= !((WO_EQ | WO_IN | WO_IS) as u32);
        }
    }

    debug_assert!(builder.p_new.btree.n_eq < p_probe.n_column);
    debug_assert!(
        builder.p_new.btree.n_eq < p_probe.n_key_col
            || p_probe.idx_type != SQLITE_IDXTYPE_PRIMARYKEY
    );

    let saved_n_eq: u16 = builder.p_new.btree.n_eq;
    let saved_n_btm: u16 = builder.p_new.btree.n_btm;
    let saved_n_top: u16 = builder.p_new.btree.n_top;
    let saved_n_skip: u16 = builder.p_new.n_skip;
    let saved_n_l_term: u16 = builder.p_new.n_l_term;
    let saved_ws_flags: u32 = builder.p_new.ws_flags;
    let saved_prereq: Bitmask = builder.p_new.prereq;
    let saved_n_out: LogEst = builder.p_new.n_out;
    let mut scan = WhereScan::default();
    let mut p_term = where_scan_init(
        db,
        parse,
        wi,
        &mut scan,
        builder.p_wc,
        p_src.i_cursor,
        saved_n_eq as i32,
        op_mask,
        Some((&p_tab, &**p_probe)),
    );
    builder.p_new.r_setup = 0;
    let r_size: LogEst = p_probe.ai_row_log_est[0]; /* Número de linhas da tabela */
    let r_log_size: LogEst = est_log(r_size); /* Logaritmo do tamanho da tabela */
    'scan: while rc == SQLITE_OK {
        let Some(t) = p_term else {
            break;
        };
        'body: {
            let (e_op, wt_flags, prereq_right, truth_prob) = {
                let term = wi.term(t);
                (term.e_operator, term.wt_flags, term.prereq_right, term.truth_prob)
            };
            let mut n_in: LogEst = 0;
            if (e_op == WO_ISNULL || (wt_flags & TERM_VNULL) != 0)
                && index_column_not_null(p_probe, &p_tab, saved_n_eq as usize)
            {
                break 'body; /* ignora IS [NOT] NULL em colunas NOT NULL */
            }
            if (prereq_right & builder.p_new.mask_self) != 0 {
                break 'body;
            }

            /* Não permite que o limite superior de uma restrição de intervalo da otimização LIKE
            ** se misture com um limite inferior de outra origem */
            if (wt_flags & TERM_LIKEOPT) != 0 && e_op == WO_LT {
                break 'body;
            }

            if (p_src.fg.jointype & (JT_LEFT | JT_LTORJ | JT_RIGHT)) != 0
                && !constraint_compatible_with_outer_join(wi.expr(t), p_src)
            {
                break 'body;
            }
            if p_probe.is_unique_index() && saved_n_eq as i32 == p_probe.n_key_col as i32 - 1 {
                builder.bld_flags1 |= SQLITE_BLDF1_UNIQUE;
            } else {
                builder.bld_flags1 |= SQLITE_BLDF1_INDEXED;
            }
            builder.p_new.ws_flags = saved_ws_flags;
            builder.p_new.btree.n_eq = saved_n_eq;
            builder.p_new.btree.n_btm = saved_n_btm;
            builder.p_new.btree.n_top = saved_n_top;
            builder.p_new.n_l_term = saved_n_l_term;
            if builder.p_new.n_l_term as usize >= builder.p_new.a_l_term.len() {
                { let n = builder.p_new.n_l_term as i32 + 1; where_loop_resize(&mut builder.p_new, n) };
            }
            let nl = builder.p_new.n_l_term as usize;
            builder.p_new.a_l_term[nl] = Some(t);
            builder.p_new.n_l_term += 1;
            builder.p_new.prereq = (saved_prereq | prereq_right) & !builder.p_new.mask_self;

            debug_assert!(
                n_in_mul == 0
                    || (builder.p_new.ws_flags & WHERE_COLUMN_NULL) != 0
                    || (builder.p_new.ws_flags & WHERE_COLUMN_IN) != 0
                    || (builder.p_new.ws_flags & WHERE_SKIPSCAN) != 0
            );

            if (e_op & WO_IN) != 0 {
                let (is_select, list_len) = {
                    let e = wi.expr(t);
                    (e.use_x_select(), e.x_list().map_or(0, |l| l.a.len()))
                };
                if is_select {
                    /* "x IN (SELECT ...)": TUNING: o SELECT devolve 25 linhas */
                    n_in = 46; /* 46==sqlite3LogEst(25) */

                    /* A expressão pode ser da forma (x, y) IN (SELECT...). Nesse caso há um termo
                    ** separado para (x) e (y). Mas o multiplicador nIn só deve ser aplicado uma
                    ** vez, não uma por termo. O laço confere que pTerm é o primeiro desses termos
                    ** em uso e volta nIn a 0 se não for. */
                    let nl1 = builder.p_new.n_l_term as i32 - 1;
                    for i in 0..nl1 {
                        if let Some(x) = builder.p_new.a_l_term[i as usize] {
                            if wi.term(x).p_expr == wi.term(t).p_expr {
                                n_in = 0;
                            }
                        }
                    }
                } else if list_len > 0 {
                    /* "x IN (valor, valor, ...)" */
                    n_in = log_est(list_len as u64);
                }
                if p_probe.has_stat1 && r_log_size >= 10 {
                    /* Sejam N o total de linhas da tabela, K o número de entradas do lado direito
                    ** do IN e M o número de linhas da tabela que casam os termos à esquerda no
                    ** mesmo índice (se o IN está na coluna mais à esquerda, M==N).
                    **
                    ** É melhor omitir o IN da busca no índice e varrer as M linhas, testando cada
                    ** uma contra o IN, se M*log(K) < K*log(N). As estimativas podem ser
                    ** imprecisas, então há uma margem de segurança de 2 (LogEst: 10) a favor do
                    ** uso do IN com o índice, que tem melhor pior caso. */
                    let m = p_probe.ai_row_log_est[saved_n_eq as usize] as i32;
                    let log_k = est_log(n_in) as i32;
                    /* TUNING          v-----  10 para favorecer o IN indexado */
                    let x = (m + log_k + 10 - (n_in as i32 + r_log_size as i32)) as i16 as i32;
                    if x >= 0 {
                        /* prefere a busca indexada */
                    } else if n_in_mul < 2 && db.optimization_enabled(SQLITE_SEEK_SCAN) {
                        /* prefere skip-scan */
                        builder.p_new.ws_flags |= WHERE_IN_SEEKSCAN;
                    } else {
                        /* prefere a varredura normal */
                        break 'body;
                    }
                }
                builder.p_new.ws_flags |= WHERE_COLUMN_IN;
            } else if (e_op & (WO_EQ | WO_IS)) != 0 {
                let i_col = p_probe.ai_column[saved_n_eq as usize];
                builder.p_new.ws_flags |= WHERE_COLUMN_EQ;
                debug_assert!(saved_n_eq == builder.p_new.btree.n_eq);
                if i_col == XN_ROWID
                    || (i_col >= 0
                        && n_in_mul == 0
                        && saved_n_eq as i32 == p_probe.n_key_col as i32 - 1)
                {
                    if i_col == XN_ROWID
                        || p_probe.uniq_not_null
                        || (p_probe.n_key_col == 1 && p_probe.on_error != 0 && e_op == WO_EQ)
                    {
                        builder.p_new.ws_flags |= WHERE_ONEROW;
                    } else {
                        builder.p_new.ws_flags |= WHERE_UNQ_WANTED;
                    }
                }
                if scan.i_equiv > 1 {
                    builder.p_new.ws_flags |= WHERE_TRANSCONS;
                }
            } else if (e_op & WO_ISNULL) != 0 {
                builder.p_new.ws_flags |= WHERE_COLUMN_NULL;
            } else {
                let n_vec_len = where_range_vector_len(
                    db,
                    parse,
                    wi,
                    p_src.i_cursor,
                    &p_tab,
                    p_probe,
                    saved_n_eq as i32,
                    t,
                );
                if (e_op & (WO_GT | WO_GE)) != 0 {
                    builder.p_new.ws_flags |= WHERE_COLUMN_RANGE | WHERE_BTM_LIMIT;
                    builder.p_new.btree.n_btm = n_vec_len as u16;
                    p_btm = Some(t);
                    p_top = None;
                    if (wt_flags & TERM_LIKEOPT) != 0 {
                        /* As restrições de intervalo da otimização LIKE são sempre usadas em
                        ** pares. */
                        let wc2 = wi.term(t).p_wc;
                        let pos = wi
                            .clause(wc2)
                            .a
                            .iter()
                            .position(|x| *x == t)
                            .expect("pTerm em pTerm->pWC->a");
                        let top = wi.clause(wc2).a[pos + 1];
                        debug_assert!((wi.term(top).wt_flags & TERM_LIKEOPT) != 0);
                        debug_assert!(wi.term(top).e_operator == WO_LT);
                        p_top = Some(top);
                        { let n = builder.p_new.n_l_term as i32 + 1; where_loop_resize(&mut builder.p_new, n) };
                        let nl = builder.p_new.n_l_term as usize;
                        builder.p_new.a_l_term[nl] = Some(top);
                        builder.p_new.n_l_term += 1;
                        builder.p_new.ws_flags |= WHERE_TOP_LIMIT;
                        builder.p_new.btree.n_top = 1;
                    }
                } else {
                    debug_assert!((e_op & (WO_LT | WO_LE)) != 0);
                    builder.p_new.ws_flags |= WHERE_COLUMN_RANGE | WHERE_TOP_LIMIT;
                    builder.p_new.btree.n_top = n_vec_len as u16;
                    p_top = Some(t);
                    p_btm = if (builder.p_new.ws_flags & WHERE_BTM_LIMIT) != 0 {
                        builder.p_new.a_l_term[builder.p_new.n_l_term as usize - 2]
                    } else {
                        None
                    };
                }
            }

            /* Neste ponto `p_new.n_out` é o número de linhas que se espera visitar na varredura do
            ** índice antes de considerar o termo pTerm, ou os valores de nIn e nInMul. Ou seja,
            ** supondo que todo "x IN(...)" fosse trocado por "x = ?". Este bloco atualiza `n_out`
            ** para levar em conta pTerm (mas não nIn/nInMul). */
            debug_assert!(builder.p_new.n_out == saved_n_out);
            if (builder.p_new.ws_flags & WHERE_COLUMN_RANGE) != 0 {
                /* Ajusta nOut com dados do stat4. Ou, sem stat4, com outra estimativa. */
                where_range_scan_est(parse, wi, p_btm, p_top, &mut builder.p_new);
            } else {
                builder.p_new.btree.n_eq += 1;
                let n_eq = builder.p_new.btree.n_eq as usize;
                debug_assert!((e_op & (WO_ISNULL | WO_EQ | WO_IN | WO_IS)) != 0);

                debug_assert!(builder.p_new.n_out == saved_n_out);
                if truth_prob <= 0 && p_probe.ai_column[saved_n_eq as usize] >= 0 {
                    debug_assert!((e_op & WO_IN) != 0 || n_in == 0);
                    builder.p_new.n_out = (builder.p_new.n_out as i32 + truth_prob as i32
                        - n_in as i32) as LogEst;
                } else {
                    let mut n_out = builder.p_new.n_out as i32;
                    n_out += p_probe.ai_row_log_est[n_eq] as i32
                        - p_probe.ai_row_log_est[n_eq - 1] as i32;
                    if (e_op & WO_ISNULL) != 0 {
                        /* TUNING: se não há valor de likelihood(), supõe-se que "col IS NULL"
                        ** casa o dobro de linhas de (col=?). */
                        n_out += 10;
                    }
                    builder.p_new.n_out = n_out as LogEst;
                }
            }

            /* Põe rCostIdx no custo estimado de visitar as linhas escolhidas no índice. A
            ** estimativa é a soma de (1) o custo de uma busca por chave para achar a primeira
            ** entrada que casa e (2) andar para a frente no índice `n_out` vezes para achar as
            ** demais. */
            debug_assert!(p_tab.sz_tab_row > 0);
            let mut r_cost_idx: LogEst;
            if p_probe.idx_type == SQLITE_IDXTYPE_IPK {
                /* O `szIdxRow` de um índice IPK é baixo porque as páginas interiores são
                ** pequenas, então ele dá uma boa estimativa do custo de busca. Mas as folhas têm o
                ** tamanho cheio, e `szIdxRow` subestimaria muito o custo da varredura. */
                r_cost_idx = (builder.p_new.n_out as i32 + 16) as LogEst;
            } else {
                r_cost_idx = (builder.p_new.n_out as i32
                    + 1
                    + (15 * p_probe.sz_idx_row as i32) / p_tab.sz_tab_row as i32)
                    as LogEst;
            }
            r_cost_idx = log_est_add(r_log_size, r_cost_idx);

            /* Estima o custo de rodar o loop. Se todos os dados vêm do índice, é só o custo da
            ** busca e da varredura. Se parte vem da tabela principal, soma-se o custo de fazer
            ** `n_out` buscas para localizar a linha da tabela de cada entrada do índice. */
            builder.p_new.r_run = r_cost_idx;
            if (builder.p_new.ws_flags & (WHERE_IDX_ONLY | WHERE_IPK | WHERE_EXPRIDX)) == 0 {
                builder.p_new.r_run =
                    log_est_add(builder.p_new.r_run, (builder.p_new.n_out as i32 + 16) as LogEst);
            }

            let n_out_unadjusted = builder.p_new.n_out; /* nOut antes de IN() e dos ajustes */
            builder.p_new.r_run =
                (builder.p_new.r_run as i32 + n_in_mul as i32 + n_in as i32) as LogEst;
            builder.p_new.n_out =
                (builder.p_new.n_out as i32 + n_in_mul as i32 + n_in as i32) as LogEst;
            where_loop_output_adjust(
                db,
                parse,
                wi,
                p_tab_list,
                builder.p_wc,
                &mut builder.p_new,
                r_size,
            );
            rc = where_loop_insert(wi, builder);

            if (builder.p_new.ws_flags & WHERE_COLUMN_RANGE) != 0 {
                builder.p_new.n_out = saved_n_out;
            } else {
                builder.p_new.n_out = n_out_unadjusted;
            }

            if (builder.p_new.ws_flags & WHERE_TOP_LIMIT) == 0
                && builder.p_new.btree.n_eq < p_probe.n_column
                && (builder.p_new.btree.n_eq < p_probe.n_key_col
                    || p_probe.idx_type != SQLITE_IDXTYPE_PRIMARYKEY)
            {
                if builder.p_new.btree.n_eq > 3 {
                    progress_check(parse, db);
                }
                where_loop_add_btree_index(
                    db,
                    parse,
                    wi,
                    p_tab_list,
                    builder,
                    p_src,
                    p_probe,
                    (n_in_mul as i32 + n_in as i32) as LogEst,
                );
            }
            builder.p_new.n_out = saved_n_out;
        }
        if rc != SQLITE_OK {
            break 'scan;
        }
        p_term = where_scan_next(db, parse, wi, &mut scan);
    }
    builder.p_new.prereq = saved_prereq;
    builder.p_new.btree.n_eq = saved_n_eq;
    builder.p_new.btree.n_btm = saved_n_btm;
    builder.p_new.btree.n_top = saved_n_top;
    builder.p_new.n_skip = saved_n_skip;
    builder.p_new.ws_flags = saved_ws_flags;
    builder.p_new.n_out = saved_n_out;
    builder.p_new.n_l_term = saved_n_l_term;

    /* Considera usar skip-scan se não há restrições do WHERE para os termos mais à esquerda do
    ** índice e se o número médio de repetições nos termos mais à esquerda é pelo menos 18.
    **
    ** O número mágico 18 vem de que varrer 17 linhas é quase sempre mais rápido que uma busca no
    ** índice (embora se assuma o contrário se o índice tem menos de 2^17 linhas). E, mesmo que
    ** não seja, não deve ser muito mais lento. Por outro lado, as buscas extras podem custar bem
    ** mais. */
    if saved_n_eq == saved_n_skip
        && saved_n_eq + 1 < p_probe.n_key_col
        && saved_n_eq == builder.p_new.n_l_term
        && !p_probe.no_skip_scan
        && p_probe.has_stat1
        && db.optimization_enabled(SQLITE_SKIP_SCAN)
        && p_probe.ai_row_log_est[saved_n_eq as usize + 1] >= 42 /* TUNING: mínimo para skip-scan */
        && {
            rc = { let n = builder.p_new.n_l_term as i32 + 1; where_loop_resize(&mut builder.p_new, n) };
            rc == SQLITE_OK
        }
    {
        builder.p_new.btree.n_eq += 1;
        builder.p_new.n_skip += 1;
        let nl = builder.p_new.n_l_term as usize;
        builder.p_new.a_l_term[nl] = None;
        builder.p_new.n_l_term += 1;
        builder.p_new.ws_flags |= WHERE_SKIPSCAN;
        let mut n_iter: LogEst = p_probe.ai_row_log_est[saved_n_eq as usize]
            - p_probe.ai_row_log_est[saved_n_eq as usize + 1];
        builder.p_new.n_out = builder.p_new.n_out.wrapping_sub(n_iter);
        /* TUNING: por causa das incertezas nas estimativas das consultas com skip-scan, soma-se um
        ** fator de 1,375 para torná-las um pouco menos prováveis. */
        n_iter += 5;
        where_loop_add_btree_index(
            db,
            parse,
            wi,
            p_tab_list,
            builder,
            p_src,
            p_probe,
            (n_iter as i32 + n_in_mul as i32) as LogEst,
        );
        builder.p_new.n_out = saved_n_out;
        builder.p_new.btree.n_eq = saved_n_eq;
        builder.p_new.n_skip = saved_n_skip;
        builder.p_new.ws_flags = saved_ws_flags;
    }

    rc
}

// ---------------------------------------------------------------------------------------------
// Chunk 009: ORDER BY por índice, índice parcial, cobertura e whereLoopAddBtree
// ---------------------------------------------------------------------------------------------

/// `indexMightHelpWithOrderBy`: verdadeiro se é possível que `p_index` ajude a implementar o
/// ORDER BY de `wi`. Falso se não há ORDER BY ou se o índice não tem como ajudar.
fn index_might_help_with_order_by(wi: &WhereInfo, p_index: &Index, i_cursor: i32) -> bool {
    if p_index.b_unordered {
        return false;
    }
    let Some(p_ob) = wi.p_order_by.as_deref() else {
        return false;
    };
    for ii in 0..p_ob.a.len() {
        let Some(p_expr) = expr_skip_collate_and_likely(p_ob.a[ii].p_expr.as_deref()) else {
            continue;
        };
        if (p_expr.op == TK_COLUMN || p_expr.op == TK_AGG_COLUMN) && p_expr.i_table == i_cursor {
            if p_expr.i_column < 0 {
                return true;
            }
            for jj in 0..p_index.n_key_col as usize {
                if p_expr.i_column == p_index.ai_column[jj] as i32 {
                    return true;
                }
            }
        } else if let Some(a_col_expr) = p_index.a_col_expr.as_deref() {
            for jj in 0..p_index.n_key_col as usize {
                if p_index.ai_column[jj] != XN_EXPR {
                    continue;
                }
                if expr_compare_skip(
                    Some(p_expr),
                    a_col_expr.a.get(jj).and_then(|it| it.p_expr.as_deref()),
                    i_cursor,
                ) == 0
                {
                    return true;
                }
            }
        }
    }
    false
}

/// `whereUsablePartialIndex`: verifica se um índice parcial com o WHERE `p_where` pode ser usado
/// na consulta atual. Devolve verdadeiro se pode.
fn where_usable_partial_index(
    db: &mut Connection,
    parse: &mut Parse,
    wi: &WhereInfo,
    wc: ClauseId,
    i_tab: i32,
    jointype: u8,
    p_where: &Expr,
) -> bool {
    if (jointype & JT_LTORJ) != 0 {
        return false;
    }
    let mut p_where = p_where;
    while p_where.op == TK_AND {
        if !where_usable_partial_index(
            db,
            parse,
            wi,
            wc,
            i_tab,
            jointype,
            p_where.p_left.as_deref().expect("pWhere->pLeft"),
        ) {
            return false;
        }
        p_where = p_where.p_right.as_deref().expect("pWhere->pRight");
    }
    /* Com SQLITE_EnableQPSG o C passa `pParse = 0` para a comparação. */
    let qpsg = (db.flags & SQLITE_ENABLE_QPSG) != 0;
    for i in 0..wi.n_term(wc) {
        let t = wi.term_at(wc, i);
        let p_expr = wi.expr(t);
        if (!p_expr.has_property(EP_OUTER_ON) || p_expr.i_join() == i_tab)
            && ((jointype & JT_OUTER) == 0 || p_expr.has_property(EP_OUTER_ON))
            && expr_implies_expr(
                if qpsg { None } else { Some((&mut *db, &mut *parse)) },
                Some(p_expr),
                Some(p_where),
                i_tab,
            ) != 0
            && (wi.term(t).wt_flags & TERM_VNULL) == 0
        {
            return true;
        }
    }
    false
}

/// `exprIsCoveredByIndex`: `p_idx` é um índice que contém expressões. Verifica se alguma das
/// expressões do índice casa com `p_expr`.
fn expr_is_covered_by_index(p_expr: &Expr, p_idx: &Index, i_tab_cur: i32) -> bool {
    for i in 0..p_idx.n_column as usize {
        if p_idx.ai_column[i] == XN_EXPR
            && expr_compare(
                None,
                Some(p_expr),
                p_idx.a_col_expr.as_deref().and_then(|l| l.a.get(i)).and_then(|it| it.p_expr.as_deref()),
                i_tab_cur,
            ) == 0
        {
            return true;
        }
    }
    false
}

/// `CoveringIndexCheck`: a estrutura passada ao callback de `whereIsCoveringIndex`
/// (`pWalk->u.pCovIdxCk`).
struct CoveringIndexCheck<'a> {
    /// O índice.
    p_idx: &'a Index,
    /// Número do cursor da tabela correspondente.
    i_tab_cur: i32,
    /// Usa uma expressão indexada.
    b_expr: bool,
    /// Usa uma coluna não indexada fora de uma expressão indexada.
    b_unidx: bool,
}

/// `whereIsCoveringIndexWalkCallback`: se o nó de expressão referencia a tabela de cursor
/// `ck.i_tab_cur`, garante que a coluna é coberta pelo índice. Sabe-se que as colunas menores que
/// 63 (BMS-1) são cobertas; só as de 63 em diante precisam ser conferidas.
///
/// Se o índice não cobre a coluna, devolve `WRC_ABORT`. Se o nó não refuta a cobertura, devolve
/// `WRC_CONTINUE`. Se o índice tem expressões indexadas e uma delas casa com o nó, poda a busca.
fn where_is_covering_index_walk_callback(
    w: &mut Walker<CoveringIndexCheck<'_>>,
    p_expr: &mut Expr,
) -> i32 {
    let ck = &mut w.u;
    if p_expr.op == TK_COLUMN || p_expr.op == TK_AGG_COLUMN {
        if p_expr.i_table != ck.i_tab_cur {
            return WRC_CONTINUE;
        }
        for i in 0..ck.p_idx.n_column as usize {
            if ck.p_idx.ai_column[i] as i32 == p_expr.i_column {
                return WRC_CONTINUE;
            }
        }
        ck.b_unidx = true;
        return WRC_ABORT;
    } else if ck.p_idx.b_has_expr && expr_is_covered_by_index(p_expr, ck.p_idx, ck.i_tab_cur) {
        ck.b_expr = true;
        return WRC_PRUNE;
    }
    WRC_CONTINUE
}

/// `whereIsCoveringIndex`: `p_idx` é um índice que cobre todas as colunas de número baixo usadas
/// pelo SELECT (colunas de 0 a 62) ou um índice com termos de expressão. Então não dá para saber se
/// ele é de cobertura pelas máscaras `colUsed`: é preciso uma busca, que esta rotina faz.
///
/// O resultado é um destes: 0 (definitivamente não é de cobertura), `WHERE_IDX_ONLY`
/// (definitivamente é) ou `WHERE_EXPRIDX` (provavelmente é, mas é difícil saber por causa das
/// expressões indexadas; pontua-se como de cobertura mas mantém-se a tabela principal aberta).
///
/// É uma otimização: devolver zero é sempre seguro.
fn where_is_covering_index(
    p_select: Option<&mut Select>,
    p_where: Option<&mut Expr>,
    p_idx: &Index,
    i_tab_cur: i32,
) -> u32 {
    let Some(p_select) = p_select else {
        /* Não há acesso à consulta completa: supõe-se que não é de cobertura. */
        return 0;
    };
    if !p_idx.b_has_expr {
        let mut i = 0usize;
        while i < p_idx.n_column as usize {
            if p_idx.ai_column[i] as i32 >= crate::consts::BMS - 1 {
                break;
            }
            i += 1;
        }
        if i >= p_idx.n_column as usize {
            /* pIdx não indexa colunas maiores que 62, mas sabe-se por colMask que há colunas
            ** maiores que 62 em uso: não é de cobertura. */
            return 0;
        }
    }
    let mut w = Walker {
        x_expr_callback: Some(where_is_covering_index_walk_callback),
        x_select_callback: Some(select_walk_noop::<CoveringIndexCheck<'_>>),
        x_select_callback2: None,
        walker_depth: 0,
        e_code: 0,
        m_w_flags: 0,
        u: CoveringIndexCheck { p_idx, i_tab_cur, b_expr: false, b_unidx: false },
    };
    walk_select(&mut w, Some(p_select));
    if w.u.b_unidx == false {
        walk_expr(&mut w, p_where);
    }
    if w.u.b_unidx {
        0
    } else if w.u.b_expr {
        WHERE_EXPRIDX
    } else {
        WHERE_IDX_ONLY
    }
}

/// `wherePartIdxExpr`: chamada para um índice parcial (com cláusula WHERE) em dois cenários.
/// Nos dois, determina se o WHERE do índice implica que uma coluna da tabela pode ser trocada com
/// segurança por uma expressão constante. Por exemplo:
///
/// ```text
///   CREATE INDEX i1 ON t1(b, c) WHERE a=<expr>;
///   SELECT a, b, c FROM t1 WHERE a=<expr> AND b=?;
/// ```
///
/// o "a" da lista de resultados pode ser trocado por <expr> se (a) <expr> é constante, (b) a
/// comparação (a=<expr>) usa a colação BINARY e (c) a coluna "a" tem afinidade diferente de NONE
/// ou BLOB.
///
/// Se `p_item` é nulo, `p_mask` não pode ser: a função está sendo chamada para decidir se o índice
/// é de cobertura, e limpa em `*p_mask` os bits das colunas que podem virar constantes. Senão, com
/// `p_item`, a função está codificando um loop que usa o índice: acrescenta entradas a
/// `parse.p_idx_part_expr` para cada coluna que pode virar constante.
///
/// O `pIdx` do C só servia para `pIdx->pTable`: aqui chega a `p_tab`. Em `p_idx_part_expr` o mais
/// novo é o último. O `sqlite3ParserAddCleanup(whereIndexedExprCleanup)` não existe: o `Parse` é o
/// dono do `Vec`.
pub(crate) fn where_part_idx_expr(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab: &Table,
    p_part: &Expr,
    mut p_mask: Option<&mut Bitmask>,
    i_idx_cur: i32,
    p_item: Option<&SrcItem>,
) {
    debug_assert!(p_item.map_or(true, |i| (i.fg.jointype & JT_RIGHT) == 0));
    debug_assert!(
        (p_item.is_none() || p_mask.is_none()) && (p_mask.is_some() || p_item.is_some())
    );

    let mut p_part = p_part;
    if p_part.op == TK_AND {
        where_part_idx_expr(
            db,
            parse,
            p_tab,
            p_part.p_right.as_deref().expect("pPart->pRight"),
            p_mask.as_deref_mut(),
            i_idx_cur,
            p_item,
        );
        p_part = p_part.p_left.as_deref().expect("pPart->pLeft");
    }

    if p_part.op == TK_EQ || p_part.op == TK_IS {
        let p_left = p_part.p_left.as_deref().expect("pPart->pLeft");
        let p_right = p_part.p_right.as_deref().expect("pPart->pRight");

        if p_left.op != TK_COLUMN {
            return;
        }
        let mut right_copy = p_right.clone();
        if expr_is_constant(None, Some(&mut right_copy)) == 0 {
            return;
        }
        if !is_binary(expr_compare_coll_seq(db, parse, p_part, None).as_ref()) {
            return;
        }
        if p_left.i_column < 0 {
            return;
        }
        let aff = p_tab.a_col[p_left.i_column as usize].affinity;
        if aff >= SQLITE_AFF_TEXT {
            if let Some(p_item) = p_item {
                let b_null_row = (p_item.fg.jointype & (JT_LEFT | JT_LTORJ)) != 0;
                parse.p_idx_part_expr.push(IndexedExpr {
                    p_expr: expr_dup(Some(p_right), 0),
                    i_data_cur: p_item.i_cursor,
                    i_idx_cur,
                    i_idx_col: p_left.i_column,
                    b_maybe_null_row: b_null_row,
                    aff,
                    z_idx_name: None,
                });
            } else if p_left.i_column < crate::consts::BMS - 1 {
                if let Some(m) = p_mask {
                    *m &= !(1u64 << p_left.i_column);
                }
            }
        }
    }
}

/// `whereLoopAddBtree`: acrescenta todos os objetos `WhereLoop` de uma tabela da junção
/// identificada por `builder.p_new.i_tab`. A tabela é garantidamente uma tabela b-tree, não uma
/// virtual.
///
/// Os custos (`rRun`) dos loops de b-tree acrescentados por esta função são calculados assim.
/// Numa varredura completa, supondo que a tabela (ou índice) tem nRow linhas:
///
/// ```text
///     custo = nRow * 3.0                    // varredura da tabela inteira
///     custo = nRow * K                      // varredura de índice de cobertura
///     custo = nRow * (K+3.0)                // varredura de índice sem cobertura
/// ```
///
/// onde K vale entre 1.1 e 3.0, conforme o tamanho médio relativo dos registros do índice e da
/// tabela. Numa varredura por índice, com nVisit linhas visitadas e nSeek buscas no b-tree:
///
/// ```text
///     custo = nSeek * (log(nRow) + K * nVisit)          // índice de cobertura
///     custo = nSeek * (log(nRow) + (K+3.0) * nVisit)    // índice sem cobertura
/// ```
///
/// Normalmente nSeek é 1. Valores maiores vêm de termos "x IN (....)" no lugar de "x=?", ou de
/// "x IN (SELECT x FROM tbl)" implícitos dos skip-scans.
///
/// As estimativas (nRow, nVisit, nSeek) têm muita incerteza, então a pontuação procura planos que
/// "façam o menor mal" se estiverem imprecisas: por exemplo, o fator log(nRow) é omitido da
/// varredura de índice sem cobertura para favorecer o uso de índice.
pub(crate) fn where_loop_add_btree(
    db: &mut Connection,
    parse: &mut Parse,
    wi: &mut WhereInfo,
    p_tab_list: &SrcList,
    mut p_select: Option<&mut Select>,
    builder: &mut WhereLoopBuilder,
    m_prereq: Bitmask,
) -> i32 {
    let mut rc = SQLITE_OK;
    let mut i_sort_idx: i32 = 1; /* Número do índice */
    let i_tab = builder.p_new.i_tab as usize;
    let p_src: &SrcItem = &p_tab_list.a[i_tab];
    let p_tab: Rc<Table> = p_src.p_tab.clone().expect("pSrc->pTab");
    let wc = builder.p_wc;
    debug_assert!(!p_tab.is_virtual());

    /* A lista dos índices a considerar, na ordem da cadeia `pNext` do C. */
    let probes: Vec<Rc<Index>> = if p_src.fg.is_indexed_by {
        debug_assert!(!p_src.fg.is_cte);
        /* Um INDEXED BY especifica um índice em particular */
        match &p_src.u2 {
            SrcU2::IbIndex(Some(ix)) => vec![ix.clone()],
            _ => Vec::new(),
        }
    } else if !p_tab.has_rowid() {
        p_tab.p_index.clone()
    } else {
        /* Sem INDEXED BY: cria um Index falso `sPk` para representar o índice da chave primária
        ** rowid, como o primeiro de uma cadeia com todos os índices reais a seguir. */
        let s_pk = Index {
            n_key_col: 1,
            n_column: 1,
            ai_column: vec![-1],
            ai_row_log_est: vec![p_tab.n_row_log_est, 0],
            a_sort_order: vec![0],
            on_error: OE_REPLACE,
            sz_idx_row: 3, /* TUNING: as linhas interiores de uma tabela IPK são bem pequenas */
            idx_type: SQLITE_IDXTYPE_IPK,
            ..Index::default()
        };
        let mut v = vec![Rc::new(s_pk)];
        if !p_src.fg.not_indexed {
            /* Os índices reais só são considerados se o NOT INDEXED foi omitido do FROM */
            v.extend(p_tab.p_index.iter().cloned());
        }
        v
    };
    let mut r_size: LogEst = p_tab.n_row_log_est;

    /* Índices automáticos */
    if builder.p_or_set.is_none() /* Não faz parte de uma otimização de OR */
        && ((wi.wctrl_flags as u32) & (WHERE_RIGHT_JOIN | WHERE_OR_SUBCLAUSE)) == 0
        && (db.flags & SQLITE_AUTO_INDEX) != 0
        && !p_src.fg.is_indexed_by /* Sem INDEXED BY */
        && !p_src.fg.not_indexed /* Sem NOT INDEXED */
        && p_tab.has_rowid() /* Não é WITHOUT ROWID. (FIXME: por quê?) */
        && !p_src.fg.is_correlated /* Não é subconsulta correlacionada */
        && !p_src.fg.is_recursive /* Não é uma CTE recursiva. */
        && (p_src.fg.jointype & JT_RIGHT) == 0
    /* Não é a tabela da direita de um RIGHT JOIN */
    {
        /* Gera os WhereLoops de índice automático */
        let r_log_size = est_log(r_size); /* Logaritmo do número de linhas da tabela */
        let n_wc = wi.n_term(wc);
        for i in 0..n_wc {
            if rc != SQLITE_OK {
                break;
            }
            let t = wi.term_at(wc, i);
            if (wi.term(t).prereq_right & builder.p_new.mask_self) != 0 {
                continue;
            }
            if term_can_drive_index(wi, t, p_src, 0) {
                where_loop_resize(&mut builder.p_new, 1);
                builder.p_new.btree.n_eq = 1;
                builder.p_new.n_skip = 0;
                builder.p_new.btree.p_index = None;
                builder.p_new.n_l_term = 1;
                builder.p_new.a_l_term[0] = Some(t);
                /* TUNING: o custo único de calcular o índice automático é estimado em
                ** X*N*log2(N), onde N é o número de linhas da tabela indexada e X é 7 (LogEst=28)
                ** para tabelas normais ou 0,5 (LogEst=-10) para views e subconsultas. X é menor
                ** para views e subconsultas para que o planejador seja mais agressivo em gerar
                ** índices automáticos para elas, já que não há como acrescentar índices do
                ** esquema a subconsultas e views. */
                let mut r_setup: i32 = r_log_size as i32 + r_size as i32;
                if !p_tab.is_view() && (p_tab.tab_flags & TF_EPHEMERAL) == 0 {
                    r_setup += 28;
                } else {
                    r_setup -= 25; /* Custo bem reduzido para índices automáticos em
                                   ** materializações efêmeras de views */
                }
                if r_setup < 0 {
                    r_setup = 0;
                }
                builder.p_new.r_setup = r_setup as LogEst;
                /* TUNING: cada busca no índice rende 20 linhas da tabela. É mais que o chute
                ** usual de 10, já que não há como saber o quão seletivo o índice será. Não seria
                ** absurdo aumentar bastante esse valor. */
                builder.p_new.n_out = 43; /* 43==sqlite3LogEst(20) */
                builder.p_new.r_run = log_est_add(r_log_size, builder.p_new.n_out);
                builder.p_new.ws_flags = WHERE_AUTO_INDEX;
                builder.p_new.prereq = m_prereq | wi.term(t).prereq_right;
                rc = where_loop_insert(wi, builder);
            }
        }
    }

    /* Percorre todos os índices. Se há INDEXED BY, só se considera o índice p_probe. */
    for p_probe in probes.iter() {
        if rc != SQLITE_OK {
            break;
        }
        let this_sort_idx = i_sort_idx;
        i_sort_idx += 1;
        if let Some(p_part_where) = p_probe.p_partial_idx_where.as_deref() {
            if !where_usable_partial_index(
                db,
                parse,
                wi,
                wc,
                p_src.i_cursor,
                p_src.fg.jointype,
                p_part_where,
            ) {
                continue; /* Índice parcial inadequado para esta consulta */
            }
        }
        if p_probe.b_no_query {
            continue;
        }
        r_size = p_probe.ai_row_log_est[0];
        builder.p_new.btree.n_eq = 0;
        builder.p_new.btree.n_btm = 0;
        builder.p_new.btree.n_top = 0;
        builder.p_new.n_skip = 0;
        builder.p_new.n_l_term = 0;
        builder.p_new.i_sort_idx = 0;
        builder.p_new.r_setup = 0;
        builder.p_new.prereq = m_prereq;
        builder.p_new.n_out = r_size;
        builder.p_new.btree.p_index = Some(p_probe.clone());
        let b = index_might_help_with_order_by(wi, p_probe, p_src.i_cursor);

        /* O ONEPASS_DESIRED nunca ocorre junto com ORDER BY */
        debug_assert!((wi.wctrl_flags as u32 & WHERE_ONEPASS_DESIRED) == 0 || !b);
        if p_probe.idx_type == SQLITE_IDXTYPE_IPK {
            /* Índice da chave primária inteira */
            builder.p_new.ws_flags = WHERE_IPK;

            /* Varredura completa da tabela */
            builder.p_new.i_sort_idx = if b { this_sort_idx as u8 } else { 0 };
            /* TUNING: o custo da varredura completa é 3.0*N. O fator 3.0 é um custo extra para
            ** desencorajar varreduras completas, já que buscas por índice têm melhor pior caso se
            ** os chutes de estatística estiverem errados. (Com STAT4 o fator cairia para 2.75; o
            ** Debian não compila STAT4.) */
            builder.p_new.r_run = (r_size as i32 + 16) as LogEst;
            where_loop_output_adjust(
                db,
                parse,
                wi,
                p_tab_list,
                wc,
                &mut builder.p_new,
                r_size,
            );
            rc = where_loop_insert(wi, builder);
            builder.p_new.n_out = r_size;
            if rc != SQLITE_OK {
                break;
            }
        } else {
            let mut m: Bitmask;
            if p_probe.is_covering {
                m = 0;
                builder.p_new.ws_flags = WHERE_IDX_ONLY | WHERE_INDEXED;
            } else {
                m = p_src.col_used & p_probe.col_not_idxed;
                if let Some(p_part_where) = p_probe.p_partial_idx_where.as_deref() {
                    where_part_idx_expr(db, parse, &p_tab, p_part_where, Some(&mut m), 0, None);
                }
                builder.p_new.ws_flags = WHERE_INDEXED;
                if m == TOPBIT || (p_probe.b_has_expr && !p_probe.b_has_v_col && m != 0) {
                    let where_ref = wi.where_root.clone();
                    let is_cov = where_is_covering_index(
                        p_select.as_deref_mut(),
                        where_ref.and_then(|r| wi.exprs.get_mut(&r)),
                        p_probe,
                        p_src.i_cursor,
                    );
                    if is_cov == 0 {
                        debug_assert!(m != 0);
                    } else {
                        m = 0;
                        builder.p_new.ws_flags |= is_cov;
                        debug_assert!((is_cov & WHERE_IDX_ONLY) != 0 || is_cov == WHERE_EXPRIDX);
                    }
                } else if m == 0 && (p_tab.has_rowid() || p_select.is_some()) {
                    builder.p_new.ws_flags = WHERE_IDX_ONLY | WHERE_INDEXED;
                }
            }

            /* Varredura completa pelo índice */
            if b
                || !p_tab.has_rowid()
                || p_probe.p_partial_idx_where.is_some()
                || p_src.fg.is_indexed_by
                || (m == 0
                    && !p_probe.b_unordered
                    && (p_probe.sz_idx_row < p_tab.sz_tab_row)
                    && ((wi.wctrl_flags as u32) & WHERE_ONEPASS_DESIRED) == 0
                    && crate::global::use_cis()
                    && db.optimization_enabled(SQLITE_COVER_IDX_SCAN))
            {
                builder.p_new.i_sort_idx = if b { this_sort_idx as u8 } else { 0 };

                /* O custo de visitar as linhas do índice é N*K, onde K vale entre 1.1 e 3.0,
                ** conforme o tamanho relativo das linhas do índice e da tabela. */
                debug_assert!(p_tab.sz_tab_row > 0);
                builder.p_new.r_run = (r_size as i32
                    + 1
                    + (15 * p_probe.sz_idx_row as i32) / p_tab.sz_tab_row as i32)
                    as LogEst;
                if m != 0 {
                    /* Se é varredura de índice sem cobertura, soma-se o custo das buscas na
                    ** tabela. O custo será 3x o número de buscas. Levam-se em conta os termos do
                    ** WHERE que podem ser satisfeitos só com o índice e que não exigem busca na
                    ** tabela. */
                    let mut n_lookup: i32 = r_size as i32 + 16; /* Custo base: N*3 */
                    let i_cur = p_src.i_cursor;
                    let wc2 = wi.s_wc;
                    for ii in 0..wi.n_term(wc2) {
                        let t = wi.term_at(wc2, ii);
                        if expr_covered_by_index(wi.expr_mut(t), i_cur, p_probe) == 0 {
                            break;
                        }
                        /* pTerm pode ser avaliado só com o índice. Reduz-se o número esperado de
                        ** buscas na tabela. */
                        let term = wi.term(t);
                        if term.truth_prob <= 0 {
                            n_lookup += term.truth_prob as i32;
                        } else {
                            n_lookup -= 1;
                            if (term.e_operator & (WO_EQ | WO_IS)) != 0 {
                                n_lookup -= 19;
                            }
                        }
                    }

                    builder.p_new.r_run = log_est_add(builder.p_new.r_run, n_lookup as LogEst);
                }
                where_loop_output_adjust(
                    db,
                    parse,
                    wi,
                    p_tab_list,
                    wc,
                    &mut builder.p_new,
                    r_size,
                );
                if (p_src.fg.jointype & JT_RIGHT) != 0 && p_probe.a_col_expr.is_some() {
                    /* Não faz SCAN de índice sobre expressão num RIGHT JOIN, porque o cursor que
                    ** acessa o índice pode não estar posicionado na linha certa durante o laço
                    ** sem casamento do right-join. */
                } else {
                    rc = where_loop_insert(wi, builder);
                }
                builder.p_new.n_out = r_size;
                if rc != SQLITE_OK {
                    break;
                }
            }
        }

        builder.bld_flags1 = 0;
        rc = where_loop_add_btree_index(db, parse, wi, p_tab_list, builder, p_src, p_probe, 0);
        if builder.bld_flags1 == SQLITE_BLDF1_INDEXED {
            /* Se um índice não único é usado, ou um prefixo da chave de um índice único (o que o
            ** torna funcionalmente não único), os dados do sqlite_stat1 passam a importar na
            ** pontuação do plano. */
            crate::build::table_flags_or(db, &p_tab, TF_MAYBE_REANALYZE);
        }
    }
    rc
}

// ---------------------------------------------------------------------------------------------
// Chunk 010: tabelas virtuais
// ---------------------------------------------------------------------------------------------

/// `isLimitTerm`: verdadeiro se `p_term` é um termo LIMIT ou OFFSET de tabela virtual.
fn is_limit_term(p_term: &crate::where_int::WhereTerm) -> bool {
    debug_assert!(p_term.e_operator == crate::consts::WO_AUX || p_term.e_match_op == 0);
    (p_term.e_match_op as i32) >= SQLITE_INDEX_CONSTRAINT_LIMIT
        && (p_term.e_match_op as i32) <= SQLITE_INDEX_CONSTRAINT_OFFSET
}

/// `allConstraintsUsed`: verdadeiro se as primeiras `n_cons` restrições do vetor `a_usage` estão
/// marcadas como em uso (`argvIndex>0`).
fn all_constraints_used(a_usage: &[IndexConstraintUsage], n_cons: usize) -> bool {
    for u in a_usage.iter().take(n_cons) {
        if u.argv_index <= 0 {
            return false;
        }
    }
    true
}

/// `whereLoopAddVirtualOne`: `p_idx_info` já está preenchido com todas as restrições que a tabela
/// virtual identificada por `builder.p_new.i_tab` pode usar. A função marca um subconjunto delas
/// como utilizável, chama o `xBestIndex` e acrescenta ao construtor o plano devolvido.
///
/// Uma restrição é marcada utilizável se (1) `m_usable` indica que seus pré-requisitos estão
/// disponíveis e (2) ela não usa um dos operadores da máscara `m_exclude` (na prática `WO_IN` ou
/// 0). `m_prereq` é a máscara das tabelas que devem ser varridas antes da virtual: entra nos
/// pré-requisitos do plano antes de ele ir para o construtor.
///
/// `*pb_in` sai verdadeiro se o plano acrescentado usa um ou mais termos `WO_IN`. `pb_retry_limit`
/// é o `int *pbRetryLimit` do C: `None` é o ponteiro nulo.
fn where_loop_add_virtual_one(
    db: &mut Connection,
    parse: &mut Parse,
    wi: &mut WhereInfo,
    p_tab_list: &SrcList,
    builder: &mut WhereLoopBuilder,
    m_prereq: Bitmask,
    m_usable: Bitmask,
    m_exclude: u16,
    p_idx_info: &mut IndexInfo,
    p_hidden: &mut HiddenIndexInfo,
    m_no_omit: u16,
    pb_in: &mut bool,
    mut pb_retry_limit: Option<&mut bool>,
) -> i32 {
    let wc = builder.p_wc;
    let p_src = &p_tab_list.a[builder.p_new.i_tab as usize];
    let p_tab: Rc<Table> = p_src.p_tab.clone().expect("pSrc->pTab");
    let n_constraint = p_idx_info.a_constraint.len();

    debug_assert!((m_usable & m_prereq) == m_prereq);
    *pb_in = false;
    builder.p_new.prereq = m_prereq;

    /* Liga o flag usable no subconjunto de restrições identificado por m_usable e m_exclude. */
    for i in 0..n_constraint {
        let t = wi.term_at(wc, p_idx_info.a_constraint[i].i_term_offset as usize);
        let p_term = wi.term(t);
        p_idx_info.a_constraint[i].usable = false;
        if (p_term.prereq_right & m_usable) == p_term.prereq_right
            && (p_term.e_operator & m_exclude) == 0
            && (pb_retry_limit.is_some() || !is_limit_term(p_term))
        {
            p_idx_info.a_constraint[i].usable = true;
        }
    }

    /* Inicializa os campos de saída da estrutura sqlite3_index_info */
    p_idx_info.a_constraint_usage.clear();
    p_idx_info.a_constraint_usage.resize(n_constraint, IndexConstraintUsage::default());
    p_idx_info.idx_str = None;
    p_idx_info.idx_num = 0;
    p_idx_info.order_by_consumed = 0;
    p_idx_info.estimated_cost = SQLITE_BIG_DBL / 2.0;
    p_idx_info.estimated_rows = 25;
    p_idx_info.idx_flags = 0;
    p_idx_info.col_used = p_src.col_used;
    p_hidden.m_handle_in = 0;

    /* Chama o método xBestIndex() da tabela virtual */
    let rc = vtab_best_index(db, parse, &p_tab, p_idx_info);
    if rc != SQLITE_OK {
        if rc == SQLITE_CONSTRAINT {
            /* Se o xBestIndex devolve SQLITE_CONSTRAINT, essa combinação particular de parâmetros
            ** é inutilizável. Não faz entradas na tabela de loops. */
            return SQLITE_OK;
        }
        return rc;
    }

    let mut mx_term: i32 = -1;
    debug_assert!(builder.p_new.a_l_term.len() >= n_constraint);
    for k in 0..n_constraint {
        builder.p_new.a_l_term[k] = None;
    }
    /* `memset(&pNew->u.vtab, 0, ...)` zera também o `u.btree` que divide a união. */
    builder.p_new.vtab = Default::default();
    builder.p_new.btree = Default::default();
    for i in 0..n_constraint {
        let i_term = p_idx_info.a_constraint_usage[i].argv_index - 1;
        if i_term >= 0 {
            let j = p_idx_info.a_constraint[i].i_term_offset;
            if i_term as usize >= n_constraint
                || j < 0
                || j as usize >= wi.n_term(wc)
                || builder.p_new.a_l_term[i_term as usize].is_some()
                || !p_idx_info.a_constraint[i].usable
            {
                error_msg(
                    db,
                    parse,
                    b"%s.xBestIndex malfunction",
                    &[PrintfArg::Text(Some(p_tab.z_name.clone()))],
                );
                return SQLITE_ERROR;
            }
            let t = wi.term_at(wc, j as usize);
            builder.p_new.prereq |= wi.term(t).prereq_right;
            builder.p_new.a_l_term[i_term as usize] = Some(t);
            if i_term > mx_term {
                mx_term = i_term;
            }
            if p_idx_info.a_constraint_usage[i].omit {
                if i < 16 && ((1u32 << i) & m_no_omit as u32) == 0 {
                    if i_term < 16 {
                        builder.p_new.vtab.omit_mask |= 1u16 << i_term;
                    }
                }
                if wi.term(t).e_match_op as i32 == SQLITE_INDEX_CONSTRAINT_OFFSET {
                    builder.p_new.vtab.b_omit_offset = true;
                }
            }
            if i < 32 && ((1u32 << i) & p_hidden.m_handle_in) != 0 {
                if i_term < 32 {
                    builder.p_new.vtab.m_handle_in |= 1u32 << i_term;
                }
            } else if (wi.term(t).e_operator & WO_IN) != 0 {
                /* Uma tabela virtual restringida por um IN não pode consumir o ORDER BY porque
                ** (1) a ordem dos termos do IN não tem relação necessária com a ordem dos termos
                ** de saída e (2) várias saídas de um mesmo valor do IN não se intercalam. */
                p_idx_info.order_by_consumed = 0;
                p_idx_info.idx_flags &= !SQLITE_INDEX_SCAN_UNIQUE;
                *pb_in = true;
                debug_assert!((m_exclude & WO_IN) == 0);
            }

            /* Sem pb_retry_limit não deve haver termos LIMIT/OFFSET. E, se houver, devem vir
            ** depois de todos os outros termos. */
            debug_assert!(pb_retry_limit.is_some() || !is_limit_term(wi.term(t)));
            debug_assert!(!is_limit_term(wi.term(t)) || i + 2 >= n_constraint);

            if is_limit_term(wi.term(t))
                && (*pb_in || !all_constraints_used(&p_idx_info.a_constraint_usage, i))
            {
                /* Se há um termo IN(...) tratado como == (uma chamada separada ao xFilter para
                ** cada valor do lado direito do IN) e também um termo LIMIT ou OFFSET tratado, o
                ** plano é inutilizável. Idem se há LIMIT/OFFSET e outros termos sem uso. Nesses
                ** casos põe-se `*pb_retry_limit` para dizer ao chamador que tente de novo com
                ** LIMIT e OFFSET desligados. */
                p_idx_info.idx_str = None;
                if let Some(r) = pb_retry_limit.as_deref_mut() {
                    *r = true;
                }
                return SQLITE_OK;
            }
        }
    }

    builder.p_new.n_l_term = (mx_term + 1) as u16;
    for i in 0..=(mx_term as isize) {
        if i >= 0 && builder.p_new.a_l_term[i as usize].is_none() {
            /* Os valores argvIdx diferentes de zero devem ser contíguos. Gera erro se não forem. */
            error_msg(
                db,
                parse,
                b"%s.xBestIndex malfunction",
                &[PrintfArg::Text(Some(p_tab.z_name.clone()))],
            );
            return SQLITE_ERROR;
        }
    }
    debug_assert!(builder.p_new.n_l_term as usize <= builder.p_new.a_l_term.len());
    builder.p_new.vtab.idx_num = p_idx_info.idx_num;
    builder.p_new.vtab.need_free = p_idx_info.idx_str.is_some();
    builder.p_new.vtab.idx_str = p_idx_info.idx_str.take();
    builder.p_new.vtab.is_ordered = if p_idx_info.order_by_consumed != 0 {
        p_idx_info.a_order_by.len() as i8
    } else {
        0
    };
    builder.p_new.r_setup = 0;
    builder.p_new.r_run = log_est_from_double(p_idx_info.estimated_cost);
    builder.p_new.n_out = log_est(p_idx_info.estimated_rows as u64);

    /* Liga WHERE_ONEROW se o xBestIndex() indicou que a varredura visita no máximo uma linha.
    ** Senão, desliga. */
    if (p_idx_info.idx_flags & SQLITE_INDEX_SCAN_UNIQUE) != 0 {
        builder.p_new.ws_flags |= WHERE_ONEROW;
    } else {
        builder.p_new.ws_flags &= !WHERE_ONEROW;
    }
    let rc = where_loop_insert(wi, builder);
    if builder.p_new.vtab.need_free {
        builder.p_new.vtab.idx_str = None;
        builder.p_new.vtab.need_free = false;
    }
    rc
}

/// `sqlite3_vtab_collation`: a colação de uma restrição passada ao xBestIndex.
///
/// Devolve o nome da colação: (1) se há um COLLATE explícito na restrição, esse; (2) senão, se a
/// coluna tem colação alternativa, essa; (3) senão, "BINARY". Devolve `None` se `i_cons` está fora
/// de faixa. O `HiddenIndexInfo` que no C vem logo depois do `sqlite3_index_info` é o
/// `p_hidden`; `db` e `parse` são o `pHidden->pParse`.
pub fn vtab_collation(
    db: &mut Connection,
    parse: &mut Parse,
    wi: &WhereInfo,
    p_idx_info: &IndexInfo,
    p_hidden: &HiddenIndexInfo,
    i_cons: i32,
) -> Option<Vec<u8>> {
    if i_cons >= 0 && (i_cons as usize) < p_idx_info.a_constraint.len() {
        let i_term = p_idx_info.a_constraint[i_cons as usize].i_term_offset;
        let t = wi.term_at(p_hidden.p_wc, i_term as usize);
        let p_x = wi.expr(t);
        let p_c = if p_x.p_left.is_some() {
            expr_compare_coll_seq(db, parse, p_x, None)
        } else {
            None
        };
        Some(match p_c {
            Some(c) => c.name.clone(),
            None => b"BINARY".to_vec(),
        })
    } else {
        None
    }
}

/// `SMASKBIT32(n)`: a máscara do bit `n`, ou 0 se `n` não cabe em 32 bits.
#[inline]
fn smaskbit32(n: i32) -> u32 {
    if (0..=31).contains(&n) {
        1u32 << n
    } else {
        0
    }
}

/// `sqlite3_vtab_in`: verdadeiro se a restrição `i_cons` é mesmo uma restrição IN(...). Se for, liga
/// (com `b_handle>0`) ou desliga (com `b_handle==0`) o flag para tratá-la com um iterador.
pub fn vtab_in(p_hidden: &mut HiddenIndexInfo, i_cons: i32, b_handle: i32) -> i32 {
    let m = smaskbit32(i_cons);
    if (m & p_hidden.m_in) != 0 {
        if b_handle == 0 {
            p_hidden.m_handle_in &= !m;
        } else if b_handle > 0 {
            p_hidden.m_handle_in |= m;
        }
        return 1;
    }
    0
}

/// `sqlite3_vtab_rhs_value`: chamável só de dentro do callback xBestIndex. Se possível, devolve o
/// valor do lado direito da restrição `i_cons`. O resultado é `(rc, valor)`: `rc` é `SQLITE_OK`,
/// `SQLITE_MISUSE` (restrição fora de faixa) ou `SQLITE_NOTFOUND` (sem valor, ou erro de
/// `sqlite3ValueFromExpr`).
pub fn vtab_rhs_value<'a>(
    db: &mut Connection,
    wi: &WhereInfo,
    p_idx_info: &IndexInfo,
    p_hidden: &'a mut HiddenIndexInfo,
    i_cons: i32,
) -> (i32, Option<&'a crate::mem::Mem>) {
    let mut rc = SQLITE_OK;
    let mut have = false;
    if i_cons < 0 || i_cons as usize >= p_idx_info.a_constraint.len() {
        rc = SQLITE_MISUSE; /* EV: R-30545-25046 */
    } else {
        let idx = i_cons as usize;
        if p_hidden.a_rhs.len() <= idx {
            p_hidden.a_rhs.resize_with(idx + 1, || None);
        }
        if p_hidden.a_rhs[idx].is_none() {
            let t = wi.term_at(p_hidden.p_wc, p_idx_info.a_constraint[idx].i_term_offset as usize);
            let enc = db.enc;
            if let Some(p_right) = wi.expr(t).p_right.as_deref() {
                rc = crate::mem::value_from_expr(
                    db,
                    p_right,
                    enc,
                    SQLITE_AFF_BLOB,
                    &mut p_hidden.a_rhs[idx],
                );
            }
        }
        have = true;
    }
    let p_val = if have { p_hidden.a_rhs[i_cons as usize].as_ref() } else { None };
    if rc == SQLITE_OK && p_val.is_none() {
        rc = SQLITE_NOTFOUND; /* IMP: R-19933-32160, R-36424-56542 */
    }
    (rc, p_val)
}

/// `sqlite3_vtab_distinct`: verdadeiro se o ORDER BY pode ser tratado como DISTINCT.
pub fn vtab_distinct(p_hidden: &HiddenIndexInfo) -> i32 {
    debug_assert!((0..=3).contains(&p_hidden.e_distinct));
    p_hidden.e_distinct
}

/// `sqlite3VtabUsesAllSchemas`: faz o comando preparado, associado a uma chamada do xBestIndex,
/// usar potencialmente todos os esquemas. Se o comando é só de leitura, inicia transações de
/// leitura em todos os esquemas; se é de escrita, de escrita. Usado pela tabela virtual embutida
/// sqlite_dbpage.
pub fn vtab_uses_all_schemas(db: &mut Connection, parse: &mut Parse) {
    let n_db = db.dbs.len();
    for i in 0..n_db {
        crate::build3::code_verify_schema(db, parse, i as i32);
    }
    if parse.write_mask != 0 {
        for i in 0..n_db {
            crate::build3::begin_write_operation(db, parse, 0, i as i32);
        }
    }
}

/// `whereLoopAddVirtual`: acrescenta todos os `WhereLoop` de uma tabela da junção identificada por
/// `builder.p_new.i_tab`. A tabela é garantidamente virtual.
///
/// Se não há LEFT ou CROSS JOIN na consulta, `m_prereq` e `m_unusable` são 0. Senão, `m_prereq` é a
/// máscara das entradas do FROM antes da tabela virtual e separadas dela por pelo menos um LEFT ou
/// CROSS JOIN. Idem `m_unusable` para as que vêm depois.
///
/// Por exemplo, em `... FROM t1, t2 LEFT JOIN t3, t4, vt CROSS JOIN t5, t6;` `m_prereq`
/// corresponde a (t1, t2) e `m_unusable` a (t5, t6).
///
/// Todas as tabelas de `m_prereq` devem ser varridas antes da virtual: termos cujos pré-requisitos
/// são satisfeitos por `m_prereq` podem ser "usable" em toda chamada do xBestIndex. Já as de
/// `m_unusable` são varridas depois: termos cujos pré-requisitos as tocam nunca são "usable".
pub(crate) fn where_loop_add_virtual(
    db: &mut Connection,
    parse: &mut Parse,
    wi: &mut WhereInfo,
    p_tab_list: &SrcList,
    builder: &mut WhereLoopBuilder,
    m_prereq: Bitmask,
    m_unusable: Bitmask,
) -> i32 {
    let wc = builder.p_wc;
    let p_src = &p_tab_list.a[builder.p_new.i_tab as usize];
    debug_assert!((m_prereq & m_unusable) == 0);
    debug_assert!(p_src.p_tab.as_ref().map_or(false, |t| t.is_virtual()));
    let (mut p, mut hidden, m_no_omit) = allocate_index_info(wi, wc, m_unusable, p_src);
    builder.p_new.r_setup = 0;
    builder.p_new.ws_flags = WHERE_VIRTUALTABLE;
    builder.p_new.n_l_term = 0;
    builder.p_new.vtab.need_free = false;
    let n_constraint = p.a_constraint.len();
    where_loop_resize(&mut builder.p_new, n_constraint as i32);

    /* Primeiro chama o xBestIndex() com todas as restrições utilizáveis. */
    let mut b_in = false; /* Verdadeiro se o plano usa o operador IN(...) */
    let mut b_retry = false; /* Verdadeiro para tentar de novo com LIMIT/OFFSET desligados */
    let mut rc = where_loop_add_virtual_one(
        db,
        parse,
        wi,
        p_tab_list,
        builder,
        m_prereq,
        ALLBITS,
        0,
        &mut p,
        &mut hidden,
        m_no_omit,
        &mut b_in,
        Some(&mut b_retry),
    );
    if b_retry {
        debug_assert!(rc == SQLITE_OK);
        rc = where_loop_add_virtual_one(
            db,
            parse,
            wi,
            p_tab_list,
            builder,
            m_prereq,
            ALLBITS,
            0,
            &mut p,
            &mut hidden,
            m_no_omit,
            &mut b_in,
            None,
        );
    }

    /* Se a chamada com todos os termos habilitados produziu um plano que não exige nenhuma
    ** tabela fonte (mBest==0) e não usa IN(...), não adianta fazer outras chamadas ao
    ** xBestIndex(): todas devolveriam o mesmo resultado (se a implementação for sensata). */
    let mut m_best: Bitmask = 0; /* Tabelas usadas pelo melhor plano possível */
    if rc == SQLITE_OK && {
        m_best = builder.p_new.prereq & !m_prereq;
        m_best != 0 || b_in
    } {
        let mut seen_zero = false; /* Verdadeiro se viu um plano sem pré-requisitos */
        let mut seen_zero_no_in = false; /* Plano sem pré-requisitos e sem IN(...) */
        let mut m_prev: Bitmask = 0;
        let mut m_best_no_in: Bitmask = 0;

        /* Se o plano da chamada anterior usa um termo IN(...), chama o xBestIndex de novo com os
        ** termos IN(...) desligados. */
        if b_in {
            rc = where_loop_add_virtual_one(
                db,
                parse,
                wi,
                p_tab_list,
                builder,
                m_prereq,
                ALLBITS,
                WO_IN,
                &mut p,
                &mut hidden,
                m_no_omit,
                &mut b_in,
                None,
            );
            debug_assert!(!b_in);
            m_best_no_in = builder.p_new.prereq & !m_prereq;
            if m_best_no_in == 0 {
                seen_zero = true;
                seen_zero_no_in = true;
            }
        }

        /* Chama o xBestIndex uma vez para cada valor distinto de (prereqRight & ~mPrereq) no
        ** conjunto de termos que se aplicam à tabela virtual corrente. */
        while rc == SQLITE_OK {
            let mut m_next: Bitmask = ALLBITS;
            debug_assert!(m_next > 0);
            for i in 0..n_constraint {
                let t = wi.term_at(wc, p.a_constraint[i].i_term_offset as usize);
                let m_this: Bitmask = wi.term(t).prereq_right & !m_prereq;
                if m_this > m_prev && m_this < m_next {
                    m_next = m_this;
                }
            }
            m_prev = m_next;
            if m_next == ALLBITS {
                break;
            }
            if m_next == m_best || m_next == m_best_no_in {
                continue;
            }
            rc = where_loop_add_virtual_one(
                db,
                parse,
                wi,
                p_tab_list,
                builder,
                m_prereq,
                m_next | m_prereq,
                0,
                &mut p,
                &mut hidden,
                m_no_omit,
                &mut b_in,
                None,
            );
            if builder.p_new.prereq == m_prereq {
                seen_zero = true;
                if !b_in {
                    seen_zero_no_in = true;
                }
            }
        }

        /* Se as chamadas do laço acima não acharam um plano que não exija nenhuma tabela fonte (um
        ** plano garantidamente utilizável), faz-se aqui uma chamada com todas as tabelas fonte
        ** desligadas. */
        if rc == SQLITE_OK && !seen_zero {
            rc = where_loop_add_virtual_one(
                db,
                parse,
                wi,
                p_tab_list,
                builder,
                m_prereq,
                m_prereq,
                0,
                &mut p,
                &mut hidden,
                m_no_omit,
                &mut b_in,
                None,
            );
            if !b_in {
                seen_zero_no_in = true;
            }
        }

        /* Se até aqui as chamadas ao xBestIndex() não acharam um plano sem tabelas fonte e sem
        ** IN(...), faz-se uma última chamada para obtê-lo. */
        if rc == SQLITE_OK && !seen_zero_no_in {
            rc = where_loop_add_virtual_one(
                db,
                parse,
                wi,
                p_tab_list,
                builder,
                m_prereq,
                m_prereq,
                WO_IN,
                &mut p,
                &mut hidden,
                m_no_omit,
                &mut b_in,
                None,
            );
        }
    }

    rc
}

// ---------------------------------------------------------------------------------------------
// Chunk 011: OR, todas as tabelas e a ordenação de um caminho
// ---------------------------------------------------------------------------------------------

/// `whereLoopAddOr`: acrescenta entradas `WhereLoop` para tratar termos OR. Funciona tanto para
/// b-trees quanto para tabelas virtuais.
///
/// O `sSubBuild = *pBuilder` do C copia o construtor e os dois compartilham o `pNew`: aqui o
/// `p_new` vai para o construtor interno e volta ao fim do laço dos operandos do OR. O
/// `WhereClause tempWC` é uma cláusula temporária na arena (`a = [pOrTerm]`, `pOuter = pWC`),
/// retirada de novo ao fim de cada volta.
pub(crate) fn where_loop_add_or(
    db: &mut Connection,
    parse: &mut Parse,
    wi: &mut WhereInfo,
    p_tab_list: &SrcList,
    mut p_select: Option<&mut Select>,
    builder: &mut WhereLoopBuilder,
    m_prereq: Bitmask,
    m_unusable: Bitmask,
) -> i32 {
    let wc = builder.p_wc;
    let wc_end = wi.n_term(wc);
    let mut rc = SQLITE_OK;
    let mut s_sum = WhereOrSet::default();
    let p_item = &p_tab_list.a[builder.p_new.i_tab as usize];
    let i_cur = p_item.i_cursor;
    let is_virtual = p_item.p_tab.as_ref().map_or(false, |t| t.is_virtual());

    /* A otimização de OR de vários índices não funciona para RIGHT e FULL JOIN */
    if (p_item.fg.jointype & JT_RIGHT) != 0 {
        return SQLITE_OK;
    }

    for i_term in 0..wc_end {
        if rc != SQLITE_OK {
            break;
        }
        let p_term = wi.term_at(wc, i_term);
        let (or_wc, indexable) = match &wi.term(p_term).p_or_info {
            Some(info) if (wi.term(p_term).e_operator & WO_OR) != 0 => (info.wc, info.indexable),
            _ => continue,
        };
        if (indexable & builder.p_new.mask_self) == 0 {
            continue;
        }
        let n_or = wi.n_term(or_wc);
        let mut once = true;

        let mut sub = WhereLoopBuilder {
            p_wc: builder.p_wc,
            p_new: std::mem::take(&mut builder.p_new),
            p_or_set: Some(WhereOrSet::default()),
            bld_flags1: builder.bld_flags1,
            bld_flags2: builder.bld_flags2,
            i_plan_limit: builder.i_plan_limit,
        };

        for k in 0..n_or {
            let p_or_term = wi.term_at(or_wc, k);
            let (e_op, left_cursor) = {
                let t = wi.term(p_or_term);
                (t.e_operator, t.left_cursor)
            };
            let saved_clauses = wi.clauses.len();
            if (e_op & WO_AND) != 0 {
                sub.p_wc = wi.term(p_or_term).p_and_info.as_ref().expect("pOrTerm->u.pAndInfo").wc;
            } else if left_cursor == i_cur {
                wi.clauses.push(WhereClause {
                    p_outer: Some(wc),
                    op: TK_AND,
                    has_or: 0,
                    n_base: 1,
                    a: vec![p_or_term],
                });
                sub.p_wc = ClauseId((wi.clauses.len() - 1) as u32);
            } else {
                continue;
            }
            if let Some(s) = sub.p_or_set.as_mut() {
                s.n = 0;
            }
            if is_virtual {
                rc = where_loop_add_virtual(db, parse, wi, p_tab_list, &mut sub, m_prereq, m_unusable);
            } else {
                rc = where_loop_add_btree(
                    db,
                    parse,
                    wi,
                    p_tab_list,
                    p_select.as_deref_mut(),
                    &mut sub,
                    m_prereq,
                );
            }
            if rc == SQLITE_OK {
                rc = where_loop_add_or(
                    db,
                    parse,
                    wi,
                    p_tab_list,
                    p_select.as_deref_mut(),
                    &mut sub,
                    m_prereq,
                    m_unusable,
                );
            }
            wi.clauses.truncate(saved_clauses);
            let s_cur: WhereOrSet = sub.p_or_set.unwrap_or_default();
            if s_cur.n == 0 {
                s_sum.n = 0;
                break;
            } else if once {
                where_or_move(&mut s_sum, &s_cur);
                once = false;
            } else {
                let mut s_prev = WhereOrSet::default();
                where_or_move(&mut s_prev, &s_sum);
                s_sum.n = 0;
                for i in 0..s_prev.n as usize {
                    for j in 0..s_cur.n as usize {
                        where_or_insert(
                            &mut s_sum,
                            s_prev.a[i].prereq | s_cur.a[j].prereq,
                            log_est_add(s_prev.a[i].r_run, s_cur.a[j].r_run),
                            log_est_add(s_prev.a[i].n_out, s_cur.a[j].n_out),
                        );
                    }
                }
            }
        }
        builder.p_new = sub.p_new;
        where_loop_resize(&mut builder.p_new, 1);
        builder.p_new.n_l_term = 1;
        builder.p_new.a_l_term[0] = Some(p_term);
        builder.p_new.ws_flags = WHERE_MULTI_OR;
        builder.p_new.r_setup = 0;
        builder.p_new.i_sort_idx = 0;
        builder.p_new.btree = Default::default();
        builder.p_new.vtab = Default::default();
        for i in 0..s_sum.n as usize {
            if rc != SQLITE_OK {
                break;
            }
            /* TUNING: o `s_sum.a[i].r_run` atual é a soma dos custos de todas as sub-varreduras
            ** exigidas pela varredura do OR. Mas, por erros de arredondamento, o custo da
            ** varredura do OR pode ficar igual ao da sub-varredura mais cara. Soma-se a menor
            ** penalidade possível (equivalente a multiplicar o custo por 1.07) para garantir que
            ** isso não aconteça. Senão, em WHEREs como o seguinte, com um índice em "y":
            **
            **     WHERE likelihood(x=?, 0.99) OR y=?
            **
            ** o planejador poderia optar por fazer "OR" de uma varredura completa com uma busca
            ** no índice. E outros resultados igualmente estranhos. */
            builder.p_new.r_run = (s_sum.a[i].r_run as i32 + 1) as LogEst;
            builder.p_new.n_out = s_sum.a[i].n_out;
            builder.p_new.prereq = s_sum.a[i].prereq;
            rc = where_loop_insert(wi, builder);
        }
    }
    rc
}

/// `whereLoopAddAll`: acrescenta todos os `WhereLoop` de todas as tabelas.
pub(crate) fn where_loop_add_all(
    db: &mut Connection,
    parse: &mut Parse,
    wi: &mut WhereInfo,
    p_tab_list: &SrcList,
    mut p_select: Option<&mut Select>,
    builder: &mut WhereLoopBuilder,
) -> i32 {
    let mut m_prereq: Bitmask = 0;
    let mut m_prior: Bitmask = 0;
    let p_end = wi.n_level as usize;
    let mut rc = SQLITE_OK;
    let mut b_first_past_rj = false;
    let mut has_right_join = false;

    /* Percorre as tabelas da junção, da esquerda para a direita. Confere que `p_new` já foi
    ** iniciado. */
    debug_assert!(builder.p_new.n_l_term == 0);
    debug_assert!(builder.p_new.ws_flags == 0);
    debug_assert!(builder.p_new.a_l_term.len() >= WHERE_LOOP_SPACE);

    builder.i_plan_limit = SQLITE_QUERY_PLANNER_LIMIT;
    for i_tab in 0..p_end {
        let p_item = &p_tab_list.a[i_tab];
        let jointype = p_item.fg.jointype;
        let mut m_unusable: Bitmask = 0;
        builder.p_new.i_tab = i_tab as u8;
        builder.i_plan_limit += SQLITE_QUERY_PLANNER_LIMIT_INCR;
        builder.p_new.mask_self = wi.s_mask_set.get_mask(p_item.i_cursor);
        if b_first_past_rj || (jointype & (JT_OUTER | JT_CROSS | JT_LTORJ)) != 0 {
            /* Acrescenta pré-requisitos para impedir a reordenação de termos do FROM através de
            ** CROSS joins e outer joins. O booleano b_first_past_rj impede que o operando direito
            ** de um RIGHT JOIN seja trocado com outros elementos ainda mais à direita.
            **
            ** O caso JT_LTORJ e o flag has_right_join funcionam juntos para impedir que termos do
            ** FROM passem do lado direito de um LEFT JOIN para o esquerdo se esse LEFT JOIN
            ** está no lado esquerdo de um RIGHT JOIN. */
            if (jointype & JT_LTORJ) != 0 {
                has_right_join = true;
            }
            m_prereq |= m_prior;
            b_first_past_rj = (jointype & JT_RIGHT) != 0;
        } else if !has_right_join {
            m_prereq = 0;
        }
        if p_item.p_tab.as_ref().map_or(false, |t| t.is_virtual()) {
            for p in p_tab_list.a[i_tab + 1..p_end].iter() {
                if m_unusable != 0 || (p.fg.jointype & (JT_OUTER | JT_CROSS)) != 0 {
                    m_unusable |= wi.s_mask_set.get_mask(p.i_cursor);
                }
            }
            rc = where_loop_add_virtual(db, parse, wi, p_tab_list, builder, m_prereq, m_unusable);
        } else {
            rc = where_loop_add_btree(
                db,
                parse,
                wi,
                p_tab_list,
                p_select.as_deref_mut(),
                builder,
                m_prereq,
            );
        }
        if rc == SQLITE_OK && wi.clause(builder.p_wc).has_or != 0 {
            rc = where_loop_add_or(
                db,
                parse,
                wi,
                p_tab_list,
                p_select.as_deref_mut(),
                builder,
                m_prereq,
                m_unusable,
            );
        }
        m_prior |= builder.p_new.mask_self;
        if rc != SQLITE_OK {
            if rc == SQLITE_DONE {
                /* Atingiu o limite de busca do planejador definido por iPlanLimit */
                crate::global::log(SQLITE_WARNING, b"abbreviated query algorithm search", &[]);
                rc = SQLITE_OK;
            } else {
                break;
            }
        }
    }

    where_loop_clear(&mut builder.p_new);
    rc
}

/// `wherePathSatisfiesOrderBy`: examina um `WherePath` (com o `WhereLoop` extra `p_last`, o sexto
/// parâmetro) para ver se ele produz linhas no ORDER BY (ou GROUP BY) pedido sem exigir uma
/// ordenação separada. Devolve N: N>0, N termos do ORDER BY são satisfeitos; N==0, nenhum; N<0,
/// ainda não se sabe quantos.
///
/// O processamento de WHERE_GROUPBY e WHERE_DISTINCTBY é menos estrito: basta que linhas
/// equivalentes fiquem adjacentes, então os termos de `p_order_by` podem casar em qualquer ordem.
/// Com ORDER BY, devem casar em ordem estrita da esquerda para a direita.
///
/// `p_order_by` NÃO pode ser um empréstimo de `wi.p_order_by` nem de `wi.p_result_set`: o chamador
/// tira a lista do `wi` (`take`) durante a chamada e a devolve depois. `p_last` é o id de um loop
/// da arena; `p_path` fica fora do `wi` (os caminhos do `wherePathSolver` são locais dele).
pub(crate) fn where_path_satisfies_order_by(
    db: &mut Connection,
    parse: &mut Parse,
    wi: &mut WhereInfo,
    p_tab_list: &SrcList,
    p_order_by: &ExprList,
    p_path: &WherePath,
    wctrl_flags: u16,
    n_loop: u16,
    p_last: LoopId,
    p_rev_mask: &mut Bitmask,
) -> i8 {
    let wctrl = wctrl_flags as u32;
    let mut ob_sat: Bitmask = 0; /* Máscara dos termos do ORDER BY satisfeitos até aqui */

    /* Um WhereLoop é "de uma linha" se gera no máximo uma linha de saída: (a) todas as colunas do
    ** índice casam com WHERE_COLUMN_EQ e (b) o índice é único. Todo WhereLoop com WHERE_COLUMN_EQ
    ** no rowid é de uma linha. Todo loop de uma linha tem WHERE_ONEROW em wsFlags.
    **
    ** Um WhereLoop é "order-distinct" se o conjunto das colunas dele que estão no ORDER BY é
    ** diferente para toda linha do loop. Todo loop de uma linha é automaticamente order-distinct.
    ** Um loop sem colunas no ORDER BY não é order-distinct. Ser order-distinct não é bem o mesmo
    ** que ser UNIQUE, porque uma coluna ou índice UNIQUE pode ter várias linhas NULL e NULLs são
    ** equivalentes para o order-distinct. Para ser order-distinct, as colunas devem ser UNIQUE e
    ** NOT NULL.
    **
    ** O rowid de uma tabela é sempre UNIQUE e NOT NULL, então sempre que o rowid aparece no ORDER
    ** BY o WhereLoop correspondente é automaticamente order-distinct. */
    if n_loop != 0 && db.optimization_disabled(SQLITE_ORDER_BY_IDX_JOIN) {
        return 0;
    }

    let n_order_by = p_order_by.a.len();
    if n_order_by > (crate::consts::BMS - 1) as usize {
        return 0; /* Não otimiza ORDER BYs grandes demais */
    }
    let mut is_order_distinct = true; /* Todos os WhereLoops anteriores são order-distinct */
    let ob_done: Bitmask = (1u64 << n_order_by) - 1; /* Máscara de todos os termos do ORDER BY */
    let mut order_distinct_mask: Bitmask = 0; /* Máscara de todos os loops bem ordenados */
    let mut ready: Bitmask = 0; /* Máscara dos loops internos */
    let mut eq_op_mask: u16 = WO_EQ | WO_IS | WO_ISNULL; /* Operadores de igualdade permitidos */
    if (wctrl & (WHERE_ORDERBY_LIMIT | WHERE_ORDERBY_MAX | WHERE_ORDERBY_MIN)) != 0 {
        eq_op_mask |= WO_IN;
    }
    let mut p_loop_id: Option<LoopId> = None; /* O WhereLoop em processamento */
    let mut i_loop: i32 = 0;
    'loops: while is_order_distinct && ob_sat < ob_done && i_loop <= n_loop as i32 {
        'iter: {
            if i_loop > 0 {
                ready |= wi.w_loop(p_loop_id.expect("pLoop")).mask_self;
            }
            let lid: LoopId;
            if i_loop < n_loop as i32 {
                lid = p_path.a_loop[i_loop as usize];
                p_loop_id = Some(lid);
                if (wctrl & WHERE_ORDERBY_LIMIT) != 0 {
                    break 'iter;
                }
            } else {
                lid = p_last;
                p_loop_id = Some(lid);
            }
            if (wi.w_loop(lid).ws_flags & WHERE_VIRTUALTABLE) != 0 {
                if wi.w_loop(lid).vtab.is_ordered != 0
                    && (wctrl & (WHERE_DISTINCTBY | WHERE_SORTBYGROUP)) != WHERE_DISTINCTBY
                {
                    ob_sat = ob_done;
                }
                break 'loops;
            } else if (wctrl & WHERE_DISTINCTBY) != 0 {
                wi.w_loop_mut(lid).btree.n_distinct_col = 0;
            }
            let p_tab: Option<Rc<Table>> = p_tab_list.a[wi.w_loop(lid).i_tab as usize].p_tab.clone();
            let i_cur: i32 = p_tab_list.a[wi.w_loop(lid).i_tab as usize].i_cursor; /* Cursor do loop corrente */

            /* Marca todo termo X do ORDER BY que é coluna da tabela do loop corrente e para o qual
            ** há no WHERE um termo da forma X IS NULL ou X=? que referencia só loops externos. */
            for i in 0..n_order_by {
                if ((1u64 << i) & ob_sat) != 0 {
                    continue;
                }
                let Some(p_ob_expr) =
                    expr_skip_collate_and_likely(p_order_by.a[i].p_expr.as_deref())
                else {
                    continue;
                };
                if p_ob_expr.op != TK_COLUMN && p_ob_expr.op != TK_AGG_COLUMN {
                    continue;
                }
                if p_ob_expr.i_table != i_cur {
                    continue;
                }
                let Some(p_term) = where_find_term(
                    db,
                    parse,
                    wi,
                    wi.s_wc,
                    i_cur,
                    p_ob_expr.i_column,
                    !ready,
                    eq_op_mask as u32,
                    None,
                ) else {
                    continue;
                };
                let term_op = wi.term(p_term).e_operator;
                if term_op == WO_IN {
                    /* Termos IN só valem para ordenar na otimização ORDER BY LIMIT, e só se são
                    ** realmente usados pelo plano da consulta */
                    debug_assert!(
                        (wctrl & (WHERE_ORDERBY_LIMIT | WHERE_ORDERBY_MIN | WHERE_ORDERBY_MAX)) != 0
                    );
                    let l = wi.w_loop(lid);
                    let mut j = 0usize;
                    while j < l.n_l_term as usize && Some(p_term) != l.a_l_term[j] {
                        j += 1;
                    }
                    if j >= l.n_l_term as usize {
                        continue;
                    }
                }
                if (term_op & (WO_EQ | WO_IS)) != 0 && p_ob_expr.i_column >= 0 {
                    let p_coll1 = expr_nn_coll_seq(db, parse, p_order_by.a[i].p_expr.as_deref(), None);
                    let p_coll2 = expr_compare_coll_seq(db, parse, wi.expr(p_term), None);
                    match p_coll2 {
                        None => continue,
                        Some(c2) => {
                            if str_icmp(&p_coll1.name, &c2.name) != 0 {
                                continue;
                            }
                        }
                    }
                }
                ob_sat |= 1u64 << i;
            }

            if (wi.w_loop(lid).ws_flags & WHERE_ONEROW) == 0 {
                let p_index: Option<Rc<Index>>;
                let n_key_col: u16;
                let n_column: u16;
                if (wi.w_loop(lid).ws_flags & WHERE_IPK) != 0 {
                    p_index = None;
                    n_key_col = 0;
                    n_column = 1;
                } else {
                    match wi.w_loop(lid).btree.p_index.clone() {
                        None => return 0,
                        Some(ix) if ix.b_unordered => return 0,
                        Some(ix) => {
                            n_key_col = ix.n_key_col;
                            n_column = ix.n_column;
                            /* Todos os termos relevantes do índice também devem ser não NULL para
                            ** isOrderDistinct ser verdadeiro. Então o valor calculado aqui pode
                            ** ser um falso positivo. Há correções em tag-20210426-1 abaixo. */
                            is_order_distinct = ix.is_unique_index()
                                && (wi.w_loop(lid).ws_flags & WHERE_SKIPSCAN) == 0;
                            p_index = Some(ix);
                        }
                    }
                }

                /* Percorre todas as colunas do índice e trata as que não são restringidas por ==
                ** ou IN. */
                let mut rev: u8 = 0; /* Ordem de classificação composta */
                let mut rev_set = false; /* Verdadeiro se rev é conhecido */
                let mut distinct_columns = false; /* O loop tem colunas UNIQUE NOT NULL */
                for j in 0..n_column as i32 {
                    let mut b_once = true; /* Verdadeiro para rodar o laço de busca no ORDER BY */
                    let n_eq = wi.w_loop(lid).btree.n_eq as i32;
                    let n_skip = wi.w_loop(lid).n_skip as i32;

                    debug_assert!(
                        j >= n_eq
                            || (wi.w_loop(lid).a_l_term[j as usize].is_none()) == (j < n_skip)
                    );
                    if j < n_eq && j >= n_skip {
                        let a_j = wi.w_loop(lid).a_l_term[j as usize].expect("aLTerm[j]");
                        let e_op = wi.term(a_j).e_operator;

                        /* Pula termos ==, IS e ISNULL. (Pula também termos IN no processamento
                        ** de WHERE_ORDERBY_LIMIT.) Mas IS e ISNULL implicam que o índice não é
                        ** UNIQUE NOT NULL, então o loop deve ser marcado como não
                        ** order-distinct, porque pode ter linhas NULL repetidas.
                        **
                        ** Se o termo atual é uma coluna de uma expressão ((?,?) IN (SELECT...))
                        ** cujo SELECT devolve mais de uma coluna, confere-se que ela é a única
                        ** usada por este loop. Senão, se é uma de duas ou mais, nenhuma das
                        ** colunas pode ser considerada casada com um termo do ORDER BY. */
                        if (e_op & eq_op_mask) != 0 {
                            if (e_op & (WO_ISNULL | WO_IS)) != 0 {
                                is_order_distinct = false;
                            }
                            continue;
                        } else if (e_op & WO_IN) != 0 {
                            /* ALWAYS(): e_op é um operador de igualdade pela condição
                            ** j<nEq acima. Toda igualdade diferente de WO_IN é capturada pelo
                            ** "if" anterior. Então este caso é sempre WO_IN. */
                            for i in (j + 1)..n_eq {
                                let a_i = wi.w_loop(lid).a_l_term[i as usize].expect("aLTerm[i]");
                                if wi.term(a_i).p_expr == wi.term(a_j).p_expr {
                                    debug_assert!((wi.term(a_i).e_operator & WO_IN) != 0);
                                    b_once = false;
                                    break;
                                }
                            }
                        }
                    }

                    /* Pega o número da coluna na tabela (i_column) e a ordem de classificação
                    ** (rev_idx) da j-ésima coluna do índice. */
                    let i_column: i32;
                    let rev_idx: u8;
                    if let Some(ix) = &p_index {
                        let mut c = ix.ai_column[j as usize] as i32;
                        rev_idx =
                            ix.a_sort_order.get(j as usize).copied().unwrap_or(0) & KEYINFO_ORDER_DESC;
                        if c == p_tab.as_ref().expect("pIndex->pTable").i_p_key as i32 {
                            c = XN_ROWID as i32;
                        }
                        i_column = c;
                    } else {
                        i_column = XN_ROWID as i32;
                        rev_idx = 0;
                    }

                    /* Uma coluna sem restrição que pode ser NULL significa que este WhereLoop não
                    ** é bem ordenado. tag-20210426-1 */
                    if is_order_distinct {
                        if i_column >= 0
                            && j >= n_eq
                            && p_tab.as_ref().expect("pIndex->pTable").a_col[i_column as usize].not_null
                                == 0
                        {
                            is_order_distinct = false;
                        }
                        if i_column == XN_EXPR as i32 {
                            is_order_distinct = false;
                        }
                    }

                    /* Acha o termo do ORDER BY que corresponde à j-ésima coluna do índice e o
                    ** marca como satisfeito */
                    let mut is_match = false; /* i_column casa com um termo do ORDER BY */
                    let mut i: usize = 0;
                    while b_once && i < n_order_by {
                        if ((1u64 << i) & ob_sat) != 0 {
                            i += 1;
                            continue;
                        }
                        let Some(p_ob_expr) =
                            expr_skip_collate_and_likely(p_order_by.a[i].p_expr.as_deref())
                        else {
                            i += 1;
                            continue;
                        };
                        if (wctrl & (WHERE_GROUPBY | WHERE_DISTINCTBY)) == 0 {
                            b_once = false;
                        }
                        if i_column >= XN_ROWID as i32 {
                            if p_ob_expr.op != TK_COLUMN && p_ob_expr.op != TK_AGG_COLUMN {
                                i += 1;
                                continue;
                            }
                            if p_ob_expr.i_table != i_cur {
                                i += 1;
                                continue;
                            }
                            if p_ob_expr.i_column != i_column {
                                i += 1;
                                continue;
                            }
                        } else {
                            let p_ix_expr = p_index
                                .as_ref()
                                .and_then(|ix| ix.a_col_expr.as_deref())
                                .and_then(|l| l.a.get(j as usize))
                                .and_then(|it| it.p_expr.as_deref());
                            if expr_compare_skip(Some(p_ob_expr), p_ix_expr, i_cur) != 0 {
                                i += 1;
                                continue;
                            }
                        }
                        if i_column != XN_ROWID as i32 {
                            let p_coll = expr_nn_coll_seq(
                                db,
                                parse,
                                p_order_by.a[i].p_expr.as_deref(),
                                None,
                            );
                            let ix = p_index.as_ref().expect("pIndex");
                            if str_icmp(&p_coll.name, &ix.az_coll[j as usize]) != 0 {
                                i += 1;
                                continue;
                            }
                        }
                        if (wctrl & WHERE_DISTINCTBY) != 0 {
                            wi.w_loop_mut(lid).btree.n_distinct_col = (j + 1) as u16;
                        }
                        is_match = true;
                        break;
                    }
                    if is_match && (wctrl & WHERE_GROUPBY) == 0 {
                        /* Garante que a ordem de classificação é compatível num ORDER BY. A ordem
                        ** é irrelevante num GROUP BY. */
                        let desc = p_order_by.a[i].fg.sort_flags & KEYINFO_ORDER_DESC;
                        if rev_set {
                            if (rev ^ rev_idx) != desc {
                                is_match = false;
                            }
                        } else {
                            rev = rev_idx ^ desc;
                            if rev != 0 {
                                *p_rev_mask |= 1u64 << i_loop;
                            }
                            rev_set = true;
                        }
                    }
                    if is_match && (p_order_by.a[i].fg.sort_flags & KEYINFO_ORDER_BIGNULL) != 0 {
                        if j == n_eq {
                            wi.w_loop_mut(lid).ws_flags |= WHERE_BIGNULL_SORT;
                        } else {
                            is_match = false;
                        }
                    }
                    if is_match {
                        if i_column == XN_ROWID as i32 {
                            distinct_columns = true;
                        }
                        ob_sat |= 1u64 << i;
                    } else {
                        /* Nenhum casamento encontrado */
                        if j == 0 || j < n_key_col as i32 {
                            is_order_distinct = false;
                        }
                        break;
                    }
                } /* fim do laço sobre todas as colunas do índice */
                if distinct_columns {
                    is_order_distinct = true;
                }
            } /* fim do if "não é de uma linha" */

            /* Marca os outros termos do ORDER BY que referenciam o loop */
            if is_order_distinct {
                order_distinct_mask |= wi.w_loop(lid).mask_self;
                for i in 0..n_order_by {
                    if ((1u64 << i) & ob_sat) != 0 {
                        continue;
                    }
                    let p = p_order_by.a[i].p_expr.as_deref();
                    let m_term = where_expr_usage(&mut wi.s_mask_set, p);
                    if m_term == 0 {
                        let mut copy = p.cloned();
                        if expr_is_constant(None, copy.as_mut()) == 0 {
                            continue;
                        }
                    }
                    if (m_term & !order_distinct_mask) == 0 {
                        ob_sat |= 1u64 << i;
                    }
                }
            }
        } /* 'iter */
        i_loop += 1;
    } /* Fim do laço sobre todos os WhereLoops do mais externo ao mais interno */
    if ob_sat == ob_done {
        return n_order_by as i8;
    }
    if !is_order_distinct {
        let mut i = n_order_by as i32 - 1;
        while i > 0 {
            let m: Bitmask = if i < crate::consts::BMS { (1u64 << i) - 1 } else { 0 };
            if (ob_sat & m) == m {
                return i as i8;
            }
            i -= 1;
        }
        return 0;
    }
    -1
}
