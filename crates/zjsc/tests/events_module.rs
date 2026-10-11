//! `require("events")` contra o bun 1.4.2: `tests/golden/events_main.js` roda como `/app/main.js` e a saída tem de ser
//! a que o bun deu (`events_main.expected.txt`, gerada com `bun events_main.js`): erros com `code`, chaves e ordem dos
//! exports e do protótipo, `once`, `on`, `captureRejections`, `EventTarget` e a ligação do protótipo do `process`.
use std::rc::Rc;

const PROGRAM: &str = include_str!("golden/events_main.js");
const EXPECTED: &str = include_str!("golden/events_main.expected.txt");

#[test]
fn events_module_matches_bun() {
    check_against_bun(None);
}

/// Com a coleta a cada 500 alocações o resultado é o mesmo: o wrapper CommonJS do principal segue vivo depois de
/// devolver (como no bun), então os erros criados nele mantêm o caminho normal de `stack`, e os módulos embutidos não
/// perdem nenhum valor para a coleta.
#[test]
fn events_module_matches_bun_under_collection_stress() {
    check_against_bun(Some(500));
}

fn check_against_bun(stress: Option<usize>) {
    let console = Rc::new(zjsc::runtime::console_host::MemoryConsole::new(b""));
    zjsc::runtime::gc::set_stress_for_thread(stress);
    let (code, _, _) = zjsc::api::eval::evaluate_main_script_reporting_signal(PROGRAM, "/app/main.js", console.clone());
    zjsc::runtime::gc::set_stress_for_thread(None);
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
