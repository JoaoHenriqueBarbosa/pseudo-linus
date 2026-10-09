//! Golden de literais de objeto contra o JavaScriptCore do bun: `tests/golden/object_literal_bun.tsv` sai de
//! `scripts/gen-object-literal-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o texto
//! da variável global `R` que ele grava. Cobre a ordem de chaves (inteiras, strings, símbolos, computed, numéricas como
//! "01", "-0", "1e3", "4294967295"), chaves repetidas com data/getter/setter/método/spread misturados, `__proto__` em
//! todas as formas (com a SyntaxError exata das duplicatas), spread com getters e proxies, métodos com `super` e home
//! object, nomes de funções em chaves computadas e símbolos, getters e setters com nomes numéricos, e JSON contra literal.
mod common;

use common::json_string;
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/object_literal_bun.tsv");

/// Roda o programa e devolve o texto de `R` (`<undefined>` quando `R` não é string), ou o motivo de não ter devolvido.
fn run(source: &str) -> Result<String, String> {
    match catch_unwind(AssertUnwindSafe(|| common::EvalMode::IndirectEval.evaluate(source, "object_literal_case.js", "R"))) {
        Ok(Ok(value)) if value.is_undefined() => Ok("<undefined>".to_string()),
        Ok(Ok(value)) => {
            let bytes = value.to_wtf_string().utf8(ConversionMode::LenientConversion);
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        }
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
fn object_literal_matches_bun() {
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let (source, expected) = line.split_once('\t').expect("fonte e resultado");
        let (source, expected) = (json_string(source), json_string(expected));
        total += 1;
        match run(&source) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!("{source}\n    esperado {expected:?}\n    veio     {actual:?}")),
            Err(reason) => failures.push(format!("{source}\n    esperado {expected:?}\n    {reason}")),
        }
    }
    assert!(total >= 3000, "golden com só {total} programas");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
