//! Golden ampliado de `Error.stack` contra o JavaScriptCore do bun: `tests/golden/stack_more_bun.tsv` sai de
//! `scripts/gen-stack-more-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (o texto canônico que o bun
//! transpila, com prelúdio fatorado em `stack_more.preludes.json`), o texto da variável global `R` que ele grava,
//! depois de esvaziadas as microtarefas, e a quinta coluna com o modo e o mapa de posições. O arquivo se chama
//! `file.js` dos dois lados, então nome, linha e coluna de cada frame têm de ser idênticos.
//! Programa que não define `R` (erro de sintaxe no programa inteiro, exceção não capturada) vale `<undefined>`,
//! que é o que o bun imprime quando `R` não foi gravado.
mod common;

use common::{check_with_meta, evaluate_golden_program, guarded_units, Units};

const GOLDEN: &str = include_str!("golden/stack_more_bun.tsv");
const PRELUDES: &str = include_str!("golden/stack_more.preludes.json");

#[test]
fn more_stack_traces_match_bun() {
    assert_eq!(GOLDEN.lines().filter(|line| !line.is_empty()).count(), 500, "o golden tem 500 programas");
    check_with_meta(GOLDEN, PRELUDES, 500, |source, meta| {
        // Exceção que escapa do programa deixa `R` sem valor no bun: `<undefined>`.
        guarded_units(|| evaluate_golden_program(source, meta, "file.js", "R"))
            .or_else(|reason| if reason == "o programa lançou exceção" { Ok(Units::from("<undefined>")) } else { Err(reason) })
    });
}
