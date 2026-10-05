//! `login` do util-linux 2.41.
//!
//! Porte de `login-utils/login.c` até onde o sandbox permite: opções, `--help`/`--version` e a
//! checagem do terminal de controle. O `login` real exige um tty de verdade (`ttyname(3)`, `lstat` e
//! `access` no dispositivo); sem ele, sai com 1 sem imprimir nada (o `FATAL: bad tty` vai ao syslog). Ficam de fora
//! a autenticação por PAM, o utmp e o início da sessão, que dependem de um terminal.

use std::ffi::OsString;
use std::io::Write;

use sysabi::sys;

use crate::groupmgmt::{Spec, parse};
use crate::util::io;

const USAGE: &str = r#"
Usage:
 login [-p] [-h <host>] [-H] [[-f] <username>]

Begin a session on the system.

Options:
 -p             do not destroy the environment
 -f             skip a login authentication
 -h <host>      hostname to be used for utmp logging
 -H             suppress hostname in the login prompt
     --help     display this help
 -V, --version  display version

For more details see login(1).
"#;

const SPEC: Spec = &[
    (b'f', "", false),
    (b'H', "", false),
    (b'h', "", true),
    (b'p', "", false),
    (b'r', "", true),
    (1, "help", false),
    (b'V', "version", false),
];

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let Some(o) = parse("login", &argv, SPEC) else {
        io::eprint("Try 'login --help' for more information.\n");
        return 1;
    };
    for (k, _) in &o.vals {
        match *k {
            1 => {
                let _ = io::stdout().write_all(USAGE.as_bytes());
                return 0;
            }
            b'V' => {
                let _ = io::stdout().write_all(b"login from util-linux 2.41.5\n");
                return 0;
            }
            _ => {}
        }
    }
    let euid = sys::current().geteuid();
    if euid != 0 {
        if o.has(b'f') {
            io::eprint("login: -f is for superuser only\n");
            return 1;
        }
        if o.has(b'h') || o.has(b'r') {
            io::eprint("login: -h is for superuser only\n");
            return 1;
        }
    }
    // init_tty(): sem terminal de controle o login recusa antes de pedir qualquer coisa. O
    // `FATAL: bad tty` do original vai só para o syslog, então nada sai no stdout nem no stderr.
    1
}
