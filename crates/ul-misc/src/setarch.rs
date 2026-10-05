//! `setarch` do util-linux 2.41 (e os links `linux32`, `linux64`, `i386`, `x86_64`, `uname26`): muda a
//! arquitetura reportada e os bits de personality antes de executar um programa.
//!
//! Porte do `sys-utils/setarch.c`. Quando o nome do programa não é `setarch`, ele próprio é a
//! arquitetura. A personality sai pela syscall `personality` do `sysabi`, que segue o seccomp padrão do
//! docker: só `PER_LINUX`, `PER_LINUX32` e `UNAME26` passam, o resto dá EPERM.

use std::ffi::OsString;
use std::io::Write;

use sysabi::sched::personality as per;
use sysabi::{Errno, sys};

use crate::util::io;
use crate::util::ul;

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

const ARCHS: &[(&str, u32)] = &[
    ("linux32", per::PER_LINUX32),
    ("linux64", per::PER_LINUX),
    ("i386", per::PER_LINUX32),
    ("i486", per::PER_LINUX32),
    ("i586", per::PER_LINUX32),
    ("i686", per::PER_LINUX32),
    ("athlon", per::PER_LINUX32),
    ("x86_64", per::PER_LINUX),
    ("uname26", per::PER_LINUX),
];

fn usage(short: &str) -> String {
    format!(
        "
Usage:
 {short} [<arch>] [options] [<program> [<argument>...]]

Change the reported architecture and set personality flags.

Options:
 -v, --verbose            say what options are being switched on
 -R, --addr-no-randomize  disable address space layout randomization
 -F, --fdpic-funcptrs     makes function pointers point to descriptors
 -Z, --mmap-page-zero     turns on MMAP_PAGE_ZERO
 -L, --addr-compat-layout changes the way virtual memory is allocated
 -X, --read-implies-exec  turns on READ_IMPLIES_EXEC
 -B, --32bit              turns on ADDR_LIMIT_32BIT
 -I, --short-inode        turns on SHORT_INODE
 -S, --whole-seconds      turns on WHOLE_SECONDS
 -T, --sticky-timeouts    turns on STICKY_TIMEOUTS
 -3, --3gb                limits the used address space to a maximum of 3 GB
     --4gb                ignored (for compatibility only)
     --uname-2.6          turns on UNAME26
     --list               list settable architectures, and exit

 -h, --help               display this help
 -V, --version            display version

For more details see setarch(8).
"
    )
}

/// `execvp`: com barra executa direto, sem barra procura no `PATH`.
fn execvp(prog: &[u8], argv: &[Vec<u8>]) -> Errno {
    let sys = sys::current();
    if prog.contains(&b'/') {
        return sys.execve(prog, argv, None);
    }
    let path = sys.getenv(b"PATH").unwrap_or_else(|| b"/usr/local/bin:/usr/bin:/bin".to_vec());
    let mut last = Errno::ENOENT;
    for dir in path.split(|b| *b == b':') {
        let mut full = if dir.is_empty() { b".".to_vec() } else { dir.to_vec() };
        full.push(b'/');
        full.extend_from_slice(prog);
        let e = sys.execve(&full, argv, None);
        if e == Errno::EACCES {
            last = e;
        } else if e != Errno::ENOENT && e != Errno::ENOTDIR && last != Errno::EACCES {
            last = e;
        }
    }
    last
}

fn flag_for_short(c: u8) -> Option<(u32, &'static str)> {
    Some(match c {
        b'R' => (per::ADDR_NO_RANDOMIZE, "ADDR_NO_RANDOMIZE"),
        b'F' => (per::FDPIC_FUNCPTRS, "FDPIC_FUNCPTRS"),
        b'Z' => (per::MMAP_PAGE_ZERO, "MMAP_PAGE_ZERO"),
        b'L' => (per::ADDR_COMPAT_LAYOUT, "ADDR_COMPAT_LAYOUT"),
        b'X' => (per::READ_IMPLIES_EXEC, "READ_IMPLIES_EXEC"),
        b'B' => (per::ADDR_LIMIT_32BIT, "ADDR_LIMIT_32BIT"),
        b'I' => (per::SHORT_INODE, "SHORT_INODE"),
        b'S' => (per::WHOLE_SECONDS, "WHOLE_SECONDS"),
        b'T' => (per::STICKY_TIMEOUTS, "STICKY_TIMEOUTS"),
        b'3' => (per::ADDR_LIMIT_3GB, "ADDR_LIMIT_3GB"),
        _ => return None,
    })
}

fn flag_for_long(name: &[u8]) -> Option<(u32, &'static str)> {
    Some(match name {
        b"addr-no-randomize" => (per::ADDR_NO_RANDOMIZE, "ADDR_NO_RANDOMIZE"),
        b"fdpic-funcptrs" => (per::FDPIC_FUNCPTRS, "FDPIC_FUNCPTRS"),
        b"mmap-page-zero" => (per::MMAP_PAGE_ZERO, "MMAP_PAGE_ZERO"),
        b"addr-compat-layout" => (per::ADDR_COMPAT_LAYOUT, "ADDR_COMPAT_LAYOUT"),
        b"read-implies-exec" => (per::READ_IMPLIES_EXEC, "READ_IMPLIES_EXEC"),
        b"32bit" => (per::ADDR_LIMIT_32BIT, "ADDR_LIMIT_32BIT"),
        b"short-inode" => (per::SHORT_INODE, "SHORT_INODE"),
        b"whole-seconds" => (per::WHOLE_SECONDS, "WHOLE_SECONDS"),
        b"sticky-timeouts" => (per::STICKY_TIMEOUTS, "STICKY_TIMEOUTS"),
        b"3gb" => (per::ADDR_LIMIT_3GB, "ADDR_LIMIT_3GB"),
        b"uname-2.6" => (per::UNAME26, "UNAME26"),
        _ => return None,
    })
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let short = ul::short_name(args);
    let mut out = io::stdout();

    let mut rest: &[Vec<u8>] = &argv[1..];
    let mut arch: Option<Vec<u8>> = None;
    if short != "setarch" {
        arch = Some(short.as_bytes().to_vec());
    } else if let Some(first) = rest.first() {
        if !first.starts_with(b"-") {
            arch = Some(first.clone());
            rest = &rest[1..];
        }
    }

    let mut verbose = false;
    let mut flags: u32 = 0;
    let mut names: Vec<&'static str> = Vec::new();
    let mut idx = 0;
    while idx < rest.len() {
        let a = &rest[idx];
        if a == b"--" {
            idx += 1;
            break;
        }
        if a.len() < 2 || a[0] != b'-' {
            break;
        }
        idx += 1;
        if a.starts_with(b"--") {
            let name = &a[2..];
            match name {
                b"help" => {
                    let _ = out.write_all(usage(&short).as_bytes());
                    return 0;
                }
                b"version" => {
                    ul::print_version(&short);
                    return 0;
                }
                b"list" => {
                    for (n, _) in ARCHS {
                        let _ = out.write_all(format!("{n}\n").as_bytes());
                    }
                    return 0;
                }
                b"verbose" => verbose = true,
                b"4gb" => {}
                _ => match flag_for_long(name) {
                    Some((f, n)) => {
                        flags |= f;
                        names.push(n);
                    }
                    None => {
                        ul::warnx(&short, format!("unrecognized option '--{}'", io::lossy(name)));
                        ul::errtryhelp(&short);
                        return 1;
                    }
                },
            }
            continue;
        }
        for &c in &a[1..] {
            match c {
                b'h' => {
                    let _ = out.write_all(usage(&short).as_bytes());
                    return 0;
                }
                b'V' => {
                    ul::print_version(&short);
                    return 0;
                }
                b'v' => verbose = true,
                _ => match flag_for_short(c) {
                    Some((f, n)) => {
                        flags |= f;
                        names.push(n);
                    }
                    None => {
                        ul::warnx(&short, format!("invalid option -- '{}'", c as char));
                        ul::errtryhelp(&short);
                        return 1;
                    }
                },
            }
        }
    }
    rest = &rest[idx..];

    let arch = match arch {
        Some(a) => a,
        None => {
            ul::warnx(&short, "no architecture argument");
            ul::errtryhelp(&short);
            return 1;
        }
    };
    let domain = match ARCHS.iter().find(|(n, _)| n.as_bytes() == arch.as_slice()) {
        Some((n, d)) => {
            if *n == "uname26" {
                flags |= per::UNAME26;
                names.push("UNAME26");
            }
            *d
        }
        None => {
            ul::warnx(&short, format!("{}: Unrecognized architecture", io::lossy(&arch)));
            return 1;
        }
    };

    if verbose {
        for n in &names {
            let _ = out.write_all(format!("Switching on {n}.\n").as_bytes());
        }
    }

    let sys = sys::current();
    let pers = domain | flags;
    if verbose {
        let _ = out.write_all(format!("Set personality 0x{pers:04x}\n").as_bytes());
    }
    if let Err(e) = sys.personality(pers) {
        ul::warn(&short, format!("failed to set personality to {}", io::lossy(&arch)), e);
        return 1;
    }

    let (prog, cargv): (Vec<u8>, Vec<Vec<u8>>) = if rest.is_empty() {
        let shell = sys.getenv(b"SHELL").filter(|s| !s.is_empty()).unwrap_or_else(|| b"/bin/sh".to_vec());
        (shell.clone(), vec![shell, b"-i".to_vec()])
    } else {
        (rest[0].clone(), rest.to_vec())
    };
    if verbose {
        let _ = out.write_all(format!("Execute command `{}'.\n", io::lossy(&prog)).as_bytes());
    }
    let _ = out.flush();
    let e = execvp(&prog, &cargv);
    ul::warn(&short, format!("failed to execute {}", io::lossy(&prog)), e);
    1
}
