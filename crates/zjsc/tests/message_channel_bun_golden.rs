//! Golden do `MessageChannel` e do `MessagePort` contra o JavaScriptCore do bun (`scripts/gen-message-channel-golden.js`,
//! um processo `bun` por linha): a forma dos dois construtores e protótipos, os erros de chamada, o brand check,
//! `postMessage` com clone e `transfer` (porta e `ArrayBuffer`), a entrega assíncrona (ordem contra microtasks,
//! `setImmediate` e timers), a espera da mensagem por um ouvinte, `onmessage` e `addEventListener`, `close`,
//! o `MessageEvent` entregue e a porta transferida por outra porta. Cada programa fecha as suas portas num timer,
//! porque um par aberto com ouvintes segura o laço (`message_channel::holds_event_loop`).
//! Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON).
mod common;

use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_named_script_running_timers_reporting_uncaught;

const GOLDEN: &str = include_str!("golden/message_channel_bun.tsv");

fn run(source: &common::Program) -> Result<common::Units, String> {
    match catch_unwind(AssertUnwindSafe(|| evaluate_named_script_running_timers_reporting_uncaught(source, "message_channel_case.js", "R"))) {
        Ok(Err(uncaught)) => Ok(common::Units::from(uncaught)),
        Ok(Ok(read)) => common::guarded_units(|| read),
        Err(panic) => {
            let reason = panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string())).unwrap_or_default();
            Err(format!("pânico: {reason}"))
        }
    }
}

#[test]
fn message_channel_matches_bun() {
    common::check(GOLDEN, common::NO_PRELUDES, 119, run);
}
