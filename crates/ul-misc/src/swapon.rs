//! `swapon` do util-linux 2.41: ativa dispositivos e arquivos de swap.
//!
//! Porte do `sys-utils/swapon.c`. O `sysabi` não oferece `swapon(2)`: uma área válida termina como
//! num contêiner sem privilégio de swap (`Operation not permitted`). `--show` lê `/proc/swaps`.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Errno, sys};
use ul_common::fsutil::size_to_human_string;

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const LONGS: &[LongOpt] = &[
    LongOpt::new("all", HasArg::No, b'a' as i32),
    LongOpt::new("discard", HasArg::Optional, b'd' as i32),
    LongOpt::new("ifexists", HasArg::No, b'e' as i32),
    LongOpt::new("fixpgsz", HasArg::No, b'f' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("options", HasArg::Required, b'o' as i32),
    LongOpt::new("priority", HasArg::Required, b'p' as i32),
    LongOpt::new("summary", HasArg::No, b's' as i32),
    LongOpt::new("show", HasArg::Optional, 256),
    LongOpt::new("noheadings", HasArg::No, 257),
    LongOpt::new("raw", HasArg::No, 258),
    LongOpt::new("bytes", HasArg::No, 259),
    LongOpt::new("verbose", HasArg::No, b'v' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
];

const USAGE: &str = r#"
Usage:
 swapon [options] [<spec>]

Enable devices and files for paging and swapping.

Options:
 -a, --all                enable all swaps from /etc/fstab
 -d, --discard[=<policy>] enable swap discards, if supported by device
 -e, --ifexists           silently skip devices that do not exist
 -f, --fixpgsz            reinitialize the swap space if necessary
 -o, --options <list>     comma-separated list of swap options
 -p, --priority <prio>    specify the priority of the swap device
 -s, --summary            display summary about used swap devices (DEPRECATED)
 -T, --fstab <path>       alternative file to /etc/fstab
     --show[=<columns>]   display summary in definable table
     --noheadings         don't print table heading (with --show)
     --raw                use the raw output format (with --show)
     --bytes              display swap size in bytes in --show output
 -v, --verbose            verbose mode

 -h, --help               display this help
 -V, --version            display version

The <spec> parameter:
 -L <label>             synonym for LABEL=<label>
 -U <uuid>              synonym for UUID=<uuid>
 LABEL=<label>          specifies device by swap area label
 UUID=<uuid>            specifies device by swap area UUID
 PARTLABEL=<label>      specifies device by partition label
 PARTUUID=<uuid>        specifies device by partition UUID
 <device>               name of device to be used
 <file>                 name of file to be used

Available discard policy types (for --discard):
 once    : only single-time area discards are issued
 pages   : freed pages are discarded before they are reused
If no policy is selected, both discard types are enabled (default).

Available output columns:
 NAME   device file or partition path
 TYPE   type of the device
 SIZE   size of the swap area
 USED   bytes in use
 PRIO   swap priority
 UUID   swap uuid
 LABEL  swap label

For more details see swapon(8).
"#;

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn show(noheadings: bool, raw: bool, bytes: bool) {
    let data = sys::read_file(b"/proc/swaps").unwrap_or_default();
    let text = String::from_utf8_lossy(&data).into_owned();
    let mut rows: Vec<[String; 5]> = Vec::new();
    for line in text.lines().skip(1) {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 5 {
            continue;
        }
        let kb = |s: &str| s.parse::<u64>().unwrap_or(0) * 1024;
        let fmt = |n: u64| if bytes { n.to_string() } else { size_to_human_string(n, false, true) };
        rows.push([
            f[0].to_string(),
            f[1].to_string(),
            fmt(kb(f[2])),
            fmt(kb(f[3])),
            f[4].to_string(),
        ]);
    }
    if rows.is_empty() {
        return;
    }
    let heads = ["NAME", "TYPE", "SIZE", "USED", "PRIO"];
    let mut out = io::stdout();
    if raw {
        if !noheadings {
            let _ = out.write_all(format!("{}\n", heads.join(" ")).as_bytes());
        }
        for r in &rows {
            let _ = out.write_all(format!("{}\n", r.join(" ")).as_bytes());
        }
        return;
    }
    let mut w = [0usize; 5];
    for (i, h) in heads.iter().enumerate() {
        w[i] = h.len();
    }
    for r in &rows {
        for i in 0..5 {
            w[i] = w[i].max(r[i].chars().count());
        }
    }
    let line = |cells: Vec<&str>| {
        let mut s = String::new();
        for (i, c) in cells.iter().enumerate() {
            if i == 4 {
                s.push_str(&format!("{c:>width$}", width = w[i]));
            } else if i >= 2 && i < 4 {
                s.push_str(&format!("{c:>width$} ", width = w[i]));
            } else {
                s.push_str(&format!("{c:<width$} ", width = w[i]));
            }
        }
        s.push('\n');
        s
    };
    if !noheadings {
        let _ = out.write_all(line(heads.to_vec()).as_bytes());
    }
    for r in &rows {
        let _ = out.write_all(line(r.iter().map(String::as_str).collect()).as_bytes());
    }
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let (mut all, mut ifexists, mut status, mut do_show) = (false, false, false, false);
    let (mut noheadings, mut raw, mut bytes) = (false, false, false);
    let mut specs: Vec<Vec<u8>> = Vec::new();
    let mut g = Getopt::from_env(&argv[1..], "ad::efhL:o:p:svVU:", LONGS);
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
            Some('e') => ifexists = true,
            Some('s') => status = true,
            Some('d') => {
                if let Some(a) = &o.arg {
                    if a != b"once" && a != b"pages" {
                        ul::warnx(&short, format!("unsupported discard policy: {}", o.arg_str()));
                        return 1;
                    }
                }
            }
            Some('p') => {
                let a = o.arg_str();
                if a.parse::<i32>().is_err() {
                    ul::warnx(&short, format!("failed to parse priority: '{a}'"));
                    return 1;
                }
            }
            Some('L') => specs.push([b"LABEL=".as_slice(), o.arg.as_deref().unwrap_or(b"")].concat()),
            Some('U') => specs.push([b"UUID=".as_slice(), o.arg.as_deref().unwrap_or(b"")].concat()),
            Some('f') | Some('o') | Some('v') => {}
            Some('h') => {
                let mut out = io::stdout();
                let _ = out.write_all(USAGE.as_bytes());
                return 0;
            }
            Some('V') => {
                ul::print_version(&short);
                return 0;
            }
            _ => match o.id {
                256 => do_show = true,
                257 => noheadings = true,
                258 => raw = true,
                259 => bytes = true,
                _ => {
                    ul::errtryhelp(&short);
                    return 1;
                }
            },
        }
    }
    specs.extend(g.operands().iter().cloned());

    if do_show || status || (specs.is_empty() && !all) {
        show(noheadings, raw, bytes);
        return 0;
    }
    if all {
        // Sem /etc/fstab com entradas swap no sandbox: nada a ativar.
        return 0;
    }
    let mut rc = 0;
    for s in &specs {
        let name = io::lossy(s);
        let st = match sys::stat(s) {
            Ok(st) => st,
            Err(e) => {
                if ifexists && e == Errno::ENOENT {
                    continue;
                }
                ul::warn(&short, format!("cannot open {name}"), e);
                rc = 255;
                continue;
            }
        };
        if st.mode & 0o170000 == 0o040000 {
            ul::warnx(&short, format!("{name}: read swap header failed"));
            rc = 255;
            continue;
        }
        let data = io::File::open(s).and_then(|mut f| {
            let mut b = vec![0u8; 4096];
            let n = f.read_full(&mut b)?;
            b.truncate(n);
            Ok(b)
        });
        let data = match data {
            Ok(d) => d,
            Err(e) => {
                ul::warn(&short, format!("{name}: open failed"), e);
                rc = 255;
                continue;
            }
        };
        if data.len() < 4096 {
            ul::warnx(&short, format!("{name}: read swap header failed"));
            rc = 255;
            continue;
        }
        if st.mode & 0o170000 == 0o100000 && st.mode & 0o077 != 0 {
            ul::warnx(
                &short,
                format!("{name}: insecure permissions {:04o}, 0600 suggested.", st.mode & 0o7777),
            );
        }
        let valid = &data[4086..4096] == b"SWAPSPACE2" || &data[4086..4096] == b"SWAP-SPACE";
        let e = if valid { Errno::EPERM } else { Errno::EINVAL };
        ul::warn(&short, format!("{name}: swapon failed"), e);
        rc = 255;
    }
    rc
}
