//! Golden de `Intl.NumberFormat` com `notation: "compact"` (25 locales, curto e longo, `useGrouping`, moeda e unidade
//! compactas, `formatToParts` e `formatRange`) contra o JavaScriptCore do bun: `tests/golden/number_compact_bun.tsv`
//! sai de `scripts/gen-number-compact-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da
//! variável global `R` que ele grava (JSON).
mod common;

use zjsc::api::eval::evaluate_named_script_result;

const GOLDEN: &str = include_str!("golden/number_compact_bun.tsv");

#[test]
fn number_compact_matches_bun() {
    common::run_golden(GOLDEN, 2000, |source| evaluate_named_script_result(source, "number_compact_case.js", "R"));
}
