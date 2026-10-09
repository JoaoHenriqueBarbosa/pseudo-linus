//! Golden de String com Unicode de borda contra o JavaScriptCore do bun: `tests/golden/string_unicode_bun.tsv` sai de
//! `scripts/gen-string-unicode-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o
//! texto da variável global `R` que ele grava. Cobre surrogates soltos, caixa especial, normalização, limites de
//! tamanho de string, `new String`, template com Symbol e `encodeURI`/`decodeURI`/`escape`/`unescape`.
mod common;

const GOLDEN: &str = include_str!("golden/string_unicode_bun.tsv");

const PRELUDES: &str = include_str!("golden/string_unicode.preludes.json");

#[test]
fn string_unicode_matches_bun() {
    common::run_mapped_golden(GOLDEN, PRELUDES, 1000, "string_unicode_case.js");
}
