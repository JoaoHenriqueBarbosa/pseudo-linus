//! `Bun.deepEquals` contra o bun 1.4.2: `tests/golden/bun_deep_equals_main.js` roda como `/app/main.js` e a saída tem
//! de ser a que o bun do oráculo deu (`bun_deep_equals_main.expected.txt`, gerada com `bun /app/main.js` na imagem do
//! oráculo): a forma da função e os três modos (comum, estrito e estrito sem identidade de protótipo) em primitivos,
//! arrays, objetos, coleções, typed arrays, buffers, erros, invólucros, funções, objetos do WebCore, ciclos e `Proxy`.
use std::rc::Rc;

const PROGRAM: &str = include_str!("golden/bun_deep_equals_main.js");
const EXPECTED: &str = include_str!("golden/bun_deep_equals_main.expected.txt");

#[test]
fn bun_deep_equals_matches_bun() {
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
}
