//! Golden de borda da família Error contra o JavaScriptCore do bun: `tests/golden/error_edge_bun.tsv` sai de
//! `scripts/gen-error-edge-golden.js`, rodado no bun 1.4.2 (um bun filho novo por programa). Cada linha é um
//! programa (JSON) e o texto da variável global `R` que ele grava. Cobre o que os goldens de erro, pilha e dispose
//! ainda não cobriam: AggregateError (errors iterável, ordem de acesso, cause), `cause` em todos os construtores
//! nativos, SuppressedError e as pilhas descartáveis, `Error.captureStackTrace` com constructorOpt,
//! `Error.stackTraceLimit`, `Error.prepareStackTrace` com cada método de CallSite, e as mensagens de
//! TypeError/RangeError/SyntaxError de APIs comuns com o sufixo `(evaluating ...)`. O nome do arquivo do script é
//! `x.js`; os programas normalizam linha e coluna sozinhos quando imprimem texto de pilha.
mod common;

use zjsc::api::eval::evaluate_script_sequence_result;

const GOLDEN: &str = include_str!("golden/error_edge_bun.tsv");
const PRELUDES: &str = include_str!("golden/error_edge.preludes.json");

#[test]
fn error_edge_matches_bun() {
    // Um programa que não grava `R` (ou falha na compilação) deixa `R` indefinido, e o golden registra isso.
    common::run_factored_big_stack(GOLDEN, PRELUDES, 1000, |source| evaluate_script_sequence_result(&[source], "x.js", "globalThis.R").1);
}
