//! Hora local do sandbox, resolvida como o glibc: `TZ` do ambiente ou o fuso do sandbox; vazio ou `UTC`
//! é UTC; caminho absoluto lê o TZif do sandbox; nome lê `/usr/share/zoneinfo/<nome>` e, na falta, o
//! banco embutido do jiff; o resto é regra POSIX. O que não resolve vira UTC.

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

fn from_spec(spec: &[u8]) -> TimeZone {
    let spec = spec.strip_prefix(b":").unwrap_or(spec);
    let Ok(s) = std::str::from_utf8(spec) else { return TimeZone::UTC };
    if s.is_empty() || s == "UTC" {
        return TimeZone::UTC;
    }
    if s.starts_with('/') {
        return read_tzif(s, spec).unwrap_or(TimeZone::UTC);
    }
    if !s.split('/').any(|c| c == "..") {
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

/// O instante corrente formatado com `strftime` no fuso local.
pub fn now_local(fmt: &str) -> String {
    let (sec, nsec) = crate::net::io::wall();
    let ts = Timestamp::new(sec, nsec as i32).unwrap_or(Timestamp::UNIX_EPOCH);
    ts.to_zoned(local()).strftime(fmt).to_string()
}
