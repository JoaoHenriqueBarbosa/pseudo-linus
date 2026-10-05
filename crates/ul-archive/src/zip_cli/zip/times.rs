//! Horários DOS e Unix (fileio.c): `dostime`, `unix2dostime`, `dos2unixtime`, no fuso local do sandbox.

use jiff::tz::TimeZone;

use super::consts::DOSTIME_MINIMUM;
use crate::tz;

/// `dostime`: ano/mês/dia hora:min:seg num valor de 4 bytes (data nos 2 altos, hora nos 2 baixos).
pub fn dostime(y: i64, n: i64, d: i64, h: i64, m: i64, s: i64) -> u64 {
    if y < 1980 {
        DOSTIME_MINIMUM
    } else {
        (((y - 1980) as u64) << 25) | ((n as u64) << 21) | ((d as u64) << 16) | ((h as u64) << 11) | ((m as u64) << 5) | ((s as u64) >> 1)
    }
}

/// `unix2dostime`: o horário Unix em formato DOS local, arredondado para o próximo segundo par.
pub fn unix2dostime(t: i64, tz: &TimeZone) -> u64 {
    let t_even = t.wrapping_add(1) & !1;
    let (y, mo, d, h, mi, s) = tz::civil(t_even, tz);
    dostime(y, mo as i64, d as i64, h as i64, mi as i64, s as i64)
}

/// `dos2unixtime`: o horário DOS (local) como horário Unix.
pub fn dos2unixtime(dos: u64, tz: &TimeZone) -> i64 {
    let sec = ((dos as i64) << 1) & 0x3e;
    let min = (dos as i64 >> 5) & 0x3f;
    let hour = (dos as i64 >> 11) & 0x1f;
    let mday = (dos >> 16) as i64 & 0x1f;
    let mon0 = ((dos >> 21) as i64 & 0x0f) - 1;
    let year = ((dos >> 25) as i64 & 0x7f) + 1980;
    tz::mktime(year, mon0, mday, hour, min, sec, tz)
}
