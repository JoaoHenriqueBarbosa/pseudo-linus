//! `git-shell` do git 2.47.3 do Debian 13: porte do `shell.c`.
//!
//! O shell de login restrito: aceita só `-c "<comando>"` com `git-receive-pack`, `git-upload-pack`,
//! `git-upload-archive` (também escritos como `git receive-pack ...`), `cvs server` ou um programa
//! de `~/git-shell-commands`. Sem argumentos entra no modo interativo, que só existe se
//! `~/git-shell-commands` for legível e executável pelo usuário.

use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;

use sysabi::{Ctx, Errno, ProcAttrs, WaitOptions, WaitStatus, WaitTarget, sys};

use crate::{cmd, os};

const COMMAND_DIR: &[u8] = b"git-shell-commands";
const HELP_COMMAND: &[u8] = b"git-shell-commands/help";
const NOLOGIN_COMMAND: &[u8] = b"git-shell-commands/no-interactive-login";

/// `die()`: `fatal: <msg>` e código 128.
fn die(msg: &str) -> i32 {
    os::flush_out();
    os::err_line("fatal: ", msg);
    128
}

/// Entrada do programa `git-shell`.
pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let argv: Vec<Vec<u8>> = args.iter().map(|a| a.as_bytes().to_vec()).collect();
    let saved = os::swap_out(Vec::new());
    let code = run(&argv);
    os::flush_out();
    os::swap_out(saved);
    code
}

/// `cd_to_homedir()`: `Err` traz o código já com a mensagem impressa.
fn cd_to_homedir() -> Result<(), i32> {
    let Some(home) = os::getenv("HOME") else {
        return Err(die("could not determine user's home directory; HOME is unset"));
    };
    if os::chdir(&home).is_err() {
        return Err(die("could not chdir to user's home directory"));
    }
    Ok(())
}

/// `is_valid_cmd_name`: sem `.` nem `/`.
fn is_valid_cmd_name(cmd: &[u8]) -> bool {
    !cmd.iter().any(|b| *b == b'.' || *b == b'/')
}

/// `split_cmdline` do git: aspas simples e duplas e barra invertida. `Err` traz a mensagem do erro.
fn split_cmdline(line: &[u8]) -> Result<Vec<Vec<u8>>, &'static str> {
    use ul_common::ctype::is_space;
    let mut out: Vec<Vec<u8>> = Vec::new();
    let mut cur: Vec<u8> = Vec::new();
    let mut in_word = false;
    let mut quote: u8 = 0;
    let mut i = 0;
    while i < line.len() {
        let c = line[i];
        i += 1;
        if quote == 0 && is_space(c) {
            if in_word {
                out.push(std::mem::take(&mut cur));
                in_word = false;
            }
            continue;
        }
        in_word = true;
        if quote == 0 && (c == b'\'' || c == b'"') {
            quote = c;
        } else if c == quote {
            quote = 0;
        } else if c == b'\\' && quote != b'\'' {
            if i >= line.len() {
                return Err("cmdline ends with \\");
            }
            cur.push(line[i]);
            i += 1;
        } else {
            cur.push(c);
        }
    }
    if quote != 0 {
        return Err("unclosed quote");
    }
    if in_word {
        out.push(cur);
    }
    Ok(out)
}

/// `sq_dequote` do git: uma palavra entre aspas simples, com `'\''` e `'\!'` como escapes.
fn sq_dequote(s: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut i = 0;
    loop {
        if s.get(i) != Some(&b'\'') {
            return None;
        }
        i += 1;
        loop {
            match s.get(i) {
                None => return None,
                Some(b'\'') => break,
                Some(c) => out.push(*c),
            }
            i += 1;
        }
        i += 1;
        match s.get(i) {
            None => return Some(out),
            Some(b'\\') => {
                let esc = s.get(i + 1).copied();
                if matches!(esc, Some(b'\'') | Some(b'!')) && s.get(i + 2) == Some(&b'\'') {
                    out.push(esc.unwrap_or(b'\''));
                    i += 2;
                } else {
                    return None;
                }
            }
            Some(_) => return None,
        }
    }
}

/// `do_generic_cmd`: roda `git <sub> <diretório>`.
fn do_generic_cmd(me: &str, arg: Option<&[u8]>) -> i32 {
    let dir = match arg.and_then(sq_dequote) {
        Some(d) if d.first() != Some(&b'-') => d,
        _ => return die("bad argument"),
    };
    let Some(sub) = me.strip_prefix("git-") else {
        return die("bad command");
    };
    cmd::main(&[b"git".to_vec(), sub.as_bytes().to_vec(), dir])
}

/// `do_cvs_cmd`: só `cvs server`.
fn do_cvs_cmd(_me: &str, arg: Option<&[u8]>) -> i32 {
    let Some(arg) = arg else {
        return die("no argument given to cvs");
    };
    if arg != b"server" {
        return die(&format!("git-cvsserver only handles server: {}", os::lossy(arg)));
    }
    cmd::main(&[b"git".to_vec(), b"cvsserver".to_vec(), b"server".to_vec()])
}

type Exec = fn(&str, Option<&[u8]>) -> i32;

const CMD_LIST: &[(&str, Exec)] = &[
    ("git-receive-pack", do_generic_cmd),
    ("git-upload-pack", do_generic_cmd),
    ("git-upload-archive", do_generic_cmd),
    ("cvs", do_cvs_cmd),
];

/// Roda um programa de `git-shell-commands` e espera. `None` quando ele não existe.
fn run_command(argv: &[Vec<u8>]) -> Option<i32> {
    if !os::exists(&argv[0]) {
        return None;
    }
    let s = sys::current();
    os::flush_out();
    let cmdv = argv.to_vec();
    let body: sysabi::ProcessFn = Box::new(move || {
        let e = sys::current().execve(&cmdv[0], &cmdv, None);
        let _ = e;
        127
    });
    let child = match s.spawn_fn(ProcAttrs::default(), argv[0].clone(), body) {
        Ok(p) => p,
        Err(_) => return Some(-1),
    };
    loop {
        match s.wait4(WaitTarget::Any, WaitOptions::empty()) {
            Err(Errno::EINTR) => continue,
            Ok(Some((p, st))) if p == child => {
                return Some(match st {
                    WaitStatus::Exited(code) => code & 0xff,
                    WaitStatus::Signaled { signal, .. } => 128 + signal.0,
                    _ => 1,
                });
            }
            _ => return Some(-1),
        }
    }
}

/// `run_shell`: o laço interativo (`git> `).
fn run_shell() -> i32 {
    if os::exists(NOLOGIN_COMMAND) {
        return run_command(&[NOLOGIN_COMMAND.to_vec()]).unwrap_or(127);
    }
    // Ajuda, quando existe (silencioso se não existe).
    let _ = run_command(&[HELP_COMMAND.to_vec()]);

    let input = os::stdin_all();
    let mut lines = input.split(|b| *b == b'\n').peekable();
    loop {
        os::flush_out();
        os::errs("git> ");
        let Some(raw) = lines.next() else {
            os::errs("\n");
            break;
        };
        // O último pedaço vazio depois do `\n` final é o EOF.
        if raw.is_empty() && lines.peek().is_none() {
            os::errs("\n");
            break;
        }
        let line = raw.strip_suffix(b"\r").unwrap_or(raw);
        let argv = match split_cmdline(line) {
            Ok(a) => a,
            Err(m) => {
                os::errs(&format!("invalid command format '{}': {}\n", os::lossy(line), m));
                continue;
            }
        };
        let Some(prog) = argv.first() else { continue };
        if matches!(prog.as_slice(), b"quit" | b"logout" | b"exit" | b"bye") {
            break;
        }
        if is_valid_cmd_name(prog) {
            let mut full = argv.clone();
            full[0] = os::join(COMMAND_DIR, prog);
            if run_command(&full).is_none() {
                os::errs(&format!("unrecognized command '{}'\n", os::lossy(prog)));
            }
        } else {
            os::errs(&format!("invalid command format '{}'\n", os::lossy(prog)));
        }
    }
    0
}

fn run(argv: &[Vec<u8>]) -> i32 {
    // O truque pra se passar por servidor CVS: `git-shell "cvs server"`.
    let (prog_arg, ok_shape): (Option<Vec<u8>>, bool) = if argv.len() == 2 && argv[1] == b"cvs server" {
        (Some(argv[1].clone()), true)
    } else if argv.len() == 1 {
        if let Err(c) = cd_to_homedir() {
            return c;
        }
        let ok = os::stat(COMMAND_DIR).is_ok_and(|s| s.mode & 0o500 != 0)
            && os::read_dir(COMMAND_DIR).is_ok();
        if !ok {
            return die(
                "Interactive git shell is not enabled.\nhint: ~/git-shell-commands should exist and have read and execute access.",
            );
        }
        return run_shell();
    } else if argv.len() != 3 || argv[1] != b"-c" {
        return die("Run with no arguments or with -c cmd");
    } else {
        (Some(argv[2].clone()), true)
    };
    let _ = ok_shape;
    let mut prog = prog_arg.unwrap_or_default();
    // Aceita "git foo" como se fosse "git-foo".
    if prog.starts_with(b"git") && prog.get(3).is_some_and(|c| matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')) {
        prog[3] = b'-';
    }

    for (name, exec) in CMD_LIST {
        let nb = name.as_bytes();
        if !prog.starts_with(nb) {
            continue;
        }
        match prog.get(nb.len()) {
            None => return exec(name, None),
            Some(b' ') => return exec(name, Some(&prog[nb.len() + 1..])),
            Some(_) => continue,
        }
    }

    if let Err(c) = cd_to_homedir() {
        return c;
    }
    let orig = if argv.len() == 2 { argv[1].clone() } else { argv[2].clone() };
    match split_cmdline(&prog) {
        Ok(mut user_argv) => {
            if let Some(first) = user_argv.first().cloned()
                && is_valid_cmd_name(&first)
            {
                user_argv[0] = os::join(COMMAND_DIR, &first);
                os::flush_out();
                let _ = sys::current().execve(&user_argv[0], &user_argv, None);
            }
            die(&format!("unrecognized command '{}'", os::lossy(&orig)))
        }
        Err(m) => die(&format!("invalid command format '{}': {}", os::lossy(&orig), m)),
    }
}
