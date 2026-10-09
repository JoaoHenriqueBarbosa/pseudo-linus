//! Golden de APIs ESNext contra o JavaScriptCore do bun: `tests/golden/esnext_bun.tsv` sai de
//! `scripts/gen-esnext-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, já com o prelúdio) e o texto da
//! variável global `R` que ele grava, depois de esvaziadas as microtarefas. Cobre helpers de Iterator (fontes,
//! fechamento do iterador, validação de argumentos, `Iterator.from`), métodos novos de Set com set-likes, `groupBy`,
//! `Array.fromAsync`, `Promise.withResolvers` e `Promise.try`, `Error.isError`, base64 e hex de `Uint8Array` e
//! `RegExp.escape`.
mod common;

use common::json_string;
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::{evaluate_run_in_this_context_with_caller, evaluate_script_sequence_result};
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/esnext_bun.tsv");

/// O chamador de `scripts/gen-esnext-golden.js`: o `catch` grava `R` com o erro síncrono do `runInThisContext`.
const ESNEXT_CALLER: &str = "try { require(\"node:vm\").runInThisContext(require(\"node:fs\").readFileSync(\"case_source.js\", \"utf8\"){options}) } catch (e) { globalThis.R = 'sync-throw ' + (e && e.name) + ': ' + (e && e.message) }\n";

/// Roda o programa e devolve o texto de `R` (`<undefined>` quando `R` não é definido), ou o motivo de não ter devolvido.
fn run(source: &str) -> Result<String, String> {
    // O `case.js` do gerador grava `R = 'sync-throw Nome: mensagem'` quando o `runInThisContext` lança (inclusive SyntaxError de parse).
    match catch_unwind(AssertUnwindSafe(|| evaluate_run_in_this_context_with_caller(source, None, "R", ESNEXT_CALLER))) {
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
fn esnext_apis_match_bun() {
    std::thread::Builder::new()
        .stack_size(THREAD_STACK_BYTES)
        .spawn(|| {
            zjsc::runtime::vm::VM::set_thread_stack_budget(THREAD_STACK_BYTES - 16 * 1024 * 1024);
            esnext_apis_body();
        })
        .expect("thread")
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
}

fn esnext_apis_body() {
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
    assert!(total >= 500, "golden com só {total} programas");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
