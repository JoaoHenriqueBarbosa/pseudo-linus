//! Golden de Proxy, Reflect, classes (campos privados, static blocks, accessor, herança de builtins, new.target, super)
//! e Symbol contra o JavaScriptCore do bun: `tests/golden/proxy_class_bun.tsv` sai de
//! `scripts/gen-proxy-class-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável
//! global `R` que ele grava, depois de esvaziadas as microtarefas: o valor formatado ou `NomeDoErro: mensagem`.
mod common;

const GOLDEN: &str = include_str!("golden/proxy_class_bun.tsv");

const PRELUDES: &str = include_str!("golden/proxy_class.preludes.json");

#[test]
fn proxy_and_class_match_bun() {
    common::run_mapped_golden(GOLDEN, PRELUDES, 600, "proxy_class_case.js");
}
