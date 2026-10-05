//! `killall5` do sysvinit-utils 3.14: envia um sinal a todos os processos fora da própria sessão,
//! exceto o pid 1, o próprio, os threads de kernel e os pids de `-o`.
//!
//! Aceita `-<número>`, `-<NOME>`, `-s <sinal>` e `-o pid[,pid...]`. Como o original, o processo
//! é parado (SIGSTOP) antes do envio e continuado (SIGCONT) depois, para que nada escape.
//! Diferença conhecida: mensagens de erro de `-o` e de `-s` reproduzidas de memória.

use std::ffi::OsString;

use sysabi::{Ctx, KillTarget, Signal, sys};
use ul_misc::util::io;

use crate::procfs;

const USAGE: &str = "Usage: killall5 -signalnumber [-o omitpid[,omitpid...]]\n";

const NAMES: [(&str, i32); 31] = [
    ("HUP", 1), ("INT", 2), ("QUIT", 3), ("ILL", 4), ("TRAP", 5), ("ABRT", 6), ("BUS", 7), ("FPE", 8),
    ("KILL", 9), ("USR1", 10), ("SEGV", 11), ("USR2", 12), ("PIPE", 13), ("ALRM", 14), ("TERM", 15),
    ("STKFLT", 16), ("CHLD", 17), ("CONT", 18), ("STOP", 19), ("TSTP", 20), ("TTIN", 21), ("TTOU", 22),
    ("URG", 23), ("XCPU", 24), ("XFSZ", 25), ("VTALRM", 26), ("PROF", 27), ("WINCH", 28), ("POLL", 29),
    ("PWR", 30), ("SYS", 31),
];

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn parse_signal(s: &[u8]) -> Option<i32> {
    if !s.is_empty() && s.iter().all(u8::is_ascii_digit) {
        return String::from_utf8_lossy(s).parse().ok();
    }
    let bare = s.strip_prefix(b"SIG").unwrap_or(s);
    NAMES.iter().find(|(n, _)| n.as_bytes() == bare).map(|(_, v)| *v)
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    if argv.len() < 2 {
        return 2;
    }
    let mut sig = 15;
    let mut omit: Vec<i32> = Vec::new();
    let mut i = 1;
    while i < argv.len() {
        let a = &argv[i];
        if a == b"-o" {
            i += 1;
            let Some(list) = argv.get(i) else {
                io::eprint(USAGE);
                return 1;
            };
            for p in list.split(|b| *b == b',') {
                match String::from_utf8_lossy(p).parse::<i32>() {
                    Ok(v) => omit.push(v),
                    Err(_) => {
                        io::eprint(format!("killall5: bad pid '{}' for -o\n", String::from_utf8_lossy(p)));
                        return 1;
                    }
                }
            }
        } else if a == b"-s" {
            i += 1;
            match argv.get(i).and_then(|s| parse_signal(s)) {
                Some(v) => sig = v,
                None => {
                    io::eprint(USAGE);
                    return 1;
                }
            }
        } else if a.first() == Some(&b'-') && a.len() > 1 {
            match parse_signal(&a[1..]) {
                Some(v) => sig = v,
                None => {
                    return 1;
                }
            }
        } else {
            io::eprint(USAGE);
            return 1;
        }
        i += 1;
    }
    let sysc = sys::current();
    let me = sysc.getpid();
    let my_sid = procfs::read(&format!("/proc/{me}/stat")).and_then(|d| procfs::parse_stat(&d)).map(|s| i64::from(s.session));
    let entries = match sys::read_dir(b"/proc") {
        Ok(e) => e,
        Err(e) => {
            io::eprint(format!("killall5: cannot read /proc: {}\n", e.message()));
            return 1;
        }
    };
    let mut targets: Vec<i32> = Vec::new();
    for e in &entries {
        if e.name.is_empty() || !e.name.iter().all(u8::is_ascii_digit) {
            continue;
        }
        let Ok(pid) = String::from_utf8_lossy(&e.name).parse::<i32>() else { continue };
        if pid == 1 || pid == me || omit.contains(&pid) {
            continue;
        }
        let Some(st) = procfs::read(&format!("/proc/{pid}/stat")).and_then(|d| procfs::parse_stat(&d)) else { continue };
        if Some(i64::from(st.session)) == my_sid || st.session == 0 {
            continue;
        }
        // Thread de kernel: sem linha de comando.
        if procfs::read(&format!("/proc/{pid}/cmdline")).is_none_or(|c| c.is_empty()) {
            continue;
        }
        targets.push(pid);
    }
    let _ = sysc.kill(KillTarget::Pid(-1), Signal(19));
    for &p in &targets {
        let _ = sysc.kill(KillTarget::Pid(p), Signal(sig));
    }
    let _ = sysc.kill(KillTarget::Pid(-1), Signal(18));
    0
}
