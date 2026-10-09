//! Golden do núcleo de `performance` (`Performance`, `PerformanceEntry`, `PerformanceMark`,
//! `PerformanceMeasure`) contra o JavaScriptCore do bun: `tests/golden/performance_bun.tsv` sai de
//! `scripts/gen-performance-golden.js`, rodado no bun 1.4.2 (um processo por linha, porque o buffer de marcas é
//! estado do programa). Cada linha é um programa (JSON) e o texto da variável global `R` que ele grava. Cobre o
//! descritor dos globais, a forma dos protótipos e construtores, `now()` e `timeOrigin` por tipo e relação (nunca
//! pelo número), `mark`, `measure`, `getEntries*`, `clearMarks`, `clearMeasures`, `toJSON`, os erros exatos e o
//! `this` inválido, `timing`/`PerformanceTiming`, `onresourcetimingbufferfull` e os métodos de resource timing.
//! O laço de eventos virtual esvazia antes da leitura de `R` (os casos de `PerformanceObserver` entregam o callback
//! numa tarefa do host). O que o porte ainda não tem (`EventTarget`, o símbolo de inspeção) fica fora do golden.
mod common;

use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_named_script_running_timers_reporting_uncaught;

const GOLDEN: &str = include_str!("golden/performance_bun.tsv");

#[test]
fn performance_matches_bun() {
    common::check(GOLDEN, common::NO_PRELUDES, 150, |source| {
        match catch_unwind(AssertUnwindSafe(|| evaluate_named_script_running_timers_reporting_uncaught(source, "performance_case.js", "R"))) {
            Ok(Err(uncaught)) => Ok(common::Units::from(uncaught)),
            Ok(Ok(read)) => common::guarded_units(|| read),
            Err(panic) => {
                let reason = panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string())).unwrap_or_default();
                Err(format!("pânico: {reason}"))
            }
        }
    });
}
