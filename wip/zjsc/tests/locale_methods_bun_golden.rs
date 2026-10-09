//! Golden de métodos sensíveis a locale contra o JavaScriptCore do bun: `tests/golden/locale_methods_bun.tsv` sai de
//! `scripts/gen-locale-methods-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável
//! global `R` que ele grava. Cobre `String.prototype.localeCompare`, `normalize`, `toLocaleUpperCase` e
//! `toLocaleLowerCase`, `toLocaleString` de Number, BigInt, Date (sempre em UTC) e Array (inclusive typed arrays), além
//! de `Intl.getCanonicalLocales` e `Intl.supportedValuesOf`, com os locales en-US, pt-BR, de-DE, ja-JP, ar-EG, hi-IN
//! e tr-TR.
mod common;


const GOLDEN: &str = include_str!("golden/locale_methods_bun.tsv");

#[test]
fn locale_methods_match_bun() {
    common::run_golden_big_stack(GOLDEN, 1400, |source| common::EvalMode::RunInThisContext.evaluate(source, "", "R"));
}
