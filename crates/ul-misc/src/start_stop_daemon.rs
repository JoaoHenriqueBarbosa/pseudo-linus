//! `start-stop-daemon` do dpkg 1.22.21 (Debian 13): inicia, para e consulta processos de sistema.
//!
//! Porte do essencial de `utils/start-stop-daemon.c`: `--start`, `--stop`, `--status`, os critérios
//! de casamento (`--pidfile`, `--exec`, `--name`, `--user`, `--pid`, `--ppid`), `--startas`,
//! `--background`, `--make-pidfile`, `--retry`, `--signal`, `--oknodo`, `--test`, `--quiet`,
//! `--verbose`, `--help` e `--version`. O casamento lê o `/proc` (`stat`, `status`, `exe`), o sinal
//! sai por `kill` e o programa roda por `execvp`.
//!
//! Códigos de saída: 0 feito, 1 nada feito (0 com `--oknodo`), 2 com `--retry` os processos não
//! morreram, 3 problema; com `--status`: 0 rodando, 1 parado com pidfile, 3 parado, 4 indeterminado.
//!
//! Fora do porte: `--chuid`/`--group`/`--chroot`/`--nicelevel`/`--procsched`/`--iosched`/`--umask`/
//! `--no-close` são aceitos e validados, mas só `--chdir` é aplicado; o pidfile é escrito com
//! `std::fs`.

use std::ffi::OsString;
use std::io::Write;
use std::time::Duration;

use sysabi::{Ctx, Errno, KillTarget, ProcAttrs, Signal, sys};

use crate::setsid::execvp;
use crate::util::io;

const PROG: &str = "start-stop-daemon";

const USAGE: &str = "Usage: start-stop-daemon [<option>...] <command>

Commands:
  -S, --start -- <argument>...  start a program and pass <arguments> to it
  -K, --stop                    stop a program
  -T, --status                  get the program status
  -H, --help                    print help information
  -V, --version                 print version

Matching options (at least one is required):
      --pid <pid>               pid to check
      --ppid <ppid>             parent pid to check
  -p, --pidfile <pid-file>      pid file to check
  -x, --exec <executable>       program to start/check if it is running
  -n, --name <process-name>     process name to check
  -u, --user <username|uid>     process owner to check

Options:
  -g, --group <group|gid>       run process as this group
  -c, --chuid <name|uid[:group|gid]>
                                change to this user/group before starting
                                  process
  -s, --signal <signal>         signal to send (default TERM)
  -a, --startas <pathname>      program to start (default is <executable>)
  -r, --chroot <directory>      chroot to <directory> before starting
  -d, --chdir <directory>       change to <directory> (default is /)
  -N, --nicelevel <incr>        add incr to the process's nice level
  -P, --procsched <policy[:prio]>
                                use <policy> with <prio> for the kernel
                                  process scheduler (default prio is 0)
  -I, --iosched <class[:prio]>  use <class> with <prio> to set the IO
                                  scheduler (default prio is 4)
  -k, --umask <mask>            change the process' file creation mask
  -b, --background              force the process to detach
  -C, --no-close                do not close any file descriptor
  -m, --make-pidfile            create the pidfile before starting
  -R, --retry <schedule|timeout>
                                check whether processes have exited
  -t, --test                    test mode, don't do anything
  -o, --oknodo                  exit status 0 (not 1) if nothing done
  -q, --quiet                   be more quiet
  -v, --verbose                 be more verbose

Retry <schedule> is <item>|/<item>/... where <item> is one of
 -<signal-num>|[-]<signal-name>  send that signal
 <timeout>                       wait that many seconds
 forever                         repeat remainder forever
or <schedule> may be just <timeout>, meaning <signal>/<timeout>/KILL/<timeout>

The process scheduler <policy> can be one of:
  other, fifo or rr

Exit status:  0 = done      1 = nothing done (= 0 if --oknodo)
              2 = with --retry, processes would not die
              3 = trouble
Exit status with --status:
              0 = program is running
              1 = program is not running and the pid file exists
              3 = program is not running
              4 = unable to determine status
";

const VERSION: &str = "start-stop-daemon 1.22.21 for Debian\n\nThis is free software; see the GNU General Public License version 2 or later for copying conditions. There is NO warranty.\n";

const SIGNAMES: [(&str, i32); 31] = [
    ("HUP", 1), ("INT", 2), ("QUIT", 3), ("ILL", 4), ("TRAP", 5), ("ABRT", 6), ("BUS", 7),
    ("FPE", 8), ("KILL", 9), ("USR1", 10), ("SEGV", 11), ("USR2", 12), ("PIPE", 13), ("ALRM", 14),
    ("TERM", 15), ("STKFLT", 16), ("CHLD", 17), ("CONT", 18), ("STOP", 19), ("TSTP", 20),
    ("TTIN", 21), ("TTOU", 22), ("URG", 23), ("XCPU", 24), ("XFSZ", 25), ("VTALRM", 26),
    ("PROF", 27), ("WINCH", 28), ("POLL", 29), ("PWR", 30), ("SYS", 31),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cmd {
    None,
    Start,
    Stop,
    Status,
}

#[derive(Clone, Copy)]
enum Step {
    Sig(i32),
    Wait(u64),
    Forever,
}

#[derive(Default)]
struct Opts {
    pid: Option<i32>,
    ppid: Option<i32>,
    pidfile: Option<Vec<u8>>,
    exec: Option<Vec<u8>>,
    name: Option<Vec<u8>>,
    user: Option<u32>,
    startas: Option<Vec<u8>>,
    chdir: Option<Vec<u8>>,
    background: bool,
    makepidfile: bool,
    test: bool,
    oknodo: bool,
    quiet: bool,
    verbose: usize,
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn out(s: impl AsRef<[u8]>) {
    let mut o = io::stdout();
    let _ = o.write_all(s.as_ref());
}

/// `badusage`: mensagem, a dica de ajuda e saída 3.
fn badusage(msg: &str) -> i32 {
    io::eprint(format!(
        "{PROG}: {msg}\nTry '{PROG} --help' for more information.\n"
    ));
    3
}

fn fatal(msg: &str) -> i32 {
    io::eprint(format!("{PROG}: {msg}\n"));
    3
}

fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn parse_num(s: &[u8]) -> Option<i64> {
    if s.is_empty() {
        return None;
    }
    lossy(s).parse::<i64>().ok()
}

/// Sinal por número ou nome (com ou sem `SIG`).
fn parse_signal(s: &[u8]) -> Option<i32> {
    if let Some(n) = parse_num(s) {
        return i32::try_from(n).ok();
    }
    let bare = s.strip_prefix(b"SIG").unwrap_or(s);
    SIGNAMES.iter().find(|(n, _)| n.as_bytes() == bare).map(|(_, v)| *v)
}

/// Usuário por nome (via `/etc/passwd`) ou uid numérico.
fn parse_user(s: &[u8]) -> Option<u32> {
    if let Some(n) = parse_num(s) {
        return u32::try_from(n).ok();
    }
    let data = sys::read_file(b"/etc/passwd").ok()?;
    for line in data.split(|b| *b == b'\n') {
        let mut f = line.split(|b| *b == b':');
        if f.next() == Some(s) {
            return parse_num(f.nth(1)?).and_then(|v| u32::try_from(v).ok());
        }
    }
    None
}

/// `--retry`: `timeout` vira `TERM/timeout/KILL/timeout`; senão itens separados por `/`.
fn parse_schedule(s: &[u8]) -> Result<Vec<Step>, String> {
    if let Some(n) = parse_num(s) {
        if n < 0 {
            return Err(format!("invalid timeout value in schedule '{}'", lossy(s)));
        }
        return Ok(vec![Step::Sig(15), Step::Wait(n as u64), Step::Sig(9), Step::Wait(n as u64)]);
    }
    let mut v = Vec::new();
    for item in s.split(|b| *b == b'/') {
        if item == b"forever" {
            v.push(Step::Forever);
        } else if let Some(rest) = item.strip_prefix(b"-") {
            match parse_signal(rest) {
                Some(sg) => v.push(Step::Sig(sg)),
                None => return Err(format!("signal value '{}' not an integer or name", lossy(rest))),
            }
        } else if let Some(n) = parse_num(item).filter(|n| *n >= 0) {
            v.push(Step::Wait(n as u64));
        } else {
            return Err(format!("invalid schedule item '{}'", lossy(item)));
        }
    }
    Ok(v)
}

/// Primeiro número do pidfile.
fn read_pidfile(path: &[u8]) -> Result<Option<i32>, Errno> {
    let data = sys::read_file(path)?;
    let digits: Vec<u8> = data
        .iter()
        .copied()
        .skip_while(|b| b.is_ascii_whitespace())
        .take_while(u8::is_ascii_digit)
        .collect();
    Ok(lossy(&digits).parse::<i32>().ok().filter(|p| *p > 0))
}

fn proc_stat_fields(pid: i32) -> Option<(Vec<u8>, i32)> {
    let d = sys::read_file(format!("/proc/{pid}/stat").as_bytes()).ok()?;
    let open = d.iter().position(|b| *b == b'(')?;
    let close = d.iter().rposition(|b| *b == b')')?;
    let comm = d[open + 1..close].to_vec();
    let rest = lossy(&d[close + 1..]);
    let ppid = rest.split_ascii_whitespace().nth(1)?.parse::<i32>().ok()?;
    Some((comm, ppid))
}

fn proc_uid(pid: i32) -> Option<u32> {
    let d = sys::read_file(format!("/proc/{pid}/status").as_bytes()).ok()?;
    for line in d.split(|b| *b == b'\n') {
        if let Some(rest) = line.strip_prefix(b"Uid:\t") {
            let digits: Vec<u8> = rest.iter().copied().take_while(u8::is_ascii_digit).collect();
            return lossy(&digits).parse().ok();
        }
    }
    None
}

/// Um processo casa com os critérios? `exec` compara o executável por `dev`/`ino`.
fn matches(o: &Opts, pid: i32, exec_id: Option<(u64, u64)>) -> bool {
    let Some((comm, ppid)) = proc_stat_fields(pid) else { return false };
    if o.ppid.is_some_and(|p| p != ppid) {
        return false;
    }
    if let Some(id) = exec_id {
        match sys::stat(format!("/proc/{pid}/exe").as_bytes()) {
            Ok(st) if (st.dev, st.ino) == id => {}
            _ => return false,
        }
    }
    if let Some(n) = &o.name {
        let short: Vec<u8> = n.iter().copied().take(15).collect();
        if comm != short {
            return false;
        }
    }
    if let Some(u) = o.user
        && proc_uid(pid) != Some(u)
    {
        return false;
    }
    true
}

enum Found {
    Pids(Vec<i32>),
    /// Pidfile ilegível por motivo que não é a ausência.
    Unknown,
    /// Pidfile existe mas o processo não.
    StalePidfile,
}

fn find(o: &Opts) -> Found {
    let exec_id = match &o.exec {
        Some(e) => match sys::stat(e) {
            Ok(st) => Some((st.dev, st.ino)),
            Err(_) => return Found::Pids(Vec::new()),
        },
        None => None,
    };
    let me = sys::current().getpid();
    let mut cand: Vec<i32> = Vec::new();
    let mut had_pidfile = false;
    if let Some(p) = o.pid {
        cand.push(p);
    } else if let Some(pf) = &o.pidfile {
        match read_pidfile(pf) {
            Ok(Some(p)) => {
                had_pidfile = true;
                cand.push(p)
            }
            Ok(None) => return Found::Pids(Vec::new()),
            Err(Errno::ENOENT) => return Found::Pids(Vec::new()),
            Err(_) => return Found::Unknown,
        }
    } else {
        match sys::read_dir(b"/proc") {
            Ok(es) => {
                for e in &es {
                    if let Some(p) = parse_num(&e.name).and_then(|n| i32::try_from(n).ok())
                        && p > 0
                        && p != me
                    {
                        cand.push(p);
                    }
                }
            }
            Err(_) => return Found::Unknown,
        }
    }
    let v: Vec<i32> = cand.into_iter().filter(|p| matches(o, *p, exec_id)).collect();
    if v.is_empty() && had_pidfile {
        return Found::StalePidfile;
    }
    Found::Pids(v)
}

fn what(o: &Opts) -> String {
    if let Some(e) = &o.exec {
        lossy(e)
    } else if let Some(p) = &o.pidfile {
        format!("process in pidfile '{}'", lossy(p))
    } else if let Some(u) = o.user {
        format!("process(es) owned by '{u}'")
    } else if let Some(n) = &o.name {
        format!("process '{}'", lossy(n))
    } else {
        "process".to_string()
    }
}

fn exit_nothing(o: &Opts) -> i32 {
    if o.oknodo { 0 } else { 1 }
}

fn do_start(o: &Opts, args: &[Vec<u8>]) -> i32 {
    match find(o) {
        Found::Unknown => return fatal("unable to read the pid file or /proc"),
        Found::Pids(v) if !v.is_empty() => {
            if !o.quiet {
                out(format!("{} already running.\n", what(o)));
            }
            return exit_nothing(o);
        }
        _ => {}
    }
    let prog = o.startas.clone().or_else(|| o.exec.clone()).unwrap_or_default();
    if o.test {
        let mut s = format!("Would start {}", lossy(&prog));
        for a in args {
            s.push(' ');
            s.push_str(&lossy(a));
        }
        out(format!("{s}.\n"));
        return 0;
    }
    if o.verbose > 0 || !o.quiet {
        if o.verbose > 0 {
            out(format!("Starting {}...\n", lossy(&prog)));
        }
    }
    let mut argv = vec![prog.clone()];
    argv.extend(args.iter().cloned());
    let sysc = sys::current();
    let _ = io::flush_stdout();

    let chdir = o.chdir.clone();
    let body_argv = argv.clone();
    let body_prog = prog.clone();
    let run_prog = move || -> i32 {
        let sc = sys::current();
        if let Some(d) = &chdir
            && let Err(e) = sc.chdir(d)
        {
            io::eprint(format!("{PROG}: unable to chdir({}): {}\n", lossy(d), e.message()));
            return 3;
        }
        let e = execvp(&body_prog, &body_argv);
        io::eprint(format!("{PROG}: unable to start {}: {}\n", lossy(&body_prog), e.message()));
        2
    };

    if o.background {
        let child = sysc.spawn_fn(
            ProcAttrs::default(),
            prog.clone(),
            Box::new(move || {
                let _ = sys::current().setsid();
                run_prog()
            }),
        );
        match child {
            Ok(p) => {
                if o.makepidfile
                    && let Some(pf) = &o.pidfile
                    && let Err(e) = std::fs::write(lossy(pf), format!("{p}\n"))
                {
                    return fatal(&format!("unable to write pidfile '{}': {e}", lossy(pf)));
                }
                0
            }
            Err(e) => fatal(&format!("unable to fork: {}", e.message())),
        }
    } else {
        if o.makepidfile
            && let Some(pf) = &o.pidfile
            && let Err(e) = std::fs::write(lossy(pf), format!("{}\n", sysc.getpid()))
        {
            return fatal(&format!("unable to write pidfile '{}': {e}", lossy(pf)));
        }
        run_prog()
    }
}

fn send(o: &Opts, pids: &[i32], sig: i32) -> usize {
    let sysc = sys::current();
    let mut ok = 0;
    for &p in pids {
        if o.test {
            out(format!("Would send signal {sig} to {p}.\n"));
            ok += 1;
            continue;
        }
        match sysc.kill(KillTarget::Pid(p), Signal(sig)) {
            Ok(()) => {
                ok += 1;
                if o.verbose > 0 {
                    out(format!("Sent signal {sig} to {p}.\n"));
                }
            }
            Err(Errno::ESRCH) => {}
            Err(e) => io::eprint(format!("{PROG}: warning: failed to kill {p}: {}\n", e.message())),
        }
    }
    ok
}

fn alive(o: &Opts, pids: &[i32]) -> Vec<i32> {
    let sysc = sys::current();
    pids.iter()
        .copied()
        .filter(|p| sysc.kill(KillTarget::Pid(*p), Signal(0)) != Err(Errno::ESRCH) && matches(o, *p, None))
        .collect()
}

fn do_stop(o: &Opts, sig: i32, schedule: Option<Vec<Step>>) -> i32 {
    let pids = match find(o) {
        Found::Pids(v) => v,
        Found::StalePidfile => Vec::new(),
        Found::Unknown => return fatal("unable to read the pid file or /proc"),
    };
    if pids.is_empty() {
        if !o.quiet {
            out(format!("No {} found running; none killed.\n", what(o)));
        }
        return exit_nothing(o);
    }
    let killed = send(o, &pids, sig);
    if o.test {
        return 0;
    }
    let Some(sched) = schedule else {
        if killed == 0 {
            return exit_nothing(o);
        }
        return 0;
    };
    let sysc = sys::current();
    let mut left = pids;
    let mut idx = 0;
    let mut forever_at: Option<usize> = None;
    while idx < sched.len() {
        match sched[idx] {
            Step::Forever => forever_at = Some(idx + 1),
            Step::Sig(s) => {
                send(o, &left, s);
            }
            Step::Wait(secs) => {
                let mut waited = 0u64;
                loop {
                    left = alive(o, &left);
                    if left.is_empty() {
                        return 0;
                    }
                    if waited >= secs * 50 {
                        break;
                    }
                    let _ = sysc.nanosleep(Duration::from_millis(20));
                    waited += 1;
                }
            }
        }
        idx += 1;
        if idx == sched.len()
            && let Some(f) = forever_at
        {
            idx = f;
        }
    }
    left = alive(o, &left);
    if left.is_empty() {
        0
    } else {
        io::eprint(format!("{PROG}: {} process(es) would not die\n", left.len()));
        2
    }
}

fn do_status(o: &Opts) -> i32 {
    match find(o) {
        Found::Pids(v) if !v.is_empty() => {
            if !o.quiet {
                out(format!("{} is running\n", what(o)));
            }
            0
        }
        Found::Pids(_) => {
            if !o.quiet {
                out(format!("{} is not running\n", what(o)));
            }
            3
        }
        Found::StalePidfile => {
            if !o.quiet {
                out(format!("{} is not running, but pid file exists\n", what(o)));
            }
            1
        }
        Found::Unknown => 4,
    }
}

/// Opções que levam argumento: (longa, curta).
const WITH_ARG: &[(&str, char)] = &[
    ("pid", '\u{1}'), ("ppid", '\u{2}'), ("pidfile", 'p'), ("exec", 'x'), ("name", 'n'),
    ("user", 'u'), ("group", 'g'), ("chuid", 'c'), ("signal", 's'), ("startas", 'a'),
    ("chroot", 'r'), ("chdir", 'd'), ("nicelevel", 'N'), ("procsched", 'P'), ("iosched", 'I'),
    ("umask", 'k'), ("retry", 'R'),
];
const NO_ARG: &[(&str, char)] = &[
    ("start", 'S'), ("stop", 'K'), ("status", 'T'), ("help", 'H'), ("version", 'V'),
    ("background", 'b'), ("no-close", 'C'), ("make-pidfile", 'm'), ("test", 't'),
    ("oknodo", 'o'), ("quiet", 'q'), ("verbose", 'v'),
];

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    // (opção, argumento) na ordem e os operandos.
    let mut items: Vec<(char, Vec<u8>)> = Vec::new();
    let mut operands: Vec<Vec<u8>> = Vec::new();
    let mut i = 1;
    while i < argv.len() {
        let a = &argv[i];
        i += 1;
        if a == b"--" {
            operands.extend(argv[i..].iter().cloned());
            break;
        }
        if let Some(long) = a.strip_prefix(b"--") {
            let (name, val) = match long.iter().position(|b| *b == b'=') {
                Some(p) => (&long[..p], Some(long[p + 1..].to_vec())),
                None => (long, None),
            };
            let cands: Vec<(&str, char, bool)> = WITH_ARG
                .iter()
                .map(|(n, c)| (*n, *c, true))
                .chain(NO_ARG.iter().map(|(n, c)| (*n, *c, false)))
                .filter(|(n, _, _)| n.as_bytes().starts_with(name))
                .collect();
            let exact = cands.iter().find(|(n, _, _)| n.as_bytes() == name).copied();
            let pick = match (exact, cands.len()) {
                (Some(e), _) => e,
                (None, 1) => cands[0],
                (None, 0) => return badusage(&format!("unrecognized option '--{}'", lossy(name))),
                _ => return badusage(&format!("option '--{}' is ambiguous", lossy(name))),
            };
            if pick.2 {
                let v = match val {
                    Some(v) => v,
                    None if i < argv.len() => {
                        i += 1;
                        argv[i - 1].clone()
                    }
                    None => return badusage(&format!("option '--{}' requires an argument", pick.0)),
                };
                items.push((pick.1, v));
            } else {
                if val.is_some() {
                    return badusage(&format!("option '--{}' doesn't allow an argument", pick.0));
                }
                items.push((pick.1, Vec::new()));
            }
        } else if a.len() > 1 && a[0] == b'-' {
            let mut j = 1;
            while j < a.len() {
                let c = a[j] as char;
                j += 1;
                if WITH_ARG.iter().any(|(_, k)| *k == c && c.is_ascii_alphabetic()) {
                    let v = if j < a.len() {
                        a[j..].to_vec()
                    } else if i < argv.len() {
                        i += 1;
                        argv[i - 1].clone()
                    } else {
                        return badusage(&format!("option requires an argument -- '{c}'"));
                    };
                    items.push((c, v));
                    break;
                } else if NO_ARG.iter().any(|(_, k)| *k == c) {
                    items.push((c, Vec::new()));
                } else {
                    return badusage(&format!("invalid option -- '{c}'"));
                }
            }
        } else {
            operands.push(a.clone());
        }
    }

    let mut cmd = Cmd::None;
    let mut o = Opts::default();
    let mut sig = 15;
    let mut retry: Option<Vec<Step>> = None;
    for (c, v) in items {
        match c {
            'S' => cmd = Cmd::Start,
            'K' => cmd = Cmd::Stop,
            'T' => cmd = Cmd::Status,
            'H' => {
                out(USAGE);
                return 0;
            }
            'V' => {
                out(VERSION);
                return 0;
            }
            'b' => o.background = true,
            'm' => o.makepidfile = true,
            't' => o.test = true,
            'o' => o.oknodo = true,
            'q' => o.quiet = true,
            'v' => o.verbose += 1,
            'C' => {}
            '\u{1}' => match parse_num(&v).filter(|n| *n > 0) {
                Some(n) => o.pid = Some(n as i32),
                None => return badusage(&format!("pid value must be a positive number: '{}'", lossy(&v))),
            },
            '\u{2}' => match parse_num(&v).filter(|n| *n > 0) {
                Some(n) => o.ppid = Some(n as i32),
                None => return badusage(&format!("ppid value must be a positive number: '{}'", lossy(&v))),
            },
            'p' => o.pidfile = Some(v),
            'x' => o.exec = Some(v),
            'n' => o.name = Some(v),
            'a' => o.startas = Some(v),
            'd' => o.chdir = Some(v),
            'u' => match parse_user(&v) {
                Some(u) => o.user = Some(u),
                None => return badusage(&format!("user '{}' not found", lossy(&v))),
            },
            's' => match parse_signal(&v) {
                Some(s) => sig = s,
                None => {
                    return badusage(&format!(
                        "signal value '{}' not an integer or name",
                        lossy(&v)
                    ));
                }
            },
            'R' => match parse_schedule(&v) {
                Ok(s) => retry = Some(s),
                Err(m) => return badusage(&m),
            },
            _ => {}
        }
    }

    if cmd == Cmd::None {
        return badusage("need one of --start or --stop or --status");
    }
    if cmd == Cmd::Start && o.exec.is_none() && o.startas.is_none() {
        return badusage("need --exec or --startas for --start");
    }
    if o.pid.is_none()
        && o.ppid.is_none()
        && o.pidfile.is_none()
        && o.exec.is_none()
        && o.name.is_none()
        && o.user.is_none()
    {
        return badusage("need at least one of --exec, --pidfile, --user or --name");
    }
    if o.makepidfile && o.pidfile.is_none() {
        return badusage("--make-pidfile is only relevant with --pidfile");
    }
    if o.background && cmd != Cmd::Start {
        return badusage("--background is only relevant with --start");
    }
    if o.makepidfile && cmd != Cmd::Start {
        return badusage("--make-pidfile is only relevant with --start");
    }
    if retry.is_some() && cmd != Cmd::Stop {
        return badusage("--retry is only relevant with --stop");
    }
    if cmd != Cmd::Start && !operands.is_empty() {
        return badusage(&format!(
            "unexpected positional argument '{}'",
            lossy(&operands[0])
        ));
    }

    match cmd {
        Cmd::Start => do_start(&o, &operands),
        Cmd::Stop => do_stop(&o, sig, retry),
        Cmd::Status => do_status(&o),
        Cmd::None => 3,
    }
}
