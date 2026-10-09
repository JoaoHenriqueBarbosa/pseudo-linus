//! Golden de BigInt em grade contra o JavaScriptCore do bun: `tests/golden/bigint_grid_bun.tsv` sai de
//! `scripts/gen-bigint-grid-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável
//! global `R` que ele grava. Cobre `asIntN`/`asUintN` com bits de fronteira e inválidos, os operadores com BigInt de 1 a
//! 300 bits (divisão por zero, expoente negativo, `>>>`), `BigInt(x)` de strings e de números não inteiros,
//! `toString(radix)` e a comparação com Number e string.
mod common;

const GOLDEN: &str = include_str!("golden/bigint_grid_bun.tsv");
const PRELUDES: &str = include_str!("golden/bigint_grid.preludes.json");

#[test]
fn bigint_grid_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "bigint_grid_case.js", "R"));
}
