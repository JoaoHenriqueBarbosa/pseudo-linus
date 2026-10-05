//! `ipcmk` do util-linux 2.41: cria recursos IPC (System V e POSIX).
//!
//! Porte do `sys-utils/ipcmk.c`. Validação de opções e mensagens idênticas às do original. O
//! `sysabi` não oferece `shmget`/`msgget`/`semget` nem `shm_open`/`sem_open`/`mq_open`, então a
//! criação em si termina com `Function not implemented` (ENOSYS); os tamanhos e contagens que o
//! kernel recusaria com EINVAL (zero) dão `Invalid argument` como no original.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Errno};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const LONGS: &[LongOpt] = &[
    LongOpt::new("shmem", HasArg::Required, b'M' as i32),
    LongOpt::new("posix-shmem", HasArg::Required, b'm' as i32),
    LongOpt::new("semaphore", HasArg::Required, b'S' as i32),
    LongOpt::new("posix-semaphore", HasArg::No, b's' as i32),
    LongOpt::new("queue", HasArg::No, b'Q' as i32),
    LongOpt::new("posix-mqueue", HasArg::No, b'q' as i32),
    LongOpt::new("mode", HasArg::Required, b'p' as i32),
    LongOpt::new("name", HasArg::Required, b'n' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
];

const USAGE: &str = "
Usage:
 ipcmk [options]

Create various IPC resources.

Options:
 -M, --shmem <size>       create shared memory segment of size <size>
 -m, --posix-shmem <size> create POSIX shared memory segment of size <size>
 -S, --semaphore <number> create semaphore array with <number> elements
 -s, --posix-semaphore    create POSIX semaphore
 -Q, --queue              create message queue
 -q, --posix-mqueue       create POSIX message queue
 -p, --mode <mode>        permission for the resource (default is 0644)
 -n, --name <name>        name of the POSIX resource

 -h, --help               display this help
 -V, --version            display version

Arguments:
 Values for <size> may be followed by a suffix: KiB, MiB,
 GiB, TiB, PiB, EiB, ZiB, or YiB (where the \"iB\" is optional).

 -n, --name <name> option is required for POSIX IPC

For more details see ipcmk(1).
";

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Shm(u64),
    PosixShm(u64),
    Sem(u32),
    PosixSem,
    Queue,
    PosixQueue,
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut wanted: Vec<Kind> = Vec::new();
    let mut name: Option<String> = None;
    let mut _mode: u32 = 0o644;

    let mut g = Getopt::from_env(&argv[1..], "hM:m:n:p:qQsS:V", LONGS);
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
            Some('M') | Some('m') => match ul::strtosize_or_err(&arg, "failed to parse size") {
                Ok(n) => wanted.push(if o.short() == Some('M') {
                    Kind::Shm(n)
                } else {
                    Kind::PosixShm(n)
                }),
                Err(m) => {
                    ul::warnx(&short, m);
                    return 1;
                }
            },
            Some('S') => match ul::strtou32_or_err(&arg, "failed to parse elements") {
                Ok(n) => wanted.push(Kind::Sem(n)),
                Err(m) => {
                    ul::warnx(&short, m);
                    return 1;
                }
            },
            Some('s') => wanted.push(Kind::PosixSem),
            Some('Q') => wanted.push(Kind::Queue),
            Some('q') => wanted.push(Kind::PosixQueue),
            Some('p') => {
                let t = io::lossy(&arg);
                match u32::from_str_radix(&t, 8) {
                    Ok(m) => _mode = m,
                    Err(_) => {
                        ul::warnx(&short, "failed to parse mode: Success");
                        return 1;
                    }
                }
            }
            Some('n') => name = Some(io::lossy(&arg)),
            Some('h') => {
                let mut out = io::stdout();
                let _ = out.write_all(USAGE.as_bytes());
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

    if wanted.is_empty() {
        ul::warnx(&short, "bad usage");
        ul::errtryhelp(&short);
        return 1;
    }
    let posix = wanted
        .iter()
        .any(|k| matches!(k, Kind::PosixShm(_) | Kind::PosixSem | Kind::PosixQueue));
    if posix && name.is_none() {
        ul::warnx(&short, "name is required for POSIX IPC");
        ul::errtryhelp(&short);
        return 1;
    }

    for k in &wanted {
        let (what, e) = match k {
            Kind::Shm(n) => (
                "create share memory failed",
                if *n == 0 { Errno::EINVAL } else { Errno::ENOSYS },
            ),
            Kind::PosixShm(_) => ("create POSIX shared memory failed", Errno::ENOSYS),
            Kind::Sem(n) => (
                "create semaphore failed",
                if *n == 0 { Errno::EINVAL } else { Errno::ENOSYS },
            ),
            Kind::PosixSem => ("create POSIX semaphore failed", Errno::ENOSYS),
            Kind::Queue => ("create message queue failed", Errno::ENOSYS),
            Kind::PosixQueue => ("create POSIX message queue failed", Errno::ENOSYS),
        };
        ul::warn(&short, what, e);
        return 1;
    }
    0
}
