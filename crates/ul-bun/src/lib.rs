//! ul-bun: o `bun` (1.4.2) do pseudo-linus. O motor é o zjsc (JavaScriptCore do WebKit do Bun traduzido para Rust, sem
//! unsafe), ligado ao sandbox por três fronteiras: o `Host` (arquivos de `/proc`, ambiente, pid, entropia, sinais), o
//! `ConsoleHost` (os fds 0, 1 e 2) e o `ModuleFs` (o VFS do processo). Nada do host real chega ao motor.

pub mod cli;
pub mod console;
pub mod host;

use sysabi::Program;

/// Programas deste crate.
pub fn programs() -> Vec<Program> {
    vec![Program::bin("bun", cli::bun_main)]
}

#[cfg(test)]
mod tests;
