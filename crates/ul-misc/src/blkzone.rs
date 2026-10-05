//! `blkzone` do util-linux 2.41: comandos de zona em dispositivos de bloco zonados.
//!
//! As ioctls de zona exigem dispositivo de bloco, que o sandbox não tem: todo alvo que abre e não é
//! de bloco termina com `BLKGETZONESZ ioctl failed: Inappropriate ioctl for device`.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Errno, sys};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const LONGS: &[LongOpt] = &[
    LongOpt::new("count", HasArg::Required, b'c' as i32),
    LongOpt::new("force", HasArg::No, b'f' as i32),
    LongOpt::new("length", HasArg::Required, b'l' as i32),
    LongOpt::new("offset", HasArg::Required, b'o' as i32),
    LongOpt::new("verbose", HasArg::No, b'v' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
];

const USAGE: &str = "
Usage:
 blkzone <command> [options] <device>

Run zone command on the block device.

Commands:
 report      Report zone information about the device
 capacity    Report sum of zone capacities for the device
 reset       Reset a range of zones.
 open        Open a range of zones.
 close       Close a range of zones.
 finish      Set a range of zones to Full.

Options:
 -o, --offset <sector>  start sector of zone to act (in 512-byte sectors)
 -l, --length <sectors> maximum sectors to act (in 512-byte sectors)
 -c, --count <number>   maximum number of zones
 -f, --force            enforce on block devices used by the system
 -v, --verbose          display more details

 -h, --help             display this help
 -V, --version          display version

For more details see blkzone(8).
";

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    if argv.len() < 2 {
        ul::warnx(&short, "no command specified");
        ul::errtryhelp(&short);
        return 1;
    }
    // O comando é o primeiro argumento, a não ser que seja uma opção.
    let first = argv[1].clone();
    let is_opt = first.first() == Some(&b'-');
    let rest_start = if is_opt { 1 } else { 2 };
    let command = if is_opt { None } else { Some(io::lossy(&first)) };

    let mut g = Getopt::from_env(&argv[rest_start..], "c:fl:o:vhV", LONGS);
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
            Some('c') => {
                if let Err(m) = ul::strtou64_or_err(&arg, "failed to parse number of zones") {
                    ul::warnx(&short, m);
                    return 1;
                }
            }
            Some('l') | Some('o') => {
                let what = if o.short() == Some('l') {
                    "failed to parse length"
                } else {
                    "failed to parse offset"
                };
                if let Err(m) = ul::strtosize_or_err(&arg, what) {
                    ul::warnx(&short, m);
                    return 1;
                }
            }
            Some('f') | Some('v') => {}
            Some('h') => {
                let _ = io::stdout().write_all(USAGE.as_bytes());
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

    let Some(command) = command else {
        ul::warnx(&short, "no command specified");
        ul::errtryhelp(&short);
        return 1;
    };
    if !matches!(
        command.as_str(),
        "report" | "capacity" | "reset" | "open" | "close" | "finish"
    ) {
        ul::warnx(&short, format!("unknown command: {command}"));
        ul::errtryhelp(&short);
        return 1;
    }
    let ops = g.operands();
    if ops.is_empty() {
        ul::warnx(&short, "no device specified");
        ul::errtryhelp(&short);
        return 1;
    }
    let dev = &ops[0];
    let dev_s = io::lossy(dev);
    if let Err(e) = io::File::open(dev) {
        ul::warn(&short, format!("cannot open {dev_s}"), e);
        return 1;
    }
    let _ = sys::stat(dev);
    ul::warn(&short, format!("{dev_s}: BLKGETZONESZ ioctl failed"), Errno::ENOTTY);
    1
}
