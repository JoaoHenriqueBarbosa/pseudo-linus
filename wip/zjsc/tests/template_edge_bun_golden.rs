//! Golden de template literals e tagged templates contra o JavaScriptCore do bun: `tests/golden/template_edge_bun.tsv`
//! sai de `scripts/gen-template-edge-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da
//! variável global `R` que ele grava, depois de esvaziadas as microtarefas. Cobre o cache do template object por site
//! (função, laço, eval, `new Function`), cooked vs raw com escapes inválidos, congelamento, formas de tag (membro,
//! chamada, `new`, optional chain), ordem de avaliação e conversão das substituições, CRLF/LS/PS, aninhamento,
//! `String.raw` com objetos raw exóticos e as sequências de escape de crase e `${`.
mod common;


const GOLDEN: &str = include_str!("golden/template_edge_bun.tsv");

#[test]
fn template_edge_matches_bun() {
    // O programa pode falhar na compilação (early error): o bun então deixa `R` indefinido e o golden registra isso,
    // por isso a exceção do script não encerra a medição.
    common::run_golden_big_stack(GOLDEN, 600, |source| common::EvalMode::RunInThisContext.evaluate(source, "", "R"));
}
