//! `require("stream")` e `require("string_decoder")` contra o bun 1.4.2: `tests/golden/stream_main.js` roda como
//! `/app/main.js` e a saída tem de ser a que o bun deu (`stream_main.expected.txt`, gerada com `bun stream_main.js`):
//! chaves e ordem dos exports e dos protótipos, erros com `code`, `Writable`, `Readable.from`, `pipeline`,
//! `finished`, `Transform`, `PassThrough`, `Duplex`, iteração assíncrona e o `StringDecoder`.
use std::rc::Rc;

mod common;
const PROGRAM: &str = include_str!("golden/stream_main.js");
const EXPECTED: &str = include_str!("golden/stream_main.expected.txt");

#[test]
fn stream_module_matches_bun() {
    common::run_with_stack(256 * 1024 * 1024, || {
        zjsc::runtime::vm::VM::set_thread_stack_budget(240 * 1024 * 1024);
        let console = Rc::new(zjsc::runtime::console_host::MemoryConsole::new(b""));
        let (code, _, _) = zjsc::api::eval::evaluate_main_script_reporting_signal(PROGRAM, "/app/main.js", console.clone());
        let actual = String::from_utf8_lossy(&console.stdout_bytes()).into_owned();
        let stderr = String::from_utf8_lossy(&console.stderr_bytes()).into_owned();
        let diverging: Vec<String> = EXPECTED
            .lines()
            .zip(actual.lines())
            .enumerate()
            .filter(|(_, (expected, actual))| expected != actual)
            .map(|(index, (expected, actual))| format!("[{index}]\n  esperado {expected}\n  obtido   {actual}"))
            .collect();
        assert_eq!(code, 0, "saída {code}, stderr:\n{stderr}");
        assert!(
            diverging.is_empty() && EXPECTED.lines().count() == actual.lines().count(),
            "{} divergências ({} linhas esperadas, {} obtidas):\n{}\n{stderr}",
            diverging.len(),
            EXPECTED.lines().count(),
            actual.lines().count(),
            diverging.join("\n")
        );
    });
}
