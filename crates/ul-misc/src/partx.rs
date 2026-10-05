//! `partx` do util-linux 2.41: informa ao kernel as partições de um disco.
//!
//! Porte do `disk-utils/partx.c`. O `sysabi` não oferece as ioctls de partição nem a leitura por
//! libblkid: um alvo que abre mas não é dispositivo de bloco (o único caso do sandbox) responde que
//! não é um dispositivo de bloco, como o original.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, sys};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const LONGS: &[LongOpt] = &[
    LongOpt::new("add", HasArg::No, b'a' as i32),
    LongOpt::new("bytes", HasArg::No, b'b' as i32),
    LongOpt::new("delete", HasArg::No, b'd' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("noheadings", HasArg::No, b'g' as i32),
    LongOpt::new("nr", HasArg::Required, b'n' as i32),
    LongOpt::new("output", HasArg::Required, b'o' as i32),
    LongOpt::new("output-all", HasArg::No, 256),
    LongOpt::new("raw", HasArg::No, b'r' as i32),
    LongOpt::new("show", HasArg::No, b's' as i32),
    LongOpt::new("type", HasArg::Required, b't' as i32),
    LongOpt::new("update", HasArg::No, b'u' as i32),
    LongOpt::new("verbose", HasArg::No, b'v' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
];

const USAGE: &str = "
Usage:
 partx <partition> [<disk>]
 partx <command> [options] <disk> | <partition>

Tell the kernel about the presence and numbering of on-disk partitions.

Commands:
 -a, --add            add specified partitions or all of them
 -d, --delete         delete specified partitions or all of them
 -u, --update         update specified partitions or all of them
 -s, --show           list partitions

Options:
 -b, --bytes          print SIZE in bytes rather than in human readable format
 -g, --noheadings     don't print headings for --show
 -n, --nr <n:m>       specify the range of partitions (e.g. --nr 2:4)
 -o, --output <type>  define which output columns to use
     --output-all     output all columns
 -r, --raw            use raw output format
 -t, --type <type>    specify the partition table type
 -v, --verbose        verbose mode

 -h, --help           display this help
 -V, --version        display version

Available output columns:
    NR  partition number
 START  start of the partition in sectors
   END  end of the partition in sectors
SECTORS  number of sectors
  SIZE  human readable size
  NAME  partition name
  UUID  partition UUID
  TYPE  partition table type (a string, a UUID, or hex)
 FLAGS  partition flags
SCHEME  partition table type (dos, gpt, ...)

For more details see partx(8).
";

const COLUMNS: &[&str] = &[
    "NR", "START", "END", "SECTORS", "SIZE", "NAME", "UUID", "TYPE", "FLAGS", "SCHEME",
];

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut cmd: Option<char> = None;
    let mut g = Getopt::from_env(&argv[1..], "abdghn:o:rst:uvV", LONGS);
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
            Some(c @ ('a' | 'd' | 'u' | 's')) => cmd = Some(c),
            Some('o') => {
                for col in o.arg_str().split(',') {
                    let name = col.trim_start_matches('+');
                    if !COLUMNS.iter().any(|c| c.eq_ignore_ascii_case(name)) {
                        ul::warnx(&short, format!("unknown column: {name}"));
                        return 1;
                    }
                }
            }
            Some('n') => {
                let a = o.arg_str();
                let ok = a.split(':').count() <= 2
                    && a.split(':').all(|p| p.is_empty() || p.parse::<u32>().is_ok());
                if !ok {
                    ul::warnx(&short, format!("failed to parse partition range: '{a}'"));
                    return 1;
                }
            }
            Some('t') | Some('b') | Some('g') | Some('r') | Some('v') => {}
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
                if o.id == 256 {
                    continue;
                }
                ul::errtryhelp(&short);
                return 1;
            }
        }
    }
    let ops = g.operands();
    if ops.is_empty() {
        ul::warnx(&short, "bad usage");
        ul::errtryhelp(&short);
        return 1;
    }
    if ops.len() > 2 {
        ul::warnx(&short, "bad usage");
        ul::errtryhelp(&short);
        return 1;
    }
    let dev = &ops[0];
    let name = io::lossy(dev);
    let st = match sys::stat(dev) {
        Ok(s) => s,
        Err(e) => {
            ul::warn(&short, format!("{name}: failed to stat"), e);
            return 1;
        }
    };
    if st.mode & 0o170000 != 0o060000 {
        ul::warnx(&short, format!("{name}: not a block device"));
        return 1;
    }
    let _ = cmd;
    ul::warn(&short, format!("{name}: failed to read partition table"), sysabi::Errno::ENOSYS);
    1
}
