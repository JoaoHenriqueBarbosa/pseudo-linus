//! `uptime` do procps-ng 4.0.4: linha padrão, `-p` (com as esquisitices do 4.0.4: `up ` sozinho
//! com 60 s exatos, `60 minutes` com uma hora exata, `24 hours, 0 minutes` com um dia exato) e `-s`.
//!
//! Tempo desde o boot de `/proc/uptime`; sem ele, de `CLOCK_BOOTTIME` (o original falha com
//! `Cannot get system uptime`; o fallback existe porque o procfs do pseudo-linus ainda não tem o
//! arquivo). Carga de `/proc/loadavg`; sem ele, 0.00. Usuários contados no utmp (sem utmp, 0, como
//! num container).

use std::ffi::OsString;

use sysabi::Ctx;
use ul_misc::util::getopt::{Getopt, HasArg, LongOpt};
use ul_misc::util::{io, time};

use crate::common::{self, out};
use crate::procfs;

const USAGE: &str = "\nUsage:\n uptime [options]\n\nOptions:\n -p, --pretty   show uptime in pretty format\n -h, --help     display this help and exit\n -s, --since    system up since\n -V, --version  output version information and exit\n\nFor more details see uptime(1).\n";

const LONGS: &[LongOpt] = &[
    LongOpt::new("pretty", HasArg::No, 'p' as i32),
    LongOpt::new("help", HasArg::No, 'h' as i32),
    LongOpt::new("since", HasArg::No, 's' as i32),
    LongOpt::new("version", HasArg::No, 'V' as i32),
];

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

/// Segundos desde o boot: `/proc/uptime`, senão `CLOCK_BOOTTIME`.
pub fn uptime_secs() -> f64 {
    match procfs::uptime_file() {
        Some((up, _)) => up,
        None => procfs::uptime_clock(),
    }
}

/// O trecho `up ...,  N users,  load average: ...` (sem a hora na frente), compartilhado com o top.
pub fn up_and_load(up: f64, users: usize, load: &procfs::LoadAvg) -> String {
    let secs = up as i64;
    let days = secs / 86_400;
    let mut s = String::from("up ");
    if days > 0 {
        s.push_str(&format!("{days} day{}, ", if days == 1 { "" } else { "s" }));
    }
    let minutes = secs / 60;
    let hours = (minutes / 60) % 24;
    let minutes = minutes % 60;
    if hours > 0 {
        s.push_str(&format!("{hours:2}:{minutes:02}, "));
    } else {
        s.push_str(&format!("{minutes} min, "));
    }
    s.push_str(&format!(
        "{users:2} user{},  load average: {:.2}, {:.2}, {:.2}",
        if users == 1 { "" } else { "s" },
        load.one,
        load.five,
        load.fifteen
    ));
    s
}

/// `uptime -p` do 4.0.4: cada unidade só entra se o resto for estritamente maior que ela, e os
/// minutos aparecem quando não são zero ou quando o que sobrou é menos de um minuto.
pub fn pretty(up: f64) -> String {
    const UNITS: [(f64, &str, &str); 5] = [
        (315_360_000.0, "decade", "decades"),
        (31_536_000.0, "year", "years"),
        (604_800.0, "week", "weeks"),
        (86_400.0, "day", "days"),
        (3_600.0, "hour", "hours"),
    ];
    let mut rest = up;
    let mut parts: Vec<String> = Vec::new();
    for (size, one, many) in UNITS {
        if rest > size {
            let n = (rest / size).floor();
            rest -= n * size;
            let n = n as i64;
            parts.push(format!("{n} {}", if n == 1 { one } else { many }));
        }
    }
    let mut minutes = 0i64;
    if rest > 60.0 {
        let n = (rest / 60.0).floor();
        rest -= n * 60.0;
        minutes = n as i64;
    }
    if minutes != 0 || rest < 60.0 {
        parts.push(format!("{minutes} {}", if minutes == 1 { "minute" } else { "minutes" }));
    }
    format!("up {}", parts.join(", "))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let mut pretty_mode = false;
    let mut since = false;
    let mut g = Getopt::from_env(&argv[1..], "phsV", LONGS);
    while let Some(r) = g.next_opt() {
        match r {
            Err(e) => {
                io::eprint(format!("{}\n{USAGE}", e.message(&argv0)));
                return 1;
            }
            Ok(o) => match o.short() {
                Some('p') => pretty_mode = true,
                Some('s') => since = true,
                Some('h') => {
                    out(USAGE);
                    return 0;
                }
                Some('V') => {
                    out("uptime from procps-ng 4.0.4\n");
                    return 0;
                }
                _ => unreachable!("tabela de opções do uptime"),
            },
        }
    }
    if !g.operands().is_empty() {
        io::eprint(USAGE);
        return 1;
    }
    let up = uptime_secs();
    let (now, _) = procfs::now_realtime();
    let tz = time::local_tz();
    if since {
        let boot = (now as f64 - up).floor() as i64;
        let dt = time::civil(boot, &tz);
        out(format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}\n",
            dt.year(),
            dt.month(),
            dt.day(),
            dt.hour(),
            dt.minute(),
            dt.second()
        ));
        return 0;
    }
    if pretty_mode {
        out(format!("{}\n", pretty(up)));
        return 0;
    }
    let dt = time::civil(now, &tz);
    let load = procfs::loadavg().unwrap_or_default();
    out(format!(
        " {:02}:{:02}:{:02} {}\n",
        dt.hour(),
        dt.minute(),
        dt.second(),
        up_and_load(up, common::utmp_users(), &load)
    ));
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pretty_quirks_of_4_0_4() {
        assert_eq!(pretty(0.5), "up 0 minutes");
        assert_eq!(pretty(60.0), "up ");
        assert_eq!(pretty(3600.0), "up 60 minutes");
        assert_eq!(pretty(3660.0), "up 1 hour");
        assert_eq!(pretty(86_400.0), "up 24 hours, 0 minutes");
        assert_eq!(pretty(90_061.0), "up 1 day, 1 hour, 1 minute");
        assert_eq!(pretty(1_000_000.0), "up 1 week, 4 days, 13 hours, 46 minutes");
        assert_eq!(pretty(31_536_000.0), "up 52 weeks, 24 hours, 0 minutes");
        assert_eq!(pretty(400_000_000.0), "up 1 decade, 2 years, 35 weeks, 4 days, 15 hours, 6 minutes");
    }

    #[test]
    fn default_line() {
        let l = procfs::LoadAvg { one: 0.52, five: 0.58, fifteen: 0.59 };
        assert_eq!(up_and_load(1000.0, 0, &l), "up 16 min,  0 users,  load average: 0.52, 0.58, 0.59");
        assert_eq!(up_and_load(3600.0, 1, &l), "up  1:00,  1 user,  load average: 0.52, 0.58, 0.59");
        assert_eq!(up_and_load(86_400.0, 0, &l), "up 1 day, 0 min,  0 users,  load average: 0.52, 0.58, 0.59");
        assert_eq!(up_and_load(90_061.0, 0, &l), "up 1 day,  1:01,  0 users,  load average: 0.52, 0.58, 0.59");
    }
}
