//! `peekfd` do psmisc 23.7: espia as leituras e escritas de um processo em descritores de arquivo.
//!
//! Diferenças conhecidas: o `peekfd` original usa `ptrace(2)`, que o sysabi ainda não expõe; por
//! isso a análise de opções, o uso, a versão e as verificações do pid existem, mas o rastreamento
//! termina com o erro de `ptrace` não suportado. Os textos foram reproduzidos de memória do
//! `peekfd.c`, sem conferência de oráculo.

use std::ffi::OsString;

use sysabi::{Ctx, Errno, KillTarget, Signal, sys};
use ul_misc::util::io;

const USAGE: &str = "Usage: peekfd [-8] [-n] [-c] [-d] [-V] [-h] <pid> [<fd> ...]\n    -8 output 8 bit clean streams.\n    -n don't display read/write from fd headers.\n    -c peek at any new child processes too.\n    -d remove duplicate read/writes from the output.\n    -V prints version info.\n    -h prints this help.\n  Press ctrl-C to end output.\n";
const VERSION: &str = "peekfd (PSmisc) 23.7\nCopyright (C) 2007 Trent Waddington\n\nPSmisc comes with ABSOLUTELY NO WARRANTY.\nThis is free software, and you are welcome to redistribute it under\nthe terms of the GNU General Public License.\nFor more information about these matters, see the files named COPYING.\n";

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let mut i = 1;
    while i < argv.len() {
        let a = &argv[i];
        if a.len() < 2 || a[0] != b'-' {
            break;
        }
        for &c in &a[1..] {
            match c {
                b'8' | b'n' | b'c' | b'd' => {}
                b'V' => {
                    io::eprint(VERSION);
                    return 0;
                }
                b'h' => {
                    io::eprint(USAGE);
                    return 0;
                }
                _ => {
                    io::eprint(USAGE);
                    return 1;
                }
            }
        }
        i += 1;
    }
    if i >= argv.len() {
        io::eprint(USAGE);
        return 1;
    }
    let pid: i32 = match String::from_utf8_lossy(&argv[i]).parse() {
        Ok(p) if p > 0 => p,
        _ => {
            io::eprint(USAGE);
            return 1;
        }
    };
    for fd in &argv[i + 1..] {
        if String::from_utf8_lossy(fd).parse::<i32>().is_err() {
            io::eprint(USAGE);
            return 1;
        }
    }
    if sys::current().kill(KillTarget::Pid(pid), Signal(0)) == Err(Errno::ESRCH) {
        io::eprint(format!("peekfd: attach: {}\n", Errno::ESRCH.message()));
        return 1;
    }
    io::eprint(format!("peekfd: ptrace: {}\n", Errno::ENOSYS.message()));
    1
}
