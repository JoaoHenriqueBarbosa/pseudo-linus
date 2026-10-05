//! `ischroot` do debianutils 5.23 (Debian 13): detecta se o processo roda dentro de um chroot.
//!
//! Saída 0 (é chroot), 1 (não é) ou 2 (não deu pra saber; `-t` e `-f` trocam esse 2 por 0 e 1). A
//! detecção compara `/proc/1/mountinfo` com `/proc/self/mountinfo` e, se isso falha, o dispositivo e o
//! inode de `/` com os de `/proc/1/root`. Dentro de um `fakechroot` (variáveis `FAKECHROOT=true`,
//! `FAKECHROOT_BASE` e `libfakechroot.so` no `LD_PRELOAD`) a resposta é sempre 0.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Errno, Fd, OFlags, sys};

use crate::util::io;
use crate::util::{Getopt, HasArg, LongOpt};

const LONGS: &[LongOpt] = &[
    LongOpt::new("default-false", HasArg::No, b'f' as i32),
    LongOpt::new("default-true", HasArg::No, b't' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
];

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn isfakechroot() -> bool {
    sys::getenv("FAKECHROOT").is_some_and(|v| v == b"true")
        && sys::getenv("FAKECHROOT_BASE").is_some()
        && sys::getenv("LD_PRELOAD").is_some_and(|v| {
            v.windows(b"libfakechroot.so".len())
                .any(|w| w == b"libfakechroot.so")
        })
}

/// Lê até `buf.len()` bytes (um `read`).
fn read_chunk(fd: Fd, buf: &mut [u8]) -> Result<usize, Errno> {
    sys::read(fd, buf)
}

/// `ischroot_mountinfo`: negativo em falha, 0 se detectou chroot, 1 se não.
fn ischroot_mountinfo() -> i32 {
    let fd1 = sys::open(b"/proc/1/mountinfo", OFlags::RDONLY, 0);
    let fd2 = sys::open(b"/proc/self/mountinfo", OFlags::RDONLY, 0);
    let ret = match (&fd1, &fd2) {
        (Ok(a), Ok(b)) => compare_fds(*a, *b),
        _ => -1,
    };
    for fd in [fd1, fd2].into_iter().flatten() {
        let _ = sys::close(fd);
    }
    ret
}

fn compare_fds(fd1: Fd, fd2: Fd) -> i32 {
    let mut buf1 = [0u8; 1024];
    let mut buf2 = [0u8; 1024];
    loop {
        let (Ok(r1), Ok(r2)) = (read_chunk(fd1, &mut buf1), read_chunk(fd2, &mut buf2)) else {
            return -1;
        };
        if r1 != r2 || buf1[..r1] != buf2[..r2] {
            return 0;
        }
        if r1 == 0 && r2 == 0 {
            return 1;
        }
    }
}

/// `ischroot`: 0, 1 ou 2.
fn ischroot() -> i32 {
    let ret = ischroot_mountinfo();
    if ret >= 0 {
        return ret;
    }
    let Ok(st1) = sys::stat(b"/") else { return 2 };
    match sys::stat(b"/proc/1/root") {
        Err(_) => {
            // Does /proc/1/root exist at all?
            if sys::lstat(b"/proc/1/root").is_err() {
                return 2;
            }
            // Are we root?
            if sys::current().geteuid() != 0 {
                return 2;
            }
            // Root can not read /proc/1/root, assume vserver or similar
            0
        }
        Ok(st2) => {
            if st1.dev == st2.dev && st1.ino == st2.ino {
                1
            } else {
                0
            }
        }
    }
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);

    let mut default_false = false;
    let mut default_true = false;
    let mut g = Getopt::from_env(&argv[1..], "fthV", LONGS);
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!(
                    "{}\nTry `ischroot --help' for more information.\n",
                    e.message(&argv0)
                ));
                return 1;
            }
        };
        match o.short() {
            Some('f') => default_false = true,
            Some('t') => default_true = true,
            Some('h') => {
                let mut out = io::stdout();
                let _ = out.write_all(
                    b"Usage: ischroot [OPTION]\n  -f, --default-false return false if detection fails\n  -t, --default-true  return true if detection fails\n  -V, --version       output version information and exit.\n  -h, --help          display this help and exit.\n",
                );
                return 0;
            }
            Some('V') => {
                let mut out = io::stdout();
                let _ = out.write_all(
                    b"Debian ischroot, version 5.23.1\nCopyright (C) 2011 Aurelien Jarno\nThis is free software; see the GNU General Public License version 2\nor later for copying conditions.  There is NO warranty.\n",
                );
                return 0;
            }
            _ => {
                io::eprint("Try `ischroot --help' for more information.\n");
                return 1;
            }
        }
    }

    if default_false && default_true {
        io::eprint(
            "Can't default to both true and false!\nTry `ischroot --help' for more information.\n",
        );
        return 1;
    }

    let mut exit_status = if isfakechroot() { 0 } else { ischroot() };
    if exit_status == 2 {
        if default_true {
            exit_status = 0;
        }
        if default_false {
            exit_status = 1;
        }
    }
    exit_status
}
