//! `skill` e `snice` do procps-ng 4.0.4 (um binário só, o nome vem do argv0).
//!
//! - A expressão é um conjunto de seletores: `-t` terminal, `-u` usuário, `-c` comando (o `comm`
//!   inteiro), `-p` pid. Argumentos soltos são classificados nessa ordem: número vira pid, nome com
//!   nó em `/dev` vira terminal, usuário existente vira usuário, o resto vira comando. Um processo
//!   casa quando passa em todas as categorias dadas (dentro de cada uma basta um item).
//! - `skill [sinal]`: o primeiro `-<sinal>` (número, nome, `RTMIN+n`) antes das opções vira o sinal;
//!   o padrão é TERM. `snice [prioridade]`: o primeiro `-n` ou `+n` numérico vira a prioridade
//!   (padrão 4).
//! - `-n` só mostra o que faria, `-v` mostra e executa, `-i` pergunta antes de cada processo.
//! - O próprio processo nunca é alvo.

use std::ffi::OsString;

use sysabi::{Ctx, Errno, KillTarget, Pid, Signal, sys};
use ul_misc::util::getopt::{Getopt, HasArg, LongOpt};
use ul_misc::util::io;

use crate::common::{self, Names, out};
use crate::procfs::{self, Want};

const USAGE_OPTS: &str = "\nOptions:\n -f, --fast         fast mode (not implemented)\n -i, --interactive  interactive\n -l, --list         list all signal names\n -L, --table        list all signal names in a nice table\n -n, --no-action    do not actually kill processes; just print what would happen\n -v, --verbose      explain what is being done\n -w, --warnings     enable warnings (not implemented)\n\nExpression can be: terminal, user, pid, command.\nThe options below may be used to ensure correct interpretation.\n -c, --command <command>  expression is a command name\n -p, --pid <pid>          expression is a process id number\n -t, --tty <tty>          expression is a terminal\n -u, --user <username>    expression is a username\n\nAlternatively, expression can be:\n --ns <pid>               match the processes that belong to the same\n                          namespace as <pid>\n --nslist <ns,...>        list which namespaces will be considered for\n                          the --ns option; available namespaces are:\n                          ipc, mnt, net, pid, user, uts\n\n\n -h, --help     display this help and exit\n -V, --version  output version information and exit\n";

const USAGE_TAIL_SKILL: &str = "\nThe default signal is TERM. Use -l or -L to list available signals.\nParticularly useful signals include HUP, INT, KILL, STOP, CONT, and 0.\nAlternate signals may be specified in three ways: -SIGKILL -KILL -9\n\nFor more details see skill(1).\n";

const USAGE_TAIL_SNICE: &str = "\nThe default priority is +4. (snice +4 ...)\nPriority numbers range from +20 (slowest) to -20 (fastest).\nNegative priority numbers are restricted to administrative users.\n\nFor more details see snice(1).\n";

const NS: i32 = 300;
const NSLIST: i32 = 301;
const NS_TYPES: [&str; 6] = ["ipc", "mnt", "net", "pid", "user", "uts"];

const LONGS: &[LongOpt] = &[
    LongOpt::new("command", HasArg::Required, 'c' as i32),
    LongOpt::new("pid", HasArg::Required, 'p' as i32),
    LongOpt::new("tty", HasArg::Required, 't' as i32),
    LongOpt::new("user", HasArg::Required, 'u' as i32),
    LongOpt::new("fast", HasArg::No, 'f' as i32),
    LongOpt::new("interactive", HasArg::No, 'i' as i32),
    LongOpt::new("list", HasArg::No, 'l' as i32),
    LongOpt::new("table", HasArg::No, 'L' as i32),
    LongOpt::new("no-action", HasArg::No, 'n' as i32),
    LongOpt::new("verbose", HasArg::No, 'v' as i32),
    LongOpt::new("warnings", HasArg::No, 'w' as i32),
    LongOpt::new("ns", HasArg::Required, NS),
    LongOpt::new("nslist", HasArg::Required, NSLIST),
    LongOpt::new("help", HasArg::No, 'h' as i32),
    LongOpt::new("version", HasArg::No, 'V' as i32),
];

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn usage(prog: &str) -> String {
    let first = if prog == "snice" { "[new priority]" } else { "[signal]" };
    let tail = if prog == "snice" { USAGE_TAIL_SNICE } else { USAGE_TAIL_SKILL };
    format!("\nUsage:\n {prog} {first} [options] <expression>\n{USAGE_OPTS}{tail}")
}

/// `-<sinal>` do skill: número 0 a 64 ou nome da tabela / tempo real.
fn skill_option(arg: &str) -> Option<i32> {
    let body = arg.strip_prefix('-')?;
    if body.is_empty() || body.starts_with('-') {
        return None;
    }
    if body.as_bytes()[0].is_ascii_digit() {
        return common::parse_long(body).filter(|n| (0..=64).contains(n)).map(|n| n as i32);
    }
    common::signal_by_table_name(body).or_else(|| common::signal_rt(body))
}

/// `-n` ou `+n` do snice: prioridade numérica.
fn snice_option(arg: &str) -> Option<i32> {
    let body = arg.strip_prefix('-').or_else(|| arg.strip_prefix('+'))?;
    if body.is_empty() || !body.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: i32 = body.parse().ok()?;
    Some(if arg.starts_with('-') { -n } else { n })
}

const WITH_ARG: [&str; 9] = ["-c", "-p", "-t", "-u", "--command", "--pid", "--tty", "--user", "--ns"];

/// Procura a opção de sinal/prioridade antes do getopt e a tira da lista.
fn take_leading(rest: &mut Vec<String>, snice: bool) -> Option<i32> {
    let mut i = 0;
    while i < rest.len() {
        let a = &rest[i];
        if a == "--" {
            return None;
        }
        if WITH_ARG.contains(&a.as_str()) || a == "--nslist" {
            i += 2;
            continue;
        }
        let v = if snice { snice_option(a) } else { skill_option(a) };
        if let Some(v) = v {
            rest.remove(i);
            return Some(v);
        }
        i += 1;
    }
    None
}

#[derive(Default)]
struct Expr {
    ttys: Vec<String>,
    users: Vec<u32>,
    cmds: Vec<Vec<u8>>,
    pids: Vec<Pid>,
    ns_pid: Option<Pid>,
    nslist: Option<Vec<usize>>,
}

impl Expr {
    fn is_empty(&self) -> bool {
        self.ttys.is_empty() && self.users.is_empty() && self.cmds.is_empty() && self.pids.is_empty() && self.ns_pid.is_none()
    }
}

fn ns_ids(pid: Pid) -> Vec<Option<Vec<u8>>> {
    let sysc = sys::current();
    NS_TYPES.iter().map(|t| sysc.readlinkat(sysabi::Fd::CWD, format!("/proc/{pid}/ns/{t}").as_bytes()).ok()).collect()
}

/// Prioridade pelo kernel, no intervalo `-20..=19` como o `renice`.
fn set_priority(pid: Pid, prio: i32) -> Result<(), Errno> {
    sys::current().setpriority(pid, prio.clamp(-20, 19))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let full = String::from_utf8_lossy(&argv[0]).into_owned();
    let base = full.rsplit('/').next().unwrap_or("skill").to_string();
    let snice = base == "snice";
    let prog = if snice { "snice" } else { "skill" };
    let mut rest: Vec<String> = argv[1..].iter().map(|a| String::from_utf8_lossy(a).into_owned()).collect();
    if rest.is_empty() {
        io::eprint(usage(prog));
        return 1;
    }
    let lead = take_leading(&mut rest, snice);
    let signal = if snice { 0 } else { lead.unwrap_or(Signal::SIGTERM.0) };
    let prio = if snice { lead.unwrap_or(4) } else { 0 };

    let rest_bytes: Vec<Vec<u8>> = rest.iter().map(|s| s.as_bytes().to_vec()).collect();
    let mut g = Getopt::from_env(&rest_bytes, "c:p:t:u:filLnvwhV", LONGS);
    let mut names = Names::new();
    let mut e = Expr::default();
    let (mut interactive, mut no_action, mut verbose) = (false, false, false);
    while let Some(r) = g.next_opt() {
        let opt = match r {
            Ok(x) => x,
            Err(er) => {
                io::eprint(format!("{}\n{}", er.message(&argv0), usage(prog)));
                return 1;
            }
        };
        let a = opt.arg_str();
        match opt.id {
            x if x == 'c' as i32 => e.cmds.push(a.into_bytes()),
            x if x == 'p' as i32 => match common::parse_long(&a) {
                Some(v) if v > 0 => e.pids.push(v as Pid),
                _ => {
                    common::warn(prog, &format!("failed to parse argument: '{a}'"));
                    return 1;
                }
            },
            x if x == 't' as i32 => e.ttys.push(a.strip_prefix("/dev/").unwrap_or(&a).to_string()),
            x if x == 'u' as i32 => match names.uid_of(&a) {
                Some(u) => e.users.push(u),
                None => {}
            },
            x if x == 'f' as i32 || x == 'w' as i32 => {}
            x if x == 'i' as i32 => interactive = true,
            x if x == 'n' as i32 => no_action = true,
            x if x == 'v' as i32 => verbose = true,
            x if x == 'l' as i32 => {
                out("HUP INT QUIT ILL TRAP ABRT BUS FPE KILL USR1 SEGV USR2 PIPE ALRM TERM STKFLT\nCHLD CONT STOP TSTP TTIN TTOU URG XCPU XFSZ VTALRM PROF WINCH POLL PWR SYS\n");
                return 0;
            }
            x if x == 'L' as i32 => {
                out(crate::kill::signal_table());
                return 0;
            }
            NS => match common::parse_long(&a) {
                Some(v) if v > 0 => e.ns_pid = Some(v as Pid),
                _ => {
                    common::warn(prog, &format!("invalid PID: {a}"));
                    return 1;
                }
            },
            NSLIST => {
                let mut v = Vec::new();
                for t in a.split(',') {
                    match NS_TYPES.iter().position(|n| *n == t) {
                        Some(i) => v.push(i),
                        None => return 1,
                    }
                }
                e.nslist = Some(v);
            }
            x if x == 'h' as i32 => {
                out(usage(prog));
                return 0;
            }
            x if x == 'V' as i32 => {
                out(format!("{prog} from procps-ng 4.0.4\n"));
                return 0;
            }
            _ => unreachable!("tabela de opções do {prog}"),
        }
    }
    for raw in g.operands() {
        let a = String::from_utf8_lossy(&raw).into_owned();
        if let Some(v) = common::parse_long(&a).filter(|v| *v > 0) {
            e.pids.push(v as Pid);
        } else if common::tty_dev_by_name(&a).is_some() {
            e.ttys.push(a.strip_prefix("/dev/").unwrap_or(&a).to_string());
        } else if let Some(u) = names.uid_of(&a) {
            e.users.push(u);
        } else {
            e.cmds.push(raw);
        }
    }
    if e.is_empty() {
        io::eprint(format!("{prog}: no process selection criteria\n"));
        return 1;
    }

    let sysc = sys::current();
    let me = sysc.getpid();
    let snap = procfs::scan(Want::default());
    let ns_ref = e.ns_pid.map(ns_ids);
    let mut status = 0;
    let mut matched = false;
    for (n, p) in snap.procs.iter().enumerate() {
        if n % 64 == 0 {
            sys::checkpoint();
        }
        if p.tgid == me {
            continue;
        }
        let tty = common::tty_name(p.stat.tty_nr, p.pid());
        let mut ok = true;
        if !e.ttys.is_empty() {
            ok &= tty.as_ref().is_some_and(|t| e.ttys.contains(t));
        }
        if !e.users.is_empty() {
            ok &= e.users.contains(&p.euid());
        }
        if !e.cmds.is_empty() {
            ok &= e.cmds.iter().any(|c| c.as_slice() == p.comm());
        }
        if !e.pids.is_empty() {
            ok &= e.pids.contains(&p.pid());
        }
        if let Some(reference) = &ns_ref {
            let theirs = ns_ids(p.pid());
            let types: Vec<usize> = e.nslist.clone().unwrap_or_else(|| (0..NS_TYPES.len()).collect());
            ok &= types.iter().all(|t| reference[*t] == theirs[*t]);
        }
        if !ok {
            continue;
        }
        matched = true;
        let line = format!(
            "{:<8} {:<8} {:>5} {:<15}",
            tty.as_deref().unwrap_or("?"),
            names.user_or_id(p.euid()),
            p.pid(),
            String::from_utf8_lossy(p.comm())
        );
        if interactive {
            io::eprint(format!("{line}? "));
            let mut ans = String::new();
            // Lê uma linha do stdin do pseudo-processo, um byte por vez para não consumir além dela.
            let mut b = [0u8; 1];
            while matches!(sysabi::sys::read(sysabi::Fd::STDIN, &mut b), Ok(1)) {
                if b[0] == b'\n' {
                    break;
                }
                ans.push(char::from(b[0]));
            }
            if !ans.trim_start().starts_with(['y', 'Y']) {
                continue;
            }
        } else if no_action || verbose {
            out(format!("{line}\n"));
        }
        if no_action {
            continue;
        }
        let res = if snice { set_priority(p.pid(), prio) } else { sysc.kill(KillTarget::Pid(p.pid()), Signal(signal)) };
        if let Err(er) = res {
            io::eprint(format!("{prog}: ({}): {}\n", p.pid(), er.message()));
            status = 1;
        }
    }
    let _ = matched;
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leading_options() {
        assert_eq!(skill_option("-9"), Some(9));
        assert_eq!(skill_option("-KILL"), Some(9));
        assert_eq!(skill_option("-65"), None);
        assert_eq!(skill_option("--x"), None);
        assert_eq!(snice_option("+5"), Some(5));
        assert_eq!(snice_option("-5"), Some(-5));
        assert_eq!(snice_option("-v"), None);
    }
}
