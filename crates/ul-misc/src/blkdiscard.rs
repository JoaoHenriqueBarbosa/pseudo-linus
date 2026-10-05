//! `blkdiscard` do util-linux 2.41: descarta setores de um dispositivo de bloco.
//!
//! Porte do `sys-utils/blkdiscard.c`. Valida as opções e o dispositivo como o original. O descarte
//! em si exige as ioctls `BLKDISCARD`/`BLKSECDISCARD`/`BLKZEROOUT`, que o `sysabi` não oferece;
//! como o sandbox não tem dispositivo de bloco, o caminho de sucesso nunca é alcançado e todo alvo
//! que abre e não é de bloco termina com `not a block device`.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, sys};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const LONGS: &[LongOpt] = &[
    LongOpt::new("force", HasArg::No, b'f' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("length", HasArg::Required, b'l' as i32),
    LongOpt::new("offset", HasArg::Required, b'o' as i32),
    LongOpt::new("quiet", HasArg::No, b'q' as i32),
    LongOpt::new("secure", HasArg::No, b's' as i32),
    LongOpt::new("step", HasArg::Required, b'p' as i32),
    LongOpt::new("verbose", HasArg::No, b'v' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
    LongOpt::new("zeroout", HasArg::No, b'z' as i32),
];

const USAGE: &str = "
Usage:
 blkdiscard [options] <device>

Discard the content of sectors on a device.

Options:
 -f, --force         disable all checking
 -l, --length <num>  length of bytes to discard from the offset
 -o, --offset <num>  offset in bytes to discard from
 -p, --step <num>    size of the discard iterations within the offset
 -q, --quiet         suppress warning messages
 -s, --secure        perform secure discard
 -v, --verbose       print aligned length and offset
 -z, --zeroout       zero-fill rather than discard

 -h, --help          display this help
 -V, --version       display version

Arguments:
 Values for <num> may be followed by a suffix: KiB, MiB,
 GiB, TiB, PiB, EiB, ZiB, or YiB (where the \"iB\" is optional).

For more details see blkdiscard(8).
";

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut g = Getopt::from_env(&argv[1..], "fhl:o:p:qsvVz", LONGS);
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
            Some('f') | Some('q') | Some('s') | Some('v') | Some('z') => {}
            Some('l') | Some('o') | Some('p') => {
                let what = match o.short() {
                    Some('l') => "failed to parse length",
                    Some('o') => "failed to parse offset",
                    _ => "failed to parse step",
                };
                if let Err(m) = ul::strtosize_or_err(&arg, what) {
                    ul::warnx(&short, m);
                    return 1;
                }
            }
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

    let ops = g.operands();
    if ops.is_empty() {
        ul::warnx(&short, "no device specified");
        return 1;
    }
    if ops.len() > 1 {
        ul::warnx(&short, "unexpected number of arguments");
        ul::errtryhelp(&short);
        return 1;
    }
    let dev = &ops[0];
    if let Err(e) = io::File::open(dev) {
        ul::warn(&short, format!("cannot open {}", io::lossy(dev)), e);
        return 1;
    }
    let is_blk = sys::stat(dev).is_ok_and(|st| st.mode & 0o170000 == 0o060000);
    if !is_blk {
        ul::warnx(&short, format!("{}: not a block device", io::lossy(dev)));
        return 1;
    }
    ul::warnx(
        &short,
        format!("{}: BLKDISCARD ioctl failed: Function not implemented", io::lossy(dev)),
    );
    1
}
