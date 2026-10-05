//! `blockdev` do util-linux 2.41: chama ioctls de dispositivo de bloco pela linha de comando.
//!
//! Porte do `sys-utils/blockdev.c`. Parsing de comandos, mensagens e códigos de saída idênticos ao
//! original. O `sysabi` não expõe ioctls de bloco: um dispositivo que abre mas não é de bloco (o
//! único caso que o sandbox produz) responde `Inappropriate ioctl for device` ao primeiro comando,
//! como o kernel faz.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Errno, sys};

use crate::util::io;
use crate::util::ul;

const USAGE: &str = "
Usage:
 blockdev [-v|-q] commands devices
 blockdev --report [devices]
 blockdev -h|-V

Call block device ioctls from the command line.

Options:
 -q             quiet mode
 -v             verbose mode
     --report   print report for specified (or all) devices

 -h, --help     display this help
 -V, --version  display version

Available commands:
 --getsz                   get size in 512-byte sectors
 --setro                   set read-only
 --setrw                   set read-write
 --getro                   get read-only
 --getdiscardzeroes        get discard zeroes support status
 --getss                   get logical block (sector) size
 --getpbsz                 get physical block (sector) size
 --getiomin                get minimum I/O size
 --getioopt                get optimal I/O size
 --getalignoff             get alignment offset in bytes
 --getmaxsect              get max sectors per request
 --getbsz                  get blocksize
 --setbsz <bytes>          set blocksize on file descriptor opening the block device
 --getsize                 get 32-bit sector count (deprecated, use --getsz)
 --getsize64               get size in bytes
 --setra <sectors>         set readahead
 --getra                   get readahead
 --setfra <sectors>        set filesystem readahead
 --getfra                  get filesystem readahead
 --getdiskseq              get disk sequence number
 --getzonesz               get zone size
 --flushbufs               flush buffers
 --rereadpt                reread partition table

For more details see blockdev(8).
";

/// Comando, nome da ioctl (para a mensagem de erro) e se leva argumento.
const COMMANDS: &[(&str, &str, bool)] = &[
    ("--setro", "BLKROSET", false),
    ("--setrw", "BLKROSET", false),
    ("--getro", "BLKROGET", false),
    ("--getdiscardzeroes", "BLKDISCARDZEROES", false),
    ("--getss", "BLKSSZGET", false),
    ("--getpbsz", "BLKPBSZGET", false),
    ("--getiomin", "BLKIOMIN", false),
    ("--getioopt", "BLKIOOPT", false),
    ("--getalignoff", "BLKALIGNOFF", false),
    ("--getmaxsect", "BLKSECTGET", false),
    ("--getbsz", "BLKBSZGET", false),
    ("--setbsz", "BLKBSZSET", true),
    ("--getsize", "BLKGETSIZE", false),
    ("--getsize64", "BLKGETSIZE64", false),
    ("--setra", "BLKRASET", true),
    ("--getra", "BLKRAGET", false),
    ("--setfra", "BLKFRASET", true),
    ("--getfra", "BLKFRAGET", false),
    ("--getdiskseq", "BLKGETDISKSEQ", false),
    ("--getzonesz", "BLKGETZONESZ", false),
    ("--flushbufs", "BLKFLSBUF", false),
    ("--rereadpt", "BLKRRPART", false),
    ("--getsz", "", false),
];

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn parse_arg(arg: &[u8]) -> Option<i64> {
    let s = std::str::from_utf8(arg).ok()?;
    s.parse::<i64>().ok()
}

/// Abre o dispositivo; erro é `cannot open <dev>: <strerror>` e saída 1.
fn open_dev(short: &str, dev: &[u8]) -> Result<io::File, i32> {
    io::File::open(dev).map_err(|e| {
        ul::warn(short, format!("cannot open {}", io::lossy(dev)), e);
        1
    })
}

fn report_header() {
    let mut out = io::stdout();
    let _ = out.write_all(b"RO    RA   SSZ   BSZ        StartSec            Size   Device\n");
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let short = ul::short_name(args);

    if argv.len() < 2 {
        ul::warnx(&short, "not enough arguments");
        ul::errtryhelp(&short);
        return 1;
    }
    match argv[1].as_slice() {
        b"-V" | b"--version" => {
            ul::print_version(&short);
            return 0;
        }
        b"-h" | b"--help" => {
            let mut out = io::stdout();
            let _ = out.write_all(USAGE.as_bytes());
            return 0;
        }
        _ => {}
    }

    if argv[1] == b"--report" {
        report_header();
        let mut devs: Vec<Vec<u8>> = argv[2..].to_vec();
        if devs.is_empty() {
            if let Ok(d) = sys::read_file(b"/proc/partitions") {
                for line in String::from_utf8_lossy(&d).lines().skip(2) {
                    let f: Vec<&str> = line.split_whitespace().collect();
                    if f.len() == 4 {
                        devs.push(format!("/dev/{}", f[3]).into_bytes());
                    }
                }
            }
        }
        for d in &devs {
            if let Err(c) = open_dev(&short, d) {
                return c;
            }
            // O RO/RA/SSZ... vêm de ioctls de bloco: num arquivo que não é de bloco elas falham.
            ul::warnx(&short, format!("ioctl error on {}", io::lossy(d)));
            return 1;
        }
        return 0;
    }

    // Comandos (e -q/-v) até o primeiro argumento que não é opção; o resto são dispositivos.
    let mut cmds: Vec<(&'static str, &'static str, Option<i64>)> = Vec::new();
    let mut j = 1;
    while j < argv.len() {
        let a = argv[j].as_slice();
        if let Some((name, ioctl, takes)) = COMMANDS.iter().find(|c| c.0.as_bytes() == a) {
            let mut val = None;
            if *takes {
                if j + 1 < argv.len() {
                    j += 1;
                    match parse_arg(&argv[j]) {
                        Some(n) => val = Some(n),
                        None => {
                            ul::warnx(
                                &short,
                                format!("failed to parse command argument: '{}'", io::lossy(&argv[j])),
                            );
                            return 1;
                        }
                    }
                } else {
                    break;
                }
            }
            cmds.push((name, ioctl, val));
            j += 1;
        } else if a.first() == Some(&b'-') {
            j += 1;
        } else {
            break;
        }
    }
    if j >= argv.len() {
        ul::warnx(&short, "no device specified");
        ul::errtryhelp(&short);
        return 1;
    }

    for dev in &argv[j..] {
        let file = match open_dev(&short, dev) {
            Ok(f) => f,
            Err(c) => return c,
        };
        let is_blk = sys::stat(dev).is_ok_and(|st| st.mode & 0o170000 == 0o060000);
        let _ = file.fd();
        for (name, ioctl, _) in &cmds {
            if is_blk {
                // Sem ioctls de bloco no sysabi: um dispositivo de bloco real não chega aqui.
                ul::warn(&short, format!("ioctl error on {ioctl}"), Errno::ENOSYS);
                return 1;
            }
            if *name == "--getsz" {
                ul::warnx(&short, "could not get device size");
            } else {
                ul::warn(&short, format!("ioctl error on {ioctl}"), Errno::ENOTTY);
            }
            return 1;
        }
    }
    0
}
