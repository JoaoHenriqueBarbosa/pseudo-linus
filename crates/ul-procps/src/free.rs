//! `free` do procps-ng 4.0.4.
//!
//! Contas do procps 4 (conferidas no oráculo contra o `/proc/meminfo` dele): usado = total -
//! disponível (`MemAvailable`), buff/cache = `Buffers` + `Cached` + `SReclaimable`, cache (com `-w`)
//! = `Cached` + `SReclaimable`, compartilhado = `Shmem`. Sem `/proc/meminfo` o programa falha como o
//! original (`Memory information file /proc/meminfo does not exist`, status 1).

use std::ffi::OsString;
use std::time::Duration;

use sysabi::{Ctx, Errno, sys};
use ul_misc::util::getopt::{Getopt, HasArg, LongOpt};
use ul_misc::util::io;

use crate::common::{self, out};
use crate::procfs::{self, MemInfo};

const USAGE: &str = "\nUsage:\n free [options]\n\nOptions:\n -b, --bytes         show output in bytes\n     --kilo          show output in kilobytes\n     --mega          show output in megabytes\n     --giga          show output in gigabytes\n     --tera          show output in terabytes\n     --peta          show output in petabytes\n -k, --kibi          show output in kibibytes\n -m, --mebi          show output in mebibytes\n -g, --gibi          show output in gibibytes\n     --tebi          show output in tebibytes\n     --pebi          show output in pebibytes\n -h, --human         show human-readable output\n     --si            use powers of 1000 not 1024\n -l, --lohi          show detailed low and high memory statistics\n -L, --line          show output on a single line\n -t, --total         show total for RAM + swap\n -v, --committed     show committed memory and commit limit\n -s N, --seconds N   repeat printing every N seconds\n -c N, --count N     repeat printing N times, then exit\n -w, --wide          wide output\n\n     --help     display this help and exit\n -V, --version  output version information and exit\n\nFor more details see free(1).\n";

const KILO: i32 = 300;
const MEGA: i32 = 301;
const GIGA: i32 = 302;
const TERA: i32 = 303;
const PETA: i32 = 304;
const TEBI: i32 = 305;
const PEBI: i32 = 306;
const SI: i32 = 307;
const HELP: i32 = 308;

const LONGS: &[LongOpt] = &[
    LongOpt::new("bytes", HasArg::No, 'b' as i32),
    LongOpt::new("kilo", HasArg::No, KILO),
    LongOpt::new("mega", HasArg::No, MEGA),
    LongOpt::new("giga", HasArg::No, GIGA),
    LongOpt::new("tera", HasArg::No, TERA),
    LongOpt::new("peta", HasArg::No, PETA),
    LongOpt::new("kibi", HasArg::No, 'k' as i32),
    LongOpt::new("mebi", HasArg::No, 'm' as i32),
    LongOpt::new("gibi", HasArg::No, 'g' as i32),
    LongOpt::new("tebi", HasArg::No, TEBI),
    LongOpt::new("pebi", HasArg::No, PEBI),
    LongOpt::new("human", HasArg::No, 'h' as i32),
    LongOpt::new("si", HasArg::No, SI),
    LongOpt::new("lohi", HasArg::No, 'l' as i32),
    LongOpt::new("line", HasArg::No, 'L' as i32),
    LongOpt::new("total", HasArg::No, 't' as i32),
    LongOpt::new("committed", HasArg::No, 'v' as i32),
    LongOpt::new("seconds", HasArg::Required, 's' as i32),
    LongOpt::new("count", HasArg::Required, 'c' as i32),
    LongOpt::new("wide", HasArg::No, 'w' as i32),
    LongOpt::new("help", HasArg::No, HELP),
    LongOpt::new("version", HasArg::No, 'V' as i32),
];

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

#[derive(Default)]
struct Opts {
    /// 0 = padrão (KiB), 1 = bytes, 2 = kilo, 3 = mega...
    exponent: u32,
    si: bool,
    human: bool,
    lohi: bool,
    line: bool,
    total: bool,
    committed: bool,
    wide: bool,
}

/// Valor em KiB formatado conforme as opções (o `scale_size` do free).
fn scale(kb: i64, o: &Opts) -> String {
    let base: f64 = if o.si { 1000.0 } else { 1024.0 };
    let bytes = kb.saturating_mul(1024);
    if !o.human {
        return match o.exponent {
            0 => (bytes / base as i64).to_string(),
            1 => bytes.to_string(),
            e => ((bytes as f64 / base.powi(e as i32 - 1)) as i64).to_string(),
        };
    }
    let plain = format!("{bytes}B");
    if plain.len() <= 4 {
        return plain;
    }
    let units = ['K', 'M', 'G', 'T', 'P'];
    let mut last = plain;
    for (i, u) in units.iter().enumerate() {
        let v = bytes as f64 / base.powi(i as i32 + 1);
        // O original formata com "%.1f" sobre um float de 32 bits.
        let (frac, int, limit) = if o.si {
            (format!("{:.1}{u}", v as f32), format!("{}{u}", v as i64), 4)
        } else {
            (format!("{:.1}{u}i", v as f32), format!("{}{u}i", v as i64), 5)
        };
        if frac.len() <= limit {
            return frac;
        }
        if int.len() <= limit {
            return int;
        }
        last = int;
    }
    last
}

struct Mem {
    total: i64,
    used: i64,
    free: i64,
    shared: i64,
    buffers: i64,
    cached: i64,
    available: i64,
    low_total: i64,
    low_free: i64,
    high_total: i64,
    high_free: i64,
    swap_total: i64,
    swap_free: i64,
    commit_limit: i64,
    committed: i64,
}

impl Mem {
    fn from(m: &MemInfo) -> Mem {
        let g = |k: &str| m.get(k) as i64;
        let total = g("MemTotal");
        let free = g("MemFree");
        let buffers = g("Buffers");
        let cached = g("Cached") + g("SReclaimable");
        let available = if m.has("MemAvailable") { g("MemAvailable") } else { free };
        let mut used = total - available;
        if used < 0 {
            used = total - free;
        }
        Mem {
            total,
            used,
            free,
            shared: g("Shmem"),
            buffers,
            cached,
            available,
            low_total: if m.has("LowTotal") { g("LowTotal") } else { total },
            low_free: if m.has("LowFree") { g("LowFree") } else { free },
            high_total: g("HighTotal"),
            high_free: g("HighFree"),
            swap_total: g("SwapTotal"),
            swap_free: g("SwapFree"),
            commit_limit: g("CommitLimit"),
            committed: g("Committed_AS"),
        }
    }
}

fn row(label: &str, vals: &[i64], o: &Opts) -> String {
    let mut s = format!("{label:<8}");
    for v in vals {
        s.push_str(&format!("{:>12}", scale(*v, o)));
    }
    s.push('\n');
    s
}

fn report(m: &Mem, o: &Opts) -> String {
    let swap_used = m.swap_total - m.swap_free;
    if o.line {
        return format!(
            "{:>7}{:>12} {:>7}{:>12} {:>7}{:>12} {:>7}{:>12} \n",
            "SwapUse",
            scale(swap_used, o),
            "CachUse",
            scale(m.buffers + m.cached, o),
            "MemUse",
            scale(m.used, o),
            "MemFree",
            scale(m.free, o)
        );
    }
    let mut s = String::new();
    if o.wide {
        s.push_str("               total        used        free      shared     buffers       cache   available\n");
        s.push_str(&row("Mem:", &[m.total, m.used, m.free, m.shared, m.buffers, m.cached, m.available], o));
    } else {
        s.push_str("               total        used        free      shared  buff/cache   available\n");
        s.push_str(&row("Mem:", &[m.total, m.used, m.free, m.shared, m.buffers + m.cached, m.available], o));
    }
    if o.lohi {
        s.push_str(&row("Low:", &[m.low_total, m.low_total - m.low_free, m.low_free], o));
        s.push_str(&row("High:", &[m.high_total, m.high_total - m.high_free, m.high_free], o));
    }
    s.push_str(&row("Swap:", &[m.swap_total, swap_used, m.swap_free], o));
    if o.total {
        s.push_str(&row("Total:", &[m.total + m.swap_total, m.used + swap_used, m.free + m.swap_free], o));
    }
    if o.committed {
        s.push_str(&row("Comm:", &[m.commit_limit, m.committed, m.commit_limit - m.committed], o));
    }
    s
}

/// Segundos do `-s`: dígitos com ponto opcional (sem expoente), espaço à esquerda aceito.
fn parse_seconds(s: &str) -> Option<f64> {
    let t = s.trim_start();
    let (neg, body) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    if body.is_empty() || body == "." {
        return None;
    }
    let mut dot = false;
    for c in body.chars() {
        match c {
            '0'..='9' => {}
            '.' if !dot => dot = true,
            _ => return None,
        }
    }
    let v: f64 = body.parse().ok().or_else(|| format!("0{body}").parse().ok())?;
    Some(if neg { -v } else { v })
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let mut o = Opts::default();
    let mut delay: Option<f64> = None;
    let mut count: Option<u64> = None;
    let mut g = Getopt::from_env(&argv[1..], "bc:ghkLlms:tvwV", LONGS);
    let unit = |o: &mut Opts, e: u32, si: bool| -> bool {
        if o.exponent != 0 {
            common::warn("free", "Multiple unit options don't make sense.");
            return false;
        }
        o.exponent = e;
        if si {
            o.si = true;
        }
        true
    };
    while let Some(r) = g.next_opt() {
        let opt = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n{USAGE}", e.message(&argv0)));
                return 1;
            }
        };
        let ok = match opt.id {
            x if x == 'b' as i32 => unit(&mut o, 1, false),
            KILO => unit(&mut o, 2, true),
            MEGA => unit(&mut o, 3, true),
            GIGA => unit(&mut o, 4, true),
            TERA => unit(&mut o, 5, true),
            PETA => unit(&mut o, 6, true),
            x if x == 'k' as i32 => unit(&mut o, 2, false),
            x if x == 'm' as i32 => unit(&mut o, 3, false),
            x if x == 'g' as i32 => unit(&mut o, 4, false),
            TEBI => unit(&mut o, 5, false),
            PEBI => unit(&mut o, 6, false),
            x if x == 'h' as i32 => {
                o.human = true;
                true
            }
            SI => {
                o.si = true;
                true
            }
            x if x == 'l' as i32 => {
                o.lohi = true;
                true
            }
            x if x == 'L' as i32 => {
                o.line = true;
                true
            }
            x if x == 't' as i32 => {
                o.total = true;
                true
            }
            x if x == 'v' as i32 => {
                o.committed = true;
                true
            }
            x if x == 'w' as i32 => {
                o.wide = true;
                true
            }
            x if x == 's' as i32 => {
                let a = opt.arg_str();
                if a.is_empty() {
                    common::warn("free", "seconds argument failed: ''");
                    return 1;
                }
                match parse_seconds(&a) {
                    Some(v) if v > 0.0 => delay = Some(v),
                    Some(_) => {
                        common::warn("free", &format!("seconds argument `{a}' is not positive number"));
                        return 1;
                    }
                    None => {
                        common::warn("free", &format!("seconds argument failed: '{a}': {}", Errno::EINVAL.message()));
                        return 1;
                    }
                }
                true
            }
            x if x == 'c' as i32 => {
                let a = opt.arg_str();
                match common::strtol(&a) {
                    common::Strtol::Ok(v) if v >= 1 => count = Some(v as u64),
                    common::Strtol::Invalid if !a.is_empty() => {
                        common::warn("free", &format!("failed to parse count argument: '{a}'"));
                        return 1;
                    }
                    common::Strtol::Invalid => {
                        common::warn("free", &format!("failed to parse count argument: '{a}': {}", Errno::ENOENT.message()));
                        return 1;
                    }
                    _ => {
                        common::warn("free", &format!("failed to parse count argument: '{a}': {}", Errno::ERANGE.message()));
                        return 1;
                    }
                }
                true
            }
            HELP => {
                out(USAGE);
                return 0;
            }
            x if x == 'V' as i32 => {
                out("free from procps-ng 4.0.4\n");
                return 0;
            }
            _ => unreachable!("tabela de opções do free"),
        };
        if !ok {
            return 1;
        }
    }
    if !g.operands().is_empty() {
        io::eprint(USAGE);
        return 1;
    }
    let repeat = delay.is_some() || count.is_some();
    let delay = Duration::from_secs_f64(delay.unwrap_or(1.0));
    let mut done = 0u64;
    loop {
        let Some(info) = procfs::meminfo() else {
            common::warn("free", "Memory information file /proc/meminfo does not exist");
            return 1;
        };
        out(report(&Mem::from(&info), &o));
        done += 1;
        if !repeat || count.is_some_and(|c| done >= c) {
            break;
        }
        if !o.line {
            out("\n");
        }
        let _ = io::flush_stdout();
        let _ = sys::current().nanosleep(delay);
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_scale_matches_procps() {
        let h = Opts { human: true, ..Opts::default() };
        let si = Opts { human: true, si: true, ..Opts::default() };
        assert_eq!(scale(10_443_160, &h), "9Gi");
        assert_eq!(scale(10_443_160, &si), "10G");
        assert_eq!(scale(1023, &h), "1.0Mi");
        assert_eq!(scale(0, &h), "0B");
        assert_eq!(scale(1, &h), "1.0Ki");
        assert_eq!(scale(-1_073_741_824, &h), "-1Ti");
        assert_eq!(scale(8_161_656, &h), "7.8Gi");
        assert_eq!(parse_seconds(".5"), Some(0.5));
        assert_eq!(parse_seconds("5."), Some(5.0));
        assert_eq!(parse_seconds("1e1"), None);
    }
}
