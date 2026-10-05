//! `mkfs.bfs` do util-linux 2.41: cria um sistema de arquivos SCO bfs.
//!
//! Porte do `disk-utils/mkfs.bfs.c` para arquivos comuns. Grava o superbloco (bloco 0), a tabela de
//! inodes e o diretório raiz com `.` e `..`, de uma vez, a partir do começo do arquivo.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, OFlags, sys};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const BLOCK: u64 = 512;
const INODE_SIZE: u64 = 64;
const ROOT_INO: u16 = 2;

const LONGS: &[LongOpt] = &[
    LongOpt::new("inodes", HasArg::Required, b'N' as i32),
    LongOpt::new("vname", HasArg::Required, b'V' as i32),
    LongOpt::new("fname", HasArg::Required, b'F' as i32),
    LongOpt::new("verbose", HasArg::No, b'v' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("version", HasArg::No, 256),
];

const USAGE: &str = "
Usage:
 mkfs.bfs [options] device [block-count]

Make an SCO bfs filesystem.

Options:
 -N, --inodes=NUM    specify desired number of inodes
 -V, --vname=NAME    specify volume name
 -F, --fname=NAME    specify file system name
 -v, --verbose       explain what is being done;
                     specify twice to dump the superblock
 -c                  this option is silently ignored
 -l                  this option is silently ignored
 -h, --help          display this help
     --version       display version

For more details see mkfs.bfs(8).
";

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn put16(b: &mut [u8], off: usize, v: u16) {
    b[off..off + 2].copy_from_slice(&v.to_le_bytes());
}

fn put32(b: &mut [u8], off: usize, v: u32) {
    b[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

fn now() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as u32)
        .unwrap_or(0)
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut inodes: u64 = 0;
    let mut verbose = 0;
    let mut vname = [b' '; 6];
    let mut fname = [b' '; 6];
    let mut g = Getopt::from_env(&argv[1..], "N:V:F:vchl", LONGS);
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                ul::errtryhelp(&short);
                return 1;
            }
        };
        if o.id == 256 {
            ul::print_version(&short);
            return 0;
        }
        match o.short() {
            Some('N') => match ul::strtou64_or_err(
                o.arg.as_deref().unwrap_or(b""),
                "invalid number of inodes",
            ) {
                Ok(n) => inodes = n,
                Err(m) => {
                    ul::warnx(&short, m);
                    return 1;
                }
            },
            Some('V') => {
                let a = o.arg.clone().unwrap_or_default();
                if a.len() > 6 {
                    ul::warnx(&short, "volume name too long");
                    return 1;
                }
                vname = [b' '; 6];
                vname[..a.len()].copy_from_slice(&a);
            }
            Some('F') => {
                let a = o.arg.clone().unwrap_or_default();
                if a.len() > 6 {
                    ul::warnx(&short, "file system name too long");
                    return 1;
                }
                fname = [b' '; 6];
                fname[..a.len()].copy_from_slice(&a);
            }
            Some('v') => verbose += 1,
            Some('c') | Some('l') => {}
            Some('h') => {
                let _ = io::stdout().write_all(USAGE.as_bytes());
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
        ul::errtryhelp(&short);
        return 1;
    }
    let dev = ops[0].clone();
    let dev_s = io::lossy(&dev);
    let st = match sys::stat(&dev) {
        Ok(s) => s,
        Err(e) => {
            ul::warn(&short, format!("cannot stat {dev_s}"), e);
            return 1;
        }
    };
    let fsize = st.size as u64;
    let mut user_blocks: Option<u64> = None;
    if ops.len() == 2 {
        match ul::strtou64_or_err(&ops[1], "invalid block count") {
            Ok(n) => user_blocks = Some(n),
            Err(m) => {
                ul::warnx(&short, m);
                return 1;
            }
        }
    } else if ops.len() > 2 {
        ul::warnx(&short, "unexpected number of arguments");
        ul::errtryhelp(&short);
        return 1;
    }
    let mut f = match io::File::open_with(&dev, OFlags::RDWR, 0) {
        Ok(f) => f,
        Err(e) => {
            ul::warn(&short, format!("cannot open {dev_s}"), e);
            return 1;
        }
    };

    let max_blocks = fsize / BLOCK;
    let total_blocks = match user_blocks {
        Some(n) => {
            if n > max_blocks {
                ul::warnx(&short, format!("blocks argument too large, max is {max_blocks}"));
                return 1;
            }
            n
        }
        None => max_blocks,
    };

    if inodes == 0 {
        let i = total_blocks.min(512);
        inodes = i.div_ceil(8) * 8;
    }
    let ino_bytes = inodes * INODE_SIZE;
    let ino_blocks = ino_bytes.div_ceil(BLOCK);
    if total_blocks < ino_blocks + 2 {
        ul::warnx(
            &short,
            format!("not enough space, need at least {} blocks", ino_blocks + 2),
        );
        return 1;
    }
    let data_start = BLOCK + ino_bytes;
    let t = now();

    // Superbloco.
    let mut image = vec![0u8; (BLOCK + ino_blocks * BLOCK + BLOCK) as usize];
    put32(&mut image, 0, 0x1bad_face);
    put32(&mut image, 4, data_start as u32);
    put32(&mut image, 8, (total_blocks * BLOCK - 1) as u32);
    for off in [12usize, 16, 20, 24] {
        put32(&mut image, off, 0xffff_ffff);
    }
    image[28..34].copy_from_slice(&fname);
    image[34..40].copy_from_slice(&vname);

    // Inode raiz: primeiro da tabela (o número 2 fica na posição zero).
    let sblock = (1 + ino_blocks) as u32;
    let base = BLOCK as usize;
    put16(&mut image, base, ROOT_INO);
    put32(&mut image, base + 4, sblock);
    put32(&mut image, base + 8, sblock);
    put32(&mut image, base + 12, data_start as u32 + 2 * 16 - 1);
    put32(&mut image, base + 16, 2); // BFS_VDIR
    put32(&mut image, base + 20, 0o040755);
    put32(&mut image, base + 32, 2); // nlink
    put32(&mut image, base + 36, t);
    put32(&mut image, base + 40, t);
    put32(&mut image, base + 44, t);

    // Diretório raiz: `.` e `..`, 16 bytes cada.
    let d = data_start as usize;
    put16(&mut image, d, ROOT_INO);
    image[d + 2] = b'.';
    put16(&mut image, d + 16, ROOT_INO);
    image[d + 18] = b'.';
    image[d + 19] = b'.';

    if verbose > 0 {
        let mut out = io::stdout();
        let _ = out.write_all(
            format!(
                "Device: {dev_s}\nVolume: <{}>\nFilesystem: <{}>\nBlock size: {BLOCK}\nInodes: {inodes}\nBlocks: {total_blocks}\nInode end: {}, Data start: {}, Data end: {}\n",
                io::lossy(&vname),
                io::lossy(&fname),
                data_start - 1,
                data_start,
                total_blocks * BLOCK - 1
            )
            .as_bytes(),
        );
    }

    if let Err(e) = f.write_all(&image) {
        ul::warn(&short, "error writing superblock", io::io_errno(&e));
        return 1;
    }
    0
}
