//! Golden de Intl.DateTimeFormat contra o JavaScriptCore do bun: `tests/golden/datetime_range_bun.tsv` sai de
//! `scripts/gen-datetime-range-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da
//! variável global `R` que ele grava. Cobre `formatRange` e `formatRangeToParts` em 25 locales (mesmo dia, mês,
//! ano, anos distintos, fusos), `hourCycle`, `calendar`, `numberingSystem`, `era`/`yearName`/`relatedYear`, data
//! inválida (mensagem exata do RangeError), `formatToParts`, `resolvedOptions` e `Date.prototype.toLocale*String`.
mod common;

const GOLDEN: &str = include_str!("golden/datetime_range_bun.tsv");
const PRELUDES: &str = include_str!("golden/datetime_range.preludes.json");

#[test]
fn datetime_range_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 2000, |source| common::EvalMode::IndirectEval.evaluate(source, "datetime_range_case.js", "R"));
}
