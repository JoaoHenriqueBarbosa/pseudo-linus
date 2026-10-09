//! Golden dos nomes longos de fuso do `Date.prototype.toString()` contra o bun 1.4.2:
//! `tests/golden/timezone_names_bun.tsv` sai de `scripts/gen-timezone-names.js` (zona, instante em ms e o
//! nome entre parênteses, em janeiro e julho de 1900, 1970, 2024 e 2100, para todas as zonas de
//! `Intl.supportedValuesOf('timeZone')` e UTC). O fuso entra pelo gancho `set_time_zone_spec_override`
//! (por thread), como em `date_tz_bun_golden.rs`; cada avaliação cria um `VM` novo.
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_script;
use zjsc::runtime::process_time_zone::set_time_zone_spec_override;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/timezone_names_bun.tsv");

/// O texto entre parênteses do `toString()` do instante, ou o motivo de não ter saído.
fn long_name(instant: &str) -> Result<String, String> {
    let program = format!("(function () {{ var m = new Date({instant}).toString().match(/\\((.*)\\)$/); return m ? m[1] : 'sem nome'; }})()");
    match catch_unwind(AssertUnwindSafe(|| evaluate_script(&program))) {
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
fn time_zone_long_names_match_bun() {
    let mut failures = Vec::new();
    let mut total = 0;
    let mut current_zone = "";
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let mut columns = line.splitn(3, '\t');
        let zone = columns.next().expect("fuso");
        let instant = columns.next().expect("instante");
        let expected = columns.next().expect("nome");
        if zone != current_zone {
            set_time_zone_spec_override(Some(zone));
            current_zone = zone;
        }
        total += 1;
        match long_name(instant) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!("[{zone}] {instant}: esperado {expected:?}, veio {actual:?}")),
            Err(reason) => failures.push(format!("[{zone}] {instant}: esperado {expected:?}, {reason}")),
        }
    }
    set_time_zone_spec_override(None);
    assert!(total >= 3000, "golden pequeno demais: {total} linhas");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
