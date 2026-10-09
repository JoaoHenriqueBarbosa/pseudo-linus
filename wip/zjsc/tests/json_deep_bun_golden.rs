//! Golden profundo de JSON contra o JavaScriptCore do bun: `tests/golden/json_deep_bun.tsv` sai de
//! `scripts/gen-json-deep-golden.js`, rodado no bun 1.4.2, cada programa num bun filho novo. Cada linha é um programa
//! (JSON, várias linhas) e o texto da variável global `R` que ele grava. Cobre `JSON.parse` com reviver (ordem de
//! chamada, holder, `delete` e mutação, `context.source`), números extremos, escapes inválidos e surrogates soltos,
//! BOM e espaços, `__proto__` como chave, ordem de chaves numéricas, profundidade extrema, mensagens de `SyntaxError`,
//! `JSON.stringify` com gap e replacer exóticos, `toJSON` em protótipos e primitivos, BigInt, Symbol, ciclos, Proxy,
//! array-like, typed arrays, `Date` inválida, `-0`, stringify bem formado e `JSON.rawJSON`/`JSON.isRawJSON`.
mod common;

const GOLDEN: &str = include_str!("golden/json_deep_bun.tsv");
const PRELUDES: &str = include_str!("golden/json_deep.preludes.json");

#[test]
fn json_deep_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "json_deep_case.js", "R"));
}
