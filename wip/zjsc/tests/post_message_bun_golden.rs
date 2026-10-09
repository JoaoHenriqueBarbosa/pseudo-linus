//! Golden de `postMessage` contra o JavaScriptCore do bun: `tests/golden/post_message_bun.tsv` sai de
//! `scripts/gen-post-message-golden.js`, rodado no bun 1.4.2 (um processo por linha, porque várias linhas mudam o
//! global). Cada linha é um programa (JSON) e o texto da variável global `R` que ele grava. Cobre o descritor, a
//! forma da função, `new`, argumentos de qualquer tipo (sem clonar, sem tocar neles), `this` alheio, o retorno
//! `undefined`, a ordem de chaves, atribuição, `delete` e redefinição (ver `src/runtime/post_message.rs`). O caso de
//! exceção lançada pelo `onmessage` compara o relato completo (stderr e código de saída) em `post_message_uncaught_bun.tsv`.
mod common;

use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_named_script_reporting_uncaught;

const GOLDEN: &str = include_str!("golden/post_message_bun.tsv");
/// Os casos em que a exceção escapa para o laço de eventos: fonte rodado como `/app/main.js`, stderr inteiro e código de
/// saída, no formato de `uncaught_bun.tsv` (ver `common::MainScriptRow`).
const UNCAUGHT_GOLDEN: &str = include_str!("golden/post_message_uncaught_bun.tsv");

#[test]
fn post_message_uncaught_matches_bun() {
    common::run_with_stack(256 * 1024 * 1024, || {
        zjsc::runtime::vm::VM::set_thread_stack_budget(240 * 1024 * 1024);
        let failures: Vec<String> = UNCAUGHT_GOLDEN.lines().filter(|line| !line.is_empty()).flat_map(|line| common::MainScriptRow::parse(line).check("post-message-uncaught")).collect();
        assert!(failures.is_empty(), "{} divergências:\n{}", failures.len(), failures.join("\n"));
    });
}

#[test]
fn post_message_match_bun() {
    common::check(GOLDEN, common::NO_PRELUDES, 150, |source| {
        match catch_unwind(AssertUnwindSafe(|| evaluate_named_script_reporting_uncaught(source, "post_message_case.js", "R"))) {
            Ok(Err(uncaught)) => Ok(common::Units::from(uncaught)),
            Ok(Ok(read)) => common::guarded_units(|| read),
            Err(panic) => {
                let reason = panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string())).unwrap_or_default();
                Err(format!("pânico: {reason}"))
            }
        }
    });
}
