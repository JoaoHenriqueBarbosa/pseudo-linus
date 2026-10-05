//! `sulogin` do util-linux 2.41: login de usuário único.
//!
//! Sem tty e sem a senha de root, só a validação de opções é observável: ajuda, versão, opção
//! inválida e, depois delas, a recusa de quem não é superusuário.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Errno};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const LONGS: &[LongOpt] = &[
    LongOpt::new("login-shell", HasArg::No, b'p' as i32),
    LongOpt::new("timeout", HasArg::Required, b't' as i32),
    LongOpt::new("force", HasArg::No, b'e' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
];

const USAGE: &str = "
Usage:
 sulogin [options] [tty device]

Single-user login.

Options:
 -p, --login-shell        start a login shell
 -t, --timeout <seconds>  max time to wait for a password (default: no limit)
 -e, --force              examine password files directly if getpwnam(3) fails

 -h, --help               display this help
 -V, --version            display version

For more details see sulogin(8).
";

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut g = Getopt::from_env(&argv[1..], "ept:Vh", LONGS);
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                // O original só avisa da opção inválida e segue adiante.
                io::eprint(format!("{}\n", e.message(&argv0)));
                continue;
            }
        };
        match o.short() {
            Some('p') | Some('e') => {}
            Some('t') => {
                let a = o.arg.as_deref().unwrap_or(b"");
                if let Err(m) = ul::strtou32_or_err(a, "invalid timeout argument") {
                    ul::warnx(&short, m);
                    return 1;
                }
            }
            Some('h') => {
                let _ = io::stdout().write_all(USAGE.as_bytes());
                return 0;
            }
            Some('V') => {
                let _ = io::stdout().write_all(
                    format!(
                        "{short} from util-linux 2.41.5 (features: selinux, plymouth, keyboard mode, widechar, serial-info)\n"
                    )
                    .as_bytes(),
                );
                return 0;
            }
            _ => {}
        }
    }
    // Sem tty, o `tcgetattr` falha e o original encerra com status 0.
    ul::warn(&short, "tcgetattr failed", Errno::EINVAL);
    0
}
