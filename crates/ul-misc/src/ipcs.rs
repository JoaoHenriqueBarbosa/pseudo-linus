//! `ipcs` do util-linux 2.41: informa sobre os recursos IPC do System V.
//!
//! Porte do `sys-utils/ipcs.c`. Em vez das chamadas `shmctl`/`msgctl`/`semctl` (que o `sysabi` não
//! oferece), lê `/proc/sysvipc/{shm,msg,sem}`, que traz os mesmos campos, e os limites de
//! `/proc/sys/kernel`. Implementados: a listagem padrão, `-m`/`-q`/`-s`/`-a`, `-l`, `-u`, `--human`,
//! `-b` e as validações de `-i`. As saídas de `-t`, `-p` e `-c` ainda não foram portadas: as opções
//! são aceitas e a listagem sai no formato padrão.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, sys};

use crate::lsmem::human_size;
use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const LONGS: &[LongOpt] = &[
    LongOpt::new("id", HasArg::Required, b'i' as i32),
    LongOpt::new("shmems", HasArg::No, b'm' as i32),
    LongOpt::new("queues", HasArg::No, b'q' as i32),
    LongOpt::new("semaphores", HasArg::No, b's' as i32),
    LongOpt::new("all", HasArg::No, b'a' as i32),
    LongOpt::new("time", HasArg::No, b't' as i32),
    LongOpt::new("pid", HasArg::No, b'p' as i32),
    LongOpt::new("creator", HasArg::No, b'c' as i32),
    LongOpt::new("limits", HasArg::No, b'l' as i32),
    LongOpt::new("summary", HasArg::No, b'u' as i32),
    LongOpt::new("human", HasArg::No, 0x100),
    LongOpt::new("bytes", HasArg::No, b'b' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
];

const USAGE: &str = "
Usage:
 ipcs [resource-option...] [output-option]
 ipcs -m|-q|-s -i <id>

Show information on IPC facilities.

Options:
 -i, --id <id>  print details on resource identified by <id>
 -h, --help     display this help
 -V, --version  display version

Resource options:
 -m, --shmems      shared memory segments
 -q, --queues      message queues
 -s, --semaphores  semaphores
 -a, --all         all (default)

Output options:
 -t, --time        show attach, detach and change times
 -p, --pid         show PIDs of creator and last operator
 -c, --creator     show creator and owner
 -l, --limits      show resource limits
 -u, --summary     show status summary
     --human       show sizes in human-readable format
 -b, --bytes       show sizes in bytes

For more details see ipcs(1).
";

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

/// Linhas numéricas de um `/proc/sysvipc/*` (sem o cabeçalho).
fn proc_rows(path: &str) -> Vec<Vec<u64>> {
    let Ok(data) = sys::read_file(path.as_bytes()) else {
        return Vec::new();
    };
    String::from_utf8_lossy(&data)
        .lines()
        .skip(1)
        .map(|l| l.split_whitespace().map(|f| f.parse().unwrap_or(0)).collect())
        .filter(|r: &Vec<u64>| !r.is_empty())
        .collect()
}

/// Valores de um arquivo de `/proc/sys/kernel` (vários campos em `sem`).
fn sysctl(name: &str, default: &[u64]) -> Vec<u64> {
    let path = format!("/proc/sys/kernel/{name}");
    match sys::read_file(path.as_bytes()) {
        Ok(d) => {
            let v: Vec<u64> = String::from_utf8_lossy(&d)
                .split_whitespace()
                .filter_map(|f| f.parse().ok())
                .collect();
            if v.len() >= default.len() { v } else { default.to_vec() }
        }
        Err(_) => default.to_vec(),
    }
}

/// Nome do dono pelo uid (`getpwuid`), ou o número quando não há entrada.
fn owner(uid: u64) -> String {
    if let Ok(d) = sys::read_file(b"/etc/passwd") {
        for line in String::from_utf8_lossy(&d).lines() {
            let f: Vec<&str> = line.split(':').collect();
            if f.len() > 2 && f[2].parse::<u64>().ok() == Some(uid) {
                return f[0].to_string();
            }
        }
    }
    uid.to_string()
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum Res {
    Msg,
    Shm,
    Sem,
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut want: Vec<Res> = Vec::new();
    let mut limits = false;
    let mut summary = false;
    let mut human = false;
    let mut id: Option<u64> = None;

    let mut g = Getopt::from_env(&argv[1..], "i:mqsatpclubhV", LONGS);
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                ul::errtryhelp(&short);
                return 1;
            }
        };
        match o.short() {
            Some('i') => match ul::strtou64_or_err(o.arg.as_deref().unwrap_or(b""), "failed to parse id argument") {
                Ok(n) => id = Some(n),
                Err(m) => {
                    ul::warnx(&short, m);
                    return 1;
                }
            },
            Some('m') => want.push(Res::Shm),
            Some('q') => want.push(Res::Msg),
            Some('s') => want.push(Res::Sem),
            Some('a') => {}
            Some('t') | Some('p') | Some('c') | Some('b') => {}
            Some('l') => limits = true,
            Some('u') => summary = true,
            Some('h') => {
                let mut out = io::stdout();
                let _ = out.write_all(USAGE.as_bytes());
                return 0;
            }
            Some('V') => {
                ul::print_version(&short);
                return 0;
            }
            None if o.id == 0x100 => human = true,
            _ => {
                ul::errtryhelp(&short);
                return 1;
            }
        }
    }

    if let Some(n) = id {
        if want.len() != 1 {
            ul::warnx(&short, "when using an ID, a single resource must be specified");
            return 1;
        }
        // Sem `shmctl`/`msgctl`/`semctl` o id só pode ser procurado nas tabelas do /proc.
        let (path, idx) = match want[0] {
            Res::Shm => ("/proc/sysvipc/shm", 1),
            Res::Msg => ("/proc/sysvipc/msg", 1),
            Res::Sem => ("/proc/sysvipc/sem", 1),
        };
        if !proc_rows(path).iter().any(|r| r.get(idx) == Some(&n)) {
            ul::warnx(&short, format!("id {n} not found"));
            return 1;
        }
    }
    if want.is_empty() {
        want = vec![Res::Msg, Res::Shm, Res::Sem];
    }

    let mut out = String::new();
    for res in &want {
        if limits {
            out.push_str(&limits_section(*res));
        } else if summary {
            out.push_str(&summary_section(*res));
        } else {
            out.push_str(&list_section(*res, human));
        }
    }
    out.push('\n');
    let mut so = io::stdout();
    let _ = so.write_all(out.as_bytes());
    0
}

fn size_cell(n: u64, human: bool) -> String {
    if human { human_size(n) } else { n.to_string() }
}

fn list_section(res: Res, human: bool) -> String {
    let mut o = String::new();
    match res {
        Res::Msg => {
            o.push_str("\n------ Message Queues --------\n");
            o.push_str(&format!(
                "{:<10} {:<10} {:<10} {:<10} {:<12} {:<12}\n",
                "key", "msqid", "owner", "perms", "used-bytes", "messages"
            ));
            for r in proc_rows("/proc/sysvipc/msg") {
                let g = |i: usize| r.get(i).copied().unwrap_or(0);
                o.push_str(&format!(
                    "0x{:08x} {:<10} {:<10} {:<10o} {:<12} {:<12}\n",
                    g(0) as u32,
                    g(1),
                    owner(g(7)),
                    g(2) & 0o777,
                    size_cell(g(3), human),
                    g(4)
                ));
            }
        }
        Res::Shm => {
            o.push_str("\n------ Shared Memory Segments --------\n");
            o.push_str(&format!(
                "{:<10} {:<10} {:<10} {:<10} {:<10} {:<10} {:<12}\n",
                "key", "shmid", "owner", "perms", "bytes", "nattch", "status"
            ));
            for r in proc_rows("/proc/sysvipc/shm") {
                let g = |i: usize| r.get(i).copied().unwrap_or(0);
                let mut status = String::new();
                if g(2) & 0o1000 != 0 {
                    status.push_str("dest");
                }
                if g(2) & 0o2000 != 0 {
                    if !status.is_empty() {
                        status.push(' ');
                    }
                    status.push_str("locked");
                }
                o.push_str(&format!(
                    "0x{:08x} {:<10} {:<10} {:<10o} {:<10} {:<10} {:<12}\n",
                    g(0) as u32,
                    g(1),
                    owner(g(7)),
                    g(2) & 0o777,
                    size_cell(g(3), human),
                    g(6),
                    status
                ));
            }
        }
        Res::Sem => {
            o.push_str("\n------ Semaphore Arrays --------\n");
            o.push_str(&format!(
                "{:<10} {:<10} {:<10} {:<10} {:<10}\n",
                "key", "semid", "owner", "perms", "nsems"
            ));
            for r in proc_rows("/proc/sysvipc/sem") {
                let g = |i: usize| r.get(i).copied().unwrap_or(0);
                o.push_str(&format!(
                    "0x{:08x} {:<10} {:<10} {:<10o} {:<10}\n",
                    g(0) as u32,
                    g(1),
                    owner(g(4)),
                    g(2) & 0o777,
                    g(3)
                ));
            }
        }
    }
    o
}

fn limits_section(res: Res) -> String {
    match res {
        Res::Msg => {
            let mni = sysctl("msgmni", &[32000])[0];
            let max = sysctl("msgmax", &[8192])[0];
            let mnb = sysctl("msgmnb", &[16384])[0];
            format!(
                "\n------ Messages Limits --------\nmax queues system wide = {mni}\nmax size of message (bytes) = {max}\ndefault max size of queue (bytes) = {mnb}\n"
            )
        }
        Res::Shm => {
            let mni = sysctl("shmmni", &[4096])[0];
            let max = sysctl("shmmax", &[u64::MAX - 16777216])[0];
            let all = sysctl("shmall", &[u64::MAX - 16777216])[0];
            let page = 4096u64;
            let all_kb = match all.checked_mul(page) {
                Some(v) => v / 1024,
                None => u64::MAX - 3,
            };
            format!(
                "\n------ Shared Memory Limits --------\nmax number of segments = {mni}\nmax seg size (kbytes) = {}\nmax total shared memory (kbytes) = {all_kb}\nmin seg size (bytes) = 1\n",
                max / 1024
            )
        }
        Res::Sem => {
            let s = sysctl("sem", &[32000, 1024000000, 500, 32000]);
            format!(
                "\n------ Semaphore Limits --------\nmax number of arrays = {}\nmax semaphores per array = {}\nmax semaphores system wide = {}\nmax ops per semop call = {}\nsemaphore max value = 32767\n",
                s[3], s[0], s[1], s[2]
            )
        }
    }
}

fn summary_section(res: Res) -> String {
    match res {
        Res::Msg => {
            let rows = proc_rows("/proc/sysvipc/msg");
            let headers: u64 = rows.iter().map(|r| r.get(4).copied().unwrap_or(0)).sum();
            let used: u64 = rows.iter().map(|r| r.get(3).copied().unwrap_or(0)).sum();
            format!(
                "\n------ Messages Status --------\nallocated queues = {}\nused headers = {headers}\nused space = {used} bytes\n",
                rows.len()
            )
        }
        Res::Shm => {
            let rows = proc_rows("/proc/sysvipc/shm");
            let sum = |i: usize| -> u64 { rows.iter().map(|r| r.get(i).copied().unwrap_or(0)).sum() };
            let pages: u64 = rows.iter().map(|r| r.get(3).copied().unwrap_or(0).div_ceil(4096)).sum();
            format!(
                "\n------ Shared Memory Status --------\nsegments allocated {}\npages allocated {pages}\npages resident  {}\npages swapped   {}\nSwap performance: 0 attempts\t 0 successes\n",
                rows.len(),
                sum(14) / 4096,
                sum(15) / 4096
            )
        }
        Res::Sem => {
            let rows = proc_rows("/proc/sysvipc/sem");
            let nsems: u64 = rows.iter().map(|r| r.get(3).copied().unwrap_or(0)).sum();
            format!(
                "\n------ Semaphore Status --------\nused arrays = {}\nallocated semaphores = {nsems}\n",
                rows.len()
            )
        }
    }
}
