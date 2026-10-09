//! Golden de Function e Error contra o JavaScriptCore do bun: `tests/golden/function_error_bun.tsv` sai de
//! `scripts/gen-function-error-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o
//! texto da variável global `R` que ele grava, depois de esvaziadas as microtarefas. Cobre
//! `Function.prototype.toString`, `name`/`length`, `Error` (cause, AggregateError, captureStackTrace,
//! stackTraceLimit, prepareStackTrace, `Error.prototype.toString`) e o formato de `err.stack`. O arquivo se chama
//! `function_error_case.js` dos dois lados, então nome, linha e coluna de cada frame têm de ser idênticos.
mod common;

const GOLDEN: &str = include_str!("golden/function_error_bun.tsv");

const PRELUDES: &str = include_str!("golden/function_error.preludes.json");

#[test]
fn function_and_error_match_bun() {
    common::run_mapped_golden(GOLDEN, PRELUDES, 400, "function_error_case.js");
}
