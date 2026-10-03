//! Tabela de programas do "pseudo-kernel" da bancada: cada nome aponta pro `uumain` portado (ou pro
//! `find_main`/`xargs_main` do findutils portado), e cada processo roda numa thread própria (modelo
//! A do design), com o contexto do shim instalado.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::Path;
use std::sync::{Arc, Mutex};

use harness::{Bytes, Invocation, Outcome};
use sysio::proc::{Executor, Proc, Spawn};
use sysio::process::RunResult;

use crate::sandbox::Sandbox;

/// Programa da tabela: recebe argv (com argv[0]) e devolve o código de saída.
pub type Main = fn(Vec<OsString>) -> i32;

/// Programas da tabela: os portados e os auxiliares escritos à mão pra bancada.
pub fn program(name: &str) -> Option<Main> {
    Some(match name {
        "cat" => run_cat,
        "head" => run_head,
        "wc" => run_wc,
        "sort" => run_sort,
        "ls" => run_ls,
        "find" => run_find,
        "xargs" => run_xargs,
        "echo" => crate::builtins::echo,
        "true" => |_| 0,
        "false" => |_| 1,
        _ => return None,
    })
}

/// Antes do `uumain`, o que a macro `uucore::bin!` fazia: localização do utilitário.
fn with_locale(util: &str, args: Vec<OsString>, main: fn(std::vec::IntoIter<OsString>) -> i32) -> i32 {
    if let Err(err) = uucore::locale::setup_localization(util) {
        sysio::eprintln!("Could not init the localization system: {err}");
        return 99;
    }
    main(args.into_iter())
}

fn run_cat(args: Vec<OsString>) -> i32 {
    with_locale("cat", args, uu_cat::uumain)
}

fn run_head(args: Vec<OsString>) -> i32 {
    with_locale("head", args, uu_head::uumain)
}

fn run_wc(args: Vec<OsString>) -> i32 {
    with_locale("wc", args, uu_wc::uumain)
}

fn run_sort(args: Vec<OsString>) -> i32 {
    with_locale("sort", args, uu_sort::uumain)
}

fn run_ls(args: Vec<OsString>) -> i32 {
    with_locale("ls", args, uu_ls::uumain)
}

/// O mesmo que `findutils/src/find/main.rs`, sem `std::env::args` nem `std::process::exit`.
fn run_find(args: Vec<OsString>) -> i32 {
    let args: Vec<String> = args.iter().map(|a| a.to_string_lossy().into_owned()).collect();
    let strs: Vec<&str> = args.iter().map(String::as_str).collect();
    let deps = findutils::find::StandardDependencies::new();
    findutils::find::find_main(&strs, &deps)
}

/// O mesmo que `findutils/src/xargs/main.rs`.
fn run_xargs(args: Vec<OsString>) -> i32 {
    let args: Vec<String> = args.iter().map(|a| a.to_string_lossy().into_owned()).collect();
    let strs: Vec<&str> = args.iter().map(String::as_str).collect();
    findutils::xargs::xargs_main(&strs)
}

/// Executor do shim: procura o programa pelo nome e roda numa thread nova. Guarda os nomes que
/// não estão na tabela (pra separar "porte errado" de "comando que a bancada não tem").
#[derive(Default)]
pub struct Table {
    pub missing: Mutex<BTreeSet<String>>,
}

impl Executor for Table {
    fn exec(&self, argv: &[OsString], stdin: Vec<u8>, cwd: Option<&Path>) -> std::io::Result<i32> {
        let name = argv.first().map(|a| a.to_string_lossy().into_owned()).unwrap_or_default();
        let base = name.rsplit('/').next().unwrap_or(&name).to_string();
        let Some(main) = program(&base) else {
            sysio::proc::lock(&self.missing).insert(base);
            return Err(std::io::Error::from_raw_os_error(sysio::errno::ENOENT));
        };
        let child = sysio::process::child_of_current(argv, stdin, cwd)?;
        Ok(match run_in_thread(child, main, argv.to_vec()) {
            RunResult::Exited(code) => code,
            RunResult::Panicked(_) => 134,
        })
    }
}

/// Roda `main` numa thread nova com `proc` como processo corrente (modelo A: thread = processo).
pub fn run_in_thread(proc: Proc, main: Main, argv: Vec<OsString>) -> RunResult {
    let handle = std::thread::Builder::new()
        .name(format!("pseudo:{}", argv.first().map(|a| a.to_string_lossy()).unwrap_or_default()))
        .stack_size(8 << 20)
        .spawn(move || sysio::process::run_main(proc, move || main(argv)))
        .expect("criar thread do pseudo-processo");
    handle.join().unwrap_or_else(|_| RunResult::Panicked("thread do pseudo-processo caiu".into()))
}

/// Um processo raiz (ou um estágio de pipeline) a rodar sobre o sandbox do caso.
pub struct Root<'a> {
    pub sandbox: &'a Sandbox,
    pub inv: &'a Invocation,
    pub table: Arc<Table>,
    pub stderr: Arc<Mutex<Vec<u8>>>,
}

impl Root<'_> {
    /// Roda `argv` com a entrada dada e devolve (stdout, resultado).
    pub fn run(&self, argv: &[String], stdin: Vec<u8>) -> Result<(Vec<u8>, RunResult), String> {
        let name = argv.first().ok_or("argv vazio")?;
        let Some(main) = program(name) else {
            return Err(format!("{name} não está na tabela de programas da bancada"));
        };
        let stdout = Arc::new(Mutex::new(Vec::new()));
        let args: Vec<OsString> = argv.iter().map(OsString::from).collect();
        let env: BTreeMap<OsString, OsString> =
            self.inv.full_env().into_iter().map(|(k, v)| (OsString::from(k), OsString::from(v))).collect();
        let proc = Spawn {
            vfs: Arc::clone(&self.sandbox.vfs),
            cwd: self.sandbox.case_dir.clone(),
            args: args.clone(),
            env,
            stdin,
            stdout: Arc::clone(&stdout),
            stderr: Arc::clone(&self.stderr),
            now: self.sandbox.now,
            executor: Some(Arc::clone(&self.table) as Arc<dyn Executor>),
        }
        .build();
        let result = run_in_thread(proc, main, args);
        let out = std::mem::take(&mut *sysio::proc::lock(&stdout));
        Ok((out, result))
    }
}

/// Monta o sandbox do caso, roda `argv` como processo raiz e devolve o `Outcome` da bancada.
pub fn run_argv(inv: &Invocation, argv: &[String]) -> Outcome {
    let sandbox = Sandbox::for_case(&inv.files, inv.faketime.as_deref());
    let root = Root {
        sandbox: &sandbox,
        inv,
        table: Arc::new(Table::default()),
        stderr: Arc::new(Mutex::new(Vec::new())),
    };
    match root.run(argv, inv.stdin.clone()) {
        Err(why) => Outcome::unsupported(why),
        Ok((stdout, result)) => finish(&root, stdout, result),
    }
}

/// Junta saída, código e retrato do FS; comando ausente da tabela vira "unsupported".
pub fn finish(root: &Root<'_>, stdout: Vec<u8>, result: RunResult) -> Outcome {
    let missing: Vec<String> = sysio::proc::lock(&root.table.missing)
        .iter()
        .filter(|m| !m.starts_with("nope"))
        .cloned()
        .collect();
    if !missing.is_empty() {
        return Outcome::unsupported(format!("comando fora da tabela da bancada: {}", missing.join(", ")));
    }
    match result {
        RunResult::Exited(code) => Outcome {
            stdout: Bytes(stdout),
            stderr: Bytes(std::mem::take(&mut *sysio::proc::lock(&root.stderr))),
            exit: Some(code),
            files: root.sandbox.snapshot(),
            ..Outcome::default()
        },
        RunResult::Panicked(msg) => Outcome::unsupported(format!("panic no porte: {msg}")),
    }
}
