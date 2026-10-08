//! Tradução de `where.c`, parte 3 (chunks `where_c.012` a `where_c.017`): `sqlite3WhereIsSorted`,
//! o custo de ordenação, o resolvedor de caminhos (`wherePathSolver`), a heurística entre os dois
//! passos do resolvedor, o atalho dos casos simples (`whereShortCut`), a omissão de junções sem
//! efeito (`whereOmitNoopJoin`), o filtro de Bloom útil, os helpers de `sqlite3WhereBegin`
//! (`whereAddIndexedExpr`, `whereReverseScanOrder`), `sqlite3WhereBegin` e `sqlite3WhereEnd`.
//!
//! O modelo de dados é o de `where_int.rs` (arenas por `WhereInfo`, `ExprRef`, handles). Decisões
//! desta fatia, além das de `where_.rs` e `where2.rs`:
//!
//! - `WhereInfo.pParse`, `pTabList` e `pSelect` não existem no `WhereInfo`: `db`, `parse`,
//!   `p_tab_list` e `p_select` são parâmetros. Por isso `where_end` recebe o `p_tab_list` (o
//!   `pWInfo->pTabList` do C): `where_end(db, parse, p_tab_list: &SrcList, Box<WhereInfo>)`. Sem a
//!   lista de origem não há como saber a tabela, o tipo de junção e o registrador de co-rotina de
//!   cada nível. Os chamadores já têm a lista, pois a passaram a `where_begin`.
//! - `sqlite3WhereBegin` recebe `pWhere` por empréstimo e o DUPLICA (`expr_dup`) para dentro da
//!   arena do `WhereInfo` (`WhereExprs::add_root`). `pOrderBy` e `pResultSet` ficam como cópias
//!   possuídas (`expr_list_dup`), como o contrato de `where_int.rs` pede. O caso `WHERE_DISTINCTBY`
//!   (`pWInfo->pOrderBy = pResultSet`) é outra cópia do conjunto de resultados.
//! - O `WhereLoop` `pNew` do construtor vive no próprio `WhereLoopBuilder` (`builder.p_new`) e NÃO
//!   está na arena de loops. O atalho `whereShortCut`, que faz `pWInfo->a[0].pWLoop = pLoop`,
//!   acrescenta uma cópia desse loop à arena (`wi.loops`) sem pô-la em `p_loops` (a lista do C
//!   também não a contém) e guarda o id em `wi.a[0].p_w_loop`.
//! - O teste `p==0` de `whereInterstageHeuristic` (nível sem `WhereLoop` porque o primeiro
//!   resolvedor falhou) não existe: `LoopId` não tem valor nulo. Como o resolvedor só deixa níveis
//!   sem loop quando devolve erro, a heurística só roda quando o primeiro resolvedor devolveu
//!   `SQLITE_OK`, o que dá o mesmo resultado que o `break` do C.
//! - `wherePathSolver` recebe o `Select` (só para o `pEList->nExpr` do custo de ordenação) e
//!   `where_path_satisfies_order_by` recebe a lista TIRADA do `wi` com `take()` e devolvida em
//!   seguida (ver `where2.rs`). Os caminhos (`WherePath`) são locais do resolvedor.
//! - `whereOmitNoopJoin` "move" os níveis seguintes com `rotate_left(1)` (o `memmove` do C): a
//!   cauda de `wi.a` fica com a cópia obsoleta, como lá; `n_level` é quem manda.
//! - Fora do que o Debian compila: `WHERETRACE_ENABLED`, `SQLITE_DEBUG` (`cId`, rastreio dos
//!   opcodes reescritos, `sqlite3WhereOpcodeRewriteTrace`), `SQLITE_ENABLE_STAT4` (segunda passada
//!   por `SQLITE_BLDF2_2NDPASS`), `SQLITE_ENABLE_CURSOR_HINTS`, `SQLITE_ENABLE_COLUMN_USED_MASK`,
//!   `SQLITE_ENABLE_OFFSET_SQL_FUNC`, `SQLITE_ENABLE_STMT_SCANSTATUS` (`sqlite3WhereAddScanStatus`),
//!   `VdbeCoverage*`, `testcase` e `VdbeModuleComment` (só com `SQLITE_ENABLE_MODULE_COMMENTS`).
//!   As falhas de alocação (`mallocFailed`) não existem em Rust. A limpeza registrada por
//!   `sqlite3ParserAddCleanup(whereIndexedExprCleanup)` não existe: o `Parse` é dono dos `Vec`.
//!
//! Dependências ainda sem tradução, com as assinaturas assumidas:
//!
//! - `wherecode::where_code_one_loop_start(db: &mut Connection, parse: &mut Parse, p_tab_list:
//!   &SrcList, wi: &mut WhereInfo, i_level: usize, not_ready: Bitmask) -> Bitmask`
//!   (`sqlite3WhereCodeOneLoopStart`) e `wherecode::where_right_join_loop(db: &mut Connection,
//!   parse: &mut Parse, p_tab_list: &SrcList, wi: &mut WhereInfo, i_level: usize)`
//!   (`sqlite3WhereRightJoinLoop`).
//! - `where_::translate_column_to_copy` precisa ser `pub(crate)` (hoje é privada).
//! - `build::table_flags_or(db: &mut Connection, &Rc<Table>, u32)` (o `pTab->tabFlags |= ...`),
//!   a mesma de `where2.rs`.
//! - `vtab::get_vtable(db: &Connection, tab: &Rc<Table>) -> Option<VTableId>`
//!   (`sqlite3GetVTable`), a mesma de `where_.rs`.
//! - `util::error_msg(db, parse, fmt, args)` (`sqlite3ErrorMsg`).

use std::rc::Rc;

pub(crate) use crate::vdbeaux::{vdbe_of_parse, current_addr};
use crate::build::{
    column_expr, primary_key_index, storage_column_to_table, table_column_to_index,
    table_flags_or, table_lock, text_arg,
};
use crate::build3::code_verify_schema;
use crate::callback::find_function;
use crate::connection::{Connection, IndexedExpr, Parse};
use crate::consts::{OP_GOTO, 
    Bitmask, ALLBITS, BMS, COLFLAG_VIRTUAL, EP_CONST_FUNC, EP_INNER_ON, EP_OUTER_ON, JT_LEFT,
    JT_LTORJ, JT_RIGHT, M10D_YES, ONEPASS_MULTI, ONEPASS_OFF, ONEPASS_SINGLE, OPFLAG_FORDELETE,
    OPFLAG_SEEKEQ, OP_BLOB, OP_COLUMN, OP_DECRJUMPZERO, OP_GOSUB, OP_IDXROWID, OP_IFNOHOPE,
    OP_IFNOTOPEN, OP_IFNULLROW, OP_IFPOS, OP_NOOP, OP_NULL, OP_NULLROW, OP_ONCE,
    OP_OPENEPHEMERAL, OP_OPENREAD, OP_OPENWRITE, OP_PREV, OP_REOPENIDX, OP_RETURN, OP_ROWID,
    OP_SEEKGT, OP_SEEKLT, OP_VOPEN, SF_MULTIVALUE, SQLITE_BLOOM_FILTER, SQLITE_DISTINCT_OPT,
    SQLITE_ERROR, SQLITE_INDEXED_EXPR, SQLITE_INTERNAL, SQLITE_JUMPIFNULL, SQLITE_OK,
    SQLITE_OMIT_NOOP_JOIN, SQLITE_ONE_PASS, SQLITE_RESULT_SUBTYPE, SQLITE_REVERSE_ORDER,
    TERM_CODED, TERM_VIRTUAL, TF_EPHEMERAL, TF_HAS_GENERATED, TF_HAS_STAT1, TF_MAYBE_REANALYZE,
    TF_WITHOUT_ROWID, TK_AND, TK_FUNCTION, WHERE_AGG_DISTINCT, WHERE_AUTO_INDEX,
    WHERE_BIGNULL_SORT, WHERE_BLOOMFILTER, WHERE_COLUMN_EQ, WHERE_COLUMN_IN, WHERE_COLUMN_NULL,
    WHERE_COLUMN_RANGE, WHERE_CONSTRAINT, WHERE_DISTINCTBY, WHERE_DISTINCT_NOOP,
    WHERE_DISTINCT_ORDERED, WHERE_DISTINCT_UNIQUE, WHERE_DISTINCT_UNORDERED, WHERE_DUPLICATES_OK,
    WHERE_IDX_ONLY, WHERE_INDEXED, WHERE_IN_ABLE, WHERE_IN_EARLYOUT, WHERE_IN_SEEKSCAN, WHERE_IPK,
    WHERE_KEEP_ALL_JOINS, WHERE_MULTI_OR, WHERE_ONEPASS_DESIRED, WHERE_ONEPASS_MULTIROW,
    WHERE_ONEROW, WHERE_ORDERBY_LIMIT, WHERE_ORDERBY_MAX, WHERE_ORDERBY_MIN, WHERE_OR_SUBCLAUSE,
    WHERE_SELFCULL, WHERE_SKIPSCAN, WHERE_SORTBYGROUP, WHERE_TRANSCONS, WHERE_USE_LIMIT,
    WHERE_VIRTUALTABLE, WHERE_WANT_DISTINCT, WO_EQ, WO_IS, WRC_ABORT, WRC_CONTINUE, WRC_PRUNE,
    XN_EXPR,
};
use crate::expr::{expr_dup, expr_list_dup};
use crate::expr_code::{expr_is_constant, select_walk_fail};
use crate::expr_code2::expr_if_false;
use crate::insert::{index_affinity_str, open_table};
use crate::mem::KeyInfo;
use crate::prepare::schema_to_index;
use crate::printf::PrintfArg;
use crate::sqlite_int::{Expr, ExprList, Index, LogEst, Select, SrcItem, SrcList, Table, Walker};
use crate::sqlite_int::SrcU2;
use crate::util::{error_msg, log_est, log_est_add};
use crate::vdbe_types::{Vdbe, P4};
use crate::vdbeaux::{
    add_op0, add_op1, add_op2, add_op3, add_op4_int, append_p4, change_p4, change_p4_vtab,
    change_p5, explain, jump_here, make_label, resolve_label, set_p4_key_info, vdbe_comment,
    vdbe_goto,
};
use crate::walker::walk_expr;
use crate::select::key_info_alloc;
use crate::vtab::get_vtable;
use crate::where2::{
    where_info_free, where_loop_add_all, where_loop_init, where_part_idx_expr,
    where_path_satisfies_order_by,
};
use crate::where_::{
    construct_automatic_index, construct_bloom_filter, create_mask, est_log,
    is_distinct_redundant, translate_column_to_copy, where_scan_init, where_scan_next,
};
use crate::where_int::{
    ExprRef, LoopId, WhereInfo, WhereLevel, WhereLoopBuilder, WherePath, WhereRightJoin,
    WhereScan,
};
use crate::wherecode::{
    where_code_one_loop_start, where_explain_one_scan, where_right_join_loop,
};
use crate::whereexpr::{
    where_add_limit, where_clause_init, where_expr_analyze, where_expr_list_usage, where_split,
    where_tab_func_args,
};

// ---------------------------------------------------------------------------------------------
// Auxiliares privados (não existem no C)
// ---------------------------------------------------------------------------------------------



/// Verdadeiro se algum bit de `mask` (constante `WHERE_*` de 32 bits) está em `flags` (o
/// `wctrlFlags` de 16 bits do C).
fn wf(flags: u16, mask: u32) -> bool {
    (flags as u32 & mask) != 0
}

/// Qual das duas listas possuídas do `WhereInfo` o `wherePathSatisfiesOrderBy` examina.
#[derive(Clone, Copy)]
enum ObList {
    /// `pWInfo->pOrderBy`.
    OrderBy,
    /// `pWInfo->pResultSet`.
    ResultSet,
}

/// Chama `where_path_satisfies_order_by` com a lista pedida: ela é TIRADA do `wi` durante a
/// chamada (o `wi` é emprestado por inteiro à função) e devolvida em seguida. Lista ausente dá 0
/// (no C seria uma desreferência nula, que o chamador nunca permite).
fn path_satisfies(
    db: &mut Connection,
    parse: &mut Parse,
    wi: &mut WhereInfo,
    p_tab_list: &SrcList,
    which: ObList,
    p_path: &WherePath,
    wctrl_flags: u16,
    n_loop: u16,
    p_last: LoopId,
    p_rev_mask: &mut Bitmask,
) -> i8 {
    let list = match which {
        ObList::OrderBy => wi.p_order_by.take(),
        ObList::ResultSet => wi.p_result_set.take(),
    };
    let rc = match list.as_deref() {
        Some(l) => where_path_satisfies_order_by(
            db,
            parse,
            wi,
            p_tab_list,
            l,
            p_path,
            wctrl_flags,
            n_loop,
            p_last,
            p_rev_mask,
        ),
        None => 0,
    };
    match which {
        ObList::OrderBy => wi.p_order_by = list,
        ObList::ResultSet => wi.p_result_set = list,
    }
    rc
}

// ---------------------------------------------------------------------------------------------
// Chunk 012: sqlite3WhereIsSorted, custo de ordenação e wherePathSolver
// ---------------------------------------------------------------------------------------------

/// `sqlite3WhereIsSorted`: se `WHERE_GROUPBY` está ligado nas flags passadas a
/// `sqlite3WhereBegin()`, o planejador supõe que o `pOrderBy` é na verdade um GROUP BY, e qualquer
/// ordem que agrupe as linhas como pedido serve. Normalmente quem chama não sabe se as linhas
/// saem mesmo ordenadas ou só agrupadas. Mas se `WHERE_SORTBYGROUP` também foi passado, esta
/// função devolve verdadeiro se as linhas realmente sairão ordenadas como pedido.
///
/// Por exemplo, com `CREATE INDEX i1 ON t1(x, Y)`:
///
/// ```text
///   SELECT * FROM t1 GROUP BY x,y ORDER BY x,y;   -- IsSorted()==1
///   SELECT * FROM t1 GROUP BY y,x ORDER BY y,x;   -- IsSorted()==0
/// ```
pub fn where_is_sorted(wi: &WhereInfo) -> bool {
    debug_assert!(wf(wi.wctrl_flags, crate::consts::WHERE_GROUPBY | WHERE_DISTINCTBY));
    debug_assert!(wf(wi.wctrl_flags, WHERE_SORTBYGROUP));
    wi.sorted
}

/// `whereSortingCost`: o custo de ordenar `n_row` linhas, supondo chaves de `n_order_by` colunas
/// das quais as `n_sorted` primeiras já estão em ordem. `n_select_expr` é o `pSelect->pEList->nExpr`.
fn where_sorting_cost(
    wi: &WhereInfo,
    n_select_expr: i32,
    n_row: LogEst,
    n_order_by: i32,
    n_sorted: i32,
) -> LogEst {
    // O custo estimado de uma ordenação externa completa de N linhas é
    //
    //   custo = (K * N * log(N)).
    //
    // Se o ORDER BY tem X termos mas só os últimos Y estão fora de ordem, a ordenação por blocos
    // reduz o custo para (K * N * log(N)) * (Y/X).
    //
    // A constante K é pelo menos 2.0, mas maior se há muitas colunas a ordenar, pois o tempo é
    // proporcional ao conteúdo ordenado. O algoritmo não distingue colunas gordas (BLOB e TEXT) de
    // magras (INT): usa o número de colunas como aproximação da largura da linha.
    //
    // Um fator extra de 2.0 ou 3.0 entra no custo se a ordenação usa OP_IdxInsert e OP_Sort em
    // vez de OP_SorterInsert.
    let mut n_row = n_row;
    // TUNING: custo da ordenação proporcional ao número de colunas de saída.
    let n_col = log_est(((n_select_expr + 59) / 30) as u64) as i32;
    let mut r_sort_cost: i32 = n_row as i32 + n_col;
    if n_sorted > 0 {
        // Escala o resultado por (Y/X).
        r_sort_cost += log_est(((n_order_by - n_sorted) * 100 / n_order_by) as u64) as i32 - 66;
    }

    // Multiplica por log(M), onde M é o número de linhas de saída. Usa o LIMIT para M se ele é
    // menor. Ou, se esta ordenação é para um DISTINCT, M será o número de linhas distintas, então
    // se rebaixa um pouco.
    if wf(wi.wctrl_flags, WHERE_USE_LIMIT) {
        r_sort_cost += 10; // TUNING: 2.0x extra com LIMIT
        if n_sorted != 0 {
            r_sort_cost += 6; // TUNING: 1.5x extra também com ordenação parcial
        }
        if wi.i_limit < n_row {
            n_row = wi.i_limit;
        }
    } else if wf(wi.wctrl_flags, WHERE_WANT_DISTINCT) {
        // TUNING: na ordenação de um DISTINCT, supõe-se que ele reduz o número de linhas de saída
        // por um fator de 2.
        if n_row > 10 {
            n_row -= 10;
            debug_assert!(10 == log_est(2));
        }
    }
    r_sort_cost += est_log(n_row) as i32;
    r_sort_cost as LogEst
}

/// `wherePathSolver`: dada a lista de `WhereLoop` em `wi.p_loops`, tenta achar o caminho de menor
/// custo que visita cada `WhereLoop` uma vez. O caminho é carregado nos campos `a[].p_w_loop`.
///
/// Supõe que o número total de linhas de saída a ordenar será `n_row_est` (na representação
/// 10*log2). Ou ignora o custo de ordenação se `n_row_est==0`.
///
/// Devolve `SQLITE_OK` em sucesso ou `SQLITE_ERROR` (com a mensagem "no query solution") se não há
/// solução.
pub(crate) fn where_path_solver(
    db: &mut Connection,
    parse: &mut Parse,
    wi: &mut WhereInfo,
    p_tab_list: &SrcList,
    p_select: Option<&Select>,
    n_row_est: LogEst,
) -> i32 {
    let n_loop = wi.n_level as usize; // Número de termos da junção
    let wctrl_flags: u16 = wi.wctrl_flags; // Não muda durante o resolvedor
    // TUNING: para consultas simples só o melhor caminho é seguido. Para junções de 2 tabelas, os
    // 5 melhores. Para 3 ou mais tabelas, os 10 melhores.
    let mx_choice: usize = if n_loop <= 1 {
        1
    } else if n_loop == 2 {
        5
    } else {
        10
    }; // Máximo de caminhos simultâneos
    debug_assert!(n_loop <= p_tab_list.a.len());

    // Se `n_row_est` é zero e há um ORDER BY, ele é ignorado. Neste caso o objetivo da chamada é
    // estimar o número de linhas devolvidas pela consulta toda. Com a estimativa, o chamador
    // chama a função uma segunda vez, passando-a como `n_row_est`.
    let n_order_by: usize = match wi.p_order_by.as_deref() {
        Some(l) if n_row_est != 0 => l.a.len(),
        _ => 0,
    }; // Número de termos do ORDER BY
    let n_select_expr: i32 = p_select
        .and_then(|s| s.p_e_list.as_deref())
        .map_or(0, |l| l.a.len() as i32);

    // `a_to`: os melhores caminhos da geração atual. `a_from`: os da geração anterior.
    // `a_sort_cost`: custos de ordenação total e parcial; cada elemento é zero (ainda não
    // calculado) ou o custo de ordenar `n_row_est` linhas com os X primeiros termos do ORDER BY já
    // em ordem, onde X é o índice.
    let blank = WherePath { a_loop: vec![LoopId(0); n_loop], ..WherePath::default() };
    let mut a_to: Vec<WherePath> = vec![blank.clone(); mx_choice];
    let mut a_from: Vec<WherePath> = vec![blank; mx_choice];
    let mut a_sort_cost: Vec<LogEst> = vec![0; n_order_by];
    let mut mx_i: usize = 0; // Índice da próxima entrada a substituir
    let mut mx_cost: LogEst = 0; // Custo máximo de um conjunto de caminhos
    let mut mx_unsorted: LogEst = 0; // Custo sem ordenação máximo de um conjunto de caminhos

    // Semeia a busca com um único `WherePath` sem nenhum `WhereLoop`.
    //
    // TUNING: o número de iterações não passa de 28. Se o custo de calcular um índice automático
    // não é pago nas primeiras 28 linhas, o índice automático não é usado.
    a_from[0].n_row = parse.n_query_loop.min(48);
    debug_assert!(48 == log_est(28));
    let mut n_from: usize = 1; // Número de entradas válidas de `a_from`
    debug_assert!(a_from[0].is_ordered == 0);
    if n_order_by != 0 {
        // Se `n_loop` é zero, não há termos no FROM. Como a consulta devolve no máximo uma linha,
        // o resultado já está na ordem pedida: `is_ordered` é `n_order_by`. Se `n_loop` é maior que
        // zero, `is_ordered` é -1: o resultado pode ou não estar ordenado, dependendo dos loops
        // acrescentados ao plano.
        a_from[0].is_ordered = if n_loop > 0 { -1 } else { n_order_by as i8 };
    }

    // Calcula `WherePath` cada vez mais longos usando a geração anterior como base da seguinte.
    // Guarda os `mx_choice` melhores caminhos de cada geração.
    for i_loop in 0..n_loop {
        let mut n_to: usize = 0; // Número de entradas válidas de `a_to`
        for ii in 0..n_from {
            for lj in 0..wi.p_loops.len() {
                let p_w_loop = wi.p_loops[lj];
                let (l_prereq, l_mask_self, l_ws_flags, l_r_setup, l_r_run, l_n_out) = {
                    let l = wi.w_loop(p_w_loop);
                    (l.prereq, l.mask_self, l.ws_flags, l.r_setup, l.r_run, l.n_out)
                };
                let (f_mask_loop, f_n_row, f_r_unsorted, f_is_ordered, f_rev_loop) = {
                    let f = &a_from[ii];
                    (f.mask_loop, f.n_row, f.r_unsorted, f.is_ordered, f.rev_loop)
                };

                if (l_prereq & !f_mask_loop) != 0 {
                    continue;
                }
                if (l_mask_self & f_mask_loop) != 0 {
                    continue;
                }
                if (l_ws_flags & WHERE_AUTO_INDEX) != 0 && f_n_row < 3 {
                    // Não usa um índice automático se este loop deve rodar menos de 1.25 vez. É
                    // tentador excluir também o uso de índice automático num loop externo, mas às
                    // vezes ele é útil no loop externo de uma subconsulta correlacionada.
                    debug_assert!(10 == log_est(2));
                    continue;
                }

                // Neste ponto `p_w_loop` é candidato a próximo loop. Calcula o custo dele.
                let mut r_unsorted: LogEst =
                    log_est_add(l_r_setup, (l_r_run as i32 + f_n_row as i32) as LogEst);
                r_unsorted = log_est_add(r_unsorted, f_r_unsorted);
                let n_out: LogEst = (f_n_row as i32 + l_n_out as i32) as LogEst; // Linhas visitadas por (from+loop)
                let mask_new: Bitmask = f_mask_loop | l_mask_self; // Máscara das origens visitadas
                let mut is_ordered: i8 = f_is_ordered;
                let rev_mask: Bitmask; // Máscara dos loops em ordem reversa
                if is_ordered < 0 {
                    let mut rm: Bitmask = 0;
                    is_ordered = path_satisfies(
                        db,
                        parse,
                        wi,
                        p_tab_list,
                        ObList::OrderBy,
                        &a_from[ii],
                        wctrl_flags,
                        i_loop as u16,
                        p_w_loop,
                        &mut rm,
                    );
                    rev_mask = rm;
                } else {
                    rev_mask = f_rev_loop;
                }
                let r_cost: LogEst; // Custo do caminho (from+loop)
                if is_ordered >= 0 && (is_ordered as usize) < n_order_by {
                    let io = is_ordered as usize;
                    if a_sort_cost[io] == 0 {
                        a_sort_cost[io] = where_sorting_cost(
                            wi,
                            n_select_expr,
                            n_row_est,
                            n_order_by as i32,
                            is_ordered as i32,
                        );
                    }
                    // TUNING: uma pequena penalidade extra (3) na ordenação, para estimular ainda
                    // mais o planejador a escolher um plano em que as linhas saem na ordem certa
                    // sem ordenação.
                    r_cost = (log_est_add(r_unsorted, a_sort_cost[io]) as i32 + 3) as LogEst;
                } else {
                    r_cost = r_unsorted;
                    r_unsorted = (r_unsorted as i32 - 2) as LogEst; // TUNING: leve viés a favor de planos sem ordenação
                }

                // Vê se `p_w_loop` deve entrar no conjunto dos `mx_choice` melhores caminhos até
                // agora.
                //
                // Primeiro procura, entre os melhores até agora, um caminho que cubra o mesmo
                // conjunto de loops e tenha o mesmo `is_ordered` do candidato.
                //
                // O termo "((pTo->isOrdered^isOrdered)&0x80)==0" equivale a
                // "(pTo->isOrdered==(-1))==(isOrdered==(-1))" para os valores legais de
                // `is_ordered`, -1..64.
                let mut jj: usize = 0;
                while jj < n_to {
                    if a_to[jj].mask_loop == mask_new
                        && (((a_to[jj].is_ordered ^ is_ordered) as u8) & 0x80) == 0
                    {
                        break;
                    }
                    jj += 1;
                }
                if jj >= n_to {
                    // Nenhum dos melhores até agora casa com o candidato.
                    if n_to >= mx_choice
                        && (r_cost > mx_cost || (r_cost == mx_cost && r_unsorted >= mx_unsorted))
                    {
                        // O candidato não é melhor que nenhum dos `mx_choice` caminhos do
                        // buffer: descartado, não é viável.
                        continue;
                    }
                    // O caminho candidato entra no conjunto dos melhores até agora.
                    if n_to < mx_choice {
                        // Aumenta o conjunto `a_to` em um.
                        jj = n_to;
                        n_to += 1;
                    } else {
                        // O caminho novo substitui o pior anterior, para manter a contagem abaixo
                        // de `mx_choice`.
                        jj = mx_i;
                    }
                } else {
                    // O melhor até agora `a_to[jj]` cobre o mesmo conjunto de loops e tem o mesmo
                    // `is_ordered` do candidato. Vê se o candidato o substitui ou se é descartado.
                    //
                    // A condição é uma comparação vetorial expandida, equivalente a:
                    //   (pTo->rCost,pTo->nRow,pTo->rUnsorted) <= (rCost,nOut,rUnsorted)
                    let p_to = &a_to[jj];
                    if p_to.r_cost < r_cost
                        || (p_to.r_cost == r_cost
                            && (p_to.n_row < n_out
                                || (p_to.n_row == n_out && p_to.r_unsorted <= r_unsorted)))
                    {
                        // Descarta o caminho candidato.
                        continue;
                    }
                    // O candidato é melhor que o caminho `p_to`: o substitui.
                }
                // `p_w_loop` é vencedor. Entra no conjunto dos melhores até agora.
                {
                    let p_to = &mut a_to[jj];
                    p_to.mask_loop = f_mask_loop | l_mask_self;
                    p_to.rev_loop = rev_mask;
                    p_to.n_row = n_out;
                    p_to.r_cost = r_cost;
                    p_to.r_unsorted = r_unsorted;
                    p_to.is_ordered = is_ordered;
                    p_to.a_loop[..i_loop].copy_from_slice(&a_from[ii].a_loop[..i_loop]);
                    p_to.a_loop[i_loop] = p_w_loop;
                }
                if n_to >= mx_choice {
                    mx_i = 0;
                    mx_cost = a_to[0].r_cost;
                    mx_unsorted = a_to[0].n_row;
                    for jj in 1..mx_choice {
                        let p_to = &a_to[jj];
                        if p_to.r_cost > mx_cost
                            || (p_to.r_cost == mx_cost && p_to.r_unsorted > mx_unsorted)
                        {
                            mx_cost = p_to.r_cost;
                            mx_unsorted = p_to.r_unsorted;
                            mx_i = jj;
                        }
                    }
                }
            }
        }

        // Troca os papéis de `a_from` e `a_to` para a próxima geração.
        std::mem::swap(&mut a_from, &mut a_to);
        n_from = n_to;
    }

    if n_from == 0 {
        error_msg(db, parse, b"no query solution", &[]);
        return SQLITE_ERROR;
    }

    // Acha o caminho de menor custo. `p_from` fica apontando para ele.
    let mut p_from: usize = 0;
    for ii in 1..n_from {
        if a_from[p_from].r_cost > a_from[ii].r_cost {
            p_from = ii;
        }
    }
    debug_assert!(wi.n_level as usize == n_loop);
    // Carrega o caminho de menor custo no `wi`.
    for i_loop in 0..n_loop {
        let p_w_loop = a_from[p_from].a_loop[i_loop];
        let i_tab = wi.w_loop(p_w_loop).i_tab;
        let p_level = &mut wi.a[i_loop];
        p_level.p_w_loop = p_w_loop;
        p_level.i_from = i_tab;
        p_level.i_tab_cur = p_tab_list.a[i_tab as usize].i_cursor;
    }
    if wf(wi.wctrl_flags, WHERE_WANT_DISTINCT)
        && !wf(wi.wctrl_flags, WHERE_DISTINCTBY)
        && wi.e_distinct as u32 == WHERE_DISTINCT_NOOP
        && n_row_est != 0
        && n_loop > 0
    {
        let mut not_used: Bitmask = 0;
        let last = a_from[p_from].a_loop[n_loop - 1];
        let rc = path_satisfies(
            db,
            parse,
            wi,
            p_tab_list,
            ObList::ResultSet,
            &a_from[p_from],
            WHERE_DISTINCTBY as u16,
            (n_loop - 1) as u16,
            last,
            &mut not_used,
        );
        let n_result = wi.p_result_set.as_deref().map_or(0, |l| l.a.len() as i32);
        if rc as i32 == n_result {
            wi.e_distinct = WHERE_DISTINCT_ORDERED as u8;
        }
    }
    wi.b_ordered_inner_loop = false;
    if wi.p_order_by.is_some() {
        let n_ob_expr = wi.p_order_by.as_deref().map_or(0, |l| l.a.len() as i32);
        wi.n_ob_sat = a_from[p_from].is_ordered;
        if wf(wi.wctrl_flags, WHERE_DISTINCTBY) {
            if a_from[p_from].is_ordered as i32 == n_ob_expr {
                wi.e_distinct = WHERE_DISTINCT_ORDERED as u8;
            }
        } else {
            wi.rev_mask = a_from[p_from].rev_loop;
            if wi.n_ob_sat <= 0 {
                wi.n_ob_sat = 0;
                if n_loop > 0 {
                    let last = a_from[p_from].a_loop[n_loop - 1];
                    let ws_flags = wi.w_loop(last).ws_flags;
                    if (ws_flags & WHERE_ONEROW) == 0
                        && (ws_flags & (WHERE_IPK | WHERE_COLUMN_IN)) != (WHERE_IPK | WHERE_COLUMN_IN)
                    {
                        let mut m: Bitmask = 0;
                        let rc = path_satisfies(
                            db,
                            parse,
                            wi,
                            p_tab_list,
                            ObList::OrderBy,
                            &a_from[p_from],
                            WHERE_ORDERBY_LIMIT as u16,
                            (n_loop - 1) as u16,
                            last,
                            &mut m,
                        );
                        if rc as i32 == n_ob_expr {
                            wi.b_ordered_inner_loop = true;
                            wi.rev_mask = m;
                        }
                    }
                }
            } else if n_loop != 0
                && wi.n_ob_sat == 1
                && wf(wi.wctrl_flags, WHERE_ORDERBY_MIN | WHERE_ORDERBY_MAX)
            {
                wi.b_ordered_inner_loop = true;
            }
        }
        if wf(wi.wctrl_flags, WHERE_SORTBYGROUP) && wi.n_ob_sat as i32 == n_ob_expr && n_loop > 0 {
            let mut rev_mask: Bitmask = 0;
            let last = a_from[p_from].a_loop[n_loop - 1];
            let n_order = path_satisfies(
                db,
                parse,
                wi,
                p_tab_list,
                ObList::OrderBy,
                &a_from[p_from],
                0,
                (n_loop - 1) as u16,
                last,
                &mut rev_mask,
            );
            debug_assert!(!wi.sorted);
            if n_order as i32 == n_ob_expr {
                wi.sorted = true;
                wi.rev_mask = rev_mask;
            }
        }
    }

    wi.n_row_out = a_from[p_from].n_row;
    SQLITE_OK
}

// ---------------------------------------------------------------------------------------------
// Chunk 013: heurística entre os resolvedores, atalho, omissão de junções, filtro de Bloom
// ---------------------------------------------------------------------------------------------

/// `whereInterstageHeuristic`: uma heurística para melhorar o planejamento, chamada entre a
/// primeira e a segunda chamada de `wherePathSolver()` (daí o nome).
///
/// A primeira chamada ("solver()") calcula o melhor caminho sem considerar a ordem das saídas. A
/// segunda se apoia na primeira para tentar achar um caminho alternativo que satisfaça o ORDER BY.
///
/// Esta rotina olha o resultado da primeira execução e, para todo termo do FROM do plano que usa
/// uma restrição de igualdade contra um índice, desabilita os outros `WhereLoop` do mesmo termo
/// que tentariam uma varredura completa da tabela. Isso impede que uma busca por índice seja
/// trocada por uma varredura completa para satisfazer um ORDER BY: mesmo que a varredura sem
/// ordenação possa ser um pouco melhor se as estimativas são muito precisas, ela pode degradar
/// muito se a estimativa de saída é grande demais. É melhor errar para o lado da cautela.
///
/// Exceto que, se a primeira chamada gerou uma varredura completa num laço externo, a análise
/// para na primeira varredura completa, pois a segunda chamada pode trocá-la por outra para pôr a
/// saída na ordem certa. Ou seja, permite-se a reescrita:
///
/// ```text
///     Primeiro solver()                   Segundo solver()
///       |-- SCAN t1                         |-- SCAN t2
///       |-- SEARCH t2                       `-- SEARCH t1
///       `-- SORT USING B-TREE
/// ```
///
/// O objetivo é proibir reescritas como esta:
///
/// ```text
///     Primeiro solver()                   Segundo solver()
///       |-- SEARCH t1                       |-- SCAN t2     <--- ruim!
///       |-- SEARCH t2                       `-- SEARCH t1
///       `-- SORT USING B-TREE
/// ```
fn where_interstage_heuristic(wi: &mut WhereInfo) {
    for i in 0..wi.n_level as usize {
        let p = wi.a[i].p_w_loop;
        let (p_ws_flags, i_tab) = {
            let l = wi.w_loop(p);
            (l.ws_flags, l.i_tab)
        };
        if (p_ws_flags & WHERE_VIRTUALTABLE) != 0 {
            continue;
        }
        if (p_ws_flags & (WHERE_COLUMN_EQ | WHERE_COLUMN_NULL | WHERE_COLUMN_IN)) != 0 {
            for lj in 0..wi.p_loops.len() {
                let id = wi.p_loops[lj];
                let p_loop = wi.w_loop_mut(id);
                if p_loop.i_tab != i_tab {
                    continue;
                }
                if (p_loop.ws_flags & (WHERE_CONSTRAINT | WHERE_AUTO_INDEX)) != 0 {
                    // Índices automáticos e loops restritos por índice podem ficar.
                    continue;
                }
                p_loop.prereq = ALLBITS; // Impede o segundo solver() de usar este.
            }
        } else {
            break;
        }
    }
}

/// `whereShortCut`: a maioria das consultas usa uma só tabela (não são junções) e tem restrições
/// `==` simples contra campos indexados. Esta rotina tenta planejar esses casos com muito menos
/// cerimônia que o planejador geral, o que acelera o `sqlite3_prepare()` do caso comum.
///
/// Devolve verdadeiro em sucesso, quando a consulta é tratada por este planejador sem frescuras.
/// Devolve falso se a consulta precisa do planejador geral.
fn where_short_cut(
    db: &mut Connection,
    parse: &mut Parse,
    wi: &mut WhereInfo,
    p_tab_list: &SrcList,
    builder: &mut WhereLoopBuilder,
) -> bool {
    if wf(wi.wctrl_flags, WHERE_OR_SUBCLAUSE) {
        return false;
    }
    debug_assert!(!p_tab_list.a.is_empty());
    let p_item = &p_tab_list.a[0];
    let Some(p_tab) = p_item.p_tab.clone() else {
        return false;
    };
    if p_tab.is_virtual() {
        return false;
    }
    if p_item.fg.is_indexed_by || p_item.fg.not_indexed {
        return false;
    }
    let i_cur = p_item.i_cursor;
    let p_wc = wi.s_wc;
    let p_loop = &mut builder.p_new;
    p_loop.ws_flags = 0;
    p_loop.n_skip = 0;
    let mut scan = WhereScan::default();
    let mut p_term = where_scan_init(
        db,
        parse,
        &*wi,
        &mut scan,
        p_wc,
        i_cur,
        -1,
        (WO_EQ | WO_IS) as u32,
        None,
    );
    while let Some(t) = p_term {
        if wi.term(t).prereq_right == 0 {
            break;
        }
        p_term = where_scan_next(db, parse, &*wi, &mut scan);
    }
    if let Some(t) = p_term {
        p_loop.ws_flags = WHERE_COLUMN_EQ | WHERE_IPK | WHERE_ONEROW;
        p_loop.a_l_term[0] = Some(t);
        p_loop.n_l_term = 1;
        p_loop.btree.n_eq = 1;
        // TUNING: o custo de uma busca por rowid é 10.
        p_loop.r_run = 33; // 33==sqlite3LogEst(10)
    } else {
        for p_idx in p_tab.p_index.iter() {
            debug_assert!(p_loop.a_l_term.len() >= 3);
            if !p_idx.is_unique_index()
                || p_idx.p_partial_idx_where.is_some()
                || p_idx.n_key_col as usize > 3
            {
                continue;
            }
            let op_mask: u16 = if p_idx.uniq_not_null { WO_EQ | WO_IS } else { WO_EQ };
            let mut j: usize = 0;
            while j < p_idx.n_key_col as usize {
                let mut p_term = where_scan_init(
                    db,
                    parse,
                    &*wi,
                    &mut scan,
                    p_wc,
                    i_cur,
                    j as i32,
                    op_mask as u32,
                    Some((&p_tab, &**p_idx)),
                );
                while let Some(t) = p_term {
                    if wi.term(t).prereq_right == 0 {
                        break;
                    }
                    p_term = where_scan_next(db, parse, &*wi, &mut scan);
                }
                match p_term {
                    None => break,
                    Some(t) => p_loop.a_l_term[j] = Some(t),
                }
                j += 1;
            }
            if j != p_idx.n_key_col as usize {
                continue;
            }
            p_loop.ws_flags = WHERE_COLUMN_EQ | WHERE_ONEROW | WHERE_INDEXED;
            if p_idx.is_covering || (p_item.col_used & p_idx.col_not_idxed) == 0 {
                p_loop.ws_flags |= WHERE_IDX_ONLY;
            }
            p_loop.n_l_term = j as u16;
            p_loop.btree.n_eq = j as u16;
            p_loop.btree.p_index = Some(p_idx.clone());
            // TUNING: o custo de uma busca por índice único é 15.
            p_loop.r_run = 39; // 39==sqlite3LogEst(15)
            break;
        }
    }
    if p_loop.ws_flags != 0 {
        p_loop.n_out = 1;
        debug_assert!(wi.s_mask_set.n == 1 && i_cur == wi.s_mask_set.ix[0]);
        p_loop.mask_self = 1; // sqlite3WhereGetMask(&pWInfo->sMaskSet, iCur);
        wi.a[0].i_tab_cur = i_cur;
        wi.n_row_out = 1;
        if let Some(ob) = wi.p_order_by.as_deref() {
            wi.n_ob_sat = ob.a.len() as i8;
        }
        if wf(wi.wctrl_flags, WHERE_WANT_DISTINCT) {
            wi.e_distinct = WHERE_DISTINCT_UNIQUE as u8;
        }
        if scan.i_equiv > 1 {
            p_loop.ws_flags |= WHERE_TRANSCONS;
        }
        // O `pNew` do C vive junto do `WhereInfo`; aqui entra uma cópia na arena de loops (sem
        // entrar em `p_loops`, que também não o contém no C).
        let copy = (*builder.p_new).clone();
        wi.loops.push(copy);
        wi.a[0].p_w_loop = LoopId((wi.loops.len() - 1) as u32);
        return true;
    }
    false
}

/// `exprNodeIsDeterministic`: função auxiliar de `exprIsDeterministic()`.
fn expr_node_is_deterministic(w: &mut Walker<()>, p_expr: &mut Expr) -> i32 {
    if p_expr.op == TK_FUNCTION && !p_expr.has_property(EP_CONST_FUNC) {
        w.e_code = 0;
        return WRC_ABORT;
    }
    WRC_CONTINUE
}

/// `exprIsDeterministic`: verdadeiro se a expressão não contém funções SQL não determinísticas.
/// Funções não determinísticas de sub-selects não são consideradas.
fn expr_is_deterministic(p: &mut Expr) -> bool {
    let mut w: Walker<()> = Walker {
        x_expr_callback: Some(expr_node_is_deterministic),
        x_select_callback: Some(select_walk_fail::<()>),
        x_select_callback2: None,
        walker_depth: 0,
        e_code: 1,
        m_w_flags: 0,
        u: (),
    };
    walk_expr(&mut w, Some(p));
    w.e_code != 0
}

/// `whereOmitNoopJoin`: tenta omitir da junção as tabelas que não afetam o resultado. Para uma
/// tabela não afetar o resultado, é preciso que:
///
/// ```text
///   1) A consulta não seja de agregado.
///   2) A tabela seja o lado direito de um LEFT JOIN.
///   3) Ou a consulta seja DISTINCT, ou o ON ou USING contenha uma restrição que limite a
///      varredura da tabela a no máximo uma linha.
///   4) A tabela não seja referenciada por nenhuma parte da consulta além do próprio USING ou ON.
///   5) A tabela não tenha ON ou USING de junção interna se há um RIGHT JOIN na consulta. Senão o
///      ON/USING poderia passar do lado direito para o esquerdo do RIGHT JOIN. Nota: pela (2),
///      isso só acontece se a tabela é a mais à direita de uma subconsulta aplanada no principal e
///      essa subconsulta era o operando direito de uma junção interna com ON ou USING.
///   6) O ORDER BY tenha 63 termos ou menos.
///   7) A otimização de omitir junções inúteis esteja habilitada.
/// ```
///
/// Os itens (1), (6) e (7) são conferidos por quem chama.
///
/// Por exemplo, dado `t1(ipk INTEGER PRIMARY KEY, v1)`, `t2(ipk ..., v2)` e `t3(ipk ..., v3)`, a
/// tabela t2 pode ser omitida de:
///
/// ```text
///     SELECT v1, v3 FROM t1
///       LEFT JOIN t2 ON (t1.ipk=t2.ipk)
///       LEFT JOIN t3 ON (t1.ipk=t3.ipk)
/// ```
///
/// ou de:
///
/// ```text
///     SELECT DISTINCT v1, v3 FROM t1
///       LEFT JOIN t2
///       LEFT JOIN t3 ON (t1.ipk=t3.ipk)
/// ```
fn where_omit_noop_join(wi: &mut WhereInfo, p_tab_list: &SrcList, not_ready: Bitmask) -> Bitmask {
    let mut not_ready = not_ready;

    // Pré-condições conferidas por quem chama.
    debug_assert!(wi.n_level >= 2);

    // Estas duas pré-condições, conferidas por quem chama, garantem a condição (1) do comentário.
    debug_assert!(wi.p_result_set.is_some());
    debug_assert!(!wf(wi.wctrl_flags, WHERE_AGG_DISTINCT));

    let mut tab_used = where_expr_list_usage(&mut wi.s_mask_set, wi.p_result_set.as_deref());
    if wi.p_order_by.is_some() {
        tab_used |= where_expr_list_usage(&mut wi.s_mask_set, wi.p_order_by.as_deref());
    }
    let has_right_join = (p_tab_list.a[0].fg.jointype & JT_LTORJ) != 0;
    let wc = wi.s_wc;
    for i in (1..wi.n_level as usize).rev() {
        let p_loop_id = wi.a[i].p_w_loop;
        let (l_i_tab, l_mask_self, l_ws_flags) = {
            let l = wi.w_loop(p_loop_id);
            (l.i_tab, l.mask_self, l.ws_flags)
        };
        let p_item = &p_tab_list.a[l_i_tab as usize];
        if (p_item.fg.jointype & (JT_LEFT | JT_RIGHT)) != JT_LEFT {
            continue;
        }
        if !wf(wi.wctrl_flags, WHERE_WANT_DISTINCT) && (l_ws_flags & WHERE_ONEROW) == 0 {
            continue;
        }
        if (tab_used & l_mask_self) != 0 {
            continue;
        }
        let n_term = wi.n_term(wc);
        let mut broke = false;
        for k in 0..n_term {
            let t = wi.term_at(wc, k);
            let prereq_all = wi.term(t).prereq_all;
            let e = wi.expr(t);
            if (prereq_all & l_mask_self) != 0
                && (!e.has_property(EP_OUTER_ON) || e.i_join() != p_item.i_cursor)
            {
                broke = true;
                break;
            }
            if has_right_join && e.has_property(EP_INNER_ON) && e.i_join() == p_item.i_cursor {
                broke = true; // restrição (5)
                break;
            }
        }
        if broke {
            continue;
        }
        not_ready &= !l_mask_self;
        for k in 0..n_term {
            let t = wi.term_at(wc, k);
            if (wi.term(t).prereq_all & l_mask_self) != 0 {
                wi.term_mut(t).wt_flags |= TERM_CODED;
            }
        }
        let n_level = wi.n_level as usize;
        if i != n_level - 1 {
            // O `memmove` dos níveis seguintes para a vaga de `i`.
            wi.a[i..n_level].rotate_left(1);
        }
        wi.n_level -= 1;
        debug_assert!(wi.n_level > 0);
    }
    not_ready
}

/// `whereCheckIfBloomFilterIsUseful`: vê se há laços SEARCH que se beneficiariam de um filtro de
/// Bloom. Considera-se um filtro de Bloom se:
///
/// ```text
///   (1)  O SEARCH acontece mais de N vezes, onde N é o número de linhas da tabela considerada
///        para o filtro.
///   (2)  Espera-se que algumas buscas não achem nenhuma linha. (Determinado pelo flag
///        WHERE_SELFCULL no termo.)
///   (3)  O processamento do filtro de Bloom não está desabilitado. (Conferido por quem chama.)
///   (4)  O tamanho da tabela pesquisada é conhecido pelo ANALYZE.
/// ```
///
/// Este bloco só confere se um filtro de Bloom seria apropriado e, se for, liga o flag
/// `WHERE_BLOOMFILTER` no `WhereLoop`. A implementação do filtro fica adiante, onde o código de
/// cada `WhereLoop` é gerado.
fn where_check_if_bloom_filter_is_useful(
    db: &mut Connection,
    wi: &mut WhereInfo,
    p_tab_list: &SrcList,
) {
    let mut n_search: LogEst = 0;

    debug_assert!(wi.n_level >= 2);
    for i in 0..wi.n_level as usize {
        let p_loop_id = wi.a[i].p_w_loop;
        let (l_i_tab, l_ws_flags, l_n_out) = {
            let l = wi.w_loop(p_loop_id);
            (l.i_tab, l.ws_flags, l.n_out)
        };
        let req_flags: u32 = WHERE_SELFCULL | WHERE_COLUMN_EQ;
        let p_item = &p_tab_list.a[l_i_tab as usize];
        let Some(p_tab) = p_item.p_tab.as_ref() else {
            break;
        };
        if (p_tab.tab_flags & TF_HAS_STAT1) == 0 {
            break;
        }
        table_flags_or(db, p_tab, TF_MAYBE_REANALYZE);
        if i >= 1
            && (l_ws_flags & req_flags) == req_flags
            // Sempre é o caso se WHERE_COLUMN_EQ está definido.
            && (l_ws_flags & (WHERE_IPK | WHERE_INDEXED)) != 0
            && n_search > p_tab.n_row_log_est
        {
            let l = wi.w_loop_mut(p_loop_id);
            l.ws_flags |= WHERE_BLOOMFILTER;
            l.ws_flags &= !WHERE_IDX_ONLY;
        }
        n_search = (n_search as i32 + l_n_out as i32) as LogEst;
    }
}

// ---------------------------------------------------------------------------------------------
// Chunk 014: subtipo, expressões indexadas, ordem reversa e sqlite3WhereBegin
// ---------------------------------------------------------------------------------------------

/// O contexto de `exprNodeCanReturnSubtype`: o `pWalker->pParse->db` do C.
struct SubtypeCtx<'a> {
    /// A conexão, para procurar a função.
    db: &'a mut Connection,
}

/// `exprNodeCanReturnSubtype`: callback de nó de expressão de `sqlite3ExprCanReturnSubtype()`.
///
/// Só uma chamada de função pode devolver um subtipo. Então, se o nó não é chamada de função,
/// devolve `WRC_PRUNE` na hora. Uma chamada de função pode devolver subtipo se tem a propriedade
/// `SQLITE_RESULT_SUBTYPE`.
///
/// Supõe-se que toda função pode repassar um subtipo de um dos argumentos (usando
/// `sqlite3_result_value()`). A maioria não faz isso, mas não há mecanismo para distinguir. Logo,
/// se um argumento é outra função que pode devolver subtipo, esta também pode.
fn expr_node_can_return_subtype(w: &mut Walker<SubtypeCtx<'_>>, p_expr: &mut Expr) -> i32 {
    if p_expr.op != TK_FUNCTION {
        return WRC_PRUNE;
    }
    debug_assert!(p_expr.use_x_list());
    let n = p_expr.x_list().map_or(0, |l| l.a.len()) as i32;
    let z_name: Vec<u8> = p_expr.z_token().map_or_else(Vec::new, |z| z.to_vec());
    let enc = w.u.db.enc;
    let p_def = find_function(w.u.db, &z_name, n, enc, 0);
    match p_def {
        Some(d) if (d.func_flags & SQLITE_RESULT_SUBTYPE as u32) == 0 => WRC_CONTINUE,
        _ => {
            w.e_code = 1;
            WRC_PRUNE
        }
    }
}

/// `sqlite3ExprCanReturnSubtype`: verdadeiro se a expressão pode devolver um subtipo. Um
/// verdadeiro não garante que um subtipo será devolvido, só indica que é possível. Falsos
/// positivos são aceitáveis (só desligam uma otimização). Falsos negativos podem dar respostas
/// erradas.
fn expr_can_return_subtype(db: &mut Connection, p_expr: &mut Expr) -> bool {
    let mut w: Walker<SubtypeCtx<'_>> = Walker {
        x_expr_callback: Some(expr_node_can_return_subtype),
        x_select_callback: None,
        x_select_callback2: None,
        walker_depth: 0,
        e_code: 0,
        m_w_flags: 0,
        u: SubtypeCtx { db },
    };
    walk_expr(&mut w, Some(p_expr));
    w.e_code != 0
}

/// `whereAddIndexedExpr`: o índice `p_idx` é usado por uma consulta e contém uma ou mais
/// expressões. Em outras palavras, é um índice sobre expressão. `i_idx_cur` é o número do cursor
/// do índice. `p_tab` é a tabela dona do índice (o `pIdx->pTable` do C) e `p_tab_item` a entrada
/// do FROM dela.
///
/// Acrescenta entradas `IndexedExpr` a `parse.p_idx_epr` para cada expressão do índice, para que o
/// gerador de código de expressões saiba trocar as ocorrências da expressão indexada por
/// referências à coluna correspondente do índice. O mais novo é o último do `Vec`.
fn where_add_indexed_expr(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab: &Rc<Table>,
    p_idx: &Index,
    i_idx_cur: i32,
    p_tab_item: &SrcItem,
) {
    debug_assert!(p_idx.b_has_expr);
    let z_aff = index_affinity_str(p_idx, p_tab);
    for i in 0..p_idx.n_column as usize {
        let j = p_idx.ai_column[i];
        let p_expr: &Expr = if j == XN_EXPR {
            match p_idx
                .a_col_expr
                .as_deref()
                .and_then(|l| l.a.get(i))
                .and_then(|it| it.p_expr.as_deref())
            {
                Some(e) => e,
                None => continue,
            }
        } else if j >= 0 && (p_tab.a_col[j as usize].col_flags & COLFLAG_VIRTUAL) != 0 {
            match column_expr(p_tab, &p_tab.a_col[j as usize]) {
                Some(e) => e,
                None => continue,
            }
        } else {
            continue;
        };
        // As verificações abaixo percorrem a árvore com um `Walker`, que exige `&mut`: trabalham
        // numa cópia (só leitura).
        let mut copy: Expr = p_expr.clone();
        if expr_is_constant(None, Some(&mut copy)) != 0 {
            continue;
        }
        if copy.op == TK_FUNCTION && expr_can_return_subtype(db, &mut copy) {
            // Funções que podem definir um subtipo não devem ser trocadas pelo valor tirado de
            // um índice de expressão, pois o índice omite o subtipo.
            continue;
        }
        let p = IndexedExpr {
            p_expr: expr_dup(Some(p_expr), 0),
            i_data_cur: p_tab_item.i_cursor,
            i_idx_cur,
            i_idx_col: i as i32,
            b_maybe_null_row: (p_tab_item.fg.jointype & (JT_LEFT | JT_LTORJ | JT_RIGHT)) != 0,
            aff: z_aff.get(i).copied().unwrap_or(0),
            z_idx_name: Some(p_idx.z_name.clone()),
        };
        parse.p_idx_epr.push(p);
    }
}

/// `whereReverseScanOrder`: liga o bit de varredura reversa em todas as tabelas da consulta, com
/// exceção das expressões de tabela comuns MATERIALIZED que têm ORDER BY próprio.
///
/// Implementa o `PRAGMA reverse_unordered_selects=ON` (e `SQLITE_DBCONFIG_REVERSE_SCANORDER`).
fn where_reverse_scan_order(wi: &mut WhereInfo, parse: &Parse, p_tab_list: &SrcList) {
    for ii in 0..p_tab_list.a.len() {
        let p_item = &p_tab_list.a[ii];
        let m10d_yes = p_item.fg.is_cte
            && match &p_item.u2 {
                SrcU2::CteUse(Some(id)) => parse.cte_uses[id.0 as usize].e_m10d == M10D_YES,
                _ => false,
            };
        if !p_item.fg.is_cte
            || !m10d_yes
            || p_item.p_select.as_deref().map_or(true, |s| s.p_order_by.is_none())
        {
            wi.rev_mask |= 1u64 << ii;
        }
    }
}

/// `sqlite3WhereBegin`: gera o começo do laço usado no processamento da cláusula WHERE. O retorno
/// é a estrutura opaca com a informação necessária para terminar o laço. Depois quem chamou deve
/// invocar `where_end()` com o valor devolvido para completar o processamento. Se há erro,
/// devolve `None`.
///
/// A ideia básica é um laço aninhado, um por tabela do FROM do select. (INSERT e UPDATE são iguais
/// a um SELECT com uma só tabela no FROM.) Por exemplo, com `SELECT * FROM t1, t2, t3 WHERE ...`,
/// o código gerado é conceitualmente:
///
/// ```text
///      foreach row1 in t1 do       \    Código gerado
///        foreach row2 in t2 do      |-- por sqlite3WhereBegin()
///          foreach row3 in t3 do   /
///            ...
///          end                     \    Código gerado
///        end                        |-- por sqlite3WhereEnd()
///      end                         /
/// ```
///
/// Os laços podem não estar aninhados na ordem do FROM, se outra ordem aproveita melhor os
/// índices. Quando o IN aparece no WHERE, pode haver laços extras para percorrer os valores do
/// lado direito.
///
/// Há cursores de b-tree associados a cada tabela: t1 usa `p_tab_list.a[0].i_cursor`, t2 usa
/// `a[1].i_cursor` e assim por diante. Esta rotina gera o código que abre esses cursores do VDBE
/// e `where_end()` gera o que os fecha.
///
/// O código gerado deixa os cursores apontando para as entradas apropriadas. O código `[...]` pode
/// usar `OP_Column` e `OP_Rowid` neles para extrair os dados.
///
/// Se o WHERE é vazio, os laços varrem as tabelas inteiras. Se as tabelas têm índices e há termos
/// do WHERE que os usam, a varredura completa pode ser evitada. A maior parte do trabalho é
/// conferir se há índices que aceleram os laços.
///
/// Termos do WHERE também limitam quais linhas chegam ao "..." do meio do laço: depois de cada
/// "foreach", os termos que usam só termos deste laço e dos externos são avaliados e, se falsos,
/// há um salto sobre todos os laços internos seguintes.
///
/// OUTER JOINS: um outer join de t1 e t2 é codificado conceitualmente assim:
///
/// ```text
///    foreach row1 in t1 do
///      flag = 0
///      foreach row2 in t2 do
///        start:
///          ...
///          flag = 1
///      end
///      if flag==0 then
///        move the row2 cursor to a null row
///        goto start
///      fi
///    end
/// ```
///
/// ORDER BY: `p_order_by` é a cláusula ORDER BY (ou GROUP BY, se `WHERE_GROUPBY` está nas flags)
/// de um SELECT, se há uma. Sem ORDER BY, ou se chamada de UPDATE ou DELETE, é `None`.
///
/// `i_aux_arg` é o número de cursor de um índice. Com `WHERE_OR_SUBCLAUSE`, é o cursor de um
/// índice a usar no processamento da cláusula OR: o WHERE deve usar esse cursor. Com
/// `WHERE_ONEPASS_DESIRED`, é o primeiro cursor de um vetor de cursores de todos os índices.
/// Com `WHERE_USE_LIMIT`, é o valor do limite.
///
/// `p_where` é DUPLICADO para dentro da arena do `WhereInfo` (ver o cabeçalho do módulo), e
/// `p_order_by` e `p_result_set` também viram cópias.
pub fn where_begin(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab_list: &mut SrcList,
    p_where: Option<&mut Expr>,
    p_order_by: Option<&mut ExprList>,
    p_result_set: Option<&mut ExprList>,
    p_select: Option<&mut Select>,
    wctrl_flags: u16,
    i_aux_arg: i32,
) -> Option<Box<WhereInfo>> {
    let mut wctrl_flags = wctrl_flags;
    debug_assert!(
        !wf(wctrl_flags, WHERE_ONEPASS_MULTIROW)
            || (wf(wctrl_flags, WHERE_ONEPASS_DESIRED) && !wf(wctrl_flags, WHERE_OR_SUBCLAUSE))
    );

    // Apenas um entre WHERE_OR_SUBCLAUSE e WHERE_USE_LIMIT.
    debug_assert!(!wf(wctrl_flags, WHERE_OR_SUBCLAUSE) || !wf(wctrl_flags, WHERE_USE_LIMIT));

    // Um ORDER/GROUP BY com mais de 63 termos não pode ser otimizado.
    let mut order_by: Option<&ExprList> = p_order_by.as_deref();
    if order_by.map_or(false, |l| l.a.len() >= BMS as usize) {
        order_by = None;
        wctrl_flags &= !(WHERE_WANT_DISTINCT as u16);
        wctrl_flags |= WHERE_KEEP_ALL_JOINS as u16; // Desabilita a otimização omit-noop-join.
    }

    // O número de tabelas do FROM é limitado pelo número de bits de um Bitmask.
    if p_tab_list.a.len() > BMS as usize {
        error_msg(db, parse, b"at most %d tables in a join", &[PrintfArg::Int(BMS as i64)]);
        return None;
    }

    // Esta função normalmente gera um laço aninhado para todas as tabelas de `p_tab_list`. Mas se
    // `WHERE_OR_SUBCLAUSE` está ligado, só gera código para a primeira tabela e supõe que os
    // cursores das seguintes não foram iniciados.
    let n_tab_list: usize =
        if wf(wctrl_flags, WHERE_OR_SUBCLAUSE) { 1 } else { p_tab_list.a.len() };

    // Aloca e inicia a estrutura `WhereInfo` que será o valor de retorno.
    let mut wi = Box::new(WhereInfo::default());
    wi.p_order_by = expr_list_dup(order_by, 0);
    wi.p_result_set = expr_list_dup(p_result_set.as_deref(), 0);
    wi.ai_cur_one_pass = [-1, -1];
    wi.n_level = n_tab_list as u8;
    let i_break = make_label(parse);
    wi.i_break = i_break;
    wi.i_continue = i_break;
    wi.wctrl_flags = wctrl_flags;
    wi.i_limit = i_aux_arg as LogEst;
    wi.saved_n_query_loop = parse.n_query_loop as i32;
    wi.a = vec![WhereLevel::default(); n_tab_list];
    debug_assert!(wi.e_one_pass as i32 == ONEPASS_OFF); // ONEPASS começa DESLIGADO.

    let ok = where_begin_body(
        db,
        parse,
        p_tab_list,
        &mut wi,
        p_where.as_deref(),
        order_by.is_some(),
        p_select,
        wctrl_flags,
        i_aux_arg,
        n_tab_list,
    );
    if !ok {
        // whereBeginError:
        parse.n_query_loop = wi.saved_n_query_loop as LogEst;
        where_info_free(&mut wi);
        return None;
    }
    Some(wi)
}

/// O corpo de `sqlite3WhereBegin` depois da alocação do `WhereInfo`. Devolve falso nos pontos em
/// que o C salta para `whereBeginError` (o chamador desfaz). `has_order_by` é o `pOrderBy!=0` do C
/// (a variável local, já sem a lista de 64 termos ou mais).
fn where_begin_body(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab_list: &mut SrcList,
    wi: &mut WhereInfo,
    p_where: Option<&Expr>,
    has_order_by: bool,
    mut p_select: Option<&mut Select>,
    wctrl_flags: u16,
    i_aux_arg: i32,
    n_tab_list: usize,
) -> bool {
    let mut wctrl_flags = wctrl_flags;
    let mut n_tab_list = n_tab_list;
    let mut builder = WhereLoopBuilder::default();

    // A máscara de cursores começa com `initMaskSet` (`WhereMaskSet::default`).
    where_loop_init(&mut builder.p_new);

    // Divide a cláusula WHERE em subexpressões separadas por AND.
    let wc = where_clause_init(wi);
    wi.s_wc = wc;
    builder.p_wc = wc;
    match p_where.and_then(|w| expr_dup(Some(w), 0)) {
        Some(e) => {
            let r: ExprRef = wi.exprs.add_root(e);
            wi.where_root = Some(r.clone());
            where_split(wi, wc, Some(&r), TK_AND);
        }
        None => where_split(wi, wc, None, TK_AND),
    }

    // Caso especial: sem cláusula FROM.
    if n_tab_list == 0 {
        if let Some(ob) = wi.p_order_by.as_deref() {
            wi.n_ob_sat = ob.a.len() as i8;
        }
        if wf(wctrl_flags, WHERE_WANT_DISTINCT) && db.optimization_enabled(SQLITE_DISTINCT_OPT) {
            wi.e_distinct = WHERE_DISTINCT_UNIQUE as u8;
        }
        if p_select.as_deref().map_or(false, |s| (s.sel_flags & SF_MULTIVALUE) == 0) {
            explain(parse, &*db, false, b"SCAN CONSTANT ROW", &[]);
        }
    } else {
        // Atribui um bit da máscara a cada termo do FROM. O N-ésimo termo recebe a máscara 1<<N.
        // A regra garante que, se X é a máscara de uma tabela T, X-1 é a máscara de todas as
        // tabelas à esquerda de T. Conhecer a máscara de todas as tabelas à esquerda de um left
        // join é importante (ticket #3015).
        //
        // As máscaras são criadas para todas as `p_tab_list.a.len()` tabelas, não só para as
        // `n_tab_list` primeiras: `n_tab_list` pode ser encurtado para 1 com `WHERE_OR_SUBCLAUSE`.
        let n_src = p_tab_list.a.len();
        let mut ii = 0usize;
        loop {
            create_mask(&mut wi.s_mask_set, p_tab_list.a[ii].i_cursor);
            where_tab_func_args(db, parse, wi, wc, &mut p_tab_list.a[ii]);
            ii += 1;
            if ii >= n_src {
                break;
            }
        }
    }

    // Analisa todas as subexpressões.
    where_expr_analyze(db, parse, wi, &*p_tab_list, wc);
    if let Some(s) = p_select.as_deref() {
        if s.p_limit.is_some() {
            where_add_limit(db, parse, wi, wc, s);
        }
    }
    if parse.n_err != 0 {
        return false;
    }

    // A otimização False-WHERE-Term-Bypass:
    //
    // Se há termos do WHERE que são falsos, nenhuma linha sai: pula todo o código gerado aqui.
    //
    // Condições:
    //
    //   (1)  O termo não deve se referir a nenhuma tabela da junção.
    //   (2)  O termo não deve vir de um ON do lado direito de um LEFT ou FULL JOIN.
    //   (3)  O termo não deve vir de um ON, ou não deve haver RIGHT ou FULL OUTER join na lista.
    //   (4)  Se a expressão contém funções não determinísticas fora de sub-select. Não é preciso
    //        para a correção, mas preserva o comportamento antigo do SQLite nestes dois casos:
    //
    //          WHERE random()>0;           -- avalia random() uma vez por linha
    //          WHERE (SELECT random())>0;  -- avalia random() uma vez só
    //
    // O termo não precisa ser constante para a otimização valer, só constante em relação à
    // subconsulta corrente (condição 1).
    let n_base = wi.clause(wc).n_base as usize;
    for ii in 0..n_base {
        let t = wi.term_at(wc, ii); // Um termo do WHERE
        if (wi.term(t).wt_flags & TERM_VIRTUAL) != 0 {
            continue;
        }
        let prereq_all = wi.term(t).prereq_all;
        let inner_on = wi.expr(t).has_property(EP_INNER_ON);
        debug_assert!(prereq_all != 0 || !wi.expr(t).has_property(EP_OUTER_ON));
        let ltorj = p_tab_list.a.first().map_or(false, |i| (i.fg.jointype & JT_LTORJ) != 0);
        if prereq_all == 0                                           // Condições (1) e (2)
            && (n_tab_list == 0 || expr_is_deterministic(wi.expr_mut(t))) // Condição (4)
            && !(inner_on && ltorj)                                  // Condição (3)
        {
            let i_break = wi.i_break;
            expr_if_false(db, parse, wi.expr_mut(t), i_break, SQLITE_JUMPIFNULL as i32, None);
            wi.term_mut(t).wt_flags |= TERM_CODED;
        }
    }

    if wf(wctrl_flags, WHERE_WANT_DISTINCT) {
        if db.optimization_disabled(SQLITE_DISTINCT_OPT) {
            // Desabilita a otimização DISTINCT se `SQLITE_DistinctOpt` foi ligado por
            // `sqlite3_test_ctrl(SQLITE_TESTCTRL_OPTIMIZATIONS,...)`.
            wctrl_flags &= !(WHERE_WANT_DISTINCT as u16);
            wi.wctrl_flags &= !(WHERE_WANT_DISTINCT as u16);
        } else if match wi.p_result_set.as_deref() {
            Some(rs) => is_distinct_redundant(db, parse, &*p_tab_list, &*wi, wc, rs),
            None => false,
        } {
            // A marca DISTINCT é inútil. Ignorada.
            wi.e_distinct = WHERE_DISTINCT_UNIQUE as u8;
        } else if !has_order_by {
            // Tenta fazer o ORDER BY do conjunto de resultados para facilitar o DISTINCT.
            wi.wctrl_flags |= WHERE_DISTINCTBY as u16;
            wi.p_order_by = expr_list_dup(wi.p_result_set.as_deref(), 0);
        }
    }

    // Constrói os objetos `WhereLoop`.
    if n_tab_list != 1 || !where_short_cut(db, parse, wi, &*p_tab_list, &mut builder) {
        let rc = where_loop_add_all(
            db,
            parse,
            wi,
            &*p_tab_list,
            p_select.as_deref_mut(),
            &mut builder,
        );
        if rc != 0 {
            return false;
        }

        let rc1 = where_path_solver(db, parse, wi, &*p_tab_list, p_select.as_deref(), 0);
        if wi.p_order_by.is_some() {
            if rc1 == SQLITE_OK {
                where_interstage_heuristic(wi);
            }
            let n_row_est = (wi.n_row_out as i32 + 1) as LogEst;
            where_path_solver(db, parse, wi, &*p_tab_list, p_select.as_deref(), n_row_est);
        }

        // TUNING: supõe que um DISTINCT numa subconsulta reduz o tamanho da saída por um fator 8
        // (LogEst -30).
        if wf(wi.wctrl_flags, WHERE_WANT_DISTINCT) {
            wi.n_row_out = wi.n_row_out.wrapping_sub(30);
        }
    }
    if wi.p_order_by.is_none() && (db.flags & SQLITE_REVERSE_ORDER) != 0 {
        where_reverse_scan_order(wi, &*parse, &*p_tab_list);
    }
    if parse.n_err != 0 {
        return false;
    }

    // Tenta omitir da junção as tabelas que não afetam o resultado. Ver o comentário de
    // `where_omit_noop_join`.
    let mut not_ready: Bitmask = !0;
    if wi.n_level >= 2                                                  // Tem de ser junção
        && wi.p_result_set.is_some()                                    // Condição (1)
        && !wf(wctrl_flags, WHERE_AGG_DISTINCT | WHERE_KEEP_ALL_JOINS)  // (1),(6)
        && db.optimization_enabled(SQLITE_OMIT_NOOP_JOIN)               // (7)
    {
        not_ready = where_omit_noop_join(wi, &*p_tab_list, not_ready);
        n_tab_list = wi.n_level as usize;
        debug_assert!(n_tab_list > 0);
    }

    // Vê se há laços SEARCH que se beneficiariam de um filtro de Bloom.
    if wi.n_level >= 2 && db.optimization_enabled(SQLITE_BLOOM_FILTER) {
        where_check_if_bloom_filter_is_useful(db, wi, &*p_tab_list);
    }

    parse.n_query_loop = (parse.n_query_loop as i32 + wi.n_row_out as i32) as LogEst;

    // Se quem chama é um UPDATE ou DELETE que pede o algoritmo de uma passada, determina se isso é
    // apropriado.
    //
    // A passada única vale se quem chama pediu e (a) a varredura visita no máximo uma linha ou (b)
    // todos os seguintes valem:
    //
    //   * quem chama indicou que a passada única vale com várias linhas
    //     (`WHERE_ONEPASS_MULTIROW`), e
    //   * a tabela não é virtual, e
    //   * a varredura não usa a otimização OR ou quem chama é um DELETE (`WHERE_DUPLICATES_OK` só
    //     é passada para DELETE).
    //
    // A última condição existe porque o UPDATE usa `aiCurOnePass[1]` para saber se pode mesmo
    // usar a passada única, e isso não é calculado com precisão nas varreduras com a otimização
    // OR.
    debug_assert!(!wf(wctrl_flags, WHERE_ONEPASS_DESIRED) || wi.n_level == 1);
    let mut b_fordelete: u8 = 0; // OPFLAG_FORDELETE ou zero, conforme o caso
    if wf(wctrl_flags, WHERE_ONEPASS_DESIRED) {
        let l0 = wi.a[0].p_w_loop;
        let ws_flags = wi.w_loop(l0).ws_flags;
        let b_onerow = (ws_flags & WHERE_ONEROW) != 0;
        let tab0 = p_tab_list.a[0].p_tab.clone();
        let is_virtual0 = tab0.as_ref().map_or(false, |t| t.is_virtual());
        let has_rowid0 = tab0.as_ref().map_or(true, |t| t.has_rowid());
        if b_onerow
            || (wf(wctrl_flags, WHERE_ONEPASS_MULTIROW)
                && !is_virtual0
                && ((ws_flags & WHERE_MULTI_OR) == 0 || wf(wctrl_flags, WHERE_DUPLICATES_OK))
                && db.optimization_enabled(SQLITE_ONE_PASS))
        {
            wi.e_one_pass = if b_onerow { ONEPASS_SINGLE as u8 } else { ONEPASS_MULTI as u8 };
            if has_rowid0 && (ws_flags & WHERE_IDX_ONLY) != 0 {
                if wf(wctrl_flags, WHERE_ONEPASS_MULTIROW) {
                    b_fordelete = OPFLAG_FORDELETE;
                }
                wi.w_loop_mut(l0).ws_flags = ws_flags & !WHERE_IDX_ONLY;
            }
        }
    }

    // Abre todas as tabelas de `p_tab_list` e os índices escolhidos para pesquisá-las.
    for ii in 0..n_tab_list {
        let i_from = wi.a[ii].i_from as usize;
        let (i_cursor, jointype, col_used) = {
            let it = &p_tab_list.a[i_from];
            (it.i_cursor, it.fg.jointype, it.col_used)
        };
        let p_tab: Rc<Table> = p_tab_list.a[i_from].p_tab.clone().expect("SrcItem.pTab");
        let i_db = schema_to_index(db, p_tab.p_schema); // Índice do banco com a tabela/índice
        let p_loop_id = wi.a[ii].p_w_loop;
        let ws_flags = wi.w_loop(p_loop_id).ws_flags;
        if (p_tab.tab_flags & TF_EPHEMERAL) != 0 || p_tab.is_view() {
            // Nada a fazer.
        } else if (ws_flags & WHERE_VIRTUALTABLE) != 0 {
            let a = add_op3(vdbe_of_parse(parse), OP_VOPEN as i32, i_cursor, 0, 0);
            if let Some(id) = get_vtable(db, &p_tab) {
                change_p4_vtab(vdbe_of_parse(parse), db, a, id);
            }
        } else if p_tab.is_virtual() {
            // noop
        } else if ((ws_flags & WHERE_IDX_ONLY) == 0 && !wf(wctrl_flags, WHERE_OR_SUBCLAUSE))
            || (jointype & (JT_LTORJ | JT_RIGHT)) != 0
        {
            let mut op = OP_OPENREAD;
            if wi.e_one_pass as i32 != ONEPASS_OFF {
                op = OP_OPENWRITE;
                wi.ai_cur_one_pass[0] = i_cursor;
            }
            open_table(db, parse, i_cursor, i_db, &p_tab, op);
            debug_assert!(i_cursor == wi.a[ii].i_tab_cur);
            if wi.e_one_pass as i32 == ONEPASS_OFF
                && (p_tab.n_col as i32) < BMS
                && (p_tab.tab_flags & (TF_HAS_GENERATED | TF_WITHOUT_ROWID)) == 0
                && (ws_flags & (WHERE_AUTO_INDEX | WHERE_BLOOMFILTER)) == 0
            {
                // Se sabemos que só um prefixo do registro será usado, é vantajoso reduzir o
                // campo "número de colunas" do P4 do OP_OpenRead/Write.
                let n: i32 = 64 - col_used.leading_zeros() as i32;
                change_p4(vdbe_of_parse(parse), -1, P4::Int32(n));
                debug_assert!(n <= p_tab.n_col as i32);
            }
            change_p5(vdbe_of_parse(parse), b_fordelete as u16);
        } else {
            table_lock(db, parse, i_db, p_tab.tnum, false, &p_tab.z_name);
        }
        if (ws_flags & WHERE_INDEXED) != 0 {
            let p_ix: Rc<Index> =
                wi.w_loop(p_loop_id).btree.p_index.clone().expect("WhereLoop.u.btree.pIndex");
            let mut op: u8 = OP_OPENREAD;
            let i_index_cur: i32;
            // `i_aux_arg` é sempre positivo se ONEPASS é possível.
            debug_assert!(i_aux_arg != 0 || !wf(wi.wctrl_flags, WHERE_ONEPASS_DESIRED));
            if !p_tab.has_rowid()
                && p_ix.is_primary_key_index()
                && wf(wctrl_flags, WHERE_OR_SUBCLAUSE)
            {
                // Um termo de uma otimização OR usando a PRIMARY KEY de uma tabela WITHOUT ROWID.
                // Não precisa de índice separado.
                i_index_cur = wi.a[ii].i_tab_cur;
                op = 0;
            } else if wi.e_one_pass as i32 != ONEPASS_OFF {
                let mut cur = i_aux_arg;
                debug_assert!(wf(wctrl_flags, WHERE_ONEPASS_DESIRED));
                for p_j in p_tab.p_index.iter() {
                    if Rc::ptr_eq(p_j, &p_ix) {
                        break;
                    }
                    cur += 1;
                }
                i_index_cur = cur;
                op = OP_OPENWRITE;
                wi.ai_cur_one_pass[1] = i_index_cur;
            } else if i_aux_arg != 0 && wf(wctrl_flags, WHERE_OR_SUBCLAUSE) {
                i_index_cur = i_aux_arg;
                op = OP_REOPENIDX;
            } else {
                i_index_cur = parse.n_tab;
                parse.n_tab += 1;
                if p_ix.b_has_expr && db.optimization_enabled(SQLITE_INDEXED_EXPR) {
                    where_add_indexed_expr(
                        db,
                        parse,
                        &p_tab,
                        &p_ix,
                        i_index_cur,
                        &p_tab_list.a[i_from],
                    );
                }
                if let Some(part) = p_ix.p_partial_idx_where.as_deref() {
                    if (jointype & JT_RIGHT) == 0 {
                        where_part_idx_expr(
                            db,
                            parse,
                            &p_tab,
                            part,
                            None,
                            i_index_cur,
                            Some(&p_tab_list.a[i_from]),
                        );
                    }
                }
            }
            wi.a[ii].i_idx_cur = i_index_cur;
            debug_assert!(i_index_cur >= 0);
            if op != 0 {
                add_op3(vdbe_of_parse(parse), op as i32, i_index_cur, p_ix.tnum as i32, i_db);
                set_p4_key_info(parse, db, &p_ix);
                if (ws_flags & WHERE_CONSTRAINT) != 0
                    && (ws_flags & (WHERE_COLUMN_RANGE | WHERE_SKIPSCAN)) == 0
                    && (ws_flags & WHERE_BIGNULL_SORT) == 0
                    && (ws_flags & WHERE_IN_SEEKSCAN) == 0
                    && !wf(wi.wctrl_flags, WHERE_ORDERBY_MIN)
                    && wi.e_distinct as u32 != WHERE_DISTINCT_ORDERED
                {
                    change_p5(vdbe_of_parse(parse), OPFLAG_SEEKEQ as u16);
                }
                vdbe_comment(vdbe_of_parse(parse), b"%s", &[text_arg(&p_ix.z_name)]);
            }
        }
        if i_db >= 0 {
            code_verify_schema(db, parse, i_db);
        }
        if (jointype & JT_RIGHT) != 0 {
            let mut rj = WhereRightJoin::default();
            rj.i_match = parse.n_tab;
            parse.n_tab += 1;
            parse.n_mem += 1;
            rj.reg_bloom = parse.n_mem;
            add_op2(vdbe_of_parse(parse), OP_BLOB as i32, 65536, rj.reg_bloom);
            parse.n_mem += 1;
            rj.reg_return = parse.n_mem;
            add_op2(vdbe_of_parse(parse), OP_NULL as i32, 0, rj.reg_return);
            if p_tab.has_rowid() {
                add_op2(vdbe_of_parse(parse), OP_OPENEPHEMERAL as i32, rj.i_match, 1);
                let mut p_info: KeyInfo = key_info_alloc(db, 1, 0);
                p_info.a_coll[0] = None;
                p_info.a_sort_flags[0] = 0;
                append_p4(vdbe_of_parse(parse), P4::KeyInfo(Rc::new(p_info)));
            } else if let Some(p_pk) = primary_key_index(&p_tab).cloned() {
                add_op2(
                    vdbe_of_parse(parse),
                    OP_OPENEPHEMERAL as i32,
                    rj.i_match,
                    p_pk.n_key_col as i32,
                );
                set_p4_key_info(parse, db, &p_pk);
            }
            wi.w_loop_mut(p_loop_id).ws_flags &= !WHERE_IDX_ONLY;
            // A natureza do processamento de RIGHT JOIN bagunça a ordem de saída. Então omite
            // qualquer otimização de eliminação de ORDER BY/GROUP BY. É preciso uma ordenação
            // de verdade para o RIGHT JOIN.
            wi.n_ob_sat = 0;
            wi.e_distinct = WHERE_DISTINCT_UNORDERED as u8;
            wi.a[ii].p_rj = Some(Box::new(rj));
        }
    }
    wi.i_top = current_addr(parse);

    // Gera o código da busca. Cada iteração do laço abaixo gera o código de um laço aninhado do
    // programa da VM.
    for ii in 0..n_tab_list {
        if parse.n_err != 0 {
            return false;
        }
        let i_from = wi.a[ii].i_from as usize;
        let ws_flags = wi.w_loop(wi.a[ii].p_w_loop).ws_flags;
        let (is_materialized, is_correlated, reg_return, addr_fill_sub) = {
            let s = &p_tab_list.a[i_from];
            (s.fg.is_materialized, s.fg.is_correlated, s.reg_return, s.addr_fill_sub)
        };
        if is_materialized {
            if is_correlated {
                add_op2(vdbe_of_parse(parse), OP_GOSUB as i32, reg_return, addr_fill_sub);
            } else {
                let i_once = add_op0(vdbe_of_parse(parse), OP_ONCE as i32);
                add_op2(vdbe_of_parse(parse), OP_GOSUB as i32, reg_return, addr_fill_sub);
                jump_here(vdbe_of_parse(parse), i_once);
            }
        }
        if (ws_flags & (WHERE_AUTO_INDEX | WHERE_BLOOMFILTER)) != 0 {
            if (ws_flags & WHERE_AUTO_INDEX) != 0 {
                construct_automatic_index(db, parse, p_tab_list, wi, wc, not_ready, ii);
            } else {
                construct_bloom_filter(db, parse, &*p_tab_list, wi, ii, not_ready);
            }
        }
        // O endereço do OP_Explain só serviria a `sqlite3WhereAddScanStatus`, ausente no Debian.
        where_explain_one_scan(&*db, parse, &*p_tab_list, &*wi, ii, wctrl_flags);
        wi.a[ii].addr_body = current_addr(parse);
        not_ready = where_code_one_loop_start(db, parse, p_tab_list, None, wi, ii, not_ready);
        wi.i_continue = wi.a[ii].addr_cont;
    }

    // Pronto.
    wi.i_end_where = current_addr(parse);
    true
}

// ---------------------------------------------------------------------------------------------
// Chunk 016: sqlite3WhereEnd
// ---------------------------------------------------------------------------------------------

/// `sqlite3WhereEnd`: gera o fim do laço do WHERE. Ver os comentários de `where_begin` para mais
/// informação. `p_tab_list` é a lista passada a `where_begin` (o `pWInfo->pTabList` do C).
pub fn where_end(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab_list: &SrcList,
    wi: Box<WhereInfo>,
) {
    let mut wi = wi;
    let i_end = current_addr(parse);
    let mut n_rj: i32 = 0;

    // Gera o código de término dos laços.
    for i in (0..wi.n_level as usize).rev() {
        if wi.a[i].p_rj.is_some() {
            // Termina a sub-rotina que forma o interior do laço da tabela do RIGHT JOIN.
            let addr_cont = wi.a[i].addr_cont;
            resolve_label(parse, db, addr_cont);
            wi.a[i].addr_cont = 0;
            let end_subrtn = current_addr(parse);
            let (reg_return, addr_subrtn) = {
                let rj = wi.a[i].p_rj.as_mut().expect("pLevel->pRJ");
                rj.end_subrtn = end_subrtn;
                (rj.reg_return, rj.addr_subrtn)
            };
            add_op3(vdbe_of_parse(parse), OP_RETURN as i32, reg_return, addr_subrtn, 1);
            n_rj += 1;
        }
        let p_loop_id = wi.a[i].p_w_loop;
        let (ws_flags, p_loop_index, n_distinct_col) = {
            let l = wi.w_loop(p_loop_id);
            (l.ws_flags, l.btree.p_index.clone(), l.btree.n_distinct_col)
        };
        let (op, p1, p2, p3, p5) = {
            let l = &wi.a[i];
            (l.op, l.p1, l.p2, l.p3, l.p5)
        };
        if op != OP_NOOP {
            let mut addr_seek: i32 = 0;
            if wi.e_distinct as u32 == WHERE_DISTINCT_ORDERED
                && i == wi.n_level as usize - 1 // Ticket [ef9318757b152e3] 2017-10-21
                && (ws_flags & WHERE_INDEXED) != 0
                && n_distinct_col > 0
                && p_loop_index
                    .as_deref()
                    .map_or(false, |ix| ix.has_stat1 && ix.ai_row_log_est[n_distinct_col as usize] >= 36)
            {
                let n = n_distinct_col as i32;
                let i_idx_cur = wi.a[i].i_idx_cur;
                let r1 = parse.n_mem + 1;
                for j in 0..n {
                    add_op3(vdbe_of_parse(parse), OP_COLUMN as i32, i_idx_cur, j, r1 + j);
                }
                parse.n_mem += n + 1;
                let seek_op = if op == OP_PREV { OP_SEEKLT } else { OP_SEEKGT };
                addr_seek = add_op4_int(vdbe_of_parse(parse), seek_op as i32, i_idx_cur, 0, r1, n);
                add_op2(vdbe_of_parse(parse), OP_GOTO as i32, 1, p2);
            }
            // O caso comum: avança para a próxima linha.
            let addr_cont = wi.a[i].addr_cont;
            if addr_cont != 0 {
                resolve_label(parse, db, addr_cont);
            }
            add_op3(vdbe_of_parse(parse), op as i32, p1, p2, p3 as i32);
            change_p5(vdbe_of_parse(parse), p5 as u16);
            let reg_bignull = wi.a[i].reg_bignull;
            if reg_bignull != 0 {
                let addr_bignull = wi.a[i].addr_bignull;
                resolve_label(parse, db, addr_bignull);
                add_op2(vdbe_of_parse(parse), OP_DECRJUMPZERO as i32, reg_bignull, p2 - 1);
            }
            if addr_seek != 0 {
                jump_here(vdbe_of_parse(parse), addr_seek);
            }
        } else if wi.a[i].addr_cont != 0 {
            let addr_cont = wi.a[i].addr_cont;
            resolve_label(parse, db, addr_cont);
        }
        if (ws_flags & WHERE_IN_ABLE) != 0 && !wi.a[i].a_in_loop.is_empty() {
            let addr_nxt = wi.a[i].addr_nxt;
            resolve_label(parse, db, addr_nxt);
            let i_idx_cur = wi.a[i].i_idx_cur;
            let i_left_join = wi.a[i].i_left_join;
            for j in (0..wi.a[i].a_in_loop.len()).rev() {
                let p_in = wi.a[i].a_in_loop[j];
                jump_here(vdbe_of_parse(parse), p_in.addr_in_top + 1);
                if p_in.e_end_loop_op != OP_NOOP {
                    if p_in.n_prefix != 0 {
                        let b_early_out: i32 = ((ws_flags & WHERE_VIRTUALTABLE) == 0
                            && (ws_flags & WHERE_IN_EARLYOUT) != 0)
                            as i32;
                        if i_left_join != 0 {
                            // Em consultas LEFT JOIN, o cursor `p_in.i_cur` pode não ter sido
                            // aberto ainda. Isso ocorre em cláusulas WHERE como
                            // "a = ? AND b IN (...)", com o índice em (a, b). Se o RHS de (a=?) é
                            // NULL, o "b IN (...)" pode nunca ter sido codificado, mas o corpo do
                            // laço roda para devolver a linha nula. Então, se o cursor ainda não
                            // está aberto, salta o OP_Next ou OP_Prev prestes a ser codificado.
                            let cur = current_addr(parse);
                            add_op2(
                                vdbe_of_parse(parse),
                                OP_IFNOTOPEN as i32,
                                p_in.i_cur,
                                cur + 2 + b_early_out,
                            );
                        }
                        if b_early_out != 0 {
                            let cur = current_addr(parse);
                            add_op4_int(
                                vdbe_of_parse(parse),
                                OP_IFNOHOPE as i32,
                                i_idx_cur,
                                cur + 2,
                                p_in.i_base,
                                p_in.n_prefix,
                            );
                            // Reaponta o OP_IsNull contra o operando esquerdo do IN para saltar
                            // além do OP_IfNoHope: o OP_IsNull também pula o OP_Affinity exigido
                            // pelo OP_IfNoHope.
                            jump_here(vdbe_of_parse(parse), p_in.addr_in_top + 1);
                        }
                    }
                    add_op2(vdbe_of_parse(parse), p_in.e_end_loop_op as i32, p_in.i_cur, p_in.addr_in_top);
                }
                jump_here(vdbe_of_parse(parse), p_in.addr_in_top - 1);
            }
        }
        let addr_brk = wi.a[i].addr_brk;
        resolve_label(parse, db, addr_brk);
        if let Some(reg_return) = wi.a[i].p_rj.as_ref().map(|rj| rj.reg_return) {
            add_op3(vdbe_of_parse(parse), OP_RETURN as i32, reg_return, 0, 1);
        }
        let addr_skip = wi.a[i].addr_skip;
        if addr_skip != 0 {
            vdbe_goto(vdbe_of_parse(parse), addr_skip);
            let name = p_loop_index.as_deref().map_or_else(Vec::new, |ix| ix.z_name.clone());
            vdbe_comment(vdbe_of_parse(parse), b"next skip-scan on %s", &[text_arg(&name)]);
            jump_here(vdbe_of_parse(parse), addr_skip);
            jump_here(vdbe_of_parse(parse), addr_skip - 2);
        }
        let addr_like_rep = wi.a[i].addr_like_rep;
        if addr_like_rep != 0 {
            let cntr = (wi.a[i].i_like_rep_cntr >> 1) as i32;
            add_op2(vdbe_of_parse(parse), OP_DECRJUMPZERO as i32, cntr, addr_like_rep);
        }
        let i_left_join = wi.a[i].i_left_join;
        if i_left_join != 0 {
            let ws = ws_flags;
            let addr = add_op1(vdbe_of_parse(parse), OP_IFPOS as i32, i_left_join);
            debug_assert!((ws & WHERE_IDX_ONLY) == 0 || (ws & WHERE_INDEXED) != 0);
            let i_from = wi.a[i].i_from as usize;
            let i_tab_cur = wi.a[i].i_tab_cur;
            let i_idx_cur = wi.a[i].i_idx_cur;
            if (ws & WHERE_IDX_ONLY) == 0 {
                let p_src = &p_tab_list.a[i_from];
                debug_assert!(i_tab_cur == p_src.i_cursor);
                if p_src.fg.via_coroutine {
                    let n = p_src.reg_result;
                    debug_assert!(p_src.p_tab.is_some());
                    let m = p_src.p_tab.as_ref().map_or(0, |t| t.n_col as i32);
                    add_op3(vdbe_of_parse(parse), OP_NULL as i32, 0, n, n + m - 1);
                }
                add_op1(vdbe_of_parse(parse), OP_NULLROW as i32, i_tab_cur);
            }
            if (ws & WHERE_INDEXED) != 0
                || ((ws & WHERE_MULTI_OR) != 0 && wi.a[i].p_covering_idx.is_some())
            {
                if (ws & WHERE_MULTI_OR) != 0 {
                    if let Some(p_ix) = wi.a[i].p_covering_idx.clone() {
                        let i_db = p_tab_list.a[i_from]
                            .p_tab
                            .as_ref()
                            .map_or(-1, |t| schema_to_index(db, t.p_schema));
                        add_op3(
                            vdbe_of_parse(parse),
                            OP_REOPENIDX as i32,
                            i_idx_cur,
                            p_ix.tnum as i32,
                            i_db,
                        );
                        set_p4_key_info(parse, db, &p_ix);
                    }
                }
                add_op1(vdbe_of_parse(parse), OP_NULLROW as i32, i_idx_cur);
            }
            let (l_op, l_p1) = (wi.a[i].op, wi.a[i].p1);
            let addr_first = wi.a[i].addr_first;
            if l_op == OP_RETURN {
                add_op2(vdbe_of_parse(parse), OP_GOSUB as i32, l_p1, addr_first);
            } else {
                vdbe_goto(vdbe_of_parse(parse), addr_first);
            }
            jump_here(vdbe_of_parse(parse), addr);
        }
    }

    debug_assert!(wi.n_level as usize <= p_tab_list.a.len());
    for i in 0..wi.n_level as usize {
        let i_from = wi.a[i].i_from as usize;
        let p_tab_item = &p_tab_list.a[i_from];
        let Some(p_tab) = p_tab_item.p_tab.clone() else {
            continue;
        };
        let p_loop_id = wi.a[i].p_w_loop;
        let (ws_flags, loop_index) = {
            let l = wi.w_loop(p_loop_id);
            (l.ws_flags, l.btree.p_index.clone())
        };

        // Faz o processamento do RIGHT JOIN. Gera código que devolve as linhas não casadas do
        // operando direito do RIGHT JOIN com todas as colunas do operando esquerdo em NULL.
        if wi.a[i].p_rj.is_some() {
            where_right_join_loop(db, parse, p_tab_list, &mut wi, i);
            continue;
        }

        // Para uma co-rotina, troca todas as referências OP_Column à tabela da co-rotina por
        // OP_Copy do resultado contido num registrador. OP_Rowid vira OP_Null.
        if p_tab_item.fg.via_coroutine {
            debug_assert!(p_tab_item.reg_result >= 0);
            translate_column_to_copy(
                &*db,
                parse,
                wi.a[i].addr_body,
                wi.a[i].i_tab_cur,
                p_tab_item.reg_result,
                0,
            );
            continue;
        }

        // Se esta varredura usa um índice, faz substituições no código do VDBE para ler dados do
        // índice e não da tabela, quando possível. Em alguns casos isso evita ler a tabela, o que
        // pode melhorar muito o desempenho.
        //
        // As chamadas ao gerador de código entre `where_begin` e `where_end` criaram código que
        // referencia a tabela diretamente. Este laço varre todo esse código atrás de opcodes que
        // referenciam a tabela e os converte em opcodes que referenciam o índice.
        let p_idx: Option<Rc<Index>> = if (ws_flags & (WHERE_INDEXED | WHERE_IDX_ONLY)) != 0 {
            loop_index
        } else if (ws_flags & WHERE_MULTI_OR) != 0 {
            wi.a[i].p_covering_idx.clone()
        } else {
            None
        };
        if let Some(p_idx) = p_idx {
            let last: i32 = if wi.e_one_pass as i32 == ONEPASS_OFF || !p_tab.has_rowid() {
                i_end
            } else {
                wi.i_end_where
            };
            let i_idx_cur = wi.a[i].i_idx_cur;
            let i_tab_cur = wi.a[i].i_tab_cur;
            if p_idx.b_has_expr {
                for p in parse.p_idx_epr.iter_mut() {
                    if p.i_idx_cur == i_idx_cur {
                        p.i_data_cur = -1;
                        p.i_idx_cur = -1;
                    }
                }
            }
            let mut k: i32 = wi.a[i].addr_body + 1;
            let mut n_internal_err = 0;
            {
                let v = vdbe_of_parse(parse);
                let mut idx = k;
                loop {
                    if idx < 0 || idx as usize >= v.a_op.len() {
                        break;
                    }
                    let p_op = &mut v.a_op[idx as usize];
                    if p_op.p1 != i_tab_cur {
                        // no-op
                    } else if p_op.opcode == OP_COLUMN {
                        let mut x: i16 = p_op.p2 as i16;
                        if !p_tab.has_rowid() {
                            let p_pk = primary_key_index(&p_tab).expect("sqlite3PrimaryKeyIndex");
                            x = p_pk.ai_column[x as usize];
                            debug_assert!(x >= 0);
                        } else {
                            x = storage_column_to_table(&p_tab, x);
                        }
                        x = table_column_to_index(&p_idx, x);
                        if x >= 0 {
                            p_op.p2 = x as i32;
                            p_op.p1 = i_idx_cur;
                        } else {
                            // Não foi possível traduzir a referência à tabela numa referência ao
                            // índice. Aqui se verifica que isso é inofensivo: que a tabela
                            // referenciada está mesmo aberta.
                            if (ws_flags & WHERE_IDX_ONLY) != 0 {
                                n_internal_err += 1;
                            }
                        }
                    } else if p_op.opcode == OP_ROWID {
                        p_op.p1 = i_idx_cur;
                        p_op.opcode = OP_IDXROWID;
                    } else if p_op.opcode == OP_IFNULLROW {
                        p_op.p1 = i_idx_cur;
                    }
                    idx += 1;
                    k = idx;
                    if idx >= last {
                        break;
                    }
                }
            }
            let _ = k;
            for _ in 0..n_internal_err {
                error_msg(db, parse, b"internal query planner error", &[]);
                parse.rc = SQLITE_INTERNAL;
            }
        }
    }

    // O ponto "break" fica aqui, logo depois do fim do laço externo. Resolve-o.
    let i_break = wi.i_break;
    resolve_label(parse, db, i_break);

    // Limpeza final.
    parse.n_query_loop = wi.saved_n_query_loop as LogEst;
    where_info_free(&mut wi);
    parse.within_rj_subrtn = parse.within_rj_subrtn.wrapping_sub(n_rj as u8);
}
