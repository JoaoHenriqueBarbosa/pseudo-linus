//! Fuso horário local do sandbox e formatação de datas, sem tocar o host.
//!
//! Resolve como o glibc: `TZ` do ambiente (ou, sem `TZ`, o fuso do sandbox via `local_timezone`);
//! vazio ou `UTC` é UTC; `:` inicial é ignorado; caminho absoluto lê o TZif do sistema de arquivos do
//! sandbox; nome lê `/usr/share/zoneinfo/<nome>` do sandbox e, na falta, o banco embutido do jiff (a
//! mesma tzdata 2026c do oráculo); o resto é tentado como regra POSIX (`EST5EDT,M3.2.0,M11.1.0`). Um
//! valor que não resolve vira UTC, como no glibc.

use jiff::Timestamp;
use jiff::tz::{TimeZone, TimeZoneDatabase};
use sysabi::sys;

/// Fuso local do processo corrente.
pub fn local() -> TimeZone {
    let spec = match sys::try_current() {
        Some(s) => s.getenv(b"TZ").unwrap_or_else(|| s.local_timezone()),
        None => Vec::new(),
    };
    from_spec(&spec)
}

/// Fuso a partir de um valor no formato da variável `TZ`.
pub fn from_spec(spec: &[u8]) -> TimeZone {
    let spec = spec.strip_prefix(b":").unwrap_or(spec);
    let Ok(s) = std::str::from_utf8(spec) else { return TimeZone::UTC };
    if s.is_empty() || s == "UTC" {
        return TimeZone::UTC;
    }
    if s.starts_with('/') {
        return read_tzif(s, spec).unwrap_or(TimeZone::UTC);
    }
    let safe_name = !s.split('/').any(|c| c == "..");
    if safe_name {
        let path = format!("/usr/share/zoneinfo/{s}");
        if let Some(tz) = read_tzif(s, path.as_bytes()) {
            return tz;
        }
        if let Ok(tz) = TimeZoneDatabase::bundled().get(s) {
            return tz;
        }
    }
    TimeZone::posix(s).unwrap_or(TimeZone::UTC)
}

fn read_tzif(name: &str, path: &[u8]) -> Option<TimeZone> {
    sys::try_current()?;
    let data = sys::read_file(path).ok()?;
    TimeZone::tzif(name, &data).ok()
}

/// Formata um instante (segundos e nanossegundos desde a época) com `strftime` do jiff no fuso dado.
/// Fora da faixa do jiff, cai pra época.
pub fn format(sec: i64, nsec: u32, tz: &TimeZone, fmt: &str) -> String {
    let ts = Timestamp::new(sec, nsec as i32).unwrap_or(Timestamp::UNIX_EPOCH);
    ts.to_zoned(tz.clone()).strftime(fmt).to_string()
}

/// Campos civis (ano, mês, dia, hora, minuto, segundo) de um instante no fuso dado.
pub fn civil(sec: i64, tz: &TimeZone) -> (i64, u32, u32, u32, u32, u32) {
    let ts = Timestamp::from_second(sec).unwrap_or(Timestamp::UNIX_EPOCH);
    let z = ts.to_zoned(tz.clone());
    (z.year() as i64, z.month() as u32, z.day() as u32, z.hour() as u32, z.minute() as u32, z.second() as u32)
}

/// Deslocamento UTC em segundos de um instante no fuso dado.
pub fn offset_seconds(sec: i64, tz: &TimeZone) -> i32 {
    let ts = Timestamp::from_second(sec).unwrap_or(Timestamp::UNIX_EPOCH);
    tz.to_offset(ts).seconds()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_and_named_zones() {
        let utc = from_spec(b"UTC");
        assert_eq!(format(1_768_478_400, 0, &utc, "%Y-%m-%d %H:%M:%S.%N %z"), "2026-01-15 12:00:00.000000000 +0000");
        let sp = from_spec(b"America/Sao_Paulo");
        assert_eq!(format(1_768_478_400, 5, &sp, "%Y-%m-%d %H:%M:%S.%N %z"), "2026-01-15 09:00:00.000000005 -0300");
        let posix = from_spec(b"EST5EDT,M3.2.0,M11.1.0");
        assert_eq!(offset_seconds(1_768_478_400, &posix), -5 * 3600);
        assert_eq!(civil(0, &utc), (1970, 1, 1, 0, 0, 0));
    }
}
