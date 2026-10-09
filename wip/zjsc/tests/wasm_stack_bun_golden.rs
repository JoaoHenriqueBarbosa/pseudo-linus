//! Golden do `e.stack` que atravessa WebAssembly, contra o bun 1.4.2. `tests/golden/wasm_stack_bun.js`
//! monta à mão um módulo (com e sem seção `name`) cuja função exportada chama outra que chama um import JS
//! que lança `new Error('x')`, e outro cujo corpo é `unreachable` (`RuntimeError`). O esperado em
//! `tests/golden/wasm_stack_bun.expected` saiu do bun rodado nesse mesmo arquivo.
//!
//! O que o bun mostra, e que o teste fixa: o frame de wasm é `at unknown` (um por função wasm da pilha, sem
//! o frame do export), mesmo com seção `name` (o nome nunca chega ao texto), e o `RuntimeError` de `trap`
//! traz o sufixo `(evaluating '...')` do `unreachable`.
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_named_script_result;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const SCRIPT: &str = include_str!("golden/wasm_stack_bun.js");
const EXPECTED: &str = include_str!("golden/wasm_stack_bun.expected");

/// Roda o script com `console.log` desviado para um array e devolve as linhas juntas.
fn run() -> Result<String, String> {
    let program = format!("var __out = [];\n{}\n", SCRIPT.replace("console.log(", "__out.push("));
    // O prelúdio ocupa uma linha a mais; o `var` entra na mesma linha 1 para não deslocar as posições.
    let program = program.replacen("var __out = [];\n", "var __out = []; ", 1);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        // O resultado é uma expressão avaliada depois de esvaziar as microtarefas: os casos JSPI escrevem em `__out`
        // numa reação de promessa, então juntar as linhas no próprio script as leria cedo demais.
        evaluate_named_script_result(&program, "wasm_stack_bun.js", "__out.join('\\n')")
    }));
    match outcome {
        Ok(Ok(value)) if value.is_string() => {
            let bytes = value.as_js_string().value().utf8(ConversionMode::LenientConversion);
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        }
        Ok(Ok(_)) => Err("o resultado não é string".to_string()),
        Ok(Err(_)) => Err("o script lançou exceção".to_string()),
        Err(_) => Err("pânico".to_string()),
    }
}

/// O texto de um caso, de `== rótulo` até o próximo.
fn section<'a>(text: &'a str, label: &str) -> &'a str {
    let start = text.find(&format!("== {label} ")).unwrap_or_else(|| panic!("caso {label} ausente"));
    let rest = &text[start..];
    let end = rest[3..].find("\n== ").map(|offset| offset + 3).unwrap_or(rest.len());
    rest[..end].trim_end_matches('\n')
}

fn check(label: &str, actual: &str) {
    assert_eq!(section(actual, label), section(EXPECTED, label), "caso {label}");
}

#[test]
fn wasm_frames_without_name_section_in_js_throw() {
    check("noname-js", &run().expect("script"));
}

#[test]
fn wasm_frames_with_name_section_in_js_throw() {
    check("named-js", &run().expect("script"));
}

#[test]
fn wasm_frames_without_name_section_in_unreachable_trap() {
    check("noname-trap", &run().expect("script"));
}

#[test]
fn wasm_frames_with_name_section_in_unreachable_trap() {
    check("named-trap", &run().expect("script"));
}

#[test]
fn wasm_frames_in_js_throw_after_jspi_resume() {
    check("jspi-resume", &run().expect("script"));
}

#[test]
fn wasm_frames_in_js_throw_inside_promising_entry() {
    check("jspi-start", &run().expect("script"));
}
