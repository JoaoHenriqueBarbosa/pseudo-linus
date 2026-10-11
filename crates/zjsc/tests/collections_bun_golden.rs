//! Golden de coleções contra o JavaScriptCore do bun: `tests/golden/collections_bun.tsv` sai de
//! `scripts/gen-collections-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável
//! global `R` que ele grava, depois de esvaziadas as microtarefas. Cobre Array (esparsos, array-likes, sort estável
//! com comparadores inconsistentes), Object (descritores, freeze/seal, groupBy), Map/Set/Weak*/FinalizationRegistry,
//! métodos novos de Set, Array.fromAsync e iterator helpers (Iterator.from/concat). O arquivo se chama
//! `collections_case.js` dos dois lados.
mod common;

const GOLDEN: &str = include_str!("golden/collections_bun.tsv");

const PRELUDES: &str = include_str!("golden/collections.preludes.json");

#[test]
fn collections_match_bun() {
    // Há casos de recursão por setter em `Array.prototype[0]` que o bun responde com `RangeError: Maximum call
    // stack size exceeded.`; o limite lógico é o do VM, então a thread precisa de pilha grande (como `call_edge`).
    common::run_mapped_golden_big_stack(GOLDEN, PRELUDES, 800, "collections_case.js");
}
