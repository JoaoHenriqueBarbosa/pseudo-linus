//! Golden de RegExp avançado contra o JavaScriptCore do bun: `tests/golden/regexp_v_bun.tsv` sai de
//! `scripts/gen-regexp-v-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável global
//! `R` que ele grava. Cobre a flag `v` (classes aninhadas, `&&`, `--`, `\q{...}`, propriedades de strings), `\p{...}`
//! (General_Category, Script, Script_Extensions, binárias), case folding unicode (`iu`, `iv`, ſ e Kelvin), named groups
//! duplicados, modificadores `(?i:...)`, lookbehind variável, backreferences nomeadas, `hasIndices`, mensagens exatas
//! de SyntaxError, `flags`/`source`/`toString` e `lastIndex` em unicode. Complementa `regexp_edge_bun_golden`.
mod common;

const GOLDEN: &str = include_str!("golden/regexp_v_bun.tsv");
const PRELUDES: &str = include_str!("golden/regexp_v.preludes.json");

#[test]
fn regexp_unicode_sets_and_properties_match_bun() {
    common::run_mapped_golden_big_stack(GOLDEN, PRELUDES, 3000, "regexp_v_case.js");
}
