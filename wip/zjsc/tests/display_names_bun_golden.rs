//! Golden do `Intl.DisplayNames` dos 69 locales de intl_available_locales contra o JavaScriptCore real:
//! `tests/golden/display_names_bun.tsv` sai de `scripts/gen-display-names-data.js`, rodado no bun.
//! Colunas: locale, type, style, languageDisplay, código, resultado (`undefined` se não há nome). Os
//! travessões do CLDR vêm escapados como `\u{2014}` no tsv.
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/display_names_bun.tsv");

/// Desfaz o escape dos travessões do tsv.
fn unescape_dashes(text: &str) -> String {
    text.replace("\\u{2013}", "\u{2013}").replace("\\u{2014}", "\u{2014}")
}

/// O programa de uma linha: `of(código)` com `fallback: "none"`, `undefined` ou `throw` como texto.
fn program(locale: &str, kind: &str, style: &str, language_display: &str, code: &str) -> String {
    format!(
        "(function () {{ try {{ var name = new Intl.DisplayNames([{locale:?}], {{ type: {kind:?}, style: {style:?}, \
         languageDisplay: {language_display:?}, fallback: \"none\" }}).of({code:?}); \
         return name === undefined ? \"undefined\" : name; }} catch (error) {{ return \"throw\"; }} }})()"
    )
}

fn run(source: &str) -> Result<String, String> {
    match catch_unwind(AssertUnwindSafe(|| evaluate_script(source))) {
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
fn display_names_match_bun() {
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let fields: Vec<&str> = line.split('\t').collect();
        let [locale, kind, style, language_display, code, expected] = fields[..] else {
            panic!("linha com seis colunas: {line}");
        };
        let expected = unescape_dashes(expected);
        total += 1;
        let label = format!("{locale} {kind} {style} {language_display} {code}");
        match run(&program(locale, kind, style, language_display, code)) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!("{label}\n    esperado {expected}\n    veio     {actual}")),
            Err(reason) => failures.push(format!("{label}\n    esperado {expected}\n    {reason}")),
        }
    }
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.iter().take(60).cloned().collect::<Vec<_>>().join("\n"));
}
