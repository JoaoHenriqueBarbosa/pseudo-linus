//! Golden de limites e mensagens de RangeError/stack contra o JavaScriptCore do bun: `tests/golden/limits_range_bun.tsv`
//! sai de `scripts/gen-limits-range-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da
//! variável global `R` que ele grava (só `e.name` e `e.message` das exceções, nunca stack nem tempo). Cobre recursão
//! infinita por forma de chamada (função, método, getter, construtor, trap de Proxy, toString, JSON), JSON aninhado,
//! spread e apply enormes, `repeat`/`padStart`/`padEnd`, tamanhos de Array, ArrayBuffer, TypedArray e DataView,
//! `toFixed`/`toPrecision`/`toExponential`/`toString(radix)`, limites de BigInt, regex grande e catastrófico curto e
//! tail call. Cada programa roda numa thread de pilha grande: o orçamento de recursão do VM é fixo, então a pilha da
//! thread só precisa ser maior que ele mais a folga dos protetores nativos.
mod common;

use common::json_string;
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/limits_range_bun.tsv");
/// 1 GiB cobre o orçamento nativo de 45609 níveis em debug e release (só é reservado, as páginas são tocadas sob demanda).
const THREAD_STACK_BYTES: usize = 1024 * 1024 * 1024;

/// Roda o programa e devolve o texto de `R` (`<undefined>` quando `R` não é string), ou o motivo de não ter devolvido.
fn run(source: &str) -> Result<String, String> {
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

/// Roda o programa numa thread de pilha grande; o pânico é o único modo de falha tolerado aqui.
fn run_on_big_stack(source: String) -> Result<String, String> {
    std::thread::Builder::new()
        .stack_size(THREAD_STACK_BYTES)
        .spawn(move || {
            // Folga de 16 MiB para os protetores nativos; sem isto o orçamento seria o padrão de 1 MiB.
            zjsc::runtime::vm::VM::set_thread_stack_budget(THREAD_STACK_BYTES - 16 * 1024 * 1024);
            run(&source)
        })
        .expect("thread")
        .join()
        .unwrap_or_else(|_| Err("thread terminou em pânico".to_string()))
}

#[test]
fn limits_range_match_bun() {
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let (source, expected) = line.split_once('\t').expect("fonte e resultado");
        let (source, expected) = (json_string(source), json_string(expected));
        total += 1;
        match run_on_big_stack(source.clone()) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!("{source:.200}\n    esperado {expected:?}\n    veio     {actual:?}")),
            Err(reason) => failures.push(format!("{source:.200}\n    esperado {expected:?}\n    {reason}")),
        }
    }
    assert!(total >= 1000, "golden com só {total} programas");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
