//! Golden de aritmética de Temporal contra o JavaScriptCore do bun: `tests/golden/temporal_math_bun.tsv` sai de
//! `scripts/gen-temporal-math-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável
//! global `R` que ele grava (`String(valor)`, `JSON.stringify(valor)` ou `Nome: mensagem` quando lança). Cobre Duration
//! (from/toString, with, add, subtract, compare, round, total, com e sem `relativeTo`), PlainDate, PlainDateTime,
//! PlainYearMonth e PlainMonthDay (add, subtract, until, since, with, overflow), ZonedDateTime com fusos fixos
//! (gap, fold, disambiguation, offset), Instant, Now (só os tipos), toLocaleString com `timeZone` fixo e calendários
//! não ISO, incluindo as mensagens exatas de RangeError e TypeError.
mod common;

use zjsc::api::eval::evaluate_script_sequence_result;

const GOLDEN: &str = include_str!("golden/temporal_math_bun.tsv");

#[test]
fn temporal_math_matches_bun() {
    common::run_golden_big_stack(GOLDEN, 500, |source| evaluate_script_sequence_result(&[source], "temporal_math_case.js", "globalThis.R").1);
}
