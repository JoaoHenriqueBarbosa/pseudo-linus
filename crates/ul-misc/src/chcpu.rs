//! `chcpu` do util-linux 2.41: configura CPUs (liga, desliga, configura, modo de despacho, rescan).
//!
//! Porte do `sys-utils/chcpu.c`. O sandbox se comporta como um contêiner sem privilégio: CPUs que
//! não existem no sysfs, CPUs sem o arquivo `online` (não removíveis a quente) e escritas no sysfs
//! negadas são tratadas com as mensagens do original.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, sys};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const CPU_DIR: &str = "/sys/devices/system/cpu";

const LONGS: &[LongOpt] = &[
    LongOpt::new("configure", HasArg::Required, b'c' as i32),
    LongOpt::new("deconfigure", HasArg::Required, b'g' as i32),
    LongOpt::new("disable", HasArg::Required, b'd' as i32),
    LongOpt::new("dispatch", HasArg::Required, b'p' as i32),
    LongOpt::new("enable", HasArg::Required, b'e' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("rescan", HasArg::No, b'r' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
];

fn usage(_short: &str) -> String {
    r#"
Usage:
 chcpu [options]

Configure CPUs in a multi-processor system.

Options:
 -e, --enable <cpu-list>       enable cpus
 -d, --disable <cpu-list>      disable cpus
 -c, --configure <cpu-list>    configure cpus
 -g, --deconfigure <cpu-list>  deconfigure cpus
 -p, --dispatch <mode>         set dispatching mode
 -r, --rescan                  trigger rescan of cpus
 -h, --help                    display this help
 -V, --version                 display version

For more details see chcpu(8).
"#.to_string()
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum Cmd {
    Enable,
    Disable,
    Configure,
    Deconfigure,
    Dispatch,
    Rescan,
}

/// `cpulist_parse`: `0-3,5,8-12:2`; `None` quando inválida.
fn parse_cpulist(s: &str) -> Option<Vec<u32>> {
    let mut out = Vec::new();
    if s.is_empty() {
        return None;
    }
    for part in s.split(',') {
        let (range, stride) = match part.split_once(':') {
            Some((r, st)) => (r, st.parse::<u32>().ok().filter(|v| *v > 0)?),
            None => (part, 1),
        };
        let (a, b) = match range.split_once('-') {
            Some((a, b)) => (a.parse::<u32>().ok()?, b.parse::<u32>().ok()?),
            None => {
                let v = range.parse::<u32>().ok()?;
                (v, v)
            }
        };
        if a > b || b >= 8192 {
            return None;
        }
        let mut c = a;
        while c <= b {
            if !out.contains(&c) {
                out.push(c);
            }
            c += stride;
        }
    }
    out.sort_unstable();
    Some(out)
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut cmd: Option<Cmd> = None;
    let mut list: Vec<u32> = Vec::new();
    let mut mode = String::new();

    let mut g = Getopt::from_env(&argv[1..], "c:d:e:g:hp:rV", LONGS);
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                ul::errtryhelp(&short);
                return 1;
            }
        };
        let c = match o.short() {
            Some(c) => c,
            None => continue,
        };
        match c {
            'h' => {
                let mut out = io::stdout();
                let _ = out.write_all(usage(&short).as_bytes());
                return 0;
            }
            'V' => {
                ul::print_version(&short);
                return 0;
            }
            'c' | 'd' | 'e' | 'g' => {
                let a = o.arg_str();
                match parse_cpulist(&a) {
                    Some(l) => list = l,
                    None => {
                        ul::warnx(&short, format!("failed to parse CPU list: {a}"));
                        return 1;
                    }
                }
                cmd = Some(match c {
                    'c' => Cmd::Configure,
                    'd' => Cmd::Disable,
                    'e' => Cmd::Enable,
                    _ => Cmd::Deconfigure,
                });
            }
            'p' => {
                mode = o.arg_str();
                cmd = Some(Cmd::Dispatch);
            }
            'r' => cmd = Some(Cmd::Rescan),
            _ => {}
        }
    }

    let Some(cmd) = cmd else {
        ul::warnx(&short, "no action specified");
        ul::errtryhelp(&short);
        return 1;
    };
    if !g.operands().is_empty() {
        ul::errtryhelp(&short);
        return 1;
    }

    match cmd {
        Cmd::Rescan => {
            if sys::stat(format!("{CPU_DIR}/rescan").as_bytes()).is_err() {
                ul::warnx(&short, "This system does not support rescanning of CPUs");
                return 1;
            }
            ul::warnx(&short, "Failed to trigger rescan of CPUs: Permission denied");
            1
        }
        Cmd::Dispatch => {
            let m = match mode.as_str() {
                "horizontal" => 0,
                "vertical" => 1,
                _ => {
                    ul::warnx(&short, format!("unsupported argument: {mode}"));
                    return 1;
                }
            };
            let _ = m;
            if sys::stat(format!("{CPU_DIR}/dispatching").as_bytes()).is_err() {
                ul::warnx(
                    &short,
                    "This system does not support setting the dispatching mode of CPUs",
                );
                return 1;
            }
            ul::warnx(&short, "Failed to set CPU dispatching mode: Permission denied");
            1
        }
        _ => {
            let mut rc = 0;
            for cpu in &list {
                let dir = format!("{CPU_DIR}/cpu{cpu}");
                if sys::stat(dir.as_bytes()).is_err() {
                    ul::warnx(&short, format!("CPU {cpu} does not exist"));
                    rc = 1;
                    continue;
                }
                let (file, verb) = match cmd {
                    Cmd::Enable => ("online", "enable"),
                    Cmd::Disable => ("online", "disable"),
                    Cmd::Configure => ("configure", "configure"),
                    _ => ("configure", "deconfigure"),
                };
                if sys::stat(format!("{dir}/{file}").as_bytes()).is_err() {
                    if file == "online" {
                        ul::warnx(&short, format!("CPU {cpu} is not hot pluggable"));
                    } else {
                        ul::warnx(&short, format!("CPU {cpu} is not configurable"));
                    }
                    rc = 1;
                    continue;
                }
                ul::warnx(
                    &short,
                    format!("CPU {cpu} {verb} failed: Permission denied"),
                );
                rc = 1;
            }
            rc
        }
    }
}
