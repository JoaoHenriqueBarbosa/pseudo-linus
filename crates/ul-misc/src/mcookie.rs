//! `mcookie` do util-linux 2.41 (pacote util-linux do Debian 13): gera um cookie mágico de 128 bits
//! pro `xauth`, o MD5 das sementes (`-f`, até `-m` bytes de cada, 4096 por padrão) seguidas de 128
//! bytes aleatórios. A saída são 32 dígitos hexadecimais e uma quebra de linha.
//!
//! Com `-v` o stderr conta quantos bytes vieram de cada semente e do aleatório
//! (`Got 128 bytes from getrandom() function`).

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Errno, Fd, OFlags, sys};

use crate::util::io;
use crate::util::md5::Md5;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const BUFFERSIZE: usize = 4096;
const RAND_BYTES: usize = 128;

const LONGS: &[LongOpt] = &[
    LongOpt::new("file", HasArg::Required, b'f' as i32),
    LongOpt::new("max-size", HasArg::Required, b'm' as i32),
    LongOpt::new("verbose", HasArg::No, b'v' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
];

const USAGE: &str = "
Usage:
 mcookie [options]

Generate magic cookies for xauth.

Options:
 -f, --file <file>     use file as a cookie seed
 -m, --max-size <num>  limit how much is read from seed files
 -v, --verbose         explain what is being done

 -h, --help            display this help
 -V, --version         display version

Arguments:
 Values for <num> may be followed by a suffix: KiB, MiB,
 GiB, TiB, PiB, EiB, ZiB, or YiB (where the \"iB\" is optional).

For more details see mcookie(1).
";

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

/// `hash_file`: mistura até `wanted` bytes de `fd` (ou 4096 sem `-m`) e um NUL separador; devolve
/// quantos bytes leu.
fn hash_file(ctx: &mut Md5, fd: Fd, maxsz: u64) -> u64 {
    let wanted = if maxsz != 0 { maxsz } else { BUFFERSIZE as u64 };
    let mut count = 0u64;
    let mut buf = [0u8; BUFFERSIZE];
    while count < wanted {
        let rdsz = ((wanted - count).min(BUFFERSIZE as u64)) as usize;
        // read_all: repete até encher o pedaço, parando no fim ou num erro
        let mut got = 0;
        while got < rdsz {
            match sys::read(fd, &mut buf[got..rdsz]) {
                Ok(0) => break,
                Ok(n) => got += n,
                Err(Errno::EINTR) => {}
                Err(_) => break,
            }
        }
        if got == 0 {
            break;
        }
        ctx.update(&buf[..got]);
        count += got as u64;
    }
    // Separate files with a null byte
    ctx.update(&[0]);
    count
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut verbose = false;
    let mut maxsz: u64 = 0;
    let mut files: Vec<Vec<u8>> = Vec::new();

    let mut g = Getopt::from_env(&argv[1..], "f:m:vVh", LONGS);
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
            Some('v') => verbose = true,
            Some('f') => files.push(o.arg.clone().unwrap_or_default()),
            Some('m') => match ul::strtosize_or_err(o.arg.as_deref().unwrap_or(b""), "failed to parse length") {
                Ok(n) => maxsz = n,
                Err(m) => {
                    ul::warnx(&short, m);
                    return 1;
                }
            },
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

    if maxsz != 0 && files.is_empty() {
        ul::warnx(&short, "--max-size ignored when used without --file");
    }

    let mut ctx = Md5::new();
    for fname in &files {
        let (fd, owned) = if fname == b"-" {
            (Fd::STDIN, false)
        } else {
            match sys::open(fname, OFlags::RDONLY, 0) {
                Ok(fd) => (fd, true),
                Err(e) => {
                    ul::warn(&short, format!("cannot open {}", io::lossy(fname)), e);
                    continue;
                }
            }
        };
        let count = hash_file(&mut ctx, fd, maxsz);
        if verbose {
            let unit = if count == 1 { "byte" } else { "bytes" };
            io::eprint(format!("Got {count} {unit} from {}\n", io::lossy(fname)));
        }
        if owned {
            if let Err(e) = sys::close(fd) {
                ul::warn(&short, format!("closing {} failed", io::lossy(fname)), e);
                return 1;
            }
        }
    }

    let mut buf = [0u8; RAND_BYTES];
    let mut filled = 0;
    while filled < RAND_BYTES {
        match sys::current().getrandom(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(Errno::EINTR) => {}
            Err(_) => break,
        }
    }
    ctx.update(&buf);
    if verbose {
        io::eprint(format!("Got {RAND_BYTES} bytes from getrandom() function\n"));
    }

    let digest = ctx.finish();
    let mut line = String::with_capacity(33);
    for b in digest {
        line.push_str(&format!("{b:02x}"));
    }
    line.push('\n');
    let mut out = io::stdout();
    let _ = out.write_all(line.as_bytes());
    if out.flush().is_err() {
        return 1;
    }
    0
}
