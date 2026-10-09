//! Golden do `Intl.DurationFormat.prototype.formatToParts` das 38 locales contra o JavaScriptCore real:
//! `tests/golden/duration_format_bun.tsv` sai de `scripts/gen-duration-format-data.js`, rodado no bun.
//! Colunas: locale, style, índice da duração (em `duration_format_durations.json`), partes `tipo:valor:unidade`
//! separadas por U+001F. Os travessões do CLDR vêm escapados como `\u{2013}` e `\u{2014}`.
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/duration_format_bun.tsv");
const DURATIONS: &str = include_str!("golden/duration_format_durations.json");

fn unescape_dashes(text: &str) -> String {
    text.replace("\\u{2013}", "\u{2013}").replace("\\u{2014}", "\u{2014}")
}

/// O programa de uma linha: as partes de `formatToParts` no mesmo formato do tsv.
fn program(locale: &str, style: &str, index: &str) -> String {
    format!(
        "(function () {{ try {{ var durations = {DURATIONS}; \
         return new Intl.DurationFormat({locale:?}, {{ style: {style:?} }}).formatToParts(durations[{index}]) \
         .map(function (part) {{ return part.type + \":\" + part.value + \":\" + (part.unit === undefined ? \"\" : part.unit); }}) \
         .join(\"\\u001f\"); }} catch (error) {{ return \"throw\"; }} }})()"
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
fn duration_format_parts_match_bun() {
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let fields: Vec<&str> = line.split('\t').collect();
        let [locale, style, index, expected] = fields[..] else {
            panic!("linha com quatro colunas: {line}");
        };
        let expected = unescape_dashes(expected);
        total += 1;
        let label = format!("{locale} {style} #{index}");
        match run(&program(locale, style, index)) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!("{label}\n    esperado {expected:?}\n    veio     {actual:?}")),
            Err(reason) => failures.push(format!("{label}\n    esperado {expected:?}\n    {reason}")),
        }
    }
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.iter().take(60).cloned().collect::<Vec<_>>().join("\n"));
}
