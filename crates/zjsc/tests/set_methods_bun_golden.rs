//! Golden dos métodos de conjunto de `Set` (`union`, `intersection`, `difference`, `symmetricDifference`, `isSubsetOf`,
//! `isSupersetOf`, `isDisjointFrom`) e de `Map.groupBy`/`Object.groupBy` contra o JavaScriptCore do bun:
//! `tests/golden/set_methods_bun.tsv` sai de `scripts/gen-set-methods-golden.js`, rodado no bun 1.4.2. Cada linha é um
//! sufixo de programa (JSON) e o texto da variável global `R` que ele grava; o prelúdio fatorado fica em
//! `tests/golden/set_methods.preludes.json`. Cobre receptores de tamanho 0..4 contra argumentos `Set`, `Map` e set-like com
//! log de chamadas, `size` exótico, `has` e `keys` inválidos, ordem de leitura das propriedades, subclasses, mutação do
//! receptor durante a operação, ordem dos elementos e mensagens exatas de `TypeError`/`RangeError`.
mod common;

const GOLDEN: &str = include_str!("golden/set_methods_bun.tsv");
const PRELUDES: &str = include_str!("golden/set_methods.preludes.json");

#[test]
fn set_methods_match_bun() {
    common::run_factored(GOLDEN, PRELUDES, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "set_methods_case.js", "R"));
}
