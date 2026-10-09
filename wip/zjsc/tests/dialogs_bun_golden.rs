//! Golden de `alert`, `confirm` e `prompt` contra o JavaScriptCore do bun: `tests/golden/dialogs_bun.tsv` sai de
//! `scripts/gen-dialogs-golden.js`, rodado no bun 1.4.2 com stdin em EOF (um processo por linha, porque várias linhas
//! mudam o global). Cada linha é um programa (JSON) e o texto da variável global `R` que ele grava. Cobre o
//! descritor, a forma da função, `new`, o retorno (`undefined`, `false`, `null`), a conversão dos argumentos, a ordem
//! de chaves, atribuição, `delete` e redefinição (ver `src/runtime/dialogs.rs`).
mod common;

use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_named_script_reporting_uncaught;

const GOLDEN: &str = include_str!("golden/dialogs_bun.tsv");

#[test]
fn dialogs_match_bun() {
    common::check(GOLDEN, common::NO_PRELUDES, 150, |source| {
        match catch_unwind(AssertUnwindSafe(|| evaluate_named_script_reporting_uncaught(source, "dialogs_case.js", "R"))) {
            Ok(Err(uncaught)) => Ok(common::Units::from(uncaught)),
            Ok(Ok(read)) => common::guarded_units(|| read),
            Err(panic) => {
                let reason = panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string())).unwrap_or_default();
                Err(format!("pânico: {reason}"))
            }
        }
    });
}
