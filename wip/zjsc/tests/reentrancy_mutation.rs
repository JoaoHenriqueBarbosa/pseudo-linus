//! Reentrância: callbacks, comparadores, `valueOf`/`toString`, getters e traps que mutam a coleção que
//! está sendo percorrida. `tests/golden/reentrancy_bun.tsv` sai de `scripts/gen-reentrancy-golden.js`
//! (rodado no bun); cada linha é `id<TAB>programa<TAB>resultado serializado`. O harness em
//! `tests/golden/reentrancy_bun_harness.js` é o mesmo do gerador. Cada programa roda num realm novo
//! e nenhum pode causar pânico (um `RefCell` já emprestado, índice fora do `Vec`).
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::{describe_exception, evaluate_script};
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/reentrancy_bun.tsv");
const HARNESS: &str = include_str!("golden/reentrancy_bun_harness.js");

/// `JSON.stringify(source)`: o que o gerador passa ao harness.
fn json_quote(source: &str) -> String {
    let mut quoted = String::with_capacity(source.len() + 2);
    quoted.push('"');
    for character in source.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            control if (control as u32) < 0x20 => quoted.push_str(&format!("\\u{:04x}", control as u32)),
            other => quoted.push(other),
        }
    }
    quoted.push('"');
    quoted
}

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
fn reentrancy_matches_bun_without_panics() {
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let mut columns = line.splitn(3, '\t');
        let (id, source, expected) = match (columns.next(), columns.next(), columns.next()) {
            (Some(id), Some(source), Some(expected)) => (id, source, expected),
            _ => panic!("linha malformada no golden: {line}"),
        };
        total += 1;
        match run(source) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!("{id}: esperado `{expected}`, veio `{actual}`")),
            Err(reason) => failures.push(format!("{id}: {reason}")),
        }
    }
    assert!(total >= 80, "golden com poucos programas: {total}");
    assert!(failures.is_empty(), "{} de {total} divergem:\n{}", failures.len(), failures.join("\n"));
}
