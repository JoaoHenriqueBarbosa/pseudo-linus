//! Golden de arredondamento e diferença de Temporal contra o JavaScriptCore do bun: `tests/golden/temporal_round_bun.tsv`
//! sai de `scripts/gen-temporal-round-golden.js`, rodado no bun 1.4.2 (um processo novo por programa). Cada linha é um
//! programa (JSON) e o texto da variável global `R` que ele grava (`String(valor)` ou `Nome: mensagem` quando lança).
//! Cobre round de PlainTime, PlainDateTime, Instant, ZonedDateTime e Duration (com `relativeTo`), since/until com
//! largestUnit, smallestUnit, roundingMode e roundingIncrement, with/add/subtract com overflow, opções de toString,
//! compare, from com strings ISO extensas, calendários não ISO, deslocamentos de fuso e as mensagens exatas de
//! RangeError e TypeError.
mod common;

const GOLDEN: &str = include_str!("golden/temporal_round_bun.tsv");

#[test]
fn temporal_round_matches_bun() {
    common::run_golden_big_stack(GOLDEN, 1000, |source| common::EvalMode::IndirectEval.evaluate(source, "temporal_round_case.js", "R"));
}
