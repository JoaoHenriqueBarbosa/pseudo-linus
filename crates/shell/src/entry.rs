//! Pontos de entrada dos programas `bash` e `sh`: opções de linha de comando e os modos de leitura
//! (`-c`, arquivo, entrada padrão, interativo).

use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::sync::Arc;

use sysabi::{Ctx, Fd};

use crate::exec::TextKind;
use crate::parse::{ParseEnv, parse_text};
use crate::shell::{Flow, Shell, sys, write_fd};

const VERSION: &str = "GNU bash, version 5.2.37(1)-release (x86_64-pc-linux-gnu)\n\
Copyright (C) 2022 Free Software Foundation, Inc.\n\
License GPLv3+: GNU GPL version 3 or later <http://gnu.org/licenses/gpl.html>\n\
\n\
This is free software; you are free to change and redistribute it.\n\
There is NO WARRANTY, to the extent permitted by law.\n";

/// O que a linha de comando pediu.
struct Invocation {
    command: Option<Vec<u8>>,
    script: Option<Vec<u8>>,
    args: Vec<Vec<u8>>,
    arg0: Vec<u8>,
    interactive: bool,
    stdin: bool,
}

fn usage_error(prog: &str, msg: &str) -> i32 {
    let _ = write_fd(Fd::STDERR, format!("{prog}: {msg}\n").as_bytes());
    2
}

fn parse_invocation(sh: &mut Shell, argv: &[Vec<u8>]) -> Result<Invocation, i32> {
    let prog = String::from_utf8_lossy(argv.first().map(|a| a.as_slice()).unwrap_or(b"bash")).into_owned();
    let mut inv = Invocation { command: None, script: None, args: Vec::new(), arg0: argv.first().cloned().unwrap_or_else(|| b"bash".to_vec()), interactive: false, stdin: false };
    let mut want_c = false;
    let mut i = 1;
    while i < argv.len() {
        let a = &argv[i];
        if a == b"--" || a == b"-" {
            i += 1;
            break;
        }
        if a.starts_with(b"--") {
            match a.as_slice() {
                b"--version" => {
                    let _ = write_fd(Fd::STDOUT, VERSION.as_bytes());
                    return Err(0);
                }
                b"--help" => {
                    let _ = write_fd(Fd::STDOUT, format!("GNU bash, version 5.2.37(1)-release-(x86_64-pc-linux-gnu)\nUsage:\t{prog} [GNU long option] [option] ...\n\t{prog} [GNU long option] [option] script-file ...\n").as_bytes());
                    return Err(0);
                }
                b"--posix" => {
                    sh.posix = true;
                    sh.opts.set("posix", true);
                }
                b"--norc" | b"--noprofile" | b"--login" | b"--noediting" | b"--restricted" | b"--verbose" => {
                    if a == b"--verbose" {
                        sh.opts.set("verbose", true);
                    }
                }
                b"--rcfile" | b"--init-file" => {
                    i += 1;
                }
                _ => {
                    return Err(usage_error(&prog, &format!("{}: invalid option", String::from_utf8_lossy(a))));
                }
            }
            i += 1;
            continue;
        }
        if a.len() < 2 || (a[0] != b'-' && a[0] != b'+') {
            break;
        }
        let on = a[0] == b'-';
        let mut j = 1;
        while j < a.len() {
            let c = a[j];
            match c {
                b'c' if on => want_c = true,
                b'i' if on => inv.interactive = true,
                b's' if on => inv.stdin = true,
                b'l' | b'r' | b'D' => {}
                b'o' => {
                    i += 1;
                    match argv.get(i) {
                        Some(name) => {
                            let n = String::from_utf8_lossy(name).into_owned();
                            if n == "posix" {
                                sh.posix = on;
                            }
                            if !sh.opts.set(&n, on) {
                                return Err(usage_error(&prog, &format!("{n}: invalid option name")));
                            }
                        }
                        None => {
                            // `bash -o` sozinho lista as opções.
                        }
                    }
                }
                b'O' => {
                    i += 1;
                    if let Some(name) = argv.get(i) {
                        let n = String::from_utf8_lossy(name).into_owned();
                        if !sh.opts.set_shopt(&n, on) {
                            return Err(usage_error(&prog, &format!("{n}: invalid shell option name")));
                        }
                    }
                }
                other => match crate::options::Options::letter_index(other) {
                    Some(idx) => {
                        sh.opts.set(crate::options::SET_OPTIONS[idx].0, on);
                    }
                    None => {
                        return Err(usage_error(&prog, &format!("-{}: invalid option", other as char)));
                    }
                },
            }
            j += 1;
        }
        i += 1;
    }
    let rest = &argv[i.min(argv.len())..];
    if want_c {
        let Some(cmd) = rest.first() else {
            return Err(usage_error(&prog, "-c: option requires an argument"));
        };
        inv.command = Some(cmd.clone());
        if let Some(name) = rest.get(1) {
            inv.arg0 = name.clone();
            inv.args = rest[2..].to_vec();
        }
    } else if !inv.stdin && !rest.is_empty() {
        inv.script = Some(rest[0].clone());
        inv.arg0 = rest[0].clone();
        inv.args = rest[1..].to_vec();
    } else {
        inv.args = rest.to_vec();
    }
    Ok(inv)
}

fn finish(sh: &mut Shell, r: Result<i32, Flow>) -> i32 {
    let status = match r {
        Ok(st) => st,
        Err(Flow::Exit(n)) => n,
        Err(Flow::Return(n)) => n,
        Err(Flow::Discard) => 1,
        Err(Flow::Break(_)) | Err(Flow::Continue(_)) => sh.status,
    };
    sh.exit_shell(status)
}

/// Corpo comum de `bash` e `sh`.
fn shell_main(args: &[OsString], posix: bool) -> i32 {
    let argv: Vec<Vec<u8>> = args.iter().map(|a| a.as_bytes().to_vec()).collect();
    let mut sh = Shell::new();
    sh.init_from_process();
    if posix {
        sh.posix = true;
        sh.invoked_as_sh = true;
        sh.opts.set("posix", true);
    }
    let inv = match parse_invocation(&mut sh, &argv) {
        Ok(i) => i,
        Err(code) => return code,
    };
    sh.arg0 = inv.arg0.clone();
    sh.params = inv.args.clone();
    if let Some(cmd) = inv.command {
        sh.dash_c = true;
        let text = String::from_utf8_lossy(&cmd).into_owned();
        let v = sh.vars.global_entry("BASH_EXECUTION_STRING");
        v.value = crate::vars::Value::Scalar(cmd.clone());
        let r = sh.run_text(&text, TextKind::Main, Arc::from("-c"), 1);
        return finish(&mut sh, r);
    }
    if let Some(script) = inv.script {
        let shown = String::from_utf8_lossy(&script).into_owned();
        let path = if script.contains(&b'/') {
            script.clone()
        } else if sys().fstatat(Fd::CWD, &script, sysabi::AtFlags::empty()).is_ok() {
            script.clone()
        } else {
            sh.search_path(&script).unwrap_or(script.clone())
        };
        let data = match sysabi::sys::read_file(&path) {
            Ok(d) => d,
            Err(e) => {
                let prog = String::from_utf8_lossy(&argv[0]).into_owned();
                let _ = write_fd(Fd::STDERR, format!("{prog}: {shown}: {}\n", e.message()).as_bytes());
                return if e == sysabi::Errno::ENOENT { 127 } else { 126 };
            }
        };
        if data.iter().take(80).any(|c| *c == 0) {
            let prog = String::from_utf8_lossy(&argv[0]).into_owned();
            let _ = write_fd(Fd::STDERR, format!("{prog}: {shown}: cannot execute binary file\n").as_bytes());
            return 126;
        }
        let text = String::from_utf8_lossy(&data).into_owned();
        let name: Arc<str> = Arc::from(shown.as_str());
        sh.script_file = true;
        sh.source_stack.push(name.clone());
        let r = sh.run_text(&text, TextKind::Main, name, 1);
        return finish(&mut sh, r);
    }
    let interactive = inv.interactive || (sys().isatty(Fd::STDIN) && sys().isatty(Fd::STDERR));
    if interactive {
        sh.interactive = true;
        sh.opts.set("histexpand", false);
        let r = run_interactive(&mut sh);
        return finish(&mut sh, r);
    }
    let r = run_stdin(&mut sh);
    finish(&mut sh, r)
}

/// Lê uma linha (com o newline) do stdin sem consumir além dela.
fn read_raw_line(sh: &mut Shell) -> Option<Vec<u8>> {
    let mut line = crate::builtins::read::read_line(sh, Fd::STDIN, b'\n')?;
    line.push(b'\n');
    Some(line)
}

/// O erro indica que falta texto (o comando continua na próxima linha)?
fn incomplete(msg: &str) -> bool {
    msg.contains("unexpected end of file") || msg.contains("unexpected EOF")
}

/// Script pela entrada padrão: lê linha a linha e executa cada comando completo assim que ele
/// fecha (um `read` no script lê a linha seguinte do mesmo stdin, como no bash).
fn run_stdin(sh: &mut Shell) -> Result<i32, Flow> {
    let mut buf = String::new();
    let mut start_line: u32 = 1;
    let mut status = 0;
    let name: Arc<str> = Arc::from(String::from_utf8_lossy(&sh.arg0).as_ref());
    sh.input_name = name.clone();
    loop {
        let line = read_raw_line(sh);
        let at_eof = line.is_none();
        if let Some(l) = &line {
            buf.push_str(&String::from_utf8_lossy(l));
        }
        if buf.trim().is_empty() {
            if at_eof {
                return Ok(status);
            }
            start_line += buf.matches('\n').count() as u32;
            buf.clear();
            continue;
        }
        let env: ParseEnv = sh.parse_env();
        match parse_text(&buf, start_line - 1, &Arc::from(""), &env) {
            Ok(parsed) if !at_eof && !parsed.heredoc_eof.is_empty() => {
                // Here-doc ainda sem o delimitador: lê mais.
                continue;
            }
            Ok(_) => {
                let text = std::mem::take(&mut buf);
                let lines = text.matches('\n').count() as u32;
                status = sh.run_text(&text, TextKind::Main, name.clone(), start_line)?;
                start_line += lines;
            }
            Err(e) if incomplete(&e.message) && !at_eof => continue,
            Err(_) => {
                let text = std::mem::take(&mut buf);
                status = sh.run_text(&text, TextKind::Main, name.clone(), start_line)?;
                start_line += text.matches('\n').count() as u32;
            }
        }
        if at_eof {
            return Ok(status);
        }
    }
}

/// Modo interativo básico: PS1/PS2 no stderr, uma linha por vez.
fn run_interactive(sh: &mut Shell) -> Result<i32, Flow> {
    let mut buf = String::new();
    let mut status = 0;
    let name: Arc<str> = Arc::from(String::from_utf8_lossy(&sh.arg0).as_ref());
    loop {
        let ps = if buf.is_empty() { "PS1" } else { "PS2" };
        let raw = sh.var_bytes(ps).map(|v| v.to_vec()).unwrap_or_else(|| if ps == "PS1" { b"\\s-\\v\\$ ".to_vec() } else { b"> ".to_vec() });
        let prompt = sh.prompt_expand(&raw);
        let _ = write_fd(Fd::STDERR, &prompt);
        let Some(line) = read_raw_line(sh) else {
            let _ = write_fd(Fd::STDERR, b"exit\n");
            return Ok(status);
        };
        buf.push_str(&String::from_utf8_lossy(&line));
        let env = sh.parse_env();
        match parse_text(&buf, 0, &Arc::from(""), &env) {
            Ok(p) if !p.heredoc_eof.is_empty() => continue,
            Err(e) if incomplete(&e.message) => continue,
            _ => {}
        }
        let text = std::mem::take(&mut buf);
        match sh.run_text(&text, TextKind::Main, name.clone(), 1) {
            Ok(st) => status = st,
            Err(Flow::Exit(n)) => return Err(Flow::Exit(n)),
            Err(_) => status = sh.status,
        }
        let _ = sh.run_pending_traps();
    }
}

pub fn bash_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    shell_main(args, false)
}

pub fn sh_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    shell_main(args, true)
}
