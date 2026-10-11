//! Golden de funcref e externref através da API JS do WebAssembly contra o JavaScriptCore do bun:
//! `tests/golden/wasm_funcref_bun.tsv` sai de `scripts/gen-wasm-funcref-golden.js`, rodado no bun 1.4.2. Os módulos são
//! montados por um mini-assembler no gerador e embutidos no prelúdio como `new Uint8Array([...])`. Cada linha é um
//! programa que grava em `R` o texto com identidade (===) de `Table.get`, `Table.set` com função JS, `call_indirect` pela
//! tabela compartilhada entre instâncias, `Table.grow` com valor inicial, `Global` funcref e externref, import e reexport de
//! funções de outra instância, `ref.func` de import, tabelas de externref e as mensagens exatas dos erros.
mod common;

const GOLDEN: &str = include_str!("golden/wasm_funcref_bun.tsv");
const PRELUDES: &str = include_str!("golden/wasm_funcref.preludes.json");

#[test]
fn wasm_funcref_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 2000, |source| common::EvalMode::IndirectEval.evaluate(source, "wasm_funcref_case.js", "R"));
}
