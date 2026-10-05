//! `zdump` da glibc 2.41 (Debian 13, pacote libc-bin), portado de `timezone/zdump.c` (tzcode).
//!
//! Como no original compilado na glibc, cada operando vira `setenv("TZ", nome); tzset()`: o nome
//! nunca falha, o que não abre como TZif é lido como regra POSIX (`Nowhere` dá UTC com abreviação
//! `Nowhere`). O fuso e o `localtime` estão em [`crate::util::tzif`].
//!
//! - sem opção: uma linha por fuso com a hora atual;
//! - `-v`/`-V`: cada transição entre os cortes, como o par de linhas "um segundo antes" e "na
//!   transição", com a hora UTC, `isdst` e `gmtoff`; o `-v` acrescenta os extremos do `time_t`
//!   (`-9223372036854775808 = NULL` e afins, porque o ano não cabe num `int`);
//! - `-i`: o formato tabular (`TZ="..."`, data, hora local, deslocamento, abreviação, isdst);
//! - `-c [L,]U` (anos) e `-t [L,]U` (segundos), com `wild -c argument` e `wild -t argument`;
//! - `--help` e `--version` são procurados em todo o argv antes do `getopt`.
//!
//! O aviso de abreviação estranha (`warning: zone "X" abbreviation "Y" has ...`) sai no stderr uma
//! vez por fuso nos modos detalhados, e o código de saída continua 0.

use std::ffi::OsString;
use std::io::Write;

use sysabi::Ctx;

use crate::util::getopt::Getopt;
use crate::util::io;
use crate::util::time;
use crate::util::tzif::{Tm, Zone, days_from_civil, gmtime};

const VERSION: &str = "zdump (Debian GLIBC 2.41-12+deb13u4) 2.41\n";

const ABS_MIN: i64 = i64::MIN;
const ABS_MAX: i64 = i64::MAX;
const SECS_PER_DAY: i64 = 86_400;

const WDAY: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MON: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn usage_text(progname: &str) -> String {
    format!(
        "{progname}: usage: {progname} OPTIONS TIMEZONE ...\n\
Options include:\n\
\x20 -c [L,]U   Start at year L (default -500), end before year U (default 2500)\n\
\x20 -t [L,]U   Start at time L, end before time U (in seconds since 1970)\n\
\x20 -i         List transitions briefly (format is experimental)\n\
\x20 -v         List transitions verbosely\n\
\x20 -V         List transitions a bit less verbosely\n\
\x20 --help     Output this help\n\
\x20 --version  Output version info\n\
\n\
Report bugs to <http://www.debian.org/Bugs/>.\n"
    )
}

/// `%jd` do `sscanf`: brancos, sinal e dígitos, saturando como o `strtoimax`.
fn scan_jd(s: &[u8], mut i: usize) -> Option<(i64, usize)> {
    while i < s.len() && s[i].is_ascii_whitespace() {
        i += 1;
    }
    let mut neg = false;
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        neg = s[i] == b'-';
        i += 1;
    }
    let start = i;
    let mut v: i128 = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        v = (v * 10 + i128::from(s[i] - b'0')).min(i128::from(i64::MAX) + 1);
        i += 1;
    }
    if i == start {
        return None;
    }
    let v = if neg { -v } else { v };
    Some((v.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64, i))
}

/// `sscanf(arg, "%jd%c", ...) == 1` e `sscanf(arg, "%jd,%jd%c", ...) == 2`.
fn scan_range(s: &[u8]) -> Option<(Option<i64>, i64)> {
    let (a, p) = scan_jd(s, 0)?;
    if p == s.len() {
        return Some((None, a));
    }
    if s[p] != b',' {
        return None;
    }
    let (b, q) = scan_jd(s, p + 1)?;
    (q == s.len()).then_some((Some(a), b))
}

/// `yeartot`: o `time_t` de 1º de janeiro do ano, saturado nos extremos.
fn yeartot(y: i64) -> i64 {
    let t = days_from_civil(i128::from(y), 1, 1) * i128::from(SECS_PER_DAY);
    t.clamp(i128::from(ABS_MIN), i128::from(ABS_MAX)) as i64
}

/// Segundos locais (o que o `delta` do original compara).
fn local_secs(t: i64, tm: &Tm) -> i128 {
    i128::from(t) + i128::from(tm.gmtoff)
}

struct Dump {
    progname: String,
    longest: usize,
    warned: bool,
}

fn dumptime(out: &mut Vec<u8>, tm: Option<&Tm>) {
    let Some(tm) = tm else {
        out.extend_from_slice(b"NULL");
        return;
    };
    let _ = write!(
        out,
        "{} {}{:3} {:02}:{:02}:{:02} {}",
        WDAY[tm.wday as usize],
        MON[tm.mon as usize],
        tm.mday,
        tm.hour,
        tm.min,
        tm.sec,
        tm.year
    );
}

impl Dump {
    fn emit(&self, buf: &[u8]) {
        let mut out = io::stdout();
        let _ = out.write_all(buf);
    }

    fn abbrok(&mut self, ab: &[u8], zone: &[u8]) {
        if self.warned {
            return;
        }
        let n = ab
            .iter()
            .take_while(|&&c| c.is_ascii_alphanumeric() || c == b'-' || c == b'+')
            .count();
        let why = if n < ab.len() {
            "has characters other than ASCII alphanumerics, '-' or '+'"
        } else if n < 3 {
            "has fewer than 3 characters"
        } else if n > 6 {
            "has more than 6 characters"
        } else {
            return;
        };
        let _ = io::flush_stdout();
        io::eprint(format!(
            "{}: warning: zone \"{}\" abbreviation \"{}\" {}\n",
            self.progname,
            io::lossy(zone),
            io::lossy(ab),
            why
        ));
        self.warned = true;
    }

    fn show(&mut self, tz: &Zone, zone: &[u8], t: i64, v: bool) {
        let mut out = Vec::new();
        out.extend_from_slice(zone);
        out.resize(out.len().max(self.longest), b' ');
        out.extend_from_slice(b"  ");
        if v {
            match gmtime(t) {
                Some(g) => {
                    dumptime(&mut out, Some(&g));
                    out.extend_from_slice(b" UT");
                }
                None => {
                    let _ = write!(out, "{t}");
                }
            }
            out.extend_from_slice(b" = ");
        }
        let tm = tz.localtime(t);
        dumptime(&mut out, tm.as_ref());
        if let Some(tm) = &tm {
            if !tm.zone.is_empty() {
                out.push(b' ');
                out.extend_from_slice(&tm.zone);
            }
            if v {
                let _ = write!(out, " isdst={} gmtoff={}", tm.isdst, tm.gmtoff);
            }
        }
        out.push(b'\n');
        self.emit(&out);
        if let Some(tm) = &tm
            && !tm.zone.is_empty() {
                self.abbrok(&tm.zone, zone);
            }
    }

    /// `showtrans`: `tm` ausente imprime só o `time_t`.
    fn showtrans(&self, fmt: &[u8], tm: Option<&Tm>, t: i64, ab: &[u8], zone: &[u8]) {
        let mut out = Vec::new();
        match tm {
            None => {
                let _ = write!(out, "{t}");
            }
            Some(tm) => istrftime(&mut out, fmt, tm, ab, zone),
        }
        out.push(b'\n');
        self.emit(&out);
    }
}

fn format_quoted_string(out: &mut Vec<u8>, s: &[u8]) {
    out.push(b'"');
    for &c in s {
        if c == b'"' || c == b'\\' {
            out.push(b'\\');
        }
        out.push(c);
    }
    out.push(b'"');
}

fn format_local_time(out: &mut Vec<u8>, tm: &Tm) {
    let _ = if tm.sec != 0 {
        write!(out, "{:02}:{:02}:{:02}", tm.hour, tm.min, tm.sec)
    } else if tm.min != 0 {
        write!(out, "{:02}:{:02}", tm.hour, tm.min)
    } else {
        write!(out, "{:02}", tm.hour)
    };
}

fn format_utc_offset(tm: &Tm) -> Vec<u8> {
    let mut off = tm.gmtoff;
    let sign = if off < 0 || (off == 0 && (tm.zone.first() == Some(&b'-') || tm.zone == b"zzz")) {
        '-'
    } else {
        '+'
    };
    off = off.abs();
    let ss = off % 60;
    let mm = off / 60 % 60;
    let hh = off / 3600;
    let s = if ss != 0 || hh >= 100 {
        format!("{sign}{hh:02}{mm:02}{ss:02}")
    } else if mm != 0 {
        format!("{sign}{hh:02}{mm:02}")
    } else {
        format!("{sign}{hh:02}")
    };
    s.into_bytes()
}

/// O `istrftime` do zdump: `strftime` com `%f` (nome do fuso entre aspas), `%L` (hora local curta)
/// e `%Q` (deslocamento, abreviação quando difere dele, e isdst).
fn istrftime(out: &mut Vec<u8>, fmt: &[u8], tm: &Tm, ab: &[u8], zone: &[u8]) {
    let mut i = 0;
    while i < fmt.len() {
        let c = fmt[i];
        if c != b'%' || i + 1 >= fmt.len() {
            out.push(c);
            i += 1;
            continue;
        }
        let k = fmt[i + 1];
        i += 2;
        match k {
            b'%' => out.push(b'%'),
            b'Y' => {
                let _ = write!(out, "{}", tm.year);
            }
            b'm' => {
                let _ = write!(out, "{:02}", tm.mon + 1);
            }
            b'd' => {
                let _ = write!(out, "{:02}", tm.mday);
            }
            b'f' => format_quoted_string(out, zone),
            b'L' => format_local_time(out, tm),
            b'Q' => {
                let off = format_utc_offset(tm);
                let show_abbr = off.as_slice() != ab;
                out.extend_from_slice(&off);
                if show_abbr {
                    out.push(b'\t');
                    if !ab.is_empty() && ab.iter().all(u8::is_ascii_alphabetic) {
                        out.extend_from_slice(ab);
                    } else {
                        format_quoted_string(out, ab);
                    }
                }
                if tm.isdst != 0 {
                    let s = format!("\t\t{}", tm.isdst);
                    out.extend_from_slice(&s.as_bytes()[usize::from(show_abbr)..]);
                }
            }
            other => {
                out.push(b'%');
                out.push(other);
            }
        }
    }
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let progname = io::argv0(args);
    for a in argv.iter().skip(1) {
        if a == b"--version" {
            let mut out = io::stdout();
            let _ = out.write_all(VERSION.as_bytes());
            return 0;
        }
        if a == b"--help" {
            let mut out = io::stdout();
            let _ = out.write_all(usage_text(&progname).as_bytes());
            return 0;
        }
    }
    let mut g = Getopt::from_env(&argv[1..], "c:it:vV", &[]);
    let (mut cutarg, mut cuttimes) = (None, None);
    let (mut iflag, mut vflag, mut big_v) = (false, false, false);
    while let Some(opt) = g.next_opt() {
        match opt {
            Ok(o) => match o.short() {
                Some('c') => cutarg = o.arg,
                Some('t') => cuttimes = o.arg,
                Some('i') => iflag = true,
                Some('v') => vflag = true,
                Some('V') => big_v = true,
                _ => {}
            },
            Err(e) => {
                io::eprint(format!("{}\n{}", e.message(&progname), usage_text(&progname)));
                return 1;
            }
        }
    }
    let zones = g.operands();
    if zones.len() == 1 && zones[0] == b"=" {
        io::eprint(usage_text(&progname));
        return 1;
    }
    let verbose = iflag || vflag || big_v;
    let mut cutlotime = ABS_MIN;
    let mut cuthitime = ABS_MAX;
    if verbose {
        let mut cutloyear: i64 = -500;
        let mut cuthiyear: i64 = 2500;
        if let Some(arg) = &cutarg {
            match scan_range(arg) {
                Some((lo, hi)) => {
                    if let Some(lo) = lo {
                        cutloyear = lo;
                    }
                    cuthiyear = hi;
                }
                None => {
                    io::eprint(format!("{progname}: wild -c argument {}\n", io::lossy(arg)));
                    return 1;
                }
            }
        }
        if cutarg.is_some() || cuttimes.is_none() {
            cutlotime = yeartot(cutloyear);
            cuthitime = yeartot(cuthiyear);
        }
        if let Some(arg) = &cuttimes {
            match scan_range(arg) {
                Some((lo, mut hi)) => {
                    // O `if (absolute_max_time < lo)` do C não dispara com time_t de 64 bits.
                    if let Some(lo) = lo
                        && cutlotime < lo
                    {
                        cutlotime = lo;
                    }
                    if hi < cuthitime {
                        if hi < ABS_MIN + 1 {
                            hi = ABS_MIN + 1;
                        }
                        cuthitime = hi;
                    }
                }
                None => {
                    io::eprint(format!("{progname}: wild -t argument {}\n", io::lossy(arg)));
                    return 1;
                }
            }
        }
    }
    let now = if verbose {
        0
    } else {
        let n = time::now().sec;
        n | i64::from(n == 0)
    };
    let mut d = Dump {
        progname: progname.clone(),
        longest: zones.iter().map(Vec::len).max().unwrap_or(0).min(i32::MAX as usize),
        warned: false,
    };
    for name in &zones {
        let tz = Zone::from_tz(name);
        if !verbose {
            d.show(&tz, name, now, false);
            continue;
        }
        d.warned = false;
        let mut t = ABS_MIN;
        if !(iflag || big_v) {
            d.show(&tz, name, t, true);
            t += SECS_PER_DAY;
            d.show(&tz, name, t, true);
        }
        if t < cutlotime {
            t = cutlotime;
        }
        let mut tm = tz.localtime(t);
        let mut ab: Option<Vec<u8>> = tm.as_ref().map(|x| x.zone.clone());
        if let (Some(x), Some(a)) = (&tm, &ab)
            && iflag {
                d.showtrans(b"\nTZ=%f", Some(x), t, a, name);
                d.showtrans(b"-\t-\t%Q", Some(x), t, a, name);
            }
        while t < cuthitime {
            let mut newt = if t < ABS_MAX - SECS_PER_DAY / 2 && t + SECS_PER_DAY / 2 < cuthitime {
                t + SECS_PER_DAY / 2
            } else {
                cuthitime
            };
            let mut newtm = tz.localtime(newt);
            let changed = match (&tm, &newtm) {
                (Some(o), Some(n)) => {
                    ab.is_some()
                        && (local_secs(newt, n) - local_secs(t, o) != i128::from(newt) - i128::from(t)
                            || n.isdst != o.isdst
                            || Some(&n.zone) != ab.as_ref())
                }
                (None, None) => false,
                _ => true,
            };
            if changed {
                newt = hunt(&tz, t, newt);
                newtm = tz.localtime(newt);
                if iflag {
                    let a = newtm.as_ref().map(|x| x.zone.clone()).unwrap_or_default();
                    d.showtrans(b"%Y-%m-%d\t%L\t%Q", newtm.as_ref(), newt, &a, name);
                } else {
                    d.show(&tz, name, newt - 1, true);
                    d.show(&tz, name, newt, true);
                }
            }
            t = newt;
            if let Some(n) = &newtm {
                ab = Some(n.zone.clone());
                tm = Some(n.clone());
            } else {
                tm = None;
            }
            sysabi::sys::checkpoint();
        }
        if !(iflag || big_v) {
            let t = ABS_MAX - SECS_PER_DAY;
            d.show(&tz, name, t, true);
            d.show(&tz, name, t + SECS_PER_DAY, true);
        }
    }
    let mut out = io::stdout();
    if out.flush().is_err() {
        return 1;
    }
    0
}

/// Busca binária do instante exato da transição entre `lot` e `hit`.
fn hunt(tz: &Zone, mut lot: i64, mut hit: i64) -> i64 {
    let mut lotm = tz.localtime(lot);
    let ab = lotm.as_ref().map(|x| x.zone.clone());
    loop {
        let diff = i128::from(hit) - i128::from(lot);
        if diff < 2 {
            break;
        }
        let mut t = (i128::from(lot) + diff / 2) as i64;
        if t <= lot {
            t += 1;
        } else if t >= hit {
            t -= 1;
        }
        let tm = tz.localtime(t);
        let same = match (&lotm, &tm) {
            (Some(o), Some(n)) => {
                local_secs(t, n) - local_secs(lot, o) == i128::from(t) - i128::from(lot)
                    && n.isdst == o.isdst
                    && Some(&n.zone) == ab.as_ref()
            }
            (None, None) => true,
            _ => false,
        };
        if same {
            lot = t;
            if tm.is_some() {
                lotm = tm;
            }
        } else {
            hit = t;
        }
    }
    hit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_parse_like_sscanf() {
        assert_eq!(scan_range(b"2000"), Some((None, 2000)));
        assert_eq!(scan_range(b"1900,2000"), Some((Some(1900), 2000)));
        assert_eq!(scan_range(b"1900, 2000"), Some((Some(1900), 2000)));
        assert_eq!(scan_range(b"x"), None);
        assert_eq!(scan_range(b"5 "), None);
        assert_eq!(scan_range(b"1,2,3"), None);
    }

    #[test]
    fn yeartot_saturates() {
        assert_eq!(yeartot(1970), 0);
        assert_eq!(yeartot(2000), 946_684_800);
        assert_eq!(yeartot(i64::MAX), i64::MAX);
        assert_eq!(yeartot(i64::MIN), i64::MIN);
    }
}
