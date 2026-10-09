//! Golden de JSON contra o JavaScriptCore do bun: `tests/golden/json_bun.tsv` sai de `scripts/gen-json-golden.js`,
//! rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o texto da variável global `R` que ele grava.
//! Cobre `JSON.parse` (reviver, ordem de visita, `context.source`, erros de sintaxe com mensagem exata, profundidade),
//! `JSON.stringify` (replacer função e array, space, toJSON, ciclos, surrogates soltos, wrappers, esparsos, typed
//! arrays, Proxy), `JSON.rawJSON`/`isRawJSON` e `JSON[Symbol.toStringTag]`. Os números isolados ficam em
//! `json_number_bun_golden.rs`.
mod common;

const GOLDEN: &str = include_str!("golden/json_bun.tsv");

#[test]
fn json_matches_bun() {
    common::run_golden(GOLDEN, 1500, |source| common::EvalMode::IndirectEval.evaluate(source, "json_case.js", "R"));
}
