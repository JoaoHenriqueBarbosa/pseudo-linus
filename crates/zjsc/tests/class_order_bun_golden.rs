//! Golden de ordem de avaliação e semântica de classes contra o JavaScriptCore do bun: `tests/golden/class_order_bun.tsv`
//! sai de `scripts/gen-class-order-golden.js`, rodado no bun 1.4.2. Cada linha é um programa e o texto da variável global
//! `R` que ele grava. Cobre getters e setters privados (de instância e estáticos) com herança, Proxy e outra classe de
//! mesmo corpo, `super` em object literals e em static blocks, campos e métodos, `new.target` com `Reflect.construct` e
//! newTarget de várias formas, `class extends (class {})` e outras expressões de herança, a palavra `accessor`,
//! `#x in obj` e o log da ordem de avaliação de computed keys, campos estáticos e blocos com efeitos.
mod common;

const GOLDEN: &str = include_str!("golden/class_order_bun.tsv");
const PRELUDES: &str = include_str!("golden/class_order.preludes.json");

#[test]
fn class_order_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "class_order_case.js", "R"));
}
