//! `findfs` do util-linux 2.41 (pacote util-linux do Debian 13): acha um sistema de arquivos por
//! rótulo ou UUID.
//!
//! Porte do `misc-utils/findfs.c`. O sandbox não tem dispositivos de bloco com superbloco legível nem
//! cache do blkid, então toda etiqueta bem formada (`LABEL=`, `UUID=`, `PARTLABEL=`, `PARTUUID=`)
//! termina em "unable to resolve" com saída 2, como acontece no oráculo, que roda em container sem
//! discos. Sem operando ou com operandos demais sai "bad usage" com a dica de ajuda (saída 1).
//! Ainda sem conferência de oráculo.

use std::ffi::OsString;
use std::io::Write;

use sysabi::Ctx;

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const LONGS: &[LongOpt] = &[
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
];

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn usage(short: &str) -> String {
    format!(
        "
Usage:
 {short} [options] NAME=value

Find a filesystem by label or UUID.

Options:
 -h, --help         display this help
 -V, --version      display version

For more details see findfs(8).
"
    )
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut g = Getopt::from_env(&argv[1..], "hV", LONGS);
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
            Some('h') => {
                let mut out = io::stdout();
                let _ = out.write_all(usage(&short).as_bytes());
                return 0;
            }
            Some('V') => {
                ul::print_version(&short);
                return 0;
            }
            _ => {
                ul::errtryhelp(&short);
                return 1;
            }
        }
    }

    let rest = g.operands();
    if rest.len() != 1 {
        io::eprint(format!("{short}: bad usage\n"));
        ul::errtryhelp(&short);
        return 1;
    }
    let tag = String::from_utf8_lossy(&rest[0]).into_owned();
    io::eprint(format!("{short}: unable to resolve '{tag}'\n"));
    2
}
