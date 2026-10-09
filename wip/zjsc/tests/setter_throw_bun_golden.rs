//! Golden de getter e setter que lançam (ou retornam normalmente) contra o JavaScriptCore do bun:
//! `tests/golden/setter_throw_bun.tsv` sai de `scripts/gen-setter-throw-golden.js`, rodado no bun com um processo filho
//! por programa. Cada linha é um programa (JSON) e o texto da variável global `R` (o log de eventos). Cobre atribuição
//! por nome, colchete e índice, destructuring com iterador que tem ou não `return()`, compound assignment, `super.x`,
//! Proxy e `Reflect.set` com receiver, acessores estáticos e privados de classe, em uma grade de posições de
//! try/catch/finally, for-of com break e return, generator, async, switch, parâmetro padrão e bloco estático.
mod common;

use zjsc::api::eval::evaluate_script_sequence_result;

const GOLDEN: &str = include_str!("golden/setter_throw_bun.tsv");
const PRELUDES: &str = include_str!("golden/setter_throw.preludes.json");

#[test]
fn setter_throw_matches_bun() {
    common::run_factored_big_stack(GOLDEN, PRELUDES, 2500, |source| evaluate_script_sequence_result(&[source], "setter_throw_case.js", "globalThis.R").1);
}
