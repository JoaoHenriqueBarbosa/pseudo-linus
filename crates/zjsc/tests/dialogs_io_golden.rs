//! Golden do convite e da leitura de `alert`, `confirm` e `prompt` contra o bun 1.4.2: `tests/golden/dialogs_io_bun.tsv`
//! sai de `bun scripts/gen-dialogs-golden.js io`. Colunas: stdin, fonte, bytes do stdout capturado (o convite) em hex
//! e o valor de `R`, todas em JSON. O console do host é um [`MemoryConsole`] com o stdin da linha (ver `src/runtime/dialogs.rs`).
mod common;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;

use zjsc::api::eval::evaluate_named_script_with_console;
use zjsc::runtime::console_host::MemoryConsole;

const GOLDEN: &str = include_str!("golden/dialogs_io_bun.tsv");

#[test]
fn dialogs_io_match_bun() {
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines() {
        let columns: Vec<&str> = line.split('\t').collect();
        assert_eq!(columns.len(), 4, "linha malformada: {line}");
        let stdin = common::json_string(columns[0]);
        let source = common::json_string(columns[1]);
        let expected_stdout = common::json_string(columns[2]);
        let expected_result = common::json_string(columns[3]);
        total += 1;
        let console = Rc::new(MemoryConsole::new(stdin.as_bytes()));
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            evaluate_named_script_with_console(&source, "dialogs_io_case.js", "R", Some(console.clone()))
                .map(|read| read.ok().map(|value| String::from_utf16_lossy(&value.to_wtf_string().characters_without_null_termination().expect("unidades UTF-16"))))
        }));
        let result = match outcome {
            Ok(Ok(Some(text))) => text,
            Ok(Ok(None)) => "<undefined>".to_string(),
            Ok(Err(uncaught)) => uncaught,
            Err(_) => "<pânico>".to_string(),
        };
        let stdout = console.stdout().bytes().map(|byte| format!("{byte:02x}")).collect::<String>();
        if stdout != expected_stdout || result != expected_result {
            failures.push(format!("stdin {stdin:?} fonte {:?}\n  stdout {stdout:?} esperado {expected_stdout:?}\n  R {result:?} esperado {expected_result:?}", &source[source.len().min(source.find("try {").unwrap_or(0))..]));
        }
    }
    assert!(total >= 150, "poucos casos: {total}");
    assert!(failures.is_empty(), "{} de {total} divergem:\n{}", failures.len(), failures.iter().take(20).cloned().collect::<Vec<_>>().join("\n"));
}
