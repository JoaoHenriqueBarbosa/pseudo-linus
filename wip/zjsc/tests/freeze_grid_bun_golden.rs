//! Golden da grade de `Object.freeze`/`seal`/`preventExtensions` contra o JavaScriptCore do bun: `tests/golden/freeze_grid_bun.tsv`
//! sai de `scripts/gen-freeze-grid-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o texto
//! da variável global `R` que ele grava. Cruza alvos (objeto, array esparso, array com `length` não gravável, função, classe,
//! typed array, `arguments`, `String` boxed, `Proxy`, objeto de protótipo nulo no estilo de namespace) com operações
//! posteriores em modo estrito e frouxo, e cobre descritores inválidos com getters que logam, `getOwnPropertyDescriptors`,
//! ciclos de `setPrototypeOf` e as mensagens exatas de `TypeError`.
mod common;

const GOLDEN: &str = include_str!("golden/freeze_grid_bun.tsv");
const PRELUDES: &str = include_str!("golden/freeze_grid.preludes.json");

#[test]
fn freeze_grid_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "freeze_grid_case.js", "R"));
}
