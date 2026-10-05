//! `dmesg` do util-linux 2.41: mostra ou controla o buffer de anéis do kernel.
//!
//! Sem `CAP_SYSLOG` (o caso de um contêiner sem privilégio) a leitura do buffer falha com
//! "read kernel buffer failed: Operation not permitted" e as ações de controle com "klogctl failed".
//! Com `-F <arquivo>` lê as mensagens do arquivo, no formato `<prioridade>texto`.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Errno};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const O_NOESCAPE: i32 = 256;
const O_TIME_FORMAT: i32 = 257;
const O_SINCE: i32 = 258;
const O_UNTIL: i32 = 259;

const LONGS: &[LongOpt] = &[
    LongOpt::new("buffer-size", HasArg::Required, b's' as i32),
    LongOpt::new("clear", HasArg::No, b'C' as i32),
    LongOpt::new("color", HasArg::Optional, b'L' as i32),
    LongOpt::new("console-level", HasArg::Required, b'n' as i32),
    LongOpt::new("console-off", HasArg::No, b'D' as i32),
    LongOpt::new("console-on", HasArg::No, b'E' as i32),
    LongOpt::new("ctime", HasArg::No, b'T' as i32),
    LongOpt::new("decode", HasArg::No, b'x' as i32),
    LongOpt::new("file", HasArg::Required, b'F' as i32),
    LongOpt::new("facility", HasArg::Required, b'f' as i32),
    LongOpt::new("follow", HasArg::No, b'w' as i32),
    LongOpt::new("follow-new", HasArg::No, b'W' as i32),
    LongOpt::new("kmsg-file", HasArg::Required, b'K' as i32),
    LongOpt::new("since", HasArg::Required, O_SINCE),
    LongOpt::new("until", HasArg::Required, O_UNTIL),
    LongOpt::new("force-prefix", HasArg::No, b'p' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("human", HasArg::No, b'H' as i32),
    LongOpt::new("json", HasArg::No, b'J' as i32),
    LongOpt::new("kernel", HasArg::No, b'k' as i32),
    LongOpt::new("level", HasArg::Required, b'l' as i32),
    LongOpt::new("noescape", HasArg::No, O_NOESCAPE),
    LongOpt::new("notime", HasArg::No, b't' as i32),
    LongOpt::new("nopager", HasArg::No, b'P' as i32),
    LongOpt::new("raw", HasArg::No, b'r' as i32),
    LongOpt::new("read-clear", HasArg::No, b'c' as i32),
    LongOpt::new("reltime", HasArg::No, b'e' as i32),
    LongOpt::new("show-delta", HasArg::No, b'd' as i32),
    LongOpt::new("syslog", HasArg::No, b'S' as i32),
    LongOpt::new("time-format", HasArg::Required, O_TIME_FORMAT),
    LongOpt::new("userspace", HasArg::No, b'u' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
];

const USAGE: &str = "
Usage:
 dmesg [options]

Display or control the kernel ring buffer.

Options:
 -C, --clear                 clear the kernel ring buffer
 -c, --read-clear            read and clear all messages
 -D, --console-off           disable printing messages to console
 -E, --console-on            enable printing messages to console
 -F, --file <file>           use the file instead of the kernel log buffer
 -K, --kmsg-file <file>      use the file in kmsg format
 -f, --facility <list>       restrict output to defined facilities
 -H, --human                 human readable output
 -J, --json                  use JSON output format
 -k, --kernel                display kernel messages
 -L, --color[=<when>]        colorize messages (auto, always or never)
                               colors are enabled by default
 -l, --level <list>          restrict output to defined levels
 -n, --console-level <level> set level of messages printed to console
 -P, --nopager               do not pipe output into a pager
 -p, --force-prefix          force timestamp output on each line of multi-line messages
 -r, --raw                   print the raw message buffer
     --noescape              don't escape unprintable character
 -S, --syslog                force to use syslog(2) rather than /dev/kmsg
 -s, --buffer-size <size>    buffer size to query the kernel ring buffer
 -u, --userspace             display userspace messages
 -w, --follow                wait for new messages
 -W, --follow-new            wait and print only new messages
 -x, --decode                decode facility and level to readable string
 -d, --show-delta            show time delta between printed messages
 -e, --reltime               show local time and time delta in readable format
 -T, --ctime                 show human-readable timestamp (may be inaccurate!)
 -t, --notime                don't show any timestamp with messages
     --time-format <format>  show timestamp using the given format:
                               [delta|reltime|ctime|notime|iso|raw]
Suspending/resume will make ctime and iso timestamps inaccurate.
     --since <time>          display the lines since the specified time
     --until <time>          display the lines until the specified time

 -h, --help                  display this help
 -V, --version               display version

Supported log facilities:
    kern - kernel messages
    user - random user-level messages
    mail - mail system
  daemon - system daemons
    auth - security/authorization messages
  syslog - messages generated internally by syslogd
     lpr - line printer subsystem
    news - network news subsystem
    uucp - UUCP subsystem
    cron - clock daemon
 authpriv - security/authorization messages (private)
     ftp - FTP daemon
    res0 - reserved 0
    res1 - reserved 1
    res2 - reserved 2
    res3 - reserved 3
  local0 - local use 0
  local1 - local use 1
  local2 - local use 2
  local3 - local use 3
  local4 - local use 4
  local5 - local use 5
  local6 - local use 6
  local7 - local use 7

Supported log levels (priorities):
   emerg - system is unusable
   alert - action must be taken immediately
    crit - critical conditions
     err - error conditions
    warn - warning conditions
  notice - normal but significant condition
    info - informational
   debug - debug-level messages

For more details see dmesg(1).
";

const LEVELS: &[&str] = &[
    "emerg", "alert", "crit", "err", "warn", "notice", "info", "debug",
];
const FACILITIES: &[&str] = &[
    "kern", "user", "mail", "daemon", "auth", "syslog", "lpr", "news", "uucp", "cron", "authpriv",
    "ftp", "res0", "res1", "res2", "res3", "local0", "local1", "local2", "local3", "local4",
    "local5", "local6", "local7",
];

/// Lê uma lista de nomes (ou números) separada por vírgulas e devolve a máscara.
fn parse_list(list: &[u8], names: &[&str], what: &str, short: &str) -> Result<u32, ()> {
    let mut mask = 0u32;
    for item in io::lossy(list).split(',') {
        if let Some(i) = names.iter().position(|n| *n == item) {
            mask |= 1 << i;
        } else if let Ok(n) = item.parse::<usize>()
            && n < names.len()
        {
            mask |= 1 << n;
        } else {
            ul::warnx(short, format!("unknown {what} '{item}'"));
            return Err(());
        }
    }
    Ok(mask)
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut control: Option<char> = None;
    let mut file: Option<Vec<u8>> = None;
    let mut raw = false;
    let mut level_mask = 0xffu32;
    let mut fac_mask = 0xff_ffffu32;
    let mut follow = false;
    let mut g = Getopt::from_env(&argv[1..], "CcDEF:K:f:HJkL::l:n:Pprs:uWwxdeTtVh", LONGS);
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                ul::errtryhelp(&short);
                return 1;
            }
        };
        let arg = o.arg.clone().unwrap_or_default();
        match o.id {
            O_NOESCAPE | O_SINCE | O_UNTIL => continue,
            O_TIME_FORMAT => {
                const FORMATS: &[&[u8]] = &[b"delta", b"reltime", b"ctime", b"notime", b"iso", b"raw"];
                if !FORMATS.contains(&arg.as_slice()) {
                    ul::warnx(
                        &short,
                        format!("unknown time format: {}", io::lossy(&arg)),
                    );
                    return 1;
                }
                continue;
            }
            _ => {}
        }
        match o.short() {
            Some('C') | Some('c') | Some('D') | Some('E') => control = o.short(),
            Some('n') => {
                match ul::strtou64_or_err(&arg, "invalid level") {
                    Ok(n) if n >= 1 && n <= 8 => {}
                    Ok(_) => {
                        ul::warnx(&short, format!("invalid level '{}'", io::lossy(&arg)));
                        return 1;
                    }
                    Err(_) => {
                        if !LEVELS.iter().any(|l| l.as_bytes() == arg.as_slice()) {
                            ul::warnx(&short, format!("unknown level '{}'", io::lossy(&arg)));
                            return 1;
                        }
                    }
                }
                control = Some('n');
            }
            Some('F') | Some('K') => file = Some(arg),
            Some('f') => match parse_list(&arg, FACILITIES, "facility", &short) {
                Ok(m) => fac_mask = m,
                Err(()) => return 1,
            },
            Some('l') => match parse_list(&arg, LEVELS, "level", &short) {
                Ok(m) => level_mask = m,
                Err(()) => return 1,
            },
            Some('L') => {
                if !arg.is_empty() && ![b"auto".as_slice(), b"always", b"never"].contains(&arg.as_slice()) {
                    ul::warnx(&short, format!("unknown color mode: {}", io::lossy(&arg)));
                    return 1;
                }
            }
            Some('s') => {
                match ul::strtosize_or_err(&arg, "invalid buffer size argument") {
                    Ok(n) if n >= 4096 => {}
                    Ok(_) => {
                        ul::warnx(&short, "invalid buffer size argument");
                        return 1;
                    }
                    Err(_) => {
                        ul::warnx(
                            &short,
                            format!("invalid buffer size argument: '{}'", io::lossy(&arg)),
                        );
                        return 1;
                    }
                }
            }
            Some('r') => raw = true,
            Some('w') | Some('W') => follow = true,
            Some('H') | Some('J') | Some('k') | Some('P') | Some('p') | Some('u') | Some('x')
            | Some('d') | Some('e') | Some('T') | Some('t') | Some('S') => {}
            Some('V') => {
                ul::print_version(&short);
                return 0;
            }
            Some('h') => {
                let mut out = io::stdout();
                let _ = out.write_all(USAGE.as_bytes());
                return 0;
            }
            _ => {
                ul::errtryhelp(&short);
                return 1;
            }
        }
    }
    let _ = follow;

    if let Some(c) = control {
        if let Some(f) = &file {
            let _ = f;
            ul::warnx(&short, "--file option is incompatible with --clear, --read-clear, --console-off, --console-on and --console-level");
            return 1;
        }
        if matches!(c, 'c') {
            ul::warn(&short, "read kernel buffer failed", Errno::EPERM);
        } else {
            ul::warn(&short, "klogctl failed", Errno::EPERM);
        }
        return 1;
    }

    let Some(path) = file else {
        ul::warn(&short, "read kernel buffer failed", Errno::EPERM);
        return 1;
    };
    let data = match io::read_path(&path) {
        Ok(d) => d,
        Err(e) => {
            ul::warn(&short, format!("cannot open {}", io::lossy(&path)), e);
            return 1;
        }
    };
    let mut out = io::stdout();
    for line in data.split(|b| *b == b'\n') {
        if line.is_empty() {
            continue;
        }
        let mut rest = line;
        let mut prio = 6u32;
        if rest.starts_with(b"<")
            && let Some(end) = rest.iter().position(|b| *b == b'>')
            && let Ok(n) = io::lossy(&rest[1..end]).parse::<u32>()
        {
            prio = n;
            if !raw {
                rest = &rest[end + 1..];
            }
        }
        if (level_mask >> (prio & 7)) & 1 == 0 || (fac_mask >> ((prio >> 3) & 31)) & 1 == 0 {
            continue;
        }
        let _ = out.write_all(rest);
        let _ = out.write_all(b"\n");
    }
    0
}
