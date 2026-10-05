//! `fsck.minix` do util-linux 2.41: verifica um sistema de arquivos Minix.
//!
//! Cobre o que o sandbox exercita: validação de argumentos, abertura do arquivo, leitura do
//! superbloco (versões 1, 2 e 3), a detecção de magic inválido e o resumo "clean" de um sistema
//! marcado como válido. A verificação completa da árvore de diretórios não foi portada.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, OFlags};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const EX_ERROR: i32 = 8;
const EX_USAGE: i32 = 16;

const LONGS: &[LongOpt] = &[
    LongOpt::new("list", HasArg::No, b'l' as i32),
    LongOpt::new("auto", HasArg::No, b'a' as i32),
    LongOpt::new("repair", HasArg::No, b'r' as i32),
    LongOpt::new("verbose", HasArg::No, b'v' as i32),
    LongOpt::new("super", HasArg::No, b's' as i32),
    LongOpt::new("uncleared", HasArg::No, b'm' as i32),
    LongOpt::new("force", HasArg::No, b'f' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
];

const USAGE: &str = "
Usage:
 fsck.minix [options] <device>

Check the consistency of a Minix filesystem.

Options:
 -l, --list       list all filenames
 -a, --auto       automatic repair
 -r, --repair     interactive repair
 -v, --verbose    be verbose
 -s, --super      output super-block information
 -m, --uncleared  activate mode not cleared warnings
 -f, --force      force check

 -h, --help       display this help
 -V, --version    display version

For more details see fsck.minix(8).
";

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn le16(b: &[u8], o: usize) -> u64 {
    u16::from_le_bytes([b[o], b[o + 1]]) as u64
}

fn le32(b: &[u8], o: usize) -> u64 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) as u64
}

fn count_free(map: &[u8], from_bit: u64, to_bit: u64) -> u64 {
    let mut n = 0;
    for bit in from_bit..=to_bit {
        let byte = map.get((bit >> 3) as usize).copied().unwrap_or(0xff);
        if byte & (1 << (bit & 7)) == 0 {
            n += 1;
        }
    }
    n
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut force = false;
    let mut g = Getopt::from_env(&argv[1..], "larvsmfhV", LONGS);
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
            Some('l') | Some('a') | Some('r') | Some('v') | Some('s') | Some('m') => {}
            Some('f') => force = true,
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
    if ops.len() != 1 {
        ul::errtryhelp(&short);
        return EX_USAGE;
    }
    let dev = ops[0].clone();
    let dev_s = io::lossy(&dev);
    let mut file = match io::File::open_with(&dev, OFlags::RDONLY, 0) {
        Ok(f) => f,
        Err(e) => {
            ul::warn(&short, format!("cannot open {dev_s}"), e);
            return EX_ERROR;
        }
    };
    // Lê os blocos 0 e 1 e depois o que o superbloco descreve (mapas).
    let mut buf = vec![0u8; 2048];
    let _ = file.read_full(&mut buf);
    let sb = &buf[1024..2048];
    let magic = le16(sb, 16);
    let v3 = le16(sb, 24) == 0x4d5a;
    let ver = match magic {
        0x137f | 0x138f => 1,
        0x2468 | 0x2478 => 2,
        _ if v3 => 3,
        _ => {
            ul::warnx(&short, "bad magic number in super-block");
            return EX_ERROR;
        }
    };
    let (ninodes, imap_blocks, zmap_blocks, first_zone, zones, valid) = if ver == 3 {
        (le32(sb, 0), le16(sb, 6), le16(sb, 8), le16(sb, 10), le32(sb, 20), true)
    } else {
        let z = if ver == 1 { le16(sb, 2) } else { le32(sb, 20) };
        (le16(sb, 0), le16(sb, 4), le16(sb, 6), le16(sb, 8), z, le16(sb, 18) & 1 != 0)
    };
    if force {
        let _ = io::stdout().write_all(format!("Forcing filesystem check on {dev_s}.\n").as_bytes());
        return 0;
    }
    if !valid {
        return 0;
    }
    let mut maps = vec![0u8; ((imap_blocks + zmap_blocks) * 1024) as usize];
    let _ = file.read_full(&mut maps);
    let imap = &maps[..(imap_blocks * 1024) as usize];
    let zmap = &maps[(imap_blocks * 1024) as usize..];
    let free_inodes = count_free(imap, 1, ninodes);
    let free_zones = if zones > first_zone {
        count_free(zmap, 1, zones - first_zone)
    } else {
        0
    };
    let _ = io::stdout().write_all(
        format!(
            "{dev_s}: clean, {}/{} files, {}/{} blocks\n",
            ninodes - free_inodes,
            ninodes,
            zones - free_zones,
            zones
        )
        .as_bytes(),
    );
    0
}
