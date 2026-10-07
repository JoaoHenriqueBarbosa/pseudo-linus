//! Conversão de tempo Unix em data civil (pro `%(fmt)T` do printf). Fuso: UTC, ou deslocamento
//! fixo de uma string POSIX de TZ sem horário de verão (`BRT3`, `<-03>3`, `UTC0`).

use ul_common::time::Civil;

use crate::printf::Tm;

/// Campos de `t + gmtoff`.
pub fn tm_with_offset(t: i64, gmtoff: i64, zone: &[u8]) -> Tm {
    let c = Civil::from_secs(t + gmtoff);
    Tm {
        year: c.year,
        mon: c.mon as u32,
        mday: c.mday as u32,
        hour: c.hour as u32,
        min: c.min as u32,
        sec: c.sec as u32,
        wday: c.wday as u32,
        yday: c.yday as u32,
        gmtoff,
        zone: zone.to_vec(),
        isdst: false,
    }
}

pub fn utc_tm(t: i64) -> Tm {
    tm_with_offset(t, 0, b"UTC")
}

/// Interpreta um TZ POSIX sem regras de horário de verão. `None` se não entender.
pub fn parse_posix_tz(tz: &[u8]) -> Option<(Vec<u8>, i64)> {
    if tz.is_empty() || tz == b"UTC" || tz == b"GMT" || tz == b"UTC0" || tz == b"GMT0" || tz == b":UTC" {
        return Some((b"UTC".to_vec(), 0));
    }
    let mut i = 0;
    let name: Vec<u8> = if tz[0] == b'<' {
        let end = tz.iter().position(|c| *c == b'>')?;
        i = end + 1;
        tz[1..end].to_vec()
    } else {
        while i < tz.len() && tz[i].is_ascii_alphabetic() {
            i += 1;
        }
        tz[..i].to_vec()
    };
    if name.len() < 3 || i >= tz.len() {
        return None;
    }
    let mut sign = 1i64;
    if tz[i] == b'+' || tz[i] == b'-' {
        if tz[i] == b'-' {
            sign = -1;
        }
        i += 1;
    }
    let rest = std::str::from_utf8(&tz[i..]).ok()?;
    let mut parts = rest.split(':');
    let h: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next().map(|x| x.parse().unwrap_or(0)).unwrap_or(0);
    let s: i64 = parts.next().map(|x| x.parse().unwrap_or(0)).unwrap_or(0);
    // No POSIX o deslocamento é "a oeste de Greenwich": `BRT3` é UTC-3.
    Some((name, -sign * (h * 3600 + m * 60 + s)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates() {
        let tm = utc_tm(1_768_478_400);
        assert_eq!((tm.year, tm.mon, tm.mday, tm.hour, tm.wday, tm.yday), (2026, 1, 15, 12, 4, 14));
        let tm = utc_tm(0);
        assert_eq!((tm.year, tm.mon, tm.mday, tm.wday), (1970, 1, 1, 4));
        assert_eq!(parse_posix_tz(b"BRT3").map(|x| x.1), Some(-3 * 3600));
    }
}
