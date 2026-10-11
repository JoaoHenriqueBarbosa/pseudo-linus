//! Golden da grade de escopo e closures contra o JavaScriptCore do bun: `tests/golden/scope_grid_bun.tsv` sai de
//! `scripts/gen-scope-grid-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável
//! global `R` que ele grava. Cobre captura em laços, let/const em switch, funções em blocos (Annex B) em sloppy e
//! strict, shadowing de parâmetros, defaults com escopo próprio, catch com destructuring, binding interno de class e
//! função, NFE, eval com var, `with`, TDZ, globais let/var e shadowing de built-ins.
mod common;

use zjsc::api::eval::evaluate_named_script_result;

const GOLDEN: &str = include_str!("golden/scope_grid_bun.tsv");
const PRELUDES: &str = include_str!("golden/scope_grid.preludes.json");

#[test]
fn scope_grid_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 2000, |source| evaluate_named_script_result(source, "scope_grid_case.js", "R"));
}
