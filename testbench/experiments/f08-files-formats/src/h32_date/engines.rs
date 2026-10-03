//! As bibliotecas candidatas atrás das traits do front-end: `parse_datetime` 0.16 (jiff), `interim`
//! (chrono), `parse_datetime` 0.11 (última versão sobre chrono), strftime do jiff e do chrono.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use chrono::{Offset, TimeZone as _};
use jiff::fmt::strtime::{BrokenDownTime, Config, PosixCustom};
use jiff::tz::{TimeZone, TimeZoneDatabase};

use super::cli::{Civil, DateFormatter, DateParser, Instant};

// ---------------------------------------------------------------------------------------------
// jiff: fusos pelo tzdb embutido (jiff-tzdb), nunca pelo banco global (que lê /usr/share/zoneinfo).

fn bundled_db() -> &'static TimeZoneDatabase {
    static DB: OnceLock<TimeZoneDatabase> = OnceLock::new();
    DB.get_or_init(TimeZoneDatabase::bundled)
}

/// Resolve o valor de TZ como a glibc: nome do tzdb (com ou sem ':'), depois string POSIX, depois POSIX
/// sem regras com as regras do `posixrules` do Debian (America/New_York), e por fim UTC com a abreviação
/// lida do começo do valor (é o que a glibc faz com nome desconhecido, ex.: "Foo/Bar" vira "Foo").
pub fn jiff_zone(tz: &str) -> TimeZone {
    static CACHE: OnceLock<Mutex<HashMap<String, TimeZone>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(z) = cache.lock().expect("cache").get(tz) {
        return z.clone();
    }
    let zone = resolve_jiff_zone(tz);
    cache.lock().expect("cache").insert(tz.to_string(), zone.clone());
    zone
}

fn resolve_jiff_zone(tz: &str) -> TimeZone {
    let name = tz.strip_prefix(':').unwrap_or(tz);
    // TZ vazio: a glibc procura o arquivo "Universal".
    let name = if name.is_empty() { "Universal" } else { name };
    if let Ok(z) = bundled_db().get(name) {
        return z;
    }
    if let Ok(z) = TimeZone::posix(name) {
        return z;
    }
    if let Ok(z) = TimeZone::posix(&format!("{name},M3.2.0,M11.1.0")) {
        return z;
    }
    let abbr: String = name.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    if abbr.len() >= 3
        && let Ok(z) = TimeZone::posix(&format!("{abbr}0"))
    {
        return z;
    }
    TimeZone::UTC
}

fn to_jiff(t: Instant, tz: &str) -> Result<jiff::Zoned, String> {
    let ts = jiff::Timestamp::from_nanosecond(t.as_nanos()).map_err(|e| e.to_string())?;
    Ok(ts.to_zoned(jiff_zone(tz)))
}

fn from_jiff(ts: jiff::Timestamp) -> Instant {
    Instant::from_nanos(ts.as_nanosecond())
}

/// `parse_datetime` 0.16: gramática do GNU portada pro jiff (uutils).
pub struct ParseDatetimeJiff;

impl DateParser for ParseDatetimeJiff {
    fn name(&self) -> String {
        "parse_datetime 0.16".into()
    }

    fn parse(&self, input: &str, now: Instant, tz: &str) -> Result<Instant, String> {
        let base = to_jiff(now, tz)?;
        match parse_datetime::parse_datetime_at_date(base, input) {
            Ok(parse_datetime::ParsedDateTime::InRange(z)) => Ok(from_jiff(z.timestamp())),
            // Anos a partir de 9999 voltam como `Extended` (o jiff vai até 9999-12-31); o instante sai
            // de `unix_seconds` e o formatador recusa o que passar do intervalo dele.
            Ok(parse_datetime::ParsedDateTime::Extended(e)) => Ok(Instant { secs: e.unix_seconds(), nanos: e.nanosecond }),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// strftime do jiff com o locale POSIX (`PosixCustom`) e modo leniente (diretiva desconhecida sai
/// literal, como no GNU).
pub struct JiffFormatter;

impl DateFormatter for JiffFormatter {
    fn name(&self) -> String {
        "jiff 0.2".into()
    }

    fn format(&self, t: Instant, tz: &str, fmt: &str) -> Result<String, String> {
        let z = to_jiff(t, tz)?;
        let config = Config::new().custom(PosixCustom::new()).lenient(true);
        BrokenDownTime::from(&z).to_string_with_config(&config, fmt).map_err(|e| e.to_string())
    }

    fn civil_to_instant(&self, c: Civil, tz: &str) -> Option<Instant> {
        let dt = jiff::civil::DateTime::new(c.year, c.month, c.day, c.hour, c.minute, c.second, 0).ok()?;
        let z = dt.to_zoned(jiff_zone(tz)).ok()?;
        Some(from_jiff(z.timestamp()))
    }
}

// ---------------------------------------------------------------------------------------------
// chrono: fusos pelo chrono-tz (tzdb embutido em tempo de compilação). Não há parser de TZ POSIX no
// chrono-tz, então valor que não é nome do tzdb vira UTC.

pub fn chrono_zone(tz: &str) -> chrono_tz::Tz {
    let name = tz.strip_prefix(':').unwrap_or(tz);
    let name = if name.is_empty() { "Universal" } else { name };
    name.parse::<chrono_tz::Tz>().unwrap_or(chrono_tz::Tz::UTC)
}

fn to_chrono(t: Instant, tz: &str) -> Result<chrono::DateTime<chrono_tz::Tz>, String> {
    chrono_zone(tz).timestamp_opt(t.secs, t.nanos).single().ok_or_else(|| format!("instante inválido: {t:?}"))
}

fn from_chrono<Tz: chrono::TimeZone>(dt: &chrono::DateTime<Tz>) -> Instant {
    Instant { secs: dt.timestamp(), nanos: dt.timestamp_subsec_nanos() }
}

/// strftime do chrono (`DateTime::format`).
pub struct ChronoFormatter;

impl DateFormatter for ChronoFormatter {
    fn name(&self) -> String {
        "chrono 0.4 + chrono-tz 0.10".into()
    }

    fn format(&self, t: Instant, tz: &str, fmt: &str) -> Result<String, String> {
        use std::fmt::Write;
        let dt = to_chrono(t, tz)?;
        let mut out = String::new();
        write!(out, "{}", dt.format(fmt)).map_err(|_| format!("formato não suportado pelo chrono: {fmt}"))?;
        Ok(out)
    }

    fn civil_to_instant(&self, c: Civil, tz: &str) -> Option<Instant> {
        let dt = chrono_zone(tz)
            .with_ymd_and_hms(c.year as i32, c.month as u32, c.day as u32, c.hour as u32, c.minute as u32, c.second as u32)
            .earliest()?;
        Some(from_chrono(&dt))
    }
}

/// `interim` 0.2 (fork do chrono-english) com backend chrono.
pub struct InterimChrono;

impl DateParser for InterimChrono {
    fn name(&self) -> String {
        "interim 0.2".into()
    }

    fn parse(&self, input: &str, now: Instant, tz: &str) -> Result<Instant, String> {
        let base = to_chrono(now, tz)?;
        interim::parse_date_string(input, base, interim::Dialect::Us)
            .map(|dt| from_chrono(&dt))
            .map_err(|e| format!("{e:?}"))
    }
}

/// `parse_datetime` 0.11, a última versão sobre chrono. A API exige `DateTime<Local>`; montamos o valor
/// com o offset do fuso do caso (`from_naive_utc_and_offset`), sem consultar o fuso do host. A
/// biblioteca trabalha com offset fixo: a hora local de outra data usa o offset do "agora".
pub struct ParseDatetimeChrono;

impl DateParser for ParseDatetimeChrono {
    fn name(&self) -> String {
        "parse_datetime 0.11".into()
    }

    fn parse(&self, input: &str, now: Instant, tz: &str) -> Result<Instant, String> {
        let base = to_chrono(now, tz)?;
        let offset = base.offset().fix();
        let local = chrono::DateTime::<chrono::Local>::from_naive_utc_and_offset(base.naive_utc(), offset);
        parse_datetime_chrono::parse_datetime_at_date(local, input)
            .map(|dt| from_chrono(&dt))
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jiff_zone_follows_glibc_fallbacks() {
        let t = Instant { secs: 1_782_907_200, nanos: 0 }; // 2026-07-01 12:00:00 UTC
        let f = JiffFormatter;
        // A glibc mostra "Foo"; o parser POSIX do jiff põe a abreviação em maiúsculas (medido), e o
        // jiff não tem construtor de fuso com abreviação arbitrária fora de TZif.
        assert_eq!(f.format(t, "Foo/Bar", "%Z %z").unwrap(), "FOO +0000");
        assert_eq!(f.format(t, "EST5EDT", "%Z %z").unwrap(), "EDT -0400");
        assert_eq!(f.format(t, "JST-9", "%Z %z").unwrap(), "JST +0900");
        assert_eq!(f.format(t, ":America/Sao_Paulo", "%Z %z").unwrap(), "-03 -0300");
    }

    #[test]
    fn civil_interpreted_in_case_zone() {
        let c = Civil { year: 2026, month: 1, day: 15, hour: 12, minute: 0, second: 0 };
        assert_eq!(JiffFormatter.civil_to_instant(c, "America/Sao_Paulo").unwrap().secs, 1_768_489_200);
        assert_eq!(ChronoFormatter.civil_to_instant(c, "America/Sao_Paulo").unwrap().secs, 1_768_489_200);
        assert_eq!(JiffFormatter.civil_to_instant(c, "UTC").unwrap().secs, 1_768_478_400);
    }

    #[test]
    fn parsers_resolve_relative_to_injected_now() {
        let now = Instant { secs: 1_768_478_400, nanos: 0 }; // quinta 2026-01-15 12:00 UTC
        let day = 86_400;
        assert_eq!(ParseDatetimeJiff.parse("2 days ago", now, "UTC").unwrap().secs, now.secs - 2 * day);
        assert_eq!(InterimChrono.parse("2 days ago", now, "UTC").unwrap().secs, now.secs - 2 * day);
        assert_eq!(ParseDatetimeChrono.parse("2 days ago", now, "UTC").unwrap().secs, now.secs - 2 * day);
    }
}
