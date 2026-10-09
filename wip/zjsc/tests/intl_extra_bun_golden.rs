//! Golden de Intl extra contra o JavaScriptCore do bun: `tests/golden/intl_extra_bun.tsv` sai de
//! `scripts/gen-intl-extra-golden.js`, rodado no bun. Cada linha é um programa (JSON) e o texto da variável global `R`
//! que ele grava. Cobre Intl.Segmenter (grapheme, word, sentence, emoji ZWJ, CJK, tailandês, bandeiras, isWordLike,
//! containing, iteração), Intl.ListFormat, Intl.PluralRules (cardinal, ordinal, selectRange, resolvedOptions),
//! Intl.Locale (maximize, minimize, getters, getWeekInfo, getTextInfo, getHourCycles, getCalendars,
//! getNumberingSystems, getCollations, getTimeZones) e Intl.DurationFormat.
mod common;


const GOLDEN: &str = include_str!("golden/intl_extra_bun.tsv");

#[test]
fn intl_extra_matches_bun() {
    common::run_golden_big_stack(GOLDEN, 2000, |source| common::EvalMode::RunInThisContext.evaluate(source, "", "R"));
}
