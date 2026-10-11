//! Golden dos protocolos de iteração dos built-ins contra o JavaScriptCore do bun: `tests/golden/builtin_iteration_bun.tsv`
//! sai de `scripts/gen-builtin-iteration-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o
//! texto da variável global `R` que ele grava. Cobre os iteradores de Array, String, Map, Set, TypedArray, arguments e
//! RegExp String Iterator (cadeia de protótipos, `@@toStringTag`, `next` com this inválido, exaustão, mutação durante a
//! iteração), os consumidores do protocolo (spread, `Array.from`, desestruturação, for-of, `yield*`, `Promise.all`,
//! construtores de coleções, `Object.fromEntries`) sobre iteráveis que registram `next`/`return`/`throw`,
//! `Symbol.iterator` trocado em protótipos e instâncias, surrogates, esparsos, TypedArray destacado ou redimensionado,
//! `%IteratorPrototype%` e `%AsyncIteratorPrototype%`.
mod common;

const GOLDEN: &str = include_str!("golden/builtin_iteration_bun.tsv");
const PRELUDES: &str = include_str!("golden/builtin_iteration.preludes.json");

#[test]
fn builtin_iteration_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 2000, |source| common::EvalMode::IndirectEval.evaluate(source, "builtin_iteration_case.js", "R"));
}
