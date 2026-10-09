//! Golden de `String.prototype.replace`/`replaceAll` contra o JavaScriptCore do bun: `tests/golden/string_replace_bun.tsv`
//! sai de `scripts/gen-string-replace-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da
//! variável global `R` que ele grava. Cobre os padrões de substituição (`$$`, `$&`, `` $` ``, `$'`, `$n`, `$nn`, `$<nome>`),
//! padrão vazio e sobreposto, função substituta (argumentos, grupos nomeados, tipos devolvidos), `lastIndex` em
//! global/sticky, `Symbol.replace` customizado, TypeError de `replaceAll` com regex não global, surrogates, `at`,
//! `normalize` e `localeCompare` combinados. Complementa `string_bun_golden` e `string_unicode_more_bun_golden`.
mod common;

use zjsc::api::eval::evaluate_script_sequence_result;

const GOLDEN: &str = include_str!("golden/string_replace_bun.tsv");

#[test]
fn string_replace_special_patterns_match_bun() {
    common::run_golden_big_stack(GOLDEN, 800, |source| evaluate_script_sequence_result(&[source], "string_replace_case.js", "globalThis.R").1);
}
