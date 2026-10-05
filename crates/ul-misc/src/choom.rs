//! `choom` do util-linux 2.41: mostra e ajusta a pontuação do OOM killer de um processo.
//!
//! Porte do `sys-utils/choom.c`. Lê `/proc/<pid>/oom_score` e `oom_score_adj`; com `-n` grava o novo
//! ajuste (de -1000 a 1000) em `oom_score_adj`, seja de um pid existente (`-p`), seja do próprio
//! processo antes do `execvp` do comando dado.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Errno, OFlags, sys};

use crate::setsid::execvp;
use crate::util::io;
use crate::util::ul;

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn usage(short: &str) -> String {
    format!(
        "
Usage:
 {short} [options] -p pid
 {short} [options] -n number -p pid
 {short} [options] -n number [--] command [args...]]

Display and adjust OOM-killer score.

Options:
 -n, --adjust <num>     specify the adjust score value
 -p, --pid <num>        process ID

 -h, --help             display this help
 -V, --version          display version

For more details see choom(1).
"
    )
}

/// Lê um inteiro de um arquivo do `/proc` (o `fscanf("%d")` do original).
fn read_int(path: &[u8]) -> Result<i32, Errno> {
    let mut f = io::File::open(path)?;
    let data = f.read_to_end_sys()?;
    let text = String::from_utf8_lossy(&data);
    text.trim().parse::<i32>().map_err(|_| Errno::EINVAL)
}

fn proc_path(pid: &str, file: &str) -> Vec<u8> {
    format!("/proc/{pid}/{file}").into_bytes()
}

/// Grava o ajuste em `/proc/<pid>/oom_score_adj`.
fn write_adj(pid: &str, adj: i32) -> Result<(), Errno> {
    let path = proc_path(pid, "oom_score_adj");
    let f = io::File::open_with(&path, OFlags::WRONLY, 0)?;
    sys::write_all(f.fd(), format!("{adj}").as_bytes())
}

/// `strtos32_or_err` restrito ao intervalo aceito pelo kernel.
fn parse_adj(arg: &[u8]) -> Option<i32> {
    let s = std::str::from_utf8(arg).ok()?;
    let v: i64 = s.trim_start().parse().ok()?;
    if (-1000..=1000).contains(&v) { Some(v as i32) } else { None }
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let short = ul::short_name(args);

    let mut pid: Option<String> = None;
    let mut adj: Option<i32> = None;
    let mut i = 1;
    // getopt_long com "+": para no primeiro argumento que não é opção.
    while i < argv.len() {
        let a = argv[i].as_slice();
        if a == b"--" {
            i += 1;
            break;
        }
        if a.len() < 2 || a[0] != b'-' {
            break;
        }
        let (name, inline): (Vec<u8>, Option<Vec<u8>>) = if a.starts_with(b"--") {
            match a.iter().position(|b| *b == b'=') {
                Some(p) => (a[2..p].to_vec(), Some(a[p + 1..].to_vec())),
                None => (a[2..].to_vec(), None),
            }
        } else if a.len() > 2 {
            (vec![a[1]], Some(a[2..].to_vec()))
        } else {
            (vec![a[1]], None)
        };
        let key: u8 = match name.as_slice() {
            b"n" | b"adjust" => b'n',
            b"p" | b"pid" => b'p',
            b"h" | b"help" => b'h',
            b"V" | b"version" => b'V',
            _ => {
                if a.starts_with(b"--") {
                    ul::warnx(&short, format!("unrecognized option '{}'", io::lossy(a)));
                } else {
                    ul::warnx(&short, format!("invalid option -- '{}'", io::lossy(&name)));
                }
                ul::errtryhelp(&short);
                return 1;
            }
        };
        match key {
            b'h' => {
                let mut out = io::stdout();
                let _ = out.write_all(usage(&short).as_bytes());
                return 0;
            }
            b'V' => {
                ul::print_version(&short);
                return 0;
            }
            _ => {}
        }
        // -n e -p pedem argumento.
        let val = match inline {
            Some(v) => v,
            None => {
                i += 1;
                match argv.get(i) {
                    Some(v) => v.clone(),
                    None => {
                        ul::warnx(
                            &short,
                            format!("option requires an argument -- '{}'", key as char),
                        );
                        ul::errtryhelp(&short);
                        return 1;
                    }
                }
            }
        };
        if key == b'n' {
            match parse_adj(&val) {
                Some(v) => adj = Some(v),
                None => {
                    ul::warnx(
                        &short,
                        format!("failed to parse OOM score adjust value: '{}'", io::lossy(&val)),
                    );
                    return 1;
                }
            }
        } else {
            match std::str::from_utf8(&val).ok().and_then(|s| s.trim().parse::<i32>().ok()) {
                Some(p) if p >= 0 => pid = Some(p.to_string()),
                _ => {
                    ul::warnx(
                        &short,
                        format!("failed to parse PID: '{}'", io::lossy(&val)),
                    );
                    return 1;
                }
            }
        }
        i += 1;
    }
    let cmd: &[Vec<u8>] = &argv[i.min(argv.len())..];

    if (pid.is_some() && !cmd.is_empty()) || (pid.is_none() && adj.is_none()) || (pid.is_none() && cmd.is_empty())
    {
        ul::errtryhelp(&short);
        return 1;
    }
    if adj.is_none() && pid.is_none() {
        ul::errtryhelp(&short);
        return 1;
    }

    if let Some(p) = &pid {
        let score = match read_int(&proc_path(p, "oom_score")) {
            Ok(v) => v,
            Err(e) => {
                ul::warn(&short, format!("failed to read OOM score: /proc/{p}/oom_score"), e);
                return 1;
            }
        };
        let cur = match read_int(&proc_path(p, "oom_score_adj")) {
            Ok(v) => v,
            Err(e) => {
                ul::warn(
                    &short,
                    format!("failed to read OOM score adjust value: /proc/{p}/oom_score_adj"),
                    e,
                );
                return 1;
            }
        };
        let mut out = io::stdout();
        let _ = out.write_all(
            format!(
                "pid {p}'s current OOM score: {score}\npid {p}'s current OOM score adjust value: {cur}\n"
            )
            .as_bytes(),
        );
    }

    if let Some(a) = adj {
        let target = pid.clone().unwrap_or_else(|| "self".to_string());
        if let Err(e) = write_adj(&target, a) {
            ul::warn(
                &short,
                format!("failed to set score adjust value: /proc/{target}/oom_score_adj"),
                e,
            );
            return 1;
        }
        if let Some(p) = &pid {
            let score = read_int(&proc_path(p, "oom_score")).unwrap_or(0);
            let now = read_int(&proc_path(p, "oom_score_adj")).unwrap_or(a);
            let mut out = io::stdout();
            let _ = out.write_all(
                format!(
                    "pid {p}'s new OOM score: {score}\npid {p}'s new OOM score adjust value: {now}\n"
                )
                .as_bytes(),
            );
        }
    }

    if pid.is_none() {
        let _ = io::flush_stdout();
        let e = execvp(&cmd[0], cmd);
        ul::warn(&short, format!("failed to execute {}", io::lossy(&cmd[0])), e);
        return if e == Errno::ENOENT { 127 } else { 126 };
    }
    0
}
