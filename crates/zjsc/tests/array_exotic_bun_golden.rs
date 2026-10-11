//! Golden de semântica exótica de Array contra o JavaScriptCore do bun: `tests/golden/array_exotic_bun.tsv` sai de
//! `scripts/gen-array-exotic-golden.js`, rodado no bun. Cada linha é um programa (JSON, sufixo do prelúdio) e o texto da
//! variável global `R` que ele grava. Cobre a atribuição a `length` (encolher com elementos não configuráveis, `length`
//! não gravável, `ToNumber`/`ToUint32` com `valueOf` logado e o `RangeError` "Invalid array length"), `defineProperty`
//! de `length` com descritores variados, índices como chaves (`2**32-2`, `2**32-1`, `"-0"`, `"01"`, `"1.0"`), arrays
//! esparsos e buracos em quase todos os métodos, `sort` com buracos e `undefined`, `join`/`toString`, `Array(n)` com `n`
//! inválido, estouro de comprimento, `Array.prototype` como array e arrays congelados, selados ou com `length`/índice
//! não gravável em métodos que escrevem. O prelúdio 0 é o modo sloppy e o 1 o modo estrito.
mod common;

const GOLDEN: &str = include_str!("golden/array_exotic_bun.tsv");
const PRELUDES: &str = include_str!("golden/array_exotic.preludes.json");

#[test]
fn array_exotic_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "array_exotic_case.js", "R"));
}
