//! `fsfreeze` do util-linux 2.41: suspende e retoma o acesso a um sistema de arquivos.
//!
//! O `FIFREEZE`/`FITHAW` exige `CAP_SYS_ADMIN`; num contêiner sem privilégio a ioctl falha com EPERM
//! (depois de o diretório abrir e ser confirmado como diretório), que é o que este porte reproduz.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Errno, FileType, OFlags, sys};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const LONGS: &[LongOpt] = &[
    LongOpt::new("freeze", HasArg::No, b'f' as i32),
    LongOpt::new("unfreeze", HasArg::No, b'u' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
];

const USAGE: &str = "
Usage:
 fsfreeze [options] <mountpoint>

Suspend access to a filesystem.

Options:
 -f, --freeze      freeze the filesystem
 -u, --unfreeze    unfreeze the filesystem
 -h, --help        display this help
 -V, --version     display version

For more details see fsfreeze(8).
";

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut freeze = false;
    let mut unfreeze = false;
    let mut g = Getopt::from_env(&argv[1..], "fuhV", LONGS);
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
            Some('f') => {
                if unfreeze {
                    ul::warnx(
                        &short,
                        "options --unfreeze and --freeze are mutually exclusive",
                    );
                    ul::errtryhelp(&short);
                    return 1;
                }
                freeze = true;
            }
            Some('u') => {
                if freeze {
                    ul::warnx(
                        &short,
                        "options --freeze and --unfreeze are mutually exclusive",
                    );
                    ul::errtryhelp(&short);
                    return 1;
                }
                unfreeze = true;
            }
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
    let ops = g.operands();
    if ops.is_empty() {
        ul::warnx(&short, "no filesystem specified");
        ul::errtryhelp(&short);
        return 1;
    }
    if ops.len() > 1 {
        ul::warnx(&short, "unexpected number of arguments");
        ul::errtryhelp(&short);
        return 1;
    }
    if !freeze && !unfreeze {
        ul::warnx(&short, "neither --freeze or --unfreeze specified");
        ul::errtryhelp(&short);
        return 1;
    }
    let path = &ops[0];
    let fd = match sys::open(path, OFlags::RDONLY, 0) {
        Ok(fd) => fd,
        Err(e) => {
            ul::warn(&short, format!("cannot open {}", io::lossy(path)), e);
            return 1;
        }
    };
    let is_dir = sys::stat(path).is_ok_and(|st| st.file_type() == FileType::Directory);
    let _ = sys::close(fd);
    if !is_dir {
        ul::warnx(&short, format!("{}: is not a directory", io::lossy(path)));
        return 1;
    }
    let what = if freeze { "freeze" } else { "unfreeze" };
    ul::warn(
        &short,
        format!("{}: {what} failed", io::lossy(path)),
        Errno::EPERM,
    );
    1
}
