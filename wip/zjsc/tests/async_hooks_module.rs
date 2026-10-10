//! `require("async_hooks")` contra o bun 1.4.2: `tests/golden/async_hooks_main.js` roda como `/app/main.js` e a saída tem
//! de ser a que o bun deu (`async_hooks_main.expected.txt`, gerada com `bun async_hooks_main.js`): forma do módulo e das
//! classes, erros com `code`, `AsyncLocalStorage` (`run`, `enterWith`, `exit`, `disable`, `withScope`, `snapshot`,
//! `bind`), `AsyncResource` e a propagação do contexto por promessas, `await`, `queueMicrotask`, `process.nextTick`,
//! `setTimeout` e `setImmediate`.
use std::rc::Rc;

const PROGRAM: &str = include_str!("golden/async_hooks_main.js");
const EXPECTED: &str = include_str!("golden/async_hooks_main.expected.txt");

#[test]
fn async_hooks_module_matches_bun() {
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
    assert!(diverging.is_empty() && EXPECTED.lines().count() == actual.lines().count(), "{} divergências ({} linhas esperadas, {} obtidas):\n{}\n{stderr}", diverging.len(), EXPECTED.lines().count(), actual.lines().count(), diverging.join("\n"));
}
