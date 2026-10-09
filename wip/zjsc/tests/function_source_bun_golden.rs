//! Golden de texto-fonte de função contra o JavaScriptCore do bun: `tests/golden/function_source_bun.tsv` sai de
//! `scripts/gen-function-source-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da
//! variável global `R` que ele grava. Cobre `Function.prototype.toString` (declaradas, expressões, arrows, métodos,
//! classes, `new Function` e construtores de generator/async, bound, nativas, Proxy, eval, objeto que não é função),
//! mais `name` e `length`. O arquivo se chama `function_source_case.js` dos dois lados.
mod common;

const GOLDEN: &str = include_str!("golden/function_source_bun.tsv");
const PRELUDES: &str = include_str!("golden/function_source.preludes.json");

#[test]
fn function_source_matches_bun() {
    common::run_mapped_golden(GOLDEN, PRELUDES, 900, "function_source_case.js");
}
