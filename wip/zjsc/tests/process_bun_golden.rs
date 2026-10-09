//! Golden do global `process` contra o bun 1.4.2: `tests/golden/process_bun.tsv` sai de `scripts/gen-process-golden.js`
//! (`JSON(sufixo)<TAB>JSON(resultado)[<TAB>índice do prelúdio]`, prelúdios em `process.preludes.json`). O resultado é
//! `stdout + "#stderr\n" + stderr + "#exit " + código`. Cada programa roda como `/app/main.js`.
//!
//! Cobre `process.nextTick` (fila, ordem e validação) e `exit`/`exitCode`/`exit`/`beforeExit`; os que dependem de
//! módulo ESM, `require`, `eval` indireto, `process.stdout` e do `this` cru de função não estrita (`typeof this` num
//! `function` sloppy) ficam de fora e são contados como esperados-falhando.
mod common;

const GOLDEN: &str = include_str!("golden/process_bun.tsv");
const PRELUDES: &str = include_str!("golden/process.preludes.json");

/// Faixas (inclusivas, índice da linha no tsv começando em 0) dos casos em escopo: nextTick e, da fatia 3, `exit`,
/// `exitCode` e os eventos `exit` e `beforeExit` (250 a 333 e 648 a 657) e, da fatia 4, `uncaughtException`,
/// `unhandledRejection`, o monitor, `rejectionHandled` e o callback de captura (335 a 426). Ficam de fora o 334 (ESM com
/// `await` no topo) e o 644 a 647 (`process.stdout`/`stderr`, cobertos pela forma medida de `process_stdio.rs`: chaves
/// próprias na ordem, cadeia `WriteStream` -> `Writable` -> `Stream` -> `EventEmitter` e `cork`/`uncork`/`end`/
/// `setDefaultEncoding`; ainda não entram na faixa porque `finish`/`close` e o stdin falam além do golden). Da fatia 5 (forma de `process`): 0 a 3, 7, 8 e
/// 10 a 15 (chaves, protótipo, tag, extensibilidade), 17, 26 a 128 (descritor e tipo de cada chave e do protótipo) e 130
/// a 145 (`name`/`length` das funções). Ficam de fora o 4 a 6 e o 9, que dependem de `require`.
const IN_SCOPE: &[(usize, usize)] = &[
    (0, 3),
    (7, 8),
    (10, 17),
    (16, 16),
    (18, 25),
    (26, 128),
    (129, 129),
    (130, 145),
    (146, 182),
    (189, 205),
    (207, 225),
    (239, 333),
    (335, 426),
    // Fatia 9: `emitWarning` e o evento `warning` (427 a 472). Ficam de fora 473, 474 e 476 (usam `require('events')`);
    // 475 (11 ouvintes de `foo` no `process`, sem aviso) entra.
    (427, 472),
    (475, 475),
    (585, 586),
    // Fatia 7: argv, execArgv, chdir (erros), platform, arch, version(s), release, config, features, title, pid, ppid,
    // ids, umask, uptime e kill (validação e ESRCH). Ficam de fora os que usam `require`, `__dirname`/`__filename`,
    // `import.meta`, `Bun` ou `process.stdout` (540 a 542 parciais: 541, 542, 544, 548 a 552, 556, 558, 559, 564, 570,
    // 587 a 590). Os que terminam por sinal (594, 596 a 600) entram: o harness lê o sinal e compara `signal:NOME`.
    (540, 540),
    (543, 543),
    (545, 547),
    (553, 555),
    (557, 557),
    (560, 563),
    (565, 569),
    (571, 584),
    (591, 636),
    // Fatia 10: `process.stdout/stderr.write` (637 a 647; 644 a 647 eram da fatia 3).
    (637, 647),
    (648, 657),
    (659, 675),
    // Fatia 6: `process.env` (477 a 529 e 534 a 537). Ficam de fora 530 a 533 (`child_process` e o env herdado) e 538 e
    // 539 (global `Bun`).
    (477, 529),
    (534, 537),
];

fn in_scope(index: usize) -> bool {
    IN_SCOPE.iter().any(|(start, end)| (*start..=*end).contains(&index))
}

#[test]
fn process_next_tick_matches_bun() {
    common::run_with_stack(256 * 1024 * 1024, || {
        zjsc::runtime::vm::VM::set_thread_stack_budget(240 * 1024 * 1024);
        let preludes = common::parse_preludes(PRELUDES);
        let mut covered = 0;
        let mut expected_failing = 0;
        let mut failures = Vec::new();
        for (index, line) in GOLDEN.lines().filter(|line| !line.is_empty()).enumerate() {
            if !in_scope(index) {
                expected_failing += 1;
                continue;
            }
            let mut columns = line.split('\t');
            let suffix = common::json_string(columns.next().expect("sufixo"));
            let expected = common::json_string(columns.next().expect("resultado"));
            let prelude = columns.next().map_or(0, |column| column.parse::<usize>().expect("índice do prelúdio"));
            let source = format!("{}{}", preludes[prelude].text, suffix);
            let console = std::rc::Rc::new(zjsc::runtime::console_host::MemoryConsole::new(b""));
            // O golden rodou como `cd /app && bun main.js x --flag`.
            zjsc::runtime::process_system::set_script_arguments(&["x", "--flag"]);
            zjsc::runtime::process_system::set_working_directory("/app");
            // O ambiente do golden, na ordem do bun (`Object.keys`).
            zjsc::runtime::process_env::set_environment(&[
                ("PATH", "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"),
                ("HOME", "/root"),
                ("NO_COLOR", "1"),
                ("FOO", "bar"),
                ("EMPTY", ""),
                ("mixedCase", "1"),
            ]);
            let (code, _, signal) = zjsc::api::eval::evaluate_main_script_reporting_signal(&source, "/app/main.js", console.clone());
            // O golden registra a morte por sinal como `signal:NOME`.
            let exit = match signal.and_then(zjsc::runtime::process_system::signal_name) {
                Some(name) => format!("signal:{name}"),
                None => code.to_string(),
            };
            // O gerador troca o pid de `(bun:PID)` e `(node:PID)` por `<pid>`.
            let pid_prefix = format!("(node:{}", std::process::id());
            let actual = format!(
                "{}#stderr\n{}#exit {exit}",
                String::from_utf8_lossy(&console.stdout_bytes()),
                String::from_utf8_lossy(&console.stderr_bytes()).replace(&pid_prefix, "(node:<pid>")
            );
            covered += 1;
            if actual != expected {
                failures.push(format!("[{index}] {suffix}\n  esperado {expected:?}\n  obtido   {actual:?}"));
            }
        }
        println!("process nextTick: {covered} casos cobertos, {expected_failing} fora do escopo desta fatia");
        assert!(covered > 100, "faixas do escopo não batem com o golden");
        assert!(failures.is_empty(), "{} divergências:\n{}", failures.len(), failures.join("\n"));
    });
}
