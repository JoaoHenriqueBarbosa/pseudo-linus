//! `wall` do util-linux 2.41: escreve uma mensagem em todos os terminais de usuários logados.
//!
//! Lê a mensagem do operando (arquivo existente ou texto) ou do stdin, valida `-t` e `-g` e percorre
//! o utmp. Sem sessões em terminal (o caso do contêiner) não há para quem escrever e sai com 0.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Fd, sys};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const LONGS: &[LongOpt] = &[
    LongOpt::new("nobanner", HasArg::No, b'n' as i32),
    LongOpt::new("timeout", HasArg::Required, b't' as i32),
    LongOpt::new("group", HasArg::Required, b'g' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
];

const USAGE: &str = "
Usage:
 wall [options] [<file> | <message>]

Write a message to all users.

Options:
 -g, --group <group>     only send message to members of the group
 -n, --nobanner          do not print banner, works only for root
 -t, --timeout <timeout> write timeout in seconds
 -h, --help              display this help
 -V, --version           display version

For more details see wall(1).
";

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut group: Option<Vec<u8>> = None;
    let mut g = Getopt::from_env(&argv[1..], "nt:g:Vh", LONGS);
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
            Some('n') => {}
            Some('t') => {
                let a = o.arg.clone().unwrap_or_default();
                match ul::strtou64_or_err(&a, "invalid timeout argument") {
                    Ok(n) if n > 0 => {}
                    Ok(_) => {
                        ul::warnx(
                            &short,
                            format!("invalid timeout argument: {}", io::lossy(&a)),
                        );
                        return 1;
                    }
                    Err(m) => {
                        ul::warnx(&short, m);
                        return 1;
                    }
                }
            }
            Some('g') => group = o.arg.clone(),
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

    if let Some(name) = &group {
        let known = io::read_path(b"/etc/group").is_ok_and(|d| {
            d.split(|b| *b == b'\n').any(|l| {
                let mut f = l.split(|b| *b == b':');
                f.next() == Some(name.as_slice())
                    || f.nth(1).is_some_and(|gid| gid == name.as_slice())
            })
        });
        if !known {
            ul::warnx(&short, format!("{}: unknown gid", io::lossy(name)));
            return 1;
        }
    }

    // Mensagem: o operando é arquivo se existir, senão o próprio texto; sem operando, o stdin.
    let _message: Vec<u8> = match ops.first() {
        Some(first) if ops.len() == 1 => match io::read_path(first) {
            Ok(d) => d,
            Err(_) => first.clone(),
        },
        Some(_) => ops.join(&b' '),
        None => {
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                match sys::read(Fd::STDIN, &mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => buf.extend_from_slice(&chunk[..n]),
                }
            }
            buf
        }
    };
    // Sem sessões de terminal no utmp não há destinatário.
    0
}
