//! `mkfs.minix` do util-linux 2.41: cria um sistema de arquivos Minix (versões 1, 2 e 3).
//!
//! Porte do `disk-utils/mkfs.minix.c` para arquivos comuns (o sandbox não tem dispositivos de
//! bloco). O sistema de arquivos é montado inteiro em memória (superbloco, mapas de inodes e de
//! zonas, tabela de inodes e o bloco do diretório raiz, que vem logo depois da tabela) e gravado de
//! uma vez a partir do começo do arquivo. Os 512 primeiros bytes (setor de boot) são zerados e os
//! 512 seguintes ficam como estavam.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, OFlags, sys};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const EX_ERROR: i32 = 8;
const EX_USAGE: i32 = 16;
const BLOCK: usize = 1024;
const BITS_PER_BLOCK: u64 = 8192;

const LONGS: &[LongOpt] = &[
    LongOpt::new("namelength", HasArg::Required, b'n' as i32),
    LongOpt::new("inodes", HasArg::Required, b'i' as i32),
    LongOpt::new("check", HasArg::No, b'c' as i32),
    LongOpt::new("badblocks", HasArg::Required, b'l' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
];

const USAGE: &str = "
Usage:
 mkfs.minix [options] /dev/name [blocks]

Make a Minix filesystem.

Options:
 -1                      use Minix version 1
 -2, -v                  use Minix version 2
 -3                      use Minix version 3
 -n, --namelength <num>  maximum length of filenames
 -i, --inodes <num>      number of inodes for the filesystem
 -c, --check             check the device for bad blocks
 -l, --badblocks <file>  list of bad blocks from file

 -h, --help              display this help
 -V, --version           display version

For more details see mkfs.minix(8).
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

fn set_bit(map: &mut [u8], n: u64) {
    map[(n >> 3) as usize] |= 1 << (n & 7);
}

fn clear_bit(map: &mut [u8], n: u64) {
    map[(n >> 3) as usize] &= !(1 << (n & 7));
}

fn upper(n: u64, d: u64) -> u64 {
    n.div_ceil(d)
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

    let mut version: u32 = 1;
    let mut namelen: Option<u64> = None;
    let mut inodes_opt: u64 = 0;
    let mut g = Getopt::from_env(&argv[1..], "ci:l:n:v123Vh", LONGS);
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                ul::errtryhelp(&short);
                return EX_USAGE;
            }
        };
        match o.short() {
            Some('c') | Some('l') => {}
            Some('i') => match ul::strtou64_or_err(
                o.arg.as_deref().unwrap_or(b""),
                "failed to parse number of inodes",
            ) {
                Ok(n) => inodes_opt = n,
                Err(m) => {
                    ul::warnx(&short, m);
                    return 1;
                }
            },
            Some('n') => match ul::strtou64_or_err(
                o.arg.as_deref().unwrap_or(b""),
                "failed to parse maximum length of filenames",
            ) {
                Ok(n) => {
                    if !matches!(n, 14 | 30 | 60) {
                        ul::errtryhelp(&short);
                        return EX_USAGE;
                    }
                    namelen = Some(n);
                }
                Err(m) => {
                    ul::warnx(&short, m);
                    return 1;
                }
            },
            Some('1') => version = 1,
            Some('2') | Some('v') => version = 2,
            Some('3') => version = 3,
            Some('h') => {
                let _ = io::stdout().write_all(USAGE.as_bytes());
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
    if ops.is_empty() {
        ul::warnx(&short, "no device specified");
        ul::errtryhelp(&short);
        return EX_USAGE;
    }
    if ops.len() > 2 {
        ul::warnx(&short, "unexpected number of arguments");
        ul::errtryhelp(&short);
        return EX_USAGE;
    }
    let dev = ops[0].clone();
    let dev_s = io::lossy(&dev);
    let mut blocks_arg: Option<u64> = None;
    if ops.len() == 2 {
        match ul::strtou64_or_err(&ops[1], "failed to parse number of blocks") {
            Ok(n) => blocks_arg = Some(n),
            Err(m) => {
                ul::warnx(&short, m);
                return 1;
            }
        }
    }

    // Nome e tamanho do diretório por versão.
    let (namelen, dirsize): (u64, usize) = match (version, namelen) {
        (3, _) => (60, 64),
        (_, Some(14)) => (14, 16),
        (_, _) => (30, 32),
    };
    let magic: u16 = match (version, namelen) {
        (3, _) => 0x4d5a,
        (1, 14) => 0x137f,
        (1, _) => 0x138f,
        (_, 14) => 0x2468,
        (_, _) => 0x2478,
    };

    let st = match sys::stat(&dev) {
        Ok(s) => s,
        Err(e) => {
            ul::warn(&short, format!("cannot open {dev_s}"), e);
            return EX_ERROR;
        }
    };
    let mut file = match io::File::open_with(&dev, OFlags::RDWR, 0) {
        Ok(f) => f,
        Err(e) => {
            ul::warn(&short, format!("cannot open {dev_s}"), e);
            return EX_ERROR;
        }
    };

    let dev_blocks = (st.size as u64) / BLOCK as u64;
    let mut blocks = blocks_arg.unwrap_or(dev_blocks);
    if version == 1 && blocks > 65535 {
        blocks = 65535;
    }
    if blocks < 10 {
        ul::warnx(&short, format!("{dev_s}: number of blocks too small"));
        return EX_ERROR;
    }

    let ipb: u64 = if version == 1 { 32 } else { 16 };
    let mut inodes = if inodes_opt == 0 { blocks / 3 } else { inodes_opt };
    inodes = inodes.div_ceil(ipb) * ipb;
    if inodes > 65535 {
        inodes = 65535;
    }
    if inodes == 0 {
        inodes = ipb;
    }
    let imap_blocks = upper(inodes + 1, BITS_PER_BLOCK);
    let inode_blocks = upper(inodes, ipb);
    if blocks <= 1 + imap_blocks + inode_blocks {
        ul::warnx(&short, format!("{dev_s}: number of blocks too small"));
        return EX_ERROR;
    }
    let zmap_blocks = upper(blocks - (1 + imap_blocks + inode_blocks), BITS_PER_BLOCK);
    let first_zone = 2 + imap_blocks + zmap_blocks + inode_blocks;
    if first_zone >= blocks {
        ul::warnx(&short, format!("{dev_s}: number of blocks too small"));
        return EX_ERROR;
    }
    let max_size: u32 = if version == 1 { 268_966_912 } else { 2_147_483_647 };

    // Superbloco.
    let mut sb = vec![0u8; BLOCK];
    if version == 3 {
        put32(&mut sb, 0, inodes as u32);
        put16(&mut sb, 6, imap_blocks as u16);
        put16(&mut sb, 8, zmap_blocks as u16);
        put16(&mut sb, 10, first_zone as u16);
        put32(&mut sb, 16, max_size);
        put32(&mut sb, 20, blocks as u32);
        put16(&mut sb, 24, magic);
        put16(&mut sb, 28, BLOCK as u16);
    } else {
        put16(&mut sb, 0, inodes as u16);
        put16(&mut sb, 2, if version == 1 { blocks as u16 } else { 0 });
        put16(&mut sb, 4, imap_blocks as u16);
        put16(&mut sb, 6, zmap_blocks as u16);
        put16(&mut sb, 8, first_zone as u16);
        put32(&mut sb, 12, max_size);
        put16(&mut sb, 16, magic);
        put16(&mut sb, 18, 1); // MINIX_VALID_FS
        if version == 2 {
            put32(&mut sb, 20, blocks as u32);
        }
    }

    // Mapas: tudo ocupado, libera o que existe; depois marca a raiz.
    let mut imap = vec![0xffu8; imap_blocks as usize * BLOCK];
    let mut zmap = vec![0xffu8; zmap_blocks as usize * BLOCK];
    for i in 1..=inodes {
        clear_bit(&mut imap, i);
    }
    for z in first_zone..blocks {
        clear_bit(&mut zmap, z - first_zone + 1);
    }
    set_bit(&mut imap, 1);
    set_bit(&mut zmap, 1);

    // Inode raiz (número 1, posição zero da tabela) e o bloco do diretório.
    let t = now();
    let mut itab = vec![0u8; inode_blocks as usize * BLOCK];
    let size = (2 * dirsize) as u32;
    if version == 1 {
        put16(&mut itab, 0, 0o040755);
        put32(&mut itab, 4, size);
        put32(&mut itab, 8, t);
        itab[13] = 2;
        put16(&mut itab, 14, first_zone as u16);
    } else {
        put16(&mut itab, 0, 0o040755);
        put16(&mut itab, 2, 2);
        put32(&mut itab, 8, size);
        put32(&mut itab, 12, t);
        put32(&mut itab, 16, t);
        put32(&mut itab, 20, t);
        put32(&mut itab, 24, first_zone as u32);
    }
    let mut root = vec![0u8; BLOCK];
    let ino_w = if version == 3 { 4 } else { 2 };
    if version == 3 {
        put32(&mut root, 0, 1);
        put32(&mut root, dirsize, 1);
    } else {
        put16(&mut root, 0, 1);
        put16(&mut root, dirsize, 1);
    }
    root[ino_w] = b'.';
    root[dirsize + ino_w] = b'.';
    root[dirsize + ino_w + 1] = b'.';

    // Preserva os bytes 512..1024 do setor que o original não toca.
    let mut head = vec![0u8; BLOCK];
    let _ = file.read_full(&mut head);
    for b in head.iter_mut().take(512) {
        *b = 0;
    }
    let mut image: Vec<u8> = Vec::new();
    image.extend_from_slice(&head);
    image.extend_from_slice(&sb);
    image.extend_from_slice(&imap);
    image.extend_from_slice(&zmap);
    image.extend_from_slice(&itab);
    image.extend_from_slice(&root);

    let mut f = match io::File::open_with(&dev, OFlags::RDWR, 0) {
        Ok(f) => f,
        Err(e) => {
            ul::warn(&short, format!("cannot open {dev_s}"), e);
            return EX_ERROR;
        }
    };
    if let Err(e) = f.write_all(&image) {
        ul::warn(&short, "unable to write super-block", io::io_errno(&e));
        return EX_ERROR;
    }

    let mut out = io::stdout();
    let _ = out.write_all(
        format!(
            "{} {}\n{} {}\nFirstdatazone={} ({})\nZonesize={}\nMaxsize={}\n\n",
            inodes,
            if inodes == 1 { "inode" } else { "inodes" },
            blocks,
            if blocks == 1 { "block" } else { "blocks" },
            first_zone,
            first_zone,
            BLOCK,
            max_size
        )
        .as_bytes(),
    );
    let _ = namelen;
    0
}
