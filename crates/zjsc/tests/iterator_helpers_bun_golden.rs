//! Golden de Iterator helpers síncronos contra o JavaScriptCore do bun: `tests/golden/iterator_helpers_bun.tsv` sai de
//! `scripts/gen-iterator-helpers-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o
//! texto da variável global `R` que ele grava. Cobre `Iterator.prototype.map/filter/flatMap/take/drop/reduce/toArray/
//! forEach/some/every/find`, `Iterator.from`, `Iterator.concat`, `%WrapForValidIteratorPrototype%` e
//! `%IteratorHelperPrototype%`: receptores e argumentos inválidos, `return()` propagado, `next` lido uma vez, helpers
//! encadeados, reentrância, exaustão, contadores de `take`/`drop` e descritores. O bun 1.4.2 não tem `AsyncIterator`.
mod common;

const GOLDEN: &str = include_str!("golden/iterator_helpers_bun.tsv");

#[test]
fn iterator_helpers_matches_bun() {
    common::run_golden(GOLDEN, 1500, |source| common::EvalMode::IndirectEval.evaluate(source, "iterator_helpers_case.js", "R"));
}
