//! `nologin` do util-linux 2.41 (pacote login do Debian 13): recusa um login com educação.
//!
//! Porte do `login-utils/nologin.c`: se `/etc/nologin.txt` existe, copia o conteúdo dele pro stdout;
//! senão imprime "This account is currently not available." Sai sempre com 1, exceto em `-h` e `-V`.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Fd, OFlags, sys};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const NOLOGIN_TXT: &[u8] = b"/etc/nologin.txt";

const LONGS: &[LongOpt] = &[
    LongOpt::new("command", HasArg::Required, b'c' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
];

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn usage(short: &str) -> String {
    format!(
        "
Usage:
 {short} [options]

Politely refuse a login.

Options:
 -c, --command <command>  does nothing (for compatibility with su -c)
 -h, --help               display this help
 -V, --version            display version

For more details see nologin(8).
"
    )
}

/// Copia o arquivo inteiro pro stdout; falha de abertura devolve `false`.
fn copy_file(path: &[u8]) -> bool {
    let Ok(fd) = sys::open(path, OFlags::RDONLY, 0) else {
        return false;
    };
    let mut out = io::stdout();
    let mut buf = [0u8; 4096];
    loop {
        match read_chunk(fd, &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let _ = out.write_all(&buf[..n]);
            }
        }
    }
    let _ = sys::close(fd);
    true
}

fn read_chunk(fd: Fd, buf: &mut [u8]) -> Result<usize, sysabi::Errno> {
    sys::read(fd, buf)
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut g = Getopt::from_env(&argv[1..], "c:hV", LONGS);
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
            Some('c') => {}
            Some('h') => {
                let mut out = io::stdout();
                let _ = out.write_all(usage(&short).as_bytes());
                return 1;
            }
            Some('V') => {
                ul::print_version(&short);
                return 1;
            }
            _ => {
                ul::errtryhelp(&short);
                return 1;
            }
        }
    }

    if copy_file(NOLOGIN_TXT) {
        return 1;
    }
    let mut out = io::stdout();
    let _ = out.write_all(b"This account is currently not available.\n");
    1
}
