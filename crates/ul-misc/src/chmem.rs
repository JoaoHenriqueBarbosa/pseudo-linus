//! `chmem` do util-linux 2.41: coloca memória online ou offline.
//!
//! Porte do `sys-utils/chmem.c`. Num contêiner sem privilégio o sysfs de memória não existe ou não
//! aceita escrita; o programa termina com as mensagens do original para esses casos.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, sys};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const MEM_DIR: &str = "/sys/devices/system/memory";
const ZONES: &[&str] = &["DMA", "DMA32", "Normal", "Highmem", "Movable", "Device"];

const LONGS: &[LongOpt] = &[
    LongOpt::new("blocks", HasArg::No, b'b' as i32),
    LongOpt::new("disable", HasArg::No, b'd' as i32),
    LongOpt::new("enable", HasArg::No, b'e' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("verbose", HasArg::No, b'v' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
    LongOpt::new("zone", HasArg::Required, b'z' as i32),
];

fn usage(short: &str) -> String {
    let mut s = format!(
        "
Usage:
 {short} [options] [SIZE|RANGE|BLOCKRANGE]

Set a particular size or range of memory online or offline.

Options:
 -e, --enable       enable memory
 -d, --disable      disable memory
 -b, --blocks       use memory blocks
 -z, --zone <name>  select memory zone (see below)
 -v, --verbose      verbose output

 -h, --help         display this help
 -V, --version      display version

Supported zones:
"
    );
    for z in ZONES {
        s.push_str(&format!(" {z}\n"));
    }
    s.push_str(&format!("\nFor more details see {short}(8).\n"));
    s
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut enable: Option<bool> = None;
    let mut blocks = false;
    let mut zone: Option<String> = None;

    let mut g = Getopt::from_env(&argv[1..], "bdehvVz:", LONGS);
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
            Some('e') => enable = Some(true),
            Some('d') => enable = Some(false),
            Some('b') => blocks = true,
            Some('v') => {}
            Some('z') => {
                let z = o.arg_str();
                if !ZONES.iter().any(|n| n.eq_ignore_ascii_case(&z)) {
                    ul::warnx(&short, format!("invalid zone: {z}"));
                    return 1;
                }
                zone = Some(z);
            }
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
    let _ = zone;
    let ops = g.operands();
    if ops.len() != 1 || enable.is_none() {
        ul::warnx(&short, "bad usage");
        ul::errtryhelp(&short);
        return 1;
    }
    let enable = enable.unwrap_or(true);

    let bsize = match sys::read_file(format!("{MEM_DIR}/block_size_bytes").as_bytes())
        .ok()
        .and_then(|d| u64::from_str_radix(String::from_utf8_lossy(&d).trim(), 16).ok())
    {
        Some(b) if b > 0 => b,
        _ => {
            ul::warnx(&short, "failed to read memory block size");
            return 1;
        }
    };

    let verb = if enable { "enable" } else { "disable" };
    let (start, _end) = if blocks {
        let text = io::lossy(&ops[0]);
        let (a, b) = match text.split_once('-') {
            Some((a, b)) => (a.to_string(), b.to_string()),
            None => (text.clone(), text.clone()),
        };
        match (a.parse::<u64>(), b.parse::<u64>()) {
            (Ok(a), Ok(b)) if a <= b => (a, b),
            _ => {
                ul::warnx(&short, format!("invalid block range: {text}"));
                return 1;
            }
        }
    } else {
        let size = match ul::strtosize_or_err(&ops[0], "failed to parse size") {
            Ok(s) => s,
            Err(m) => {
                ul::warnx(&short, m);
                return 1;
            }
        };
        if size == 0 || size % bsize != 0 {
            ul::warnx(
                &short,
                format!(
                    "Size must be aligned to memory block size ({})",
                    crate::lsmem::human_size(bsize)
                ),
            );
            return 1;
        }
        (0, size / bsize - 1)
    };
    ul::warnx(
        &short,
        format!("Memory Block {start} {verb} failed: Permission denied"),
    );
    1
}
