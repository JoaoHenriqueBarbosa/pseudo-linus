//! Golden complementar de String com Unicode de borda contra o JavaScriptCore do bun: `tests/golden/string_unicode_more_bun.tsv`
//! sai de `scripts/gen-string-unicode-more-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e
//! o texto da variável global `R` que ele grava. Cobre `normalize` (quatro formas, Hangul, ordem canônica, forma inválida),
//! caixa (ß, İ, sigma final, ligaduras, locales), `localeCompare`, `isWellFormed`/`toWellFormed`, `at`/`codePointAt` com
//! surrogates, `padStart`/`padEnd` com preenchimento longo, `replaceAll` com padrões `$`, `split` com regex Unicode,
//! `String.raw`, `matchAll` e as flags `v`, `u`, `d`, `y`.
mod common;

const GOLDEN: &str = include_str!("golden/string_unicode_more_bun.tsv");

#[test]
fn string_unicode_more_matches_bun() {
    common::run_golden(GOLDEN, 500, |source| common::EvalMode::IndirectEval.evaluate(source, "string_unicode_more_case.js", "R"));
}
