//! Golden das tabelas Unicode do yarr contra o JavaScriptCore do bun: `tests/golden/regexp_tables_bun.tsv` sai de
//! `scripts/gen-regexp-tables-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da
//! variável global `R` que ele grava. Cobre o case folding de `/\u{X}/iu` e `/\u{X}/iv` (quais pontos equivalentes
//! casam) e as fronteiras de intervalo de cerca de 270 propriedades `\p{...}` e `\P{...}` (Script, scx, gc e binárias).
mod common;

const GOLDEN: &str = include_str!("golden/regexp_tables_bun.tsv");

#[test]
fn regexp_unicode_tables_match_bun() {
    common::run_golden(GOLDEN, 800, |source| common::EvalMode::IndirectEval.evaluate(source, "regexp_tables_case.js", "R"));
}
