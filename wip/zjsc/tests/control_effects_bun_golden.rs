//! Golden de efeitos das instruções de controle contra o JavaScriptCore do bun: `tests/golden/control_effects_bun.tsv`
//! sai de `scripts/gen-control-effects-golden.js`, rodado no bun. Cada linha é um programa (JSON) e o texto da variável
//! global `R` que ele grava, depois de esvaziadas as microtarefas. Cobre a ordem dos efeitos de switch (fallthrough,
//! default em qualquer posição, TDZ), break/continue com label dentro de switch/try/finally/with, for com closures,
//! ordem de chaves e mutação em for-in e for-of, destructuring no cabeçalho em generator e async, do-while, curto-circuito,
//! Annex B, exceções no cabeçalho do for e nos ganchos do iterador, try/catch/finally sobrescrevendo e finally com yield.
mod common;

const GOLDEN: &str = include_str!("golden/control_effects_bun.tsv");
const PRELUDES: &str = include_str!("golden/control_effects.preludes.json");

#[test]
fn control_effects_match_bun() {
    // O programa pode falhar na compilação: o bun então deixa `R` indefinido e o golden registra isso, por isso a
    // exceção do script não encerra a medição.
    common::run_factored_big_stack(GOLDEN, PRELUDES, 2500, |source| common::EvalMode::IndirectEvalDrained.evaluate(source, "control_effects_case.js", "R"));
}
