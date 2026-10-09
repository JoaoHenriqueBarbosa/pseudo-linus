//! Casos do species das 12 typed arrays depois de mutações (`defineProperty` e `delete` de `Symbol.species`,
//! getter em `%TypedArray%`, `setPrototypeOf` nas construtoras, troca de `prototype.constructor`, species que
//! devolve outra classe, não construtor, typed array menor ou de outro tipo de conteúdo), `slice`/`map`/`filter`/
//! `subarray` contra `toReversed`/`toSorted`/`with`, e os species equivalentes de ArrayBuffer, Array, Promise e
//! RegExp. `tests/golden/typed_array_species_bun.tsv` (fonte, `\t`, resultado) sai de
//! `scripts/gen-typed-array-species-golden.js`, rodado no bun 1.4.2 com `node:vm` `runInThisContext`; cada fonte
//! empilha em `out` e o resultado é `out.join('|')` depois de esvaziar as microtarefas.
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_named_script_result;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/typed_array_species_bun.tsv");

/// Mesmo texto de `HARNESS` em `scripts/gen-typed-array-species-golden.js`.
const HARNESS: &str = r#"globalThis.out = [];
globalThis.t = function (f) { try { var r = f(); return ArrayBuffer.isView(r) ? r[Symbol.toStringTag] + '[' + Array.prototype.join.call(r) + ']' : Array.isArray(r) ? 'A[' + r.join() + ']' : String(r); } catch (e) { return e.name + ': ' + e.message; } };
globalThis.__final = function () { return out.join('|'); };
globalThis.__run = function (source) {
  try { (0, eval)('(function () { ' + source + ' })()'); } catch (e) { out.push('ERR ' + e.name + ': ' + e.message); }
};"#;

/// `JSON.stringify(source)`: o que o prelúdio recebe em `__run`.
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

/// Roda um caso num realm novo, esvazia as microtarefas e devolve `out.join('|')`.
fn run(source: &str) -> Result<String, String> {
    let program = format!("{}\n__run({});", HARNESS, json_quote(source));
    let outcome = catch_unwind(AssertUnwindSafe(|| evaluate_named_script_result(&program, "typed_array_species_bun_golden.js", "__final()")));
    match outcome {
        Ok(Ok(value)) if value.is_string() => {
            let bytes = value.as_js_string().value().utf8(ConversionMode::LenientConversion);
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        }
        Ok(Ok(_)) => Err("o resultado não é string".to_string()),
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

/// Pilha da thread do golden, igual à dos outros goldens.
const THREAD_STACK_BYTES: usize = 256 * 1024 * 1024;

#[test]
fn typed_array_species_programs_match_bun() {
    std::thread::Builder::new()
        .stack_size(THREAD_STACK_BYTES)
        .spawn(|| {
            zjsc::runtime::vm::VM::set_thread_stack_budget(THREAD_STACK_BYTES - 16 * 1024 * 1024);
            typed_array_species_body();
        })
        .expect("thread")
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
}

fn typed_array_species_body() {
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
    assert!(total >= 80, "o golden tem só {total} programas");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
