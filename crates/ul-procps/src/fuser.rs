//! `fuser` do psmisc 23.7: lista (e opcionalmente mata) os processos que usam arquivos, pontos de
//! montagem ou sockets TCP/UDP.
//!
//! Um processo "usa" o alvo por diretório de trabalho (`c`), executável (`e`), descritor aberto
//! (`f`, ou `F` aberto para escrita), raiz (`r`) ou mapeamento de memória (`m`).
//!
//! Diferenças conhecidas: formato de `-v` e largura das colunas reproduzidos de memória do
//! `fuser.c`; `-n tcp|udp` casa só porta local (e porta remota opcional), sem host remoto; `-i`
//! sem terminal interativo nega a confirmação; `-M` e `-I` são aceitos e tratados como no modo
//! arquivo.

use std::collections::BTreeMap;
use std::ffi::OsString;

use sysabi::{Ctx, Errno, Fd, FileType, KillTarget, Signal, sys};
use ul_misc::util::io;

use crate::common::{self, out};
use crate::procfs;

const USAGE: &str = "Usage: fuser [-fIMuvw] [-a|-s] [-4|-6] [-c|-m|-n SPACE]\n             [-k [-i] [-SIGNAL]] NAME...\n       fuser -l\n       fuser -V\nShow which processes use the named files, sockets, or filesystems.\n\n  -a,--all              display unused files too\n  -i,--interactive      ask before killing (ignored without -k)\n  -I,--inode            use always inodes to compare files\n  -k,--kill             kill processes accessing the named file\n  -l,--list-signals     list available signal names\n  -m,--mount            show all processes using the named filesystems or\n                        block device\n  -M,--ismountpoint     fulfill request only if NAME is a mount point\n  -n,--namespace SPACE  search in this name space (file, udp, or tcp)\n  -s,--silent           silent operation\n  -SIGNAL               send this signal instead of SIGKILL\n  -u,--user             display user IDs\n  -v,--verbose          verbose output\n  -w,--writeonly        kill only processes with write access\n  -V,--version          display version information\n  -4,--ipv4             search IPv4 sockets only\n  -6,--ipv6             search IPv6 sockets only\n  -                     reset options\n\n  udp/tcp names: [local_port][,[rmt_host][,[rmt_port]]]\n\n";
const VERSION: &str = "fuser (PSmisc) 23.7\nCopyright (C) 1993-2024 Werner Almesberger and Craig Small\n\nPSmisc comes with ABSOLUTELY NO WARRANTY.\nThis is free software, and you are welcome to redistribute it under\nthe terms of the GNU General Public License.\nFor more information about these matters, see the files named COPYING.\n";

#[derive(Clone, Default)]
struct Opts {
    all: bool,
    kill: bool,
    mount: bool,
    silent: bool,
    user: bool,
    verbose: bool,
    write_only: bool,
    v4: bool,
    v6: bool,
    interactive: bool,
    space: Space,
    signal: i32,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Space {
    #[default]
    File,
    Tcp,
    Udp,
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn parse_signal(s: &str) -> Option<i32> {
    if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) {
        return s.parse().ok().filter(|v| (1..=64).contains(v));
    }
    common::signal_by_table_name(s).or_else(|| common::signal_rt(s))
}

fn list_signals() {
    let mut s = String::new();
    let mut col = 0usize;
    for name in common::SIGNAL_NAMES.iter() {
        if col + name.len() + 1 > 80 {
            s.push('\n');
            col = 0;
        }
        if col != 0 {
            s.push(' ');
        }
        s.push_str(name);
        col += name.len() + 1;
    }
    s.push('\n');
    out(s);
}

fn usage() -> i32 {
    io::eprint(USAGE);
    1
}

fn run(args: &[OsString]) -> i32 {
    let argv: Vec<String> = io::args_bytes(args).iter().map(|a| io::lossy(a)).collect();
    if argv.len() < 2 {
        return usage();
    }
    let mut o = Opts { signal: 9, ..Opts::default() };
    let mut names: Vec<(String, Opts)> = Vec::new();
    let mut list = false;
    let mut i = 1;
    while i < argv.len() {
        let a = argv[i].clone();
        i += 1;
        if a == "-" {
            o = Opts { signal: 9, ..Opts::default() };
            continue;
        }
        if let Some(l) = a.strip_prefix("--") {
            match l {
                "all" => o.all = true,
                "interactive" => o.interactive = true,
                "inode" | "ismountpoint" | "fuser" => {}
                "kill" => o.kill = true,
                "list-signals" => list = true,
                "mount" => o.mount = true,
                "silent" => o.silent = true,
                "user" => o.user = true,
                "verbose" => o.verbose = true,
                "writeonly" => o.write_only = true,
                "ipv4" => o.v4 = true,
                "ipv6" => o.v6 = true,
                "version" => {
                    io::eprint(VERSION);
                    return 0;
                }
                _ if l == "namespace" || l.starts_with("namespace=") => {
                    let v = match l.strip_prefix("namespace=") {
                        Some(v) => v.to_string(),
                        None => match argv.get(i) {
                            Some(v) => {
                                i += 1;
                                v.clone()
                            }
                            None => return usage(),
                        },
                    };
                    match set_space(&mut o, &v) {
                        true => {}
                        false => return usage(),
                    }
                }
                _ => return usage(),
            }
            continue;
        }
        if a.len() > 1 && a.starts_with('-') {
            let body = &a[1..];
            // `-9`, `-TERM`: sinal no lugar de cluster de letras.
            let multi = body.len() > 1 && body.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit());
            if (body.bytes().all(|b| b.is_ascii_digit()) || multi) && let Some(sg) = parse_signal(body) {
                o.signal = sg;
                continue;
            }
            let bytes = body.as_bytes();
            let mut k = 0;
            while k < bytes.len() {
                match bytes[k] {
                    b'a' => o.all = true,
                    b'i' => o.interactive = true,
                    b'I' | b'M' | b'f' => {}
                    b'k' => o.kill = true,
                    b'l' => list = true,
                    b'm' | b'c' => o.mount = true,
                    b's' => o.silent = true,
                    b'u' => o.user = true,
                    b'v' => o.verbose = true,
                    b'w' => o.write_only = true,
                    b'4' => o.v4 = true,
                    b'6' => o.v6 = true,
                    b'V' => {
                        io::eprint(VERSION);
                        return 0;
                    }
                    b'n' => {
                        let rest = &body[k + 1..];
                        let v = if rest.is_empty() {
                            match argv.get(i) {
                                Some(v) => {
                                    i += 1;
                                    v.clone()
                                }
                                None => return usage(),
                            }
                        } else {
                            rest.to_string()
                        };
                        if !set_space(&mut o, &v) {
                            return usage();
                        }
                        k = bytes.len();
                        continue;
                    }
                    _ => return usage(),
                }
                k += 1;
            }
            continue;
        }
        names.push((a, o.clone()));
    }
    if list {
        list_signals();
        return 0;
    }
    if names.is_empty() {
        return usage();
    }
    let any_kill = names.iter().any(|(_, o)| o.kill);
    let mut found_any = false;
    let mut first = true;
    for (name, opts) in &names {
        if opts.verbose && first && !opts.silent {
            out("                     USER        PID ACCESS COMMAND\n");
        }
        first = false;
        if scan_name(name, opts) {
            found_any = true;
        }
    }
    let _ = any_kill;
    if found_any { 0 } else { 1 }
}

fn set_space(o: &mut Opts, v: &str) -> bool {
    o.space = match v {
        "file" => Space::File,
        "tcp" => Space::Tcp,
        "udp" => Space::Udp,
        _ => return false,
    };
    true
}

/// Alvo resolvido: pares (dispositivo, inode) ou, para sockets, o conjunto de inodes.
enum Target {
    Inode { dev: u64, ino: u64, mount: bool },
    Sockets(Vec<u64>),
}

fn resolve(name: &str, o: &Opts) -> Result<Target, String> {
    let (spec, space) = match name.rsplit_once('/') {
        Some((p, "tcp")) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit() || b == b',') => (p.to_string(), Space::Tcp),
        Some((p, "udp")) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit() || b == b',') => (p.to_string(), Space::Udp),
        _ => (name.to_string(), o.space),
    };
    if space == Space::File {
        let st = sys::stat(name.as_bytes()).map_err(|_| format!("Specified filename {name} does not exist.\n"))?;
        let mount = o.mount;
        let dev = if mount && st.file_type() == FileType::BlockDevice { st.rdev } else { st.dev };
        return Ok(Target::Inode { dev, ino: st.ino, mount });
    }
    let mut parts = spec.split(',');
    let lport: Option<u32> = parts.next().filter(|p| !p.is_empty()).and_then(|p| p.parse().ok());
    let _rhost = parts.next();
    let rport: Option<u32> = parts.next().filter(|p| !p.is_empty()).and_then(|p| p.parse().ok());
    let proto = if space == Space::Tcp { "tcp" } else { "udp" };
    let mut files = Vec::new();
    if !o.v6 {
        files.push(format!("/proc/net/{proto}"));
    }
    if !o.v4 {
        files.push(format!("/proc/net/{proto}6"));
    }
    let mut inodes = Vec::new();
    for f in files {
        let Some(data) = procfs::read(&f) else { continue };
        for line in String::from_utf8_lossy(&data).lines().skip(1) {
            let c: Vec<&str> = line.split_ascii_whitespace().collect();
            if c.len() < 10 {
                continue;
            }
            let port = |s: &str| s.rsplit_once(':').and_then(|(_, p)| u32::from_str_radix(p, 16).ok());
            if lport.is_some() && port(c[1]) != lport {
                continue;
            }
            if rport.is_some() && port(c[2]) != rport {
                continue;
            }
            if let Ok(ino) = c[9].parse::<u64>()
                && ino != 0 {
                    inodes.push(ino);
                }
        }
    }
    Ok(Target::Sockets(inodes))
}

#[derive(Default)]
struct Access {
    letters: [bool; 5],
    write: bool,
}

const LETTERS: [char; 5] = ['c', 'e', 'f', 'r', 'm'];

fn link_matches(path: &str, t: &Target) -> bool {
    match t {
        Target::Inode { dev, ino, mount } => match sys::stat(path.as_bytes()) {
            Ok(st) => st.dev == *dev && (*mount || st.ino == *ino),
            Err(_) => false,
        },
        Target::Sockets(inodes) => match sys::current().readlinkat(Fd::CWD, path.as_bytes()) {
            Ok(l) => {
                let l = io::lossy(&l);
                l.strip_prefix("socket:[").and_then(|r| r.strip_suffix(']')).and_then(|n| n.parse::<u64>().ok()).is_some_and(|n| inodes.contains(&n))
            }
            Err(_) => false,
        },
    }
}

fn maps_match(pid: i32, t: &Target) -> bool {
    let Target::Inode { dev, ino, mount } = t else { return false };
    let Some(data) = procfs::read(&format!("/proc/{pid}/maps")) else { return false };
    for line in String::from_utf8_lossy(&data).lines() {
        let c: Vec<&str> = line.split_ascii_whitespace().collect();
        if c.len() < 5 {
            continue;
        }
        let Some((ma, mi)) = c[3].split_once(':') else { continue };
        let (Ok(ma), Ok(mi), Ok(n)) = (u64::from_str_radix(ma, 16), u64::from_str_radix(mi, 16), c[4].parse::<u64>()) else { continue };
        if n != 0 && common::makedev(ma, mi) == *dev && (*mount || n == *ino) {
            return true;
        }
    }
    false
}

fn fd_is_write(pid: i32, fd: &str) -> bool {
    procfs::read(&format!("/proc/{pid}/fdinfo/{fd}"))
        .and_then(|d| {
            String::from_utf8_lossy(&d).lines().find_map(|l| l.strip_prefix("flags:").map(|v| v.trim().to_string()))
        })
        .and_then(|v| u32::from_str_radix(&v, 8).ok())
        .is_some_and(|f| f & 3 != 0)
}

fn scan_name(name: &str, o: &Opts) -> bool {
    let target = match resolve(name, o) {
        Ok(t) => t,
        Err(m) => {
            io::eprint(m);
            return false;
        }
    };
    let me = sys::current().getpid();
    let mut found: BTreeMap<i32, Access> = BTreeMap::new();
    if let Ok(entries) = sys::read_dir(b"/proc") {
        for e in &entries {
            if e.name.is_empty() || !e.name.iter().all(u8::is_ascii_digit) {
                continue;
            }
            let Ok(pid) = io::lossy(&e.name).parse::<i32>() else { continue };
            let mut acc = Access::default();
            let is_file = matches!(target, Target::Inode { .. });
            if is_file {
                for (idx, l) in ["cwd", "exe", "", "root"].iter().enumerate() {
                    if !l.is_empty() && link_matches(&format!("/proc/{pid}/{l}"), &target) {
                        acc.letters[idx] = true;
                    }
                }
                if maps_match(pid, &target) {
                    acc.letters[4] = true;
                }
            }
            if let Ok(fds) = sys::read_dir(format!("/proc/{pid}/fd").as_bytes()) {
                for f in &fds {
                    if f.name.is_empty() || !f.name.iter().all(u8::is_ascii_digit) {
                        continue;
                    }
                    let fname = io::lossy(&f.name);
                    if link_matches(&format!("/proc/{pid}/fd/{fname}"), &target) {
                        acc.letters[2] = true;
                        if fd_is_write(pid, &fname) {
                            acc.write = true;
                        }
                    }
                }
            }
            if acc.letters.iter().any(|b| *b) {
                found.insert(pid, acc);
            }
        }
    }
    let used = !found.is_empty();
    if o.silent {
        if o.kill {
            kill_all(&found, o, me);
        }
        return used;
    }
    let mut names = common::Names::new();
    if o.verbose {
        if !used && o.all {
            io::eprint(format!("{name}:\n"));
        }
        for (pid, acc) in &found {
            let uid = proc_uid(*pid);
            let user = names.user_or_id(uid);
            let flags: String = LETTERS
                .iter()
                .enumerate()
                .map(|(k, c)| if acc.letters[k] { if k == 2 && acc.write { 'F' } else { *c } } else { '.' })
                .collect();
            let cmd = procfs::read(&format!("/proc/{pid}/comm")).map(|c| io::lossy(&c).trim_end().to_string()).unwrap_or_default();
            out(format!("{:<20} {:<8} {:>5} {:<6} {}\n", format!("{name}:"), user, pid, flags, cmd));
        }
    } else if used || o.all {
        let _ = io::flush_stdout();
        io::eprint(format!("{name}:"));
        for (pid, acc) in &found {
            out(format!(" {pid:>5}"));
            let _ = io::flush_stdout();
            let mut s = String::new();
            for (k, c) in LETTERS.iter().enumerate() {
                if acc.letters[k] {
                    s.push(if k == 2 && acc.write { 'F' } else { *c });
                }
            }
            if o.user {
                s.push_str(&format!("({})", names.user_or_id(proc_uid(*pid))));
            }
            io::eprint(s);
        }
        io::eprint("\n");
    }
    if o.kill {
        kill_all(&found, o, me);
    }
    used
}

fn proc_uid(pid: i32) -> u32 {
    procfs::read(&format!("/proc/{pid}/status"))
        .and_then(|d| procfs::Status::parse(&d).ids("Uid"))
        .map_or(0, |i| i[0])
}

fn kill_all(found: &BTreeMap<i32, Access>, o: &Opts, me: i32) {
    let sysc = sys::current();
    for (pid, acc) in found {
        if *pid == me || (o.write_only && !acc.write) {
            continue;
        }
        if o.interactive {
            continue;
        }
        if let Err(e) = sysc.kill(KillTarget::Pid(*pid), Signal(o.signal)) {
            let e: Errno = e;
            io::eprint(format!("Could not kill process {pid}: {}\n", e.message()));
        }
    }
}
