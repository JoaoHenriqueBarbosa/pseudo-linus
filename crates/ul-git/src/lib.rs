//! `git` do pseudo-linus.
//!
//! Repositório, índice, refs e worktree vivem no FS do sandbox e são lidos e escritos só por
//! `sysabi`. O formato dos objetos, o pack, o índice, as refs e a configuração são nossos (ver os
//! módulos); o diff de linhas vem do `gix-imara-diff`. A meta é a saída do git 2.47.3 do Debian 13
//! byte a byte.

pub mod config;
pub mod date;
pub mod error;
pub mod hash;
pub mod ident;
pub mod ignore;
pub mod index;
pub mod object;
pub mod odb;
pub mod os;
pub mod pathspec;
pub mod refs;
pub mod repo;
pub mod wildmatch;

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
    let code = run(&argv);
    os::flush_out();
    os::swap_out(saved);
    code
}

fn run(argv: &[Vec<u8>]) -> i32 {
    match argv.get(1) {
        None => {
            os::outs("usage: git [-v | --version] [-h | --help] [-C <path>] [-c <name>=<value>]\n");
            1
        }
        Some(sub) => {
            os::err_line("", &format!("git: '{}' is not a git command. See 'git --help'.", os::lossy(sub)));
            1
        }
    }
}
