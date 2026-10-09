//! Golden de fluxo de controle contra o JavaScriptCore real: `tests/golden/control_flow_bun.tsv` sai de
//! `scripts/gen-control-flow-golden.js`, rodado no bun 1.4.2. Cobre try/finally com return/break/continue,
//! switch, labels, destructuring, spread, tagged templates, optional chaining, getters e setters, closures,
//! iteradores, geradores, async/await, async geradores e for-await. Cada linha é um programa que registra
//! eventos no array global `log` (com os auxiliares de `tests/golden/async_bun_harness.js`, o mesmo texto que o
//! gerador usa) e o JSON do log depois de esvaziar as microtarefas, ou `error`, `name`, `message` (JSON) se o
//! programa lançou de forma síncrona. Cada programa roda num realm novo.
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_named_script_result;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/control_flow_bun.tsv");
const HARNESS: &str = include_str!("golden/async_bun_harness.js");

/// `JSON.stringify(source)`: o que o gerador embute no programa.
fn json_quote(source: &str) -> String {
    let mut quoted = String::with_capacity(source.len() + 2);
    quoted.push('"');
    for character in source.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\u{8}' => quoted.push_str("\\b"),
            '\u{c}' => quoted.push_str("\\f"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            control if (control as u32) < 0x20 => quoted.push_str(&format!("\\u{:04x}", control as u32)),
            other => quoted.push(other),
        }
    }
    quoted.push('"');
    quoted
}

/// Roda um programa no harness, esvazia as microtarefas e devolve o resultado de `__final()`.
fn run(source: &str) -> Result<String, String> {
    let program = format!("{}\n__run({});", HARNESS.trim_end(), json_quote(source));
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        evaluate_named_script_result(&program, "control_flow_bun_golden.js", "__final()")
    }));
    match outcome {
        Ok(Ok(value)) if value.is_string() => {
            let bytes = value.as_js_string().value().utf8(ConversionMode::LenientConversion);
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        }
        Ok(Ok(_)) => Err("__final não devolveu string".to_string()),
        Ok(Err(_)) => Err("o harness lançou exceção".to_string()),
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
fn control_flow_programs_match_bun() {
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let (source, expected) = line.split_once('\t').expect("linha com fonte e resultado");
        total += 1;
        match run(source) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!("{source}\n    esperado {expected}\n    veio     {actual}")),
            Err(reason) => failures.push(format!("{source}\n    esperado {expected}\n    {reason}")),
        }
    }
    assert!(total >= 700, "o golden tem só {total} programas");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
