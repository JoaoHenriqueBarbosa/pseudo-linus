//! Golden de `CountQueuingStrategy` e `ByteLengthQueuingStrategy` contra o JavaScriptCore do bun:
//! `tests/golden/queuing_strategy_bun.tsv` sai de `scripts/gen-queuing-strategy-golden.js`, rodado no bun 1.4.2.
//! Cada linha é um programa (JSON) e o texto da variável global `R` que ele grava. Cobre descritor do global,
//! `length`, `name`, chaves do construtor e do protótipo, os acessores, a função `size` compartilhada, o
//! dicionário `QueuingStrategyInit` e os erros (ver `src/runtime/queuing_strategy.rs`).
mod common;

use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_named_script_reporting_uncaught;

const GOLDEN: &str = include_str!("golden/queuing_strategy_bun.tsv");

#[test]
fn queuing_strategy_matches_bun() {
    common::check(GOLDEN, common::NO_PRELUDES, 210, |source| {
        match catch_unwind(AssertUnwindSafe(|| evaluate_named_script_reporting_uncaught(source, "queuing_strategy_case.js", "R"))) {
            Ok(Err(uncaught)) => Ok(common::Units::from(uncaught)),
            Ok(Ok(read)) => common::guarded_units(|| read),
            Err(panic) => {
                let reason = panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string())).unwrap_or_default();
                Err(format!("pânico: {reason}"))
            }
        }
    });
}
