//! Golden de funções e this/new/super contra o JavaScriptCore do bun: `tests/golden/ctor_this_bun.tsv` sai de
//! `scripts/gen-ctor-this-golden.js`, rodado no bun com `node:vm` (`runInThisContext`). Cada linha é um programa
//! (JSON) e o texto da variável global `R` que ele grava, depois de esvaziadas as microtarefas. Cobre
//! `Function.prototype.bind/call/apply` (name, length, new em bound, `Symbol.hasInstance`), `new.target` em todos os
//! contextos, construtores derivados e retorno de objeto/primitivo, `super()` duas vezes, `this` antes de `super`,
//! class fields e static blocks com `this`, métodos e accessors privados, `#x in obj`, herança de built-ins (Array,
//! Error, Promise, Map, RegExp) e `Symbol.species`.
mod common;

use zjsc::api::eval::evaluate_script_sequence_result;

const GOLDEN: &str = include_str!("golden/ctor_this_bun.tsv");

#[test]
fn constructors_and_this_match_bun() {
    common::run_golden_big_stack(GOLDEN, 1000, |source| evaluate_script_sequence_result(&[source], "ctor_this_case.js", "globalThis.R").1);
}
