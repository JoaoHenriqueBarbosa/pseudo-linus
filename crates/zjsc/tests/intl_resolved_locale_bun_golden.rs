//! Golden do `resolvedOptions().locale` das nove classes do `Intl` contra o bun:
//! `tests/golden/resolved_locale_bun.tsv` sai de `scripts/gen-resolved-locale-golden.js`.
//! Cada linha é um programa de uma linha e o resultado em JSON.
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/resolved_locale_bun.tsv");

/// Roda o programa e devolve `JSON.stringify` do valor, ou o motivo de não ter devolvido.
fn run(source: &str) -> Result<String, String> {
    let program = format!("JSON.stringify({source})");
    match catch_unwind(AssertUnwindSafe(|| evaluate_script(&program))) {
        Ok(Ok(value)) if value.is_string() => {
            let bytes = value.as_js_string().value().utf8(ConversionMode::LenientConversion);
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        }
        Ok(Ok(_)) => Err("o programa não devolveu string".to_string()),
        Ok(Err(_)) => Ok("throw".to_string()),
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
fn resolved_locale_of_every_intl_class_matches_bun() {
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let (source, expected) = line.split_once('\t').expect("linha com programa e resultado");
        total += 1;
        match run(source) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!("{source}\n    esperado {expected}\n    veio     {actual}")),
            Err(reason) => failures.push(format!("{source}\n    {reason}")),
        }
    }
    assert!(total >= 180, "o golden devia ter 180 linhas, tem {total}");
    assert!(failures.is_empty(), "{} de {total} divergem do bun:\n{}", failures.len(), failures.join("\n"));
}
