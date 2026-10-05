//! `ionice` do util-linux 2.41: lê e muda a classe e a prioridade de E/S de processos.
//!
//! Porte do `schedutils/ionice.c`. Cobre `-c`, `-n`, `-p`, `-P`, `-u` e `-t`, e a consulta
//! (`best-effort: prio 4`, `idle`). O modo de execução (`ionice -c 3 comando`) não está portado,
//! porque o `sysabi` não expõe um `execve` pra este programa; ele sai com erro.

use std::ffi::OsString;
use std::io::Write;

use sysabi::sched::{
    IOPRIO_CLASS_BE, IOPRIO_CLASS_IDLE, IOPRIO_CLASS_NONE, IOPRIO_CLASS_RT, IOPRIO_WHO_PGRP, IOPRIO_WHO_PROCESS,
    IOPRIO_WHO_USER, ioprio_class, ioprio_level, ioprio_value,
};
use sysabi::sys;

use crate::util::io;
use crate::util::ul;

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn usage(short: &str) -> String {
    format!(
        "
Usage:
 {short} [options] -p <pid>...
 {short} [options] -P <pgid>...
 {short} [options] -u <uid>...
 {short} [options] <command>

Show or change the I/O-scheduling class and priority of a process.

Options:
 -c, --class <class>    name or number of scheduling class,
                          0: none, 1: realtime, 2: best-effort, 3: idle
 -n, --classdata <num>  priority (0..7) in the specified scheduling class,
                          only for the realtime and best-effort classes
 -p, --pid <pid>...     act on these already running processes
 -P, --pgid <pgrp>...   act on already running processes in these groups
 -t, --ignore           ignore failures
 -u, --uid <uid>...     act on already running processes owned by these users

 -h, --help             display this help
 -V, --version          display version

For more details see ionice(1).
"
    )
}

const CLASS_NAMES: [&str; 4] = ["none", "realtime", "best-effort", "idle"];

fn parse_class(s: &[u8]) -> Option<i32> {
    for (i, n) in CLASS_NAMES.iter().enumerate() {
        if s == n.as_bytes() {
            return Some(i as i32);
        }
    }
    parse_num(s).and_then(|v| i32::try_from(v).ok())
}

fn parse_num(s: &[u8]) -> Option<u64> {
    if s.is_empty() || !s.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(s).ok()?.parse().ok()
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let short = ul::short_name(args);

    let mut class: Option<i32> = None;
    let mut data: Option<i32> = None;
    let mut which = 0;
    let mut who: Option<Vec<u8>> = None;
    let mut tolerant = false;
    let mut idx = 1;

    // Opção com argumento: `-c2`, `-c 2`, `--class=2` e `--class 2`.
    macro_rules! optarg {
        ($a:expr, $short_len:expr) => {{
            let a: &[u8] = $a;
            if let Some(p) = a.iter().position(|b| *b == b'=').filter(|_| a.starts_with(b"--")) {
                a[p + 1..].to_vec()
            } else if a.len() > $short_len {
                a[$short_len..].to_vec()
            } else {
                idx += 1;
                match argv.get(idx) {
                    Some(v) => v.clone(),
                    None => {
                        ul::warnx(&short, format!("option requires an argument -- '{}'", a[1] as char));
                        ul::errtryhelp(&short);
                        return 1;
                    }
                }
            }
        }};
    }

    while idx < argv.len() {
        let a = argv[idx].as_slice();
        if a == b"--" {
            idx += 1;
            break;
        }
        if a.len() < 2 || a[0] != b'-' {
            break;
        }
        let name: &[u8] = if a.starts_with(b"--") {
            a.split(|b| *b == b'=').next().unwrap_or(a)
        } else {
            &a[..2]
        };
        match name {
            b"-h" | b"--help" => {
                let _ = io::stdout().write_all(usage(&short).as_bytes());
                return 0;
            }
            b"-V" | b"--version" => {
                ul::print_version(&short);
                return 0;
            }
            b"-t" | b"--ignore" => tolerant = true,
            b"-c" | b"--class" => {
                let v = optarg!(a, if a.starts_with(b"--") { 2 } else { 2 });
                match parse_class(&v) {
                    Some(c) => class = Some(c),
                    None => {
                        ul::warnx(&short, format!("unknown scheduling class: '{}'", io::lossy(&v)));
                        return 1;
                    }
                }
            }
            b"-n" | b"--classdata" => {
                let v = optarg!(a, 2);
                match parse_num(&v).and_then(|n| i32::try_from(n).ok()) {
                    Some(n) => data = Some(n),
                    None => {
                        ul::warnx(&short, format!("invalid class data argument: '{}'", io::lossy(&v)));
                        return 1;
                    }
                }
            }
            b"-p" | b"--pid" => {
                which = IOPRIO_WHO_PROCESS;
                who = Some(optarg!(a, 2));
            }
            b"-P" | b"--pgid" => {
                which = IOPRIO_WHO_PGRP;
                who = Some(optarg!(a, 2));
            }
            b"-u" | b"--uid" => {
                which = IOPRIO_WHO_USER;
                who = Some(optarg!(a, 2));
            }
            _ => {
                if a.starts_with(b"--") {
                    ul::warnx(&short, format!("unrecognized option '{}'", io::lossy(a)));
                } else {
                    ul::warnx(&short, format!("invalid option -- '{}'", a[1] as char));
                }
                ul::errtryhelp(&short);
                return 1;
            }
        }
        idx += 1;
    }
    let rest: Vec<Vec<u8>> = argv[idx.min(argv.len())..].to_vec();

    match class {
        Some(IOPRIO_CLASS_NONE) | Some(IOPRIO_CLASS_IDLE) if data.is_some() => {
            ul::warnx(&short, format!("ignoring given class data for {} class", CLASS_NAMES[class.unwrap() as usize]));
            data = None;
        }
        Some(c) if !(0..=3).contains(&c) => {
            ul::warnx(&short, format!("unknown scheduling class: '{c}'"));
            return 1;
        }
        _ => {}
    }
    if let Some(n) = data {
        if !(0..8).contains(&n) {
            ul::warnx(&short, format!("invalid class data argument: '{n}'"));
            return 1;
        }
    }

    let set = class.is_some() || data.is_some();
    if set && class.is_none() {
        class = Some(IOPRIO_CLASS_BE);
    }
    if set && data.is_none() && matches!(class, Some(IOPRIO_CLASS_RT) | Some(IOPRIO_CLASS_BE)) {
        data = Some(4);
    }

    if which == 0 {
        if rest.is_empty() && !set {
            // Sem alvo nem comando: consulta o próprio processo.
            return query(&short, IOPRIO_WHO_PROCESS, 0, tolerant);
        }
        if rest.is_empty() {
            io::eprint(usage(&short));
            return 1;
        }
        ul::warnx(&short, "executing a command is not supported");
        return 1;
    }

    // Alvos: o argumento da opção e os que vêm depois dela.
    let mut targets = vec![who.unwrap_or_default()];
    targets.extend(rest);
    let mut rc = 0;
    for t in &targets {
        let Some(id) = parse_num(t).and_then(|n| i32::try_from(n).ok()) else {
            let what = match which {
                IOPRIO_WHO_PROCESS => "PID",
                IOPRIO_WHO_PGRP => "PGID",
                _ => "UID",
            };
            ul::warnx(&short, format!("invalid {what} argument: '{}'", io::lossy(t)));
            return 1;
        };
        if set {
            let value = ioprio_value(class.unwrap_or(IOPRIO_CLASS_BE), data.unwrap_or(0));
            if let Err(e) = sys::current().ioprio_set(which, id, value) {
                if !tolerant {
                    ul::warn(&short, "ioprio_set failed".to_string(), e);
                    rc = 1;
                }
            }
        } else {
            rc |= query(&short, which, id, tolerant);
        }
    }
    rc
}

fn query(short: &str, which: i32, who: i32, tolerant: bool) -> i32 {
    match sys::current().ioprio_get(which, who) {
        Ok(v) => {
            let c = ioprio_class(v);
            let name = CLASS_NAMES.get(c as usize).copied().unwrap_or("unknown");
            let line = if c == IOPRIO_CLASS_IDLE {
                format!("{name}\n")
            } else {
                format!("{name}: prio {}\n", ioprio_level(v))
            };
            let _ = io::stdout().write_all(line.as_bytes());
            0
        }
        Err(e) => {
            if tolerant {
                return 0;
            }
            ul::warn(short, "ioprio_get failed".to_string(), e);
            1
        }
    }
}
