//! Golden de ordem de microtarefas com thenables e promises contra o JavaScriptCore do bun:
//! `tests/golden/microtask_order_bun.tsv` sai de `scripts/gen-microtask-order-golden.js`. Cobre await de thenable com
//! getter de `then`, `then` que chama resolve duas vezes ou lança depois de resolver, `Promise.resolve` e await de
//! promise subclasse (getters de `constructor` e de `Symbol.species` observados), await em promise nativa vs thenable
//! (número de turnos), async generator com yield e return de promise, e `for await` sobre iterador síncrono com
//! promises rejeitadas. O log da ordem é a string global `R`, lida depois de esvaziadas as microtarefas. Cada linha é
//! um arquivo (prelúdio com o harness e o programa) e ele se chama `microtask_order_case.js` dos dois lados.
mod common;

const GOLDEN: &str = include_str!("golden/microtask_order_bun.tsv");
const PRELUDES: &str = include_str!("golden/microtask_order.preludes.json");

#[test]
fn microtask_order_matches_bun() {
    common::run_mapped_golden_big_stack(GOLDEN, PRELUDES, 3000, "microtask_order_case.js");
}
