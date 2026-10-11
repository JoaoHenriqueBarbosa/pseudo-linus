//! Golden de módulos WebAssembly binários contra o JavaScriptCore do bun: `tests/golden/wasm_module_bun.tsv` sai de
//! `scripts/gen-wasm-module-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) que monta os bytes do módulo
//! com helpers do prelúdio e o texto da variável global `R` que ele grava. Cobre `Module.exports`/`imports`/
//! `customSections`, `CompileError` com a mensagem exata para módulos inválidos, `LinkError` de `Instance` com imports
//! errados, exports de `Global`/`Table`/`Memory`/`Tag` e a API de `Global` (BigInt, v128) e `Table` (get/set/grow).
mod common;

const GOLDEN: &str = include_str!("golden/wasm_module_bun.tsv");
const PRELUDES: &str = include_str!("golden/wasm_module.preludes.json");

#[test]
fn wasm_module_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "wasm_module_case.js", "R"));
}
