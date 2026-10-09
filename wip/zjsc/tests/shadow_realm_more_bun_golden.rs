//! Golden complementar de ShadowRealm contra o JavaScriptCore do bun: `tests/golden/shadow_realm_more_bun.tsv` sai de
//! `scripts/gen-shadow-realm-more-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da
//! variável global `R` que ele grava, depois de esvaziadas as microtarefas. Cobre a matriz de retorno do `evaluate`,
//! erros entre realms (tipo e mensagem), wrappers de função (name, length, protótipo, this, new), isolamento de
//! globais, subclasses, `evaluate` com não-string, evaluate recursivo e a forma do `importValue`.
mod common;


const GOLDEN: &str = include_str!("golden/shadow_realm_more_bun.tsv");

#[test]
fn shadow_realm_more_matches_bun() {
    // O programa pode falhar na compilação: o bun então deixa `R` indefinido e o golden registra isso.
    common::run_golden_big_stack(GOLDEN, 900, |source| common::EvalMode::RunInThisContext.evaluate(source, "", "R"));
}
