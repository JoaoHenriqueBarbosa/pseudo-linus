//! Golden do objeto global e das funções globais de borda contra o JavaScriptCore do bun:
//! `tests/golden/global_edge_bun.tsv` sai de `scripts/gen-global-edge-golden.js`, rodado no bun 1.4.2. Cada linha é um
//! programa (JSON) e o texto da variável global `R` que ele grava. Cobre parseInt/parseFloat/isNaN/isFinite com entradas
//! exóticas, encodeURI/decodeURI/escape/unescape, descritores de `globalThis`, delete de globais, `this` em funções sloppy
//! e strict, call/apply/bind, Symbol.hasInstance, caller/arguments, `arguments.callee`, descritores de Function e
//! `toString` de funções nativas.
mod common;


const GOLDEN: &str = include_str!("golden/global_edge_bun.tsv");

#[test]
fn global_edge_matches_bun() {
    // O programa pode falhar na compilação (early error): o bun então deixa `R` indefinido e o golden registra isso,
    // por isso a exceção do script não encerra a medição.
    common::run_golden(GOLDEN, 500, |source| common::EvalMode::RunInThisContext.evaluate(source, "", "R"));
}
