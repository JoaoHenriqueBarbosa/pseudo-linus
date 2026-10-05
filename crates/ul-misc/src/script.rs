//! `script` do util-linux 2.41: grava um terminal numa sessão com typescript.
//!
//! Este porte faz o parse de opções, as validações e as mensagens de erro do original. A gravação em
//! si precisa de um pseudo-terminal (`openpty`), que o `sysabi` ainda não expõe; sem ele o programa
//! termina depois de validar tudo com a mesma falha que o original teria sem `/dev/ptmx`.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Errno};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const O_FORCE: i32 = 256;

const LONGS: &[LongOpt] = &[
    LongOpt::new("append", HasArg::No, b'a' as i32),
    LongOpt::new("command", HasArg::Required, b'c' as i32),
    LongOpt::new("echo", HasArg::Required, b'E' as i32),
    LongOpt::new("return", HasArg::No, b'e' as i32),
    LongOpt::new("flush", HasArg::No, b'f' as i32),
    LongOpt::new("force", HasArg::No, O_FORCE),
    LongOpt::new("log-in", HasArg::Required, b'I' as i32),
    LongOpt::new("log-out", HasArg::Required, b'O' as i32),
    LongOpt::new("log-io", HasArg::Required, b'B' as i32),
    LongOpt::new("log-timing", HasArg::Required, b'T' as i32),
    LongOpt::new("logging-format", HasArg::Required, b'm' as i32),
    LongOpt::new("output-limit", HasArg::Required, b'o' as i32),
    LongOpt::new("quiet", HasArg::No, b'q' as i32),
    LongOpt::new("timing", HasArg::Optional, b't' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
];

const USAGE: &str = "
Usage:
 script [options] [file]

Make a typescript of a terminal session.

Options:
 -I, --log-in <file>           log stdin to file
 -O, --log-out <file>          log stdout to file (default)
 -B, --log-io <file>           log stdin and stdout to file

 -T, --log-timing <file>       log timing information to file
 -t[<file>], --timing[=<file>] deprecated alias to -T (default file is stderr)
 -m, --logging-format <name>   force to 'classic' or 'advanced' format

 -a, --append                  append to the log file
 -c, --command <command>       run command rather than interactive shell
 -e, --return                  return exit code of the child process
 -f, --flush                   run flush after each write
     --force                   use output file even when it is a link
 -E, --echo <when>             echo input in session (auto, always or never)
 -o, --output-limit <size>     terminate if output files exceed size
 -q, --quiet                   be quiet

 -h, --help                    display this help
 -V, --version                 display version

For more details see script(1).
";

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut outfile_set = false;
    let mut g = Getopt::from_env(&argv[1..], "aB:c:E:efI:O:o:qm:T:t::Vh", LONGS);
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                ul::errtryhelp(&short);
                return 1;
            }
        };
        let arg = o.arg.clone().unwrap_or_default();
        if o.id == O_FORCE {
            continue;
        }
        match o.short() {
            Some('a') | Some('c') | Some('e') | Some('f') | Some('q') | Some('B') | Some('I')
            | Some('O') | Some('T') | Some('t') => {
                if matches!(o.short(), Some('B') | Some('I') | Some('O')) {
                    outfile_set = true;
                }
            }
            Some('E') => {
                if ![b"auto".as_slice(), b"always", b"never"].contains(&arg.as_slice()) {
                    ul::warnx(
                        &short,
                        format!("unssuported echo mode: '{}'", io::lossy(&arg)),
                    );
                    return 1;
                }
            }
            Some('m') => {
                if arg != b"classic" && arg != b"advanced" {
                    ul::warnx(
                        &short,
                        format!("unsupported logging format: '{}'", io::lossy(&arg)),
                    );
                    return 1;
                }
            }
            Some('o') => {
                if let Err(m) = ul::strtosize_or_err(&arg, "failed to parse output limit size") {
                    ul::warnx(&short, m);
                    return 1;
                }
            }
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
    if ops.len() > 1 || (ops.len() == 1 && outfile_set) {
        ul::warnx(&short, "unexpected number of arguments");
        ul::errtryhelp(&short);
        return 1;
    }
    ul::warn(&short, "failed to create pseudo-terminal", Errno::ENOENT);
    1
}
