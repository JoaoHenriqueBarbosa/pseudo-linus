//! Golden de `global`, `self` e `navigator` contra o JavaScriptCore do bun: `tests/golden/global_navigator_bun.tsv`
//! sai de `scripts/gen-global-navigator-golden.js`, rodado no bun 1.4.2 (um processo por linha, porque várias
//! linhas mudam o global). Cada linha é um programa (JSON) e o texto da variável global `R` que ele grava. Cobre o
//! descritor, a identidade com `globalThis`, atribuição e `delete`, o par `get`/`set` do `self`, a ordem de chaves
//! e o objeto `navigator` (chaves, `Symbol.toStringTag`, acessores, `toString`). Valores que dependem da máquina
//! entram só por tipo (ver `src/runtime/navigator.rs`).
mod common;

use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_named_script_reporting_uncaught;

const GOLDEN: &str = include_str!("golden/global_navigator_bun.tsv");

#[test]
fn global_navigator_match_bun() {
    common::check(GOLDEN, common::NO_PRELUDES, 150, |source| {
        // O gerador roda `(0, eval)("var R")` antes de cada programa, então `R` já existe como global (o programa pode gravá-lo
        // de uma função estrita sem `ReferenceError`). O `var R;` na mesma linha reproduz isso sem mexer nas posições.
        let source = format!("var R;{source}");
        match catch_unwind(AssertUnwindSafe(|| evaluate_named_script_reporting_uncaught(&source, "global_navigator_case.js", "R"))) {
            Ok(Err(uncaught)) => Ok(common::Units::from(uncaught)),
            Ok(Ok(read)) => common::guarded_units(|| read),
            Err(panic) => {
                let reason = panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string())).unwrap_or_default();
                Err(format!("pânico: {reason}"))
            }
        }
    });
}
