//! Golden de semântica da linguagem contra o JavaScriptCore real: `tests/golden/language_bun.tsv` sai de
//! `scripts/gen-language-golden.js`, rodado no bun. Cada linha é um programa de uma linha seguido do
//! resultado: `value<TAB>serialização`, `error<TAB>name<TAB>message JSON` (exceção) ou `thrown<TAB>typeof<TAB>valor`.
//!
//! A serialização é o JavaScript de `tests/golden/language_bun_harness.js`, o mesmo que o gerador usa no
//! bun: cada programa vira `harness("<fonte>")`, que faz o `(0, eval)(fonte)` indireto e devolve a linha.
//! O zjsc tem de devolver exatamente a mesma string. Cada programa roda num `evaluate_script` próprio
//! (realm novo), porque vários declaram globais e alteram protótipos. Quando o harness em si falha
//! (exceção que escapa), a mensagem sai por `describe_exception`.
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::{describe_exception, evaluate_script};
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/language_bun.tsv");
const HARNESS: &str = include_str!("golden/language_bun_harness.js");

/// `JSON.stringify(source)` com todo caractere fora do ASCII como `\uXXXX`: o fonte chega ao motor em
/// Latin-1, então o literal precisa ser ASCII puro para valer a mesma string que o bun viu.
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
            control if (control as u32) < 0x20 || (control as u32) > 0x7e => {
                let mut units = [0u16; 2];
                for unit in control.encode_utf16(&mut units) {
                    quoted.push_str(&format!("\\u{unit:04x}"));
                }
            }
            other => quoted.push(other),
        }
    }
    quoted.push('"');
    quoted
}

/// Roda um programa pelo harness e devolve a linha de resultado, ou o motivo de não ter devolvido.
fn run(source: &str) -> Result<String, String> {
    let program = format!("{}({})", HARNESS.trim_end(), json_quote(source));
    match catch_unwind(AssertUnwindSafe(|| evaluate_script(&program))) {
        Ok(Ok(value)) if value.is_string() => {
            let bytes = value.as_js_string().value().utf8(ConversionMode::LenientConversion);
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        }
        Ok(Ok(_)) => Err("o harness não devolveu string".to_string()),
        Ok(Err(exception)) => Err(format!("o harness lançou exceção: {}", describe_exception(&exception))),
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
fn language_semantics_match_bun() {
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let (source, expected) = line.split_once('\t').expect("linha com fonte e resultado");
        total += 1;
        match run(source) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!("{source}\n    esperado {expected}\n    veio     {actual}")),
            Err(reason) => failures.push(format!("{source}\n    esperado {expected}\n    {reason}")),
        }
    }
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
