//! `clear` do ncurses 6.5.20250216 (`clear.c`, `clear_cmd.c`).

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, sys};

use super::terminfo::{SetupOpts, Term, TiStr, setupterm};
use super::tparm::tputs;
use super::{StdoutSink, VERSION, rootname, save_tty_settings};
use crate::util::io;
use crate::util::{Getopt, GetoptError};

/// `clear_cmd(legacy)`: limpa a tela e, sem `legacy`, o histórico (`E3`). `false` quando o
/// terminal não tem `clear`.
pub fn clear_cmd(term: &Term, legacy: bool) -> bool {
    let lines = term.tt.n("lines");
    let affcnt = if lines > 0 { lines } else { 1 };
    let mut sink = StdoutSink;
    let ok = match term.tt.sv("clear_screen") {
        Some(c) => {
            tputs(Some(&term.tt), c, affcnt, false, &mut sink);
            true
        }
        None => false,
    };
    if !legacy
        && let TiStr::Val(e3) = term.tigetstr(b"E3")
    {
        tputs(Some(&term.tt), e3, affcnt, false, &mut sink);
    }
    ok
}

fn usage(progname: &str) -> ! {
    let msg = "\nOptions:\n  -T TERM     use this instead of $TERM\n  -V          print curses-version\n  -x          do not try to clear scrollback\n";
    io::eprint(format!("Usage: {progname} [options]\n{msg}"));
    sys::exit(1);
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let progname = io::lossy(rootname(&argv[0]));
    let mut term: Option<Vec<u8>> = sys::getenv("TERM");
    let mut opts = SetupOpts::default();
    let mut opt_x = false;
    let mut g = Getopt::from_env(&argv[1..], "T:Vx", &[]);
    while let Some(r) = g.next_opt() {
        match r {
            Ok(o) => match o.short() {
                Some('T') => {
                    opts = SetupOpts { use_env: false, use_tioctl: true };
                    term = o.arg.clone();
                }
                Some('V') => {
                    let mut out = io::stdout();
                    let _ = writeln!(out, "{VERSION}");
                    return 0;
                }
                Some('x') => opt_x = true,
                _ => usage(&progname),
            },
            Err(e) => bad_option(&e, &argv0, &progname),
        }
    }
    if !g.operands().is_empty() {
        usage(&progname);
    }
    let fd = save_tty_settings(&progname, false);
    let t = match setupterm(term.as_deref(), fd, opts) {
        Ok(t) => t,
        Err(f) => {
            io::eprint(f.message);
            return 1;
        }
    };
    i32::from(!clear_cmd(&t, opt_x))
}

fn bad_option(e: &GetoptError, argv0: &str, progname: &str) -> ! {
    io::eprint(format!("{}\n", e.message(argv0)));
    usage(progname)
}
