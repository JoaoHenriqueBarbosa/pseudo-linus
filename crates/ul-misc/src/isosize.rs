//! `isosize` do util-linux 2.41: mostra o tamanho de um sistema de arquivos ISO-9660, lido do
//! descritor de volume primário (setor 16, 2048 bytes por setor): `volume_space_size` no deslocamento
//! 80 e `logical_block_size` no 128, ambos no formato duplo (little-endian primeiro).
//!
//! Sem opções imprime os bytes (setores vezes tamanho do setor, dividido por `-d`); com `-x` imprime
//! a contagem e o tamanho do setor.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Errno, OFlags, sys};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const LONGS: &[LongOpt] = &[
    LongOpt::new("divisor", HasArg::Required, b'd' as i32),
    LongOpt::new("sectors", HasArg::No, b'x' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
];

const USAGE: &str = "
Usage:
 isosize [options] <iso9660_image_file> ...

Show the length of an ISO-9660 filesystem.

Options:
 -d, --divisor=<number>  divide the amount of bytes by <number>
 -x, --sectors           show sector count and size
 -h, --help              display this help
 -V, --version           display version

For more details see isosize(8).
";

/// Deslocamento do descritor de volume primário: setor 16 de 2048 bytes.
const PVD_OFFSET: usize = 16 * 2048;
const NEEDED: usize = PVD_OFFSET + 132;

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut divisor: u64 = 1;
    let mut sectors = false;

    let mut g = Getopt::from_env(&argv[1..], "d:xVh", LONGS);
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
            Some('d') => {
                let text = o.arg.clone().unwrap_or_default();
                match std::str::from_utf8(&text)
                    .ok()
                    .and_then(|s| s.parse::<u32>().ok())
                {
                    Some(n) => divisor = n as u64,
                    None => {
                        ul::warnx(
                            &short,
                            format!("invalid divisor argument: '{}'", io::lossy(&text)),
                        );
                        return 1;
                    }
                }
            }
            Some('x') => sectors = true,
            Some('V') => {
                ul::print_version(&short);
                return 0;
            }
            Some('h') => {
                let mut out = io::stdout();
                let _ = out.write_all(USAGE.as_bytes());
                return 0;
            }
            _ => {
                ul::errtryhelp(&short);
                return 1;
            }
        }
    }

    let files = g.operands();
    if files.is_empty() {
        ul::warnx(&short, "no device specified".to_string());
        ul::errtryhelp(&short);
        return 1;
    }

    let mut status = 0;
    let mut out = io::stdout();
    for f in &files {
        let name = io::lossy(f);
        let fd = match sys::open(f, OFlags::RDONLY, 0) {
            Ok(fd) => fd,
            Err(e) => {
                let _ = out.flush();
                ul::warn(&short, format!("cannot open {name}"), e);
                return 32;
            }
        };
        let mut buf = vec![0u8; NEEDED];
        let mut got = 0;
        let mut failure: Option<Errno> = None;
        while got < NEEDED {
            match sys::read(fd, &mut buf[got..]) {
                Ok(0) => break,
                Ok(n) => got += n,
                Err(Errno::EINTR) => {}
                Err(e) => {
                    failure = Some(e);
                    break;
                }
            }
        }
        let _ = sys::close(fd);
        if let Some(e) = failure {
            let _ = out.flush();
            ul::warn(&short, format!("read error on {name}"), e);
            return 1;
        }
        if got < NEEDED {
            let _ = out.flush();
            io::eprint(format!("{short}: read error on {name}: Success\n"));
            return 1;
        }
        let size = &buf[PVD_OFFSET + 80..PVD_OFFSET + 84];
        let nsecs = u32::from_le_bytes([size[0], size[1], size[2], size[3]]) as u64;
        let block = &buf[PVD_OFFSET + 128..PVD_OFFSET + 130];
        let ssize = u16::from_le_bytes([block[0], block[1]]) as u64;
        let line = if sectors {
            format!("sector count: {nsecs}, sector size: {ssize}\n")
        } else if divisor == 0 {
            let _ = out.flush();
            io::eprint(format!("{short}: division by zero\n"));
            return 1;
        } else {
            format!("{}\n", nsecs * ssize / divisor)
        };
        if out.write_all(line.as_bytes()).is_err() {
            status = 1;
        }
    }
    if out.flush().is_err() {
        return 1;
    }
    status
}
