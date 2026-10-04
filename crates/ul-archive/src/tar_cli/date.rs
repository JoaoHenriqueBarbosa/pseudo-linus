//! Datas de `--mtime`, `-N`/`--newer` e `--newer-mtime`: um subconjunto da gramática do `getdate` do
//! GNU que cobre o uso comum (`@segundos`, `AAAA-MM-DD`, `AAAA-MM-DD HH:MM[:SS[.fração]]`, `T` no lugar
//! do espaço, fuso `Z`/`UTC`/`GMT`/`±HH[:MM]`, `now`, `today`, `yesterday`, `tomorrow`), no fuso do
//! sandbox quando a data não traz fuso.

use jiff::civil::DateTime;
use jiff::tz::TimeZone;

use super::member::Time;

fn now() -> Time {
    match sysabi::sys::try_current().and_then(|s| s.clock_gettime(sysabi::Clock::Realtime).ok()) {
        Some(t) => Time::new(t.sec, t.nsec),
        None => Time::new(0, 0),
    }
}

fn parse_epoch(s: &str) -> Option<Time> {
    let (neg, body) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let (int, frac) = body.split_once(['.', ',']).unwrap_or((body, ""));
    if int.is_empty() || !int.bytes().all(|b| b.is_ascii_digit()) || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let sec: i64 = int.parse().ok()?;
    let mut nsec: u64 = 0;
    for (i, c) in frac.bytes().enumerate() {
        if i >= 9 {
            break;
        }
        nsec += (c - b'0') as u64 * 10u64.pow(8 - i as u32);
    }
    if neg {
        if nsec > 0 { Some(Time::new(-sec - 1, (1_000_000_000 - nsec) as u32)) } else { Some(Time::new(-sec, 0)) }
    } else {
        Some(Time::new(sec, nsec as u32))
    }
}

/// Deslocamento de um fuso escrito: "Z", "UTC", "+0300", "-03:00", "+3".
fn parse_zone(z: &str) -> Option<i64> {
    match z {
        "Z" | "z" | "UTC" | "GMT" | "UT" | "utc" | "gmt" => return Some(0),
        _ => {}
    }
    let (sign, rest) = match z.as_bytes().first()? {
        b'+' => (1i64, &z[1..]),
        b'-' => (-1i64, &z[1..]),
        _ => return None,
    };
    let digits: String = rest.chars().filter(|c| *c != ':').collect();
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (h, m) = match digits.len() {
        1 | 2 => (digits.parse::<i64>().ok()?, 0),
        3 | 4 => {
            let n = digits.len();
            (digits[..n - 2].parse::<i64>().ok()?, digits[n - 2..].parse::<i64>().ok()?)
        }
        _ => return None,
    };
    Some(sign * (h * 3600 + m * 60))
}

/// Interpreta a data; `None` quando o formato não é reconhecido.
pub fn parse(s: &[u8], tz: &TimeZone) -> Option<Time> {
    let s = std::str::from_utf8(s).ok()?.trim();
    if let Some(e) = s.strip_prefix('@') {
        return parse_epoch(e.trim());
    }
    match s {
        "now" => return Some(now()),
        "today" | "yesterday" | "tomorrow" => {
            let n = now();
            let day = match s {
                "yesterday" => -86_400,
                "tomorrow" => 86_400,
                _ => 0,
            };
            return Some(Time::new(n.sec + day, n.nsec));
        }
        _ => {}
    }
    // AAAA-MM-DD[ T]HH:MM[:SS[.fff]][ fuso]
    let (date, rest) = match s.find(['T', ' ']) {
        Some(p) => (&s[..p], s[p + 1..].trim()),
        None => (s, ""),
    };
    let mut dp = date.split('-');
    let y: i16 = dp.next()?.parse().ok()?;
    let mo: i8 = dp.next()?.parse().ok()?;
    let d: i8 = dp.next()?.parse().ok()?;
    if dp.next().is_some() {
        return None;
    }
    let (mut h, mut mi, mut sec, mut nsec) = (0i8, 0i8, 0i8, 0i32);
    let mut zone: Option<i64> = None;
    if !rest.is_empty() {
        // Separa o fuso (Z, UTC, +HHMM) do relógio.
        let (clock, z) = match rest.find(['+', 'Z', 'U', 'G']).or_else(|| rest.rfind('-').filter(|&p| p > 0)) {
            Some(p) => (rest[..p].trim(), Some(rest[p..].trim())),
            None => match rest.split_once(' ') {
                Some((c, z)) => (c.trim(), Some(z.trim())),
                None => (rest, None),
            },
        };
        if let Some(z) = z {
            zone = Some(parse_zone(z)?);
        }
        let mut cp = clock.split(':');
        h = cp.next()?.parse().ok()?;
        mi = cp.next()?.parse().ok()?;
        if let Some(sp) = cp.next() {
            let (si, sf) = sp.split_once(['.', ',']).unwrap_or((sp, ""));
            sec = si.parse().ok()?;
            for (i, c) in sf.bytes().enumerate() {
                if i >= 9 {
                    break;
                }
                if !c.is_ascii_digit() {
                    return None;
                }
                nsec += (c - b'0') as i32 * 10i32.pow(8 - i as u32);
            }
        }
        if cp.next().is_some() {
            return None;
        }
    }
    let dt = DateTime::new(y, mo, d, h, mi, sec, 0).ok()?;
    let secs = match zone {
        Some(off) => dt.to_zoned(TimeZone::UTC).ok()?.timestamp().as_second() - off,
        None => dt.to_zoned(tz.clone()).ok()?.timestamp().as_second(),
    };
    Some(Time::new(secs, nsec as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_formats() {
        let utc = TimeZone::UTC;
        assert_eq!(parse(b"@0", &utc), Some(Time::new(0, 0)));
        assert_eq!(parse(b"2026-01-15 12:00:00", &utc), Some(Time::new(1_768_478_400, 0)));
        assert_eq!(parse(b"2026-01-15T12:00:00Z", &utc), Some(Time::new(1_768_478_400, 0)));
        assert_eq!(parse(b"2026-01-15 12:00 +0100", &utc), Some(Time::new(1_768_478_400 - 3600, 0)));
        assert_eq!(parse(b"2026-01-15", &utc), Some(Time::new(1_768_435_200, 0)));
        assert_eq!(parse(b"bad", &utc), None);
    }
}
