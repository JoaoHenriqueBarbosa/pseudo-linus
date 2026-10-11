//! Golden de `console.dir`, `console.dirxml` e `console.trace` contra o bun 1.4.2: `tests/golden/console_dir_bun.tsv`
//! sai de `bun scripts/gen-console-dir-golden.js`. Colunas, todas em JSON: fonte, bytes do stdout em hex, bytes do
//! stderr em hex e o valor de `R`. O programa roda como o próprio script `console_dir_case.js` (os frames de
//! `console.trace` citam esse nome).
mod common;
mod zbun_transpiler;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;

use zjsc::api::eval::evaluate_named_script_with_console;
use zjsc::runtime::console_host::MemoryConsole;

const GOLDEN: &str = include_str!("golden/console_dir_bun.tsv");

/// Linhas (1-based) que ainda divergem do bun com o transpilador instalado, cada uma com a causa medida.
const KNOWN: &[common::KnownCase] = &[
    common::KnownCase { label: "380", cause: "o frame de `new K().p` no principal transpilado sai na coluna do callee (4:5), o bun na do `new` (4:1): 4 colunas, o tamanho de `new `" },
    common::KnownCase { label: "383", cause: "o frame `eval` do principal ESM sai sem `file:///` (a origem do módulo é a chave plana, js_module_loader.rs); o bun imprime `eval (file:///console_dir_case.js:1:2)`" },
    common::KnownCase { label: "384", cause: "o frame `eval` do principal ESM sai sem `file:///` (a origem do módulo é a chave plana, js_module_loader.rs); o bun imprime `eval (file:///console_dir_case.js:1:14)`" },
    common::KnownCase { label: "385", cause: "o frame `anonymous` de `new Function` no principal ESM sai sem `file:///` (mesma origem plana); o bun imprime `anonymous (file:///console_dir_case.js:...)`" },
    common::KnownCase { label: "412", cause: "`new F()` de função comum no principal: o frame sai `at <anonymous> (...)` e o bun imprime `at new  (...)` (nome vazio do construtor)" },
];

fn hex(text: &str) -> String {
    text.bytes().map(|byte| format!("{byte:02x}")).collect()
}

/// O frame sem fonte (inicializador de campo de classe) sai no `console.trace` sem URL, `new A (1:17)`, enquanto a linha de
/// `stack` escreve `unknown:1:17` (medido no bun 1.4.2).
#[test]
fn trace_frame_without_source_has_no_url() {
    let console = Rc::new(MemoryConsole::new(b""));
    let source = "class A { x = console.trace('field') }\nnew A";
    let _ = evaluate_named_script_with_console(source, "console_dir_case.js", "R", Some(console.clone()));
    let stdout = String::from_utf8_lossy(&console.stdout_bytes()).into_owned();
    assert!(stdout.lines().any(|line| line == "      at new A (1:17)"), "{stdout}");
}

#[test]
fn console_dir_match_bun() {
    common::run_with_stack(256 * 1024 * 1024, || {
        zbun_transpiler::install();
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
            // O programa roda como o arquivo principal `console_dir_case.js` com o transpilador instalado, como o `bun run` o roda.
            let outcome = catch_unwind(AssertUnwindSafe(|| common::originals::run_original_capture(&source, "console_dir_case.js", common::originals::Preload::AfterProgram)));
            let (stdout, stderr, result) = match outcome {
                Ok(run) => (hex(&run.stdout), hex(&run.stderr), run.result),
                Err(_) => (String::new(), String::new(), "<pânico>".to_string()),
            };
            if stdout != expected_stdout || stderr != expected_stderr || result != expected_result {
                let shown = source.split_once("try {\n").map_or(source.as_str(), |(_, program)| program);
                failures.push(format!("[{total}] {shown}\n  stdout {stdout:?} esperado {expected_stdout:?}\n  stderr {stderr:?} esperado {expected_stderr:?}\n  R {result:?} esperado {expected_result:?}"));
            }
        }
        assert!(total >= 400, "poucos casos: {total}");
        common::assert_only_known(&failures, KNOWN);
    });
}
