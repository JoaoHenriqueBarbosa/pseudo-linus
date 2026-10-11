//! Golden de String Unicode (terceiro complemento) contra o JavaScriptCore do bun: `tests/golden/string_unicode_extra_bun.tsv`
//! sai de `scripts/gen-string-unicode-extra-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o
//! texto da variável global `R` que ele grava. Cobre caixa com locale (lt, tr, az, el), normalize de Hangul, combinantes e
//! compatíveis, split/indexOf/includes com surrogates isolados, codePointAt/fromCodePoint nos limites, isWellFormed/toWellFormed,
//! localeCompare com acentos, encodeURI/decodeURI/escape/unescape com URIError, trim e conversões numéricas com espaços Unicode,
//! identificadores Unicode e escapes `\u` inválidos em strings e templates.
mod common;

const GOLDEN: &str = include_str!("golden/string_unicode_extra_bun.tsv");
const PRELUDES: &str = include_str!("golden/string_unicode_extra.preludes.json");

#[test]
fn string_unicode_extra_matches_bun() {
    common::run_factored(GOLDEN, PRELUDES, 1500, |source| common::EvalMode::IndirectEval.evaluate(source, "string_unicode_extra_case.js", "R"));
}
