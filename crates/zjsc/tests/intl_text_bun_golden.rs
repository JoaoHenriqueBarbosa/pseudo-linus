//! Golden de texto do Intl contra o JavaScriptCore do bun: `tests/golden/intl_text_bun.tsv` sai de
//! `scripts/gen-intl-text-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o texto da
//! variável global `R` que ele grava. Cobre `Intl.Collator` (sensitivity, numeric, caseFirst, ignorePunctuation, usage
//! search, extensões `-u-co`/`-kn`), `Intl.PluralRules` (select, selectRange, cardinal e ordinal, notation compact),
//! `Intl.ListFormat`, `Intl.RelativeTimeFormat`, `Intl.Segmenter` (grapheme, word, sentence, containing),
//! `Intl.DisplayNames`, `Intl.getCanonicalLocales` e `Intl.supportedValuesOf`.
mod common;

use zjsc::runtime::process_time_zone::set_time_zone_spec_override;

const GOLDEN: &str = include_str!("golden/intl_text_bun.tsv");
const PRELUDES: &str = include_str!("golden/intl_text.preludes.json");

#[test]
fn intl_text_matches_bun() {
    common::run_factored_big_stack(GOLDEN, PRELUDES, 1500, |source| {
        set_time_zone_spec_override(Some("UTC"));
        common::EvalMode::IndirectEval.evaluate(source, "intl_text_case.js", "R")
    });
}
