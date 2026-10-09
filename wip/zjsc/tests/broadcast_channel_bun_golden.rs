//! Golden do `BroadcastChannel` contra o JavaScriptCore do bun. Dois arquivos, ambos de programas (JSON) com o texto da
//! variável global `R` que eles gravam:
//! - `tests/golden/broadcast_channel_bun.tsv` (`scripts/gen-broadcast-channel-golden.js`): a forma do construtor e do
//!   protótipo, os descritores, os erros de chamada sem `new`, sem argumento, `this` alheio, `postMessage` de função e
//!   depois de `close`, a conversão do nome;
//! - `tests/golden/broadcast_channel_delivery_bun.tsv` (`scripts/gen-broadcast-channel-delivery-golden.js`, um processo
//!   `bun` por linha): a entrega assíncrona entre canais do mesmo nome (ordem contra microtasks, `setImmediate` e timers,
//!   nunca ao remetente, nunca a outro nome), o `MessageEvent`, `onmessage` e `addEventListener`, o clone por canal,
//!   `close` no meio e `ref`/`unref`.
//! O laço de eventos virtual esvazia antes da leitura de `R`: a entrega é uma tarefa do host (`deliver_pending`).
mod common;

use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::{evaluate_named_script_reporting_uncaught, evaluate_named_script_running_timers_reporting_uncaught};

const SHAPE_GOLDEN: &str = include_str!("golden/broadcast_channel_bun.tsv");
const UNCAUGHT_GOLDEN: &str = include_str!("golden/broadcast_channel_uncaught_bun.tsv");
const DELIVERY_GOLDEN: &str = include_str!("golden/broadcast_channel_delivery_bun.tsv");

fn run(source: &common::Program) -> Result<common::Units, String> {
    match catch_unwind(AssertUnwindSafe(|| evaluate_named_script_running_timers_reporting_uncaught(source, "broadcast_channel_case.js", "R"))) {
        Ok(Err(uncaught)) => Ok(common::Units::from(uncaught)),
        Ok(Ok(read)) => common::guarded_units(|| read),
        Err(panic) => {
            let reason = panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string())).unwrap_or_default();
            Err(format!("pânico: {reason}"))
        }
    }
}

/// A forma não roda o laço: os programas deixam canais abertos, e canal aberto com `ref` segura o laço para sempre.
fn run_without_loop(source: &common::Program) -> Result<common::Units, String> {
    match catch_unwind(AssertUnwindSafe(|| evaluate_named_script_reporting_uncaught(source, "broadcast_channel_case.js", "R"))) {
        Ok(Err(uncaught)) => Ok(common::Units::from(uncaught)),
        Ok(Ok(read)) => common::guarded_units(|| read),
        Err(panic) => {
            let reason = panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string())).unwrap_or_default();
            Err(format!("pânico: {reason}"))
        }
    }
}

#[test]
fn broadcast_channel_shape_matches_bun() {
    common::check(SHAPE_GOLDEN, common::NO_PRELUDES, 37, run_without_loop);
}

/// Handler que lança: relato completo (stderr, código de saída e stdout) do programa principal contra o bun.
#[test]
fn broadcast_channel_uncaught_matches_bun() {
    common::run_with_stack(256 * 1024 * 1024, || {
        zjsc::runtime::vm::VM::set_thread_stack_budget(240 * 1024 * 1024);
        let failures: Vec<String> = UNCAUGHT_GOLDEN.lines().filter(|line| !line.is_empty()).flat_map(|line| common::MainScriptRow::parse(line).check("broadcast-channel-uncaught")).collect();
        assert!(failures.is_empty(), "{} divergências:\n{}", failures.len(), failures.join("\n"));
    });
}

#[test]
fn broadcast_channel_delivery_matches_bun() {
    common::check(DELIVERY_GOLDEN, common::NO_PRELUDES, 58, run);
}
