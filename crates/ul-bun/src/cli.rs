//! A linha de comando do `bun` mínima: `bun -e CÓDIGO`, `bun run ARQUIVO [ARGS...]`, `bun ARQUIVO [ARGS...]` e
//! `bun --version`. O motor é o zjsc; o que ele lê do sistema vem do pseudo-processo (`host.rs`, `console.rs`).

use std::ffi::OsString;
use std::rc::Rc;
use std::time::Duration;

use sysabi::{sys, Ctx, Fd, KillTarget, Signal};
use zjsc::api::eval::{evaluate_main_script_reporting_signal, set_main_module_fs};
use zjsc::runtime::host;
use zjsc::runtime::process_shape::{BUN_REVISION, BUN_VERSION};
use zjsc::runtime::process_system::set_script_arguments;
use zjsc::runtime::vm::VM;

use crate::console::{FdConsole, SandboxFs};
use crate::help;
use crate::package_json;
use crate::run_script;
use crate::host::SysabiHost;
use crate::transpiler::BunTranspiler;
use zjsc::api::source_transpiler::install_source_transpiler;

/// A pilha da thread do motor (o interpretador recursa no C do bun, que roda em 8 MiB de pilha nativa com a CLoopStack
/// de 1 MiB; aqui a pilha nativa de cada nível é maior, então a thread leva uma grande, reservada e não comprometida).
const STACK_BYTES: usize = 256 * 1024 * 1024;

/// Folga para os protetores nativos, fora do orçamento do VM.
const STACK_GUARD_BYTES: usize = 16 * 1024 * 1024;

enum Job {
    Eval(String, Vec<String>),
    /// `bun run NOME ARGS...` ou `bun NOME ARGS...`: um script do `package.json` ou um arquivo.
    Named { name: String, arguments: Vec<String>, silent: bool, if_present: bool },
}

/// O que a linha de comando pede: escrever um texto e sair, ou rodar um programa.
enum Action {
    Output { stdout: String, stderr: String, code: i32 },
    Run(Job),
}

/// Um erro de linha de comando do bun: a ajuda curta no stdout e a mensagem no stderr, código 1.
fn usage_error(message: String) -> Action {
    Action::Output { stdout: help::top(false), stderr: format!("error: {message}\n"), code: 1 }
}

/// O que a varredura das opções achou antes do primeiro argumento posicional.
#[derive(Default)]
struct Scan {
    help: bool,
    version: bool,
    revision: bool,
    run: bool,
    eval: Option<String>,
    silent: bool,
    if_present: bool,
    invalid_short: Option<char>,
    missing_value: Option<String>,
    /// Índice do primeiro argumento posicional.
    positional: usize,
}

/// Varre as opções como o bun: valem até o primeiro argumento que não é opção (o resto é do script); opção longa
/// desconhecida é ignorada, opção curta desconhecida é erro (medido no oráculo, 1.4.2). O `run` solto antes do
/// primeiro posicional é o subcomando.
fn scan(args: &[String]) -> Scan {
    let table = help::flags();
    let mut found = Scan::default();
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        if arg == "run" && !found.run {
            found.run = true;
            index += 1;
            continue;
        }
        if let Some(long) = arg.strip_prefix("--").filter(|long| !long.is_empty()) {
            let (name, inline) = long.split_once('=').map_or((long, None), |(name, value)| (name, Some(value)));
            match name {
                "help" => found.help = true,
                "version" => found.version = true,
                "revision" => found.revision = true,
                "silent" => found.silent = true,
                "if-present" => found.if_present = true,
                _ => {}
            }
            let takes_value = table.iter().any(|flag| flag.long.as_deref() == Some(name) && flag.takes_value);
            let value = match (takes_value, inline) {
                (true, Some(value)) => Some(value.to_string()),
                (true, None) => {
                    index += 1;
                    let value = args.get(index).cloned();
                    if value.is_none() && found.missing_value.is_none() {
                        found.missing_value = Some(format!("--{name}"));
                    }
                    value
                }
                (false, _) => None,
            };
            if matches!(name, "eval" | "print") {
                found.eval = value.or(found.eval);
            }
        } else if arg.starts_with('-') && arg.len() > 1 {
            let letters: Vec<char> = arg[1..].chars().collect();
            for (position, letter) in letters.iter().copied().enumerate() {
                match letter {
                    'h' => found.help = true,
                    'v' => found.version = true,
                    _ => {}
                }
                let Some(flag) = table.iter().find(|flag| flag.short == Some(letter)) else {
                    found.invalid_short.get_or_insert(letter);
                    continue;
                };
                // `-pe CODE`: só a última letra do grupo consome o valor.
                if flag.takes_value && position + 1 == letters.len() {
                    index += 1;
                    let value = args.get(index).cloned();
                    if value.is_none() && found.missing_value.is_none() {
                        found.missing_value = Some(format!("-{letter}"));
                    }
                    if matches!(letter, 'e' | 'p') {
                        found.eval = value.or(found.eval);
                    }
                }
            }
        } else {
            break;
        }
        index += 1;
    }
    found.positional = index.min(args.len());
    found
}

fn parse(args: &[String]) -> Action {
    let found = scan(args);
    if let Some(letter) = found.invalid_short {
        return usage_error(format!("Invalid Argument '-{letter}'"));
    }
    if let Some(flag) = &found.missing_value {
        return usage_error(format!("The argument '{flag}' requires a value but none was supplied."));
    }
    let output = |stdout: String| Action::Output { stdout, stderr: String::new(), code: 0 };
    if found.help {
        return output(if found.run { help::RUN.to_string() } else { help::top(true) });
    }
    if found.version {
        return output(format!("{BUN_VERSION}\n"));
    }
    if found.revision {
        return output(format!("{BUN_VERSION}+{}\n", &BUN_REVISION[..9]));
    }
    let rest = &args[found.positional..];
    if found.run && rest.is_empty() {
        let package = sysio::env::current_dir().ok().and_then(|dir| package_json::find(&dir.to_string_lossy()));
        return output(run_script::listing(package.as_ref()));
    }
    if let (false, Some(code)) = (found.run, found.eval) {
        return Action::Run(Job::Eval(code, rest.to_vec()));
    }
    match rest.split_first() {
        Some((name, arguments)) => Action::Run(Job::Named { name: name.clone(), arguments: arguments.to_vec(), silent: found.silent, if_present: found.if_present }),
        None => output(help::top(false)),
    }
}

/// O nome é de arquivo (tem caminho ou a extensão de um loader do bun), não de script.
fn looks_like_file(name: &str) -> bool {
    name.contains('/') || bun_transpile::loader_for(name).is_some()
}

fn fail(message: &str) -> i32 {
    let _ = sys::write_all(Fd::STDERR, format!("error: {message}\n").as_bytes());
    1
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
        Action::Run(job) => job,
        Action::Output { stdout, stderr, code } => {
            let _ = sys::write_all(Fd::STDOUT, stdout.as_bytes());
            let _ = sys::write_all(Fd::STDERR, stderr.as_bytes());
            return code;
        }
    };
    let (source, url, arguments) = match job {
        Job::Eval(code, arguments) => (code, absolute("[eval]"), arguments),
        Job::Named { name, arguments, silent, if_present } => {
            // Um script do package.json vence um arquivo de mesmo nome (medido).
            let package = sysio::env::current_dir().ok().and_then(|dir| package_json::find(&dir.to_string_lossy()));
            if let Some(package) = package.as_ref().filter(|package| package.script(&name).is_some()) {
                return run_script::run(package, &name, &arguments, silent);
            }
            let path = absolute(&name);
            if !looks_like_file(&name) && !sysio::fs::exists(&path) {
                return if if_present { 0 } else { fail(&format!("Script not found \"{name}\"")) };
            }
            match sysio::fs::read(&path).map(String::from_utf8) {
                Ok(Ok(source)) => (source, path, arguments),
                Ok(Err(_)) => return fail(&format!("Cannot read non-UTF-8 file \"{name}\"")),
                Err(_) if if_present => return 0,
                Err(_) => return fail(&format!("Module not found \"{name}\"")),
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
