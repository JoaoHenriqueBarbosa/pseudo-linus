//! Golden de ValidateAndApplyPropertyDescriptor em grade contra o JavaScriptCore do bun:
//! `tests/golden/define_property_grid_bun.tsv` sai de `scripts/gen-define-property-grid-golden.js`. Cada linha aplica um
//! descritor novo a um estado inicial (dado ou acessor com todas as combinações de writable, enumerable e configurable,
//! ausente em objeto extensível e não extensível) com `Object.defineProperty` e `Reflect.defineProperty` e registra o
//! descritor final. Inclui alvos exóticos: `length` e índices de array, typed array, String boxed, `arguments` mapeado e
//! funções (`length`, `name`, `prototype`).
mod common;

const GOLDEN: &str = include_str!("golden/define_property_grid_bun.tsv");
const PRELUDES: &str = include_str!("golden/define_property_grid.preludes.json");

#[test]
fn define_property_grid_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "define_property_grid_case.js", "R"));
}
