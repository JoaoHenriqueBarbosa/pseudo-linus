//! `agetty` do util-linux 2.41: abre um terminal e ajusta o modo.
//!
//! O sandbox não tem tty: valida opções e argumentos e, quando a linha não pode ser aberta, falha
//! com a mensagem de `open_tty`.

use std::ffi::OsString;
use std::io::Write;

use sysabi::Ctx;

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const LONGS: &[LongOpt] = &[
    LongOpt::new("8bits", HasArg::No, b'8' as i32),
    LongOpt::new("autologin", HasArg::Required, b'a' as i32),
    LongOpt::new("noreset", HasArg::No, b'c' as i32),
    LongOpt::new("remote", HasArg::No, b'E' as i32),
    LongOpt::new("issue-file", HasArg::Required, b'f' as i32),
    LongOpt::new("flow-control", HasArg::No, b'h' as i32),
    LongOpt::new("host", HasArg::Required, b'H' as i32),
    LongOpt::new("noissue", HasArg::No, b'i' as i32),
    LongOpt::new("init-string", HasArg::Required, b'I' as i32),
    LongOpt::new("noclear", HasArg::No, b'J' as i32),
    LongOpt::new("login-program", HasArg::Required, b'l' as i32),
    LongOpt::new("local-line", HasArg::Optional, b'L' as i32),
    LongOpt::new("extract-baud", HasArg::No, b'm' as i32),
    LongOpt::new("skip-login", HasArg::No, b'n' as i32),
    LongOpt::new("nonewline", HasArg::No, b'N' as i32),
    LongOpt::new("login-options", HasArg::Required, b'o' as i32),
    LongOpt::new("login-pause", HasArg::No, b'p' as i32),
    LongOpt::new("chroot", HasArg::Required, b'r' as i32),
    LongOpt::new("hangup", HasArg::No, b'R' as i32),
    LongOpt::new("keep-baud", HasArg::No, b's' as i32),
    LongOpt::new("timeout", HasArg::Required, b't' as i32),
    LongOpt::new("detect-case", HasArg::No, b'U' as i32),
    LongOpt::new("wait-cr", HasArg::No, b'w' as i32),
    LongOpt::new("version", HasArg::No, 312),
    LongOpt::new("help", HasArg::No, 300),
    LongOpt::new("list-speeds", HasArg::No, 301),
    LongOpt::new("show-issue", HasArg::No, 302),
    LongOpt::new("reload", HasArg::No, 303),
    LongOpt::new("nohints", HasArg::No, 304),
    LongOpt::new("nohostname", HasArg::No, 305),
    LongOpt::new("long-hostname", HasArg::No, 306),
    LongOpt::new("erase-chars", HasArg::Required, 307),
    LongOpt::new("kill-chars", HasArg::Required, 308),
    LongOpt::new("chdir", HasArg::Required, 309),
    LongOpt::new("delay", HasArg::Required, 310),
    LongOpt::new("nice", HasArg::Required, 311),
];

const USAGE: &str = "
Usage:
 agetty [options] <line> [<baud_rate>,...] [<termtype>]
 agetty [options] <baud_rate>,... <line> [<termtype>]

Open a terminal and set its mode.

Options:
 -8, --8bits                assume 8-bit tty
 -a, --autologin <user>     login the specified user automatically
 -c, --noreset              do not reset control mode
 -E, --remote               use -r <hostname> for login(1)
 -f, --issue-file <list>    display issue files or directories
     --show-issue           display issue file and exit
 -h, --flow-control         enable hardware flow control
 -H, --host <hostname>      specify login host
 -i, --noissue              do not display issue file
 -I, --init-string <string> set init string
 -J, --noclear              do not clear the screen before prompt
 -l, --login-program <file> specify login program
 -L, --local-line[=<mode>]  control the local line flag
 -m, --extract-baud         extract baud rate during connect
 -n, --skip-login           do not prompt for login
 -N, --nonewline            do not print a newline before issue
 -o, --login-options <opts> options that are passed to login
 -p, --login-pause          wait for any key before the login
 -r, --chroot <dir>         change root to the directory
 -R, --hangup               do virtually hangup on the tty
 -s, --keep-baud            try to keep baud rate after break
 -t, --timeout <number>     login process timeout
 -U, --detect-case          detect uppercase terminal
 -w, --wait-cr              wait carriage-return
     --nohints              do not print hints
     --nohostname           no hostname at all will be shown
     --long-hostname        show full qualified hostname
     --erase-chars <string> additional backspace chars
     --kill-chars <string>  additional kill chars
     --chdir <directory>    chdir before the login
     --delay <number>       sleep seconds before prompt
     --nice <number>        run login with this priority
     --reload               reload prompts on running agetty instances
     --list-speeds          display supported baud rates
     --help                 display this help
     --version              display version

For more details see agetty(8).
";

const SPEEDS: &str = "50 75 110 134 150 200 300 600 1200 1800 2400 4800 9600 19200 38400 57600 115200 230400 460800 500000 576000 921600 1000000 1152000 1500000 2000000 2500000 3000000 3500000 4000000";

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut g = Getopt::from_env(&argv[1..], "8a:cEf:hH:iI:Jl:L::mnNo:pr:Rst:Uw", LONGS);
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                ul::errtryhelp(&short);
                return 1;
            }
        };
        match o.id {
            300 => {
                let _ = io::stdout().write_all(USAGE.as_bytes());
                return 0;
            }
            301 => {
                let _ = io::stdout().write_all(format!("{SPEEDS}\n").as_bytes());
                return 0;
            }
            312 => {
                ul::print_version(&short);
                return 0;
            }
            id if id == 't' as i32 => {
                let a = o.arg.as_deref().unwrap_or(b"");
                if let Err(m) = ul::strtou32_or_err(a, "invalid timeout argument") {
                    ul::warnx(&short, m);
                    return 1;
                }
            }
            310 | 311 => {
                let a = o.arg.as_deref().unwrap_or(b"");
                let what = if o.id == 310 { "invalid delay argument" } else { "invalid nice argument" };
                if let Err(m) = ul::strtou32_or_err(a, what) {
                    ul::warnx(&short, m);
                    return 1;
                }
            }
            _ => {}
        }
    }
    let ops = g.operands();
    if ops.is_empty() {
        ul::warnx(&short, "not enough arguments");
        return 1;
    }
    // Se o primeiro operando começa com dígito, é a lista de velocidades e a linha vem depois.
    let line_idx = if ops[0].first().is_some_and(|b| b.is_ascii_digit()) && ops.len() > 1 { 1 } else { 0 };
    let line = io::lossy(&ops[line_idx]);
    if line == "-" {
        return 0;
    }
    // O oráculo termina em silêncio, com status 1, quando a linha não é um tty utilizável.
    1
}
