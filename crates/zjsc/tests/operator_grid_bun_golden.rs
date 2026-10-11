//! Golden da grade de operadores contra o JavaScriptCore do bun: `tests/golden/operator_grid_bun.tsv` sai de
//! `scripts/gen-operator-grid-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o texto
//! da variável global `R` que ele grava (`resultado|rastro` ou `throw Nome: mensagem|rastro`). Cobre os operadores
//! binários, unários, `++`/`--`, atribuição composta e lógica, `in`, `instanceof`, `?.`, `??` e as conversões explícitas
//! sobre valores exóticos (objetos com `valueOf`/`toString`/`@@toPrimitive` que registram a ordem, Proxy, Symbol,
//! BigInt, `-0`, strings numéricas). Complementa `coercion_bun`, `coercion_semantics_bun` e `operator_edge_bun`.
//! O prelúdio comum das linhas fica em `tests/golden/operator_grid.preludes.json` (ver `tests/common/mod.rs`).

mod common;

const GOLDEN: &str = include_str!("golden/operator_grid_bun.tsv");
const PRELUDES: &str = include_str!("golden/operator_grid.preludes.json");

#[test]
fn operator_grid_matches_bun() {
    common::check(GOLDEN, PRELUDES, 3000, |source| common::guarded_units(|| common::EvalMode::IndirectEval.evaluate(source, "operator_grid_case.js", "R")));
}
