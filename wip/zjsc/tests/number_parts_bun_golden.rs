//! Golden de `Intl.NumberFormat` (`format`, `formatToParts`, `formatRange`, `formatRangeToParts`,
//! notações, `signDisplay`, arredondamento, BigInt e string decimal) contra o JavaScriptCore real:
//! `tests/golden/number_parts_bun.tsv` sai de `scripts/gen-number-parts-golden.js`, rodado no bun. Cada
//! linha é um programa de uma linha, o `typeof` do valor (ou `throw`) e a serialização determinística
//! dele, feita pelo mesmo `tests/golden/e2e_values_harness.js` que o gerador usa. O que passa de ASCII
//! sai como `\uXXXX` nos dois lados.
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/number_parts_bun.tsv");
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

/// Troca cada caractere fora do ASCII por `\uXXXX` (unidades UTF-16), como o gerador.
fn escape_non_ascii(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        if character.is_ascii() {
            escaped.push(character);
        } else {
            let mut units = [0u16; 2];
            for unit in character.encode_utf16(&mut units) {
                escaped.push_str(&format!("\\u{unit:04x}"));
            }
        }
    }
    escaped
}

/// Roda um programa pelo harness e devolve `KIND\tREPR`, ou o motivo de não ter devolvido.
fn run(source: &str) -> Result<String, String> {
    let program = format!("{}({})", HARNESS.trim_end(), json_quote(source));
    match catch_unwind(AssertUnwindSafe(|| evaluate_script(&program))) {
        Ok(Ok(value)) if value.is_string() => {
            let bytes = value.as_js_string().value().utf8(ConversionMode::LenientConversion);
            Ok(escape_non_ascii(&String::from_utf8_lossy(&bytes)))
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
fn number_format_parts_match_bun() {
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
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
