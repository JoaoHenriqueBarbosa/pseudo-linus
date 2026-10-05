//! `lslogins` do util-linux 2.41: informações sobre as contas conhecidas do sistema.
//!
//! Porte do `login-utils/lslogins.c`. Lê `/etc/passwd`, `/etc/group` e `/etc/shadow`, o
//! `/etc/login.defs` (faixas de UID), os registros de login de `/var/log/wtmp` e `/var/log/btmp`
//! (ausentes, as colunas de último login ficam vazias) e a lista de processos em `/proc` para a
//! coluna `PROC`. Saída em tabela, `--raw`, `--colon-separate`, `--export` e `--newline`. O
//! lslogins 2.41.5 não tem `--json` nem caminhos alternativos de passwd e group: o oráculo rejeita
//! essas opções, e aqui também. Tempos são exibidos em UTC.

use std::ffi::OsString;
use std::io::Write;

use sysabi::sys;
use sysabi::{Fd, OFlags};

use crate::util::io;
use crate::util::ul;

const DEFAULT_WTMP: &str = "/var/log/wtmp";
const DEFAULT_BTMP: &str = "/var/log/btmp";
const UTMP_RECORD: usize = 384;
const USER_PROCESS: i16 = 7;

#[derive(Copy, Clone, PartialEq, Eq)]
enum Kind {
    Text,
    Num,
    Flag,
    Date,
}

struct ColDef {
    name: &'static str,
    help: &'static str,
    kind: Kind,
}

const COLS: &[ColDef] = &[
    ColDef { name: "USER", help: "user name", kind: Kind::Text },
    ColDef { name: "UID", help: "user ID", kind: Kind::Num },
    ColDef { name: "GECOS", help: "full user name", kind: Kind::Text },
    ColDef { name: "HOMEDIR", help: "home directory", kind: Kind::Text },
    ColDef { name: "SHELL", help: "login shell", kind: Kind::Text },
    ColDef { name: "NOLOGIN", help: "log in disabled by nologin(8) or pam_nologin(8)", kind: Kind::Flag },
    ColDef { name: "PWD-LOCK", help: "password defined, but locked", kind: Kind::Flag },
    ColDef { name: "PWD-EMPTY", help: "password not defined", kind: Kind::Flag },
    ColDef { name: "PWD-DENY", help: "login by password disabled", kind: Kind::Flag },
    ColDef { name: "PWD-METHOD", help: "password encryption method", kind: Kind::Text },
    ColDef { name: "GROUP", help: "primary group name", kind: Kind::Text },
    ColDef { name: "GID", help: "primary group ID", kind: Kind::Num },
    ColDef { name: "SUPP-GROUPS", help: "supplementary group names", kind: Kind::Text },
    ColDef { name: "SUPP-GIDS", help: "supplementary group IDs", kind: Kind::Text },
    ColDef { name: "LAST-LOGIN", help: "date of last login", kind: Kind::Date },
    ColDef { name: "LAST-TTY", help: "last tty used", kind: Kind::Text },
    ColDef { name: "LAST-HOSTNAME", help: "hostname during the last session", kind: Kind::Text },
    ColDef { name: "FAILED-LOGIN", help: "date of last failed login", kind: Kind::Date },
    ColDef { name: "FAILED-TTY", help: "where did the login fail?", kind: Kind::Text },
    ColDef { name: "HUSHED", help: "user's hush settings", kind: Kind::Flag },
    ColDef { name: "PWD-WARN", help: "days user is warned of password expiration", kind: Kind::Num },
    ColDef { name: "PWD-CHANGE", help: "date of last password change", kind: Kind::Date },
    ColDef { name: "PWD-MIN", help: "number of days required between changes", kind: Kind::Num },
    ColDef { name: "PWD-MAX", help: "max number of days a password may remain unchanged", kind: Kind::Num },
    ColDef { name: "PWD-EXPIR", help: "password expiration date", kind: Kind::Date },
    ColDef { name: "CONTEXT", help: "the user's security context", kind: Kind::Text },
    ColDef { name: "PROC", help: "number of processes run by the user", kind: Kind::Num },
];

const DEFAULT_COLS: &[&str] = &[
    "UID",
    "USER",
    "PROC",
    "PWD-LOCK",
    "PWD-DENY",
    "LAST-LOGIN",
    "GECOS",
];

fn col_index(name: &[u8]) -> Option<usize> {
    COLS.iter()
        .position(|c| c.name.as_bytes().eq_ignore_ascii_case(name))
}

fn usage(short: &str) -> String {
    let mut s = format!(
        "
Usage:
 {short} [options] [<username>]

Display information about known users in the system.

Options:
 -a, --acc-expiration     display info about passwords expiration
 -c, --colon-separate     display data in a format similar to /etc/passwd
 -e, --export             display in an export-able output format
 -f, --failed             display data about the users' last failed logins
 -G, --supp-groups        display information about groups
 -g, --groups=<groups>    display users belonging to a group in <groups>
 -L, --last               show info about the users' last login sessions
 -l, --logins=<logins>    display only users from <logins>
 -n, --newline            display each piece of information on a new line
     --noheadings         don't print headings
     --notruncate         don't truncate output
 -o, --output[=<list>]    define the columns to output
     --output-all         output all columns
 -p, --pwd                display information related to login by password
 -r, --raw                display in raw mode
 -s, --system-accs        display system accounts
     --time-format=<type> display dates in short, full or iso format
 -u, --user-accs          display user accounts
 -y, --shell              use column names to be usable as shell variable identifiers
 -Z, --context            display SELinux contexts
 -z, --print0             delimit user entries with a nul character
     --wtmp-file <path>   set an alternate path for wtmp
     --btmp-file <path>   set an alternate path for btmp
     --lastlog <path>     set an alternate path for lastlog
     --lastlog2 <path>    set an alternate path for lastlog2

 -h, --help               display this help
 -V, --version            display version

Available output columns:
"
    );
    for c in COLS {
        s.push_str(&format!(" {:>14}  {}\n", c.name, c.help));
    }
    s.push_str(&format!("\nFor more details see {short}(1).\n"));
    s
}

struct User {
    name: String,
    passwd: String,
    uid: u32,
    gid: u32,
    gecos: String,
    home: String,
    shell: String,
}

struct Group {
    name: String,
    gid: u32,
    members: Vec<String>,
}

#[derive(Default, Clone)]
struct Shadow {
    hash: String,
    lastchg: Option<i64>,
    min: Option<i64>,
    max: Option<i64>,
    warn: Option<i64>,
}

fn read_text(path: &str) -> Option<String> {
    let data = sys::read_file(path.as_bytes()).ok()?;
    Some(String::from_utf8_lossy(&data).to_string())
}

fn parse_passwd(text: &str) -> Vec<User> {
    let mut out = Vec::new();
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let f: Vec<&str> = line.split(':').collect();
        if f.len() < 7 {
            continue;
        }
        let (Ok(uid), Ok(gid)) = (f[2].parse::<u32>(), f[3].parse::<u32>()) else {
            continue;
        };
        out.push(User {
            name: f[0].to_string(),
            passwd: f[1].to_string(),
            uid,
            gid,
            gecos: f[4].to_string(),
            home: f[5].to_string(),
            shell: f[6].to_string(),
        });
    }
    out
}

fn parse_group(text: &str) -> Vec<Group> {
    let mut out = Vec::new();
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let f: Vec<&str> = line.split(':').collect();
        if f.len() < 4 {
            continue;
        }
        let Ok(gid) = f[2].parse::<u32>() else {
            continue;
        };
        out.push(Group {
            name: f[0].to_string(),
            gid,
            members: f[3]
                .split(',')
                .filter(|m| !m.is_empty())
                .map(str::to_string)
                .collect(),
        });
    }
    out
}

fn parse_shadow(text: &str, name: &str) -> Option<Shadow> {
    for line in text.lines() {
        let f: Vec<&str> = line.split(':').collect();
        if f.len() >= 2 && f[0] == name {
            let num = |i: usize| f.get(i).and_then(|v| v.parse::<i64>().ok());
            return Some(Shadow {
                hash: f[1].to_string(),
                lastchg: num(2),
                min: num(3),
                max: num(4),
                warn: num(5),
            });
        }
    }
    None
}

/// Valor numérico de uma chave do `login.defs` (`KEY valor`).
fn login_defs(text: &str, key: &str) -> Option<u32> {
    let mut found = None;
    for line in text.lines() {
        let l = line.trim_start();
        if l.starts_with('#') {
            continue;
        }
        let mut it = l.split_whitespace();
        if it.next() == Some(key) {
            if let Some(v) = it.next().and_then(|v| v.parse::<u32>().ok()) {
                found = Some(v);
            }
        }
    }
    found
}

/// Registro do utmp: `(tipo, linha, usuário, host, segundos)`.
struct Utmp {
    kind: i16,
    line: String,
    user: String,
    host: String,
    sec: i64,
}

fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|c| *c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).to_string()
}

fn read_utmp(path: &str) -> Vec<Utmp> {
    let Ok(data) = sys::read_file(path.as_bytes()) else {
        return Vec::new();
    };
    data.chunks_exact(UTMP_RECORD)
        .map(|r| Utmp {
            kind: i16::from_le_bytes([r[0], r[1]]),
            line: cstr(&r[8..40]),
            user: cstr(&r[44..76]),
            host: cstr(&r[76..332]),
            sec: i64::from(i32::from_le_bytes([r[340], r[341], r[342], r[343]])),
        })
        .collect()
}

/// O registro mais recente de `user` (no `wtmp` só sessões de usuário; o `btmp` guarda tentativas).
fn latest<'a>(recs: &'a [Utmp], user: &str, only_user_process: bool) -> Option<&'a Utmp> {
    recs.iter()
        .filter(|r| r.user == user && (!only_user_process || r.kind == USER_PROCESS))
        .max_by_key(|r| r.sec)
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum TimeFmt {
    Short,
    Full,
    Iso,
}

fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn fmt_time(secs: i64, fmt: TimeFmt) -> String {
    const WD: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MN: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil(days);
    let (hh, mm, ss) = (rem / 3600, rem % 3600 / 60, rem % 60);
    let wd = WD[(days + 4).rem_euclid(7) as usize];
    let mon = MN[(m - 1) as usize];
    match fmt {
        TimeFmt::Iso => format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}+0000"),
        TimeFmt::Full => format!("{wd} {mon} {d:>2} {hh:02}:{mm:02}:{ss:02} {y}"),
        TimeFmt::Short => format!("{wd} {mon} {d:>2} {y}"),
    }
}

fn fmt_day(days: i64) -> String {
    let (y, m, d) = civil(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Quantidade de processos por UID, lida de `/proc/<pid>/status`.
fn count_procs() -> Vec<(u32, u64)> {
    let mut counts: Vec<(u32, u64)> = Vec::new();
    let Ok(fd) = sys::open(b"/proc", OFlags::RDONLY | OFlags::DIRECTORY, 0) else {
        return counts;
    };
    let mut pids: Vec<String> = Vec::new();
    loop {
        match sys::current().getdents(fd) {
            Ok(v) if !v.is_empty() => {
                for e in v {
                    let n = String::from_utf8_lossy(&e.name).to_string();
                    if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) {
                        pids.push(n);
                    }
                }
            }
            _ => break,
        }
    }
    let _ = sys::close(fd);
    for pid in pids {
        let Some(st) = read_text(&format!("/proc/{pid}/status")) else {
            continue;
        };
        let uid = st
            .lines()
            .find_map(|l| l.strip_prefix("Uid:"))
            .and_then(|v| v.split_whitespace().next().and_then(|u| u.parse::<u32>().ok()));
        if let Some(u) = uid {
            match counts.iter_mut().find(|(x, _)| *x == u) {
                Some(e) => e.1 += 1,
                None => counts.push((u, 1)),
            }
        }
    }
    counts
}

/// Nome do método de criptografia a partir do prefixo do hash do shadow.
fn pwd_method(hash: &str) -> Option<&'static str> {
    if hash.starts_with("$1$") {
        Some("MD5")
    } else if hash.starts_with("$2") {
        Some("BLOWFISH")
    } else if hash.starts_with("$5$") {
        Some("SHA256")
    } else if hash.starts_with("$6$") {
        Some("SHA512")
    } else if hash.starts_with("$y$") {
        Some("YESCRYPT")
    } else {
        None
    }
}

/// Nome de coluna utilizável como identificador de shell (`-y`).
fn shell_name(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
        .collect()
}

/// Codificação do libsmartcols para raw e export: o espaço vira `\x20`.
fn raw_escape(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        match c {
            ' ' => o.push_str("\\x20"),
            '\\' => o.push_str("\\x5c"),
            '\n' => o.push_str("\\x0a"),
            '\t' => o.push_str("\\x09"),
            c => o.push(c),
        }
    }
    o
}

#[derive(PartialEq, Eq, Copy, Clone)]
enum Mode {
    Table,
    Raw,
    Colon,
    Export,
    Newline,
}

struct Ctx {
    users: Vec<User>,
    groups: Vec<Group>,
    shadow_text: Option<String>,
    wtmp: Vec<Utmp>,
    btmp: Vec<Utmp>,
    procs: Vec<(u32, u64)>,
    time_fmt: TimeFmt,
}

impl Ctx {
    fn group_name(&self, gid: u32) -> Option<String> {
        self.groups.iter().find(|g| g.gid == gid).map(|g| g.name.clone())
    }

    fn supp(&self, u: &User) -> Vec<&Group> {
        self.groups
            .iter()
            .filter(|g| g.gid != u.gid && g.members.iter().any(|m| *m == u.name))
            .collect()
    }

    fn shadow(&self, u: &User) -> Option<Shadow> {
        if u.passwd != "x" {
            return Some(Shadow {
                hash: u.passwd.clone(),
                ..Shadow::default()
            });
        }
        self.shadow_text.as_ref().and_then(|t| parse_shadow(t, &u.name))
    }

    fn value(&self, col: usize, u: &User) -> Option<String> {
        let flag = |b: bool| Some(if b { "1" } else { "0" }.to_string());
        let num = |v: Option<i64>| v.filter(|n| *n >= 0).map(|n| n.to_string());
        match COLS[col].name {
            "USER" => Some(u.name.clone()),
            "UID" => Some(u.uid.to_string()),
            "GECOS" => Some(u.gecos.clone()),
            "HOMEDIR" => Some(u.home.clone()),
            "SHELL" => Some(u.shell.clone()),
            "NOLOGIN" => flag(u.shell.ends_with("nologin")),
            "PWD-LOCK" => flag(self.shadow(u).is_some_and(|s| s.hash.starts_with('!'))),
            "PWD-EMPTY" => flag(self.shadow(u).is_some_and(|s| s.hash.is_empty())),
            "PWD-DENY" => flag(self.shadow(u).is_some_and(|s| s.hash.starts_with('*'))),
            "PWD-METHOD" => self
                .shadow(u)
                .and_then(|s| pwd_method(&s.hash).map(str::to_string)),
            "GROUP" => self.group_name(u.gid),
            "GID" => Some(u.gid.to_string()),
            "SUPP-GROUPS" => {
                let v: Vec<String> = self.supp(u).iter().map(|g| g.name.clone()).collect();
                (!v.is_empty()).then(|| v.join(","))
            }
            "SUPP-GIDS" => {
                let v: Vec<String> = self.supp(u).iter().map(|g| g.gid.to_string()).collect();
                (!v.is_empty()).then(|| v.join(","))
            }
            "LAST-LOGIN" => latest(&self.wtmp, &u.name, true).map(|r| fmt_time(r.sec, self.time_fmt)),
            "LAST-TTY" => latest(&self.wtmp, &u.name, true).map(|r| r.line.clone()),
            "LAST-HOSTNAME" => latest(&self.wtmp, &u.name, true).map(|r| r.host.clone()),
            "FAILED-LOGIN" => latest(&self.btmp, &u.name, false).map(|r| fmt_time(r.sec, self.time_fmt)),
            "FAILED-TTY" => latest(&self.btmp, &u.name, false).map(|r| r.line.clone()),
            "HUSHED" => {
                let p = format!("{}/.hushlogin", u.home);
                flag(sys::lstat(p.as_bytes()).is_ok())
            }
            "PWD-WARN" => num(self.shadow(u).and_then(|s| s.warn)),
            "PWD-MIN" => num(self.shadow(u).and_then(|s| s.min)),
            "PWD-MAX" => num(self.shadow(u).and_then(|s| s.max)),
            "PWD-CHANGE" => self
                .shadow(u)
                .and_then(|s| s.lastchg)
                .filter(|d| *d >= 0)
                .map(fmt_day),
            "PWD-EXPIR" => self.shadow(u).and_then(|s| {
                let (l, m) = (s.lastchg?, s.max?);
                (l >= 0 && m >= 0).then(|| fmt_day(l + m))
            }),
            "CONTEXT" => None,
            "PROC" => Some(
                self.procs
                    .iter()
                    .find(|(x, _)| *x == u.uid)
                    .map(|(_, n)| *n)
                    .unwrap_or(0)
                    .to_string(),
            ),
            _ => None,
        }
    }
}

fn errx(short: &str, msg: impl AsRef<str>) -> i32 {
    ul::warnx(short, msg);
    1
}

fn split_list(v: &[u8]) -> Vec<String> {
    io::lossy(v)
        .split(',')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let short = ul::short_name(args);

    let mut mode_flags: Vec<&'static str> = Vec::new();
    let mut output: Option<Vec<u8>> = None;
    let mut mode = Mode::Table;
    let mut noheadings = false;
    let mut print0 = false;
    let mut want_user = false;
    let mut want_sys = false;
    let mut logins: Vec<String> = Vec::new();
    let mut groups_sel: Vec<String> = Vec::new();
    let mut output_all = false;
    let mut time_fmt = TimeFmt::Short;
    let mut shellvar = false;
    let mut excl_first: Option<&'static str> = None;
    let mut wtmp_path = DEFAULT_WTMP.to_string();
    let mut btmp_path = DEFAULT_BTMP.to_string();
    let passwd_path = "/etc/passwd";
    let group_path = "/etc/group";

    // (nome, tem argumento)
    let longs: &[(&str, bool)] = &[
        ("acc-expiration", false),
        ("colon-separate", false),
        ("export", false),
        ("failed", false),
        ("groups", true),
        ("help", false),
        ("logins", true),
        ("supp-groups", false),
        ("newline", false),
        ("noheadings", false),
        ("notruncate", false),
        ("output", true),
        ("output-all", false),
        ("last", false),
        ("lastlog", true),
        ("lastlog2", true),
        ("shell", false),
        ("raw", false),
        ("system-accs", false),
        ("time-format", true),
        ("user-accs", false),
        ("version", false),
        ("print0", false),
        ("wtmp-file", true),
        ("btmp-file", true),
        ("pwd", false),
        ("context", false),
    ];
    let long_to_short = |n: &str| -> &'static str {
        match n {
            "acc-expiration" => "-a",
            "colon-separate" => "-c",
            "export" => "-e",
            "failed" => "-f",
            "groups" => "-g",
            "help" => "-h",
            "logins" => "-l",
            "supp-groups" => "-G",
            "newline" => "-n",
            "output" => "-o",
            "last" => "-L",
            "raw" => "-r",
            "system-accs" => "-s",
            "user-accs" => "-u",
            "version" => "-V",
            "print0" => "-z",
            "pwd" => "-p",
            "context" => "-Z",
            "shell" => "-y",
            "noheadings" => "--noheadings",
            "notruncate" => "--notruncate",
            "output-all" => "--output-all",
            "lastlog" => "--lastlog",
            "lastlog2" => "--lastlog2",
            "time-format" => "--time-format",
            "wtmp-file" => "--wtmp-file",
            "btmp-file" => "--btmp-file",
            _ => "",
        }
    };

    let mut i = 1;
    let mut operands: Vec<Vec<u8>> = Vec::new();
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
        let mut items: Vec<(&'static str, Option<Vec<u8>>)> = Vec::new();
        if a.starts_with(b"--") {
            let (name, inline) = match a.iter().position(|b| *b == b'=') {
                Some(p) => (a[2..p].to_vec(), Some(a[p + 1..].to_vec())),
                None => (a[2..].to_vec(), None),
            };
            let name_s = String::from_utf8_lossy(&name).to_string();
            let exact = longs.iter().find(|(n, _)| *n == name_s);
            let cands: Vec<_> = longs.iter().filter(|(n, _)| n.starts_with(&name_s)).collect();
            let found = match exact {
                Some(e) => *e,
                None if cands.len() == 1 => *cands[0],
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
            if needs && val.is_none() {
                i += 1;
                match argv.get(i) {
                    Some(v) => val = Some(v.clone()),
                    None => {
                        ul::warnx(&short, format!("option '--{n}' requires an argument"));
                        ul::errtryhelp(&short);
                        return 1;
                    }
                }
            } else if !needs && val.is_some() && n != "output" {
                ul::warnx(&short, format!("option '--{n}' doesn't allow an argument"));
                ul::errtryhelp(&short);
                return 1;
            }
            items.push((long_to_short(n), val));
        } else {
            let mut k = 1;
            while k < a.len() {
                let c = a[k];
                match c {
                    b'a' | b'c' | b'e' | b'f' | b'G' | b'L' | b'n' | b'p' | b'r' | b's' | b'u'
                    | b'z' | b'Z' | b'y' | b'h' | b'V' => {
                        let key: &'static str = match c {
                            b'a' => "-a",
                            b'c' => "-c",
                            b'e' => "-e",
                            b'f' => "-f",
                            b'G' => "-G",
                            b'L' => "-L",
                            b'n' => "-n",
                            b'p' => "-p",
                            b'r' => "-r",
                            b's' => "-s",
                            b'u' => "-u",
                            b'z' => "-z",
                            b'Z' => "-Z",
                            b'y' => "-y",
                            b'h' => "-h",
                            _ => "-V",
                        };
                        items.push((key, None));
                        k += 1;
                    }
                    b'o' => {
                        // Argumento opcional: só colado ao `-o`.
                        let val = if k + 1 < a.len() { Some(a[k + 1..].to_vec()) } else { None };
                        items.push(("-o", val));
                        break;
                    }
                    b'g' | b'l' => {
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
                        items.push((if c == b'g' { "-g" } else { "-l" }, Some(val)));
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
            match key {
                "-h" => {
                    let mut out = io::stdout();
                    let _ = out.write_all(usage(&short).as_bytes());
                    return 0;
                }
                "-V" => {
                    let mut out = io::stdout();
                    let _ = out.write_all(
                        format!("{short} from util-linux 2.41.5 (features: lastlog2)\n").as_bytes(),
                    );
                    return 0;
                }
                "-c" | "-n" | "-r" | "-z" => {
                    match excl_first {
                        Some(f) if f != key => {
                            return errx(
                                &short,
                                "mutually exclusive arguments: --colon-separate --newline --raw --print0",
                            );
                        }
                        _ => excl_first = Some(key),
                    }
                    match key {
                        "-c" => mode = Mode::Colon,
                        "-n" => mode = Mode::Newline,
                        "-r" => mode = Mode::Raw,
                        _ => print0 = true,
                    }
                }
                "-y" => shellvar = true,
                "-a" => mode_flags.push("-a"),
                "-f" => mode_flags.push("-f"),
                "-G" => mode_flags.push("-G"),
                "-L" => mode_flags.push("-L"),
                "-p" => mode_flags.push("-p"),
                "-Z" => mode_flags.push("-Z"),
                "-e" => mode = Mode::Export,
                "-s" => want_sys = true,
                "-u" => want_user = true,
                "--noheadings" => noheadings = true,
                "--notruncate" => {}
                "--output-all" => output_all = true,
                "-o" => output = Some(val.unwrap_or_default()),
                "-g" => groups_sel.extend(split_list(&val.unwrap_or_default())),
                "-l" => logins.extend(split_list(&val.unwrap_or_default())),
                "--lastlog" | "--lastlog2" => {}
                "--time-format" => {
                    let v = val.unwrap_or_default();
                    time_fmt = match v.as_slice() {
                        b"short" => TimeFmt::Short,
                        b"full" => TimeFmt::Full,
                        b"iso" => TimeFmt::Iso,
                        _ => {
                            return errx(
                                &short,
                                format!("unknown time format: {}", io::lossy(&v)),
                            )
                        }
                    };
                }
                "--wtmp-file" => wtmp_path = io::lossy(&val.unwrap_or_default()).to_string(),
                "--btmp-file" => btmp_path = io::lossy(&val.unwrap_or_default()).to_string(),
                _ => {}
            }
        }
    }

    if operands.len() > 1 {
        return errx(
            &short,
            "Only one user may be specified. Use -l for multiple users.",
        );
    }
    if let Some(u) = operands.first() {
        logins.push(io::lossy(u).to_string());
    }

    // Colunas.
    let mut cols: Vec<usize> = Vec::new();
    if output_all {
        cols = (0..COLS.len()).collect();
    } else if let Some(list) = &output {
        let text = io::lossy(list).to_string();
        let (extend, body) = match text.strip_prefix('+') {
            Some(rest) => (true, rest.to_string()),
            None => (false, text),
        };
        if extend {
            cols = DEFAULT_COLS
                .iter()
                .filter_map(|n| col_index(n.as_bytes()))
                .collect();
        }
        for name in body.split(',') {
            if name.is_empty() {
                continue;
            }
            match col_index(name.as_bytes()) {
                Some(c) => cols.push(c),
                None => return errx(&short, format!("unknown column: {name}")),
            }
        }
        if cols.is_empty() && !extend && !body.is_empty() {
            return errx(&short, "no output columns specified");
        }
    }
    if cols.is_empty() && !output_all && output.as_ref().is_none_or(|o| o.is_empty()) {
        if mode_flags.is_empty() {
            cols = DEFAULT_COLS
                .iter()
                .filter_map(|n| col_index(n.as_bytes()))
                .collect();
        } else {
            let mut names: Vec<&str> = vec!["USER"];
            for f in &mode_flags {
                match *f {
                    "-a" => names.extend(["PWD-WARN", "PWD-MIN", "PWD-MAX", "PWD-CHANGE", "PWD-EXPIR"]),
                    "-f" => names.extend(["FAILED-LOGIN", "FAILED-TTY"]),
                    "-G" => names.extend(["GID", "GROUP", "SUPP-GIDS", "SUPP-GROUPS"]),
                    "-L" => names.extend(["LAST-LOGIN", "LAST-TTY", "LAST-HOSTNAME"]),
                    "-p" => names.extend(["PWD-EMPTY", "PWD-LOCK", "PWD-DENY"]),
                    "-Z" => names.push("CONTEXT"),
                    _ => {}
                }
            }
            for n in names {
                if let Some(c) = col_index(n.as_bytes()) {
                    if !cols.contains(&c) {
                        cols.push(c);
                    }
                }
            }
        }
    }

    // Dados.
    let Some(passwd) = read_text(passwd_path) else {
        return errx(&short, format!("cannot open {passwd_path}"));
    };
    let users_all = parse_passwd(&passwd);
    let groups = read_text(group_path).map(|t| parse_group(&t)).unwrap_or_default();
    let defs = read_text("/etc/login.defs").unwrap_or_default();
    let uid_min = login_defs(&defs, "UID_MIN").unwrap_or(1000);
    let uid_max = login_defs(&defs, "UID_MAX").unwrap_or(60000);
    let sys_uid_max = login_defs(&defs, "SYS_UID_MAX").unwrap_or(uid_min.saturating_sub(1));

    // Seleção por login (nome ou UID).
    for l in &logins {
        let found = users_all
            .iter()
            .any(|u| u.name == *l || l.parse::<u32>().is_ok_and(|n| n == u.uid));
        if !found {
            return errx(&short, format!("cannot found login: {l}"));
        }
    }
    // Seleção por grupo (nome ou GID).
    let mut sel_gids: Vec<u32> = Vec::new();
    for g in &groups_sel {
        match groups
            .iter()
            .find(|x| x.name == *g || g.parse::<u32>().is_ok_and(|n| n == x.gid))
        {
            Some(x) => sel_gids.push(x.gid),
            None => return errx(&short, format!("cannot found group: {g}")),
        }
    }

    let selected: Vec<&User> = users_all
        .iter()
        .filter(|u| {
            if !logins.is_empty()
                && !logins
                    .iter()
                    .any(|l| u.name == *l || l.parse::<u32>().is_ok_and(|n| n == u.uid))
            {
                return false;
            }
            if !sel_gids.is_empty() {
                let in_group = sel_gids.contains(&u.gid)
                    || groups
                        .iter()
                        .any(|g| sel_gids.contains(&g.gid) && g.members.iter().any(|m| *m == u.name));
                if !in_group {
                    return false;
                }
            }
            if logins.is_empty() && (want_user || want_sys) {
                let is_user = u.uid >= uid_min && u.uid <= uid_max;
                let is_sys = u.uid <= sys_uid_max;
                if !((want_user && is_user) || (want_sys && is_sys)) {
                    return false;
                }
            }
            true
        })
        .collect();

    let needs = |n: &str| col_index(n.as_bytes()).is_some_and(|c| cols.contains(&c));
    let ctx = Ctx {
        users: Vec::new(),
        groups,
        shadow_text: read_text("/etc/shadow"),
        wtmp: if needs("LAST-LOGIN") || needs("LAST-TTY") || needs("LAST-HOSTNAME") {
            read_utmp(&wtmp_path)
        } else {
            Vec::new()
        },
        btmp: if needs("FAILED-LOGIN") || needs("FAILED-TTY") {
            read_utmp(&btmp_path)
        } else {
            Vec::new()
        },
        procs: if needs("PROC") { count_procs() } else { Vec::new() },
        time_fmt,
    };
    let _ = &ctx.users;

    let rows: Vec<Vec<Option<String>>> = selected
        .iter()
        .map(|u| cols.iter().map(|c| ctx.value(*c, u)).collect())
        .collect();

    let eol = if print0 { '\0' } else { '\n' };
    let mut out = String::new();
    match mode {
        Mode::Export => {
            for row in &rows {
                let line: Vec<String> = cols
                    .iter()
                    .zip(row)
                    .map(|(c, v)| {
                        format!(
                            "{}=\"{}\"",
                            if shellvar { shell_name(COLS[*c].name) } else { COLS[*c].name.to_string() },
                            raw_escape(v.as_deref().unwrap_or(""))
                        )
                    })
                    .collect();
                out.push_str(&line.join(" "));
                out.push(eol);
            }
        }
        Mode::Raw | Mode::Colon => {
            let sep = if mode == Mode::Colon { ":" } else { " " };
            if !noheadings {
                let h: Vec<String> = cols
                    .iter()
                    .map(|c| if shellvar { shell_name(COLS[*c].name) } else { COLS[*c].name.to_string() })
                    .collect();
                out.push_str(&h.join(sep));
                out.push(eol);
            }
            for row in &rows {
                let cells: Vec<String> = row
                    .iter()
                    .map(|v| raw_escape(v.as_deref().unwrap_or("")))
                    .collect();
                out.push_str(&cells.join(sep));
                out.push(eol);
            }
        }
        Mode::Newline => {
            let w = cols.iter().map(|c| COLS[*c].name.len()).max().unwrap_or(0) + 1;
            for (n, row) in rows.iter().enumerate() {
                if n > 0 {
                    out.push(eol);
                }
                for (k, c) in cols.iter().enumerate() {
                    out.push_str(&format!(
                        "{:<w$} {}{}",
                        format!("{}:", COLS[*c].name),
                        row[k].as_deref().unwrap_or(""),
                        eol
                    ));
                }
            }
        }
        Mode::Table => {
            let mut widths: Vec<usize> = cols.iter().map(|c| COLS[*c].name.len()).collect();
            for row in &rows {
                for (k, v) in row.iter().enumerate() {
                    widths[k] = widths[k].max(v.as_deref().unwrap_or("").chars().count());
                }
            }
            let fmt_row = |cells: Vec<&str>| -> String {
                let mut line = String::new();
                for (k, cell) in cells.iter().enumerate() {
                    if k > 0 {
                        line.push(' ');
                    }
                    let pad = widths[k].saturating_sub(cell.chars().count());
                    let right = matches!(COLS[cols[k]].kind, Kind::Num | Kind::Flag);
                    if right {
                        line.push_str(&" ".repeat(pad));
                        line.push_str(cell);
                    } else if k + 1 < cols.len() {
                        line.push_str(cell);
                        line.push_str(&" ".repeat(pad));
                    } else {
                        line.push_str(cell);
                    }
                }
                line.push(eol);
                line
            };
            if !noheadings {
                out.push_str(&fmt_row(cols.iter().map(|c| COLS[*c].name).collect()));
            }
            for row in &rows {
                out.push_str(&fmt_row(
                    row.iter().map(|s| s.as_deref().unwrap_or("")).collect(),
                ));
            }
        }
    }

    let mut so = io::stdout();
    let _ = so.write_all(out.as_bytes());
    let _ = Fd::STDOUT;
    0
}

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}
