//! Golden de `Array.prototype` em receptores array-like exóticos contra o JavaScriptCore do bun:
//! `tests/golden/array_generic_bun.tsv` sai de `scripts/gen-array-generic-golden.js`, rodado no bun 1.4.2. Cada linha é um
//! programa (JSON, várias linhas) e o texto da variável global `R` que ele grava: o resultado, o estado final do receptor e
//! o log exato de get/set/has/delete/define dos traps. Cobre objetos com getter de `length`, `length` negativo, fracionário
//! e 2**53, Proxy que loga, String boxed, `arguments`, typed array via `call`, mutadores, callbacks e comparadores que
//! mutam o receptor, `flat`/`flatMap`, `Symbol.isConcatSpreadable`, `toSpliced`/`with`/`toSorted` e mensagens de erro.
mod common;

const GOLDEN: &str = include_str!("golden/array_generic_bun.tsv");
const PRELUDES: &str = include_str!("golden/array_generic.preludes.json");

#[test]
fn array_generic_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "array_generic_case.js", "R"));
}
