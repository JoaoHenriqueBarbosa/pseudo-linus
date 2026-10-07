//! `taskset` do util-linux 2.41: lê e muda a afinidade de CPU de um processo.
//!
//! Porte do `schedutils/taskset.c`. Cobre `-p`, `-c` e `-a`: sem máscara imprime a afinidade atual
//! (`pid N's current affinity mask: f` ou `list: 0-3` com `-c`); com máscara imprime a de antes e a
//! de depois. O modo de execução (`taskset MASK comando`) não está portado, porque o `sysabi` não
//! expõe um `execve` pra este programa; ele sai com erro. O `-a` age só sobre o próprio pid, já que
//! a lista de threads não passa pelo `sysabi`.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Errno, Pid, sys};
use ul_common::ctype::parse_decimal_usize as parse_num;

use crate::util::io;
use crate::util::ul;

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn usage(short: &str) -> String {
    format!(
        "Usage: {short} [options] [mask | cpu-list] [pid|cmd [args...]]


Show or change the CPU affinity of a process.

Options:
 -a, --all-tasks         operate on all the tasks (threads) for a given pid
 -p, --pid               operate on existing given pid
 -c, --cpu-list          display and specify cpus in list format
 -h, --help              display this help
 -V, --version           display version

The default behavior is to run a new command:
    {short} 03 sshd -b 1024
You can retrieve the mask of an existing task:
    {short} -p 700
Or set it:
    {short} -p 03 700
List format uses a comma-separated list instead of a mask:
    {short} -pc 0,3,7-11 700
Ranges in list format can take a stride argument:
    e.g. 0-31:2 is equivalent to mask 0x55555555

For more details see taskset(1).
"
    )
}

/// Máscara em hexadecimal como o `cpumask_create`: nibbles do mais alto pro mais baixo, sem zeros à
/// esquerda (e `0` se não há CPU).
fn mask_string(cpus: &[usize]) -> String {
    let Some(max) = cpus.iter().max().copied() else {
        return "0".to_string();
    };
    let mut nibbles = vec![0u8; max / 4 + 1];
    for c in cpus {
        nibbles[c / 4] |= 1 << (c % 4);
    }
    nibbles.iter().rev().map(|n| format!("{n:x}")).collect()
}

/// Lista como o `cpulist_create`: `0-3,5`, faixas só a partir de três CPUs seguidas pelo `-`; duas
/// seguidas saem como `0,1`.
fn list_string(cpus: &[usize]) -> String {
    let mut sorted = cpus.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let mut parts: Vec<String> = Vec::new();
    let mut i = 0;
    while i < sorted.len() {
        let mut j = i;
        while j + 1 < sorted.len() && sorted[j + 1] == sorted[j] + 1 {
            j += 1;
        }
        let (a, b) = (sorted[i], sorted[j]);
        if a == b {
            parts.push(a.to_string());
        } else if b == a + 1 {
            parts.push(format!("{a},{b}"));
        } else {
            parts.push(format!("{a}-{b}"));
        }
        i = j + 1;
    }
    parts.join(",")
}

fn parse_mask(s: &[u8]) -> Option<Vec<usize>> {
    let s = s.strip_prefix(b"0x").or_else(|| s.strip_prefix(b"0X")).unwrap_or(s);
    if s.is_empty() {
        return None;
    }
    let mut cpus = Vec::new();
    for (i, ch) in s.iter().rev().enumerate() {
        let d = (*ch as char).to_digit(16)?;
        for b in 0..4 {
            if d & (1 << b) != 0 {
                cpus.push(i * 4 + b);
            }
        }
    }
    Some(cpus)
}

/// Lista `0,2-5,8-31:2` (o `cpulist_parse`).
fn parse_list(s: &[u8]) -> Option<Vec<usize>> {
    if s.is_empty() {
        return None;
    }
    let mut cpus = Vec::new();
    for item in s.split(|b| *b == b',') {
        let (range, stride) = match item.iter().position(|b| *b == b':') {
            Some(p) => (&item[..p], parse_num(&item[p + 1..])?),
            None => (item, 1),
        };
        if stride == 0 {
            return None;
        }
        let (a, b) = match range.iter().position(|b| *b == b'-') {
            Some(p) => (parse_num(&range[..p])?, parse_num(&range[p + 1..])?),
            None => {
                let v = parse_num(range)?;
                (v, v)
            }
        };
        if a > b || b >= 1024 {
            return None;
        }
        let mut c = a;
        while c <= b {
            cpus.push(c);
            c += stride;
        }
    }
    Some(cpus)
}

fn show(short: &str, pid: i64, label: &str, cpus: &[usize], list: bool) {
    let _ = short;
    let body = if list { list_string(cpus) } else { mask_string(cpus) };
    let kind = if list { "list" } else { "mask" };
    let mut out = io::stdout();
    let _ = out.write_all(format!("pid {pid}'s {label} affinity {kind}: {body}\n").as_bytes());
}

fn to_pid(pid: i64) -> Result<Pid, Errno> {
    let me = sys::current().getpid();
    if pid == 0 { Ok(me) } else { Pid::try_from(pid).map_err(|_| Errno::ESRCH) }
}

fn get_affinity(pid: i64) -> Result<Vec<usize>, Errno> {
    sys::current().sched_getaffinity_of(to_pid(pid)?)
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let short = ul::short_name(args);

    let mut pid_mode = false;
    let mut list = false;
    let mut idx = 1;
    while idx < argv.len() {
        let a = argv[idx].as_slice();
        if a == b"--" {
            idx += 1;
            break;
        }
        if a.len() < 2 || a[0] != b'-' {
            break;
        }
        match a {
            b"--help" => {
                let _ = io::stdout().write_all(usage(&short).as_bytes());
                return 0;
            }
            b"--version" => {
                ul::print_version(&short);
                return 0;
            }
            b"--pid" => pid_mode = true,
            b"--cpu-list" => list = true,
            b"--all-tasks" => {}
            _ if a.starts_with(b"--") => {
                ul::warnx(&short, format!("unrecognized option '{}'", io::lossy(a)));
                ul::errtryhelp(&short);
                return 1;
            }
            _ => {
                for ch in &a[1..] {
                    match ch {
                        b'p' => pid_mode = true,
                        b'c' => list = true,
                        b'a' => {}
                        b'h' => {
                            let _ = io::stdout().write_all(usage(&short).as_bytes());
                            return 0;
                        }
                        b'V' => {
                            ul::print_version(&short);
                            return 0;
                        }
                        _ => {
                            ul::warnx(&short, format!("invalid option -- '{}'", *ch as char));
                            ul::errtryhelp(&short);
                            return 1;
                        }
                    }
                }
            }
        }
        idx += 1;
    }
    let rest = &argv[idx.min(argv.len())..];

    if rest.is_empty() || (!pid_mode && rest.len() < 2) {
        ul::warnx(&short, "bad usage");
        ul::errtryhelp(&short);
        return 1;
    }
    if !pid_mode {
        ul::warnx(&short, "executing a command is not supported");
        return 1;
    }

    let (mask_arg, pid_arg) = if rest.len() == 1 { (None, &rest[0]) } else { (Some(&rest[0]), &rest[1]) };
    let pid = match std::str::from_utf8(pid_arg).ok().and_then(|s| s.parse::<i64>().ok()) {
        Some(p) if p >= 0 => p,
        _ => {
            ul::warnx(&short, format!("invalid PID argument: '{}'", io::lossy(pid_arg)));
            return 1;
        }
    };

    let parsed = match mask_arg {
        Some(m) => {
            let p = if list { parse_list(m) } else { parse_mask(m) };
            let Some(cpus) = p else {
                let what = if list { "list" } else { "mask" };
                ul::warnx(&short, format!("failed to parse CPU {what}: {}", io::lossy(m)));
                return 1;
            };
            Some(cpus)
        }
        None => None,
    };

    let cur = match get_affinity(pid) {
        Ok(c) => c,
        Err(e) => {
            ul::warn(&short, format!("failed to get pid {pid}'s affinity"), e);
            return 1;
        }
    };
    show(&short, pid, "current", &cur, list);
    let Some(cpus) = parsed else {
        return 0;
    };
    let target = match to_pid(pid) {
        Ok(p) => p,
        Err(e) => {
            ul::warn(&short, format!("failed to set pid {pid}'s affinity"), e);
            return 1;
        }
    };
    if let Err(e) = sys::current().sched_setaffinity(target, &cpus) {
        ul::warn(&short, format!("failed to set pid {pid}'s affinity"), e);
        return 1;
    }
    match get_affinity(pid) {
        Ok(c) => show(&short, pid, "new", &c, list),
        Err(e) => {
            ul::warn(&short, format!("failed to get pid {pid}'s affinity"), e);
            return 1;
        }
    }
    0
}
