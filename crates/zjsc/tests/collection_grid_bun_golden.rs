//! Golden de grade de chaves e receptores de `Map`/`Set`/`WeakMap`/`WeakSet` contra o JavaScriptCore do bun:
//! `tests/golden/collection_grid_bun.tsv` sai de `scripts/gen-collection-grid-golden.js`, rodado no bun 1.4.2. Cada
//! linha é um programa (JSON, várias linhas) e o texto da variável global `R` que ele grava. Cobre SameValueZero
//! (-0/+0, NaN, BigInt, strings, objetos, símbolos, boxed), ordem de inserção após `delete` e reinserção, mutação
//! durante `forEach` e iteradores, `clear` durante iteração, construtores com iteráveis (entries inválidas, adder
//! sobrescrito, iterador fechado em erro), receptores inválidos, chaves inválidas de `WeakMap`/`WeakSet`, getter
//! `size`, descritores e subclasses.
mod common;

const GOLDEN: &str = include_str!("golden/collection_grid_bun.tsv");
const PRELUDES: &str = include_str!("golden/collection_grid.preludes.json");

#[test]
fn collection_grid_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "collection_grid_case.js", "R"));
}
