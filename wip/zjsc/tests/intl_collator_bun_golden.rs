//! Golden de Intl.Collator, Intl.Segmenter, Intl.getCanonicalLocales, Intl.supportedValuesOf e Intl.Locale
//! (maximize, minimize, getters de calendário e hourCycle) contra o JavaScriptCore do bun:
//! `tests/golden/intl_collator_bun.tsv` sai de `scripts/gen-intl-collator-golden.js`, rodado no bun 1.4.2. Cada linha é
//! uma expressão e o resultado medido já convertido por `String(...)` (`throw` quando lançou). O teste embrulha a
//! expressão em `String(...)` para igualar a medição.
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/intl_collator_bun.tsv");

/// Roda a expressão e devolve a string, `throw` se lançou, ou o motivo de não ter devolvido.
fn run(source: &str) -> Result<String, String> {
    let wrapped = format!("String({source})");
    match catch_unwind(AssertUnwindSafe(|| evaluate_script(&wrapped))) {
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

/// Todas as linhas do golden cujo fonte contém `needle` (a classe medida).
fn check(needle: &str) {
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let (source, expected) = line.split_once('\t').expect("linha com fonte e resultado");
        if !source.contains(needle) {
            continue;
        }
        total += 1;
        match run(source) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!("{source}\n    esperado {expected}\n    veio     {actual}")),
            Err(reason) => failures.push(format!("{source}\n    esperado {expected}\n    {reason}")),
        }
    }
    assert!(total > 0, "nenhuma linha de {needle} no golden");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}

#[test]
fn collator_matches_bun() {
    check("Intl.Collator");
}

#[test]
fn locale_compare_sort_matches_bun() {
    check("localeCompare");
}

#[test]
fn segmenter_matches_bun() {
    check("Intl.Segmenter");
}

#[test]
fn get_canonical_locales_matches_bun() {
    check("Intl.getCanonicalLocales");
}

#[test]
fn supported_values_of_matches_bun() {
    check("Intl.supportedValuesOf");
}

#[test]
fn locale_matches_bun() {
    check("Intl.Locale");
}
