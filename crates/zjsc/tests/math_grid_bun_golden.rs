//! Golden da grade de `Math.*` contra o JavaScriptCore do bun: `tests/golden/math_grid_bun.tsv` sai de
//! `scripts/gen-math-grid-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o texto da
//! variável global `R` que ele grava. Cobre as funções exatas (round, trunc, sign, abs, floor, ceil, max, min, clz32,
//! imul, fround, f16round, sqrt, cbrt, hypot, pow, atan2, sumPrecise) em grade de valores de fronteira, os casos
//! especiais de `pow`, `hypot` com muitos argumentos, poucas transcendentais e a ordem das chamadas de `valueOf`.
mod common;

const GOLDEN: &str = include_str!("golden/math_grid_bun.tsv");
const PRELUDES: &str = include_str!("golden/math_grid.preludes.json");

#[test]
fn math_grid_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "math_grid_case.js", "R"));
}
