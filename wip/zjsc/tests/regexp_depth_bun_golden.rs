//! Golden de RegExp em profundidade contra o JavaScriptCore do bun: `tests/golden/regexp_depth_bun.tsv` sai de
//! `scripts/gen-regexp-depth-golden.js`, rodado no bun 1.4.2 (cada programa num bun filho novo). Cada linha é um
//! programa (JSON) e o texto da variável global `R` que ele grava. Complementa os goldens `regexp_*` com matrizes:
//! lookbehind da direita para a esquerda, reinício de capturas em laços, laços vazios, case folding de caracteres
//! especiais com i/iu/iv, coerção de `lastIndex`, backtracking pesado limitado, sintaxe Annex B, `Symbol.match`/
//! `replace`/`search`/`split`/`matchAll` com subclasses e `exec` personalizado, descritores do protótipo, escapes de
//! `source`, propriedades Unicode em amostras, flag d e mensagens exatas de SyntaxError.
mod common;

const GOLDEN: &str = include_str!("golden/regexp_depth_bun.tsv");
const PRELUDES: &str = include_str!("golden/regexp_depth.preludes.json");

#[test]
fn regexp_depth_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 1500, |source| common::EvalMode::IndirectEval.evaluate(source, "regexp_depth_case.js", "R"));
}
