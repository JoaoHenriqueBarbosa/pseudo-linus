//! Golden de Intl.NumberFormat#formatRange e #formatRangeToParts contra o JavaScriptCore do bun: `tests/golden/number_range_bun.tsv`
//! sai de `scripts/gen-number-range-golden.js`, rodado no bun. Cada linha é um programa (JSON) e o texto da variável global `R`
//! que ele grava. Cobre 15 locales, styles decimal, currency, percent e unit, notations standard, compact, scientific e
//! engineering, pares de números iguais, próximos, invertidos, NaN, Infinity, BigInt e strings decimais longas, formatToParts com
//! signDisplay, useGrouping, roundingMode, roundingIncrement e trailingZeroDisplay, e os RangeError exatos.
mod common;

const GOLDEN: &str = include_str!("golden/number_range_bun.tsv");
const PRELUDES: &str = include_str!("golden/number_range.preludes.json");

#[test]
fn number_range_matches_bun() {
    common::run_factored_big_stack(GOLDEN, PRELUDES, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "number_range_case.js", "R"));
}
