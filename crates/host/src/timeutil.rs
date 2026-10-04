//! Datas em UTC e durações legíveis, sem depender do fuso do host.
//!
//! Tudo é segundo desde a época Unix (`u64`). As conversões de calendário são as de Howard Hinnant
//! (`days_from_civil` e `civil_from_days`), exatas no calendário gregoriano proléptico.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Agora, em segundos desde a época.
pub fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Agora, em milissegundos desde a época.
pub fn now_unix_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let mp = u64::from(if m > 2 { m - 3 } else { m + 9 });
    let doy = (153 * mp + 2) / 5 + u64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe as i64 - 719_468
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `2026-10-02T23:39:00Z`.
pub fn fmt_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// Duração no formato `90s`, `45m`, `12h`, `30d` ou `2w` (um número e uma unidade).
pub fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    let split = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    if num.is_empty() {
        return Err(format!("duração inválida: {s:?} (use, por exemplo, 90s, 45m, 12h, 30d ou 2w)"));
    }
    let n: u64 = num.parse().map_err(|_| format!("duração inválida: {s:?}"))?;
    let mult = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86_400,
        "w" => 7 * 86_400,
        "" => return Err(format!("duração sem unidade: {s:?} (use s, m, h, d ou w)")),
        _ => return Err(format!("unidade de duração desconhecida em {s:?} (use s, m, h, d ou w)")),
    };
    n.checked_mul(mult).map(Duration::from_secs).ok_or_else(|| format!("duração grande demais: {s:?}"))
}

/// Instante absoluto em UTC: `2027-01-31` (meia-noite) ou `2027-01-31T12:00:00Z`.
pub fn parse_utc(s: &str) -> Result<u64, String> {
    let bad = || format!("data inválida: {s:?} (use AAAA-MM-DD ou AAAA-MM-DDTHH:MM:SSZ, em UTC)");
    let (date, time) = match s.split_once('T') {
        Some((d, t)) => (d, Some(t.strip_suffix('Z').ok_or_else(bad)?)),
        None => (s, None),
    };
    let mut parts = date.split('-');
    let y: i64 = parts.next().and_then(|p| p.parse().ok()).ok_or_else(bad)?;
    let m: u32 = parts.next().and_then(|p| p.parse().ok()).ok_or_else(bad)?;
    let d: u32 = parts.next().and_then(|p| p.parse().ok()).ok_or_else(bad)?;
    if parts.next().is_some() || !(1..=12).contains(&m) || d == 0 || d > days_in_month(y, m) || y < 1970 {
        return Err(bad());
    }
    let mut secs = days_from_civil(y, m, d) as u64 * 86_400;
    if let Some(t) = time {
        let hms: Vec<&str> = t.split(':').collect();
        if hms.len() != 3 {
            return Err(bad());
        }
        let h: u64 = hms[0].parse().map_err(|_| bad())?;
        let mi: u64 = hms[1].parse().map_err(|_| bad())?;
        let se: u64 = hms[2].parse().map_err(|_| bad())?;
        if h > 23 || mi > 59 || se > 59 {
            return Err(bad());
        }
        secs += h * 3600 + mi * 60 + se;
    }
    Ok(secs)
}

fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        _ => 28,
    }
}

/// Expiração de uma chave: `never`, uma duração a partir de `now` ou uma data absoluta.
pub fn parse_expiry(s: &str, now: u64) -> Result<Option<u64>, String> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("never") {
        return Ok(None);
    }
    if s.contains('-') {
        let at = parse_utc(s)?;
        if at <= now {
            return Err(format!("a data de expiração {s} já passou"));
        }
        return Ok(Some(at));
    }
    let d = parse_duration(s)?;
    if d.is_zero() {
        return Err("a expiração precisa ser maior que zero".into());
    }
    Ok(Some(now.saturating_add(d.as_secs())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_formatting_round_trips() {
        assert_eq!(fmt_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(fmt_utc(1_768_478_400), "2026-01-15T12:00:00Z");
        assert_eq!(fmt_utc(951_782_400), "2000-02-29T00:00:00Z");
        for secs in [0u64, 86_399, 951_782_400, 1_768_478_400, 4_102_444_800] {
            assert_eq!(parse_utc(&fmt_utc(secs)).unwrap(), secs);
        }
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("90s").unwrap().as_secs(), 90);
        assert_eq!(parse_duration("30d").unwrap().as_secs(), 30 * 86_400);
        assert_eq!(parse_duration("2w").unwrap().as_secs(), 14 * 86_400);
        assert!(parse_duration("30").is_err());
        assert!(parse_duration("d").is_err());
        assert!(parse_duration("3y").is_err());
    }

    #[test]
    fn expiry_forms() {
        let now = 1_768_478_400;
        assert_eq!(parse_expiry("never", now).unwrap(), None);
        assert_eq!(parse_expiry("1h", now).unwrap(), Some(now + 3600));
        assert_eq!(parse_expiry("2027-01-01", now).unwrap(), Some(parse_utc("2027-01-01").unwrap()));
        assert!(parse_expiry("2020-01-01", now).is_err());
        assert!(parse_expiry("0s", now).is_err());
        assert!(parse_utc("2026-02-29").is_err());
        assert!(parse_utc("2026-13-01").is_err());
    }
}
