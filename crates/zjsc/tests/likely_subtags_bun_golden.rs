//! Golden de `maximize` e `minimize` de `Intl.Locale` contra o JavaScriptCore real: `tests/golden/likely_subtags_bun.tsv`
//! sai de `scripts/gen-likely-subtags.js`, rodado no bun. Cada linha é a expressão, uma tabulação e o resultado.
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/likely_subtags_bun.tsv");

/// O invólucro do gerador de `locale_more_bun.tsv`, igual byte a byte.
fn program(expr: &str) -> String {
    format!(
        "(function () {{ try {{ var v = ({expr}); return v === undefined ? \"undefined\" : typeof v === \"string\" ? v : \
         JSON.stringify(v); }} catch (e) {{ return \"ERR \" + e.name + \": \" + e.message; }} }})()"
    )
}

/// Roda a expressão e devolve o resultado serializado, ou o motivo de não ter devolvido.
fn run(expr: &str) -> Result<String, String> {
    match catch_unwind(AssertUnwindSafe(|| evaluate_script(&program(expr)))) {
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
fn likely_subtags_match_bun() {
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let (expr, expected) = line.split_once('\t').expect("linha com a expressão e o resultado");
        total += 1;
        match run(expr) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!("{expr}\n    esperado {expected}\n    veio     {actual}")),
            Err(reason) => failures.push(format!("{expr}\n    esperado {expected}\n    {reason}")),
        }
    }
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
