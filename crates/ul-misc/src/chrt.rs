//! `chrt` do util-linux 2.41: mostra ou muda os atributos de escalonamento de um processo, ou roda um
//! comando com eles.
//!
//! Porte do `schedutils/chrt.c`. Usa as syscalls de escalonamento do `sysabi` (`sched_setscheduler`,
//! `sched_getscheduler`, `sched_getparam`, `sched_setattr` pra `SCHED_DEADLINE`). `-a` age só sobre o
//! pid dado, porque o sandbox não expõe as threads por `/proc/<pid>/task` a este programa.

use std::ffi::OsString;
use std::io::Write;

use sysabi::sched::{self, SchedAttr, SchedParam};
use sysabi::{Errno, Pid, sys};

use crate::util::io;
use crate::util::ul;

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn usage(short: &str) -> String {
    format!(
        "Show or change the real-time scheduling attributes of a process.

Set policy:
 {short} [options] <priority> <command> [<arg>...]
 {short} [options] --pid <priority> <pid>

Get policy:
 {short} [options] -p <pid>

Policy options:
 -b, --batch          set policy to SCHED_BATCH
 -d, --deadline       set policy to SCHED_DEADLINE
 -f, --fifo           set policy to SCHED_FIFO
 -i, --idle           set policy to SCHED_IDLE
 -o, --other          set policy to SCHED_OTHER
 -r, --rr             set policy to SCHED_RR (default)

Scheduling options:
 -R, --reset-on-fork       set reset-on-fork flag
 -T, --sched-runtime <ns>  runtime parameter for DEADLINE
 -P, --sched-period <ns>   period parameter for DEADLINE
 -D, --sched-deadline <ns> deadline parameter for DEADLINE

Other options:
 -a, --all-tasks      operate on all the tasks (threads) for a given pid
 -m, --max            show min and max valid priorities
 -p, --pid            operate on existing given pid
 -v, --verbose        display status information

 -h, --help           display this help
 -V, --version        display version

For more details see chrt(1).
"
    )
}

fn policy_name(policy: i32) -> &'static str {
    match policy {
        sched::SCHED_OTHER => "SCHED_OTHER",
        sched::SCHED_FIFO => "SCHED_FIFO",
        sched::SCHED_RR => "SCHED_RR",
        sched::SCHED_BATCH => "SCHED_BATCH",
        sched::SCHED_IDLE => "SCHED_IDLE",
        sched::SCHED_DEADLINE => "SCHED_DEADLINE",
        _ => "unknown",
    }
}

/// `strtos32_or_err`: decimal, hexadecimal (`0x`) ou octal (`0`) com sinal, como o `strtol` base 0.
fn parse_i32(s: &[u8]) -> Result<i32, Option<Errno>> {
    let mut i = 0;
    while i < s.len() && ul::is_space(s[i]) {
        i += 1;
    }
    let mut neg = false;
    if i < s.len() && (s[i] == b'-' || s[i] == b'+') {
        neg = s[i] == b'-';
        i += 1;
    }
    let mut base: i128 = 10;
    if s[i..].starts_with(b"0x") || s[i..].starts_with(b"0X") {
        base = 16;
        i += 2;
    } else if s[i..].starts_with(b"0") && s.len() > i + 1 {
        base = 8;
        i += 1;
    }
    let start = i;
    let mut v: i128 = 0;
    while i < s.len() {
        let d = match s[i] {
            c @ b'0'..=b'9' => i128::from(c - b'0'),
            c @ b'a'..=b'f' => i128::from(c - b'a' + 10),
            c @ b'A'..=b'F' => i128::from(c - b'A' + 10),
            _ => break,
        };
        if d >= base {
            break;
        }
        v = (v * base + d).min(1 << 70);
        i += 1;
    }
    if (i == start && base != 8) || i != s.len() || s.is_empty() {
        return Err(None);
    }
    let v = if neg { -v } else { v };
    i32::try_from(v).map_err(|_| Some(Errno::ERANGE))
}

fn parse_u64(s: &[u8]) -> Result<u64, Option<Errno>> {
    let t = std::str::from_utf8(s).map_err(|_| None)?;
    let t = t.trim_start();
    let r = if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        u64::from_str_radix(h, 16)
    } else {
        t.parse::<u64>()
    };
    r.map_err(|e| if matches!(e.kind(), std::num::IntErrorKind::PosOverflow) { Some(Errno::ERANGE) } else { None })
}

fn bad_number(short: &str, what: &str, s: &[u8], e: Option<Errno>) -> i32 {
    let msg = format!("{what}: '{}'", io::lossy(s));
    match e {
        Some(e) => ul::warn(short, msg, e),
        None => ul::warnx(short, msg),
    }
    1
}

fn show_min_max(out: &mut impl Write) {
    let sys = sys::current();
    for p in [
        sched::SCHED_OTHER,
        sched::SCHED_FIFO,
        sched::SCHED_RR,
        sched::SCHED_BATCH,
        sched::SCHED_IDLE,
        sched::SCHED_DEADLINE,
    ] {
        let min = sys.sched_get_priority_min(p);
        let max = sys.sched_get_priority_max(p);
        match (min, max) {
            (Ok(a), Ok(b)) => {
                let _ = out.write_all(format!("{} min/max priority\t: {a}/{b}\n", policy_name(p)).as_bytes());
            }
            _ => {
                let _ = out.write_all(format!("{} not supported?\n", policy_name(p)).as_bytes());
            }
        }
    }
}

fn show_sched_info(short: &str, pid: Pid, verbose_old: Option<&str>) -> i32 {
    let sys = sys::current();
    let mut out = io::stdout();
    let pol = match sys.sched_getscheduler(pid) {
        Ok(p) => p,
        Err(e) => {
            ul::warn(short, format!("failed to get pid {pid}'s policy"), e);
            return 1;
        }
    };
    let word = verbose_old.unwrap_or("current");
    let mut name = policy_name(pol & !sched::SCHED_RESET_ON_FORK).to_string();
    if pol & sched::SCHED_RESET_ON_FORK != 0 {
        name.push_str("|SCHED_RESET_ON_FORK");
    }
    let _ = out.write_all(format!("pid {pid}'s {word} scheduling policy: {name}\n").as_bytes());
    let param = match sys.sched_getparam(pid) {
        Ok(p) => p,
        Err(e) => {
            ul::warn(short, format!("failed to get pid {pid}'s attributes"), e);
            return 1;
        }
    };
    let _ = out.write_all(format!("pid {pid}'s {word} scheduling priority: {}\n", param.priority).as_bytes());
    let base = pol & !sched::SCHED_RESET_ON_FORK;
    if base == sched::SCHED_DEADLINE {
        if let Ok(a) = sys.sched_getattr(pid, sched::SCHED_ATTR_SIZE_VER1, 0) {
            let _ = out.write_all(format!("pid {pid}'s {word} runtime/deadline/period parameters: {}/{}/{}\n", a.runtime, a.deadline, a.period).as_bytes());
        }
    } else if base == sched::SCHED_OTHER || base == sched::SCHED_BATCH {
        if let Ok(a) = sys.sched_getattr(pid, sched::SCHED_ATTR_SIZE_VER1, 0) {
            let _ = out.write_all(format!("pid {pid}'s {word} runtime parameter: {}\n", a.runtime).as_bytes());
        }
    }
    0
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let short = ul::short_name(args);
    let mut out = io::stdout();

    let mut policy = sched::SCHED_RR;
    let mut reset_on_fork = false;
    let (mut runtime, mut period, mut deadline) = (0u64, 0u64, 0u64);
    let (mut verbose, mut max, mut pid_mode) = (false, false, false);

    let rest = &argv[1..];
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
        // Opções com argumento: devolve o valor, vindo junto ou do próximo argv.
        let take_arg = |inline: Option<&[u8]>, idx: &mut usize, label: &str| -> Result<Vec<u8>, ()> {
            if let Some(v) = inline {
                return Ok(v.to_vec());
            }
            if *idx < rest.len() {
                *idx += 1;
                return Ok(rest[*idx - 1].clone());
            }
            ul::warnx(&short, format!("option requires an argument -- '{label}'"));
            ul::errtryhelp(&short);
            Err(())
        };
        if a.starts_with(b"--") {
            let body = &a[2..];
            let (name, inline) = match body.iter().position(|b| *b == b'=') {
                Some(p) => (&body[..p], Some(&body[p + 1..])),
                None => (body, None),
            };
            match name {
                b"help" => {
                    let _ = out.write_all(usage(&short).as_bytes());
                    return 0;
                }
                b"version" => {
                    ul::print_version(&short);
                    return 0;
                }
                b"batch" => policy = sched::SCHED_BATCH,
                b"deadline" => policy = sched::SCHED_DEADLINE,
                b"fifo" => policy = sched::SCHED_FIFO,
                b"idle" => policy = sched::SCHED_IDLE,
                b"other" => policy = sched::SCHED_OTHER,
                b"rr" => policy = sched::SCHED_RR,
                b"reset-on-fork" => reset_on_fork = true,
                b"all-tasks" => {}
                b"max" => max = true,
                b"pid" => pid_mode = true,
                b"verbose" => verbose = true,
                b"sched-runtime" | b"sched-period" | b"sched-deadline" => {
                    let Ok(v) = take_arg(inline, &mut idx, &io::lossy(name)) else { return 1 };
                    let n = match parse_u64(&v) {
                        Ok(n) => n,
                        Err(e) => return bad_number(&short, "invalid runtime argument", &v, e),
                    };
                    match name {
                        b"sched-runtime" => runtime = n,
                        b"sched-period" => period = n,
                        _ => deadline = n,
                    }
                }
                _ => {
                    ul::warnx(&short, format!("unrecognized option '--{}'", io::lossy(name)));
                    ul::errtryhelp(&short);
                    return 1;
                }
            }
            continue;
        }
        let mut j = 1;
        while j < a.len() {
            let c = a[j];
            j += 1;
            match c {
                b'h' => {
                    let _ = out.write_all(usage(&short).as_bytes());
                    return 0;
                }
                b'V' => {
                    ul::print_version(&short);
                    return 0;
                }
                b'b' => policy = sched::SCHED_BATCH,
                b'd' => policy = sched::SCHED_DEADLINE,
                b'f' => policy = sched::SCHED_FIFO,
                b'i' => policy = sched::SCHED_IDLE,
                b'o' => policy = sched::SCHED_OTHER,
                b'r' => policy = sched::SCHED_RR,
                b'R' => reset_on_fork = true,
                b'a' => {}
                b'm' => max = true,
                b'p' => pid_mode = true,
                b'v' => verbose = true,
                b'T' | b'P' | b'D' => {
                    let inline = if j < a.len() { Some(&a[j..]) } else { None };
                    j = a.len();
                    let Ok(v) = take_arg(inline, &mut idx, &(c as char).to_string()) else { return 1 };
                    let n = match parse_u64(&v) {
                        Ok(n) => n,
                        Err(e) => return bad_number(&short, "invalid runtime argument", &v, e),
                    };
                    match c {
                        b'T' => runtime = n,
                        b'P' => period = n,
                        _ => deadline = n,
                    }
                }
                _ => {
                    ul::warnx(&short, format!("invalid option -- '{}'", c as char));
                    ul::errtryhelp(&short);
                    return 1;
                }
            }
        }
    }
    let rest = &rest[idx..];

    if max && rest.is_empty() {
        show_min_max(&mut out);
        return 0;
    }

    let needed = if pid_mode { 1 } else { 2 };
    let is_dl = policy == sched::SCHED_DEADLINE;
    if rest.is_empty() || rest.len() < needed {
        ul::warnx(&short, "bad usage");
        ul::errtryhelp(&short);
        return 1;
    }

    let mut pid: Pid = 0;
    if pid_mode {
        let last = &rest[rest.len() - 1];
        pid = match parse_i32(last) {
            Ok(p) => p,
            Err(e) => return bad_number(&short, "invalid PID argument", last, e),
        };
    }

    // `chrt -p <pid>` sem prioridade só mostra.
    if pid_mode && rest.len() == 1 {
        return show_sched_info(&short, pid, None);
    }

    let priority = if is_dl {
        0
    } else {
        match parse_i32(&rest[0]) {
            Ok(p) => p,
            Err(e) => return bad_number(&short, "invalid priority argument", &rest[0], e),
        }
    };
    let cmd = &rest[1..];

    let sys = sys::current();
    if !is_dl {
        let min = sys.sched_get_priority_min(policy).unwrap_or(0);
        let max = sys.sched_get_priority_max(policy).unwrap_or(0);
        if priority < min || priority > max {
            ul::warnx(&short, format!("unsupported priority value for the policy: {priority}: see --max for valid range"));
            return 1;
        }
    }
    if verbose && pid_mode {
        let r = show_sched_info(&short, pid, Some("old"));
        if r != 0 {
            return r;
        }
    }

    let result = if is_dl {
        let mut dl = deadline;
        let mut pe = period;
        if dl == 0 {
            dl = pe;
        }
        if pe == 0 {
            pe = dl;
        }
        let attr = SchedAttr {
            size: sched::SCHED_ATTR_SIZE_VER1,
            policy: policy as u32,
            flags: if reset_on_fork { sched::SCHED_FLAG_RESET_ON_FORK } else { 0 },
            runtime,
            deadline: dl,
            period: pe,
            ..SchedAttr::default()
        };
        sys.sched_setattr(pid, &attr, 0)
    } else {
        let pol = if reset_on_fork { policy | sched::SCHED_RESET_ON_FORK } else { policy };
        sys.sched_setscheduler(pid, pol, SchedParam { priority })
    };
    if let Err(e) = result {
        ul::warn(&short, format!("failed to set pid {pid}'s policy"), e);
        return 1;
    }

    if verbose && pid_mode {
        let r = show_sched_info(&short, pid, Some("new"));
        if r != 0 {
            return r;
        }
    }

    if !pid_mode && !cmd.is_empty() {
        let _ = out.flush();
        let prog = cmd[0].clone();
        let e = if prog.contains(&b'/') {
            sys.execve(&prog, cmd, None)
        } else {
            let path = sys.getenv(b"PATH").unwrap_or_else(|| b"/usr/local/bin:/usr/bin:/bin".to_vec());
            let mut last = Errno::ENOENT;
            for dir in path.split(|b| *b == b':') {
                let mut full = if dir.is_empty() { b".".to_vec() } else { dir.to_vec() };
                full.push(b'/');
                full.extend_from_slice(&prog);
                let e = sys.execve(&full, cmd, None);
                if e == Errno::EACCES {
                    last = e;
                } else if e != Errno::ENOENT && e != Errno::ENOTDIR && last != Errno::EACCES {
                    last = e;
                }
            }
            last
        };
        ul::warn(&short, format!("failed to execute {}", io::lossy(&prog)), e);
        return if e == Errno::ENOENT { 127 } else { 126 };
    }
    0
}
