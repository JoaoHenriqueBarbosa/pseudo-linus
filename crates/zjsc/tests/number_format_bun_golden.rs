//! Golden de formatação e leitura de Number contra o JavaScriptCore do bun: `tests/golden/number_format_bun.tsv` sai
//! de `scripts/gen-number-format-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da
//! variável global `R` que ele grava. Cobre `Number.prototype.toString(radix)`, `toFixed`, `toExponential`,
//! `toPrecision`, `Number()`/`parseFloat`/`parseInt`, as fronteiras do shortest repr, os predicados e constantes de
//! Number, `BigInt(number)`, `asIntN`/`asUintN`, o trecho de Math fora do libm e `toLocaleString('en-US')`.
mod common;

use zjsc::api::eval::evaluate_named_script_result;

const GOLDEN: &str = include_str!("golden/number_format_bun.tsv");

#[test]
fn number_format_matches_bun() {
    common::run_golden(GOLDEN, 2500, |source| evaluate_named_script_result(source, "number_format_case.js", "R"));
}
