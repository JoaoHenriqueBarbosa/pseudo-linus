//! `scriptlive` do util-linux 2.41: reexecuta um typescript no terminal, usando os tempos gravados.
//!
//! Este porte faz o parse de opções e as validações do original. Reexecutar a sessão exige um
//! pseudo-terminal, que o `sysabi` ainda não expõe, então depois de validar tudo ele termina com a
//! falha de abertura do pty.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Errno};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const LONGS: &[LongOpt] = &[
    LongOpt::new("timing", HasArg::Required, b't' as i32),
    LongOpt::new("log-timing", HasArg::Required, b'T' as i32),
    LongOpt::new("log-in", HasArg::Required, b'I' as i32),
    LongOpt::new("log-io", HasArg::Required, b'B' as i32),
    LongOpt::new("typescript", HasArg::Required, b's' as i32),
    LongOpt::new("command", HasArg::Required, b'c' as i32),
    LongOpt::new("divisor", HasArg::Required, b'd' as i32),
    LongOpt::new("maxdelay", HasArg::Required, b'm' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
];

const USAGE: &str = "
Usage:
 scriptlive [options] [-t] timingfile [-I|-B] typescript

Execute terminal typescript, using timing information.

Options:
 -t, --timing <file>     script timing log file
 -T, --log-timing <file> alias to -t
 -I, --log-in <file>     script stdin log file
 -B, --log-io <file>     script stdin and stdout log file
 -s, --typescript <file> deprecated alias to -I
 -c, --command <cmd>     run command rather than interactive shell
 -d, --divisor <num>     speed up or slow down execution with time divisor
 -m, --maxdelay <num>    wait at most this many seconds between updates
 -h, --help              display this help
 -V, --version           display version

For more details see scriptlive(1).
";

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn parse_f64(text: &[u8]) -> Option<f64> {
    std::str::from_utf8(text).ok()?.trim().parse::<f64>().ok()
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut timing: Option<Vec<u8>> = None;
    let mut typescript: Option<Vec<u8>> = None;
    let mut divisor = 1.0f64;
    let mut g = Getopt::from_env(&argv[1..], "B:c:d:I:m:s:T:t:Vh", LONGS);
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
        match o.short() {
            Some('t') | Some('T') => timing = Some(arg),
            Some('I') | Some('B') | Some('s') => typescript = Some(arg),
            Some('c') => {}
            Some('d') => match parse_f64(&arg) {
                Some(v) => divisor = v,
                None => {
                    ul::warnx(&short, format!("unsupported divisor: '{}'", io::lossy(&arg)));
                    return 1;
                }
            },
            Some('m') => {
                if parse_f64(&arg).is_none() {
                    ul::warnx(&short, format!("unsupported maxdelay: '{}'", io::lossy(&arg)));
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
    let mut ops = g.operands().into_iter();
    if timing.is_none() {
        timing = ops.next();
    }
    if typescript.is_none() {
        typescript = ops.next();
    }
    let Some(timing) = timing else {
        ul::warnx(&short, "timing file not specified");
        return 1;
    };
    let Some(typescript) = typescript else {
        ul::warnx(&short, "typescript file not specified");
        return 1;
    };
    if divisor <= 0.0 {
        ul::warnx(&short, "unsupported divisor: must be greater than zero");
        return 1;
    }
    for f in [&timing, &typescript] {
        if let Err(e) = io::read_path(f) {
            ul::warn(&short, format!("cannot open {}", io::lossy(f)), e);
            return 1;
        }
    }
    ul::warn(&short, "failed to create pseudo-terminal", Errno::ENOENT);
    1
}
