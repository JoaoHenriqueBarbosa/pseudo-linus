//! `awk` e `gawk` do pseudo-linus: interpretador próprio com o gawk 5.2.1 como alvo.
//!
//! Todo I/O passa por `sysabi`; nada aqui toca o host.
//!
//! - [`lexer`], [`parser`], [`ast`]: o front-end, com as mensagens de erro do gawk.
//! - [`interp`]: o interpretador de árvore (variáveis especiais, registros, saídas, funções).
//! - [`fields`], [`io`], [`builtins`], [`sort`]: divisão de campos, E/S, funções embutidas, ordens.
//! - [`array`], [`format`], [`regex`], [`time`]: arrays com a ordem do gawk, `printf`, regex ERE
//!   leftmost-longest e tempo.

pub mod array;
pub mod ast;
pub mod builtins;
pub mod cli;
pub mod fields;
pub mod format;
pub mod interp;
pub mod io;
pub mod lexer;
pub mod mawk;
pub mod parser;
pub mod regex;
pub mod sort;
pub mod time;
pub mod value;

use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;

use sysabi::{Ctx, Program};

/// Os programas deste crate.
pub fn programs() -> Vec<Program> {
    vec![Program::bin("awk", awk_main), Program::bin("gawk", awk_main), Program::bin("mawk", mawk_main)]
}

fn mawk_main(ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let argv: Vec<Vec<u8>> = args.iter().map(|a| a.as_bytes().to_vec()).collect();
    let sys = ctx.sys().clone();
    mawk::run(sys, argv)
}

fn awk_main(ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let prog = ctx.prog.clone();
    let argv: Vec<Vec<u8>> = args.iter().map(|a| a.as_bytes().to_vec()).collect();
    let sys = ctx.sys().clone();
    cli::run(sys, &prog, argv)
}
