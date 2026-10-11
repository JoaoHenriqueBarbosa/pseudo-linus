//! Golden de `call_ref` e `return_call_ref` (typed function references) e da ausência de type reflection
//! (`Memory/Table/Global/Tag.prototype.type`) contra o JavaScriptCore do bun 1.4.2: `tests/golden/wasm_callref_bun.tsv`
//! sai de `scripts/gen-wasm-callref-golden.js`. Cada programa grava em `R` um texto.
mod common;

const GOLDEN: &str = include_str!("golden/wasm_callref_bun.tsv");

#[test]
fn wasm_callref_matches_bun() {
    common::run_golden(GOLDEN, 13, |source| common::EvalMode::IndirectEval.evaluate(source, "wasm_callref_case.js", "R"));
}
