//! Funções de data e hora do SQLite 3.46.1 (porte do date.c), com "agora" vindo do relógio do sandbox
//! e `localtime`/`utc` usando o fuso do sandbox.
//!
//! O SQLite pega o "agora" do `xCurrentTimeInt64` do VFS e o fuso do `localtime_r` da libc, e os
//! dois seriam os do host: o sqlite-plugin não expõe `xCurrentTime` na trait, e o `localtime_r` lê
//! o `TZ` do processo host. Por isso `julianday`, `unixepoch`, `date`, `time`, `datetime`,
//! `strftime`, `timediff` e `current_*` são registradas por cima das nativas, com a mesma
//! aritmética (inteiros e `double` nos mesmos pontos) e com o `printf` do próprio SQLite pros
//! formatos `%f` e `%g`.

use std::cell::{Cell, RefCell};

use rusqlite::Connection;
use rusqlite::functions::{Context, FunctionFlags};
use rusqlite::types::{Value, ValueRef};
use sysabi::{Clock, sys};

use super::{fmt_real, sqlite_printf};
use crate::unwind;

thread_local! {
    /// "Agora" do comando corrente (o `iCurrentTime` do Vdbe): o mesmo instante em todo o comando.
    static NOW: Cell<i64> = const { Cell::new(0) };
    /// Fuso do sandbox já carregado: (texto do `local_timezone`, fuso).
    static TZ: RefCell<Option<(Vec<u8>, jiff::tz::TimeZone)>> = const { RefCell::new(None) };
}

/// Começo de comando: o próximo "agora" é lido de novo.
pub fn reset_statement_time() {
    NOW.with(|n| n.set(0));
}

/// `sqlite3StmtCurrentTime`: dia juliano × 86400000 do relógio do sandbox.
fn current_time() -> Option<i64> {
    let cached = NOW.with(|n| n.get());
    if cached > 0 {
        return Some(cached);
    }
    let t = unwind::guard(|| sys::current().clock_gettime(Clock::Realtime)).and_then(Result::ok)?;
    let ms = 210_866_760_000_000i64 + 1000 * t.sec + i64::from(t.nsec / 1_000_000);
    NOW.with(|n| n.set(ms));
    Some(ms)
}

/// Fuso do sandbox (o mesmo que a libc usaria com o `TZ`/`/etc/localtime` do sandbox).
fn sandbox_tz() -> jiff::tz::TimeZone {
    let raw = unwind::guard(|| sys::current().local_timezone()).unwrap_or_default();
    if let Some(tz) = TZ.with(|t| t.borrow().as_ref().filter(|(k, _)| *k == raw).map(|(_, z)| z.clone())) {
        return tz;
    }
    let tz = resolve_tz(&raw);
    TZ.with(|t| *t.borrow_mut() = Some((raw, tz.clone())));
    tz
}

fn resolve_tz(raw: &[u8]) -> jiff::tz::TimeZone {
    use jiff::tz::TimeZone;
    if raw.starts_with(b"TZif") {
        return TimeZone::tzif("localtime", raw).unwrap_or(TimeZone::UTC);
    }
    let s = String::from_utf8_lossy(raw).trim().to_string();
    let s = s.strip_prefix(':').unwrap_or(&s).to_string();
    if s.is_empty() {
        return TimeZone::UTC;
    }
    let path = if s.starts_with('/') { s.clone() } else { format!("/usr/share/zoneinfo/{s}") };
    if !s.contains("..")
        && let Some(Ok(data)) = unwind::guard(|| sys::read_file(path.as_bytes()))
        && let Ok(tz) = TimeZone::tzif(&s, &data)
    {
        return tz;
    }
    TimeZone::posix(&s).unwrap_or(TimeZone::UTC)
}

#[derive(Clone, Copy, Default, Debug)]
struct DateTime {
    ijd: i64,
    y: i32,
    mo: i32,
    d: i32,
    h: i32,
    mi: i32,
    tz: i32,
    s: f64,
    valid_jd: bool,
    valid_ymd: bool,
    valid_hms: bool,
    n_floor: i32,
    raw_s: bool,
    is_error: bool,
    use_subsec: bool,
    is_utc: bool,
    is_local: bool,
}

fn isspace(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

fn at(z: &[u8], i: usize) -> u8 {
    z.get(i).copied().unwrap_or(0)
}

/// `getDigits`: `fmt` é uma lista de (dígitos, mínimo, máximo, separador).
fn get_digits(z: &[u8], fmt: &[(usize, i32, i32, u8)], out: &mut [i32]) -> usize {
    let mut cnt = 0;
    let mut p = 0;
    for (k, &(n, min, max, next)) in fmt.iter().enumerate() {
        let mut val = 0;
        for _ in 0..n {
            let c = at(z, p);
            if !c.is_ascii_digit() {
                return cnt;
            }
            val = val * 10 + i32::from(c - b'0');
            p += 1;
        }
        if val < min || val > max || (next != 0 && next != at(z, p)) {
            return cnt;
        }
        out[k] = val;
        p += 1;
        cnt += 1;
    }
    cnt
}

/// `parseTimezone`: `true` em erro.
fn parse_timezone(z: &[u8], p: &mut DateTime) -> bool {
    let mut i = 0;
    while isspace(at(z, i)) {
        i += 1;
    }
    p.tz = 0;
    let c = at(z, i);
    
    let sgn = if c == b'-' {
        -1
    } else if c == b'+' {
        1
    } else if c == b'Z' || c == b'z' {
        i += 1;
        p.is_local = false;
        p.is_utc = true;
        while isspace(at(z, i)) {
            i += 1;
        }
        return at(z, i) != 0;
    } else {
        return c != 0;
    };
    i += 1;
    let mut v = [0; 2];
    if get_digits(&z[i.min(z.len())..], &[(2, 0, 14, b':'), (2, 0, 59, 0)], &mut v) != 2 {
        return true;
    }
    i += 5;
    p.tz = sgn * (v[1] + v[0] * 60);
    while isspace(at(z, i)) {
        i += 1;
    }
    at(z, i) != 0
}

/// `parseHhMmSs`: `true` em erro.
fn parse_hhmmss(z: &[u8], p: &mut DateTime) -> bool {
    let mut v = [0; 2];
    if get_digits(z, &[(2, 0, 24, b':'), (2, 0, 59, 0)], &mut v) != 2 {
        return true;
    }
    let (h, m) = (v[0], v[1]);
    let mut i = 5;
    let mut ms = 0.0f64;
    let s;
    if at(z, i) == b':' {
        i += 1;
        let mut sv = [0; 1];
        if get_digits(&z[i.min(z.len())..], &[(2, 0, 59, 0)], &mut sv) != 1 {
            return true;
        }
        s = sv[0];
        i += 2;
        if at(z, i) == b'.' && at(z, i + 1).is_ascii_digit() {
            let mut scale = 1.0f64;
            i += 1;
            while at(z, i).is_ascii_digit() {
                ms = ms * 10.0 + f64::from(at(z, i) - b'0');
                scale *= 10.0;
                i += 1;
            }
            ms /= scale;
        }
    } else {
        s = 0;
    }
    p.valid_jd = false;
    p.raw_s = false;
    p.valid_hms = true;
    p.h = h;
    p.mi = m;
    p.s = f64::from(s) + ms;
    parse_timezone(&z[i.min(z.len())..], p)
}

fn datetime_error(p: &mut DateTime) {
    *p = DateTime { is_error: true, ..DateTime::default() };
}

/// `computeJD`.
fn compute_jd(p: &mut DateTime) {
    if p.valid_jd {
        return;
    }
    let (mut y, mut m, d) = if p.valid_ymd { (p.y, p.mo, p.d) } else { (2000, 1, 1) };
    if !(-4713..=9999).contains(&y) || p.raw_s {
        datetime_error(p);
        return;
    }
    if m <= 2 {
        y -= 1;
        m += 12;
    }
    let a = y / 100;
    let b = 2 - a + (a / 4);
    let x1 = 36525 * (y + 4716) / 100;
    let x2 = 306001 * (m + 1) / 10000;
    p.ijd = ((f64::from(x1 + x2 + d + b) - 1524.5) * 86_400_000.0) as i64;
    p.valid_jd = true;
    if p.valid_hms {
        p.ijd += i64::from(p.h * 3_600_000 + p.mi * 60000) + (p.s * 1000.0 + 0.5) as i64;
        if p.tz != 0 {
            p.ijd -= i64::from(p.tz) * 60000;
            p.valid_ymd = false;
            p.valid_hms = false;
            p.tz = 0;
            p.is_utc = true;
            p.is_local = false;
        }
    }
}

/// `computeFloor`.
fn compute_floor(p: &mut DateTime) {
    if p.d <= 28 {
        p.n_floor = 0;
    } else if (1 << p.mo) & 0x15aa != 0 {
        p.n_floor = 0;
    } else if p.mo != 2 {
        p.n_floor = i32::from(p.d == 31);
    } else if p.y % 4 != 0 || (p.y % 100 == 0 && p.y % 400 != 0) {
        p.n_floor = p.d - 28;
    } else {
        p.n_floor = p.d - 29;
    }
}

/// `parseYyyyMmDd`: `true` em erro.
fn parse_yyyymmdd(z0: &[u8], p: &mut DateTime) -> bool {
    let (z, neg) = if at(z0, 0) == b'-' { (&z0[1..], true) } else { (z0, false) };
    let mut v = [0; 3];
    if get_digits(z, &[(4, 0, 14712, b'-'), (2, 1, 12, b'-'), (2, 1, 31, 0)], &mut v) != 3 {
        return true;
    }
    let mut i = 10;
    while isspace(at(z, i)) || at(z, i) == b'T' {
        i += 1;
    }
    let rest = &z[i.min(z.len())..];
    if !parse_hhmmss(rest, p) {
    } else if at(rest, 0) == 0 {
        p.valid_hms = false;
    } else {
        return true;
    }
    p.valid_jd = false;
    p.valid_ymd = true;
    p.y = if neg { -v[0] } else { v[0] };
    p.mo = v[1];
    p.d = v[2];
    compute_floor(p);
    if p.tz != 0 {
        compute_jd(p);
    }
    false
}

fn clear_ymd_hms_tz(p: &mut DateTime) {
    p.valid_ymd = false;
    p.valid_hms = false;
    p.tz = 0;
}

/// `setDateTimeToCurrent`: `true` em erro.
fn set_to_current(p: &mut DateTime) -> bool {
    match current_time() {
        Some(t) if t > 0 => {
            p.ijd = t;
            p.valid_jd = true;
            p.is_utc = true;
            p.is_local = false;
            clear_ymd_hms_tz(p);
            false
        }
        _ => true,
    }
}

fn set_raw_number(p: &mut DateTime, r: f64) {
    p.s = r;
    p.raw_s = true;
    if (0.0..5_373_484.5).contains(&r) {
        p.ijd = (r * 86_400_000.0 + 0.5) as i64;
        p.valid_jd = true;
    }
}

/// `sqlite3AtoF`: valor e o tipo (>0 número válido, 1 inteiro, 2+ com ponto ou expoente).
pub fn atof(z: &[u8]) -> (f64, i32) {
    let mut i = 0;
    let n = z.len();
    while i < n && isspace(z[i]) {
        i += 1;
    }
    if i >= n {
        return (0.0, 0);
    }
    let start = i;
    if z[i] == b'-' || z[i] == b'+' {
        i += 1;
    }
    let mut n_digit = 0;
    let mut e_type = 1;
    let mut e_valid = true;
    while i < n && z[i].is_ascii_digit() {
        i += 1;
        n_digit += 1;
    }
    if i < n && z[i] == b'.' {
        i += 1;
        e_type += 1;
        while i < n && z[i].is_ascii_digit() {
            i += 1;
            n_digit += 1;
        }
    }
    let mut num_end = i;
    if i < n && (z[i] == b'e' || z[i] == b'E') {
        i += 1;
        e_valid = false;
        e_type += 1;
        if i < n && (z[i] == b'-' || z[i] == b'+') {
            i += 1;
        }
        while i < n && z[i].is_ascii_digit() {
            i += 1;
            e_valid = true;
        }
        if e_valid {
            num_end = i;
        }
    }
    let after_num = i;
    while i < n && isspace(z[i]) {
        i += 1;
    }
    let text = String::from_utf8_lossy(&z[start..num_end]).into_owned();
    let mut t = text.clone();
    if t.ends_with('.') {
        t.push('0');
    }
    if t.starts_with('.') || t.starts_with("-.") || t.starts_with("+.") {
        t = t.replacen('.', "0.", 1);
    }
    let v = if n_digit > 0 { t.parse::<f64>().unwrap_or(0.0) } else { 0.0 };
    let _ = after_num;
    let rc = if i == n && n_digit > 0 && e_valid && e_type > 0 {
        e_type
    } else if e_type >= 2 && (e_type == 3 || e_valid) && n_digit > 0 {
        -1
    } else {
        0
    };
    (v, rc)
}

/// `parseDateOrTime`: `true` em erro.
fn parse_date_or_time(z: &[u8], p: &mut DateTime) -> bool {
    if !parse_yyyymmdd(z, p) {
        return false;
    }
    if !parse_hhmmss(z, p) {
        return false;
    }
    if z.eq_ignore_ascii_case(b"now") {
        return set_to_current(p);
    }
    let (r, rc) = atof(z);
    if rc > 0 {
        set_raw_number(p, r);
        return false;
    }
    if z.eq_ignore_ascii_case(b"subsec") || z.eq_ignore_ascii_case(b"subsecond") {
        p.use_subsec = true;
        return set_to_current(p);
    }
    true
}

const MAX_JD: i64 = (0x1a640i64 << 32) | 0x1072fdff;

fn valid_julian_day(ijd: i64) -> bool {
    (0..=MAX_JD).contains(&ijd)
}

/// `computeYMD`.
fn compute_ymd(p: &mut DateTime) {
    if p.valid_ymd {
        return;
    }
    if !p.valid_jd {
        p.y = 2000;
        p.mo = 1;
        p.d = 1;
    } else if !valid_julian_day(p.ijd) {
        datetime_error(p);
        return;
    } else {
        let z = ((p.ijd + 43_200_000) / 86_400_000) as i32;
        let mut a = ((f64::from(z) - 1_867_216.25) / 36_524.25) as i32;
        a = z + 1 + a - (a / 4);
        let b = a + 1524;
        let c = ((f64::from(b) - 122.1) / 365.25) as i32;
        let d = (36525 * (c & 32767)) / 100;
        let e = (f64::from(b - d) / 30.6001) as i32;
        let x1 = (30.6001 * f64::from(e)) as i32;
        p.d = b - d - x1;
        p.mo = if e < 14 { e - 1 } else { e - 13 };
        p.y = if p.mo > 2 { c - 4716 } else { c - 4715 };
    }
    p.valid_ymd = true;
}

/// `computeHMS`.
fn compute_hms(p: &mut DateTime) {
    if p.valid_hms {
        return;
    }
    compute_jd(p);
    let day_ms = ((p.ijd + 43_200_000) % 86_400_000) as i32;
    p.s = f64::from(day_ms % 60000) / 1000.0;
    let day_min = day_ms / 60000;
    p.mi = day_min % 60;
    p.h = day_min / 60;
    p.raw_s = false;
    p.valid_hms = true;
}

fn compute_ymd_hms(p: &mut DateTime) {
    compute_ymd(p);
    compute_hms(p);
}

/// Dias desde 1970-01-01 pra uma data civil (proleptic gregorian).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `localtime_r` com o fuso do sandbox: (ano, mês, dia, hora, minuto, segundo).
fn os_localtime(t: i64) -> Option<(i64, u32, u32, u32, u32, u32)> {
    let ts = jiff::Timestamp::from_second(t).ok()?;
    let off = i64::from(sandbox_tz().to_offset(ts).seconds());
    let local = t + off;
    let days = local.div_euclid(86400);
    let secs = local.rem_euclid(86400);
    let (y, m, d) = civil_from_days(days);
    Some((y, m, d, (secs / 3600) as u32, ((secs % 3600) / 60) as u32, (secs % 60) as u32))
}

/// `toLocaltime`. `Err` com a mensagem de erro da função.
fn to_localtime(p: &mut DateTime) -> Result<(), String> {
    compute_jd(p);
    let (t, year_diff) = if p.ijd < 2_108_667_600 * 100_000 || p.ijd > 2_130_141_456 * 100_000 {
        let mut x = *p;
        compute_ymd_hms(&mut x);
        let diff = (2000 + x.y % 4) - x.y;
        x.y += diff;
        x.valid_jd = false;
        compute_jd(&mut x);
        (x.ijd / 1000 - 21_086_676 * 10000, diff)
    } else {
        (p.ijd / 1000 - 21_086_676 * 10000, 0)
    };
    let Some((y, m, d, h, mi, s)) = os_localtime(t) else {
        return Err("local time unavailable".into());
    };
    p.y = y as i32 - year_diff;
    p.mo = m as i32;
    p.d = d as i32;
    p.h = h as i32;
    p.mi = mi as i32;
    p.s = f64::from(s) + (p.ijd % 1000) as f64 * 0.001;
    p.valid_ymd = true;
    p.valid_hms = true;
    p.valid_jd = false;
    p.raw_s = false;
    p.tz = 0;
    p.is_error = false;
    Ok(())
}

const XFORM: [(&str, f32, f64); 6] = [
    ("second", 4.6427e14, 1.0),
    ("minute", 7.7379e12, 60.0),
    ("hour", 1.2897e11, 3600.0),
    ("day", 5_373_485.0, 86400.0),
    ("month", 176_546.0, 30.0 * 86400.0),
    ("year", 14713.0, 365.0 * 86400.0),
];

fn auto_adjust_date(p: &mut DateTime) {
    if !p.raw_s || p.valid_jd {
        p.raw_s = false;
    } else if p.s >= (-21_086_676i64 * 10000) as f64 && p.s <= (25_340_230i64 * 10000 + 799) as f64 {
        let r = p.s * 1000.0 + 210_866_760_000_000.0;
        clear_ymd_hms_tz(p);
        p.ijd = (r + 0.5) as i64;
        p.valid_jd = true;
        p.raw_s = false;
    }
}

/// `parseModifier`: `Ok(true)` em erro "normal" (resultado NULL), `Err` com mensagem.
fn parse_modifier(z: &[u8], p: &mut DateTime, idx: usize) -> Result<bool, String> {
    let z = &z[..z.iter().position(|&b| b == 0).unwrap_or(z.len())];
    let first = at(z, 0).to_ascii_lowercase();
    let mut rc = true;
    match first {
        b'a' => {
            if z.eq_ignore_ascii_case(b"auto") {
                if idx > 1 {
                    return Ok(true);
                }
                auto_adjust_date(p);
                rc = false;
            }
        }
        b'c' => {
            if z.eq_ignore_ascii_case(b"ceiling") {
                compute_jd(p);
                clear_ymd_hms_tz(p);
                rc = false;
                p.n_floor = 0;
            }
        }
        b'f' => {
            if z.eq_ignore_ascii_case(b"floor") {
                compute_jd(p);
                p.ijd -= i64::from(p.n_floor) * 86_400_000;
                clear_ymd_hms_tz(p);
                rc = false;
            }
        }
        b'j' => {
            if z.eq_ignore_ascii_case(b"julianday") {
                if idx > 1 {
                    return Ok(true);
                }
                if p.valid_jd && p.raw_s {
                    rc = false;
                    p.raw_s = false;
                }
            }
        }
        b'l' => {
            if z.eq_ignore_ascii_case(b"localtime") {
                if !p.is_local {
                    to_localtime(p)?;
                }
                rc = false;
                p.is_utc = false;
                p.is_local = true;
            }
        }
        b'u' => {
            if z.eq_ignore_ascii_case(b"unixepoch") && p.raw_s {
                if idx > 1 {
                    return Ok(true);
                }
                let r = p.s * 1000.0 + 210_866_760_000_000.0;
                if (0.0..464_269_060_800_000.0).contains(&r) {
                    clear_ymd_hms_tz(p);
                    p.ijd = (r + 0.5) as i64;
                    p.valid_jd = true;
                    p.raw_s = false;
                    rc = false;
                }
            } else if z.eq_ignore_ascii_case(b"utc") {
                if !p.is_utc {
                    compute_jd(p);
                    let orig = p.ijd;
                    let mut guess = orig;
                    let mut err = 0i64;
                    let mut cnt = 0;
                    loop {
                        let mut nw = DateTime::default();
                        guess -= err;
                        nw.ijd = guess;
                        nw.valid_jd = true;
                        to_localtime(&mut nw)?;
                        compute_jd(&mut nw);
                        err = nw.ijd - orig;
                        if err == 0 || cnt >= 3 {
                            break;
                        }
                        cnt += 1;
                    }
                    *p = DateTime::default();
                    p.ijd = guess;
                    p.valid_jd = true;
                    p.is_utc = true;
                    p.is_local = false;
                }
                rc = false;
            }
        }
        b'w' => {
            if z.len() >= 8 && z[..8].eq_ignore_ascii_case(b"weekday ") {
                let (r, ok) = atof(&z[8..]);
                if ok > 0 && (0.0..7.0).contains(&r) && (r as i32) as f64 == r {
                    let n = r as i64;
                    compute_ymd_hms(p);
                    p.tz = 0;
                    p.valid_jd = false;
                    compute_jd(p);
                    let mut zz = ((p.ijd + 129_600_000) / 86_400_000) % 7;
                    if zz > n {
                        zz -= 7;
                    }
                    p.ijd += (n - zz) * 86_400_000;
                    clear_ymd_hms_tz(p);
                    rc = false;
                }
            }
        }
        b's' => {
            if !(z.len() >= 9 && z[..9].eq_ignore_ascii_case(b"start of ")) {
                if z.eq_ignore_ascii_case(b"subsec") || z.eq_ignore_ascii_case(b"subsecond") {
                    p.use_subsec = true;
                    rc = false;
                }
                return Ok(rc);
            }
            if !p.valid_jd && !p.valid_ymd && !p.valid_hms {
                return Ok(true);
            }
            let rest = &z[9..];
            compute_ymd(p);
            p.valid_hms = true;
            p.h = 0;
            p.mi = 0;
            p.s = 0.0;
            p.raw_s = false;
            p.tz = 0;
            p.valid_jd = false;
            if rest.eq_ignore_ascii_case(b"month") {
                p.d = 1;
                rc = false;
            } else if rest.eq_ignore_ascii_case(b"year") {
                p.mo = 1;
                p.d = 1;
                rc = false;
            } else if rest.eq_ignore_ascii_case(b"day") {
                rc = false;
            }
        }
        b'+' | b'-' | b'0'..=b'9' => return numeric_modifier(z, p),
        _ => {}
    }
    Ok(rc)
}

/// Modificadores `+NNN unidade`, `+HH:MM:SS.FFF` e `±YYYY-MM-DD[ HH:MM]`.
fn numeric_modifier(z: &[u8], p: &mut DateTime) -> Result<bool, String> {
    let z0 = z[0];
    let mut n = 1;
    let mut y = [0; 1];
    while n < z.len() {
        let c = z[n];
        if c == b':' || isspace(c) {
            break;
        }
        if c == b'-' {
            if n == 5 && get_digits(&z[1..], &[(4, 0, 14712, 0)], &mut y) == 1 {
                break;
            }
            if n == 6 && get_digits(&z[1..], &[(5, 0, 14712, 0)], &mut y) == 1 {
                break;
            }
        }
        n += 1;
    }
    let (r, ok) = atof(&z[..n]);
    if ok <= 0 {
        return Ok(true);
    }
    let mut zz: &[u8] = z;
    let mut z2: &[u8] = z;
    let mut n2 = n;
    if at(z, n) == b'-' {
        if z0 != b'+' && z0 != b'-' {
            return Ok(true);
        }
        let mut v = [0; 3];
        if n == 5 {
            if get_digits(&z[1..], &[(4, 0, 14712, b'-'), (2, 0, 12, b'-'), (2, 0, 31, 0)], &mut v) != 3 {
                return Ok(true);
            }
        } else {
            if get_digits(&z[1..], &[(5, 0, 14712, b'-'), (2, 0, 12, b'-'), (2, 0, 31, 0)], &mut v) != 3 {
                return Ok(true);
            }
            zz = &z[1..];
        }
        let (yy, mm, mut dd) = (v[0], v[1], v[2]);
        if mm >= 12 || dd >= 31 {
            return Ok(true);
        }
        compute_ymd_hms(p);
        p.valid_jd = false;
        if z0 == b'-' {
            p.y -= yy;
            p.mo -= mm;
            dd = -dd;
        } else {
            p.y += yy;
            p.mo += mm;
        }
        let x = if p.mo > 0 { (p.mo - 1) / 12 } else { (p.mo - 12) / 12 };
        p.y += x;
        p.mo -= x * 12;
        compute_floor(p);
        compute_jd(p);
        p.valid_hms = false;
        p.valid_ymd = false;
        p.ijd += i64::from(dd) * 86_400_000;
        if at(zz, 11) == 0 {
            return Ok(false);
        }
        let mut hm = [0; 2];
        if isspace(at(zz, 11)) && get_digits(&zz[12.min(zz.len())..], &[(2, 0, 24, b':'), (2, 0, 59, 0)], &mut hm) == 2 {
            z2 = &zz[12..];
            n2 = 2;
        } else {
            return Ok(true);
        }
    }
    if at(z2, n2) == b':' {
        let mut tx = DateTime::default();
        let zs = if !at(z2, 0).is_ascii_digit() { &z2[1..] } else { z2 };
        if parse_hhmmss(zs, &mut tx) {
            return Ok(true);
        }
        compute_jd(&mut tx);
        tx.ijd -= 43_200_000;
        let day = tx.ijd / 86_400_000;
        tx.ijd -= day * 86_400_000;
        if z0 == b'-' {
            tx.ijd = -tx.ijd;
        }
        compute_jd(p);
        clear_ymd_hms_tz(p);
        p.ijd += tx.ijd;
        return Ok(false);
    }
    let mut rest = &z[n..];
    while rest.first().is_some_and(|&c| isspace(c)) {
        rest = &rest[1..];
    }
    let mut len = rest.len();
    if !(3..=10).contains(&len) {
        return Ok(true);
    }
    if rest[len - 1].eq_ignore_ascii_case(&b's') {
        len -= 1;
    }
    compute_jd(p);
    let rounder = if r < 0.0 { -0.5 } else { 0.5 };
    p.n_floor = 0;
    let mut rc = true;
    let mut r = r;
    for (i, (name, limit, xform)) in XFORM.iter().enumerate() {
        if name.len() == len
            && name.as_bytes().eq_ignore_ascii_case(&rest[..len])
            && r > -f64::from(*limit)
            && r < f64::from(*limit)
        {
            match i {
                4 => {
                    compute_ymd_hms(p);
                    p.mo += r as i32;
                    let x = if p.mo > 0 { (p.mo - 1) / 12 } else { (p.mo - 12) / 12 };
                    p.y += x;
                    p.mo -= x * 12;
                    compute_floor(p);
                    p.valid_jd = false;
                    r -= (r as i32) as f64;
                }
                5 => {
                    let yy = r as i32;
                    compute_ymd_hms(p);
                    p.y += yy;
                    compute_floor(p);
                    p.valid_jd = false;
                    r -= (r as i32) as f64;
                }
                _ => {}
            }
            compute_jd(p);
            p.ijd += (r * 1000.0 * xform + rounder) as i64;
            rc = false;
            break;
        }
    }
    clear_ymd_hms_tz(p);
    Ok(rc)
}

/// Texto de um argumento como `sqlite3_value_text`.
fn value_text(v: ValueRef<'_>) -> Option<Vec<u8>> {
    match v {
        ValueRef::Null => None,
        ValueRef::Integer(i) => Some(i.to_string().into_bytes()),
        ValueRef::Real(f) => Some(fmt_real(f, 15)),
        ValueRef::Text(t) | ValueRef::Blob(t) => Some(t.to_vec()),
    }
}

/// `isDate`: `Ok(None)` = resultado NULL.
fn is_date(args: &[ValueRef<'_>]) -> Result<Option<DateTime>, String> {
    let mut p = DateTime::default();
    if args.is_empty() {
        return Ok(if set_to_current(&mut p) { None } else { Some(p) });
    }
    match args[0] {
        ValueRef::Integer(i) => set_raw_number(&mut p, i as f64),
        ValueRef::Real(f) => set_raw_number(&mut p, f),
        other => {
            let Some(z) = value_text(other) else { return Ok(None) };
            let z = &z[..z.iter().position(|&b| b == 0).unwrap_or(z.len())];
            if parse_date_or_time(z, &mut p) {
                return Ok(None);
            }
        }
    }
    for (i, a) in args.iter().enumerate().skip(1) {
        let Some(z) = value_text(*a) else { return Ok(None) };
        if parse_modifier(&z, &mut p, i)? {
            return Ok(None);
        }
    }
    compute_jd(&mut p);
    if p.is_error || !valid_julian_day(p.ijd) {
        return Ok(None);
    }
    if args.len() == 1 && p.valid_ymd && p.d > 28 {
        p.valid_ymd = false;
    }
    Ok(Some(p))
}

fn args_of<'a>(ctx: &'a Context<'_>) -> Vec<ValueRef<'a>> {
    (0..ctx.len()).map(|i| ctx.get_raw(i)).collect()
}

fn err(msg: String) -> rusqlite::Error {
    rusqlite::Error::UserFunctionError(msg.into())
}

fn text_value(b: Vec<u8>) -> Value {
    match String::from_utf8(b) {
        Ok(s) => Value::Text(s),
        Err(e) => Value::Blob(e.into_bytes()),
    }
}

fn julianday(args: &[ValueRef<'_>]) -> Result<Value, String> {
    Ok(match is_date(args)? {
        Some(mut x) => {
            compute_jd(&mut x);
            Value::Real(x.ijd as f64 / 86_400_000.0)
        }
        None => Value::Null,
    })
}

fn unixepoch(args: &[ValueRef<'_>]) -> Result<Value, String> {
    Ok(match is_date(args)? {
        Some(mut x) => {
            compute_jd(&mut x);
            if x.use_subsec {
                Value::Real((x.ijd - 21_086_676 * 10_000_000) as f64 / 1000.0)
            } else {
                Value::Integer(x.ijd / 1000 - 21_086_676 * 10000)
            }
        }
        None => Value::Null,
    })
}

fn two(v: i32) -> [u8; 2] {
    [b'0' + ((v / 10) % 10) as u8, b'0' + (v % 10) as u8]
}

fn ymd_text(x: &DateTime) -> Vec<u8> {
    let mut y = x.y;
    if y < 0 {
        y = -y;
    }
    let mut out = Vec::new();
    if x.y < 0 {
        out.push(b'-');
    }
    out.extend_from_slice(&[
        b'0' + ((y / 1000) % 10) as u8,
        b'0' + ((y / 100) % 10) as u8,
        b'0' + ((y / 10) % 10) as u8,
        b'0' + (y % 10) as u8,
        b'-',
    ]);
    out.extend_from_slice(&two(x.mo));
    out.push(b'-');
    out.extend_from_slice(&two(x.d));
    out
}

fn hms_text(x: &DateTime) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&two(x.h));
    out.push(b':');
    out.extend_from_slice(&two(x.mi));
    out.push(b':');
    if x.use_subsec {
        let s = (1000.0 * x.s + 0.5) as i32;
        out.extend_from_slice(&[
            b'0' + ((s / 10000) % 10) as u8,
            b'0' + ((s / 1000) % 10) as u8,
            b'.',
            b'0' + ((s / 100) % 10) as u8,
            b'0' + ((s / 10) % 10) as u8,
            b'0' + (s % 10) as u8,
        ]);
    } else {
        out.extend_from_slice(&two(x.s as i32));
    }
    out
}

fn datetime(args: &[ValueRef<'_>]) -> Result<Value, String> {
    Ok(match is_date(args)? {
        Some(mut x) => {
            compute_ymd_hms(&mut x);
            let mut out = ymd_text(&x);
            out.push(b' ');
            out.extend(hms_text(&x));
            text_value(out)
        }
        None => Value::Null,
    })
}

fn time(args: &[ValueRef<'_>]) -> Result<Value, String> {
    Ok(match is_date(args)? {
        Some(mut x) => {
            compute_hms(&mut x);
            text_value(hms_text(&x))
        }
        None => Value::Null,
    })
}

fn date(args: &[ValueRef<'_>]) -> Result<Value, String> {
    Ok(match is_date(args)? {
        Some(mut x) => {
            compute_ymd(&mut x);
            text_value(ymd_text(&x))
        }
        None => Value::Null,
    })
}

fn days_after_jan01(p: &DateTime) -> i32 {
    let mut jan = *p;
    jan.valid_jd = false;
    jan.mo = 1;
    jan.d = 1;
    compute_jd(&mut jan);
    ((p.ijd - jan.ijd + 43_200_000) / 86_400_000) as i32
}

fn days_after_monday(p: &DateTime) -> i32 {
    (((p.ijd + 43_200_000) / 86_400_000) % 7) as i32
}

fn days_after_sunday(p: &DateTime) -> i32 {
    (((p.ijd + 129_600_000) / 86_400_000) % 7) as i32
}

/// `%2d`/`%02d`/`%03d`/`%04d` do printf do SQLite pra inteiros.
fn int_fmt(v: i64, width: usize, zero: bool) -> String {
    if zero {
        if v < 0 { format!("-{:0w$}", -v, w = width.saturating_sub(1)) } else { format!("{v:0width$}") }
    } else {
        format!("{v:>width$}")
    }
}

fn strftime(args: &[ValueRef<'_>]) -> Result<Value, String> {
    if args.is_empty() {
        return Ok(Value::Null);
    }
    let Some(fmt) = value_text(args[0]) else { return Ok(Value::Null) };
    let fmt = &fmt[..fmt.iter().position(|&b| b == 0).unwrap_or(fmt.len())];
    let Some(mut x) = is_date(&args[1..])? else { return Ok(Value::Null) };
    compute_jd(&mut x);
    compute_ymd_hms(&mut x);
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0;
    let mut j = 0;
    while i < fmt.len() {
        if fmt[i] != b'%' {
            i += 1;
            continue;
        }
        if j < i {
            out.extend_from_slice(&fmt[j..i]);
        }
        i += 1;
        j = i + 1;
        let cf = at(fmt, i);
        match cf {
            b'd' | b'e' => out.extend(int_fmt(i64::from(x.d), 2, cf == b'd').into_bytes()),
            b'f' => {
                let s = if x.s > 59.999 { 59.999 } else { x.s };
                out.extend(sqlite_printf("%06.3f", Value::Real(s)));
            }
            b'F' => out.extend(format!("{}-{}-{}", int_fmt(i64::from(x.y), 4, true), int_fmt(i64::from(x.mo), 2, true), int_fmt(i64::from(x.d), 2, true)).into_bytes()),
            b'G' | b'g' => {
                let mut y = x;
                y.ijd += i64::from(3 - days_after_monday(&x)) * 86_400_000;
                y.valid_ymd = false;
                compute_ymd(&mut y);
                if cf == b'g' {
                    out.extend(int_fmt(i64::from(y.y % 100), 2, true).into_bytes());
                } else {
                    out.extend(int_fmt(i64::from(y.y), 4, true).into_bytes());
                }
            }
            b'H' | b'k' => out.extend(int_fmt(i64::from(x.h), 2, cf == b'H').into_bytes()),
            b'I' | b'l' => {
                let mut h = x.h;
                if h > 12 {
                    h -= 12;
                }
                if h == 0 {
                    h = 12;
                }
                out.extend(int_fmt(i64::from(h), 2, cf == b'I').into_bytes());
            }
            b'j' => out.extend(int_fmt(i64::from(days_after_jan01(&x) + 1), 3, true).into_bytes()),
            b'J' => out.extend(sqlite_printf("%.16g", Value::Real(x.ijd as f64 / 86_400_000.0))),
            b'm' => out.extend(int_fmt(i64::from(x.mo), 2, true).into_bytes()),
            b'M' => out.extend(int_fmt(i64::from(x.mi), 2, true).into_bytes()),
            b'p' | b'P' => {
                let pm = x.h >= 12;
                out.extend_from_slice(match (cf == b'p', pm) {
                    (true, true) => b"PM",
                    (true, false) => b"AM",
                    (false, true) => b"pm",
                    (false, false) => b"am",
                });
            }
            b'R' => out.extend(format!("{}:{}", int_fmt(i64::from(x.h), 2, true), int_fmt(i64::from(x.mi), 2, true)).into_bytes()),
            b's' => {
                if x.use_subsec {
                    out.extend(sqlite_printf("%.3f", Value::Real((x.ijd - 21_086_676 * 10_000_000) as f64 / 1000.0)));
                } else {
                    out.extend((x.ijd / 1000 - 21_086_676 * 10000).to_string().into_bytes());
                }
            }
            b'S' => out.extend(int_fmt(i64::from(x.s as i32), 2, true).into_bytes()),
            b'T' => out.extend(
                format!(
                    "{}:{}:{}",
                    int_fmt(i64::from(x.h), 2, true),
                    int_fmt(i64::from(x.mi), 2, true),
                    int_fmt(i64::from(x.s as i32), 2, true)
                )
                .into_bytes(),
            ),
            b'u' | b'w' => {
                let mut c = days_after_sunday(&x) as u8 + b'0';
                if c == b'0' && cf == b'u' {
                    c = b'7';
                }
                out.push(c);
            }
            b'U' => out.extend(int_fmt(i64::from((days_after_jan01(&x) - days_after_sunday(&x) + 7) / 7), 2, true).into_bytes()),
            b'V' => {
                let mut y = x;
                y.ijd += i64::from(3 - days_after_monday(&x)) * 86_400_000;
                y.valid_ymd = false;
                compute_ymd(&mut y);
                out.extend(int_fmt(i64::from(days_after_jan01(&y) / 7 + 1), 2, true).into_bytes());
            }
            b'W' => out.extend(int_fmt(i64::from((days_after_jan01(&x) - days_after_monday(&x) + 7) / 7), 2, true).into_bytes()),
            b'Y' => out.extend(int_fmt(i64::from(x.y), 4, true).into_bytes()),
            b'%' => out.push(b'%'),
            _ => return Ok(Value::Null),
        }
        i += 1;
    }
    if j < i.min(fmt.len()) {
        out.extend_from_slice(&fmt[j..i.min(fmt.len())]);
    }
    Ok(text_value(out))
}

fn timediff(args: &[ValueRef<'_>]) -> Result<Value, String> {
    let Some(mut d1) = is_date(&args[0..1])? else { return Ok(Value::Null) };
    let Some(mut d2) = is_date(&args[1..2])? else { return Ok(Value::Null) };
    compute_ymd_hms(&mut d1);
    compute_ymd_hms(&mut d2);
    let sign;
    let mut y;
    let mut m;
    if d1.ijd >= d2.ijd {
        sign = '+';
        y = d1.y - d2.y;
        if y != 0 {
            d2.y = d1.y;
            d2.valid_jd = false;
            compute_jd(&mut d2);
        }
        m = d1.mo - d2.mo;
        if m < 0 {
            y -= 1;
            m += 12;
        }
        if m != 0 {
            d2.mo = d1.mo;
            d2.valid_jd = false;
            compute_jd(&mut d2);
        }
        while d1.ijd < d2.ijd {
            m -= 1;
            if m < 0 {
                m = 11;
                y -= 1;
            }
            d2.mo -= 1;
            if d2.mo < 1 {
                d2.mo = 12;
                d2.y -= 1;
            }
            d2.valid_jd = false;
            compute_jd(&mut d2);
        }
        d1.ijd -= d2.ijd;
        d1.ijd += 1_486_995_408i64 * 100_000;
    } else {
        sign = '-';
        y = d2.y - d1.y;
        if y != 0 {
            d2.y = d1.y;
            d2.valid_jd = false;
            compute_jd(&mut d2);
        }
        m = d2.mo - d1.mo;
        if m < 0 {
            y -= 1;
            m += 12;
        }
        if m != 0 {
            d2.mo = d1.mo;
            d2.valid_jd = false;
            compute_jd(&mut d2);
        }
        while d1.ijd > d2.ijd {
            m -= 1;
            if m < 0 {
                m = 11;
                y -= 1;
            }
            d2.mo += 1;
            if d2.mo > 12 {
                d2.mo = 1;
                d2.y += 1;
            }
            d2.valid_jd = false;
            compute_jd(&mut d2);
        }
        d1.ijd = d2.ijd - d1.ijd;
        d1.ijd += 1_486_995_408i64 * 100_000;
    }
    clear_ymd_hms_tz(&mut d1);
    compute_ymd_hms(&mut d1);
    let mut out = format!(
        "{sign}{}-{}-{} {}:{}:",
        int_fmt(i64::from(y), 4, true),
        int_fmt(i64::from(m), 2, true),
        int_fmt(i64::from(d1.d - 1), 2, true),
        int_fmt(i64::from(d1.h), 2, true),
        int_fmt(i64::from(d1.mi), 2, true)
    )
    .into_bytes();
    out.extend(sqlite_printf("%06.3f", Value::Real(d1.s)));
    Ok(text_value(out))
}

/// Registra as funções de data por cima das nativas.
pub fn register(conn: &Connection) {
    // As nativas são inócuas: podem aparecer em DEFAULT, CHECK, índice e view com trusted_schema
    // desligado (o padrão do CLI).
    let det = FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC | FunctionFlags::SQLITE_INNOCUOUS;
    let nondet = FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_INNOCUOUS;
    type F = fn(&[ValueRef<'_>]) -> Result<Value, String>;
    let variadic: [(&str, F); 6] = [
        ("julianday", julianday),
        ("unixepoch", unixepoch),
        ("date", date),
        ("time", time),
        ("datetime", datetime),
        ("strftime", strftime),
    ];
    for (name, f) in variadic {
        let _ = conn.create_scalar_function(name, -1, det, move |ctx| f(&args_of(ctx)).map_err(err));
    }
    let _ = conn.create_scalar_function("timediff", 2, det, |ctx| timediff(&args_of(ctx)).map_err(err));
    let current: [(&str, F); 3] = [("current_time", time), ("current_timestamp", datetime), ("current_date", date)];
    for (name, f) in current {
        let _ = conn.create_scalar_function(name, 0, nondet, move |_ctx| f(&[]).map_err(err));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(f: F, args: &[Value]) -> Value {
        let refs: Vec<ValueRef<'_>> = args.iter().map(ValueRef::from).collect();
        f(&refs).unwrap()
    }
    type F = fn(&[ValueRef<'_>]) -> Result<Value, String>;

    #[test]
    fn date_functions_without_now() {
        let t = |s: &str| Value::Text(s.to_string());
        assert_eq!(run(date, &[t("2026-01-31"), t("+1 month")]), t("2026-03-03"));
        assert_eq!(run(date, &[t("2026-01-15"), t("start of month"), t("+1 month"), t("-1 day")]), t("2026-01-31"));
        assert_eq!(run(datetime, &[Value::Integer(0), t("unixepoch")]), t("1970-01-01 00:00:00"));
        assert_eq!(run(unixepoch, &[t("2026-01-15 12:00:00")]), Value::Integer(1_768_478_400));
        assert_eq!(run(time, &[t("12:34:56"), t("+1 hour")]), t("13:34:56"));
        assert_eq!(run(julianday, &[t("2026-01-15")]), Value::Real(2_461_055.5));
        assert_eq!(run(date, &[t("2026-02-30")]), t("2026-03-02"));
        assert_eq!(run(date, &[t("2024-03-31"), t("-1 month"), t("floor")]), t("2024-02-29"));
        assert_eq!(run(date, &[t("bogus")]), Value::Null);
    }
}
