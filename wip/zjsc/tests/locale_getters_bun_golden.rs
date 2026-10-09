//! Golden dos sete getters de `Intl.Locale` (`getCalendars`, `getCollations`, `getHourCycles`,
//! `getNumberingSystems`, `getTimeZones`, `getTextInfo`, `getWeekInfo`) contra o JavaScriptCore real:
//! `tests/golden/locale_getters_bun.tsv` sai de `scripts/gen-locale-data.js`, rodado no bun. Cada linha é a
//! tag e os sete resultados em JSON, separados por tabulação (`undefined` quando o getter devolve isso).
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/locale_getters_bun.tsv");

/// O programa que mede os sete getters da tag, na mesma serialização do gerador.
fn program(tag: &str) -> String {
    format!(
        "(function (tag) {{ var locale = new Intl.Locale(tag); \
         return [\"getCalendars\", \"getCollations\", \"getHourCycles\", \"getNumberingSystems\", \"getTimeZones\", \
         \"getTextInfo\", \"getWeekInfo\"].map(function (getter) {{ var value = locale[getter](); \
         return value === undefined ? \"undefined\" : JSON.stringify(value); }}).join(\"\\t\"); }})(\"{tag}\")"
    )
}

/// Roda a tag e devolve os sete resultados unidos por tabulação, ou o motivo de não ter devolvido.
fn run(tag: &str) -> Result<String, String> {
    match catch_unwind(AssertUnwindSafe(|| evaluate_script(&program(tag)))) {
        Ok(Ok(value)) if value.is_string() => {
            let bytes = value.as_js_string().value().utf8(ConversionMode::LenientConversion);
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        }
        Ok(Ok(_)) => Err("o programa não devolveu string".to_string()),
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
fn locale_getters_match_bun() {
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let (tag, expected) = line.split_once('\t').expect("linha com a tag e os sete getters");
        total += 1;
        match run(tag) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!("{tag}\n    esperado {expected}\n    veio     {actual}")),
            Err(reason) => failures.push(format!("{tag}\n    esperado {expected}\n    {reason}")),
        }
    }
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
