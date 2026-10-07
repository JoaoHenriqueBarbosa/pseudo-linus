//! Datas do jq 1.7.1 (`gmtime`, `localtime`, `mktime`, `strftime`, `strflocaltime`, `strptime`,
//! `now`) com a semântica da glibc que o jq do Debian chama, e com relógio e fuso do sandbox.
//!
//! - O "tempo quebrado" é o array do `tm2jv`: `[ano, mês 0-11, dia, hora, min, seg, dia da semana,
//!   dia do ano]`. Do array para `struct tm` (`jv2tm`) os oito campos têm que ser números, e a
//!   conversão de double para `int` trunca (saturando como o x86 faz).
//! - `mktime` é `timegm` (normaliza campos fora da faixa); `-1` vira o erro "invalid gmtime
//!   representation", como no jq.
//! - O `strftime` monta o `tm` a partir do array, então `tm_zone` fica nulo, `tm_gmtoff` zero e
//!   `tm_isdst` zero: `%Z` sai como o `tzname[0]` do fuso (a abreviação do horário padrão), `%z` sai
//!   `+0000`, e `%s` é o `mktime` local da glibc.
//! - O fuso vem do `TZ` do ambiente do processo (ou de `local_timezone()` do sandbox quando não há
//!   `TZ`): arquivo TZif no sistema de arquivos do sandbox, depois a base IANA embutida
//!   (`jiff-tzdb`), depois texto POSIX; o que não resolver vira UTC com o próprio texto como
//!   abreviação, como a glibc faz.

use jaq_json::{Rc, Val, err};
use sysabi::{Clock, Syscalls};
use ul_common::time::strftime::{StrfTime, strftime_lazy};
use ul_common::time::{Civil, civil_from_days, days_from_civil, weekday};

type ValR = jaq_json::ValR;

/// Fuso horário local do processo.
pub struct TimeZone {
    tz: Option<jiff::tz::TimeZone>,
    /// Abreviação usada quando o `TZ` não resolve (a glibc mantém o texto como nome do fuso).
    fallback: String,
}

impl TimeZone {
    pub fn utc() -> TimeZone {
        TimeZone { tz: Some(jiff::tz::TimeZone::UTC), fallback: "UTC".into() }
    }

    /// Resolve o fuso do processo: `TZ` se existir, senão o fuso do sandbox.
    pub fn from_process(sys: &dyn Syscalls) -> TimeZone {
        let spec = match sys.getenv(b"TZ") {
            Some(tz) => tz,
            None => sys.local_timezone(),
        };
        if spec.starts_with(b"TZif") {
            return match jiff::tz::TimeZone::tzif("localtime", &spec) {
                Ok(tz) => TimeZone { tz: Some(tz), fallback: "UTC".into() },
                Err(_) => TimeZone::utc(),
            };
        }
        let text = String::from_utf8_lossy(&spec).into_owned();
        if text.is_empty() {
            return TimeZone::utc();
        }
        let name = text.strip_prefix(':').unwrap_or(&text);
        // Arquivo no sistema de arquivos do sandbox (caminho absoluto ou relativo ao TZDIR).
        let path = if name.starts_with('/') { name.to_string() } else { format!("/usr/share/zoneinfo/{name}") };
        if !name.contains("..")
            && let Ok(data) = sysabi::sys::read_file(path.as_bytes())
                && let Ok(tz) = jiff::tz::TimeZone::tzif(name, &data) {
                    return TimeZone { tz: Some(tz), fallback: name.into() };
                }
        if let Some((canon, data)) = jiff_tzdb::get(name)
            && let Ok(tz) = jiff::tz::TimeZone::tzif(canon, data) {
                return TimeZone { tz: Some(tz), fallback: name.into() };
            }
        if let Ok(tz) = jiff::tz::TimeZone::posix(name) {
            return TimeZone { tz: Some(tz), fallback: name.into() };
        }
        TimeZone { tz: None, fallback: name.into() }
    }

    /// Deslocamento (segundos a leste de UTC), horário de verão e abreviação no instante `t`.
    fn info(&self, t: i64) -> (i64, bool, String) {
        let Some(tz) = &self.tz else {
            return (0, false, self.fallback.clone());
        };
        let t = t.clamp(-377_705_116_800, 253_402_207_200);
        let ts = jiff::Timestamp::from_second(t).unwrap_or(jiff::Timestamp::UNIX_EPOCH);
        let info = tz.to_offset_info(ts);
        (info.offset().seconds() as i64, info.dst().is_dst(), info.abbreviation().to_string())
    }

    /// `tzname[0]`: a abreviação do horário padrão do fuso (o que a glibc usa no `%Z` de um `tm`
    /// sem `tm_zone`).
    fn std_name(&self) -> String {
        if self.tz.is_none() {
            return self.fallback.clone();
        }
        // Regra atual do fuso: janeiro e julho de um ano distante; o que não for verão é o padrão.
        for t in [4_102_444_800i64, 4_118_083_200] {
            let (_, dst, abbr) = self.info(t);
            if !dst {
                return abbr;
            }
        }
        self.info(4_102_444_800).2
    }

    /// Deslocamento do horário padrão perto de `t` (o `mktime` com `tm_isdst = 0`).
    fn std_offset_near(&self, t: i64) -> i64 {
        let (off, dst, _) = self.info(t);
        if !dst {
            return off;
        }
        for delta in [-200 * 86400, 200 * 86400, -100 * 86400, 100 * 86400] {
            let (o, d, _) = self.info(t + delta);
            if !d {
                return o;
            }
        }
        off
    }
}

/// `now`: `gettimeofday` (microssegundos) do relógio do sandbox.
pub fn now(sys: &dyn Syscalls) -> f64 {
    match sys.clock_gettime(Clock::Realtime) {
        Ok(ts) => ts.sec as f64 + (ts.nsec / 1000) as f64 / 1_000_000.0,
        Err(_) => 0.0,
    }
}

/// `struct tm`.
#[derive(Clone, Debug, Default)]
pub struct Tm {
    pub sec: i64,
    pub min: i64,
    pub hour: i64,
    pub mday: i64,
    pub mon: i64,
    /// Anos desde 1900.
    pub year: i64,
    pub wday: i64,
    pub yday: i64,
    pub isdst: i64,
    pub gmtoff: i64,
    pub zone: Option<String>,
}

/// `gmtime_r`: `None` quando o ano não cabe num `int`.
pub fn gmtime_r(t: i64) -> Option<Tm> {
    let c = Civil::offtime(t, 0)?;
    Some(Tm {
        sec: c.sec,
        min: c.min,
        hour: c.hour,
        mday: c.mday,
        mon: c.mon - 1,
        year: c.year - 1900,
        wday: c.wday,
        yday: c.yday,
        isdst: 0,
        gmtoff: 0,
        zone: Some("GMT".into()),
    })
}

/// `localtime_r` no fuso dado.
pub fn localtime_r(t: i64, tz: &TimeZone) -> Option<Tm> {
    let (off, dst, abbr) = tz.info(t);
    let mut tm = gmtime_r(t.checked_add(off)?)?;
    tm.isdst = dst as i64;
    tm.gmtoff = off;
    tm.zone = Some(abbr);
    Some(tm)
}

/// `timegm`: normaliza os campos e devolve os segundos desde a época.
pub fn timegm(tm: &Tm) -> i64 {
    let mon = tm.mon;
    let year = 1900 + tm.year + mon.div_euclid(12);
    let mon = mon.rem_euclid(12);
    let days = days_from_civil(year, mon + 1, 1) + tm.mday - 1;
    days * 86_400 + tm.hour * 3600 + tm.min * 60 + tm.sec
}

/// `mktime` local da glibc com `tm_isdst = 0` (usado pelo `%s` do `strftime`).
fn mktime_local(tm: &Tm, tz: &TimeZone) -> i64 {
    let as_utc = timegm(tm);
    as_utc - tz.std_offset_near(as_utc)
}

/// Conversão de double para `int` do C no x86 (truncamento; fora da faixa vira `INT_MIN`).
fn c_int(d: f64) -> i64 {
    if d.is_nan() || d >= 2_147_483_648.0 || d <= -2_147_483_649.0 {
        i32::MIN as i64
    } else {
        d.trunc() as i64
    }
}

/// `tm2jv`.
fn tm2jv(tm: &Tm) -> Vec<Val> {
    [tm.year + 1900, tm.mon, tm.mday, tm.hour, tm.min, tm.sec, tm.wday, tm.yday]
        .into_iter()
        .map(|n| Val::num(n as f64))
        .collect()
}

/// `jv2tm`: os oito campos numéricos do array.
fn jv2tm(v: &Val) -> Option<Tm> {
    let a = v.as_arr()?;
    let mut f = [0i64; 8];
    for (i, slot) in f.iter_mut().enumerate() {
        *slot = c_int(a.get(i)?.as_f64()?);
    }
    Some(Tm {
        year: f[0] - 1900,
        mon: f[1],
        mday: f[2],
        hour: f[3],
        min: f[4],
        sec: f[5],
        wday: f[6],
        yday: f[7],
        ..Tm::default()
    })
}

fn broken_down(tm: Option<Tm>, fsecs: f64) -> ValR {
    let tm = tm.ok_or_else(|| err("error converting number of seconds since epoch to datetime"))?;
    let mut a = tm2jv(&tm);
    a[5] = Val::num(tm.sec as f64 + (fsecs - fsecs.floor()));
    Ok(Val::Arr(Rc::new(a)))
}

/// `time_t secs = fsecs` no x86 (fora da faixa vira o menor `time_t`).
fn to_time_t(f: f64) -> i64 {
    if f.is_nan() || f >= 9.223_372_036_854_776e18 || f < -9.223_372_036_854_776e18 {
        i64::MIN
    } else {
        f.trunc() as i64
    }
}

/// `gmtime`.
pub fn gmtime(v: &Val) -> ValR {
    let Some(fsecs) = v.as_f64() else {
        return Err(err("gmtime() requires numeric inputs"));
    };
    broken_down(gmtime_r(to_time_t(fsecs)), fsecs)
}

/// `localtime`.
pub fn localtime(v: &Val, tz: &TimeZone) -> ValR {
    let Some(fsecs) = v.as_f64() else {
        return Err(err("localtime() requires numeric inputs"));
    };
    broken_down(localtime_r(to_time_t(fsecs), tz), fsecs)
}

/// `mktime`.
pub fn mktime(v: &Val) -> ValR {
    let Some(a) = v.as_arr() else {
        return Err(err("mktime requires array inputs"));
    };
    if a.len() < 6 {
        return Err(err("mktime requires parsed datetime inputs"));
    }
    let tm = jv2tm(v).ok_or_else(|| err("mktime requires parsed datetime inputs"))?;
    let t = timegm(&tm);
    if t == -1 {
        return Err(err("invalid gmtime representation"));
    }
    Ok(Val::num(t as f64))
}

/// `strftime/1` (`tz = None`) e `strflocaltime/1` (`tz = Some`).
pub fn strftime(v: &Val, fmt: &Val, tz: Option<&TimeZone>) -> ValR {
    let name = if tz.is_some() { "strflocaltime/1" } else { "strftime/1" };
    let utc = TimeZone::utc();
    let zone = tz.unwrap_or(&utc);
    let arr = match v {
        Val::Num(n) => {
            let f = n.as_f64();
            let tm = if tz.is_some() { localtime_r(to_time_t(f), zone) } else { gmtime_r(to_time_t(f)) };
            broken_down(tm, f)?
        }
        Val::Arr(_) => {
            if !fmt.is_str() {
                return Err(err(format!("{name} requires a string format")));
            }
            v.clone()
        }
        _ => return Err(err(format!("{name} requires parsed datetime inputs"))),
    };
    let Some(tm) = jv2tm(&arr) else {
        return Err(err(format!("{name} requires parsed datetime inputs")));
    };
    let Some(f) = fmt.as_str() else {
        // O jq 1.7.1 chega aqui com número na entrada e formato que não é string, e aborta numa
        // asserção do `jv_string_value`.
        crate::abort_with("jq: src/jv.c:1450: jv_string_value: Assertion `JVP_HAS_KIND(j, JV_KIND_STRING)' failed.\n");
    };
    // O `tm` do jq não tem `tm_zone` (o `%Z` sai como o `tzname[0]` do fuso) nem `tm_gmtoff`.
    let zone_name = tm.zone.clone().unwrap_or_else(|| zone.std_name());
    let when = StrfTime {
        civil: Civil {
            year: tm.year + 1900,
            mon: tm.mon + 1,
            mday: tm.mday,
            hour: tm.hour,
            min: tm.min,
            sec: tm.sec,
            wday: tm.wday,
            yday: tm.yday,
        },
        gmtoff: tm.gmtoff,
        zone: zone_name.as_bytes(),
    };
    // O `%s` é o `mktime` local do `tm`, só quando o formato o pede. Sem limite de saída, o
    // `strftime_lazy` não devolve `None`.
    let out = strftime_lazy(f.as_bytes(), &when, &mut || mktime_local(&tm, zone), usize::MAX).unwrap_or_default();
    if out.is_empty() {
        return Err(err(format!("{name}: unknown system failure")));
    }
    Ok(Val::from(String::from_utf8_lossy(&out).into_owned()))
}

const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MONTHS: [&str; 12] =
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

/// Estado do `strptime` da glibc.
#[derive(Default)]
struct PState {
    have_i: bool,
    is_pm: bool,
    century: Option<i64>,
    want_century: bool,
    want_xday: bool,
    have_wday: bool,
    have_yday: bool,
    have_mon: bool,
    have_mday: bool,
}

/// `strptime/1`.
pub fn strptime(v: &Val, fmt: &Val) -> ValR {
    let (Some(input), Some(f)) = (v.as_str(), fmt.as_str()) else {
        return Err(err("strptime/1 requires string inputs and arguments"));
    };
    let mut tm = Tm { wday: 8, yday: 367, ..Tm::default() };
    let utc = TimeZone::utc();
    let rest = match parse_tm(input, f, &mut tm, &utc) {
        Some(r) if r.is_empty() || r.starts_with(|c: char| c.is_ascii_whitespace()) => r,
        _ => return Err(err(format!("date \"{input}\" does not match format \"{f}\""))),
    };
    if tm.wday == 8 && tm.mday != 0 && (0..=11).contains(&tm.mon) {
        tm.wday = weekday(days_from_civil(tm.year + 1900, tm.mon + 1, tm.mday));
    }
    if tm.yday == 367 && tm.mday != 0 && (0..=11).contains(&tm.mon) {
        tm.yday = days_from_civil(tm.year + 1900, tm.mon + 1, tm.mday) - days_from_civil(tm.year + 1900, 1, 1);
    }
    let mut out = tm2jv(&tm);
    if !rest.is_empty() {
        out.push(Val::from(rest.to_string()));
    }
    Ok(Val::Arr(Rc::new(out)))
}

fn read_num(s: &str, max_digits: usize) -> Option<(i64, &str)> {
    let s = s.trim_start_matches(|c: char| c.is_ascii_whitespace());
    let (neg, body) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let len = body.bytes().take(max_digits).take_while(u8::is_ascii_digit).count();
    if len == 0 {
        return None;
    }
    let n: i64 = body[..len].parse().ok()?;
    Some((if neg { -n } else { n }, &body[len..]))
}

fn read_unsigned(s: &str, max_digits: usize) -> Option<(i64, &str)> {
    let s = s.trim_start_matches(|c: char| c.is_ascii_whitespace());
    let len = s.bytes().take(max_digits).take_while(u8::is_ascii_digit).count();
    if len == 0 {
        return None;
    }
    Some((s[..len].parse().ok()?, &s[len..]))
}

fn match_name<'a>(s: &'a str, names: &[&str]) -> Option<(usize, &'a str)> {
    for (i, name) in names.iter().enumerate() {
        // Nome completo primeiro, depois a abreviação de três letras.
        for cand in [*name, &name[..3]] {
            if s.len() >= cand.len() && s[..cand.len()].eq_ignore_ascii_case(cand) {
                return Some((i, &s[cand.len()..]));
            }
        }
    }
    None
}

/// Parte comum do `strptime` (glibc, locale C). Devolve o resto da entrada.
fn parse_tm<'a>(input: &'a str, fmt: &str, tm: &mut Tm, tz: &TimeZone) -> Option<&'a str> {
    let mut st = PState::default();
    let rest = parse_rec(input, fmt, tm, &mut st, tz)?;
    if st.have_i && st.is_pm {
        tm.hour += 12;
    }
    if let Some(c) = st.century {
        if st.want_century {
            tm.year = tm.year % 100 + (c - 19) * 100;
        } else {
            tm.year += (c - 19) * 100;
        }
    }
    if st.want_xday && !st.have_wday {
        if !(st.have_mon && st.have_mday) && st.have_yday {
            // Mês e dia a partir do dia do ano.
            let y = tm.year + 1900;
            let days = days_from_civil(y, 1, 1) + tm.yday;
            let (_, m, d) = civil_from_days(days);
            if !st.have_mon {
                tm.mon = m - 1;
            }
            if !st.have_mday {
                tm.mday = d;
            }
            st.have_mon = true;
            st.have_mday = true;
        }
        if tm.year >= -1900 {
            let y = tm.year + 1900;
            tm.wday = weekday(days_from_civil(y, tm.mon + 1, 1) + tm.mday - 1);
        }
    }
    if st.want_xday && !st.have_yday && (st.have_mon || (tm.year as u64) <= 199) {
        let y = tm.year + 1900;
        tm.yday = days_from_civil(y, tm.mon.clamp(0, 11) + 1, 1) + tm.mday - 1 - days_from_civil(y, 1, 1);
    }
    Some(rest)
}

fn parse_rec<'a>(input: &'a str, fmt: &str, tm: &mut Tm, st: &mut PState, tz: &TimeZone) -> Option<&'a str> {
    let mut s = input;
    let f = fmt.as_bytes();
    let mut i = 0;
    while i < f.len() {
        let c = f[i];
        if c.is_ascii_whitespace() {
            s = s.trim_start_matches(|c: char| c.is_ascii_whitespace());
            i += 1;
            continue;
        }
        if c != b'%' {
            let ch = fmt[i..].chars().next()?;
            s = s.strip_prefix(ch)?;
            i += ch.len_utf8();
            continue;
        }
        i += 1;
        while i < f.len() && matches!(f[i], b'E' | b'O' | b'-' | b'_' | b'0' | b'^' | b'#') {
            i += 1;
        }
        while i < f.len() && f[i].is_ascii_digit() {
            i += 1;
        }
        if i >= f.len() {
            return None;
        }
        let conv = f[i];
        i += 1;
        match conv {
            b'%' => s = s.strip_prefix('%')?,
            b'n' | b't' => s = s.trim_start_matches(|c: char| c.is_ascii_whitespace()),
            b'a' | b'A' => {
                let t = s.trim_start_matches(|c: char| c.is_ascii_whitespace());
                let (d, r) = match_name(t, &DAYS)?;
                tm.wday = d as i64;
                st.have_wday = true;
                s = r;
            }
            b'b' | b'B' | b'h' => {
                let t = s.trim_start_matches(|c: char| c.is_ascii_whitespace());
                let (m, r) = match_name(t, &MONTHS)?;
                tm.mon = m as i64;
                st.have_mon = true;
                st.want_xday = true;
                s = r;
            }
            b'c' => s = parse_rec(s, "%a %b %e %H:%M:%S %Y", tm, st, tz)?,
            b'C' => {
                let (n, r) = read_num(s, 2)?;
                st.century = Some(n);
                st.want_xday = true;
                s = r;
            }
            b'd' | b'e' => {
                let (n, r) = read_unsigned(s, 2)?;
                if !(1..=31).contains(&n) {
                    return None;
                }
                tm.mday = n;
                st.have_mday = true;
                st.want_xday = true;
                s = r;
            }
            b'D' | b'x' => {
                s = parse_rec(s, "%m/%d/%y", tm, st, tz)?;
                st.want_xday = true;
            }
            b'F' => {
                s = parse_rec(s, "%Y-%m-%d", tm, st, tz)?;
                st.want_xday = true;
            }
            b'k' | b'H' => {
                let (n, r) = read_unsigned(s, 2)?;
                if n > 23 {
                    return None;
                }
                tm.hour = n;
                st.have_i = false;
                s = r;
            }
            b'l' | b'I' => {
                let (n, r) = read_unsigned(s, 2)?;
                if !(1..=12).contains(&n) {
                    return None;
                }
                tm.hour = n % 12;
                st.have_i = true;
                s = r;
            }
            b'j' => {
                let (n, r) = read_unsigned(s, 3)?;
                if !(1..=366).contains(&n) {
                    return None;
                }
                tm.yday = n - 1;
                st.have_yday = true;
                s = r;
            }
            b'm' => {
                let (n, r) = read_unsigned(s, 2)?;
                if !(1..=12).contains(&n) {
                    return None;
                }
                tm.mon = n - 1;
                st.have_mon = true;
                st.want_xday = true;
                s = r;
            }
            b'M' => {
                let (n, r) = read_unsigned(s, 2)?;
                if n > 59 {
                    return None;
                }
                tm.min = n;
                s = r;
            }
            b'p' | b'P' => {
                let t = s.trim_start_matches(|c: char| c.is_ascii_whitespace());
                if t.len() >= 2 && t[..2].eq_ignore_ascii_case("AM") {
                    st.is_pm = false;
                } else if t.len() >= 2 && t[..2].eq_ignore_ascii_case("PM") {
                    st.is_pm = true;
                } else {
                    return None;
                }
                s = &t[2..];
            }
            b'r' => s = parse_rec(s, "%I:%M:%S %p", tm, st, tz)?,
            b'R' => s = parse_rec(s, "%H:%M", tm, st, tz)?,
            b's' => {
                let (n, r) = read_num(s, 20)?;
                let lt = localtime_r(n, tz)?;
                *tm = lt;
                s = r;
            }
            b'S' => {
                let (n, r) = read_unsigned(s, 2)?;
                if n > 61 {
                    return None;
                }
                tm.sec = n;
                s = r;
            }
            b'T' | b'X' => s = parse_rec(s, "%H:%M:%S", tm, st, tz)?,
            b'u' => {
                let (n, r) = read_unsigned(s, 1)?;
                if !(1..=7).contains(&n) {
                    return None;
                }
                tm.wday = n % 7;
                st.have_wday = true;
                s = r;
            }
            b'w' => {
                let (n, r) = read_unsigned(s, 1)?;
                if n > 6 {
                    return None;
                }
                tm.wday = n;
                st.have_wday = true;
                s = r;
            }
            b'U' | b'V' | b'W' => {
                let (n, r) = read_unsigned(s, 2)?;
                if n > 53 {
                    return None;
                }
                s = r;
            }
            b'g' => {
                let (_, r) = read_unsigned(s, 2)?;
                s = r;
            }
            b'G' => {
                let (_, r) = read_num(s, 4)?;
                s = r;
            }
            b'y' => {
                let (n, r) = read_unsigned(s, 2)?;
                if n > 99 {
                    return None;
                }
                tm.year = if n >= 69 { n } else { n + 100 };
                st.want_century = true;
                st.want_xday = true;
                s = r;
            }
            b'Y' => {
                let (n, r) = read_num(s, 4)?;
                tm.year = n - 1900;
                st.want_century = false;
                st.want_xday = true;
                s = r;
            }
            b'z' => {
                let t = s.trim_start_matches(|c: char| c.is_ascii_whitespace());
                if let Some(r) = t.strip_prefix('Z') {
                    tm.gmtoff = 0;
                    s = r;
                } else {
                    let (neg, body) = match t.as_bytes().first() {
                        Some(b'+') => (false, &t[1..]),
                        Some(b'-') => (true, &t[1..]),
                        _ => return None,
                    };
                    let digits: String = body.chars().take(5).filter(|c| *c != ':').take(4).collect();
                    if !(digits.len() == 2 || digits.len() == 4) || !digits.bytes().all(|c| c.is_ascii_digit()) {
                        return None;
                    }
                    let consumed = {
                        let mut n = 0;
                        let mut d = 0;
                        for ch in body.chars() {
                            if d == digits.len() {
                                break;
                            }
                            n += ch.len_utf8();
                            if ch != ':' {
                                d += 1;
                            }
                        }
                        n
                    };
                    let hh: i64 = digits[..2].parse().ok()?;
                    let mm: i64 = if digits.len() == 4 { digits[2..].parse().ok()? } else { 0 };
                    if hh > 12 && !(hh == 13 || hh == 14) && mm == 0 {
                        // A glibc aceita até +14 horas.
                    }
                    let off = hh * 3600 + mm * 60;
                    tm.gmtoff = if neg { -off } else { off };
                    s = &body[consumed..];
                }
            }
            b'Z' => {
                let t = s.trim_start_matches(|c: char| c.is_ascii_whitespace());
                let end = t.find(|c: char| c.is_ascii_whitespace()).unwrap_or(t.len());
                s = &t[end..];
            }
            _ => return None,
        }
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gmtime_round_trip() {
        let tm = gmtime_r(1_700_000_000).unwrap();
        assert_eq!((tm.year + 1900, tm.mon, tm.mday, tm.hour, tm.min, tm.sec, tm.wday, tm.yday), (2023, 10, 14, 22, 13, 20, 2, 317));
        assert_eq!(timegm(&tm), 1_700_000_000);
    }

    #[test]
    fn strftime_builds_the_tm_without_zone_or_offset() {
        // O `strftime` do jq parte do array: `%Z` é o `tzname[0]` do fuso, `%z` é zero e `%s` é o
        // `mktime` local. A formatação em si é testada no `ul-common`.
        let out = strftime(&Val::num(1_700_000_000.0), &Val::from("%c|%s|%Z|%z|%G-%V".to_string()), None);
        let Ok(v) = out else { panic!("strftime falhou") };
        assert_eq!(v.as_str(), Some("Tue Nov 14 22:13:20 2023|1700000000|UTC|+0000|2023-46"));
    }
}
