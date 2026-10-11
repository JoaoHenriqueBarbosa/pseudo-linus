//! Golden de `Error.stack` contra o JavaScriptCore do bun: `tests/golden/stack_bun.tsv` sai de
//! `scripts/gen-stack-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (o texto canônico que o bun
//! transpila, com prelúdio fatorado em `stack.preludes.json`), o texto da variável global `R` que ele grava depois
//! de esvaziadas as microtarefas, e a quinta coluna com o modo e o mapa de posições. O arquivo se chama
//! `stack_case.js` dos dois lados, então nome, linha e coluna de cada frame têm de ser idênticos.
mod common;

const GOLDEN: &str = include_str!("golden/stack_bun.tsv");
const PRELUDES: &str = include_str!("golden/stack.preludes.json");

#[test]
fn stack_traces_match_bun() {
    common::run_mapped_golden(GOLDEN, PRELUDES, 73, "stack_case.js");
}
