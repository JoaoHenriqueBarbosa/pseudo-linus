//! Golden do objeto `arguments` em grade contra o JavaScriptCore do bun: `tests/golden/arguments_grid_bun.tsv` sai de
//! `scripts/gen-arguments-grid-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o texto
//! da variável global `R` que ele grava. A grade cruza listas de parâmetros (simples, default, rest, destructuring,
//! duplicados em sloppy), modo (sloppy, strict externo, `'use strict'` interno), corpo (leitura e escrita com reflexo no
//! parâmetro, `length`, `delete`, `defineProperty` em índice mapeado, `Object.keys`, spread, `callee`, `Symbol.iterator`,
//! `toStringTag`, arrow aninhada, `eval` direto, função interna, closure em laço) e chamadas com menos ou mais argumentos.
mod common;

const GOLDEN: &str = include_str!("golden/arguments_grid_bun.tsv");
const PRELUDES: &str = include_str!("golden/arguments_grid.preludes.json");

#[test]
fn arguments_grid_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "arguments_grid_case.js", "R"));
}
