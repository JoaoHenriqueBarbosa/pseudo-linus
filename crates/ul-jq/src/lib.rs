//! ul-jq: `jq` (jq 1.7.1 do Debian 13) e `yq` (yq 3.4.3 do kislyuk) do pseudo-linus.
//!
//! O `jq` roda sobre o fork localizado do jaq em `vendor/` (núcleo, valores e biblioteca padrão) com a
//! camada nossa por cima: CLI do `main.c` do jq 1.7.1, leitura e impressão de JSON byte a byte iguais,
//! mensagens de erro, regex do Oniguruma (ferroni), datas com relógio e fuso do sandbox, e checkpoint
//! do escalonador a cada nó avaliado. Todo I/O passa pelo `sysabi`.

pub mod cli;
pub mod engine;
pub mod input;
pub mod io;
pub mod syntax;
pub mod time;

use std::ffi::OsString;

use sysabi::{Ctx, Program, Signal};

/// Programas deste crate.
pub fn programs() -> Vec<Program> {
    vec![Program::bin("jq", jq_main)]
}

fn jq_main(ctx: &mut Ctx, argv: &[OsString]) -> i32 {
    cli::jq_main(ctx, argv)
}

/// Escreve `msg` no stderr e termina com SIGABRT, como o `abort()` do jq (o stdout com buffer não
/// é descarregado).
pub fn abort_with(msg: &str) -> ! {
    io::stderr(msg.as_bytes());
    std::panic::resume_unwind(Box::new(sysabi::KillUnwind(Signal::SIGABRT)))
}

/// Roda `f`; se a avaliação acabar em falta de memória (`jaq_core::OutOfMemory`), faz o que o jq
/// faz no `memory_exhausted`: "jq: error: cannot allocate memory" e `abort()`. Outros desenrolares
/// (exit, sinais do kernel) seguem adiante.
pub fn catch_oom<R>(f: impl FnOnce() -> R) -> R {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(payload) => {
            if payload.downcast_ref::<jaq_core::OutOfMemory>().is_some() {
                abort_with("jq: error: cannot allocate memory\n");
            }
            std::panic::resume_unwind(payload)
        }
    }
}
