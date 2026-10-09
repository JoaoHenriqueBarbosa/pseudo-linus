//! Golden de `Date` contra o JavaScriptCore do bun: `tests/golden/date_parse_bun.tsv` sai de
//! `scripts/gen-date-parse-golden.js`, rodado no bun 1.4.2 com `TZ=UTC`. Cada linha é um programa (JSON) e o texto da
//! variável global `R` que ele grava. Cobre `Date.parse` e `new Date(string)` numa grade de formatos (ISO com e sem `Z`,
//! offsets, anos expandidos, formato legado, RFC 2822, parênteses, meses abreviados, AM/PM, inválidos), `Date.UTC` e
//! setters com `NaN` e overflow, os formatadores nos limites (±8,64e15, ano 0, negativo, acima de 9999),
//! `Symbol.toPrimitive` com hints inválidos, `getTimezoneOffset`, `valueOf` e aritmética com `Date`.
mod common;

use zjsc::runtime::process_time_zone::set_time_zone_spec_override;

const GOLDEN: &str = include_str!("golden/date_parse_bun.tsv");
const PRELUDES: &str = include_str!("golden/date_parse.preludes.json");

#[test]
fn date_parse_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 3000, |source| {
        set_time_zone_spec_override(Some("UTC"));
        common::EvalMode::IndirectEval.evaluate(source, "date_parse_case.js", "R")
    });
}
