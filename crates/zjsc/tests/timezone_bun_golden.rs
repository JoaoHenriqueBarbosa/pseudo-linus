//! Golden de fusos IANA (Intl.DateTimeFormat, Date local, Temporal.ZonedDateTime) contra o JavaScriptCore do bun:
//! `tests/golden/timezone_bun.tsv` sai de `scripts/gen-timezone-golden.js`, rodado no bun 1.4.2 com `TZ` por
//! subprocesso (UTC e America/Sao_Paulo). Cada linha é `fuso`, fonte do programa (literal JSON) e o texto da
//! variável global `R` (literal JSON). O fuso local entra pelo gancho `set_time_zone_spec_override`.
mod common;

use common::json_string;
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_named_script_result;
use zjsc::runtime::process_time_zone::set_time_zone_spec_override;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/timezone_bun.tsv");

/// Roda o programa e devolve o texto de `R` (`<undefined>` quando `R` não é string), ou o motivo de não ter devolvido.
fn run(source: &str) -> Result<String, String> {
    match catch_unwind(AssertUnwindSafe(|| evaluate_named_script_result(source, "timezone_case.js", "R"))) {
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
fn time_zone_programs_match_bun() {
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let mut columns = line.splitn(3, '\t');
        let zone = columns.next().expect("fuso");
        let source = json_string(columns.next().expect("fonte"));
        let expected = json_string(columns.next().expect("resultado"));
        set_time_zone_spec_override(Some(zone));
        total += 1;
        match run(&source) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!("[{zone}] {source}\n    esperado {expected:?}\n    veio     {actual:?}")),
            Err(reason) => failures.push(format!("[{zone}] {source}\n    esperado {expected:?}\n    {reason}")),
        }
    }
    set_time_zone_spec_override(None);
    assert!(
        failures.is_empty(),
        "{} de {total} programas divergem do bun:\n{}",
        failures.len(),
        failures.iter().take(40).cloned().collect::<Vec<_>>().join("\n")
    );
}
