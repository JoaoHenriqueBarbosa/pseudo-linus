//! Golden de Temporal.PlainDate, PlainDateTime, PlainYearMonth e PlainMonthDay (calendário iso8601) contra o
//! JavaScriptCore do bun: `tests/golden/temporal_plain_bun.tsv` sai de `scripts/gen-temporal-plain-golden.js`, rodado no
//! bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável global `R` que ele grava (`String(valor)` ou
//! `Nome: mensagem` quando lança). Cobre `from()` com strings ISO de fronteira e inválidas, `with()` com campos fora de
//! faixa e overflow, add/subtract em grade, until/since com largestUnit/smallestUnit/roundingIncrement/roundingMode,
//! compare, equals, toString com opções e Duration round/total/compare com `relativeTo`, com as mensagens exatas de erro.
mod common;

const GOLDEN: &str = include_str!("golden/temporal_plain_bun.tsv");
const PRELUDES: &str = include_str!("golden/temporal_plain.preludes.json");

#[test]
fn temporal_plain_matches_bun() {
    common::run_factored_big_stack(GOLDEN, PRELUDES, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "temporal_plain_case.js", "R"));
}
