//! A linha de comando do `bun` mínima: `bun -e CÓDIGO`, `bun run ARQUIVO [ARGS...]`, `bun ARQUIVO [ARGS...]` e
//! `bun --version`. O motor é o zjsc; o que ele lê do sistema vem do pseudo-processo (`host.rs`, `console.rs`).

use std::ffi::OsString;
use std::rc::Rc;
use std::time::Duration;

use sysabi::{sys, Ctx, Fd, KillTarget, Signal};
use zjsc::api::eval::{evaluate_main_script_reporting_signal, set_main_module_fs};
use zjsc::runtime::host;
use zjsc::runtime::process_system::set_script_arguments;
use zjsc::runtime::vm::VM;

use crate::console::{FdConsole, SandboxFs};
use crate::host::SysabiHost;
use crate::transpiler::BunTranspiler;
use zjsc::api::source_transpiler::install_source_transpiler;

/// A versão do bun que o zjsc reproduz.
const VERSION: &str = "1.4.2";

/// A pilha da thread do motor (o interpretador recursa no C do bun, que roda em 8 MiB de pilha nativa com a CLoopStack
/// de 1 MiB; aqui a pilha nativa de cada nível é maior, então a thread leva uma grande, reservada e não comprometida).
const STACK_BYTES: usize = 256 * 1024 * 1024;

/// Folga para os protetores nativos, fora do orçamento do VM.
const STACK_GUARD_BYTES: usize = 16 * 1024 * 1024;

enum Job {
    Eval(String),
    File(String, Vec<String>),
}

fn fail(message: &str) -> i32 {
    let _ = sys::write_all(Fd::STDERR, format!("error: {message}\n").as_bytes());
    1
}

fn parse(args: &[String]) -> Result<Option<Job>, String> {
    let Some(first) = args.first() else { return Err(String::from("no script or command given")) };
    match first.as_str() {
        "--version" | "-v" => Ok(None),
        "-e" | "--eval" => match args.get(1) {
            Some(code) => Ok(Some(Job::Eval(code.clone()))),
            None => Err(String::from("Missing code for -e")),
        },
        "run" => match args.get(1) {
            Some(file) => Ok(Some(Job::File(file.clone(), args[2..].to_vec()))),
            None => Err(String::from("Missing script to run")),
        },
        file if !file.starts_with('-') => Ok(Some(Job::File(file.to_string(), args[1..].to_vec()))),
        option => Err(format!("Unknown option: {option}")),
    }
}

/// `file` resolvido no diretório de trabalho do processo.
fn absolute(file: &str) -> String {
    if file.starts_with('/') {
        return file.to_string();
    }
    let cwd = String::from_utf8_lossy(&sys::current().getcwd().unwrap_or_else(|_| b"/".to_vec())).into_owned();
    format!("{}/{}", cwd.trim_end_matches('/'), file.trim_start_matches("./"))
}

/// Roda o programa na thread do motor, com o host, o console e o sistema de arquivos do sandbox instalados nela.
fn run_program(source: String, url: String, arguments: Vec<String>) -> (i32, bool, Option<i32>) {
    ul_common::thread::run_with_stack(STACK_BYTES, move || {
        host::install(Rc::new(SysabiHost));
        install_source_transpiler(Some(Rc::new(BunTranspiler)));
        set_main_module_fs(Some(Rc::new(SandboxFs)));
        VM::set_thread_stack_budget(STACK_BYTES - STACK_GUARD_BYTES);
        let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
        set_script_arguments(&arguments);
        evaluate_main_script_reporting_signal(&source, &url, Rc::new(FdConsole::default()))
    })
    .unwrap_or((70, false, None))
}

pub fn bun_main(_ctx: &mut Ctx, argv: &[OsString]) -> i32 {
    let args: Vec<String> = argv.iter().skip(1).map(|arg| arg.to_string_lossy().into_owned()).collect();
    let job = match parse(&args) {
        Ok(Some(job)) => job,
        Ok(None) => {
            let _ = sys::write_all(Fd::STDOUT, format!("{VERSION}\n").as_bytes());
            return 0;
        }
        Err(message) => return fail(&message),
    };
    let (source, url, arguments) = match job {
        Job::Eval(code) => (code, absolute("[eval]"), Vec::new()),
        Job::File(file, arguments) => {
            let path = absolute(&file);
            match sysio::fs::read(&path).map(String::from_utf8) {
                Ok(Ok(source)) => (source, path, arguments),
                Ok(Err(_)) => return fail(&format!("Cannot read non-UTF-8 file \"{file}\"")),
                Err(_) => return fail(&format!("Module not found \"{file}\"")),
            }
        }
    };
    let (code, held, signal) = run_program(source, url, arguments);
    if let Some(number) = signal {
        // Morte por sinal (`process.kill(process.pid)` sem ouvinte, `process.abort()`): o processo morre pelo próprio sinal.
        let current = sys::current();
        let _ = current.kill(KillTarget::Pid(current.getpid()), Signal(number));
    }
    // Uma fonte `ref` sem trabalho pendente segura o processo vivo até um sinal; o sono entrega os sinais.
    while held {
        ul_common::time::sleep(Duration::from_secs(1));
    }
    code
}
