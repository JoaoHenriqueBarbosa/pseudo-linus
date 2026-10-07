//! `zic` da glibc 2.41 (Debian 13, pacote libc-bin), portado de `timezone/zic.c` (tzcode).
//!
//! Compila arquivos de regras (`Rule`, `Zone`, `Link`, `Leap`) para binários TZif versão 2 ou 3, com
//! o rodapé de TZ string. O padrão é `-b slim` (como o zic do tzcode desde 2020b); `-b fat` gera os
//! dados de 32 bits completos, as transições até 2037 e os tipos de compatibilidade.
//!
//! Opções tratadas: `-b`, `-d`, `-l`, `-L`, `-p`, `-P`, `-r`, `-R`, `-s`, `-t`, `-v`, `-y`,
//! `--help` e `--version`. `-l`, `-p` e as linhas `Link` viram hard links (com cópia de reserva);
//! `Link - NOME` remove o nome.
//!
//! O leitor TZif e as rotinas de fuso do `zdump` ficam em [`crate::util::tzif`]; este módulo só
//! escreve, então a conferência dos binários gerados é feita com `od` ou com o próprio `zdump`
//! apontando `TZDIR` para o diretório de saída.
//!
//! Tudo passa por `sysabi`; nada toca o host.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{AtFlags, Ctx, Errno, Fd, OFlags, RenameFlags, sys};
use ul_common::ctype::{cstr_at, is_space};

use crate::util::getopt::Getopt;
use crate::util::io;

const VERSION: &str = "zic (Debian GLIBC 2.41-12+deb13u4) 2.41\n";

const ZIC_MIN: i64 = i64::MIN;
const ZIC_MAX: i64 = i64::MAX;
const ZIC32_MIN: i64 = -(1 << 31);
const ZIC32_MAX: i64 = (1 << 31) - 1;
const Y2038_BOUNDARY: i64 = 1 << 31;
const EPOCH_YEAR: i64 = 1970;
const EPOCH_WDAY: i64 = 4;
const SECS_PER_DAY: i64 = 86_400;
const YEAR_32BIT_MIN: i64 = 1901;
const YEARS_OF_OBSERVATIONS: i64 = 401;
const TZDIR: &str = "/usr/share/zoneinfo";
const TZDEFAULT: &str = "/etc/localtime";
const TZDEFRULES: &str = "posixrules";

const LEN_MONTHS: [[i64; 12]; 2] = [
    [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31],
    [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31],
];

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const WDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];
const LINE_CODES: [&str; 4] = ["Rule", "Zone", "Link", "Leap"];
const BEGIN_YEARS: [&str; 1] = ["minimum"];
const END_YEARS: [&str; 2] = ["only", "maximum"];
const LEAP_TYPES: [&str; 2] = ["Rolling", "Stationary"];

const DC_DOM: u8 = 0;
const DC_DOWGEQ: u8 = 1;
const DC_DOWLEQ: u8 = 2;

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn usage_text(progname: &str) -> String {
    format!(
        "{progname}: usage is {progname} [ --version ] [ --help ] [ -v ] \\\n\
\t[ -b {{slim|fat}} ] [ -d directory ] [ -l localtime ] [ -L leapseconds ] \\\n\
\t[ -p posixrules ] [ -r '[@lo][/@hi]' ] [ -R '@hi' ] \\\n\
\t[ -t localtime-link ] \\\n\
\t[ filename ... ]\n\
\n\
Report bugs to <http://www.debian.org/Bugs/>.\n"
    )
}

fn isleap(y: i64) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}

fn len_year(y: i64) -> i64 {
    if isleap(y) { 366 } else { 365 }
}

fn leap_idx(y: i64) -> usize {
    usize::from(isleap(y))
}

/// `byword`: nome exato (sem diferenciar caixa) ou prefixo único.
fn byword(word: &str, table: &[&str]) -> Option<usize> {
    if word.is_empty() {
        return None;
    }
    for (i, t) in table.iter().enumerate() {
        if t.eq_ignore_ascii_case(word) {
            return Some(i);
        }
    }
    let mut found = None;
    let mut n = 0;
    for (i, t) in table.iter().enumerate() {
        if t.len() >= word.len()
            && word.is_ascii()
            && t.as_bytes()[..word.len()].eq_ignore_ascii_case(word.as_bytes())
        {
            found = Some(i);
            n += 1;
        }
    }
    if n == 1 { found } else { None }
}

/// O `%d%c == 1` do `sscanf`: o texto inteiro é um número.
fn parse_whole(s: &str) -> Option<i64> {
    let t = s.trim_start();
    if t.is_empty() {
        return None;
    }
    t.parse::<i64>().ok()
}

/// `getfields`: campos separados por brancos, aspas agrupam e `#` abre comentário.
fn getfields(line: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    loop {
        while i < line.len() && is_space(line[i]) {
            i += 1;
        }
        if i >= line.len() || line[i] == b'#' {
            break;
        }
        let mut f = Vec::new();
        let mut inq = false;
        while i < line.len() {
            let c = line[i];
            if inq {
                if c == b'"' {
                    inq = false;
                    i += 1;
                } else if c == b'\\' && i + 1 < line.len() {
                    f.push(line[i + 1]);
                    i += 2;
                } else {
                    f.push(c);
                    i += 1;
                }
            } else {
                if c == b'"' {
                    inq = true;
                    i += 1;
                    continue;
                }
                if is_space(c) || c == b'#' {
                    break;
                }
                f.push(c);
                i += 1;
            }
        }
        out.push(String::from_utf8_lossy(&f).into_owned());
    }
    out
}

#[derive(Clone, Default)]
struct Rule {
    name: String,
    file: String,
    line: usize,
    loyear: i64,
    hiyear: i64,
    lowasnum: bool,
    hiwasnum: bool,
    month: usize,
    dycode: u8,
    dayofmonth: i64,
    wday: i64,
    tod: i64,
    todisstd: bool,
    todisut: bool,
    save: i64,
    isdst: bool,
    abbrvar: String,
    todo: bool,
    temp: i64,
}

#[derive(Clone, Default)]
struct Zone {
    file: String,
    line: usize,
    name: Option<String>,
    stdoff: i64,
    rule: String,
    format: String,
    spec: u8,
    save: i64,
    isdst: bool,
    untilrule: Rule,
    untiltime: i64,
    rules: Vec<Rule>,
}

struct Link {
    file: String,
    line: usize,
    target: String,
    name: String,
}

/// O que o `doabbr` recebe como `letters`.
enum Letters<'a> {
    /// `NULL`: usa o literal `%s`.
    Null,
    /// `disable_percent_s`.
    Disable,
    Str(&'a str),
}

struct ZFmt {
    format: String,
    spec: u8,
    stdoff: i64,
}

#[derive(Clone, Copy)]
struct AtType {
    at: i64,
    dontmerge: bool,
    ty: usize,
}

#[derive(Default)]
struct Tz {
    utoffs: Vec<i64>,
    isdsts: Vec<i8>,
    ttisstds: Vec<bool>,
    ttisuts: Vec<bool>,
    desigidx: Vec<usize>,
    chars: Vec<u8>,
    attypes: Vec<AtType>,
}

struct Range {
    defaulttype: usize,
    base: usize,
    count: usize,
    leapbase: usize,
    leapcount: usize,
}

fn limitrange(mut r: Range, lo: i64, hi: i64, ats: &[i64], types: &[usize], trans: &[i64]) -> Range {
    while r.count > 0 && ats[r.base] < lo {
        r.defaulttype = types[r.base];
        r.count -= 1;
        r.base += 1;
    }
    while r.leapcount > 1 && trans[r.leapbase + 1] <= lo {
        r.leapcount -= 1;
        r.leapbase += 1;
    }
    if hi < ZIC_MAX {
        while r.count > 0 && hi.saturating_add(1) < ats[r.base + r.count - 1] {
            r.count -= 1;
        }
        while r.leapcount > 0 && hi.saturating_add(1) < trans[r.leapbase + r.leapcount - 1] {
            r.leapcount -= 1;
        }
    }
    r
}

fn rule_cmp(a: Option<&Rule>, b: &Rule) -> i32 {
    let Some(a) = a else {
        return -1;
    };
    if a.hiyear != b.hiyear {
        return if a.hiyear < b.hiyear { -1 } else { 1 };
    }
    if a.hiyear == ZIC_MAX {
        return 0;
    }
    if a.month != b.month {
        return a.month as i32 - b.month as i32;
    }
    (a.dayofmonth - b.dayofmonth) as i32
}

/// `stringoffset`: `None` quando as horas passam de 167.
fn stringoffset(offset: i64) -> Option<String> {
    let negative = offset < 0;
    let mut off = offset.unsigned_abs();
    let mut s = String::new();
    if negative {
        s.push('-');
    }
    let hours = off / 3600;
    if hours >= 24 * 7 {
        return None;
    }
    s.push_str(&hours.to_string());
    off %= 3600;
    if off != 0 {
        s.push_str(&format!(":{:02}", off / 60));
        off %= 60;
        if off != 0 {
            s.push_str(&format!(":{off:02}"));
        }
    }
    Some(s)
}

fn abbroffset(offset: i64) -> Option<String> {
    let mut sign = '+';
    let mut off = offset;
    if off < 0 {
        off = -off;
        sign = '-';
    }
    let seconds = off % 60;
    off /= 60;
    let minutes = off % 60;
    off /= 60;
    if off >= 100 {
        return None;
    }
    let mut s = format!("{sign}{off:02}");
    if minutes != 0 || seconds != 0 {
        s.push_str(&format!("{minutes:02}"));
        if seconds != 0 {
            s.push_str(&format!("{seconds:02}"));
        }
    }
    Some(s)
}

struct Z {
    progname: String,
    errors: usize,
    filename: Option<String>,
    linenum: usize,
    rloc: Option<(String, usize)>,
    bloat: i32,
    lo_time: i64,
    hi_time: i64,
    redundant_time: i64,
    noise: bool,
    directory: String,
    zones: Vec<Zone>,
    rules: Vec<Rule>,
    links: Vec<Link>,
    leaps: Vec<(i64, i64)>,
    leapseen: bool,
    leapminyear: i64,
    leapmaxyear: i64,
    tz: Tz,
}

impl Z {
    fn want_bloat(&self) -> bool {
        self.bloat >= 0
    }

    fn loc(&self) -> String {
        match &self.filename {
            Some(f) => {
                let mut s = format!("\"{}\", line {}: ", f, self.linenum);
                if let Some((rf, rl)) = &self.rloc {
                    s.push_str(&format!("(rule from \"{rf}\", line {rl}) "));
                }
                s
            }
            None => String::new(),
        }
    }

    fn error(&mut self, msg: &str) {
        self.errors += 1;
        io::eprint(format!("{}{}\n", self.loc(), msg));
    }

    fn warning(&mut self, msg: &str) {
        io::eprint(format!("{}warning: {}\n", self.loc(), msg));
    }

    fn fatal(&mut self, msg: &str) -> ! {
        self.error(msg);
        sys::exit(1)
    }

    fn eat(&mut self, file: &str, line: usize) {
        self.filename = Some(file.to_string());
        self.linenum = line;
        self.rloc = None;
    }

    fn tadd(&mut self, t1: i64, t2: i64) -> i64 {
        if t1 == ZIC_MAX && t2 > 0 {
            return ZIC_MAX;
        }
        if t1 == ZIC_MIN && t2 < 0 {
            return ZIC_MIN;
        }
        match t1.checked_add(t2) {
            Some(v) => v,
            None => self.fatal("time overflow"),
        }
    }

    fn oadd(&mut self, t1: i64, t2: i64) -> i64 {
        match t1.checked_add(t2) {
            Some(v) => v,
            None => self.fatal("time overflow"),
        }
    }

    /// `gethms`: `[-]h[:mm[:ss[.f]]]` em segundos; vazio é zero.
    fn gethms(&mut self, string: &str, errstring: &str) -> i64 {
        if string.is_empty() {
            return 0;
        }
        let (sign, s) = match string.strip_prefix('-') {
            Some(r) => (-1i64, r),
            None => (1i64, string),
        };
        let b = s.as_bytes();
        let mut i = 0;
        let num = |i: &mut usize| -> Option<i64> {
            let st = *i;
            let mut v: i64 = 0;
            while *i < b.len() && b[*i].is_ascii_digit() {
                v = v.saturating_mul(10).saturating_add(i64::from(b[*i] - b'0'));
                *i += 1;
            }
            if *i == st { None } else { Some(v) }
        };
        let mut ok = true;
        let (mut mm, mut ss, mut tenths) = (0i64, 0i64, 0i64);
        let mut xr = b'0';
        let hh = num(&mut i);
        if hh.is_none() {
            ok = false;
        }
        if ok && i < b.len() {
            if b[i] != b':' {
                ok = false;
            } else {
                i += 1;
                match num(&mut i) {
                    None => ok = false,
                    Some(v) => mm = v,
                }
                if ok && i < b.len() {
                    if b[i] != b':' {
                        ok = false;
                    } else {
                        i += 1;
                        match num(&mut i) {
                            None => ok = false,
                            Some(v) => ss = v,
                        }
                        if ok && i < b.len() {
                            if b[i] != b'.' {
                                ok = false;
                            } else {
                                i += 1;
                                if i < b.len() && b[i].is_ascii_digit() {
                                    tenths = i64::from(b[i] - b'0');
                                    i += 1;
                                    while i < b.len() && b[i] == b'0' {
                                        i += 1;
                                    }
                                    if i < b.len() {
                                        xr = b[i];
                                        ok = xr.is_ascii_digit();
                                    }
                                } else {
                                    ok = false;
                                }
                            }
                        }
                    }
                }
            }
        }
        let hh = hh.unwrap_or(0);
        if !ok || !(0..=59).contains(&mm) || !(0..=60).contains(&ss) {
            self.error(errstring);
            return 0;
        }
        let round = i64::from(5 + ((ss ^ 1) & i64::from(xr == b'0')) <= tenths);
        ss += round;
        let total = hh
            .checked_mul(3600)
            .and_then(|v| v.checked_add(mm * 60 + ss));
        match total {
            Some(v) => sign * v,
            None => {
                self.error(errstring);
                0
            }
        }
    }

    fn getsave(&mut self, field: &str) -> (i64, bool) {
        let mut dst: i32 = -1;
        let mut f = field.to_string();
        if let Some(last) = f.chars().last() {
            match last {
                'd' => {
                    dst = 1;
                    f.pop();
                }
                's' => {
                    dst = 0;
                    f.pop();
                }
                _ => {}
            }
        }
        let save = self.gethms(&f, "invalid saved time");
        let isdst = if dst < 0 { save != 0 } else { dst != 0 };
        (save, isdst)
    }

    fn rulesub(
        &mut self,
        rp: &mut Rule,
        loyearp: &str,
        hiyearp: &str,
        typep: &str,
        monthp: &str,
        dayp: &str,
        timep: &str,
    ) -> bool {
        let Some(m) = byword(monthp, &MONTHS) else {
            self.error("invalid month name");
            return false;
        };
        rp.month = m;
        rp.todisstd = false;
        rp.todisut = false;
        let mut dp = timep.to_string();
        if let Some(last) = dp.chars().last() {
            match last.to_ascii_lowercase() {
                's' => {
                    rp.todisstd = true;
                    rp.todisut = false;
                    dp.pop();
                }
                'w' => {
                    rp.todisstd = false;
                    rp.todisut = false;
                    dp.pop();
                }
                'g' | 'u' | 'z' => {
                    rp.todisstd = true;
                    rp.todisut = true;
                    dp.pop();
                }
                _ => {}
            }
        }
        rp.tod = self.gethms(&dp, "invalid time of day");
        // Anos.
        if byword(loyearp, &BEGIN_YEARS).is_some() {
            self.warning(&format!(
                "FROM year \"{}\" is obsolete; treated as {}",
                loyearp,
                YEAR_32BIT_MIN - 1
            ));
            rp.loyear = YEAR_32BIT_MIN - 1;
            rp.lowasnum = false;
        } else if let Some(y) = parse_whole(loyearp) {
            rp.loyear = y;
            rp.lowasnum = true;
        } else {
            self.error("invalid starting year");
            return false;
        }
        match byword(hiyearp, &END_YEARS) {
            Some(0) => {
                rp.hiwasnum = false;
                rp.hiyear = rp.loyear;
            }
            Some(_) => {
                rp.hiwasnum = false;
                rp.hiyear = ZIC_MAX;
            }
            None => {
                rp.hiwasnum = true;
                match parse_whole(hiyearp) {
                    Some(y) => rp.hiyear = y,
                    None => {
                        self.error("invalid ending year");
                        return false;
                    }
                }
            }
        }
        if rp.loyear > rp.hiyear {
            self.error("starting year greater than ending year");
            return false;
        }
        if !typep.is_empty() {
            self.error(&format!(
                "year type \"{typep}\" is unsupported; use \"-\" instead"
            ));
            return false;
        }
        // Dia: `1`, `lastSun`, `Sun<=20`, `Sun>=7`.
        let mut last_wday = None;
        if dayp.len() > 4 && dayp.is_ascii() && dayp.as_bytes()[..4].eq_ignore_ascii_case(b"last") {
            let mut rest = &dayp[4..];
            if let Some(r) = rest.strip_prefix('-') {
                self.warning(&format!(
                    "\"{dayp}\" is undocumented; use \"lastSunday\" instead"
                ));
                rest = r;
            }
            last_wday = byword(rest, &WDAYS);
        }
        if let Some(w) = last_wday {
            rp.dycode = DC_DOWLEQ;
            rp.wday = w as i64;
            rp.dayofmonth = LEN_MONTHS[1][rp.month];
        } else {
            let (dpart, rest, code);
            if let Some(p) = dayp.find('<') {
                code = DC_DOWLEQ;
                dpart = &dayp[..p];
                rest = &dayp[p + 1..];
            } else if let Some(p) = dayp.find('>') {
                code = DC_DOWGEQ;
                dpart = &dayp[..p];
                rest = &dayp[p + 1..];
            } else {
                code = DC_DOM;
                dpart = "";
                rest = dayp;
            }
            rp.dycode = code;
            let mut numtxt = rest;
            if code != DC_DOM {
                match numtxt.strip_prefix('=') {
                    Some(r) => numtxt = r,
                    None => {
                        self.error("invalid day of month");
                        return false;
                    }
                }
                match byword(dpart, &WDAYS) {
                    Some(w) => rp.wday = w as i64,
                    None => {
                        self.error("invalid weekday name");
                        return false;
                    }
                }
            }
            match parse_whole(numtxt) {
                Some(d) if d > 0 && d <= LEN_MONTHS[1][rp.month] => rp.dayofmonth = d,
                _ => {
                    self.error("invalid day of month");
                    return false;
                }
            }
        }
        true
    }

    fn inrule(&mut self, f: &[String]) {
        if f.len() != 10 {
            self.error("wrong number of fields on Rule line");
            return;
        }
        let mut r = Rule {
            name: f[1].clone(),
            file: self.filename.clone().unwrap_or_default(),
            line: self.linenum,
            ..Rule::default()
        };
        let typ = if f[4] == "-" { "" } else { f[4].as_str() };
        if !self.rulesub(&mut r, &f[2], &f[3], typ, &f[5], &f[6], &f[7]) {
            return;
        }
        let (save, isdst) = self.getsave(&f[8]);
        r.save = save;
        r.isdst = isdst;
        r.abbrvar = if f[9] == "-" { String::new() } else { f[9].clone() };
        self.rules.push(r);
    }

    fn inzone(&mut self, f: &[String]) -> bool {
        if f.len() < 5 || f.len() > 9 {
            self.error("wrong number of fields on Zone line");
            return false;
        }
        for z in &self.zones {
            if z.name.as_deref() == Some(f[1].as_str()) {
                let msg = format!(
                    "duplicate zone name {} (file \"{}\", line {})",
                    f[1], z.file, z.line
                );
                self.error(&msg);
                return false;
            }
        }
        self.inzsub(f, false)
    }

    fn inzcont(&mut self, f: &[String]) -> bool {
        if f.len() < 3 || f.len() > 7 {
            self.error("wrong number of fields on Zone continuation line");
            return false;
        }
        self.inzsub(f, true)
    }

    fn inzsub(&mut self, f: &[String], iscont: bool) -> bool {
        let (i_stdoff, i_rule, i_format, i_untilyear) =
            if iscont { (0, 1, 2, 3) } else { (2, 3, 4, 5) };
        let mut z = Zone {
            file: self.filename.clone().unwrap_or_default(),
            line: self.linenum,
            ..Zone::default()
        };
        if !iscont {
            z.name = Some(f[1].clone());
        }
        z.stdoff = self.gethms(&f[i_stdoff], "invalid UT offset");
        let fmt = &f[i_format];
        if let Some(p) = fmt.find('%') {
            let after = &fmt[p + 1..];
            let c = after.chars().next();
            if (c != Some('s') && c != Some('z')) || after.contains('%') || after.contains('/') {
                self.error("invalid abbreviation format");
                return false;
            }
            z.spec = after.as_bytes()[0];
        }
        z.format = fmt.clone();
        if z.spec == b'z' {
            let p = fmt.find('%').unwrap_or(0);
            let mut s = fmt.clone();
            s.replace_range(p + 1..p + 2, "s");
            z.format = s;
        }
        z.rule = if f[i_rule] == "-" { String::new() } else { f[i_rule].clone() };
        let hasuntil = f.len() > i_untilyear;
        if hasuntil {
            let month = f.get(i_untilyear + 1).map_or("Jan", String::as_str);
            let day = f.get(i_untilyear + 2).map_or("1", String::as_str);
            let time = f.get(i_untilyear + 3).map_or("0", String::as_str);
            let mut ur = Rule::default();
            if !self.rulesub(&mut ur, &f[i_untilyear], "only", "", month, day, time) {
                return false;
            }
            z.untilrule = ur.clone();
            z.untiltime = self.rpytime(&ur, ur.loyear);
            if iscont
                && let Some(prev) = self.zones.last()
            {
                let pu = prev.untiltime;
                if z.untiltime > ZIC_MIN
                    && z.untiltime < ZIC_MAX
                    && pu > ZIC_MIN
                    && pu < ZIC_MAX
                    && pu >= z.untiltime
                {
                    self.error("Zone continuation line end time is not after end time of previous line");
                    return false;
                }
            }
        }
        self.zones.push(z);
        hasuntil
    }

    fn inlink(&mut self, f: &[String]) {
        if f.len() != 3 {
            self.error("wrong number of fields on Link line");
            return;
        }
        if f[1].is_empty() || f[1] == "-" {
            self.error("blank TARGET field on Link line");
            return;
        }
        self.links.push(Link {
            file: self.filename.clone().unwrap_or_default(),
            line: self.linenum,
            target: f[1].clone(),
            name: f[2].clone(),
        });
    }

    fn inleap(&mut self, f: &[String]) {
        if f.len() != 8 {
            self.error("wrong number of fields on Leap line");
            return;
        }
        let Some(year) = parse_whole(&f[1]) else {
            self.error("invalid leaping year");
            return;
        };
        if !self.leapseen || self.leapmaxyear < year {
            self.leapmaxyear = year;
        }
        if !self.leapseen || self.leapminyear > year {
            self.leapminyear = year;
        }
        self.leapseen = true;
        let Some(month) = byword(&f[2], &MONTHS) else {
            self.error("invalid month name");
            return;
        };
        let day = match parse_whole(&f[3]) {
            Some(d) if d > 0 && d <= LEN_MONTHS[leap_idx(year)][month] => d,
            _ => {
                self.error("invalid day of month");
                return;
            }
        };
        let mut dayoff: i64 = 0;
        let mut j = EPOCH_YEAR;
        while j != year {
            if year > j {
                dayoff += len_year(j);
                j += 1;
            } else {
                j -= 1;
                dayoff -= len_year(j);
            }
        }
        for m in 0..month {
            dayoff += LEN_MONTHS[leap_idx(year)][m];
        }
        dayoff += day - 1;
        let tod = self.gethms(&f[4], "invalid time of day");
        let t = dayoff * SECS_PER_DAY + tod;
        let correction = match f[5].as_str() {
            "+" => 1,
            "-" => -1,
            _ => {
                self.error("illegal CORRECTION field on Leap line");
                return;
            }
        };
        if byword(&f[6], &LEAP_TYPES).is_none() {
            self.error("illegal Rolling/Stationary field on Leap line");
            return;
        }
        // `leapadd`: inserção ordenada; `adjleap` acumula as correções depois.
        let mut i = 0;
        while i < self.leaps.len() && t > self.leaps[i].0 {
            i += 1;
        }
        self.leaps.insert(i, (t, correction));
    }

    fn adjleap(&mut self) {
        let mut last = 0i64;
        let mut prevtrans = 0i64;
        for i in 0..self.leaps.len() {
            if self.leaps[i].0 - prevtrans < 28 * SECS_PER_DAY {
                self.fatal("Leap seconds too close together");
            }
            prevtrans = self.leaps[i].0;
            self.leaps[i].0 += last;
            self.leaps[i].1 += last;
            last = self.leaps[i].1;
        }
    }

    fn infile(&mut self, name: &str) {
        let data = if name == "-" {
            io::read_stdin()
        } else {
            io::read_path(name.as_bytes())
        };
        let data = match data {
            Ok(d) => d,
            Err(e) => {
                io::eprint(format!(
                    "{}: Can't open {}: {}\n",
                    self.progname,
                    name,
                    e.message()
                ));
                sys::exit(1);
            }
        };
        let shown = if name == "-" { "standard input" } else { name };
        self.filename = Some(shown.to_string());
        self.rloc = None;
        let mut wantcont = false;
        let mut linenum = 0usize;
        let mut lines: Vec<&[u8]> = data.split(|&c| c == b'\n').collect();
        if data.last() == Some(&b'\n') {
            lines.pop();
        }
        for line in lines {
            linenum += 1;
            self.linenum = linenum;
            let f = getfields(line);
            if f.is_empty() {
                continue;
            }
            if wantcont {
                wantcont = self.inzcont(&f);
                continue;
            }
            match byword(&f[0], &LINE_CODES) {
                Some(0) => self.inrule(&f),
                Some(1) => wantcont = self.inzone(&f),
                Some(2) => self.inlink(&f),
                Some(_) => self.inleap(&f),
                None => self.error("input line of unknown type"),
            }
        }
        if wantcont {
            self.linenum = linenum + 1;
            self.error("expected continuation line not found");
        }
    }

    /// `rpytime`: o instante (hora local do relógio da regra) do `rp` no ano dado.
    fn rpytime(&mut self, rp: &Rule, wantedy: i64) -> i64 {
        if wantedy == ZIC_MIN || wantedy == ZIC_MAX {
            return wantedy;
        }
        let mut y = EPOCH_YEAR;
        let mut dayoff: i64 = 0;
        while wantedy != y {
            let i;
            if wantedy > y {
                i = len_year(y);
                y += 1;
            } else {
                y -= 1;
                i = -len_year(y);
            }
            dayoff = self.oadd(dayoff, i);
        }
        let mut m = 0usize;
        while m != rp.month {
            let i = LEN_MONTHS[leap_idx(y)][m];
            dayoff = self.oadd(dayoff, i);
            m += 1;
        }
        let mut i = rp.dayofmonth;
        if m == 1 && i == 29 && !isleap(y) {
            if rp.dycode == DC_DOWLEQ {
                i -= 1;
            } else {
                self.fatal("use of 2/29 in non leap-year");
            }
        }
        i -= 1;
        dayoff = self.oadd(dayoff, i);
        if rp.dycode == DC_DOWGEQ || rp.dycode == DC_DOWLEQ {
            let mut wday = (EPOCH_WDAY + dayoff % 7 + 7) % 7;
            while wday != rp.wday {
                if rp.dycode == DC_DOWGEQ {
                    dayoff = self.oadd(dayoff, 1);
                    wday += 1;
                    if wday >= 7 {
                        wday = 0;
                    }
                    i += 1;
                } else {
                    dayoff = self.oadd(dayoff, -1);
                    wday -= 1;
                    if wday < 0 {
                        wday = 6;
                    }
                    i -= 1;
                }
            }
            if (i < 0 || i >= LEN_MONTHS[leap_idx(y)][m]) && self.noise {
                self.warning(
                    "rule goes past start/end of month; will not work with pre-2004 versions of zic",
                );
            }
        }
        if dayoff < ZIC_MIN / SECS_PER_DAY {
            return ZIC_MIN;
        }
        if dayoff > ZIC_MAX / SECS_PER_DAY {
            return ZIC_MAX;
        }
        let t = dayoff * SECS_PER_DAY;
        self.tadd(t, rp.tod)
    }

    /// `associate`: liga cada zona às suas regras (ou ao deslocamento fixo do campo RULES).
    fn associate(&mut self) {
        let mut rules = std::mem::take(&mut self.rules);
        rules.sort_by(|a, b| a.name.cmp(&b.name));
        let mut zones = std::mem::take(&mut self.zones);
        for z in zones.iter_mut() {
            z.rules = rules
                .iter()
                .filter(|r| !z.rule.is_empty() && r.name == z.rule)
                .cloned()
                .collect();
        }
        for z in zones.iter_mut() {
            if z.rules.is_empty() {
                self.eat(&z.file.clone(), z.line);
                let (save, isdst) = self.getsave(&z.rule.clone());
                z.save = save;
                z.isdst = isdst;
                if z.spec == b's' {
                    self.error("%s in rule-less zone");
                }
            }
        }
        self.rules = rules;
        self.zones = zones;
        if self.errors > 0 {
            sys::exit(1);
        }
    }

    // ---- tabelas de tipos e transições ----

    fn newabbr(&mut self, s: &str) {
        let b = s.as_bytes();
        if s != "Local time zone must be set--see zic manual page" {
            let mut n = 0;
            while n < b.len() && (b[n].is_ascii_alphanumeric() || b[n] == b'-' || b[n] == b'+') {
                n += 1;
            }
            let mut mp: Option<&str> = None;
            if self.noise && n < 3 {
                mp = Some("time zone abbreviation has fewer than 3 characters");
            }
            if n > 6 {
                mp = Some("time zone abbreviation has too many characters");
            }
            if n < b.len() {
                mp = Some("time zone abbreviation differs from POSIX standard");
            }
            if let Some(m) = mp {
                self.warning(&format!("{m} ({s})"));
            }
        }
        if self.tz.chars.len() + b.len() + 1 > 50 * 256 {
            self.fatal("too many, or too long, time zone abbreviations");
        }
        self.tz.chars.extend_from_slice(b);
        self.tz.chars.push(0);
    }

    fn addtype(&mut self, utoff: i64, abbr: &str, isdst: bool, ttisstd: bool, ttisut: bool) -> usize {
        if !(-1 - 2_147_483_647..=2_147_483_647).contains(&utoff) {
            self.fatal("UT offset out of range");
        }
        let (ttisstd, ttisut) = if self.want_bloat() { (ttisstd, ttisut) } else { (false, false) };
        let ab = abbr.as_bytes();
        let mut found = None;
        for j in 0..self.tz.chars.len() {
            if cstr_at(&self.tz.chars, j) == ab {
                found = Some(j);
                break;
            }
        }
        let j = match found {
            Some(j) => {
                for i in 0..self.tz.utoffs.len() {
                    if utoff == self.tz.utoffs[i]
                        && i8::from(isdst) == self.tz.isdsts[i]
                        && j == self.tz.desigidx[i]
                        && ttisstd == self.tz.ttisstds[i]
                        && ttisut == self.tz.ttisuts[i]
                    {
                        return i;
                    }
                }
                j
            }
            None => {
                let j = self.tz.chars.len();
                self.newabbr(abbr);
                j
            }
        };
        if self.tz.utoffs.len() >= 256 {
            self.fatal("too many local time types");
        }
        self.tz.utoffs.push(utoff);
        self.tz.isdsts.push(i8::from(isdst));
        self.tz.ttisstds.push(ttisstd);
        self.tz.ttisuts.push(ttisut);
        self.tz.desigidx.push(j);
        self.tz.utoffs.len() - 1
    }

    fn addtt(&mut self, at: i64, ty: usize) {
        self.tz.attypes.push(AtType {
            at,
            dontmerge: false,
            ty,
        });
    }

    fn doabbr(&mut self, zf: &ZFmt, letters: Letters<'_>, isdst: bool, save: i64, doquotes: bool) -> String {
        let format = zf.format.as_str();
        let mut abbr: String;
        if let Some(p) = format.find('/') {
            abbr = if isdst { format[p + 1..].to_string() } else { format[..p].to_string() };
        } else {
            let lt: String = if zf.spec == b'z' {
                match abbroffset(zf.stdoff + save) {
                    Some(s) => s,
                    None => {
                        self.error("%z UT offset magnitude exceeds 99:59:59");
                        "%z".to_string()
                    }
                }
            } else {
                match letters {
                    Letters::Null => "%s".to_string(),
                    Letters::Disable => return String::new(),
                    Letters::Str(s) => s.to_string(),
                }
            };
            abbr = match format.find("%s") {
                Some(p) => format!("{}{}{}", &format[..p], lt, &format[p + 2..]),
                None => format.to_string(),
            };
        }
        if !doquotes {
            return abbr;
        }
        let plain = abbr.bytes().all(|c| c.is_ascii_alphabetic());
        if !abbr.is_empty() && plain {
            return abbr;
        }
        abbr = format!("<{abbr}>");
        abbr
    }

    /// `stringrule`: a regra POSIX `Mm.w.d/hora` ou `Jn`/`n`; `None` quando não cabe.
    fn stringrule(&mut self, rp: &Rule, save: i64, stdoff: i64) -> Option<(String, i32)> {
        let mut tod = rp.tod;
        let mut compat = 0;
        let mut s = String::new();
        if rp.dycode == DC_DOM {
            if rp.dayofmonth == 29 && rp.month == 1 {
                return None;
            }
            let mut total = 0;
            for m in 0..rp.month {
                total += LEN_MONTHS[0][m];
            }
            if rp.month <= 1 {
                s.push_str(&(total + rp.dayofmonth - 1).to_string());
            } else {
                s.push_str(&format!("J{}", total + rp.dayofmonth));
            }
        } else {
            let week;
            if rp.dycode == DC_DOWGEQ {
                let wdayoff = (rp.dayofmonth - 1) % 7;
                if wdayoff != 0 {
                    compat = 2013;
                }
                tod += wdayoff * SECS_PER_DAY;
                week = 1 + (rp.dayofmonth - 1) / 7;
            } else if rp.dycode == DC_DOWLEQ {
                if rp.dayofmonth == LEN_MONTHS[1][rp.month] {
                    week = 5;
                } else {
                    let wdayoff = rp.dayofmonth % 7;
                    if wdayoff != 0 {
                        compat = 2013;
                    }
                    tod -= wdayoff * SECS_PER_DAY;
                    week = rp.dayofmonth / 7;
                }
            } else {
                return None;
            }
            s.push_str(&format!("M{}.{}.{}", rp.month + 1, week, rp.wday));
        }
        if rp.todisut {
            tod += stdoff;
        }
        if rp.todisstd && !rp.isdst {
            tod += save;
        }
        if tod != 2 * 3600 {
            s.push('/');
            s.push_str(&stringoffset(tod)?);
            if tod < 0 {
                if compat < 2013 {
                    compat = 2013;
                }
            } else if tod >= SECS_PER_DAY && compat < 1994 {
                compat = 1994;
            }
        }
        Some((s, compat))
    }

    /// `stringzone`: o rodapé TZ e o ano da compatibilidade (negativo quando não cabe).
    fn stringzone(&mut self, zones: &[Zone]) -> (String, i32) {
        if self.hi_time < ZIC_MAX {
            return (String::new(), -1);
        }
        let zp = &zones[zones.len() - 1];
        let zf = ZFmt {
            format: zp.format.clone(),
            spec: zp.spec,
            stdoff: zp.stdoff,
        };
        let mut stdrp: Option<Rule> = None;
        let mut dstrp: Option<Rule> = None;
        for rp in &zp.rules {
            if rp.hiwasnum || rp.hiyear != ZIC_MAX {
                continue;
            }
            if !rp.isdst {
                if stdrp.is_none() {
                    stdrp = Some(rp.clone());
                } else {
                    return (String::new(), -1);
                }
            } else if dstrp.is_none() {
                dstrp = Some(rp.clone());
            } else {
                return (String::new(), -1);
            }
        }
        if stdrp.is_none() && dstrp.is_none() {
            let mut stdabbrrp: Option<Rule> = None;
            for rp in &zp.rules {
                if !rp.isdst && rule_cmp(stdabbrrp.as_ref(), rp) < 0 {
                    stdabbrrp = Some(rp.clone());
                }
                if rule_cmp(stdrp.as_ref(), rp) < 0 {
                    stdrp = Some(rp.clone());
                }
            }
            if let Some(s) = stdrp.clone()
                && s.isdst
            {
                // DST perpétuo.
                let dstr = Rule {
                    month: 0,
                    dycode: DC_DOM,
                    dayofmonth: 1,
                    tod: 0,
                    todisstd: false,
                    todisut: false,
                    isdst: s.isdst,
                    save: s.save,
                    abbrvar: s.abbrvar.clone(),
                    ..Rule::default()
                };
                let stdr = Rule {
                    month: 11,
                    dycode: DC_DOM,
                    dayofmonth: 31,
                    tod: SECS_PER_DAY + s.save,
                    todisstd: false,
                    todisut: false,
                    isdst: false,
                    save: 0,
                    abbrvar: stdabbrrp.map(|r| r.abbrvar).unwrap_or_default(),
                    ..Rule::default()
                };
                dstrp = Some(dstr);
                stdrp = Some(stdr);
            }
        }
        if stdrp.is_none() && (!zp.rules.is_empty() || zp.isdst) {
            return (String::new(), -1);
        }
        let mut compat = 0;
        let abbrvar = stdrp.as_ref().map(|r| r.abbrvar.clone()).unwrap_or_default();
        let mut result = self.doabbr(&zf, Letters::Str(&abbrvar), false, 0, true);
        match stringoffset(-zp.stdoff) {
            Some(o) => result.push_str(&o),
            None => return (String::new(), -1),
        }
        let Some(dstrp) = dstrp else {
            return (result, compat);
        };
        let stdrp = stdrp.unwrap_or_default();
        let a = self.doabbr(&zf, Letters::Str(&dstrp.abbrvar), dstrp.isdst, dstrp.save, true);
        result.push_str(&a);
        if dstrp.save != 3600 {
            match stringoffset(-(zp.stdoff + dstrp.save)) {
                Some(o) => result.push_str(&o),
                None => return (String::new(), -1),
            }
        }
        result.push(',');
        match self.stringrule(&dstrp, dstrp.save, zp.stdoff) {
            Some((s, c)) => {
                result.push_str(&s);
                compat = compat.max(c);
            }
            None => return (String::new(), -1),
        }
        result.push(',');
        match self.stringrule(&stdrp, dstrp.save, zp.stdoff) {
            Some((s, c)) => {
                result.push_str(&s);
                compat = compat.max(c);
            }
            None => return (String::new(), -1),
        }
        (result, compat)
    }

    fn outzone(&mut self, zones: &mut [Zone]) {
        let zonecount = zones.len();
        self.tz = Tz::default();
        let mut defaulttype: i64 = -1;
        let mut lastatmax: i64 = -1;
        let mut unspecifiedtype: i64 = -1;
        let mut startttisstd = false;
        let mut startttisut = false;
        let mut starttime: i64 = 0;
        let mut prodstic = zonecount == 1;
        let mut min_year = EPOCH_YEAR;
        let mut max_year = EPOCH_YEAR;
        if self.leapseen {
            let (a, b) = (self.leapminyear, self.leapmaxyear + i64::from(self.leapmaxyear < ZIC_MAX));
            min_year = min_year.min(a).min(b);
            max_year = max_year.max(a).max(b);
        }
        for i in 0..zonecount {
            if i < zonecount - 1 {
                let y = zones[i].untilrule.loyear;
                min_year = min_year.min(y);
                max_year = max_year.max(y);
            }
            for rp in &zones[i].rules {
                if rp.lowasnum {
                    min_year = min_year.min(rp.loyear);
                    max_year = max_year.max(rp.loyear);
                }
                if rp.hiwasnum {
                    min_year = min_year.min(rp.hiyear);
                    max_year = max_year.max(rp.hiyear);
                }
                if rp.lowasnum || rp.hiwasnum {
                    prodstic = false;
                }
            }
        }
        let (envvar, compat) = self.stringzone(zones);
        let version = if compat < 2013 { b'2' } else { b'3' };
        let do_extend = compat < 0;
        if do_extend {
            if min_year >= ZIC_MIN + YEARS_OF_OBSERVATIONS {
                min_year -= YEARS_OF_OBSERVATIONS;
            } else {
                min_year = ZIC_MIN;
            }
            if max_year <= ZIC_MAX - YEARS_OF_OBSERVATIONS {
                max_year += YEARS_OF_OBSERVATIONS;
            } else {
                max_year = ZIC_MAX;
            }
            if prodstic {
                min_year = 1900;
                max_year = min_year + YEARS_OF_OBSERVATIONS;
            }
        }
        let floor = (self.redundant_time / (SECS_PER_DAY * 365)).saturating_add(EPOCH_YEAR + 1);
        max_year = max_year.max(floor);
        let max_year0 = max_year;
        if self.want_bloat() {
            if min_year > 1900 {
                min_year = 1900;
            }
            if max_year < 2038 {
                max_year = 2038;
            }
        }
        if ZIC_MIN < self.lo_time || self.hi_time < ZIC_MAX {
            unspecifiedtype = self.addtype(0, "-00", false, false, false) as i64;
        }
        for i in 0..zonecount {
            let mut prevrp_hi: Option<i64> = None;
            let mut prevktime: i64 = 0;
            let mut save: i64 = 0;
            let mut usestart = i > 0 && zones[i - 1].untiltime > ZIC_MIN;
            let useuntil = i < zonecount - 1;
            if useuntil && zones[i].untiltime <= ZIC_MIN {
                continue;
            }
            let stdoff = zones[i].stdoff;
            let zf = ZFmt {
                format: zones[i].format.clone(),
                spec: zones[i].spec,
                stdoff,
            };
            let (zfile, zline) = (zones[i].file.clone(), zones[i].line);
            self.eat(&zfile, zline);
            let mut startbuf = String::new();
            let mut startoff = stdoff;
            let nrules = zones[i].rules.len();
            if nrules == 0 {
                save = zones[i].save;
                startbuf = self.doabbr(&zf, Letters::Null, zones[i].isdst, save, false);
                let off = self.oadd(stdoff, save);
                let ty = self.addtype(off, &startbuf, zones[i].isdst, startttisstd, startttisut);
                if usestart {
                    self.addtt(starttime, ty);
                    usestart = false;
                } else {
                    defaulttype = ty as i64;
                }
            } else {
                let mut year = min_year;
                while year <= max_year {
                    if useuntil && year > zones[i].untilrule.hiyear {
                        break;
                    }
                    for j in 0..nrules {
                        let r = zones[i].rules[j].clone();
                        let mut todo = year >= r.loyear && year <= r.hiyear;
                        let mut temp = 0;
                        if todo {
                            temp = self.rpytime(&r, year);
                            todo = temp < Y2038_BOUNDARY || year <= max_year0;
                        }
                        zones[i].rules[j].todo = todo;
                        zones[i].rules[j].temp = temp;
                    }
                    loop {
                        let mut untiltime = 0;
                        if useuntil {
                            untiltime = zones[i].untiltime;
                            if !zones[i].untilrule.todisut {
                                untiltime = self.tadd(untiltime, -stdoff);
                            }
                            if !zones[i].untilrule.todisstd {
                                untiltime = self.tadd(untiltime, -save);
                            }
                        }
                        let mut k: i64 = -1;
                        let mut ktime: i64 = 0;
                        for j in 0..nrules {
                            let r = zones[i].rules[j].clone();
                            if !r.todo {
                                continue;
                            }
                            let mut offset = if r.todisut { 0 } else { stdoff };
                            if !r.todisstd {
                                offset = self.oadd(offset, save);
                            }
                            let mut jtime = r.temp;
                            if jtime == ZIC_MIN || jtime == ZIC_MAX {
                                continue;
                            }
                            jtime = self.tadd(jtime, -offset);
                            if k < 0 || jtime < ktime {
                                k = j as i64;
                                ktime = jtime;
                            } else if jtime == ktime {
                                let msg = "two rules for same instant";
                                self.eat(&zfile, zline);
                                self.rloc = Some((r.file.clone(), r.line));
                                self.warning(msg);
                                let r0 = zones[i].rules[k as usize].clone();
                                self.rloc = Some((r0.file.clone(), r0.line));
                                self.error(msg);
                                self.eat(&zfile, zline);
                            }
                        }
                        if k < 0 {
                            break;
                        }
                        let rp = zones[i].rules[k as usize].clone();
                        zones[i].rules[k as usize].todo = false;
                        if useuntil && ktime >= untiltime {
                            let o = self.oadd(stdoff, rp.save);
                            if startbuf.is_empty() && o == startoff {
                                startbuf = self.doabbr(&zf, Letters::Str(&rp.abbrvar), rp.isdst, rp.save, false);
                            }
                            break;
                        }
                        save = rp.save;
                        if usestart && ktime == starttime {
                            usestart = false;
                        }
                        if usestart {
                            if ktime < starttime {
                                startoff = self.oadd(stdoff, save);
                                startbuf =
                                    self.doabbr(&zf, Letters::Str(&rp.abbrvar), rp.isdst, rp.save, false);
                                continue;
                            }
                            let o = self.oadd(stdoff, save);
                            if startbuf.is_empty() && startoff == o {
                                startbuf =
                                    self.doabbr(&zf, Letters::Str(&rp.abbrvar), rp.isdst, rp.save, false);
                            }
                        }
                        let ab = self.doabbr(&zf, Letters::Str(&rp.abbrvar), rp.isdst, rp.save, false);
                        let offset = self.oadd(stdoff, rp.save);
                        if !self.want_bloat()
                            && !useuntil
                            && !do_extend
                            && let Some(ph) = prevrp_hi
                            && self.lo_time <= prevktime
                            && self.redundant_time <= ktime
                            && rp.hiyear == ZIC_MAX
                            && ph == ZIC_MAX
                        {
                            break;
                        }
                        let ty = self.addtype(offset, &ab, rp.isdst, rp.todisstd, rp.todisut);
                        if defaulttype < 0 && !rp.isdst {
                            defaulttype = ty as i64;
                        }
                        if rp.hiyear == ZIC_MAX
                            && !(lastatmax >= 0 && ktime < self.tz.attypes[lastatmax as usize].at)
                        {
                            lastatmax = self.tz.attypes.len() as i64;
                        }
                        self.addtt(ktime, ty);
                        prevrp_hi = Some(rp.hiyear);
                        prevktime = ktime;
                    }
                    if year == i64::MAX {
                        break;
                    }
                    year += 1;
                }
            }
            if usestart {
                let isdst = startoff != stdoff;
                if startbuf.is_empty() && !zf.format.is_empty() {
                    startbuf = self.doabbr(&zf, Letters::Disable, isdst, save, false);
                }
                self.eat(&zfile, zline);
                if startbuf.is_empty() {
                    self.error("can't determine time zone abbreviation to use just after until time");
                } else {
                    let ty = self.addtype(startoff, &startbuf, isdst, startttisstd, startttisut);
                    if defaulttype < 0 && !isdst {
                        defaulttype = ty as i64;
                    }
                    self.addtt(starttime, ty);
                }
            }
            if useuntil {
                startttisstd = zones[i].untilrule.todisstd;
                startttisut = zones[i].untilrule.todisut;
                starttime = zones[i].untiltime;
                if !startttisstd {
                    starttime = self.tadd(starttime, -save);
                }
                if !startttisut {
                    starttime = self.tadd(starttime, -stdoff);
                }
            }
        }
        if defaulttype < 0 {
            defaulttype = 0;
        }
        if lastatmax >= 0 {
            self.tz.attypes[lastatmax as usize].dontmerge = true;
        }
        let name = zones[0].name.clone().unwrap_or_default();
        self.writezone(&name, &envvar, version, defaulttype as usize, unspecifiedtype);
    }

    fn put32(out: &mut Vec<u8>, v: i64) {
        out.extend_from_slice(&(v as i32).to_be_bytes());
    }

    fn put_pass(out: &mut Vec<u8>, v: i64, pass: u32) {
        if pass == 1 {
            Z::put32(out, v);
        } else {
            out.extend_from_slice(&v.to_be_bytes());
        }
    }

    fn writezone(&mut self, name: &str, string: &str, version: u8, defaulttype: usize, unspecifiedtype: i64) {
        // Ordena (estável) e otimiza.
        self.tz.attypes.sort_by_key(|a| a.at);
        let mut kept: Vec<AtType> = Vec::new();
        for a in self.tz.attypes.clone() {
            let toi = kept.len();
            if toi != 0 {
                let prev = kept[toi - 1];
                let prev2 = if toi == 1 { 0 } else { kept[toi - 2].ty };
                if a.at.wrapping_add(self.tz.utoffs[prev.ty])
                    <= prev.at.wrapping_add(self.tz.utoffs[prev2])
                {
                    kept[toi - 1].ty = a.ty;
                    continue;
                }
            }
            let push = toi == 0 || a.dontmerge || {
                let prev = kept[toi - 1];
                self.tz.utoffs[prev.ty] != self.tz.utoffs[a.ty]
                    || self.tz.isdsts[prev.ty] != self.tz.isdsts[a.ty]
                    || self.tz.desigidx[prev.ty] != self.tz.desigidx[a.ty]
            };
            if push {
                kept.push(a);
            }
        }
        let mut ats: Vec<i64> = kept.iter().map(|a| a.at).collect();
        let mut types: Vec<usize> = kept.iter().map(|a| a.ty).collect();
        let trans: Vec<i64> = self.leaps.iter().map(|l| l.0).collect();
        let corr: Vec<i64> = self.leaps.iter().map(|l| l.1).collect();
        let leapcnt = trans.len();
        for at in ats.iter_mut() {
            let mut j = leapcnt;
            while j > 0 {
                j -= 1;
                if *at > trans[j].wrapping_sub(corr[j]) {
                    *at = at.saturating_add(corr[j]);
                    break;
                }
            }
        }
        if self.want_bloat()
            && !ats.is_empty()
            && ats[ats.len() - 1] < Y2038_BOUNDARY - 1
            && string.contains('<')
        {
            let last = types[types.len() - 1];
            ats.push(Y2038_BOUNDARY - 1);
            types.push(last);
        }
        let rangeall = Range {
            defaulttype,
            base: 0,
            count: ats.len(),
            leapbase: 0,
            leapcount: leapcnt,
        };
        let hi64 = self
            .hi_time
            .max(self.redundant_time - i64::from(ZIC_MIN < self.redundant_time));
        let range64 = limitrange(rangeall, self.lo_time, hi64, &ats, &types, &trans);
        let range64_default = range64.defaulttype;
        let range32 = limitrange(
            Range {
                defaulttype: range64.defaulttype,
                base: range64.base,
                count: range64.count,
                leapbase: range64.leapbase,
                leapcount: range64.leapcount,
            },
            ZIC32_MIN,
            ZIC32_MAX,
            &ats,
            &types,
            &trans,
        );
        let mut out: Vec<u8> = Vec::new();
        for pass in 1..=2u32 {
            let r = if pass == 1 { &range32 } else { &range64 };
            let mut thisdefaulttype = r.defaulttype;
            let thistimei = r.base;
            let mut thistimecnt = r.count;
            let thisleapi = r.leapbase;
            let mut thisleapcnt = r.leapcount;
            let (thismin, thismax) = if pass == 1 { (ZIC32_MIN, ZIC32_MAX) } else { (ZIC_MIN, ZIC_MAX) };
            let toomanytimes = if pass == 1 { thistimecnt >> 31 != 0 } else { false };
            if toomanytimes {
                self.error("too many transition times");
            }
            let locut = thismin < self.lo_time && self.lo_time <= thismax;
            let mut hicut = thismin <= self.hi_time && self.hi_time < thismax;
            let thistimelim = thistimei + thistimecnt;
            let mut typecnt = self.tz.utoffs.len();
            let mut omittype = vec![true; typecnt];
            let mut pretranstype: i64 = -1;
            if (locut || (pass == 1 && thistimei != 0))
                && !(thistimecnt != 0 && ats[thistimei] == self.lo_time)
            {
                pretranstype = thisdefaulttype as i64;
                omittype[thisdefaulttype] = false;
            }
            if pass == 1 && self.lo_time <= thismin {
                thisdefaulttype = range64_default;
            }
            if locut {
                thisdefaulttype = unspecifiedtype as usize;
            }
            omittype[thisdefaulttype] = false;
            for i in thistimei..thistimelim {
                omittype[types[i]] = false;
            }
            if hicut {
                omittype[unspecifiedtype as usize] = false;
            }
            let old0 = omittype.iter().position(|&o| !o).unwrap_or(typecnt);
            let swap = |i: usize| -> usize {
                if i == old0 {
                    thisdefaulttype
                } else if i == thisdefaulttype {
                    old0
                } else {
                    i
                }
            };
            if self.want_bloat() {
                let (mut mrudst, mut mrustd, mut hidst, mut histd) = (-1i64, -1i64, -1i64, -1i64);
                if pretranstype >= 0 {
                    if self.tz.isdsts[pretranstype as usize] != 0 {
                        mrudst = pretranstype;
                    } else {
                        mrustd = pretranstype;
                    }
                }
                for i in thistimei..thistimelim {
                    if self.tz.isdsts[types[i]] != 0 {
                        mrudst = types[i] as i64;
                    } else {
                        mrustd = types[i] as i64;
                    }
                }
                for i in old0..typecnt {
                    let h = swap(i);
                    if !omittype[h] {
                        if self.tz.isdsts[h] != 0 {
                            hidst = i as i64;
                        } else {
                            histd = i as i64;
                        }
                    }
                }
                if hidst >= 0
                    && mrudst >= 0
                    && hidst != mrudst
                    && self.tz.utoffs[hidst as usize] != self.tz.utoffs[mrudst as usize]
                {
                    let m = mrudst as usize;
                    self.tz.isdsts[m] = -1;
                    let ab = String::from_utf8_lossy(cstr_at(&self.tz.chars, self.tz.desigidx[m])).into_owned();
                    let (uo, s, u) = (self.tz.utoffs[m], self.tz.ttisstds[m], self.tz.ttisuts[m]);
                    let ty = self.addtype(uo, &ab, true, s, u);
                    self.tz.isdsts[m] = 1;
                    omittype.resize(self.tz.utoffs.len(), true);
                    omittype[ty] = false;
                }
                if histd >= 0
                    && mrustd >= 0
                    && histd != mrustd
                    && self.tz.utoffs[histd as usize] != self.tz.utoffs[mrustd as usize]
                {
                    let m = mrustd as usize;
                    self.tz.isdsts[m] = -1;
                    let ab = String::from_utf8_lossy(cstr_at(&self.tz.chars, self.tz.desigidx[m])).into_owned();
                    let (uo, s, u) = (self.tz.utoffs[m], self.tz.ttisstds[m], self.tz.ttisuts[m]);
                    let ty = self.addtype(uo, &ab, false, s, u);
                    self.tz.isdsts[m] = 0;
                    omittype.resize(self.tz.utoffs.len(), true);
                    omittype[ty] = false;
                }
                typecnt = self.tz.utoffs.len();
            }
            let mut typemap = vec![0usize; typecnt];
            let mut thistypecnt = 0usize;
            for i in old0..typecnt {
                if !omittype[i] {
                    typemap[swap(i)] = thistypecnt;
                    thistypecnt += 1;
                }
            }
            let mut indmap: Vec<Option<usize>> = vec![None; self.tz.chars.len().max(1)];
            let mut thischars: Vec<u8> = Vec::new();
            let (mut stdcnt, mut utcnt) = (0usize, 0usize);
            for i in old0..typecnt {
                if omittype[i] {
                    continue;
                }
                if self.tz.ttisstds[i] {
                    stdcnt = thistypecnt;
                }
                if self.tz.ttisuts[i] {
                    utcnt = thistypecnt;
                }
                let di = self.tz.desigidx[i];
                if indmap[di].is_some() {
                    continue;
                }
                let abbr = cstr_at(&self.tz.chars, di);
                let mut found = None;
                for j in 0..thischars.len() {
                    if cstr_at(&thischars, j) == abbr {
                        found = Some(j);
                        break;
                    }
                }
                let j = match found {
                    Some(j) => j,
                    None => {
                        let j = thischars.len();
                        thischars.extend_from_slice(&abbr);
                        thischars.push(0);
                        j
                    }
                };
                indmap[di] = Some(j);
            }
            let slim_pass1 = pass == 1 && !self.want_bloat();
            if slim_pass1 {
                hicut = false;
                pretranstype = -1;
                thistimecnt = 0;
                thisleapcnt = 0;
                thistypecnt = 1;
                stdcnt = 0;
                utcnt = 0;
            }
            // Cabeçalho.
            out.extend_from_slice(b"TZif");
            out.push(version);
            out.extend_from_slice(&[0u8; 15]);
            let nchars = if slim_pass1 { 1 } else { thischars.len() };
            let timecnt_out = usize::from(pretranstype >= 0) + thistimecnt + usize::from(hicut);
            for v in [utcnt, stdcnt, thisleapcnt, timecnt_out, thistypecnt, nchars] {
                out.extend_from_slice(&(v as u32).to_be_bytes());
            }
            if slim_pass1 {
                // Bloco mínimo com um só tipo de hora.
                out.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0]);
                continue;
            }
            let lo = if pass == 1 && self.lo_time < ZIC32_MIN { ZIC32_MIN } else { self.lo_time };
            if pretranstype >= 0 {
                Z::put_pass(&mut out, lo, pass);
            }
            for i in thistimei..thistimelim {
                Z::put_pass(&mut out, ats[i], pass);
            }
            if hicut {
                Z::put_pass(&mut out, self.hi_time + 1, pass);
            }
            if pretranstype >= 0 {
                out.push(typemap[pretranstype as usize] as u8);
            }
            for i in thistimei..thistimelim {
                out.push(typemap[types[i]] as u8);
            }
            if hicut {
                out.push(typemap[unspecifiedtype as usize] as u8);
            }
            for i in old0..typecnt {
                let h = swap(i);
                if !omittype[h] {
                    Z::put32(&mut out, self.tz.utoffs[h]);
                    out.push(self.tz.isdsts[h] as u8);
                    out.push(indmap[self.tz.desigidx[h]].unwrap_or(0) as u8);
                }
            }
            out.extend_from_slice(&thischars);
            for i in thisleapi..thisleapi + thisleapcnt {
                Z::put_pass(&mut out, trans[i], pass);
                Z::put32(&mut out, corr[i]);
            }
            if stdcnt != 0 {
                for i in old0..typecnt {
                    let h = swap(i);
                    if !omittype[h] {
                        out.push(u8::from(self.tz.ttisstds[h]));
                    }
                }
            }
            if utcnt != 0 {
                for i in old0..typecnt {
                    let h = swap(i);
                    if !omittype[h] {
                        out.push(u8::from(self.tz.ttisuts[h]));
                    }
                }
            }
        }
        out.push(b'\n');
        out.extend_from_slice(string.as_bytes());
        out.push(b'\n');
        self.write_out(name, &out);
    }

    fn join(&self, name: &str) -> Vec<u8> {
        if name.starts_with('/') {
            name.as_bytes().to_vec()
        } else {
            format!("{}/{}", self.directory, name).into_bytes()
        }
    }

    /// `mkdirs`: cria os diretórios que faltam no caminho (sem o último componente).
    fn mkdirs(&mut self, path: &[u8]) -> Result<(), Errno> {
        for i in 1..path.len() {
            if path[i] != b'/' || path[i - 1] == b'/' {
                continue;
            }
            let prefix = &path[..i];
            match sys::current().mkdirat(Fd::CWD, prefix, 0o755) {
                Ok(()) | Err(Errno::EEXIST) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    fn write_out(&mut self, name: &str, data: &[u8]) {
        let outname = self.join(name);
        if let Err(e) = self.mkdirs(&outname) {
            io::eprint(format!(
                "{}: Can't create directory {}: {}\n",
                self.progname,
                io::lossy(&outname),
                e.message()
            ));
            sys::exit(1);
        }
        let pid = sys::current().getpid();
        let mut tmp = outname.clone();
        tmp.extend_from_slice(format!(".tmp.{pid}").as_bytes());
        let res = (|| -> Result<(), Errno> {
            let fd = sys::open(
                &tmp,
                OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC | OFlags::CLOEXEC,
                0o666,
            )?;
            let w = sys::write_all(fd, data);
            let c = sys::close(fd);
            w?;
            c?;
            sys::current().renameat2(Fd::CWD, &tmp, Fd::CWD, &outname, RenameFlags::empty())
        })();
        if let Err(e) = res {
            let _ = sys::current().unlinkat(Fd::CWD, &tmp, AtFlags::empty());
            io::eprint(format!(
                "{}: Can't create {}/{}: {}\n",
                self.progname,
                self.directory,
                name,
                e.message()
            ));
            sys::exit(1);
        }
    }

    /// Tenta criar o nome `linkname` apontando para `target`: hard link, e cópia se não der.
    fn try_link(&mut self, target: &str, linkname: &str) -> Result<(), Errno> {
        let remove_only = target == "-";
        let tp = self.join(target);
        let lp = self.join(linkname);
        if !remove_only
            && let Ok(st) = sys::stat(&tp)
            && st.file_type() == sysabi::FileType::Directory
        {
            io::eprint(format!(
                "{}: linking target {}/{} failed: {}\n",
                self.progname,
                self.directory,
                target,
                Errno::EPERM.message()
            ));
            sys::exit(1);
        }
        if remove_only {
            let _ = sys::current().unlinkat(Fd::CWD, &lp, AtFlags::empty());
            return Ok(());
        }
        // O alvo precisa existir antes de tirar o nome antigo do lugar.
        sys::stat(&tp)?;
        let _ = sys::current().unlinkat(Fd::CWD, &lp, AtFlags::empty());
        let mut r = sys::current().linkat(Fd::CWD, &tp, Fd::CWD, &lp, AtFlags::SYMLINK_FOLLOW);
        if r == Err(Errno::ENOENT) {
            self.mkdirs(&lp)?;
            r = sys::current().linkat(Fd::CWD, &tp, Fd::CWD, &lp, AtFlags::SYMLINK_FOLLOW);
        }
        if r.is_err() {
            // Cópia de reserva (outro sistema de arquivos, por exemplo).
            let data = io::read_path(&tp)?;
            let fd = sys::open(
                &lp,
                OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC | OFlags::CLOEXEC,
                0o666,
            )?;
            let w = sys::write_all(fd, &data);
            let _ = sys::close(fd);
            return w;
        }
        Ok(())
    }

    fn report_link_failure(&mut self, target: &str, linkname: &str, e: Errno) -> ! {
        io::eprint(format!(
            "{}: Can't link {}/{} to {}/{}: {}\n",
            self.progname,
            self.directory,
            target,
            self.directory,
            linkname,
            e.message()
        ));
        sys::exit(1)
    }

    fn make_links(&mut self) {
        let mut links = std::mem::take(&mut self.links);
        links.sort_by(|a, b| {
            a.name
                .cmp(&b.name)
                .then(a.file.cmp(&b.file))
                .then(a.line.cmp(&b.line))
        });
        // O último `Link` de cada nome vence.
        let mut kept: Vec<Link> = Vec::new();
        for l in links {
            if kept.last().is_some_and(|p| p.name == l.name) {
                kept.pop();
            }
            kept.push(l);
        }
        let mut pending = kept;
        while !pending.is_empty() {
            let mut next = Vec::new();
            let mut progress = false;
            for l in pending {
                self.eat(&l.file.clone(), l.line);
                match self.try_link(&l.target, &l.name) {
                    Ok(()) => progress = true,
                    Err(Errno::ENOENT) => next.push(l),
                    Err(e) => self.report_link_failure(&l.target, &l.name, e),
                }
            }
            if !progress && !next.is_empty() {
                let l = &next[0];
                let (t, n) = (l.target.clone(), l.name.clone());
                self.report_link_failure(&t, &n, Errno::ENOENT);
            }
            pending = next;
        }
    }
}

/// `-r '[@lo][/@hi]'`.
fn parse_timerange(s: &str) -> Option<(Option<i64>, Option<i64>)> {
    let mut lo = None;
    let mut rest = s;
    if let Some(r) = rest.strip_prefix('@') {
        let end = r.find('/').unwrap_or(r.len());
        lo = Some(parse_whole(&r[..end])?);
        rest = &r[end..];
    }
    let mut hi = None;
    if let Some(r) = rest.strip_prefix('/') {
        hi = Some(parse_whole(r.strip_prefix('@')?)?);
        rest = "";
    }
    if !rest.is_empty() {
        return None;
    }
    Some((lo, hi))
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
    let mut z = Z {
        progname: progname.clone(),
        errors: 0,
        filename: None,
        linenum: 0,
        rloc: None,
        bloat: 0,
        lo_time: ZIC_MIN,
        hi_time: ZIC_MAX,
        redundant_time: ZIC_MIN,
        noise: false,
        directory: String::new(),
        zones: Vec::new(),
        rules: Vec::new(),
        links: Vec::new(),
        leaps: Vec::new(),
        leapseen: false,
        leapminyear: 0,
        leapmaxyear: 0,
        tz: Tz::default(),
    };
    let mut directory: Option<String> = None;
    let mut lcltime: Option<String> = None;
    let mut psxrules: Option<String> = None;
    let mut tzdefault: Option<String> = None;
    let mut leapsec: Option<String> = None;
    let mut timerange_given = false;
    let mut g = Getopt::from_env(&argv[1..], "b:d:l:L:p:Pr:R:st:vy:", &[]);
    while let Some(opt) = g.next_opt() {
        match opt {
            Ok(o) => {
                let arg = o.arg_str();
                match o.short() {
                    Some('b') => match arg.as_str() {
                        "slim" => {
                            if z.bloat > 0 {
                                z.error("incompatible -b options");
                            }
                            z.bloat = -1;
                        }
                        "fat" => {
                            if z.bloat < 0 {
                                z.error("incompatible -b options");
                            }
                            z.bloat = 1;
                        }
                        _ => {
                            io::eprint(format!("invalid option: -b '{arg}'\n"));
                            return 1;
                        }
                    },
                    Some('d') => {
                        if directory.is_some() {
                            io::eprint(format!("{progname}: More than one -d option specified\n"));
                            return 1;
                        }
                        directory = Some(arg);
                    }
                    Some('l') => {
                        if lcltime.is_some() {
                            io::eprint(format!("{progname}: More than one -l option specified\n"));
                            return 1;
                        }
                        lcltime = Some(arg);
                    }
                    Some('p') => {
                        if psxrules.is_some() {
                            io::eprint(format!("{progname}: More than one -p option specified\n"));
                            return 1;
                        }
                        psxrules = Some(arg);
                    }
                    Some('t') => {
                        if tzdefault.is_some() {
                            io::eprint(format!("{progname}: More than one -t option specified\n"));
                            return 1;
                        }
                        tzdefault = Some(arg);
                    }
                    Some('L') => {
                        if leapsec.is_some() {
                            io::eprint(format!("{progname}: More than one -L option specified\n"));
                            return 1;
                        }
                        leapsec = Some(arg);
                    }
                    Some('r') => {
                        if timerange_given {
                            io::eprint(format!("{progname}: More than one -r option specified\n"));
                            return 1;
                        }
                        match parse_timerange(&arg) {
                            Some((lo, hi)) => {
                                if let Some(lo) = lo {
                                    z.lo_time = z.lo_time.max(lo);
                                }
                                if let Some(hi) = hi {
                                    // O limite superior de `-r` é exclusivo.
                                    z.hi_time = z.hi_time.min(hi.saturating_sub(1));
                                }
                                timerange_given = true;
                            }
                            None => {
                                io::eprint(format!("{progname}: invalid time range: {arg}\n"));
                                return 1;
                            }
                        }
                    }
                    Some('R') => match arg.strip_prefix('@').and_then(parse_whole) {
                        Some(v) => z.redundant_time = v,
                        None => {
                            io::eprint(format!("{progname}: invalid time: {arg}\n"));
                            return 1;
                        }
                    },
                    Some('s') => z.warning("-s ignored"),
                    Some('y') => z.warning("-y ignored"),
                    Some('v') => z.noise = true,
                    _ => {}
                }
            }
            Err(e) => {
                io::eprint(format!("{}\n{}", e.message(&progname), usage_text(&progname)));
                return 1;
            }
        }
    }
    let files = g.operands();
    if files.len() == 1 && files[0] == b"=" {
        io::eprint(usage_text(&progname));
        return 1;
    }
    if z.bloat == 0 {
        z.bloat = -1;
    }
    z.directory = directory.unwrap_or_else(|| TZDIR.to_string());
    let tzdefault = tzdefault.unwrap_or_else(|| TZDEFAULT.to_string());
    if let Some(l) = &leapsec {
        z.infile(l);
        z.adjleap();
    }
    for f in &files {
        z.infile(&io::lossy(f));
    }
    z.filename = None;
    if z.errors > 0 {
        return 1;
    }
    z.associate();
    let mut zones = std::mem::take(&mut z.zones);
    let mut i = 0;
    while i < zones.len() {
        let mut j = i + 1;
        while j < zones.len() && zones[j].name.is_none() {
            j += 1;
        }
        z.outzone(&mut zones[i..j]);
        i = j;
    }
    z.filename = None;
    z.make_links();
    for (target, name) in [(lcltime, tzdefault), (psxrules, TZDEFRULES.to_string())]
        .into_iter()
        .filter_map(|(t, n)| t.map(|t| (t, n)))
    {
        match z.try_link(&target, &name) {
            Ok(()) => {}
            Err(e) => z.report_link_failure(&target, &name, e),
        }
    }
    i32::from(z.errors > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posix_offsets() {
        assert_eq!(stringoffset(3 * 3600).as_deref(), Some("3"));
        assert_eq!(stringoffset(-(5 * 3600 + 30 * 60)).as_deref(), Some("-5:30"));
        assert_eq!(stringoffset(3661).as_deref(), Some("1:01:01"));
        assert_eq!(abbroffset(-3 * 3600).as_deref(), Some("-03"));
        assert_eq!(abbroffset(5 * 3600 + 30 * 60).as_deref(), Some("+0530"));
    }

    #[test]
    fn words_and_fields() {
        assert_eq!(byword("mar", &MONTHS), Some(2));
        assert_eq!(byword("ma", &MONTHS), None);
        assert_eq!(byword("Sun", &WDAYS), Some(0));
        let f = getfields(b"Zone  \"A B\" -3:00 # c");
        assert_eq!(f, vec!["Zone", "A B", "-3:00"]);
    }
}
