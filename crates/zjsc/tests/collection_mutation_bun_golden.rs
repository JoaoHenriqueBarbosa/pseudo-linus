//! Golden de Map/Set/WeakMap/WeakSet sob mutação contra o JavaScriptCore do bun: `tests/golden/collection_mutation_bun.tsv`
//! sai de `scripts/gen-collection-mutation-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da
//! variável global `R` que ele grava. Cobre delete, re-add, clear e add durante forEach, for-of e iterador manual,
//! iteradores esgotados que voltam após add, ordem de inserção, chaves -0/NaN/BigInt/símbolo/objeto, chaves inválidas de
//! Weak*, Map.groupBy, os métodos novos de Set com set-likes que registram acessos e mensagens de erro, subclasses e
//! species, construtores com iterável que lança no meio (IteratorClose) e getOrInsert/getOrInsertComputed.
mod common;


const GOLDEN: &str = include_str!("golden/collection_mutation_bun.tsv");

#[test]
fn collection_mutation_matches_bun() {
    common::run_golden(GOLDEN, 800, |source| common::EvalMode::RunInThisContext.evaluate(source, "", "R"));
}
