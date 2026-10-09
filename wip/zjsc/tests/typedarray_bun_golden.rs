//! Golden de TypedArray ponta a ponta contra o JavaScriptCore real: construtores (comprimento, array, iterável,
//! buffer com offset e length, erros de alinhamento), from/of, métodos, conversões numéricas, índices canônicos,
//! defineProperty/freeze/seal em elementos, Reflect.ownKeys, getters de protótipo e acesso fora do limite, para
//! as 11 classes mais Float16Array. `tests/golden/typedarray_bun.tsv` sai de `scripts/gen-typedarray-golden.js`,
//! rodado no bun. Cada linha é um programa de uma linha, o `typeof` do valor de conclusão (ou `throw` com
//! `Nome: mensagem`) e a serialização determinística dele, feita pelo mesmo `tests/golden/e2e_values_harness.js`
//! que o gerador usa no bun. Cada programa roda em realm novo. Buffers redimensionáveis e detach ficam em
//! `typedarray_more_bun_golden.rs`.
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/typedarray_bun.tsv");
const HARNESS: &str = include_str!("golden/e2e_values_harness.js");

/// `JSON.stringify(source)` para fonte ASCII: o que o gerador embute no programa.
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

/// Roda um programa pelo harness (em realm novo) e devolve `KIND\tREPR`, ou o motivo de não ter devolvido.
fn run(source: &str) -> Result<String, String> {
    let program = format!("{}({})", HARNESS.trim_end(), json_quote(source));
    match catch_unwind(AssertUnwindSafe(|| evaluate_script(&program))) {
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
fn typedarray_programs_match_bun() {
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let (source, expected) = line.split_once('\t').expect("linha com fonte, KIND e REPR");
        total += 1;
        match run(source) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!("{source}\n    esperado {expected}\n    veio     {actual}")),
            Err(reason) => failures.push(format!("{source}\n    esperado {expected}\n    {reason}")),
        }
    }
    assert!(total >= 600, "golden pequeno demais: {total}");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
