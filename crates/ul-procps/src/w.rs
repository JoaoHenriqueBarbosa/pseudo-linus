//! `w` do procps-ng 4.0.4 (src/w.c), no caminho do utmp (o do Debian quando o systemd não está
//! rodando, como num container).
//!
//! - Cabeçalho: a mesma linha do `uptime` e a linha `USER TTY FROM LOGIN@ IDLE JCPU PCPU WHAT`
//!   (`-s` troca o fim por `IDLE WHAT`; `-f` alterna a coluna FROM, que no Debian vem ligada).
//! - Cada registro `USER_PROCESS` com nome (ou com o nome pedido no operando) vira uma linha; o
//!   registro cujo `ut_pid` não existe mais é pulado, como no original (registro velho).
//! - O processo que aparece em WHAT é o mais novo do terminal que é líder do grupo em primeiro plano
//!   e pertence ao usuário (`-u` ignora o usuário); JCPU soma os tempos de todos os processos do
//!   terminal; PCPU é o do processo escolhido.
//! - Largura de WHAT: `COLUMNS` (sem ele, 512), menos as colunas fixas, presa entre 7 e 512.
//! - `PROCPS_USERLEN` e `PROCPS_FROMLEN` mudam as larguras, com os avisos do original.

use std::ffi::OsString;

use sysabi::{Ctx, Pid, sys};
use ul_misc::util::getopt::{Getopt, HasArg, LongOpt};
use ul_misc::util::{io, time};

use crate::common::{self, out};
use crate::procfs::{self, Want};
use crate::uptime;

const USAGE: &str = "\nUsage:\n w [options] [user]\n\nOptions:\n -h, --no-header     do not print header\n -u, --no-current    ignore current process username\n -s, --short         short format\n -t, --terminal      show terminals\n -f, --from          show remote hostname field\n -o, --old-style     old style output\n -i, --ip-addr       display IP address instead of hostname (if possible)\n -p, --pids          show the PID(s) of processes in WHAT\n\n     --help     display this help and exit\n -V, --version  output version information and exit\n\nFor more details see w(1).\n";

const HELP: i32 = 256;

const LONGS: &[LongOpt] = &[
    LongOpt::new("no-header", HasArg::No, 'h' as i32),
    LongOpt::new("no-current", HasArg::No, 'u' as i32),
    LongOpt::new("short", HasArg::No, 's' as i32),
    LongOpt::new("from", HasArg::No, 'f' as i32),
    LongOpt::new("old-style", HasArg::No, 'o' as i32),
    LongOpt::new("ip-addr", HasArg::No, 'i' as i32),
    LongOpt::new("pids", HasArg::No, 'p' as i32),
    LongOpt::new("help", HasArg::No, HELP),
    LongOpt::new("version", HasArg::No, 'V' as i32),
];

/// `sizeof(ut_user)` e `sizeof(ut_host)` do utmp da glibc.
const USERSZ: i64 = 32;
const HOSTSZ: i64 = 256;
const MIN_CMD_WIDTH: i64 = 7;
const MAX_CMD_WIDTH: i64 = 512;

/// Um registro do utmp da glibc x86_64 (384 bytes).
struct Utmp {
    kind: i16,
    pid: Pid,
    line: Vec<u8>,
    user: Vec<u8>,
    host: Vec<u8>,
    tv_sec: i64,
    addr_v6: [u32; 4],
}

fn cstr(b: &[u8]) -> Vec<u8> {
    let end = b.iter().position(|c| *c == 0).unwrap_or(b.len());
    b[..end].to_vec()
}

fn read_utmp() -> Vec<Utmp> {
    const RECORD: usize = 384;
    let Ok(data) = sys::read_file(b"/var/run/utmp") else { return Vec::new() };
    let le32 = |r: &[u8], o: usize| u32::from_le_bytes([r[o], r[o + 1], r[o + 2], r[o + 3]]);
    data.as_chunks::<RECORD>()
        .0
        .iter()
        .map(|r| Utmp {
            kind: i16::from_le_bytes([r[0], r[1]]),
            pid: le32(r, 4) as i32,
            line: r[8..40].to_vec(),
            user: cstr(&r[44..76]),
            host: r[76..332].to_vec(),
            tv_sec: i64::from(le32(r, 340) as i32),
            addr_v6: [le32(r, 348), le32(r, 352), le32(r, 356), le32(r, 360)],
        })
        .collect()
}

/// `atoi(3)`: espaço, sinal e dígitos do começo; o resto é ignorado.
fn atoi(s: &str) -> i64 {
    let t = s.trim_start_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r']);
    let (neg, rest) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let mut v: i64 = 0;
    for b in rest.bytes().take_while(u8::is_ascii_digit) {
        v = v.wrapping_mul(10).wrapping_add(i64::from(b - b'0'));
    }
    let v = if neg { -v } else { v };
    i64::from(v as i32)
}

fn env(name: &str) -> Option<String> {
    sysio::env::var_os(name).map(|v| v.to_string_lossy().into_owned())
}

struct Opts {
    longform: bool,
    from: bool,
    oldstyle: bool,
    ignoreuser: bool,
    ip_addresses: bool,
    pids: bool,
    userlen: usize,
    fromlen: usize,
    maxcmd: usize,
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let prog = argv0.rsplit('/').next().unwrap_or(&argv0).to_string();
    let mut header = true;
    let mut o = Opts {
        longform: true,
        from: true,
        oldstyle: false,
        ignoreuser: false,
        ip_addresses: false,
        pids: false,
        userlen: 8,
        fromlen: 16,
        maxcmd: 0,
    };
    let mut g = Getopt::from_env(&argv[1..], "husfoVip", LONGS);
    while let Some(r) = g.next_opt() {
        match r {
            Err(e) => {
                io::eprint(format!("{}\n{USAGE}", e.message(&argv0)));
                return 1;
            }
            Ok(opt) => match opt.id {
                HELP => {
                    out(USAGE);
                    return 0;
                }
                id => match u8::try_from(id).map(char::from) {
                    Ok('h') => header = false,
                    Ok('l') => o.longform = true,
                    Ok('s') => o.longform = false,
                    Ok('f') => o.from = !o.from,
                    Ok('V') => {
                        out("w from procps-ng 4.0.4\n");
                        return 0;
                    }
                    Ok('u') => o.ignoreuser = true,
                    Ok('o') => o.oldstyle = true,
                    Ok('i') => {
                        o.ip_addresses = true;
                        o.from = true;
                    }
                    Ok('p') => o.pids = true,
                    _ => unreachable!("tabela de opções do w"),
                },
            },
        }
    }
    let operands = g.operands();
    let user: Option<Vec<u8>> = operands.first().cloned();

    if let Some(v) = env("PROCPS_USERLEN") {
        let ul = atoi(&v);
        if !(8..=USERSZ).contains(&ul) {
            io::eprint(format!(
                "{prog}: User length environment PROCPS_USERLEN must be between 8 and {USERSZ}, ignoring.\n\n"
            ));
        } else {
            o.userlen = ul as usize;
        }
    }
    if let Some(v) = env("PROCPS_FROMLEN") {
        let fl = atoi(&v);
        if !(8..=HOSTSZ).contains(&fl) {
            io::eprint(format!(
                "{prog}: from length environment PROCPS_FROMLEN must be between 8 and {HOSTSZ}, ignoring\n\n"
            ));
            o.fromlen = 16;
        } else {
            o.fromlen = fl as usize;
        }
    }

    let mut maxcmd = match env("COLUMNS") {
        Some(c) => atoi(&c),
        None => MAX_CMD_WIDTH,
    };
    maxcmd = maxcmd.clamp(MIN_CMD_WIDTH, MAX_CMD_WIDTH);
    maxcmd -= 21 + o.userlen as i64 + if o.from { o.fromlen as i64 } else { 0 } + if o.longform { 20 } else { 0 };
    o.maxcmd = maxcmd.clamp(MIN_CMD_WIDTH, MAX_CMD_WIDTH) as usize;

    if header {
        let up = uptime::uptime_secs();
        let (now, _) = procfs::now_realtime();
        let dt = time::civil(now, &time::local_tz());
        let load = procfs::loadavg().unwrap_or_default();
        out(format!(
            " {:02}:{:02}:{:02} {}\n",
            dt.hour(),
            dt.minute(),
            dt.second(),
            uptime::up_and_load(up, common::utmp_users(), &load)
        ));
        let mut h = format!("{:<width$} TTY      ", "USER", width = o.userlen);
        if o.from {
            h.push_str(&format!("{:<width$}", "FROM", width = o.fromlen));
        }
        if o.longform {
            h.push_str(" LOGIN@   IDLE   JCPU   PCPU  WHAT\n");
        } else {
            h.push_str("   IDLE WHAT\n");
        }
        out(h);
    }

    let records = read_utmp();
    if records.iter().any(|u| u.kind == 7) {
        let snap = procfs::scan(Want { cmdline: true, ..Want::default() });
        let mut names = crate::common::Names::default();
        for u in &records {
            if u.kind != 7 {
                continue;
            }
            let wanted = match &user {
                Some(name) => {
                    let n = &name[..name.len().min(USERSZ as usize)];
                    u.user.as_slice() == n
                }
                None => !u.user.is_empty(),
            };
            if wanted {
                showinfo(u, &o, &snap.procs, &mut names);
            }
        }
    }
    0
}

/// O que `find_best_proc` devolve.
struct Best {
    jcpu: u64,
    pcpu: u64,
    cmdline: Vec<u8>,
    pid: Pid,
}

fn tics_all(p: &procfs::Proc) -> u64 {
    p.stat.utime + p.stat.stime
}

/// `find_best_proc` do w.c: `None` quando o `ut_pid` do registro não existe mais.
fn find_best_proc(u: &Utmp, tty: &str, o: &Opts, procs: &[procfs::Proc], names: &mut common::Names) -> Option<Best> {
    let uid = if o.ignoreuser { None } else { names.uid_of(&String::from_utf8_lossy(&u.user)) };
    let line = sys::stat(format!("/dev/{tty}").as_bytes()).ok().map(|st| st.rdev);
    let mut best = Best { jcpu: 0, pcpu: 0, cmdline: b"-".to_vec(), pid: -1 };
    let mut best_time: u64 = 0;
    let mut secondbest_time: u64 = 0;
    let mut found_utpid = false;
    for p in procs {
        let start = p.stat.starttime;
        if p.pid() == u.pid {
            found_utpid = true;
            if best_time == 0 {
                best_time = start;
                best.cmdline = p.cmdline_string();
                best.pid = p.pid();
                best.pcpu = tics_all(p);
            }
        }
        let on_line = p.stat.tty_nr != 0 && Some(common::tty_nr_dev(p.stat.tty_nr)) == line;
        if !on_line {
            continue;
        }
        best.jcpu += tics_all(p);
        if !(secondbest_time != 0 && start <= secondbest_time) {
            secondbest_time = start;
            if best.cmdline == b"-" {
                best.cmdline = p.cmdline_string();
                best.pid = p.pid();
                best.pcpu = tics_all(p);
            }
        }
        let owner_mismatch = !o.ignoreuser && uid != Some(p.euid());
        if owner_mismatch || p.stat.pgrp != p.stat.tpgid || start <= best_time {
            continue;
        }
        best_time = start;
        best.cmdline = p.cmdline_string();
        best.pid = p.pid();
        best.pcpu = tics_all(p);
    }
    found_utpid.then_some(best)
}

/// `print_time_ival7`: sete colunas pra um intervalo, no formato novo ou no antigo (`-o`).
fn time_ival7(t: i64, centi: u64, oldstyle: bool) -> String {
    let t = t.max(0) as u64;
    if oldstyle {
        if t >= 48 * 3600 {
            format!(" {:2}days", t / 86_400)
        } else if t >= 3600 {
            format!(" {:2}:{:02} ", t / 3600, (t / 60) % 60)
        } else if t > 60 {
            format!("    {:2} ", t / 60)
        } else {
            "       ".to_string()
        }
    } else if t >= 48 * 3600 {
        format!(" {:3}days", t / 86_400)
    } else if t >= 3600 {
        format!(" {:2}:{:02}m", t / 3600, (t / 60) % 60)
    } else if t >= 60 {
        format!(" {:2}:{:02} ", t / 60, t % 60)
    } else {
        format!(" {:2}.{:02}s", t, centi)
    }
}

/// `print_logintime`: hora e minuto no mesmo dia (ou há menos de 12 horas), dia da semana e hora
/// na mesma semana, senão dia, mês e ano.
fn logintime(logt: i64, now: i64) -> String {
    let tz = time::local_tz();
    let cur = time::civil(now, &tz);
    let log = time::civil(logt, &tz);
    if now - logt > 12 * 3600 && log.day_of_year() != cur.day_of_year() {
        if now - logt > 6 * 86_400 {
            format!(" {:02}{:>3}{:02}", log.day(), time::MONTHS[log.month() as usize - 1], log.year().rem_euclid(100))
        } else {
            format!(" {:>3}{:02}  ", time::WEEKDAYS[time::wday(&log)], log.hour())
        }
    } else {
        format!(" {:02}:{:02}  ", log.hour(), log.minute())
    }
}

/// `print_host`: até `len` bytes imprimíveis sem espaço; o primeiro que não é vira `-` e encerra.
/// Completa com espaços até `fromlen`. Devolve a largura escrita sem o preenchimento.
fn host_field(host: &[u8], len: usize, fromlen: usize, s: &mut Vec<u8>) -> usize {
    let len = len.min(fromlen);
    let mut width = 0;
    for &c in host.iter().take(len) {
        if c == 0 {
            break;
        }
        if (0x20..0x7f).contains(&c) && c != b' ' {
            s.push(c);
            width += 1;
        } else {
            s.push(b'-');
            width += 1;
            break;
        }
    }
    let written = width;
    if width == 0 {
        s.push(b'-');
        width += 1;
    }
    while width < fromlen {
        s.push(b' ');
        width += 1;
    }
    written
}

/// `print_display_or_interface`: o `:display` do `ut_host` (quando há um só `:`) depois do IP.
fn display_or_interface(host: &[u8], restlen: i64, s: &mut Vec<u8>) {
    if restlen <= 0 {
        return;
    }
    let mut restlen = restlen;
    let printable = |c: u8| (0x20..0x7f).contains(&c);
    let disp = host.iter().position(|&c| c == b':' || !printable(c));
    if let Some(d) = disp
        && host[d] == b':'
    {
        let tail = &host[d + 1..];
        let next = tail.iter().position(|&c| c == b':' || !printable(c));
        let multiple = next.is_some_and(|n| tail[n] == b':');
        if !multiple {
            let len = ((host.len() - d) as i64).min(restlen) as usize;
            let w = host_field(&host[d..], len, len, s) as i64;
            restlen -= w.max(1);
        }
    }
    if restlen > 0 {
        s.push(b' ');
        s.extend(std::iter::repeat_n(b' ', restlen as usize - 1));
    }
}

fn from_field(u: &Utmp, o: &Opts, s: &mut Vec<u8>) {
    if o.ip_addresses {
        let a = u.addr_v6;
        let ip = if a[1] != 0 || a[2] != 0 || a[3] != 0 {
            let mut b = [0u8; 16];
            for (i, w) in a.iter().enumerate() {
                b[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
            }
            Some(std::net::Ipv6Addr::from(b).to_string())
        } else if a[0] != 0 {
            Some(std::net::Ipv4Addr::from(a[0].to_le_bytes()).to_string())
        } else {
            None
        };
        if let Some(ip) = ip {
            s.extend_from_slice(ip.as_bytes());
            display_or_interface(&u.host, o.fromlen as i64 - ip.len() as i64, s);
            return;
        }
    }
    host_field(&u.host, u.host.len(), o.fromlen, s);
}

fn showinfo(u: &Utmp, o: &Opts, procs: &[procfs::Proc], names: &mut common::Names) {
    // A linha do terminal sem os caracteres estranhos (o original corta no primeiro que não é
    // alfanumérico nem `/`).
    let tty: String = u
        .line
        .iter()
        .take_while(|c| c.is_ascii_alphanumeric() || **c == b'/')
        .map(|c| char::from(*c))
        .collect();
    let Some(best) = find_best_proc(u, &tty, o, procs, names) else { return };
    let (now, _) = procfs::now_realtime();
    let line = cstr(&u.line);
    let mut s: Vec<u8> = Vec::new();
    let user = &u.user[..u.user.len().min(o.userlen)];
    s.extend_from_slice(user);
    s.extend(std::iter::repeat_n(b' ', o.userlen + 1 - user.len()));
    let l = &line[..line.len().min(8)];
    s.extend_from_slice(l);
    s.extend(std::iter::repeat_n(b' ', 9 - l.len()));
    if o.from {
        from_field(u, o, &mut s);
    }
    let idle = || -> i64 {
        match sys::stat(format!("/dev/{tty}").as_bytes()) {
            Ok(st) => now - st.atime.sec,
            Err(_) => 0,
        }
    };
    let hz = procfs::HZ;
    if o.longform {
        s.extend_from_slice(logintime(u.tv_sec, now).as_bytes());
        if line.first() == Some(&b':') {
            s.extend_from_slice(b" ?xdm? ");
        } else {
            s.extend_from_slice(time_ival7(idle(), 0, o.oldstyle).as_bytes());
        }
        s.extend_from_slice(time_ival7((best.jcpu / hz) as i64, (best.jcpu % hz) * (100 / hz), o.oldstyle).as_bytes());
        if best.pcpu > 0 {
            s.extend_from_slice(time_ival7((best.pcpu / hz) as i64, (best.pcpu % hz) * (100 / hz), o.oldstyle).as_bytes());
        } else {
            s.extend_from_slice(b"   ?   ");
        }
    } else if line.first() == Some(&b':') {
        s.extend_from_slice(b" ?xdm? ");
    } else {
        s.extend_from_slice(time_ival7(idle(), 0, o.oldstyle).as_bytes());
    }
    let mut pids_length = 0;
    if o.pids {
        let text = if u.pid == best.pid { format!(" {}", u.pid) } else { format!(" {}/{}", u.pid, best.pid) };
        pids_length = text.len();
        s.extend_from_slice(text.as_bytes());
    }
    s.push(b' ');
    let width = o.maxcmd.saturating_sub(pids_length);
    s.extend_from_slice(&best.cmdline[..best.cmdline.len().min(width)]);
    s.push(b'\n');
    out(s);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intervals() {
        assert_eq!(time_ival7(0, 3, false), "  0.03s");
        assert_eq!(time_ival7(75, 0, false), "  1:15 ");
        assert_eq!(time_ival7(3700, 0, false), "  1:01m");
        assert_eq!(time_ival7(200_000, 0, false), "   2days");
        assert_eq!(time_ival7(30, 0, true), "       ");
        assert_eq!(time_ival7(120, 0, true), "     2 ");
    }

    #[test]
    fn host_column() {
        let mut s = Vec::new();
        host_field(b"10.0.0.1\0", 256, 16, &mut s);
        assert_eq!(s, b"10.0.0.1        ");
        let mut s = Vec::new();
        host_field(b"\0", 256, 16, &mut s);
        assert_eq!(s, b"-               ");
        assert_eq!(atoi(" 12x"), 12);
    }
}
