//! `lsns` do util-linux 2.41: lista os namespaces do sistema.
//!
//! Porte do `sys-utils/lsns.c`. Varre `/proc/<pid>/ns/*`, lê o link `tipo:[inode]` de cada um e agrupa
//! por namespace: número de processos, o processo representante (o de menor pid, trocado pelo pai
//! quando o pai também está no namespace), usuário e comando. Imprime tabela, `-l`, `-r` (raw) ou
//! `-J` (JSON). Com `-H` lista também os namespaces persistentes (montagens `nsfs` do mountinfo).
//!
//! Limites do `sysabi`: as colunas `NETNSID`, `PNS` e `ONS` dependem de ioctl e netlink que o sandbox
//! não expõe e saem vazias, e `--tree` aceita `parent`, `owner` e `none` mas imprime sempre em lista.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Write;

use sysabi::{Fd, sys};

use crate::util::getopt::{Getopt, HasArg, LongOpt};
use crate::util::io;
use crate::util::ul;

#[derive(Copy, Clone, PartialEq, Eq)]
enum Col {
    Ns,
    Type,
    Path,
    Nprocs,
    Pid,
    Ppid,
    Command,
    Uid,
    User,
    Netnsid,
    Nsfs,
    Pns,
    Ons,
}

/// Coluna, nome, alinhada à direita, é número no JSON, descrição.
const COLS: &[(Col, &str, bool, bool, &str)] = &[
    (Col::Ns, "NS", true, true, "namespace identifier (inode number)"),
    (Col::Type, "TYPE", false, false, "kind of namespace"),
    (Col::Path, "PATH", false, false, "path to the namespace"),
    (Col::Nprocs, "NPROCS", true, true, "number of processes in the namespace"),
    (Col::Pid, "PID", true, true, "lowest PID in the namespace"),
    (Col::Ppid, "PPID", true, true, "PPID of the PID"),
    (Col::Command, "COMMAND", false, false, "command line of the PID"),
    (Col::Uid, "UID", true, true, "UID of the PID"),
    (Col::User, "USER", false, false, "username of the PID"),
    (Col::Netnsid, "NETNSID", true, true, "namespace ID as used by network subsystem"),
    (Col::Nsfs, "NSFS", false, false, "nsfs mountpoint (usually used network subsystem)"),
    (Col::Pns, "PNS", true, true, "parent namespace identifier (inode number)"),
    (Col::Ons, "ONS", true, true, "owner namespace identifier (inode number)"),
];

const DEFAULT_COLS: &[Col] = &[Col::Ns, Col::Type, Col::Nprocs, Col::Pid, Col::User, Col::Command];

const NS_TYPES: &[&str] = &["mnt", "net", "ipc", "user", "pid", "uts", "cgroup", "time"];

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn usage(short: &str) -> String {
    let mut s = format!(
        "
Usage:
 {short} [options] [<namespace>]

List system namespaces.

Options:
 -J, --json             use JSON output format
 -l, --list             use list format output
 -n, --noheadings       don't print headings
 -o, --output <list>    define which output columns to use
     --output-all       output all columns
 -p, --task <pid>       print process namespaces
 -r, --raw              use the raw output format
 -u, --notruncate       don't truncate text in columns
 -W, --nowrap           don't use multi-line representation
 -t, --type <name>      namespace type (mnt, net, ipc, user, pid, uts, cgroup, time)

 -T, --tree <rel>       use tree format (parent, owner, or none)
 -H, --persistent       list persistent namespaces

 -h, --help             display this help
 -V, --version          display version

Available output columns:
"
    );
    for (_, name, _, _, help) in COLS {
        s.push_str(&format!(" {name:>11}  {help}\n"));
    }
    s.push_str(&format!("\nFor more details see {short}(8).\n"));
    s
}

#[derive(Default, Clone)]
struct Ns {
    ino: u64,
    ty: String,
    path: String,
    nprocs: u64,
    pid: Option<i32>,
    ppid: Option<i32>,
    uid: Option<u32>,
    cmd: String,
    nsfs: Vec<String>,
}

fn parse_link(link: &[u8]) -> Option<(String, u64)> {
    let s = std::str::from_utf8(link).ok()?;
    let (ty, rest) = s.split_once(":[")?;
    let ino = rest.strip_suffix(']')?.parse::<u64>().ok()?;
    Some((ty.to_string(), ino))
}

fn passwd_name(uid: u32) -> Option<String> {
    let data = io::read_path(b"/etc/passwd").ok()?;
    for line in data.split(|b| *b == b'\n') {
        let mut f = line.split(|b| *b == b':');
        let name = f.next()?;
        let _ = f.next();
        let u = f.next().and_then(|u| std::str::from_utf8(u).ok()?.parse::<u32>().ok());
        if u == Some(uid) {
            return Some(io::lossy(name));
        }
    }
    None
}

fn read_ppid(pid: i32) -> Option<i32> {
    let data = io::read_path(format!("/proc/{pid}/stat").as_bytes()).ok()?;
    let p = data.iter().rposition(|b| *b == b')')?;
    let rest = std::str::from_utf8(&data[p + 1..]).ok()?;
    let mut f = rest.split_whitespace();
    let _state = f.next()?;
    f.next()?.parse().ok()
}

fn read_cmd(pid: i32) -> String {
    let cmdline = io::read_path(format!("/proc/{pid}/cmdline").as_bytes()).unwrap_or_default();
    let mut parts: Vec<&[u8]> = cmdline.split(|b| *b == 0).collect();
    while parts.last().is_some_and(|p| p.is_empty()) {
        parts.pop();
    }
    if !parts.is_empty() {
        return parts.iter().map(|p| io::lossy(p)).collect::<Vec<_>>().join(" ");
    }
    let comm = io::read_path(format!("/proc/{pid}/comm").as_bytes()).unwrap_or_default();
    let comm = io::lossy(&comm);
    format!("[{}]", comm.trim_end_matches('\n'))
}

/// Montagens `nsfs` do mountinfo: `(tipo, inode, ponto de montagem)`.
fn nsfs_mounts() -> Vec<(String, u64, String)> {
    let data = io::read_path(b"/proc/self/mountinfo").unwrap_or_default();
    let mut out = Vec::new();
    for line in data.split(|b| *b == b'\n') {
        let text = io::lossy(line);
        let (pre, post) = match text.split_once(" - ") {
            Some(p) => p,
            None => continue,
        };
        if post.split(' ').next() != Some("nsfs") {
            continue;
        }
        let f: Vec<&str> = pre.split(' ').collect();
        if f.len() < 5 {
            continue;
        }
        if let Some((ty, ino)) = parse_link(f[3].as_bytes()) {
            out.push((ty, ino, f[4].to_string()));
        }
    }
    out
}

fn json_escape(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\t' => o.push_str("\\t"),
            '\r' => o.push_str("\\r"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
}

fn raw_escape(s: &str) -> String {
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

fn cell(ns: &Ns, col: Col, users: &mut BTreeMap<u32, String>) -> String {
    match col {
        Col::Ns => ns.ino.to_string(),
        Col::Type => ns.ty.clone(),
        Col::Path => ns.path.clone(),
        Col::Nprocs => ns.nprocs.to_string(),
        Col::Pid => ns.pid.map(|p| p.to_string()).unwrap_or_default(),
        Col::Ppid => ns.ppid.map(|p| p.to_string()).unwrap_or_default(),
        Col::Command => ns.cmd.clone(),
        Col::Uid => ns.uid.map(|p| p.to_string()).unwrap_or_default(),
        Col::User => match ns.uid {
            Some(u) => users
                .entry(u)
                .or_insert_with(|| passwd_name(u).unwrap_or_else(|| u.to_string()))
                .clone(),
            None => String::new(),
        },
        Col::Nsfs => ns.nsfs.join(" "),
        Col::Netnsid | Col::Pns | Col::Ons => String::new(),
    }
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let short = ul::short_name(args);
    let argv0 = io::argv0(args);

    const OPT_OUTPUT_ALL: i32 = 256;
    let longs = [
        LongOpt::new("json", HasArg::No, 'J' as i32),
        LongOpt::new("list", HasArg::No, 'l' as i32),
        LongOpt::new("noheadings", HasArg::No, 'n' as i32),
        LongOpt::new("output", HasArg::Required, 'o' as i32),
        LongOpt::new("output-all", HasArg::No, OPT_OUTPUT_ALL),
        LongOpt::new("task", HasArg::Required, 'p' as i32),
        LongOpt::new("raw", HasArg::No, 'r' as i32),
        LongOpt::new("notruncate", HasArg::No, 'u' as i32),
        LongOpt::new("nowrap", HasArg::No, 'W' as i32),
        LongOpt::new("type", HasArg::Required, 't' as i32),
        LongOpt::new("tree", HasArg::Required, 'T' as i32),
        LongOpt::new("persistent", HasArg::No, 'H' as i32),
        LongOpt::new("help", HasArg::No, 'h' as i32),
        LongOpt::new("version", HasArg::No, 'V' as i32),
    ];

    let mut json = false;
    let mut raw = false;
    let mut noheadings = false;
    let mut persistent = false;
    let mut cols: Vec<Col> = DEFAULT_COLS.to_vec();
    let mut task: Option<i32> = None;
    let mut type_filter: Option<String> = None;

    let mut g = Getopt::from_env(&argv[1.min(argv.len())..], "Jlno:p:ruWt:T:HhV", &longs);
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
            x if x == 'J' as i32 => json = true,
            x if x == 'l' as i32 || x == 'u' as i32 || x == 'W' as i32 => {}
            x if x == 'n' as i32 => noheadings = true,
            x if x == 'r' as i32 => raw = true,
            x if x == 'H' as i32 => persistent = true,
            OPT_OUTPUT_ALL => cols = COLS.iter().map(|c| c.0).collect(),
            x if x == 'p' as i32 => {
                let a = o.arg.clone().unwrap_or_default();
                let text = io::lossy(&a);
                match text.trim_start().parse::<i32>() {
                    Ok(p) => task = Some(p),
                    Err(_) => {
                        ul::warnx(&short, format!("invalid pid argument: '{text}'"));
                        return 1;
                    }
                }
            }
            x if x == 't' as i32 => {
                let a = io::lossy(&o.arg.clone().unwrap_or_default());
                match NS_TYPES.iter().find(|t| t.eq_ignore_ascii_case(&a)) {
                    Some(t) => type_filter = Some((*t).to_string()),
                    None => {
                        ul::warnx(&short, format!("unknown namespace type: {a}"));
                        return 1;
                    }
                }
            }
            x if x == 'T' as i32 => {
                let a = io::lossy(&o.arg.clone().unwrap_or_default());
                if !matches!(a.as_str(), "parent" | "owner" | "none") {
                    ul::warnx(&short, format!("unsupported --tree <relation>: {a}"));
                    return 1;
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
            _ => {
                ul::errtryhelp(&short);
                return 1;
            }
        }
    }
    let operands = g.operands();
    let ns_filter: Option<u64> = match operands.first() {
        Some(a) => match ul::strtou64_or_err(a, "invalid namespace argument") {
            Ok(v) => Some(v),
            Err(m) => {
                ul::warnx(&short, m);
                return 1;
            }
        },
        None => None,
    };

    let sys = sys::current();
    let mut found: BTreeMap<(u64, String), Ns> = BTreeMap::new();

    let pids: Vec<i32> = match task {
        Some(p) => vec![p],
        None => {
            let mut v: Vec<i32> = sys::read_dir(b"/proc")
                .unwrap_or_default()
                .iter()
                .filter_map(|e| std::str::from_utf8(&e.name).ok()?.parse::<i32>().ok())
                .collect();
            v.sort_unstable();
            v
        }
    };
    if !persistent || task.is_some() {
        for pid in pids {
            let mut info: Option<(Option<i32>, Option<u32>, String)> = None;
            for ty in NS_TYPES {
                if type_filter.as_deref().is_some_and(|t| t != *ty) {
                    continue;
                }
                let path = format!("/proc/{pid}/ns/{ty}");
                let link = match sys.readlinkat(Fd::CWD, path.as_bytes()) {
                    Ok(l) => l,
                    Err(_) => continue,
                };
                let (lty, ino) = match parse_link(&link) {
                    Some(v) => v,
                    None => continue,
                };
                if ns_filter.is_some_and(|f| f != ino) {
                    continue;
                }
                let (ppid, uid, cmd) = info
                    .get_or_insert_with(|| {
                        let uid = sys::stat(format!("/proc/{pid}").as_bytes()).ok().map(|s| s.uid);
                        (read_ppid(pid), uid, read_cmd(pid))
                    })
                    .clone();
                let entry = found.entry((ino, lty.clone())).or_insert_with(|| Ns {
                    ino,
                    ty: lty.clone(),
                    path: path.clone(),
                    ..Default::default()
                });
                entry.nprocs += 1;
                let replace = match entry.pid {
                    None => true,
                    Some(_) => entry.ppid == Some(pid),
                };
                if replace {
                    entry.pid = Some(pid);
                    entry.ppid = ppid;
                    entry.uid = uid;
                    entry.cmd = cmd;
                    entry.path = path;
                }
            }
        }
    }

    if persistent || cols.contains(&Col::Nsfs) {
        for (ty, ino, mp) in nsfs_mounts() {
            if type_filter.as_deref().is_some_and(|t| t != ty) || ns_filter.is_some_and(|f| f != ino) {
                continue;
            }
            if persistent && task.is_none() {
                let e = found.entry((ino, ty.clone())).or_insert_with(|| Ns {
                    ino,
                    ty: ty.clone(),
                    ..Default::default()
                });
                e.nsfs.push(mp);
            } else if let Some(e) = found.get_mut(&(ino, ty)) {
                e.nsfs.push(mp);
            }
        }
    }

    let list: Vec<Ns> = found.into_values().collect();
    if list.is_empty() && ns_filter.is_some() {
        return 1;
    }

    let mut users: BTreeMap<u32, String> = BTreeMap::new();
    let rows: Vec<Vec<String>> = list
        .iter()
        .map(|n| cols.iter().map(|c| cell(n, *c, &mut users)).collect())
        .collect();
    let meta: Vec<&(Col, &str, bool, bool, &str)> =
        cols.iter().map(|c| COLS.iter().find(|e| e.0 == *c).unwrap()).collect();

    let mut out = String::new();
    if json {
        out.push_str("{\n   \"namespaces\": [");
        if rows.is_empty() {
            out.push_str("]\n}\n");
        } else {
            out.push('\n');
            for (ri, r) in rows.iter().enumerate() {
                out.push_str("      {\n");
                for (ci, c) in r.iter().enumerate() {
                    let key = meta[ci].1.to_lowercase();
                    let val = if c.is_empty() {
                        "null".to_string()
                    } else if meta[ci].3 {
                        c.clone()
                    } else {
                        format!("\"{}\"", json_escape(c))
                    };
                    let comma = if ci + 1 < r.len() { "," } else { "" };
                    out.push_str(&format!("         \"{key}\": {val}{comma}\n"));
                }
                out.push_str(if ri + 1 < rows.len() { "      },\n" } else { "      }\n" });
            }
            out.push_str("   ]\n}\n");
        }
    } else if raw {
        if !noheadings {
            out.push_str(&meta.iter().map(|m| m.1).collect::<Vec<_>>().join(" "));
            out.push('\n');
        }
        for r in &rows {
            out.push_str(&r.iter().map(|c| raw_escape(c)).collect::<Vec<_>>().join(" "));
            out.push('\n');
        }
    } else if !rows.is_empty() {
        let mut widths: Vec<usize> = meta
            .iter()
            .map(|m| if noheadings { 0 } else { m.1.len() })
            .collect();
        for r in &rows {
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
                if meta[i].2 {
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
            out.push_str(&fmt_row(meta.iter().map(|m| m.1).collect()));
        }
        for r in &rows {
            out.push_str(&fmt_row(r.iter().map(|s| s.as_str()).collect()));
        }
    }
    let mut so = io::stdout();
    let _ = so.write_all(out.as_bytes());
    0
}
