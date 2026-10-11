//! Golden de estáticos de Object e Function e de propriedades dos protótipos de built-ins contra o JavaScriptCore do
//! bun: `tests/golden/object_statics_bun.tsv` sai de `scripts/gen-object-statics-golden.js`, rodado no bun 1.4.2.
//! Cada linha é um programa (JSON) e o texto da variável global `R` que ele grava. Cobre Object.keys/values/entries/
//! assign/fromEntries/groupBy/hasOwn/is/setPrototypeOf/create sobre vários alvos e Proxies, ordem de chaves,
//! `Object.prototype.toString` com `Symbol.toStringTag`, `propertyIsEnumerable`, `__lookupGetter__`, `__proto__`,
//! `Function.prototype` (caller, arguments, hasInstance, bind) e nome/length/descritores de todos os métodos built-in.
mod common;

use common::json_string;
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/object_statics_bun.tsv");

/// Roda o programa e devolve o texto de `R` (`<undefined>` quando `R` não é string), ou o motivo de não ter devolvido.
fn run(source: &str) -> Result<String, String> {
    // O programa pode falhar na compilação: o bun então deixa `R` indefinido e o golden registra isso, por isso a
    // exceção do script não encerra a medição.
    match catch_unwind(AssertUnwindSafe(|| common::EvalMode::RunInThisContext.evaluate(source, "", "R"))) {
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

/// Pilha da thread do golden, igual à do golden de escopo.
const THREAD_STACK_BYTES: usize = 256 * 1024 * 1024;

#[test]
fn object_statics_and_builtin_properties_match_bun() {
    std::thread::Builder::new()
        .stack_size(THREAD_STACK_BYTES)
        .spawn(|| {
            zjsc::runtime::vm::VM::set_thread_stack_budget(THREAD_STACK_BYTES - 16 * 1024 * 1024);
            object_statics_body();
        })
        .expect("thread")
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
}

fn object_statics_body() {
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let (source, expected) = line.split_once('\t').expect("fonte e resultado");
        let (source, expected) = (json_string(source), json_string(expected));
        total += 1;
        match run(&source) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!("{}\n    esperado {expected:?}\n    veio     {actual:?}", source_tail(&source))),
            Err(reason) => failures.push(format!("{}\n    esperado {expected:?}\n    {reason}", source_tail(&source))),
        }
    }
    assert!(total >= 600, "golden com só {total} programas");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}

/// O prelúdio de funções auxiliares é longo e igual em todos os programas: a mensagem mostra só o corpo.
fn source_tail(source: &str) -> &str {
    source.rsplit_once("\n}\n").map_or(source, |(_, tail)| tail)
}
