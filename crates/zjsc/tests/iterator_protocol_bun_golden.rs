//! Golden do protocolo de geradores e iteradores nativos contra o JavaScriptCore do bun:
//! `tests/golden/iterator_protocol_bun.tsv` sai de `scripts/gen-iterator-protocol-golden.js`, rodado no bun 1.4.2.
//! Cada linha é um programa (JSON) e o texto da variável global `R` que ele grava, depois de esvaziadas as
//! microtarefas. Cobre `Generator.prototype.next/return/throw` em todos os estados, `yield*` com iteradores sem
//! `return`/`throw`, `AsyncGenerator` com fila de pedidos, helpers de `Iterator.prototype`, os iteradores nativos
//! (Array, String, Map, Set, RegExpStringIterator), `%IteratorPrototype%[Symbol.iterator]` e o fechamento de iterador
//! em destructuring, for-of, spread e consumidores nativos.
mod common;


const GOLDEN: &str = include_str!("golden/iterator_protocol_bun.tsv");

#[test]
fn iterator_protocol_matches_bun() {
    // O programa pode falhar na compilação: o bun então deixa `R` indefinido e o golden registra isso, por isso a
    // exceção do script não encerra a medição.
    common::run_golden_big_stack(GOLDEN, 400, |source| common::EvalMode::RunInThisContext.evaluate(source, "", "R"));
}
