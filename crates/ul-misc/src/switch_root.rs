//! `switch_root` do util-linux 2.41: troca a raiz do initramfs pela raiz real e executa o init.
//!
//! Só funciona como PID 1; fora disso o original recusa logo depois de validar os argumentos, e é
//! o que este porte faz. Opções são lidas só até o primeiro operando.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Errno};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const LONGS: &[LongOpt] = &[
    LongOpt::new("version", HasArg::No, b'V' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
];

fn usage(short: &str) -> String {
    format!(
        "
Usage:
 {short} [options] <newrootdir> <init> <args to init>

Switch to another filesystem as the root of the mount tree.

Options:
 -h, --help     display this help
 -V, --version  display version

For more details see switch_root(8).
"
    )
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut g = Getopt::from_env(&argv[1..], "+Vh", LONGS);
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                ul::errtryhelp(&short);
                return 1;
            }
        };
        match o.short() {
            Some('V') => {
                ul::print_version(&short);
                return 0;
            }
            Some('h') => {
                let mut out = io::stdout();
                let _ = out.write_all(usage(&short).as_bytes());
                return 0;
            }
            _ => {
                ul::errtryhelp(&short);
                return 1;
            }
        }
    }
    let ops = g.operands();
    if ops.len() < 2 || ops[0].is_empty() || ops[1].is_empty() {
        ul::warnx(&short, "not enough arguments");
        ul::errtryhelp(&short);
        return 1;
    }
    ul::warn(
        &short,
        format!("failed to mount moving {} to /", io::lossy(&ops[0])),
        Errno::EPERM,
    );
    ul::warnx(&short, "failed. Sorry.");
    1
}
