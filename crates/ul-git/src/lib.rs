//! `git` do pseudo-linus.
//!
//! Repositório, índice, refs e worktree vivem no FS do sandbox e são lidos e escritos só por
//! `sysabi`. O formato dos objetos, o pack, o índice, as refs e a configuração são nossos (ver os
//! módulos); o diff de linhas vem do `gix-imara-diff`. A meta é a saída do git 2.47.3 do Debian 13
//! byte a byte.

pub mod cmd;
pub mod config;
pub mod date;
pub mod diff;
pub mod editor;
pub mod error;
pub mod graph;
pub mod hash;
pub mod ident;
pub mod ignore;
pub mod index;
pub mod msg;
pub mod object;
pub mod odb;
pub mod opts;
pub mod os;
pub mod pathspec;
pub mod quote;
pub mod re;
pub mod refs;
pub mod repo;
pub mod rev;
pub mod store;
pub mod usage;
pub mod wildmatch;
pub mod worktree;

use std::ffi::OsString;

use sysabi::{Ctx, Program};

/// Os programas deste crate.
pub fn programs() -> Vec<Program> {
    vec![Program::bin("git", git_main)]
}

fn git_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    use std::os::unix::ffi::OsStrExt;
    let argv: Vec<Vec<u8>> = args.iter().map(|a| a.as_bytes().to_vec()).collect();
    let saved = os::swap_out(Vec::new());
    let code = cmd::main(&argv);
    os::flush_out();
    os::swap_out(saved);
    code
}
