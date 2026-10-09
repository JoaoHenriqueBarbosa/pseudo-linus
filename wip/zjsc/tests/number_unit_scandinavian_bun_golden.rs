//! Golden da unidade `mile-scandinavian` do `Intl.NumberFormat` em `en` (a 45ª unidade sancionada) e do tamanho de
//! `Intl.supportedValuesOf("unit")`, contra o bun 1.4.2: `tests/golden/number_unit_scandinavian_bun.tsv`.
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_indirect_eval;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/number_unit_scandinavian_bun.tsv");

fn run(source: &str) -> Result<String, String> {
    match catch_unwind(AssertUnwindSafe(|| evaluate_indirect_eval(source))) {
        Ok(Ok(value)) if value.is_string() => {
            let bytes = value.as_js_string().value().utf8(ConversionMode::LenientConversion);
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        }
        Ok(Ok(_)) => Err("o programa não devolveu string".to_string()),
        Ok(Err(_)) => Ok("throw".to_string()),
        Err(_) => Err("pânico".to_string()),
    }
}

#[test]
fn mile_scandinavian_matches_bun() {
    let mut failures = Vec::new();
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let (source, expected) = line.split_once('\t').expect("linha com fonte e resultado");
        match run(source) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!("{source}\n    esperado {expected}\n    veio     {actual}")),
            Err(reason) => failures.push(format!("{source}\n    esperado {expected}\n    {reason}")),
        }
    }
    assert!(failures.is_empty(), "divergem do bun:\n{}", failures.join("\n"));
}
