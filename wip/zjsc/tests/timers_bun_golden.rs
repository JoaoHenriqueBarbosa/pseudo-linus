//! Golden de `setTimeout`, `setInterval`, `setImmediate` e `clear*` contra o bun: `tests/golden/timers_bun.tsv` sai de
//! `scripts/gen-timers-golden.js`. Cobre a forma dos globais e dos objetos `Timeout`/`Immediate`, os erros de argumento,
//! a normalização do atraso, a ordem entre timers, immediates, microtasks e promessas (sequências de três itens e
//! aninhamentos), intervalos, `clear*` entre famílias, `refresh`/`ref`/`unref`, `this` e argumentos extras. O log é a
//! string global `R`, lida depois que o laço de eventos virtual (`timers.rs`) esvaziou; cada programa leva um timer
//! sentinela de 30 ms que mantém o laço vivo, como no gerador.
mod common;

use zjsc::api::eval::evaluate_script_sequence_result_running_timers;

const GOLDEN: &str = include_str!("golden/timers_bun.tsv");
const PRELUDES: &str = include_str!("golden/microtask_order.preludes.json");

#[test]
fn timers_match_bun() {
    common::run_factored_big_stack(GOLDEN, PRELUDES, 25, |source| {
        evaluate_script_sequence_result_running_timers(&[source], "timers_case.js", "globalThis.R").1
    });
}
