//! `clear_console` do console-tools/kbd do Debian.
//!
//! Limpa o console virtual escrevendo as sequências de limpeza no terminal. Como as demais
//! ferramentas do kbd, localiza o console por `getfd`: sem nenhum descritor que seja um terminal,
//! falha com a mensagem padrão do kbd e código 1.

use std::ffi::OsString;

use sysabi::{Fd, sys};

use crate::util::io;

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(_args: &[OsString]) -> i32 {
    let s = sys::current();
    let fd = [Fd::STDIN, Fd::STDOUT, Fd::STDERR].into_iter().find(|f| s.isatty(*f));
    let Some(fd) = fd else {
        io::eprint("Couldn't get a file descriptor referring to the console.\n");
        return 1;
    };
    if sys::write_all(fd, b"\x1b[H\x1b[J").is_err() {
        return 1;
    }
    0
}
