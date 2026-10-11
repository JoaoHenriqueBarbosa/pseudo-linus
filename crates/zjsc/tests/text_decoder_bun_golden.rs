//! Golden de `TextDecoder` contra o JavaScriptCore do bun: `tests/golden/text_decoder_bun.tsv` sai de
//! `scripts/gen-text-decoder-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da
//! variável global `R` que ele grava. Cobre descritor, forma do construtor e do protótipo, rótulos, opções,
//! `decode` com `stream` em sequência partida, BOM com e sem `ignoreBOM`, `fatal`, UTF-8, UTF-16LE/BE e
//! windows-1252, `this` inválido e entradas de tipos variados (ver `src/runtime/text_decoder.rs`).
mod common;

use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_named_script_reporting_uncaught;

const GOLDEN: &str = include_str!("golden/text_decoder_bun.tsv");

#[test]
fn text_decoder_matches_bun() {
    common::check(GOLDEN, common::NO_PRELUDES, 150, |source| {
        match catch_unwind(AssertUnwindSafe(|| evaluate_named_script_reporting_uncaught(source, "text_decoder_case.js", "R"))) {
            Ok(Err(uncaught)) => Ok(common::Units::from(uncaught)),
            Ok(Ok(read)) => common::guarded_units(|| read),
            Err(panic) => {
                let reason = panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string())).unwrap_or_default();
                Err(format!("pânico: {reason}"))
            }
        }
    });
}
