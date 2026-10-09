//! Golden de grade de `Promise` contra o JavaScriptCore do bun: `tests/golden/promise_grid_bun.tsv` sai de
//! `scripts/gen-promise-grid-golden.js`, rodado no bun 1.4.2. Cada linha é um arquivo: o prelúdio do golden (o harness
//! do gerador), o programa e a global `R`, que devolve o JSON do array de rótulos (`labels`) depois de esvaziar as
//! microtarefas. Cobre `Promise.all`/`allSettled`/`any`/`race`/`withResolvers`/`try` com thenables que lançam, getters
//! de `then`, subclasses com constructor customizado, resolve com ciclo, rejeição não tratada com `Symbol`, a ordem das
//! reações de `then` em grade, async functions com await de thenable e de subclasse, `finally` com retorno e
//! lançamento, receptores inválidos e as mensagens de erro. O arquivo se chama `promise_grid_case.js` dos dois lados.
mod common;

const GOLDEN: &str = include_str!("golden/promise_grid_bun.tsv");
const PRELUDES: &str = include_str!("golden/promise_grid.preludes.json");

#[test]
fn promise_grid_programs_match_bun() {
    common::run_mapped_golden_big_stack(GOLDEN, PRELUDES, 2800, "promise_grid_case.js");
}
