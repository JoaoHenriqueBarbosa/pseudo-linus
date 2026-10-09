//! Golden de SyntaxError e early errors contra o JavaScriptCore do bun: `tests/golden/syntax_early_bun.tsv` sai de
//! `scripts/gen-syntax-early-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) que analisa uma fonte via
//! `new Function` (ou `eval` indireto) dentro de try/catch e grava `name: message` em `R` (ou "ok"), e o texto esperado
//! de `R`. A grade cobre redeclaração em escopos, `use strict` com parâmetros não simples, labels, break/continue/return,
//! palavras reservadas em sloppy, strict, generator e async, new.target e super, regex com flags, números, escapes de
//! string, `delete` em strict, alvos de atribuição, destructuring, optional chaining com template, import/export, await
//! top-level e membros de classe.
mod common;

const GOLDEN: &str = include_str!("golden/syntax_early_bun.tsv");

#[test]
fn syntax_early_matches_bun() {
    common::run_golden(GOLDEN, 2000, |source| common::EvalMode::IndirectEval.evaluate(source, "syntax_early_case.js", "R"));
}
