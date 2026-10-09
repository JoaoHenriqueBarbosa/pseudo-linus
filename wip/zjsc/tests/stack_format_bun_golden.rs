//! Golden do formato de `Error.prototype.stack` contra o JavaScriptCore do bun: `tests/golden/stack_format_bun.tsv`
//! sai de `scripts/gen-stack-format-golden.js`, rodado no bun 1.4.2. Complementa `error_stack_bun_golden.rs` com
//! métodos de classe, estáticos, accessors, async, geradores, eval e `new Function` aninhados, frames nativos,
//! `stackTraceLimit` com valores estranhos, `prepareStackTrace` com `CallSite` por contexto, `cause`,
//! `AggregateError`, `captureStackTrace` com `constructorOpt`, async stack traces e nomes inferidos.
//! O arquivo se chama `error_stack_case.js` dos dois lados, então nome, linha e coluna têm de ser idênticos.
//!
//! O runner do gerador engole a exceção do programa (`try { go(depth) } catch (e) { }`) e lê `globalThis.R`, que
//! fica `undefined` quando o programa lança antes de gravá-lo (`with` em modo estrito, `new Promise(cb)` com `cb`
//! que lança, aspas aninhadas). O mesmo vale aqui: `evaluate_script_sequence_result` engole a exceção e lê
//! `globalThis.R`, sem o `ReferenceError` que a leitura de `R` solto daria.
mod common;

use zjsc::api::eval::evaluate_script_sequence_result;

const GOLDEN: &str = include_str!("golden/stack_format_bun.tsv");

#[test]
fn stack_format_matches_bun() {
    common::run_golden(GOLDEN, 400, |source| evaluate_script_sequence_result(&[source], "error_stack_case.js", "globalThis.R").1);
}
