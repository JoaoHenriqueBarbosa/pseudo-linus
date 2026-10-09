//! Golden de Map/Set/WeakMap/WeakSet/WeakRef/FinalizationRegistry e do registro de símbolos contra o JavaScriptCore do
//! bun: `tests/golden/weak_more_bun.tsv` sai de `scripts/gen-weak-more-golden.js`, rodado no bun 1.4.2. Cada linha é
//! um programa (JSON) e o texto da variável global `R` que ele grava. Cobre mutação durante forEach/for-of/iterador,
//! chaves -0/NaN/objeto/símbolo, chaves fracas (símbolo não registrado permitido, registrado TypeError), WeakRef,
//! FinalizationRegistry com token, Symbol.for/keyFor/description/well-known, ordem de getOwnPropertySymbols,
//! subclasses, species, this inválido e o descritor de size.
mod common;


const GOLDEN: &str = include_str!("golden/weak_more_bun.tsv");

#[test]
fn weak_collections_and_symbol_registry_match_bun() {
    common::run_golden_big_stack(GOLDEN, 800, |source| common::EvalMode::RunInThisContext.evaluate(source, "", "R"));
}
