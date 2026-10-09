//! Golden de herança de builtins contra o JavaScriptCore do bun: `tests/golden/subclass_edge_bun.tsv` sai de
//! `scripts/gen-subclass-edge-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável
//! global `R` que ele grava. Cobre `class extends` dos builtins, `Reflect.construct` com newTarget diferente (protótipo
//! do newTarget e fallback para o realm do newTarget), cross-realm por `ShadowRealm`, `Symbol.species`, `super` em objeto literal,
//! `Object.setPrototypeOf` em instâncias, `Error.captureStackTrace` e `cause`. Nenhum programa usa API do host.
mod common;

use common::json_string;
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_run_in_this_context_with_caller;
use zjsc::runtime::process_time_zone::set_time_zone_spec_override;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/subclass_edge_bun.tsv");

/// O chamador de `scripts/gen-subclass-edge-golden.js`: `vm` vira global antes do `try` (segunda linha do `case.js`).
const SUBCLASS_CALLER: &str = "globalThis.vm = require(\"node:vm\");\ntry { vm.runInThisContext(require(\"node:fs\").readFileSync(\"case_source.js\", \"utf8\"){options}) } catch (e) {}\n";

/// Roda o programa e devolve o texto de `R` (`<undefined>` quando `R` não é string), ou o motivo de não ter devolvido.
fn run(source: &str) -> Result<String, String> {
    // A exceção do script não encerra a medição: o bun deixa `R` indefinido e o golden registra isso.
    match catch_unwind(AssertUnwindSafe(|| evaluate_run_in_this_context_with_caller(source, None, "R", SUBCLASS_CALLER))) {
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
fn builtin_subclassing_matches_bun() {
    // O golden saiu de um bun rodando em America/Sao_Paulo; o motor sozinho resolve UTC.
    set_time_zone_spec_override(Some("America/Sao_Paulo"));
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let (source, expected) = line.split_once('\t').expect("fonte e resultado");
        let (source, expected) = (json_string(source), json_string(expected));
        // `Date()` sem `new` devolve a hora do relógio: o esperado é a hora da geração, sem como comparar.
        if source.contains("String(Date(0))") {
            continue;
        }
        total += 1;
        match run(&source) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!("{source}\n    esperado {expected:?}\n    veio     {actual:?}")),
            Err(reason) => failures.push(format!("{source}\n    esperado {expected:?}\n    {reason}")),
        }
    }
    assert!(total >= 1200, "golden com só {total} programas");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
