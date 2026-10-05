//! `uclampset` do util-linux 2.41: mostra ou muda os atributos de utilization clamping de um processo.
//!
//! Porte do `schedutils/uclampset.c`. O `sysabi` não expõe `sched_getattr`/`sched_setattr`: um PID
//! inexistente responde `No such process` como o kernel, e um PID existente (ou um comando a
//! executar) responde `Operation not supported`, como num kernel sem `CONFIG_UCLAMP_TASK`.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, sys};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const LONGS: &[LongOpt] = &[
    LongOpt::new("util-min", HasArg::Required, b'm' as i32),
    LongOpt::new("util-max", HasArg::Required, b'M' as i32),
    LongOpt::new("pid", HasArg::Required, b'p' as i32),
    LongOpt::new("reset-on-fork", HasArg::No, b'R' as i32),
    LongOpt::new("verbose", HasArg::No, b'v' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
];

fn usage(_short: &str) -> String {
    r#"
Usage:
 uclampset [options]
 uclampset [options] --pid <pid> | --system | <command> <arg>...

Show or change the utilization clamping attributes.

Options:
 -m <value>           util_min value to set
 -M <value>           util_max value to set
 -a, --all-tasks      operate on all the tasks (threads) for a given pid
 -p, --pid <pid>      operate on existing given pid
 -s, --system         operate on system
 -R, --reset-on-fork  set reset-on-fork flag
 -v, --verbose        display status information
 -h, --help           display this help
 -V, --version        display version

Utilization value range is [0:1024]. Use special -1 value to reset to system's default.

For more details see uclampset(1).
"#.to_string()
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut pid: Option<u32> = None;
    let mut changing = false;

    // `+`: a varredura para no primeiro operando, que é o comando a executar.
    let mut g = Getopt::from_env(&argv[1..], "+m:M:p:RvhV", LONGS);
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
            Some('m') | Some('M') => {
                let what = if o.short() == Some('m') {
                    "invalid util-min argument"
                } else {
                    "invalid util-max argument"
                };
                match ul::strtou32_or_err(&o.arg.clone().unwrap_or_default(), what) {
                    Ok(v) if v <= 1024 => changing = true,
                    Ok(_) => {
                        ul::warnx(&short, format!("{what}: out of range (0..1024)"));
                        return 1;
                    }
                    Err(m) => {
                        ul::warnx(&short, m);
                        return 1;
                    }
                }
            }
            Some('p') => match ul::strtou32_or_err(&o.arg.clone().unwrap_or_default(), "invalid PID argument") {
                Ok(p) => pid = Some(p),
                Err(m) => {
                    ul::warnx(&short, m);
                    return 1;
                }
            },
            Some('R') | Some('v') => changing = changing || o.short() == Some('R'),
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

    let ops = g.operands();
    if pid.is_none() && ops.is_empty() {
        let _ = io::stdout().write_all(usage(&short).as_bytes());
        return 0;
    }
    if pid.is_some() && !ops.is_empty() {
        ul::warnx(&short, "bad usage");
        ul::errtryhelp(&short);
        return 1;
    }

    match pid {
        Some(p) => {
            if sys::stat(format!("/proc/{p}").as_bytes()).is_err() {
                ul::warnx(
                    &short,
                    format!("failed to get pid {p}'s uclamp values: No such process"),
                );
                return 1;
            }
            let verb = if changing { "set" } else { "get" };
            ul::warnx(
                &short,
                format!("failed to {verb} pid {p}'s uclamp values: Operation not supported"),
            );
            1
        }
        None => {
            ul::warnx(
                &short,
                "failed to set pid 0's uclamp values: Operation not supported",
            );
            1
        }
    }
}
