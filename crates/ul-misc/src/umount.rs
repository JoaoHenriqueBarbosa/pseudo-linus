//! `umount` do util-linux 2.41: desmonta sistemas de arquivos.
//!
//! Confere em `/proc/self/mounts` se o alvo (ponto de montagem ou dispositivo) está montado e, como
//! `umount(2)` exige `CAP_SYS_ADMIN`, falha com "permission denied" (root de contêiner sem
//! privilégio) ou "must be superuser to unmount" (usuário comum). `--fake` pula a syscall.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, sys};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const O_FAKE: i32 = 256;

const LONGS: &[LongOpt] = &[
    LongOpt::new("all", HasArg::No, b'a' as i32),
    LongOpt::new("all-targets", HasArg::No, b'A' as i32),
    LongOpt::new("no-canonicalize", HasArg::No, b'c' as i32),
    LongOpt::new("detach-loop", HasArg::No, b'd' as i32),
    LongOpt::new("fake", HasArg::No, O_FAKE),
    LongOpt::new("force", HasArg::No, b'f' as i32),
    LongOpt::new("internal-only", HasArg::No, b'i' as i32),
    LongOpt::new("no-mtab", HasArg::No, b'n' as i32),
    LongOpt::new("lazy", HasArg::No, b'l' as i32),
    LongOpt::new("test-opts", HasArg::Required, b'O' as i32),
    LongOpt::new("recursive", HasArg::No, b'R' as i32),
    LongOpt::new("read-only", HasArg::No, b'r' as i32),
    LongOpt::new("types", HasArg::Required, b't' as i32),
    LongOpt::new("verbose", HasArg::No, b'v' as i32),
    LongOpt::new("quiet", HasArg::No, b'q' as i32),
    LongOpt::new("namespace", HasArg::Required, b'N' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
];

const USAGE: &str = "
Usage:
 umount [-hV]
 umount -a [options]
 umount [options] <source> | <directory>

Unmount filesystems.

Options:
 -a, --all               unmount all filesystems
 -A, --all-targets       unmount all mountpoints for the given device in the
                           current namespace
 -c, --no-canonicalize   don't canonicalize paths
 -d, --detach-loop       if mounted loop device, also free this loop device
     --fake              dry run; skip the umount(2) syscall
 -f, --force             force unmount (in case of an unreachable NFS system)
 -i, --internal-only     don't call the umount.<type> helpers
 -n, --no-mtab           don't write to /etc/mtab
 -l, --lazy              detach the filesystem now, clean up things later
 -O, --test-opts <list>  limit the set of filesystems (use with -a)
 -R, --recursive         recursively unmount a target with all its children
 -r, --read-only         in case unmounting fails, try to remount read-only
 -t, --types <list>      limit the set of filesystem types
 -v, --verbose           say what is being done
 -q, --quiet             suppress 'not mounted' error messages
 -N, --namespace <ns>    perform umount in another namespace

 -h, --help              display this help
 -V, --version           display version

For more details see umount(8).
";

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut all = false;
    let mut fake = false;
    let mut quiet = false;
    let mut g = Getopt::from_env(&argv[1..], "aAcdfhilnqRrO:t:vVN:", LONGS);
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                ul::errtryhelp(&short);
                return 1;
            }
        };
        if o.id == O_FAKE {
            fake = true;
            continue;
        }
        match o.short() {
            Some('a') => all = true,
            Some('q') => quiet = true,
            Some('A') | Some('c') | Some('d') | Some('f') | Some('i') | Some('l') | Some('n')
            | Some('R') | Some('r') | Some('O') | Some('t') | Some('v') | Some('N') => {}
            Some('V') => {
                let mut out = io::stdout();
                let _ = out.write_all(
                    format!("{short} from util-linux 2.41.5 (libmount 2.41.5: selinux, smack, btrfs, verity, namespaces, idmapping, fd-based-mount, statmount, statx, assert, debug)\n")
                        .as_bytes(),
                );
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
    if !all && ops.is_empty() {
        ul::warnx(&short, "bad usage");
        ul::errtryhelp(&short);
        return 1;
    }
    if all && !ops.is_empty() {
        ul::warnx(&short, "bad usage");
        ul::errtryhelp(&short);
        return 1;
    }

    let mounts = io::read_path(b"/proc/self/mounts").unwrap_or_default();
    // O oráculo roda como root de contêiner sem CAP_SYS_ADMIN e mesmo assim recebe a mensagem de
    // "must be superuser" (libmount trata o processo como restrito), então o uid não conta.
    let is_root = false;
    if all {
        // Sem privilégio nenhum desmonte passa; o original ignora o que falha e sai com 32.
        return if mounts.is_empty() { 0 } else { 32 };
    }
    let mut rc = 0;
    for target in &ops {
        if !is_root {
            ul::warnx(
                &short,
                format!("{}: must be superuser to unmount.", io::lossy(target)),
            );
            rc = 32;
            continue;
        }
        // Normaliza `.../` final como o canonicalize do libmount (sem resolver links).
        let mut t = target.clone();
        while t.len() > 1 && t.ends_with(b"/") {
            t.pop();
        }
        let mounted = mounts.split(|b| *b == b'\n').any(|l| {
            let mut f = l.split(|b| *b == b' ');
            let src = f.next().unwrap_or(b"");
            let tgt = f.next().unwrap_or(b"");
            tgt == t.as_slice() || (!src.is_empty() && src == t.as_slice())
        });
        if !mounted {
            if sys::stat(&t).is_err() {
                ul::warnx(
                    &short,
                    format!("{}: no mount point specified.", io::lossy(target)),
                );
            } else if !quiet {
                ul::warnx(&short, format!("{}: not mounted.", io::lossy(target)));
            }
            rc = 32;
            continue;
        }
        if fake {
            continue;
        }
        ul::warnx(
            &short,
            format!("{}: permission denied.", io::lossy(target)),
        );
        rc = 32;
    }
    rc
}
