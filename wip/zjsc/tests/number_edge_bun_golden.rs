//! Golden de bordas de Number, Math e operadores numéricos contra o JavaScriptCore do bun:
//! `tests/golden/number_edge_bun.tsv` sai de `scripts/gen-number-edge-golden.js`, rodado no bun 1.4.2. Cada linha é um
//! programa (JSON) e o texto da variável global `R` que ele grava. Cobre `toFixed`/`toPrecision`/`toExponential`/
//! `toString(radix)` nas bordas e nos erros de intervalo, `parseFloat`/`parseInt`/`Number()` com texto difícil, todas as
//! funções de `Math` com valores especiais e bits exatos (via `Float64Array`), e os operadores `**`, `%`, shifts, `>>>`,
//! bit a bit, `++`/`--` e a aritmética mista de BigInt com Number.
mod common;

const GOLDEN: &str = include_str!("golden/number_edge_bun.tsv");

#[test]
fn number_edge_matches_bun() {
    common::run_golden(GOLDEN, 1000, |source| common::EvalMode::IndirectEval.evaluate(source, "number_edge_case.js", "R"));
}
