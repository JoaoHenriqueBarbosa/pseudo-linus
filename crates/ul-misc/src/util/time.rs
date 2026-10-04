//! Hora local do sandbox: o fuso vem de `Syscalls::local_timezone` (conteúdo de `/etc/localtime`
//! ou o `TZ`), resolvido pelo banco de fusos embutido no binário. Nunca lê o fuso do host.

use jiff::Timestamp;
use jiff::civil::DateTime;
use jiff::tz::{TimeZone, TimeZoneDatabase};
use sysabi::{Clock, TimeSpec, sys};

/// Banco de fusos embutido (tzdata do `jiff-tzdb`).
fn bundled() -> &'static TimeZoneDatabase {
    static DB: std::sync::OnceLock<TimeZoneDatabase> = std::sync::OnceLock::new();
    DB.get_or_init(TimeZoneDatabase::bundled)
}

/// Resolve uma especificação de fuso como a glibc: TZif cru, nome do banco (`America/Sao_Paulo`,
/// com ou sem `:` na frente) ou regra POSIX (`EST5EDT`, `UTC0`). Nome desconhecido vira UTC, que é o
/// que a glibc faz.
pub fn parse_tz(spec: &[u8]) -> TimeZone {
    if spec.starts_with(b"TZif") {
        return TimeZone::tzif("localtime", spec).unwrap_or(TimeZone::UTC);
    }
    let text = String::from_utf8_lossy(spec);
    let text = text.trim();
    let name = text.strip_prefix(':').unwrap_or(text);
    if name.is_empty() || name == "UTC" || name == "UTC0" || name == "GMT" {
        return TimeZone::UTC;
    }
    if let Ok(tz) = bundled().get(name) {
        return tz;
    }
    TimeZone::posix(name).unwrap_or(TimeZone::UTC)
}

/// O fuso local do processo corrente.
pub fn local_tz() -> TimeZone {
    match sys::try_current() {
        Some(s) => parse_tz(&s.local_timezone()),
        None => TimeZone::UTC,
    }
}

/// Agora (relógio de parede do sandbox).
pub fn now() -> TimeSpec {
    sys::try_current().and_then(|s| s.clock_gettime(Clock::Realtime).ok()).unwrap_or_default()
}

/// Segundos desde a época em data e hora civis no fuso dado.
pub fn civil(sec: i64, tz: &TimeZone) -> DateTime {
    let ts = Timestamp::from_second(sec).unwrap_or(Timestamp::UNIX_EPOCH);
    tz.to_datetime(ts)
}

/// Abreviação do fuso naquele instante (`UTC`, `-03`, `EST`), como o `%Z` do strftime.
pub fn abbreviation(sec: i64, tz: &TimeZone) -> String {
    let ts = Timestamp::from_second(sec).unwrap_or(Timestamp::UNIX_EPOCH);
    tz.to_offset_info(ts).abbreviation().to_string()
}

pub const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
pub const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// Dia da semana com domingo = 0, como o `tm_wday`.
pub fn wday(dt: &DateTime) -> usize {
    dt.weekday().to_sunday_zero_offset() as usize
}

/// `ctime(3)` sem o `\n`: `Thu Jan 15 12:00:00 2026`.
pub fn ctime(sec: i64, tz: &TimeZone) -> String {
    let dt = civil(sec, tz);
    format!(
        "{} {} {:2} {:02}:{:02}:{:02} {}",
        WEEKDAYS[wday(&dt)],
        MONTHS[dt.month() as usize - 1],
        dt.day(),
        dt.hour(),
        dt.minute(),
        dt.second(),
        dt.year()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_names_posix_rules_and_utc() {
        let sp = parse_tz(b"America/Sao_Paulo");
        assert_eq!(civil(1_768_478_400, &sp).hour(), 9);
        assert_eq!(abbreviation(1_768_478_400, &sp), "-03");
        let est = parse_tz(b"EST5EDT");
        assert_eq!(civil(1_768_478_400, &est).hour(), 7);
        assert_eq!(ctime(1_768_478_400, &parse_tz(b"UTC")), "Thu Jan 15 12:00:00 2026");
        assert_eq!(civil(0, &parse_tz(b"Nowhere/Land")).hour(), 0);
    }
}
