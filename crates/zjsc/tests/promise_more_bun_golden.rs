//! Golden de Promise avançado contra o JavaScriptCore real:
//! `tests/golden/promise_more_bun.tsv` sai de `scripts/gen-promise-more-golden.js`, rodado no bun 1.4.2.
//! Cobre subclasses com constructor hostil e species, thenables que lançam ou com getter `then` (inclusive em
//! `Object.prototype`), ciclo de resolução, combinadores com iteráveis hostis, ordem dos errors do
//! AggregateError, `finally` com thenables e `this` inválido, await de thenable contra promessa nativa,
//! funções de resolução (name, length, chamadas repetidas), executor reentrante e ordem entre várias cadeias.
//! Cada linha é um arquivo: o harness de `tests/golden/async_bun_harness.js` (o prelúdio do golden), o programa, que
//! registra eventos no array global `log`, e a global `R`, que devolve o JSON do log depois de esvaziar as microtarefas,
//! ou `error`, `name`, `message` (JSON) se o programa lançou de forma síncrona. O arquivo se chama
//! `promise_more_case.js` dos dois lados. queueMicrotask e timers têm golden próprio (`queue_microtask_bun_golden.rs`, `timers_bun_golden.rs`);
//! PENDENTE (medido no bun, não fora do escopo): `process.nextTick` antes das microtasks, que depende do global `process`, ainda ausente.
mod common;

const GOLDEN: &str = include_str!("golden/promise_more_bun.tsv");
const PRELUDES: &str = include_str!("golden/promise_more.preludes.json");

#[test]
fn promise_more_programs_match_bun() {
    common::run_mapped_golden(GOLDEN, PRELUDES, 450, "promise_more_case.js");
}
