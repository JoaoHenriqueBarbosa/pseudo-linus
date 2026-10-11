//! Golden do `queueMicrotask` do global contra o bun: `tests/golden/queue_microtask_bun.tsv` sai de
//! `scripts/gen-queue-microtask-golden.js`. Cobre o descritor, `name`, `length`, `toString`, a ordem relativa a
//! `Promise.resolve().then` e a `await`, e a mensagem do erro com argumento que não é função. O log da ordem é a
//! string global `R`, lida depois de esvaziadas as microtarefas. Cada linha é um arquivo (o prelúdio de
//! `microtask_order` e o programa) chamado `queue_microtask_case.js` dos dois lados.
mod common;

const GOLDEN: &str = include_str!("golden/queue_microtask_bun.tsv");
const PRELUDES: &str = include_str!("golden/queue_microtask.preludes.json");

#[test]
fn queue_microtask_matches_bun() {
    common::run_mapped_golden_big_stack(GOLDEN, PRELUDES, 25, "queue_microtask_case.js");
}
