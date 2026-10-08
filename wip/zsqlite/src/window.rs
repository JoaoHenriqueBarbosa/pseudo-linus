//! `window.c`: as funções de janela do SQLite 3.46.1 (`row_number`, `rank`, `ntile`, `lead`, ...),
//! a reescrita de um SELECT com funções de janela (`sqlite3WindowRewrite`) e a geração de código
//! do laço de janelas (`sqlite3WindowCodeInit`, `sqlite3WindowCodeStep`), no modelo v2 (ver
//! `CONVENTIONS.md`).
//!
//! Desvios do C, todos decorrentes do modelo v2:
//!
//! * **A janela mora na expressão.** O C liga as janelas de um select numa lista (`Select.pWin`,
//!   por `Window.pNextWin`/`ppThis`) e cada janela aponta de volta para a expressão dona
//!   (`Window.pOwner`). Aqui a posse é da expressão (`ExprY::Win`), `pOwner`, `ppThis` e o
//!   `pNextWin` da lista do select não existem, e a lista do select é reconstruída por
//!   [`linked_windows`]: as expressões de função de janela do resultado e do ORDER BY cuja
//!   `Window.link_seq` é diferente de zero, da cabeça para a cauda. O `pNextWin` que sobra no
//!   `Window` só encadeia as definições da cláusula WINDOW durante o parse (`windowdefn_list`).
//! * **`link_seq`.** `sqlite3WindowLink` do C decide na hora se a janela entra na lista comparando
//!   com a cabeça. [`window_link`] só recebe o contador do select, então apenas numera a janela
//!   (valor provisório: 1, 2, ... na ordem de ligação). A decisão do C (lista vazia ou janela
//!   igual à cabeça entra, e vira a nova cabeça; a diferente fica de fora e liga `SF_MultiPart`)
//!   é tomada por [`window_finish_link`], que o chamador roda uma vez, depois que todas as
//!   janelas do select foram numeradas. Depois dela `link_seq` vale `LINK_FINAL | posição`, com a
//!   cabeça na posição 1 (a janela de menor `link_seq` é a cabeça, como `first_window` de
//!   `select2.rs` supõe). [`window_rewrite`] a executa de novo por segurança (é idempotente).
//! * **Identidade das funções embutidas.** O C compara o ponteiro `zName` com as constantes
//!   estáticas (`pFunc->zName==nth_valueName`). Aqui é a comparação do nome de uma função com
//!   `SQLITE_FUNC_BUILTIN`, que uma função do usuário nunca tem.
//! * **`pTab` da reescrita.** O C aloca uma `Table` zerada, deixa as colunas reescritas apontando
//!   para ela e a preenche com `memcpy` depois. Aqui as colunas apontam para uma `Rc<Table>`
//!   provisória e, quando a tabela definitiva existe, [`retarget_table`] troca todas as
//!   referências.
//! * A geração de código opera sobre uma fotografia das janelas (`WindowCodeArg.wins`: a janela
//!   mais a lista de argumentos da expressão dona), porque as rotinas auxiliares precisam das
//!   janelas e do `Parse` ao mesmo tempo. `sqlite3WindowCodeStep` não altera nenhuma janela, então
//!   a fotografia é indistinguível.
//! * Sem `SQLITE_DEBUG`, `VdbeCoverage*`, `TREETRACE` e `VdbeModuleComment`. As falhas de alocação
//!   (`mallocFailed`) que o C trata não existem, exceto onde ele consulta `db.malloc_failed`.
//!   `sqlite3WindowDelete` e `sqlite3WindowListDelete` não existem: soltar o valor basta.

use std::rc::Rc;

use crate::build3::{may_abort, src_list_append, src_list_assign_cursors};
use crate::callback::insert_builtin_funcs;
use crate::connection::{Connection, Context, FinalFn, FuncDef, Parse, ScalarFn, UserData};
use crate::consts::{
    EP_COLLATE, EP_DISTINCT, EP_FULL_SIZE, EP_INT_VALUE, EP_IS_FALSE, EP_IS_TRUE, EP_SKIP,
    EP_UNLIKELY, EP_WIN_FUNC, KEYINFO_ORDER_BIGNULL, KEYINFO_ORDER_DESC, OE_ABORT, OP_ADD,
    OP_ADDIMM, OP_AGGFINAL, OP_AGGINVERSE, OP_AGGSTEP, OP_AGGVALUE, OP_COLLSEQ, OP_COLUMN,
    OP_COMPARE, OP_COPY, OP_DELETE, OP_EQ, OP_GE, OP_GOSUB, OP_GOTO, OP_GT, OP_HALT, OP_IDXINSERT,
    OP_IFNOT, OP_IFPOS, OP_INSERT, OP_INTEGER, OP_ISNULL, OP_JUMP, OP_LAST, OP_LE, OP_LT,
    OP_MAKERECORD, OP_MUSTBEINT, OP_NE, OP_NEWROWID, OP_NEXT, OP_NOTNULL, OP_NULL, OP_OPENDUP,
    OP_OPENEPHEMERAL, OP_RESETSORTER, OP_RETURN, OP_REWIND, OP_ROWID, OP_SCOPY, OP_SEEKGE,
    OP_SEEKROWID, OP_STRING8, OP_SUBTRACT, OPFLAG_SAVEPOSITION, SF_AGGREGATE, SF_EXPANDED,
    SF_MULTIPART, SF_ORDERBYREQD, SF_WINREWRITE, SQLITE_AFF_NONE, SQLITE_AFF_NUMERIC,
    SQLITE_ERROR, SQLITE_FLOAT, SQLITE_FUNC_BUILTIN, SQLITE_FUNC_MINMAX, SQLITE_FUNC_NEEDCOLL,
    SQLITE_FUNC_WINDOW, SQLITE_INTEGER, SQLITE_JUMPIFNULL, SQLITE_NOMEM, SQLITE_NULLEQ, SQLITE_OK,
    SQLITE_SUBTYPE, SQLITE_UTF8, SQLITE_WINDOW_FUNC, TF_EPHEMERAL, TK_AGG_FUNCTION, TK_COLLATE,
    TK_COLUMN, TK_CURRENT, TK_FILTER, TK_FOLLOWING, TK_FUNCTION, TK_GROUP, TK_GROUPS,
    TK_IF_NULL_ROW, TK_INTEGER, TK_NO, TK_NULL, TK_PRECEDING, TK_RANGE, TK_ROWS, TK_TIES,
    TK_UNBOUNDED, WRC_ABORT, WRC_CONTINUE, WRC_PRUNE,
};
use crate::expr::{
    expr, expr_alloc, expr_dup, expr_list_append, expr_list_dup, expr_nn_coll_seq,
    expr_unmap_and_delete,
};
use crate::expr_code::{expr_is_constant, expr_is_integer};
use crate::expr_code2::{
    agg_info_persist_walker_init, expr_code, expr_code_expr_list, expr_compare, expr_list_compare,
    get_temp_range, get_temp_reg, release_temp_range, release_temp_reg,
};
use crate::mem::{value_from_expr, value_numeric_type, Mem};
use crate::printf::PrintfArg;
use crate::select::{
    current_addr, get_vdbe, key_info_from_expr_list, result_set_of_select, select_new,
    vdbe_of_parse,
};
use crate::sqlite_int::{
    Expr, ExprList, ExprU, ExprX, ExprY, Select, TabRef, Table, Token, Walker, Window,
};
use crate::util::{error_msg, str_icmp};
use crate::vdbe_types::P4;
use crate::vdbeapi::{
    aggregate_context, result_double, result_error, result_error_nomem, result_int64,
    result_value, value_double, value_dup, value_free, value_int, value_int64,
};
use crate::vdbeaux::{
    add_op0, add_op1, add_op2, add_op3, add_op4, add_op4_int, append_p4, change_p1, change_p5,
    get_op, jump_here, make_label, resolve_label, vdbe_comment,
};
use crate::walker::{
    select_walk_noop, walk_expr_list, walk_select, walker_depth_decrease, walker_depth_increase,
};
use crate::where_::{where_end, WhereInfo};

/// `vdbe_of_parse(parse)`: o VDBE em preparo (o gerador de código sempre o tem criado).
macro_rules! vd {
    ($parse:expr) => {
        vdbe_of_parse($parse)
    };
}

// ---------------------------------------------------------------------------------------------
// chunk 000 e 001: as funções de janela embutidas
// ---------------------------------------------------------------------------------------------

/// `row_numberStepFunc`: `row_number()`. Supõe o quadro `ROWS BETWEEN UNBOUNDED PRECEDING AND
/// CURRENT ROW`.
fn row_number_step_func(ctx: &mut Context<'_>, _argv: &[Mem]) {
    if let Some(p) = aggregate_context::<i64>(ctx, true) {
        *p += 1;
    }
}

/// `row_numberValueFunc`.
fn row_number_value_func(ctx: &mut Context<'_>) {
    let n = aggregate_context::<i64>(ctx, true).map_or(0, |p| *p);
    result_int64(ctx, n);
}

/// `struct CallCount`: o contexto de `rank()`, `dense_rank()`, `percent_rank()` e `cume_dist()`.
#[derive(Default)]
struct CallCount {
    n_value: i64,
    n_step: i64,
    n_total: i64,
}

/// `dense_rankStepFunc`. Supõe o quadro `RANGE BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW`.
fn dense_rank_step_func(ctx: &mut Context<'_>, _argv: &[Mem]) {
    if let Some(p) = aggregate_context::<CallCount>(ctx, true) {
        p.n_step = 1;
    }
}

/// `dense_rankValueFunc`.
fn dense_rank_value_func(ctx: &mut Context<'_>) {
    let r = aggregate_context::<CallCount>(ctx, true).map(|p| {
        if p.n_step != 0 {
            p.n_value += 1;
            p.n_step = 0;
        }
        p.n_value
    });
    if let Some(v) = r {
        result_int64(ctx, v);
    }
}

/// `struct NthValueCtx`: o contexto de `nth_value()` e `first_value()`.
#[derive(Default)]
struct NthValueCtx {
    n_step: i64,
    p_value: Option<Mem>,
}

/// `nth_valueStepFunc`: usada só no "modo lento" (a cláusula EXCLUDE não é `NO OTHERS`).
fn nth_value_step_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    if aggregate_context::<NthValueCtx>(ctx, true).is_none() {
        return;
    }
    let mut a1 = argv[1].clone();
    let i_val = match value_numeric_type(&mut a1) {
        SQLITE_INTEGER => Some(value_int64(&a1)),
        SQLITE_FLOAT => {
            let f_val = value_double(&a1);
            if (f_val as i64) as f64 != f_val {
                None
            } else {
                Some(f_val as i64)
            }
        }
        _ => None,
    };
    match i_val {
        Some(i_val) if i_val > 0 => {
            let mut failed = false;
            if let Some(p) = aggregate_context::<NthValueCtx>(ctx, true) {
                p.n_step += 1;
                if i_val == p.n_step {
                    p.p_value = value_dup(&argv[0]);
                    failed = p.p_value.is_none();
                }
            }
            if failed {
                result_error_nomem(ctx);
            }
        }
        _ => result_error(ctx, b"second argument to nth_value must be a positive integer", -1),
    }
}

/// `nth_valueFinalizeFunc`.
fn nth_value_finalize_func(ctx: &mut Context<'_>) {
    let v = aggregate_context::<NthValueCtx>(ctx, false).and_then(|p| p.p_value.take());
    if let Some(v) = v {
        result_value(ctx, &v);
        value_free(v);
    }
}

/// `first_valueStepFunc`.
fn first_value_step_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    let mut failed = false;
    if let Some(p) = aggregate_context::<NthValueCtx>(ctx, true) {
        if p.p_value.is_none() {
            p.p_value = value_dup(&argv[0]);
            failed = p.p_value.is_none();
        }
    }
    if failed {
        result_error_nomem(ctx);
    }
}

/// `first_valueFinalizeFunc`.
fn first_value_finalize_func(ctx: &mut Context<'_>) {
    let v = aggregate_context::<NthValueCtx>(ctx, true).and_then(|p| p.p_value.take());
    if let Some(v) = v {
        result_value(ctx, &v);
        value_free(v);
    }
}

/// `rankStepFunc`. Supõe o quadro `RANGE BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW`.
fn rank_step_func(ctx: &mut Context<'_>, _argv: &[Mem]) {
    if let Some(p) = aggregate_context::<CallCount>(ctx, true) {
        p.n_step += 1;
        if p.n_value == 0 {
            p.n_value = p.n_step;
        }
    }
}

/// `rankValueFunc`.
fn rank_value_func(ctx: &mut Context<'_>) {
    let r = aggregate_context::<CallCount>(ctx, true).map(|p| {
        let v = p.n_value;
        p.n_value = 0;
        v
    });
    if let Some(v) = r {
        result_int64(ctx, v);
    }
}

/// `percent_rankStepFunc`. Supõe o quadro `GROUPS BETWEEN CURRENT ROW AND UNBOUNDED FOLLOWING`.
fn percent_rank_step_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    debug_assert!(argv.is_empty());
    if let Some(p) = aggregate_context::<CallCount>(ctx, true) {
        p.n_total += 1;
    }
}

/// `percent_rankInvFunc`.
fn percent_rank_inv_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    debug_assert!(argv.is_empty());
    if let Some(p) = aggregate_context::<CallCount>(ctx, true) {
        p.n_step += 1;
    }
}

/// `percent_rankValueFunc` (também o finalizador).
fn percent_rank_value_func(ctx: &mut Context<'_>) {
    let r = aggregate_context::<CallCount>(ctx, true).map(|p| {
        p.n_value = p.n_step;
        if p.n_total > 1 {
            p.n_value as f64 / (p.n_total - 1) as f64
        } else {
            0.0
        }
    });
    if let Some(r) = r {
        result_double(ctx, r);
    }
}

/// `cume_distStepFunc`. Supõe o quadro `GROUPS BETWEEN 1 FOLLOWING AND UNBOUNDED FOLLOWING`.
fn cume_dist_step_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    debug_assert!(argv.is_empty());
    if let Some(p) = aggregate_context::<CallCount>(ctx, true) {
        p.n_total += 1;
    }
}

/// `cume_distInvFunc`.
fn cume_dist_inv_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    debug_assert!(argv.is_empty());
    if let Some(p) = aggregate_context::<CallCount>(ctx, true) {
        p.n_step += 1;
    }
}

/// `cume_distValueFunc` (também o finalizador).
fn cume_dist_value_func(ctx: &mut Context<'_>) {
    let r = aggregate_context::<CallCount>(ctx, false)
        .map(|p| p.n_step as f64 / p.n_total as f64);
    if let Some(r) = r {
        result_double(ctx, r);
    }
}

/// `struct NtileCtx`: o contexto de `ntile()`.
#[derive(Default)]
struct NtileCtx {
    /// Total de linhas da partição.
    n_total: i64,
    /// O parâmetro de `ntile(N)`.
    n_param: i64,
    /// A linha corrente.
    i_row: i64,
}

/// `ntileStepFunc`. Supõe o quadro `ROWS CURRENT ROW AND UNBOUNDED FOLLOWING`.
fn ntile_step_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    debug_assert!(argv.len() == 1);
    let mut bad = false;
    if let Some(p) = aggregate_context::<NtileCtx>(ctx, true) {
        if p.n_total == 0 {
            p.n_param = value_int64(&argv[0]);
            if p.n_param <= 0 {
                bad = true;
            }
        }
        p.n_total += 1;
    }
    if bad {
        result_error(ctx, b"argument of ntile must be a positive integer", -1);
    }
}

/// `ntileInvFunc`.
fn ntile_inv_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    debug_assert!(argv.len() == 1);
    if let Some(p) = aggregate_context::<NtileCtx>(ctx, true) {
        p.i_row += 1;
    }
}

/// `ntileValueFunc` (também o finalizador).
fn ntile_value_func(ctx: &mut Context<'_>) {
    let r = aggregate_context::<NtileCtx>(ctx, true).and_then(|p| {
        if p.n_param <= 0 {
            return None;
        }
        let n_size = (p.n_total / p.n_param) as i32 as i64;
        if n_size == 0 {
            Some(p.i_row + 1)
        } else {
            let n_large = p.n_total - p.n_param * n_size;
            let i_small = n_large * (n_size + 1);
            let i_row = p.i_row;
            debug_assert!(n_large * (n_size + 1) + (p.n_param - n_large) * n_size == p.n_total);
            if i_row < i_small {
                Some(1 + i_row / (n_size + 1))
            } else {
                Some(1 + n_large + (i_row - i_small) / n_size)
            }
        }
    });
    if let Some(r) = r {
        result_int64(ctx, r);
    }
}

/// `struct LastValueCtx`: o contexto de `last_value()`.
#[derive(Default)]
struct LastValueCtx {
    p_val: Option<Mem>,
    n_val: i32,
}

/// `last_valueStepFunc`.
fn last_value_step_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    let mut failed = false;
    if let Some(p) = aggregate_context::<LastValueCtx>(ctx, true) {
        if let Some(old) = p.p_val.take() {
            value_free(old);
        }
        p.p_val = value_dup(&argv[0]);
        if p.p_val.is_none() {
            failed = true;
        } else {
            p.n_val += 1;
        }
    }
    if failed {
        result_error_nomem(ctx);
    }
}

/// `last_valueInvFunc`.
fn last_value_inv_func(ctx: &mut Context<'_>, _argv: &[Mem]) {
    if let Some(p) = aggregate_context::<LastValueCtx>(ctx, true) {
        p.n_val -= 1;
        if p.n_val == 0 {
            if let Some(old) = p.p_val.take() {
                value_free(old);
            }
        }
    }
}

/// `last_valueValueFunc`.
fn last_value_value_func(ctx: &mut Context<'_>) {
    let v = aggregate_context::<LastValueCtx>(ctx, false).and_then(|p| p.p_val.clone());
    if let Some(v) = v {
        result_value(ctx, &v);
    }
}

/// `last_valueFinalizeFunc`.
fn last_value_finalize_func(ctx: &mut Context<'_>) {
    let v = aggregate_context::<LastValueCtx>(ctx, true).and_then(|p| p.p_val.take());
    if let Some(v) = v {
        result_value(ctx, &v);
        value_free(v);
    }
}

/// `noopStepFunc`: espaço reservado para o xStep e o xInverse das funções de janela que nunca os
/// chamam (implementadas por bytecode). Nunca é invocada.
fn noop_step_func(_ctx: &mut Context<'_>, _argv: &[Mem]) {
    debug_assert!(false);
}

/// `noopValueFunc`: é chamada, mas não faz nada.
fn noop_value_func(_ctx: &mut Context<'_>) {}

/// Os nomes estáticos das funções de janela embutidas (`row_numberName` e companhia).
const ROW_NUMBER_NAME: &[u8] = b"row_number";
const DENSE_RANK_NAME: &[u8] = b"dense_rank";
const RANK_NAME: &[u8] = b"rank";
const PERCENT_RANK_NAME: &[u8] = b"percent_rank";
const CUME_DIST_NAME: &[u8] = b"cume_dist";
const NTILE_NAME: &[u8] = b"ntile";
const LAST_VALUE_NAME: &[u8] = b"last_value";
const NTH_VALUE_NAME: &[u8] = b"nth_value";
const FIRST_VALUE_NAME: &[u8] = b"first_value";
const LEAD_NAME: &[u8] = b"lead";
const LAG_NAME: &[u8] = b"lag";

/// `pFunc->zName==xxxName`: a função é a embutida de nome `name`.
fn is_builtin_named(p_func: &FuncDef, name: &[u8]) -> bool {
    (p_func.func_flags & SQLITE_FUNC_BUILTIN) != 0 && p_func.z_name == name
}

/// `pFunc->xSFunc==noopStepFunc`.
fn is_noop_step(p_func: &FuncDef) -> bool {
    p_func.x_s_func.map_or(false, |f| f as usize == noop_step_func as ScalarFn as usize)
}

/// As macros `WINDOWFUNCALL`, `WINDOWFUNCNOOP` e `WINDOWFUNCX`: monta o `FuncDef` de uma função de
/// janela embutida.
fn window_def(
    z_name: &[u8],
    n_arg: i8,
    step: ScalarFn,
    finalize: FinalFn,
    value: FinalFn,
    inverse: ScalarFn,
) -> FuncDef {
    FuncDef {
        n_arg,
        func_flags: SQLITE_FUNC_BUILTIN | SQLITE_UTF8 as u32 | SQLITE_FUNC_WINDOW,
        p_user_data: UserData::None,
        x_s_func: Some(step),
        x_finalize: Some(finalize),
        x_value: Some(value),
        x_inverse: Some(inverse),
        z_name: z_name.to_vec(),
        p_destructor: None,
    }
}

/// `sqlite3WindowFunctions`: registra as funções de janela embutidas que não são também
/// agregadas.
pub fn window_functions() {
    let defs = vec![
        // WINDOWFUNCX: o mesmo xValue serve de xFinalize, e o xInverse nunca é chamado.
        window_def(ROW_NUMBER_NAME, 0, row_number_step_func, row_number_value_func, row_number_value_func, noop_step_func),
        window_def(DENSE_RANK_NAME, 0, dense_rank_step_func, dense_rank_value_func, dense_rank_value_func, noop_step_func),
        window_def(RANK_NAME, 0, rank_step_func, rank_value_func, rank_value_func, noop_step_func),
        // WINDOWFUNCALL: usam as quatro interfaces.
        window_def(PERCENT_RANK_NAME, 0, percent_rank_step_func, percent_rank_value_func, percent_rank_value_func, percent_rank_inv_func),
        window_def(CUME_DIST_NAME, 0, cume_dist_step_func, cume_dist_value_func, cume_dist_value_func, cume_dist_inv_func),
        window_def(NTILE_NAME, 1, ntile_step_func, ntile_value_func, ntile_value_func, ntile_inv_func),
        window_def(LAST_VALUE_NAME, 1, last_value_step_func, last_value_finalize_func, last_value_value_func, last_value_inv_func),
        window_def(NTH_VALUE_NAME, 2, nth_value_step_func, nth_value_finalize_func, noop_value_func, noop_step_func),
        window_def(FIRST_VALUE_NAME, 1, first_value_step_func, first_value_finalize_func, noop_value_func, noop_step_func),
        // WINDOWFUNCNOOP: implementadas por bytecode.
        window_def(LEAD_NAME, 1, noop_step_func, noop_value_func, noop_value_func, noop_step_func),
        window_def(LEAD_NAME, 2, noop_step_func, noop_value_func, noop_value_func, noop_step_func),
        window_def(LEAD_NAME, 3, noop_step_func, noop_value_func, noop_value_func, noop_step_func),
        window_def(LAG_NAME, 1, noop_step_func, noop_value_func, noop_value_func, noop_step_func),
        window_def(LAG_NAME, 2, noop_step_func, noop_value_func, noop_value_func, noop_step_func),
        window_def(LAG_NAME, 3, noop_step_func, noop_value_func, noop_value_func, noop_step_func),
    ];
    insert_builtin_funcs(defs);
}

// ---------------------------------------------------------------------------------------------
// chunk 001 (cont.) e 003: a definição das janelas
// ---------------------------------------------------------------------------------------------

/// `windowFind`: procura entre `candidates` (a lista de definições da cláusula WINDOW, da cabeça
/// para a cauda) a janela de nome `z_name`; deixa um erro no `parse` se não existe.
fn window_find<'a>(
    db: &mut Connection,
    parse: &mut Parse,
    mut candidates: impl Iterator<Item = &'a Window>,
    z_name: &[u8],
) -> Option<&'a Window> {
    let found = candidates
        .find(|p| p.z_name.as_deref().map_or(false, |n| str_icmp(n, z_name) == 0));
    if found.is_none() {
        error_msg(db, parse, b"no such window: %s", &[PrintfArg::Text(Some(z_name.to_vec()))]);
    }
    found
}

/// `sqlite3WindowUpdate`: chamada logo depois de resolver o nome de uma função de janela de um
/// SELECT. `p_list` são as definições da cláusula WINDOW do SELECT, `p_win` a janela do OVER e
/// `p_func` a função resolvida. Atualiza `p_win`: copia a definição se o OVER citou uma janela
/// nomeada (ou deixa o erro no `parse`), e coage o quadro nas funções embutidas que o exigem (ver
/// "BUILT-IN WINDOW FUNCTIONS" no topo do `window.c`).
pub fn window_update(
    parse: &mut Parse,
    db: &mut Connection,
    p_list: &[Window],
    p_win: &mut Window,
    p_func: &Rc<FuncDef>,
) {
    if p_win.z_name.is_some() && p_win.e_frm_type == 0 {
        let z_name = p_win.z_name.clone().unwrap_or_default();
        let Some(p) = window_find(db, parse, p_list.iter(), &z_name) else {
            return;
        };
        p_win.p_partition = expr_list_dup(p.p_partition.as_deref(), 0);
        p_win.p_order_by = expr_list_dup(p.p_order_by.as_deref(), 0);
        p_win.p_start = expr_dup(p.p_start.as_deref(), 0);
        p_win.p_end = expr_dup(p.p_end.as_deref(), 0);
        p_win.e_start = p.e_start;
        p_win.e_end = p.e_end;
        p_win.e_frm_type = p.e_frm_type;
        p_win.e_exclude = p.e_exclude;
    } else {
        window_chain_in(db, parse, p_win, p_list.iter());
    }
    if p_win.e_frm_type == TK_RANGE
        && (p_win.p_start.is_some() || p_win.p_end.is_some())
        && p_win.p_order_by.as_deref().map_or(true, |l| l.a.len() != 1)
    {
        error_msg(
            db,
            parse,
            b"RANGE with offset PRECEDING/FOLLOWING requires one ORDER BY expression",
            &[],
        );
    } else if (p_func.func_flags & SQLITE_FUNC_WINDOW) != 0 {
        if p_win.p_filter.is_some() {
            error_msg(
                db,
                parse,
                b"FILTER clause may only be used with aggregate window functions",
                &[],
            );
        } else {
            // O quadro que cada função embutida exige: (nome, tipo, início, fim).
            let a_up: [(&[u8], u8, u8, u8); 8] = [
                (ROW_NUMBER_NAME, TK_ROWS, TK_UNBOUNDED, TK_CURRENT),
                (DENSE_RANK_NAME, TK_RANGE, TK_UNBOUNDED, TK_CURRENT),
                (RANK_NAME, TK_RANGE, TK_UNBOUNDED, TK_CURRENT),
                (PERCENT_RANK_NAME, TK_GROUPS, TK_CURRENT, TK_UNBOUNDED),
                (CUME_DIST_NAME, TK_GROUPS, TK_FOLLOWING, TK_UNBOUNDED),
                (NTILE_NAME, TK_ROWS, TK_CURRENT, TK_UNBOUNDED),
                (LEAD_NAME, TK_ROWS, TK_UNBOUNDED, TK_UNBOUNDED),
                (LAG_NAME, TK_ROWS, TK_UNBOUNDED, TK_CURRENT),
            ];
            for (z_func, e_frm_type, e_start, e_end) in a_up {
                if is_builtin_named(p_func, z_func) {
                    p_win.p_end = None;
                    p_win.p_start = None;
                    p_win.e_frm_type = e_frm_type;
                    p_win.e_start = e_start;
                    p_win.e_end = e_end;
                    p_win.e_exclude = 0;
                    if p_win.e_start == TK_FOLLOWING {
                        p_win.p_start = expr(TK_INTEGER as i32, Some(b"1"));
                    }
                    break;
                }
            }
        }
    }
    p_win.p_w_func = Some(p_func.clone());
}

/// `sqlite3WindowOffsetExpr`: a expressão é um deslocamento PRECEDING ou FOLLOWING, que deve ser
/// um inteiro não negativo. Se não é constante vira NULL (o erro sai depois), para nunca deixar um
/// valor variável na árvore.
fn window_offset_expr(parse: &mut Parse, p_expr: Option<Box<Expr>>) -> Option<Box<Expr>> {
    let mut p_expr = p_expr;
    let is_const = match p_expr.as_deref_mut() {
        Some(e) => expr_is_constant(None, Some(e)) != 0,
        None => true,
    };
    if is_const {
        return p_expr;
    }
    expr_unmap_and_delete(parse, p_expr);
    expr_alloc(TK_NULL as i32, None, 0)
}

/// `sqlite3WindowAlloc`: aloca uma janela com a definição de um quadro. `e_type` é `TK_RANGE`,
/// `TK_ROWS`, `TK_GROUPS` ou 0.
pub fn window_alloc(
    db: &mut Connection,
    parse: &mut Parse,
    e_type: i32,
    e_start: i32,
    p_start: Option<Box<Expr>>,
    e_end: i32,
    p_end: Option<Box<Expr>>,
    e_exclude: u8,
) -> Option<Box<Window>> {
    let mut e_type = e_type;
    let mut e_exclude = e_exclude;
    let mut b_implicit_frame = 0u8;

    // O parser garante o seguinte:
    debug_assert!(
        e_type == 0
            || e_type == TK_RANGE as i32
            || e_type == TK_ROWS as i32
            || e_type == TK_GROUPS as i32
    );
    debug_assert!(
        (e_start == TK_PRECEDING as i32 || e_start == TK_FOLLOWING as i32) == p_start.is_some()
    );
    debug_assert!((e_end == TK_FOLLOWING as i32 || e_end == TK_PRECEDING as i32) == p_end.is_some());

    if e_type == 0 {
        b_implicit_frame = 1;
        e_type = TK_RANGE as i32;
    }

    // O limite inicial não pode aparecer antes do final nesta lista:
    //
    //   UNBOUNDED PRECEDING
    //   <expr> PRECEDING
    //   CURRENT ROW
    //   <expr> FOLLOWING
    //   UNBOUNDED FOLLOWING
    //
    // O parser garante que "UNBOUNDED PRECEDING" não é limite final e que "UNBOUNDED FOLLOWING"
    // não é limite inicial.
    if (e_start == TK_CURRENT as i32 && e_end == TK_PRECEDING as i32)
        || (e_start == TK_FOLLOWING as i32
            && (e_end == TK_PRECEDING as i32 || e_end == TK_CURRENT as i32))
    {
        error_msg(db, parse, b"unsupported frame specification", &[]);
        return None;
    }

    if e_exclude == 0 && db.optimization_disabled(SQLITE_WINDOW_FUNC) {
        e_exclude = TK_NO;
    }
    let p_end = window_offset_expr(parse, p_end);
    let p_start = window_offset_expr(parse, p_start);
    Some(Box::new(Window {
        e_frm_type: e_type as u8,
        e_start: e_start as u8,
        e_end: e_end as u8,
        e_exclude,
        b_implicit_frame,
        p_end,
        p_start,
        ..Window::default()
    }))
}

/// `sqlite3WindowAssemble`: liga o PARTITION BY e o ORDER BY à janela. Se `p_base` não é nulo,
/// grava em `z_base` o nome da janela base.
pub fn window_assemble(
    _db: &mut Connection,
    _parse: &mut Parse,
    p_win: Option<Box<Window>>,
    p_partition: Option<Box<ExprList>>,
    p_order_by: Option<Box<ExprList>>,
    p_base: Option<&Token>,
) -> Option<Box<Window>> {
    let mut p_win = p_win;
    if let Some(w) = p_win.as_deref_mut() {
        w.p_partition = p_partition;
        w.p_order_by = p_order_by;
        if let Some(b) = p_base {
            w.z_base = Some(b.z.clone());
        }
    }
    p_win
}

/// O miolo de `sqlite3WindowChain`, sobre qualquer sequência de definições candidatas.
fn window_chain_in<'a>(
    db: &mut Connection,
    parse: &mut Parse,
    p_win: &mut Window,
    candidates: impl Iterator<Item = &'a Window>,
) {
    let Some(z_base) = p_win.z_base.clone() else {
        return;
    };
    let Some(p_exist) = window_find(db, parse, candidates, &z_base) else {
        return;
    };
    // Verifica os erros.
    let z_err: Option<&[u8]> = if p_win.p_partition.is_some() {
        Some(b"PARTITION clause")
    } else if p_exist.p_order_by.is_some() && p_win.p_order_by.is_some() {
        Some(b"ORDER BY clause")
    } else if p_exist.b_implicit_frame == 0 {
        Some(b"frame specification")
    } else {
        None
    };
    if let Some(z_err) = z_err {
        error_msg(
            db,
            parse,
            b"cannot override %s of window: %s",
            &[PrintfArg::Text(Some(z_err.to_vec())), PrintfArg::Text(Some(z_base))],
        );
    } else {
        p_win.p_partition = expr_list_dup(p_exist.p_partition.as_deref(), 0);
        if p_exist.p_order_by.is_some() {
            debug_assert!(p_win.p_order_by.is_none());
            p_win.p_order_by = expr_list_dup(p_exist.p_order_by.as_deref(), 0);
        }
        p_win.z_base = None;
    }
}

/// `sqlite3WindowChain`: `p_win` acabou de ser criada de uma cláusula WINDOW, com a janela base
/// em `z_base`. As definições anteriores da mesma cláusula estão na cadeia que começa em
/// `p_list` (por `p_next_win`). Atualiza `p_win` segundo a base ou deixa um erro no `parse`.
pub fn window_chain(
    db: &mut Connection,
    parse: &mut Parse,
    p_win: &mut Window,
    p_list: Option<&Window>,
) {
    window_chain_in(db, parse, p_win, std::iter::successors(p_list, |w| w.p_next_win.as_deref()));
}

/// `sqlite3WindowAttach`: liga a janela `p_win` à expressão `p`.
pub fn window_attach(
    db: &mut Connection,
    parse: &mut Parse,
    p: Option<&mut Expr>,
    p_win: Option<Box<Window>>,
) {
    let (Some(p), Some(p_win)) = (p, p_win) else {
        // Sem expressão, a janela é solta.
        return;
    };
    debug_assert!(p.op == TK_FUNCTION);
    debug_assert!(p.is_full_size());
    let e_frm_type = p_win.e_frm_type;
    p.y = ExprY::Win(p_win);
    p.set_property(EP_WIN_FUNC | EP_FULL_SIZE);
    if (p.flags & EP_DISTINCT) != 0 && e_frm_type != TK_FILTER {
        error_msg(db, parse, b"DISTINCT is not supported for window functions", &[]);
    }
}

/// A marca de `Window.link_seq` que diz que a janela já passou por [`window_finish_link`]: o
/// resto dos bits é a posição na lista do select, a cabeça na posição 1.
const LINK_FINAL: u32 = 0x8000_0000;

/// `sqlite3WindowLink`, primeira metade: numera a janela na ordem de ligação (`*cnt` é o contador
/// do select, `Select.n_win_linked`). A decisão de entrar na lista é de [`window_finish_link`].
pub fn window_link(cnt: &mut u32, p_win: &mut Window) {
    *cnt += 1;
    p_win.link_seq = *cnt;
}

/// `sqlite3WindowUnlinkFromSelect`: desliga a janela do select a que estiver ligada.
pub fn window_unlink_from_select(p: &mut Window) {
    p.link_seq = 0;
}

/// `sqlite3WindowLink`, segunda metade. A janela entra na lista do select se a lista está vazia
/// ou se é igual à cabeça, e então vira a nova cabeça; senão fica de fora e, se o PARTITION BY
/// difere do da cabeça, liga `SF_MultiPart`. Roda sobre as janelas numeradas por [`window_link`],
/// na ordem em que foram ligadas, e deixa `Select.n_win_linked` com o tamanho da lista.
/// Idempotente: janelas já finalizadas só contam como a lista corrente.
pub fn window_finish_link(p: &mut Select) {
    let mut multi_part = false;
    let n_linked;
    {
        let mut wins = linked_windows(p);
        let is_final = |e: &Expr| e.y_win().map_or(false, |w| w.link_seq & LINK_FINAL != 0);
        let pending: Vec<usize> = (0..wins.len()).filter(|&i| !is_final(&*wins[i])).collect();
        if pending.is_empty() {
            // Nada novo para decidir: só ressincroniza o contador com a lista.
            let n = wins.len() as u32;
            drop(wins);
            p.n_win_linked = n;
            return;
        }
        // A lista corrente, da cabeça para a cauda.
        let mut list: Vec<usize> = (0..wins.len()).filter(|&i| is_final(&*wins[i])).collect();
        let mut dropped: Vec<usize> = Vec::new();
        for i in pending {
            let linked = match list.first() {
                None => true,
                Some(&h) => window_compare(None, wins[h].y_win(), wins[i].y_win(), 0) == 0,
            };
            if linked {
                list.insert(0, i);
            } else {
                let h = list[0];
                let part_i = wins[i].y_win().and_then(|w| w.p_partition.as_deref());
                let part_h = wins[h].y_win().and_then(|w| w.p_partition.as_deref());
                if expr_list_compare(part_i, part_h, -1) != 0 {
                    multi_part = true;
                }
                dropped.push(i);
            }
        }
        for (pos, &i) in list.iter().enumerate() {
            if let Some(w) = wins[i].y_win_mut() {
                w.link_seq = LINK_FINAL | (pos as u32 + 1);
            }
        }
        for i in dropped {
            if let Some(w) = wins[i].y_win_mut() {
                w.link_seq = 0;
            }
        }
        n_linked = list.len() as u32;
    }
    p.n_win_linked = n_linked;
    if multi_part {
        p.sel_flags |= SF_MULTIPART;
    }
}

/// `sqlite3WindowCompare`: 0 se as duas janelas são idênticas, 1 se são diferentes, 2 se não dá
/// para saber. Janelas idênticas são processadas numa só varredura.
pub fn window_compare(
    mut pp: Option<(&mut Connection, &mut Parse)>,
    p1: Option<&Window>,
    p2: Option<&Window>,
    b_filter: i32,
) -> i32 {
    // Reempresta o par `(db, parse)` para cada chamada sem consumi-lo.
    macro_rules! rb {
        () => {
            pp.as_mut().map(|(d, p)| (&mut **d, &mut **p))
        };
    }
    let (Some(p1), Some(p2)) = (p1, p2) else {
        return 1;
    };
    if p1.e_frm_type != p2.e_frm_type {
        return 1;
    }
    if p1.e_start != p2.e_start {
        return 1;
    }
    if p1.e_end != p2.e_end {
        return 1;
    }
    if p1.e_exclude != p2.e_exclude {
        return 1;
    }
    if expr_compare(rb!(), p1.p_start.as_deref(), p2.p_start.as_deref(), -1) != 0 {
        return 1;
    }
    if expr_compare(rb!(), p1.p_end.as_deref(), p2.p_end.as_deref(), -1) != 0 {
        return 1;
    }
    let res = expr_list_compare(p1.p_partition.as_deref(), p2.p_partition.as_deref(), -1);
    if res != 0 {
        return res;
    }
    let res = expr_list_compare(p1.p_order_by.as_deref(), p2.p_order_by.as_deref(), -1);
    if res != 0 {
        return res;
    }
    if b_filter != 0 {
        let res = expr_compare(rb!(), p1.p_filter.as_deref(), p2.p_filter.as_deref(), -1);
        if res != 0 {
            return res;
        }
    }
    0
}

/// `sqlite3WindowDup`: uma cópia da janela. A cópia nasce desligada de qualquer select
/// (`link_seq` zero, sem `p_next_win`), como a do C (`ppThis` nulo).
pub fn window_dup(p: &Window) -> Box<Window> {
    Box::new(Window {
        z_name: p.z_name.clone(),
        z_base: p.z_base.clone(),
        p_filter: expr_dup(p.p_filter.as_deref(), 0),
        p_w_func: p.p_w_func.clone(),
        p_partition: expr_list_dup(p.p_partition.as_deref(), 0),
        p_order_by: expr_list_dup(p.p_order_by.as_deref(), 0),
        e_frm_type: p.e_frm_type,
        e_end: p.e_end,
        e_start: p.e_start,
        e_exclude: p.e_exclude,
        reg_result: p.reg_result,
        reg_accum: p.reg_accum,
        i_arg_col: p.i_arg_col,
        i_eph_csr: p.i_eph_csr,
        b_expr_args: p.b_expr_args,
        p_start: expr_dup(p.p_start.as_deref(), 0),
        p_end: expr_dup(p.p_end.as_deref(), 0),
        b_implicit_frame: p.b_implicit_frame,
        ..Window::default()
    })
}

/// `sqlite3WindowListDup`: uma cópia da lista de definições da cláusula WINDOW.
pub fn window_list_dup(p: &[Window]) -> Vec<Window> {
    p.iter().map(|w| *window_dup(w)).collect()
}

/// A cadeia de definições que o parser monta por `p_next_win` (cabeça primeiro) como o vetor de
/// `Select.p_win_defn`, na mesma ordem.
pub fn window_list_to_vec(p_chain: Option<Box<Window>>) -> Vec<Window> {
    let mut out = Vec::new();
    let mut cur = p_chain;
    while let Some(mut w) = cur {
        cur = w.p_next_win.take();
        out.push(*w);
    }
    out
}

/// `pSelect->pWin`: as expressões de função de janela ligadas ao select (as do resultado e do
/// ORDER BY com `link_seq` diferente de zero), da cabeça da lista para a cauda. Não desce nas
/// subconsultas nem nos argumentos da própria função de janela.
fn linked_windows(p: &mut Select) -> Vec<&mut Expr> {
    fn collect<'a>(e: &'a mut Expr, out: &mut Vec<&'a mut Expr>) {
        if e.has_property(EP_WIN_FUNC) && e.y_win().map_or(false, |w| w.link_seq != 0) {
            out.push(e);
            return;
        }
        if let Some(l) = e.p_left.as_deref_mut() {
            collect(l, out);
        }
        if let Some(r) = e.p_right.as_deref_mut() {
            collect(r, out);
        }
        if let ExprX::List(list) = &mut e.x {
            for it in list.a.iter_mut() {
                if let Some(x) = it.p_expr.as_deref_mut() {
                    collect(x, out);
                }
            }
        }
    }
    let mut out: Vec<&mut Expr> = Vec::new();
    for list in [p.p_e_list.as_deref_mut(), p.p_order_by.as_deref_mut()].into_iter().flatten() {
        for it in list.a.iter_mut() {
            if let Some(e) = it.p_expr.as_deref_mut() {
                collect(e, &mut out);
            }
        }
    }
    out.sort_by_key(|e| e.y_win().map_or(0, |w| w.link_seq));
    out
}

// ---------------------------------------------------------------------------------------------
// chunk 002: a reescrita do SELECT
// ---------------------------------------------------------------------------------------------

/// `struct WindowRewrite`, mais o par `(db, parse)` que o `pWalker->pParse` do C carrega.
struct WindowRewrite<'a> {
    db: &'a mut Connection,
    parse: &'a mut Parse,
    /// `pWin->iEphCsr` da janela principal.
    i_eph_csr: i32,
    /// `pSrc->a[i].iCursor`, de cada termo do FROM movido para a subconsulta.
    src_cursors: Vec<i32>,
    /// A lista de expressões da subconsulta (`pSub`).
    p_sub: Option<Box<ExprList>>,
    /// A tabela provisória que as colunas reescritas citam (`pTab`).
    p_tab: Rc<Table>,
    /// O endereço da subconsulta escalar corrente (`pSubSelect`), 0 se não há.
    p_sub_select: usize,
}

/// `selectWindowRewriteExprCb`: se preciso, acrescenta a expressão à lista de saída e a troca por
/// uma coluna da tabela efêmera.
fn window_rewrite_expr_cb(w: &mut Walker<WindowRewrite<'_>>, p_expr: &mut Expr) -> i32 {
    let p = &mut w.u;

    // Numa subconsulta escalar do SELECT, só as colunas TK_COLUMN que citam o SELECT de fora
    // são processadas. Agregadas e funções de janela pertencem à subconsulta escalar.
    if p.p_sub_select != 0 {
        if p_expr.op != TK_COLUMN {
            return WRC_CONTINUE;
        }
        if !p.src_cursors.contains(&p_expr.i_table) {
            return WRC_CONTINUE;
        }
    }

    let op = p_expr.op;
    match op {
        TK_FUNCTION | TK_IF_NULL_ROW | TK_AGG_FUNCTION | TK_COLUMN => {
            if p_expr.op == TK_FUNCTION {
                if !p_expr.has_property(EP_WIN_FUNC) {
                    return WRC_CONTINUE;
                }
                // Uma função de janela da lista do select fica onde está.
                if p_expr.y_win().map_or(false, |win| win.link_seq != 0) {
                    return WRC_PRUNE;
                }
            }
            let mut i_col: i32 = -1;
            if p.db.malloc_failed != 0 {
                return WRC_ABORT;
            }
            if let Some(sub) = p.p_sub.as_deref() {
                for (i, it) in sub.a.iter().enumerate() {
                    if expr_compare(None, it.p_expr.as_deref(), Some(&*p_expr), -1) == 0 {
                        i_col = i as i32;
                        break;
                    }
                }
            }
            if i_col < 0 {
                let mut p_dup = expr_dup(Some(&*p_expr), 0);
                if let Some(d) = p_dup.as_deref_mut() {
                    if d.op == TK_AGG_FUNCTION {
                        d.op = TK_FUNCTION;
                    }
                }
                p.p_sub = expr_list_append(p.p_sub.take(), p_dup);
            }
            if let Some(sub) = p.p_sub.as_deref() {
                let f = p_expr.flags & EP_COLLATE;
                // O C apaga os filhos e zera o nó (`memset(pExpr, 0, sizeof(Expr))`).
                *p_expr = Expr {
                    op: TK_COLUMN,
                    i_column: if i_col < 0 { sub.a.len() as i32 - 1 } else { i_col },
                    i_table: p.i_eph_csr,
                    y: ExprY::Tab(Some(TabRef::Rc(p.p_tab.clone()))),
                    flags: f,
                    ..Expr::default()
                };
            }
            if p.db.malloc_failed != 0 {
                return WRC_ABORT;
            }
        }
        _ => {}
    }
    WRC_CONTINUE
}

/// `selectWindowRewriteSelectCb`.
fn window_rewrite_select_cb(w: &mut Walker<WindowRewrite<'_>>, p_select: &mut Select) -> i32 {
    let key = p_select as *const Select as usize;
    let p_save = w.u.p_sub_select;
    if p_save == key {
        return WRC_CONTINUE;
    }
    w.u.p_sub_select = key;
    walk_select(w, Some(p_select));
    w.u.p_sub_select = p_save;
    WRC_PRUNE
}

/// `selectWindowRewriteEList`: percorre cada expressão de `p_list`; as colunas, as agregadas e as
/// funções de janela que não são da lista do select são acrescentadas a `*pp_sub` e trocadas por
/// uma coluna da tabela efêmera `i_eph_csr`.
fn window_rewrite_e_list(
    db: &mut Connection,
    parse: &mut Parse,
    i_eph_csr: i32,
    src_cursors: &[i32],
    p_list: Option<&mut ExprList>,
    p_tab: &Rc<Table>,
    pp_sub: &mut Option<Box<ExprList>>,
) {
    let mut w = Walker {
        x_expr_callback: Some(window_rewrite_expr_cb),
        x_select_callback: Some(window_rewrite_select_cb),
        x_select_callback2: None,
        walker_depth: 0,
        e_code: 0,
        m_w_flags: 0,
        u: WindowRewrite {
            db,
            parse,
            i_eph_csr,
            src_cursors: src_cursors.to_vec(),
            p_sub: pp_sub.take(),
            p_tab: p_tab.clone(),
            p_sub_select: 0,
        },
    };
    walk_expr_list(&mut w, p_list);
    *pp_sub = w.u.p_sub.take();
}

/// `sqlite3ExprSkipCollateAndLikely`, sobre uma referência mutável.
fn skip_collate_and_likely_mut(e: &mut Expr) -> &mut Expr {
    if !e.has_property(EP_SKIP | EP_UNLIKELY) {
        return e;
    }
    if e.has_property(EP_UNLIKELY) {
        match &mut e.x {
            ExprX::List(l) => {
                skip_collate_and_likely_mut(l.a[0].p_expr.as_deref_mut().expect("x.pList->a[0]"))
            }
            _ => unreachable!("EP_Unlikely sem x.pList"),
        }
    } else {
        debug_assert!(e.op == TK_COLLATE);
        skip_collate_and_likely_mut(e.p_left.as_deref_mut().expect("pLeft"))
    }
}

/// `exprListAppendList`: acrescenta a `p_list` uma cópia de cada expressão de `p_append`. Com
/// `b_int_to_null`, os inteiros viram NULL.
fn expr_list_append_list(
    p_list: Option<Box<ExprList>>,
    p_append: Option<&ExprList>,
    b_int_to_null: bool,
) -> Option<Box<ExprList>> {
    let mut p_list = p_list;
    if let Some(app) = p_append {
        let n_init = p_list.as_deref().map_or(0, |l| l.a.len());
        for (i, it) in app.a.iter().enumerate() {
            let mut p_dup = expr_dup(it.p_expr.as_deref(), 0);
            if b_int_to_null {
                if let Some(d) = p_dup.as_deref_mut() {
                    let p_sub = skip_collate_and_likely_mut(d);
                    let mut i_dummy = 0;
                    if expr_is_integer(p_sub, &mut i_dummy) != 0 {
                        p_sub.op = TK_NULL;
                        p_sub.flags &= !(EP_INT_VALUE | EP_IS_TRUE | EP_IS_FALSE);
                        p_sub.u = ExprU::Token(None);
                    }
                }
            }
            p_list = expr_list_append(p_list.take(), p_dup);
            if let Some(l) = p_list.as_deref_mut() {
                l.a[n_init + i].fg.sort_flags = it.fg.sort_flags;
            }
        }
    }
    p_list
}

/// `sqlite3WindowExtraAggFuncDepth`: ao acrescentar a camada da subconsulta, os nós
/// TK_AGG_FUNCTION que citam o select de fora ganham um nível em `op2` (ver `incrAggDepth` em
/// `resolve.c`).
fn window_extra_agg_func_depth(w: &mut Walker<()>, p_expr: &mut Expr) -> i32 {
    if p_expr.op == TK_AGG_FUNCTION && p_expr.op2 as i32 >= w.walker_depth {
        p_expr.op2 += 1;
    }
    WRC_CONTINUE
}

/// O contexto do walker de `disallowAggregatesInOrderByCb`.
struct DisallowAgg<'a> {
    db: &'a mut Connection,
    parse: &'a mut Parse,
}

/// `disallowAggregatesInOrderByCb`.
fn disallow_aggregates_in_order_by_cb(w: &mut Walker<DisallowAgg<'_>>, p_expr: &mut Expr) -> i32 {
    if p_expr.op == TK_AGG_FUNCTION && p_expr.p_agg_info.is_none() {
        debug_assert!(!p_expr.has_property(EP_INT_VALUE));
        error_msg(
            w.u.db,
            w.u.parse,
            b"misuse of aggregate: %s()",
            &[PrintfArg::Text(p_expr.z_token().map(|z| z.to_vec()))],
        );
    }
    WRC_CONTINUE
}

/// Troca em cada coluna dos selects percorridos a tabela provisória `.0` pela definitiva `.1`:
/// o que o `memcpy(pTab, pTab2, ...)` do C faz por ponteiro compartilhado.
fn retarget_table_cb(w: &mut Walker<(Rc<Table>, Rc<Table>)>, p_expr: &mut Expr) -> i32 {
    if p_expr.op == TK_COLUMN {
        if let ExprY::Tab(Some(TabRef::Rc(t))) = &mut p_expr.y {
            if Rc::ptr_eq(t, &w.u.0) {
                *t = w.u.1.clone();
            }
        }
    }
    WRC_CONTINUE
}

/// Aplica [`retarget_table_cb`] às listas `p_e_list` e `p_order_by` (e às subconsultas delas).
fn retarget_table(
    old: &Rc<Table>,
    new: &Rc<Table>,
    p_e_list: Option<&mut ExprList>,
    p_order_by: Option<&mut ExprList>,
) {
    let mut w = Walker {
        x_expr_callback: Some(retarget_table_cb),
        x_select_callback: Some(select_walk_noop::<(Rc<Table>, Rc<Table>)>),
        x_select_callback2: None,
        walker_depth: 0,
        e_code: 0,
        m_w_flags: 0,
        u: (old.clone(), new.clone()),
    };
    walk_expr_list(&mut w, p_e_list);
    walk_expr_list(&mut w, p_order_by);
}

/// `sqlite3WindowRewrite`: se o SELECT não usa funções de janela, não faz nada. Senão o reescreve
/// para que os xStep das funções de janela rodem na ordem certa, como descrito em "SELECT
/// REWRITING" no topo do `window.c`: FROM, WHERE, GROUP BY e HAVING vão para uma subconsulta
/// (co-rotina) e o select de fora lê as colunas dela. Devolve `SQLITE_OK` ou `SQLITE_NOMEM`.
pub fn window_rewrite(db: &mut Connection, parse: &mut Parse, p: &mut Select) -> i32 {
    let mut rc = SQLITE_OK;
    window_finish_link(p);
    if p.n_win_linked > 0
        && p.p_prior.is_none()
        && (p.sel_flags & SF_WINREWRITE) == 0
        && !parse.in_rename_object()
    {
        get_vdbe(db, parse);
        let sel_flags = p.sel_flags;

        let p_tab = Rc::new(Table::default());
        let mut w: Walker<()> = Walker::default();
        agg_info_persist_walker_init(&mut w);
        walk_select(&mut w, Some(&mut *p));
        if (p.sel_flags & SF_AGGREGATE) == 0 {
            let mut w2 = Walker {
                x_expr_callback: Some(disallow_aggregates_in_order_by_cb),
                x_select_callback: None,
                x_select_callback2: None,
                walker_depth: 0,
                e_code: 0,
                m_w_flags: 0,
                u: DisallowAgg { db: &mut *db, parse: &mut *parse },
            };
            walk_expr_list(&mut w2, p.p_order_by.as_deref_mut());
        }

        let p_src = p.p_src.take();
        let p_where = p.p_where.take();
        let p_group_by = p.p_group_by.take();
        let p_having = p.p_having.take();
        p.sel_flags &= !SF_AGGREGATE;
        p.sel_flags |= SF_WINREWRITE;

        // O PARTITION BY e o ORDER BY da janela principal.
        let (p_part, p_win_order) = {
            let wins = linked_windows(p);
            let win = wins[0].y_win().expect("pMWin");
            (win.p_partition.clone(), win.p_order_by.clone())
        };

        // Cria o ORDER BY da subconsulta: a concatenação do PARTITION BY e do ORDER BY da janela.
        // Se isso torna redundante o ORDER BY do select de fora, ele é removido.
        let mut p_sort = expr_list_append_list(None, p_part.as_deref(), true);
        p_sort = expr_list_append_list(p_sort, p_win_order.as_deref(), true);
        let redundant = match (p_sort.as_deref(), p.p_order_by.as_deref()) {
            (Some(sort), Some(order)) if order.a.len() <= sort.a.len() => {
                let head = ExprList { a: sort.a[..order.a.len()].to_vec() };
                expr_list_compare(Some(&head), Some(order), -1) == 0
            }
            _ => false,
        };
        if redundant {
            p.p_order_by = None;
        }

        // Número do cursor da tabela efêmera que guarda as linhas. O OpenEphemeral é codificado
        // depois, quando se sabe quantas colunas terá.
        let i_eph_csr = parse.n_tab;
        parse.n_tab += 1;
        parse.n_tab += 3;
        if let Some(win) = linked_windows(p)[0].y_win_mut() {
            win.i_eph_csr = i_eph_csr;
        }

        let src_cursors: Vec<i32> =
            p_src.as_deref().map_or_else(Vec::new, |s| s.a.iter().map(|i| i.i_cursor).collect());
        let mut p_sublist: Option<Box<ExprList>> = None;
        window_rewrite_e_list(
            db,
            parse,
            i_eph_csr,
            &src_cursors,
            p.p_e_list.as_deref_mut(),
            &p_tab,
            &mut p_sublist,
        );
        window_rewrite_e_list(
            db,
            parse,
            i_eph_csr,
            &src_cursors,
            p.p_order_by.as_deref_mut(),
            &p_tab,
            &mut p_sublist,
        );
        let n_buffer_col = p_sublist.as_deref().map_or(0, |l| l.a.len() as i32);
        if let Some(win) = linked_windows(p)[0].y_win_mut() {
            win.n_buffer_col = n_buffer_col;
        }

        // Acrescenta o PARTITION BY e o ORDER BY à lista da subconsulta: servem para achar as
        // fronteiras das partições e dos conjuntos de pares.
        p_sublist = expr_list_append_list(p_sublist, p_part.as_deref(), false);
        p_sublist = expr_list_append_list(p_sublist, p_win_order.as_deref(), false);

        // Acrescenta à lista da subconsulta os argumentos de cada função de janela e aloca dois
        // registradores por função: um para o acumulador, outro para os resultados parciais.
        {
            let mut wins = linked_windows(p);
            for owner in wins.iter_mut() {
                let Expr { x, y, .. } = &mut **owner;
                let ExprY::Win(win) = y else {
                    unreachable!("EP_WinFunc sem y.pWin")
                };
                debug_assert!(win.p_w_func.is_some());
                let f_flags = win.p_w_func.as_ref().map_or(0, |f| f.func_flags);
                let p_args: Option<&mut ExprList> = match x {
                    ExprX::List(l) => Some(&mut **l),
                    _ => None,
                };
                if (f_flags & SQLITE_SUBTYPE as u32) != 0 {
                    window_rewrite_e_list(
                        db,
                        parse,
                        i_eph_csr,
                        &src_cursors,
                        p_args,
                        &p_tab,
                        &mut p_sublist,
                    );
                    win.i_arg_col = p_sublist.as_deref().map_or(0, |l| l.a.len() as i32);
                    win.b_expr_args = 1;
                } else {
                    win.i_arg_col = p_sublist.as_deref().map_or(0, |l| l.a.len() as i32);
                    p_sublist = expr_list_append_list(p_sublist, p_args.as_deref(), false);
                }
                if let Some(f) = win.p_filter.as_deref() {
                    let p_filter = expr_dup(Some(f), 0);
                    p_sublist = expr_list_append(p_sublist.take(), p_filter);
                }
                parse.n_mem += 1;
                win.reg_accum = parse.n_mem;
                parse.n_mem += 1;
                win.reg_result = parse.n_mem;
                add_op2(vd!(parse), OP_NULL as i32, 0, win.reg_accum);
            }
        }

        // Sem ORDER BY nem PARTITION BY, com uma função de janela sem argumentos e nenhuma outra
        // coluna selecionada (`SELECT row_number() OVER () FROM t1`), `p_sublist` ainda é nula:
        // uma constante a mantém válida.
        if p_sublist.is_none() {
            p_sublist = expr_list_append(None, expr(TK_INTEGER as i32, Some(b"0")));
        }

        let p_sub = select_new(parse, p_sublist, p_src, p_where, p_group_by, p_having, p_sort, 0, None);
        p.p_src = src_list_append(db, parse, None, None, None);
        if let (Some(src), Some(sub)) = (p.p_src.as_deref_mut(), p_sub) {
            src.a[0].p_select = Some(sub);
            src.a[0].fg.is_correlated = true;
            src_list_assign_cursors(parse, src);
            let sub = src.a[0].p_select.as_deref_mut().expect("pSelect");
            sub.sel_flags |= SF_EXPANDED | SF_ORDERBYREQD;
            let p_tab2 = result_set_of_select(db, parse, sub, SQLITE_AFF_NONE);
            sub.sel_flags |= sel_flags & SF_AGGREGATE;
            match p_tab2 {
                // Pode ser outro tipo de erro, mas nesse caso `parse.n_err` está ligado, e o
                // SQLITE_NOMEM só pesa se for o erro de verdade.
                None => rc = SQLITE_NOMEM,
                Some(mut t2) => {
                    t2.tab_flags |= TF_EPHEMERAL;
                    let tab = Rc::new(t2);
                    let mut w3: Walker<()> = Walker {
                        x_expr_callback: Some(window_extra_agg_func_depth),
                        x_select_callback: Some(walker_depth_increase::<()>),
                        x_select_callback2: Some(walker_depth_decrease::<()>),
                        walker_depth: 0,
                        e_code: 0,
                        m_w_flags: 0,
                        u: (),
                    };
                    walk_select(&mut w3, Some(sub));
                    src.a[0].p_tab = Some(tab.clone());
                    retarget_table(
                        &p_tab,
                        &tab,
                        p.p_e_list.as_deref_mut(),
                        p.p_order_by.as_deref_mut(),
                    );
                }
            }
        }
        if db.malloc_failed != 0 {
            rc = SQLITE_NOMEM;
        }
    }

    debug_assert!(rc == SQLITE_OK || parse.n_err != 0);
    rc
}

// ---------------------------------------------------------------------------------------------
// chunk 003 (cont.): a inicialização do código de janelas
// ---------------------------------------------------------------------------------------------

/// `sqlite3WindowCodeInit`: chamada pelo `select.c` antes de `sqlite3WhereBegin()`, aloca e
/// inicializa os registradores e cursores de que `sqlite3WindowCodeStep()` precisa.
pub fn window_code_init(db: &mut Connection, parse: &mut Parse, p_select: &mut Select) {
    let n_eph_expr = p_select
        .p_src
        .as_deref()
        .and_then(|s| s.a[0].p_select.as_deref())
        .and_then(|s| s.p_e_list.as_deref())
        .map_or(0, |l| l.a.len() as i32);
    get_vdbe(db, parse);
    let mut wins = linked_windows(p_select);

    let i_eph_csr;
    {
        let mw = wins[0].y_win_mut().expect("pMWin");
        i_eph_csr = mw.i_eph_csr;
        add_op2(vd!(parse), OP_OPENEPHEMERAL as i32, mw.i_eph_csr, n_eph_expr);
        add_op2(vd!(parse), OP_OPENDUP as i32, mw.i_eph_csr + 1, mw.i_eph_csr);
        add_op2(vd!(parse), OP_OPENDUP as i32, mw.i_eph_csr + 2, mw.i_eph_csr);
        add_op2(vd!(parse), OP_OPENDUP as i32, mw.i_eph_csr + 3, mw.i_eph_csr);

        // Aloca os registradores dos valores do PARTITION BY, se houver, e os zera.
        if let Some(part) = mw.p_partition.as_deref() {
            let n_expr = part.a.len() as i32;
            mw.reg_part = parse.n_mem + 1;
            parse.n_mem += n_expr;
            add_op3(vd!(parse), OP_NULL as i32, 0, mw.reg_part, mw.reg_part + n_expr - 1);
        }

        parse.n_mem += 1;
        mw.reg_one = parse.n_mem;
        add_op2(vd!(parse), OP_INTEGER as i32, 1, mw.reg_one);

        if mw.e_exclude != 0 {
            parse.n_mem += 1;
            mw.reg_start_rowid = parse.n_mem;
            parse.n_mem += 1;
            mw.reg_end_rowid = parse.n_mem;
            mw.csr_app = parse.n_tab;
            parse.n_tab += 1;
            add_op2(vd!(parse), OP_INTEGER as i32, 1, mw.reg_start_rowid);
            add_op2(vd!(parse), OP_INTEGER as i32, 0, mw.reg_end_rowid);
            add_op2(vd!(parse), OP_OPENDUP as i32, mw.csr_app, mw.i_eph_csr);
            return;
        }
    }

    for owner in wins.iter_mut() {
        let Expr { x, y, .. } = &mut **owner;
        let ExprY::Win(win) = y else {
            unreachable!("EP_WinFunc sem y.pWin")
        };
        let p_func = win.p_w_func.clone().expect("pWFunc");
        if (p_func.func_flags & SQLITE_FUNC_MINMAX) != 0 && win.e_start != TK_UNBOUNDED {
            // As versões inline de min() e max() precisam de uma tabela efêmera e 3 registradores:
            //
            //   regApp+0: onde copiar o argumento de min()/max() para o MakeRecord
            //   regApp+1: inteiro que garante a unicidade das chaves
            //   regApp+2: a saída do MakeRecord
            let p_list = match x {
                ExprX::List(l) => &**l,
                _ => unreachable!("min/max sem x.pList"),
            };
            let mut p_key_info = key_info_from_expr_list(db, parse, p_list, 0, 0);
            win.csr_app = parse.n_tab;
            parse.n_tab += 1;
            win.reg_app = parse.n_mem + 1;
            parse.n_mem += 3;
            if p_func.z_name.get(1) == Some(&b'i') {
                debug_assert!(p_key_info.a_sort_flags[0] == 0);
                Rc::make_mut(&mut p_key_info).a_sort_flags[0] = KEYINFO_ORDER_DESC;
            }
            add_op2(vd!(parse), OP_OPENEPHEMERAL as i32, win.csr_app, 2);
            append_p4(vd!(parse), P4::KeyInfo(p_key_info));
            add_op2(vd!(parse), OP_INTEGER as i32, 0, win.reg_app + 1);
        } else if is_builtin_named(&p_func, NTH_VALUE_NAME)
            || is_builtin_named(&p_func, FIRST_VALUE_NAME)
        {
            // Dois registradores em regApp guardam o índice inicial e o final do quadro corrente.
            win.reg_app = parse.n_mem + 1;
            win.csr_app = parse.n_tab;
            parse.n_tab += 1;
            parse.n_mem += 2;
            add_op2(vd!(parse), OP_OPENDUP as i32, win.csr_app, i_eph_csr);
        } else if is_builtin_named(&p_func, LEAD_NAME) || is_builtin_named(&p_func, LAG_NAME) {
            win.csr_app = parse.n_tab;
            parse.n_tab += 1;
            add_op2(vd!(parse), OP_OPENDUP as i32, win.csr_app, i_eph_csr);
        }
    }
}

const WINDOW_STARTING_INT: i32 = 0;
const WINDOW_ENDING_INT: i32 = 1;
const WINDOW_NTH_VALUE_INT: i32 = 2;
const WINDOW_STARTING_NUM: i32 = 3;
const WINDOW_ENDING_NUM: i32 = 4;

/// `windowCheckValue`: um "PRECEDING <expr>" (`e_cond` 0), um "FOLLOWING <expr>" (1) ou o segundo
/// argumento de `nth_value()` (2) acabou de ser avaliado, com o resultado em `reg`. Gera o código
/// que confere que o valor é um inteiro não negativo e lança uma exceção se não é.
fn window_check_value(parse: &mut Parse, reg: i32, e_cond: i32) {
    const AZ_ERR: [&[u8]; 5] = [
        b"frame starting offset must be a non-negative integer",
        b"frame ending offset must be a non-negative integer",
        b"second argument to nth_value must be a positive integer",
        b"frame starting offset must be a non-negative number",
        b"frame ending offset must be a non-negative number",
    ];
    const A_OP: [u8; 5] = [OP_GE, OP_GE, OP_GT, OP_GE, OP_GE];
    debug_assert!((WINDOW_STARTING_INT..=WINDOW_ENDING_NUM).contains(&e_cond));
    let reg_zero = get_temp_reg(parse);
    add_op2(vd!(parse), OP_INTEGER as i32, 0, reg_zero);
    if e_cond >= WINDOW_STARTING_NUM {
        let reg_string = get_temp_reg(parse);
        add_op4(vd!(parse), OP_STRING8 as i32, 0, reg_string, 0, P4::Text(Vec::new()));
        let a = current_addr(parse) + 2;
        add_op3(vd!(parse), OP_GE as i32, reg_string, a, reg);
        change_p5(vd!(parse), (SQLITE_AFF_NUMERIC | SQLITE_JUMPIFNULL) as u16);
        debug_assert!(e_cond == 3 || e_cond == 4);
    } else {
        let a = current_addr(parse) + 2;
        add_op2(vd!(parse), OP_MUSTBEINT as i32, reg, a);
        debug_assert!(e_cond == 0 || e_cond == 1 || e_cond == 2);
    }
    let a = current_addr(parse) + 2;
    add_op3(vd!(parse), A_OP[e_cond as usize] as i32, reg_zero, a, reg);
    change_p5(vd!(parse), SQLITE_AFF_NUMERIC as u16);
    may_abort(parse);
    add_op2(vd!(parse), OP_HALT as i32, SQLITE_ERROR, OE_ABORT as i32);
    append_p4(vd!(parse), P4::Text(AZ_ERR[e_cond as usize].to_vec()));
    release_temp_reg(parse, reg_zero);
}

// ---------------------------------------------------------------------------------------------
// chunk 004 e 005: as rotinas auxiliares de sqlite3WindowCodeStep
// ---------------------------------------------------------------------------------------------

/// Uma janela da lista do select com a lista de argumentos da expressão dona (`pOwner->x.pList`):
/// o que as rotinas de geração de código leem de `Window` e de `Window.pOwner`.
struct WinCode {
    w: Window,
    args: Option<ExprList>,
}

/// `struct WindowCsrAndReg`: um cursor da tabela efêmera e o primeiro registrador do array de
/// valores de pares lidos dele.
#[derive(Default, Clone, Copy)]
struct CsrAndReg {
    /// Número do cursor.
    csr: i32,
    /// Primeiro registrador do array de valores dos pares.
    reg: i32,
}

/// `struct WindowCodeArg`: o contexto que `sqlite3WindowCodeStep()` passa às rotinas auxiliares
/// (o `pParse`, o `pVdbe` e a conexão vão como parâmetros). `wins[0]` é `pMWin`; a lista inteira é
/// a lista de funções em processamento.
///
/// `reg_arg`: primeiro de um array de registradores de acumulador, um por função da lista.
///
/// `e_delete`: quando as linhas do cache da tabela efêmera podem ser removidas (zero: nunca;
/// `WINDOW_RETURN_ROW`: depois de devolvida ao chamador; `WINDOW_AGGINVERSE`: depois do xInverse;
/// `WINDOW_AGGSTEP`: depois do xStep).
///
/// `start`, `current`, `end`: os três cursores sobre a tabela efêmera. `current` aponta a próxima
/// linha a devolver, `end` a próxima a que se aplica o xStep, `start` a próxima a que se aplica o
/// xInverse. Cada um tem o cursor do VDBE e o array de registradores com uma cópia dos valores de
/// pares lidos dele. Conforme o quadro, nem os três são necessários; o que não é fica zerado.
struct WindowCodeArg {
    wins: Vec<WinCode>,
    addr_gosub: i32,
    reg_gosub: i32,
    reg_arg: i32,
    e_delete: i32,
    reg_rowid: i32,
    start: CsrAndReg,
    current: CsrAndReg,
    end: CsrAndReg,
}

/// Os valores de `op` de `windowCodeOp`.
const WINDOW_RETURN_ROW: i32 = 1;
const WINDOW_AGGINVERSE: i32 = 2;
const WINDOW_AGGSTEP: i32 = 3;

/// Os opcodes como `i32`, que é como `windowCodeRangeTest` os recebe e troca.
const OPI_GE: i32 = OP_GE as i32;
const OPI_GT: i32 = OP_GT as i32;
const OPI_LE: i32 = OP_LE as i32;
const OPI_LT: i32 = OP_LT as i32;
const OPI_ADD: i32 = OP_ADD as i32;
const OPI_SUBTRACT: i32 = OP_SUBTRACT as i32;

/// `windowArgCount`: o número de argumentos da função de janela (a lista de argumentos da
/// expressão dona, `pOwner->x.pList`).
fn window_arg_count(args: &Option<ExprList>) -> i32 {
    args.as_ref().map_or(0, |l| l.a.len() as i32)
}

/// O número de termos do ORDER BY da janela principal.
fn n_order_by(s: &WindowCodeArg) -> i32 {
    s.wins[0].w.p_order_by.as_deref().map_or(0, |l| l.a.len() as i32)
}

/// `windowReadPeerValues`: lê os valores de pares do quadro do cursor `csr` para o array de
/// registradores que começa em `reg`.
fn window_read_peer_values(parse: &mut Parse, s: &WindowCodeArg, csr: i32, reg: i32) {
    let mw = &s.wins[0].w;
    if let Some(p_order_by) = mw.p_order_by.as_deref() {
        let n_part = mw.p_partition.as_deref().map_or(0, |l| l.a.len() as i32);
        let i_col_off = mw.n_buffer_col + n_part;
        for i in 0..p_order_by.a.len() as i32 {
            add_op3(vd!(parse), OP_COLUMN as i32, csr, i_col_off + i, reg + i);
        }
    }
}

/// `windowAggStep`: gera o código que invoca o xStep (`b_inverse` zero) ou o xInverse de cada
/// função da lista, ou, nas funções embutidas que não usam a API padrão, o código inline.
///
/// Com `csr >= 0`, `reg` é o primeiro de um array de registradores grande o bastante para os
/// argumentos de cada função e os argumentos são extraídos da linha corrente de `csr` para ele
/// antes do OP_AggStep ou OP_AggInverse. Com `csr < 0` o array em `reg` já tem todas as colunas
/// da linha corrente da subconsulta.
fn window_agg_step(
    db: &mut Connection,
    parse: &mut Parse,
    s: &mut WindowCodeArg,
    csr: i32,
    b_inverse: i32,
    reg: i32,
) {
    let mw_reg_start_rowid = s.wins[0].w.reg_start_rowid;
    let mw_eph_csr = s.wins[0].w.i_eph_csr;
    for wc in s.wins.iter_mut() {
        let p_func = wc.w.p_w_func.clone().expect("pWFunc");
        let mut n_arg = if wc.w.b_expr_args != 0 { 0 } else { window_arg_count(&wc.args) };
        debug_assert!(b_inverse == 0 || wc.w.e_start != TK_UNBOUNDED);
        let (i_arg_col, reg_app, csr_app, reg_accum) =
            (wc.w.i_arg_col, wc.w.reg_app, wc.w.csr_app, wc.w.reg_accum);

        for i in 0..n_arg {
            if i != 1 || !is_builtin_named(&p_func, NTH_VALUE_NAME) {
                add_op3(vd!(parse), OP_COLUMN as i32, csr, i_arg_col + i, reg + i);
            } else {
                add_op3(vd!(parse), OP_COLUMN as i32, mw_eph_csr, i_arg_col + i, reg + i);
            }
        }
        let mut reg_arg = reg;

        if mw_reg_start_rowid == 0
            && (p_func.func_flags & SQLITE_FUNC_MINMAX) != 0
            && wc.w.e_start != TK_UNBOUNDED
        {
            let addr_is_null = add_op1(vd!(parse), OP_ISNULL as i32, reg_arg);
            if b_inverse == 0 {
                add_op2(vd!(parse), OP_ADDIMM as i32, reg_app + 1, 1);
                add_op2(vd!(parse), OP_SCOPY as i32, reg_arg, reg_app);
                add_op3(vd!(parse), OP_MAKERECORD as i32, reg_app, 2, reg_app + 2);
                add_op2(vd!(parse), OP_IDXINSERT as i32, csr_app, reg_app + 2);
            } else {
                add_op4_int(vd!(parse), OP_SEEKGE as i32, csr_app, 0, reg_arg, 1);
                add_op1(vd!(parse), OP_DELETE as i32, csr_app);
                let a = current_addr(parse) - 2;
                jump_here(vd!(parse), a);
            }
            jump_here(vd!(parse), addr_is_null);
        } else if reg_app != 0 {
            debug_assert!(
                is_builtin_named(&p_func, NTH_VALUE_NAME)
                    || is_builtin_named(&p_func, FIRST_VALUE_NAME)
            );
            debug_assert!(b_inverse == 0 || b_inverse == 1);
            add_op2(vd!(parse), OP_ADDIMM as i32, reg_app + 1 - b_inverse, 1);
        } else if !is_noop_step(&p_func) {
            let mut addr_if = 0;
            if wc.w.p_filter.is_some() {
                debug_assert!(
                    wc.w.b_expr_args != 0 || n_arg == 0 || n_arg == window_arg_count(&wc.args)
                );
                let reg_tmp = get_temp_reg(parse);
                add_op3(vd!(parse), OP_COLUMN as i32, csr, i_arg_col + n_arg, reg_tmp);
                addr_if = add_op3(vd!(parse), OP_IFNOT as i32, reg_tmp, 0, 1);
                release_temp_reg(parse, reg_tmp);
            }

            if wc.w.b_expr_args != 0 {
                let i_op = current_addr(parse);
                n_arg = window_arg_count(&wc.args);
                reg_arg = get_temp_range(parse, n_arg);
                expr_code_expr_list(
                    db,
                    parse,
                    wc.args.as_mut().expect("pOwner->x.pList"),
                    reg_arg,
                    0,
                    0,
                    None,
                );
                let i_end = current_addr(parse);
                for i_op in i_op..i_end {
                    if let Some(op) = get_op(vd!(parse), i_op) {
                        if op.opcode == OP_COLUMN && op.p1 == mw_eph_csr {
                            op.p1 = csr;
                        }
                    }
                }
            }
            if (p_func.func_flags & SQLITE_FUNC_NEEDCOLL) != 0 {
                debug_assert!(n_arg > 0);
                let p_coll = expr_nn_coll_seq(
                    db,
                    parse,
                    wc.args.as_ref().and_then(|l| l.a[0].p_expr.as_deref()),
                    None,
                );
                add_op4(vd!(parse), OP_COLLSEQ as i32, 0, 0, 0, P4::Coll(Some(p_coll)));
            }
            let op = if b_inverse != 0 { OP_AGGINVERSE } else { OP_AGGSTEP };
            add_op3(vd!(parse), op as i32, b_inverse, reg_arg, reg_accum);
            append_p4(vd!(parse), P4::FuncDef(p_func.clone()));
            change_p5(vd!(parse), n_arg as u8 as u16);
            if wc.w.b_expr_args != 0 {
                release_temp_range(parse, reg_arg, n_arg);
            }
            if addr_if != 0 {
                jump_here(vd!(parse), addr_if);
            }
        }
    }
}

/// `windowAggFinal`: gera o código que invoca o xValue (`b_fin` falso) ou o xFinalize de cada
/// função da lista, ou, nas funções embutidas que não usam a API padrão, o código equivalente.
fn window_agg_final(parse: &mut Parse, s: &WindowCodeArg, b_fin: bool) {
    let mw_reg_start_rowid = s.wins[0].w.reg_start_rowid;
    for wc in s.wins.iter() {
        let w = &wc.w;
        let p_func = w.p_w_func.clone().expect("pWFunc");
        if mw_reg_start_rowid == 0
            && (p_func.func_flags & SQLITE_FUNC_MINMAX) != 0
            && w.e_start != TK_UNBOUNDED
        {
            add_op2(vd!(parse), OP_NULL as i32, 0, w.reg_result);
            add_op1(vd!(parse), OP_LAST as i32, w.csr_app);
            add_op3(vd!(parse), OP_COLUMN as i32, w.csr_app, 0, w.reg_result);
            let a = current_addr(parse) - 2;
            jump_here(vd!(parse), a);
        } else if w.reg_app != 0 {
            debug_assert!(mw_reg_start_rowid == 0);
        } else {
            let n_arg = window_arg_count(&wc.args);
            if b_fin {
                add_op2(vd!(parse), OP_AGGFINAL as i32, w.reg_accum, n_arg);
                append_p4(vd!(parse), P4::FuncDef(p_func));
                add_op2(vd!(parse), OP_COPY as i32, w.reg_accum, w.reg_result);
                add_op2(vd!(parse), OP_NULL as i32, 0, w.reg_accum);
            } else {
                add_op3(vd!(parse), OP_AGGVALUE as i32, w.reg_accum, n_arg, w.reg_result);
                append_p4(vd!(parse), P4::FuncDef(p_func));
            }
        }
    }
}

/// `windowFullScan`: gera o código que calcula os valores correntes de todas as funções da lista
/// varrendo o quadro corrente inteiro. Os resultados ficam nos registradores `Window.regResult`,
/// prontos para a camada de cima.
fn window_full_scan(db: &mut Connection, parse: &mut Parse, s: &mut WindowCodeArg) {
    let csr = s.wins[0].w.csr_app;
    let n_peer = n_order_by(s);
    let i_eph_csr = s.wins[0].w.i_eph_csr;
    let reg_start_rowid = s.wins[0].w.reg_start_rowid;
    let reg_end_rowid = s.wins[0].w.reg_end_rowid;
    let e_exclude = s.wins[0].w.e_exclude;

    let lbl_next = make_label(parse);
    let lbl_brk = make_label(parse);

    let reg_c_rowid = get_temp_reg(parse); // O valor corrente do rowid.
    let reg_rowid = get_temp_reg(parse); // O valor do rowid do AggStep.
    let (mut reg_c_peer, mut reg_peer) = (0, 0); // Os valores de pares corrente e do AggStep.
    if n_peer > 0 {
        reg_c_peer = get_temp_range(parse, n_peer);
        reg_peer = get_temp_range(parse, n_peer);
    }

    add_op2(vd!(parse), OP_ROWID as i32, i_eph_csr, reg_c_rowid);
    window_read_peer_values(parse, s, i_eph_csr, reg_c_peer);

    for wc in s.wins.iter() {
        add_op2(vd!(parse), OP_NULL as i32, 0, wc.w.reg_accum);
    }

    add_op3(vd!(parse), OP_SEEKGE as i32, csr, lbl_brk, reg_start_rowid);
    let addr_next = current_addr(parse);
    add_op2(vd!(parse), OP_ROWID as i32, csr, reg_rowid);
    add_op3(vd!(parse), OP_GT as i32, reg_end_rowid, lbl_brk, reg_rowid);

    if e_exclude == TK_CURRENT {
        add_op3(vd!(parse), OP_EQ as i32, reg_c_rowid, lbl_next, reg_rowid);
    } else if e_exclude != TK_NO {
        let mut addr_eq = 0;
        let mut p_key_info = None;

        if let Some(ob) = s.wins[0].w.p_order_by.as_deref() {
            p_key_info = Some(key_info_from_expr_list(db, parse, ob, 0, 0));
        }
        if e_exclude == TK_TIES {
            addr_eq = add_op3(vd!(parse), OP_EQ as i32, reg_c_rowid, 0, reg_rowid);
        }
        if let Some(ki) = p_key_info {
            window_read_peer_values(parse, s, csr, reg_peer);
            add_op3(vd!(parse), OP_COMPARE as i32, reg_peer, reg_c_peer, n_peer);
            append_p4(vd!(parse), P4::KeyInfo(ki));
            let addr = current_addr(parse) + 1;
            add_op3(vd!(parse), OP_JUMP as i32, addr, lbl_next, addr);
        } else {
            add_op2(vd!(parse), OP_GOTO as i32, 0, lbl_next);
        }
        if addr_eq != 0 {
            jump_here(vd!(parse), addr_eq);
        }
    }

    let reg_arg = s.reg_arg;
    window_agg_step(db, parse, s, csr, 0, reg_arg);

    resolve_label(parse, db, lbl_next);
    add_op2(vd!(parse), OP_NEXT as i32, csr, addr_next);
    jump_here(vd!(parse), addr_next - 1);
    jump_here(vd!(parse), addr_next + 1);
    release_temp_reg(parse, reg_rowid);
    release_temp_reg(parse, reg_c_rowid);
    if n_peer > 0 {
        release_temp_range(parse, reg_peer, n_peer);
        release_temp_range(parse, reg_c_peer, n_peer);
    }

    window_agg_final(parse, s, true);
}

/// `windowReturnOneRow`: invoca a sub-rotina em `regGosub` (gerada pelo `select.c`) para devolver
/// a linha corrente de `Window.iEphCsr`. Se todas as funções de janela são agregadas pela API
/// padrão, um único OP_Gosub basta. Código extra por linha só sai para `nth_value()`,
/// `first_value()`, `lag()` e `lead()`.
fn window_return_one_row(db: &mut Connection, parse: &mut Parse, s: &mut WindowCodeArg) {
    if s.wins[0].w.reg_start_rowid != 0 {
        window_full_scan(db, parse, s);
    } else {
        let i_eph = s.wins[0].w.i_eph_csr;
        for i in 0..s.wins.len() {
            let w = &s.wins[i].w;
            let p_func = w.p_w_func.clone().expect("pWFunc");
            let (csr, i_arg_col, reg_result, reg_app) = (w.csr_app, w.i_arg_col, w.reg_result, w.reg_app);
            if is_builtin_named(&p_func, NTH_VALUE_NAME) || is_builtin_named(&p_func, FIRST_VALUE_NAME)
            {
                let lbl = make_label(parse);
                let tmp_reg = get_temp_reg(parse);
                add_op2(vd!(parse), OP_NULL as i32, 0, reg_result);

                if is_builtin_named(&p_func, NTH_VALUE_NAME) {
                    add_op3(vd!(parse), OP_COLUMN as i32, i_eph, i_arg_col + 1, tmp_reg);
                    window_check_value(parse, tmp_reg, WINDOW_NTH_VALUE_INT);
                } else {
                    add_op2(vd!(parse), OP_INTEGER as i32, 1, tmp_reg);
                }
                add_op3(vd!(parse), OP_ADD as i32, tmp_reg, reg_app, tmp_reg);
                add_op3(vd!(parse), OP_GT as i32, reg_app + 1, lbl, tmp_reg);
                add_op3(vd!(parse), OP_SEEKROWID as i32, csr, 0, tmp_reg);
                add_op3(vd!(parse), OP_COLUMN as i32, csr, i_arg_col, reg_result);
                resolve_label(parse, db, lbl);
                release_temp_reg(parse, tmp_reg);
            } else if is_builtin_named(&p_func, LEAD_NAME) || is_builtin_named(&p_func, LAG_NAME) {
                let n_arg = window_arg_count(&s.wins[i].args);
                let lbl = make_label(parse);
                let tmp_reg = get_temp_reg(parse);

                if n_arg < 3 {
                    add_op2(vd!(parse), OP_NULL as i32, 0, reg_result);
                } else {
                    add_op3(vd!(parse), OP_COLUMN as i32, i_eph, i_arg_col + 2, reg_result);
                }
                add_op2(vd!(parse), OP_ROWID as i32, i_eph, tmp_reg);
                if n_arg < 2 {
                    let val = if is_builtin_named(&p_func, LEAD_NAME) { 1 } else { -1 };
                    add_op2(vd!(parse), OP_ADDIMM as i32, tmp_reg, val);
                } else {
                    let op = if is_builtin_named(&p_func, LEAD_NAME) { OP_ADD } else { OP_SUBTRACT };
                    let tmp_reg2 = get_temp_reg(parse);
                    add_op3(vd!(parse), OP_COLUMN as i32, i_eph, i_arg_col + 1, tmp_reg2);
                    add_op3(vd!(parse), op as i32, tmp_reg2, tmp_reg, tmp_reg);
                    release_temp_reg(parse, tmp_reg2);
                }

                add_op3(vd!(parse), OP_SEEKROWID as i32, csr, lbl, tmp_reg);
                add_op3(vd!(parse), OP_COLUMN as i32, csr, i_arg_col, reg_result);
                resolve_label(parse, db, lbl);
                release_temp_reg(parse, tmp_reg);
            }
        }
    }
    add_op2(vd!(parse), OP_GOSUB as i32, s.reg_gosub, s.addr_gosub);
}

/// `windowInitAccum`: gera o código que zera o registrador acumulador de cada função da lista e
/// faz a inicialização equivalente das funções embutidas que a exigem. Devolve o primeiro de um
/// array de registradores de argumentos (grande o bastante para a função de mais argumentos).
fn window_init_accum(parse: &mut Parse, s: &WindowCodeArg) -> i32 {
    let mut n_arg = 0;
    let mw_reg_start_rowid = s.wins[0].w.reg_start_rowid;
    for wc in s.wins.iter() {
        let w = &wc.w;
        let p_func = w.p_w_func.clone().expect("pWFunc");
        debug_assert!(w.reg_accum != 0);
        add_op2(vd!(parse), OP_NULL as i32, 0, w.reg_accum);
        n_arg = n_arg.max(window_arg_count(&wc.args));
        if mw_reg_start_rowid == 0 {
            if is_builtin_named(&p_func, NTH_VALUE_NAME) || is_builtin_named(&p_func, FIRST_VALUE_NAME)
            {
                add_op2(vd!(parse), OP_INTEGER as i32, 0, w.reg_app);
                add_op2(vd!(parse), OP_INTEGER as i32, 0, w.reg_app + 1);
            }

            if (p_func.func_flags & SQLITE_FUNC_MINMAX) != 0 && w.csr_app != 0 {
                debug_assert!(w.e_start != TK_UNBOUNDED);
                add_op1(vd!(parse), OP_RESETSORTER as i32, w.csr_app);
                add_op2(vd!(parse), OP_INTEGER as i32, 0, w.reg_app + 1);
            }
        }
    }
    let reg_arg = parse.n_mem + 1;
    parse.n_mem += n_arg;
    reg_arg
}

/// `windowCacheFrame`: verdadeiro se o quadro corrente deve ficar no cache da tabela efêmera,
/// mesmo sem nenhum xInverse a chamar.
fn window_cache_frame(s: &WindowCodeArg) -> bool {
    if s.wins[0].w.reg_start_rowid != 0 {
        return true;
    }
    s.wins.iter().any(|wc| {
        wc.w.p_w_func.as_deref().map_or(false, |f| {
            is_builtin_named(f, NTH_VALUE_NAME)
                || is_builtin_named(f, FIRST_VALUE_NAME)
                || is_builtin_named(f, LEAD_NAME)
                || is_builtin_named(f, LAG_NAME)
        })
    })
}

/// `windowIfNewPeer`: `reg_old` e `reg_new` são o primeiro registrador de arrays do tamanho de
/// `p_order_by`. Gera o código que compara os dois arrays com as colações e demais parâmetros do
/// ORDER BY. Se diferem, copia `reg_new` em `reg_old` e segue adiante; se são iguais, executa um
/// OP_Goto para `addr`.
fn window_if_new_peer(
    db: &mut Connection,
    parse: &mut Parse,
    p_order_by: Option<&ExprList>,
    reg_new: i32,
    reg_old: i32,
    addr: i32,
) {
    if let Some(ob) = p_order_by {
        let n_val = ob.a.len() as i32;
        let p_key_info = key_info_from_expr_list(db, parse, ob, 0, 0);
        add_op3(vd!(parse), OP_COMPARE as i32, reg_old, reg_new, n_val);
        append_p4(vd!(parse), P4::KeyInfo(p_key_info));
        let next = current_addr(parse) + 1;
        add_op3(vd!(parse), OP_JUMP as i32, next, addr, next);
        add_op3(vd!(parse), OP_COPY as i32, reg_new, reg_old, n_val - 1);
    } else {
        add_op2(vd!(parse), OP_GOTO as i32, 0, addr);
    }
}

/// `windowCodeRangeTest`: parte do código dos limites "RANGE ... PRECEDING/FOLLOWING". Supondo
/// a ordem ASC e `op` igual a OP_Ge, gera o equivalente de
///
///   if( csr1.peerVal + regVal >= csr2.peerVal ) goto lbl;
///
/// `op` também pode ser OP_Gt ou OP_Le, e o operador acima muda para ">" ou "<=". Com ORDER BY
/// DESC a comparação é invertida: subtrai-se `regVal` em vez de somar, e o operador se inverte.
/// Uma aritmética especial devolve uma cópia de `csr1.peerVal` se ele não é numérico.
#[allow(clippy::too_many_arguments)]
fn window_code_range_test(
    db: &mut Connection,
    parse: &mut Parse,
    s: &WindowCodeArg,
    op: i32,
    csr1: i32,
    reg_val: i32,
    csr2: i32,
    lbl: i32,
) {
    let mut op = op;
    let reg1 = get_temp_reg(parse); // Registrador de csr1.peerVal+regVal.
    let reg2 = get_temp_reg(parse); // Registrador de csr2.peerVal.
    parse.n_mem += 1;
    let reg_string = parse.n_mem; // Registrador da constante ''.
    let mut arith = OPI_ADD;
    let addr_done = make_label(parse); // Endereço depois do OP_Ge.

    // Lê o valor de pares de cada cursor para um registrador.
    window_read_peer_values(parse, s, csr1, reg1);
    window_read_peer_values(parse, s, csr2, reg2);

    debug_assert!(op == OPI_GE || op == OPI_GT || op == OPI_LE);
    let p_order_by = s.wins[0].w.p_order_by.as_deref().expect("pOrderBy");
    debug_assert!(p_order_by.a.len() == 1);
    let sort_flags = p_order_by.a[0].fg.sort_flags;
    if (sort_flags & KEYINFO_ORDER_DESC) != 0 {
        op = match op {
            OPI_GE => OPI_LE,
            OPI_GT => OPI_LT,
            _ => {
                debug_assert!(op == OPI_LE);
                OPI_GE
            }
        };
        arith = OPI_SUBTRACT;
    }

    // Se o BIGNULL está ligado no ORDER BY, o NULL vale mais que qualquer outro valor, em vez do
    // menos de sempre. Os opcodes OP_Ge e companhia não tratam isso (e ensiná-los custa
    // desempenho), então com BIGNULL os casos em que reg1 ou reg2 são NULL saem neste bloco. O
    // código equivale a:
    //
    //   if( reg1 IS NULL ){
    //     if( op==OP_Ge ) goto lbl;
    //     if( op==OP_Gt && reg2 IS NOT NULL ) goto lbl;
    //     if( op==OP_Le && reg2 IS NULL ) goto lbl;
    //   }else if( reg2 IS NULL ){
    //     if( op==OP_Le ) goto lbl;
    //   }
    //
    // Além disso, se reg1 ou reg2 é NULL e o salto para lbl não é tomado, o controle pula a
    // comparação codificada mais abaixo.
    if (sort_flags & KEYINFO_ORDER_BIGNULL) != 0 {
        // Este bloco roda se reg1 contém NULL.
        let addr = add_op1(vd!(parse), OP_NOTNULL as i32, reg1);
        match op {
            OPI_GE => {
                add_op2(vd!(parse), OP_GOTO as i32, 0, lbl);
            }
            OPI_GT => {
                add_op2(vd!(parse), OP_NOTNULL as i32, reg2, lbl);
            }
            OPI_LE => {
                add_op2(vd!(parse), OP_ISNULL as i32, reg2, lbl);
            }
            _ => debug_assert!(op == OPI_LT), // nada a gerar
        }
        add_op2(vd!(parse), OP_GOTO as i32, 0, addr_done);

        // Este bloco roda se reg1 não é NULL, mas reg2 é.
        jump_here(vd!(parse), addr);
        let dest = if op == OPI_GT || op == OPI_GE { addr_done } else { lbl };
        add_op2(vd!(parse), OP_ISNULL as i32, reg2, dest);
    }

    // O registrador reg1 contém csr1.peerVal. Este bloco soma (ou subtrai, no DESC) o valor
    // numérico de regVal. Se reg1 não é numérico (NULL, texto ou blob) fica como está. Em
    // pseudocódigo:
    //
    //   if( reg1>='' ) goto addrGe;
    //   reg1 = reg1 +/- regVal
    //   addrGe:
    //
    // Todo texto e blob é maior ou igual a uma string vazia, então a soma ou subtração é pulada
    // para eles, como deve. Se reg1 é NULL a aritmética roda, mas somar ou subtrair de NULL dá
    // NULL de qualquer jeito, o que também é o certo.
    add_op4(vd!(parse), OP_STRING8 as i32, 0, reg_string, 0, P4::Text(Vec::new()));
    let addr_ge = add_op3(vd!(parse), OP_GE as i32, reg_string, 0, reg1);
    if (op == OPI_GE && arith == OPI_ADD) || (op == OPI_LE && arith == OPI_SUBTRACT) {
        add_op3(vd!(parse), op, reg2, lbl, reg1);
    }
    add_op3(vd!(parse), arith, reg_val, reg1, reg1);
    jump_here(vd!(parse), addr_ge);

    // Compara reg2 com reg1 e toma o salto se for o caso. Com BIGNULL e algum dos dois NULL o
    // controle pula este teste.
    add_op3(vd!(parse), op, reg2, lbl, reg1);
    let p_coll = expr_nn_coll_seq(db, parse, p_order_by.a[0].p_expr.as_deref(), None);
    append_p4(vd!(parse), P4::Coll(Some(p_coll)));
    change_p5(vd!(parse), SQLITE_NULLEQ as u16);
    resolve_label(parse, db, addr_done);

    debug_assert!(op == OPI_GE || op == OPI_GT || op == OPI_LT || op == OPI_LE);
    release_temp_reg(parse, reg1);
    release_temp_reg(parse, reg2);
}

/// `windowCodeOp`: auxiliar de `sqlite3WindowCodeStep()`. Cada chamada gera o código de uma
/// operação RETURN_ROW, AGGSTEP ou AGGINVERSE (ver o comentário de `window_code_step`).
fn window_code_op(
    db: &mut Connection,
    parse: &mut Parse,
    s: &mut WindowCodeArg,
    op: i32,
    reg_countdown: i32,
    jump_on_eof: bool,
) -> i32 {
    let e_frm_type = s.wins[0].w.e_frm_type;
    let e_start = s.wins[0].w.e_start;
    let e_end = s.wins[0].w.e_end;
    let reg_start_rowid = s.wins[0].w.reg_start_rowid;
    let reg_end_rowid = s.wins[0].w.reg_end_rowid;
    let mut ret = 0;
    let b_peer = e_frm_type != TK_ROWS;

    let lbl_done = make_label(parse);
    let mut addr_next_range = 0;

    // Caso especial: WINDOW_AGGINVERSE é sempre um no-op se o quadro começa em UNBOUNDED
    // PRECEDING.
    if op == WINDOW_AGGINVERSE && e_start == TK_UNBOUNDED {
        debug_assert!(reg_countdown == 0 && !jump_on_eof);
        return 0;
    }

    if reg_countdown > 0 {
        if e_frm_type == TK_RANGE {
            addr_next_range = current_addr(parse);
            debug_assert!(op == WINDOW_AGGINVERSE || op == WINDOW_AGGSTEP);
            if op == WINDOW_AGGINVERSE {
                if e_start == TK_FOLLOWING {
                    let (c, st) = (s.current.csr, s.start.csr);
                    window_code_range_test(db, parse, s, OPI_LE, c, reg_countdown, st, lbl_done);
                } else {
                    let (c, st) = (s.current.csr, s.start.csr);
                    window_code_range_test(db, parse, s, OPI_GE, st, reg_countdown, c, lbl_done);
                }
            } else {
                let (c, e) = (s.current.csr, s.end.csr);
                window_code_range_test(db, parse, s, OPI_GT, e, reg_countdown, c, lbl_done);
            }
        } else {
            add_op3(vd!(parse), OP_IFPOS as i32, reg_countdown, lbl_done, 1);
        }
    }

    if op == WINDOW_RETURN_ROW && reg_start_rowid == 0 {
        window_agg_final(parse, s, false);
    }
    let addr_continue = current_addr(parse);

    // Num quadro (RANGE BETWEEN a FOLLOWING AND b FOLLOWING) ou (RANGE BETWEEN b PRECEDING AND a
    // PRECEDING), o cursor inicial não pode passar do final dentro da tabela temporária, o que
    // aconteceria se a>b. E se o cursor de entrada ainda acha linhas novas, o cursor final não
    // pode passar dele até o EOF.
    if e_start == e_end && reg_countdown != 0 && e_frm_type == TK_RANGE {
        let reg_rowid1 = get_temp_reg(parse);
        let reg_rowid2 = get_temp_reg(parse);
        if op == WINDOW_AGGINVERSE {
            add_op2(vd!(parse), OP_ROWID as i32, s.start.csr, reg_rowid1);
            add_op2(vd!(parse), OP_ROWID as i32, s.end.csr, reg_rowid2);
            add_op3(vd!(parse), OP_GE as i32, reg_rowid2, lbl_done, reg_rowid1);
        } else if s.reg_rowid != 0 {
            add_op2(vd!(parse), OP_ROWID as i32, s.end.csr, reg_rowid1);
            add_op3(vd!(parse), OP_GE as i32, s.reg_rowid, lbl_done, reg_rowid1);
        }
        release_temp_reg(parse, reg_rowid1);
        release_temp_reg(parse, reg_rowid2);
        debug_assert!(e_start == TK_PRECEDING || e_start == TK_FOLLOWING);
    }

    let (csr, reg);
    match op {
        WINDOW_RETURN_ROW => {
            csr = s.current.csr;
            reg = s.current.reg;
            window_return_one_row(db, parse, s);
        }
        WINDOW_AGGINVERSE => {
            csr = s.start.csr;
            reg = s.start.reg;
            if reg_start_rowid != 0 {
                debug_assert!(reg_end_rowid != 0);
                add_op2(vd!(parse), OP_ADDIMM as i32, reg_start_rowid, 1);
            } else {
                let reg_arg = s.reg_arg;
                window_agg_step(db, parse, s, csr, 1, reg_arg);
            }
        }
        _ => {
            debug_assert!(op == WINDOW_AGGSTEP);
            csr = s.end.csr;
            reg = s.end.reg;
            if reg_start_rowid != 0 {
                debug_assert!(reg_end_rowid != 0);
                add_op2(vd!(parse), OP_ADDIMM as i32, reg_end_rowid, 1);
            } else {
                let reg_arg = s.reg_arg;
                window_agg_step(db, parse, s, csr, 0, reg_arg);
            }
        }
    }

    if op == s.e_delete {
        add_op1(vd!(parse), OP_DELETE as i32, csr);
        change_p5(vd!(parse), OPFLAG_SAVEPOSITION as u16);
    }

    if jump_on_eof {
        let a = current_addr(parse) + 2;
        add_op2(vd!(parse), OP_NEXT as i32, csr, a);
        ret = add_op0(vd!(parse), OP_GOTO as i32);
    } else {
        let a = current_addr(parse) + 1 + b_peer as i32;
        add_op2(vd!(parse), OP_NEXT as i32, csr, a);
        if b_peer {
            add_op2(vd!(parse), OP_GOTO as i32, 0, lbl_done);
        }
    }

    if b_peer {
        let n_reg = n_order_by(s);
        let reg_tmp = if n_reg > 0 { get_temp_range(parse, n_reg) } else { 0 };
        window_read_peer_values(parse, s, csr, reg_tmp);
        window_if_new_peer(db, parse, s.wins[0].w.p_order_by.as_deref(), reg_tmp, reg, addr_continue);
        release_temp_range(parse, reg_tmp, n_reg);
    }

    if addr_next_range != 0 {
        add_op2(vd!(parse), OP_GOTO as i32, 0, addr_next_range);
    }
    resolve_label(parse, db, lbl_done);
    ret
}

/// `windowExprGtZero`: verdadeiro se dá para determinar em tempo de compilação que a expressão,
/// convertida em inteiro, vale mais que zero.
fn window_expr_gt_zero(db: &mut Connection, p_expr: Option<&Expr>) -> bool {
    let mut p_val: Option<Mem> = None;
    let enc = db.enc;
    value_from_expr(db, p_expr, enc, SQLITE_AFF_NUMERIC, &mut p_val);
    match p_val {
        Some(v) => {
            let ret = value_int(&v) > 0;
            value_free(v);
            ret
        }
        None => false,
    }
}

// ---------------------------------------------------------------------------------------------
// chunk 006 e 007: sqlite3WindowCodeStep
// ---------------------------------------------------------------------------------------------

/// `sqlite3WindowCodeStep`: `sqlite3WhereBegin()` já foi chamada para o SELECT `p` quando esta
/// função roda. Ela gera o código que preenche o `Window.regResult` de cada função de janela e
/// invoca a sub-rotina em `addr_gosub` uma vez por linha. `sqlite3WhereEnd()` sempre é chamada
/// antes de voltar (aqui, `where_end`, que consome o `wi`).
///
/// A função trata vários tipos de quadro, que pedem processamento um pouco diferente. O
/// pseudocódigo a seguir vale para quadros da forma:
///
///   ROWS BETWEEN <expr1> PRECEDING AND <expr2> FOLLOWING
///
/// Os outros tipos de quadro são variantes dele.
///
/// ```text
///     ... laço iniciado por sqlite3WhereBegin() ...
///       if( new partition ){
///         Gosub flush
///       }
///       Insert new row into eph table.
///
///       if( first row of partition ){
///         // Rewind three cursors, all open on the eph table.
///         Rewind(csrEnd);
///         Rewind(csrStart);
///         Rewind(csrCurrent);
///
///         regEnd = <expr2>          // FOLLOWING expression
///         regStart = <expr1>        // PRECEDING expression
///       }else{
///         // First time this branch is taken, the eph table contains two
///         // rows. The first row in the partition, which all three cursors
///         // currently point to, and the following row.
///         AGGSTEP
///         if( (regEnd--)<=0 ){
///           RETURN_ROW
///           if( (regStart--)<=0 ){
///             AGGINVERSE
///           }
///         }
///       }
///     }
///     flush:
///       AGGSTEP
///       while( 1 ){
///         RETURN ROW
///         if( csrCurrent is EOF ) break;
///         if( (regStart--)<=0 ){
///           AggInverse(csrStart)
///           Next(csrStart)
///         }
///       }
/// ```
///
/// O pseudocódigo usa a seguinte abreviação:
///
///   AGGSTEP:    invoca o xStep() agregado de cada função de janela com argumentos lidos da
///               linha corrente do cursor csrEnd, e avança csrEnd uma linha.
///
///   RETURN_ROW: devolve uma linha ao chamador com base na linha corrente de csrCurrent e no
///               estado corrente de todos os agregados. Depois avança csrCurrent uma linha.
///
///   AGGINVERSE: invoca o xInverse() agregado de cada função de janela com argumentos lidos da
///               linha corrente do cursor csrStart. Depois avança csrStart uma linha.
///
/// Dois outros quadros ROWS têm tratamento bem diferente: "BETWEEN <expr> PRECEDING AND <expr>
/// PRECEDING" e "BETWEEN <expr> FOLLOWING AND <expr> FOLLOWING". São casos especiais porque
/// mudam a ordem em que os três cursores percorrem a tabela efêmera. Os casos com UNBOUNDED ou
/// CURRENT ROW são variações bem mais simples de um destes três. Os quadros GROUPS usam os mesmos
/// padrões do ROWS, com cada passo processando a linha corrente do cursor e todas as seguintes do
/// mesmo grupo. Os quadros RANGE também operam por grupos e, nos limites com deslocamento,
/// comparam o valor de ordenação (`csr.key + regVal`) em vez de contar linhas.
///
/// Quando (expr1 < expr2) em "ROWS BETWEEN <expr1> PRECEDING AND <expr2> PRECEDING" e no
/// equivalente FOLLOWING, a detecção é em tempo de execução: no primeiro registro da partição o
/// código devolve a linha, apaga o conteúdo da tabela efêmera e segue para a próxima iteração do
/// laço externo.
pub fn window_code_step(
    db: &mut Connection,
    parse: &mut Parse,
    p: &mut Select,
    wi: WhereInfo,
    reg_gosub: i32,
    addr_gosub: i32,
) {
    let (csr_input, n_input) = {
        let item = &p.p_src.as_deref().expect("pSrc").a[0];
        (item.i_cursor, item.p_tab.as_deref().expect("pTab").n_col as i32)
    };
    get_vdbe(db, parse);
    let wins: Vec<WinCode> = linked_windows(p)
        .into_iter()
        .map(|e| WinCode {
            w: e.y_win().expect("y.pWin").clone(),
            args: e.x_list().cloned(),
        })
        .collect();
    let mut s = WindowCodeArg {
        wins,
        addr_gosub,
        reg_gosub,
        reg_arg: 0,
        e_delete: 0,
        reg_rowid: 0,
        start: CsrAndReg::default(),
        current: CsrAndReg::default(),
        end: CsrAndReg::default(),
    };

    // Os campos de `pMWin` que o resto lê. Nenhum deles muda aqui.
    let e_start = s.wins[0].w.e_start;
    let e_end = s.wins[0].w.e_end;
    let e_frm_type = s.wins[0].w.e_frm_type;
    let e_exclude = s.wins[0].w.e_exclude;
    let n_buffer_col = s.wins[0].w.n_buffer_col;
    let reg_part = s.wins[0].w.reg_part;
    let reg_one = s.wins[0].w.reg_one;
    let reg_start_rowid = s.wins[0].w.reg_start_rowid;
    let reg_end_rowid = s.wins[0].w.reg_end_rowid;
    let n_part = s.wins[0].w.p_partition.as_deref().map_or(0, |l| l.a.len() as i32);
    let has_partition = s.wins[0].w.p_partition.is_some();
    let n_order_by_terms = n_order_by(&s);

    debug_assert!(
        e_start == TK_PRECEDING
            || e_start == TK_CURRENT
            || e_start == TK_FOLLOWING
            || e_start == TK_UNBOUNDED
    );
    debug_assert!(
        e_end == TK_FOLLOWING
            || e_end == TK_CURRENT
            || e_end == TK_UNBOUNDED
            || e_end == TK_PRECEDING
    );
    debug_assert!(
        e_exclude == 0
            || e_exclude == TK_CURRENT
            || e_exclude == TK_GROUP
            || e_exclude == TK_TIES
            || e_exclude == TK_NO
    );

    let lbl_where_end = make_label(parse); // Rótulo logo antes do código de sqlite3WhereEnd().

    // Preenche o objeto de contexto.
    s.current.csr = s.wins[0].w.i_eph_csr;
    let csr_write = s.current.csr + 1; // Cursor que escreve na tabela efêmera.
    s.start.csr = s.current.csr + 2;
    s.end.csr = s.current.csr + 3;

    // Decide quando as linhas podem ser apagadas da tabela efêmera. São quatro opções: nunca
    // (eDelete==0), assim que saem do quadro (WINDOW_AGGINVERSE), depois de devolvidas ao
    // chamador (WINDOW_RETURN_ROW) ou depois que entram no quadro (WINDOW_AGGSTEP).
    match e_start {
        TK_FOLLOWING => {
            if e_frm_type != TK_RANGE
                && window_expr_gt_zero(db, s.wins[0].w.p_start.as_deref())
            {
                s.e_delete = WINDOW_RETURN_ROW;
            }
        }
        TK_UNBOUNDED => {
            if !window_cache_frame(&s) {
                if e_end == TK_PRECEDING {
                    if e_frm_type != TK_RANGE
                        && window_expr_gt_zero(db, s.wins[0].w.p_end.as_deref())
                    {
                        s.e_delete = WINDOW_AGGSTEP;
                    }
                } else {
                    s.e_delete = WINDOW_RETURN_ROW;
                }
            }
        }
        _ => s.e_delete = WINDOW_AGGINVERSE,
    }

    // Aloca registradores para o array de valores da subconsulta, os mesmos valores em forma de
    // registro e o rowid com que esse registro entra na tabela efêmera.
    let reg_new = parse.n_mem + 1;
    parse.n_mem += n_input;
    parse.n_mem += 1;
    let reg_record = parse.n_mem;
    parse.n_mem += 1;
    s.reg_rowid = parse.n_mem;

    // Se o quadro tem um "<expr> PRECEDING" ou "<expr> FOLLOWING", aloca registradores para o
    // resultado de cada <expr>.
    let mut reg_start = 0;
    let mut reg_end = 0;
    if e_start == TK_PRECEDING || e_start == TK_FOLLOWING {
        parse.n_mem += 1;
        reg_start = parse.n_mem;
    }
    if e_end == TK_PRECEDING || e_end == TK_FOLLOWING {
        parse.n_mem += 1;
        reg_end = parse.n_mem;
    }

    // Se o quadro não é "ROWS BETWEEN ...", aloca arrays de registradores para cópias das
    // expressões do ORDER BY (os valores de pares) do laço principal e de cada cursor (start,
    // current e end).
    let mut reg_new_peer = 0;
    let mut reg_peer = 0;
    if e_frm_type != TK_ROWS {
        let n_peer = n_order_by_terms;
        reg_new_peer = reg_new + n_buffer_col;
        if has_partition {
            reg_new_peer += n_part;
        }
        reg_peer = parse.n_mem + 1;
        parse.n_mem += n_peer;
        s.start.reg = parse.n_mem + 1;
        parse.n_mem += n_peer;
        s.current.reg = parse.n_mem + 1;
        parse.n_mem += n_peer;
        s.end.reg = parse.n_mem + 1;
        parse.n_mem += n_peer;
    }

    // Carrega os valores da linha devolvida pela subconsulta num array de registradores que
    // começa em regNew, e os monta num registro em regRecord.
    for i_input in 0..n_input {
        add_op3(vd!(parse), OP_COLUMN as i32, csr_input, i_input, reg_new + i_input);
    }
    add_op3(vd!(parse), OP_MAKERECORD as i32, reg_new, n_input, reg_record);

    // Uma linha de entrada acabou de ser lida para o array que começa em regNew. Se a janela tem
    // PARTITION BY, este bloco gera o código que confere se a linha começa uma partição nova. Se
    // sim, faz um OP_Gosub para um endereço que se preenche depois (`addr_gosub_flush`).
    let mut reg_flush_part = 0;
    let mut addr_gosub_flush = 0;
    if has_partition {
        let reg_new_part = reg_new + n_buffer_col;
        let p_key_info = key_info_from_expr_list(
            db,
            parse,
            s.wins[0].w.p_partition.as_deref().expect("pPartition"),
            0,
            0,
        );

        parse.n_mem += 1;
        reg_flush_part = parse.n_mem;
        let addr = add_op3(vd!(parse), OP_COMPARE as i32, reg_new_part, reg_part, n_part);
        append_p4(vd!(parse), P4::KeyInfo(p_key_info));
        add_op3(vd!(parse), OP_JUMP as i32, addr + 2, addr + 4, addr + 2);
        addr_gosub_flush = add_op1(vd!(parse), OP_GOSUB as i32, reg_flush_part);
        vdbe_comment(vd!(parse), b"call flush_partition", &[]);
        add_op3(vd!(parse), OP_COPY as i32, reg_new_part, reg_part, n_part - 1);
    }

    // Insere a linha nova na tabela efêmera.
    add_op2(vd!(parse), OP_NEWROWID as i32, csr_write, s.reg_rowid);
    add_op3(vd!(parse), OP_INSERT as i32, csr_write, reg_record, s.reg_rowid);
    let addr_ne = add_op3(vd!(parse), OP_NE as i32, reg_one, 0, s.reg_rowid);

    // Este bloco roda para a primeira linha de cada partição.
    s.reg_arg = window_init_accum(parse, &s);

    if reg_start != 0 {
        let p_start = s.wins[0].w.p_start.as_deref_mut().expect("pStart");
        expr_code(db, parse, p_start, reg_start, None);
        let c = WINDOW_STARTING_INT + if e_frm_type == TK_RANGE { 3 } else { 0 };
        window_check_value(parse, reg_start, c);
    }
    if reg_end != 0 {
        let p_end = s.wins[0].w.p_end.as_deref_mut().expect("pEnd");
        expr_code(db, parse, p_end, reg_end, None);
        let c = WINDOW_ENDING_INT + if e_frm_type == TK_RANGE { 3 } else { 0 };
        window_check_value(parse, reg_end, c);
    }

    if e_frm_type != TK_RANGE && e_start == e_end && reg_start != 0 {
        let op = if e_start == TK_FOLLOWING { OP_GE } else { OP_LE };
        let addr_ge = add_op3(vd!(parse), op as i32, reg_start, 0, reg_end);
        window_agg_final(parse, &s, false);
        add_op1(vd!(parse), OP_REWIND as i32, s.current.csr);
        window_return_one_row(db, parse, &mut s);
        add_op1(vd!(parse), OP_RESETSORTER as i32, s.current.csr);
        add_op2(vd!(parse), OP_GOTO as i32, 0, lbl_where_end);
        jump_here(vd!(parse), addr_ge);
    }
    if e_start == TK_FOLLOWING && e_frm_type != TK_RANGE && reg_end != 0 {
        debug_assert!(e_end == TK_FOLLOWING);
        add_op3(vd!(parse), OP_SUBTRACT as i32, reg_start, reg_end, reg_start);
    }

    if e_start != TK_UNBOUNDED {
        add_op1(vd!(parse), OP_REWIND as i32, s.start.csr);
    }
    add_op1(vd!(parse), OP_REWIND as i32, s.current.csr);
    add_op1(vd!(parse), OP_REWIND as i32, s.end.csr);
    if reg_peer != 0 && n_order_by_terms > 0 {
        let n = n_order_by_terms - 1;
        add_op3(vd!(parse), OP_COPY as i32, reg_new_peer, reg_peer, n);
        add_op3(vd!(parse), OP_COPY as i32, reg_peer, s.start.reg, n);
        add_op3(vd!(parse), OP_COPY as i32, reg_peer, s.current.reg, n);
        add_op3(vd!(parse), OP_COPY as i32, reg_peer, s.end.reg, n);
    }

    add_op2(vd!(parse), OP_GOTO as i32, 0, lbl_where_end);

    jump_here(vd!(parse), addr_ne);

    // Começo do bloco executado da segunda linha em diante.
    if reg_peer != 0 {
        window_if_new_peer(
            db,
            parse,
            s.wins[0].w.p_order_by.as_deref(),
            reg_new_peer,
            reg_peer,
            lbl_where_end,
        );
    }
    if e_start == TK_FOLLOWING {
        window_code_op(db, parse, &mut s, WINDOW_AGGSTEP, 0, false);
        if e_end != TK_UNBOUNDED {
            if e_frm_type == TK_RANGE {
                let lbl = make_label(parse);
                let addr_next = current_addr(parse);
                let (c, e) = (s.current.csr, s.end.csr);
                window_code_range_test(db, parse, &s, OPI_GE, c, reg_end, e, lbl);
                window_code_op(db, parse, &mut s, WINDOW_AGGINVERSE, reg_start, false);
                window_code_op(db, parse, &mut s, WINDOW_RETURN_ROW, 0, false);
                add_op2(vd!(parse), OP_GOTO as i32, 0, addr_next);
                resolve_label(parse, db, lbl);
            } else {
                window_code_op(db, parse, &mut s, WINDOW_RETURN_ROW, reg_end, false);
                window_code_op(db, parse, &mut s, WINDOW_AGGINVERSE, reg_start, false);
            }
        }
    } else if e_end == TK_PRECEDING {
        let b_rps = e_start == TK_PRECEDING && e_frm_type == TK_RANGE;
        window_code_op(db, parse, &mut s, WINDOW_AGGSTEP, reg_end, false);
        if b_rps {
            window_code_op(db, parse, &mut s, WINDOW_AGGINVERSE, reg_start, false);
        }
        window_code_op(db, parse, &mut s, WINDOW_RETURN_ROW, 0, false);
        if !b_rps {
            window_code_op(db, parse, &mut s, WINDOW_AGGINVERSE, reg_start, false);
        }
    } else {
        let mut addr = 0;
        window_code_op(db, parse, &mut s, WINDOW_AGGSTEP, 0, false);
        if e_end != TK_UNBOUNDED {
            if e_frm_type == TK_RANGE {
                let mut lbl = 0;
                addr = current_addr(parse);
                if reg_end != 0 {
                    lbl = make_label(parse);
                    let (c, e) = (s.current.csr, s.end.csr);
                    window_code_range_test(db, parse, &s, OPI_GE, c, reg_end, e, lbl);
                }
                window_code_op(db, parse, &mut s, WINDOW_RETURN_ROW, 0, false);
                window_code_op(db, parse, &mut s, WINDOW_AGGINVERSE, reg_start, false);
                if reg_end != 0 {
                    add_op2(vd!(parse), OP_GOTO as i32, 0, addr);
                    resolve_label(parse, db, lbl);
                }
            } else {
                if reg_end != 0 {
                    addr = add_op3(vd!(parse), OP_IFPOS as i32, reg_end, 0, 1);
                }
                window_code_op(db, parse, &mut s, WINDOW_RETURN_ROW, 0, false);
                window_code_op(db, parse, &mut s, WINDOW_AGGINVERSE, reg_start, false);
                if reg_end != 0 {
                    jump_here(vd!(parse), addr);
                }
            }
        }
    }

    // Fim do laço principal de entrada.
    resolve_label(parse, db, lbl_where_end);
    where_end(db, parse, p.p_src.as_deref().expect("pSrc"), Box::new(wi));

    // Segue em frente.
    let mut addr_integer = 0;
    if has_partition {
        addr_integer = add_op2(vd!(parse), OP_INTEGER as i32, 0, reg_flush_part);
        jump_here(vd!(parse), addr_gosub_flush);
    }

    s.reg_rowid = 0;
    let addr_empty = add_op1(vd!(parse), OP_REWIND as i32, csr_write);
    if e_end == TK_PRECEDING {
        let b_rps = e_start == TK_PRECEDING && e_frm_type == TK_RANGE;
        window_code_op(db, parse, &mut s, WINDOW_AGGSTEP, reg_end, false);
        if b_rps {
            window_code_op(db, parse, &mut s, WINDOW_AGGINVERSE, reg_start, false);
        }
        window_code_op(db, parse, &mut s, WINDOW_RETURN_ROW, 0, false);
    } else if e_start == TK_FOLLOWING {
        window_code_op(db, parse, &mut s, WINDOW_AGGSTEP, 0, false);
        let (addr_start, addr_break1, addr_break2);
        if e_frm_type == TK_RANGE {
            addr_start = current_addr(parse);
            addr_break2 = window_code_op(db, parse, &mut s, WINDOW_AGGINVERSE, reg_start, true);
            addr_break1 = window_code_op(db, parse, &mut s, WINDOW_RETURN_ROW, 0, true);
        } else if e_end == TK_UNBOUNDED {
            addr_start = current_addr(parse);
            addr_break1 = window_code_op(db, parse, &mut s, WINDOW_RETURN_ROW, reg_start, true);
            addr_break2 = window_code_op(db, parse, &mut s, WINDOW_AGGINVERSE, 0, true);
        } else {
            debug_assert!(e_end == TK_FOLLOWING);
            addr_start = current_addr(parse);
            addr_break1 = window_code_op(db, parse, &mut s, WINDOW_RETURN_ROW, reg_end, true);
            addr_break2 = window_code_op(db, parse, &mut s, WINDOW_AGGINVERSE, reg_start, true);
        }
        add_op2(vd!(parse), OP_GOTO as i32, 0, addr_start);
        jump_here(vd!(parse), addr_break2);
        let addr_start = current_addr(parse);
        let addr_break3 = window_code_op(db, parse, &mut s, WINDOW_RETURN_ROW, 0, true);
        add_op2(vd!(parse), OP_GOTO as i32, 0, addr_start);
        jump_here(vd!(parse), addr_break1);
        jump_here(vd!(parse), addr_break3);
    } else {
        window_code_op(db, parse, &mut s, WINDOW_AGGSTEP, 0, false);
        let addr_start = current_addr(parse);
        let addr_break = window_code_op(db, parse, &mut s, WINDOW_RETURN_ROW, 0, true);
        window_code_op(db, parse, &mut s, WINDOW_AGGINVERSE, reg_start, false);
        add_op2(vd!(parse), OP_GOTO as i32, 0, addr_start);
        jump_here(vd!(parse), addr_break);
    }
    jump_here(vd!(parse), addr_empty);

    add_op1(vd!(parse), OP_RESETSORTER as i32, s.current.csr);
    if has_partition {
        if reg_start_rowid != 0 {
            add_op2(vd!(parse), OP_INTEGER as i32, 1, reg_start_rowid);
            add_op2(vd!(parse), OP_INTEGER as i32, 0, reg_end_rowid);
        }
        let a = current_addr(parse);
        change_p1(vd!(parse), addr_integer, a);
        add_op1(vd!(parse), OP_RETURN as i32, reg_flush_part);
    }
}


