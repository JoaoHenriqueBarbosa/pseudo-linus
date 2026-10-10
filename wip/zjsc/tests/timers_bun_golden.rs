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

/// A volta que rodou immediates fecha com a fase de timers da volta seguinte antes de o laço sair: o timer sem `ref` do
/// `AbortSignal.timeout(0)` vence ali e aborta; o de 5 ms não, e sem immediate nenhum o de 0 também não (medido no bun
/// 1.4.2, sem sentinela que mantenha o laço vivo).
#[test]
fn unref_timer_due_after_an_immediate_pass_still_fires() {
    let cases = [
        ("0", "setImmediate(() => log.push('imm'));", r#"["imm","abort"]"#),
        ("0", "setImmediate(() => { log.push('imm'); setImmediate(() => log.push('imm2')) });", r#"["imm","abort","imm2"]"#),
        ("0", "setTimeout(() => log.push('to0'), 0);", r#"["abort","to0"]"#),
        ("5", "setImmediate(() => log.push('imm'));", r#"["imm"]"#),
        ("0", "", "[]"),
    ];
    for (milliseconds, rest, expected) in cases {
        let source = format!(
            "var log = []; Object.defineProperty(globalThis, 'R', {{ get() {{ return JSON.stringify(log) }} }}); \
             AbortSignal.timeout({milliseconds}).addEventListener('abort', () => log.push('abort')); {rest}"
        );
        let result = evaluate_script_sequence_result_running_timers(&[&source], "timers_case.js", "globalThis.R").1;
        let text = result.map(|value| String::from_utf8_lossy(&value.to_wtf_string().utf8(zjsc::wtf::text::conversion_mode::ConversionMode::LenientConversion)).into_owned());
        assert_eq!(text.as_deref().ok(), Some(expected), "{source}");
    }
}
