//! Golden de texto sensível a locale contra o JavaScriptCore real: `tests/golden/text_locale_bun.tsv` sai de
//! `scripts/gen-text-locale-golden.js`, rodado no bun. Cada linha é um programa de uma linha (ASCII) e o
//! resultado em JSON ASCII: `Intl.Segmenter` (grapheme, word, sentence), `localeCompare` e `Intl.Collator`,
//! `toLocaleUpperCase`/`toLocaleLowerCase`, `normalize`, `resolvedOptions` e `supportedLocalesOf`.
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_indirect_eval;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/text_locale_bun.tsv");

/// O que o gerador faz com o resultado: tudo acima de `~` sai como `\uXXXX`.
fn ascii_escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    let mut units = [0u16; 2];
    for character in text.chars() {
        if (' '..='~').contains(&character) {
            escaped.push(character);
        } else {
            for unit in character.encode_utf16(&mut units) {
                escaped.push_str(&format!("\\u{:04x}", unit));
            }
        }
    }
    escaped
}

/// Roda o programa e devolve `JSON.stringify` do valor, ou o motivo de não ter devolvido.
fn run(source: &str) -> Result<String, String> {
    let program = format!("JSON.stringify({source})");
    match catch_unwind(AssertUnwindSafe(|| evaluate_indirect_eval(&program))) {
        Ok(Ok(value)) if value.is_string() => {
            let bytes = value.as_js_string().value().utf8(ConversionMode::LenientConversion);
            Ok(ascii_escape(&String::from_utf8_lossy(&bytes)))
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

#[test]
fn text_locale_programs_match_bun() {
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let (source, expected) = line.split_once('\t').expect("linha com programa e resultado");
        total += 1;
        match run(source) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!("{source}\n    esperado {expected}\n    veio     {actual}")),
            Err(reason) => failures.push(format!("{source}\n    esperado {expected}\n    {reason}")),
        }
    }
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
