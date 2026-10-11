//! `Error.stack` do programa ORIGINAL contra o `bun run` do bun 1.4.2, com o transpilador do zbun instalado: o fonte
//! original passa pelo impressor (texto reimpresso) e as posições saem do mapa dele, como no bun. O golden
//! (`tests/golden/error_stack_originals_bun.tsv`, de `ORIGINALS=1 bun scripts/gen-error-stack-golden.js`) guarda o
//! programa e o valor da global `R`, sem o texto canônico e o mapa de corridas do `error_stack_bun`.
mod common;
mod zbun_transpiler;

use std::rc::Rc;

use common::json_string;

const GOLDEN: &str = include_str!("golden/error_stack_originals_bun.tsv");

/// Divergências conhecidas: `(trecho do programa, causa medida no oráculo)`. Uma entrada que passa a bater faz o teste
/// pedir a remoção dela.
const KNOWN: &[(&str, &str)] = &[
    (
        ".test(f().stack.split('\\n')[1])",
        "o oráculo roda o arquivo num diretório temporário e o frame sai `(/diretório/error_stack_case.js:L:C)` (medido: `at <anonymous> (/w/b.js:2:68)`), então o regex ancorado em `(error_stack_case.js` dá `false`; o gerador só limpa o diretório do texto da pilha, não de um `test()`. O porte roda com URL relativa e dá `true`. Artefato do gerador",
    ),
];

fn run(program: &str) -> String {
    // O preload do gerador (`process.on('exit')`) registra antes do programa: vai na linha 1, depois da diretiva, para
    // não mover as linhas do corpo e para o `exit` rodar também quando o programa morre com exceção.
    let preload = "process.on('exit', () => { console.log('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R))); });";
    let (head, rest) = program.split_once('\n').expect("programa de várias linhas");
    let program = format!("{head}{preload}\n{rest}");
    let console = Rc::new(zjsc::runtime::console_host::MemoryConsole::new(b""));
    let (_code, _, _) = zjsc::api::eval::evaluate_main_script_reporting_signal(&program, "error_stack_case.js", console.clone());
    let stdout = String::from_utf8_lossy(&console.stdout_bytes()).into_owned();
    match stdout.lines().find_map(|line| line.strip_prefix('\u{1}')) {
        Some(json) => json_string(json),
        // O programa nem rodou (o bun recusa o arquivo por erro de sintaxe) e o preload do gerador, que roda à parte, grava
        // `<undefined>`; aqui o preload vai no mesmo arquivo e some junto.
        None => "<undefined>".to_string(),
    }
}

#[test]
fn error_stack_of_original_programs_matches_bun() {
    common::run_with_stack(256 * 1024 * 1024, || {
        zjsc::runtime::vm::VM::set_thread_stack_budget(240 * 1024 * 1024);
        zbun_transpiler::install();
        let mut failures = Vec::new();
        let mut total = 0;
        for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
            let mut columns = line.splitn(2, '\t');
            let program = json_string(columns.next().expect("programa"));
            let expected = json_string(columns.next().expect("resultado"));
            total += 1;
            let actual = std::panic::catch_unwind(|| run(&program)).unwrap_or_else(|_| "<pânico>".to_string());
            let known = KNOWN.iter().find(|(fragment, _)| program.contains(fragment));
            match (actual != expected, known) {
                (true, None) => failures.push(format!("{program}\n    esperado {expected:?}\n    veio     {actual:?}")),
                (false, Some((fragment, _))) => failures.push(format!("passou a bater, tire de KNOWN: {fragment}")),
                _ => {}
            }
        }
        assert!(total >= 900, "golden com só {total} casos");
        assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
    });
}
