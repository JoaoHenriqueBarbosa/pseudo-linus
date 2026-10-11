//! Golden da matriz de Intl.NumberFormat contra o JavaScriptCore do bun: `tests/golden/number_matrix_bun.tsv` sai de
//! `scripts/gen-number-matrix-golden.js`, rodado no bun. Cada linha é um programa (JSON) e o texto da variável global `R`
//! que ele grava. Cobre formatToParts, formatRange e formatRangeToParts em 12 locales, com style, notation,
//! compactDisplay, signDisplay, currencyDisplay, currencySign, useGrouping, roundingMode, roundingIncrement,
//! roundingPriority, trailingZeroDisplay e dígitos significativos, sobre valores extremos (0, -0, NaN, Infinity, 1e21,
//! BigInt e strings decimais longas).
mod common;

const GOLDEN: &str = include_str!("golden/number_matrix_bun.tsv");
const PRELUDES: &str = include_str!("golden/number_matrix.preludes.json");

#[test]
fn number_matrix_matches_bun() {
    common::run_factored_big_stack(GOLDEN, PRELUDES, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "number_matrix_case.js", "R"));
}
