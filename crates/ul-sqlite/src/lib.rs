//! `sqlite3` do pseudo-linus: o CLI do SQLite 3.46.1 (shell.c portado) sobre o rusqlite ligado na
//! libsqlite3 do Debian 13, com o banco no FS do sandbox por um VFS nosso.
//!
//! - [`vfs`]: VFS sobre o `sysabi` (porte do `os_unix.c`, travas OFD do kernel).
//! - [`cli`]: o programa (argumentos, leitura de comandos, modos de saída, comandos de ponto).
//! - [`funcs`]: funções SQL do CLI e as trocadas pra não depender do host (data, random).
//! - [`unwind`]: `exit` e morte por sinal atravessando o código C do SQLite sem abortar o host.

pub mod cli;
pub mod funcs;
pub mod unwind;
pub mod vfs;

/// Os programas deste crate.
pub fn programs() -> Vec<sysabi::Program> {
    vec![sysabi::Program::bin("sqlite3", cli::main)]
}
