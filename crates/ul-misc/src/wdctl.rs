//! `wdctl` do util-linux 2.41: mostra o estado do watchdog de hardware.
//!
//! Porte do `sys-utils/wdctl.c`. O sandbox não tem `/dev/watchdog*`: sem dispositivo informado o
//! programa avisa que não há dispositivo padrão, e com dispositivo informado falha ao abri-lo.

use std::ffi::OsString;
use std::io::Write;

use sysabi::Ctx;

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const COLUMNS: &[(&str, &str)] = &[
    ("FLAG", "flag name"),
    ("DESCRIPTION", "flag description"),
    ("STATUS", "flag status"),
    ("BOOT-STATUS", "flag boot status"),
    ("DEVICE", "watchdog device name"),
];

const LONGS: &[LongOpt] = &[
    LongOpt::new("flags", HasArg::Required, b'f' as i32),
    LongOpt::new("noflags", HasArg::No, b'F' as i32),
    LongOpt::new("noident", HasArg::No, b'I' as i32),
    LongOpt::new("noheadings", HasArg::No, b'n' as i32),
    LongOpt::new("oneline", HasArg::No, b'O' as i32),
    LongOpt::new("output", HasArg::Required, b'o' as i32),
    LongOpt::new("setpretimeout", HasArg::Required, b'p' as i32),
    LongOpt::new("raw", HasArg::No, b'r' as i32),
    LongOpt::new("settimeout", HasArg::Required, b's' as i32),
    LongOpt::new("notimeouts", HasArg::No, b'T' as i32),
    LongOpt::new("flags-only", HasArg::No, b'x' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
];

fn usage(short: &str) -> String {
    let mut s = format!(
        "
Usage:
 {short} [options] [<device> ...]

Show the status of the hardware watchdog.

Options:
 -f, --flags <list>      print selected flags only
 -F, --noflags           don't print information about flags
 -I, --noident           don't print watchdog identity information
 -n, --noheadings        don't print headings for flags table
 -O, --oneline           print all information on one line
 -o, --output <list>     output columns of the flags
 -p, --setpretimeout <sec> set watchdog pre-timeout
 -r, --raw               use raw output format for flags table
 -s, --settimeout <sec>  set watchdog timeout
 -T, --notimeouts        don't print watchdog timeouts
 -x, --flags-only        print only the flags table (same as -I -T)

 -h, --help              display this help
 -V, --version           display version

Available columns:
"
    );
    for (n, h) in COLUMNS {
        s.push_str(&format!(" {n:>11}  {h}\n"));
    }
    s.push_str(&format!("\nFor more details see {short}(8).\n"));
    s
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut g = Getopt::from_env(&argv[1..], "f:FhIno:Op:rs:TVx", LONGS);
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
            Some('o') => {
                for name in o.arg_str().split(',').filter(|n| !n.is_empty()) {
                    if !COLUMNS.iter().any(|c| c.0.eq_ignore_ascii_case(name)) {
                        ul::warnx(&short, format!("unknown column: {name}"));
                        return 1;
                    }
                }
            }
            Some('s') | Some('p') => {
                let what = if o.short() == Some('s') {
                    "invalid timeout argument"
                } else {
                    "invalid pretimeout argument"
                };
                if let Err(m) = ul::strtou32_or_err(&o.arg.clone().unwrap_or_default(), what) {
                    ul::warnx(&short, m);
                    return 1;
                }
            }
            _ => {}
        }
    }

    let ops = g.operands();
    if ops.is_empty() {
        io::eprint(format!(
            "{short}: No default device is available.: No such file or directory\n"
        ));
        return 1;
    }
    let mut rc = 0;
    for d in &ops {
        if let Err(e) = io::File::open(d) {
            ul::warn(&short, format!("cannot open {}", io::lossy(d)), e);
            rc = 1;
        } else {
            ul::warnx(&short, format!("{}: unable to read the watchdog status: Inappropriate ioctl for device", io::lossy(d)));
            rc = 1;
        }
    }
    rc
}
