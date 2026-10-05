//! `fstab-decode` do pacote initscripts do Debian 13: roda um programa com os argumentos
//! decodificados das sequências de escape do `fstab` (`\040` vira espaço, `\\` vira barra).
//!
//! Uso: `fstab-decode COMMAND [ARG...]`. Cada argumento passa por `decode`; o programa é procurado
//! no `PATH` como no `execvp(3)`. Falha no exec sai com 127 (não achou) ou 126 (outro erro).

use std::ffi::OsString;

use sysabi::{Ctx, Errno};

use crate::setsid::execvp;
use crate::util::io;
use crate::util::ul;

/// Decodifica `\NNN` (três dígitos octais) e `\\`; qualquer outra barra fica como está.
pub fn decode(arg: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(arg.len());
    let mut i = 0;
    while i < arg.len() {
        let b = arg[i];
        if b == b'\\' {
            if i + 3 < arg.len() && arg[i + 1..i + 4].iter().all(|c| (b'0'..=b'7').contains(c)) {
                let v = u32::from(arg[i + 1] - b'0') * 64
                    + u32::from(arg[i + 2] - b'0') * 8
                    + u32::from(arg[i + 3] - b'0');
                out.push((v & 0xff) as u8);
                i += 4;
                continue;
            }
            if i + 1 < arg.len() && arg[i + 1] == b'\\' {
                out.push(b'\\');
                i += 2;
                continue;
            }
        }
        out.push(b);
        i += 1;
    }
    out
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    if argv.len() < 2 {
        io::eprint("Usage: fstab-decode command [arguments]\n".to_string());
        return 1;
    }
    let cmd: Vec<Vec<u8>> = argv[1..].iter().map(|a| decode(a)).collect();
    let _ = io::flush_stdout();
    let e = execvp(&cmd[0], &cmd);
    ul::warn("fstab-decode", io::lossy(&cmd[0]), e);
    if e == Errno::ENOENT { 127 } else { 126 }
}
