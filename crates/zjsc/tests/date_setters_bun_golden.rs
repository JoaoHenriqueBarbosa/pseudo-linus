//! Golden dos setters de `Date` contra o JavaScriptCore do bun: `tests/golden/date_setters_bun.tsv` sai de
//! `scripts/gen-date-setters-golden.js`, rodado no bun 1.4.2 com `TZ=America/Sao_Paulo`. Cada linha é um programa e o
//! texto da variável global `R` que ele grava. Cobre `setFullYear`, `setMonth`, `setDate`, `setHours`, `setMinutes`,
//! `setSeconds`, `setMilliseconds`, as versões UTC, `setYear` e `setTime` com argumentos extras, `NaN`, `Infinity`,
//! coerção com `valueOf` que lança e a ordem das coerções, em datas válidas perto de transições de horário de verão
//! históricas, em datas inválidas e nos limites de +-8.64e15.
mod common;

use zjsc::runtime::process_time_zone::set_time_zone_spec_override;

const GOLDEN: &str = include_str!("golden/date_setters_bun.tsv");
const PRELUDES: &str = include_str!("golden/date_setters.preludes.json");

#[test]
fn date_setters_match_bun() {
    // O golden saiu de um bun em America/Sao_Paulo; fixar o fuso torna o resultado independente da máquina do teste.
    set_time_zone_spec_override(Some("America/Sao_Paulo"));
    common::run_factored(GOLDEN, PRELUDES, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "date_setters_case.js", "R"));
    set_time_zone_spec_override(None);
}
