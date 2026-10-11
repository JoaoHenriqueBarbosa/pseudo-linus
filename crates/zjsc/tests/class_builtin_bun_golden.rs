//! Golden de herança de built-ins e de classes contra o JavaScriptCore do bun: `tests/golden/class_builtin_bun.tsv` sai de
//! `scripts/gen-class-builtin-golden.js`, rodado no bun 1.4.2, um processo novo por programa. Cada linha é um programa
//! (JSON) e o texto da variável global `R` que ele grava. Cobre built-ins x newTarget (classe, função, bound, Proxy,
//! `prototype` inválido, outro realm), formas de construtor derivado, `new.target` em contextos, estáticos com `this` e
//! static blocks, `accessor` (ainda sem suporte no bun 1.4.2), privados e `#x in o` com carimbo por retorno de objeto,
//! `extends null` e não construtores, `Symbol.species` em subclasses e `toString` de classes e membros.
mod common;

const GOLDEN: &str = include_str!("golden/class_builtin_bun.tsv");
const PRELUDES: &str = include_str!("golden/class_builtin.preludes.json");

#[test]
fn class_builtin_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 1000, |source| common::EvalMode::IndirectEval.evaluate(source, "class_builtin_case.js", "R"));
}
