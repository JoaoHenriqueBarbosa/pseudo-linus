//! `swapoff` do util-linux 2.41: desativa dispositivos e arquivos de swap.
//!
//! Porte do `sys-utils/swapoff.c`. O `sysabi` não oferece `swapoff(2)`: o alvo é procurado em
//! `/proc/swaps` e, como num contêiner sem privilégio, o que existir termina sem permissão; o que
//! não estiver em uso responde como o kernel (`Invalid argument` ou `No such file or directory`).

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Errno, sys};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const LONGS: &[LongOpt] = &[
    LongOpt::new("all", HasArg::No, b'a' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("verbose", HasArg::No, b'v' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
];

const USAGE: &str = "
Usage:
 swapoff [options] [<spec>]

Disable devices and files for paging and swapping.

Options:
 -a, --all              disable all swaps from /proc/swaps
 -v, --verbose          verbose mode

 -h, --help             display this help
 -V, --version          display version

The <spec> parameter:
 -L <label>             LABEL of device to be used
 -U <uuid>              UUID of device to be used
 LABEL=<label>          specifies device by swap area label
 UUID=<uuid>            specifies device by swap area UUID
 <device>               name of device to be used
 <file>                 name of file to be used

For more details see swapoff(8).
";

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn active() -> Vec<String> {
    let data = sys::read_file(b"/proc/swaps").unwrap_or_default();
    String::from_utf8_lossy(&data)
        .lines()
        .skip(1)
        .filter_map(|l| l.split_whitespace().next().map(str::to_string))
        .collect()
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut all = false;
    let mut specs: Vec<Vec<u8>> = Vec::new();
    let mut g = Getopt::from_env(&argv[1..], "ahvVL:U:", LONGS);
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
            Some('a') => all = true,
            Some('v') => {}
            Some('L') => specs.push([b"LABEL=".as_slice(), o.arg.as_deref().unwrap_or(b"")].concat()),
            Some('U') => specs.push([b"UUID=".as_slice(), o.arg.as_deref().unwrap_or(b"")].concat()),
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
    specs.extend(g.operands().iter().cloned());
    if !all && specs.is_empty() {
        ul::warnx(&short, "bad usage");
        ul::errtryhelp(&short);
        return 1;
    }
    let in_use = active();
    let mut rc = 0;
    if all {
        for name in &in_use {
            ul::warn(&short, format!("{name}: swapoff failed"), Errno::EPERM);
            rc = 255;
        }
    }
    for s in &specs {
        let name = io::lossy(s);
        let e = if s.starts_with(b"LABEL=") || s.starts_with(b"UUID=") {
            ul::warnx(&short, format!("cannot find the device for {name}"));
            rc = 255;
            continue;
        } else if in_use.iter().any(|a| *a == name) {
            Errno::EPERM
        } else {
            match sys::stat(s) {
                Ok(_) => Errno::EINVAL,
                Err(e) => e,
            }
        };
        ul::warn(&short, format!("{name}: swapoff failed"), e);
        rc = 255;
    }
    rc
}
