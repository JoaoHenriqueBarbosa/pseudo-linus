//! Término e execução de programas.
//!
//! `exit` não pode ser `std::process::exit` (mataria o host com todos os pseudo-processos): vira um
//! unwind com [`ExitRequest`], que quem executou o programa transforma em código de saída. `spawn`
//! despacha pra tabela de programas do pseudo-kernel ([`crate::proc::Executor`]), sem fork nem exec.

use std::ffi::OsString;
use std::io;
use std::path::Path;
use std::sync::Arc;

pub use crate::proc::ExitRequest;
use crate::proc::{self, Spawn, lock};

/// `exit(2)` do pseudo-processo.
pub fn exit(code: i32) -> ! {
    std::panic::resume_unwind(Box::new(ExitRequest(code)))
}

/// Roda `argv` como processo filho: mesmo VFS, mesmo stdout/stderr, ambiente e cwd copiados.
/// Devolve o código de saída ou o erro de exec (ENOENT se o programa não existe).
pub fn spawn(argv: &[OsString], stdin: Vec<u8>) -> io::Result<i32> {
    spawn_in(argv, stdin, None)
}

/// Como [`spawn`], com o diretório de trabalho do filho trocado.
pub fn spawn_in(argv: &[OsString], stdin: Vec<u8>, cwd: Option<&Path>) -> io::Result<i32> {
    let parent = proc::current();
    let exec = parent
        .executor
        .clone()
        .ok_or_else(|| io::Error::new(io::ErrorKind::Unsupported, "sysio: processo sem executor"))?;
    exec.exec(argv, stdin, cwd)
}

/// Monta o processo filho a partir do corrente (o "fork" sem cópia de memória). Um `cwd` relativo
/// é resolvido contra o do pai.
pub fn child_of_current(argv: &[OsString], stdin: Vec<u8>, cwd: Option<&Path>) -> io::Result<proc::Proc> {
    let parent = proc::current();
    let cwd = match cwd {
        Some(dir) => {
            let abs = crate::fs::canonicalize(dir)?;
            if !crate::fs::metadata(&abs)?.is_dir() {
                return Err(crate::errno::err(crate::errno::ENOTDIR));
            }
            abs
        }
        None => lock(&parent.cwd).clone(),
    };
    Ok(Spawn {
        vfs: Arc::clone(&parent.vfs),
        cwd,
        args: argv.to_vec(),
        env: lock(&parent.env).clone(),
        stdin,
        stdout: Arc::clone(&parent.stdout),
        stderr: Arc::clone(&parent.stderr),
        now: parent.now,
        executor: parent.executor.clone(),
    }
    .build())
}

/// Executa `main` no processo `p` (nesta thread) e devolve o código de saída, tratando
/// `ExitRequest` como saída normal e panic como sinal (134, como um abort).
pub fn run_main(p: proc::Proc, main: impl FnOnce() -> i32) -> RunResult {
    let result = proc::enter(p, || std::panic::catch_unwind(std::panic::AssertUnwindSafe(main)));
    match result {
        Ok(code) => RunResult::Exited(code),
        Err(payload) => {
            if let Some(ExitRequest(code)) = payload.downcast_ref::<ExitRequest>() {
                RunResult::Exited(*code)
            } else {
                let msg = payload
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "panic sem mensagem".into());
                RunResult::Panicked(msg)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunResult {
    Exited(i32),
    Panicked(String),
}
