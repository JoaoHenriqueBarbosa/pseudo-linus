//! Golden de eval e escopo dinâmico contra o JavaScriptCore do bun: `tests/golden/eval_bun.tsv` sai de
//! `scripts/gen-eval-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável global
//! `R` que ele grava, depois de esvaziadas as microtarefas. Cobre eval direto e indireto (this, hoisting, strict,
//! let/const isolados, new.target, arguments, super), `with` (Symbol.unscopables, Proxy, ordem de has/get), `delete`
//! de variável, conflito de declaração com let global, `new Function`, `arguments` mapeado, binding imutável de função
//! nomeada, var/let/class globais e `eval` como nome. Cada programa roda como script, num realm novo.
mod common;

use zjsc::api::eval::evaluate_named_script_result;

const GOLDEN: &str = include_str!("golden/eval_bun.tsv");

#[test]
fn eval_and_dynamic_scope_match_bun() {
    common::run_golden(GOLDEN, 1500, |source| evaluate_named_script_result(source, "eval_case.js", "R"));
}
