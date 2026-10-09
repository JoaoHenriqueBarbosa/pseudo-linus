//! Golden de WebAssembly GC e tipos de referência contra o JavaScriptCore real: `tests/golden/wasm_gc_bun.tsv` sai
//! de `scripts/gen-wasm-gc-golden.js`, rodado no bun. Os módulos são montados de wat com wasm-tools na hora de gerar
//! e embutidos no fonte de cada programa em hexadecimal (`HX("...")`). Cada linha é um programa que registra eventos no
//! array global `log` (auxiliares de `tests/golden/wasm_js_bun_harness.js`, `wasm_exceptions_bun_extra.js` e
//! `wasm_gc_bun_extra.js`) e o JSON do log, ou `error`, `name`, `message` (JSON) se o programa lançou de forma
//! síncrona. Cada programa roda num realm novo.
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_named_script_result;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/wasm_gc_bun.tsv");
const HARNESS: &str = include_str!("golden/wasm_js_bun_harness.js");
const EXCEPTIONS_EXTRA: &str = include_str!("golden/wasm_exceptions_bun_extra.js");
const EXTRA: &str = include_str!("golden/wasm_gc_bun_extra.js");

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
        "{}\n{}\n{}\n__run({});",
        HARNESS.trim_end(),
        EXCEPTIONS_EXTRA.trim_end(),
        EXTRA.trim_end(),
        json_quote(source)
    );
    let outcome = catch_unwind(AssertUnwindSafe(|| evaluate_named_script_result(&program, "wasm_gc_bun_golden.js", "__final()")));
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
fn wasm_gc_programs_match_bun() {
    std::thread::Builder::new()
        .stack_size(THREAD_STACK_BYTES)
        .spawn(|| {
            zjsc::runtime::vm::VM::set_thread_stack_budget(THREAD_STACK_BYTES - 16 * 1024 * 1024);
            wasm_gc_body();
        })
        .expect("thread")
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
}

fn wasm_gc_body() {
    let mut failures = Vec::new();
    let mut total = 0;
    // `WASM_GC_LINES=inicio-fim` (linhas do tsv, a partir de 1) mede só uma faixa, para bisseccionar lentidão;
    // com ela o piso de 250 programas não se aplica e cada caso imprime o tempo gasto em stderr.
    let range = std::env::var("WASM_GC_LINES").ok().and_then(|text| {
        let (start, end) = text.split_once('-')?;
        Some((start.parse::<usize>().ok()?, end.parse::<usize>().ok()?))
    });
    for (index, line) in GOLDEN.lines().enumerate().filter(|(_, line)| !line.is_empty()) {
        if range.is_some_and(|(start, end)| index + 1 < start || index + 1 > end) {
            continue;
        }
        let started = std::time::Instant::now();
        let (source, expected) = line.split_once('\t').expect("linha com fonte e resultado");
        total += 1;
        match run(source) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!("{source}\n    esperado {expected}\n    veio     {actual}")),
            Err(reason) => failures.push(format!("{source}\n    esperado {expected}\n    {reason}")),
        }
        if range.is_some() {
            eprintln!("linha {}: {:?}", index + 1, started.elapsed());
        }
    }
    assert!(range.is_some() || total >= 250, "o golden tem só {total} programas");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
