//! Golden das estáticas legadas de RegExp e de `@@replace`/`split`/`matchAll` contra o JavaScriptCore do bun:
//! `tests/golden/regexp_legacy_bun.tsv` sai de `scripts/gen-regexp-legacy-golden.js`, rodado no bun 1.4.2. Cada linha é
//! um programa (JSON, várias linhas) e o texto da variável global `R` que ele grava. Cobre `RegExp.$1..$9`,
//! `lastMatch`, `lastParen`, `leftContext`, `rightContext`, `input` e os aliases, a atualização depois de cada
//! operação, subclasses, outro realm via eval indireto, `$<nome>`, `$0`, `$01`, `$10`, replacer função, `lastIndex`
//! depois de replace, `replaceAll` com regex não global, `split` com captura e limite, `matchAll` e o iterador.
mod common;

const GOLDEN: &str = include_str!("golden/regexp_legacy_bun.tsv");

const PRELUDES: &str = include_str!("golden/regexp_legacy.preludes.json");

#[test]
fn regexp_legacy_and_replace_match_bun() {
    common::run_mapped_golden(GOLDEN, PRELUDES, 900, "regexp_legacy_case.js");
}
