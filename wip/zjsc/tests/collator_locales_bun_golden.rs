//! Golden do `Intl.Collator` para et, lv, is, vi, az, mt, fr-CA e el contra o JavaScriptCore real:
//! `tests/golden/collator_locales_bun.tsv` sai de `scripts/gen-collator-locales-golden.js`, rodado no bun.
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_indirect_eval;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/collator_locales_bun.tsv");

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
        Err(_) => Err("pânico".to_string()),
    }
}

#[test]
fn collator_locales_match_bun() {
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
    assert!(failures.is_empty(), "{} de {total} divergem do bun:\n{}", failures.len(), failures.join("\n"));
}
