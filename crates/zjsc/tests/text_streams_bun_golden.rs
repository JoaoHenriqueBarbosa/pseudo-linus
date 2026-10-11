//! Golden de `TextEncoderStream` e `TextDecoderStream` contra o JavaScriptCore do bun: `tests/golden/text_streams_bun.tsv`
//! sai de `scripts/gen-text-streams-golden.js` rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável
//! global `R` que ele grava; o corredor esvazia as microtasks e o laço de eventos virtual.
mod common;

use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_named_script_running_timers_reporting_uncaught;

const GOLDEN: &str = include_str!("golden/text_streams_bun.tsv");

#[test]
fn text_streams_match_bun() {
    let expected = GOLDEN.lines().count();
    common::check(GOLDEN, common::NO_PRELUDES, expected, |source| {
        match catch_unwind(AssertUnwindSafe(|| evaluate_named_script_running_timers_reporting_uncaught(source, "text_streams_case.js", "R"))) {
            Ok(Err(uncaught)) => Ok(common::Units::from(uncaught)),
            Ok(Ok(read)) => common::guarded_units(|| read),
            Err(panic) => {
                let reason = panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string())).unwrap_or_default();
                Err(format!("pânico: {reason}"))
            }
        }
    });
}
