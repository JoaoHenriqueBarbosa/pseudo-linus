//! `Error.stackTraceLimit` (`ErrorConstructor::put`/`deleteProperty` e a captura em `Error.cpp`) e a
//! captura de `stack` do `AggregateError` e do `SuppressedError`. As expectativas vêm do C++:
//! `m_stackTraceLimit` é um `std::optional<unsigned>` que só `put` e `delete` de `Error.stackTraceLimit`
//! mudam (`defineProperty` não); sem valor o erro não guarda pilha, e `captureStackTrace` usa `value_or(0)`.
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_named_script_result;
use zjsc::wtf::text::conversion_mode::ConversionMode;

/// Roda o programa e devolve o texto da global `R`.
fn run(source: &str) -> String {
    let result = catch_unwind(AssertUnwindSafe(|| evaluate_named_script_result(source, "stack_case.js", "R")));
    match result {
        Ok(Ok(value)) if value.is_undefined() => "<undefined>".to_string(),
        Ok(Ok(value)) => String::from_utf8_lossy(&value.to_wtf_string().utf8(ConversionMode::LenientConversion)).into_owned(),
        Ok(Err(_)) => panic!("o programa lançou exceção: {source}"),
        Err(_) => panic!("pânico ao rodar: {source}"),
    }
}

/// `new Error('x').stack` de dentro de `f`, chamada por `g`, depois de `setup`.
fn error_stack_after(setup: &str) -> String {
    run(&format!(
        "\"use strict\";\nfunction f() {{ return String(new Error('x').stack) }}\nfunction g() {{ var r = f(); return r }}\n{setup}\nglobalThis.R = g()"
    ))
}

#[test]
fn aggregate_error_captures_stack() {
    let stack = run("\"use strict\";\nfunction f() { return new AggregateError([], 'x').stack }\nglobalThis.R = f()");
    assert!(stack.starts_with("AggregateError: x\n    at f (stack_case.js:2:"), "{stack:?}");
}

#[test]
fn aggregate_error_called_without_new_captures_stack() {
    let stack = run("\"use strict\";\nfunction f() { return AggregateError([], 'x').stack }\nglobalThis.R = f()");
    assert!(stack.starts_with("AggregateError: x\n    at f (stack_case.js:2:"), "{stack:?}");
}

#[test]
fn suppressed_error_captures_stack() {
    let stack = run("\"use strict\";\nfunction f() { return new SuppressedError(1, 2, 'm').stack }\nglobalThis.R = f()");
    assert!(stack.starts_with("SuppressedError: m\n    at f (stack_case.js:2:"), "{stack:?}");
}

#[test]
fn aggregate_and_suppressed_errors_respect_stack_trace_limit() {
    let source = "\"use strict\";\nfunction f() { return String(new AggregateError([], 'x').stack) + '|' + String(new SuppressedError(1, 2, 'm').stack) }\nError.stackTraceLimit = 0;\nglobalThis.R = f()";
    assert_eq!(run(source), "undefined|undefined");
}

#[test]
fn put_of_a_number_sets_the_limit_truncated() {
    let stack = error_stack_after("Error.stackTraceLimit = 1.9;");
    assert!(stack.contains("at f (") && !stack.contains("at g ("), "{stack:?}");
}

#[test]
fn put_of_infinity_means_no_limit() {
    let stack = error_stack_after("Error.stackTraceLimit = Infinity;");
    assert!(stack.contains("at f (") && stack.contains("at g ("), "{stack:?}");
}

#[test]
fn put_of_a_negative_number_means_zero_frames() {
    assert_eq!(error_stack_after("Error.stackTraceLimit = -5;"), "undefined");
}

#[test]
fn put_of_a_non_number_clears_the_limit_and_the_error_has_no_stack() {
    assert_eq!(error_stack_after("Error.stackTraceLimit = 'a';"), "undefined");
    assert_eq!(error_stack_after("Error.stackTraceLimit = undefined;"), "undefined");
    assert_eq!(error_stack_after("Error.stackTraceLimit = null;"), "undefined");
}

#[test]
fn delete_clears_the_limit() {
    assert_eq!(error_stack_after("delete Error.stackTraceLimit;"), "undefined");
}

#[test]
fn define_property_does_not_update_the_limit() {
    // Só `put` e `deleteProperty` espelham no global: o `defineProperty` deixa o limite anterior (o padrão).
    let stack = error_stack_after("Object.defineProperty(Error, 'stackTraceLimit', { value: 0 });");
    assert!(stack.contains("at f (") && stack.contains("at g ("), "{stack:?}");
}

#[test]
fn capture_stack_trace_without_a_limit_keeps_the_frames() {
    // Medido no bun 1.4.2: `captureStackTrace` com o limite limpo ainda captura os frames (`indexOf` dá 6), ao contrário
    // do `new Error`, que fica sem `stack`.
    let source = "\"use strict\";\nfunction f() { var o = {}; Error.stackTraceLimit = 'a'; Error.captureStackTrace(o); return typeof o.stack + '|' + o.stack.indexOf('    at ') }\nglobalThis.R = f()";
    assert_eq!(run(source), "string|6");
}
