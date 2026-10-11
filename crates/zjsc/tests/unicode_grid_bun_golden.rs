//! Golden de grade de Unicode contra o JavaScriptCore do bun: `tests/golden/unicode_grid_bun.tsv` sai de
//! `scripts/gen-unicode-grid-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o texto da
//! variável global `R` que ele grava. Cobre `normalize` (NFC, NFD, NFKC, NFKD) em grade de sequências (hangul,
//! combinantes empilhados, singletons, compatibilidade, astrais), `toUpperCase`/`toLowerCase` com casos especiais,
//! `String.fromCodePoint`/`codePointAt` nos limites, surrogates soltos em cada método, `isWellFormed`/`toWellFormed`,
//! emojis com ZWJ, `at()`, iteração por code point e `encodeURI`/`decodeURI`/`escape`/`unescape` com os `URIError` exatos.
mod common;

const GOLDEN: &str = include_str!("golden/unicode_grid_bun.tsv");

#[test]
fn unicode_grid_matches_bun() {
    common::run_golden(GOLDEN, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "unicode_grid_case.js", "R"));
}
