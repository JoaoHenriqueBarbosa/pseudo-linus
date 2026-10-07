//! Datas: leitura estrita (variáveis `GIT_*_DATE`, `--date` do commit), leitura aproximada
//! (`--since`, `@{yesterday}`) e os formatos de exibição do `--date`.
//!
//! A leitura segue os formatos documentados em git-commit(1) ("DATE FORMATS": formato interno,
//! RFC 2822, ISO 8601 e as variantes `AAAA.MM.DD`, `MM/DD/AAAA`, `DD.MM.AAAA`), com o que o
//! oráculo aceita e rejeita como referência (hora obrigatória, anos de 1970 a 2099). O fuso local
//! vem do `TZ` do sandbox (string POSIX ou zona de `/usr/share/zoneinfo` no FS do sandbox).

use ul_common::time::strftime::{StrfTime, strftime};
use ul_common::time::{Civil, days_from_civil};

use crate::os;

pub const WEEKDAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
pub const MONTHS: [&str; 12] =
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

/// Data quebrada em campos (já deslocada pelo fuso).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tm {
    pub year: i64,
    /// 0..11
    pub mon: u32,
    pub mday: u32,
    pub hour: u32,
    pub min: u32,
    pub sec: u32,
    /// 0 = domingo
    pub wday: u32,
    /// 0..365
    pub yday: u32,
}

/// Campos de `t + offset_secs` lidos como UTC.
pub fn tm_of(t: i64, offset_secs: i64) -> Tm {
    let c = Civil::from_secs(t + offset_secs);
    Tm {
        year: c.year,
        mon: (c.mon - 1) as u32,
        mday: c.mday as u32,
        hour: c.hour as u32,
        min: c.min as u32,
        sec: c.sec as u32,
        wday: c.wday as u32,
        yday: c.yday as u32,
    }
}

/// Segundos desde a época de uma data lida como UTC (dia fora do mês transborda pro seguinte).
pub fn timegm(year: i64, mon0: u32, mday: u32, hour: u32, min: u32, sec: u32) -> i64 {
    days_from_civil(year, i64::from(mon0) + 1, i64::from(mday)) * 86_400
        + i64::from(hour) * 3600
        + i64::from(min) * 60
        + i64::from(sec)
}

/// Fuso `+hhmm` (inteiro decimal) a partir de segundos.
pub fn tz_from_secs(off: i64) -> i32 {
    let sign = if off < 0 { -1 } else { 1 };
    let a = off.abs() / 60;
    (sign * ((a / 60) * 100 + a % 60)) as i32
}

// ---- fuso local -------------------------------------------------------------------------------

/// Regras de fuso: deslocamento fixo ou tabela de transições (TZif).
#[derive(Clone, Debug)]
pub enum Zone {
    Fixed(i64),
    Table { transitions: Vec<(i64, usize)>, types: Vec<i64>, default: usize },
}

impl Zone {
    pub fn offset_at(&self, t: i64) -> i64 {
        match self {
            Zone::Fixed(o) => *o,
            Zone::Table { transitions, types, default } => {
                let idx = match transitions.binary_search_by(|(at, _)| at.cmp(&t)) {
                    Ok(i) => transitions[i].1,
                    Err(0) => *default,
                    Err(i) => transitions[i - 1].1,
                };
                types.get(idx).copied().unwrap_or(0)
            }
        }
    }

    /// Deslocamento pra uma hora local (a que vale naquele instante local).
    pub fn offset_for_local(&self, local: i64) -> i64 {
        let guess = self.offset_at(local);
        self.offset_at(local - guess)
    }
}

/// `std offset` POSIX (só a parte padrão; horário de verão sem regra fica no padrão).
fn parse_posix_tz(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    let mut i = 0;
    if b.first() == Some(&b'<') {
        i = s.find('>')? + 1;
    } else {
        while i < b.len() && b[i].is_ascii_alphabetic() {
            i += 1;
        }
        if i < 3 {
            return None;
        }
    }
    if i >= b.len() {
        return Some(0);
    }
    let mut sign = 1;
    if b[i] == b'+' {
        i += 1;
    } else if b[i] == b'-' {
        sign = -1;
        i += 1;
    }
    let rest = &s[i..];
    let end = rest.find(|c: char| !(c.is_ascii_digit() || c == ':')).unwrap_or(rest.len());
    let mut parts = rest[..end].split(':');
    let h: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next().map(|x| x.parse().unwrap_or(0)).unwrap_or(0);
    let sec: i64 = parts.next().map(|x| x.parse().unwrap_or(0)).unwrap_or(0);
    // POSIX: o número é o que se soma ao horário local pra chegar ao UTC.
    Some(-sign * (h * 3600 + m * 60 + sec))
}

fn be32(d: &[u8], at: usize) -> Option<i64> {
    Some(i32::from_be_bytes(d.get(at..at + 4)?.try_into().ok()?) as i64)
}

fn be64(d: &[u8], at: usize) -> Option<i64> {
    Some(i64::from_be_bytes(d.get(at..at + 8)?.try_into().ok()?))
}

/// Lê um arquivo TZif (formato do tzfile(5): bloco v1 ou o de 64 bits do v2+).
pub fn parse_tzif(d: &[u8]) -> Option<Zone> {
    if !d.starts_with(b"TZif") {
        return None;
    }
    let counts = |at: usize| -> Option<[usize; 6]> {
        let mut c = [0usize; 6];
        for (k, v) in c.iter_mut().enumerate() {
            *v = be32(d, at + 20 + k * 4)? as usize;
        }
        Some(c)
    };
    let c1 = counts(0)?;
    let [isutc, isstd, leap, timecnt, typecnt, charcnt] = c1;
    let v1_len = 44 + timecnt * 5 + typecnt * 6 + charcnt + leap * 8 + isstd + isutc;
    let (base, tsize, c) = if d[4] >= b'2' && d.len() > v1_len + 44 { (v1_len, 8, counts(v1_len)?) } else { (0, 4, c1) };
    let [_, _, _, timecnt, typecnt, _] = c;
    let mut at = base + 44;
    let mut times = Vec::with_capacity(timecnt);
    for k in 0..timecnt {
        times.push(if tsize == 8 { be64(d, at + k * 8)? } else { be32(d, at + k * 4)? });
    }
    at += timecnt * tsize;
    let mut idx = Vec::with_capacity(timecnt);
    for k in 0..timecnt {
        idx.push(*d.get(at + k)? as usize);
    }
    at += timecnt;
    let mut types = Vec::with_capacity(typecnt);
    let mut first_std = None;
    for k in 0..typecnt {
        let off = be32(d, at + k * 6)?;
        let isdst = *d.get(at + k * 6 + 4)?;
        if isdst == 0 && first_std.is_none() {
            first_std = Some(k);
        }
        types.push(off);
    }
    Some(Zone::Table { transitions: times.into_iter().zip(idx).collect(), types, default: first_std.unwrap_or(0) })
}

/// Fuso local do processo.
pub fn local_zone() -> Zone {
    let spec = match os::getenv_str("TZ") {
        Some(t) => t,
        None => {
            let Some(s) = sysabi::sys::try_current() else { return Zone::Fixed(0) };
            let data = s.local_timezone();
            if let Some(z) = parse_tzif(&data) {
                return z;
            }
            String::from_utf8_lossy(&data).into_owned()
        }
    };
    let spec = spec.strip_prefix(':').unwrap_or(&spec).trim().to_string();
    if spec.is_empty() || spec == "UTC" || spec == "GMT" || spec == "UTC0" || spec == "GMT0" {
        return Zone::Fixed(0);
    }
    if let Some(o) = parse_posix_tz(&spec)
        && (spec.starts_with('<') || spec.chars().any(|c| c.is_ascii_digit()))
    {
        return Zone::Fixed(o);
    }
    let path = if spec.starts_with('/') { spec.clone() } else { format!("/usr/share/zoneinfo/{spec}") };
    if let Ok(Some(data)) = os::read_opt(path.as_bytes())
        && let Some(z) = parse_tzif(&data)
    {
        return z;
    }
    Zone::Fixed(0)
}

/// Deslocamento local (segundos) no instante `t`.
pub fn local_offset(t: i64) -> i64 {
    local_zone().offset_at(t)
}

/// Agora com o fuso local: `(segundos, fuso +hhmm)`.
pub fn now_with_tz() -> (i64, i32) {
    let t = os::now();
    (t, tz_from_secs(local_offset(t)))
}

// ---- leitura estrita --------------------------------------------------------------------------

/// Abreviações de fuso aceitas (as de uso comum; nomes desconhecidos são ignorados).
const ZONE_NAMES: &[(&str, i64)] = &[
    ("UTC", 0),
    ("UT", 0),
    ("GMT", 0),
    ("Z", 0),
    ("WET", 0),
    ("BST", 60),
    ("CET", 60),
    ("CEST", 120),
    ("MET", 60),
    ("MEST", 120),
    ("EET", 120),
    ("EEST", 180),
    ("EST", -300),
    ("EDT", -240),
    ("CST", -360),
    ("CDT", -300),
    ("MST", -420),
    ("MDT", -360),
    ("PST", -480),
    ("PDT", -420),
    ("HST", -600),
    ("AKST", -540),
    ("JST", 540),
];

#[derive(Default, Debug)]
struct Fields {
    year: Option<i64>,
    mon: Option<i64>,
    mday: Option<i64>,
    hour: Option<i64>,
    min: Option<i64>,
    sec: Option<i64>,
    /// Deslocamento em minutos.
    tz: Option<i64>,
    /// Segundos desde a época (número grande solto).
    epoch: Option<i64>,
    pm: Option<bool>,
}

fn normalize_year(y: i64, digits: usize) -> Option<i64> {
    if digits >= 4 {
        return Some(y);
    }
    if (70..100).contains(&y) {
        Some(1900 + y)
    } else if y < 38 {
        Some(2000 + y)
    } else {
        None
    }
}

impl Fields {
    /// Tenta (ano, mês, dia) e grava se fizer sentido.
    fn try_ymd(&mut self, y: i64, ydigits: usize, m: i64, d: i64) -> bool {
        if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
            return false;
        }
        let Some(y) = normalize_year(y, ydigits) else { return false };
        self.year = Some(y);
        self.mon = Some(m - 1);
        self.mday = Some(d);
        true
    }

    /// Data com separador (`a-b-c`, `a/b/c`, `a.b.c`).
    fn date_triple(&mut self, a: (i64, usize), b: i64, c: Option<(i64, usize)>, sep: u8) -> bool {
        let Some(c) = c else {
            // Só dois números: mês e dia do ano corrente não são aceitos pelo modo estrito.
            return false;
        };
        if a.1 == 4 || a.0 > 70 {
            // Ano primeiro: ano-mês-dia, ou ano-dia-mês se o mês não couber.
            return self.try_ymd(a.0, a.1, b, c.0) || self.try_ymd(a.0, a.1, c.0, b);
        }
        if sep != b'.' && self.try_ymd(c.0, c.1, a.0, b) {
            return true;
        }
        if self.try_ymd(c.0, c.1, b, a.0) {
            return true;
        }
        sep == b'.' && self.try_ymd(c.0, c.1, a.0, b)
    }
}

fn read_num(s: &[u8], i: usize) -> (i64, usize) {
    let mut j = i;
    let mut n: i64 = 0;
    while j < s.len() && s[j].is_ascii_digit() {
        n = n.saturating_mul(10).saturating_add((s[j] - b'0') as i64);
        j += 1;
    }
    (n, j - i)
}

fn month_from_word(w: &str) -> Option<usize> {
    if w.len() < 3 {
        return None;
    }
    MONTHS.iter().position(|m| m.len() >= w.len() && m[..w.len()].eq_ignore_ascii_case(w))
}

fn weekday_from_word(w: &str) -> Option<usize> {
    if w.len() < 3 {
        return None;
    }
    WEEKDAYS.iter().position(|d| d.len() >= w.len() && d[..w.len()].eq_ignore_ascii_case(w))
}

/// Leitura estrita: `(segundos, deslocamento em minutos)`.
pub fn parse_date_basic(input: &[u8]) -> Option<(i64, i64)> {
    let s = match input.iter().position(|c| *c == b'\n') {
        Some(n) => &input[..n],
        None => input,
    };
    // Formato interno: `<segundos> <+hhmm>`, com `@` opcional.
    let raw = s.strip_prefix(b"@").unwrap_or(s);
    if let Some(sp) = raw.iter().position(|c| *c == b' ') {
        let (num, rest) = (&raw[..sp], &raw[sp + 1..]);
        if !num.is_empty()
            && num.iter().all(u8::is_ascii_digit)
            && rest.len() == 5
            && (rest[0] == b'+' || rest[0] == b'-')
            && rest[1..].iter().all(u8::is_ascii_digit)
        {
            let secs = std::str::from_utf8(num).ok()?.parse::<i64>().ok()?;
            let hhmm: i64 = std::str::from_utf8(&rest[1..]).ok()?.parse().ok()?;
            let mut off = (hhmm / 100) * 60 + hhmm % 100;
            if rest[0] == b'-' {
                off = -off;
            }
            return Some((secs, off));
        }
    }
    let mut f = Fields::default();
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c.is_ascii_alphabetic() {
            let start = i;
            while i < s.len() && s[i].is_ascii_alphabetic() {
                i += 1;
            }
            let w = std::str::from_utf8(&s[start..i]).unwrap_or("");
            if (w == "T" || w == "t") && s.get(i).is_some_and(u8::is_ascii_digit) {
                continue;
            }
            if let Some(m) = month_from_word(w) {
                f.mon = Some(m as i64);
            } else if weekday_from_word(w).is_some() {
            } else if w.eq_ignore_ascii_case("pm") {
                f.pm = Some(true);
            } else if w.eq_ignore_ascii_case("am") {
                f.pm = Some(false);
            } else if let Some((_, off)) = ZONE_NAMES.iter().find(|(n, _)| n.eq_ignore_ascii_case(w))
                && f.tz.is_none() {
                    f.tz = Some(*off);
                }
            continue;
        }
        if (c == b'+' || c == b'-') && s.get(i + 1).is_some_and(u8::is_ascii_digit) {
            let (n, len) = read_num(s, i + 1);
            let mut j = i + 1 + len;
            let sign = if c == b'-' { -1 } else { 1 };
            let parsed = match len {
                4 => Some((n / 100, n % 100)),
                2 if s.get(j) == Some(&b':') && s.get(j + 1).is_some_and(u8::is_ascii_digit) => {
                    let (m, ml) = read_num(s, j + 1);
                    j += 1 + ml;
                    (ml == 2).then_some((n, m))
                }
                2 => Some((n, 0)),
                _ => None,
            };
            if let Some((h, m)) = parsed
                && h < 24
                && m < 60
            {
                f.tz = Some(sign * (h * 60 + m));
            }
            i = j;
            continue;
        }
        if c.is_ascii_digit() {
            let (n, len) = read_num(s, i);
            let mut j = i + len;
            let next = s.get(j).copied();
            let next_digit = s.get(j + 1).is_some_and(u8::is_ascii_digit);
            if next == Some(b':') && next_digit {
                let (m, ml) = read_num(s, j + 1);
                j += 1 + ml;
                let mut sec = 0;
                if s.get(j) == Some(&b':') && s.get(j + 1).is_some_and(u8::is_ascii_digit) {
                    let (sv, sl) = read_num(s, j + 1);
                    sec = sv;
                    j += 1 + sl;
                }
                // Fração de segundo é ignorada.
                if s.get(j) == Some(&b'.') && s.get(j + 1).is_some_and(u8::is_ascii_digit) {
                    let (_, fl) = read_num(s, j + 1);
                    j += 1 + fl;
                }
                if n < 25 && m < 60 && sec <= 60 {
                    f.hour = Some(n);
                    f.min = Some(m);
                    f.sec = Some(sec);
                }
                i = j;
                continue;
            }
            if let Some(sep) = next
                && matches!(sep, b'-' | b'/' | b'.')
                && next_digit
                && f.year.is_none()
            {
                let (b, bl) = read_num(s, j + 1);
                j += 1 + bl;
                let mut cpart = None;
                if s.get(j) == Some(&sep) && s.get(j + 1).is_some_and(u8::is_ascii_digit) {
                    let (cv, cl) = read_num(s, j + 1);
                    cpart = Some((cv, cl));
                    j += 1 + cl;
                }
                f.date_triple((n, len), b, cpart, sep);
                i = j;
                continue;
            }
            if len >= 9 && f.year.is_none() && f.hour.is_none() && f.epoch.is_none() {
                f.epoch = Some(n);
            } else if len == 8 {
                f.try_ymd(n / 10000, 4, (n / 100) % 100, n % 100);
            } else if len == 6 {
                let (h, m, sec) = (n / 10000, (n / 100) % 100, n % 100);
                if h < 25 && m < 60 && sec <= 60 {
                    f.hour = Some(h);
                    f.min = Some(m);
                    f.sec = Some(sec);
                }
                if s.get(j) == Some(&b'.') && s.get(j + 1).is_some_and(u8::is_ascii_digit) {
                    let (_, fl) = read_num(s, j + 1);
                    j += 1 + fl;
                }
            } else if len == 4 {
                if n > 1900 && n < 2100 {
                    f.year = Some(n);
                }
            } else if len <= 2 {
                if f.mday.is_none() && (1..=31).contains(&n) {
                    f.mday = Some(n);
                } else if f.year.is_none() && len == 2 && f.mday.is_some() {
                    f.year = normalize_year(n, 2);
                } else if f.mon.is_none() && (1..=12).contains(&n) {
                    f.mon = Some(n - 1);
                }
            }
            i = j;
            continue;
        }
        i += 1;
    }
    if let Some(e) = f.epoch {
        let off = f.tz.unwrap_or(0);
        return Some((e, off));
    }
    let (year, mon, mday, mut hour, min) = (f.year?, f.mon?, f.mday?, f.hour?, f.min?);
    let sec = f.sec.unwrap_or(0);
    if !(1970..2100).contains(&year) {
        return None;
    }
    match f.pm {
        Some(true) => hour = hour % 12 + 12,
        Some(false) => hour %= 12,
        None => {}
    }
    let local = timegm(year, mon as u32, mday as u32, hour as u32, min as u32, sec as u32);
    let off = match f.tz {
        Some(o) => o,
        None => local_zone().offset_for_local(local) / 60,
    };
    Some((local - off * 60, off))
}

/// `(segundos, fuso +hhmm)`.
pub fn parse_date(input: &[u8]) -> Option<(i64, i32)> {
    let (t, off) = parse_date_basic(input)?;
    let sign = if off < 0 { -1 } else { 1 };
    let a = off.abs();
    Some((t, (sign * ((a / 60) * 100 + a % 60)) as i32))
}

// ---- leitura aproximada -----------------------------------------------------------------------

/// Datas aproximadas (o que o `--since`/`--until` e o `@{...}` aceitam): as datas do modo estrito,
/// datas sem hora, `N unidades ago`, `yesterday`, `now`, `today`, `noon`, `midnight`, nomes de
/// dia da semana.
pub fn approxidate(input: &str) -> Option<i64> {
    approxidate_at(input, os::now())
}

pub fn approxidate_at(input: &str, now: i64) -> Option<i64> {
    let s = input.trim();
    if let Some((t, _)) = parse_date_basic(s.as_bytes()) {
        return Some(t);
    }
    let lower = s.to_ascii_lowercase().replace(['.', '_'], " ");
    let words: Vec<&str> = lower.split_whitespace().collect();
    if words.is_empty() {
        return None;
    }
    let zone = local_zone();
    let mut t = now;
    let mut i = 0;
    let mut matched = false;
    let mut pending: Option<i64> = None;
    let mut date_set: Option<(i64, u32, u32)> = None;
    let mut time_set: Option<(u32, u32, u32)> = None;
    while i < words.len() {
        let w = words[i];
        if let Ok(n) = w.parse::<i64>() {
            if words.get(i + 1).is_some_and(|nx| unit_secs(nx).is_some()) {
                pending = Some(n);
            }
            i += 1;
            continue;
        }
        if let Some(d) = iso_date(w) {
            date_set = Some(d);
            matched = true;
        } else if let Some(hm) = hms(w) {
            time_set = Some(hm);
            matched = true;
        } else if let Some(u) = unit_secs(w) {
            let n = pending.take().unwrap_or(1);
            match u {
                Unit::Secs(k) => t -= n * k,
                Unit::Month => t = add_months(t, -n),
                Unit::Year => t = add_months(t, -12 * n),
            }
            matched = true;
        } else {
            match w {
                "ago" | "and" | "at" | "on" | "last" => {}
                "now" | "today" => matched = true,
                "yesterday" => {
                    t -= 86_400;
                    matched = true;
                }
                "noon" => {
                    time_set = Some((12, 0, 0));
                    matched = true;
                }
                "midnight" => {
                    time_set = Some((0, 0, 0));
                    matched = true;
                }
                _ => {
                    let wd = weekday_from_word(w)?;
                    let cur = tm_of(t, zone.offset_at(t));
                    let mut back = (cur.wday as i64 - wd as i64).rem_euclid(7);
                    if back == 0 {
                        back = 7;
                    }
                    t -= back * 86_400;
                    matched = true;
                }
            }
        }
        i += 1;
    }
    if !matched {
        return None;
    }
    let off = zone.offset_at(t);
    let cur = tm_of(t, off);
    let (y, mo, d) = date_set.unwrap_or((cur.year, cur.mon, cur.mday));
    let (h, mi, s) = time_set.unwrap_or((cur.hour, cur.min, cur.sec));
    Some(timegm(y, mo, d, h, mi, s) - off)
}

enum Unit {
    Secs(i64),
    Month,
    Year,
}

fn unit_secs(w: &str) -> Option<Unit> {
    let w = w.strip_suffix('s').unwrap_or(w);
    Some(match w {
        "second" | "sec" => Unit::Secs(1),
        "minute" | "min" => Unit::Secs(60),
        "hour" => Unit::Secs(3600),
        "day" => Unit::Secs(86_400),
        "week" => Unit::Secs(7 * 86_400),
        "fortnight" => Unit::Secs(14 * 86_400),
        "month" => Unit::Month,
        "year" => Unit::Year,
        _ => return None,
    })
}

fn add_months(t: i64, n: i64) -> i64 {
    let tm = tm_of(t, 0);
    let total = tm.year * 12 + tm.mon as i64 + n;
    timegm(total.div_euclid(12), total.rem_euclid(12) as u32, tm.mday, tm.hour, tm.min, tm.sec)
}

fn iso_date(w: &str) -> Option<(i64, u32, u32)> {
    let p: Vec<&str> = w.split('-').collect();
    if p.len() != 3 {
        return None;
    }
    let y: i64 = p[0].parse().ok()?;
    let m: u32 = p[1].parse().ok()?;
    let d: u32 = p[2].parse().ok()?;
    ((1..=12).contains(&m) && (1..=31).contains(&d) && y > 1900).then_some((y, m - 1, d))
}

fn hms(w: &str) -> Option<(u32, u32, u32)> {
    let p: Vec<&str> = w.split(':').collect();
    if p.len() < 2 || p.len() > 3 {
        return None;
    }
    let h: u32 = p[0].parse().ok()?;
    let m: u32 = p[1].parse().ok()?;
    let s: u32 = if p.len() == 3 { p[2].parse().ok()? } else { 0 };
    (h < 24 && m < 60 && s <= 60).then_some((h, m, s))
}

// ---- exibição ---------------------------------------------------------------------------------

/// Modos do `--date` (git-log(1)).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DateMode {
    Normal,
    Local,
    Iso,
    IsoStrict,
    Rfc,
    Short,
    Raw,
    Unix,
    Relative,
    Human,
    /// `format:` (o bool diz se é `format-local:`).
    Strftime(String, bool),
}

impl DateMode {
    pub fn parse(s: &str) -> Option<DateMode> {
        if let Some(f) = s.strip_prefix("format:") {
            return Some(DateMode::Strftime(f.to_string(), false));
        }
        if let Some(f) = s.strip_prefix("format-local:") {
            return Some(DateMode::Strftime(f.to_string(), true));
        }
        let (base, local) = match s.strip_suffix("-local") {
            Some(b) => (b, true),
            None => (s, false),
        };
        let m = match base {
            "default" | "normal" => {
                if local {
                    DateMode::Local
                } else {
                    DateMode::Normal
                }
            }
            "local" => DateMode::Local,
            "iso" | "iso8601" => DateMode::Iso,
            "iso-strict" | "iso8601-strict" => DateMode::IsoStrict,
            "rfc" | "rfc2822" => DateMode::Rfc,
            "short" => DateMode::Short,
            "raw" => DateMode::Raw,
            "unix" => DateMode::Unix,
            "relative" => DateMode::Relative,
            "human" => DateMode::Human,
            _ => return None,
        };
        Some(m)
    }
}

fn tz_str(tz: i32) -> String {
    format!("{tz:+05}")
}

/// Uma data no modo pedido.
pub fn show_date(t: i64, tz: i32, mode: &DateMode) -> String {
    match mode {
        DateMode::Raw => return format!("{t} {}", tz_str(tz)),
        DateMode::Unix => return t.to_string(),
        DateMode::Relative => return relative(t, os::now()),
        _ => {}
    }
    let local = matches!(mode, DateMode::Local | DateMode::Strftime(_, true));
    let tz = if local { tz_from_secs(local_offset(t)) } else { tz };
    let tm = tm_of(t, crate::object::tz_offset_secs(tz));
    let wd = &WEEKDAYS[tm.wday as usize][..3];
    let mo = &MONTHS[tm.mon as usize][..3];
    match mode {
        DateMode::Iso => format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02} {}", tm.year, tm.mon + 1, tm.mday, tm.hour, tm.min, tm.sec, tz_str(tz)),
        DateMode::IsoStrict => {
            let mut s = format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", tm.year, tm.mon + 1, tm.mday, tm.hour, tm.min, tm.sec);
            if tz == 0 {
                s.push('Z');
            } else {
                s.push(if tz >= 0 { '+' } else { '-' });
                s.push_str(&format!("{:02}:{:02}", tz.abs() / 100, tz.abs() % 100));
            }
            s
        }
        DateMode::Rfc => format!("{wd}, {} {mo} {} {:02}:{:02}:{:02} {}", tm.mday, tm.year, tm.hour, tm.min, tm.sec, tz_str(tz)),
        DateMode::Short => format!("{:04}-{:02}-{:02}", tm.year, tm.mon + 1, tm.mday),
        DateMode::Strftime(f, _) => {
            let civil = Civil {
                year: tm.year,
                mon: i64::from(tm.mon) + 1,
                mday: i64::from(tm.mday),
                hour: i64::from(tm.hour),
                min: i64::from(tm.min),
                sec: i64::from(tm.sec),
                wday: i64::from(tm.wday),
                yday: i64::from(tm.yday),
            };
            // O git não tem o nome do fuso de uma data com deslocamento explícito: `%Z` sai vazio.
            let when = StrfTime { civil, gmtoff: crate::object::tz_offset_secs(tz), zone: b"" };
            String::from_utf8_lossy(&strftime(f.as_bytes(), &when, t)).into_owned()
        }
        DateMode::Human => human(t, tz),
        _ => {
            let mut s = format!("{wd} {mo} {} {:02}:{:02}:{:02} {}", tm.mday, tm.hour, tm.min, tm.sec, tm.year);
            if !local {
                s.push(' ');
                s.push_str(&tz_str(tz));
            }
            s
        }
    }
}

fn ago(n: i64, unit: &str) -> String {
    if n == 1 { format!("{n} {unit} ago") } else { format!("{n} {unit}s ago") }
}

/// `--date=relative`. Faixas medidas no oráculo (`git log --format=%ar` sob `faketime`, com
/// bisseção nas mudanças de texto): cada unidade arredonda a anterior pela metade
/// (minutos = `(s+30)/60`, horas = `(min+30)/60`, dias = `(h+12)/24`) e passa pra seguinte em 90
/// segundos, 90 minutos, 36 horas, 14 dias (semanas = `(d+3)/7`), 70 dias (meses = `(d+15)/30`),
/// 365 dias (anos e meses) e 5 anos (só anos, `(d+183)/365`).
pub fn relative(t: i64, now: i64) -> String {
    if now < t {
        return "in the future".into();
    }
    let secs = now - t;
    if secs < 90 {
        return ago(secs, "second");
    }
    let mins = (secs + 30) / 60;
    if mins < 90 {
        return ago(mins, "minute");
    }
    let hours = (mins + 30) / 60;
    if hours < 36 {
        return ago(hours, "hour");
    }
    let days = (hours + 12) / 24;
    if days < 14 {
        return ago(days, "day");
    }
    if days < 70 {
        return ago((days + 3) / 7, "week");
    }
    if days < 365 {
        return ago((days + 15) / 30, "month");
    }
    if days < 5 * 365 {
        let months_total = (days * 24 + 365) / 730;
        let (years, months) = (months_total / 12, months_total % 12);
        let y = if years == 1 { format!("{years} year") } else { format!("{years} years") };
        if months == 0 {
            return format!("{y} ago");
        }
        return format!("{y}, {}", ago(months, "month"));
    }
    ago((days + 183) / 365, "year")
}

/// `--date=human`: relativo no mesmo dia; depois a data sem as partes que se repetem com hoje.
fn human(t: i64, tz: i32) -> String {
    let now = os::now();
    if now >= t && now - t < 86_400 {
        return relative(t, now);
    }
    let tm = tm_of(t, crate::object::tz_offset_secs(tz));
    let now_tm = tm_of(now, crate::object::tz_offset_secs(tz));
    let wd = &WEEKDAYS[tm.wday as usize][..3];
    let mo = &MONTHS[tm.mon as usize][..3];
    if tm.year == now_tm.year {
        if (now - t).abs() < 5 * 86_400 {
            return format!("{wd} {:02}:{:02}", tm.hour, tm.min);
        }
        return format!("{wd} {mo} {} {:02}:{:02}", tm.mday, tm.hour, tm.min);
    }
    format!("{mo} {} {}", tm.mday, tm.year)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        let t = 1_768_478_400;
        assert_eq!(show_date(t, 0, &DateMode::Normal), "Thu Jan 15 12:00:00 2026 +0000");
        assert_eq!(show_date(t, -300, &DateMode::Normal), "Thu Jan 15 09:00:00 2026 -0300");
        assert_eq!(show_date(t, 0, &DateMode::Iso), "2026-01-15 12:00:00 +0000");
        assert_eq!(show_date(t, 0, &DateMode::Rfc), "Thu, 15 Jan 2026 12:00:00 +0000");
        assert_eq!(show_date(t, 0, &DateMode::Short), "2026-01-15");
        let fmt = DateMode::Strftime("%Y-%m-%d %H:%M:%S %z|%Z|%s|%G-%V|%-d|%e|%a|%^b".into(), false);
        assert_eq!(show_date(t, -300, &fmt), "2026-01-15 09:00:00 -0300||1768478400|2026-03|15|15|Thu|JAN");
    }

    #[test]
    fn relative_matches_oracle_bisection() {
        let t = 1_768_478_400;
        for (d, want) in [
            (0, "0 seconds ago"),
            (89, "89 seconds ago"),
            (90, "2 minutes ago"),
            (150, "3 minutes ago"),
            (5369, "89 minutes ago"),
            (5370, "2 hours ago"),
            (127_770, "2 days ago"),
            (1_164_570, "2 weeks ago"),
            (6_434_970, "3 months ago"),
            (31_490_970, "1 year ago"),
            (34_000_000, "1 year, 1 month ago"),
            (61_730_970, "2 years ago"),
            (500_000_000, "16 years ago"),
        ] {
            assert_eq!(relative(t, t + d), want, "d={d}");
        }
        assert_eq!(relative(t, t - 5), "in the future");
    }

    #[test]
    fn strict_parse_matches_oracle() {
        for (input, want) in [
            ("1768478400 +0000", Some((1_768_478_400, 0))),
            ("@1768478400 +0200", Some((1_768_478_400, 200))),
            ("2026-01-15T12:00:00Z", Some((1_768_478_400, 0))),
            ("2026-01-15 12:00:00 +0300", Some((1_768_467_600, 300))),
            ("2026-01-15T12:00:00+02:00", Some((1_768_471_200, 200))),
            ("Thu, 15 Jan 2026 12:00:00 -0500", Some((1_768_496_400, -500))),
            ("Thu Jan 15 12:00:00 2026 +0000", Some((1_768_478_400, 0))),
            ("2026-01-15 10:30", Some((1_768_473_000, 0))),
            ("2026-01-15T12:00:00.123+0100", Some((1_768_474_800, 100))),
            ("2026.01.15 12:00:00", Some((1_768_478_400, 0))),
            ("01/15/2026 12:00:00", Some((1_768_478_400, 0))),
            ("15.01.2026 12:00:00", Some((1_768_478_400, 0))),
            ("15/01/2026 12:00:00", Some((1_768_478_400, 0))),
            ("12:00:00 2026-01-15", Some((1_768_478_400, 0))),
            ("Jan 15 2026 12:00:00", Some((1_768_478_400, 0))),
            ("15 Jan 2026 12:00:00 -0300", Some((1_768_489_200, -300))),
            ("January 15, 2026 12:00:00 +0000", Some((1_768_478_400, 0))),
            ("2026-01-15 12:00:00 EST", Some((1_768_496_400, -500))),
            ("2026-01-15 12:00:00 CET", Some((1_768_474_800, 100))),
            ("2026-01-15 12:00:00 +03", Some((1_768_467_600, 300))),
            ("2026-01-15 12:00:00 -12:30", Some((1_768_523_400, -1230))),
            ("2026-01-15 3:00pm", Some((1_768_489_200, 0))),
            ("20260115T120000", Some((1_768_478_400, 0))),
            ("1768478400", Some((1_768_478_400, 0))),
            ("1768478400 -0300", Some((1_768_478_400, -300))),
            ("2026-02-30 12:00:00", Some((1_772_452_800, 0))),
            ("2026-13-01 12:00:00", Some((1_768_305_600, 0))),
            ("2026-01-15", None),
            ("Jan 15 2026", None),
            ("yesterday", None),
            ("garbage", None),
            ("2101-01-01 00:00:00", None),
            ("2026-01-15 12:60:00", None),
        ] {
            assert_eq!(parse_date(input.as_bytes()), want, "{input}");
        }
    }
}
