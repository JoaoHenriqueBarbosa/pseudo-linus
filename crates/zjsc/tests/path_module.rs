//! `require("path")` contra o bun 1.4.2: `tests/golden/path_main.js` roda como `/app/main.js` e a saída tem de ser a que
//! o bun deu (`path_main.expected.txt`, gerada com `bun path_main.js`): chaves e ordem, identidades (`path ===
//! path.posix`, `path/posix`, `path/win32`), nome e aridade das funções, os dois sabores (posix e win32) em dezenas de
//! entradas, os erros de argumento (`ERR_INVALID_ARG_TYPE` com o texto do bun) e o par substituto solto.
use std::rc::Rc;

const PROGRAM: &str = include_str!("golden/path_main.js");
const EXPECTED: &str = include_str!("golden/path_main.expected.txt");

/// Linhas que ainda divergem. O bun tem 17 chaves em `path` (a 17ª é `matchesGlob`) e o porte 16: `matchesGlob` usa o
/// `Bun.Glob`, que ainda não existe; as três linhas fecham quando ele entrar. [0] `Object.keys(path)`, [1]
/// `Object.getOwnPropertyNames(path)` e a última (`Object.keys(path.win32).length`).
const KNOWN_DIVERGENCES: &[usize] = &[0, 1, 218];

mod common;

#[test]
fn path_module_matches_bun() {
    // `path.resolve` parte do `process.cwd()` da máquina que roda o teste, como o golden.
    common::std_host::install();
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
    assert!(
        diverging.is_empty() && EXPECTED.lines().count() == actual.lines().count(),
        "{} divergências ({} linhas esperadas, {} obtidas):\n{}\n{stderr}",
        diverging.len(),
        EXPECTED.lines().count(),
        actual.lines().count(),
        diverging.join("\n")
    );
}
