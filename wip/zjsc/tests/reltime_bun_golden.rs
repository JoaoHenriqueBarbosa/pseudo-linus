//! Golden do `Intl.RelativeTimeFormat` de es, fr, de, it, ja, ru e ar contra o JavaScriptCore real:
//! `tests/golden/reltime_bun.tsv` sai de `scripts/gen-reltime-golden.js`, rodado no bun. Cada linha é
//! `língua, estilo, numeric, unidade, valor, texto esperado` separados por tabulação.
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/reltime_bun.tsv");

/// Formata no motor e devolve o texto, ou o motivo de não ter devolvido.
fn run(lang: &str, style: &str, numeric: &str, unit: &str, value: &str) -> Result<String, String> {
    let program = format!(
        "new Intl.RelativeTimeFormat(\"{lang}\", {{ style: \"{style}\", numeric: \"{numeric}\" }}).format({value}, \"{unit}\")"
    );
    match catch_unwind(AssertUnwindSafe(|| evaluate_script(&program))) {
        Ok(Ok(result)) if result.is_string() => {
            let bytes = result.as_js_string().value().utf8(ConversionMode::LenientConversion);
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
fn relative_time_format_matches_bun() {
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let fields: Vec<&str> = line.split('\t').collect();
        assert_eq!(fields.len(), 6, "linha com seis campos: {line}");
        let (lang, style, numeric, unit, value, expected) =
            (fields[0], fields[1], fields[2], fields[3], fields[4], fields[5]);
        total += 1;
        match run(lang, style, numeric, unit, value) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!(
                "{lang} {style} {numeric} {unit} {value}\n    esperado {expected:?}\n    veio     {actual:?}"
            )),
            Err(reason) => failures.push(format!("{lang} {style} {numeric} {unit} {value}\n    {reason}")),
        }
    }
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
