//! Golden de `Intl.RelativeTimeFormat`, `Intl.ListFormat` e `Intl.PluralRules` em 20 locales fora dos cobertos por
//! `reltime_bun.tsv` (ca sk sl lt lv et sq af ga gl eu is br fy si ne bn ta ml ur) contra o JavaScriptCore do bun:
//! `tests/golden/reltime_more_bun.tsv` sai de `scripts/gen-reltime-more-golden.js`, rodado no bun 1.4.2. Cada linha é
//! uma expressão que devolve string e o resultado medido (`throw` quando lançou).
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_indirect_eval;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/reltime_more_bun.tsv");

/// Roda a expressão e devolve a string, `throw` se lançou, ou o motivo de não ter devolvido.
fn run(source: &str) -> Result<String, String> {
    match catch_unwind(AssertUnwindSafe(|| evaluate_indirect_eval(source))) {
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

/// Todas as linhas do golden cujo fonte contém `needle` (a classe medida).
fn check(needle: &str) {
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let (source, escaped) = line.split_once('\t').expect("linha com fonte e resultado");
        if !source.contains(needle) {
            continue;
        }
        let expected = escaped.replace("\\t", "\t").replace("\\n", "\n").replace("\\r", "\r");
        total += 1;
        match run(source) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!("{source}\n    esperado {expected}\n    veio     {actual}")),
            Err(reason) => failures.push(format!("{source}\n    esperado {expected}\n    {reason}")),
        }
    }
    assert!(total > 0, "nenhuma linha de {needle} no golden");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}

#[test]
fn relative_time_format_more_locales_matches_bun() {
    check("Intl.RelativeTimeFormat(");
}

#[test]
fn list_format_more_locales_matches_bun() {
    check("Intl.ListFormat(");
}

#[test]
fn plural_rules_more_locales_matches_bun() {
    check("Intl.PluralRules(");
}
