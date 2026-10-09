//! Golden de `console.dir`, `console.dirxml` e `console.trace` contra o bun 1.4.2: `tests/golden/console_dir_bun.tsv`
//! sai de `bun scripts/gen-console-dir-golden.js`. Colunas, todas em JSON: fonte, bytes do stdout em hex, bytes do
//! stderr em hex e o valor de `R`. O programa roda como o próprio script `console_dir_case.js` (os frames de
//! `console.trace` citam esse nome).
mod common;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;

use zjsc::api::eval::evaluate_named_script_with_console;
use zjsc::runtime::console_host::MemoryConsole;

const GOLDEN: &str = include_str!("golden/console_dir_bun.tsv");

fn hex(text: &str) -> String {
    text.bytes().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn console_dir_match_bun() {
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines() {
        let columns: Vec<&str> = line.split('\t').collect();
        assert_eq!(columns.len(), 4, "linha malformada: {line}");
        let source = common::json_string(columns[0]);
        let expected_stdout = common::json_string(columns[1]);
        let expected_stderr = common::json_string(columns[2]);
        let expected_result = common::json_string(columns[3]);
        total += 1;
        let console = Rc::new(MemoryConsole::new(b""));
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            evaluate_named_script_with_console(&source, "console_dir_case.js", "R", Some(console.clone()))
                .map(|read| read.ok().filter(|value| !value.is_undefined()).map(|value| String::from_utf16_lossy(&value.to_wtf_string().characters_without_null_termination().expect("unidades UTF-16"))))
        }));
        let result = match outcome {
            Ok(Ok(Some(text))) => text,
            Ok(Ok(None)) => "<undefined>".to_string(),
            Ok(Err(uncaught)) => uncaught,
            Err(_) => "<pânico>".to_string(),
        };
        let (stdout, stderr) = (hex(&console.stdout()), hex(&console.stderr()));
        if stdout != expected_stdout || stderr != expected_stderr || result != expected_result {
            let shown = source.split_once("try {\n").map_or(source.as_str(), |(_, program)| program);
            failures.push(format!("{shown}\n  stdout {stdout:?} esperado {expected_stdout:?}\n  stderr {stderr:?} esperado {expected_stderr:?}\n  R {result:?} esperado {expected_result:?}"));
        }
    }
    assert!(total >= 400, "poucos casos: {total}");
    assert!(failures.is_empty(), "{} de {total} divergem:\n{}", failures.len(), failures.iter().take(20).cloned().collect::<Vec<_>>().join("\n"));
}
