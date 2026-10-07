//! `trap`, `kill`, `wait`, `jobs`, `fg`, `bg`, `disown`, `suspend`.

use sysabi::{Errno, Fd, KillTarget, Pid, SigDisposition, Signal, WaitOptions, WaitTarget};

use super::{out, parse_int};
use crate::shell::{Exec, Shell, TRAP_EXIT, Traps, sys, write_fd};

/// Sinal especial do trap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TrapSpec {
    Signal(i32),
    Debug,
    Err,
    Return,
}

fn parse_trap_spec(s: &[u8]) -> Option<TrapSpec> {
    let t = String::from_utf8_lossy(s).to_ascii_uppercase();
    match t.as_str() {
        "EXIT" | "0" | "SIGEXIT" => return Some(TrapSpec::Signal(TRAP_EXIT)),
        "DEBUG" => return Some(TrapSpec::Debug),
        "ERR" => return Some(TrapSpec::Err),
        "RETURN" => return Some(TrapSpec::Return),
        _ => {}
    }
    parse_signal(s).map(|sig| TrapSpec::Signal(sig.0))
}

/// Nome, `SIGnome` ou número (sem caixa), como o bash aceita.
pub use ul_common::signal::parse_shell as parse_signal;

fn signal_display(n: i32) -> String {
    if n == TRAP_EXIT {
        return "EXIT".to_string();
    }
    match Signal(n).name() {
        Some(name) => format!("SIG{name}"),
        None => n.to_string(),
    }
}

fn trap_lines(t: &Traps, only: Option<&[TrapSpec]>) -> String {
    let q = |s: &str| String::from_utf8_lossy(&crate::quote::single_quote(s.as_bytes())).into_owned();
    let mut out = String::new();
    let want = |spec: TrapSpec| only.is_none_or(|o| o.contains(&spec));
    for (n, cmd) in &t.signals {
        if want(TrapSpec::Signal(*n)) {
            out.push_str(&format!("trap -- {} {}\n", q(cmd), signal_display(*n)));
        }
    }
    for (spec, v, name) in [(TrapSpec::Debug, &t.debug, "DEBUG"), (TrapSpec::Err, &t.err, "ERR"), (TrapSpec::Return, &t.ret, "RETURN")] {
        if let Some(cmd) = v
            && want(spec) {
                out.push_str(&format!("trap -- {} {name}\n", q(cmd)));
            }
    }
    out
}

/// Lista `kill -l` / `trap -l`: 5 por linha, `%2d) SIGNOME` separados por tab.
fn signal_table() -> String {
    let mut entries = Vec::new();
    for n in 1..=64 {
        if let Some(name) = Signal(n).name() {
            entries.push(format!("{n:2}) SIG{name}"));
        }
    }
    let mut out = String::new();
    for (i, e) in entries.iter().enumerate() {
        out.push_str(e);
        if (i + 1) % 5 == 0 || i + 1 == entries.len() {
            out.push('\n');
        } else {
            out.push('\t');
        }
    }
    out
}

pub fn trap(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    let mut i = 1;
    let mut print = false;
    let mut list = false;
    while i < argv.len() {
        match argv[i].as_slice() {
            b"-p" => print = true,
            b"-l" => list = true,
            b"--" => {
                i += 1;
                break;
            }
            a if a.starts_with(b"-") && a.len() > 1 => {
                sh.builtin_error("trap", format!("{}: invalid option", String::from_utf8_lossy(&a[..2])));
                let _ = write_fd(Fd::STDERR, b"trap: usage: trap [-lp] [[arg] signal_spec ...]\n");
                return Ok(2);
            }
            _ => break,
        }
        i += 1;
    }
    let rest = &argv[i..];
    if list {
        out(sh, "trap", signal_table().as_bytes());
        return Ok(0);
    }
    if print || rest.is_empty() {
        let shown: Traps = match &sh.traps.inherited_display {
            Some(parent) if trap_lines(&sh.traps, None).is_empty() => (**parent).clone(),
            _ => sh.traps.clone(),
        };
        let mut status = 0;
        let filter: Option<Vec<TrapSpec>> = if rest.is_empty() {
            None
        } else {
            let mut v = Vec::new();
            for s in rest {
                match parse_trap_spec(s) {
                    Some(sp) => v.push(sp),
                    None => {
                        sh.builtin_error("trap", format!("{}: invalid signal specification", String::from_utf8_lossy(s)));
                        status = 1;
                    }
                }
            }
            Some(v)
        };
        let text = trap_lines(&shown, filter.as_deref());
        out(sh, "trap", text.as_bytes());
        return Ok(status);
    }
    // `trap SIG` (um operando que é sinal) e `trap - SIG...`: volta ao padrão.
    let (action, specs): (Option<Vec<u8>>, &[Vec<u8>]) = if rest.len() == 1 || rest[0] == b"-" {
        if rest[0] == b"-" { (None, &rest[1..]) } else { (None, rest) }
    } else if rest[0].iter().all(|c| c.is_ascii_digit()) && parse_trap_spec(&rest[0]).is_some() {
        (None, rest)
    } else {
        (Some(rest[0].clone()), &rest[1..])
    };
    let mut status = 0;
    sh.traps.inherited_display = None;
    for s in specs {
        let Some(spec) = parse_trap_spec(s) else {
            sh.builtin_error("trap", format!("{}: invalid signal specification", String::from_utf8_lossy(s)));
            status = 1;
            continue;
        };
        let cmd = action.as_ref().map(|a| String::from_utf8_lossy(a).into_owned());
        match spec {
            TrapSpec::Debug => sh.traps.debug = cmd.filter(|c| !c.is_empty()),
            TrapSpec::Err => sh.traps.err = cmd.filter(|c| !c.is_empty()),
            TrapSpec::Return => sh.traps.ret = cmd.filter(|c| !c.is_empty()),
            TrapSpec::Signal(n) => {
                let disp = match &cmd {
                    None => SigDisposition::Default,
                    Some(c) if c.is_empty() => SigDisposition::Ignore,
                    Some(_) => SigDisposition::Catch,
                };
                match cmd {
                    None => {
                        sh.traps.signals.remove(&n);
                    }
                    Some(c) => {
                        sh.traps.signals.insert(n, c);
                    }
                }
                if n != TRAP_EXIT {
                    let sig = Signal(n);
                    // Sem trap do usuário, o SIGCHLD volta para a captura interna, não para o padrão.
                    let disp = if sig == Signal::SIGCHLD && disp == SigDisposition::Default && sh.sigchld_armed {
                        SigDisposition::Catch
                    } else {
                        disp
                    };
                    if !sig.is_uncatchable() {
                        let _ = sys().sigaction(sig, disp);
                    }
                }
            }
        }
    }
    Ok(status)
}

/// Resolve `%N`, `%%`, `%+`, `%-` em pids do job.
fn job_pids(sh: &Shell, spec: &[u8]) -> Option<Vec<Pid>> {
    let rest = &spec[1..];
    let job = if rest.is_empty() || rest == b"%" || rest == b"+" {
        sh.jobs.last()
    } else if rest == b"-" {
        sh.jobs.iter().rev().nth(1)
    } else if let Some(n) = parse_int(rest) {
        sh.jobs.iter().find(|j| j.id as i64 == n)
    } else {
        sh.jobs.iter().find(|j| j.text.as_bytes().starts_with(rest))
    };
    job.map(|j| j.pids.clone())
}

/// Nome do sinal `n` na tabela do dash (`signames.c`): sem o prefixo `SIG`, os números sem nome
/// (16, 32, 33) como o próprio número, e os de tempo real contados a partir de `RTMIN`/`RTMAX`.
fn dash_signal_name(n: i32) -> String {
    match n {
        0 | 16 => n.to_string(),
        _ => Signal(n).name().unwrap_or_else(|| n.to_string()),
    }
}

/// `get_signum` do dash: número menor que `NSIG` ou nome sem `SIG`, sem diferenciar maiúsculas.
fn dash_signum(s: &[u8]) -> Option<i32> {
    if !s.is_empty() && s.iter().all(u8::is_ascii_digit) {
        return std::str::from_utf8(s).ok()?.parse::<i32>().ok().filter(|n| *n < 65);
    }
    (1..65).find(|n| dash_signal_name(*n).as_bytes().eq_ignore_ascii_case(s))
}

/// `number` do dash: só dígitos, senão `Illegal number` (status 2).
fn dash_number(sh: &Shell, s: &[u8]) -> Result<i64, i32> {
    match std::str::from_utf8(s).ok().filter(|t| !t.is_empty() && t.bytes().all(|c| c.is_ascii_digit())).and_then(|t| t.parse().ok()) {
        Some(n) => Ok(n),
        None => {
            sh.builtin_error("kill", format!("Illegal number: {}", String::from_utf8_lossy(s)));
            Err(2)
        }
    }
}

/// O `killcmd` do dash (o `kill` do `sh` do Debian).
fn dash_kill(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    const USAGE: &str = "Usage: kill [-s sigspec | -signum | -sigspec] [pid | job]... or\nkill -l [exitstatus]";
    let mut sig = 15;
    let mut list = false;
    let mut i = 1;
    if argv.get(1).is_some_and(|a| a.first() == Some(&b'-')) {
        match dash_signum(&argv[1][1..]) {
            Some(n) => {
                sig = n;
                i = 2;
            }
            None => {
                // `nextopt("ls:")`.
                'opts: while let Some(a) = argv.get(i) {
                    if a.first() != Some(&b'-') || a.len() < 2 {
                        break;
                    }
                    i += 1;
                    if a == b"--" {
                        break;
                    }
                    let mut j = 1;
                    while j < a.len() {
                        match a[j] {
                            b'l' => list = true,
                            b's' => {
                                let arg = if j + 1 < a.len() {
                                    a[j + 1..].to_vec()
                                } else if let Some(n) = argv.get(i) {
                                    i += 1;
                                    n.clone()
                                } else {
                                    sh.builtin_error("kill", "No arg for -s option");
                                    return Ok(2);
                                };
                                match dash_signum(&arg) {
                                    Some(n) => sig = n,
                                    None => {
                                        sh.builtin_error("kill", format!("invalid signal number or name: {}", String::from_utf8_lossy(&arg)));
                                        return Ok(2);
                                    }
                                }
                                continue 'opts;
                            }
                            c => {
                                sh.builtin_error("kill", format!("Illegal option -{}", c as char));
                                return Ok(2);
                            }
                        }
                        j += 1;
                    }
                }
            }
        }
    }
    let rest = &argv[i.min(argv.len())..];
    if list {
        let Some(first) = rest.first() else {
            let text: String = (0..65).map(|n| format!("{}\n", dash_signal_name(n))).collect();
            out(sh, "kill", text.as_bytes());
            return Ok(0);
        };
        let mut n = match dash_number(sh, first) {
            Ok(n) => n,
            Err(st) => return Ok(st),
        };
        if n > 128 {
            n -= 128;
        }
        if 0 < n && n < 65 {
            out(sh, "kill", format!("{}\n", dash_signal_name(n as i32)).as_bytes());
            return Ok(0);
        }
        sh.builtin_error("kill", format!("invalid signal number or exit status: {}", String::from_utf8_lossy(first)));
        return Ok(2);
    }
    if rest.is_empty() {
        sh.builtin_error("kill", USAGE);
        return Ok(2);
    }
    let s = sys();
    let mut status = 0;
    for t in rest {
        let target = if t.first() == Some(&b'%') {
            match job_pids(sh, t).and_then(|p| p.first().copied()) {
                Some(p) => KillTarget::Group(p),
                None => {
                    sh.builtin_error("kill", format!("No such job: {}", String::from_utf8_lossy(t)));
                    return Ok(2);
                }
            }
        } else if let Some(num) = t.strip_prefix(b"-") {
            match dash_number(sh, num) {
                Ok(1) => KillTarget::All,
                Ok(0) => KillTarget::Pid(0),
                Ok(n) => KillTarget::Group(n as Pid),
                Err(st) => return Ok(st),
            }
        } else {
            match dash_number(sh, t) {
                Ok(n) => KillTarget::Pid(n as Pid),
                Err(st) => return Ok(st),
            }
        };
        if let Err(e) = s.kill(target, Signal(sig)) {
            sh.builtin_error("kill", format!("{}\n", e.message()));
            status = 1;
        }
    }
    Ok(status)
}

pub fn kill(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    if sh.dash_style() {
        return dash_kill(sh, argv);
    }
    let mut sig = Signal::SIGTERM;
    let mut i = 1;
    if argv.len() < 2 {
        let _ = write_fd(Fd::STDERR, b"kill: usage: kill [-s sigspec | -n signum | -sigspec] pid | jobspec ... or kill -l [sigspec]\n");
        return Ok(2);
    }
    while i < argv.len() {
        let a = &argv[i];
        if a == b"-l" || a == b"-L" {
            let rest = &argv[i + 1..];
            if rest.is_empty() {
                out(sh, "kill", signal_table().as_bytes());
                return Ok(0);
            }
            let mut status = 0;
            let mut text = String::new();
            for r in rest {
                if let Some(n) = parse_int(r) {
                    let n = if n > 128 { n - 128 } else { n };
                    match Signal(n as i32).name() {
                        Some(name) if n > 0 => text.push_str(&format!("{name}\n")),
                        _ => {
                            sh.builtin_error("kill", format!("{}: invalid signal specification", String::from_utf8_lossy(r)));
                            status = 1;
                        }
                    }
                } else {
                    match parse_signal(r) {
                        Some(s) => text.push_str(&format!("{}\n", s.0)),
                        None => {
                            sh.builtin_error("kill", format!("{}: invalid signal specification", String::from_utf8_lossy(r)));
                            status = 1;
                        }
                    }
                }
            }
            out(sh, "kill", text.as_bytes());
            return Ok(status);
        }
        if a == b"-s" || a == b"-n" {
            let Some(spec) = argv.get(i + 1) else {
                sh.builtin_error("kill", format!("{}: option requires an argument", String::from_utf8_lossy(a)));
                return Ok(2);
            };
            match parse_signal(spec).or_else(|| (spec == b"0").then_some(Signal(0))) {
                Some(s) => sig = s,
                None => {
                    sh.builtin_error("kill", format!("{}: invalid signal specification", String::from_utf8_lossy(spec)));
                    return Ok(1);
                }
            }
            i += 2;
            continue;
        }
        if a == b"--" {
            i += 1;
            break;
        }
        if a.len() > 1 && a[0] == b'-' && !a[1..].iter().all(|c| c.is_ascii_digit()) || (a.len() > 1 && a[0] == b'-' && i == 1 && argv.len() > 2) {
            let spec = &a[1..];
            match parse_signal(spec).or_else(|| (spec == b"0").then_some(Signal(0))) {
                Some(s) => sig = s,
                None => {
                    sh.builtin_error("kill", format!("{}: invalid signal specification", String::from_utf8_lossy(spec)));
                    return Ok(1);
                }
            }
            i += 1;
            continue;
        }
        break;
    }
    let targets = &argv[i..];
    if targets.is_empty() {
        let _ = write_fd(Fd::STDERR, b"kill: usage: kill [-s sigspec | -n signum | -sigspec] pid | jobspec ... or kill -l [sigspec]\n");
        return Ok(2);
    }
    let s = sys();
    let mut status = 0;
    for t in targets {
        let pids: Vec<KillTarget> = if t.starts_with(b"%") {
            match job_pids(sh, t) {
                Some(p) => p.into_iter().map(KillTarget::Pid).collect(),
                None => {
                    sh.builtin_error("kill", format!("{}: no such job", String::from_utf8_lossy(t)));
                    status = 1;
                    continue;
                }
            }
        } else {
            match parse_int(t) {
                Some(n) if n < -1 => vec![KillTarget::Group((-n) as Pid)],
                Some(-1) => vec![KillTarget::All],
                Some(n) => vec![KillTarget::Pid(n as Pid)],
                None => {
                    sh.builtin_error("kill", format!("{}: arguments must be process or job IDs", String::from_utf8_lossy(t)));
                    status = 1;
                    continue;
                }
            }
        };
        for target in pids {
            if let Err(e) = s.kill(target, sig) {
                let shown = String::from_utf8_lossy(t);
                if e == Errno::ESRCH {
                    sh.builtin_error("kill", format!("({shown}) - No such process"));
                } else {
                    sh.builtin_error("kill", format!("({shown}) - {}", e.message()));
                }
                status = 1;
            }
        }
    }
    // Sinal pra si mesmo com trap: roda logo depois do comando.
    Ok(status)
}

impl Shell {
    /// Marca o fim de um job e devolve o status (pra `wait`).
    fn wait_job(&mut self, idx: usize) -> i32 {
        let pids = self.jobs[idx].pids.clone();
        let mut last = 0;
        for (k, pid) in pids.iter().enumerate() {
            let st = match self.jobs[idx].status[k] {
                Some(st) => st,
                None => self.wait_pid(*pid),
            };
            self.jobs[idx].status[k] = Some(st);
            last = st;
        }
        last
    }
}

pub fn wait(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    let mut any = false;
    let mut var: Option<String> = None;
    let mut i = 1;
    while i < argv.len() {
        match argv[i].as_slice() {
            b"-n" => any = true,
            b"-f" => {}
            b"-p" => {
                var = argv.get(i + 1).map(|v| String::from_utf8_lossy(v).into_owned());
                i += 1;
            }
            b"--" => {
                i += 1;
                break;
            }
            _ => break,
        }
        i += 1;
    }
    let targets = &argv[i..];
    if targets.is_empty() {
        if any {
            // Espera qualquer um que ainda esteja rodando.
            let pending: Vec<usize> = (0..sh.jobs.len()).filter(|j| sh.jobs[*j].status.iter().any(|s| s.is_none())).collect();
            if pending.is_empty() {
                return Ok(127);
            }
            let s = sys();
            loop {
                match s.wait4(WaitTarget::Any, WaitOptions::empty()) {
                    Ok(Some((pid, st))) => {
                        for j in &mut sh.jobs {
                            if let Some(k) = j.pids.iter().position(|p| *p == pid) {
                                j.status[k] = Some(st.shell_status());
                                let code = st.shell_status();
                                if let Some(v) = var.clone() {
                                    sh.assign_scalar(&v, pid.to_string().into_bytes(), false)?;
                                }
                                return Ok(code);
                            }
                        }
                    }
                    Ok(None) => s.sched_yield(),
                    Err(Errno::EINTR) => {
                        sh.run_pending_traps()?;
                        return Ok(128 + 10);
                    }
                    Err(_) => return Ok(127),
                }
            }
        }
        for j in 0..sh.jobs.len() {
            sh.wait_job(j);
        }
        sh.jobs.clear();
        return Ok(0);
    }
    let mut status = 0;
    for t in targets {
        if t.starts_with(b"%") {
            let Some(pids) = job_pids(sh, t) else {
                sh.builtin_error("wait", format!("{}: no such job", String::from_utf8_lossy(t)));
                status = 127;
                continue;
            };
            let idx = sh.jobs.iter().position(|j| j.pids == pids);
            status = match idx {
                Some(i) => sh.wait_job(i),
                None => 127,
            };
            continue;
        }
        let Some(pid) = parse_int(t) else {
            sh.builtin_error("wait", format!("`{}': not a pid or valid job spec", String::from_utf8_lossy(t)));
            status = 2;
            continue;
        };
        let pid = pid as Pid;
        let job = sh.jobs.iter().position(|j| j.pids.contains(&pid));
        match job {
            Some(i) => {
                let k = sh.jobs[i].pids.iter().position(|p| *p == pid).unwrap_or(0);
                status = match sh.jobs[i].status[k] {
                    Some(st) => st,
                    None => {
                        let st = sh.wait_pid(pid);
                        sh.jobs[i].status[k] = Some(st);
                        st
                    }
                };
                if let Some(v) = var.clone() {
                    sh.assign_scalar(&v, pid.to_string().into_bytes(), false)?;
                }
            }
            None => {
                // Pode ser uma substituição de processo ou filho fora da tabela.
                match sys().wait4(WaitTarget::Pid(pid), WaitOptions::empty()) {
                    Ok(Some((_, st))) => status = st.shell_status(),
                    _ => {
                        sh.builtin_error("wait", format!("pid {pid} is not a child of this shell"));
                        status = 127;
                    }
                }
            }
        }
    }
    Ok(status)
}

pub fn jobs(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    let pids_only = argv.iter().skip(1).any(|a| a == b"-p");
    let long = argv.iter().skip(1).any(|a| a == b"-l");
    let s = sys();
    // Atualiza o estado sem bloquear.
    for j in &mut sh.jobs {
        for (k, pid) in j.pids.iter().enumerate() {
            if j.status[k].is_none()
                && let Ok(Some((_, st))) = s.wait4(WaitTarget::Pid(*pid), WaitOptions::NOHANG) {
                    j.status[k] = Some(st.shell_status());
                }
        }
    }
    let n = sh.jobs.len();
    let mut text = String::new();
    for (idx, j) in sh.jobs.iter().enumerate() {
        if pids_only {
            for p in &j.pids {
                text.push_str(&format!("{p}\n"));
            }
            continue;
        }
        let mark = if idx + 1 == n { '+' } else if idx + 2 == n { '-' } else { ' ' };
        let done = j.status.iter().all(|s| s.is_some());
        let state = if done {
            match j.status.last().copied().flatten() {
                Some(0) => "Done".to_string(),
                Some(st) => format!("Exit {st}"),
                None => "Done".to_string(),
            }
        } else {
            "Running".to_string()
        };
        if long {
            text.push_str(&format!("[{}]{mark} {:>5} {state:<24}{}\n", j.id, j.pids[0], j.text));
        } else {
            text.push_str(&format!("[{}]{mark}  {state:<24}{}\n", j.id, j.text));
        }
    }
    sh.jobs.retain(|j| !j.status.iter().all(|s| s.is_some()));
    out(sh, "jobs", text.as_bytes());
    Ok(0)
}

pub fn fg_bg(sh: &mut Shell, name: &str, _argv: &[Vec<u8>]) -> Exec {
    if !sh.opts.get("monitor") {
        sh.builtin_error(name, "no job control");
        return Ok(1);
    }
    sh.builtin_error(name, "no current job");
    Ok(1)
}

pub fn disown(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    let all = argv.iter().skip(1).any(|a| a == b"-a");
    if all || argv.len() == 1 {
        if all {
            sh.jobs.clear();
        } else {
            sh.jobs.pop();
        }
        return Ok(0);
    }
    let mut status = 0;
    for a in &argv[1..] {
        if a.starts_with(b"-") {
            continue;
        }
        let pids = if a.starts_with(b"%") { job_pids(sh, a) } else { parse_int(a).map(|p| vec![p as Pid]) };
        match pids {
            Some(p) => sh.jobs.retain(|j| j.pids != p && !p.iter().all(|x| j.pids.contains(x))),
            None => {
                sh.builtin_error("disown", format!("{}: no such job", String::from_utf8_lossy(a)));
                status = 1;
            }
        }
    }
    Ok(status)
}

pub fn suspend(sh: &mut Shell, _argv: &[Vec<u8>]) -> Exec {
    if !sh.opts.get("monitor") {
        sh.builtin_error("suspend", "cannot suspend a shell without job control");
        return Ok(1);
    }
    let _ = sys().kill(KillTarget::Pid(sys().getpid()), Signal::SIGSTOP);
    Ok(0)
}
