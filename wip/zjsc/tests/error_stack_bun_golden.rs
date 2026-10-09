//! Golden de `Error.stack` contra o JavaScriptCore do bun: `tests/golden/error_stack_bun.tsv` sai de
//! `scripts/gen-error-stack-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o
//! texto da variável global `R` que ele grava, depois de esvaziadas as microtarefas. Cobre o formato das linhas de
//! frame (função anônima, método, construtor, async, eval, `new Function`), erro dentro de callback nativo
//! (inclusive armadilhas de `Proxy`), `captureStackTrace`, `stackTraceLimit`, `prepareStackTrace` com `CallSite`,
//! `cause`, `AggregateError`, erro de sintaxe em `eval` e geradores. O arquivo se chama `error_stack_case.js` dos
//! dois lados, então nome, linha e coluna de cada frame têm de ser idênticos.
mod common;

const GOLDEN: &str = include_str!("golden/error_stack_bun.tsv");

const PRELUDES: &str = include_str!("golden/error_stack.preludes.json");

#[test]
fn error_stack_matches_bun() {
    common::run_mapped_golden(GOLDEN, PRELUDES, 400, "error_stack_case.js");
}
