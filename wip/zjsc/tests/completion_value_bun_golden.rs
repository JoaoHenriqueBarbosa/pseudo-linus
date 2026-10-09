//! Golden do valor de completude (completion value) de statements contra o JavaScriptCore do bun:
//! `tests/golden/completion_value_bun.tsv` sai de `scripts/gen-completion-value-golden.js`, rodado no bun 1.4.2. Cada
//! linha é o sufixo do programa (JSON) e o texto da variável global `R` (JSON); o programa avalia a fonte por `eval`
//! direto sloppy, indireto ou direto com "use strict" e grava o valor formatado por `S`. O prelúdio comum fica em
//! `tests/golden/completion_value.preludes.json` (ver `tests/common/mod.rs`).

mod common;

const GOLDEN: &str = include_str!("golden/completion_value_bun.tsv");
const PRELUDES: &str = include_str!("golden/completion_value.preludes.json");

#[test]
fn completion_values_match_bun() {
    common::check(GOLDEN, PRELUDES, 3000, |source| common::guarded_units(|| common::EvalMode::IndirectEval.evaluate(source, "completion_case.js", "R")));
}
