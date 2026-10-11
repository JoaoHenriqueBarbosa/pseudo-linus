//! ul-bun: o `bun` (1.4.2) do pseudo-linus. O motor é o zjsc (JavaScriptCore do WebKit do Bun traduzido para Rust, sem
//! unsafe), ligado ao sandbox por três fronteiras: o `Host` (arquivos de `/proc`, ambiente, pid, entropia, sinais), o
//! `ConsoleHost` (os fds 0, 1 e 2) e o `ModuleFs` (o VFS do processo). Nada do host real chega ao motor.

pub mod cli;
pub mod console;
pub mod help;
pub mod host;
pub mod package_json;
pub mod run_script;
pub mod transpiler;

use sysabi::Program;

/// Programas deste crate.
pub fn programs() -> Vec<Program> {
    // O bun 1.4.2 do oráculo vive em `/usr/local/bin` (é o `execPath` do motor e o alvo dos links do `PATH` dos scripts).
    let dir = zjsc::runtime::process_shape::EXEC_PATH.rsplit_once('/').map_or("/usr/local/bin", |(dir, _)| dir);
    vec![Program { name: "bun", dir, main: cli::bun_main }]
}

#[cfg(test)]
mod tests;
