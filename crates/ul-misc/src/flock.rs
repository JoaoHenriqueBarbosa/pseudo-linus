//! `flock` do util-linux 2.41 (pacote util-linux do Debian 13): trava de arquivo para scripts.
//!
//! Porte do `sys-utils/flock.c`. Abre o arquivo (ou diretório) com `O_RDONLY|O_NOCTTY|O_CREAT`,
//! cai para somente leitura em `EISDIR`/`EROFS`, toma a trava com `flock(2)` (ou `F_OFD_SETLK[W]`
//! com `--fcntl`) e roda o comando num filho (`-c` passa pelo `$SHELL`, ou `/bin/sh`).
//! O `-w` arma um `SIGALRM` capturado e um `ITIMER_REAL` de um disparo e toma a trava bloqueante, que o
//! sinal interrompe com `EINTR` (o fim do prazo), como o original.

use std::ffi::OsString;
use std::io::Write;

use sysabi::fcntl::{F_RDLCK, F_UNLCK, F_WRLCK, LOCK_EX, LOCK_NB, LOCK_SH, LOCK_UN, SEEK_SET};
use sysabi::{
    AccessMode, AtFlags, Clock, Ctx, Errno, Fd, Flock, Itimer, Itimerval, LockCmd, OFlags, ProcAttrs,
    SigDisposition, Signal, SysResult, Syscalls, WaitOptions, WaitStatus, WaitTarget, sys,
};
use ul_common::ctype::{WholeLong, strtod, strtol_whole};

use crate::setsid::execvp;
use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

/// Códigos do `sysexits.h` que o `flock` usa.
const EX_USAGE: i32 = 64;
const EX_DATAERR: i32 = 65;
const EX_NOINPUT: i32 = 66;
const EX_UNAVAILABLE: i32 = 69;
const EX_OSERR: i32 = 71;
const EX_CANTCREAT: i32 = 73;

/// Ids das opções longas sem letra (`CHAR_MAX + 1` em diante no original).
const OPT_VERBOSE: i32 = 0x100;
const OPT_FCNTL: i32 = 0x101;

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
    LongOpt::new("verbose", HasArg::No, OPT_VERBOSE),
    LongOpt::new("fcntl", HasArg::No, OPT_FCNTL),
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
 -s, --shared             get a shared lock
 -x, --exclusive          get an exclusive lock (default)
 -u, --unlock             remove a lock
 -n, --nonblock           fail rather than wait
 -w, --timeout <secs>     wait for a limited amount of time
 -E, --conflict-exit-code <number>  exit code after conflict or timeout
 -o, --close              close file descriptor before running command
 -c, --command <command>  run a single command string through the shell
 -F, --no-fork            execute command without forking
     --fcntl              use fcntl(F_OFD_SETLK) rather than flock()
     --verbose            increase verbosity

 -h, --help               display this help
 -V, --version            display version

For more details see flock(1).
"
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Shared,
    Exclusive,
    Unlock,
}

/// O que o laço de aquisição precisa saber, vindo das opções.
struct Plan<'a> {
    short: &'a str,
    sys: &'a dyn Syscalls,
    kind: Kind,
    nonblock: bool,
    use_fcntl: bool,
    verbose: bool,
    conflict_code: i32,
    /// `-w` com valor não nulo, em microssegundos.
    timeout_us: Option<u64>,
}

/// `strtotimeval_or_err`: segundos decimais não negativos (o `strtod` inteiro), em microssegundos.
fn parse_timeout(arg: &[u8]) -> Option<u64> {
    let c = strtod(arg);
    if c.used == 0 || c.used != arg.len() || c.overflow || !c.value.is_finite() || c.value < 0.0 {
        return None;
    }
    let secs = c.value.trunc();
    if secs >= i64::MAX as f64 / 1e6 {
        return None;
    }
    Some(secs as u64 * 1_000_000 + ((c.value - secs) * 1e6) as u64)
}

/// `strtoint_or_err` + a faixa de `-E`: a mensagem pronta (sem o prefixo do programa) em caso de erro.
fn parse_conflict_code(arg: &[u8]) -> Result<i32, String> {
    let text = io::lossy(arg);
    let range = |what: &str| format!("{what}: '{text}': {}", Errno::ERANGE.message());
    match strtol_whole(arg, 10) {
        WholeLong::Invalid => Err(format!("invalid exit code: '{text}'")),
        WholeLong::Range(_) => Err(range("invalid exit code")),
        WholeLong::Ok(n) if i32::try_from(n).is_err() => Err(range("invalid exit code")),
        WholeLong::Ok(n) if !(0..=255).contains(&n) => {
            Err("exit code out of range (expected 0 to 255)".to_string())
        }
        WholeLong::Ok(n) => Ok(n as i32),
    }
}

fn now_us(sys: &dyn Syscalls) -> u64 {
    let t = sys.clock_gettime(Clock::Monotonic).unwrap_or_default();
    t.sec as u64 * 1_000_000 + u64::from(t.nsec) / 1000
}

/// Uma tentativa de trava: `flock(2)`, ou `F_OFD_SETLK[W]` com `--fcntl` (EAGAIN em conflito nos dois).
fn lock_once(p: &Plan, fd: Fd, nonblock: bool) -> SysResult<()> {
    if p.use_fcntl {
        let l_type = match p.kind {
            Kind::Shared => F_RDLCK,
            Kind::Exclusive => F_WRLCK,
            Kind::Unlock => F_UNLCK,
        };
        let cmd = if nonblock { LockCmd::OfdSet } else { LockCmd::OfdSetWait };
        let request = Flock { l_type, whence: SEEK_SET, start: 0, len: 0, pid: 0 };
        return p.sys.fcntl_lock(fd, cmd, request).map(|_| ());
    }
    let op = match p.kind {
        Kind::Shared => LOCK_SH,
        Kind::Exclusive => LOCK_EX,
        Kind::Unlock => LOCK_UN,
    };
    p.sys.flock(fd, op | if nonblock { LOCK_NB } else { 0 })
}

/// Saída do `flock` quando o `open` do arquivo de trava falha.
fn open_exit_code(e: Errno) -> i32 {
    match e {
        Errno::ENOMEM | Errno::EMFILE | Errno::ENFILE => EX_OSERR,
        Errno::EROFS | Errno::ENOSPC => EX_CANTCREAT,
        _ => EX_NOINPUT,
    }
}

/// Abre o arquivo de trava: com `O_CREAT`, e só leitura se for diretório ou sistema só de leitura.
fn open_lock_file(sys: &dyn Syscalls, name: &[u8], flags: OFlags) -> SysResult<Fd> {
    let create = flags | OFlags::CREAT;
    match sys.openat(Fd::CWD, name, create, 0o666) {
        Err(e) if e == Errno::EISDIR || e == Errno::EROFS => sys.openat(Fd::CWD, name, flags, 0),
        r => r,
    }
}

/// O prazo do `-w`: um `SIGALRM` capturado e um `ITIMER_REAL` de um disparo, que interrompem a trava bloqueante
/// com `EINTR`. Guarda o que havia antes para devolver ao fim.
struct Timeout {
    old_action: SigDisposition,
    old_timer: Itimerval,
}

impl Timeout {
    fn arm(sys: &dyn Syscalls, us: u64) -> SysResult<Timeout> {
        let old_action = sys.sigaction(Signal::SIGALRM, SigDisposition::Catch)?;
        let timer = Itimerval { value_sec: (us / 1_000_000) as i64, value_usec: (us % 1_000_000) as i64, ..Itimerval::default() };
        match sys.setitimer(Itimer::Real as i32, timer) {
            Ok(old_timer) => Ok(Timeout { old_action, old_timer }),
            Err(e) => {
                let _ = sys.sigaction(Signal::SIGALRM, old_action);
                Err(e)
            }
        }
    }

    /// Cancela o timer (devolvendo o anterior) e restaura o tratador do `SIGALRM`.
    fn cancel(self, sys: &dyn Syscalls) {
        let _ = sys.setitimer(Itimer::Real as i32, self.old_timer);
        let _ = sys.sigaction(Signal::SIGALRM, self.old_action);
    }
}

/// O laço de aquisição do `main` do original. `Err(code)` é o código de saída do `flock`.
fn acquire(p: &Plan, fd: &mut Fd, filename: Option<&[u8]>) -> Result<(), i32> {
    let timeout = match p.timeout_us.filter(|_| !p.nonblock) {
        Some(us) => match Timeout::arm(p.sys, us) {
            Ok(t) => Some(t),
            Err(e) => return Err(report_lock_error(p, fd.0, filename, e)),
        },
        None => None,
    };
    let r = acquire_loop(p, fd, filename, timeout.is_some());
    if let Some(t) = timeout {
        t.cancel(p.sys);
    }
    r
}

/// A trava em si; `timed` diz que o `-w` armou o `SIGALRM`, cuja chegada (`EINTR`) é o fim do prazo.
fn acquire_loop(p: &Plan, fd: &mut Fd, filename: Option<&[u8]>, timed: bool) -> Result<(), i32> {
    let mut rdwr = false;
    let started = now_us(p.sys);
    loop {
        match lock_once(p, *fd, p.nonblock) {
            Ok(()) => break,
            Err(Errno::EAGAIN) => {
                if p.verbose {
                    ul::warnx(p.short, "failed to get lock");
                }
                return Err(p.conflict_code);
            }
            Err(Errno::EINTR) => {
                if timed && p.sys.take_caught_signals().contains(&Signal::SIGALRM) {
                    if p.verbose {
                        ul::warnx(p.short, "timeout while waiting to get lock");
                    }
                    return Err(p.conflict_code);
                }
            }
            Err(e @ (Errno::EIO | Errno::EBADF)) => {
                // Provavelmente NFSv4, onde o flock() é emulado por fcntl(): tenta reabrir em leitura e escrita.
                let reopen = filename.filter(|name| {
                    !rdwr
                        && p.kind != Kind::Shared
                        && p.sys
                            .faccessat(Fd::CWD, name, AccessMode::R_OK | AccessMode::W_OK, AtFlags::empty())
                            .is_ok()
                });
                let Some(name) = reopen else {
                    return Err(report_lock_error(p, fd.0, filename, e));
                };
                let _ = p.sys.close(*fd);
                rdwr = true;
                match p.sys.openat(Fd::CWD, name, OFlags::RDWR | OFlags::NOCTTY | OFlags::CREAT, 0o666) {
                    Ok(new_fd) => *fd = new_fd,
                    Err(e2) => {
                        ul::warn(p.short, format!("cannot open lock file {}", io::lossy(name)), e2);
                        return Err(open_exit_code(e2));
                    }
                }
            }
            Err(e) => return Err(report_lock_error(p, fd.0, filename, e)),
        }
    }
    if p.verbose {
        let took = now_us(p.sys).saturating_sub(started);
        io::eprint(format!(
            "{}: getting lock took {}.{:06} seconds\n",
            p.short,
            took / 1_000_000,
            took % 1_000_000
        ));
    }
    Ok(())
}

/// `warn("%s", filename)` (ou `warn("%d", fd)`) e `EX_DATAERR` para erro de trava fora do conflito.
fn report_lock_error(p: &Plan, fd: i32, filename: Option<&[u8]>, e: Errno) -> i32 {
    let what = filename.map_or_else(|| fd.to_string(), |n| io::lossy(n).to_string());
    ul::warn(p.short, what, e);
    EX_DATAERR
}

/// Roda o comando num filho (ou no próprio processo com `-F`) e devolve o código de saída.
fn run_command(
    short: &str,
    cmd: Vec<Vec<u8>>,
    fd: Fd,
    close_fd: bool,
    no_fork: bool,
    verbose: bool,
) -> i32 {
    let sys = sys::current();
    if verbose {
        io::eprint(format!("{short}: executing {}\n", io::lossy(&cmd[0])));
    }
    let _ = io::flush_stdout();
    let exec_failed = |short: &str, cmd: &[Vec<u8>]| -> i32 {
        let e = execvp(&cmd[0], cmd);
        ul::warn(short, format!("failed to execute {}", io::lossy(&cmd[0])), e);
        if e == Errno::ENOMEM { EX_OSERR } else { EX_UNAVAILABLE }
    };
    if no_fork {
        return exec_failed(short, &cmd);
    }
    let child_short = short.to_string();
    let child_cmd = cmd.clone();
    let body: sysabi::ProcessFn = Box::new(move || {
        if close_fd {
            let _ = sys::current().close(fd);
        }
        exec_failed(&child_short, &child_cmd)
    });
    let child = match sys.spawn_fn(ProcAttrs::default(), cmd[0].clone(), body) {
        Ok(pid) => pid,
        Err(e) => {
            ul::warn(short, "fork failed", e);
            return EX_OSERR;
        }
    };
    loop {
        match sys.wait4(WaitTarget::Pid(child), WaitOptions::empty()) {
            Err(Errno::EINTR) => continue,
            Ok(Some((_, WaitStatus::Exited(code)))) => return code & 0xff,
            Ok(Some((_, st @ WaitStatus::Signaled { .. }))) => return st.shell_status(),
            Ok(Some(_)) => return EX_OSERR,
            Ok(None) => continue,
            Err(e) => {
                ul::warn(short, "waitpid failed", e);
                return 1;
            }
        }
    }
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    if argv.len() < 2 {
        ul::warnx(&short, "not enough arguments");
        ul::errtryhelp(&short);
        return EX_USAGE;
    }

    let sys = sys::current();
    let mut plan = Plan {
        short: &short,
        sys: &*sys,
        kind: Kind::Exclusive,
        nonblock: false,
        use_fcntl: false,
        verbose: false,
        conflict_code: 1,
        timeout_us: None,
    };
    let mut close_fd = false;
    let mut no_fork = false;

    let mut g = Getopt::from_env(&argv[1..], "+sexnoFuw:E:hV?", LONGS);
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                ul::errtryhelp(&short);
                return EX_USAGE;
            }
        };
        match o.id {
            OPT_VERBOSE => plan.verbose = true,
            OPT_FCNTL => plan.use_fcntl = true,
            _ => match o.short() {
                Some('s') => plan.kind = Kind::Shared,
                Some('e') | Some('x') => plan.kind = Kind::Exclusive,
                Some('u') => plan.kind = Kind::Unlock,
                Some('o') => close_fd = true,
                Some('F') => no_fork = true,
                Some('n') => plan.nonblock = true,
                Some('w') => match parse_timeout(o.arg.as_deref().unwrap_or_default()) {
                    // `-w 0` vale `-n`: o itimer zerado desligaria o temporizador.
                    Some(0) => plan.nonblock = true,
                    Some(us) => plan.timeout_us = Some(us),
                    None => {
                        ul::warnx(&short, format!("invalid timeout value: '{}'", o.arg_str()));
                        return EX_USAGE;
                    }
                },
                Some('E') => match parse_conflict_code(o.arg.as_deref().unwrap_or_default()) {
                    Ok(n) => plan.conflict_code = n,
                    Err(msg) => {
                        ul::warnx(&short, msg);
                        return EX_USAGE;
                    }
                },
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
            },
        }
    }

    if no_fork && close_fd {
        ul::warnx(&short, "the --no-fork and --close options are incompatible");
        return EX_USAGE;
    }

    let ops = g.operands();
    let mut command: Option<Vec<Vec<u8>>> = None;
    let mut filename: Option<&[u8]> = None;
    let mut fd;
    let open_flags = OFlags::RDONLY | OFlags::NOCTTY;
    if ops.len() > 1 {
        if ops[1] == b"-c" || ops[1] == b"--command" {
            if ops.len() != 3 {
                ul::warnx(
                    &short,
                    format!("{} requires exactly one command argument", io::lossy(&ops[1])),
                );
                return EX_USAGE;
            }
            let shell = sys.getenv(b"SHELL").filter(|s| !s.is_empty());
            let shell = shell.unwrap_or_else(|| b"/bin/sh".to_vec());
            command = Some(vec![shell, b"-c".to_vec(), ops[2].clone()]);
        } else {
            command = Some(ops[1..].to_vec());
        }
        filename = Some(&ops[0]);
        fd = match open_lock_file(&*sys, &ops[0], open_flags) {
            Ok(fd) => fd,
            Err(e) => {
                ul::warn(&short, format!("cannot open lock file {}", io::lossy(&ops[0])), e);
                return open_exit_code(e);
            }
        };
    } else if ops.len() == 1 {
        let n = match strtol_whole(&ops[0], 10) {
            WholeLong::Ok(n) => n,
            WholeLong::Invalid => {
                ul::warnx(&short, format!("bad file descriptor: '{}'", io::lossy(&ops[0])));
                return EX_USAGE;
            }
            WholeLong::Range(_) => {
                let msg = format!(
                    "bad file descriptor: '{}': {}",
                    io::lossy(&ops[0]),
                    Errno::ERANGE.message()
                );
                ul::warnx(&short, msg);
                return EX_USAGE;
            }
        };
        fd = Fd(n as i32);
    } else {
        ul::warnx(&short, "requires file descriptor, file or directory");
        return EX_USAGE;
    }

    if let Err(code) = acquire(&plan, &mut fd, filename) {
        return code;
    }
    match command {
        Some(cmd) => run_command(&short, cmd, fd, close_fd, no_fork, plan.verbose),
        None => 0,
    }
}
