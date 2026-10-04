// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

// spell-checker:ignore (ToDO) strtime

//! Set of functions related to time handling

use jiff::Zoned;
use jiff::fmt::StdIoWrite;
use jiff::fmt::strtime::{BrokenDownTime, Config};
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{UResult, USimpleError};
use crate::show_error;

/// Format the given date according to this time format style.
fn format_zoned<W: Write>(out: &mut W, zoned: Zoned, fmt: &str) -> UResult<()> {
    let tm = BrokenDownTime::from(&zoned);
    let mut out = StdIoWrite(out);
    let config = Config::new().lenient(true);
    tm.format_with_config(&config, fmt, &mut out)
        .map_err(|x| USimpleError::new(1, x.to_string()))
}

/// Convert a SystemTime` to a number of seconds since UNIX_EPOCH
pub fn system_time_to_sec(time: SystemTime) -> (i64, u32) {
    if time > UNIX_EPOCH {
        let d = time.duration_since(UNIX_EPOCH).unwrap();
        (d.as_secs() as i64, d.subsec_nanos())
    } else {
        let d = UNIX_EPOCH.duration_since(time).unwrap();
        (-(d.as_secs() as i64), d.subsec_nanos())
    }
}

pub mod format {
    pub static FULL_ISO: &str = "%Y-%m-%d %H:%M:%S.%N %z";
    pub static LONG_ISO: &str = "%Y-%m-%d %H:%M";
    pub static ISO: &str = "%Y-%m-%d";
}

/// Sets how `format_system_time` behaves if the time cannot be converted.
pub enum FormatSystemTimeFallback {
    Integer,      // Just print seconds since epoch (`ls`)
    IntegerError, // The above, and print an error (`du``)
    Float,        // Just print seconds+nanoseconds since epoch (`stat`)
}

/// Porte pseudo-linus: fuso do pseudo-processo como o `tzset` da glibc 2.41 o resolve. O original
/// usava `TimeZone::system()`, que lê o TZ do processo host e o /etc/localtime do host.
///
/// - Sem `TZ`: o tzfile `/etc/localtime` do FS do pseudo-processo; sem ele, UTC.
/// - `TZ` vazio vira `Universal`; o `:` inicial sai.
/// - O tzfile `$TZDIR/<nome>` (padrão `/usr/share/zoneinfo`), ou o caminho absoluto, no FS do
///   pseudo-processo (a imagem tem o zoneinfo do tzdata do Debian 13, sem os links legados).
/// - Senão, a regra POSIX (`EST5EDT`...).
/// - Senão, o que a glibc faz com uma regra que não termina de analisar: a sigla inicial (três ou
///   mais letras; menos que isso fica vazia) com deslocamento zero.
pub fn process_time_zone() -> jiff::tz::TimeZone {
    use jiff::tz::TimeZone;
    let Some(tz) = sysio::env::var_os("TZ") else {
        return sysio::fs::read("/etc/localtime")
            .ok()
            .and_then(|data| TimeZone::tzif("Local", &data).ok())
            .unwrap_or(TimeZone::UTC);
    };
    let tz = tz.to_string_lossy().into_owned();
    let name = tz.strip_prefix(':').unwrap_or(&tz);
    let name = if name.is_empty() { "Universal" } else { name };
    let path = if name.starts_with('/') {
        name.to_owned()
    } else {
        let dir = sysio::env::var("TZDIR").unwrap_or_else(|_| "/usr/share/zoneinfo".to_owned());
        format!("{dir}/{name}")
    };
    if let Ok(data) = sysio::fs::read(&path)
        && let Ok(zone) = TimeZone::tzif(name, &data)
    {
        return zone;
    }
    if let Ok(zone) = TimeZone::posix(name) {
        return zone;
    }
    // Regra com horário de verão mas sem datas de transição (`EST5EDT`): a glibc usa as do
    // `posixrules` (o rodapé POSIX do tzfile, no Debian o de America/New_York).
    if !name.contains(',') {
        let dir = sysio::env::var("TZDIR").unwrap_or_else(|_| "/usr/share/zoneinfo".to_owned());
        if let Some(rules) = sysio::fs::read(format!("{dir}/posixrules"))
            .ok()
            .and_then(|data| tzif_footer_rules(&data))
            && let Ok(zone) = TimeZone::posix(&format!("{name},{rules}"))
        {
            return zone;
        }
    }
    let abbr: String = name.chars().take_while(char::is_ascii_alphabetic).collect();
    let abbr = if abbr.len() >= 3 { abbr } else { String::new() };
    fixed_abbreviated_utc(&abbr)
}

/// As regras de transição (o que vem depois da primeira vírgula) do rodapé POSIX de um TZif v2+.
fn tzif_footer_rules(data: &[u8]) -> Option<String> {
    let text = data.strip_suffix(b"\n")?;
    let start = text.iter().rposition(|b| *b == b'\n')? + 1;
    let footer = std::str::from_utf8(&text[start..]).ok()?;
    footer.split_once(',').map(|(_, rules)| rules.to_owned())
}

/// Fuso de deslocamento zero com a sigla dada (inclusive vazia), por um TZif mínimo: o jiff não
/// tem como montar isso de outro jeito.
fn fixed_abbreviated_utc(abbr: &str) -> jiff::tz::TimeZone {
    let mut data = Vec::new();
    data.extend_from_slice(b"TZif2");
    data.extend_from_slice(&[0; 15]);
    // isutcnt, isstdcnt, leapcnt, timecnt, typecnt, charcnt (versão 1, vazia).
    for n in [0u32, 0, 0, 0, 1, 1] {
        data.extend_from_slice(&n.to_be_bytes());
    }
    data.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
    data.push(0);
    // Bloco da versão 2 com a sigla.
    data.extend_from_slice(b"TZif2");
    data.extend_from_slice(&[0; 15]);
    let chars = abbr.len() as u32 + 1;
    for n in [0u32, 0, 0, 0, 1, chars] {
        data.extend_from_slice(&n.to_be_bytes());
    }
    data.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
    data.extend_from_slice(abbr.as_bytes());
    data.push(0);
    // Rodapé POSIX vazio (sem regra pra datas depois da última transição).
    data.extend_from_slice(b"\n\n");
    jiff::tz::TimeZone::tzif(abbr, &data).unwrap_or(jiff::tz::TimeZone::UTC)
}

/// Porte pseudo-linus: a base IANA embutida no binário (a mesma pra todos os pseudo-processos, sem
/// ler o FS do host).
pub fn tz_database() -> &'static jiff::tz::TimeZoneDatabase {
    static DB: std::sync::OnceLock<jiff::tz::TimeZoneDatabase> = std::sync::OnceLock::new();
    DB.get_or_init(jiff::tz::TimeZoneDatabase::bundled)
}

/// Porte pseudo-linus: deslocamento UTC (em segundos) do fuso do pseudo-processo num instante, pra
/// quem formata data com outra crate (o `chrono::Local` leria o fuso do host).
pub fn utc_offset_seconds(time: SystemTime) -> i32 {
    jiff::Timestamp::try_from(time).map_or(0, |ts| process_time_zone().to_offset(ts).seconds())
}

pub fn format_system_time<W: Write>(
    out: &mut W,
    time: SystemTime,
    fmt: &str,
    mode: FormatSystemTimeFallback,
) -> UResult<()> {
    let zoned: Result<Zoned, jiff::Error> =
        jiff::Timestamp::try_from(time).map(|ts| ts.to_zoned(process_time_zone()));
    if let Ok(zoned) = zoned {
        format_zoned(out, zoned, fmt)
    } else {
        // Assume that if we cannot build a Zoned element, the timestamp is
        // out of reasonable range, just print it then.
        // TODO: The range allowed by jiff is different from what GNU accepts,
        // but it still far enough in the future/past to be unlikely to matter:
        //  jiff: Year between -9999 to 9999 (UTC) [-377705023201..=253402207200]
        //  GNU: Year fits in signed 32 bits (timezone dependent)
        let (mut secs, mut nsecs) = system_time_to_sec(time);
        match mode {
            FormatSystemTimeFallback::Integer => out.write_all(secs.to_string().as_bytes())?,
            FormatSystemTimeFallback::IntegerError => {
                let str = secs.to_string();
                show_error!("time '{str}' is out of range");
                out.write_all(str.as_bytes())?;
            }
            FormatSystemTimeFallback::Float => {
                if secs < 0 && nsecs != 0 {
                    secs -= 1;
                    nsecs = 1_000_000_000 - nsecs;
                }
                out.write_fmt(format_args!("{secs}.{nsecs:09}"))?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::time::{FormatSystemTimeFallback, format_system_time};
    use std::time::{Duration, UNIX_EPOCH};

    // Test epoch SystemTime get printed correctly at UTC0, with 2 simple formats.
    #[test]
    fn test_simple_system_time() {
        unsafe { std::env::set_var("TZ", "UTC0") };

        let time = UNIX_EPOCH;
        let mut out = Vec::new();
        format_system_time(
            &mut out,
            time,
            "%Y-%m-%d %H:%M",
            FormatSystemTimeFallback::Integer,
        )
        .expect("Formatting error.");
        assert_eq!(String::from_utf8(out).unwrap(), "1970-01-01 00:00");

        let mut out = Vec::new();
        format_system_time(
            &mut out,
            time,
            "%Y-%m-%d %H:%M:%S.%N %z",
            FormatSystemTimeFallback::Integer,
        )
        .expect("Formatting error.");
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "1970-01-01 00:00:00.000000000 +0000"
        );
    }

    // Test that very large (positive or negative) lead to just the timestamp being printed.
    #[test]
    fn test_large_system_time() {
        let time = UNIX_EPOCH + Duration::from_secs(67_768_036_191_763_200);
        let mut out = Vec::new();
        format_system_time(
            &mut out,
            time,
            "%Y-%m-%d %H:%M",
            FormatSystemTimeFallback::Integer,
        )
        .expect("Formatting error.");
        assert_eq!(String::from_utf8(out).unwrap(), "67768036191763200");

        let time = UNIX_EPOCH - Duration::from_secs(67_768_040_922_076_800);
        let mut out = Vec::new();
        format_system_time(
            &mut out,
            time,
            "%Y-%m-%d %H:%M",
            FormatSystemTimeFallback::Integer,
        )
        .expect("Formatting error.");
        assert_eq!(String::from_utf8(out).unwrap(), "-67768040922076800");
    }

    // Test that very large (positive or negative) lead to just the timestamp being printed.
    #[test]
    fn test_large_system_time_float() {
        let time =
            UNIX_EPOCH + Duration::from_secs(67_768_036_191_763_000) + Duration::from_nanos(123);
        let mut out = Vec::new();
        format_system_time(
            &mut out,
            time,
            "%Y-%m-%d %H:%M",
            FormatSystemTimeFallback::Float,
        )
        .expect("Formatting error.");
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "67768036191763000.000000123"
        );

        let time =
            UNIX_EPOCH - Duration::from_secs(67_768_040_922_076_000) + Duration::from_nanos(123);
        let mut out = Vec::new();
        format_system_time(
            &mut out,
            time,
            "%Y-%m-%d %H:%M",
            FormatSystemTimeFallback::Float,
        )
        .expect("Formatting error.");
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "-67768040922076000.000000123"
        );
    }
}
