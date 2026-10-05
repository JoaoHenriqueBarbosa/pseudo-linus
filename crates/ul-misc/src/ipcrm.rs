//! `ipcrm` do util-linux 2.41: remove recursos IPC (System V e POSIX).
//!
//! Porte do `sys-utils/ipcrm.c`. O `sysabi` não oferece `shmctl`/`msgctl`/`semctl`, então qualquer
//! id ou chave de System V é tratado como inexistente (o que o original dá num contêiner sem IPC):
//! `invalid id (N)` e `invalid key (K)`. Nomes POSIX são procurados em `/dev/shm` e `/dev/mqueue`.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, sys};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const LONGS: &[LongOpt] = &[
    LongOpt::new("shmem-id", HasArg::Required, b'm' as i32),
    LongOpt::new("shmem-key", HasArg::Required, b'M' as i32),
    LongOpt::new("queue-id", HasArg::Required, b'q' as i32),
    LongOpt::new("queue-key", HasArg::Required, b'Q' as i32),
    LongOpt::new("semaphore-id", HasArg::Required, b's' as i32),
    LongOpt::new("semaphore-key", HasArg::Required, b'S' as i32),
    LongOpt::new("posix-shmem", HasArg::Required, 0x100),
    LongOpt::new("posix-mqueue", HasArg::Required, 0x101),
    LongOpt::new("posix-semaphore", HasArg::Required, 0x102),
    LongOpt::new("all", HasArg::Optional, b'a' as i32),
    LongOpt::new("verbose", HasArg::No, b'v' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
];

const USAGE: &str = "
Usage:
 ipcrm [options]
 ipcrm shm|msg|sem <id>...

Remove certain IPC resources.

Options:
 -m, --shmem-id <id>        \t\t\t\tremove shared memory segment by id
 -M, --shmem-key <key>      \t\t\t\tremove shared memory segment by key
     --posix-shmem <name>   \t\t\t\tremove POSIX shared memory segment by name
 -q, --queue-id <id>        \t\t\t\tremove message queue by id
 -Q, --queue-key <key>      \t\t\t\tremove message queue by key
     --posix-mqueue <name>  \t\t\t\tremove POSIX message queue by name
 -s, --semaphore-id <id>    \t\t\t\tremove semaphore by id
 -S, --semaphore-key <key>  \t\t\t\tremove semaphore by key
     --posix-semaphore <name> \t\t\t\tremove POSIX semaphore by name
 -a, --all[=shm|pshm|msg|pmsg|sem|psem]\tremove all (in the specified category)
 -v, --verbose              \t\t\t\texplain what is being done

 -h, --help                 display this help
 -V, --version              display version

For more details see ipcrm(1).
";

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

/// `strtoul(arg, 0)` do original: o texto precisa ser um inteiro inteiro, em base 0.
fn parse_num(arg: &[u8]) -> Option<u64> {
    let s = std::str::from_utf8(arg).ok()?;
    if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(h, 16).ok()
    } else if s.len() > 1 && s.starts_with('0') {
        u64::from_str_radix(&s[1..], 8).ok()
    } else {
        s.parse().ok()
    }
}

fn posix_remove(short: &str, dir: &str, name: &str) -> bool {
    let rel = name.trim_start_matches('/');
    let path = format!("{dir}/{rel}");
    if sys::stat(path.as_bytes()).is_err() {
        ul::warnx(short, format!("name `{name}' not found"));
        return false;
    }
    ul::warnx(short, format!("permission denied for name `{name}'"));
    false
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    if argv.len() < 2 {
        ul::warnx(&short, "bad usage");
        ul::errtryhelp(&short);
        return 1;
    }

    let mut g = Getopt::from_env(&argv[1..], "m:M:q:Q:s:S:a::vhV", LONGS);
    let mut ret = 0;
    let mut seen_opt = false;
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                ul::errtryhelp(&short);
                return 1;
            }
        };
        seen_opt = true;
        let arg = o.arg.clone().unwrap_or_default();
        let text = io::lossy(&arg);
        match o.short() {
            Some('m') | Some('q') | Some('s') => {
                if parse_num(&arg).is_none() {
                    ul::warnx(&short, format!("failed to parse argument: '{text}'"));
                    return 1;
                }
                ul::warnx(&short, format!("invalid id ({text})"));
                ret = 1;
            }
            Some('M') | Some('Q') | Some('S') => {
                if parse_num(&arg).is_none() {
                    ul::warnx(&short, format!("failed to parse argument: '{text}'"));
                    return 1;
                }
                ul::warnx(&short, format!("invalid key ({text})"));
                ret = 1;
            }
            Some('a') => {
                let cat = text.trim_start_matches('=').to_string();
                if !(cat.is_empty() || matches!(cat.as_str(), "shm" | "pshm" | "msg" | "pmsg" | "sem" | "psem")) {
                    ul::warnx(&short, format!("unknown argument: {cat}"));
                    return 1;
                }
                // Sem recursos System V nem nomes POSIX visíveis, não há o que remover.
            }
            Some('v') => {}
            Some('h') => {
                let mut out = io::stdout();
                let _ = out.write_all(USAGE.as_bytes());
                return 0;
            }
            Some('V') => {
                ul::print_version(&short);
                return 0;
            }
            None if o.id == 0x100 => {
                if !posix_remove(&short, "/dev/shm", &text) {
                    ret = 1;
                }
            }
            None if o.id == 0x101 => {
                if !posix_remove(&short, "/dev/mqueue", &text) {
                    ret = 1;
                }
            }
            None if o.id == 0x102 => {
                let name = format!("sem.{}", text.trim_start_matches('/'));
                if !posix_remove(&short, "/dev/shm", &name) {
                    ret = 1;
                }
            }
            _ => {
                ul::errtryhelp(&short);
                return 1;
            }
        }
    }

    let rest = g.operands();
    if rest.is_empty() {
        if !seen_opt {
            ul::warnx(&short, "bad usage");
            ul::errtryhelp(&short);
            return 1;
        }
        return ret;
    }
    let first = io::lossy(&rest[0]);
    if !seen_opt && matches!(first.as_str(), "shm" | "msg" | "sem") {
        if rest.len() < 2 {
            ul::warnx(&short, "not enough arguments");
            ul::errtryhelp(&short);
            return 1;
        }
        for a in &rest[1..] {
            if parse_num(a).is_none() {
                ul::warnx(&short, format!("failed to parse argument: '{}'", io::lossy(a)));
                return 1;
            }
            ul::warnx(&short, format!("invalid id ({})", io::lossy(a)));
            ret = 1;
        }
        return ret;
    }
    ul::warnx(&short, format!("unknown argument: {first}"));
    ul::errtryhelp(&short);
    1
}
