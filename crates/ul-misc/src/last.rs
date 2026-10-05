//! `last` e `lastb` do util-linux 2.41 (o mesmo binário; o nome do `argv[0]` escolhe o arquivo
//! padrão, `/var/log/wtmp` ou `/var/log/btmp`).
//!
//! Lê o arquivo utmp binário (`struct utmp` do x86_64, 384 bytes) do mais novo pro mais antigo,
//! casa cada login com o logout, o desligamento ou a reinicialização seguinte e imprime as linhas.

use std::ffi::OsString;
use std::io::Write;

use sysabi::Errno;

use crate::util::io;
use crate::util::time;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

pub const UTMP_SIZE: usize = 384;

const RUN_LVL: i16 = 1;
const BOOT_TIME: i16 = 2;
const LOGIN_PROCESS: i16 = 6;
const USER_PROCESS: i16 = 7;
const DEAD_PROCESS: i16 = 8;

/// Um registro `struct utmp` já decodificado.
pub struct Utmp {
    pub kind: i16,
    pub pid: i32,
    pub line: Vec<u8>,
    pub id: Vec<u8>,
    pub user: Vec<u8>,
    pub host: Vec<u8>,
    pub sec: i32,
    pub usec: i32,
    pub addr: [i32; 4],
}

fn cstr(b: &[u8]) -> Vec<u8> {
    let end = b.iter().position(|c| *c == 0).unwrap_or(b.len());
    b[..end].to_vec()
}

fn i32_at(b: &[u8], off: usize) -> i32 {
    i32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

pub fn decode(rec: &[u8]) -> Utmp {
    Utmp {
        kind: i16::from_le_bytes([rec[0], rec[1]]),
        pid: i32_at(rec, 4),
        line: cstr(&rec[8..40]),
        id: cstr(&rec[40..44]),
        user: cstr(&rec[44..76]),
        host: cstr(&rec[76..332]),
        sec: i32_at(rec, 340),
        usec: i32_at(rec, 344),
        addr: [
            i32_at(rec, 348),
            i32_at(rec, 352),
            i32_at(rec, 356),
            i32_at(rec, 360),
        ],
    }
}

const LONGS: &[LongOpt] = &[
    LongOpt::new("hostlast", HasArg::No, b'a' as i32),
    LongOpt::new("dns", HasArg::No, b'd' as i32),
    LongOpt::new("file", HasArg::Required, b'f' as i32),
    LongOpt::new("fulltimes", HasArg::No, b'F' as i32),
    LongOpt::new("ip", HasArg::No, b'i' as i32),
    LongOpt::new("limit", HasArg::Required, b'n' as i32),
    LongOpt::new("nohostname", HasArg::No, b'R' as i32),
    LongOpt::new("since", HasArg::Required, b's' as i32),
    LongOpt::new("until", HasArg::Required, b't' as i32),
    LongOpt::new("present", HasArg::Required, b'p' as i32),
    LongOpt::new("fulllogin", HasArg::No, b'w' as i32),
    LongOpt::new("system", HasArg::No, b'x' as i32),
    LongOpt::new("time-format", HasArg::Required, 0x100),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
];

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn usage(short: &str, default_file: &str) -> String {
    format!(
        "
Usage:
 {short} [options] [<username>...] [<tty>...]

Show a listing of last logged in users.

Options:
 -<number>            how many lines to show
 -a, --hostlast       display hostnames in the last column
 -d, --dns            translate the IP number back into a hostname
 -f, --file <file>    use a specific file instead of {default_file}
 -F, --fulltimes      print full login and logout times and dates
 -i, --ip             display IP numbers in numbers-and-dots notation
 -n, --limit <number> how many lines to show
 -R, --nohostname     don't display the hostname field
 -s, --since <time>   display the lines since the specified time
 -t, --until <time>   display the lines until the specified time
 -p, --present <time> display who was present at the specified time
 -w, --fulllogin      show full user and domain names in the listing
 -x, --system         display system shutdown entries and run level changes
     --time-format <format>  show timestamps in the specified <format>:
                               notime|short|full|iso

 -h, --help     display this help
 -V, --version  display version

For more details see last(1).
"
    )
}

#[derive(Clone, Copy, PartialEq)]
enum TimeFmt {
    NoTime,
    Short,
    Full,
    Iso,
}

struct Opts {
    lastb: bool,
    hostlast: bool,
    ip: bool,
    nohost: bool,
    fulllogin: bool,
    system: bool,
    fmt: TimeFmt,
    limit: Option<u64>,
    since: Option<i64>,
    until: Option<i64>,
    present: Option<i64>,
    filters: Vec<Vec<u8>>,
}

/// Interpreta `YYYY-MM-DD[ HH:MM[:SS]]`, `now`, `today`, `yesterday`, `tomorrow` no fuso local.
fn parse_time(s: &str) -> Option<i64> {
    let tz = time::local_tz();
    let now = time::now().sec;
    let day = 86400;
    let midnight = |t: i64| -> i64 {
        let dt = time::civil(t, &tz);
        let secs = i64::from(dt.hour()) * 3600 + i64::from(dt.minute()) * 60 + i64::from(dt.second());
        t - secs
    };
    match s {
        "now" => return Some(now),
        "today" => return Some(midnight(now)),
        "yesterday" => return Some(midnight(now) - day),
        "tomorrow" => return Some(midnight(now) + day),
        _ => {}
    }
    let (date, clock) = match s.split_once([' ', 'T']) {
        Some((d, c)) => (d, c),
        None => (s, "00:00:00"),
    };
    let mut dp = date.split('-');
    let y: i16 = dp.next()?.parse().ok()?;
    let m: i8 = dp.next()?.parse().ok()?;
    let d: i8 = dp.next()?.parse().ok()?;
    let mut cp = clock.split(':');
    let hh: i8 = cp.next()?.parse().ok()?;
    let mm: i8 = cp.next().map_or(Some(0), |v| v.parse().ok())?;
    let ss: i8 = cp.next().map_or(Some(0), |v| v.parse().ok())?;
    let dt = jiff::civil::DateTime::new(y, m, d, hh, mm, ss, 0).ok()?;
    let z = dt.to_zoned(tz).ok()?;
    Some(z.timestamp().as_second())
}

fn fmt_time(t: i64, fmt: TimeFmt, login: bool) -> String {
    let tz = time::local_tz();
    let dt = time::civil(t, &tz);
    match fmt {
        TimeFmt::NoTime => String::new(),
        TimeFmt::Short => {
            if login {
                format!(
                    "{} {} {:2} {:02}:{:02}",
                    time::WEEKDAYS[time::wday(&dt)],
                    time::MONTHS[dt.month() as usize - 1],
                    dt.day(),
                    dt.hour(),
                    dt.minute()
                )
            } else {
                format!("{:02}:{:02}", dt.hour(), dt.minute())
            }
        }
        TimeFmt::Full => time::ctime(t, &tz),
        TimeFmt::Iso => {
            let off = jiff::Timestamp::from_second(t)
                .map(|ts| tz.to_offset_info(ts).offset().seconds())
                .unwrap_or(0);
            let sign = if off < 0 { '-' } else { '+' };
            let a = off.abs();
            format!(
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{}{:02}:{:02}",
                dt.year(),
                dt.month(),
                dt.day(),
                dt.hour(),
                dt.minute(),
                dt.second(),
                sign,
                a / 3600,
                (a % 3600) / 60
            )
        }
    }
}

fn duration(secs: i64) -> String {
    let secs = secs.max(0);
    let (d, h, m) = (secs / 86400, (secs % 86400) / 60 / 60, (secs % 3600) / 60);
    if d > 0 {
        format!("({d}+{h:02}:{m:02})")
    } else {
        format!("({h:02}:{m:02})")
    }
}

fn host_text(u: &Utmp, o: &Opts) -> String {
    if o.ip {
        let a = u.addr;
        if a[1] == 0 && a[2] == 0 && a[3] == 0 {
            let b = a[0].to_le_bytes();
            return format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3]);
        }
    }
    io::lossy(&u.host)
}

struct Row {
    user: String,
    line: String,
    host: String,
    login: i64,
    /// `None` quando ainda logado; o texto especial quando não há horário de saída.
    logout: Logout,
}

enum Logout {
    Still,
    At(i64),
    Down(i64),
    Crash(i64),
    Gone,
    None,
}

fn emit(row: &Row, o: &Opts) {
    let mut out = io::stdout();
    let (user, line) = if o.fulllogin {
        (row.user.clone(), row.line.clone())
    } else {
        (trunc(&row.user, 8), trunc(&row.line, 12))
    };
    let mut s = format!("{user:<8} {line:<12} ");
    if !o.nohost && !o.hostlast {
        let h = if o.fulllogin { row.host.clone() } else { trunc(&row.host, 16) };
        s.push_str(&format!("{h:<16} "));
    }
    let lt = fmt_time(row.login, o.fmt, true);
    let (outt, len): (String, String) = match row.logout {
        Logout::Still => ("  still".into(), "logged in".into()),
        Logout::At(t) => {
            let end = fmt_time(t, o.fmt, false);
            (format!("- {end}"), duration(t - row.login))
        }
        Logout::Down(t) => (format!("- {}", fmt_time(t, o.fmt, false)), "down".into()),
        Logout::Crash(t) => (format!("- {}", fmt_time(t, o.fmt, false)), "crash".into()),
        Logout::Gone => (" gone".into(), "- no logout".into()),
        Logout::None => (String::new(), String::new()),
    };
    if o.fmt == TimeFmt::NoTime {
        s.push_str(&format!("{len}"));
    } else if o.fmt == TimeFmt::Short {
        s.push_str(&format!("{lt:<16} {outt:<7} {len}"));
    } else {
        let end = match row.logout {
            Logout::At(t) | Logout::Down(t) | Logout::Crash(t) => {
                format!("- {}", fmt_time(t, o.fmt, false).replace(" ", " "))
            }
            _ => outt,
        };
        s.push_str(&format!("{lt} {end} {len}"));
    }
    if o.hostlast && !o.nohost {
        s.push(' ');
        s.push_str(&row.host);
    }
    let _ = out.write_all(format!("{}\n", s.trim_end()).as_bytes());
}

fn trunc(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn matches(o: &Opts, user: &[u8], line: &[u8]) -> bool {
    if o.filters.is_empty() {
        return true;
    }
    o.filters.iter().any(|f| {
        f == user || f == line || {
            let l = line.strip_prefix(b"tty").unwrap_or(line);
            f == l
        }
    })
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);
    let lastb = short == "lastb";
    let default_file: &str = if lastb { "/var/log/btmp" } else { "/var/log/wtmp" };

    let mut o = Opts {
        lastb,
        hostlast: false,
        ip: false,
        nohost: false,
        fulllogin: false,
        system: false,
        fmt: TimeFmt::Short,
        limit: None,
        since: None,
        until: None,
        present: None,
        filters: Vec::new(),
    };
    let mut file: Vec<u8> = default_file.as_bytes().to_vec();
    let mut digits: Option<u64> = None;

    let mut g = Getopt::from_env(&argv[1..], "hVf:n:RxFaditws:p:t:0123456789", LONGS);
    while let Some(r) = g.next_opt() {
        let opt = match r {
            Ok(v) => v,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                ul::errtryhelp(&short);
                return 1;
            }
        };
        let bad_time = |a: &str| {
            io::eprint(format!("{short}: invalid time value \"{a}\"\n"));
            1
        };
        if opt.id == 0x100 {
            o.fmt = match opt.arg_str().as_str() {
                "notime" => TimeFmt::NoTime,
                "short" => TimeFmt::Short,
                "full" => TimeFmt::Full,
                "iso" => TimeFmt::Iso,
                other => {
                    io::eprint(format!("{short}: unknown time format: {other}\n"));
                    return 1;
                }
            };
            continue;
        }
        match opt.short() {
            Some(c @ '0'..='9') => {
                let d = u64::from(c as u8 - b'0');
                digits = Some(digits.unwrap_or(0).saturating_mul(10).saturating_add(d));
            }
            Some('a') => o.hostlast = true,
            Some('d') | Some('i') => o.ip = true,
            Some('f') => file = opt.arg.clone().unwrap_or_default(),
            Some('F') => o.fmt = TimeFmt::Full,
            Some('n') => match ul::strtou32_or_err(opt.arg.as_deref().unwrap_or(b""), "failed to parse number") {
                Ok(n) => o.limit = Some(u64::from(n)),
                Err(m) => {
                    io::eprint(format!("{short}: {m}\n"));
                    return 1;
                }
            },
            Some('R') => o.nohost = true,
            Some('w') => o.fulllogin = true,
            Some('x') => o.system = true,
            Some('s') => match parse_time(&opt.arg_str()) {
                Some(t) => o.since = Some(t),
                None => return bad_time(&opt.arg_str()),
            },
            Some('t') => match parse_time(&opt.arg_str()) {
                Some(t) => o.until = Some(t),
                None => return bad_time(&opt.arg_str()),
            },
            Some('p') => match parse_time(&opt.arg_str()) {
                Some(t) => o.present = Some(t),
                None => return bad_time(&opt.arg_str()),
            },
            Some('h') => {
                let mut out = io::stdout();
                let _ = out.write_all(usage(&short, default_file).as_bytes());
                return 0;
            }
            Some('V') => {
                ul::print_version(&short);
                return 0;
            }
            _ => {
                ul::errtryhelp(&short);
                return 1;
            }
        }
    }
    if let Some(n) = digits {
        o.limit = Some(n);
    }
    o.filters = g.operands();

    let data = match io::read_path(&file) {
        Ok(d) => d,
        Err(e) => {
            ul::warn(&short, format!("cannot open {}", io::lossy(&file)), e);
            return 1;
        }
    };
    let begintime = if data.len() >= UTMP_SIZE {
        i64::from(decode(&data[..UTMP_SIZE]).sec)
    } else {
        sysabi::sys::stat(&file).map(|s| s.ctime.sec).unwrap_or(0)
    };

    let recs: Vec<Utmp> = data.chunks_exact(UTMP_SIZE).map(decode).collect();
    let mut printed: u64 = 0;
    let mut pending: Vec<(Vec<u8>, i64)> = Vec::new();
    let mut boot: Option<i64> = None;
    let mut down: Option<i64> = None;
    let mut seen_reboot = false;
    let limit_hit = |n: u64, o: &Opts| o.limit.is_some_and(|l| n >= l);

    for u in recs.iter().rev() {
        if limit_hit(printed, &o) {
            break;
        }
        let t = i64::from(u.sec);
        if o.until.is_some_and(|x| t > x) {
            continue;
        }
        if o.since.is_some_and(|x| t < x) {
            break;
        }
        let line = u.line.strip_prefix(b"/dev/").unwrap_or(&u.line).to_vec();
        if o.lastb {
            if u.kind == USER_PROCESS || u.kind == LOGIN_PROCESS {
                if matches(&o, &u.user, &line) {
                    emit(
                        &Row {
                            user: io::lossy(&u.user),
                            line: io::lossy(&line),
                            host: host_text(u, &o),
                            login: t,
                            logout: Logout::None,
                        },
                        &o,
                    );
                    printed += 1;
                }
            }
            continue;
        }
        match u.kind {
            RUN_LVL => {
                if u.user == b"shutdown" || u.line == b"~" && u.user == b"shutdown" {
                    down = Some(t);
                    if o.system && matches(&o, b"shutdown", b"system down") {
                        emit(
                            &Row {
                                user: "shutdown".into(),
                                line: "system down".into(),
                                host: io::lossy(&u.host),
                                login: t,
                                logout: Logout::At(boot.unwrap_or(t)),
                            },
                            &o,
                        );
                        printed += 1;
                    }
                } else if o.system {
                    let lvl = (u.pid & 0xff) as u8 as char;
                    let text = format!("(to lvl {lvl})");
                    emit(
                        &Row {
                            user: "runlevel".into(),
                            line: text,
                            host: io::lossy(&u.host),
                            login: t,
                            logout: Logout::None,
                        },
                        &o,
                    );
                    printed += 1;
                }
            }
            BOOT_TIME => {
                if matches(&o, b"reboot", b"system boot") {
                    emit(
                        &Row {
                            user: "reboot".into(),
                            line: "system boot".into(),
                            host: io::lossy(&u.host),
                            login: t,
                            logout: match down {
                                _ if !seen_reboot => Logout::Still,
                                _ => Logout::At(boot.unwrap_or(t)),
                            },
                        },
                        &o,
                    );
                    printed += 1;
                }
                seen_reboot = true;
                boot = Some(t);
                down = None;
                pending.clear();
            }
            DEAD_PROCESS => pending.push((line, t)),
            USER_PROCESS => {
                let idx = pending.iter().position(|(l, _)| *l == line);
                let logout = match idx {
                    Some(i) => {
                        let (_, lt) = pending.remove(i);
                        Logout::At(lt)
                    }
                    None => match (seen_reboot, down, boot) {
                        (false, _, _) => Logout::Still,
                        (true, Some(d), _) => Logout::Down(d),
                        (true, None, Some(b)) => Logout::Crash(b),
                        (true, None, None) => Logout::Gone,
                    },
                };
                if let Some(p) = o.present {
                    let end = match logout {
                        Logout::Still => i64::MAX,
                        Logout::At(x) | Logout::Down(x) | Logout::Crash(x) => x,
                        _ => t,
                    };
                    if !(t <= p && p <= end) {
                        continue;
                    }
                }
                if !matches(&o, &u.user, &line) {
                    continue;
                }
                emit(
                    &Row {
                        user: io::lossy(&u.user),
                        line: io::lossy(&line),
                        host: host_text(u, &o),
                        login: t,
                        logout,
                    },
                    &o,
                );
                printed += 1;
            }
            _ => {}
        }
    }

    let base = file.rsplit(|b| *b == b'/').next().unwrap_or(&file);
    let tz = time::local_tz();
    let mut out = io::stdout();
    let _ = out.write_all(
        format!("\n{} begins {}\n", io::lossy(base), time::ctime(begintime, &tz)).as_bytes(),
    );
    let _ = Errno::ENOENT;
    0
}
