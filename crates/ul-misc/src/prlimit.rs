//! `prlimit` do util-linux 2.41: mostra e ajusta os limites de recursos de um processo.
//!
//! Porte do `sys-utils/prlimit.c`. Cada opção de recurso (`--core`, `--nofile`, ...) pode levar
//! `=soft:hard`; sem valor o recurso entra na tabela de saída, com valor ele é alterado. Sem nenhuma
//! opção de recurso mostra todos. Com um comando, ajusta os limites do próprio processo e faz `execvp`.
//!
//! O `sysabi` só tem `getrlimit`/`setrlimit` do processo corrente, então `-p` com outro pid devolve
//! ESRCH (o processo não existe) ou EPERM (existe, mas o sandbox não alcança os limites dele).

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Errno, RLIM_INFINITY, Resource, Rlimit, sys};

use crate::setsid::execvp;
use crate::util::getopt::{Getopt, HasArg, LongOpt};
use crate::util::io;
use crate::util::ul;

struct Desc {
    name: &'static str,
    desc: &'static str,
    units: &'static str,
    res: Resource,
    opt: u8,
}

/// Na ordem da tabela do original (a de `--output` sem recursos escolhidos).
const DESCS: &[Desc] = &[
    Desc { name: "AS", desc: "address space limit", units: "bytes", res: Resource::As, opt: b'v' },
    Desc { name: "CORE", desc: "max core file size", units: "bytes", res: Resource::Core, opt: b'c' },
    Desc { name: "CPU", desc: "CPU time", units: "seconds", res: Resource::Cpu, opt: b't' },
    Desc { name: "DATA", desc: "max data size", units: "bytes", res: Resource::Data, opt: b'd' },
    Desc { name: "FSIZE", desc: "max file size", units: "bytes", res: Resource::Fsize, opt: b'f' },
    Desc { name: "LOCKS", desc: "max number of file locks held", units: "locks", res: Resource::Locks, opt: b'x' },
    Desc { name: "MEMLOCK", desc: "max locked-in-memory address space", units: "bytes", res: Resource::Memlock, opt: b'l' },
    Desc { name: "MSGQUEUE", desc: "max bytes in POSIX mqueues", units: "bytes", res: Resource::Msgqueue, opt: b'q' },
    Desc { name: "NICE", desc: "max nice prio allowed to raise", units: "", res: Resource::Nice, opt: b'e' },
    Desc { name: "NOFILE", desc: "max number of open files", units: "files", res: Resource::Nofile, opt: b'n' },
    Desc { name: "NPROC", desc: "max number of processes", units: "processes", res: Resource::Nproc, opt: b'u' },
    Desc { name: "RSS", desc: "max resident set size", units: "pages", res: Resource::Rss, opt: b'm' },
    Desc { name: "RTPRIO", desc: "max real-time priority", units: "", res: Resource::Rtprio, opt: b'r' },
    Desc { name: "RTTIME", desc: "timeout for real-time tasks", units: "microsecs", res: Resource::Rttime, opt: b'y' },
    Desc { name: "SIGPENDING", desc: "max number of pending signals", units: "signals", res: Resource::Sigpending, opt: b'i' },
    Desc { name: "STACK", desc: "max stack size", units: "bytes", res: Resource::Stack, opt: b's' },
];

#[derive(Copy, Clone, PartialEq, Eq)]
enum Col {
    Description,
    Resource,
    Soft,
    Hard,
    Units,
}

const COLS: &[(Col, &str, &str, bool)] = &[
    (Col::Description, "DESCRIPTION", "resource description", false),
    (Col::Resource, "RESOURCE", "resource name", false),
    (Col::Soft, "SOFT", "soft limit", true),
    (Col::Hard, "HARD", "hard limit (ceiling)", true),
    (Col::Units, "UNITS", "units", false),
];

const DEFAULT_COLS: &[Col] = &[Col::Resource, Col::Description, Col::Soft, Col::Hard, Col::Units];

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn usage(short: &str) -> String {
    let mut s = format!(
        "
Usage:
 {short} [options] [-p PID]
 {short} [options] COMMAND

Show or change the resource limits of a process.

General Options:
 -p, --pid <pid>        process id
 -o, --output <list>    define which output columns to use
     --noheadings       don't print headings
     --raw              use the raw output format
     --verbose          verbose output

 -h, --help             display this help
 -V, --version          display version

Resources Options:
 -c, --core             maximum size of core files created
 -d, --data             maximum size of a process's data segment
 -e, --nice             maximum nice priority allowed to raise
 -f, --fsize            maximum size of files written by the process
 -i, --sigpending       maximum number of pending signals
 -l, --memlock          maximum size a process may lock into memory
 -m, --rss              maximum resident set size
 -n, --nofile           maximum number of open files
 -q, --msgqueue         maximum bytes in POSIX message queues
 -r, --rtprio           maximum real-time scheduling priority
 -s, --stack            maximum stack size
 -t, --cpu              maximum amount of CPU time in seconds
 -u, --nproc            maximum number of user processes
 -v, --as               size of virtual memory
 -x, --locks            maximum number of file locks
 -y, --rttime           CPU time in microseconds a process scheduled
                        under real-time scheduling

Available columns (for --output):
"
    );
    for (_, name, help, _) in COLS {
        s.push_str(&format!(" {name:>11}  {help}\n"));
    }
    s.push_str(&format!("\nFor more details see {short}(1).\n"));
    s
}

fn fmt_limit(v: u64) -> String {
    if v == RLIM_INFINITY { "unlimited".to_string() } else { v.to_string() }
}

/// Um valor do `soft:hard`: número decimal ou `unlimited`.
fn parse_value(s: &[u8]) -> Option<u64> {
    if s == b"unlimited" {
        return Some(RLIM_INFINITY);
    }
    if s.is_empty() || !s.iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    std::str::from_utf8(s).ok()?.parse::<u64>().ok()
}

/// `soft`, `soft:`, `:hard` ou `soft:hard`; um valor só vale pros dois.
fn parse_range(s: &[u8]) -> Option<(Option<u64>, Option<u64>)> {
    match s.iter().position(|b| *b == b':') {
        None => {
            let v = parse_value(s)?;
            Some((Some(v), Some(v)))
        }
        Some(p) => {
            let (a, b) = (&s[..p], &s[p + 1..]);
            let soft = if a.is_empty() { None } else { Some(parse_value(a)?) };
            let hard = if b.is_empty() { None } else { Some(parse_value(b)?) };
            if soft.is_none() && hard.is_none() {
                return None;
            }
            Some((soft, hard))
        }
    }
}

struct Request {
    idx: usize,
    modify: Option<(Option<u64>, Option<u64>)>,
}

fn get_limit(pid: i32, res: Resource) -> Result<Rlimit, Errno> {
    let sys = sys::current();
    if pid == 0 || pid == i32::from(sys.getpid()) {
        return sys.getrlimit(res);
    }
    if sys.list_processes().iter().any(|p| i32::from(p.pid) == pid) {
        Err(Errno::EPERM)
    } else {
        Err(Errno::ESRCH)
    }
}

fn set_limit(pid: i32, res: Resource, lim: Rlimit) -> Result<(), Errno> {
    let sys = sys::current();
    if pid == 0 || pid == i32::from(sys.getpid()) {
        return sys.setrlimit(res, lim);
    }
    get_limit(pid, res).map(|_| ())
}

fn escape_raw(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        if c == ' ' || c == '\\' || (c as u32) < 0x20 || c as u32 == 0x7f {
            o.push_str(&format!("\\x{:02x}", c as u32));
        } else {
            o.push(c);
        }
    }
    o
}

fn print_table(cols: &[Col], rows: &[Vec<String>], noheadings: bool, raw: bool) {
    let names: Vec<(&str, bool)> = cols
        .iter()
        .map(|c| {
            let e = COLS.iter().find(|e| e.0 == *c).unwrap();
            (e.1, e.3)
        })
        .collect();
    let mut out = String::new();
    if raw {
        if !noheadings {
            let h: Vec<String> = names.iter().map(|n| n.0.to_string()).collect();
            out.push_str(&h.join(" "));
            out.push('\n');
        }
        for r in rows {
            let v: Vec<String> = r.iter().map(|c| escape_raw(c)).collect();
            out.push_str(&v.join(" "));
            out.push('\n');
        }
    } else {
        let mut widths: Vec<usize> = names
            .iter()
            .map(|n| if noheadings { 0 } else { n.0.chars().count() })
            .collect();
        for r in rows {
            for (i, c) in r.iter().enumerate() {
                widths[i] = widths[i].max(c.chars().count());
            }
        }
        let fmt_row = |cells: Vec<&str>| -> String {
            let mut line = String::new();
            for (i, c) in cells.iter().enumerate() {
                if i > 0 {
                    line.push(' ');
                }
                let n = c.chars().count();
                if names[i].1 {
                    line.push_str(&" ".repeat(widths[i] - n));
                    line.push_str(c);
                } else {
                    line.push_str(c);
                    if i + 1 < cells.len() {
                        line.push_str(&" ".repeat(widths[i] - n));
                    }
                }
            }
            line.trim_end().to_string() + "\n"
        };
        if !noheadings {
            out.push_str(&fmt_row(names.iter().map(|n| n.0).collect()));
        }
        for r in rows {
            out.push_str(&fmt_row(r.iter().map(|s| s.as_str()).collect()));
        }
    }
    let mut so = io::stdout();
    let _ = so.write_all(out.as_bytes());
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let short = ul::short_name(args);
    let argv0 = io::argv0(args);

    const OPT_NOHEADINGS: i32 = 256;
    const OPT_RAW: i32 = 257;
    const OPT_VERBOSE: i32 = 258;
    let longs = [
        LongOpt::new("pid", HasArg::Required, 'p' as i32),
        LongOpt::new("output", HasArg::Required, 'o' as i32),
        LongOpt::new("noheadings", HasArg::No, OPT_NOHEADINGS),
        LongOpt::new("raw", HasArg::No, OPT_RAW),
        LongOpt::new("verbose", HasArg::No, OPT_VERBOSE),
        LongOpt::new("help", HasArg::No, 'h' as i32),
        LongOpt::new("version", HasArg::No, 'V' as i32),
        LongOpt::new("core", HasArg::Optional, 'c' as i32),
        LongOpt::new("data", HasArg::Optional, 'd' as i32),
        LongOpt::new("nice", HasArg::Optional, 'e' as i32),
        LongOpt::new("fsize", HasArg::Optional, 'f' as i32),
        LongOpt::new("sigpending", HasArg::Optional, 'i' as i32),
        LongOpt::new("memlock", HasArg::Optional, 'l' as i32),
        LongOpt::new("rss", HasArg::Optional, 'm' as i32),
        LongOpt::new("nofile", HasArg::Optional, 'n' as i32),
        LongOpt::new("msgqueue", HasArg::Optional, 'q' as i32),
        LongOpt::new("rtprio", HasArg::Optional, 'r' as i32),
        LongOpt::new("stack", HasArg::Optional, 's' as i32),
        LongOpt::new("cpu", HasArg::Optional, 't' as i32),
        LongOpt::new("nproc", HasArg::Optional, 'u' as i32),
        LongOpt::new("as", HasArg::Optional, 'v' as i32),
        LongOpt::new("locks", HasArg::Optional, 'x' as i32),
        LongOpt::new("rttime", HasArg::Optional, 'y' as i32),
    ];

    let mut pid: Option<i32> = None;
    let mut cols: Vec<Col> = DEFAULT_COLS.to_vec();
    let mut noheadings = false;
    let mut raw = false;
    let mut verbose = false;
    let mut reqs: Vec<Request> = Vec::new();

    let mut g = Getopt::from_env(
        &argv[1.min(argv.len())..],
        "+c::d::e::f::i::l::m::n::q::r::s::t::u::v::x::y::p:o:hV",
        &longs,
    );
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                ul::errtryhelp(&short);
                return 1;
            }
        };
        match o.id {
            x if x == 'h' as i32 => {
                let mut out = io::stdout();
                let _ = out.write_all(usage(&short).as_bytes());
                return 0;
            }
            x if x == 'V' as i32 => {
                ul::print_version(&short);
                return 0;
            }
            x if x == 'p' as i32 => {
                let a = o.arg.clone().unwrap_or_default();
                let text = io::lossy(&a);
                match text.trim_start().parse::<i32>() {
                    Ok(p) => pid = Some(p),
                    Err(_) => {
                        ul::warnx(&short, format!("failed to parse PID: '{text}'"));
                        return 1;
                    }
                }
            }
            x if x == 'o' as i32 => {
                let a = o.arg.clone().unwrap_or_default();
                let (append, list) = match a.strip_prefix(b"+") {
                    Some(rest) => (true, rest.to_vec()),
                    None => (false, a),
                };
                let mut parsed = Vec::new();
                for name in list.split(|b| *b == b',') {
                    if name.is_empty() {
                        continue;
                    }
                    match COLS.iter().find(|c| c.1.as_bytes().eq_ignore_ascii_case(name)) {
                        Some(c) => parsed.push(c.0),
                        None => {
                            ul::warnx(&short, format!("unknown column: {}", io::lossy(name)));
                            return 1;
                        }
                    }
                }
                if append {
                    cols.extend(parsed);
                } else {
                    cols = parsed;
                }
            }
            OPT_NOHEADINGS => noheadings = true,
            OPT_RAW => raw = true,
            OPT_VERBOSE => verbose = true,
            id => {
                let key = id as u8;
                let idx = match DESCS.iter().position(|d| d.opt == key) {
                    Some(i) => i,
                    None => {
                        ul::errtryhelp(&short);
                        return 1;
                    }
                };
                let modify = match &o.arg {
                    None => None,
                    Some(a) => match parse_range(a) {
                        Some(r) => Some(r),
                        None => {
                            ul::warnx(&short, format!("failed to parse {} limit", DESCS[idx].name));
                            return 1;
                        }
                    },
                };
                reqs.push(Request { idx, modify });
            }
        }
    }
    let cmd: Vec<Vec<u8>> = g.operands();

    if !cmd.is_empty() && pid.is_some() {
        ul::warnx(&short, "--pid <pid> option and COMMAND are mutually exclusive");
        ul::errtryhelp(&short);
        return 1;
    }
    let target = pid.unwrap_or(0);

    if reqs.is_empty() {
        reqs = (0..DESCS.len()).map(|idx| Request { idx, modify: None }).collect();
    }

    let mut rows: Vec<Vec<String>> = Vec::new();
    for r in &reqs {
        let d = &DESCS[r.idx];
        let old = match get_limit(target, d.res) {
            Ok(l) => l,
            Err(e) => {
                ul::warn(&short, format!("failed to get old {} limit", d.name), e);
                return 1;
            }
        };
        if let Some((soft, hard)) = r.modify {
            let new = Rlimit { cur: soft.unwrap_or(old.cur), max: hard.unwrap_or(old.max) };
            if let Err(e) = set_limit(target, d.res, new) {
                ul::warn(&short, format!("failed to set the {} resource limit", d.name), e);
                return 1;
            }
            if verbose {
                let shown = if target == 0 { i32::from(sys::current().getpid()) } else { target };
                let mut out = io::stdout();
                let _ = out.write_all(
                    format!(
                        "New {} limit for pid {shown}: soft={} hard={}\n",
                        d.name,
                        fmt_limit(new.cur),
                        fmt_limit(new.max)
                    )
                    .as_bytes(),
                );
            }
        } else {
            let row: Vec<String> = cols
                .iter()
                .map(|c| match c {
                    Col::Description => d.desc.to_string(),
                    Col::Resource => d.name.to_string(),
                    Col::Soft => fmt_limit(old.cur),
                    Col::Hard => fmt_limit(old.max),
                    Col::Units => d.units.to_string(),
                })
                .collect();
            rows.push(row);
        }
    }

    if !rows.is_empty() {
        print_table(&cols, &rows, noheadings, raw);
    }

    if !cmd.is_empty() {
        let _ = io::flush_stdout();
        let e = execvp(&cmd[0], &cmd);
        ul::warn(&short, format!("failed to execute {}", io::lossy(&cmd[0])), e);
        return if e == Errno::ENOENT { 127 } else { 126 };
    }
    0
}
