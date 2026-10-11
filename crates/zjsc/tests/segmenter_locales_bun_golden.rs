//! Golden do `Intl.Segmenter` nos locales pt, de, ko e hi contra o JavaScriptCore real:
//! `tests/golden/segmenter_locales_bun.tsv` sai de `scripts/gen-segmenter-locales-golden.js`, rodado no bun.
//! Mesmo formato de `segmenter_bun_golden.rs`: granularidade, locale, texto (literal JS em ASCII) e o
//! resultado em JSON ASCII (segmentos `[segment, index, isWordLike]` e `containing(i)` de 0 a `length`).
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/segmenter_locales_bun.tsv");

/// O mesmo cálculo do gerador, com o texto e as opções embutidos como literais.
fn program(granularity: &str, locale: &str, text_literal: &str) -> String {
    format!(
        r#"(function () {{
  var text = {text_literal};
  var segments = new Intl.Segmenter("{locale}", {{ granularity: "{granularity}" }}).segment(text);
  var parts = [];
  for (var segment of segments) parts.push([segment.segment, segment.index, segment.isWordLike]);
  var containing = [];
  for (var i = 0; i <= text.length; i++) {{
    var found = segments.containing(i);
    containing.push(found === undefined ? -1 : found.index);
  }}
  return JSON.stringify({{ parts: parts, containing: containing }}).replace(/[^\x20-\x7e]/g, function (c) {{
    return "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0");
  }});
}})()"#
    )
}

/// Roda um programa e devolve a string resultante, ou o motivo de não ter devolvido.
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
fn segmenter_locales_match_bun() {
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let mut columns = line.splitn(4, '\t');
        let (granularity, locale, text, expected) = (
            columns.next().expect("granularidade"),
            columns.next().expect("locale"),
            columns.next().expect("texto"),
            columns.next().expect("resultado"),
        );
        total += 1;
        let label = format!("{granularity} {locale} {text}");
        match run(&program(granularity, locale, text)) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!("{label}\n    esperado {expected}\n    veio     {actual}")),
            Err(reason) => failures.push(format!("{label}\n    esperado {expected}\n    {reason}")),
        }
    }
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
