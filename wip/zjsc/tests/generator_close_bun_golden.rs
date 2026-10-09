//! Golden de controle não local em generators e async contra o JavaScriptCore do bun: `tests/golden/generator_close_bun.tsv`
//! sai de `scripts/gen-generator-close-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (prelúdio fatorado) e o
//! texto da variável global `R`, um acessor que junta o log das chamadas depois de esvaziar as microtarefas.
//! Cobre break/continue rotulados atravessando try/finally com yield/await no finally, return em finally sobrescrevendo
//! throw, for-of com break cujo return() faz yield, lança ou devolve não objeto, destructuring de generator com elementos
//! a mais e a menos, spread, Array.from com mapfn que lança, Promise.all/allSettled/race/any, new Map/Set/WeakMap com
//! generator que lança depois de duas entradas, for await e geradores assíncronos, e a ordem das chamadas no log.
//! O prelúdio comum fica em `tests/golden/generator_close.preludes.json` (ver `tests/common/mod.rs`).

mod common;

const GOLDEN: &str = include_str!("golden/generator_close_bun.tsv");
const PRELUDES: &str = include_str!("golden/generator_close.preludes.json");

#[test]
fn generator_close_matches_bun() {
    common::run_mapped_golden_big_stack(GOLDEN, PRELUDES, 3000, "generator_close_case.js");
}
