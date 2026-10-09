//! Golden de borda de `Object.*` e `Reflect.*` contra o JavaScriptCore do bun: `tests/golden/object_edge_bun.tsv` sai de
//! `scripts/gen-object-edge-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o texto da
//! variável global `R` que ele grava. Cobre `defineProperty`/`defineProperties` com descritores parciais e inválidos,
//! `freeze`/`seal`/`preventExtensions` em arrays, typed arrays e funções, `getOwnPropertyDescriptors`, `fromEntries`,
//! `groupBy`, `setPrototypeOf` com ciclos, `__proto__`, `__defineGetter__`/`__lookupGetter__`, `Object.assign` com
//! getters e símbolos, `Reflect.construct` com `newTarget` e a ordem de `Reflect.ownKeys` (inteiro, string, símbolo).
mod common;

const GOLDEN: &str = include_str!("golden/object_edge_bun.tsv");

#[test]
fn object_edge_matches_bun() {
    common::run_golden(GOLDEN, 1500, |source| common::EvalMode::IndirectEval.evaluate(source, "object_edge_case.js", "R"));
}
