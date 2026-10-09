//! Golden de this/brand dos métodos e getters de protótipo dos builtins contra o JavaScriptCore do bun:
//! `tests/golden/brand_bun.tsv` sai de `scripts/gen-brand-check-golden.js`, rodado no bun 1.4.2. Cada linha é um
//! programa (um método ou getter, um `this`) e o texto da variável global `R` que ele grava: `typeof resultado` ou
//! `Nome|mensagem` do erro, depois de esvaziadas as microtarefas.
mod common;

const GOLDEN: &str = include_str!("golden/brand_bun.tsv");

const PRELUDES: &str = include_str!("golden/brand.preludes.json");

#[test]
fn brand_checks_match_bun() {
    common::run_mapped_golden(GOLDEN, PRELUDES, 1000, "brand_case.js");
}
