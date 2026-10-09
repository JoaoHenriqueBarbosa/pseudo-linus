//! Golden dos construtores `WebAssembly.Memory`, `Table` e `Global` contra o JavaScriptCore real (bun 1.4.2):
//! `tests/golden/wasm_ctor_bun.tsv` reúne os casos que o golden amplo (`wasm_api_bun.tsv`) não cobre: a opção
//! `address` (Memory64 desligado), memória `shared`, `grow` e destacamento do buffer, |this| inválido, `Table.grow`
//! acima do máximo e conversões de `Global`. Mesmo harness e mesmo formato de `wasm_api_bun_golden.rs`.
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_named_script_result;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/wasm_ctor_bun.tsv");
const HARNESS: &str = include_str!("golden/wasm_api_bun_harness.js");

/// `JSON.stringify(source)`: o que o gerador embute no programa.
fn json_quote(source: &str) -> String {
    let mut quoted = String::with_capacity(source.len() + 2);
    quoted.push('"');
    for character in source.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\n' => quoted.push_str("\\n"),
            control if (control as u32) < 0x20 => quoted.push_str(&format!("\\u{:04x}", control as u32)),
            other => quoted.push(other),
        }
    }
    quoted.push('"');
    quoted
}

/// Roda um programa no harness, esvazia as microtarefas e devolve o resultado de `__final()`.
fn run(source: &str) -> Result<String, String> {
    let program = format!("{}\n__run({});", HARNESS.trim_end(), json_quote(source));
    let outcome =
        catch_unwind(AssertUnwindSafe(|| evaluate_named_script_result(&program, "wasm_ctor_bun_golden.js", "__final()")));
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

/// Pilha da thread do golden, igual à do golden da API.
const THREAD_STACK_BYTES: usize = 256 * 1024 * 1024;

#[test]
fn wasm_ctor_programs_match_bun() {
    std::thread::Builder::new()
        .stack_size(THREAD_STACK_BYTES)
        .spawn(|| {
            zjsc::runtime::vm::VM::set_thread_stack_budget(THREAD_STACK_BYTES - 16 * 1024 * 1024);
            wasm_ctor_body();
        })
        .expect("thread")
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
}

fn wasm_ctor_body() {
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
    assert!(total >= 17, "o golden tem só {total} programas");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
