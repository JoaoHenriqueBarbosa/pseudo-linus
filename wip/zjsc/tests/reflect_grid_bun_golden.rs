//! Golden em grade de `Reflect.*` e operadores de objeto contra o JavaScriptCore do bun: `tests/golden/reflect_grid_bun.tsv`
//! sai de `scripts/gen-reflect-grid-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o
//! texto da variável global `R` que ele grava. Cobre `Reflect.get`/`set` com receiver (primitivo, Proxy, accessor, não
//! gravável, typed array), as demais funções de `Reflect` contra alvos exóticos, argumentos inválidos com as mensagens
//! exatas, `CreateListFromArrayLike` com array-like exótico e comprimento enorme, `newTarget` inválido e a ordem dos efeitos
//! observada por um Proxy que registra cada trap, em `Reflect.*` e nos operadores de objeto.
//! O prelúdio comum das linhas fica em `tests/golden/reflect_grid.preludes.json` (ver `tests/common/mod.rs`).

mod common;

const GOLDEN: &str = include_str!("golden/reflect_grid_bun.tsv");
const PRELUDES: &str = include_str!("golden/reflect_grid.preludes.json");

#[test]
fn reflect_grid_matches_bun() {
    common::check(GOLDEN, PRELUDES, 2000, |source| common::guarded_units(|| common::EvalMode::IndirectEval.evaluate(source, "reflect_grid_case.js", "R")));
}
