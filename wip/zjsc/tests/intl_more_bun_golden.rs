//! Golden de `Intl.ListFormat`, `Intl.PluralRules`, `Intl.RelativeTimeFormat` e `Intl.Collator` em 38
//! locales contra o JavaScriptCore real: `tests/golden/intl_more_bun.tsv` sai de
//! `scripts/gen-intl-more-golden.js`, rodado no bun. Cada linha é uma expressão que devolve string e o
//! resultado medido (`throw` quando lançou).
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/intl_more_bun.tsv");

/// Roda a expressão e devolve a string, `throw` se lançou, ou o motivo de não ter devolvido.
fn run(source: &str) -> Result<String, String> {
    match catch_unwind(AssertUnwindSafe(|| evaluate_script(source))) {
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

/// Todas as linhas do golden cujo fonte começa com `prefix` (a classe medida).
fn check(prefix: &str) {
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let (source, expected) = line.split_once('\t').expect("linha com fonte e resultado");
        if !source.contains(prefix) {
            continue;
        }
        total += 1;
        match run(source) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!("{source}\n    esperado {expected}\n    veio     {actual}")),
            Err(reason) => failures.push(format!("{source}\n    esperado {expected}\n    {reason}")),
        }
    }
    assert!(total > 0, "nenhuma linha de {prefix} no golden");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}

#[test]
fn list_format_more_locales_match_bun() {
    check("Intl.ListFormat(");
}

#[test]
fn plural_rules_more_locales_match_bun() {
    check("Intl.PluralRules(");
}

#[test]
fn relative_time_format_more_locales_match_bun() {
    check("Intl.RelativeTimeFormat(");
}

#[test]
fn collator_more_locales_match_bun() {
    check("Intl.Collator(");
}
