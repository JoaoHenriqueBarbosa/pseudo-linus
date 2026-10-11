//! Goldens de programas ORIGINAIS contra o `bun run`: o programa roda como arquivo principal, com o transpilador do zbun
//! instalado pelo teste (`zbun_transpiler::install()`), e o `R` sai por um preload `process.on('exit')` como no gerador
//! (`ORIGINALS=1 bun scripts/gen-<nome>-golden.js`). As colunas do golden são o programa original e o valor de `R`.
use std::rc::Rc;

use super::{assert_only_known, json_string, KnownCase};

/// O que um programa original escreveu e o `R` dele.
pub struct OriginalRun {
    /// O stdout do programa, sem a linha marcada do preload.
    pub stdout: String,
    pub stderr: String,
    /// O `R` em JSON como o golden o guarda (`<undefined>` quando não foi gravado ou o programa nem rodou).
    pub result: String,
}

/// Onde o preload do gerador (`process.on('exit')`) entra no arquivo. O do `bun run` vem por `--preload`, fora do arquivo; aqui ele
/// precisa ir dentro dele sem mover as linhas do corpo.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Preload {
    /// No fim da linha 1: registra antes do programa, então o `exit` roda também quando ele morre com exceção. O trecho de código de
    /// um relato de erro que cita a linha 1 leva o preload junto, então só serve onde esse trecho não entra no resultado.
    EndOfFirstLine,
    /// Numa linha depois do programa: o trecho de código do relato de erro não o vê, mas o programa precisa chegar ao fim (o dos
    /// goldens de `console` captura tudo com `try`).
    AfterProgram,
}

/// Roda `program` como o arquivo principal `filename`.
pub fn run_original_capture(program: &str, filename: &str, placement: Preload) -> OriginalRun {
    let preload = "process.on('exit', () => { console.log('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R))); });";
    let program = match placement {
        Preload::EndOfFirstLine => {
            let (head, rest) = program.split_once('\n').expect("programa de várias linhas");
            format!("{head}{preload}\n{rest}")
        }
        Preload::AfterProgram => format!("{program}\n{preload}"),
    };
    let console = Rc::new(zjsc::runtime::console_host::MemoryConsole::new(b""));
    let (_code, _, _) = zjsc::api::eval::evaluate_main_script_reporting_signal(&program, filename, console.clone());
    let stdout = String::from_utf8_lossy(&console.stdout_bytes()).into_owned();
    let stderr = String::from_utf8_lossy(&console.stderr_bytes()).into_owned();
    // O programa pode nem rodar (o bun recusa o arquivo por erro de sintaxe) e o preload do gerador, que roda à parte, grava
    // `<undefined>`; aqui o preload vai no mesmo arquivo e some junto.
    let mut result = "<undefined>".to_string();
    let mut kept = stdout.clone();
    if let Some(mark) = stdout.find('\u{1}') {
        let end = stdout[mark..].find('\n').map_or(stdout.len(), |offset| mark + offset + 1);
        result = json_string(stdout[mark + 1..end].trim_end_matches('\n'));
        // O `console.log` do preload herda a indentação de `console.group` e pode vir logo depois de saída sem quebra de linha:
        // tira só os espaços que formam a linha inteira da marca.
        let before = &stdout[..mark];
        let line_start = before.rfind('\n').map_or(0, |at| at + 1);
        let prefix = if before[line_start..].bytes().all(|byte| byte == b' ') { &before[..line_start] } else { before };
        kept = format!("{prefix}{}", &stdout[end..]);
    }
    OriginalRun { stdout: kept, stderr, result }
}

/// Mede cada linha do golden `programa<TAB>resultado` (1-based) contra o motor com o transpilador instalado. As linhas que ainda
/// divergem entram em `known` com a causa individual medida; uma linha fora da lista, ou da lista que passou a bater, falha.
pub fn check_original_programs(golden: &str, filename: &str, min_total: usize, known: &[KnownCase]) {
    let mut failures = Vec::new();
    let mut total = 0;
    for (index, line) in golden.lines().enumerate().filter(|(_, line)| !line.is_empty()) {
        let mut columns = line.splitn(2, '\t');
        let program = json_string(columns.next().expect("programa"));
        let expected = json_string(columns.next().expect("resultado"));
        total += 1;
        let actual = std::panic::catch_unwind(|| run_original_capture(&program, filename, Preload::EndOfFirstLine).result).unwrap_or_else(|_| "<pânico>".to_string());
        if actual != expected {
            failures.push(format!("[{}] {program}\n    esperado {expected:?}\n    veio     {actual:?}", index + 1));
        }
    }
    assert!(total >= min_total, "golden com só {total} casos");
    assert_only_known(&failures, known);
}
