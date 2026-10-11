//! O fonte ORIGINAL de um arquivo principal, rodado como o bun o roda: reimpresso pelo transpilador (`wip/zbun`,
//! instalado como `SourceTranspiler`) e remapeado pelo mapa do impressor dele. Cada `tests/golden/transpiled_main/NOME.EXT`
//! roda como `/app/main.EXT` e a saída tem de ser a do `bun run` do oráculo (`NOME.EXT.expected.txt`, gerado por
//! `scripts/gen-transpiled-main-golden.sh`). Cobre as três famílias que a reimpressão explica: o texto da expressão
//! nas mensagens do motor, o `Function.prototype.toString` do arquivo principal e as linhas e colunas de `Error.stack`.
mod zbun_transpiler;

use std::rc::Rc;

fn run_case(program: &str, extension: &str, expected: &str) {
    zbun_transpiler::install();
    let console = Rc::new(zjsc::runtime::console_host::MemoryConsole::new(b""));
    let url = format!("/app/main.{extension}");
    let (code, _, _) = zjsc::api::eval::evaluate_main_script_reporting_signal(program, &url, console.clone());
    let actual = String::from_utf8_lossy(&console.stdout_bytes()).into_owned();
    let stderr = String::from_utf8_lossy(&console.stderr_bytes()).into_owned();
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

macro_rules! cases {
    ($($test:ident: $name:literal, $extension:literal;)*) => {
        $(
            #[test]
            fn $test() {
                run_case(
                    include_str!(concat!("golden/transpiled_main/", $name, ".", $extension)),
                    $extension,
                    include_str!(concat!("golden/transpiled_main/", $name, ".", $extension, ".expected.txt")),
                );
            }
        )*
    };
}

cases! {
    not_constructor_text_matches_bun: "not_constructor", "js";
    function_source_matches_bun: "function_source", "js";
    stack_positions_match_bun: "stack", "js";
    commonjs_main_matches_bun: "commonjs", "js";
    typescript_main_matches_bun: "typed", "ts";
}
