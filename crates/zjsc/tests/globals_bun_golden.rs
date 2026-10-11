//! Golden de funções globais e escopo contra o JavaScriptCore do bun: `tests/golden/globals_bun.tsv` sai de
//! `scripts/gen-globals-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável
//! global `R` que ele grava; programa que lança sem captura tem como resultado `Uncaught Nome: mensagem`. Cobre
//! `parseInt`/`parseFloat`/`isNaN`/`isFinite`, as funções de URI, `escape`/`unescape`, `eval` direto e indireto,
//! `new Function`, `globalThis`, TDZ, Annex B, `with`/`Symbol.unscopables`, `arguments`, rótulos, closures em
//! laço e modo strict. Ver `wip-notes/globals-audit.md`.
mod common;

use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_named_script_reporting_uncaught;

const GOLDEN: &str = include_str!("golden/globals_bun.tsv");

#[test]
fn globals_match_bun() {
    // Como o corredor do gerador: exceção sem captura vira `Uncaught Nome: mensagem` e `R` não é lido.
    common::check(GOLDEN, common::NO_PRELUDES, 600, |source| {
        match catch_unwind(AssertUnwindSafe(|| evaluate_named_script_reporting_uncaught(source, "globals_case.js", "R"))) {
            Ok(Err(uncaught)) => Ok(common::Units::from(uncaught)),
            Ok(Ok(read)) => common::guarded_units(|| read),
            Err(panic) => {
                let reason = panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string())).unwrap_or_default();
                Err(format!("pânico: {reason}"))
            }
        }
    });
}
