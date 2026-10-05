//! `losetup` do util-linux 2.41: configura e controla dispositivos loop.
//!
//! Porte do `sys-utils/losetup.c`. O `sysabi` não oferece as ioctls de loop e o sandbox não tem
//! `/dev/loop-control`: listar não mostra nada, e procurar, configurar ou desanexar responde com o
//! erro de quem não encontra o dispositivo, como o original num contêiner.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Errno, sys};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const LONGS: &[LongOpt] = &[
    LongOpt::new("all", HasArg::No, b'a' as i32),
    LongOpt::new("set-capacity", HasArg::Required, b'c' as i32),
    LongOpt::new("detach", HasArg::Required, b'd' as i32),
    LongOpt::new("detach-all", HasArg::No, b'D' as i32),
    LongOpt::new("find", HasArg::No, b'f' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("associated", HasArg::Required, b'j' as i32),
    LongOpt::new("json", HasArg::No, b'J' as i32),
    LongOpt::new("list", HasArg::No, b'l' as i32),
    LongOpt::new("noheadings", HasArg::No, b'n' as i32),
    LongOpt::new("nooverlap", HasArg::No, b'L' as i32),
    LongOpt::new("offset", HasArg::Required, b'o' as i32),
    LongOpt::new("output", HasArg::Required, b'O' as i32),
    LongOpt::new("output-all", HasArg::No, 256),
    LongOpt::new("sizelimit", HasArg::Required, 257),
    LongOpt::new("sector-size", HasArg::Required, 258),
    LongOpt::new("partscan", HasArg::No, b'P' as i32),
    LongOpt::new("read-only", HasArg::No, b'r' as i32),
    LongOpt::new("raw", HasArg::No, 259),
    LongOpt::new("show", HasArg::No, b's' as i32),
    LongOpt::new("verbose", HasArg::No, b'v' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
    LongOpt::new("direct-io", HasArg::Optional, 260),
    LongOpt::new("loop-ref", HasArg::Required, 261),
];

const USAGE: &str = "
Usage:
 losetup [options] [<loopdev>]
 losetup [options] -f | <loopdev> <file>

Set up and control loop devices.

Options:
 -a, --all                     list all used devices
 -d, --detach <loopdev>...     detach one or more devices
 -D, --detach-all              detach all used devices
 -f, --find                    find first unused device
 -c, --set-capacity <loopdev>  resize the device
 -j, --associated <file>       list all devices associated with <file>
 -L, --nooverlap               avoid possible conflicts between devices
     --direct-io[=<on|off>]    open backing file with O_DIRECT
     --loop-ref <string>       specify device reference (kernel 6.x)
 -o, --offset <num>            start at offset <num> into file
     --sizelimit <num>         device limited to <num> bytes of the file
     --sector-size <num>       set the logical sector size to <num>
 -P, --partscan                create a partitioned loop device
 -r, --read-only               set up a read-only loop device
     --show                    print device name after setup (with -f)
 -v, --verbose                 verbose mode

 -l, --list                    list all devices (default)
 -O, --output <cols>           specify columns to output for --list
     --output-all              output all columns
 -n, --noheadings              don't print headings for --list output
 -J, --json                    use JSON --list output format
     --raw                     use raw --list output format

 -h, --help                    display this help
 -V, --version                 display version

Available output columns:
         NAME  loop device name
    AUTOCLEAR  autoclear flag set
    BACK-FILE  device backing file
     BACK-INO  backing file inode number
 BACK-MAJ:MIN  backing file major:minor device number
      MAJ:MIN  loop device major:minor number
       OFFSET  offset from the beginning
     PARTSCAN  partscan flag set
           RO  read-only device
    SIZELIMIT  size limit of the file in bytes
          DIO  access backing file with direct-io
      LOG-SEC  logical sector size in bytes

For more details see losetup(8).
";

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let (mut find, mut detach_all, mut list, mut json) = (false, false, false, false);
    let mut detach: Vec<Vec<u8>> = Vec::new();
    let mut capacity: Vec<Vec<u8>> = Vec::new();
    let mut assoc: Option<Vec<u8>> = None;
    let mut g = Getopt::from_env(&argv[1..], "acd:Dfhj:JlLno:O:PrsvV", LONGS);
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
            Some('a') | Some('l') => list = true,
            Some('c') => capacity.push(o.arg.clone().unwrap_or_default()),
            Some('d') => detach.push(o.arg.clone().unwrap_or_default()),
            Some('D') => detach_all = true,
            Some('f') => find = true,
            Some('j') => assoc = o.arg.clone(),
            Some('J') => json = true,
            Some('o') => {
                if let Err(m) = ul::strtosize_or_err(o.arg.as_deref().unwrap_or(b""), "failed to parse offset") {
                    ul::warnx(&short, m);
                    return 1;
                }
            }
            Some('O') | Some('L') | Some('n') | Some('P') | Some('r') | Some('s') | Some('v') => {}
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
                257 => {
                    if let Err(m) = ul::strtosize_or_err(o.arg.as_deref().unwrap_or(b""), "failed to parse size limit") {
                        ul::warnx(&short, m);
                        return 1;
                    }
                }
                258 => {
                    if let Err(m) = ul::strtou64_or_err(o.arg.as_deref().unwrap_or(b""), "failed to parse logical block size") {
                        ul::warnx(&short, m);
                        return 1;
                    }
                }
                256 | 259 | 260 | 261 => {}
                _ => {
                    ul::errtryhelp(&short);
                    return 1;
                }
            },
        }
    }
    let ops: Vec<Vec<u8>> = g.operands().to_vec();

    if detach_all {
        return 0;
    }
    if !detach.is_empty() {
        let mut rc = 0;
        for d in detach.iter().chain(ops.iter()) {
            let e = sys::stat(d).err().unwrap_or(Errno::EINVAL);
            ul::warn(&short, format!("{}: detach failed", io::lossy(d)), e);
            rc = 1;
        }
        return rc;
    }
    if !capacity.is_empty() {
        let mut rc = 1;
        for d in &capacity {
            let e = sys::stat(d).err().unwrap_or(Errno::EINVAL);
            ul::warn(&short, format!("{}: set capacity failed", io::lossy(d)), e);
            rc = 1;
        }
        return rc;
    }
    if let Some(f) = assoc {
        // Sem dispositivos loop ativos nada está associado ao arquivo.
        let _ = f;
        return 0;
    }
    if find {
        if ops.is_empty() {
            ul::warn(&short, "cannot find an unused loop device", Errno::ENOENT);
        } else {
            let file = io::lossy(&ops[0]);
            if let Err(e) = sys::stat(&ops[0]) {
                ul::warn(&short, format!("{file}: failed to use backing file"), e);
            } else {
                ul::warn(&short, "cannot find an unused loop device", Errno::ENOENT);
            }
        }
        return 1;
    }
    if ops.len() == 2 {
        let dev = io::lossy(&ops[0]);
        if let Err(e) = sys::stat(&ops[1]) {
            ul::warn(&short, format!("{}: failed to use backing file", io::lossy(&ops[1])), e);
            return 1;
        }
        let e = sys::stat(&ops[0]).err().unwrap_or(Errno::EPERM);
        ul::warn(&short, format!("{dev}: failed to set up loop device"), e);
        return 1;
    }
    if ops.len() == 1 {
        let dev = io::lossy(&ops[0]);
        let e = sys::stat(&ops[0]).err().unwrap_or(Errno::ENODEV);
        ul::warn(&short, format!("{dev}: failed to use device"), e);
        return 1;
    }
    if ops.len() > 2 {
        ul::warnx(&short, "unexpected arguments");
        ul::errtryhelp(&short);
        return 1;
    }
    // Lista (padrão): sem dispositivos loop em uso não há saída.
    let _ = (list, json);
    0
}
