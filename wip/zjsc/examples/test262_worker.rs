//! Executa UM programa do test262 e imprime o resultado numa forma que o `scripts/test262-run.py` lê.
//!
//! Uso: `test262_worker script <arquivo>` ou `test262_worker module <arquivo> [fixture...]`. A primeira linha da saída é
//! `THROWN <texto em uma linha>` (ou `THROWN -`), e depois de `--OUTPUT--` vem o que o teste imprimiu com `print`.

use std::env;
use std::fs;

fn main() {
    // A recursão profunda dos testes precisa de pilha: o orçamento do VM é o da thread, menos uma folga.
    let worker = std::thread::Builder::new().stack_size(64 * 1024 * 1024).spawn(run).expect("thread");
    let _ = worker.join();
}

fn run() {
    zjsc::runtime::vm::VM::set_thread_stack_budget(56 * 1024 * 1024);
    let args: Vec<String> = env::args().skip(1).collect();
    let (Some(mode), Some(path)) = (args.first(), args.get(1)) else {
        eprintln!("uso: test262_worker script|module <arquivo> [fixture...]");
        std::process::exit(2);
    };
    let source = fs::read_to_string(path).unwrap_or_else(|error| {
        eprintln!("{path}: {error}");
        std::process::exit(2);
    });
    let outcome = if mode == "module" {
        let mut files = vec![("main.mjs".to_owned(), source)];
        for fixture in &args[2..] {
            let name = fixture.rsplit('/').next().unwrap_or(fixture).to_owned();
            if let Ok(text) = fs::read_to_string(fixture) {
                files.push((name, text));
            }
        }
        zjsc::api::test262::run_module(&files, "main.mjs")
    } else {
        zjsc::api::test262::run_script(&source, "test.js")
    };
    println!("THROWN {}", outcome.thrown.as_deref().map_or("-".to_owned(), |text| text.replace('\n', "\\n")));
    println!("--OUTPUT--");
    println!("{}", outcome.output);
}
