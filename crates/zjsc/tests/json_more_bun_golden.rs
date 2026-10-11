//! Complemento de borda do golden de JSON contra o JavaScriptCore do bun: `tests/golden/json_more_bun.tsv` sai de
//! `scripts/gen-json-more-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o texto da
//! variável global `R` que ele grava. Cobre a matriz de mensagens de erro de posição do `JSON.parse` (token ruim em
//! cada ponto da gramática, números, escapes, caracteres de controle, literais, espaços, BOM, profundidade), reviver
//! (buracos, deleção, holder, `context.source`), `JSON.stringify` (replacer array e função, space, toJSON, ciclos,
//! Proxy, wrappers, Map/Set, typed arrays, getters, BigInt, surrogates soltos) e `JSON.rawJSON`/`isRawJSON`.
mod common;

use zjsc::runtime::process_time_zone::set_time_zone_spec_override;

const GOLDEN: &str = include_str!("golden/json_more_bun.tsv");

#[test]
fn json_more_matches_bun() {
    // O golden saiu de um bun em America/Sao_Paulo; fixar o fuso torna o resultado independente da máquina do teste.
    set_time_zone_spec_override(Some("America/Sao_Paulo"));
    common::run_golden(GOLDEN, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "json_more_case.js", "R"));
}
