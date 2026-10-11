//! Golden de `atob` e `btoa` contra o JavaScriptCore do bun: `tests/golden/base64_globals_bun.tsv` sai de
//! `scripts/gen-base64-globals-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da
//! variável global `R` que ele grava. Cobre descritor, `length`, `name`, `toString()`, entradas válidas e
//! inválidas, espaço, preenchimento, coerções de argumento e o erro de caractere inválido (`name`, `message`,
//! `code`; a classe `DOMException` não existe no porte, ver `src/runtime/base64_globals.rs`).
mod common;

use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_named_script_reporting_uncaught;

const GOLDEN: &str = include_str!("golden/base64_globals_bun.tsv");

#[test]
fn base64_globals_match_bun() {
    common::check(GOLDEN, common::NO_PRELUDES, 150, |source| {
        match catch_unwind(AssertUnwindSafe(|| evaluate_named_script_reporting_uncaught(source, "base64_globals_case.js", "R"))) {
            Ok(Err(uncaught)) => Ok(common::Units::from(uncaught)),
            Ok(Ok(read)) => common::guarded_units(|| read),
            Err(panic) => {
                let reason = panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string())).unwrap_or_default();
                Err(format!("pânico: {reason}"))
            }
        }
    });
}
