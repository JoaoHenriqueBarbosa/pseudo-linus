//! Golden de instruções e sintaxe contra o JavaScriptCore do bun: `tests/golden/statements_bun.tsv` sai de
//! `scripts/gen-statements-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável
//! global `R` que ele grava. Cobre labels, switch, try/finally, for-in/of, optional chaining, atribuição lógica,
//! `**`, templates, destructuring, spread, literais, números, ASI, escapes Unicode, identificadores contextuais,
//! comentários HTML, hashbang e erros de sintaxe antecipados. Resultado `TOP:` no golden significa que o script
//! inteiro lançou (SyntaxError no topo, tipicamente): o programa tem de lançar também, com o mesmo `nome: mensagem`
//! (`evaluate_named_script_reporting_uncaught` devolve `Uncaught nome: mensagem`, que vira `TOP:nome: mensagem`).
mod common;

use common::json_string;
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_named_script_reporting_uncaught;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/statements_bun.tsv");

/// Roda o programa e devolve o texto de `R` (`<undefined>` quando `R` não é string), ou o motivo de não ter devolvido.
fn run(source: &str) -> Result<String, String> {
    match catch_unwind(AssertUnwindSafe(|| evaluate_named_script_reporting_uncaught(source, "statements_case.js", "R"))) {
        Ok(Ok(Ok(value))) if value.is_undefined() => Ok("<undefined>".to_string()),
        Ok(Ok(Ok(value))) => {
            let bytes = value.to_wtf_string().utf8(ConversionMode::LenientConversion);
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        }
        Ok(Ok(Err(_))) => Err("a leitura de R lançou exceção".to_string()),
        Ok(Err(uncaught)) => Ok(format!("TOP:{}", uncaught.strip_prefix("Uncaught ").unwrap_or(&uncaught))),
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
fn statements_match_bun() {
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let (source, expected) = line.split_once('\t').expect("fonte e resultado");
        let (source, expected) = (json_string(source), json_string(expected));
        total += 1;
        let outcome = run(&source);
        match outcome {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!("{source}\n    esperado {expected:?}\n    veio     {actual:?}")),
            Err(reason) => failures.push(format!("{source}\n    esperado {expected:?}\n    {reason}")),
        }
    }
    assert!(total >= 1500, "golden com só {total} programas");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
