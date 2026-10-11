//! O programa principal de um golden do bun (`tests/golden/*_main.js`, rodado como `/app/main.js`) contra a saída que
//! o bun do oráculo deu (`*_main.expected.txt`): código de saída 0 e o stdout igual, linha a linha.
use std::rc::Rc;

pub fn assert_main_script_matches(program: &str, expected: &str) {
    let console = Rc::new(zjsc::runtime::console_host::MemoryConsole::new(b""));
    let (code, _, _) = zjsc::api::eval::evaluate_main_script_reporting_signal(program, "/app/main.js", console.clone());
    let actual = String::from_utf8_lossy(&console.stdout_bytes()).into_owned();
    let stderr = String::from_utf8_lossy(&console.stderr_bytes()).into_owned();
    // `ZJSC_GOLDEN_DUMP=<arquivo>` grava a saída obtida, para comparar por caminho em vez de por linha quando o golden
    // diverge e o desalinhamento das linhas esconde o resto.
    if let Ok(path) = std::env::var("ZJSC_GOLDEN_DUMP") {
        std::fs::write(path, &actual).expect("gravar a saída obtida do golden");
    }
    let diverging: Vec<String> = expected
        .lines()
        .zip(actual.lines())
        .enumerate()
        .filter(|(_, (expected, actual))| expected != actual)
        .map(|(index, (expected, actual))| format!("[{index}]\n  esperado {expected}\n  obtido   {actual}"))
        .collect();
    assert_eq!(code, 0, "saída {code}, stderr:\n{stderr}");
    assert!(
        diverging.is_empty() && expected.lines().count() == actual.lines().count(),
        "{} divergências ({} linhas esperadas, {} obtidas):\n{}\n{stderr}",
        diverging.len(),
        expected.lines().count(),
        actual.lines().count(),
        diverging.join("\n")
    );
}
