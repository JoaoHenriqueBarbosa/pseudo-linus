//! `setsid` do util-linux 2.41 (pacote util-linux do Debian 13): roda um programa numa sessão nova.
//!
//! Porte do `sys-utils/setsid.c`: quando o processo já é líder de grupo (ou com `-f`) faz `fork` e o
//! pai sai na hora com 0, ou com `-w` espera o filho e devolve o código dele. O filho (ou o próprio
//! processo) chama `setsid()`, com `-c` toma o terminal do stdin como terminal de controle, e faz
//! `execvp` do programa. Falha no exec sai com 127 (não achou) ou 126 (outro erro).

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Errno, ProcAttrs, WaitOptions, WaitStatus, WaitTarget, sys};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

/// `EX_EXEC_FAILED` e `EX_EXEC_ENOENT` do `exitcodes.h`.
const EX_EXEC_FAILED: i32 = 126;
const EX_EXEC_ENOENT: i32 = 127;

const LONGS: &[LongOpt] = &[
    LongOpt::new("ctty", HasArg::No, b'c' as i32),
    LongOpt::new("fork", HasArg::No, b'f' as i32),
    LongOpt::new("wait", HasArg::No, b'w' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
];

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn usage(short: &str) -> String {
    format!(
        "
Usage:
 {short} [options] <program> [<argument>...]

Run a program in a new session.

Options:
 -c, --ctty     set the controlling terminal to the current one
 -f, --fork     always fork
 -w, --wait     wait program to exit, and use the same return
 -h, --help     display this help
 -V, --version  display version

For more details see setsid(1).
"
    )
}

/// `execvp(3)` da glibc: com `/` no nome vai direto; senão tenta cada diretório do `PATH` (ou
/// `/bin:/usr/bin` sem ele), lembrando EACCES e seguindo em ENOENT, ENOTDIR e ESTALE. ENOEXEC roda
/// o arquivo com `/bin/sh`. Só volta em erro.
fn execvp(file: &[u8], argv: &[Vec<u8>]) -> Errno {
    let sys = sys::current();
    let try_exec = |path: &[u8]| -> Errno {
        let e = sys.execve(path, argv, None);
        if e == Errno::ENOEXEC {
            let mut sh_argv = vec![b"/bin/sh".to_vec(), path.to_vec()];
            sh_argv.extend(argv.iter().skip(1).cloned());
            let _ = sys.execve(b"/bin/sh", &sh_argv, None);
        }
        e
    };
    if file.is_empty() {
        return Errno::ENOENT;
    }
    if file.contains(&b'/') {
        return try_exec(file);
    }
    let path = sys.getenv(b"PATH").unwrap_or_else(|| b"/bin:/usr/bin".to_vec());
    let mut got_eacces = false;
    let mut last = Errno::ENOENT;
    for dir in path.split(|b| *b == b':') {
        let mut full = if dir.is_empty() { Vec::new() } else { dir.to_vec() };
        if !full.is_empty() {
            full.push(b'/');
        }
        full.extend_from_slice(file);
        let e = try_exec(&full);
        match e {
            Errno::EACCES => got_eacces = true,
            Errno::ENOENT | Errno::ENOTDIR | Errno::ESTALE => {}
            Errno::ENODEV | Errno::ETIMEDOUT => {}
            other => return other,
        }
        last = e;
    }
    if got_eacces { Errno::EACCES } else { last }
}

/// O que o processo da sessão nova faz: `setsid()`, o terminal de controle e o `execvp`.
fn session_body(short: &str, ctty: bool, cmd: &[Vec<u8>]) -> i32 {
    let sys = sys::current();
    if let Err(e) = sys.setsid() {
        ul::warn(short, "setsid failed", e);
        return 1;
    }
    if ctty {
        // ioctl(STDIN_FILENO, TIOCSCTTY, 1): sem terminal no stdin é ENOTTY. Com terminal, ele é o de
        // controle da sessão de onde o setsid veio, e roubá-lo de outra sessão exige root (EPERM).
        let err = if !sys.isatty(sysabi::Fd::STDIN) {
            Some(Errno::ENOTTY)
        } else if sys.geteuid() != 0 {
            Some(Errno::EPERM)
        } else {
            None
        };
        if let Some(e) = err {
            ul::warn(short, "failed to set the controlling terminal", e);
            return 1;
        }
    }
    let _ = io::flush_stdout();
    let e = execvp(&cmd[0], cmd);
    ul::warn(short, format!("failed to execute {}", io::lossy(&cmd[0])), e);
    if e == Errno::ENOENT { EX_EXEC_ENOENT } else { EX_EXEC_FAILED }
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut ctty = false;
    let mut forcefork = false;
    let mut wait = false;

    let mut g = Getopt::from_env(&argv[1..], "+Vhcfw", LONGS);
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
            Some('c') => ctty = true,
            Some('f') => forcefork = true,
            Some('w') => wait = true,
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
                return 1;
            }
        }
    }

    let cmd = g.operands();
    if cmd.is_empty() {
        ul::warnx(&short, "no command specified");
        ul::errtryhelp(&short);
        return 1;
    }

    let sys = sys::current();
    let pid = sys.getpid();
    let leader = sys.getpgid(pid).is_ok_and(|pg| pg == pid);
    if !(forcefork || leader) {
        return session_body(&short, ctty, &cmd);
    }

    let _ = io::flush_stdout();
    let child_short = short.clone();
    let child_cmd = cmd.clone();
    let body: sysabi::ProcessFn = Box::new(move || session_body(&child_short, ctty, &child_cmd));
    let child = match sys.spawn_fn(ProcAttrs::default(), cmd[0].clone(), body) {
        Ok(p) => p,
        Err(e) => {
            ul::warn(&short, "fork", e);
            return 1;
        }
    };
    if !wait {
        return 0;
    }
    let status = loop {
        match sys.wait4(WaitTarget::Any, WaitOptions::empty()) {
            Err(Errno::EINTR) => continue,
            Ok(Some((p, st))) if p == child => break st,
            Ok(_) => {
                ul::warn(&short, "wait", Errno::ECHILD);
                return 1;
            }
            Err(e) => {
                ul::warn(&short, "wait", e);
                return 1;
            }
        }
    };
    match status {
        WaitStatus::Exited(code) => code & 0xff,
        WaitStatus::Signaled { signal, core_dumped } => {
            // err(status, ...): o errno ainda é o do wait bem-sucedido (0, "Success"), e o código de
            // saída é o status cru do wait (o número do sinal, com 0x80 se gerou core).
            let raw = signal.0 | if core_dumped { 0x80 } else { 0 };
            io::eprint(format!("{short}: child {child} did not exit normally: Success\n"));
            raw & 0xff
        }
        WaitStatus::Stopped(_) | WaitStatus::Continued => 1,
    }
}
