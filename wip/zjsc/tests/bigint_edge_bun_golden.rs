//! Golden de BigInt de borda contra o JavaScriptCore do bun: `tests/golden/bigint_edge_bun.tsv` sai de
//! `scripts/gen-bigint-edge-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável
//! global `R` que ele grava. Cobre literais e `BigInt()` de strings, asIntN/asUintN, toString em radix 2..36 de
//! números gigantes, operadores com negativos em complemento de dois, comparação com Number e string,
//! BigInt64Array/BigUint64Array/DataView, JSON, Math, mistura de tipos, toLocaleString, parseInt/Number e mensagens.
mod common;


const GOLDEN: &str = include_str!("golden/bigint_edge_bun.tsv");

#[test]
fn bigint_edge_matches_bun() {
    common::run_golden_big_stack(GOLDEN, 1000, |source| common::EvalMode::RunInThisContext.evaluate(source, "", "R"));
}
