//! Golden do protocolo de iteração observável nos built-ins consumidores contra o JavaScriptCore do bun:
//! `tests/golden/iter_protocol_bun.tsv` sai de `scripts/gen-iter-protocol-golden.js`. Cobre `Array.from`, construtores de
//! Map/Set/WeakMap/WeakSet, `Promise.all/allSettled/any/race`, `Object.fromEntries`, `TypedArray.from`, spread,
//! desestruturação, for-of e `yield*` sobre iteradores cujo `next`/`return`/`throw` lançam ou devolvem não objeto, com
//! getters de `done`/`value`, `Symbol.iterator` devolvendo primitivo e `%ArrayIteratorPrototype%.next` sobrescrito; o log
//! registra a ordem exata dos acessos. O gerador avalia o programa inteiro com `(0, eval)(fonte)` num bun filho, sem
//! arquivo nem transpilação; o teste faz o mesmo com o `eval` indireto, e o valor de completude é o `globalThis.R = ...`.
mod common;

const GOLDEN: &str = include_str!("golden/iter_protocol_bun.tsv");
const PRELUDES: &str = include_str!("golden/iter_protocol.preludes.json");

#[test]
fn iter_protocol_matches_bun() {
    common::run_factored_big_stack(GOLDEN, PRELUDES, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "", "R"));
}
