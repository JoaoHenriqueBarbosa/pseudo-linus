//! `flock` do util-linux 2.41 (pacote util-linux do Debian 13): trava de arquivo para scripts.
//!
//! Porte do `sys-utils/flock.c`. Abre o arquivo (ou diretório) com `O_RDONLY|O_NOCTTY|O_CREAT`,
//! cai para somente leitura em `EISDIR`/`EROFS`, e executa o comando (`-c` passa pelo shell).
//! LIMITAÇÃO: o trait `Syscalls` do `sysabi` ainda não tem `flock(2)`, então a trava em si não é
//! tomada: a abertura, a validação de argumentos, as mensagens e os códigos de saída são fiéis, mas
//! o conflito nunca acontece (`-n`, `-w` e `-E` só são validados).

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Errno, Fd, OFlags, sys};

use crate::setsid::execvp;
use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

/// `EX_USAGE`, `EX_NOINPUT`, `EX_CANTCREAT`, `EX_NOPERM` do `sysexits.h`.
const EX_USAGE: i32 = 64;
const EX_NOINPUT: i32 = 66;
const EX_CANTCREAT: i32 = 73;
const EX_NOPERM: i32 = 77;
/// `EX_EXEC_FAILED` e `EX_EXEC_ENOENT` do `exitcodes.h`.
const EX_EXEC_FAILED: i32 = 126;
const EX_EXEC_ENOENT: i32 = 127;

const LONGS: &[LongOpt] = &[
    LongOpt::new("shared", HasArg::No, b's' as i32),
    LongOpt::new("exclusive", HasArg::No, b'x' as i32),
    LongOpt::new("unlock", HasArg::No, b'u' as i32),
    LongOpt::new("nonblocking", HasArg::No, b'n' as i32),
    LongOpt::new("nb", HasArg::No, b'n' as i32),
    LongOpt::new("nonblock", HasArg::No, b'n' as i32),
    LongOpt::new("timeout", HasArg::Required, b'w' as i32),
    LongOpt::new("wait", HasArg::Required, b'w' as i32),
    LongOpt::new("conflict-exit-code", HasArg::Required, b'E' as i32),
    LongOpt::new("close", HasArg::No, b'o' as i32),
    LongOpt::new("no-fork", HasArg::No, b'F' as i32),
    LongOpt::new("command", HasArg::Required, b'c' as i32),
    LongOpt::new("verbose", HasArg::No, b'v' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
];

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn usage(short: &str) -> String {
    format!(
        "
Usage:
 {short} [options] <file>|<directory> <command> [<argument>...]
 {short} [options] <file>|<directory> -c <command>
 {short} [options] <file descriptor number>

Manage file locks from shell scripts.

Options:
 -s, --shared               get a shared lock
 -x, --exclusive            get an exclusive lock (default)
 -u, --unlock               remove a lock
 -n, --nonblock             fail rather than wait
 -w, --timeout <secs>       wait for a limited amount of time
 -E, --conflict-exit-code <number>  exit code after conflict or timeout
 -o, --close                close file descriptor before running command
 -c, --command <command>    run a single command string through the shell
 -F, --no-fork              execute command without forking

 -h, --help                 display this help
 -V, --version              display version

For more details see flock(1).
"
    )
}

/// Valida o argumento de `-w` como o `strtotimeval` do util-linux: número decimal não negativo.
fn valid_timeout(s: &str) -> bool {
    let t = s.trim_start();
    if t.is_empty() || t.starts_with('-') {
        return false;
    }
    let t = t.strip_prefix('+').unwrap_or(t);
    let (int, frac) = match t.split_once('.') {
        Some((a, b)) => (a, b),
        None => (t, ""),
    };
    !(int.is_empty() && frac.is_empty())
        && int.bytes().all(|b| b.is_ascii_digit())
        && frac.bytes().all(|b| b.is_ascii_digit())
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut have_lock_mode = false;
    let mut conflict_code: i32 = 1;
    let mut close_fd = false;
    let mut command: Option<Vec<u8>> = None;

    let mut g2 = Getopt::from_env(&argv[1..], "+sxnoE:uw:hVc:Fv", LONGS);
    while let Some(r) = g2.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                ul::errtryhelp(&short);
                return EX_USAGE;
            }
        };
        match o.short() {
            Some('s') | Some('x') | Some('u') => have_lock_mode = true,
            Some('n') | Some('F') | Some('v') => {}
            Some('o') => close_fd = true,
            Some('w') => {
                let a = o.arg_str();
                if !valid_timeout(&a) {
                    ul::warnx(&short, format!("invalid timeout value: '{a}'"));
                    return EX_USAGE;
                }
            }
            Some('E') => {
                let a = o.arg_str();
                match a.trim().parse::<i64>() {
                    Ok(n) if (0..=255).contains(&n) => conflict_code = n as i32,
                    _ => {
                        ul::warnx(&short, format!("invalid exit code: '{a}'"));
                        return EX_USAGE;
                    }
                }
            }
            Some('c') => command = Some(o.arg.clone().unwrap_or_default()),
            Some('h') => {
                let mut out = io::stdout();
                let _ = out.write_all(usage(&short).as_bytes());
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
    let _ = conflict_code;
    let _ = have_lock_mode;

    let ops = g2.operands();
    if ops.is_empty() {
        ul::warnx(&short, "requires file descriptor, file or directory");
        ul::errtryhelp(&short);
        return EX_USAGE;
    }
    let sys = sys::current();
    let target = &ops[0];
    let rest = &ops[1..];

    if command.is_none() && rest.is_empty() {
        // Modo `flock <fd>`: trava (ou destrava) um descritor já aberto.
        let digits = target.iter().all(|b| b.is_ascii_digit()) && !target.is_empty();
        if !digits {
            ul::warnx(&short, "requires file descriptor, file or directory");
            ul::errtryhelp(&short);
            return EX_USAGE;
        }
        let n: i32 = io::lossy(target).parse().unwrap_or(-1);
        if n < 0 || sys.fstat(Fd(n)).is_err() {
            ul::warn(&short, format!("bad file descriptor: {n}"), Errno::EBADF);
            return EX_USAGE;
        }
        return 0;
    }

    let rdonly = OFlags::RDONLY | OFlags::NOCTTY;
    let fd = match sys.openat(Fd::CWD, target, rdonly | OFlags::CREAT, 0o666) {
        Ok(fd) => fd,
        Err(e) if e == Errno::EISDIR || e == Errno::EROFS => {
            match sys.openat(Fd::CWD, target, rdonly, 0) {
                Ok(fd) => fd,
                Err(e2) => return open_failed(&short, target, e2),
            }
        }
        Err(e) => return open_failed(&short, target, e),
    };

    let cmd: Vec<Vec<u8>> = match command {
        Some(c) => {
            if !rest.is_empty() {
                ul::warnx(&short, "the --command option requires exactly one command argument");
                ul::errtryhelp(&short);
                return EX_USAGE;
            }
            let shell = sys.getenv(b"SHELL").unwrap_or_else(|| b"/bin/sh".to_vec());
            vec![shell, b"-c".to_vec(), c]
        }
        None => rest.to_vec(),
    };

    if close_fd {
        let _ = sys.close(fd);
    }
    let _ = io::flush_stdout();
    let e = execvp(&cmd[0], &cmd);
    ul::warn(
        &short,
        format!("failed to execute {}", io::lossy(&cmd[0])),
        e,
    );
    if e == Errno::ENOENT {
        EX_EXEC_ENOENT
    } else {
        EX_EXEC_FAILED
    }
}

fn open_failed(short: &str, target: &[u8], e: Errno) -> i32 {
    ul::warn(
        short,
        format!("cannot open lock file {}", io::lossy(target)),
        e,
    );
    match e {
        Errno::ENOENT => EX_NOINPUT,
        Errno::EACCES => EX_NOPERM,
        _ => EX_CANTCREAT,
    }
}
