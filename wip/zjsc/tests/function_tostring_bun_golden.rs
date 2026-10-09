//! Function.prototype.toString contra o JavaScriptCore do bun: `tests/golden/function_tostring_bun.tsv` sai de
//! `scripts/gen-function-tostring-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da
//! variável global `R` que ele grava. Cobre o texto exato do fonte de funções, arrows, métodos, getters, setters e
//! classes (comentários, espaços, separadores Unicode, nomes computados e privados), bound functions e Proxy de função,
//! `new Function` e construtores de gerador e async, eval, funções nativas com nomes de símbolo e a TypeError de
//! toString em não funções.
mod common;

const GOLDEN: &str = include_str!("golden/function_tostring_bun.tsv");
const PRELUDES: &str = include_str!("golden/function_tostring.preludes.json");

#[test]
fn function_tostring_matches_bun() {
    // O programa pode falhar na compilação: o bun então deixa `R` indefinido e o golden registra isso, por isso a
    // exceção do script não encerra a medição.
    common::run_factored_big_stack(GOLDEN, PRELUDES, 1500, |source| common::EvalMode::IndirectEval.evaluate(source, "function_tostring_case.js", "R"));
}
