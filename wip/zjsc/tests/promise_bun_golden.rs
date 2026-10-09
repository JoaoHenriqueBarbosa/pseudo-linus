//! Golden focado em Promise e ordem de microtarefas contra o JavaScriptCore real:
//! `tests/golden/promise_bun.tsv` sai de `scripts/gen-promise-golden.js`, rodado no bun 1.4.2.
//! Cada linha é um arquivo: o harness de `tests/golden/async_bun_harness.js` (o prelúdio do golden), o programa, que
//! registra eventos no array global `log`, e a global `R`, que devolve o JSON do log depois de esvaziar as microtarefas,
//! ou `error`, `name`, `message` (JSON) se o programa lançou de forma síncrona. O arquivo se chama `promise_case.js`
//! dos dois lados.
mod common;

const GOLDEN: &str = include_str!("golden/promise_bun.tsv");
const PRELUDES: &str = include_str!("golden/promise.preludes.json");

#[test]
fn promise_programs_match_bun() {
    common::run_mapped_golden(GOLDEN, PRELUDES, 1200, "promise_case.js");
}
