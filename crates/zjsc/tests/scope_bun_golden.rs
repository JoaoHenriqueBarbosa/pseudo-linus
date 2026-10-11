//! Golden de escopo e closures contra o JavaScriptCore do bun: `tests/golden/scope_bun.tsv` sai de
//! `scripts/gen-scope-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável global
//! `R` que ele grava, depois de esvaziadas as microtarefas. Cobre TDZ, bindings por iteração, parâmetros default,
//! named function expression, class binding, catch e Annex B, hoisting em blocos, redeclarações (SyntaxError),
//! eval, delete, with, generators, async, this/arguments/new.target lexicais, private names, closures e listas de
//! parâmetros grandes e identificadores unicode.
mod common;


const GOLDEN: &str = include_str!("golden/scope_bun.tsv");

#[test]
fn scope_and_closures_match_bun() {
    // O programa pode falhar na compilação (early error, como `for (let i;;) { var i }`): o bun então deixa `R`
    // indefinido e o golden registra isso, por isso a exceção do script não encerra a medição.
    common::run_golden_big_stack(GOLDEN, 800, |source| common::EvalMode::RunInThisContext.evaluate(source, "", "R"));
}
