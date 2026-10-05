//! `readprofile` do util-linux 2.41: mostra as amostras de profiling do kernel.
//!
//! Porte do `sys-utils/readprofile.c`. O sandbox não tem `/proc/profile` (kernel sem `profile=`):
//! o programa falha ao abri-lo, como o original, depois de validar as opções.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, OFlags};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const LONGS: &[LongOpt] = &[
    LongOpt::new("mapfile", HasArg::Required, b'm' as i32),
    LongOpt::new("profile", HasArg::Required, b'p' as i32),
    LongOpt::new("multiplier", HasArg::Required, b'M' as i32),
    LongOpt::new("info", HasArg::No, b'i' as i32),
    LongOpt::new("verbose", HasArg::No, b'v' as i32),
    LongOpt::new("all", HasArg::No, b'a' as i32),
    LongOpt::new("histbin", HasArg::No, b'b' as i32),
    LongOpt::new("counters", HasArg::No, b's' as i32),
    LongOpt::new("reset", HasArg::No, b'r' as i32),
    LongOpt::new("no-auto", HasArg::No, b'n' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
];

fn usage(_short: &str) -> String {
    r#"
Usage:
 readprofile [options]

Display kernel profiling information.

Options:
 -m, --mapfile <mapfile>   (defaults: "/boot/System.map" and
                                      "/boot/System.map-6.12.101+deb13-amd64")
 -p, --profile <pro-file>  (default:  "/proc/profile")
 -M, --multiplier <mult>   set the profiling multiplier to <mult>
 -i, --info                print only info about the sampling step
 -v, --verbose             print verbose data
 -a, --all                 print all symbols, even if count is 0
 -b, --histbin             print individual histogram-bin counts
 -s, --counters            print individual counters within functions
 -r, --reset               reset all the counters (root only)
 -n, --no-auto             disable byte order auto-detection

 -h, --help                display this help
 -V, --version             display version

For more details see readprofile(8).
"#.to_string()
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut map = b"/boot/System.map".to_vec();
    let mut profile = b"/proc/profile".to_vec();
    let mut reset = false;
    let mut multiplier = false;

    let mut g = Getopt::from_env(&argv[1..], "m:p:M:ivabsrnhV", LONGS);
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
            Some('m') => map = o.arg.clone().unwrap_or_default(),
            Some('p') => profile = o.arg.clone().unwrap_or_default(),
            Some('M') => {
                if let Err(m) = ul::strtou32_or_err(&o.arg.clone().unwrap_or_default(), "invalid multiplier") {
                    ul::warnx(&short, m);
                    return 1;
                }
                multiplier = true;
            }
            Some('r') => reset = true,
            Some('i') | Some('v') | Some('a') | Some('b') | Some('s') | Some('n') => {}
            Some('h') => {
                let mut out = io::stdout();
                let _ = out.write_all(usage(&short).as_bytes());
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
    if !g.operands().is_empty() {
        ul::errtryhelp(&short);
        return 1;
    }

    // Reset e multiplicador gravam em /proc/profile; o resto só lê.
    if reset || multiplier {
        let path = b"/proc/profile";
        if let Err(e) = io::File::open_with(path, OFlags::WRONLY, 0) {
            ul::warn(&short, "error opening /proc/profile", e);
        } else {
            ul::warnx(&short, "error writing /proc/profile: Permission denied");
        }
        return 1;
    }
    if let Err(e) = io::File::open(&profile) {
        ul::warn(&short, format!("error opening {}", io::lossy(&profile)), e);
        return 1;
    }
    if let Err(e) = io::File::open(&map) {
        ul::warn(&short, format!("error opening {}", io::lossy(&map)), e);
        return 1;
    }
    ul::warnx(&short, "input file is too short to be a profile");
    1
}
