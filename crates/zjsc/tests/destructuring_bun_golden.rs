//! Golden de destructuring e parâmetros contra o JavaScriptCore do bun: `tests/golden/destructuring_bun.tsv` sai de
//! `scripts/gen-destructuring-golden.js`, rodado no bun. Cada linha é um programa (JSON) e o texto da variável global
//! `R` que ele grava, depois de esvaziadas as microtarefas. Cobre array e object patterns (defaults, rest, aninhados),
//! iteradores que lançam e fecham, ordem de avaliação, parâmetros com defaults e escopo, arguments mapeado e não
//! mapeado, TDZ de parâmetros, for-of e for-in com patterns, catch com pattern e os SyntaxError correspondentes.
mod common;


const GOLDEN: &str = include_str!("golden/destructuring_bun.tsv");

#[test]
fn destructuring_and_parameters_match_bun() {
    // O programa pode falhar na compilação: o bun então deixa `R` indefinido e o golden registra isso, por isso a
    // exceção do script não encerra a medição.
    common::run_golden_big_stack(GOLDEN, 1000, |source| common::EvalMode::RunInThisContext.evaluate(source, "", "R"));
}
