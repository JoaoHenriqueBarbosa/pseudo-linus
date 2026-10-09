//! Golden de grade de JSON contra o JavaScriptCore do bun: `tests/golden/json_grid_bun.tsv` sai de
//! `scripts/gen-json-grid-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o texto da
//! variável global `R` que ele grava. Cobre `JSON.parse` inválido com a mensagem exata de `SyntaxError` (prefixo
//! estrutural e cauda ruim, escapes, números, comentários, BOM, aspas simples), parse válido de borda (chaves duplicadas,
//! `__proto__`, ordem de chaves inteiras, profundidade), reviver com `context.source`, `JSON.stringify` com
//! `toJSON`/replacer/space sobre Proxy, TypedArray, Map, Set, Date inválida, wrappers, símbolos e getters que lançam,
//! ciclos indiretos, BigInt, surrogates isolados e `JSON.rawJSON`/`isRawJSON`. Complementa `json_bun_golden.rs`,
//! `json_more_bun` e `json_number_bun_golden.rs` sem repetir fonte deles.
mod common;

const GOLDEN: &str = include_str!("golden/json_grid_bun.tsv");
const PRELUDES: &str = include_str!("golden/json_grid.preludes.json");

#[test]
fn json_grid_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 1500, |source| common::EvalMode::IndirectEval.evaluate(source, "json_grid_case.js", "R"));
}
