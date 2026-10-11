//! Golden de construção dinâmica de função e eval contra o JavaScriptCore do bun: `tests/golden/dynamic_fn_bun.tsv` sai
//! de `scripts/gen-dynamic-fn-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável
//! global `R` que ele grava. Cobre `Function(...)` e `new Function` com parâmetros em vários formatos (rest, defaults,
//! destructuring, comentários, vírgulas finais, nomes inválidos, injeção `a){`), os construtores de GeneratorFunction,
//! AsyncFunction e AsyncGeneratorFunction, `Function.prototype.toString` de cada forma (nomes, computed, getters,
//! métodos, classes, async arrow, native de built-ins e bound), eval direto x indireto (this, vazamento de var,
//! propagação de strict, new.target, super em método, arguments), valores de conclusão do eval e as mensagens exatas de
//! SyntaxError de Function() e eval.
mod common;


const GOLDEN: &str = include_str!("golden/dynamic_fn_bun.tsv");

#[test]
fn dynamic_function_and_eval_match_bun() {
    // O programa pode falhar na compilação: o bun então deixa `R` indefinido e o golden registra isso, por isso a
    // exceção do script não encerra a medição.
    common::run_golden_big_stack(GOLDEN, 2000, |source| common::EvalMode::RunInThisContext.evaluate(source, "", "R"));
}
