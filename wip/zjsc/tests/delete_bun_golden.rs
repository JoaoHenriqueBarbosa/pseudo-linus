//! Golden de `delete` de identificador contra o JavaScriptCore do bun: `tests/golden/delete_bun.tsv` sai de
//! `scripts/gen-delete-golden.js`, rodado no bun 1.4.2. Cada linha é um programa sloppy (JSON) e o texto da variável
//! global `R` que ele grava (`"Nome: mensagem"` quando lança). Cobre o `var` criado por eval sloppy (deletável e some do
//! escopo), os SyntaxError de strict mode (`Cannot delete unqualified property 'x' in strict mode.`), e os bindings de
//! `var`, parâmetro, `let`, `const`, função e `arguments` (que `delete` recusa com `false`).
mod common;

use zjsc::api::eval::evaluate_named_script_result;

const GOLDEN: &str = include_str!("golden/delete_bun.tsv");

#[test]
fn delete_identifier_matches_bun() {
    common::run_golden(GOLDEN, 30, |source| evaluate_named_script_result(source, "delete_case.js", "R"));
}
