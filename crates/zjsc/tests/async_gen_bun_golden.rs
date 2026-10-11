//! Golden de async functions, async generators, generators e `for await` contra o JavaScriptCore real:
//! `tests/golden/async_gen_bun.tsv` sai de `scripts/gen-async-gen-golden.js`, rodado no bun 1.4.2 com
//! `require('node:vm').runInThisContext(src)` (nunca como arquivo, para o transpilador do bun não tocar na fonte).
//! Cobre return, throw, `yield*`, ordem de microtarefas, await em `finally`, iteradores async de iteráveis sync,
//! erros em `next` e `return` e a reentrância "already running". Os programas não usam API de host: só os
//! auxiliares `L`, `tick`, `thenable`, `ok` e `bad` do prelúdio abaixo (o mesmo texto que o gerador embute).
//! Cada linha é um programa e o JSON do log depois de esvaziar as microtarefas, ou `error`, `name`, `message`
//! (JSON) se a fonte lançou de forma síncrona. Cada programa roda num realm novo.
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_named_script_result;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/async_gen_bun.tsv");

/// Mesmo texto de `HARNESS` em `scripts/gen-async-gen-golden.js`.
const HARNESS: &str = r#"globalThis.log = [];
globalThis.L = function (x) { log.push(x); };
globalThis.tick = function (n, label) {
  var p = Promise.resolve();
  for (var i = 0; i < n; i++) p = p.then(function () {});
  return p.then(function () { L(label); });
};
globalThis.thenable = function (v, label) {
  return { then: function (res) { L("then:" + label); res(v); } };
};
globalThis.ok = function (v) { L("v:" + JSON.stringify(v)); };
globalThis.bad = function (e) { L("e:" + (e && e.name) + ":" + (e && e.message)); };
globalThis.__err = null;
globalThis.__final = function () { return __err !== null ? __err : JSON.stringify(log); };
globalThis.__run = function (src) {
  try { (0, eval)(src); } catch (e) { __err = "error\t" + e.name + "\t" + JSON.stringify(String(e.message)); }
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

/// Roda um programa no prelúdio, esvazia as microtarefas e devolve o resultado de `__final()`.
fn run(source: &str) -> Result<String, String> {
    let program = format!("{}\n__run({});", HARNESS, json_quote(source));
    let outcome =
        catch_unwind(AssertUnwindSafe(|| evaluate_named_script_result(&program, "async_gen_bun_golden.js", "__final()")));
    match outcome {
        Ok(Ok(value)) if value.is_string() => {
            let bytes = value.as_js_string().value().utf8(ConversionMode::LenientConversion);
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        }
        Ok(Ok(_)) => Err("__final não devolveu string".to_string()),
        Ok(Err(_)) => Err("o prelúdio lançou exceção".to_string()),
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
fn async_generator_programs_match_bun() {
    std::thread::Builder::new()
        .stack_size(THREAD_STACK_BYTES)
        .spawn(|| {
            zjsc::runtime::vm::VM::set_thread_stack_budget(THREAD_STACK_BYTES - 16 * 1024 * 1024);
            async_generator_body();
        })
        .expect("thread")
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
}

fn async_generator_body() {
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
    assert!(total >= 400, "o golden tem só {total} programas");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
