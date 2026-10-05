//! `renice` do util-linux 2.41 (pacote bsdutils do Debian 13): muda a prioridade (nice) de processos
//! em execução.
//!
//! Porte do `sys-utils/renice.c`. A linha de comando não usa `getopt`: o primeiro argumento pode ser
//! `-n`, `--priority` ou `--relative`, depois vem a prioridade, e o resto é uma lista de alvos em que
//! `-p`, `-g` e `-u` trocam o tipo dos que vêm depois. Pra cada alvo lê a prioridade antiga, aplica a
//! nova (absoluta ou somada à antiga) e relê pra imprimir `old priority`/`new priority`.
//!
//! O `sysabi` só tem `getpriority`/`setpriority` por processo; `PRIO_PGRP` sai da lista de processos
//! do sandbox como o `kernel/sys.c` faz (a menor nice do grupo na leitura, todos os membros na
//! escrita, ESRCH sem membro). `PRIO_USER` só alcança o próprio processo, porque a lista de
//! processos não traz o dono.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Errno, Pid, sys};

use crate::util::io;
use crate::util::ul;

#[derive(Copy, Clone, PartialEq, Eq)]
enum Which {
    Process,
    Pgrp,
    User,
}

impl Which {
    fn name(self) -> &'static str {
        match self {
            Which::Process => "process ID",
            Which::Pgrp => "process group ID",
            Which::User => "user ID",
        }
    }
}

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn usage(short: &str) -> String {
    format!(
        "
Usage:
 {short} [-n|--priority|--relative] <priority> [-p|--pid] <pid>...
 {short} [-n|--priority|--relative] <priority>  -g|--pgrp <pgid>...
 {short} [-n|--priority|--relative] <priority>  -u|--user <user>...

Alter the priority of running processes.

Options:
 -n <num>               specify the nice value;
                          if POSIXLY_CORRECT flag is set in environment,
                          then the priority is 'relative' to current
                          process priority; otherwise it is 'absolute'
 --priority <num>       specify the 'absolute' nice value
 --relative <num>       specify the 'relative' nice value
 -p, --pid              interpret arguments as process ID (default)
 -g, --pgrp             interpret arguments as process group ID
 -u, --user             interpret arguments as username or user ID

 -h, --help             display this help
 -V, --version          display version

For more details see renice(1).
"
    )
}

/// `strtol(s, &end, 10)` com o teste `*end` do original: espaço à frente e sinal valem, lixo depois
/// não. Estouro satura como o `strtol` (o valor sai em `LONG_MIN`/`LONG_MAX`).
fn strtol(s: &[u8]) -> Option<i64> {
    let mut i = 0;
    while i < s.len() && ul::is_space(s[i]) {
        i += 1;
    }
    let neg = match s.get(i) {
        Some(b'-') => {
            i += 1;
            true
        }
        Some(b'+') => {
            i += 1;
            false
        }
        _ => false,
    };
    let start = i;
    let mut v: i64 = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        let d = i64::from(s[i] - b'0');
        v = v.saturating_mul(10).saturating_add(d);
        i += 1;
    }
    if i == start {
        // Sem dígito, o strtol não consome nada: `*end` é o primeiro byte (vazio só se a cadeia for).
        return if s.is_empty() { Some(0) } else { None };
    }
    if i != s.len() {
        return None;
    }
    Some(if neg { v.saturating_neg() } else { v })
}

/// Uid de um nome em `/etc/passwd` (o `getpwnam`).
fn getpwnam(name: &[u8]) -> Option<u32> {
    let data = io::File::open(b"/etc/passwd").ok()?.read_to_end_sys().ok()?;
    for line in data.split(|b| *b == b'\n') {
        let mut f = line.split(|b| *b == b':');
        if f.next() != Some(name) {
            continue;
        }
        let _ = f.next();
        let uid = f.next()?;
        return std::str::from_utf8(uid).ok()?.parse().ok();
    }
    None
}

/// Processos alcançados por `(which, who)`, com `who == 0` sendo o próprio grupo ou usuário.
fn targets(which: Which, who: i64) -> Result<Vec<Pid>, Errno> {
    let sys = sys::current();
    let me = sys.getpid();
    match which {
        Which::Process => {
            let pid = if who == 0 { me } else { Pid::try_from(who).map_err(|_| Errno::ESRCH)? };
            Ok(vec![pid])
        }
        Which::Pgrp => {
            let pg = if who == 0 {
                sys.getpgid(me)?
            } else {
                Pid::try_from(who).map_err(|_| Errno::ESRCH)?
            };
            let list: Vec<Pid> = sys
                .list_processes()
                .into_iter()
                .filter(|p| p.pgid == pg && p.state != 'Z')
                .map(|p| p.pid)
                .collect();
            if list.is_empty() { Err(Errno::ESRCH) } else { Ok(list) }
        }
        Which::User => {
            let uid = if who == 0 { i64::from(sys.getuid()) } else { who };
            if uid == i64::from(sys.getuid()) {
                Ok(vec![me])
            } else {
                Err(Errno::ESRCH)
            }
        }
    }
}

fn getprio(which: Which, who: i64) -> Result<i32, Errno> {
    let sys = sys::current();
    let mut best: Option<i32> = None;
    for pid in targets(which, who)? {
        match sys.getpriority(pid) {
            Ok(n) => best = Some(best.map_or(n, |b| b.min(n))),
            Err(e) if which == Which::Process => return Err(e),
            Err(_) => {}
        }
    }
    best.ok_or(Errno::ESRCH)
}

fn setprio(which: Which, who: i64, prio: i64) -> Result<(), Errno> {
    let sys = sys::current();
    // O kernel corta a nice em [-20, 19] antes de aplicar.
    let nice = prio.clamp(-20, 19) as i32;
    let mut err = None;
    for pid in targets(which, who)? {
        if let Err(e) = sys.setpriority(pid, nice) {
            err = Some(e);
        }
    }
    match err {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

fn donice(short: &str, which: Which, who: i64, prio: i64, relative: bool) -> i32 {
    let old = match getprio(which, who) {
        Ok(p) => p,
        Err(e) => {
            ul::warn(short, format!("failed to get priority for {who} ({})", which.name()), e);
            return 1;
        }
    };
    let newprio = if relative { i64::from(old).saturating_add(prio) } else { prio };
    if let Err(e) = setprio(which, who, newprio) {
        ul::warn(short, format!("failed to set priority for {who} ({})", which.name()), e);
        return 1;
    }
    let new = match getprio(which, who) {
        Ok(p) => p,
        Err(e) => {
            ul::warn(short, format!("failed to get priority for {who} ({})", which.name()), e);
            return 1;
        }
    };
    let mut out = io::stdout();
    let _ = out.write_all(
        format!("{who} ({}) old priority {old}, new priority {new}\n", which.name()).as_bytes(),
    );
    0
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let short = ul::short_name(args);
    let sys = sys::current();

    if argv.len() == 2 && (argv[1] == b"-h" || argv[1] == b"--help") {
        let mut out = io::stdout();
        let _ = out.write_all(usage(&short).as_bytes());
        return 0;
    }
    if argv.len() == 2 && (argv[1] == b"-v" || argv[1] == b"-V" || argv[1] == b"--version") {
        ul::print_version(&short);
        return 0;
    }

    let mut rest: &[Vec<u8>] = &argv[1..];
    let mut relative = false;
    if let Some(first) = rest.first() {
        if first == b"-n" {
            relative = sys.getenv(b"POSIXLY_CORRECT").is_some();
            rest = &rest[1..];
        } else if first == b"--priority" {
            rest = &rest[1..];
        } else if first == b"--relative" {
            relative = true;
            rest = &rest[1..];
        }
    }

    if rest.len() < 2 {
        ul::warnx(&short, "not enough arguments");
        ul::errtryhelp(&short);
        return 1;
    }

    let prio = match strtol(&rest[0]) {
        Some(p) => p,
        None => {
            ul::warnx(&short, format!("invalid priority '{}'", io::lossy(&rest[0])));
            ul::errtryhelp(&short);
            return 1;
        }
    };

    let mut which = Which::Process;
    let mut errs = 0;
    for a in &rest[1..] {
        match a.as_slice() {
            b"-g" | b"--pgrp" => {
                which = Which::Pgrp;
                continue;
            }
            b"-u" | b"--user" => {
                which = Which::User;
                continue;
            }
            b"-p" | b"--pid" => {
                which = Which::Process;
                continue;
            }
            _ => {}
        }
        let who = if which == Which::User {
            match getpwnam(a).map(i64::from).or_else(|| strtol(a)) {
                Some(w) if w >= 0 => w,
                _ => {
                    ul::warnx(&short, format!("unknown user {}", io::lossy(a)));
                    errs = 1;
                    continue;
                }
            }
        } else {
            match strtol(a) {
                Some(w) if w >= 0 => w,
                _ => {
                    ul::warnx(&short, format!("bad {} value: {}", which.name(), io::lossy(a)));
                    errs = 1;
                    continue;
                }
            }
        };
        errs |= donice(&short, which, who, prio, relative);
    }
    if errs != 0 { 1 } else { 0 }
}
