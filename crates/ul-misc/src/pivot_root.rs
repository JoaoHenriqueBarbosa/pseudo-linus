//! `pivot_root` do util-linux 2.41: troca o sistema de arquivos raiz.
//!
//! Sem `CAP_SYS_ADMIN` efetivo (o caso de um contêiner sem privilégio) a syscall devolve EPERM antes
//! de olhar os caminhos, e é isso que este porte reproduz depois de validar os argumentos.

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

const USAGE: &str = "
Usage:
 pivot_root new_root put_old

Change the root filesystem.

Options:
 -h, --help     display this help
 -V, --version  display version

For more details see pivot_root(8).
";

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut g = Getopt::from_env(&argv[1..], "Vh", LONGS);
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
                let _ = out.write_all(USAGE.as_bytes());
                return 0;
            }
            _ => {
                ul::errtryhelp(&short);
                return 1;
            }
        }
    }
    let ops = g.operands();
    if ops.len() != 2 {
        ul::warnx(&short, "bad usage");
        ul::errtryhelp(&short);
        return 1;
    }
    ul::warn(
        &short,
        format!(
            "failed to change root from `{}' to `{}'",
            io::lossy(&ops[0]),
            io::lossy(&ops[1])
        ),
        Errno::EPERM,
    );
    1
}
