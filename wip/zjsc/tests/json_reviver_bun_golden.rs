//! Golden do reviver de `JSON.parse` contra o JavaScriptCore do bun: `tests/golden/json_reviver_bun.tsv` sai de
//! `scripts/gen-json-reviver-golden.js`, rodado no bun 1.4.2, cada programa num bun filho novo. Cobre a ordem das
//! chamadas (holder, key, this), reviver que apaga, adiciona e troca irmãs ainda não visitadas, devolve `undefined`,
//! muta arrays (`length`, buracos), lança, holder congelado ou com propriedade não configurável (CreateDataProperty
//! falhando em silêncio), `context.source` e a mensagem exata de `SyntaxError` de entradas inválidas.
mod common;

const GOLDEN: &str = include_str!("golden/json_reviver_bun.tsv");
const PRELUDES: &str = include_str!("golden/json_reviver.preludes.json");

#[test]
fn json_reviver_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "json_reviver_case.js", "R"));
}
