//! Golden de erros de runtime contra o JavaScriptCore real: `tests/golden/errors_bun.tsv` sai de
//! `scripts/gen-errors-golden.js`, rodado no bun. Cada linha é um programa de uma linha que provoca um
//! erro, seguido de `error`, `e.name`, `e.message` (JSON), `e.constructor.name` e se `e.stack` começa
//! com o header `String(e)`, ou `thrown` (valor não-erro) ou `nothrow`.
//!
//! O texto que descreve a exceção é o JavaScript de `tests/golden/errors_bun_harness.js`, o mesmo que o
//! gerador usa no bun: cada programa vira `harness("<fonte>")`, que faz o `(0, eval)(fonte)` indireto,
//! captura a exceção e devolve a linha. O zjsc tem de devolver exatamente a mesma string. Cada programa
//! roda num `evaluate_script` próprio, porque vários congelam ou alteram protótipos e o global.
mod common;

use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_indirect_eval;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/errors_bun.tsv");
const HARNESS: &str = include_str!("golden/errors_bun_harness.js");

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

/// Roda um programa pelo harness e devolve a linha de resultado, ou o motivo de não ter devolvido.
///
/// O filho do gerador imprime a linha logo que o harness volta (`console.log`) e só depois o laço esvazia as
/// microtarefas, que ele nunca espera: um programa como `function f() { Promise.resolve().then(f); f() } f()` enche a
/// fila sem fim e o filho morre no limite de tempo com a linha já impressa. Por isso o harness roda como valor de
/// conclusão de um eval indireto, que não esvazia as microtarefas (o `evaluate_script` esvaziaria e nunca voltaria).
fn run(source: &str) -> Result<String, String> {
    let program = format!("{}({})", HARNESS.trim_end(), json_quote(source));
    match catch_unwind(AssertUnwindSafe(|| evaluate_indirect_eval(&program))) {
        Ok(Ok(value)) if value.is_string() => {
            let bytes = value.as_js_string().value().utf8(ConversionMode::LenientConversion);
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        }
        Ok(Ok(_)) => Err("o harness não devolveu string".to_string()),
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
fn runtime_errors_match_bun() {
    // O golden sai de um bun em America/Sao_Paulo; o fuso é por thread.
    zjsc::runtime::process_time_zone::set_time_zone_spec_override(Some("America/Sao_Paulo"));
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let (source, rest) = line.split_once('\t').expect("linha com fonte e resultado");
        // Coluna final opcional: os índices de token que variam no bun (o valor de `Math.random`). O resultado também
        // tem tabulações, então ela só existe quando o último campo é um array de inteiros.
        let (expected, varying) = rest
            .rsplit_once('\t')
            .filter(|(_, last)| last.starts_with('[') && last[1..].bytes().all(|byte| byte.is_ascii_digit() || byte == b',' || byte == b']'))
            .map_or((rest, Vec::new()), |(expected, varying)| (expected, common::parse_varying(varying)));
        total += 1;
        match run(source) {
            Ok(actual) if common::matches_varying(&expected.encode_utf16().collect::<Vec<_>>(), &actual.encode_utf16().collect::<Vec<_>>(), &varying) => {}
            Ok(actual) => failures.push(format!("{source}\n    esperado {expected}\n    veio     {actual}")),
            Err(reason) => failures.push(format!("{source}\n    esperado {expected}\n    {reason}")),
        }
    }
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
