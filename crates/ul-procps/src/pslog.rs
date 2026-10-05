//! `pslog` do psmisc 23.7: mostra o diretório de trabalho e os destinos de stdout e stderr de um
//! processo, lidos dos links de `/proc/<pid>/cwd` e `/proc/<pid>/fd/{1,2}`.
//!
//! Diferença conhecida: o formato exato das linhas foi reproduzido de memória do `pslog.c`, sem
//! conferência de oráculo.

use std::ffi::OsString;

use sysabi::{Ctx, Fd, sys};
use ul_misc::util::io;

use crate::common::out;

const USAGE: &str = "Usage: pslog pid\n       pslog -V\n";
const VERSION: &str = "pslog (PSmisc) 23.7\nCopyright (C) 1993-2024 Werner Almesberger and Craig Small\n\nPSmisc comes with ABSOLUTELY NO WARRANTY.\nThis is free software, and you are welcome to redistribute it under\nthe terms of the GNU General Public License.\nFor more information about these matters, see the files named COPYING.\n";

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    if argv.len() != 2 {
        io::eprint(USAGE);
        return 1;
    }
    let a = &argv[1];
    if a == b"-V" || a == b"--version" {
        io::eprint(VERSION);
        return 0;
    }
    if a.first() == Some(&b'-') {
        io::eprint(USAGE);
        return 1;
    }
    let digits: Vec<u8> = a.iter().copied().take_while(u8::is_ascii_digit).collect();
    let pid: i64 = String::from_utf8_lossy(&digits).parse().unwrap_or(0);
    if pid <= 0 {
        io::eprint(USAGE);
        return 1;
    }
    let sysc = sys::current();
    let mut s = Vec::new();
    for (label, path) in [("Path", format!("/proc/{pid}/cwd")), ("Stdout", format!("/proc/{pid}/fd/1")), ("Stderr", format!("/proc/{pid}/fd/2"))] {
        match sysc.readlinkat(Fd::CWD, path.as_bytes()) {
            Ok(t) => {
                s.extend_from_slice(label.as_bytes());
                s.extend_from_slice(b"\t: ");
                s.extend_from_slice(&t);
                s.push(b'\n');
            }
            Err(e) => {
                io::eprint(format!("pslog: cannot read {path}: {}\n", e.message()));
                return 1;
            }
        }
    }
    out(s);
    0
}
