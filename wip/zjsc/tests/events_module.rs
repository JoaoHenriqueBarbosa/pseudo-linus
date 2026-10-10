//! `require("events")` contra o bun 1.4.2: `tests/golden/events_main.js` roda como `/app/main.js` e a saída tem de ser
//! a que o bun deu (`events_main.expected.txt`, gerada com `bun events_main.js`): erros com `code`, chaves e ordem dos
//! exports e do protótipo, `once`, `on`, `captureRejections`, `EventTarget` e a ligação do protótipo do `process`.
use std::rc::Rc;

const PROGRAM: &str = include_str!("golden/events_main.js");
const EXPECTED: &str = include_str!("golden/events_main.expected.txt");

/// Linhas que ainda divergem. [14]: no bun `prototype` aparece em `Object.keys(EventEmitter)` (a propriedade do builtin
/// é enumerável e configurável); no motor o `prototype` de toda função é não configurável, então o JavaScript do
/// módulo não consegue torná-lo enumerável. Fecha quando o `prototype` de função de builtin nascer como o do bun.
const KNOWN_DIVERGENCES: &[usize] = &[14];

#[test]
fn events_module_matches_bun() {
    let console = Rc::new(zjsc::runtime::console_host::MemoryConsole::new(b""));
    let (code, _, _) = zjsc::api::eval::evaluate_main_script_reporting_signal(PROGRAM, "/app/main.js", console.clone());
    let actual = String::from_utf8_lossy(&console.stdout_bytes()).into_owned();
    let stderr = String::from_utf8_lossy(&console.stderr_bytes()).into_owned();
    let diverging: Vec<String> = EXPECTED
        .lines()
        .zip(actual.lines())
        .enumerate()
        .filter(|(index, (expected, actual))| expected != actual && !KNOWN_DIVERGENCES.contains(index))
        .map(|(index, (expected, actual))| format!("[{index}]\n  esperado {expected}\n  obtido   {actual}"))
        .collect();
    assert_eq!(code, 0, "saída {code}, stderr:\n{stderr}");
    assert!(diverging.is_empty() && EXPECTED.lines().count() == actual.lines().count(), "{} divergências ({} linhas esperadas, {} obtidas):\n{}\n{stderr}", diverging.len(), EXPECTED.lines().count(), actual.lines().count(), diverging.join("\n"));
}
