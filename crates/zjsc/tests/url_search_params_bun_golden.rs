//! Golden de `URLSearchParams` contra o JavaScriptCore do bun: `tests/golden/url_search_params_bun.tsv` sai de
//! `scripts/gen-url-search-params-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da
//! variável global `R` que ele grava. Sem divergência conhecida (ver `src/runtime/url_search_params.rs`).
mod common;

use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_named_script_reporting_uncaught;

const GOLDEN: &str = include_str!("golden/url_search_params_bun.tsv");

#[test]
fn url_search_params_matches_bun() {
    common::check(GOLDEN, common::NO_PRELUDES, 120, |source| {
        match catch_unwind(AssertUnwindSafe(|| evaluate_named_script_reporting_uncaught(source, "url_search_params_case.js", "R"))) {
            Ok(Err(uncaught)) => Ok(common::Units::from(uncaught)),
            Ok(Ok(read)) => common::guarded_units(|| read),
            Err(panic) => {
                let reason = panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string())).unwrap_or_default();
                Err(format!("pânico: {reason}"))
            }
        }
    });
}
