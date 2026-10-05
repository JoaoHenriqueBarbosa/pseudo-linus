//! `fsck.cramfs` e `mkfs.cramfs` do util-linux 2.41.
//!
//! `fsck.cramfs` valida argumentos, abre o arquivo e confere o magic do superbloco. A compressão
//! zlib de `mkfs.cramfs` e a extração de `fsck.cramfs` não foram portadas: ambos validam os
//! argumentos e os caminhos como o original e terminam com erro explícito quando o trabalho de
//! compressão seria necessário.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, OFlags, sys};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const EX_ERROR: i32 = 8;
const EX_USAGE: i32 = 16;
const CRAMFS_MAGIC: u32 = 0x28cd_3d45;

const FSCK_LONGS: &[LongOpt] = &[
    LongOpt::new("verbose", HasArg::No, b'v' as i32),
    LongOpt::new("blocksize", HasArg::Required, b'b' as i32),
    LongOpt::new("extract", HasArg::Optional, 256),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
];

const FSCK_USAGE: &str = "
Usage:
 fsck.cramfs [options] <file>

Check and repair a compressed ROM file system.

Options:
 -v, --verbose      be more verbose
 -b, --blocksize <size>  use this blocksize, defaults to page size
     --extract[=<dir>]  test uncompression, optionally extract into <dir>

 -h, --help         display this help
 -V, --version      display version

For more details see fsck.cramfs(8).
";

const MKFS_USAGE: &str = "Usage: mkfs.cramfs [-h] [-v] [-b blksz] [-e edition] [-N endian] [-i file] [-n name] dirname outfile
 -h         print this help
 -v         be verbose
 -E         make all warnings errors (non-zero exit status)
 -b blksz   use this blocksize, must equal page size
 -e edition set edition number (part of fsid)
 -N endian  set cramfs endianness (big|little|host), default host
 -i file    insert a file image into the filesystem (requires >= 2.4.0)
 -n name    set name of cramfs filesystem
 -p         pad by 512 bytes for boot code
 -s         sort directory entries (old option, ignored)
 -z         make explicit holes
 dirname    root of the directory tree to be compressed
 outfile    output file
";

pub fn fsck_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| fsck_run(args))
}

pub fn mkfs_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| mkfs_run(args))
}

fn fsck_run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut g = Getopt::from_env(&argv[1..], "hb:vV", FSCK_LONGS);
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                ul::errtryhelp(&short);
                return EX_USAGE;
            }
        };
        if o.id == 256 {
            continue;
        }
        match o.short() {
            Some('v') => {}
            Some('b') => {
                let a = o.arg.as_deref().unwrap_or(b"");
                if let Err(m) = ul::strtou64_or_err(a, "invalid blocksize argument") {
                    ul::warnx(&short, m);
                    return 1;
                }
            }
            Some('h') => {
                let _ = io::stdout().write_all(FSCK_USAGE.as_bytes());
                return 0;
            }
            Some('V') => {
                ul::print_version(&short);
                return 0;
            }
            _ => {
                ul::errtryhelp(&short);
                return EX_USAGE;
            }
        }
    }
    let ops = g.operands();
    if ops.len() != 1 {
        io::eprint(FSCK_USAGE.to_string());
        return EX_USAGE;
    }
    let dev = ops[0].clone();
    let dev_s = io::lossy(&dev);
    let mut f = match io::File::open_with(&dev, OFlags::RDONLY, 0) {
        Ok(f) => f,
        Err(e) => {
            ul::warn(&short, format!("cannot open {dev_s}"), e);
            return EX_ERROR;
        }
    };
    let mut head = [0u8; 4];
    let _ = f.read_full(&mut head);
    let le = u32::from_le_bytes(head);
    let be = u32::from_be_bytes(head);
    if le != CRAMFS_MAGIC && be != CRAMFS_MAGIC {
        ul::warnx(&short, format!("{dev_s}: superblock magic not found"));
        return EX_ERROR;
    }
    ul::warnx(&short, format!("{dev_s}: verification of the file tree is not available"));
    EX_ERROR
}

fn mkfs_run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut g = Getopt::from_env(&argv[1..], "hEb:e:N:i:n:psvVz", &[]);
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                io::eprint(MKFS_USAGE.to_string());
                return EX_USAGE;
            }
        };
        match o.short() {
            Some('E') | Some('p') | Some('s') | Some('v') | Some('z') | Some('i') | Some('n') | Some('e') => {}
            Some('b') => {
                let a = o.arg.as_deref().unwrap_or(b"");
                if let Err(m) = ul::strtou64_or_err(a, "invalid blocksize argument") {
                    ul::warnx(&short, m);
                    return 1;
                }
            }
            Some('N') => {
                let a = o.arg_str();
                if !matches!(a.as_str(), "big" | "little" | "host") {
                    ul::warnx(&short, "invalid endianness given. Must be 'big', 'little' or 'host'");
                    return EX_USAGE;
                }
            }
            Some('h') => {
                let _ = io::stdout().write_all(MKFS_USAGE.as_bytes());
                return 0;
            }
            Some('V') => {
                ul::print_version(&short);
                return 0;
            }
            _ => {
                io::eprint(MKFS_USAGE.to_string());
                return EX_USAGE;
            }
        }
    }
    let ops = g.operands();
    if ops.len() != 2 {
        io::eprint(MKFS_USAGE.to_string());
        return EX_USAGE;
    }
    let dir = &ops[0];
    match sys::stat(dir) {
        Ok(_) => {}
        Err(e) => {
            ul::warn(&short, io::lossy(dir), e);
            return EX_USAGE;
        }
    }
    ul::warnx(&short, "compression of the file tree is not available");
    EX_ERROR
}
