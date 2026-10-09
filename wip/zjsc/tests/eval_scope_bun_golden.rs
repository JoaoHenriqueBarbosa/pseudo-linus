//! Golden de eval e escopo, segunda leva, contra o JavaScriptCore do bun: `tests/golden/eval_scope_bun.tsv` sai de
//! `scripts/gen-eval-scope-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável
//! global `R` que ele grava, depois de esvaziadas as microtarefas. Cobre eval direto e indireto (vazamento de var,
//! strict, let/const, arguments, this, new.target), `new Function`/GeneratorFunction/AsyncFunction (corpo, parâmetros
//! com comentários, toString), `with` + Symbol.unscopables, hoisting de function em bloco (Annex B), `arguments`
//! mapeado e não mapeado, closures em laços, getters e setters em literais e classes, label + break em blocos, vírgula
//! e `void`/`typeof`/`delete` de borda. Cada programa roda como script, num realm novo.
mod common;

use zjsc::api::eval::evaluate_named_script_result;

const GOLDEN: &str = include_str!("golden/eval_scope_bun.tsv");

#[test]
fn eval_and_scope_edges_match_bun() {
    common::run_golden(GOLDEN, 450, |source| evaluate_named_script_result(source, "eval_scope_case.js", "R"));
}
