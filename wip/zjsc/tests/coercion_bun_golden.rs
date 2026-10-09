//! Golden de coerção e operadores contra o JavaScriptCore do bun: `tests/golden/coercion_bun.tsv` sai de
//! `scripts/gen-coercion-golden.js`, rodado no bun 1.4.2. Cada linha é uma expressão (sobre os 60 valores de
//! `tests/golden/coercion_prelude.js`) e o resultado serializado por `ser` (tipo e valor, `-0` e `NaN` distintos) ou
//! `error Nome: mensagem`. O teste põe o prelúdio na frente de cada programa e lê a variável global `R`.
use std::panic::{catch_unwind, AssertUnwindSafe};

mod common;

use common::json_string;
use zjsc::runtime::process_time_zone::set_time_zone_spec_override;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/coercion_bun.tsv");
const PRELUDE: &str = include_str!("golden/coercion_prelude.js");

/// Roda a expressão dentro do prelúdio e devolve o texto de `R`, ou o motivo de não ter devolvido.
fn run(expression: &str) -> Result<String, String> {
    let source = format!(
        "{PRELUDE}\nvar R; try {{ R = ser({expression}); }} catch (e) {{ R = 'error ' + (e && e.name) + ': ' + (e && e.message); }}"
    );
    match catch_unwind(AssertUnwindSafe(|| common::EvalMode::IndirectEval.evaluate(&source, "coercion_case.js", "R"))) {
        Ok(Ok(value)) if value.is_undefined() => Ok("<undefined>".to_string()),
        Ok(Ok(value)) => {
            let bytes = value.to_wtf_string().utf8(ConversionMode::LenientConversion);
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        }
        Ok(Err(_)) => Err("o programa lançou exceção".to_string()),
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
fn coercion_and_operators_match_bun() {
    // O golden saiu de um bun em America/Sao_Paulo; fixar o fuso torna o resultado independente da máquina do teste.
    set_time_zone_spec_override(Some("America/Sao_Paulo"));
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let (expression, expected) = line.split_once('\t').expect("expressão e resultado");
        let expected = json_string(expected);
        total += 1;
        match run(expression) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!("{expression}\n    esperado {expected:?}\n    veio     {actual:?}")),
            Err(reason) => failures.push(format!("{expression}\n    esperado {expected:?}\n    {reason}")),
        }
    }
    assert!(total >= 11000, "golden com só {total} expressões");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
