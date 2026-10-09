//! Golden dos descritores das propriedades dos builtins contra o JavaScriptCore do bun: `tests/golden/builtin_descriptor_bun.tsv`
//! sai de `scripts/gen-builtin-descriptor-golden.js`, rodado no bun 1.4.2 com `vm.runInThisContext`. Cada linha é um
//! programa (JSON) e o texto da variável global `R` que ele grava. Os programas cobrem, por construtor, protótipo,
//! namespace e intrínseco: todos os descritores (`Reflect.ownKeys`, data vs accessor, writable/enumerable/configurable,
//! `length`/`name` das funções, getter/setter), protótipo e `Symbol.toStringTag`, contagem de atributos e a forma de
//! `length`/`name`/`prototype`/`constructor` dos construtores. Objeto ausente no porte aparece como `absent` e conta
//! como divergência: o golden registra o que o bun tem.
mod common;

use common::json_string;
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_script_sequence_result;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/builtin_descriptor_bun.tsv");

/// Roda o programa e devolve o texto de `R` (`<undefined>` quando `R` não é string), ou o motivo de não ter devolvido.
fn run(source: &str) -> Result<String, String> {
    match catch_unwind(AssertUnwindSafe(|| evaluate_script_sequence_result(&[source], "builtin_descriptor_case.js", "globalThis.R").1)) {
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

/// Pilha da thread do golden, igual à dos outros golden de programa: o orçamento padrão do VM (1 MiB) é curto.
const THREAD_STACK_BYTES: usize = 256 * 1024 * 1024;

#[test]
fn builtin_descriptors_match_bun() {
    std::thread::Builder::new()
        .stack_size(THREAD_STACK_BYTES)
        .spawn(|| {
            // Folga de 16 MiB para os protetores nativos.
            zjsc::runtime::vm::VM::set_thread_stack_budget(THREAD_STACK_BYTES - 16 * 1024 * 1024);
            builtin_descriptors_body();
        })
        .expect("thread")
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
}

fn builtin_descriptors_body() {
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let (source, expected) = line.split_once('\t').expect("fonte e resultado");
        let (source, expected) = (json_string(source), json_string(expected));
        total += 1;
        // O objeto do programa fica no fim da fonte (`var o = ...`); o rótulo da falha é esse trecho.
        let label = source.find("var o = ").map(|at| source[at..].chars().take(110).collect::<String>()).unwrap_or_default();
        match run(&source) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!("{label}\n    esperado {expected:?}\n    veio     {actual:?}")),
            Err(reason) => failures.push(format!("{label}\n    esperado {expected:?}\n    {reason}")),
        }
    }
    assert!(total >= 400, "golden com só {total} programas");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
