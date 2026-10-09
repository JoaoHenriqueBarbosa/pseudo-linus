//! Golden de `reportError` e `structuredClone` contra o JavaScriptCore do bun: `tests/golden/structured_clone_bun.tsv`
//! sai de `scripts/gen-structured-clone-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da
//! variável global `R` que ele grava. Cobre descritor, `length`, `name`, `toString()`, a ordem de chaves entre os
//! globais do host, primitivos, `Object` e `Array` com ciclos e referências repetidas, protótipo perdido, getters
//! executados, não enumeráveis e símbolos perdidos, função e `Symbol` lançando `DataCloneError` (`name`, `message`,
//! `code`; a classe `DOMException` existe no porte, ver `src/runtime/js_dom_exception.rs`), as chaves próprias do
//! `DataCloneError` lançado por nativo (`line`, `column`, `stack`) e argumentos/opções.
//!
//! Tudo que o bun clona o porte clona (Date, invólucros
//! Number/String/Boolean/BigInt, `ArrayBuffer`, `SharedArrayBuffer`, typed arrays, `DataView`, `transfer`, RegExp, Map,
//! Set e Error com subclasses preservadas por nome).
mod common;

use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_named_script_reporting_uncaught;

const GOLDEN: &str = include_str!("golden/structured_clone_bun.tsv");

#[test]
fn structured_clone_and_report_error_match_bun() {
    common::check(GOLDEN, common::NO_PRELUDES, 214, |source| {
        match catch_unwind(AssertUnwindSafe(|| evaluate_named_script_reporting_uncaught(source, "structured_clone_case.js", "R"))) {
            Ok(Err(uncaught)) => Ok(common::Units::from(uncaught)),
            Ok(Ok(read)) => common::guarded_units(|| read),
            Err(panic) => {
                let reason = panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string())).unwrap_or_default();
                Err(format!("pânico: {reason}"))
            }
        }
    });
}
