//! Golden de geradores síncronos contra o JavaScriptCore do bun: `tests/golden/generator_bun.tsv` sai de
//! `scripts/gen-generator-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o texto da
//! variável global `R` que ele grava. Cobre a matriz corpo x sequência de `next`/`return`/`throw` (try/catch/finally,
//! `yield` no finally, return e throw no finally), `yield*` contra iteradores manuais, reentrância, protótipos e
//! descritores, construção, consumidores (spread, destructuring, for-of, helpers de Iterator) e erros de sintaxe de `yield`.
mod common;

const GOLDEN: &str = include_str!("golden/generator_bun.tsv");

#[test]
fn generator_matches_bun() {
    common::run_golden(GOLDEN, 600, |source| common::EvalMode::IndirectEval.evaluate(source, "generator_case.js", "R"));
}
