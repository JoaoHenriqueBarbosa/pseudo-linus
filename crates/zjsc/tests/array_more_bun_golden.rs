//! Golden de Array (ordenação e busca, terceira camada) contra o JavaScriptCore do bun:
//! `tests/golden/array_more_bun.tsv` sai de `scripts/gen-array-more-golden.js`, rodado no bun 1.4.2. Cada linha é um
//! programa (JSON) e o texto da variável global `R` que ele grava. Cobre sort/toSorted com comparadores
//! inconsistentes, buracos e undefined, grades de argumentos extremos (at, with, fill, copyWithin, slice, toSpliced,
//! indexOf, includes, flat), array-likes com length acima de 2**32, Array.from, Array.of, o construtor com um
//! argumento, species, concat com isConcatSpreadable e join/toString cíclicos.
mod common;


const GOLDEN: &str = include_str!("golden/array_more_bun.tsv");

#[test]
fn array_sort_and_search_more_match_bun() {
    common::run_golden_big_stack(GOLDEN, 1000, |source| common::EvalMode::RunInThisContext.evaluate(source, "", "R"));
}
