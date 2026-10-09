//! Golden de instruções numéricas de WebAssembly (i32, i64, f32, f64, conversões, SIMD v128, memória) contra o
//! JavaScriptCore real: `tests/golden/wasm_numeric_bun.tsv` sai de `scripts/gen-wasm-numeric-golden.js`, rodado no bun. Os
//! módulos são montados de wat com wasm-tools na hora de gerar e embutidos no fonte de cada programa em hexadecimal
//! (`HX("...")`). Cada linha é um programa que registra eventos no array global `log` (auxiliares de
//! `tests/golden/wasm_js_bun_harness.js`, `wasm_exceptions_bun_extra.js`, `wasm_gc_bun_extra.js` e
//! `wasm_numeric_bun_extra.js`) e o JSON do log, ou `error`, `name`, `message` (JSON) se o programa lançou de forma
//! síncrona. Cada programa roda num realm novo.
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_named_script_result;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/wasm_numeric_bun.tsv");
const HARNESS: &str = include_str!("golden/wasm_js_bun_harness.js");
const EXCEPTIONS_EXTRA: &str = include_str!("golden/wasm_exceptions_bun_extra.js");
const GC_EXTRA: &str = include_str!("golden/wasm_gc_bun_extra.js");
const EXTRA: &str = include_str!("golden/wasm_numeric_bun_extra.js");

/// `JSON.stringify(source)`: o que o gerador embute no programa.
fn json_quote(source: &str) -> String {
    let mut quoted = String::with_capacity(source.len() + 2);
    quoted.push('"');
    for character in source.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\u{8}' => quoted.push_str("\\b"),
            '\u{c}' => quoted.push_str("\\f"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            control if (control as u32) < 0x20 => quoted.push_str(&format!("\\u{:04x}", control as u32)),
            other => quoted.push(other),
        }
    }
    quoted.push('"');
    quoted
}

/// Roda um programa no harness, esvazia as microtarefas e devolve o resultado de `__final()`.
fn run(source: &str) -> Result<String, String> {
    let program = format!(
        "{}\n{}\n{}\n{}\n__run({});",
        HARNESS.trim_end(),
        EXCEPTIONS_EXTRA.trim_end(),
        GC_EXTRA.trim_end(),
        EXTRA.trim_end(),
        json_quote(source)
    );
    let outcome = catch_unwind(AssertUnwindSafe(|| evaluate_named_script_result(&program, "wasm_numeric_bun_golden.js", "__final()")));
    match outcome {
        Ok(Ok(value)) if value.is_string() => {
            let bytes = value.as_js_string().value().utf8(ConversionMode::LenientConversion);
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        }
        Ok(Ok(_)) => Err("__final não devolveu string".to_string()),
        Ok(Err(_)) => Err("o harness lançou exceção".to_string()),
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
fn wasm_numeric_programs_match_bun() {
    std::thread::Builder::new()
        .stack_size(THREAD_STACK_BYTES)
        .spawn(|| {
            zjsc::runtime::vm::VM::set_thread_stack_budget(THREAD_STACK_BYTES - 16 * 1024 * 1024);
            wasm_numeric_body();
        })
        .expect("thread")
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
}

fn wasm_numeric_body() {
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let (source, expected) = line.split_once('\t').expect("linha com fonte e resultado");
        total += 1;
        match run(source) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!("{source}\n    esperado {expected}\n    veio     {actual}")),
            Err(reason) => failures.push(format!("{source}\n    esperado {expected}\n    {reason}")),
        }
    }
    assert!(total >= 500, "o golden tem só {total} programas");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
