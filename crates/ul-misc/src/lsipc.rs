//! `lsipc` do util-linux 2.41: informações sobre as facilidades IPC do System V.
//!
//! Porte do `sys-utils/lsipc.c`. Lê `/proc/sysvipc/{shm,msg,sem}` e monta a lista de segmentos de
//! memória compartilhada, filas de mensagens e arrays de semáforos, em tabela, `-r`, `-e` ou `-J`.
//! Os tempos aparecem em segundos desde a época. O modo `-g` (uso global do sistema) não é
//! coberto: a opção é aceita e ignorada.

use std::ffi::OsString;
use std::io::Write;

use sysabi::sys;

use crate::lsmem::human_size;
use crate::util::io;
use crate::util::ul;

#[derive(Copy, Clone, PartialEq, Eq)]
enum Res {
    Shm,
    Msg,
    Sem,
}

/// (nome, descrição, alinhado à direita, numérico)
type ColDef = (&'static str, &'static str, bool, bool);

const SHM_COLS: &[ColDef] = &[
    ("KEY", "Resource key", false, false),
    ("ID", "Resource ID", true, true),
    ("OWNER", "Owner's username or UID", false, false),
    ("PERMS", "Permissions", false, false),
    ("CUID", "Creator UID", true, true),
    ("CUSER", "Creator user", false, false),
    ("CGID", "Creator GID", true, true),
    ("CGROUP", "Creator group", false, false),
    ("UID", "User ID", true, true),
    ("USER", "User name", false, false),
    ("GID", "Group ID", true, true),
    ("GROUP", "Group name", false, false),
    ("CTIME", "Time of the last change", true, false),
    ("SIZE", "Size of the segment", true, true),
    ("NATTCH", "Number of attached processes", true, true),
    ("STATUS", "Status", false, false),
    ("ATTACH", "Attach time", true, false),
    ("DETACH", "Detach time", true, false),
    ("COMMAND", "Creator command line", false, false),
    ("CPID", "PID of the creator", true, true),
    ("LPID", "PID of last user", true, true),
];
const MSG_COLS: &[ColDef] = &[
    ("KEY", "Resource key", false, false),
    ("ID", "Resource ID", true, true),
    ("OWNER", "Owner's username or UID", false, false),
    ("PERMS", "Permissions", false, false),
    ("CUID", "Creator UID", true, true),
    ("CUSER", "Creator user", false, false),
    ("CGID", "Creator GID", true, true),
    ("CGROUP", "Creator group", false, false),
    ("UID", "User ID", true, true),
    ("USER", "User name", false, false),
    ("GID", "Group ID", true, true),
    ("GROUP", "Group name", false, false),
    ("CTIME", "Time of the last change", true, false),
    ("USEDBYTES", "Bytes used", true, true),
    ("MSGS", "Number of messages", true, true),
    ("SEND", "Time of last msgsnd", true, false),
    ("RECV", "Time of last msgrcv", true, false),
    ("LSPID", "PID of last msgsnd", true, true),
    ("LRPID", "PID of last msgrcv", true, true),
];
const SEM_COLS: &[ColDef] = &[
    ("KEY", "Resource key", false, false),
    ("ID", "Resource ID", true, true),
    ("OWNER", "Owner's username or UID", false, false),
    ("PERMS", "Permissions", false, false),
    ("CUID", "Creator UID", true, true),
    ("CUSER", "Creator user", false, false),
    ("CGID", "Creator GID", true, true),
    ("CGROUP", "Creator group", false, false),
    ("UID", "User ID", true, true),
    ("USER", "User name", false, false),
    ("GID", "Group ID", true, true),
    ("GROUP", "Group name", false, false),
    ("CTIME", "Time of the last change", true, false),
    ("NSEMS", "Number of semaphores in a set", true, true),
    ("OTIME", "Time of last semop", true, false),
];

impl Res {
    fn defs(self) -> &'static [ColDef] {
        match self {
            Res::Shm => SHM_COLS,
            Res::Msg => MSG_COLS,
            Res::Sem => SEM_COLS,
        }
    }
    fn path(self) -> &'static str {
        match self {
            Res::Shm => "/proc/sysvipc/shm",
            Res::Msg => "/proc/sysvipc/msg",
            Res::Sem => "/proc/sysvipc/sem",
        }
    }
    fn title(self) -> &'static str {
        match self {
            Res::Shm => "Shared Memory Segments:",
            Res::Msg => "Message Queues:",
            Res::Sem => "Semaphore Arrays:",
        }
    }
    fn json_key(self) -> &'static str {
        match self {
            Res::Shm => "sharedmemory",
            Res::Msg => "messages",
            Res::Sem => "semaphores",
        }
    }
    fn default_cols(self) -> &'static [&'static str] {
        match self {
            Res::Shm => &[
                "KEY", "ID", "PERMS", "OWNER", "SIZE", "NATTCH", "STATUS", "CTIME", "CPID",
                "LPID", "COMMAND",
            ],
            Res::Msg => &["KEY", "ID", "PERMS", "OWNER", "USEDBYTES", "MSGS", "LSPID", "LRPID"],
            Res::Sem => &["KEY", "ID", "PERMS", "OWNER", "NSEMS"],
        }
    }
    /// Índice do campo em `/proc/sysvipc/<res>` para cada coluna que vem direto dele.
    fn field(self, col: &str) -> Option<usize> {
        let t: &[(&str, usize)] = match self {
            Res::Shm => &[
                ("KEY", 0),
                ("ID", 1),
                ("PERMS", 2),
                ("SIZE", 3),
                ("CPID", 4),
                ("LPID", 5),
                ("NATTCH", 6),
                ("UID", 7),
                ("GID", 8),
                ("CUID", 9),
                ("CGID", 10),
                ("ATTACH", 11),
                ("DETACH", 12),
                ("CTIME", 13),
            ],
            Res::Msg => &[
                ("KEY", 0),
                ("ID", 1),
                ("PERMS", 2),
                ("USEDBYTES", 3),
                ("MSGS", 4),
                ("LSPID", 5),
                ("LRPID", 6),
                ("UID", 7),
                ("GID", 8),
                ("CUID", 9),
                ("CGID", 10),
                ("SEND", 11),
                ("RECV", 12),
                ("CTIME", 13),
            ],
            Res::Sem => &[
                ("KEY", 0),
                ("ID", 1),
                ("PERMS", 2),
                ("NSEMS", 3),
                ("UID", 4),
                ("GID", 5),
                ("CUID", 6),
                ("CGID", 7),
                ("OTIME", 8),
                ("CTIME", 9),
            ],
        };
        t.iter().find(|(n, _)| *n == col).map(|(_, i)| *i)
    }
}

const USAGE: &str = "
Usage:
 lsipc [options]

Show information on IPC facilities.

Resource options:
 -m, --shmems             shared memory segments
 -M, --posix-shmems       POSIX shared memory segments
 -q, --queues             message queues
 -Q, --posix-mqueues      POSIX message queues
 -s, --semaphores         semaphores
 -S, --posix-semaphores   POSIX semaphores
 -g, --global             info about system-wide usage
                            (may be used with -m, -q and -s)
 -i, --id <id>            System V resource identified by <id>
 -N, --name <name>        POSIX resource identified by <name>

Options:
     --noheadings         don't print headings
     --notruncate         don't truncate output
     --time-format=<type> display dates in short, full or iso format
 -b, --bytes              print SIZE in bytes rather
 -c, --creator            show creator and owner
 -e, --export             display in an export-able output format
 -J, --json               use the JSON output format
 -n, --newline            display each piece of information on a new line
 -l, --list               force list output format (for example with --id)
 -o, --output[=<list>]    define the columns to output
 -P, --numeric-perms      print numeric permissions (PERMS column)
 -r, --raw                display in raw mode
 -t, --time               show attach, detach and change times
 -y, --shell              use column names to be usable as shell variables

 -h, --help               display this help
 -V, --version            display version

Generic System V columns:
            KEY  Resource key
             ID  Resource ID
          OWNER  Owner's username or UID
          PERMS  Permissions
           CUID  Creator UID
          CUSER  Creator user
           CGID  Creator GID
         CGROUP  Creator group
            UID  User ID
           USER  User name
            GID  Group ID
          GROUP  Group name
          CTIME  Time of the last change

Generic POSIX columns:
           NAME  POSIX resource name
          OWNER  Owner's username or UID
          PERMS  Permissions
           CUID  Creator UID
          CUSER  Creator user
           CGID  Creator GID
         CGROUP  Creator group
          MTIME  Time of last action

System V Shared-memory columns (--shmems):
           SIZE  Segment size
         NATTCH  Number of attached processes
         STATUS  Status
         ATTACH  Attach time
         DETACH  Detach time
        COMMAND  Creator command line
           CPID  PID of the creator
           LPID  PID of last user

System V Message-queue columns (--queues):
      USEDBYTES  Bytes used
           MSGS  Number of messages
           SEND  Time of last msg sent
           RECV  Time of last msg received
          LSPID  PID of the last msg sender
          LRPID  PID of the last msg receiver

System V Semaphore columns (--semaphores):
          NSEMS  Number of semaphores
          OTIME  Time of the last operation

POSIX Semaphore columns (--posix-semaphores):
           SVAL  Semaphore value

Summary columns (--global):
       RESOURCE  Resource name
    DESCRIPTION  Resource description
          LIMIT  System-wide limit
           USED  Currently used
           USE%  Currently use percentage

For more details see lsipc(1).
";

fn usage(short: &str) -> String {
    USAGE.replace("lsipc", short)
}

fn proc_u64(path: &str, idx: usize) -> u64 {
    read_text(path)
        .and_then(|t| t.split_whitespace().nth(idx).and_then(|s| s.parse().ok()))
        .unwrap_or(0)
}

/// Linhas de dados (sem o cabeçalho) de um `/proc/sysvipc/*`, já divididas em campos.
fn sysv_rows(res: Res) -> Vec<Vec<u64>> {
    read_text(res.path())
        .unwrap_or_default()
        .lines()
        .skip(1)
        .map(|l| l.split_whitespace().map(|s| s.parse().unwrap_or(0)).collect())
        .collect()
}

fn limit_text(v: u64) -> String {
    if v >= 1024 { human_size(v) } else { v.to_string() }
}

/// Resumo de uso global do sistema (`lsipc -g`), uma linha por limite do kernel.
fn global_summary() -> String {
    let msg_used = sysv_rows(Res::Msg).len() as u64;
    let shm = sysv_rows(Res::Shm);
    let shm_used = shm.len() as u64;
    let shm_pages: u64 = shm.iter().map(|r| r.get(3).copied().unwrap_or(0).div_ceil(4096)).sum();
    let sem = sysv_rows(Res::Sem);
    let sem_used = sem.len() as u64;
    let sem_total: u64 = sem.iter().map(|r| r.get(3).copied().unwrap_or(0)).sum();
    let mq_used = sys::read_dir(b"/dev/mqueue")
        .map(|e| e.iter().filter(|x| x.name != b"." && x.name != b"..").count() as u64)
        .unwrap_or(0);
    let sem_p = "/proc/sys/kernel/sem";
    // (recurso, descrição, limite, usado). `None` no usado vira "-".
    let rows: Vec<(&str, &str, String, Option<u64>)> = vec![
        ("MSGMNI", "Number of System V message queues", proc_u64("/proc/sys/kernel/msgmni", 0).to_string(), Some(msg_used)),
        ("MSGMAX", "Max size of System V message (bytes)", limit_text(proc_u64("/proc/sys/kernel/msgmax", 0)), None),
        ("MSGMNB", "Default max size of System V queue (bytes)", limit_text(proc_u64("/proc/sys/kernel/msgmnb", 0)), None),
        ("MQUMNI", "Number of POSIX message queues", proc_u64("/proc/sys/fs/mqueue/queues_max", 0).to_string(), Some(mq_used)),
        ("MQUMAX", "Max size of POSIX message (bytes)", limit_text(proc_u64("/proc/sys/fs/mqueue/msgsize_max", 0)), None),
        ("MQUMNB", "Number of messages in POSIX message queue", limit_text(proc_u64("/proc/sys/fs/mqueue/msg_max", 0)), None),
        ("SHMMNI", "Shared memory segments", proc_u64("/proc/sys/kernel/shmmni", 0).to_string(), Some(shm_used)),
        ("SHMALL", "Shared memory pages", proc_u64("/proc/sys/kernel/shmall", 0).to_string(), Some(shm_pages)),
        ("SHMMAX", "Max size of shared memory segment (bytes)", limit_text(proc_u64("/proc/sys/kernel/shmmax", 0)), None),
        ("SHMMIN", "Min size of shared memory segment (bytes)", "1B".to_string(), None),
        ("SEMMNI", "Number of semaphore identifiers", proc_u64(sem_p, 3).to_string(), Some(sem_used)),
        ("SEMMNS", "Total number of semaphores", proc_u64(sem_p, 1).to_string(), Some(sem_total)),
        ("SEMMSL", "Max semaphores per semaphore set.", proc_u64(sem_p, 0).to_string(), None),
        ("SEMOPM", "Max number of operations per semop(2)", proc_u64(sem_p, 2).to_string(), None),
        ("SEMVMX", "Semaphore max value", "32767".to_string(), None),
    ];
    let mut out = format!("{:<8} {:<42} {:>20} {:>4} {:>5}\n", "RESOURCE", "DESCRIPTION", "LIMIT", "USED", "USE%");
    for (r, d, lim, used) in rows {
        let (u, pct) = match used {
            Some(u) => {
                let l: f64 = lim.parse().unwrap_or(0.0);
                let p = if l > 0.0 { u as f64 * 100.0 / l } else { 0.0 };
                (u.to_string(), format!("{p:.2}%"))
            }
            None => ("-".to_string(), "-".to_string()),
        };
        out.push_str(&format!("{r:<8} {d:<42} {lim:>20} {u:>4} {pct:>5}\n"));
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
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
}

fn read_text(path: &str) -> Option<String> {
    let data = sys::read_file(path.as_bytes()).ok()?;
    Some(String::from_utf8_lossy(&data).to_string())
}

/// Resolve um id numérico em nome via `/etc/passwd` (`group == false`) ou `/etc/group`.
fn id_name(table: &str, id: u64) -> String {
    for line in table.lines() {
        let f: Vec<&str> = line.split(':').collect();
        if f.len() > 2 && f[2].parse::<u64>().ok() == Some(id) {
            return f[0].to_string();
        }
    }
    id.to_string()
}

fn octal_perms(raw: &str, numeric: bool) -> String {
    let n = u32::from_str_radix(raw.trim(), 8).unwrap_or(0) & 0o777;
    if numeric {
        return format!("{n:04o}");
    }
    let mut s = String::new();
    for sh in [6, 3, 0] {
        let b = (n >> sh) & 7;
        s.push(if b & 4 != 0 { 'r' } else { '-' });
        s.push(if b & 2 != 0 { 'w' } else { '-' });
        s.push(if b & 1 != 0 { 'x' } else { '-' });
    }
    s
}

struct Opts {
    bytes: bool,
    numeric_perms: bool,
}

fn cell(res: Res, col: &str, f: &[String], passwd: &str, group: &str, o: &Opts) -> String {
    let get = |name: &str| -> String {
        res.field(name)
            .and_then(|i| f.get(i))
            .cloned()
            .unwrap_or_default()
    };
    let num = |name: &str| get(name).parse::<u64>().unwrap_or(0);
    match col {
        "KEY" => {
            let k = get("KEY").parse::<i64>().unwrap_or(0) as u32;
            format!("0x{k:08x}")
        }
        "PERMS" => octal_perms(&get("PERMS"), o.numeric_perms),
        "OWNER" | "USER" => id_name(passwd, num("UID")),
        "CUSER" => id_name(passwd, num("CUID")),
        "GROUP" => id_name(group, num("GID")),
        "CGROUP" => id_name(group, num("CGID")),
        "SIZE" | "USEDBYTES" => {
            let v = num(col);
            if o.bytes { v.to_string() } else { human_size(v) }
        }
        "STATUS" => {
            // O campo de modo do shm não vem em /proc; o status fica vazio sem o `shmctl`.
            String::new()
        }
        "COMMAND" => {
            let pid = num("CPID");
            let t = read_text(&format!("/proc/{pid}/cmdline")).unwrap_or_default();
            t.replace('\0', " ").trim_end().to_string()
        }
        _ => get(col),
    }
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let short = ul::short_name(args);

    let mut json = false;
    let mut raw = false;
    let mut export = false;
    let mut newline = false;
    let mut creator = false;
    let mut time = false;
    let mut noheadings = false;
    let mut global = false;
    let mut opts = Opts {
        bytes: false,
        numeric_perms: false,
    };
    let mut want: Vec<Res> = Vec::new();
    let mut id_filter: Option<String> = None;
    let mut out_cols: Option<Vec<Vec<u8>>> = None;

    let longs: &[(&str, bool)] = &[
        ("id", true),
        ("global", false),
        ("creator", false),
        ("export", false),
        ("newline", false),
        ("list", false),
        ("json", false),
        ("bytes", false),
        ("raw", false),
        ("time", false),
        ("numeric-perms", false),
        ("output", true),
        ("noheadings", false),
        ("notruncate", false),
        ("shmems", false),
        ("queues", false),
        ("semaphores", false),
        ("posix-shmems", false),
        ("posix-mqueues", false),
        ("posix-semaphores", false),
        ("name", true),
        ("time-format", true),
        ("shell", false),
        ("help", false),
        ("version", false),
    ];

    let mut i = 1;
    let mut operands = Vec::new();
    while i < argv.len() {
        let a = argv[i].as_slice();
        if a == b"--" {
            operands.extend(argv[i + 1..].iter().cloned());
            break;
        }
        if a.len() < 2 || a[0] != b'-' {
            operands.push(a.to_vec());
            i += 1;
            continue;
        }
        let mut items: Vec<(String, Option<Vec<u8>>)> = Vec::new();
        if a.starts_with(b"--") {
            let (name, inline) = match a.iter().position(|b| *b == b'=') {
                Some(p) => (a[2..p].to_vec(), Some(a[p + 1..].to_vec())),
                None => (a[2..].to_vec(), None),
            };
            let name_s = String::from_utf8_lossy(&name).to_string();
            let exact = longs.iter().find(|(n, _)| *n == name_s);
            let cands: Vec<_> = longs.iter().filter(|(n, _)| n.starts_with(&name_s)).collect();
            let found = match exact {
                Some(e) => e,
                None if cands.len() == 1 => cands[0],
                None if cands.is_empty() => {
                    ul::warnx(&short, format!("unrecognized option '{}'", io::lossy(a)));
                    ul::errtryhelp(&short);
                    return 1;
                }
                None => {
                    ul::warnx(&short, format!("option '{}' is ambiguous", io::lossy(a)));
                    ul::errtryhelp(&short);
                    return 1;
                }
            };
            let (n, needs) = found;
            let mut val = inline;
            if *needs && val.is_none() {
                i += 1;
                match argv.get(i) {
                    Some(v) => val = Some(v.clone()),
                    None => {
                        ul::warnx(&short, format!("option '--{n}' requires an argument"));
                        ul::errtryhelp(&short);
                        return 1;
                    }
                }
            }
            items.push((format!("--{n}"), val));
        } else {
            let mut k = 1;
            while k < a.len() {
                let c = a[k];
                match c {
                    b'g' | b'c' | b'e' | b'n' | b'l' | b'J' | b'b' | b'r' | b't' | b'P'
                    | b'm' | b'q' | b's' | b'h' | b'V' | b'M' | b'Q' | b'S' | b'y' => {
                        items.push((format!("-{}", c as char), None));
                        k += 1;
                    }
                    b'i' | b'o' | b'N' => {
                        let val = if k + 1 < a.len() {
                            a[k + 1..].to_vec()
                        } else {
                            i += 1;
                            match argv.get(i) {
                                Some(v) => v.clone(),
                                None => {
                                    ul::warnx(
                                        &short,
                                        format!("option requires an argument -- '{}'", c as char),
                                    );
                                    ul::errtryhelp(&short);
                                    return 1;
                                }
                            }
                        };
                        items.push((format!("-{}", c as char), Some(val)));
                        break;
                    }
                    _ => {
                        ul::warnx(&short, format!("invalid option -- '{}'", c as char));
                        ul::errtryhelp(&short);
                        return 1;
                    }
                }
            }
        }
        i += 1;
        for (key, val) in items {
            match key.as_str() {
                "-h" | "--help" => {
                    let mut out = io::stdout();
                    let _ = out.write_all(usage(&short).as_bytes());
                    return 0;
                }
                "-V" | "--version" => {
                    ul::print_version(&short);
                    return 0;
                }
                "-g" | "--global" => global = true,
                "-l" | "--list" => {}
                "-c" | "--creator" => creator = true,
                "-e" | "--export" => export = true,
                "-n" | "--newline" => newline = true,
                "-J" | "--json" => json = true,
                "-b" | "--bytes" => opts.bytes = true,
                "-r" | "--raw" => raw = true,
                "-t" | "--time" => time = true,
                "-P" | "--numeric-perms" => opts.numeric_perms = true,
                "--noheadings" => noheadings = true,
                "--notruncate" => {}
                "-m" | "--shmems" => want.push(Res::Shm),
                "-q" | "--queues" => want.push(Res::Msg),
                "-s" | "--semaphores" => want.push(Res::Sem),
                "-i" | "--id" => {
                    let v = val.unwrap_or_default();
                    match ul::strtou64_or_err(&v, "failed to parse IPC ID") {
                        Ok(n) => id_filter = Some(n.to_string()),
                        Err(m) => {
                            ul::warnx(&short, m);
                            return 1;
                        }
                    }
                }
                "-o" | "--output" => {
                    let list = val.unwrap_or_default();
                    let names: Vec<Vec<u8>> = list
                        .split(|b| *b == b',')
                        .filter(|n| !n.is_empty())
                        .map(|n| n.to_vec())
                        .collect();
                    out_cols = Some(names);
                }
                _ => {}
            }
        }
    }
    // Operandos são ignorados, como no original (getopt sem checagem de sobra).
    let _ = operands;
    if global || (want.is_empty() && id_filter.is_none() && out_cols.is_none()) {
        let mut so = io::stdout();
        let _ = so.write_all(global_summary().as_bytes());
        return 0;
    }
    if id_filter.is_some() && want.is_empty() {
        ul::warnx(&short, "--id <id> requires a resource option (--shmems, --queues or --semaphores)");
        ul::errtryhelp(&short);
        return 1;
    }
    // Valida as colunas de -o contra o conjunto das facilidades pedidas.
    let explicit = !want.is_empty();
    if want.is_empty() {
        want = vec![Res::Msg, Res::Shm, Res::Sem];
    }
    if let Some(names) = &out_cols {
        for n in names {
            let ok = want.iter().any(|r| {
                r.defs()
                    .iter()
                    .any(|d| d.0.as_bytes().eq_ignore_ascii_case(n))
            });
            if !ok {
                ul::warnx(&short, format!("unknown column: {}", io::lossy(n)));
                return 1;
            }
        }
    }

    let passwd = read_text("/etc/passwd").unwrap_or_default();
    let group = read_text("/etc/group").unwrap_or_default();
    let mut out = String::new();
    let mut json_parts: Vec<String> = Vec::new();

    for (ri, res) in want.iter().enumerate() {
        let defs = res.defs();
        let mut names: Vec<&'static str> = Vec::new();
        match &out_cols {
            Some(list) => {
                for n in list {
                    if let Some(d) = defs.iter().find(|d| d.0.as_bytes().eq_ignore_ascii_case(n))
                    {
                        names.push(d.0);
                    }
                }
            }
            None => {
                names.extend_from_slice(res.default_cols());
                if creator {
                    names.extend_from_slice(&["CUID", "CUSER", "CGID", "CGROUP"]);
                }
                if time {
                    let t: &[&str] = match res {
                        Res::Shm => &["ATTACH", "DETACH"],
                        Res::Msg => &["SEND", "RECV"],
                        Res::Sem => &["OTIME"],
                    };
                    names.extend_from_slice(t);
                }
            }
        }
        let text = read_text(res.path()).unwrap_or_default();
        let mut rows: Vec<Vec<String>> = Vec::new();
        for line in text.lines().skip(1) {
            let f: Vec<String> = line.split_whitespace().map(|s| s.to_string()).collect();
            if f.len() < 4 {
                continue;
            }
            if let Some(id) = &id_filter {
                if f.get(1) != Some(id) {
                    continue;
                }
            }
            rows.push(
                names
                    .iter()
                    .map(|c| cell(*res, c, &f, &passwd, &group, &opts))
                    .collect(),
            );
        }
        let info = |n: &str| defs.iter().find(|d| d.0 == n).copied().unwrap();

        if json {
            let mut p = format!("   \"{}\": [", res.json_key());
            for (n, row) in rows.iter().enumerate() {
                p.push_str(if n == 0 { "\n" } else { ",\n" });
                p.push_str("      {\n");
                for (k, c) in names.iter().enumerate() {
                    let v = &row[k];
                    let d = info(c);
                    let numeric = d.3 && !v.is_empty() && !(matches!(*c, "SIZE" | "USEDBYTES") && !opts.bytes);
                    let rendered = if numeric {
                        v.clone()
                    } else {
                        format!("\"{}\"", json_escape(v))
                    };
                    p.push_str(&format!("         \"{}\": {rendered}", c.to_ascii_lowercase()));
                    p.push_str(if k + 1 < names.len() { ",\n" } else { "\n" });
                }
                p.push_str("      }");
            }
            p.push_str("\n   ]");
            json_parts.push(p);
            continue;
        }
        if rows.is_empty() && explicit && id_filter.is_some() {
            continue;
        }
        if !raw && !export && !noheadings && want.len() > 1 || (!raw && !export && explicit && want.len() == 1 && false) {
            if ri > 0 {
                out.push('\n');
            }
            out.push_str(res.title());
            out.push('\n');
        } else if !raw && !export && !noheadings && want.len() == 1 && !explicit {
            out.push_str(res.title());
            out.push('\n');
        }
        if export {
            for row in &rows {
                if newline {
                    for (k, c) in names.iter().enumerate() {
                        out.push_str(&format!("{c}=\"{}\"\n", row[k]));
                    }
                } else {
                    let line: Vec<String> = names
                        .iter()
                        .zip(row)
                        .map(|(c, v)| format!("{c}=\"{v}\""))
                        .collect();
                    out.push_str(&line.join(" "));
                    out.push('\n');
                }
            }
        } else if raw {
            if !noheadings {
                out.push_str(&names.join(" "));
                out.push('\n');
            }
            for row in &rows {
                out.push_str(&row.join(" "));
                out.push('\n');
            }
        } else {
            let mut widths: Vec<usize> = names.iter().map(|c| c.len()).collect();
            for row in &rows {
                for (k, v) in row.iter().enumerate() {
                    widths[k] = widths[k].max(v.chars().count());
                }
            }
            let fmt_row = |cells: Vec<&str>| -> String {
                let mut line = String::new();
                for (k, c) in cells.iter().enumerate() {
                    if k > 0 {
                        line.push(' ');
                    }
                    let pad = widths[k].saturating_sub(c.chars().count());
                    if info(names[k]).2 {
                        line.push_str(&" ".repeat(pad));
                        line.push_str(c);
                    } else if k + 1 < cells.len() {
                        line.push_str(c);
                        line.push_str(&" ".repeat(pad));
                    } else {
                        line.push_str(c);
                    }
                }
                line.push('\n');
                line
            };
            if !noheadings {
                out.push_str(&fmt_row(names.to_vec()));
            }
            for row in &rows {
                out.push_str(&fmt_row(row.iter().map(|s| s.as_str()).collect()));
            }
        }
    }
    if json {
        out.push_str("{\n");
        out.push_str(&json_parts.join(",\n"));
        out.push_str("\n}\n");
    }

    let mut so = io::stdout();
    let _ = so.write_all(out.as_bytes());
    0
}

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}
