//! `fstrim` do util-linux 2.41: descarta blocos não usados de um sistema de arquivos montado.
//!
//! O `FITRIM` exige `CAP_SYS_ADMIN`; num contêiner sem privilégio a ioctl falha com EPERM. Com
//! `-a` o porte percorre `/proc/self/mounts` e tenta os sistemas de arquivos com dispositivo de
//! bloco (`/dev/...`), como o original.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Errno, FileType, OFlags, sys};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const LONGS: &[LongOpt] = &[
    LongOpt::new("all", HasArg::No, b'a' as i32),
    LongOpt::new("fstab", HasArg::No, b'A' as i32),
    LongOpt::new("listed-in", HasArg::Required, b'I' as i32),
    LongOpt::new("offset", HasArg::Required, b'o' as i32),
    LongOpt::new("length", HasArg::Required, b'l' as i32),
    LongOpt::new("minimum", HasArg::Required, b'm' as i32),
    LongOpt::new("types", HasArg::Required, b't' as i32),
    LongOpt::new("verbose", HasArg::No, b'v' as i32),
    LongOpt::new("quiet-unsupported", HasArg::No, 256),
    LongOpt::new("dry-run", HasArg::No, b'n' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
];

const USAGE: &str = "
Usage:
 fstrim [options] <-A|-a|mount point>

Discard unused blocks on a mounted filesystem.

Options:
 -a, --all                trim mounted filesystems
 -A, --fstab              trim filesystems from /etc/fstab
 -I, --listed-in <list>   trim filesystems listed in specified files
 -o, --offset <num>       the offset in bytes to start discarding from
 -l, --length <num>       the number of bytes to discard
 -m, --minimum <num>      the minimum extent length to discard
 -t, --types <list>       limit the set of filesystem types
 -v, --verbose            print number of discarded bytes
     --quiet-unsupported  suppress error messages if trim unsupported
 -n, --dry-run            does everything, but trim

 -h, --help          display this help
 -V, --version       display version

Arguments:
 Values for <num> may be followed by a suffix: KiB, MiB,
 GiB, TiB, PiB, EiB, ZiB, or YiB (where the \"iB\" is optional).

For more details see fstrim(8).
";

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut all = false;
    let mut dry = false;
    let mut types: Option<Vec<u8>> = None;
    let mut g = Getopt::from_env(&argv[1..], "AahI:l:m:no:t:vV", LONGS);
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
            Some('a') => all = true,
            Some('A') | Some('I') => all = true,
            Some('n') => dry = true,
            Some('v') => {}
            Some('t') => types = Some(arg),
            Some('o') | Some('l') | Some('m') => {
                let what = match o.short() {
                    Some('o') => "failed to parse offset",
                    Some('l') => "failed to parse length",
                    _ => "failed to parse minimum extent length",
                };
                if let Err(m) = ul::strtosize_or_err(&arg, what) {
                    ul::warnx(&short, m);
                    return 1;
                }
            }
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
                if o.id == 256 {
                    continue;
                }
                ul::errtryhelp(&short);
                return 1;
            }
        }
    }
    let ops = g.operands();
    if !all && ops.is_empty() {
        ul::warnx(&short, "no mountpoint specified");
        return 1;
    }
    if all && !ops.is_empty() {
        ul::warnx(&short, "options --{all,fstab,listed-in} and <mountpoint> are mutually exclusive");
        ul::errtryhelp(&short);
        return 1;
    }
    if !all {
        if ops.len() > 1 {
            ul::warnx(&short, "unexpected number of arguments");
            ul::errtryhelp(&short);
            return 1;
        }
        return trim_one(&short, &ops[0], dry);
    }

    let mounts = io::read_path(b"/proc/self/mounts").unwrap_or_default();
    let wanted: Option<Vec<String>> = types
        .as_ref()
        .map(|t| io::lossy(t).split(',').map(str::to_string).collect());
    let mut failed = false;
    let mut seen: Vec<Vec<u8>> = Vec::new();
    for line in mounts.split(|b| *b == b'\n') {
        let mut f = line.split(|b| *b == b' ');
        let (Some(src), Some(tgt), Some(fs)) = (f.next(), f.next(), f.next()) else {
            continue;
        };
        if !src.starts_with(b"/dev/") || seen.iter().any(|s| s == src) {
            continue;
        }
        if let Some(w) = &wanted
            && !w.iter().any(|t| t.as_bytes() == fs)
        {
            continue;
        }
        seen.push(src.to_vec());
        if trim_one(&short, tgt, dry) != 0 {
            failed = true;
        }
    }
    if failed { 32 } else { 0 }
}

fn trim_one(short: &str, path: &[u8], dry: bool) -> i32 {
    let st = match sys::stat(path) {
        Ok(st) => st,
        Err(e) => {
            ul::warn(short, format!("stat of {} failed", io::lossy(path)), e);
            return 1;
        }
    };
    let is_dir = st.file_type() == FileType::Directory;
    if !is_dir {
        ul::warnx(short, format!("{}: not a directory", io::lossy(path)));
        return 1;
    }
    let fd = match sys::open(path, OFlags::RDONLY, 0) {
        Ok(fd) => fd,
        Err(e) => {
            ul::warn(short, format!("cannot open {}", io::lossy(path)), e);
            return 1;
        }
    };
    let _ = sys::close(fd);
    if dry {
        let mut out = io::stdout();
        let _ = out.write_all(format!("{}: 0 B (dry run) trimmed\n", io::lossy(path)).as_bytes());
        return 0;
    }
    ul::warn(
        short,
        format!("{}: FITRIM ioctl failed", io::lossy(path)),
        Errno::EPERM,
    );
    1
}
