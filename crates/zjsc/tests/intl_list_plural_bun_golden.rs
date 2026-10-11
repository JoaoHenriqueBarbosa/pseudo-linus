//! Golden de `Intl.ListFormat`, `Intl.PluralRules` e `Intl.Segmenter` contra o JavaScriptCore do bun:
//! `tests/golden/intl_list_plural_bun.tsv` sai de `scripts/gen-intl-list-plural-golden.js`, rodado no bun 1.4.2. Cada
//! linha é um programa e o texto da variável global `R` que ele grava. Cobre `ListFormat` (type e style em 15 locales,
//! listas de 0 a 5 itens, `formatToParts`, iteráveis exóticos e itens não string com a mensagem exata do TypeError),
//! `PluralRules` (select e selectRange, cardinal e ordinal em 20 locales, números de fronteira, dígitos mínimos de
//! fração) e `Segmenter` (grapheme, word e sentence, `containing()` em índices de fronteira, `isWordLike`).
mod common;

const GOLDEN: &str = include_str!("golden/intl_list_plural_bun.tsv");
const PRELUDES: &str = include_str!("golden/intl_list_plural.preludes.json");

#[test]
fn intl_list_plural_matches_bun() {
    common::run_factored_big_stack(GOLDEN, PRELUDES, 3000, |source| common::EvalMode::IndirectEval.evaluate(source, "intl_list_plural_case.js", "R"));
}
