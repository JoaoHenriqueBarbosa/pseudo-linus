//! Golden de `Date` em oito fusos contra o JavaScriptCore real: `tests/golden/date_tz_bun.tsv` sai de
//! `scripts/gen-date-tz-golden.js`, rodado no bun 1.4.2 com `TZ` definida por subprocesso (UTC,
//! America/Sao_Paulo, America/New_York, Europe/London, Asia/Kolkata, Australia/Lord_Howe,
//! Pacific/Chatham, Asia/Tehran). Cada linha é `fuso`, fonte do programa, KIND e REPR; o serializador
//! é `tests/golden/date_bun_harness.js`, o mesmo do gerador. O fuso entra pelo gancho
//! `set_time_zone_spec_override` (por thread, então as linhas são agrupadas por fuso e rodadas na
//! mesma thread), e cada avaliação cria um `VM` novo, com realm novo, que resolve o fuso na primeira
//! leitura.
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_script;
use zjsc::runtime::process_time_zone::set_time_zone_spec_override;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/date_tz_bun.tsv");
const HARNESS: &str = include_str!("golden/date_bun_harness.js");

const ZONES: [&str; 8] = [
    "UTC",
    "America/Sao_Paulo",
    "America/New_York",
    "Europe/London",
    "Asia/Kolkata",
    "Australia/Lord_Howe",
    "Pacific/Chatham",
    "Asia/Tehran",
];

/// `JSON.stringify(source)` para o texto do programa: o que o gerador embute no harness.
fn json_quote(source: &str) -> String {
    let mut quoted = String::with_capacity(source.len() + 2);
    quoted.push('"');
    for character in source.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            control if (control as u32) < 0x20 => quoted.push_str(&format!("\\u{:04x}", control as u32)),
            other if (other as u32) > 0x7e => {
                let mut units = [0u16; 2];
                for unit in other.encode_utf16(&mut units) {
                    quoted.push_str(&format!("\\u{unit:04x}"));
                }
            }
            other => quoted.push(other),
        }
    }
    quoted.push('"');
    quoted
}

/// Roda um programa pelo harness e devolve `KIND\tREPR`, ou o motivo de não ter devolvido.
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
fn date_programs_match_bun_in_eight_time_zones() {
    let mut failures = Vec::new();
    let mut total = 0;
    for zone in ZONES {
        set_time_zone_spec_override(Some(zone));
        for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
            let mut columns = line.splitn(3, '\t');
            let line_zone = columns.next().expect("fuso");
            if line_zone != zone {
                continue;
            }
            let source = columns.next().expect("fonte");
            let expected = columns.next().expect("KIND e REPR");
            total += 1;
            match run(source) {
                Ok(actual) if actual == expected => {}
                Ok(actual) => failures.push(format!("[{zone}] {source}\n    esperado {expected}\n    veio     {actual}")),
                Err(reason) => failures.push(format!("[{zone}] {source}\n    esperado {expected}\n    {reason}")),
            }
        }
    }
    set_time_zone_spec_override(None);
    assert!(total >= 2000, "golden pequeno demais: {total} linhas");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
