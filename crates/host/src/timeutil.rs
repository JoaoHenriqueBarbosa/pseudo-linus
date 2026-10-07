//! Datas em UTC e durações legíveis, sem depender do fuso do host.
//!
//! Tudo é segundo desde a época Unix (`u64`). As conversões de calendário vêm de
//! [`ul_common::time`], exatas no calendário gregoriano proléptico.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ul_common::time::{Civil, days_from_civil, days_in_month};

/// Agora, em segundos desde a época.
pub fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Agora, em milissegundos desde a época.
pub fn now_unix_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// `2026-10-02T23:39:00Z`.
pub fn fmt_utc(secs: u64) -> String {
    let c = Civil::from_days((secs / 86_400) as i64, (secs % 86_400) as i64);
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", c.year, c.mon, c.mday, c.hour, c.min, c.sec)
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
    if parts.next().is_some()
        || !(1..=12).contains(&m)
        || d == 0
        || i64::from(d) > days_in_month(y, i64::from(m))
        || y < 1970
    {
        return Err(bad());
    }
    let mut secs = days_from_civil(y, i64::from(m), i64::from(d)) as u64 * 86_400;
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
