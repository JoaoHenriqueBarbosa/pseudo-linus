//! Golden dos buracos de cobertura por método contra o JavaScriptCore do bun: `tests/golden/builtins_gap_bun.tsv` sai de
//! `scripts/gen-builtins-gap-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o texto da
//! variável global `R` que ele grava. Cobre os métodos Annex B de `String` (`anchor`, `big`, `blink`, `bold`, `fixed`,
//! `fontcolor`, `fontsize`, `italics`, `link`, `small`, `strike`, `sub`, `sup`, `trimLeft`, `trimRight`, `substr`, `search`) e
//! os getters de flag de `RegExp`, `flags`, `source`, `toString` e `compile`, com argumentos limite, ordem de coerção
//! registrada e a mensagem exata de cada erro. O arquivo se chama `builtins_gap_case.js` dos dois lados.
mod common;

const GOLDEN: &str = include_str!("golden/builtins_gap_bun.tsv");

const PRELUDES: &str = include_str!("golden/builtins_gap.preludes.json");

#[test]
fn string_annex_b_and_regexp_flags_match_bun() {
    common::run_mapped_golden(GOLDEN, PRELUDES, 700, "builtins_gap_case.js");
}
