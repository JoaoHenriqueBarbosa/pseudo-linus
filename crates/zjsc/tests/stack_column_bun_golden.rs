//! Golden da coluna de cada frame de `Error.prototype.stack` contra o JavaScriptCore do bun:
//! `tests/golden/stack_column_bun.tsv` sai de `scripts/gen-stack-column-golden.js`, rodado no bun 1.4.2.
//! O bun transpila o fonte e remapeia a posição do JSC pelo source map do transpilador, então a coluna exibida é o
//! início do último token mapeável com início <= divot (o `(` da chamada) na mesma linha; a regra está em
//! `wip/notes/stack-column-rule.md` e o porte em `src/runtime/stack_frame.rs::callee_back_offset`.
//! Os programas cobrem chamadas por identificador, membro, índice, `?.`, `new`, template com tag, `super`, e
//! espaços, quebras de linha e comentários entre o callee e o `(`. Cada programa normaliza a própria pilha (só
//! `linha:coluna` dos frames do arquivo `error_stack_case.js`), então o resultado não leva caminho da máquina.
mod common;

use zjsc::api::eval::evaluate_script_sequence_result;

const GOLDEN: &str = include_str!("golden/stack_column_bun.tsv");

#[test]
fn stack_column_matches_bun() {
    common::run_golden(GOLDEN, 300, |source| evaluate_script_sequence_result(&[source], "error_stack_case.js", "globalThis.R").1);
}
