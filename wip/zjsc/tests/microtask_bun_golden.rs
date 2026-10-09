//! Golden de ordem exata de microtarefas contra o JavaScriptCore real:
//! `tests/golden/microtask_bun.tsv` sai de `scripts/gen-microtask-golden.js`, rodado no bun 1.4.2.
//! Cobre await em cada forma de função, fila de requests do async generator, `yield*` e `for await`
//! (AsyncFromSyncIterator e IteratorClose), combinadores de Promise com thenables, `finally` e species.
//! Cada linha é um arquivo: o harness de `tests/golden/async_bun_harness.js` (o prelúdio do golden), o programa, que
//! registra eventos no array global `log`, e a global `R`, que devolve o JSON do log depois de esvaziar as microtarefas,
//! ou `error`, `name`, `message` (JSON) se o programa lançou de forma síncrona. O arquivo se chama `microtask_case.js`
//! dos dois lados. queueMicrotask e timers têm golden próprio (`queue_microtask_bun_golden.rs`, `timers_bun_golden.rs`);
//! PENDENTE (medido no bun, não fora do escopo): `process.nextTick` antes das microtasks, que depende do global `process`, ainda ausente.
mod common;

const GOLDEN: &str = include_str!("golden/microtask_bun.tsv");
const PRELUDES: &str = include_str!("golden/microtask.preludes.json");

#[test]
fn microtask_programs_match_bun() {
    common::run_mapped_golden(GOLDEN, PRELUDES, 450, "microtask_case.js");
}
