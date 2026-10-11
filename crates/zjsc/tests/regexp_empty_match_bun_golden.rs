//! Golden de RegExp que casa vazio contra o JavaScriptCore do bun: `tests/golden/regexp_empty_match_bun.tsv` sai de
//! `scripts/gen-regexp-empty-match-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da
//! variável global `R` que ele grava. Cobre Symbol.split/replace/match/matchAll/search com padrões que casam vazio,
//! flags g, y, u, v, strings com surrogates, limit de split, replace com função, lastIndex inicial variado e o avanço
//! de lastIndex por code point contra code unit.
mod common;

const GOLDEN: &str = include_str!("golden/regexp_empty_match_bun.tsv");
const PRELUDES: &str = include_str!("golden/regexp_empty_match.preludes.json");

#[test]
fn regexp_empty_match_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "regexp_empty_match_case.js", "R"));
}
