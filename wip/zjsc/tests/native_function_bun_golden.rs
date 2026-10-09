//! Golden de função nativa como objeto contra o JavaScriptCore do bun: `tests/golden/native_function_bun.tsv` sai de
//! `scripts/gen-native-function-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável
//! global `R` que ele grava. Para cada função, getter e setter dos builtins: chaves próprias, atributos de
//! `length`/`name`/`prototype`, `IsConstructor`, `new`, `Reflect.construct` com `newTarget`, `Function.prototype.toString`
//! e o resultado (valor ou mensagem de exceção) de chamadas com receptores e argumentos fixos.
mod common;

const GOLDEN: &str = include_str!("golden/native_function_bun.tsv");

#[test]
fn native_function_matches_bun() {
    common::run_golden(GOLDEN, 300, |source| common::EvalMode::IndirectEval.evaluate(source, "native_function_case.js", "R"));
}
