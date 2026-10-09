//! Golden de `EventTarget`, `Event`, `CustomEvent`, `AbortController` e `AbortSignal` contra o JavaScriptCore do bun: `tests/golden/event_target_bun.tsv` sai de
//! `scripts/gen-event-target-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável
//! global `R` que ele grava. Cobre descritores, protótipos, constantes, construção, brand check, erros de argumento
//! e a lista de ouvintes (ver `src/runtime/event_target.rs` e `abort_signal.rs`; captura fica de fora). Os programas rodam
//! o laço de eventos virtual antes da leitura de `R` (os casos de `AbortSignal.timeout`). Os casos de processo (o bun
//! os roda cada um num processo próprio e lê `R`, um getter de `S(log)`, na saída) valem o mesmo: o laço do porte só
//! segue enquanto algo com `ref` vive (`State::alive`), então o timer nativo sozinho não o mantém, e `R` é lido
//! quando o laço esvazia.
mod common;

use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_named_script_running_timers_reporting_uncaught;

const GOLDEN: &str = include_str!("golden/event_target_bun.tsv");

/// Os casos com a pilha natural: fonte rodado como `/app/main.js`, stderr inteiro e código de saída (ver `common::MainScriptRow`).
const MAIN_GOLDEN: &str = include_str!("golden/event_target_main_bun.tsv");

#[test]
fn event_target_main_matches_bun() {
    common::run_with_stack(256 * 1024 * 1024, || {
        zjsc::runtime::vm::VM::set_thread_stack_budget(240 * 1024 * 1024);
        let failures: Vec<String> = MAIN_GOLDEN.lines().filter(|line| !line.is_empty()).flat_map(|line| common::MainScriptRow::parse(line).check("event-target-main")).collect();
        assert!(failures.is_empty(), "{} divergências:\n{}", failures.len(), failures.join("\n"));
    });
}

#[test]
fn event_target_matches_bun() {
    common::check(GOLDEN, common::NO_PRELUDES, 150, |source| {
        match catch_unwind(AssertUnwindSafe(|| evaluate_named_script_running_timers_reporting_uncaught(source, "event_target_case.js", "R"))) {
            Ok(Err(uncaught)) => Ok(common::Units::from(uncaught)),
            Ok(Ok(read)) => common::guarded_units(|| read),
            Err(panic) => {
                let reason = panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string())).unwrap_or_default();
                Err(format!("pânico: {reason}"))
            }
        }
    });
}
