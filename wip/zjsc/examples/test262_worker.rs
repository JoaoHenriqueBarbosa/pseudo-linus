//! Executa UM programa do test262 e imprime o resultado numa forma que o `scripts/test262-run.py` lê.
//!
//! Uso: `test262_worker script|module <arquivo> <chave> [chave=arquivo...]`: `<arquivo>` é o programa montado (em `module`, o harness como script global, a linha `//--MODULE--` e o módulo de entrada), `<chave>` o caminho do
//! teste relativo a `test/` (a origem do script e a chave do módulo de entrada) e cada `chave=arquivo` um módulo que o teste importa
//! (`*_FIXTURE.js`), na chave relativa a `test/` que os `import` dele resolvem. A primeira linha da saída é
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
    let (Some(mode), Some(path), Some(key)) = (args.first(), args.get(1), args.get(2)) else {
        eprintln!("uso: test262_worker script|module <arquivo> <chave> [chave=arquivo...]");
        std::process::exit(2);
    };
    let source = fs::read_to_string(path).unwrap_or_else(|error| {
        eprintln!("{path}: {error}");
        std::process::exit(2);
    });
    let mut fixtures: Vec<(String, String)> = Vec::new();
    for pair in &args[3..] {
        if let Some((name, file)) = pair.split_once('=') {
            if let Ok(text) = fs::read_to_string(file) {
                fixtures.push((name.to_owned(), text));
            }
        }
    }
    let outcome = if mode == "module" {
        // O programa montado traz o harness (script global) e, depois da linha `//--MODULE--`, o módulo de entrada.
        let (global_script, module_source) = source.split_once("\n//--MODULE--\n").unwrap_or(("", source.as_str()));
        fixtures.insert(0, (key.clone(), module_source.to_owned()));
        zjsc::api::test262::run_module(&fixtures, key, global_script)
    } else {
        zjsc::api::test262::run_script(&source, key, &fixtures)
    };
    println!("THROWN {}", outcome.thrown.as_deref().map_or("-".to_owned(), |text| text.replace('\n', "\\n")));
    println!("--OUTPUT--");
    println!("{}", outcome.output);
}
