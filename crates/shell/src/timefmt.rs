//! Conversão de tempo Unix em data civil (pro `%(fmt)T` do printf). Fuso: UTC, ou deslocamento
//! fixo de uma string POSIX de TZ sem horário de verão (`BRT3`, `<-03>3`, `UTC0`).

use crate::printf::Tm;

/// Dias desde 1970-01-01 -> (ano, mês 1-12, dia 1-31) (algoritmo civil de Howard Hinnant).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

/// Campos de `t + gmtoff`.
pub fn tm_with_offset(t: i64, gmtoff: i64, zone: &[u8]) -> Tm {
    let local = t + gmtoff;
    let days = local.div_euclid(86_400);
    let secs = local.rem_euclid(86_400);
    let (year, mon, mday) = civil_from_days(days);
    let wday = ((days % 7 + 11) % 7) as u32; // 1970-01-01 foi quinta (4).
    let cum = [0u32, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let mut yday = cum[(mon - 1) as usize] + mday - 1;
    if mon > 2 && is_leap(year) {
        yday += 1;
    }
    Tm {
        year,
        mon,
        mday,
        hour: (secs / 3600) as u32,
        min: ((secs % 3600) / 60) as u32,
        sec: (secs % 60) as u32,
        wday,
        yday,
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
