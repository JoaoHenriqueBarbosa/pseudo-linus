//! Golden de fonte exótica no realm remoto do ShadowRealm contra o JavaScriptCore do bun:
//! `tests/golden/shadow_realm_source_bun.tsv` sai de `scripts/gen-shadow-realm-source-golden.js`, rodado no bun 1.4.2.
//! Cada linha é um programa (JSON) e o texto da variável global `R` que ele grava. Sem `vm` no porte, o ShadowRealm é
//! a forma de analisar e executar fonte num segundo realm: comentários HTML-like, octais legados, `let`/`yield`/`await`
//! como identificador, ASI, regex contra divisão, labels, os quatro construtores de função com parâmetros e corpos
//! exóticos, intrínsecos de cada global vistos pelo wrapper, funções nativas chamadas pelo wrapper e declarações
//! globais persistentes entre `evaluate`.
mod common;


const GOLDEN: &str = include_str!("golden/shadow_realm_source_bun.tsv");
const PRELUDES: &str = include_str!("golden/shadow_realm_source.preludes.json");

#[test]
fn shadow_realm_source_matches_bun() {
    // O programa pode falhar na compilação: o bun então deixa `R` indefinido e o golden registra isso.
    common::run_factored_big_stack(GOLDEN, PRELUDES, 8000, |source| common::EvalMode::RunInThisContext.evaluate(source, "", "R"));
}
