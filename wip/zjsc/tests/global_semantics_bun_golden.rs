//! Golden da semântica de globais contra o JavaScriptCore do bun: `tests/golden/global_semantics_bun.tsv` sai de
//! `scripts/gen-global-semantics-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) com um ou mais
//! scripts separados por `SEP`, o resultado `<erros separados por |>#<texto de globalThis.R>`;
//! todos conferidos: os que olham o próprio objeto global foram medidos num `vm.createContext({})`, o global puro do
//! JavaScriptCore, sem process, Bun, fetch e demais extras do host. Cobre var/function/let/const/class no topo de
//! script, descritores, redeclaração entre scripts, `delete`, atribuição a não declarada, TDZ, getters e setters no
//! global, `undefined`/`NaN`/`Infinity`, `defineProperty` sobre o global, `this`, `with(globalThis)`, Annex B e eval
//! indireto. Os scripts rodam em sequência num mesmo realm novo.
mod common;

use common::json_string;
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_script_sequence_result;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/global_semantics_bun.tsv");
const SEP: &str = "\n/*--script--*/\n";

/// Roda os scripts e devolve `<erros>#<R>` (`<undefined>` quando `R` não é string), ou o motivo de não ter devolvido.
fn run(source: &str) -> Result<String, String> {
    let scripts: Vec<&str> = source.split(SEP).collect();
    match catch_unwind(AssertUnwindSafe(|| evaluate_script_sequence_result(&scripts, "eval_case.js", "globalThis.R"))) {
        Ok((errors, Ok(value))) => {
            let text = if value.is_undefined() {
                "<undefined>".to_string()
            } else {
                String::from_utf8_lossy(&value.to_wtf_string().utf8(ConversionMode::LenientConversion)).into_owned()
            };
            Ok(format!("{}#{text}", errors.join("|")))
        }
        // O gerador do golden registra `R: <nome do erro>` quando a leitura de `globalThis.R` lança (um
        // `let globalThis` de um script anterior sombreia o global).
        Ok((errors, Err(exception))) => {
            let description = zjsc::api::eval::describe_exception(&exception);
            let name = description.split(':').next().unwrap_or_default();
            Ok(format!("{}#R: {name}", errors.join("|")))
        }
        Err(panic) => {
            let reason = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string()))
                .unwrap_or_default();
            Err(format!("pânico: {reason}"))
        }
    }
}

#[test]
fn global_semantics_match_bun() {
    common::run_with_stack(256 * 1024 * 1024, global_semantics_body);
}

fn global_semantics_body() {
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let mut columns = line.split('\t');
        let (source, expected) = (columns.next().expect("fonte"), columns.next().expect("resultado"));
        let (source, expected) = (json_string(source), json_string(expected));
        total += 1;
        match run(&source) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!("{source}\n    esperado {expected:?}\n    veio     {actual:?}")),
            Err(reason) => failures.push(format!("{source}\n    esperado {expected:?}\n    {reason}")),
        }
    }
    assert!(total >= 1200, "golden com só {total} programas");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
