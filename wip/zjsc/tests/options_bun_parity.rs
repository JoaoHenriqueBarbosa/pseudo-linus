//! Paridade de `Options` com o bun 1.4.2: `tests/golden/options_parity_bun.tsv` sai de
//! `scripts/gen-options-parity.js` (expressão e `typeof`, ou `throws <Nome>`). Todos os pontos de entrada do host
//! (`evaluate_script`, `evaluate_named_script_result`, `evaluate_module_map`) precisam criar o realm com as mesmas
//! opções do bun: as APIs que ele liga (`ShadowRealm`, `Temporal`, `DisposableStack`, `Iterator.zip`...) existem, e as
//! que ele deixa desligadas (`Promise.isPromise`, `BigInt.sqrt`...) continuam ausentes, para o sandbox responder
//! `undefined` ou `ReferenceError` como o bun.
use zjsc::api::eval::{evaluate_named_script_result, evaluate_script};
use zjsc::api::module::evaluate_module_map;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/options_parity_bun.tsv");

fn cases() -> Vec<(&'static str, &'static str)> {
    GOLDEN.lines().filter(|line| !line.is_empty()).map(|line| line.split_once('\t').expect("expressão e typeof")).collect()
}

/// O mesmo programa de `probeProgram` em `gen-options-parity.js`: `sink(expressão)` recebe o `typeof` ou o erro.
fn probe_program(cases: &[(&str, &str)], sink: &str) -> String {
    let mut program = String::new();
    for (expression, _) in cases {
        program.push_str(&format!("try {{ {sink}(typeof ({expression})); }} catch (e) {{ {sink}(\"throws \" + e.name); }}\n"));
    }
    program
}

fn compare(entry: &str, actual: &[String], cases: &[(&str, &str)]) {
    assert_eq!(actual.len(), cases.len(), "{entry}: número de resultados");
    let failures: Vec<String> = cases
        .iter()
        .zip(actual)
        .filter(|((_, expected), got)| expected != got)
        .map(|((expression, expected), got)| format!("{expression}: bun {expected:?}, porte {got:?}"))
        .collect();
    assert!(failures.is_empty(), "{entry}: {} de {} divergem do bun:\n{}", failures.len(), cases.len(), failures.join("\n"));
}

#[test]
fn evaluate_script_matches_bun_options() {
    let cases = cases();
    assert!(cases.len() >= 60, "golden com só {} expressões", cases.len());
    let program = format!("var out = [];\n{}var R = out.join(\"\\n\"); R", probe_program(&cases, "out.push"));
    let value = evaluate_script(&program).expect("programa de medição sem exceção");
    let text = String::from_utf8_lossy(&value.to_wtf_string().utf8(ConversionMode::LenientConversion)).into_owned();
    let actual: Vec<String> = text.lines().map(str::to_owned).collect();
    compare("evaluate_script", &actual, &cases);
}

#[test]
fn evaluate_named_script_result_matches_bun_options() {
    let cases = cases();
    let program = format!("var out = [];\n{}var R = out.join(\"\\n\");", probe_program(&cases, "out.push"));
    let value = evaluate_named_script_result(&program, "options_case.js", "R").expect("programa de medição sem exceção");
    let text = String::from_utf8_lossy(&value.to_wtf_string().utf8(ConversionMode::LenientConversion)).into_owned();
    let actual: Vec<String> = text.lines().map(str::to_owned).collect();
    compare("evaluate_named_script_result", &actual, &cases);
}

#[test]
fn evaluate_module_map_matches_bun_options() {
    let cases = cases();
    let files = vec![("main.mjs".to_owned(), probe_program(&cases, "L"))];
    let outcome = evaluate_module_map(&files, "main.mjs");
    assert_eq!(outcome.error, None, "o módulo de medição rejeitou");
    // `log` serializado: ["function","object",...], sem aspas nem vírgulas dentro dos valores.
    let inner = outcome.log_json.trim().trim_start_matches('[').trim_end_matches(']');
    let actual: Vec<String> = inner.split(',').map(|item| item.trim_matches('"').to_owned()).collect();
    compare("evaluate_module_map", &actual, &cases);
}
