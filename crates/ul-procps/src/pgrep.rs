//! `pgrep`, `pkill` e `pidwait` do procps-ng 4.0.4 (um programa só, como no original).
//!
//! - Casamento por ERE ([`crate::matcher`]) contra o `comm` (até 15 caracteres) ou, com `-f`, contra
//!   a linha de comando (`[comm]` sem argumentos, `[comm] <defunct>` no zumbi); `-x` envolve o
//!   padrão em `^(...)$`.
//! - O próprio processo nunca entra; `-A` tira também os ancestrais.
//! - `-t` compara o nome do terminal (`?` quando não há ou não resolve), como o original.
//! - Listas numéricas aceitam item vazio como 0; `-O` inválido vale 0 (desliga).
//! - Status: 0 casou, 1 nada casou (ou o pkill não conseguiu sinalizar ninguém), 2 erro de
//!   sintaxe, 3 erro fatal.
//! - `pkill -q valor` usa `sigqueue` no original; aqui o sinal sai por `kill` (o contrato não tem
//!   `sigqueue`). `pidwait` espera consultando o processo a cada 100 ms (o original usa pidfd).

use std::ffi::OsString;
use std::time::Duration;

use sysabi::{Ctx, Errno, KillTarget, OFlags, Pid, Signal, sys};
use ul_misc::util::getopt::{Getopt, HasArg, LongOpt};
use ul_misc::util::io;

use crate::common::{self, Names, out};
use crate::matcher;
use crate::procfs::{self, Proc, Want};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Pgrep,
    Pkill,
    Pidwait,
}

impl Mode {
    fn name(self) -> &'static str {
        match self {
            Mode::Pgrep => "pgrep",
            Mode::Pkill => "pkill",
            Mode::Pidwait => "pidwait",
        }
    }
}

const USAGE_TAIL: &str = " -c, --count               count of matching processes\n -f, --full                use full process name to match\n -g, --pgroup <PGID,...>   match listed process group IDs\n -G, --group <GID,...>     match real group IDs\n -i, --ignore-case         match case insensitively\n -n, --newest              select most recently started\n -o, --oldest              select least recently started\n -O, --older <seconds>     select where older than seconds\n -P, --parent <PPID,...>   match only child processes of the given parent\n -s, --session <SID,...>   match session IDs\n     --signal <sig>        signal to send (either number or name)\n -t, --terminal <tty,...>  match by controlling terminal\n -u, --euid <ID,...>       match by effective IDs\n -U, --uid <ID,...>        match by real IDs\n -x, --exact               match exactly with the command name\n -F, --pidfile <file>      read PIDs from file\n -L, --logpidfile          fail if PID file is not locked\n -r, --runstates <state>   match runstates [D,S,Z,...]\n -A, --ignore-ancestors    exclude our ancestors from results\n --cgroup <grp,...>        match by cgroup v2 names\n --ns <PID>                match the processes that belong to the same\n                           namespace as <pid>\n --nslist <ns,...>         list which namespaces will be considered for\n                           the --ns option.\n                           Available namespaces: ipc, mnt, net, pid, user, uts\n\n -h, --help     display this help and exit\n -V, --version  output version information and exit\n\nFor more details see pgrep(1).\n";

fn usage(mode: Mode) -> String {
    let head = match mode {
        Mode::Pgrep => " -d, --delimiter <string>  specify output delimiter\n -l, --list-name           list PID and process name\n -a, --list-full           list PID and full command line\n -v, --inverse             negates the matching\n -w, --lightweight         list all TID\n",
        Mode::Pkill => " -<sig>                    signal to send (either number or name)\n -H, --require-handler     match only if signal handler is present\n -q, --queue <value>       integer value to be sent with the signal\n -e, --echo                display what is killed\n",
        Mode::Pidwait => " -e, --echo                display PIDs before waiting\n",
    };
    format!("\nUsage:\n {} [options] <pattern>\n\nOptions:\n{head}{USAGE_TAIL}", mode.name())
}

const SIGNAL: i32 = 300;
const CGROUP: i32 = 301;
const NS: i32 = 302;
const NSLIST: i32 = 303;

const LONGS_PGREP: &[LongOpt] = &[
    LongOpt::new("delimiter", HasArg::Required, 'd' as i32),
    LongOpt::new("list-name", HasArg::No, 'l' as i32),
    LongOpt::new("list-full", HasArg::No, 'a' as i32),
    LongOpt::new("inverse", HasArg::No, 'v' as i32),
    LongOpt::new("lightweight", HasArg::No, 'w' as i32),
    LongOpt::new("count", HasArg::No, 'c' as i32),
    LongOpt::new("full", HasArg::No, 'f' as i32),
    LongOpt::new("pgroup", HasArg::Required, 'g' as i32),
    LongOpt::new("group", HasArg::Required, 'G' as i32),
    LongOpt::new("ignore-case", HasArg::No, 'i' as i32),
    LongOpt::new("newest", HasArg::No, 'n' as i32),
    LongOpt::new("oldest", HasArg::No, 'o' as i32),
    LongOpt::new("older", HasArg::Required, 'O' as i32),
    LongOpt::new("parent", HasArg::Required, 'P' as i32),
    LongOpt::new("session", HasArg::Required, 's' as i32),
    LongOpt::new("signal", HasArg::Required, SIGNAL),
    LongOpt::new("terminal", HasArg::Required, 't' as i32),
    LongOpt::new("euid", HasArg::Required, 'u' as i32),
    LongOpt::new("uid", HasArg::Required, 'U' as i32),
    LongOpt::new("exact", HasArg::No, 'x' as i32),
    LongOpt::new("pidfile", HasArg::Required, 'F' as i32),
    LongOpt::new("logpidfile", HasArg::No, 'L' as i32),
    LongOpt::new("runstates", HasArg::Required, 'r' as i32),
    LongOpt::new("ignore-ancestors", HasArg::No, 'A' as i32),
    LongOpt::new("cgroup", HasArg::Required, CGROUP),
    LongOpt::new("ns", HasArg::Required, NS),
    LongOpt::new("nslist", HasArg::Required, NSLIST),
    LongOpt::new("help", HasArg::No, 'h' as i32),
    LongOpt::new("version", HasArg::No, 'V' as i32),
];

const LONGS_PKILL: &[LongOpt] = &[
    LongOpt::new("require-handler", HasArg::No, 'H' as i32),
    LongOpt::new("queue", HasArg::Required, 'q' as i32),
    LongOpt::new("echo", HasArg::No, 'e' as i32),
    LongOpt::new("count", HasArg::No, 'c' as i32),
    LongOpt::new("full", HasArg::No, 'f' as i32),
    LongOpt::new("pgroup", HasArg::Required, 'g' as i32),
    LongOpt::new("group", HasArg::Required, 'G' as i32),
    LongOpt::new("ignore-case", HasArg::No, 'i' as i32),
    LongOpt::new("newest", HasArg::No, 'n' as i32),
    LongOpt::new("oldest", HasArg::No, 'o' as i32),
    LongOpt::new("older", HasArg::Required, 'O' as i32),
    LongOpt::new("parent", HasArg::Required, 'P' as i32),
    LongOpt::new("session", HasArg::Required, 's' as i32),
    LongOpt::new("signal", HasArg::Required, SIGNAL),
    LongOpt::new("terminal", HasArg::Required, 't' as i32),
    LongOpt::new("euid", HasArg::Required, 'u' as i32),
    LongOpt::new("uid", HasArg::Required, 'U' as i32),
    LongOpt::new("exact", HasArg::No, 'x' as i32),
    LongOpt::new("pidfile", HasArg::Required, 'F' as i32),
    LongOpt::new("logpidfile", HasArg::No, 'L' as i32),
    LongOpt::new("runstates", HasArg::Required, 'r' as i32),
    LongOpt::new("ignore-ancestors", HasArg::No, 'A' as i32),
    LongOpt::new("cgroup", HasArg::Required, CGROUP),
    LongOpt::new("ns", HasArg::Required, NS),
    LongOpt::new("nslist", HasArg::Required, NSLIST),
    LongOpt::new("help", HasArg::No, 'h' as i32),
    LongOpt::new("version", HasArg::No, 'V' as i32),
];

const LONGS_PIDWAIT: &[LongOpt] = &[
    LongOpt::new("echo", HasArg::No, 'e' as i32),
    LongOpt::new("count", HasArg::No, 'c' as i32),
    LongOpt::new("full", HasArg::No, 'f' as i32),
    LongOpt::new("pgroup", HasArg::Required, 'g' as i32),
    LongOpt::new("group", HasArg::Required, 'G' as i32),
    LongOpt::new("ignore-case", HasArg::No, 'i' as i32),
    LongOpt::new("newest", HasArg::No, 'n' as i32),
    LongOpt::new("oldest", HasArg::No, 'o' as i32),
    LongOpt::new("older", HasArg::Required, 'O' as i32),
    LongOpt::new("parent", HasArg::Required, 'P' as i32),
    LongOpt::new("session", HasArg::Required, 's' as i32),
    LongOpt::new("signal", HasArg::Required, SIGNAL),
    LongOpt::new("terminal", HasArg::Required, 't' as i32),
    LongOpt::new("euid", HasArg::Required, 'u' as i32),
    LongOpt::new("uid", HasArg::Required, 'U' as i32),
    LongOpt::new("exact", HasArg::No, 'x' as i32),
    LongOpt::new("pidfile", HasArg::Required, 'F' as i32),
    LongOpt::new("logpidfile", HasArg::No, 'L' as i32),
    LongOpt::new("runstates", HasArg::Required, 'r' as i32),
    LongOpt::new("ignore-ancestors", HasArg::No, 'A' as i32),
    LongOpt::new("cgroup", HasArg::Required, CGROUP),
    LongOpt::new("ns", HasArg::Required, NS),
    LongOpt::new("nslist", HasArg::Required, NSLIST),
    LongOpt::new("help", HasArg::No, 'h' as i32),
    LongOpt::new("version", HasArg::No, 'V' as i32),
];

pub fn pgrep_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args, Mode::Pgrep))
}

pub fn pkill_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args, Mode::Pkill))
}

pub fn pidwait_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args, Mode::Pidwait))
}

const NS_TYPES: [&str; 6] = ["ipc", "mnt", "net", "pid", "user", "uts"];

#[derive(Default)]
struct Opts {
    delim: Vec<u8>,
    list_name: bool,
    list_full: bool,
    inverse: bool,
    lightweight: bool,
    count: bool,
    full: bool,
    pgroups: Option<Vec<i64>>,
    rgids: Option<Vec<u32>>,
    icase: bool,
    newest: bool,
    oldest: bool,
    older: Option<f64>,
    parents: Option<Vec<i64>>,
    sessions: Option<Vec<i64>>,
    ttys: Option<Vec<String>>,
    euids: Option<Vec<u32>>,
    ruids: Option<Vec<u32>>,
    exact: bool,
    pidfile: Option<Vec<u8>>,
    logpidfile: bool,
    runstates: Option<String>,
    ignore_ancestors: bool,
    cgroups: Option<Vec<String>>,
    ns_pid: Option<Pid>,
    nslist: Option<Vec<usize>>,
    echo: bool,
    require_handler: bool,
    signal: i32,
}

enum Fail {
    /// Mensagem + "Try ..." e status.
    Try(String, i32),
    /// Mensagem simples (`prog: msg`) e status.
    Plain(String, i32),
    /// Mensagem já pronta (pode ser vazia) seguida do uso no stderr, status 2.
    Usage(String),
    /// Sai sem dizer nada.
    Silent(i32),
}

/// Lista separada por vírgula; item vazio vale 0 (é o que o strtol do original faz).
fn num_list(s: &str) -> Option<Vec<i64>> {
    let mut out = Vec::new();
    for part in s.split(',') {
        if part.is_empty() {
            out.push(0);
            continue;
        }
        out.push(common::parse_long(part)?);
    }
    Some(out)
}

fn id_list(s: &str, names: &mut Names, user: bool) -> Option<Vec<u32>> {
    let mut out = Vec::new();
    for part in s.split(',') {
        if part.is_empty() {
            out.push(0);
            continue;
        }
        if let Some(v) = common::parse_long(part) {
            out.push(v as u32);
            continue;
        }
        let v = if user { names.uid_of(part) } else { names.gid_of(part) };
        out.push(v?);
    }
    Some(out)
}

/// `-<sinal>` antes do getopt: o primeiro argumento que é sinal válido vira o sinal.
fn take_signal_option(args: &mut Vec<String>) -> Option<i32> {
    let with_arg = ["-g", "-G", "-O", "-P", "-s", "-t", "-u", "-U", "-F", "-r", "-q", "--signal", "--cgroup", "--ns", "--nslist", "--pgroup", "--group", "--older", "--parent", "--session", "--terminal", "--euid", "--uid", "--pidfile", "--runstates", "--queue"];
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a == "--" {
            return None;
        }
        if with_arg.contains(&a.as_str()) {
            i += 2;
            continue;
        }
        if let Some(body) = a.strip_prefix('-')
            && !body.is_empty() && !body.starts_with('-') {
                let sig = if body.as_bytes()[0].is_ascii_digit() {
                    common::parse_long(body).filter(|n| (0..=64).contains(n)).map(|n| n as i32)
                } else {
                    common::signal_by_table_name(body).or_else(|| common::signal_rt(body))
                };
                if let Some(s) = sig {
                    args.remove(i);
                    return Some(s);
                }
            }
        i += 1;
    }
    None
}

/// Sinal do `--signal`: nome da tabela, tempo real ou número.
fn parse_signal(s: &str) -> Option<i32> {
    if let Some(n) = common::signal_by_table_name(s).or_else(|| common::signal_rt(s)) {
        return Some(n);
    }
    common::parse_long(s).filter(|n| (0..=64).contains(n)).map(|n| n as i32)
}

fn parse_opts(mode: Mode, argv: &[Vec<u8>], argv0: &str, names: &mut Names) -> Result<(Opts, Option<Vec<u8>>), Fail> {
    let prog = mode.name();
    let mut rest: Vec<String> = argv[1..].iter().map(|a| String::from_utf8_lossy(a).into_owned()).collect();
    let mut o = Opts { delim: b"\n".to_vec(), signal: Signal::SIGTERM.0, ..Opts::default() };
    if mode == Mode::Pkill
        && let Some(s) = take_signal_option(&mut rest) {
            o.signal = s;
        }
    let rest_bytes: Vec<Vec<u8>> = rest.iter().map(|s| s.as_bytes().to_vec()).collect();
    let (spec, longs) = match mode {
        Mode::Pgrep => ("lad:vwcfg:G:inoO:P:s:t:u:U:xF:Lr:AhV", LONGS_PGREP),
        Mode::Pkill => ("eHq:cfg:G:inoO:P:s:t:u:U:xF:Lr:AhV", LONGS_PKILL),
        Mode::Pidwait => ("ecfg:G:inoO:P:s:t:u:U:xF:Lr:AhV", LONGS_PIDWAIT),
    };
    let mut g = Getopt::from_env(&rest_bytes, spec, longs);
    let mut criteria = false;
    while let Some(r) = g.next_opt() {
        let opt = match r {
            Ok(x) => x,
            Err(e) => return Err(Fail::Usage(format!("{}\n", e.message(argv0)))),
        };
        let a = opt.arg_str();
        match opt.id {
            x if x == 'd' as i32 => o.delim = opt.arg.clone().unwrap_or_default(),
            x if x == 'l' as i32 => o.list_name = true,
            x if x == 'a' as i32 => o.list_full = true,
            x if x == 'v' as i32 => o.inverse = true,
            x if x == 'w' as i32 => o.lightweight = true,
            x if x == 'c' as i32 => o.count = true,
            x if x == 'f' as i32 => o.full = true,
            x if x == 'g' as i32 => {
                o.pgroups = Some(num_list(&a).ok_or_else(|| Fail::Plain(format!("invalid process group: {a}"), 2))?);
                criteria = true;
            }
            x if x == 'G' as i32 => {
                o.rgids = Some(id_list(&a, names, false).ok_or_else(|| Fail::Plain(format!("invalid group name: {a}"), 2))?);
                criteria = true;
            }
            x if x == 'i' as i32 => o.icase = true,
            x if x == 'n' as i32 => o.newest = true,
            x if x == 'o' as i32 => o.oldest = true,
            x if x == 'O' as i32 => {
                // atoi: lixo ou negativo desliga a opção, mas ela conta como critério.
                let v = common::parse_long(a.split(|c: char| !c.is_ascii_digit() && c != '-' && c != '+').next().unwrap_or("")).unwrap_or(0);
                o.older = Some(v.max(0) as f64);
                criteria = true;
            }
            x if x == 'P' as i32 => {
                o.parents = Some(num_list(&a).ok_or_else(|| Fail::Plain(format!("not a number: {a}"), 2))?);
                criteria = true;
            }
            x if x == 's' as i32 => {
                o.sessions = Some(num_list(&a).ok_or_else(|| Fail::Plain(format!("invalid session id: {a}"), 2))?);
                criteria = true;
            }
            SIGNAL => match parse_signal(&a) {
                Some(s) => o.signal = s,
                None => return Err(Fail::Usage(format!("Unknown signal \"{a}\"."))),
            },
            x if x == 't' as i32 => {
                if a.is_empty() {
                    return Err(Fail::Usage(String::new()));
                }
                o.ttys = Some(a.split(',').map(|t| t.strip_prefix("/dev/").unwrap_or(t).to_string()).collect());
                criteria = true;
            }
            x if x == 'u' as i32 => {
                o.euids = Some(id_list(&a, names, true).ok_or_else(|| Fail::Plain(format!("invalid user name: {a}"), 2))?);
                criteria = true;
            }
            x if x == 'U' as i32 => {
                o.ruids = Some(id_list(&a, names, true).ok_or_else(|| Fail::Plain(format!("invalid user name: {a}"), 2))?);
                criteria = true;
            }
            x if x == 'x' as i32 => o.exact = true,
            x if x == 'F' as i32 => {
                o.pidfile = opt.arg.clone();
                criteria = true;
            }
            x if x == 'L' as i32 => o.logpidfile = true,
            x if x == 'r' as i32 => {
                o.runstates = Some(a);
                criteria = true;
            }
            x if x == 'A' as i32 => o.ignore_ancestors = true,
            CGROUP => {
                o.cgroups = Some(a.split(',').map(str::to_string).collect());
                criteria = true;
            }
            NS => {
                let pid = common::parse_long(&a).ok_or_else(|| Fail::Plain(format!("invalid PID: {a}"), 2))?;
                o.ns_pid = Some(pid as Pid);
                criteria = true;
            }
            NSLIST => {
                let mut v = Vec::new();
                for t in a.split(',') {
                    match NS_TYPES.iter().position(|n| *n == t) {
                        Some(i) => v.push(i),
                        None => return Err(Fail::Silent(2)),
                    }
                }
                o.nslist = Some(v);
            }
            x if x == 'e' as i32 => o.echo = true,
            x if x == 'H' as i32 => o.require_handler = true,
            x if x == 'q' as i32 => {}
            x if x == 'h' as i32 => {
                out(usage(mode));
                return Err(Fail::Silent(0));
            }
            x if x == 'V' as i32 => {
                out(format!("{prog} from procps-ng 4.0.4\n"));
                return Err(Fail::Silent(0));
            }
            _ => unreachable!("tabela de opções do {prog}"),
        }
    }
    if o.newest && o.oldest {
        return Err(Fail::Usage(String::new()));
    }
    let ops = g.operands();
    if ops.len() > 1 {
        return Err(Fail::Try("only one pattern can be provided".to_string(), 2));
    }
    let pattern = ops.into_iter().next();
    if pattern.is_none() && !criteria {
        return Err(Fail::Try("no matching criteria specified".to_string(), 2));
    }
    if o.logpidfile && o.pidfile.is_none() {
        return Err(Fail::Try("-L without -F makes no sense".to_string(), 2));
    }
    Ok((o, pattern))
}

/// Pid de um arquivo de pid: espaços e linhas em branco na frente, número positivo, lixo depois
/// aceito. Com `-L`, o arquivo tem que estar travado por outro.
fn read_pidfile(path: &[u8], need_lock: bool) -> Option<Pid> {
    let file = ul_misc::util::io::File::open_with(path, OFlags::RDONLY, 0).ok()?;
    if need_lock {
        let lk = sysabi::FileLock { kind: sysabi::LockKind::Write, start: 0, len: 0 };
        match sys::current().ofd_getlk(file.fd(), lk) {
            Ok(Some(_)) => {}
            _ => return None,
        }
    }
    let data = sys::read_file(path).ok()?;
    let text = String::from_utf8_lossy(&data);
    let t = text.trim_start();
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    let v: i64 = digits.parse().ok()?;
    (v > 0 && v <= i64::from(i32::MAX)).then_some(v as Pid)
}

fn ns_ids(pid: Pid) -> [Option<Vec<u8>>; 6] {
    let sysc = sys::current();
    NS_TYPES.map(|t| sysc.readlinkat(sysabi::Fd::CWD, format!("/proc/{pid}/ns/{t}").as_bytes()).ok())
}

fn cgroups_of(pid: Pid) -> Vec<String> {
    match procfs::read(&format!("/proc/{pid}/cgroup")) {
        Some(d) => String::from_utf8_lossy(&d)
            .lines()
            .filter_map(|l| l.splitn(3, ':').nth(2).map(str::to_string))
            .collect(),
        None => Vec::new(),
    }
}

fn run(args: &[OsString], mode: Mode) -> i32 {
    let prog = mode.name();
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let mut names = Names::new();
    let (o, pattern) = match parse_opts(mode, &argv, &argv0, &mut names) {
        Ok(v) => v,
        Err(Fail::Try(msg, code)) => {
            io::eprint(format!("{prog}: {msg}\nTry `{prog} --help' for more information.\n"));
            return code;
        }
        Err(Fail::Plain(msg, code)) => {
            common::warn(prog, &msg);
            return code;
        }
        Err(Fail::Usage(msg)) => {
            io::eprint(format!("{msg}{}", usage(mode)));
            return 2;
        }
        Err(Fail::Silent(code)) => return code,
    };
    let pidfile_pid = match &o.pidfile {
        Some(path) => match read_pidfile(path, o.logpidfile) {
            Some(p) => Some(p),
            None => {
                io::eprint(format!("{prog}: pidfile not valid\nTry `{prog} --help' for more information.\n"));
                return 1;
            }
        },
        None => None,
    };
    let re = match &pattern {
        Some(p) => {
            let mut text = p.clone();
            if o.exact {
                let mut t = b"^(".to_vec();
                t.extend_from_slice(p);
                t.extend_from_slice(b")$");
                text = t;
            }
            match matcher::compile(&text, o.icase) {
                Ok(m) => Some(m),
                Err(e) => {
                    common::warn(prog, &format!("regex error: {e}"));
                    return 2;
                }
            }
        }
        None => None,
    };
    let sysc = sys::current();
    let me = sysc.getpid();
    let snap = procfs::scan(Want { cmdline: true, ..Want::default() });
    let mut ancestors = Vec::new();
    if o.ignore_ancestors {
        let mut cur = procfs::self_stat().ppid;
        let mut guard = 0;
        while cur > 0 && guard < 4096 {
            ancestors.push(cur);
            guard += 1;
            cur = match snap.procs.iter().find(|p| p.pid() == cur) {
                Some(p) if p.stat.ppid != cur => p.stat.ppid,
                _ => break,
            };
        }
    }
    let ns_ref = o.ns_pid.map(ns_ids);
    let uptime = procfs::uptime_clock();
    let mut units: Vec<Proc> = Vec::new();
    for p in &snap.procs {
        if o.lightweight {
            units.extend(procfs::threads(p, Want { cmdline: true, ..Want::default() }));
        } else {
            units.push(p.clone());
        }
    }
    let mut matches: Vec<&Proc> = Vec::new();
    for (n, p) in units.iter().enumerate() {
        if n % 64 == 0 {
            sys::checkpoint();
        }
        if p.tid == me || p.tgid == me || ancestors.contains(&p.tgid) {
            continue;
        }
        let mut ok = true;
        if let Some(l) = &o.euids {
            ok &= l.contains(&p.euid());
        }
        if let Some(l) = &o.ruids {
            ok &= l.contains(&p.ruid());
        }
        if let Some(l) = &o.rgids {
            ok &= l.contains(&p.rgid());
        }
        if let Some(l) = &o.pgroups {
            ok &= l.contains(&i64::from(p.stat.pgrp));
        }
        if let Some(l) = &o.parents {
            ok &= l.contains(&i64::from(p.stat.ppid));
        }
        if let Some(l) = &o.sessions {
            ok &= l.contains(&i64::from(p.stat.session));
        }
        if let Some(l) = &o.ttys {
            let name = common::tty_name(p.stat.tty_nr, p.pid()).unwrap_or_else(|| "?".to_string());
            ok &= l.contains(&name);
        }
        if let Some(rs) = &o.runstates {
            ok &= rs.contains(p.stat.state);
        }
        if let Some(secs) = o.older {
            ok &= p.age(uptime) >= secs;
        }
        if let Some(pid) = pidfile_pid {
            ok &= p.tgid == pid;
        }
        if let Some(cg) = &o.cgroups {
            let mine = cgroups_of(p.pid());
            ok &= mine.iter().any(|c| cg.contains(c));
        }
        if let Some(reference) = &ns_ref {
            let theirs = ns_ids(p.pid());
            let types: Vec<usize> = match &o.nslist {
                Some(v) => v.clone(),
                None => (0..NS_TYPES.len()).collect(),
            };
            ok &= types.iter().all(|t| reference[*t] == theirs[*t]);
        }
        if let Some(m) = &re {
            let target = if o.full { p.cmdline_string() } else { p.comm().to_vec() };
            ok &= m.is_match(&target);
        }
        if mode == Mode::Pkill && o.require_handler {
            let cgt = p.status.as_ref().and_then(|s| s.hex("SigCgt")).unwrap_or(0);
            ok &= o.signal > 0 && cgt & (1u64 << (o.signal - 1)) != 0;
        }
        if ok != o.inverse {
            matches.push(p);
        }
    }
    if o.newest || o.oldest {
        let mut best: Option<&Proc> = None;
        for p in &matches {
            let better = match best {
                None => true,
                Some(b) if o.newest => p.stat.starttime > b.stat.starttime,
                Some(b) => p.stat.starttime < b.stat.starttime,
            };
            if better {
                best = Some(p);
            }
        }
        matches = best.into_iter().collect();
    }
    if matches.is_empty()
        && let Some(p) = &pattern
            && !o.full && p.len() > 15 {
                io::eprint(format!(
                    "{prog}: pattern that searches for process name longer than 15 characters will result in zero matches\nTry `{prog} -f' option to match against the complete command line.\n"
                ));
            }
    match mode {
        Mode::Pgrep => {
            if o.count {
                out(format!("{}\n", matches.len()));
            } else if !matches.is_empty() {
                let mut line = Vec::new();
                for (i, p) in matches.iter().enumerate() {
                    if i > 0 {
                        line.extend_from_slice(&o.delim);
                    }
                    line.extend_from_slice(p.tid.to_string().as_bytes());
                    if o.list_full {
                        line.push(b' ');
                        line.extend(p.cmdline_string());
                    } else if o.list_name {
                        line.push(b' ');
                        line.extend_from_slice(p.comm());
                    }
                }
                line.push(b'\n');
                out(line);
            }
            i32::from(matches.is_empty())
        }
        Mode::Pkill => {
            let mut killed = 0;
            for p in &matches {
                match sysc.kill(KillTarget::Pid(p.tid), Signal(o.signal)) {
                    Ok(()) => {
                        killed += 1;
                        if o.echo {
                            let mut l = p.comm().to_vec();
                            l.extend_from_slice(format!(" killed (pid {})\n", p.tid).as_bytes());
                            out(l);
                        }
                    }
                    Err(Errno::ESRCH) => {}
                    Err(e) => common::warn(prog, &format!("killing pid {} failed: {}", p.tid, e.message())),
                }
            }
            if o.count {
                out(format!("{}\n", matches.len()));
            }
            i32::from(killed == 0)
        }
        Mode::Pidwait => {
            if o.count {
                out(format!("{}\n", matches.len()));
            }
            if o.echo {
                for p in &matches {
                    let mut l = b"waiting for ".to_vec();
                    l.extend_from_slice(p.comm());
                    l.extend_from_slice(format!(" (pid {})\n", p.tid).as_bytes());
                    out(l);
                }
            }
            let _ = io::flush_stdout();
            let mut pending: Vec<Pid> = matches.iter().map(|p| p.tid).collect();
            while !pending.is_empty() {
                pending.retain(|pid| alive(*pid));
                if pending.is_empty() {
                    break;
                }
                if sysc.nanosleep(Duration::from_millis(100)).is_err() {
                    sys::checkpoint();
                }
            }
            i32::from(matches.is_empty())
        }
    }
}

/// O processo ainda existe e não virou zumbi?
fn alive(pid: Pid) -> bool {
    let sysc = sys::current();
    if sysc.kill(KillTarget::Pid(pid), Signal(0)) == Err(Errno::ESRCH) {
        return false;
    }
    if let Some(st) = procfs::read(&format!("/proc/{pid}/stat")).and_then(|d| procfs::parse_stat(&d)) {
        return st.state != 'Z' && st.state != 'X';
    }
    match sysc.list_processes().into_iter().find(|p| p.pid == pid) {
        Some(info) => info.state != 'Z',
        None => false,
    }
}
