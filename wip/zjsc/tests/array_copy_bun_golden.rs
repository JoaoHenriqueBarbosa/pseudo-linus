//! Golden de métodos de cópia e agrupamento de Array contra o JavaScriptCore do bun:
//! `tests/golden/array_copy_bun.tsv` sai de `scripts/gen-array-copy-golden.js`, rodado no bun. Cada linha é um programa
//! (sufixo JSON, prelúdio fatorado) e o texto da variável global `R` que ele grava: o resultado, o estado final do
//! receptor e o log exato dos traps. Cobre `toSorted`, `toReversed`, `toSpliced`, `with`, `findLast`, `findLastIndex`, `at`,
//! `Object.groupBy`, `Map.groupBy`, a estabilidade de `sort`, comparadores inconsistentes, que lançam e que mutam o array,
//! buracos, array-likes com `length` estranho, Proxy que registra os traps, typed arrays (RangeError e TypeError exatos)
//! e receptores null/undefined.
mod common;

const GOLDEN: &str = include_str!("golden/array_copy_bun.tsv");
const PRELUDES: &str = include_str!("golden/array_copy.preludes.json");

#[test]
fn array_copy_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "array_copy_case.js", "R"));
}
