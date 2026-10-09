//! Golden do léxico de literais em grade contra o JavaScriptCore do bun: `tests/golden/lexer_grid_bun.tsv` sai de
//! `scripts/gen-lexer-grid-golden.js`, rodado no bun 1.4.2. Cada linha é um programa que avalia um trecho por
//! `(0,eval)` ou `new Function` e grava na variável global `R` o valor serializado ou o `Nome: mensagem` exato do
//! erro. A grade cobre escapes de string e template (`\x`, `\u`, `\u{}`, octais legados em sloppy e strict, `\8` e `\9`,
//! continuação de linha, LS e PS), literais numéricos (separadores, `0b`/`0o`/`0x`, octal legado, BigInt, ponto e
//! expoente), identificadores com escapes e palavras reservadas escapadas, espaços Unicode e terminadores de linha,
//! comentários HTML-like e hashbang.
mod common;

const GOLDEN: &str = include_str!("golden/lexer_grid_bun.tsv");
const PRELUDES: &str = include_str!("golden/lexer_grid.preludes.json");

#[test]
fn lexer_grid_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "lexer_grid_case.js", "R"));
}
