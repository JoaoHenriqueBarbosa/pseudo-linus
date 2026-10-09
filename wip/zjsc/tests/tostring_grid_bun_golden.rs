//! Golden de `Object.prototype.toString` e da conversão para string em grade contra o JavaScriptCore do bun:
//! `tests/golden/tostring_grid_bun.tsv` sai de `scripts/gen-tostring-grid-golden.js`. A grade cruza valores (primitivos,
//! boxed, built-ins e instâncias, Proxy e revogados, `arguments`, subclasses de Error, classes com `toStringTag`
//! estático) com variantes de `Symbol.toStringTag` e com `String(x)`, template, `+`, `Object.prototype.toString.call`
//! e `Array.prototype.toString`; cobre ainda a ordem das armadilhas de Proxy, `Function.prototype.toString` (built-ins,
//! bound, Proxy), `Symbol.prototype.description`/`toString` e `Array.prototype.toString` com `join` que não é função.
mod common;

const GOLDEN: &str = include_str!("golden/tostring_grid_bun.tsv");
const PRELUDES: &str = include_str!("golden/tostring_grid.preludes.json");

#[test]
fn tostring_grid_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "tostring_grid_case.js", "R"));
}
