//! `Date` ponta a ponta contra o JavaScriptCore real: os valores abaixo foram medidos no bun 1.4.2 com
//! `TZ=America/Sao_Paulo` (`toString`, `toISOString`, `Date.parse` de vários formatos, `setMonth`,
//! `Date.UTC`, `getTimezoneOffset`). O motor sozinho não tem pseudo-processo instalado e o fuso do
//! processo resolveria UTC; o `TZ` entra pelo gancho `set_time_zone_spec_override`, antes de qualquer
//! avaliação (cada `evaluate_indirect_eval` cria um `VM` novo, que resolve o fuso na primeira leitura).
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_indirect_eval;
use zjsc::runtime::process_time_zone::set_time_zone_spec_override;
use zjsc::wtf::text::conversion_mode::ConversionMode;

/// O que o bun mediu para um programa.
enum Expected {
    Text(&'static str),
    Number(f64),
    NaN,
}

/// A criação de `d` de que quase todos os casos partem.
const D: &str = "var d = new Date(Date.UTC(2024,1,29,23,5,9,7)); ";

fn cases() -> Vec<(String, Expected)> {
    use Expected::{NaN, Number, Text};
    let on_d = |expression: &str| format!("{D}{expression}");
    vec![
        (on_d("d.toString()"), Text("Thu Feb 29 2024 20:05:09 GMT-0300 (Brasilia Standard Time)")),
        (on_d("d.toDateString()"), Text("Thu Feb 29 2024")),
        (on_d("d.toTimeString()"), Text("20:05:09 GMT-0300 (Brasilia Standard Time)")),
        (on_d("d.toISOString()"), Text("2024-02-29T23:05:09.007Z")),
        (on_d("d.toJSON()"), Text("2024-02-29T23:05:09.007Z")),
        (on_d("d.toUTCString()"), Text("Thu, 29 Feb 2024 23:05:09 GMT")),
        ("String(new Date(NaN))".to_string(), Text("Invalid Date")),
        ("Date.parse('2024-02-29')".to_string(), Number(1709164800000.0)),
        ("Date.parse('2024-02-29T10:00')".to_string(), Number(1709211600000.0)),
        ("Date.parse('Feb 29 2024')".to_string(), Number(1709175600000.0)),
        ("Date.parse('29 Feb 2024 10:00 GMT+0200')".to_string(), Number(1709193600000.0)),
        ("Date.parse('2024/02/29')".to_string(), Number(1709175600000.0)),
        ("Date.parse('Thu, 29 Feb 2024 23:05:09 GMT')".to_string(), Number(1709247909000.0)),
        ("Date.parse('garbage')".to_string(), NaN),
        ("Date.parse('+275760-09-13T00:00:00.000Z')".to_string(), Number(8640000000000000.0)),
        ("Date.parse('1970-01-01T00:00:00.000+01:00')".to_string(), Number(-3600000.0)),
        ("new Date(2024,0,31).setMonth(1)".to_string(), Number(1709348400000.0)),
        ("Date.UTC(99)".to_string(), Number(915148800000.0)),
        ("new Date(0).getTimezoneOffset()".to_string(), Number(180.0)),
    ]
}

/// O resultado do programa como texto de comparação, ou o motivo de não ter um.
fn run(source: &str, expected: &Expected) -> Result<(), String> {
    let outcome = catch_unwind(AssertUnwindSafe(|| evaluate_indirect_eval(source)));
    let value = match outcome {
        Ok(Ok(value)) => value,
        Ok(Err(_)) => return Err("lançou exceção".to_string()),
        Err(panic) => {
            let reason = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string()))
                .unwrap_or_default();
            return Err(format!("pânico: {reason}"));
        }
    };
    match expected {
        Expected::Text(text) => {
            if !value.is_string() {
                return Err("não devolveu string".to_string());
            }
            let bytes = value.as_js_string().value().utf8(ConversionMode::LenientConversion);
            let actual = String::from_utf8_lossy(&bytes).into_owned();
            if actual == *text { Ok(()) } else { Err(format!("veio {actual:?}")) }
        }
        Expected::Number(number) => {
            if !value.is_number() {
                return Err("não devolveu número".to_string());
            }
            let actual = value.as_number();
            if actual == *number { Ok(()) } else { Err(format!("veio {actual}")) }
        }
        Expected::NaN => {
            if value.is_number() && value.as_number().is_nan() { Ok(()) } else { Err("não devolveu NaN".to_string()) }
        }
    }
}

#[test]
fn date_matches_bun_in_sao_paulo() {
    set_time_zone_spec_override(Some("America/Sao_Paulo"));

    let mut failures = Vec::new();
    let cases = cases();
    let total = cases.len();
    for (source, expected) in &cases {
        if let Err(reason) = run(source, expected) {
            failures.push(format!("{source}\n    {reason}"));
        }
    }
    set_time_zone_spec_override(None);
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
