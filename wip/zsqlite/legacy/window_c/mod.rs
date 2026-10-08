// Mesclado das partes traduzidas de window_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

// Tradução de window.c, parte 0 (SQLite 3.46.1). SQLITE_OMIT_WINDOWFUNC não está definido no
// Debian 13, então o arquivo inteiro entra.
//
// REESCRITA DO SELECT
//
//   Todo SELECT com uma ou mais funções de janela na lista de seleção ou no ORDER BY é
//   transformado por window_rewrite(): FROM, WHERE, GROUP BY e HAVING vão para uma subconsulta
//   (sempre uma co-rotina, com a otimização de achatamento desligada) que entrega as linhas já
//   ordenadas pelo PARTITION BY e ORDER BY da janela; ORDER BY, LIMIT e OFFSET ficam na consulta
//   externa. Os terminais (referências a colunas e funções agregadas) das expressões da lista de
//   seleção e do ORDER BY são selecionados pela subconsulta. Funções que usam a mesma declaração
//   de janela compartilham uma única varredura; declarações diferentes geram subconsultas
//   aninhadas, para que cada função processe as linhas na ordem da sua própria janela.
//
// INTERFACE COM SELECT.C
//
//   select.c chama where_begin() para iterar sobre o resultado da subconsulta e depois
//   window_code_step() para processar as linhas e terminar a varredura com where_end(). Para cada
//   linha, window_code_step() gera código que invoca a sub-rotina (OP_Gosub) codificada por
//   select.c: os resultados das funções de janela ficam nos registradores Window.reg_result e os
//   terminais necessários na linha corrente da tabela temporária Window.i_eph_csr. Dependendo do
//   quadro e das funções, a partição inteira é guardada numa tabela temporária ou não; esse
//   detalhe fica encapsulado neste arquivo.
//
// FUNÇÕES DE JANELA EMBUTIDAS
//
//   row_number(), rank(), dense_rank(), percent_rank(), cume_dist(), ntile(N),
//   lead(expr [, offset [, default]]), lag(expr [, offset [, default]]), first_value(expr),
//   last_value(expr) e nth_value(expr, N), as mesmas do Postgres. Algumas usam a mesma API das
//   funções agregadas de janela, outras são implementadas direto em instruções da VDBE (ver
//   window_update() para o quadro forçado em cada uma). Os agregados min() e max() também são
//   implementados em instruções da VDBE quando o início do quadro não é UNBOUNDED PRECEDING.
//
// Modelo de memória do contexto de agregação: `sqlite3_aggregate_context(pCtx, n)` devolve um
// bloco zerado de `n` bytes, que o C interpreta como uma struct. Aqui o bloco é um valor tipado
// `T: Default + 'static` guardado no contexto: `api::aggregate_context::<T>(ctx, n_byte)` devolve
// `Option<&mut T>`, `None` quando `n_byte` é 0 e o contexto ainda não foi alocado (ou quando a
// alocação falha), e cria `T::default()` na primeira chamada com `n_byte > 0`.

/// Implementação da função de janela embutida row_number(). Pressupõe que o quadro foi forçado
/// para ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW.
pub(crate) fn row_number_step_func(p_ctx: &mut Sqlite3Context, _ap_arg: &[Sqlite3ValueRef]) {
    if let Some(p) = api::aggregate_context::<i64>(p_ctx, core::mem::size_of::<i64>()) {
        *p = p.wrapping_add(1);
    }
}

pub(crate) fn row_number_value_func(p_ctx: &mut Sqlite3Context) {
    let v: i64 = match api::aggregate_context::<i64>(p_ctx, core::mem::size_of::<i64>()) {
        Some(p) => *p,
        None => 0,
    };
    api::result_int64(p_ctx, v);
}

/// Tipo do objeto de contexto usado por rank(), dense_rank(), percent_rank() e cume_dist().
#[derive(Default, Clone)]
pub(crate) struct CallCount {
    pub n_value: i64,
    pub n_step: i64,
    pub n_total: i64,
}

/// Implementação da função de janela embutida dense_rank(). Pressupõe que o quadro foi definido
/// como RANGE BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW.
pub(crate) fn dense_rank_step_func(p_ctx: &mut Sqlite3Context, _ap_arg: &[Sqlite3ValueRef]) {
    if let Some(p) = api::aggregate_context::<CallCount>(p_ctx, core::mem::size_of::<CallCount>()) {
        p.n_step = 1;
    }
}

pub(crate) fn dense_rank_value_func(p_ctx: &mut Sqlite3Context) {
    let v: Option<i64> =
        match api::aggregate_context::<CallCount>(p_ctx, core::mem::size_of::<CallCount>()) {
            Some(p) => {
                if p.n_step != 0 {
                    p.n_value = p.n_value.wrapping_add(1);
                    p.n_step = 0;
                }
                Some(p.n_value)
            }
            None => None,
        };
    if let Some(v) = v {
        api::result_int64(p_ctx, v);
    }
}

/// Contexto da função de janela embutida nth_value(). Esta implementação é usada só no "modo
/// lento", quando a cláusula EXCLUDE não tem o valor padrão "NO OTHERS".
#[derive(Default, Clone)]
pub(crate) struct NthValueCtx {
    pub n_step: i64,
    pub p_value: Option<Sqlite3ValueRef>,
}

pub(crate) fn nth_value_step_func(p_ctx: &mut Sqlite3Context, ap_arg: &[Sqlite3ValueRef]) {
    // `error_out` e o ramo de falta de memória precisam do contexto, que fica emprestado
    // enquanto `p` vive: os dois viram sinalizadores tratados depois do bloco.
    let mut error_out = false;
    let mut nomem = false;
    if let Some(p) =
        api::aggregate_context::<NthValueCtx>(p_ctx, core::mem::size_of::<NthValueCtx>())
    {
        'body: {
            let i_val: i64;
            match api::value_numeric_type(&ap_arg[1]) {
                SQLITE_INTEGER => {
                    i_val = api::value_int64(&ap_arg[1]);
                }
                SQLITE_FLOAT => {
                    let f_val: f64 = api::value_double(&ap_arg[1]);
                    if ((f_val as i64) as f64) != f_val {
                        error_out = true;
                        break 'body;
                    }
                    i_val = f_val as i64;
                }
                _ => {
                    error_out = true;
                    break 'body;
                }
            }
            if i_val <= 0 {
                error_out = true;
                break 'body;
            }

            p.n_step = p.n_step.wrapping_add(1);
            if i_val == p.n_step {
                p.p_value = api::value_dup(&ap_arg[0]);
                if p.p_value.is_none() {
                    nomem = true;
                }
            }
        }
    }
    if error_out {
        // error_out:
        api::result_error(
            p_ctx,
            b"second argument to nth_value must be a positive integer",
            -1,
        );
    } else if nomem {
        api::result_error_nomem(p_ctx);
    }
}

pub(crate) fn nth_value_finalize_func(p_ctx: &mut Sqlite3Context) {
    let v: Option<Sqlite3ValueRef> = match api::aggregate_context::<NthValueCtx>(p_ctx, 0) {
        Some(p) => {
            if p.p_value.is_some() {
                p.p_value.take()
            } else {
                None
            }
        }
        None => None,
    };
    if let Some(v) = v {
        api::result_value(p_ctx, &v);
        api::value_free(Some(v));
    }
}

pub(super) use super::noop_step_func as nth_value_inv_func;
pub(super) use super::noop_value_func as nth_value_value_func;

pub(crate) fn first_value_step_func(p_ctx: &mut Sqlite3Context, ap_arg: &[Sqlite3ValueRef]) {
    let mut nomem = false;
    if let Some(p) =
        api::aggregate_context::<NthValueCtx>(p_ctx, core::mem::size_of::<NthValueCtx>())
    {
        if p.p_value.is_none() {
            p.p_value = api::value_dup(&ap_arg[0]);
            if p.p_value.is_none() {
                nomem = true;
            }
        }
    }
    if nomem {
        api::result_error_nomem(p_ctx);
    }
}

pub(crate) fn first_value_finalize_func(p_ctx: &mut Sqlite3Context) {
    let v: Option<Sqlite3ValueRef> =
        match api::aggregate_context::<NthValueCtx>(p_ctx, core::mem::size_of::<NthValueCtx>()) {
            Some(p) => {
                if p.p_value.is_some() {
                    p.p_value.take()
                } else {
                    None
                }
            }
            None => None,
        };
    if let Some(v) = v {
        api::result_value(p_ctx, &v);
        api::value_free(Some(v));
    }
}

pub(super) use super::noop_step_func as first_value_inv_func;
pub(super) use super::noop_value_func as first_value_value_func;

/// Implementação da função de janela embutida rank(). Pressupõe que o quadro foi definido como
/// RANGE BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW.
pub(crate) fn rank_step_func(p_ctx: &mut Sqlite3Context, _ap_arg: &[Sqlite3ValueRef]) {
    if let Some(p) = api::aggregate_context::<CallCount>(p_ctx, core::mem::size_of::<CallCount>()) {
        p.n_step = p.n_step.wrapping_add(1);
        if p.n_value == 0 {
            p.n_value = p.n_step;
        }
    }
}

pub(crate) fn rank_value_func(p_ctx: &mut Sqlite3Context) {
    let v: Option<i64> =
        match api::aggregate_context::<CallCount>(p_ctx, core::mem::size_of::<CallCount>()) {
            Some(p) => {
                let v = p.n_value;
                p.n_value = 0;
                Some(v)
            }
            None => None,
        };
    if let Some(v) = v {
        api::result_int64(p_ctx, v);
    }
}

/// Implementação da função de janela embutida percent_rank(). Pressupõe que o quadro foi definido
/// como GROUPS BETWEEN CURRENT ROW AND UNBOUNDED FOLLOWING.
pub(crate) fn percent_rank_step_func(p_ctx: &mut Sqlite3Context, _ap_arg: &[Sqlite3ValueRef]) {
    if let Some(p) = api::aggregate_context::<CallCount>(p_ctx, core::mem::size_of::<CallCount>()) {
        p.n_total = p.n_total.wrapping_add(1);
    }
}

pub(crate) fn percent_rank_inv_func(p_ctx: &mut Sqlite3Context, _ap_arg: &[Sqlite3ValueRef]) {
    if let Some(p) = api::aggregate_context::<CallCount>(p_ctx, core::mem::size_of::<CallCount>()) {
        p.n_step = p.n_step.wrapping_add(1);
    }
}


// ---- part_001.rs ----

// Tradução de window.c, parte 1 (SQLite 3.46.1). Continua as funções de janela embutidas
// (percent_rank, cume_dist, ntile, last_value), os nomes estáticos, o registro das funções e
// window_update(). O modelo do contexto de agregação é o descrito na parte 0:
// `api::aggregate_context::<T>(ctx, n_byte)` devolve `Option<&mut T>`.

/// Valor corrente de percent_rank() e, pelo `#define` do C, também o finalizador.
pub(crate) fn percent_rank_value_func(p_ctx: &mut Sqlite3Context) {
    let r: Option<f64> =
        match api::aggregate_context::<CallCount>(p_ctx, core::mem::size_of::<CallCount>()) {
            Some(p) => {
                p.n_value = p.n_step;
                if p.n_total > 1 {
                    Some((p.n_value as f64) / (p.n_total.wrapping_sub(1) as f64))
                } else {
                    Some(0.0)
                }
            }
            None => None,
        };
    if let Some(r) = r {
        api::result_double(p_ctx, r);
    }
}
// `#define percent_rankFinalizeFunc percent_rankValueFunc`
pub(super) use self::percent_rank_value_func as percent_rank_finalize_func;

/// Implementação da função de janela embutida cume_dist(). Pressupõe que o quadro foi definido
/// como:
///
///   GROUPS BETWEEN 1 FOLLOWING AND UNBOUNDED FOLLOWING
pub(crate) fn cume_dist_step_func(p_ctx: &mut Sqlite3Context, _ap_arg: &[Sqlite3ValueRef]) {
    if let Some(p) = api::aggregate_context::<CallCount>(p_ctx, core::mem::size_of::<CallCount>()) {
        p.n_total = p.n_total.wrapping_add(1);
    }
}

pub(crate) fn cume_dist_inv_func(p_ctx: &mut Sqlite3Context, _ap_arg: &[Sqlite3ValueRef]) {
    if let Some(p) = api::aggregate_context::<CallCount>(p_ctx, core::mem::size_of::<CallCount>()) {
        p.n_step = p.n_step.wrapping_add(1);
    }
}

pub(crate) fn cume_dist_value_func(p_ctx: &mut Sqlite3Context) {
    let r: Option<f64> = match api::aggregate_context::<CallCount>(p_ctx, 0) {
        Some(p) => Some((p.n_step as f64) / (p.n_total as f64)),
        None => None,
    };
    if let Some(r) = r {
        api::result_double(p_ctx, r);
    }
}
// `#define cume_distFinalizeFunc cume_distValueFunc`
pub(super) use self::cume_dist_value_func as cume_dist_finalize_func;

/// Objeto de contexto da função de janela ntile().
#[derive(Default, Clone)]
pub(crate) struct NtileCtx {
    /// Total de linhas na partição.
    pub n_total: i64,
    /// Parâmetro passado a ntile(N).
    pub n_param: i64,
    /// Linha corrente.
    pub i_row: i64,
}

/// Implementação de ntile(). Pressupõe que o quadro foi forçado para:
///
///   ROWS CURRENT ROW AND UNBOUNDED FOLLOWING
pub(crate) fn ntile_step_func(p_ctx: &mut Sqlite3Context, ap_arg: &[Sqlite3ValueRef]) {
    // O erro precisa do contexto, que fica emprestado enquanto `p` vive: vira sinalizador.
    let mut bad_param = false;
    if let Some(p) = api::aggregate_context::<NtileCtx>(p_ctx, core::mem::size_of::<NtileCtx>()) {
        if p.n_total == 0 {
            p.n_param = api::value_int64(&ap_arg[0]);
            if p.n_param <= 0 {
                bad_param = true;
            }
        }
        p.n_total = p.n_total.wrapping_add(1);
    }
    if bad_param {
        api::result_error(p_ctx, b"argument of ntile must be a positive integer", -1);
    }
}

pub(crate) fn ntile_inv_func(p_ctx: &mut Sqlite3Context, _ap_arg: &[Sqlite3ValueRef]) {
    if let Some(p) = api::aggregate_context::<NtileCtx>(p_ctx, core::mem::size_of::<NtileCtx>()) {
        p.i_row = p.i_row.wrapping_add(1);
    }
}

pub(crate) fn ntile_value_func(p_ctx: &mut Sqlite3Context) {
    let r: Option<i64> =
        match api::aggregate_context::<NtileCtx>(p_ctx, core::mem::size_of::<NtileCtx>()) {
            Some(p) if p.n_param > 0 => {
                // `int nSize` no C: o quociente é truncado para 32 bits.
                let n_size: i32 = (p.n_total / p.n_param) as i32;
                if n_size == 0 {
                    Some(p.i_row.wrapping_add(1))
                } else {
                    let n_size64: i64 = n_size as i64;
                    let n_size_plus1: i64 = n_size.wrapping_add(1) as i64;
                    let n_large: i64 = p.n_total.wrapping_sub(p.n_param.wrapping_mul(n_size64));
                    let i_small: i64 = n_large.wrapping_mul(n_size_plus1);
                    let i_row: i64 = p.i_row;

                    if i_row < i_small {
                        Some(1i64.wrapping_add(i_row / n_size_plus1))
                    } else {
                        Some(
                            1i64.wrapping_add(n_large)
                                .wrapping_add((i_row.wrapping_sub(i_small)) / n_size64),
                        )
                    }
                }
            }
            _ => None,
        };
    if let Some(r) = r {
        api::result_int64(p_ctx, r);
    }
}
// `#define ntileFinalizeFunc ntileValueFunc`
pub(super) use self::ntile_value_func as ntile_finalize_func;

/// Objeto de contexto da função de janela last_value().
#[derive(Default, Clone)]
pub(crate) struct LastValueCtx {
    pub p_val: Option<Sqlite3ValueRef>,
    pub n_val: i32,
}

/// Implementação de last_value().
pub(crate) fn last_value_step_func(p_ctx: &mut Sqlite3Context, ap_arg: &[Sqlite3ValueRef]) {
    let mut nomem = false;
    if let Some(p) =
        api::aggregate_context::<LastValueCtx>(p_ctx, core::mem::size_of::<LastValueCtx>())
    {
        api::value_free(p.p_val.take());
        p.p_val = api::value_dup(&ap_arg[0]);
        if p.p_val.is_none() {
            nomem = true;
        } else {
            p.n_val = p.n_val.wrapping_add(1);
        }
    }
    if nomem {
        api::result_error_nomem(p_ctx);
    }
}

pub(crate) fn last_value_inv_func(p_ctx: &mut Sqlite3Context, _ap_arg: &[Sqlite3ValueRef]) {
    if let Some(p) =
        api::aggregate_context::<LastValueCtx>(p_ctx, core::mem::size_of::<LastValueCtx>())
    {
        p.n_val = p.n_val.wrapping_sub(1);
        if p.n_val == 0 {
            api::value_free(p.p_val.take());
        }
    }
}

pub(crate) fn last_value_value_func(p_ctx: &mut Sqlite3Context) {
    let v: Option<Sqlite3ValueRef> = match api::aggregate_context::<LastValueCtx>(p_ctx, 0) {
        Some(p) => p.p_val.clone(),
        None => None,
    };
    if let Some(v) = v {
        api::result_value(p_ctx, &v);
    }
}

pub(crate) fn last_value_finalize_func(p_ctx: &mut Sqlite3Context) {
    let v: Option<Sqlite3ValueRef> =
        match api::aggregate_context::<LastValueCtx>(p_ctx, core::mem::size_of::<LastValueCtx>()) {
            Some(p) => p.p_val.take(),
            None => None,
        };
    if let Some(v) = v {
        api::result_value(p_ctx, &v);
        api::value_free(Some(v));
    }
}

// Nomes estáticos das funções de janela embutidas. No C o ponteiro de `zName` é comparado por
// identidade (`pFunc->zName==row_valueName`); aqui os `FuncDef` embutidos são reconhecidos pelo
// conteúdo do nome junto com SQLITE_FUNC_BUILTIN (ver window_update()).
pub(crate) const ROW_NUMBER_NAME: &[u8] = b"row_number";
pub(crate) const DENSE_RANK_NAME: &[u8] = b"dense_rank";
pub(crate) const RANK_NAME: &[u8] = b"rank";
pub(crate) const PERCENT_RANK_NAME: &[u8] = b"percent_rank";
pub(crate) const CUME_DIST_NAME: &[u8] = b"cume_dist";
pub(crate) const NTILE_NAME: &[u8] = b"ntile";
pub(crate) const LAST_VALUE_NAME: &[u8] = b"last_value";
pub(crate) const NTH_VALUE_NAME: &[u8] = b"nth_value";
pub(crate) const FIRST_VALUE_NAME: &[u8] = b"first_value";
pub(crate) const LEAD_NAME: &[u8] = b"lead";
pub(crate) const LAG_NAME: &[u8] = b"lag";

/// Implementações vazias de xStep() e xFinalize(). Servem de substitutas para as funções de
/// janela embutidas que nunca chamam essas interfaces. A noop_value_func() é chamada mas deve
/// não fazer nada; a noop_step_func() nunca é chamada.
pub(crate) fn noop_step_func(_p: &mut Sqlite3Context, _a: &[Sqlite3ValueRef]) {}

pub(crate) fn noop_value_func(_p: &mut Sqlite3Context) {}

/// `WINDOWFUNCALL`: função de janela que usa todas as interfaces (xStep, xFinal, xValue e
/// xInverse). As demais macros `WINDOWFUNC*` do C passam pela mesma construção.
pub(crate) fn windowfuncall(
    n_arg: i8,
    extra: u32,
    x_step: XSFunc,
    x_final: XFinalFunc,
    x_value: XFinalFunc,
    x_inverse: XSFunc,
    z_name: &[u8],
) -> FuncDef {
    FuncDef {
        n_arg,
        func_flags: SQLITE_FUNC_BUILTIN | (SQLITE_UTF8 as u32) | SQLITE_FUNC_WINDOW | extra,
        p_user_data: None,
        p_next: None,
        x_s_func: Some(x_step),
        x_finalize: Some(x_final),
        x_value: Some(x_value),
        x_inverse: Some(x_inverse),
        z_name: z_name.to_vec(),
        u: FuncDefU::PHash(None),
    }
}

/// `WINDOWFUNCNOOP`: função de janela implementada em bytecode, que por isso tem rotinas vazias
/// para os métodos.
pub(crate) fn windowfuncnoop(n_arg: i8, extra: u32, z_name: &[u8]) -> FuncDef {
    windowfuncall(
        n_arg,
        extra,
        Rc::new(noop_step_func),
        Rc::new(noop_value_func),
        Rc::new(noop_value_func),
        Rc::new(noop_step_func),
        z_name,
    )
}

/// `WINDOWFUNCX`: função de janela com xStep, a mesma rotina para xFinalize e xValue, e que
/// nunca chama xInverse.
pub(crate) fn windowfuncx(
    n_arg: i8,
    extra: u32,
    x_step: XSFunc,
    x_value: XFinalFunc,
    z_name: &[u8],
) -> FuncDef {
    windowfuncall(
        n_arg,
        extra,
        x_step,
        x_value.clone(),
        x_value,
        Rc::new(noop_step_func),
        z_name,
    )
}

/// Registra as funções de janela embutidas que não são também agregados.
pub fn window_functions() {
    let a_window_funcs: Vec<FuncDef> = vec![
        windowfuncx(0, 0, Rc::new(row_number_step_func), Rc::new(row_number_value_func), ROW_NUMBER_NAME),
        windowfuncx(0, 0, Rc::new(dense_rank_step_func), Rc::new(dense_rank_value_func), DENSE_RANK_NAME),
        windowfuncx(0, 0, Rc::new(rank_step_func), Rc::new(rank_value_func), RANK_NAME),
        windowfuncall(
            0,
            0,
            Rc::new(percent_rank_step_func),
            Rc::new(percent_rank_finalize_func),
            Rc::new(percent_rank_value_func),
            Rc::new(percent_rank_inv_func),
            PERCENT_RANK_NAME,
        ),
        windowfuncall(
            0,
            0,
            Rc::new(cume_dist_step_func),
            Rc::new(cume_dist_finalize_func),
            Rc::new(cume_dist_value_func),
            Rc::new(cume_dist_inv_func),
            CUME_DIST_NAME,
        ),
        windowfuncall(
            1,
            0,
            Rc::new(ntile_step_func),
            Rc::new(ntile_finalize_func),
            Rc::new(ntile_value_func),
            Rc::new(ntile_inv_func),
            NTILE_NAME,
        ),
        windowfuncall(
            1,
            0,
            Rc::new(last_value_step_func),
            Rc::new(last_value_finalize_func),
            Rc::new(last_value_value_func),
            Rc::new(last_value_inv_func),
            LAST_VALUE_NAME,
        ),
        windowfuncall(
            2,
            0,
            Rc::new(nth_value_step_func),
            Rc::new(nth_value_finalize_func),
            Rc::new(nth_value_value_func),
            Rc::new(nth_value_inv_func),
            NTH_VALUE_NAME,
        ),
        windowfuncall(
            1,
            0,
            Rc::new(first_value_step_func),
            Rc::new(first_value_finalize_func),
            Rc::new(first_value_value_func),
            Rc::new(first_value_inv_func),
            FIRST_VALUE_NAME,
        ),
        windowfuncnoop(1, 0, LEAD_NAME),
        windowfuncnoop(2, 0, LEAD_NAME),
        windowfuncnoop(3, 0, LEAD_NAME),
        windowfuncnoop(1, 0, LAG_NAME),
        windowfuncnoop(2, 0, LAG_NAME),
        windowfuncnoop(3, 0, LAG_NAME),
    ];
    insert_builtin_funcs(&a_window_funcs);
}

/// Procura na lista `p_list` a definição WINDOW de nome `z_name`; sem achar, deixa o erro
/// "no such window" em `p_parse`.
fn window_find<'a>(
    p_parse: &mut Parse,
    p_list: Option<&'a Window>,
    z_name: &[u8],
) -> Option<&'a Window> {
    let mut p: Option<&'a Window> = p_list;
    while let Some(w) = p {
        if str_i_cmp(w.z_name.as_deref().unwrap_or(b""), z_name) == 0 {
            break;
        }
        p = w.p_next_win.as_deref();
    }
    if p.is_none() {
        let mut z_msg: Vec<u8> = b"no such window: ".to_vec();
        // O nome vai pelo %s do formato: o '%' do nome é escapado (como em window_c/part_002).
        for &c in z_name.iter() {
            if c == b'%' {
                z_msg.push(b'%');
            }
            z_msg.push(c);
        }
        error_msg(p_parse, Some(&z_msg));
    }
    p
}

/// Esta função é chamada logo depois de resolver o nome da função de janela dentro de um SELECT.
/// O argumento `p_list` é a lista ligada de definições WINDOW do SELECT corrente. `p_func` é a
/// definição de função recém resolvida e `p_win` o objeto Window da cláusula OVER associada.
/// Esta função atualiza `p_win` assim:
///
///   * Se a cláusula OVER se referia a uma janela nomeada (como em "max(x) OVER win"), procura em
///     `p_list` a definição WINDOW correspondente e atualiza `p_win` de acordo. Sem achar, deixa
///     um erro em `p_parse`.
///
///   * Se a função é uma função de janela embutida que exige que a janela seja forçada (ver
///     "FUNÇÕES DE JANELA EMBUTIDAS" no começo do arquivo), `p_win` é atualizada aqui.
pub fn window_update(
    p_parse: &mut Parse,
    p_list: Option<&Window>,
    p_win: &mut Window,
    p_func: &Rc<FuncDef>,
) {
    let db = p_parse.db.upgrade().expect("conexão do Parse");
    if p_win.z_name.is_some() && p_win.e_frm_type == 0 {
        let z_win_name: Vec<u8> = p_win.z_name.clone().unwrap_or_default();
        let p = match window_find(p_parse, p_list, &z_win_name) {
            Some(p) => p,
            None => return,
        };
        p_win.p_partition = expr_list_dup(&db, p.p_partition.as_deref(), 0);
        p_win.p_order_by = expr_list_dup(&db, p.p_order_by.as_deref(), 0);
        p_win.p_start = expr_dup(&db, p.p_start.as_deref(), 0);
        p_win.p_end = expr_dup(&db, p.p_end.as_deref(), 0);
        p_win.e_start = p.e_start;
        p_win.e_end = p.e_end;
        p_win.e_frm_type = p.e_frm_type;
        p_win.e_exclude = p.e_exclude;
    } else {
        window_chain(p_parse, p_win, p_list);
    }
    if (p_win.e_frm_type == TK_RANGE)
        && (p_win.p_start.is_some() || p_win.p_end.is_some())
        && (p_win.p_order_by.is_none()
            || p_win.p_order_by.as_ref().map_or(true, |o| o.n_expr != 1))
    {
        error_msg(
            p_parse,
            Some(b"RANGE with offset PRECEDING/FOLLOWING requires one ORDER BY expression"),
        );
    } else if (p_func.func_flags & SQLITE_FUNC_WINDOW) != 0 {
        if p_win.p_filter.is_some() {
            error_msg(
                p_parse,
                Some(b"FILTER clause may only be used with aggregate window functions"),
            );
        } else {
            struct WindowUpdate {
                z_func: &'static [u8],
                e_frm_type: u8,
                e_start: u8,
                e_end: u8,
            }
            let a_up: [WindowUpdate; 8] = [
                WindowUpdate { z_func: ROW_NUMBER_NAME, e_frm_type: TK_ROWS, e_start: TK_UNBOUNDED, e_end: TK_CURRENT },
                WindowUpdate { z_func: DENSE_RANK_NAME, e_frm_type: TK_RANGE, e_start: TK_UNBOUNDED, e_end: TK_CURRENT },
                WindowUpdate { z_func: RANK_NAME, e_frm_type: TK_RANGE, e_start: TK_UNBOUNDED, e_end: TK_CURRENT },
                WindowUpdate { z_func: PERCENT_RANK_NAME, e_frm_type: TK_GROUPS, e_start: TK_CURRENT, e_end: TK_UNBOUNDED },
                WindowUpdate { z_func: CUME_DIST_NAME, e_frm_type: TK_GROUPS, e_start: TK_FOLLOWING, e_end: TK_UNBOUNDED },
                WindowUpdate { z_func: NTILE_NAME, e_frm_type: TK_ROWS, e_start: TK_CURRENT, e_end: TK_UNBOUNDED },
                WindowUpdate { z_func: LEAD_NAME, e_frm_type: TK_ROWS, e_start: TK_UNBOUNDED, e_end: TK_UNBOUNDED },
                WindowUpdate { z_func: LAG_NAME, e_frm_type: TK_ROWS, e_start: TK_UNBOUNDED, e_end: TK_CURRENT },
            ];
            for up in a_up.iter() {
                if (p_func.func_flags & SQLITE_FUNC_BUILTIN) != 0 && p_func.z_name == up.z_func {
                    expr_delete(&db, p_win.p_start.take());
                    expr_delete(&db, p_win.p_end.take());
                    p_win.p_end = None;
                    p_win.p_start = None;
                    p_win.e_frm_type = up.e_frm_type;
                    p_win.e_start = up.e_start;
                    p_win.e_end = up.e_end;
                    p_win.e_exclude = 0;
                    if p_win.e_start == TK_FOLLOWING {
                        p_win.p_start = expr(&db, TK_INTEGER as i32, Some(b"1"));
                    }
                    break;
                }
            }
        }
    }
    p_win.p_w_func = Some(Rc::clone(p_func));
}


// ---- part_002.rs ----

// Notas de modelo para o tech lead (tudo o que o C resolve com ponteiros e aqui precisa de
// decisão na integração):
//
// * Identidade de Window e de Select: o C compara ponteiros (`pExpr->y.pWin==pWin`,
//   `pSave==pSelect`). Aqui a identidade é o endereço do objeto (`as usize`), que vale tanto
//   para `Box<Window>` da lista `Select.p_win` quanto para o `WindowRef` de `Expr.y.p_win`,
//   desde que a integração faça os dois apontarem para o mesmo objeto.
// * `expr_skip_collate_and_likely` é usada com a assinatura `fn(Option<&mut Expr>) ->
//   Option<&mut Expr>` (o C devolve um ponteiro para dentro da árvore e o chamador edita o nó).
// * A tabela zerada (`sqlite3DbMallocZero(db, sizeof(Table))`) e o `Expr` zerado do
//   `memset` são montados campo a campo, sem depender de `Default` em `Table` e `Expr`.
// * O `memcpy(pTab, pTab2, ...)` vira a movimentação do conteúdo de `pTab2` para a tabela
//   `p_tab` (que as expressões reescritas já referenciam); o adiamento da liberação vira um
//   `parser_add_cleanup` que segura a referência até o fim da análise.
// * Os callbacks do Walker que vêm de walker_c (`walker_depth_increase`/`_decrease`) recebem
//   `&Select`; o campo do Walker espera `&mut Select`. A integração uniformiza a assinatura.

/// Objeto de contexto passado por `walk_expr_list()` para `select_window_rewrite_expr_cb()`
/// por `select_window_rewrite_e_list()`.
///
/// No C os campos são ponteiros para objetos que continuam pertencendo ao chamador. Como o
/// Walker guarda este objeto por valor (`WalkerU::Rewrite`), os campos são instantâneos:
/// `p_win` tem o endereço (identidade) de cada Window da lista, `i_eph_csr` é o `iEphCsr` da
/// primeira, `p_src` tem o `iCursor` de cada item da cláusula FROM (`nSrc` é o tamanho).
pub struct WindowRewrite {
    /// Identidade de cada Window da lista (a primeira é a janela principal).
    pub p_win: Vec<usize>,
    /// `pWin->iEphCsr` da janela principal.
    pub i_eph_csr: i32,
    /// Cursores (`a[i].iCursor`) dos itens da cláusula FROM.
    pub p_src: Vec<i32>,
    /// Lista de expressões da sub-consulta, in/out.
    pub p_sub: Option<Box<ExprList>>,
    /// A tabela que as colunas reescritas vão referenciar.
    pub p_tab: Option<TableRef>,
    /// Identidade do sub-select corrente, se houver.
    pub p_sub_select: Option<usize>,
}

/// Walker zerado (o `memset(&sWalker, 0, sizeof(Walker))` do C).
fn walker_zeroed(p_parse: Option<ParseRef>) -> Walker {
    Walker {
        p_parse,
        x_expr_callback: None,
        x_select_callback: None,
        x_select_callback2: None,
        walker_depth: 0,
        e_code: 0,
        m_w_flags: 0,
        u: WalkerU::None,
    }
}

/// Callback usado por `select_window_rewrite_e_list()`. Se necessário, anexa à lista de
/// expressões de saída e atualiza a expressão `p_expr` no lugar.
fn select_window_rewrite_expr_cb(p_walker: &mut Walker, p_expr: &mut Expr) -> i32 {
    let p_parse = match &p_walker.p_parse {
        Some(p) => p.clone(),
        None => return WRC_CONTINUE,
    };
    let p = match &mut p_walker.u {
        WalkerU::Rewrite(p) => p,
        _ => return WRC_CONTINUE,
    };
    let db = p_parse
        .borrow()
        .db
        .upgrade()
        .expect("a conexão precisa estar viva enquanto o Parse existe");

    // Se esta função é chamada de dentro de um sub-select escalar usado pelo SELECT em
    // processamento, só processa as expressões TK_COLUMN que se referem a ele (o SELECT
    // externo). Agregadas e funções de janela não são processadas, pois pertencem ao
    // sub-select escalar.
    if p.p_sub_select.is_some() {
        if p_expr.op != TK_COLUMN {
            return WRC_CONTINUE;
        } else {
            let n_src = p.p_src.len();
            let mut i = 0;
            while i < n_src {
                if p_expr.i_table == p.p_src[i] {
                    break;
                }
                i += 1;
            }
            if i == n_src {
                return WRC_CONTINUE;
            }
        }
    }

    // `case TK_FUNCTION` cai em `case TK_IF_NULL_ROW/TK_AGG_FUNCTION/TK_COLUMN`.
    let mut common = false;
    match p_expr.op {
        TK_FUNCTION => {
            if !expr_has_property(p_expr, EP_WIN_FUNC) {
                // break
            } else {
                if let Some(w) = &p_expr.y.p_win {
                    let id = w.as_ptr() as usize;
                    for win_id in p.p_win.iter() {
                        if id == *win_id {
                            return WRC_PRUNE;
                        }
                    }
                }
                // no break: deliberate_fall_through
                common = true;
            }
        }
        TK_IF_NULL_ROW | TK_AGG_FUNCTION | TK_COLUMN => {
            common = true;
        }
        _ => {} // no-op
    }

    if common {
        let mut i_col: i32 = -1;
        if db.borrow().malloc_failed != 0 {
            return WRC_ABORT;
        }
        if let Some(p_sub) = &p.p_sub {
            let mut i = 0;
            while i < p_sub.n_expr {
                if 0 == expr_compare(None, p_sub.a[i as usize].p_expr.as_deref(), Some(&*p_expr), -1) {
                    i_col = i;
                    break;
                }
                i += 1;
            }
        }
        if i_col < 0 {
            let mut p_dup = expr_dup(&db, Some(&*p_expr), 0);
            if let Some(d) = p_dup.as_mut() {
                if d.op == TK_AGG_FUNCTION {
                    d.op = TK_FUNCTION;
                }
            }
            p.p_sub = expr_list_append(&p_parse, p.p_sub.take(), p_dup);
        }
        if let Some(p_sub) = &p.p_sub {
            let f = p_expr.flags & EP_COLLATE;
            // Libera o conteúdo do nó mantendo o próprio nó (EP_Static), depois zera.
            expr_set_property(p_expr, EP_STATIC);
            let zeroed = Expr {
                op: 0,
                aff_expr: 0,
                op2: 0,
                flags: 0,
                u: ExprU::default(),
                p_left: None,
                p_right: None,
                x: ExprX::default(),
                n_height: 0,
                i_table: 0,
                i_column: 0,
                i_agg: 0,
                w: ExprW::default(),
                p_agg_info: None,
                y: ExprY::default(),
            };
            let old = std::mem::replace(p_expr, zeroed);
            expr_delete(&db, Some(Box::new(old)));

            p_expr.op = TK_COLUMN;
            p_expr.i_column = (if i_col < 0 { p_sub.n_expr - 1 } else { i_col }) as YnVar;
            p_expr.i_table = p.i_eph_csr;
            p_expr.y.p_tab = p.p_tab.clone();
            p_expr.flags = f;
        }
        if db.borrow().malloc_failed != 0 {
            return WRC_ABORT;
        }
    }

    WRC_CONTINUE
}

fn select_window_rewrite_select_cb(p_walker: &mut Walker, p_select: &mut Select) -> i32 {
    let id = &*p_select as *const Select as usize;
    let p_save = match &p_walker.u {
        WalkerU::Rewrite(p) => p.p_sub_select,
        _ => return WRC_CONTINUE,
    };
    if p_save == Some(id) {
        return WRC_CONTINUE;
    } else {
        if let WalkerU::Rewrite(p) = &mut p_walker.u {
            p.p_sub_select = Some(id);
        }
        walk_select(p_walker, p_select);
        if let WalkerU::Rewrite(p) = &mut p_walker.u {
            p.p_sub_select = p_save;
        }
    }
    WRC_PRUNE
}

/// Percorre cada expressão da lista `p_e_list`. Para cada uma:
///
///   * TK_COLUMN,
///   * função de agregação, ou
///   * função de janela com um objeto Window que não é membro da lista de janelas
///     (`p_win_ids`, a identidade de cada Window da lista, com `i_eph_csr` da primeira),
///
/// anexa o nó à lista de expressões de saída (`pp_sub`) e o substitui por um TK_COLUMN que lê
/// o (N-1)-ésimo elemento da tabela `i_eph_csr`, onde N é o número de elementos de `pp_sub`
/// depois de anexar o novo.
fn select_window_rewrite_e_list(
    p_parse: &ParseRef,
    p_win_ids: &[usize],
    i_eph_csr: i32,
    p_src: &SrcList,
    p_e_list: Option<&mut ExprList>, // Reescreve as expressões desta lista
    p_tab: &TableRef,
    pp_sub: &mut Option<Box<ExprList>>, // IN/OUT: lista de expressões do sub-select
) {
    let mut s_walker = walker_zeroed(Some(p_parse.clone()));

    let s_rewrite = WindowRewrite {
        p_win: p_win_ids.to_vec(),
        i_eph_csr,
        p_src: p_src.a.iter().take(p_src.n_src as usize).map(|it| it.i_cursor).collect(),
        p_sub: pp_sub.take(),
        p_tab: Some(p_tab.clone()),
        p_sub_select: None,
    };

    s_walker.x_expr_callback = Some(select_window_rewrite_expr_cb);
    s_walker.x_select_callback = Some(select_window_rewrite_select_cb);
    s_walker.u = WalkerU::Rewrite(Box::new(s_rewrite));

    if let Some(l) = p_e_list {
        let _ = walk_expr_list(&mut s_walker, l);
    }

    if let WalkerU::Rewrite(r) = s_walker.u {
        *pp_sub = r.p_sub;
    }
}

/// Anexa uma cópia de cada expressão da lista `p_append` à lista `p_list`. Devolve o
/// resultado.
fn expr_list_append_list(
    p_parse: &ParseRef,        // Contexto de análise
    mut p_list: Option<Box<ExprList>>, // Lista à qual anexar. Pode ser None
    p_append: Option<&ExprList>, // Lista de valores a anexar. Pode ser None
    b_int_to_null: i32,
) -> Option<Box<ExprList>> {
    if let Some(p_append) = p_append {
        let n_init = p_list.as_ref().map_or(0, |l| l.n_expr);
        let mut i = 0;
        while i < p_append.n_expr {
            let db = p_parse
                .borrow()
                .db
                .upgrade()
                .expect("a conexão precisa estar viva enquanto o Parse existe");
            let mut p_dup = expr_dup(&db, p_append.a[i as usize].p_expr.as_deref(), 0);
            if db.borrow().malloc_failed != 0 {
                expr_delete(&db, p_dup);
                break;
            }
            if b_int_to_null != 0 {
                let mut i_dummy: i32 = 0;
                if let Some(p_sub) = expr_skip_collate_and_likely(p_dup.as_deref_mut()) {
                    if expr_is_integer(p_sub, &mut i_dummy) != 0 {
                        p_sub.op = TK_NULL;
                        p_sub.flags &= !(EP_INT_VALUE | EP_IS_TRUE | EP_IS_FALSE);
                        p_sub.u.z_token = None;
                    }
                }
            }
            p_list = expr_list_append(p_parse, p_list, p_dup);
            if let Some(l) = p_list.as_mut() {
                l.a[(n_init + i) as usize].fg.sort_flags = p_append.a[i as usize].fg.sort_flags;
            }
            i += 1;
        }
    }
    p_list
}

/// Ao reescrever uma consulta, se a nova subconsulta na cláusula FROM contém nós
/// TK_AGG_FUNCTION que se referem a uma consulta externa, é preciso aumentar os valores
/// `Expr.op2` desses nós por causa da camada extra de subconsulta acrescentada.
///
/// Veja também `incr_agg_depth()` em resolve.c.
pub fn window_extra_agg_func_depth(p_walker: &mut Walker, p_expr: &mut Expr) -> i32 {
    if p_expr.op == TK_AGG_FUNCTION && (p_expr.op2 as i32) >= p_walker.walker_depth {
        p_expr.op2 = p_expr.op2.wrapping_add(1);
    }
    WRC_CONTINUE
}

fn disallow_aggregates_in_order_by_cb(p_walker: &mut Walker, p_expr: &mut Expr) -> i32 {
    if p_expr.op == TK_AGG_FUNCTION && p_expr.p_agg_info.is_none() {
        if let Some(p_parse) = &p_walker.p_parse {
            // O nome da função vai pelo %s do formato: o '%' do nome é escapado.
            let mut msg: Vec<u8> = b"misuse of aggregate: ".to_vec();
            if let Some(z) = &p_expr.u.z_token {
                for &c in z.iter() {
                    if c == b'%' {
                        msg.push(b'%');
                    }
                    msg.push(c);
                }
            }
            msg.extend_from_slice(b"()");
            error_msg(&mut p_parse.borrow_mut(), Some(&msg));
        }
    }
    WRC_CONTINUE
}

/// Se o SELECT passado como segundo argumento não invoca nenhuma função de janela SQL, esta
/// função não faz nada. Caso contrário, reescreve o SELECT para que as funções xStep das
/// funções de janela sejam invocadas na ordem correta, como descrito em "SELECT REWRITING" no
/// início deste arquivo.
pub fn window_rewrite(p_parse: &ParseRef, p: &mut Select) -> i32 {
    let mut rc = SQLITE_OK;
    if p.p_win.is_some()
        && p.p_prior.is_none()
        && (p.sel_flags & SF_WINREWRITE) == 0
        && !(p_parse.borrow().e_parse_mode >= PARSE_MODE_RENAME)
    {
        let v = get_vdbe(p_parse);
        let db = p_parse
            .borrow()
            .db
            .upgrade()
            .expect("a conexão precisa estar viva enquanto o Parse existe");

        let sel_flags = p.sel_flags;

        // sqlite3DbMallocZero(db, sizeof(Table)): falha se mallocFailed já está ligado.
        if db.borrow().malloc_failed != 0 {
            return error_to_parser(Some(&mut db.borrow_mut()), SQLITE_NOMEM);
        }
        // A tabela zerada; as expressões reescritas apontam para ela e o conteúdo é
        // preenchido mais abaixo.
        let p_tab: TableRef = Rc::new(RefCell::new(Table {
            z_name: Vec::new(),
            a_col: Vec::new(),
            p_index: None,
            z_col_aff: Vec::new(),
            p_check: None,
            tnum: 0,
            n_tab_ref: 0,
            tab_flags: 0,
            i_p_key: 0,
            n_col: 0,
            n_nv_col: 0,
            n_row_log_est: 0,
            sz_tab_row: 0,
            key_conf: 0,
            e_tab_type: 0,
            u: TableU::default(),
            p_trigger: None,
            p_schema: None,
        }));

        let mut w = walker_zeroed(None);
        agg_info_persist_walker_init(&mut w, p_parse);
        walk_select(&mut w, p);
        if (p.sel_flags & SF_AGGREGATE) == 0 {
            w.x_expr_callback = Some(disallow_aggregates_in_order_by_cb);
            w.x_select_callback = None;
            if let Some(ob) = p.p_order_by.as_deref() {
                walk_expr_list(&mut w, ob);
            }
        }

        let p_src = p.p_src.take();
        let p_where = p.p_where.take();
        let p_group_by = p.p_group_by.take();
        let p_having = p.p_having.take();
        p.sel_flags &= !SF_AGGREGATE;
        p.sel_flags |= SF_WINREWRITE;

        // Cria a cláusula ORDER BY do sub-select. É a concatenação das cláusulas PARTITION e
        // ORDER BY da janela. Se isso a tornar redundante, remove o ORDER BY do SELECT pai.
        let mut p_sort = expr_list_append_list(
            p_parse,
            None,
            p.p_win.as_deref().unwrap().p_partition.as_deref(),
            1,
        );
        p_sort = expr_list_append_list(
            p_parse,
            p_sort,
            p.p_win.as_deref().unwrap().p_order_by.as_deref(),
            1,
        );
        if let Some(sort) = p_sort.as_mut() {
            let ob_n = p.p_order_by.as_ref().map(|o| o.n_expr);
            if let Some(ob_n) = ob_n {
                if ob_n <= sort.n_expr {
                    let n_save = sort.n_expr;
                    sort.n_expr = ob_n;
                    let same = expr_list_compare(Some(&*sort), p.p_order_by.as_deref(), -1) == 0;
                    if same {
                        expr_list_delete(&db, p.p_order_by.take());
                    }
                    sort.n_expr = n_save;
                }
            }
        }

        // Atribui um número de cursor à tabela efêmera que armazena as linhas. O
        // OpenEphemeral é codificado depois, quando se sabe quantas colunas ela terá.
        let i_eph_csr = {
            let mut pb = p_parse.borrow_mut();
            let c = pb.n_tab;
            pb.n_tab += 1;
            pb.n_tab += 3;
            c
        };
        p.p_win.as_deref_mut().unwrap().i_eph_csr = i_eph_csr;

        // Identidade de cada Window da lista (a lista não muda durante a reescrita).
        let mut win_ids: Vec<usize> = Vec::new();
        {
            let mut cur = p.p_win.as_deref();
            while let Some(wn) = cur {
                win_ids.push(wn as *const Window as usize);
                cur = wn.p_next_win.as_deref();
            }
        }

        let p_src_box = p_src.expect("o SELECT com janela sempre tem a cláusula FROM");
        let mut p_sublist: Option<Box<ExprList>> = None; // Lista de expressões da sub-consulta
        select_window_rewrite_e_list(
            p_parse,
            &win_ids,
            i_eph_csr,
            &p_src_box,
            p.p_elist.as_deref_mut(),
            &p_tab,
            &mut p_sublist,
        );
        select_window_rewrite_e_list(
            p_parse,
            &win_ids,
            i_eph_csr,
            &p_src_box,
            p.p_order_by.as_deref_mut(),
            &p_tab,
            &mut p_sublist,
        );
        p.p_win.as_deref_mut().unwrap().n_buffer_col = p_sublist.as_ref().map_or(0, |l| l.n_expr);

        // Anexa as expressões PARTITION BY e ORDER BY à lista do sub-select. Elas são
        // necessárias para saber onde ficam as fronteiras das partições e dos conjuntos de
        // linhas pares.
        p_sublist = expr_list_append_list(
            p_parse,
            p_sublist,
            p.p_win.as_deref().unwrap().p_partition.as_deref(),
            0,
        );
        p_sublist = expr_list_append_list(
            p_parse,
            p_sublist,
            p.p_win.as_deref().unwrap().p_order_by.as_deref(),
            0,
        );

        // Anexa os argumentos de cada função de janela à lista do sub-select. Também aloca
        // dois registros para cada função de janela: um para o acumulador, outro para os
        // resultados intermediários.
        {
            let mut cur = p.p_win.as_deref_mut();
            while let Some(p_win) = cur {
                let owner = p_win
                    .p_owner
                    .as_ref()
                    .and_then(|o| o.upgrade())
                    .expect("a função de janela sempre tem a expressão dona");
                let subtype = p_win
                    .p_w_func
                    .as_ref()
                    .map_or(false, |f| (f.func_flags as i32 & SQLITE_SUBTYPE) != 0);
                if subtype {
                    let mut owner_b = owner.borrow_mut();
                    let p_args = owner_b.x.p_list.as_deref_mut();
                    select_window_rewrite_e_list(
                        p_parse,
                        &win_ids,
                        i_eph_csr,
                        &p_src_box,
                        p_args,
                        &p_tab,
                        &mut p_sublist,
                    );
                    p_win.i_arg_col = p_sublist.as_ref().map_or(0, |l| l.n_expr);
                    p_win.b_expr_args = 1;
                } else {
                    p_win.i_arg_col = p_sublist.as_ref().map_or(0, |l| l.n_expr);
                    let owner_b = owner.borrow();
                    p_sublist = expr_list_append_list(p_parse, p_sublist, owner_b.x.p_list.as_deref(), 0);
                }
                if p_win.p_filter.is_some() {
                    let p_filter = expr_dup(&db, p_win.p_filter.as_deref(), 0);
                    p_sublist = expr_list_append(p_parse, p_sublist, p_filter);
                }
                {
                    let mut pb = p_parse.borrow_mut();
                    pb.n_mem += 1;
                    p_win.reg_accum = pb.n_mem;
                    pb.n_mem += 1;
                    p_win.reg_result = pb.n_mem;
                }
                if let Some(v) = &v {
                    vdbe_add_op2(&mut v.borrow_mut(), OP_NULL as i32, 0, p_win.reg_accum);
                }
                cur = p_win.p_next_win.as_deref_mut();
            }
        }

        // Se não há ORDER BY nem PARTITION BY, a função de janela não aceita argumentos e
        // nenhuma outra coluna é selecionada (por exemplo "SELECT row_number() OVER () FROM
        // t1"), `p_sublist` ainda pode ser None aqui. Acrescenta uma expressão constante para
        // manter tudo legal nesse caso.
        if p_sublist.is_none() {
            let p_zero = expr(Some(&mut db.borrow_mut()), TK_INTEGER as i32, Some(b"0"));
            p_sublist = expr_list_append(p_parse, None, p_zero);
        }

        let mut p_sub = select_new(
            p_parse,
            p_sublist,
            Some(p_src_box),
            p_where,
            p_group_by,
            p_having,
            p_sort,
            None,
            None,
        );
        // TREETRACE(0x40, ...) só existe com SQLITE_DEBUG e some.
        p.p_src = src_list_append(&mut p_parse.borrow_mut(), None, None, None);
        if p.p_src.is_some() {
            let src = p.p_src.as_mut().unwrap();
            src.a[0].p_select = p_sub.take();
            src.a[0].fg.is_correlated = 1;
            src_list_assign_cursors(p_parse, src);
            let sub = src.a[0]
                .p_select
                .as_deref_mut()
                .expect("o sub-select acabou de ser instalado");
            sub.sel_flags |= SF_EXPANDED | SF_ORDERBYREQD;
            let p_tab2 = result_set_of_select(p_parse, sub, SQLITE_AFF_NONE);
            sub.sel_flags |= sel_flags & SF_AGGREGATE;
            match p_tab2 {
                None => {
                    // Pode ser outro tipo de erro, mas nesse caso `pParse->nErr` estará
                    // ligado, então se SQLITE_NOMEM está definido a mensagem correta sai de
                    // qualquer forma.
                    rc = SQLITE_NOMEM;
                }
                Some(t2) => {
                    // memcpy(pTab, pTab2, sizeof(Table)): o conteúdo da tabela do resultado
                    // passa para a tabela já referenciada pelas expressões reescritas.
                    let mut t2 = *t2;
                    t2.tab_flags |= TF_EPHEMERAL;
                    *p_tab.borrow_mut() = t2;
                    src.a[0].p_tab = Some(p_tab.clone());
                    w = walker_zeroed(None);
                    w.x_expr_callback = Some(window_extra_agg_func_depth);
                    w.x_select_callback = Some(walker_depth_increase);
                    w.x_select_callback2 = Some(walker_depth_decrease);
                    let sub = src.a[0]
                        .p_select
                        .as_deref()
                        .expect("o sub-select acabou de ser instalado");
                    walk_select(&mut w, sub);
                }
            }
        } else {
            select_delete(&db, p_sub.take());
        }
        if db.borrow().malloc_failed != 0 {
            rc = SQLITE_NOMEM;
        }

        // Adia a liberação da tabela temporária `p_tab` porque, se ocorreu um erro, ainda
        // pode haver referências a ela no conjunto de resultados ou no ORDER BY do SELECT.
        let keep = p_tab;
        parser_add_cleanup(
            &mut p_parse.borrow_mut(),
            Box::new(move |_db: &Rc<RefCell<Sqlite3>>| drop(keep)),
            None,
        );
    }

    debug_assert!(rc == SQLITE_OK || p_parse.borrow().n_err != 0);
    rc
}


// ---- part_003.rs ----

// Notas de modelo desta parte (window.c, parte 3):
//
//  - `Window` vive atrás de `WindowRef = Rc<RefCell<Window>>`, porque o mesmo objeto é acessível
//    por `Expr.y.p_win` e pelas listas `Select.p_win` e `Select.p_win_defn` (encadeadas por
//    `Window.p_next_win`). Por isso `Select.p_win`, `Select.p_win_defn` e `Window.p_next_win`
//    precisam ser `Option<WindowRef>` (como o resolve.c já assume).
//  - `ppThis` do C não existe: o desligamento da lista recebe a cabeça da lista e acha o nó.
//  - `Window` precisa derivar `Default` (equivale ao `sqlite3DbMallocZero`).
//  - `sqlite3ExprDelete` e afins viram o `Drop` dos `Box`: atribuir `None` libera.
//  - `VdbeCoverage*` só existe sob SQLITE_VDBE_COVERAGE e some.
//  - Os nomes `nth_valueName` e afins (window.c, parte 1) são comparados por conteúdo.

/// Desliga o objeto Window da lista de janelas do Select a que está ligado, se estiver. O
/// `ppThis` do C vira a cabeça da lista (`Select.p_win`) passada pelo chamador.
pub fn window_unlink_from_select(p_head: &mut Option<WindowRef>, p: &WindowRef) {
    let head_is_p = match p_head {
        Some(h) => Rc::ptr_eq(h, p),
        None => false,
    };
    if head_is_p {
        let next = p.borrow().p_next_win.clone();
        *p_head = next;
        return;
    }
    let mut prev = match p_head {
        Some(h) => h.clone(),
        None => return,
    };
    loop {
        let next = prev.borrow().p_next_win.clone();
        match next {
            Some(n) => {
                if Rc::ptr_eq(&n, p) {
                    let after = p.borrow().p_next_win.clone();
                    prev.borrow_mut().p_next_win = after;
                    return;
                }
                prev = n;
            }
            None => return,
        }
    }
}

/// Libera o objeto Window passado. Quem tem o Select em mãos chama antes
/// `window_unlink_from_select`.
pub fn window_delete(_db: &Sqlite3Ref, p: Option<WindowRef>) {
    if let Some(p) = p {
        let mut w = p.borrow_mut();
        w.p_filter = None;
        w.p_partition = None;
        w.p_order_by = None;
        w.p_end = None;
        w.p_start = None;
        w.z_name = None;
        w.z_base = None;
    }
}

/// Libera a lista ligada de objetos Window que começa no segundo argumento.
pub fn window_list_delete(db: &Sqlite3Ref, mut p: Option<WindowRef>) {
    while let Some(w) = p {
        let p_next = w.borrow().p_next_win.clone();
        window_delete(db, Some(w));
        p = p_next;
    }
}

/// A expressão do argumento é um deslocamento PRECEDING ou FOLLOWING. O valor deve ser um
/// inteiro não negativo. Se não for constante, troca por NULL. O fato de ser um inteiro não
/// negativo é verificado depois. Mas é importante não deixar valores variáveis na árvore.
fn window_offset_expr(p_parse: &ParseRef, p_expr: Option<Box<Expr>>) -> Option<Box<Expr>> {
    // Uma expressão ausente é constante (o caminhador do C devolve CONTINUE para NULL)
    let not_constant = match &p_expr {
        Some(e) => expr_is_constant(None, e) == 0,
        None => false,
    };
    if not_constant {
        if in_rename_object(&p_parse.borrow()) {
            rename_expr_unmap(&Some(p_parse.clone()), p_expr.as_ref().unwrap());
        }
        drop(p_expr);
        let db = p_parse.borrow().db.upgrade().unwrap();
        return expr_alloc(&mut db.borrow_mut(), TK_NULL as i32, None, false);
    }
    p_expr
}

/// Aloca e devolve um objeto Window novo que descreve uma definição de janela.
pub fn window_alloc(
    p_parse: &ParseRef,          // Contexto de análise
    e_type: i32,                 // Tipo de frame: TK_RANGE, TK_ROWS, TK_GROUPS ou 0
    e_start: i32,                // Início: CURRENT, PRECEDING, FOLLOWING, UNBOUNDED
    p_start: Option<Box<Expr>>,  // Tamanho do início se TK_PRECEDING ou FOLLOWING
    e_end: i32,                  // Fim: CURRENT, FOLLOWING, TK_UNBOUNDED, PRECEDING
    p_end: Option<Box<Expr>>,    // Tamanho do fim se TK_FOLLOWING ou PRECEDING
    mut e_exclude: u8,           // Cláusula EXCLUDE
) -> Option<WindowRef> {
    let mut e_type = e_type;
    let mut b_implicit_frame: u8 = 0;

    // O analisador garante o seguinte:
    debug_assert!(
        e_type == 0
            || e_type == TK_RANGE as i32
            || e_type == TK_ROWS as i32
            || e_type == TK_GROUPS as i32
    );
    debug_assert!(
        e_start == TK_CURRENT as i32
            || e_start == TK_PRECEDING as i32
            || e_start == TK_UNBOUNDED as i32
            || e_start == TK_FOLLOWING as i32
    );
    debug_assert!(
        e_end == TK_CURRENT as i32
            || e_end == TK_FOLLOWING as i32
            || e_end == TK_UNBOUNDED as i32
            || e_end == TK_PRECEDING as i32
    );
    debug_assert!(
        (e_start == TK_PRECEDING as i32 || e_start == TK_FOLLOWING as i32) == p_start.is_some()
    );
    debug_assert!(
        (e_end == TK_FOLLOWING as i32 || e_end == TK_PRECEDING as i32) == p_end.is_some()
    );

    if e_type == 0 {
        b_implicit_frame = 1;
        e_type = TK_RANGE as i32;
    }

    // Além disso, o tipo de fronteira inicial não pode aparecer antes, na lista abaixo, do
    // tipo de fronteira final:
    //
    //   UNBOUNDED PRECEDING
    //   <expr> PRECEDING
    //   CURRENT ROW
    //   <expr> FOLLOWING
    //   UNBOUNDED FOLLOWING
    //
    // O analisador garante que "UNBOUNDED PRECEDING" não pode ser fronteira final e que
    // "UNBOUNDED FOLLOWING" não pode ser fronteira inicial.
    if (e_start == TK_CURRENT as i32 && e_end == TK_PRECEDING as i32)
        || (e_start == TK_FOLLOWING as i32
            && (e_end == TK_PRECEDING as i32 || e_end == TK_CURRENT as i32))
    {
        error_msg(
            &mut p_parse.borrow_mut(),
            Some(b"unsupported frame specification"),
        );
        // windowAllocErr: p_end e p_start são liberados ao sair de escopo
        drop(p_end);
        drop(p_start);
        return None;
    }

    let mut p_win = Window::default();
    p_win.e_frm_type = e_type as u8;
    p_win.e_start = e_start as u8;
    p_win.e_end = e_end as u8;
    if e_exclude == 0 {
        let db = p_parse.borrow().db.upgrade().unwrap();
        if optimization_disabled(&db.borrow(), SQLITE_WINDOW_FUNC) {
            e_exclude = TK_NO;
        }
    }
    p_win.e_exclude = e_exclude;
    p_win.b_implicit_frame = b_implicit_frame;
    p_win.p_end = window_offset_expr(p_parse, p_end);
    p_win.p_start = window_offset_expr(p_parse, p_start);
    Some(Rc::new(RefCell::new(p_win)))
}

/// Anexa as cláusulas PARTITION e ORDER BY (`p_partition` e `p_order_by`) à janela `p_win`.
/// Se `p_base` não é nulo, grava em `p_win.z_base` a string equivalente.
pub fn window_assemble(
    p_parse: &ParseRef,
    p_win: Option<WindowRef>,
    p_partition: Option<Box<ExprList>>,
    p_order_by: Option<Box<ExprList>>,
    p_base: Option<&Token>,
) -> Option<WindowRef> {
    if let Some(w) = &p_win {
        let mut win = w.borrow_mut();
        win.p_partition = p_partition;
        win.p_order_by = p_order_by;
        if let Some(base) = p_base {
            let db = p_parse.borrow().db.upgrade().unwrap();
            win.z_base = Some(db_str_n_dup(&mut db.borrow_mut(), Some(&base.z), base.n as u64));
        }
    } else {
        drop(p_partition);
        drop(p_order_by);
    }
    p_win
}

/// A janela `p_win` acaba de ser criada a partir de uma cláusula WINDOW. `p_base` é a janela
/// base. As janelas anteriores da mesma cláusula WINDOW ficam na lista ligada que começa em
/// `p_list`. A função atualiza `p_win` conforme a especificação base ou deixa um erro em
/// `p_parse`.
pub fn window_chain(p_parse: &ParseRef, p_win: &WindowRef, p_list: Option<&WindowRef>) {
    let z_base = p_win.borrow().z_base.clone();
    if let Some(z_base) = z_base {
        let db = p_parse.borrow().db.upgrade().unwrap();
        let p_exist = window_find(p_parse, p_list, &z_base);
        if let Some(p_exist) = p_exist {
            let mut z_err: Option<&'static [u8]> = None;
            // Verifica erros
            {
                let win = p_win.borrow();
                let exist = p_exist.borrow();
                if win.p_partition.is_some() {
                    z_err = Some(b"PARTITION clause");
                } else if exist.p_order_by.is_some() && win.p_order_by.is_some() {
                    z_err = Some(b"ORDER BY clause");
                } else if exist.b_implicit_frame == 0 {
                    z_err = Some(b"frame specification");
                }
            }
            if let Some(z_err) = z_err {
                let mut msg: Vec<u8> = Vec::new();
                msg.extend_from_slice(b"cannot override ");
                msg.extend_from_slice(z_err);
                msg.extend_from_slice(b" of window: ");
                msg.extend_from_slice(&z_base);
                error_msg(&mut p_parse.borrow_mut(), Some(&msg));
            } else {
                let (dup_partition, dup_order_by) = {
                    let exist = p_exist.borrow();
                    let dup_partition =
                        expr_list_dup(&mut db.borrow_mut(), exist.p_partition.as_deref(), 0);
                    let dup_order_by = if exist.p_order_by.is_some() {
                        expr_list_dup(&mut db.borrow_mut(), exist.p_order_by.as_deref(), 0)
                    } else {
                        None
                    };
                    (dup_partition, dup_order_by)
                };
                let mut win = p_win.borrow_mut();
                win.p_partition = dup_partition;
                if p_exist.borrow().p_order_by.is_some() {
                    debug_assert!(win.p_order_by.is_none());
                    win.p_order_by = dup_order_by;
                }
                win.z_base = None;
            }
        }
    }
}

/// Anexa o objeto de janela `p_win` à expressão `p`.
pub fn window_attach(p_parse: &ParseRef, p: Option<&ExprRef>, p_win: WindowRef) {
    if let Some(p) = p {
        let flags;
        {
            let mut e = p.borrow_mut();
            debug_assert!(e.op == TK_FUNCTION);
            debug_assert!(expr_is_full_size(&e));
            e.y.p_win = Some(p_win.clone());
            expr_set_property(&mut e, EP_WIN_FUNC | EP_FULL_SIZE);
            flags = e.flags;
        }
        p_win.borrow_mut().p_owner = Some(Rc::downgrade(p));
        if (flags & EP_DISTINCT) != 0 && p_win.borrow().e_frm_type != TK_FILTER {
            error_msg(
                &mut p_parse.borrow_mut(),
                Some(b"DISTINCT is not supported for window functions"),
            );
        }
    } else {
        let db = p_parse.borrow().db.upgrade().unwrap();
        window_delete(&db, Some(p_win));
    }
}

/// Possivelmente liga a janela `p_win` à lista em `p_sel.p_win` (funções de janela a processar
/// como parte do SELECT `p_sel`). A janela entra se (a) não há outras janelas ligadas a este
/// SELECT ou (b) as janelas já ligadas usam um frame compatível.
pub fn window_link(p_sel: Option<&mut Select>, p_win: &WindowRef) {
    if let Some(p_sel) = p_sel {
        let compatible = match &p_sel.p_win {
            None => true,
            Some(sw) => window_compare(None, &sw.borrow(), &p_win.borrow(), 0) == 0,
        };
        if compatible {
            p_win.borrow_mut().p_next_win = p_sel.p_win.take();
            p_sel.p_win = Some(p_win.clone());
        } else {
            let differs = {
                let sw = p_sel.p_win.as_ref().unwrap().borrow();
                let w = p_win.borrow();
                expr_list_compare(w.p_partition.as_deref(), sw.p_partition.as_deref(), -1) != 0
            };
            if differs {
                p_sel.sel_flags |= SF_MULTIPART;
            }
        }
    }
}

/// Devolve 0 se os dois objetos de janela são idênticos, 1 se são diferentes ou 2 se não dá
/// para determinar se são idênticos. Objetos de janela idênticos podem ser processados numa
/// única varredura.
pub fn window_compare(p_parse: Option<&Parse>, p1: &Window, p2: &Window, b_filter: i32) -> i32 {
    let mut res;
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
    if expr_compare(p_parse, p1.p_start.as_deref(), p2.p_start.as_deref(), -1) != 0 {
        return 1;
    }
    if expr_compare(p_parse, p1.p_end.as_deref(), p2.p_end.as_deref(), -1) != 0 {
        return 1;
    }
    res = expr_list_compare(p1.p_partition.as_deref(), p2.p_partition.as_deref(), -1);
    if res != 0 {
        return res;
    }
    res = expr_list_compare(p1.p_order_by.as_deref(), p2.p_order_by.as_deref(), -1);
    if res != 0 {
        return res;
    }
    if b_filter != 0 {
        res = expr_compare(p_parse, p1.p_filter.as_deref(), p2.p_filter.as_deref(), -1);
        if res != 0 {
            return res;
        }
    }
    0
}

/// Chamada pelo select.c antes de chamar `where_begin()` para começar a iterar pelos
/// resultados da subconsulta. Serve para alocar e inicializar registros e cursores usados por
/// `window_code_step()`.
pub fn window_code_init(p_parse: &ParseRef, p_select: &mut Select) {
    let n_eph_expr = p_select.p_src.as_ref().unwrap().a[0]
        .p_select
        .as_ref()
        .unwrap()
        .p_elist
        .as_ref()
        .unwrap()
        .n_expr;
    let p_m_win: WindowRef = p_select.p_win.clone().unwrap();
    let v: VdbeRef = get_vdbe(p_parse).unwrap();

    let i_eph_csr = p_m_win.borrow().i_eph_csr;
    vdbe_add_op2(&mut v.borrow_mut(), OP_OPENEPHEMERAL as i32, i_eph_csr, n_eph_expr);
    vdbe_add_op2(&mut v.borrow_mut(), OP_OPENDUP as i32, i_eph_csr + 1, i_eph_csr);
    vdbe_add_op2(&mut v.borrow_mut(), OP_OPENDUP as i32, i_eph_csr + 2, i_eph_csr);
    vdbe_add_op2(&mut v.borrow_mut(), OP_OPENDUP as i32, i_eph_csr + 3, i_eph_csr);

    // Aloca registros para os valores de PARTITION BY, se houver. Inicializa os registros
    // com NULL.
    let n_part_expr = p_m_win.borrow().p_partition.as_ref().map(|l| l.n_expr);
    if let Some(n_expr) = n_part_expr {
        let reg_part = p_parse.borrow().n_mem + 1;
        p_m_win.borrow_mut().reg_part = reg_part;
        p_parse.borrow_mut().n_mem += n_expr;
        vdbe_add_op3(
            &mut v.borrow_mut(),
            OP_NULL as i32,
            0,
            reg_part,
            reg_part + n_expr - 1,
        );
    }

    {
        let mut parse = p_parse.borrow_mut();
        parse.n_mem += 1;
        p_m_win.borrow_mut().reg_one = parse.n_mem;
    }
    let reg_one = p_m_win.borrow().reg_one;
    vdbe_add_op2(&mut v.borrow_mut(), OP_INTEGER as i32, 1, reg_one);

    if p_m_win.borrow().e_exclude != 0 {
        {
            let mut parse = p_parse.borrow_mut();
            let mut m = p_m_win.borrow_mut();
            parse.n_mem += 1;
            m.reg_start_rowid = parse.n_mem;
            parse.n_mem += 1;
            m.reg_end_rowid = parse.n_mem;
            m.csr_app = parse.n_tab;
            parse.n_tab += 1;
        }
        let (reg_start_rowid, reg_end_rowid, csr_app) = {
            let m = p_m_win.borrow();
            (m.reg_start_rowid, m.reg_end_rowid, m.csr_app)
        };
        vdbe_add_op2(&mut v.borrow_mut(), OP_INTEGER as i32, 1, reg_start_rowid);
        vdbe_add_op2(&mut v.borrow_mut(), OP_INTEGER as i32, 0, reg_end_rowid);
        vdbe_add_op2(&mut v.borrow_mut(), OP_OPENDUP as i32, csr_app, i_eph_csr);
        return;
    }

    let mut p_win_opt: Option<WindowRef> = Some(p_m_win.clone());
    while let Some(p_win) = p_win_opt {
        let p: Rc<FuncDef> = p_win.borrow().p_w_func.clone().unwrap();
        let e_start = p_win.borrow().e_start;
        if (p.func_flags & SQLITE_FUNC_MINMAX) != 0 && e_start != TK_UNBOUNDED {
            // As versões inline de min() e max() exigem uma única tabela efêmera e 3
            // registros. Os registros são usados assim:
            //
            //   reg_app+0: posição para copiar o argumento de min()/max() para o MakeRecord
            //   reg_app+1: valor inteiro que garante chaves únicas
            //   reg_app+2: saída do MakeRecord
            let p_owner = p_win.borrow().p_owner.as_ref().unwrap().upgrade().unwrap();
            let p_key_info: Option<KeyInfoRef> = {
                let owner = p_owner.borrow();
                debug_assert!(expr_use_x_list(&owner));
                let p_list = owner.x.p_list.as_ref().unwrap();
                key_info_from_expr_list(p_parse, p_list, 0, 0)
            };
            {
                let mut parse = p_parse.borrow_mut();
                let mut w = p_win.borrow_mut();
                w.csr_app = parse.n_tab;
                parse.n_tab += 1;
                w.reg_app = parse.n_mem + 1;
                parse.n_mem += 3;
            }
            if let Some(ki) = &p_key_info {
                if p.z_name.get(1) == Some(&b'i') {
                    debug_assert!(ki.borrow().a_sort_flags[0] == 0);
                    ki.borrow_mut().a_sort_flags[0] = KEYINFO_ORDER_DESC;
                }
            }
            let (csr_app, reg_app) = {
                let w = p_win.borrow();
                (w.csr_app, w.reg_app)
            };
            vdbe_add_op2(&mut v.borrow_mut(), OP_OPENEPHEMERAL as i32, csr_app, 2);
            // Sem KeyInfo (falta de memória no C) o P4 fica sem uso
            if let Some(ki) = p_key_info {
                vdbe_append_p4(&mut v.borrow_mut(), P4Value::KeyInfo(ki), P4_KEYINFO as i32);
            }
            vdbe_add_op2(&mut v.borrow_mut(), OP_INTEGER as i32, 0, reg_app + 1);
        } else if p.z_name.as_slice() == NTH_VALUE_NAME || p.z_name.as_slice() == FIRST_VALUE_NAME
        {
            // Aloca dois registros em p_win.reg_app. Guardam o índice inicial e o final do
            // frame corrente.
            {
                let mut parse = p_parse.borrow_mut();
                let mut w = p_win.borrow_mut();
                w.reg_app = parse.n_mem + 1;
                w.csr_app = parse.n_tab;
                parse.n_tab += 1;
                parse.n_mem += 2;
            }
            let csr_app = p_win.borrow().csr_app;
            vdbe_add_op2(&mut v.borrow_mut(), OP_OPENDUP as i32, csr_app, i_eph_csr);
        } else if p.z_name.as_slice() == LEAD_NAME || p.z_name.as_slice() == LAG_NAME {
            {
                let mut parse = p_parse.borrow_mut();
                p_win.borrow_mut().csr_app = parse.n_tab;
                parse.n_tab += 1;
            }
            let csr_app = p_win.borrow().csr_app;
            vdbe_add_op2(&mut v.borrow_mut(), OP_OPENDUP as i32, csr_app, i_eph_csr);
        }
        p_win_opt = p_win.borrow().p_next_win.clone();
    }
}

pub const WINDOW_STARTING_INT: i32 = 0;
pub const WINDOW_ENDING_INT: i32 = 1;
pub const WINDOW_NTH_VALUE_INT: i32 = 2;
pub const WINDOW_STARTING_NUM: i32 = 3;
pub const WINDOW_ENDING_NUM: i32 = 4;

/// Um "PRECEDING <expr>" (e_cond==0), "FOLLOWING <expr>" (e_cond==1) ou o valor do segundo
/// argumento de nth_value() (e_cond==2) acaba de ser avaliado e o resultado ficou no registro
/// `reg`. Esta função gera código da VM que confere se o valor é um inteiro não negativo e
/// lança uma exceção se não for.
fn window_check_value(p_parse: &ParseRef, reg: i32, e_cond: i32) {
    static AZ_ERR: [&str; 5] = [
        "frame starting offset must be a non-negative integer",
        "frame ending offset must be a non-negative integer",
        "second argument to nth_value must be a positive integer",
        "frame starting offset must be a non-negative number",
        "frame ending offset must be a non-negative number",
    ];
    static A_OP: [u8; 5] = [OP_GE, OP_GE, OP_GT, OP_GE, OP_GE];
    let v: VdbeRef = get_vdbe(p_parse).unwrap();
    let reg_zero = get_temp_reg(&mut p_parse.borrow_mut());
    debug_assert!(e_cond >= 0 && (e_cond as usize) < AZ_ERR.len());
    vdbe_add_op2(&mut v.borrow_mut(), OP_INTEGER as i32, 0, reg_zero);
    if e_cond >= WINDOW_STARTING_NUM {
        let reg_string = get_temp_reg(&mut p_parse.borrow_mut());
        vdbe_add_op4(
            &mut v.borrow_mut(),
            OP_STRING8 as i32,
            0,
            reg_string,
            0,
            P4Value::Static(Vec::new()),
            P4_STATIC as i8,
        );
        let addr = vdbe_current_addr(&v.borrow());
        vdbe_add_op3(&mut v.borrow_mut(), OP_GE as i32, reg_string, addr + 2, reg);
        vdbe_change_p5(
            &mut v.borrow_mut(),
            (SQLITE_AFF_NUMERIC | SQLITE_JUMPIFNULL) as u16,
        );
        debug_assert!(e_cond == 3 || e_cond == 4);
    } else {
        let addr = vdbe_current_addr(&v.borrow());
        vdbe_add_op2(&mut v.borrow_mut(), OP_MUSTBEINT as i32, reg, addr + 2);
        debug_assert!(e_cond == 0 || e_cond == 1 || e_cond == 2);
    }
    let addr = vdbe_current_addr(&v.borrow());
    vdbe_add_op3(
        &mut v.borrow_mut(),
        A_OP[e_cond as usize] as i32,
        reg_zero,
        addr + 2,
        reg,
    );
    vdbe_change_p5(&mut v.borrow_mut(), SQLITE_AFF_NUMERIC as u16);
    may_abort(&mut p_parse.borrow_mut());
    vdbe_add_op2(
        &mut v.borrow_mut(),
        OP_HALT as i32,
        SQLITE_ERROR,
        OE_ABORT as i32,
    );
    vdbe_append_p4(
        &mut v.borrow_mut(),
        P4Value::Static(AZ_ERR[e_cond as usize].as_bytes().to_vec()),
        P4_STATIC as i32,
    );
    release_temp_reg(&mut p_parse.borrow_mut(), reg_zero);
}


// ---- part_004.rs ----

// Modelo adotado nas partes 4 a 7 de window.c (decidido pela parte 6, que define `Window`):
//   - `Window` é uma lista de `Box<Window>` encadeada por `p_next_win`; `p_owner` é
//     `Option<Weak<RefCell<Expr>>>`;
//   - `WindowCodeArg<'a>` guarda `p_parse: ParseRef`, `p_vdbe: VdbeRef` e `p_m_win: &'a Window`;
//   - as rotinas da VDBE recebem `&mut Vdbe` (`&mut v.borrow_mut()`) e o opcode como `i32`.

/// Devolve a expressão dona (`Window.pOwner`) de uma função de janela.
fn window_owner(p_win: &Window) -> Rc<RefCell<Expr>> {
    p_win
        .p_owner
        .as_ref()
        .and_then(|w| w.upgrade())
        .expect("janela sem expressão dona")
}

/// Número de argumentos da função de janela associada ao objeto recebido.
fn window_arg_count(p_win: &Window) -> i32 {
    let owner = window_owner(p_win);
    let n = match owner.borrow().x.p_list.as_ref() {
        Some(p_list) => p_list.n_expr,
        None => 0,
    };
    n
}

/// Ver os comentários acima de `WindowCodeArg`.
#[derive(Clone, Copy, Default)]
pub struct WindowCsrAndReg {
    /// Número do cursor.
    pub csr: i32,
    /// Primeiro de um vetor de valores de pares.
    pub reg: i32,
}

/// Uma única instância desta estrutura é alocada na pilha por `window_code_step()`
/// e passada às rotinas auxiliares, para reduzir o número de argumentos de cada uma.
///
/// `reg_arg`: primeiro de um vetor de registradores acumuladores, um para cada
/// função de janela da lista `p_m_win`.
///
/// `e_delete`: quando as linhas de entrada em cache numa tabela temporária podem
/// ser removidas (`WINDOW_RETURN_ROW`: depois de devolvida ao chamador;
/// `WINDOW_AGGINVERSE`: depois dos xInverse(); `WINDOW_AGGSTEP`: depois dos xStep()).
///
/// `start`, `current`, `end`: os três cursores sobre a tabela temporária. `current`
/// aponta a próxima linha a devolver, `end` a próxima a receber xStep() e `start` a
/// próxima a receber xInverse(). Cada um tem o cursor VDBE (`csr`) e o vetor de
/// registradores (`reg`) com uma cópia dos valores de pares lidos dele. Se o cursor
/// não é necessário, ambos valem 0.
pub struct WindowCodeArg<'a> {
    /// Contexto de análise.
    pub p_parse: ParseRef,
    /// Primeiro da lista de funções em processamento.
    pub p_m_win: &'a Window,
    /// Objeto VDBE.
    pub p_vdbe: VdbeRef,
    /// OP_Gosub para este endereço para devolver uma linha.
    pub addr_gosub: i32,
    /// Registrador usado com OP_Gosub(addr_gosub).
    pub reg_gosub: i32,
    /// Primeiro do vetor de registradores acumuladores.
    pub reg_arg: i32,
    /// Ver acima.
    pub e_delete: i32,
    pub reg_rowid: i32,

    pub start: WindowCsrAndReg,
    pub current: WindowCsrAndReg,
    pub end: WindowCsrAndReg,
}

/// Gera código VM para ler os valores de pares da janela do cursor `csr` para um
/// vetor de registradores começando em `reg`.
fn window_read_peer_values(p: &WindowCodeArg, csr: i32, reg: i32) {
    let p_m_win = p.p_m_win;
    if let Some(p_order_by) = p_m_win.p_order_by.as_deref() {
        let v = get_vdbe(&p.p_parse).expect("sem Vdbe");
        let i_col_off = p_m_win.n_buffer_col
            + match p_m_win.p_partition.as_deref() {
                Some(p_part) => p_part.n_expr,
                None => 0,
            };
        let mut i = 0;
        while i < p_order_by.n_expr {
            vdbe_add_op3(&mut v.borrow_mut(), OP_COLUMN as i32, csr, i_col_off + i, reg + i);
            i += 1;
        }
    }
}

/// Gera código VM para invocar xStep() (se `b_inverse` é 0) ou xInverse (se não
/// é zero) para cada função de janela da lista ligada que começa em `p_m_win`.
/// Para funções de janela embutidas que não usam a API padrão, gera o código
/// VM equivalente em linha.
///
/// Se `csr` é maior ou igual a 0, `reg` é o primeiro de um vetor de registradores
/// grande o bastante para os argumentos de cada função, e os argumentos são
/// extraídos da linha corrente de `csr` antes de OP_AggStep ou OP_AggInverse.
///
/// Se `csr` é menor que zero, o vetor em `reg` já está preenchido com todas as
/// colunas da linha corrente da subconsulta.
fn window_agg_step(p: &WindowCodeArg, p_m_win: &Window, csr: i32, b_inverse: i32, reg: i32) {
    let p_parse = &p.p_parse;
    let v = get_vdbe(p_parse).expect("sem Vdbe");

    let mut cur: Option<&Window> = Some(p_m_win);
    while let Some(p_win) = cur {
        let p_func = p_win
            .p_w_func
            .clone()
            .expect("função de janela sem FuncDef");
        let z_name: &[u8] = p_func.z_name.as_slice();
        let mut reg_arg: i32;
        let mut n_arg: i32 = if p_win.b_expr_args != 0 {
            0
        } else {
            window_arg_count(p_win)
        };

        debug_assert!(b_inverse == 0 || p_win.e_start != TK_UNBOUNDED);

        // Todas as cláusulas OVER na mesma etapa de agregação de função de janela
        // precisam ser iguais.

        let mut i = 0;
        while i < n_arg {
            if i != 1 || z_name != NTH_VALUE_NAME {
                vdbe_add_op3(&mut v.borrow_mut(), OP_COLUMN as i32, csr, p_win.i_arg_col + i, reg + i);
            } else {
                vdbe_add_op3(
                    &mut v.borrow_mut(),
                    OP_COLUMN as i32,
                    p_m_win.i_eph_csr,
                    p_win.i_arg_col + i,
                    reg + i,
                );
            }
            i += 1;
        }
        reg_arg = reg;

        if p_m_win.reg_start_rowid == 0
            && (p_func.func_flags & (SQLITE_FUNC_MINMAX as u32)) != 0
            && p_win.e_start != TK_UNBOUNDED
        {
            let addr_is_null = vdbe_add_op1(&mut v.borrow_mut(), OP_ISNULL as i32, reg_arg);
            if b_inverse == 0 {
                vdbe_add_op2(&mut v.borrow_mut(), OP_ADDIMM as i32, p_win.reg_app + 1, 1);
                vdbe_add_op2(&mut v.borrow_mut(), OP_SCOPY as i32, reg_arg, p_win.reg_app);
                vdbe_add_op3(
                    &mut v.borrow_mut(),
                    OP_MAKERECORD as i32,
                    p_win.reg_app,
                    2,
                    p_win.reg_app + 2,
                );
                vdbe_add_op2(&mut v.borrow_mut(), OP_IDXINSERT as i32, p_win.csr_app, p_win.reg_app + 2);
            } else {
                vdbe_add_op4_int(&mut v.borrow_mut(), OP_SEEKGE as i32, p_win.csr_app, 0, reg_arg, 1);
                vdbe_add_op1(&mut v.borrow_mut(), OP_DELETE as i32, p_win.csr_app);
                let addr = vdbe_current_addr(&v.borrow()) - 2;
                vdbe_jump_here(&mut v.borrow_mut(), addr);
            }
            vdbe_jump_here(&mut v.borrow_mut(), addr_is_null);
        } else if p_win.reg_app != 0 {
            debug_assert!(z_name == NTH_VALUE_NAME || z_name == FIRST_VALUE_NAME);
            debug_assert!(b_inverse == 0 || b_inverse == 1);
            vdbe_add_op2(&mut v.borrow_mut(), OP_ADDIMM as i32, p_win.reg_app + 1 - b_inverse, 1);
        } else if p_func.x_s_func != Some(noop_step_func) {
            let mut addr_if: i32 = 0;
            if p_win.p_filter.is_some() {
                let reg_tmp = get_temp_reg(&mut p_parse.borrow_mut());
                vdbe_add_op3(
                    &mut v.borrow_mut(),
                    OP_COLUMN as i32,
                    csr,
                    p_win.i_arg_col + n_arg,
                    reg_tmp,
                );
                addr_if = vdbe_add_op3(&mut v.borrow_mut(), OP_IFNOT as i32, reg_tmp, 0, 1);
                release_temp_reg(&mut p_parse.borrow_mut(), reg_tmp);
            }

            if p_win.b_expr_args != 0 {
                let mut i_op = vdbe_current_addr(&v.borrow());
                let owner = window_owner(p_win);
                let owner_ref = owner.borrow();
                let p_list = owner_ref
                    .x
                    .p_list
                    .as_ref()
                    .expect("função de janela sem lista de argumentos");
                n_arg = p_list.n_expr;
                reg_arg = get_temp_range(&mut p_parse.borrow_mut(), n_arg);
                expr_code_expr_list(&mut p_parse.borrow_mut(), p_list, reg_arg, 0, 0);

                let i_end = vdbe_current_addr(&v.borrow());
                while i_op < i_end {
                    let mut vm = v.borrow_mut();
                    // Requer que `vdbe_get_op` devolva `&mut VdbeOp` (ver nota ao integrador).
                    let p_op = vdbe_get_op(&mut vm, i_op);
                    if p_op.opcode == OP_COLUMN && p_op.p1 == p_m_win.i_eph_csr {
                        p_op.p1 = csr;
                    }
                    i_op += 1;
                }
            }
            if (p_func.func_flags & (SQLITE_FUNC_NEEDCOLL as u32)) != 0 {
                debug_assert!(n_arg > 0);
                let owner = window_owner(p_win);
                let owner_ref = owner.borrow();
                let p_list = owner_ref
                    .x
                    .p_list
                    .as_ref()
                    .expect("função de janela sem lista de argumentos");
                let p_expr = p_list.a[0]
                    .p_expr
                    .as_deref()
                    .expect("argumento sem expressão");
                let p_coll = expr_nn_coll_seq(&mut p_parse.borrow_mut(), p_expr);
                vdbe_add_op4(
                    &mut v.borrow_mut(),
                    OP_COLLSEQ as i32,
                    0,
                    0,
                    0,
                    P4Value::CollSeq(p_coll),
                    P4_COLLSEQ as i32,
                );
            }
            vdbe_add_op3(
                &mut v.borrow_mut(),
                if b_inverse != 0 { OP_AGGINVERSE as i32 } else { OP_AGGSTEP as i32 },
                b_inverse,
                reg_arg,
                p_win.reg_accum,
            );
            vdbe_append_p4(&mut v.borrow_mut(), P4Value::FuncDef(p_func.clone()), P4_FUNCDEF as i32);
            vdbe_change_p5(&mut v.borrow_mut(), (n_arg as u8) as u16);
            if p_win.b_expr_args != 0 {
                release_temp_range(&mut p_parse.borrow_mut(), reg_arg, n_arg);
            }
            if addr_if != 0 {
                vdbe_jump_here(&mut v.borrow_mut(), addr_if);
            }
        }

        cur = p_win.p_next_win.as_deref();
    }
}

/// Valores que podem ser passados como segundo argumento de `window_code_op()`.
pub const WINDOW_RETURN_ROW: i32 = 1;
pub const WINDOW_AGGINVERSE: i32 = 2;
pub const WINDOW_AGGSTEP: i32 = 3;

/// Gera código VM para invocar xValue() (`b_fin` == 0) ou xFinalize() (`b_fin` == 1)
/// para cada função de janela da lista ligada que começa em `p_m_win`. Para
/// funções de janela embutidas que não usam a API padrão, gera o código VM
/// equivalente.
fn window_agg_final(p: &WindowCodeArg, b_fin: i32) {
    let p_m_win = p.p_m_win;
    let v = get_vdbe(&p.p_parse).expect("sem Vdbe");

    let mut cur: Option<&Window> = Some(p_m_win);
    while let Some(p_win) = cur {
        let p_func = p_win
            .p_w_func
            .clone()
            .expect("função de janela sem FuncDef");
        if p_m_win.reg_start_rowid == 0
            && (p_func.func_flags & (SQLITE_FUNC_MINMAX as u32)) != 0
            && p_win.e_start != TK_UNBOUNDED
        {
            vdbe_add_op2(&mut v.borrow_mut(), OP_NULL as i32, 0, p_win.reg_result);
            vdbe_add_op1(&mut v.borrow_mut(), OP_LAST as i32, p_win.csr_app);
            vdbe_add_op3(&mut v.borrow_mut(), OP_COLUMN as i32, p_win.csr_app, 0, p_win.reg_result);
            let addr = vdbe_current_addr(&v.borrow()) - 2;
            vdbe_jump_here(&mut v.borrow_mut(), addr);
        } else if p_win.reg_app != 0 {
            debug_assert!(p_m_win.reg_start_rowid == 0);
        } else {
            let n_arg = window_arg_count(p_win);
            if b_fin != 0 {
                vdbe_add_op2(&mut v.borrow_mut(), OP_AGGFINAL as i32, p_win.reg_accum, n_arg);
                vdbe_append_p4(&mut v.borrow_mut(), P4Value::FuncDef(p_func.clone()), P4_FUNCDEF as i32);
                vdbe_add_op2(&mut v.borrow_mut(), OP_COPY as i32, p_win.reg_accum, p_win.reg_result);
                vdbe_add_op2(&mut v.borrow_mut(), OP_NULL as i32, 0, p_win.reg_accum);
            } else {
                vdbe_add_op3(
                    &mut v.borrow_mut(),
                    OP_AGGVALUE as i32,
                    p_win.reg_accum,
                    n_arg,
                    p_win.reg_result,
                );
                vdbe_append_p4(&mut v.borrow_mut(), P4Value::FuncDef(p_func.clone()), P4_FUNCDEF as i32);
            }
        }
        cur = p_win.p_next_win.as_deref();
    }
}

/// Gera código para calcular os valores correntes de todas as funções de janela
/// da lista `p.p_m_win` por uma varredura completa da janela corrente. Guarda os
/// resultados nos registradores `Window.reg_result`, prontos para devolver à
/// camada superior.
fn window_full_scan(p: &WindowCodeArg) {
    let p_parse = &p.p_parse;
    let p_m_win = p.p_m_win;
    let v = p.p_vdbe.clone();

    let mut reg_c_peer: i32 = 0; // Valores de pares correntes
    let mut reg_peer: i32 = 0; // Valores de pares do AggStep

    let csr: i32 = p_m_win.csr_app;
    let n_peer: i32 = match p_m_win.p_order_by.as_deref() {
        Some(p_order_by) => p_order_by.n_expr,
        None => 0,
    };

    let lbl_next = vdbe_make_label(p_parse);
    let lbl_brk = vdbe_make_label(p_parse);

    let reg_c_rowid = get_temp_reg(&mut p_parse.borrow_mut()); // Valor do rowid corrente
    let reg_rowid = get_temp_reg(&mut p_parse.borrow_mut()); // Valor do rowid do AggStep
    if n_peer != 0 {
        reg_c_peer = get_temp_range(&mut p_parse.borrow_mut(), n_peer);
        reg_peer = get_temp_range(&mut p_parse.borrow_mut(), n_peer);
    }

    vdbe_add_op2(&mut v.borrow_mut(), OP_ROWID as i32, p_m_win.i_eph_csr, reg_c_rowid);
    window_read_peer_values(p, p_m_win.i_eph_csr, reg_c_peer);

    let mut cur: Option<&Window> = Some(p_m_win);
    while let Some(p_win) = cur {
        vdbe_add_op2(&mut v.borrow_mut(), OP_NULL as i32, 0, p_win.reg_accum);
        cur = p_win.p_next_win.as_deref();
    }

    vdbe_add_op3(&mut v.borrow_mut(), OP_SEEKGE as i32, csr, lbl_brk, p_m_win.reg_start_rowid);
    let addr_next = vdbe_current_addr(&v.borrow());
    vdbe_add_op2(&mut v.borrow_mut(), OP_ROWID as i32, csr, reg_rowid);
    vdbe_add_op3(&mut v.borrow_mut(), OP_GT as i32, p_m_win.reg_end_rowid, lbl_brk, reg_rowid);

    if p_m_win.e_exclude == TK_CURRENT {
        vdbe_add_op3(&mut v.borrow_mut(), OP_EQ as i32, reg_c_rowid, lbl_next, reg_rowid);
    } else if p_m_win.e_exclude != TK_NO {
        let mut addr_eq: i32 = 0;
        let mut p_key_info: Option<KeyInfoRef> = None;

        if let Some(p_order_by) = p_m_win.p_order_by.as_deref() {
            p_key_info = key_info_from_expr_list(p_parse, p_order_by, 0, 0);
        }
        if p_m_win.e_exclude == TK_TIES {
            addr_eq = vdbe_add_op3(&mut v.borrow_mut(), OP_EQ as i32, reg_c_rowid, 0, reg_rowid);
        }
        if let Some(p_key_info) = p_key_info {
            window_read_peer_values(p, csr, reg_peer);
            vdbe_add_op3(&mut v.borrow_mut(), OP_COMPARE as i32, reg_peer, reg_c_peer, n_peer);
            vdbe_append_p4(&mut v.borrow_mut(), P4Value::KeyInfo(p_key_info), P4_KEYINFO as i32);
            let addr = vdbe_current_addr(&v.borrow()) + 1;
            vdbe_add_op3(&mut v.borrow_mut(), OP_JUMP as i32, addr, lbl_next, addr);
        } else {
            vdbe_add_op2(&mut v.borrow_mut(), OP_GOTO as i32, 0, lbl_next);
        }
        if addr_eq != 0 {
            vdbe_jump_here(&mut v.borrow_mut(), addr_eq);
        }
    }

    window_agg_step(p, p_m_win, csr, 0, p.reg_arg);

    vdbe_resolve_label(&v, lbl_next);
    vdbe_add_op2(&mut v.borrow_mut(), OP_NEXT as i32, csr, addr_next);
    vdbe_jump_here(&mut v.borrow_mut(), addr_next - 1);
    vdbe_jump_here(&mut v.borrow_mut(), addr_next + 1);
    release_temp_reg(&mut p_parse.borrow_mut(), reg_rowid);
    release_temp_reg(&mut p_parse.borrow_mut(), reg_c_rowid);
    if n_peer != 0 {
        release_temp_range(&mut p_parse.borrow_mut(), reg_peer, n_peer);
        release_temp_range(&mut p_parse.borrow_mut(), reg_c_peer, n_peer);
    }

    window_agg_final(p, 1);
}


// ---- part_005.rs ----

// Tradução de window.c, parte 5 (SQLite 3.46.1).
//
// Convenções locais desta parte (os macros de cobertura VdbeCoverage*, testcase e
// VdbeModuleComment não existem neste porte, então somem):
//   - `p.p_parse` é um `ParseRef`, `p.p_vdbe` é um `VdbeRef` e `p.p_m_win` é um `&Window`
//     (lista de `Box<Window>` ligada por `p_next_win`, como na parte 6);
//   - a comparação `pFunc->zName==nth_valueName` do C (identidade de ponteiro) vira comparação
//     de bytes do nome com as constantes `NTH_VALUE_NAME`, `FIRST_VALUE_NAME`, `LEAD_NAME` e
//     `LAG_NAME` definidas na parte 1.

/// Invoca a sub-rotina em reg_gosub (gerada por código de select.c) para devolver a linha
/// corrente de Window.i_eph_csr. Se todas as funções de janela são agregados de janela que usam
/// a API padrão, uma única instrução OP_Gosub é tudo o que esta rotina gera. Código extra para
/// o processamento por linha só é gerado para as funções de janela embutidas:
///
///   nth_value()
///   first_value()
///   lag()
///   lead()
fn window_return_one_row(p: &WindowCodeArg) {
    let p_m_win = p.p_m_win;
    let v = p.p_vdbe.clone();

    if p_m_win.reg_start_rowid != 0 {
        window_full_scan(p);
    } else {
        let p_parse = p.p_parse.clone();
        let i_eph_csr_m = p_m_win.i_eph_csr;

        let mut cur: Option<&Window> = Some(p_m_win);
        while let Some(p_win) = cur {
            let p_func = p_win
                .p_w_func
                .clone()
                .expect("função de janela sem FuncDef");
            let z_name: &[u8] = p_func.z_name.as_slice();
            if z_name == NTH_VALUE_NAME || z_name == FIRST_VALUE_NAME {
                let csr = p_win.csr_app;
                let lbl = vdbe_make_label(&p_parse);
                let tmp_reg = get_temp_reg(&mut p_parse.borrow_mut());
                vdbe_add_op2(&mut v.borrow_mut(), OP_NULL as i32, 0, p_win.reg_result);

                if z_name == NTH_VALUE_NAME {
                    vdbe_add_op3(
                        &mut v.borrow_mut(),
                        OP_COLUMN as i32,
                        i_eph_csr_m,
                        p_win.i_arg_col + 1,
                        tmp_reg,
                    );
                    window_check_value(&p_parse, tmp_reg, 2);
                } else {
                    vdbe_add_op2(&mut v.borrow_mut(), OP_INTEGER as i32, 1, tmp_reg);
                }
                vdbe_add_op3(&mut v.borrow_mut(), OP_ADD as i32, tmp_reg, p_win.reg_app, tmp_reg);
                vdbe_add_op3(
                    &mut v.borrow_mut(),
                    OP_GT as i32,
                    p_win.reg_app + 1,
                    lbl,
                    tmp_reg,
                );
                vdbe_add_op3(&mut v.borrow_mut(), OP_SEEKROWID as i32, csr, 0, tmp_reg);
                vdbe_add_op3(
                    &mut v.borrow_mut(),
                    OP_COLUMN as i32,
                    csr,
                    p_win.i_arg_col,
                    p_win.reg_result,
                );
                vdbe_resolve_label(&v, lbl);
                release_temp_reg(&mut p_parse.borrow_mut(), tmp_reg);
            } else if z_name == LEAD_NAME || z_name == LAG_NAME {
                let n_arg = {
                    let owner = window_owner(p_win);
                    let n = owner
                        .borrow()
                        .x
                        .p_list
                        .as_ref()
                        .expect("função de janela sem lista de argumentos")
                        .n_expr;
                    n
                };
                let csr = p_win.csr_app;
                let lbl = vdbe_make_label(&p_parse);
                let tmp_reg = get_temp_reg(&mut p_parse.borrow_mut());
                let i_eph = i_eph_csr_m;

                if n_arg < 3 {
                    vdbe_add_op2(&mut v.borrow_mut(), OP_NULL as i32, 0, p_win.reg_result);
                } else {
                    vdbe_add_op3(
                        &mut v.borrow_mut(),
                        OP_COLUMN as i32,
                        i_eph,
                        p_win.i_arg_col + 2,
                        p_win.reg_result,
                    );
                }
                vdbe_add_op2(&mut v.borrow_mut(), OP_ROWID as i32, i_eph, tmp_reg);
                if n_arg < 2 {
                    let val = if z_name == LEAD_NAME { 1 } else { -1 };
                    vdbe_add_op2(&mut v.borrow_mut(), OP_ADDIMM as i32, tmp_reg, val);
                } else {
                    let op = if z_name == LEAD_NAME {
                        OP_ADD as i32
                    } else {
                        OP_SUBTRACT as i32
                    };
                    let tmp_reg2 = get_temp_reg(&mut p_parse.borrow_mut());
                    vdbe_add_op3(
                        &mut v.borrow_mut(),
                        OP_COLUMN as i32,
                        i_eph,
                        p_win.i_arg_col + 1,
                        tmp_reg2,
                    );
                    vdbe_add_op3(&mut v.borrow_mut(), op, tmp_reg2, tmp_reg, tmp_reg);
                    release_temp_reg(&mut p_parse.borrow_mut(), tmp_reg2);
                }

                vdbe_add_op3(&mut v.borrow_mut(), OP_SEEKROWID as i32, csr, lbl, tmp_reg);
                vdbe_add_op3(
                    &mut v.borrow_mut(),
                    OP_COLUMN as i32,
                    csr,
                    p_win.i_arg_col,
                    p_win.reg_result,
                );
                vdbe_resolve_label(&v, lbl);
                release_temp_reg(&mut p_parse.borrow_mut(), tmp_reg);
            }
            cur = p_win.p_next_win.as_deref();
        }
    }
    vdbe_add_op2(&mut v.borrow_mut(), OP_GOSUB as i32, p.reg_gosub, p.addr_gosub);
}

/// Gera código para zerar o registrador acumulador de cada função de janela da lista ligada
/// passada como segundo argumento. E realiza qualquer inicialização equivalente exigida pelas
/// funções de janela embutidas da lista.
fn window_init_accum(p_parse: &ParseRef, p_m_win: &Window) -> i32 {
    let v = get_vdbe(p_parse).expect("sem Vdbe");
    let mut n_arg: i32 = 0;
    let reg_start_rowid = p_m_win.reg_start_rowid;
    let mut cur: Option<&Window> = Some(p_m_win);
    while let Some(p_win) = cur {
        let p_func = p_win
            .p_w_func
            .clone()
            .expect("função de janela sem FuncDef");
        debug_assert!(p_win.reg_accum != 0);
        vdbe_add_op2(&mut v.borrow_mut(), OP_NULL as i32, 0, p_win.reg_accum);
        n_arg = n_arg.max(window_arg_count(p_win));
        if reg_start_rowid == 0 {
            let z_name: &[u8] = p_func.z_name.as_slice();
            if z_name == NTH_VALUE_NAME || z_name == FIRST_VALUE_NAME {
                vdbe_add_op2(&mut v.borrow_mut(), OP_INTEGER as i32, 0, p_win.reg_app);
                vdbe_add_op2(&mut v.borrow_mut(), OP_INTEGER as i32, 0, p_win.reg_app + 1);
            }

            if (p_func.func_flags & (SQLITE_FUNC_MINMAX as u32)) != 0 && p_win.csr_app != 0 {
                debug_assert!(p_win.e_start != TK_UNBOUNDED);
                vdbe_add_op1(&mut v.borrow_mut(), OP_RESETSORTER as i32, p_win.csr_app);
                vdbe_add_op2(&mut v.borrow_mut(), OP_INTEGER as i32, 0, p_win.reg_app + 1);
            }
        }
        cur = p_win.p_next_win.as_deref();
    }
    let reg_arg = p_parse.borrow().n_mem + 1;
    p_parse.borrow_mut().n_mem += n_arg;
    reg_arg
}

/// Devolve verdadeiro se o quadro corrente deve ser guardado na tabela temporária, mesmo que
/// não haja chamadas a xInverse() necessárias.
fn window_cache_frame(p_m_win: &Window) -> i32 {
    if p_m_win.reg_start_rowid != 0 {
        return 1;
    }
    let mut cur: Option<&Window> = Some(p_m_win);
    while let Some(p_win) = cur {
        let p_func = p_win
            .p_w_func
            .clone()
            .expect("função de janela sem FuncDef");
        let z_name: &[u8] = p_func.z_name.as_slice();
        if z_name == NTH_VALUE_NAME
            || z_name == FIRST_VALUE_NAME
            || z_name == LEAD_NAME
            || z_name == LAG_NAME
        {
            return 1;
        }
        cur = p_win.p_next_win.as_deref();
    }
    0
}

/// reg_old e reg_new são cada um o primeiro registrador de um array de tamanho
/// p_order_by.n_expr. Esta função gera código para comparar os dois arrays de registradores
/// usando as sequências de colação e os demais parâmetros de comparação de p_order_by.
///
/// Se os dois arrays não são iguais, o conteúdo de reg_new é copiado para reg_old e o controle
/// segue adiante. Caso contrário, se o conteúdo dos arrays é igual, um OP_Goto é executado.
fn window_if_new_peer(
    p_parse: &ParseRef,
    p_order_by: Option<&ExprList>,
    reg_new: i32,  // Primeiro do array de valores novos
    reg_old: i32,  // Primeiro do array de valores antigos
    addr: i32,     // Salta para cá
) {
    let v = get_vdbe(p_parse).expect("sem Vdbe");
    if let Some(p_order_by) = p_order_by {
        let n_val = p_order_by.n_expr;
        let p_key_info = key_info_from_expr_list(p_parse, p_order_by, 0, 0);
        vdbe_add_op3(&mut v.borrow_mut(), OP_COMPARE as i32, reg_old, reg_new, n_val);
        if let Some(ki) = p_key_info {
            vdbe_append_p4(&mut v.borrow_mut(), P4Value::KeyInfo(ki), P4_KEYINFO as i32);
        }
        let cur_addr = vdbe_current_addr(&v.borrow());
        vdbe_add_op3(&mut v.borrow_mut(), OP_JUMP as i32, cur_addr + 1, addr, cur_addr + 1);
        vdbe_add_op3(&mut v.borrow_mut(), OP_COPY as i32, reg_new, reg_old, n_val - 1);
    } else {
        vdbe_add_op2(&mut v.borrow_mut(), OP_GOTO as i32, 0, addr);
    }
}

/// Esta função é chamada como parte da geração de programas da VM para fronteiras de quadro
/// RANGE offset PRECEDING/FOLLOWING. Supondo ordem "ASC" para o termo ORDER BY da janela, e que
/// o argumento op seja OP_Ge, gera código equivalente a:
///
///   if( csr1.peerVal + regVal >= csr2.peerVal ) goto lbl;
///
/// O valor do parâmetro op também pode ser OP_Gt ou OP_Le. Nesses casos o operador do
/// pseudocódigo acima é trocado por ">" ou "<=", respectivamente.
///
/// Se a ordem do termo ORDER BY da janela é DESC, a comparação é invertida. Em vez de somar
/// reg_val a csr1.peerVal, ele é subtraído. E o operador de comparação é invertido: ">=" vira
/// "<=", ">" vira "<", e assim por diante. Então, com ordem DESC, se o argumento op é OP_Ge, o
/// código gerado equivale a:
///
///   if( csr1.peerVal - regVal <= csr2.peerVal ) goto lbl;
///
/// Usa-se uma aritmética especial: se csr1.peerVal não é de tipo numérico (real ou inteiro), o
/// resultado da soma ou subtração é uma cópia de csr1.peerVal.
fn window_code_range_test(
    p: &WindowCodeArg,
    op: i32,       // OP_Ge, OP_Gt ou OP_Le
    csr1: i32,     // Número do cursor 1
    reg_val: i32,  // Registrador com número não negativo
    csr2: i32,     // Número do cursor 2
    lbl: i32,      // Destino do salto se a condição é verdadeira
) {
    let mut op = op;
    let p_parse = p.p_parse.clone();
    let v = get_vdbe(&p_parse).expect("sem Vdbe");
    let p_m_win = p.p_m_win;
    let reg1 = get_temp_reg(&mut p_parse.borrow_mut()); // Reg. para csr1.peerVal+regVal
    let reg2 = get_temp_reg(&mut p_parse.borrow_mut()); // Reg. para csr2.peerVal
    let reg_string = {
        let mut pp = p_parse.borrow_mut();
        pp.n_mem += 1;
        pp.n_mem
    }; // Reg. para o valor constante ''
    let mut arith = OP_ADD as i32; // OP_Add ou OP_Subtract
    let addr_ge: i32; // Destino do salto
    let addr_done = vdbe_make_label(&p_parse); // Endereço depois do OP_Ge

    // Lê o valor de par de cada cursor num registrador
    window_read_peer_values(p, csr1, reg1);
    window_read_peer_values(p, csr2, reg2);

    debug_assert!(op == OP_GE as i32 || op == OP_GT as i32 || op == OP_LE as i32);
    let sort_flags: u8 = {
        let p_order_by = p_m_win.p_order_by.as_deref().expect("janela sem ORDER BY");
        debug_assert!(p_order_by.n_expr == 1);
        p_order_by.a[0].fg.sort_flags
    };
    if (sort_flags & (KEYINFO_ORDER_DESC as u8)) != 0 {
        if op == OP_GE as i32 {
            op = OP_LE as i32;
        } else if op == OP_GT as i32 {
            op = OP_LT as i32;
        } else {
            debug_assert!(op == OP_LE as i32);
            op = OP_GE as i32;
        }
        arith = OP_SUBTRACT as i32;
    }

    // Se o flag BIGNULL está ligado no ORDER BY, é preciso considerar os valores NULL maiores
    // que todos os outros, em vez do usual, menores. Os opcodes OP_Ge e afins da VDBE não
    // tratam isso (e acrescentar essa capacidade causa uma regressão de desempenho), então, se
    // o flag BIGNULL está ligado, os casos em que reg1 ou reg2 são NULL são tratados à parte no
    // bloco a seguir. O código gerado equivale a:
    //
    //   if( reg1 IS NULL ){
    //     if( op==OP_Ge ) goto lbl;
    //     if( op==OP_Gt && reg2 IS NOT NULL ) goto lbl;
    //     if( op==OP_Le && reg2 IS NULL ) goto lbl;
    //   }else if( reg2 IS NULL ){
    //     if( op==OP_Le ) goto lbl;
    //   }
    //
    // Além disso, se reg1 ou reg2 é NULL mas o salto para lbl não é tomado, o controle salta
    // por cima do operador de comparação codificado abaixo deste bloco.
    if (sort_flags & (KEYINFO_ORDER_BIGNULL as u8)) != 0 {
        // Este bloco roda se reg1 contém um NULL.
        let addr = vdbe_add_op1(&mut v.borrow_mut(), OP_NOTNULL as i32, reg1);
        if op == OP_GE as i32 {
            vdbe_add_op2(&mut v.borrow_mut(), OP_GOTO as i32, 0, lbl);
        } else if op == OP_GT as i32 {
            vdbe_add_op2(&mut v.borrow_mut(), OP_NOTNULL as i32, reg2, lbl);
        } else if op == OP_LE as i32 {
            vdbe_add_op2(&mut v.borrow_mut(), OP_ISNULL as i32, reg2, lbl);
        } else {
            debug_assert!(op == OP_LT as i32); // não faz nada
        }
        vdbe_add_op2(&mut v.borrow_mut(), OP_GOTO as i32, 0, addr_done);

        // Este bloco roda se reg1 não é NULL, mas reg2 é.
        vdbe_jump_here(&mut v.borrow_mut(), addr);
        vdbe_add_op2(
            &mut v.borrow_mut(),
            OP_ISNULL as i32,
            reg2,
            if op == OP_GT as i32 || op == OP_GE as i32 {
                addr_done
            } else {
                lbl
            },
        );
    }

    // O registrador reg1 contém agora csr1.peerVal (o valor de par de csr1). Este bloco soma (ou
    // subtrai, para DESC) o valor numérico de reg_val a ele. Ou, se reg1 não é numérico (é um
    // NULL, um texto ou um blob), deixa reg1 como está. Em pseudocódigo:
    //
    //   if( reg1>='' ) goto addrGe;
    //   reg1 = reg1 +/- regVal
    //   addrGe:
    //
    // Como todo texto e todo blob são maiores ou iguais a uma string vazia, a soma ou subtração
    // é pulada para eles, como exigido. Se reg1 é NULL, a aritmética é feita, mas somar ou
    // subtrair de NULL sempre dá NULL, então esse caso também sai como exigido.
    vdbe_add_op4(
        &mut v.borrow_mut(),
        OP_STRING8 as i32,
        0,
        reg_string,
        0,
        P4Value::Static(Vec::new()),
        P4_STATIC as i32,
    );
    addr_ge = vdbe_add_op3(&mut v.borrow_mut(), OP_GE as i32, reg_string, 0, reg1);
    if (op == OP_GE as i32 && arith == OP_ADD as i32)
        || (op == OP_LE as i32 && arith == OP_SUBTRACT as i32)
    {
        vdbe_add_op3(&mut v.borrow_mut(), op, reg2, lbl, reg1);
    }
    vdbe_add_op3(&mut v.borrow_mut(), arith, reg_val, reg1, reg1);
    vdbe_jump_here(&mut v.borrow_mut(), addr_ge);

    // Compara os registradores reg2 e reg1, tomando o salto se preciso. Note que o controle
    // pula este teste se o flag BIGNULL está ligado e reg1 ou reg2 contém NULL.
    vdbe_add_op3(&mut v.borrow_mut(), op, reg2, lbl, reg1);
    let p_coll = {
        let p_order_by = p_m_win.p_order_by.as_deref().expect("janela sem ORDER BY");
        let p_expr = p_order_by.a[0]
            .p_expr
            .as_deref()
            .expect("termo ORDER BY sem expressão");
        expr_nn_coll_seq(&mut p_parse.borrow_mut(), p_expr)
    };
    vdbe_append_p4(&mut v.borrow_mut(), P4Value::CollSeq(p_coll), P4_COLLSEQ as i32);
    vdbe_change_p5(&mut v.borrow_mut(), SQLITE_NULLEQ as u16);
    vdbe_resolve_label(&v, addr_done);

    debug_assert!(
        op == OP_GE as i32 || op == OP_GT as i32 || op == OP_LT as i32 || op == OP_LE as i32
    );
    release_temp_reg(&mut p_parse.borrow_mut(), reg1);
    release_temp_reg(&mut p_parse.borrow_mut(), reg2);
}

/// Função auxiliar de window_code_step(). Cada chamada a esta função gera código da VM para uma
/// única operação RETURN_ROW, AGGSTEP ou AGGINVERSE. Para detalhes, veja o comentário de
/// cabeçalho de window_code_step().
fn window_code_op(
    p: &WindowCodeArg,     // Objeto de contexto
    op: i32,               // WINDOW_RETURN_ROW, AGGSTEP ou AGGINVERSE
    reg_countdown: i32,    // Registrador para a contagem regressiva de OP_IfPos
    jump_on_eof: i32,      // Salta para cá se o cursor avançado chega ao EOF
) -> i32 {
    let csr: i32;
    let reg: i32;
    let p_parse = p.p_parse.clone();
    let p_m_win = p.p_m_win;
    let mut ret: i32 = 0;
    let v = p.p_vdbe.clone();
    let addr_continue: i32;
    let e_frm_type = p_m_win.e_frm_type;
    let e_start = p_m_win.e_start;
    let e_end = p_m_win.e_end;
    let b_peer: i32 = (e_frm_type != TK_ROWS) as i32;

    let lbl_done = vdbe_make_label(&p_parse);
    let mut addr_next_range: i32 = 0;

    // Caso especial: WINDOW_AGGINVERSE é sempre um no-op se o quadro começa com UNBOUNDED
    // PRECEDING.
    if op == WINDOW_AGGINVERSE && e_start == TK_UNBOUNDED {
        debug_assert!(reg_countdown == 0 && jump_on_eof == 0);
        return 0;
    }

    if reg_countdown > 0 {
        if e_frm_type == TK_RANGE {
            addr_next_range = vdbe_current_addr(&v.borrow());
            debug_assert!(op == WINDOW_AGGINVERSE || op == WINDOW_AGGSTEP);
            if op == WINDOW_AGGINVERSE {
                if e_start == TK_FOLLOWING {
                    window_code_range_test(p, OP_LE as i32, p.current.csr, reg_countdown, p.start.csr, lbl_done);
                } else {
                    window_code_range_test(p, OP_GE as i32, p.start.csr, reg_countdown, p.current.csr, lbl_done);
                }
            } else {
                window_code_range_test(p, OP_GT as i32, p.end.csr, reg_countdown, p.current.csr, lbl_done);
            }
        } else {
            vdbe_add_op3(&mut v.borrow_mut(), OP_IFPOS as i32, reg_countdown, lbl_done, 1);
        }
    }

    if op == WINDOW_RETURN_ROW && p_m_win.reg_start_rowid == 0 {
        window_agg_final(p, 0);
    }
    addr_continue = vdbe_current_addr(&v.borrow());

    // Se este é um quadro (RANGE BETWEEN a FOLLOWING AND b FOLLOWING) ou (RANGE BETWEEN b
    // PRECEDING AND a PRECEDING), garante que o cursor de início não avance além do cursor de
    // fim dentro da tabela temporária. Poderia avançar, se (a>b). Garante também que, se o
    // cursor de entrada ainda está achando linhas novas, o cursor de fim não passe dele até o
    // EOF.
    if e_start == e_end && reg_countdown != 0 && e_frm_type == TK_RANGE {
        let reg_rowid1 = get_temp_reg(&mut p_parse.borrow_mut());
        let reg_rowid2 = get_temp_reg(&mut p_parse.borrow_mut());
        if op == WINDOW_AGGINVERSE {
            vdbe_add_op2(&mut v.borrow_mut(), OP_ROWID as i32, p.start.csr, reg_rowid1);
            vdbe_add_op2(&mut v.borrow_mut(), OP_ROWID as i32, p.end.csr, reg_rowid2);
            vdbe_add_op3(&mut v.borrow_mut(), OP_GE as i32, reg_rowid2, lbl_done, reg_rowid1);
        } else if p.reg_rowid != 0 {
            vdbe_add_op2(&mut v.borrow_mut(), OP_ROWID as i32, p.end.csr, reg_rowid1);
            vdbe_add_op3(&mut v.borrow_mut(), OP_GE as i32, p.reg_rowid, lbl_done, reg_rowid1);
        }
        release_temp_reg(&mut p_parse.borrow_mut(), reg_rowid1);
        release_temp_reg(&mut p_parse.borrow_mut(), reg_rowid2);
        debug_assert!(e_start == TK_PRECEDING || e_start == TK_FOLLOWING);
    }

    if op == WINDOW_RETURN_ROW {
        csr = p.current.csr;
        reg = p.current.reg;
        window_return_one_row(p);
    } else if op == WINDOW_AGGINVERSE {
        csr = p.start.csr;
        reg = p.start.reg;
        let reg_start_rowid = p_m_win.reg_start_rowid;
        if reg_start_rowid != 0 {
            debug_assert!(p_m_win.reg_end_rowid != 0);
            vdbe_add_op2(&mut v.borrow_mut(), OP_ADDIMM as i32, reg_start_rowid, 1);
        } else {
            window_agg_step(p, p_m_win, csr, 1, p.reg_arg);
        }
    } else {
        debug_assert!(op == WINDOW_AGGSTEP);
        csr = p.end.csr;
        reg = p.end.reg;
        let (reg_start_rowid, reg_end_rowid) = (p_m_win.reg_start_rowid, p_m_win.reg_end_rowid);
        if reg_start_rowid != 0 {
            debug_assert!(reg_end_rowid != 0);
            vdbe_add_op2(&mut v.borrow_mut(), OP_ADDIMM as i32, reg_end_rowid, 1);
        } else {
            window_agg_step(p, p_m_win, csr, 0, p.reg_arg);
        }
    }

    if op == p.e_delete {
        vdbe_add_op1(&mut v.borrow_mut(), OP_DELETE as i32, csr);
        vdbe_change_p5(&mut v.borrow_mut(), OPFLAG_SAVEPOSITION as u16);
    }

    if jump_on_eof != 0 {
        let cur_addr = vdbe_current_addr(&v.borrow());
        vdbe_add_op2(&mut v.borrow_mut(), OP_NEXT as i32, csr, cur_addr + 2);
        ret = vdbe_add_op0(&mut v.borrow_mut(), OP_GOTO as i32);
    } else {
        let cur_addr = vdbe_current_addr(&v.borrow());
        vdbe_add_op2(&mut v.borrow_mut(), OP_NEXT as i32, csr, cur_addr + 1 + b_peer);
        if b_peer != 0 {
            vdbe_add_op2(&mut v.borrow_mut(), OP_GOTO as i32, 0, lbl_done);
        }
    }

    if b_peer != 0 {
        let n_reg: i32 = match p_m_win.p_order_by.as_deref() {
            Some(ob) => ob.n_expr,
            None => 0,
        };
        let reg_tmp = if n_reg != 0 {
            get_temp_range(&mut p_parse.borrow_mut(), n_reg)
        } else {
            0
        };
        window_read_peer_values(p, csr, reg_tmp);
        window_if_new_peer(&p_parse, p_m_win.p_order_by.as_deref(), reg_tmp, reg, addr_continue);
        release_temp_range(&mut p_parse.borrow_mut(), reg_tmp, n_reg);
    }

    if addr_next_range != 0 {
        vdbe_add_op2(&mut v.borrow_mut(), OP_GOTO as i32, 0, addr_next_range);
    }
    vdbe_resolve_label(&v, lbl_done);
    ret
}


// ---- part_006.rs ----

// Tradução de window.c, parte 6 (SQLite 3.46.1): window_dup(), window_list_dup() e
// window_expr_gt_zero(). O trecho termina no meio do comentário de cabeçalho de
// window_code_op(), cuja tradução continua na parte seguinte.

/// Aloca e retorna uma duplicata do objeto Window indicado pelo terceiro argumento.
/// Define o campo Window.p_owner do novo objeto como `p_owner`.
pub fn window_dup(
    db: &Sqlite3Ref,
    p_owner: Option<Weak<RefCell<Expr>>>,
    p: Option<&Window>,
) -> Option<Box<Window>> {
    let mut p_new: Option<Box<Window>> = None;
    // ALWAYS(p)
    if let Some(p) = p {
        let z_name = match &p.z_name {
            Some(z) => Some(db_str_dup(&mut db.borrow_mut(), z)),
            None => None,
        };
        let z_base = match &p.z_base {
            Some(z) => Some(db_str_dup(&mut db.borrow_mut(), z)),
            None => None,
        };
        let p_filter = expr_dup(db, p.p_filter.as_deref(), 0);
        let p_w_func = p.p_w_func.clone();
        let p_partition = expr_list_dup(db, p.p_partition.as_deref(), 0);
        let p_order_by = expr_list_dup(db, p.p_order_by.as_deref(), 0);
        let p_start = expr_dup(db, p.p_start.as_deref(), 0);
        let p_end = expr_dup(db, p.p_end.as_deref(), 0);
        p_new = Some(Box::new(Window {
            z_name,
            z_base,
            p_partition,
            p_order_by,
            e_frm_type: p.e_frm_type,
            e_start: p.e_start,
            e_end: p.e_end,
            b_implicit_frame: p.b_implicit_frame,
            e_exclude: p.e_exclude,
            p_start,
            p_end,
            p_next_win: None,
            p_filter,
            p_w_func,
            i_eph_csr: p.i_eph_csr,
            reg_accum: p.reg_accum,
            reg_result: p.reg_result,
            csr_app: 0,
            reg_app: 0,
            reg_part: 0,
            p_owner,
            n_buffer_col: 0,
            i_arg_col: p.i_arg_col,
            reg_one: 0,
            reg_start_rowid: 0,
            reg_end_rowid: 0,
            b_expr_args: p.b_expr_args,
        }));
    }
    p_new
}

/// Retorna uma cópia da lista ligada de objetos Window passada como segundo argumento.
pub fn window_list_dup(db: &Sqlite3Ref, p: Option<&Window>) -> Option<Box<Window>> {
    // `pp` do C percorre os campos p_next_win da lista nova; aqui as cópias são
    // acumuladas na ordem e depois encadeadas do fim para o começo.
    let mut copies: Vec<Box<Window>> = Vec::new();

    let mut p_win = p;
    while let Some(w) = p_win {
        match window_dup(db, None, Some(w)) {
            Some(n) => copies.push(n),
            None => break,
        }
        p_win = w.p_next_win.as_deref();
    }

    let mut p_ret: Option<Box<Window>> = None;
    while let Some(mut n) = copies.pop() {
        n.p_next_win = p_ret;
        p_ret = Some(n);
    }
    p_ret
}

/// Retorna verdadeiro se for possível determinar em tempo de compilação que a expressão
/// `p_expr` avalia para um valor que, convertido para inteiro, é maior que zero. Falso
/// caso contrário.
///
/// Se ocorrer um erro de OOM, esta função define o flag Parse.db.mallocFailed e retorna zero.
fn window_expr_gt_zero(p_parse: &Parse, p_expr: Option<&Expr>) -> i32 {
    let mut ret: i32 = 0;
    let db = match p_parse.db.upgrade() {
        Some(db) => db,
        None => return 0,
    };
    let mut p_val: Option<Box<Mem>> = None;
    let enc = db.borrow().enc;
    value_from_expr(&db, p_expr, enc, SQLITE_AFF_NUMERIC, &mut p_val);
    if let Some(v) = p_val.as_deref() {
        if api::value_int(v) > 0 {
            ret = 1;
        }
    }
    value_free(p_val);
    ret
}


// ---- part_007.rs ----

// Tradução de window.c, parte 7 (SQLite 3.46.1): o fim do comentário que descreve os esquemas de
// código de `window_code_step()` e a própria função.
//
// Premissas sobre os tipos das partes vizinhas (nomes pela convenção):
//   - `WindowCodeArg<'a>` (parte 4) tem os campos `p_parse: ParseRef`, `p_m_win: &'a Window`,
//     `p_vdbe: VdbeRef`, `addr_gosub`, `reg_gosub`, `reg_arg`, `e_delete`, `reg_rowid` (todos
//     `i32`) e `start`, `current`, `end` do tipo `WindowCsrAndReg { csr: i32, reg: i32 }`.
//   - Todas as rotinas auxiliares das partes 4 e 5 recebem `&WindowCodeArg` (aqui passa-se `&mut s`
//     ou `&s`, que coagem) e `window_code_op(.., e_op, n_reg, b_ifnull) -> i32`;
//     `window_init_accum(&ParseRef, &Window) -> i32`, `window_check_value(&ParseRef, reg, e_cond)`,
//     `window_cache_frame(&Window) -> i32`,
//     `window_if_new_peer(&ParseRef, Option<&ExprList>, reg_new, reg_old, addr_goto)`.
//   - As macros `VdbeCoverage*` e `VdbeComment` não geram nada nesta compilação (sem
//     SQLITE_DEBUG nem SQLITE_ENABLE_EXPLAIN_COMMENTS) e por isso somem da tradução.
//
//   RANGE BETWEEN <expr1> PRECEDING AND <expr2> PRECEDING (continuação do esquema em pseudocódigo
//   que começa na parte anterior):
//
//         Rewind(csrEnd) ; Rewind(csrStart) ; Rewind(csrCurrent)
//         regEnd = <expr2>
//         regStart = <expr1>
//       }else{
//         while( (csrEnd.key + regEnd) <= csrCurrent.key ){
//           AGGSTEP
//         }
//         while( (csrStart.key + regStart) < csrCurrent.key ){
//           AGGINVERSE
//         }
//         RETURN_ROW
//       }
//     }
//     flush:
//       while( (csrEnd.key + regEnd) <= csrCurrent.key ){
//         AGGSTEP
//       }
//       while( (csrStart.key + regStart) < csrCurrent.key ){
//         AGGINVERSE
//       }
//       RETURN_ROW
//
//   RANGE BETWEEN <expr1> FOLLOWING AND <expr2> FOLLOWING
//
//     ... loop started by where_begin() ...
//       if( new partition ){
//         Gosub flush
//       }
//       Insert new row into eph table.
//       if( first row of partition ){
//         Rewind(csrEnd) ; Rewind(csrStart) ; Rewind(csrCurrent)
//         regEnd = <expr2>
//         regStart = <expr1>
//       }else{
//         AGGSTEP
//         while( (csrCurrent.key + regEnd) < csrEnd.key ){
//           while( (csrCurrent.key + regStart) > csrStart.key ){
//             AGGINVERSE
//           }
//           RETURN_ROW
//         }
//       }
//     }
//     flush:
//       AGGSTEP
//       while( 1 ){
//         while( (csrCurrent.key + regStart) > csrStart.key ){
//           AGGINVERSE
//           if( eof ) break "while( 1 )" loop.
//         }
//         RETURN_ROW
//       }
//       while( !eof csrCurrent ){
//         RETURN_ROW
//       }
//
// O texto acima omite muitos detalhes. Consulte o código e os comentários abaixo para um
// quadro mais completo.

/// Gera o código da etapa de processamento das funções de janela de um SELECT reescrito.
///
/// Parâmetros: `p_parse` é o contexto de parsing, `p` o SELECT reescrito, `p_w_info` o contexto
/// devolvido por `where_begin()`, `reg_gosub` o registro do OP_Gosub e `addr_gosub` o endereço
/// para onde o OP_Gosub devolve cada linha.
pub fn window_code_step(
    p_parse: &ParseRef,
    p: &Select,
    p_w_info: WhereInfoRef,
    reg_gosub: i32,
    addr_gosub: i32,
) {
    let p_m_win: &Window = p.p_win.as_deref().unwrap();
    let p_order_by: Option<&ExprList> = p_m_win.p_order_by.as_deref();
    let v: VdbeRef = get_vdbe(p_parse).unwrap();
    let csr_write: i32; // Cursor usado para escrever na tabela efêmera
    let p_src_item = &p.p_src.as_ref().unwrap().a[0];
    let csr_input: i32 = p_src_item.i_cursor; // Cursor da subconsulta
    let n_input: i32 = p_src_item.p_tab.as_ref().unwrap().borrow().n_col as i32; // Colunas devolvidas pela subconsulta
    let mut i_input: i32; // Para percorrer as colunas da subconsulta
    let addr_ne: i32; // Endereço do OP_Ne
    let mut addr_gosub_flush: i32 = 0; // Endereço do OP_Gosub para o flush
    let mut addr_integer: i32 = 0; // Endereço do OP_Integer
    let addr_empty: i32; // Endereço do OP_Rewind em flush:
    let reg_new: i32; // Array de registros com a nova linha de entrada
    let reg_record: i32; // O array regNew em forma de registro
    let mut reg_new_peer: i32 = 0; // Valores de pares da nova linha (parte de regNew)
    let mut reg_peer: i32 = 0; // Valores de pares da linha corrente
    let mut reg_flush_part: i32 = 0; // Registro do "Gosub flush_partition"
    let lbl_where_end: i32; // Rótulo logo antes do código de where_end()
    let mut reg_start: i32 = 0; // Valor de <expr> PRECEDING
    let mut reg_end: i32 = 0; // Valor de <expr> FOLLOWING

    debug_assert!(
        p_m_win.e_start == TK_PRECEDING
            || p_m_win.e_start == TK_CURRENT
            || p_m_win.e_start == TK_FOLLOWING
            || p_m_win.e_start == TK_UNBOUNDED
    );
    debug_assert!(
        p_m_win.e_end == TK_FOLLOWING
            || p_m_win.e_end == TK_CURRENT
            || p_m_win.e_end == TK_UNBOUNDED
            || p_m_win.e_end == TK_PRECEDING
    );
    debug_assert!(
        p_m_win.e_exclude == 0
            || p_m_win.e_exclude == TK_CURRENT
            || p_m_win.e_exclude == TK_GROUP
            || p_m_win.e_exclude == TK_TIES
            || p_m_win.e_exclude == TK_NO
    );

    lbl_where_end = vdbe_make_label(p_parse);

    // Preenche o objeto de contexto
    let mut s = WindowCodeArg {
        p_parse: p_parse.clone(),
        p_m_win,
        p_vdbe: v.clone(),
        addr_gosub,
        reg_gosub,
        reg_arg: 0,
        e_delete: 0,
        reg_rowid: 0,
        start: WindowCsrAndReg { csr: 0, reg: 0 },
        current: WindowCsrAndReg { csr: 0, reg: 0 },
        end: WindowCsrAndReg { csr: 0, reg: 0 },
    };
    s.current.csr = p_m_win.i_eph_csr;
    csr_write = s.current.csr + 1;
    s.start.csr = s.current.csr + 2;
    s.end.csr = s.current.csr + 3;

    // Descobre quando as linhas podem ser apagadas da tabela efêmera. Há quatro opções: nunca
    // (eDelete==0), assim que deixam o frame da janela (eDelete==WINDOW_AGGINVERSE), depois que
    // a linha foi devolvida ao chamador (WINDOW_RETURN_ROW), ou depois que entram no frame
    // (WINDOW_AGGSTEP).
    match p_m_win.e_start {
        TK_FOLLOWING => {
            if p_m_win.e_frm_type != TK_RANGE
                && window_expr_gt_zero(&p_parse.borrow(), p_m_win.p_start.as_deref()) != 0
            {
                s.e_delete = WINDOW_RETURN_ROW;
            }
        }
        TK_UNBOUNDED => {
            if window_cache_frame(p_m_win) == 0 {
                if p_m_win.e_end == TK_PRECEDING {
                    if p_m_win.e_frm_type != TK_RANGE
                        && window_expr_gt_zero(&p_parse.borrow(), p_m_win.p_end.as_deref()) != 0
                    {
                        s.e_delete = WINDOW_AGGSTEP;
                    }
                } else {
                    s.e_delete = WINDOW_RETURN_ROW;
                }
            }
        }
        _ => {
            s.e_delete = WINDOW_AGGINVERSE;
        }
    }

    // Aloca registros para o array de valores da subconsulta, os mesmos valores em forma de
    // registro, e o rowid usado para inserir esse registro na tabela efêmera.
    {
        let mut pp = p_parse.borrow_mut();
        reg_new = pp.n_mem + 1;
        pp.n_mem += n_input;
        pp.n_mem += 1;
        reg_record = pp.n_mem;
        pp.n_mem += 1;
        s.reg_rowid = pp.n_mem;

        // Se o frame da janela contém uma cláusula "<expr> PRECEDING" ou "<expr> FOLLOWING",
        // aloca registros para guardar o resultado da avaliação de cada <expr>.
        if p_m_win.e_start == TK_PRECEDING || p_m_win.e_start == TK_FOLLOWING {
            pp.n_mem += 1;
            reg_start = pp.n_mem;
        }
        if p_m_win.e_end == TK_PRECEDING || p_m_win.e_end == TK_FOLLOWING {
            pp.n_mem += 1;
            reg_end = pp.n_mem;
        }

        // Se não é um frame "ROWS BETWEEN ...", aloca arrays de registros para guardar cópias das
        // expressões do ORDER BY (valores de pares) para o laço principal e para cada cursor
        // (start, current e end).
        if p_m_win.e_frm_type != TK_ROWS {
            let n_peer: i32 = match p_order_by {
                Some(ob) => ob.n_expr,
                None => 0,
            };
            reg_new_peer = reg_new + p_m_win.n_buffer_col;
            if let Some(part) = p_m_win.p_partition.as_deref() {
                reg_new_peer += part.n_expr;
            }
            reg_peer = pp.n_mem + 1;
            pp.n_mem += n_peer;
            s.start.reg = pp.n_mem + 1;
            pp.n_mem += n_peer;
            s.current.reg = pp.n_mem + 1;
            pp.n_mem += n_peer;
            s.end.reg = pp.n_mem + 1;
            pp.n_mem += n_peer;
        }
    }

    // Carrega os valores das colunas da linha devolvida pela subconsulta num array de registros
    // que começa em regNew e os monta num registro em regRecord.
    i_input = 0;
    while i_input < n_input {
        vdbe_add_op3(&mut v.borrow_mut(), OP_COLUMN as i32, csr_input, i_input, reg_new + i_input);
        i_input += 1;
    }
    vdbe_add_op3(&mut v.borrow_mut(), OP_MAKERECORD as i32, reg_new, n_input, reg_record);

    // Uma linha de entrada acabou de ser lida num array de registros que começa em regNew. Se a
    // janela tem PARTITION, este bloco gera o código que confere se a linha inicia uma nova
    // partição. Se sim, faz um OP_Gosub para um endereço preenchido depois. O endereço do
    // OP_Gosub fica na variável local addrGosubFlush.
    if let Some(p_part) = p_m_win.p_partition.as_deref() {
        let addr: i32;
        let n_part: i32 = p_part.n_expr;
        let reg_new_part: i32 = reg_new + p_m_win.n_buffer_col;
        let p_key_info: Option<KeyInfoRef> = key_info_from_expr_list(p_parse, p_part, 0, 0);

        {
            let mut pp = p_parse.borrow_mut();
            pp.n_mem += 1;
            reg_flush_part = pp.n_mem;
        }
        {
            let mut vm = v.borrow_mut();
            addr = vdbe_add_op3(&mut vm, OP_COMPARE as i32, reg_new_part, p_m_win.reg_part, n_part);
            if let Some(ki) = p_key_info {
                vdbe_append_p4(&mut vm, P4Value::KeyInfo(ki), P4_KEYINFO as i32);
            }
            vdbe_add_op3(&mut vm, OP_JUMP as i32, addr + 2, addr + 4, addr + 2);
            addr_gosub_flush = vdbe_add_op1(&mut vm, OP_GOSUB as i32, reg_flush_part);
            vdbe_add_op3(&mut vm, OP_COPY as i32, reg_new_part, p_m_win.reg_part, n_part - 1);
        }
    }

    // Insere a nova linha na tabela efêmera
    vdbe_add_op2(&mut v.borrow_mut(), OP_NEWROWID as i32, csr_write, s.reg_rowid);
    vdbe_add_op3(&mut v.borrow_mut(), OP_INSERT as i32, csr_write, reg_record, s.reg_rowid);
    addr_ne = vdbe_add_op3(&mut v.borrow_mut(), OP_NE as i32, p_m_win.reg_one, 0, s.reg_rowid);

    // Este bloco roda para a primeira linha de cada partição
    s.reg_arg = window_init_accum(p_parse, p_m_win);

    if reg_start != 0 {
        {
            let mut pp = p_parse.borrow_mut();
            expr_code(&mut pp, p_m_win.p_start.as_deref(), reg_start);
        }
        window_check_value(p_parse, reg_start, 0 + (if p_m_win.e_frm_type == TK_RANGE { 3 } else { 0 }));
    }
    if reg_end != 0 {
        {
            let mut pp = p_parse.borrow_mut();
            expr_code(&mut pp, p_m_win.p_end.as_deref(), reg_end);
        }
        window_check_value(p_parse, reg_end, 1 + (if p_m_win.e_frm_type == TK_RANGE { 3 } else { 0 }));
    }

    if p_m_win.e_frm_type != TK_RANGE && p_m_win.e_start == p_m_win.e_end && reg_start != 0 {
        let op: i32 = if p_m_win.e_start == TK_FOLLOWING { OP_GE as i32 } else { OP_LE as i32 };
        let addr_ge: i32 = vdbe_add_op3(&mut v.borrow_mut(), op, reg_start, 0, reg_end);
        // NeverNull porque os valores limite <expr> já foram conferidos.
        window_agg_final(&mut s, 0);
        vdbe_add_op1(&mut v.borrow_mut(), OP_REWIND as i32, s.current.csr);
        window_return_one_row(&mut s);
        vdbe_add_op1(&mut v.borrow_mut(), OP_RESETSORTER as i32, s.current.csr);
        vdbe_add_op2(&mut v.borrow_mut(), OP_GOTO as i32, 0, lbl_where_end);
        vdbe_jump_here(&mut v.borrow_mut(), addr_ge);
    }
    if p_m_win.e_start == TK_FOLLOWING && p_m_win.e_frm_type != TK_RANGE && reg_end != 0 {
        debug_assert!(p_m_win.e_end == TK_FOLLOWING);
        vdbe_add_op3(&mut v.borrow_mut(), OP_SUBTRACT as i32, reg_start, reg_end, reg_start);
    }

    if p_m_win.e_start != TK_UNBOUNDED {
        vdbe_add_op1(&mut v.borrow_mut(), OP_REWIND as i32, s.start.csr);
    }
    vdbe_add_op1(&mut v.borrow_mut(), OP_REWIND as i32, s.current.csr);
    vdbe_add_op1(&mut v.borrow_mut(), OP_REWIND as i32, s.end.csr);
    if reg_peer != 0 {
        if let Some(ob) = p_order_by {
            vdbe_add_op3(&mut v.borrow_mut(), OP_COPY as i32, reg_new_peer, reg_peer, ob.n_expr - 1);
            vdbe_add_op3(&mut v.borrow_mut(), OP_COPY as i32, reg_peer, s.start.reg, ob.n_expr - 1);
            vdbe_add_op3(&mut v.borrow_mut(), OP_COPY as i32, reg_peer, s.current.reg, ob.n_expr - 1);
            vdbe_add_op3(&mut v.borrow_mut(), OP_COPY as i32, reg_peer, s.end.reg, ob.n_expr - 1);
        }
    }

    vdbe_add_op2(&mut v.borrow_mut(), OP_GOTO as i32, 0, lbl_where_end);

    vdbe_jump_here(&mut v.borrow_mut(), addr_ne);

    // Início do bloco executado para a segunda linha e as seguintes.
    if reg_peer != 0 {
        window_if_new_peer(p_parse, p_order_by, reg_new_peer, reg_peer, lbl_where_end);
    }
    if p_m_win.e_start == TK_FOLLOWING {
        window_code_op(&mut s, WINDOW_AGGSTEP, 0, 0);
        if p_m_win.e_end != TK_UNBOUNDED {
            if p_m_win.e_frm_type == TK_RANGE {
                let lbl: i32 = vdbe_make_label(p_parse);
                let addr_next: i32 = vdbe_current_addr(&v.borrow());
                window_code_range_test(&s, OP_GE as i32, s.current.csr, reg_end, s.end.csr, lbl);
                window_code_op(&mut s, WINDOW_AGGINVERSE, reg_start, 0);
                window_code_op(&mut s, WINDOW_RETURN_ROW, 0, 0);
                vdbe_add_op2(&mut v.borrow_mut(), OP_GOTO as i32, 0, addr_next);
                vdbe_resolve_label(&v, lbl);
            } else {
                window_code_op(&mut s, WINDOW_RETURN_ROW, reg_end, 0);
                window_code_op(&mut s, WINDOW_AGGINVERSE, reg_start, 0);
            }
        }
    } else if p_m_win.e_end == TK_PRECEDING {
        let b_rps: bool = p_m_win.e_start == TK_PRECEDING && p_m_win.e_frm_type == TK_RANGE;
        window_code_op(&mut s, WINDOW_AGGSTEP, reg_end, 0);
        if b_rps {
            window_code_op(&mut s, WINDOW_AGGINVERSE, reg_start, 0);
        }
        window_code_op(&mut s, WINDOW_RETURN_ROW, 0, 0);
        if !b_rps {
            window_code_op(&mut s, WINDOW_AGGINVERSE, reg_start, 0);
        }
    } else {
        let mut addr: i32 = 0;
        window_code_op(&mut s, WINDOW_AGGSTEP, 0, 0);
        if p_m_win.e_end != TK_UNBOUNDED {
            if p_m_win.e_frm_type == TK_RANGE {
                let mut lbl: i32 = 0;
                addr = vdbe_current_addr(&v.borrow());
                if reg_end != 0 {
                    lbl = vdbe_make_label(p_parse);
                    window_code_range_test(&s, OP_GE as i32, s.current.csr, reg_end, s.end.csr, lbl);
                }
                window_code_op(&mut s, WINDOW_RETURN_ROW, 0, 0);
                window_code_op(&mut s, WINDOW_AGGINVERSE, reg_start, 0);
                if reg_end != 0 {
                    vdbe_add_op2(&mut v.borrow_mut(), OP_GOTO as i32, 0, addr);
                    vdbe_resolve_label(&v, lbl);
                }
            } else {
                if reg_end != 0 {
                    addr = vdbe_add_op3(&mut v.borrow_mut(), OP_IFPOS as i32, reg_end, 0, 1);
                }
                window_code_op(&mut s, WINDOW_RETURN_ROW, 0, 0);
                window_code_op(&mut s, WINDOW_AGGINVERSE, reg_start, 0);
                if reg_end != 0 {
                    vdbe_jump_here(&mut v.borrow_mut(), addr);
                }
            }
        }
    }

    // Fim do laço principal de entrada
    vdbe_resolve_label(&v, lbl_where_end);
    where_end(p_w_info);

    // Segue em frente (fall through)
    if p_m_win.p_partition.is_some() {
        addr_integer = vdbe_add_op2(&mut v.borrow_mut(), OP_INTEGER as i32, 0, reg_flush_part);
        vdbe_jump_here(&mut v.borrow_mut(), addr_gosub_flush);
    }

    s.reg_rowid = 0;
    addr_empty = vdbe_add_op1(&mut v.borrow_mut(), OP_REWIND as i32, csr_write);
    if p_m_win.e_end == TK_PRECEDING {
        let b_rps: bool = p_m_win.e_start == TK_PRECEDING && p_m_win.e_frm_type == TK_RANGE;
        window_code_op(&mut s, WINDOW_AGGSTEP, reg_end, 0);
        if b_rps {
            window_code_op(&mut s, WINDOW_AGGINVERSE, reg_start, 0);
        }
        window_code_op(&mut s, WINDOW_RETURN_ROW, 0, 0);
    } else if p_m_win.e_start == TK_FOLLOWING {
        let mut addr_start: i32;
        let addr_break1: i32;
        let addr_break2: i32;
        let addr_break3: i32;
        window_code_op(&mut s, WINDOW_AGGSTEP, 0, 0);
        if p_m_win.e_frm_type == TK_RANGE {
            addr_start = vdbe_current_addr(&v.borrow());
            addr_break2 = window_code_op(&mut s, WINDOW_AGGINVERSE, reg_start, 1);
            addr_break1 = window_code_op(&mut s, WINDOW_RETURN_ROW, 0, 1);
        } else if p_m_win.e_end == TK_UNBOUNDED {
            addr_start = vdbe_current_addr(&v.borrow());
            addr_break1 = window_code_op(&mut s, WINDOW_RETURN_ROW, reg_start, 1);
            addr_break2 = window_code_op(&mut s, WINDOW_AGGINVERSE, 0, 1);
        } else {
            debug_assert!(p_m_win.e_end == TK_FOLLOWING);
            addr_start = vdbe_current_addr(&v.borrow());
            addr_break1 = window_code_op(&mut s, WINDOW_RETURN_ROW, reg_end, 1);
            addr_break2 = window_code_op(&mut s, WINDOW_AGGINVERSE, reg_start, 1);
        }
        vdbe_add_op2(&mut v.borrow_mut(), OP_GOTO as i32, 0, addr_start);
        vdbe_jump_here(&mut v.borrow_mut(), addr_break2);
        addr_start = vdbe_current_addr(&v.borrow());
        addr_break3 = window_code_op(&mut s, WINDOW_RETURN_ROW, 0, 1);
        vdbe_add_op2(&mut v.borrow_mut(), OP_GOTO as i32, 0, addr_start);
        vdbe_jump_here(&mut v.borrow_mut(), addr_break1);
        vdbe_jump_here(&mut v.borrow_mut(), addr_break3);
    } else {
        let addr_break: i32;
        let addr_start: i32;
        window_code_op(&mut s, WINDOW_AGGSTEP, 0, 0);
        addr_start = vdbe_current_addr(&v.borrow());
        addr_break = window_code_op(&mut s, WINDOW_RETURN_ROW, 0, 1);
        window_code_op(&mut s, WINDOW_AGGINVERSE, reg_start, 0);
        vdbe_add_op2(&mut v.borrow_mut(), OP_GOTO as i32, 0, addr_start);
        vdbe_jump_here(&mut v.borrow_mut(), addr_break);
    }
    vdbe_jump_here(&mut v.borrow_mut(), addr_empty);

    vdbe_add_op1(&mut v.borrow_mut(), OP_RESETSORTER as i32, s.current.csr);
    if p_m_win.p_partition.is_some() {
        if p_m_win.reg_start_rowid != 0 {
            vdbe_add_op2(&mut v.borrow_mut(), OP_INTEGER as i32, 1, p_m_win.reg_start_rowid);
            vdbe_add_op2(&mut v.borrow_mut(), OP_INTEGER as i32, 0, p_m_win.reg_end_rowid);
        }
        let cur: i32 = vdbe_current_addr(&v.borrow());
        vdbe_change_p1(&mut v.borrow_mut(), addr_integer, cur);
        vdbe_add_op1(&mut v.borrow_mut(), OP_RETURN as i32, reg_flush_part);
    }
}


// ---- part_008.rs ----

// O trecho window_c.008.c contém apenas o `#endif` que fecha o bloco
// SQLITE_OMIT_WINDOWFUNC (opção não definida no Debian 13). Não há funções a traduzir.

