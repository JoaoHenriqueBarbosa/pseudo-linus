//! Golden de Error e stack traces de runtime contra o JavaScriptCore do bun: `tests/golden/error_bun.tsv` sai de
//! `scripts/gen-error-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o texto da
//! variável global `R` que ele grava, depois de esvaziadas as microtarefas. Cobre o construtor (`message`,
//! `options.cause` e a ordem de leitura), `AggregateError`, `SuppressedError`, `Error.captureStackTrace`,
//! `Error.stackTraceLimit`, `Error.prepareStackTrace` com `CallSite`, a propriedade `stack`, `toString`,
//! `instanceof`, as mensagens `(evaluating '...')` dos erros nativos e os tipos de erro. O arquivo se chama
//! `error_case.js` dos dois lados, então nome, linha e coluna de cada frame têm de ser idênticos.
mod common;

const GOLDEN: &str = include_str!("golden/error_bun.tsv");

const PRELUDES: &str = include_str!("golden/error.preludes.json");

#[test]
fn error_matches_bun() {
    common::run_mapped_golden(GOLDEN, PRELUDES, 1500, "error_case.js");
}
