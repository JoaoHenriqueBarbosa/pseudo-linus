//! Golden de RegExp de borda contra o JavaScriptCore do bun: `tests/golden/regexp_edge_bun.tsv` sai de
//! `scripts/gen-regexp-edge-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável
//! global `R` que ele grava. Cobre named groups duplicados, lookbehind, propriedades Unicode, flag v (conjuntos e
//! strings), flag d (indices), backreferences nomeadas, quantificadores lazy aninhados, sticky e lastIndex,
//! Symbol.replace/split/matchAll personalizados, `RegExp.escape`, modifiers inline e mensagens de SyntaxError.
mod common;

const GOLDEN: &str = include_str!("golden/regexp_edge_bun.tsv");
const PRELUDES: &str = include_str!("golden/regexp_edge.preludes.json");

#[test]
fn regexp_edge_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 2000, |source| common::EvalMode::IndirectEval.evaluate(source, "regexp_edge_case.js", "R"));
}
