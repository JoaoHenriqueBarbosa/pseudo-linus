//! `pwdx` do procps-ng 4.0.4: o diretório corrente de cada processo, lido do link
//! `/proc/<pid>/cwd`.
//!
//! O argumento é `NNNN` ou `/proc/NNNN` (o `check_pid_argument` do original: `strtol` em base 10,
//! tudo consumido, maior que zero). Falha no `readlink` sai como `<arg>: <strerror>`, com ENOENT
//! trocado por ESRCH (`No such process`), e o código de saída vira 1 sem interromper os demais.

use std::ffi::OsString;

use sysabi::{Errno, Fd, sys};
use ul_misc::util::getopt::{Getopt, HasArg, LongOpt};
use ul_misc::util::io;

use crate::common::{self, out};

const USAGE: &str = "\nUsage:\n pwdx [options] pid...\n\nOptions:\n -h, --help     display this help and exit\n -V, --version  output version information and exit\n\nFor more details see pwdx(1).\n";

const LONGS: &[LongOpt] = &[LongOpt::new("version", HasArg::No, 'V' as i32), LongOpt::new("help", HasArg::No, 'h' as i32)];

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

/// `check_pid_argument`: aceita `NNNN` e `/proc/NNNN`, com `NNNN` inteiro positivo em base 10.
pub fn valid_pid_argument(arg: &str) -> bool {
    let rest = arg.strip_prefix("/proc/").unwrap_or(arg);
    match common::strtol(rest) {
        common::Strtol::Ok(v) => v >= 1,
        _ => false,
    }
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let mut g = Getopt::from_env(&argv[1..], "Vh", LONGS);
    // Toda opção aceita encerra o programa, então só a primeira importa.
    if let Some(r) = g.next_opt() {
        match r {
            Err(e) => {
                io::eprint(format!("{}\n{USAGE}", e.message(&argv0)));
                return 1;
            }
            Ok(o) => match o.short() {
                Some('V') => {
                    out("pwdx from procps-ng 4.0.4\n");
                    return 0;
                }
                Some('h') => {
                    out(USAGE);
                    return 0;
                }
                _ => unreachable!("tabela de opções do pwdx"),
            },
        }
    }
    let operands = g.operands();
    if operands.is_empty() {
        io::eprint(USAGE);
        return 1;
    }
    let sysc = sys::current();
    let mut status = 0;
    for raw in &operands {
        let arg = String::from_utf8_lossy(raw).into_owned();
        if !valid_pid_argument(&arg) {
            common::warn("pwdx", &format!("invalid process id: {arg}"));
            return 1;
        }
        // Os dois formatos começam por dígito ou por `/`, então o primeiro byte decide.
        let link = if raw.first() == Some(&b'/') { format!("{arg}/cwd") } else { format!("/proc/{arg}/cwd") };
        match sysc.readlinkat(Fd::CWD, link.as_bytes()) {
            Ok(target) => {
                let mut line = raw.clone();
                line.extend_from_slice(b": ");
                line.extend_from_slice(&target);
                line.push(b'\n');
                out(line);
            }
            Err(e) => {
                let e = if e == Errno::ENOENT { Errno::ESRCH } else { e };
                // O original escreve no stderr com stdout ainda no buffer; o nosso faz o mesmo.
                io::eprint(format!("{arg}: {}\n", e.message()));
                status = 1;
            }
        }
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pid_arguments() {
        assert!(valid_pid_argument("1"));
        assert!(valid_pid_argument("/proc/42"));
        assert!(!valid_pid_argument("0"));
        assert!(!valid_pid_argument("-3"));
        assert!(!valid_pid_argument("12x"));
        assert!(!valid_pid_argument("/proc/"));
        assert!(!valid_pid_argument("abc"));
    }
}
