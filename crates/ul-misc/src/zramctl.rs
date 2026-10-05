//! `zramctl` do util-linux 2.41: configura e consulta dispositivos zram.
//!
//! Porte do `sys-utils/zramctl.c`. O sandbox não tem o módulo zram nem `/dev/zramN`: a listagem
//! mostra só o cabeçalho, `--find` não acha dispositivo livre e abrir um dispositivo falha como o
//! original.

use std::ffi::OsString;
use std::io::Write;

use sysabi::Ctx;

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const COLUMNS: &[(&str, &str)] = &[
    ("NAME", "zram device name"),
    ("DISKSIZE", "limit on the uncompressed amount of data"),
    ("DATA", "uncompressed size of stored data"),
    ("COMPR", "compressed size of stored data"),
    ("ALGORITHM", "the selected compression algorithm"),
    ("STREAMS", "number of concurrent compress operations"),
    ("ZERO-PAGES", "empty pages with no data"),
    ("TOTAL", "all memory including allocator fragmentation and metadata overhead"),
    ("MEM-LIMIT", "memory limit used to store compressed data"),
    ("MEM-USED", "memory zram have been consumed to store compressed data"),
    ("MIGRATED", "number of objects migrated by compaction"),
    ("COMP-RATIO", "compression ratio: DATA/TOTAL"),
    ("MOUNTPOINT", "where the device is mounted"),
];

const DEFAULT_COLS: &[&str] = &[
    "NAME", "ALGORITHM", "DISKSIZE", "DATA", "COMPR", "TOTAL", "STREAMS", "MOUNTPOINT",
];

const LONGS: &[LongOpt] = &[
    LongOpt::new("algorithm", HasArg::Required, b'a' as i32),
    LongOpt::new("algorithm-params", HasArg::Required, 0x100),
    LongOpt::new("bytes", HasArg::No, b'b' as i32),
    LongOpt::new("find", HasArg::No, b'f' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("noheadings", HasArg::No, b'n' as i32),
    LongOpt::new("output", HasArg::Required, b'o' as i32),
    LongOpt::new("output-all", HasArg::No, 0x101),
    LongOpt::new("raw", HasArg::No, 0x102),
    LongOpt::new("reset", HasArg::No, b'r' as i32),
    LongOpt::new("size", HasArg::Required, b's' as i32),
    LongOpt::new("streams", HasArg::Required, b't' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
];

fn usage(_short: &str) -> String {
    r#"
Usage:
 zramctl [options] <device>
 zramctl -r <device> [...]
 zramctl [options] -f | <device> -s <size>

Set up and control zram devices.

Options:
 -a, --algorithm <alg>              compression algorithm to use
 -b, --bytes                        print sizes in bytes rather than in human readable format
 -f, --find                         find a free device
 -n, --noheadings                   don't print headings
 -o, --output <list>                columns to use for status output
     --output-all                   output all columns
 -p, --algorithm-params <params>    algorithm parameters to use
     --raw                          use raw status output format
 -r, --reset                        reset all specified devices
 -s, --size <size>                  device size
 -t, --streams <number>             number of compression streams

 -h, --help                display this help
 -V, --version             display version

Arguments:
 Values for <size> may be followed by a suffix: KiB, MiB,
 GiB, TiB, PiB, EiB, ZiB, or YiB (where the "iB" is optional).
 <alg> is the name of an algorithm; supported are:
   lzo, lz4, lz4hc, deflate, 842, zstd
   (List may be inaccurate, consult man page.)

Available output columns:
        NAME  zram device name
    DISKSIZE  limit on the uncompressed amount of data
        DATA  uncompressed size of stored data
       COMPR  compressed size of stored data
   ALGORITHM  the selected compression algorithm
     STREAMS  number of concurrent compress operations
  ZERO-PAGES  empty pages with no allocated memory
       TOTAL  all memory including allocator fragmentation and metadata overhead
   MEM-LIMIT  memory limit used to store compressed data
    MEM-USED  peak memory usage to store compressed data
    MIGRATED  number of objects migrated by compaction
  COMP-RATIO  compression ratio: DATA/TOTAL
  MOUNTPOINT  where the device is mounted

For more details see zramctl(8).
"#.to_string()
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut cols: Vec<&'static str> = DEFAULT_COLS.to_vec();
    let mut find = false;
    let mut reset = false;
    let mut noheadings = false;
    let mut have_size = false;
    let mut want_setup = false;

    let mut g = Getopt::from_env(&argv[1..], "a:bfhno:rs:t:V", LONGS);
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
            x if x == b'h' as i32 => {
                let mut out = io::stdout();
                let _ = out.write_all(usage(&short).as_bytes());
                return 0;
            }
            x if x == b'V' as i32 => {
                ul::print_version(&short);
                return 0;
            }
            x if x == b'f' as i32 => find = true,
            x if x == b'r' as i32 => reset = true,
            x if x == b'n' as i32 => noheadings = true,
            x if x == 0x101 => cols = COLUMNS.iter().map(|c| c.0).collect(),
            x if x == b'a' as i32 || x == 0x100 || x == b't' as i32 => {
                want_setup = true;
                if x == b't' as i32 {
                    if let Err(m) = ul::strtou64_or_err(
                        &o.arg.clone().unwrap_or_default(),
                        "failed to parse streams",
                    ) {
                        ul::warnx(&short, m);
                        return 1;
                    }
                }
            }
            x if x == b's' as i32 => {
                match ul::strtosize_or_err(&o.arg.clone().unwrap_or_default(), "failed to parse size") {
                    Ok(_) => have_size = true,
                    Err(m) => {
                        ul::warnx(&short, m);
                        return 1;
                    }
                }
            }
            x if x == b'o' as i32 => {
                let list = o.arg_str();
                let mut parsed: Vec<&'static str> = Vec::new();
                let mut rest = list.as_str();
                if let Some(r) = rest.strip_prefix('+') {
                    parsed = cols.clone();
                    rest = r;
                }
                for name in rest.split(',').filter(|n| !n.is_empty()) {
                    match COLUMNS.iter().find(|c| c.0.eq_ignore_ascii_case(name)) {
                        Some(c) => parsed.push(c.0),
                        None => {
                            ul::warnx(&short, format!("unknown column: {name}"));
                            return 1;
                        }
                    }
                }
                cols = parsed;
            }
            _ => {}
        }
    }
    let ops = g.operands();

    if find && !ops.is_empty() {
        ul::warnx(&short, "option --find is mutually exclusive with <device>");
        ul::errtryhelp(&short);
        return 1;
    }
    if find && !have_size {
        ul::warnx(&short, "option --find requires option --size");
        ul::errtryhelp(&short);
        return 1;
    }
    if reset && ops.is_empty() {
        ul::warnx(&short, "no device specified");
        ul::errtryhelp(&short);
        return 1;
    }
    if (have_size || want_setup) && !find && ops.is_empty() {
        ul::warnx(&short, "no device specified");
        ul::errtryhelp(&short);
        return 1;
    }
    if find {
        ul::warnx(&short, "no free zram device found");
        return 1;
    }
    if !ops.is_empty() {
        for d in &ops {
            if let Err(e) = io::File::open(d) {
                ul::warn(&short, format!("cannot open {}", io::lossy(d)), e);
                return 1;
            }
        }
        return 0;
    }

    let mut out = String::new();
    if !noheadings {
        out.push_str(&cols.join(" "));
        out.push('\n');
    }
    let mut so = io::stdout();
    let _ = so.write_all(out.as_bytes());
    0
}
