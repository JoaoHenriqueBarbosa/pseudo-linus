//! Golden de borda de `Intl.DateTimeFormat` contra o JavaScriptCore do bun: `tests/golden/datetime_edge_bun.tsv` sai de
//! `scripts/gen-datetime-edge-golden.js`. Cada linha é uma expressão com a tag da seção em comentário (`/*hc*/` e
//! assim por diante) que devolve string, inclusive `throw Nome: mensagem` quando a opção é inválida, e o resultado
//! medido. Seções: hourCycle contra hour12, fractionalSecondDigits, timeZoneName, calendários, numberingSystem,
//! dayPeriod, eras a.C., datas extremas, resolvedOptions por locale, supportedLocalesOf e erros de opção.
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_indirect_eval;
use zjsc::runtime::process_time_zone::set_time_zone_spec_override;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/datetime_edge_bun.tsv");

/// Roda a expressão e devolve a string, ou o motivo de não ter devolvido.
fn run(source: &str) -> Result<String, String> {
    set_time_zone_spec_override(Some("UTC"));
    match catch_unwind(AssertUnwindSafe(|| evaluate_indirect_eval(source))) {
        Ok(Ok(value)) if value.is_string() => {
            let bytes = value.as_js_string().value().utf8(ConversionMode::LenientConversion);
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        }
        Ok(Ok(_)) => Err("o programa não devolveu string".to_string()),
        Ok(Err(_)) => Err("o programa lançou fora do try".to_string()),
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

/// Todas as linhas do golden cuja fonte começa com a tag `/*tag*/`.
fn check(tag: &str) {
    let needle = format!("/*{tag}*/");
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let (source, expected) = line.split_once('\t').expect("linha com fonte e resultado");
        if !source.starts_with(&needle) {
            continue;
        }
        total += 1;
        match run(source) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!("{source}\n    esperado {expected}\n    veio     {actual}")),
            Err(reason) => failures.push(format!("{source}\n    esperado {expected}\n    {reason}")),
        }
    }
    assert!(total > 0, "nenhuma linha da seção {tag} no golden");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}

#[test]
fn hour_cycle_against_hour12_matches_bun() {
    check("hc");
}

#[test]
fn fractional_second_digits_match_bun() {
    check("frac");
}

#[test]
fn time_zone_name_styles_match_bun() {
    check("tzname");
}

#[test]
fn calendars_format_to_parts_match_bun() {
    check("cal");
}

#[test]
fn numbering_systems_match_bun() {
    check("nu");
}

#[test]
fn day_period_matches_bun() {
    check("dp");
}

#[test]
fn bc_eras_match_bun() {
    check("bc");
}

#[test]
fn extreme_dates_match_bun() {
    check("ext");
}

#[test]
fn resolved_options_per_locale_match_bun() {
    check("resolved");
}

#[test]
fn supported_locales_of_matches_bun() {
    check("supported");
}

#[test]
fn invalid_option_errors_match_bun() {
    check("err");
}
